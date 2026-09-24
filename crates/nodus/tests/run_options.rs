//! The composed entry point: every seam at once. The point of composing them is
//! that a host policy can govern a real model call and a real settlement rail —
//! which no single-seam `run_with_*` could ever do.

use std::sync::{Arc, Mutex};

use nodus::{
    AuditProvider, CapabilityManifest, DialogOutcome, DialogProvider, ExecutionEvent,
    ExecutionMode, ExtensionRole, HostCapabilities, ModelProvider, PolicyProvider, RunManifest,
    RunOptions, SchemaProvider, SettlementRail, SimFidelity, Status, Value, ast::CommandCall,
    vocab, workflows,
};

// ─── Providers that record what reached them ─────────────────────────────────

#[derive(Clone, Default)]
struct CountingModel {
    calls: Arc<Mutex<u32>>,
}

impl CountingModel {
    fn calls(&self) -> u32 {
        *self.calls.lock().expect("lock")
    }
}

impl ModelProvider for CountingModel {
    fn model_id(&self) -> &str {
        "real-model"
    }

    fn generate(&self, prompt: &str, _modifiers: &[(String, String)]) -> String {
        *self.calls.lock().expect("lock") += 1;
        format!("real answer to {prompt}")
    }

    fn analyze(&self, _text: &str, _flags: &[String]) -> Value {
        Value::Null
    }
}

/// Permits or denies by effect class, and remembers what it was asked.
#[derive(Clone)]
struct GatePolicy {
    deny: &'static [&'static str],
    asked: Arc<Mutex<Vec<String>>>,
}

impl GatePolicy {
    fn denying(deny: &'static [&'static str]) -> Self {
        GatePolicy {
            deny,
            asked: Arc::default(),
        }
    }

    fn asked(&self) -> Vec<String> {
        self.asked.lock().expect("lock").clone()
    }
}

impl PolicyProvider for GatePolicy {
    fn evaluate(&self, gate: &str, _context: &Value) -> bool {
        self.asked.lock().expect("lock").push(gate.to_string());
        !self.deny.contains(&gate)
    }
}

#[derive(Clone, Default)]
struct CountingRail {
    payments: Arc<Mutex<u32>>,
}

impl CountingRail {
    fn payments(&self) -> u32 {
        *self.payments.lock().expect("lock")
    }
}

impl SettlementRail for CountingRail {
    fn settle(&self, _cmd: &CommandCall) -> Option<Value> {
        *self.payments.lock().expect("lock") += 1;
        Some(Value::Map(vec![(
            "receipt".to_string(),
            Value::Text("r-1".to_string()),
        )]))
    }
}

struct AnswerDialog;

impl DialogProvider for AnswerDialog {
    fn ask(&self, _prompt: &str, _modifiers: &[(String, String)]) -> DialogOutcome {
        DialogOutcome::Answer(Value::Text("blue".to_string()))
    }

    fn confirm(&self, _content: &str, _modifiers: &[(String, String)]) -> DialogOutcome {
        DialogOutcome::Answer(Value::Bool(true))
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

struct HostVocabulary;

impl SchemaProvider for HostVocabulary {
    fn host_commands(&self) -> &[&str] {
        &["FLUSHX"]
    }

    fn host_reserved_variables(&self) -> &[&str] {
        &[]
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

const SETTLE_WF: &str = "\
§wf:settle_wf v1.0
§runtime: { core: schema.nodus }
@out: $out
@err: ESCALATE(human)
@steps:
  1. SETTLE(vendor, 10.00, invoice) → $out
  2. LOG($out)
";

const ASK_WF: &str = "\
§wf:ask_wf v1.0
§runtime: { core: schema.nodus }
@out: $out
@err: ESCALATE(human)
@steps:
  1. ASK(colour) → $answer
  2. GEN($answer) → $out
";

const HOST_CMD_WF: &str = "\
§wf:host_cmd v1.0
§runtime: { core: schema.nodus }
@out: $out
@err: ESCALATE(human)
@steps:
  1. FLUSHX(cache)
  2. GEN(done) → $out
";

const INPUT_WF: &str = "\
§wf:input_wf v1.0
§runtime: { core: schema.nodus }
@in: { query: str }
@out: $out
@err: ESCALATE(human)
@steps:
  1. GEN($in.query) → $out
";

// ─── A policy governs a real model call ──────────────────────────────────────

#[test]
fn a_permitting_policy_lets_a_real_model_answer() {
    let model = CountingModel::default();
    let policy = GatePolicy::denying(&[]);
    let result = workflows::run_with_options(
        GEN_WF,
        "gen_wf.nodus",
        None,
        RunOptions::new()
            .model(model.clone())
            .policy(policy.clone()),
    )
    .expect("runs");

    assert_eq!(result.status, Status::Ok, "errors: {:?}", result.errors);
    assert_eq!(model.calls(), 1);
    assert_eq!(
        result.vars.get("out"),
        Some(&Value::Text("real answer to hello".to_string()))
    );
    assert!(policy.asked().contains(&"model_call".to_string()));
}

#[test]
fn a_denying_policy_stops_the_real_model_before_it_is_called() {
    let model = CountingModel::default();
    let policy = GatePolicy::denying(&["model_call"]);
    let result = workflows::run_with_options(
        GEN_WF,
        "gen_wf.nodus",
        None,
        RunOptions::new().model(model.clone()).policy(policy),
    )
    .expect("runs");

    assert_eq!(
        model.calls(),
        0,
        "the gate decides before the effect: the real model was never reached"
    );
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code == vocab::error_code::POLICY_DENIED),
        "{:?}",
        result.errors
    );
    assert_eq!(result.status, Status::Partial);
}

// ─── A policy governs a real settlement rail ─────────────────────────────────

#[test]
fn a_permitting_policy_lets_a_real_rail_settle() {
    let rail = CountingRail::default();
    let policy = GatePolicy::denying(&[]);
    let result = workflows::run_with_options(
        SETTLE_WF,
        "settle_wf.nodus",
        None,
        RunOptions::new()
            .settlement(rail.clone())
            .policy(policy.clone()),
    )
    .expect("runs");

    assert_eq!(result.status, Status::Ok, "errors: {:?}", result.errors);
    assert_eq!(rail.payments(), 1);
    assert!(policy.asked().contains(&"settlement".to_string()));
}

#[test]
fn a_denying_policy_stops_a_real_rail_before_any_payment() {
    let rail = CountingRail::default();
    let result = workflows::run_with_options(
        SETTLE_WF,
        "settle_wf.nodus",
        None,
        RunOptions::new()
            .settlement(rail.clone())
            .policy(GatePolicy::denying(&["settlement"])),
    )
    .expect("runs");

    assert_eq!(rail.payments(), 0, "no payment leaves on a denied gate");
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code == vocab::error_code::POLICY_DENIED)
    );
}

