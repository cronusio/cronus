//! Agent session loop — TurnContext, IterationBudget, InterruptFence,
//! text-loop detection, durable prompt admission, and session runner registry.

pub mod entry;
pub mod hooks;
pub mod interrupt;
pub mod migration;
pub mod turn;

pub use entry::SessionEntry;
pub use hooks::{HookOutcome, StopHook};
pub use interrupt::InterruptFence;
pub use turn::{IterationBudget, TurnContext};

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::context_router::SessionContext;

// ── SessionId ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(String);

impl SessionId {
    pub fn new(id: impl Into<String>) -> Self {
        SessionId(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

// ── RunnerState ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerStatus {
    Idle,
    Busy,
}

pub struct RunnerState {
    pub status: RunnerStatus,
    pub interrupt: InterruptFence,
}

// ── RunnerMap ─────────────────────────────────────────────────────────────────

/// Registry of active session runners.
///
/// Each session has at most one runner at a time. `assert_not_busy` is the
/// admission gate that prevents two callers from driving the same session
/// concurrently.
pub struct RunnerMap {
    inner: Mutex<HashMap<SessionId, RunnerState>>,
}

impl RunnerMap {
    pub fn new() -> Self {
        RunnerMap {
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// Lock the registry, recovering from a poisoned lock.
    ///
    /// Poisoning only records that another holder panicked. The map holds
    /// independent per-session entries that are each inserted, replaced or
    /// removed whole, so no half-applied update can be observed; recovering
    /// keeps every other session usable instead of turning one panicking
    /// runner into a failure of the whole process.
    fn lock(&self) -> MutexGuard<'_, HashMap<SessionId, RunnerState>> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Register a new session and return its fence.
    ///
    /// Initialises the session in `Idle` state.
    pub fn register(&self, id: SessionId) -> InterruptFence {
        let fence = InterruptFence::new();
        let mut map = self.lock();
        map.insert(
            id,
            RunnerState {
                status: RunnerStatus::Idle,
                interrupt: fence.clone(),
            },
        );
        fence
    }

    /// Atomically assert that the session is not busy and mark it Busy.
    ///
    /// Returns `Err` if the session is already busy or not registered.
    pub fn assert_not_busy(&self, id: &SessionId) -> Result<(), SessionError> {
        let mut map = self.lock();
        let state = map.get_mut(id).ok_or(SessionError::NotRegistered)?;
        if state.status == RunnerStatus::Busy {
            return Err(SessionError::AlreadyBusy);
        }
        state.status = RunnerStatus::Busy;
        Ok(())
    }

    /// Mark a session as Idle again after a turn completes.
    pub fn mark_idle(&self, id: &SessionId) {
        let mut map = self.lock();
        if let Some(state) = map.get_mut(id) {
            state.status = RunnerStatus::Idle;
        }
    }

    /// Remove a session from the registry (retire).
    pub fn retire(&self, id: &SessionId) {
        let mut map = self.lock();
        map.remove(id);
    }

    pub fn is_registered(&self, id: &SessionId) -> bool {
        self.lock().contains_key(id)
    }
}

impl Default for RunnerMap {
    fn default() -> Self {
        Self::new()
    }
}

// ── SessionError ──────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq, Eq)]
pub enum SessionError {
    AlreadyBusy,
    NotRegistered,
    InterruptRequested,
    TextLoopDetected,
    GoalCapExceeded,
    Oversized { original_len: usize },
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionError::AlreadyBusy => write!(f, "session is already running a turn"),
            SessionError::NotRegistered => write!(f, "session not registered"),
            SessionError::InterruptRequested => write!(f, "interrupted by fence"),
            SessionError::TextLoopDetected => write!(f, "text loop detected"),
            SessionError::GoalCapExceeded => write!(f, "goal re-entry cap exceeded"),
            SessionError::Oversized { original_len } => {
                write!(f, "output oversized ({original_len} chars), truncated")
            }
        }
    }
}

impl std::error::Error for SessionError {}

// ── Loop runner (seam) ────────────────────────────────────────────────────────

/// Seam: follow-up messages injected after each turn (wiring deferred).
pub fn get_follow_up_messages(_ctx: &SessionContext) -> Vec<String> {
    vec![]
}

/// Seam: steering messages injected on text-loop detection (wiring deferred).
pub fn get_steering_messages(_ctx: &SessionContext) -> Vec<String> {
    vec![]
}

// ── Output size guard ─────────────────────────────────────────────────────────

/// Maximum assistant output characters before truncation.
pub const MAX_OUTPUT_CHARS: usize = 15_000;

/// Truncate oversized output and annotate it.
///
/// The budget counts characters, not bytes: text in a script whose letters
/// take several bytes each is not penalised for its encoding, and the cut
/// always falls between two characters — slicing at a raw byte offset would
/// panic inside a multi-byte one.
pub fn guard_output_size(output: &str) -> (String, Option<SessionError>) {
    // Every character is at least one byte, so a string that fits the budget
    // in bytes also fits it in characters — the common case pays no scan.
    if output.len() <= MAX_OUTPUT_CHARS {
        return (output.to_owned(), None);
    }
    // Byte offset of the first character past the budget; `None` means the
    // text is longer than the budget in bytes but still within it in characters.
    let Some((cut, _)) = output.char_indices().nth(MAX_OUTPUT_CHARS) else {
        return (output.to_owned(), None);
    };
    let total_chars = output.chars().count();
    let truncated = format!(
        "{}\n[output truncated: {} chars]",
        &output[..cut],
        total_chars
    );
    let err = SessionError::Oversized {
        original_len: total_chars,
    };
    (truncated, Some(err))
}

// ── Goal cap ──────────────────────────────────────────────────────────────────

pub const MAX_GOAL_REACT: u32 = 12;

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::Arc;

    use super::*;

    #[test]
    fn output_within_the_budget_passes_through_untouched() {
        let (out, err) = guard_output_size("short");
        assert_eq!(out, "short");
        assert!(err.is_none());
    }

    #[test]
    fn oversized_ascii_output_is_cut_at_the_character_budget() {
        let big = "x".repeat(MAX_OUTPUT_CHARS + 100);
        let (out, err) = guard_output_size(&big);
        assert!(out.starts_with(&"x".repeat(MAX_OUTPUT_CHARS)));
        assert!(!out.starts_with(&"x".repeat(MAX_OUTPUT_CHARS + 1)));
        assert!(out.ends_with(&format!(
            "[output truncated: {} chars]",
            MAX_OUTPUT_CHARS + 100
        )));
        assert_eq!(
            err,
            Some(SessionError::Oversized {
                original_len: MAX_OUTPUT_CHARS + 100
            })
        );
    }

    /// A one-byte character followed by two-byte ones puts the raw byte offset
    /// `MAX_OUTPUT_CHARS` inside a character — slicing there used to panic.
    #[test]
    fn oversized_multibyte_output_is_cut_between_characters() {
        let big = format!("a{}", "é".repeat(MAX_OUTPUT_CHARS + 500));
        assert!(!big.is_char_boundary(MAX_OUTPUT_CHARS));
        let (out, err) = guard_output_size(&big);
        let kept = out
            .split("\n[output truncated")
            .next()
            .expect("annotation follows the kept prefix");
        assert_eq!(kept.chars().count(), MAX_OUTPUT_CHARS);
        assert!(out.ends_with(&format!(
            "[output truncated: {} chars]",
            MAX_OUTPUT_CHARS + 501
        )));
        assert_eq!(
            err,
            Some(SessionError::Oversized {
                original_len: MAX_OUTPUT_CHARS + 501
            })
        );
    }

    /// Longer than the budget in bytes, within it in characters: not oversized.
    #[test]
    fn multibyte_output_within_the_character_budget_is_not_truncated() {
        let text = "ж".repeat(MAX_OUTPUT_CHARS - 10);
        assert!(text.len() > MAX_OUTPUT_CHARS);
        let (out, err) = guard_output_size(&text);
        assert_eq!(out, text);
        assert!(err.is_none());
    }

    #[test]
    fn output_exactly_at_the_budget_is_not_truncated() {
        let text = "ж".repeat(MAX_OUTPUT_CHARS);
        let (out, err) = guard_output_size(&text);
        assert_eq!(out, text);
        assert!(err.is_none());
    }

    /// A runner that panics while holding the registry lock poisons it; the
    /// registry must stay usable for every other session.
    #[test]
    fn runner_map_survives_a_poisoned_lock() {
        let map = Arc::new(RunnerMap::new());
        let id = SessionId::new("survivor");
        map.register(id.clone());

        let poisoner = Arc::clone(&map);
        let panicked = catch_unwind(AssertUnwindSafe(|| {
            let _held = poisoner.inner.lock().expect("lock not yet poisoned");
            panic!("simulated runner panic while holding the registry lock");
        }));
        assert!(panicked.is_err());
        assert!(map.inner.is_poisoned(), "the test must actually poison");

        assert!(map.is_registered(&id));
        assert_eq!(map.assert_not_busy(&id), Ok(()));
        assert_eq!(map.assert_not_busy(&id), Err(SessionError::AlreadyBusy));
        map.mark_idle(&id);
        let other = SessionId::new("newcomer");
        map.register(other.clone());
        assert!(map.is_registered(&other));
        map.retire(&id);
        assert!(!map.is_registered(&id));
    }
}
