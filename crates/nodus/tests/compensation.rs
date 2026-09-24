//! Integration tests for the compensation seam.
//!
//! Reverse-completion-order driving, completed-only (a step whose
//! own action fails is never compensated), fallible-compensation-continues
//! (a failed compensation doesn't abort the unwind), armed-not-automatic
//! (a clean run touches nothing), and observer neutrality (no new
//! `ExecutionEvent` variant).

use nodus::observability::{AuditProvider, ExecutionEvent};
use nodus::workflows::{self, run_with_audit};
use std::sync::{Arc, Mutex};

// ─── Recording audit provider ─────────────────────────────────────────────────

struct RecordingProvider {
    events: Arc<Mutex<Vec<ExecutionEvent>>>,
}

impl RecordingProvider {
    fn new() -> Self {
        RecordingProvider {
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// The ordered sequence of `step_command` names for every `StepStart`
    /// event recorded — the observable proxy for "which effects ran, in
    /// what order," since the ledger itself is private to the executor.
    fn step_start_commands(&self) -> Vec<String> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                ExecutionEvent::StepStart { step_command, .. } => Some(step_command.clone()),
                _ => None,
            })
            .collect()
    }
}

impl Clone for RecordingProvider {
    fn clone(&self) -> Self {
        RecordingProvider {
            events: Arc::clone(&self.events),
        }
    }
}

impl AuditProvider for RecordingProvider {
    fn record_event(&self, event: ExecutionEvent) {
        self.events.lock().unwrap().push(event);
    }
    fn run_complete(&self, _manifest: nodus::observability::RunManifest) {}
}

// ─── Fixtures ─────────────────────────────────────────────────────────────────

// Two compensable steps (REMEMBER→NOTIFY, STORE→LOG), one non-compensable
// step (GEN), then a step that violates !!NEVER: FORGET — failing the run
// after two effects have already completed.
const COMPENSATING_WF: &str = r#"§wf:compensating_multi v1.0
§runtime: { core: schema.nodus }
!!NEVER: FORGET
@out: $out
@err: ESCALATE(human)
@steps:
  1. REMEMBER("mydoc") → $url ~COMPENSATE: NOTIFY($url)
  2. STORE("mydata") → $key ~COMPENSATE: LOG($key)
  3. GEN(third) → $draft
  4. FORGET(x)
"#;

// Same shape, but the second compensation (LOG) is itself forbidden — its
// own attempt must fail without aborting the rest of the unwind.
const COMPENSATION_ITSELF_FAILS_WF: &str = r#"§wf:compensation_fails v1.0
§runtime: { core: schema.nodus }
!!NEVER: FORGET
!!NEVER: LOG
@out: $out
@err: ESCALATE(human)
@steps:
  1. REMEMBER("mydoc") → $url ~COMPENSATE: NOTIFY($url)
  2. STORE("mydata") → $key ~COMPENSATE: LOG($key)
  3. FORGET(x)
"#;

// A clean run: no rule violation, so Status::Ok — compensation must never
// fire on success (armed, not automatic).
const CLEAN_COMPENSATING_WF: &str = r#"§wf:compensating_clean v1.0
§runtime: { core: schema.nodus }
@out: $out
@err: ESCALATE(human)
@steps:
  1. REMEMBER("mydoc") → $url ~COMPENSATE: NOTIFY($url)
  2. GEN(done) → $out
"#;

// A step whose own action violates a rule — its compensation must never run,
// because a step that never completed is never compensated.
const FAILING_STEP_HAS_COMPENSATION_WF: &str = r#"§wf:failing_step v1.0
§runtime: { core: schema.nodus }
!!NEVER: FORGET
@out: $out
@err: ESCALATE(human)
@steps:
  1. FORGET(x) → $ignored ~COMPENSATE: NOTIFY($ignored)
"#;

// ─── Reverse order + completed-only ───────────────────────────────────────────

#[test]
fn compensations_run_in_reverse_completion_order_on_failure() {
    let recorder = RecordingProvider::new();
    let result = run_with_audit(
        COMPENSATING_WF,
        "compensating_multi.nodus",
        None,
        recorder.clone(),
        "",
        "",
    )
    .expect("run");

    assert_eq!(
        result.status,
        nodus::executor::Status::Failed,
        "errors: {:?}",
        result.errors
    );

    let commands = recorder.step_start_commands();
    // Original steps in order: REMEMBER, STORE, GEN, (FORGET is blocked before
    // its own StepStart — check_rules fires first). Then the unwind: LOG
    // (step 2's compensation) before NOTIFY (step 1's) — reverse completion
    // order, not declaration order.
    let log_pos = commands.iter().position(|c| c == "LOG");
    let notify_pos = commands.iter().position(|c| c == "NOTIFY");
    assert!(
        log_pos.is_some() && notify_pos.is_some(),
        "both compensations must run; commands: {commands:?}"
    );
    assert!(
        log_pos < notify_pos,
        "LOG (step 2's compensation) must run before NOTIFY (step 1's) — LIFO order; commands: {commands:?}"
    );
    // GEN (step 3) has no compensation — it must never appear a second time.
    assert_eq!(
        commands.iter().filter(|c| *c == "GEN").count(),
        1,
        "the non-compensable step must not be touched during the unwind; commands: {commands:?}"
    );
}