// ─── Seams together ──────────────────────────────────────────────────────────

#[test]
fn model_dialog_policy_and_audit_all_apply_in_one_run() {
    let model = CountingModel::default();
    let recorder = Recorder::default();
    let policy = GatePolicy::denying(&[]);
    let result = workflows::run_with_options(
        ASK_WF,
        "ask_wf.nodus",
        None,
        RunOptions::new()
            .model(model.clone())
            .dialog(AnswerDialog)
            .policy(policy.clone())
            .audit(recorder.clone())
            .run_metadata("run-7", "2026-09-24T00:00:00Z"),
    )
    .expect("runs");

    assert_eq!(result.status, Status::Ok, "errors: {:?}", result.errors);
    assert_eq!(
        result.vars.get("out"),
        Some(&Value::Text("real answer to blue".to_string())),
        "the dialog's answer fed the real model"
    );
    let asked = policy.asked();
    assert!(
        asked.contains(&"deferred".to_string()),
        "the dialog is gated: {asked:?}"
    );
    assert!(
        asked.contains(&"model_call".to_string()),
        "and so is the model: {asked:?}"
    );
    assert!(!recorder.events.lock().expect("lock").is_empty());
    let manifests = recorder.manifests.lock().expect("lock");
    assert_eq!(
        manifests[0].run_id, "run-7",
        "run metadata reaches the manifest"
    );
    assert_eq!(
        manifests[0].execution_mode,
        ExecutionMode::Real,
        "a real model, so a real run"
    );
}

#[test]
fn a_host_vocabulary_composes_with_the_other_seams() {
    let model = CountingModel::default();
    let result = workflows::run_with_options(
        HOST_CMD_WF,
        "host_cmd.nodus",
        None,
        RunOptions::new()
            .model(model.clone())
            .schema(&HostVocabulary),
    )
    .expect("the host command is known under its own vocabulary");
    assert_eq!(result.status, Status::Ok, "errors: {:?}", result.errors);
    assert_eq!(model.calls(), 1);

    let without = workflows::run_with_options(
        HOST_CMD_WF,
        "host_cmd.nodus",
        None,
        RunOptions::new().model(CountingModel::default()),
    )
    .expect_err("unknown without the host vocabulary");
    assert!(without.iter().any(|d| d.code == "E021"));
}

// ─── The same gate as every other entry point ────────────────────────────────

#[test]
fn the_input_contract_applies() {
    let err = workflows::run_with_options(INPUT_WF, "input_wf.nodus", None, RunOptions::new())
        .expect_err("required input missing");
    assert!(err.iter().any(|d| d.code == "E022"));
}

#[test]
fn validation_applies() {
    let err = workflows::run_with_options(
        "§wf:x v1.0\n@steps:\n  1. GEN(a)\n",
        "x.nodus",
        None,
        RunOptions::new(),
    )
    .expect_err("no runtime block");
    assert!(err.iter().any(|d| d.severity == nodus::Severity::Error));
}

#[test]
fn an_unsatisfiable_capability_gate_rejects_before_any_step() {
    let model = CountingModel::default();
    let manifest = CapabilityManifest::new().require_role(ExtensionRole::Settlement);
    let host = HostCapabilities::builtin();
    let result = workflows::run_with_options(
        GEN_WF,
        "gen_wf.nodus",
        None,
        RunOptions::new()
            .model(model.clone())
            .capability_gate(manifest, host),
    )
    .expect("a rejection is a result, not a diagnostic");

    assert_eq!(result.status, Status::Failed);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code == vocab::error_code::CAPABILITY_UNMET)
    );
    assert_eq!(model.calls(), 0, "no step ran");
}

// ─── Defaults ────────────────────────────────────────────────────────────────

#[test]
fn the_defaults_are_the_built_ins_and_record_a_simulation() {
    let recorder = Recorder::default();
    let result = workflows::run_with_options(
        GEN_WF,
        "gen_wf.nodus",
        None,
        RunOptions::new().audit(recorder.clone()),
    )
    .expect("runs");
    assert_eq!(result.status, Status::Ok);
    assert!(
        matches!(&result.vars["out"], Value::Text(s) if s.contains("STUB")),
        "the default model is the stub: {:?}",
        result.vars["out"]
    );
    assert_eq!(
        recorder.manifests.lock().expect("lock")[0].execution_mode,
        ExecutionMode::Simulated {
            fidelity: SimFidelity::Structural
        }
    );
}
