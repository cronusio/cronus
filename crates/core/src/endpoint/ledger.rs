//! The work ledger: the names of the work running in this process.
//!
//! Whether *any* work is running anywhere is the operating system's to say
//! (`worklock`); this ledger is what lets a refusal say **which**. Every piece
//! of long-running work enters it before it starts and leaves when it ends,
//! through a guard whose drop ends it however the work ends — a normal return,
//! an error, or a panic that unwinds.
//!
//! The kinds of work are a closed set. A new producer adds its kind here, so
//! the gate's idea of "work" is read in one place and never a free-form label.

use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

/// What sort of work an item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkKind {
    /// A model turn in a conversation or an agent's run.
    AgentTurn,
    /// A workflow or automation run.
    WorkflowRun,
    /// A scheduled job.
    ScheduledJob,
    /// A long-running build of an index or other derived store.
    IndexBuild,
}

impl WorkKind {
    /// The kind as a word, the same on the wire and in a refusal.
    pub fn as_str(self) -> &'static str {
        match self {
            WorkKind::AgentTurn => "agent-turn",
            WorkKind::WorkflowRun => "workflow-run",
            WorkKind::ScheduledJob => "scheduled-job",
            WorkKind::IndexBuild => "index-build",
        }
    }

    /// The kind a word names.
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "agent-turn" => Some(WorkKind::AgentTurn),
            "workflow-run" => Some(WorkKind::WorkflowRun),
            "scheduled-job" => Some(WorkKind::ScheduledJob),
            "index-build" => Some(WorkKind::IndexBuild),
            _ => None,
        }
    }

    /// The kind in words a person reads.
    pub fn describe(self) -> &'static str {
        match self {
            WorkKind::AgentTurn => "an agent turn",
            WorkKind::WorkflowRun => "a workflow run",
            WorkKind::ScheduledJob => "a scheduled job",
            WorkKind::IndexBuild => "an index build",
        }
    }
}

/// One running piece of work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkItem {
    pub kind: WorkKind,
    /// What it is, for a person: the session a turn belongs to, the workflow's
    /// name. Never a secret and never an argument value.
    pub label: String,
    /// When it began, in seconds since the Unix epoch.
    pub since: u64,
}

#[derive(Default)]
struct State {
    next: u64,
    items: Vec<(u64, WorkItem)>,
}

/// The ledger. Cheap to clone; every clone is the same ledger.
#[derive(Clone, Default)]
pub struct Ledger {
    state: Arc<Mutex<State>>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Ledger {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // Every mutation below is a single push or remove, so a panic while the
        // ledger was locked leaves no half-state worth refusing over.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Enter a piece of work; the guard ends it when dropped.
    pub fn enter(&self, kind: WorkKind, label: impl Into<String>) -> Entry {
        let mut state = self.lock();
        let id = state.next;
        state.next += 1;
        state.items.push((
            id,
            WorkItem {
                kind,
                label: label.into(),
                since: now(),
            },
        ));
        Entry {
            ledger: self.clone(),
            id,
        }
    }

    /// Everything running here now, oldest first.
    pub fn snapshot(&self) -> Vec<WorkItem> {
        self.lock()
            .items
            .iter()
            .map(|(_, item)| item.clone())
            .collect()
    }
}

/// A piece of work in the ledger. Dropping it takes the work out.
pub struct Entry {
    ledger: Ledger,
    id: u64,
}

impl Drop for Entry {
    fn drop(&mut self) {
        self.ledger.lock().items.retain(|(id, _)| *id != self.id);
    }
}

/// The ledger of this process.
pub fn global() -> &'static Ledger {
    static LEDGER: OnceLock<Ledger> = OnceLock::new();
    LEDGER.get_or_init(Ledger::new)
}

/// What a refused change says is running, in a person's words.
pub fn describe_running(items: &[WorkItem]) -> String {
    let mut lines: Vec<String> = items
        .iter()
        .map(|item| format!("{} ({})", item.kind.describe(), item.label))
        .collect();
    lines.sort();
    lines.dedup();
    format!(
        "work is running that this change would alter: {}",
        lines.join("; ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_is_listed_while_it_runs_and_gone_when_its_entry_drops() {
        let ledger = Ledger::new();
        let entry = ledger.enter(WorkKind::AgentTurn, "s-1");

        let running = ledger.snapshot();
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].kind, WorkKind::AgentTurn);
        assert_eq!(running[0].label, "s-1");

        drop(entry);
        assert!(ledger.snapshot().is_empty());
    }

    #[test]
    fn the_oldest_work_is_listed_first() {
        let ledger = Ledger::new();
        let _a = ledger.enter(WorkKind::AgentTurn, "first");
        let _b = ledger.enter(WorkKind::WorkflowRun, "second");
        let labels: Vec<_> = ledger.snapshot().into_iter().map(|i| i.label).collect();
        assert_eq!(labels, vec!["first", "second"]);
    }

    #[test]
    fn an_entry_ends_its_work_even_when_the_thread_panics() {
        let ledger = Ledger::new();
        let inner = ledger.clone();
        let result = std::thread::spawn(move || {
            let _entry = inner.enter(WorkKind::WorkflowRun, "boom");
            panic!("the work failed");
        })
        .join();
        assert!(result.is_err());
        assert!(
            ledger.snapshot().is_empty(),
            "a panicked run is not still running"
        );
    }

    #[test]
    fn kinds_round_trip_through_their_words() {
        for kind in [
            WorkKind::AgentTurn,
            WorkKind::WorkflowRun,
            WorkKind::ScheduledJob,
            WorkKind::IndexBuild,
        ] {
            assert_eq!(WorkKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(WorkKind::parse("other"), None);
    }

    #[test]
    fn a_refusal_reads_as_what_is_running_without_repeating_itself() {
        let item = |since| WorkItem {
            kind: WorkKind::AgentTurn,
            label: "s-1".to_string(),
            since,
        };
        assert_eq!(
            describe_running(&[item(0), item(1)]),
            "work is running that this change would alter: an agent turn (s-1)"
        );
    }
}
