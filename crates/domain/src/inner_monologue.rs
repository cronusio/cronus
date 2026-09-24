//! Inner monologue — the heartbeat-gated background reflection cycle.
//!
//! The cycle fires only when the office is running (Active/Idle) and the foreground
//! is idle, and never when the Pulse subsystem is individually paused.
//! It parses reflection output into typed intentions, logs every intention —
//! including NoAction — to the Pulse log *before* any dispatch, and stays
//! within a declared token budget. Intentions route through standard
//! subsystem APIs; this module never writes a store directly.
//!
//! The reflection prompt (a nodus step) and the inbox-backed Pulse log are seams;
//! the gating, typing, and log-before-dispatch algebra are implemented here.

/// A typed intention produced by a reflection cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intention {
    ProactiveMessage(String),
    AutomationTrigger(String),
    MemoryWriteProposal(String),
    TaskProposal(String),
    NoAction,
}

impl Intention {
    /// Whether this intention results in a dispatch (NoAction does not).
    pub fn is_actionable(&self) -> bool {
        !matches!(self, Intention::NoAction)
    }
}

/// One Pulse-log entry, one per cycle.
#[derive(Debug, Clone)]
pub struct PulseEntry {
    pub cycle_id: String,
    pub intentions: Vec<Intention>,
    /// True if the token budget cut the cycle short.
    pub truncated: bool,
}

/// The Pulse log (inbox-backed in production). Append-only here.
#[derive(Debug, Default)]
pub struct PulseLog {
    entries: Vec<PulseEntry>,
}

impl PulseLog {
    pub fn new() -> Self {
        PulseLog::default()
    }

    pub fn entries(&self) -> &[PulseEntry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The gating inputs for whether a cycle may fire.
#[derive(Debug, Clone, Copy)]
pub struct CycleGate {
    /// Office is Active or Idle.
    pub office_running: bool,
    /// The foreground session has no blocking work (turn state Waiting/Idle).
    pub foreground_idle: bool,
    /// The Heartbeat/Pulse subsystem is individually paused.
    pub pulse_paused: bool,
}

impl CycleGate {
    /// Whether the cycle may fire.
    pub fn should_fire(&self) -> bool {
        self.office_running && self.foreground_idle && !self.pulse_paused
    }
}

/// Errors surfaced by dispatch attempts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MonologueError {
    /// A dispatch was attempted for an intention that was never logged.
    UnloggedDispatch,
}

/// The inner-monologue driver. Owns the Pulse log and enforces log-before-dispatch.
#[derive(Debug, Default)]
pub struct InnerMonologue {
    log: PulseLog,
    /// Cycle ids whose intentions have been logged and may be dispatched.
    logged: std::collections::HashSet<String>,
}

impl InnerMonologue {
    pub fn new() -> Self {
        InnerMonologue::default()
    }

    pub fn log(&self) -> &PulseLog {
        &self.log
    }

    /// Run one cycle if the gate permits. Reflection output arrives as `intentions`
    /// (produced by the nodus reflection step within the token budget). Returns the
    /// logged entry, or `None` if the gate suppressed the cycle.
    ///
    /// ALL intentions are logged before any dispatch. Budget exhaustion
    /// marks the entry truncated.
    pub fn run_cycle(
        &mut self,
        cycle_id: &str,
        gate: CycleGate,
        mut intentions: Vec<Intention>,
        budget_exhausted: bool,
    ) -> Option<PulseEntry> {
        if !gate.should_fire() {
            return None;
        }
        // A cycle with no reflection is a valid NoAction cycle.
        if intentions.is_empty() {
            intentions.push(Intention::NoAction);
        }
        let entry = PulseEntry {
            cycle_id: cycle_id.to_string(),
            intentions,
            truncated: budget_exhausted,
        };
        self.log.entries.push(entry.clone());
        self.logged.insert(cycle_id.to_string());
        Some(entry)
    }

    /// Dispatch the actionable intentions of a logged cycle through subsystem APIs
    /// (the monologue proposes; the caller routes). Fails if the cycle was
    /// never logged. Returns the intentions to route (NoAction excluded).
    pub fn dispatch(&self, cycle_id: &str) -> Result<Vec<Intention>, MonologueError> {
        if !self.logged.contains(cycle_id) {
            return Err(MonologueError::UnloggedDispatch);
        }
        let entry = self
            .log
            .entries
            .iter()
            .find(|e| e.cycle_id == cycle_id)
            .ok_or(MonologueError::UnloggedDispatch)?;
        Ok(entry
            .intentions
            .iter()
            .filter(|i| i.is_actionable())
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_gate() -> CycleGate {
        CycleGate {
            office_running: true,
            foreground_idle: true,
            pulse_paused: false,
        }
    }

    #[test]
    fn cycle_suppressed_unless_running_idle_and_unpaused() {
        assert!(open_gate().should_fire());
        assert!(
            !CycleGate {
                office_running: false,
                ..open_gate()
            }
            .should_fire()
        );
        assert!(
            !CycleGate {
                foreground_idle: false,
                ..open_gate()
            }
            .should_fire()
        );
        assert!(
            !CycleGate {
                pulse_paused: true,
                ..open_gate()
            }
            .should_fire()
        );
    }

    #[test]
    fn suppressed_cycle_logs_nothing() {
        let mut im = InnerMonologue::new();
        let out = im.run_cycle(
            "c1",
            CycleGate {
                pulse_paused: true,
                ..open_gate()
            },
            vec![Intention::ProactiveMessage("hi".into())],
            false,
        );
        assert!(out.is_none());
        assert!(im.log().is_empty());
    }

    #[test]
    fn all_intentions_logged_before_dispatch() {
        // NoAction is logged too; dispatch returns only actionable ones.
        let mut im = InnerMonologue::new();
        im.run_cycle(
            "c1",
            open_gate(),
            vec![
                Intention::TaskProposal("index the repo".into()),
                Intention::NoAction,
            ],
            false,
        );
        assert_eq!(im.log().len(), 1);
        assert_eq!(im.log().entries()[0].intentions.len(), 2);
        let dispatched = im.dispatch("c1").unwrap();
        assert_eq!(
            dispatched,
            vec![Intention::TaskProposal("index the repo".into())]
        );
    }

    #[test]
    fn dispatch_of_unlogged_cycle_is_refused() {
        // An action with no prior log entry is a protocol violation.
        let im = InnerMonologue::new();
        assert_eq!(im.dispatch("ghost"), Err(MonologueError::UnloggedDispatch));
    }

    #[test]
    fn empty_reflection_is_a_noaction_cycle() {
        let mut im = InnerMonologue::new();
        let entry = im.run_cycle("c1", open_gate(), vec![], false).unwrap();
        assert_eq!(entry.intentions, vec![Intention::NoAction]);
        assert!(im.dispatch("c1").unwrap().is_empty());
    }

    #[test]
    fn budget_exhaustion_marks_entry_truncated() {
        let mut im = InnerMonologue::new();
        let entry = im
            .run_cycle("c1", open_gate(), vec![Intention::NoAction], true)
            .unwrap();
        assert!(entry.truncated);
    }
}