#[test]
fn a_step_whose_own_action_fails_is_never_compensated() {
    let recorder = RecordingProvider::new();
    let result = run_with_audit(
        FAILING_STEP_HAS_COMPENSATION_WF,
        "failing_step.nodus",
        None,
        recorder.clone(),
        "",
        "",
    )
    .expect("run");

    assert_eq!(result.status, nodus::executor::Status::Failed);
    let commands = recorder.step_start_commands();
    assert!(
        !commands.iter().any(|c| c == "NOTIFY"),
        "a step that never completed must not have its compensation run; commands: {commands:?}"
    );
}

// ─── Fallible compensation continues the unwind ──────────────────────────────

#[test]
fn a_failed_compensation_is_surfaced_and_does_not_abort_the_unwind() {
    let recorder = RecordingProvider::new();
    let result = run_with_audit(
        COMPENSATION_ITSELF_FAILS_WF,
        "compensation_fails.nodus",
        None,
        recorder.clone(),
        "",
        "",
    )
    .expect("run");

    assert_eq!(result.status, nodus::executor::Status::Failed);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code == "NODUS:COMPENSATION_FAILED"),
        "a compensation that itself violates a rule must be surfaced as COMPENSATION_FAILED, \
         not swallowed; errors: {:?}",
        result.errors
    );
    // The failed compensation (step 2's LOG) must not stop step 1's NOTIFY
    // from still being attempted.
    let commands = recorder.step_start_commands();
    assert!(
        commands.iter().any(|c| c == "NOTIFY"),
        "the unwind must continue past a failed compensation; commands: {commands:?}"
    );
}

// ─── Armed, never automatic ───────────────────────────────────────────────────

#[test]
fn a_clean_run_never_triggers_compensation() {
    let recorder = RecordingProvider::new();
    let result = run_with_audit(
        CLEAN_COMPENSATING_WF,
        "compensating_clean.nodus",
        None,
        recorder.clone(),
        "",
        "",
    )
    .expect("run");

    assert_eq!(
        result.status,
        nodus::executor::Status::Ok,
        "errors: {:?}",
        result.errors
    );
    let commands = recorder.step_start_commands();
    assert!(
        !commands.iter().any(|c| c == "NOTIFY"),
        "a clean (Status::Ok) run must never run a compensation; commands: {commands:?}"
    );
}

// ─── Observer neutrality (no new ExecutionEvent variant) ─────────────────────

#[test]
fn compensation_emits_only_existing_event_variants() {
    let recorder = RecordingProvider::new();
    run_with_audit(
        COMPENSATING_WF,
        "compensating_multi.nodus",
        None,
        recorder.clone(),
        "",
        "",
    )
    .expect("run");

    // Compensations route through the same execute_command path as any
    // ordinary command, so every event they produce is an existing variant
    // (StepStart/StepEnd/etc.) — this exhaustive match compiles only because
    // no new ExecutionEvent variant exists (the closed taxonomy preserved). A source change
    // that added one would fail to compile here, not just silently pass.
    let events = recorder.events.lock().unwrap();
    for e in events.iter() {
        match e {
            ExecutionEvent::StepStart { .. }
            | ExecutionEvent::StepEnd { .. }
            | ExecutionEvent::StepError { .. }
            | ExecutionEvent::ConstraintHit { .. }
            | ExecutionEvent::BranchTaken { .. }
            | ExecutionEvent::LoopIteration { .. }
            | ExecutionEvent::MacroEnter { .. }
            | ExecutionEvent::MacroExit { .. }
            | ExecutionEvent::ModelCall { .. }
            | ExecutionEvent::ModelResponse { .. } => {}
        }
    }
}

// ─── Zero-dep sanity (workflows::run stays validated) ────────────────────────

#[test]
fn compensating_workflow_passes_validation() {
    workflows::run(CLEAN_COMPENSATING_WF, "compensating_clean.nodus", None)
        .expect("a ~COMPENSATE-bearing workflow must validate and run normally");
}
