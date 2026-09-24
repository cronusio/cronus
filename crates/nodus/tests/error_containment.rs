//! An error nothing caught ends the step sequence — whether or not the workflow
//! declares an `@err:` handler — and arms the compensation unwind, so the
//! effects already committed are undone rather than left live.

use std::sync::{Arc, Mutex};

use nodus::{
    AuditProvider, ExecutionEvent, Executor, ModelError, ModelProvider, RunManifest, Status, Value,
    parser::Parser, vocab,
};

/// Fails every generation; every other command runs normally.
struct FailingModel;

impl ModelProvider for FailingModel {
    fn model_id(&self) -> &str {
        "failing-host"
    }

    fn generate(&self, _prompt: &str, _modifiers: &[(String, String)]) -> String {
        String::new()
    }

    fn analyze(&self, _text: &str, _flags: &[String]) -> Value {
        Value::Null
    }

    fn try_generate(
        &self,
        _prompt: &str,
        _modifiers: &[(String, String)],
    ) -> Result<String, ModelError> {
        Err(ModelError::new("endpoint unavailable"))
    }
}

#[derive(Clone, Default)]
struct Recorder {
    events: Arc<Mutex<Vec<ExecutionEvent>>>,
}

impl Recorder {
    fn commands(&self) -> Vec<String> {
        self.events
            .lock()
            .expect("lock")
            .iter()
            .filter_map(|e| match e {
                ExecutionEvent::StepStart { step_command, .. } => Some(step_command.clone()),
                _ => None,
            })
            .collect()
    }

    fn error_codes(&self) -> Vec<String> {
        self.events
            .lock()
            .expect("lock")
            .iter()
            .filter_map(|e| match e {
                ExecutionEvent::StepError { error_code, .. } => Some(error_code.clone()),
                _ => None,
            })
            .collect()
    }
}

impl AuditProvider for Recorder {
    fn record_event(&self, event: ExecutionEvent) {
        self.events.lock().expect("lock").push(event);
    }

    fn run_complete(&self, _manifest: RunManifest) {}
}

/// A committed, compensable effect; a step that fails; a step that must not run.
const WITH_HANDLER: &str = "\
§wf:with_handler v1.0
§runtime: { core: schema.nodus }
@out: $out
@err: ESCALATE(human)
@steps:
  1. REMEMBER(\"doc\") → $url ~COMPENSATE: NOTIFY($url)
  2. GEN(draft) → $out
  3. LOG($out)
";

const WITHOUT_HANDLER: &str = "\
§wf:without_handler v1.0
§runtime: { core: schema.nodus }
@out: $out
@steps:
  1. REMEMBER(\"doc\") → $url ~COMPENSATE: NOTIFY($url)
  2. GEN(draft) → $out
  3. LOG($out)
";

const CLEAN: &str = "\
§wf:clean v1.0
§runtime: { core: schema.nodus }
@out: $out
@steps:
  1. REMEMBER(\"doc\") → $url ~COMPENSATE: NOTIFY($url)
  2. LOG($url)
";

fn run(source: &str) -> (nodus::RunResult, Recorder) {
    let recorder = Recorder::default();
    let ast = Parser::parse(source).expect("parse");
    let result = Executor::with_audit(FailingModel, recorder.clone()).execute(&ast, None);
    (result, recorder)
}

// ─── No handler declared ─────────────────────────────────────────────────────

#[test]
fn without_a_handler_the_error_ends_the_sequence_as_unhandled() {
    let (result, recorder) = run(WITHOUT_HANDLER);

    assert!(
        !recorder.commands().iter().any(|c| c == "LOG"),
        "the step after the failed one must not run: {:?}",
        recorder.commands()
    );
    let codes: Vec<&str> = result.errors.iter().map(|e| e.code.as_str()).collect();
    assert_eq!(
        codes,
        [
            vocab::error_code::MODEL_CALL_FAILED,
            vocab::error_code::UNHANDLED_ERROR
        ],
        "the original error stays, followed by the unhandled marker"
    );
    let unhandled = result.errors.last().expect("marker");
    assert_eq!(unhandled.step, 2);
    assert!(
        unhandled
            .reason
            .contains(vocab::error_code::MODEL_CALL_FAILED),
        "the marker names what failed: {}",
        unhandled.reason
    );
    assert_eq!(result.status, Status::Partial);
    assert!(
        recorder
            .error_codes()
            .contains(&vocab::error_code::UNHANDLED_ERROR.to_string()),
        "and it appears in the trace"
    );
}

#[test]
fn with_a_handler_there_is_no_unhandled_marker_and_the_sequence_still_ends() {
    let (result, recorder) = run(WITH_HANDLER);

    assert!(
        result.flags.iter().any(|f| f.starts_with("ESCALATE:")),
        "the handler ran: {:?}",
        result.flags
    );
    assert!(!recorder.commands().iter().any(|c| c == "LOG"));
    assert!(
        result
            .errors
            .iter()
            .all(|e| e.code != vocab::error_code::UNHANDLED_ERROR),
        "a handled error is not also reported unhandled: {:?}",
        result.errors
    );
}

// ─── The unwind is armed by an uncaught error ────────────────────────────────

#[test]
fn an_error_routed_to_the_handler_still_unwinds_the_committed_effects() {
    let (_, recorder) = run(WITH_HANDLER);
    let commands = recorder.commands();
    let unwound = commands.iter().filter(|c| *c == "NOTIFY").count();
    assert_eq!(
        unwound, 1,
        "the compensation for step 1 ran once: {commands:?}"
    );
}

#[test]
fn an_unhandled_error_unwinds_the_committed_effects_too() {
    let (_, recorder) = run(WITHOUT_HANDLER);
    let commands = recorder.commands();
    assert_eq!(
        commands.iter().filter(|c| *c == "NOTIFY").count(),
        1,
        "{commands:?}"
    );
}

#[test]
fn the_failing_step_itself_is_never_compensated() {
    // Step 2 fails and declares no compensation; only step 1's runs.
    let (_, recorder) = run(WITH_HANDLER);
    let commands = recorder.commands();
    let gen_pos = commands.iter().position(|c| c == "GEN").expect("GEN ran");
    let notify_pos = commands
        .iter()
        .position(|c| c == "NOTIFY")
        .expect("NOTIFY ran");
    assert!(
        notify_pos > gen_pos,
        "the unwind comes after the failure: {commands:?}"
    );
}

#[test]
fn a_clean_run_never_unwinds() {
    let recorder = Recorder::default();
    let ast = Parser::parse(CLEAN).expect("parse");
    let result = Executor::with_audit(FailingModel, recorder.clone()).execute(&ast, None);
    assert_eq!(result.status, Status::Ok, "errors: {:?}", result.errors);
    assert!(
        !recorder.commands().iter().any(|c| c == "NOTIFY"),
        "compensation is armed by failure, never by success"
    );
}
