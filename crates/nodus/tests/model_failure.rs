//! A model call that fails is a failed step, never a short answer; the run
//! manifest's status says what actually happened (clean, errored, suspended).

use std::sync::{Arc, Mutex};

use nodus::{
    AuditProvider, DefaultDialogProvider, ExecutionEvent, Executor, ModelError, ModelProvider,
    RunManifest, RunStatus, Status, Value, parser::Parser, vocab,
};

// ─── Providers ───────────────────────────────────────────────────────────────

/// Fails every call the way a dropped connection would.
struct FailingModel;

impl ModelProvider for FailingModel {
    fn model_id(&self) -> &str {
        "failing-host"
    }

    fn generate(&self, _prompt: &str, _modifiers: &[(String, String)]) -> String {
        // What an infallible-only integration would return after a failure.
        "partial text before the connection dropped".to_string()
    }

    fn analyze(&self, _text: &str, _flags: &[String]) -> Value {
        Value::Null
    }

    fn try_generate(
        &self,
        _prompt: &str,
        _modifiers: &[(String, String)],
    ) -> Result<String, ModelError> {
        Err(ModelError::new("connection dropped"))
    }

    fn try_analyze(&self, _text: &str, _flags: &[String]) -> Result<Value, ModelError> {
        Err(ModelError::new("connection dropped"))
    }
}

/// Fails the first `n` calls, then answers — proves a retry re-runs the call.
struct FlakyModel {
    failures_left: Mutex<u32>,
}

impl ModelProvider for FlakyModel {
    fn model_id(&self) -> &str {
        "flaky-host"
    }

    fn generate(&self, _prompt: &str, _modifiers: &[(String, String)]) -> String {
        "recovered answer".to_string()
    }

    fn analyze(&self, _text: &str, _flags: &[String]) -> Value {
        Value::Null
    }

    fn try_generate(
        &self,
        prompt: &str,
        modifiers: &[(String, String)],
    ) -> Result<String, ModelError> {
        let mut left = self.failures_left.lock().expect("lock");
        if *left > 0 {
            *left -= 1;
            return Err(ModelError::new("transient failure"));
        }
        Ok(self.generate(prompt, modifiers))
    }
}

/// A provider that only implements the infallible pair keeps working unchanged.
struct LegacyModel;

impl ModelProvider for LegacyModel {
    fn model_id(&self) -> &str {
        "legacy-host"
    }

    fn generate(&self, _prompt: &str, _modifiers: &[(String, String)]) -> String {
        "legacy answer".to_string()
    }

    fn analyze(&self, _text: &str, _flags: &[String]) -> Value {
        Value::Null
    }
}

#[derive(Clone, Default)]
struct Recorder {
    events: Arc<Mutex<Vec<ExecutionEvent>>>,
    manifests: Arc<Mutex<Vec<RunManifest>>>,
}

impl AuditProvider for Recorder {
    fn record_event(&self, event: ExecutionEvent) {
        self.events.lock().expect("lock").push(event);
    }

    fn run_complete(&self, manifest: RunManifest) {
        self.manifests.lock().expect("lock").push(manifest);
    }
}

// ─── Workflows ───────────────────────────────────────────────────────────────

const GEN_WF: &str = "\
§wf:gen_wf v1.0
§runtime: { core: schema.nodus }
@out: $out
@err: ESCALATE(human)
@steps:
  1. GEN(hello) → $out
  2. LOG($out)
";

const GEN_NO_HANDLER_WF: &str = "\
§wf:gen_no_handler v1.0
§runtime: { core: schema.nodus }
@out: $out
@steps:
  1. GEN(hello) → $out
";

const RETRY_WF: &str = "\
§wf:retry_wf v1.0
§runtime: { core: schema.nodus }
@out: $out
@err: ESCALATE(human)
@steps:
  1. ~RETRY:3 GEN(hello) → $out
  2. LOG($out)
";

const ANALYZE_WF: &str = "\
§wf:analyze_wf v1.0
§runtime: { core: schema.nodus }
@out: $verdict
@err: ESCALATE(human)
@steps:
  1. ANALYZE(hello) ~sentiment → $verdict
";

const PAUSE_WF: &str = "\
§wf:pause_wf v1.0
§runtime: { core: schema.nodus }
@out: $out
@err: ESCALATE(human)
@steps:
  1. ASK(question) → $answer
  2. GEN($answer) → $out
";

fn run_on(provider: impl ModelProvider + 'static, source: &str) -> nodus::RunResult {
    let ast = Parser::parse(source).expect("parse");
    Executor::new(provider).execute(&ast, None)
}

// ─── A failed call is a failed step ──────────────────────────────────────────

#[test]
fn a_failed_generation_is_a_typed_error_not_a_short_answer() {
    let result = run_on(FailingModel, GEN_WF);

    assert_eq!(result.status, Status::Partial);
    let error = result.errors.first().expect("the failure is recorded");
    assert_eq!(error.code, vocab::error_code::MODEL_CALL_FAILED);
    assert_eq!(error.step, 1);
    assert!(
        error.reason.contains("connection dropped"),
        "the provider's reason reaches the record: {}",
        error.reason
    );
    // The partial text the infallible method would have returned never binds.
    assert_eq!(
        result.vars.get("out"),
        Some(&Value::Null),
        "the pipeline target must not hold an answer the call never produced"
    );
}

