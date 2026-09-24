//! Office control — the OfficeState machine driving start/pause/resume/hibernate.
//!
//! Foundation: the state machine, guarded transitions, the master switch, the
//! state-change event emitted before a transition commits, the token-exhaustion
//! hibernation ladder (substitute-before-hibernate, auto resource-recovery wake),
//! and per-subsystem pause toggles. The cooperative worker drain is modeled as a
//! recorded checkpoint here; real wiring to the orchestration drain bus, the
//! session-checkpoint store, and the model-router substitution is deferred — the
//! substitution *decision* is resolved by the caller (delegated entirely to
//! the model-router) and handed in.

use std::collections::HashSet;

/// The lifecycle state of an office.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfficeState {
    /// Engine running; workers accept tasks; crons fire.
    Active,
    /// Running; no active tasks; crons fire as scheduled.
    Idle,
    /// User-initiated pause; no task intake; crons suppressed; checkpoint written.
    Paused,
    /// Resource-triggered pause; auto-resume on recovery; checkpoint written.
    Hibernating,
    /// Halted on an unrecoverable fault; requires user acknowledgement.
    Error,
    /// Office not loaded in memory.
    Offline,
}

impl OfficeState {
    /// Whether the office accepts new task intake in this state.
    pub fn accepts_intake(self) -> bool {
        matches!(self, OfficeState::Active | OfficeState::Idle)
    }

    /// Whether this state is a frozen (drained + checkpointed) state.
    pub fn is_frozen(self) -> bool {
        matches!(self, OfficeState::Paused | OfficeState::Hibernating)
    }
}

/// A state-change event. Emitted before a transition is considered complete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateChange {
    pub office_id: String,
    pub from: OfficeState,
    pub to: OfficeState,
    /// Emission instant (caller-supplied clock; ms since epoch in production).
    pub at: u64,
}

/// A transition that the state machine refused, with no side effects performed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionRejected {
    pub from: OfficeState,
    pub requested: OfficeState,
}

impl std::fmt::Display for TransitionRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "transition rejected: {:?} -> {:?} is not a valid edge",
            self.from, self.requested
        )
    }
}

impl std::error::Error for TransitionRejected {}

/// Whether the office has queued work, deciding Active vs Idle on resume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Workload {
    Queued,
    Empty,
}

/// A subsystem that can be individually paused, independent of the master switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Subsystem {
    /// Scheduled/cron jobs.
    Scheduler,
    /// Auto-starting kanban tasks.
    KanbanAutorun,
    /// Event-driven automation pipelines.
    Automation,
    /// Heartbeat / pulse (inner monologue).
    Heartbeat,
}

/// The outcome of handling a quota-exhaustion signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HibernationOutcome {
    /// A viable substitute model was available; the office stayed running.
    Substituted,
    /// No substitute within budget; the office drained and hibernated.
    Hibernated,
}

/// The office control service. Sole writer of `OfficeState`; frontends read the
/// state and request transitions, never set it directly.
///
/// Emitted `StateChange` events accumulate in an in-process sink standing in for
/// the event mesh; a host drains them via [`OfficeControl::take_events`]. Every
/// committed transition pushes its event to the sink *before* mutating the state,
/// so an observer never sees a state the machine did not announce.
#[derive(Debug)]
pub struct OfficeControl {
    office_id: String,
    state: OfficeState,
    /// Whether a drain checkpoint exists to restore from. A frozen
    /// state always has one; resume clears it after restore.
    checkpoint: bool,
    /// Subsystems the user has individually paused. Survive a master
    /// resume; the master switch only resumes subsystems not in this set.
    paused_subsystems: HashSet<Subsystem>,
    events: Vec<StateChange>,
}

impl OfficeControl {
    /// Construct control for an office, starting `Offline`.
    pub fn new(office_id: &str) -> Self {
        OfficeControl {
            office_id: office_id.to_string(),
            state: OfficeState::Offline,
            checkpoint: false,
            paused_subsystems: HashSet::new(),
            events: Vec::new(),
        }
    }

    pub fn state(&self) -> OfficeState {
        self.state
    }

    /// Whether a restorable drain checkpoint currently exists.
    pub fn has_checkpoint(&self) -> bool {
        self.checkpoint
    }

    /// Drain the emitted state-change events (host consumes these off the mesh).
    pub fn take_events(&mut self) -> Vec<StateChange> {
        std::mem::take(&mut self.events)
    }

    /// Is `to` a valid edge from the current state?
    fn is_valid_edge(&self, to: OfficeState) -> bool {
        use OfficeState::*;
        match (self.state, to) {
            // load / unload
            (Offline, Active | Idle) => true,
            (_, Offline) => true,
            // master pause / resume
            (Active | Idle, Paused) => true,
            (Paused, Active | Idle) => true,
            // hibernation (quota wiring lands with the hibernation ladder task)
            (Active | Idle, Hibernating) => true,
            (Hibernating, Active | Idle) => true,
            // fault
            (_, Error) => true,
            (Error, Active | Idle) => true,
            _ => false,
        }
    }

    /// The single state mutator. Runs the guard, performs drain/restore side
    /// effects, emits the `StateChange` **before** committing, then commits.
    ///
    /// A rejected transition returns `Err` and performs no side effects.
    pub fn transition(&mut self, to: OfficeState, at: u64) -> Result<(), TransitionRejected> {
        if self.state == to {
            return Ok(());
        }
        if !self.is_valid_edge(to) {
            return Err(TransitionRejected {
                from: self.state,
                requested: to,
            });
        }

        // Side effects at the boundary: freezing drains to a checkpoint;
        // leaving a frozen state restores and clears it.
        if to.is_frozen() && !self.state.is_frozen() {
            self.checkpoint = true;
        } else if self.state.is_frozen() && !to.is_frozen() {
            self.checkpoint = false;
        }

        // Emit before commit — no silent transition.
        self.events.push(StateChange {
            office_id: self.office_id.clone(),
            from: self.state,
            to,
            at,
        });
        self.state = to;
        Ok(())
    }

    /// Master switch — pause. `Active`/`Idle` drain to a checkpoint and freeze to
    /// `Paused`. Inert on `Error`/`Offline` (different recovery paths).
    pub fn pause(&mut self, at: u64) -> Result<(), TransitionRejected> {
        match self.state {
            OfficeState::Active | OfficeState::Idle => self.transition(OfficeState::Paused, at),
            // No-op on states the master switch does not govern.
            OfficeState::Paused => Ok(()),
            other => Err(TransitionRejected {
                from: other,
                requested: OfficeState::Paused,
            }),
        }
    }

    /// Master switch — resume. `Paused` restores to `Active` (queued work) or
    /// `Idle` (empty). Inert if already running.
    pub fn resume(&mut self, workload: Workload, at: u64) -> Result<(), TransitionRejected> {
        match self.state {
            OfficeState::Paused => {
                let to = match workload {
                    Workload::Queued => OfficeState::Active,
                    Workload::Empty => OfficeState::Idle,
                };
                self.transition(to, at)
            }
            OfficeState::Active | OfficeState::Idle => Ok(()),
            other => Err(TransitionRejected {
                from: other,
                requested: OfficeState::Active,
            }),
        }
    }

    /// Handle a model quota-exhaustion signal. The substitution decision is
    /// resolved by the caller via the model-router (`substitute_available`); this
    /// service holds no substitution logic. A viable substitute keeps the office
    /// running; otherwise it drains and hibernates.
    ///
    /// Inert unless the office is running (`Active`/`Idle`).
    pub fn on_quota_exhausted(
        &mut self,
        substitute_available: bool,
        at: u64,
    ) -> Option<HibernationOutcome> {
        if !self.state.accepts_intake() {
            return None;
        }
        if substitute_available {
            // The swap itself is the model-router's job; the office stays running.
            Some(HibernationOutcome::Substituted)
        } else {
            // No substitute within budget: drain and hibernate.
            self.transition(OfficeState::Hibernating, at)
                .expect("Active/Idle -> Hibernating is a valid edge");
            Some(HibernationOutcome::Hibernated)
        }
    }