#[test]
fn a_failed_generation_reaches_the_declared_error_handler() {
    let result = run_on(FailingModel, GEN_WF);
    // NL-9: the handler ran (step 2 did not) and `$error` names the failure.
    assert!(
        result.flags.iter().any(|f| f.starts_with("ESCALATE:")),
        "the @err handler ran: {:?}",
        result.flags
    );
    assert!(
        result.log.iter().all(|entry| entry.command != "LOG"),
        "the steps after the failed one did not run"
    );
    match result.vars.get("error") {
        Some(Value::Map(entries)) => {
            let code = entries.iter().find(|(k, _)| k == "code").map(|(_, v)| v);
            assert_eq!(
                code,
                Some(&Value::Text(
                    vocab::error_code::MODEL_CALL_FAILED.to_string()
                )),
                "$error carries the typed code"
            );
        }
        other => panic!("$error must be populated, got {other:?}"),
    }
}

#[test]
fn a_failed_analysis_is_a_typed_error_too() {
    let result = run_on(FailingModel, ANALYZE_WF);
    let error = result.errors.first().expect("the failure is recorded");
    assert_eq!(error.code, vocab::error_code::MODEL_CALL_FAILED);
    assert_eq!(
        result.vars.get("verdict"),
        None,
        "the failed analysis binds nothing"
    );
}

#[test]
fn a_retry_reruns_a_failed_model_call_and_a_later_success_clears_the_failure() {
    let flaky = FlakyModel {
        failures_left: Mutex::new(2),
    };
    let result = run_on(flaky, RETRY_WF);
    assert_eq!(result.status, Status::Ok, "errors: {:?}", result.errors);
    assert_eq!(
        result.vars.get("out"),
        Some(&Value::Text("recovered answer".to_string()))
    );
}

#[test]
fn an_exhausted_retry_leaves_the_failure_standing() {
    let flaky = FlakyModel {
        failures_left: Mutex::new(9),
    };
    let result = run_on(flaky, RETRY_WF);
    assert_eq!(result.status, Status::Partial);
    assert_eq!(
        result.errors.last().map(|e| e.code.as_str()),
        Some(vocab::error_code::MODEL_CALL_FAILED)
    );
}

#[test]
fn the_failure_is_in_the_trace_with_a_stable_fault_identity() {
    let recorder = Recorder::default();
    let ast = Parser::parse(GEN_WF).expect("parse");
    let _ = Executor::with_audit(FailingModel, recorder.clone()).execute(&ast, None);
    let events = recorder.events.lock().expect("lock");
    let failure = events.iter().find_map(|e| match e {
        ExecutionEvent::StepError {
            error_code,
            fault_identity,
            ..
        } => Some((error_code.clone(), fault_identity.code.clone())),
        _ => None,
    });
    assert_eq!(
        failure,
        Some((
            vocab::error_code::MODEL_CALL_FAILED.to_string(),
            vocab::error_code::MODEL_CALL_FAILED.to_string()
        ))
    );
}

#[test]
fn a_provider_implementing_only_the_infallible_pair_is_unchanged() {
    let result = run_on(LegacyModel, GEN_WF);
    assert_eq!(result.status, Status::Ok, "errors: {:?}", result.errors);
    assert_eq!(
        result.vars.get("out"),
        Some(&Value::Text("legacy answer".to_string()))
    );
}

// ─── The manifest says what happened ─────────────────────────────────────────

fn manifest_status(provider: impl ModelProvider + 'static, source: &str) -> RunStatus {
    let recorder = Recorder::default();
    let ast = Parser::parse(source).expect("parse");
    let _ = Executor::with_audit(provider, recorder.clone()).execute(&ast, None);
    let manifests = recorder.manifests.lock().expect("lock");
    manifests[0].status.clone()
}

#[test]
fn a_clean_run_is_recorded_ok() {
    assert_eq!(manifest_status(LegacyModel, GEN_WF), RunStatus::Ok);
}

#[test]
fn a_run_that_ended_with_errors_is_recorded_as_an_error_not_ok() {
    assert_eq!(manifest_status(FailingModel, GEN_WF), RunStatus::Error);
}

#[test]
fn a_suspended_run_is_recorded_as_paused_not_ok() {
    let recorder = Recorder::default();
    let ast = Parser::parse(PAUSE_WF).expect("parse");
    let result = Executor::with_dialog_and_audit(DefaultDialogProvider, recorder.clone())
        .execute(&ast, None);
    assert_eq!(result.status, Status::Paused);
    let manifests = recorder.manifests.lock().expect("lock");
    assert_eq!(manifests[0].status, RunStatus::Paused);
}

#[test]
fn a_rule_violation_is_recorded_as_a_constraint_halt() {
    let source = "\
§wf:never_wf v1.0
§runtime: { core: schema.nodus }
!!NEVER: GEN
@out: $out
@steps:
  1. GEN(hello) → $out
";
    assert_eq!(
        manifest_status(LegacyModel, source),
        RunStatus::ConstraintHalt
    );
}

#[test]
fn a_run_without_a_handler_still_reports_the_failure() {
    let result = run_on(FailingModel, GEN_NO_HANDLER_WF);
    assert_eq!(result.status, Status::Partial);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code == vocab::error_code::MODEL_CALL_FAILED)
    );
}