    /// Handle a resource-recovery signal for a hibernation-causing resource.
    /// A `Hibernating` office auto-resumes with no user action. Inert otherwise.
    pub fn on_quota_recovered(&mut self, workload: Workload, at: u64) -> bool {
        if self.state != OfficeState::Hibernating {
            return false;
        }
        let to = match workload {
            Workload::Queued => OfficeState::Active,
            Workload::Empty => OfficeState::Idle,
        };
        self.transition(to, at)
            .expect("Hibernating -> Active/Idle is a valid edge");
        true
    }

    /// Individually pause a subsystem. Persists across a master resume.
    pub fn pause_subsystem(&mut self, subsystem: Subsystem) {
        self.paused_subsystems.insert(subsystem);
    }

    /// Individually resume a subsystem.
    pub fn resume_subsystem(&mut self, subsystem: Subsystem) {
        self.paused_subsystems.remove(&subsystem);
    }

    /// Whether a subsystem is individually paused.
    pub fn is_subsystem_paused(&self, subsystem: Subsystem) -> bool {
        self.paused_subsystems.contains(&subsystem)
    }

    /// Whether a subsystem is currently active: the office is running AND the
    /// subsystem is not individually paused. A master resume therefore restores
    /// only subsystems not individually paused.
    pub fn is_subsystem_active(&self, subsystem: Subsystem) -> bool {
        self.state.accepts_intake() && !self.is_subsystem_paused(subsystem)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn active_office() -> OfficeControl {
        let mut oc = OfficeControl::new("office-1");
        oc.transition(OfficeState::Active, 1).unwrap();
        oc.take_events();
        oc
    }

    #[test]
    fn starts_offline() {
        let oc = OfficeControl::new("office-1");
        assert_eq!(oc.state(), OfficeState::Offline);
        assert!(!oc.has_checkpoint());
    }

    #[test]
    fn pause_drains_to_checkpoint_and_freezes() {
        // Freezing writes a checkpoint.
        let mut oc = active_office();
        oc.pause(10).unwrap();
        assert_eq!(oc.state(), OfficeState::Paused);
        assert!(oc.has_checkpoint());
        assert!(!oc.state().accepts_intake());
    }

    #[test]
    fn resume_restores_exact_state_by_workload() {
        // Resume restores; queued -> Active, empty -> Idle; checkpoint cleared.
        let mut queued = active_office();
        queued.pause(10).unwrap();
        queued.resume(Workload::Queued, 20).unwrap();
        assert_eq!(queued.state(), OfficeState::Active);
        assert!(!queued.has_checkpoint());

        let mut empty = active_office();
        empty.pause(10).unwrap();
        empty.resume(Workload::Empty, 20).unwrap();
        assert_eq!(empty.state(), OfficeState::Idle);
    }

    #[test]
    fn every_transition_emits_before_commit() {
        // No silent transition — each committed transition emits its event.
        let mut oc = OfficeControl::new("office-1");
        oc.transition(OfficeState::Active, 1).unwrap();
        oc.pause(2).unwrap();
        oc.resume(Workload::Empty, 3).unwrap();
        let events = oc.take_events();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].from, OfficeState::Offline);
        assert_eq!(events[0].to, OfficeState::Active);
        assert_eq!(events[1].to, OfficeState::Paused);
        assert_eq!(events[2].to, OfficeState::Idle);
        assert!(events.iter().all(|e| e.office_id == "office-1"));
    }

    #[test]
    fn invalid_transition_is_rejected_without_side_effects() {
        // A rejected transition performs no state or checkpoint change, no event.
        let mut oc = OfficeControl::new("office-1"); // Offline
        let err = oc.transition(OfficeState::Paused, 5).unwrap_err();
        assert_eq!(err.from, OfficeState::Offline);
        assert_eq!(err.requested, OfficeState::Paused);
        assert_eq!(oc.state(), OfficeState::Offline);
        assert!(!oc.has_checkpoint());
        assert!(oc.take_events().is_empty());
    }

    #[test]
    fn master_switch_inert_on_error_and_offline() {
        let mut offline = OfficeControl::new("office-1");
        assert!(offline.pause(1).is_err());

        let mut errored = active_office();
        errored.transition(OfficeState::Error, 5).unwrap();
        assert!(errored.pause(6).is_err());
        assert_eq!(errored.state(), OfficeState::Error);
    }

    #[test]
    fn same_state_transition_is_a_noop() {
        let mut oc = active_office();
        oc.transition(OfficeState::Active, 9).unwrap();
        assert!(oc.take_events().is_empty());
    }

    #[test]
    fn resume_is_idempotent_while_running() {
        let mut oc = active_office();
        oc.resume(Workload::Queued, 5).unwrap();
        assert_eq!(oc.state(), OfficeState::Active);
        assert!(oc.take_events().is_empty());
    }

    #[test]
    fn quota_exhausted_with_substitute_stays_running() {
        // A viable substitute keeps the office running — no hibernation.
        let mut oc = active_office();
        let outcome = oc.on_quota_exhausted(true, 10);
        assert_eq!(outcome, Some(HibernationOutcome::Substituted));
        assert_eq!(oc.state(), OfficeState::Active);
        assert!(oc.take_events().is_empty());
    }

    #[test]
    fn quota_exhausted_without_substitute_hibernates() {
        // No substitute within budget -> drain + hibernate + checkpoint.
        let mut oc = active_office();
        let outcome = oc.on_quota_exhausted(false, 10);
        assert_eq!(outcome, Some(HibernationOutcome::Hibernated));
        assert_eq!(oc.state(), OfficeState::Hibernating);
        assert!(oc.has_checkpoint());
        let events = oc.take_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].to, OfficeState::Hibernating);
    }

    #[test]
    fn quota_recovered_auto_wakes_from_hibernation() {
        // Recovery auto-resumes with no user action; checkpoint cleared.
        let mut oc = active_office();
        oc.on_quota_exhausted(false, 10);
        oc.take_events();
        let woke = oc.on_quota_recovered(Workload::Queued, 20);
        assert!(woke);
        assert_eq!(oc.state(), OfficeState::Active);
        assert!(!oc.has_checkpoint());
    }

    #[test]
    fn quota_signals_inert_when_not_applicable() {
        // Exhaustion is inert while frozen; recovery is inert while running.
        let mut paused = active_office();
        paused.pause(5).unwrap();
        assert_eq!(paused.on_quota_exhausted(false, 6), None);
        assert_eq!(paused.state(), OfficeState::Paused);

        let mut running = active_office();
        assert!(!running.on_quota_recovered(Workload::Empty, 6));
        assert_eq!(running.state(), OfficeState::Active);
    }

    #[test]
    fn individually_paused_subsystem_survives_master_resume() {
        // A subsystem paused individually stays paused after master resume.
        let mut oc = active_office();
        oc.pause_subsystem(Subsystem::Scheduler);
        assert!(oc.is_subsystem_paused(Subsystem::Scheduler));

        oc.pause(10).unwrap();
        oc.resume(Workload::Empty, 20).unwrap();
        // Master resume restored the office, but the scheduler stays paused.
        assert_eq!(oc.state(), OfficeState::Idle);
        assert!(oc.is_subsystem_paused(Subsystem::Scheduler));
        assert!(!oc.is_subsystem_active(Subsystem::Scheduler));
        // A subsystem never individually paused is active once the office runs.
        assert!(oc.is_subsystem_active(Subsystem::Automation));
    }

    #[test]
    fn subsystem_inactive_while_office_frozen() {
        let mut oc = active_office();
        oc.pause(10).unwrap();
        // No subsystem is active while the office itself is paused.
        assert!(!oc.is_subsystem_active(Subsystem::Heartbeat));
        oc.resume_subsystem(Subsystem::Scheduler); // idempotent when not paused
        assert!(!oc.is_subsystem_paused(Subsystem::Scheduler));
    }
}
