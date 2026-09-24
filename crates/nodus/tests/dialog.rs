//! Integration tests for the human-in-the-loop dialog contract.
//!
//! Covers the synchronous default resolver, the pause/resume signal, typed
//! binding through a custom provider, the typed failure outcomes, and the
//! capability-manifest derivation of the `Dialog` role.

use nodus::{
    AuditProvider, DefaultDialogProvider, ExecutionEvent, RunManifest,
    executor::{DialogOutcome, DialogProvider, Status, Value},
    parser::Parser,
    portability::{CapabilityManifest, ExtensionRole},
    workflows,
};
use std::sync::{Arc, Mutex};

// A dialog whose ASK carries a `+default` — resolved by the built-in provider.
const ASK_DEFAULT_WF: &str = r#"§wf:ask_default v1.0
§runtime: { core: schema.nodus }
@out: $answer
@err: ESCALATE(human)
@steps:
  1. ASK(question) +default=yes → $answer
  2. LOG($answer)
"#;

// A dialog with no default and a following step — pauses without a backend.
const ASK_PAUSE_WF: &str = r#"§wf:ask_pause v1.0
§runtime: { core: schema.nodus }
@out: $out
@err: ESCALATE(human)
@steps:
  1. ASK(question) → $answer
  2. GEN($answer) → $out
"#;

// ─── Custom dialog providers ─────────────────────────────────────────────────

struct AnswerProvider(&'static str);
impl DialogProvider for AnswerProvider {
    fn ask(&self, _prompt: &str, _modifiers: &[(String, String)]) -> DialogOutcome {
        DialogOutcome::Answer(Value::Text(self.0.to_string()))
    }
    fn confirm(&self, _content: &str, _modifiers: &[(String, String)]) -> DialogOutcome {
        DialogOutcome::Answer(Value::Bool(true))
    }
}

struct TimeoutProvider;
impl DialogProvider for TimeoutProvider {
    fn ask(&self, _p: &str, _m: &[(String, String)]) -> DialogOutcome {
        DialogOutcome::Timeout
    }
    fn confirm(&self, _c: &str, _m: &[(String, String)]) -> DialogOutcome {
        DialogOutcome::Timeout
    }
}

struct RejectProvider;
impl DialogProvider for RejectProvider {
    fn ask(&self, _p: &str, _m: &[(String, String)]) -> DialogOutcome {
        DialogOutcome::Rejected
    }
    fn confirm(&self, _c: &str, _m: &[(String, String)]) -> DialogOutcome {
        DialogOutcome::Rejected
    }
}

// ─── Default-on-absence ────────────────────────────────────────────────

#[test]
fn dialog_default_resolves_synchronously() {
    let result = workflows::run(ASK_DEFAULT_WF, "ask_default.nodus", None)
        .expect("a defaulted dialog must run");
    assert_eq!(
        result.status,
        Status::Ok,
        "a +default dialog resolves without pausing; errors: {:?}",
        result.errors
    );
    assert_eq!(
        result.vars.get("answer"),
        Some(&Value::Text("yes".to_string()))
    );
}

// ─── Pause + resume descriptor ──────────────────────────────────

#[test]
fn dialog_pauses_without_resolution() {
    let result =
        workflows::run(ASK_PAUSE_WF, "ask_pause.nodus", None).expect("the run returns a result");
    assert_eq!(
        result.status,
        Status::Paused,
        "an unresolved dialog with no default must pause"
    );
    let resume = result
        .resume
        .expect("a paused run carries a resume descriptor");
    assert_eq!(resume.workflow, "wf:ask_pause");
    assert_eq!(resume.step_index, 1, "suspended at the ASK step");
    // The following step must not have executed.
    assert!(
        !result.log.iter().any(|e| e.command == "GEN"),
        "no step after the dialog may run on pause; log: {:?}",
        result.log
    );
}

// ─── Typed binding via a custom provider ───────────────────────────────

#[test]
fn dialog_custom_answer_binds_to_target() {
    let result = workflows::run_with_dialog(
        ASK_PAUSE_WF,
        "ask_pause.nodus",
        None,
        AnswerProvider("hello"),
    )
    .expect("custom-answered dialog runs");
    assert_eq!(result.status, Status::Ok, "errors: {:?}", result.errors);
    assert_eq!(
        result.vars.get("answer"),
        Some(&Value::Text("hello".to_string()))
    );
}

// ─── Typed failure outcomes ────────────────────────────────────────────

#[test]
fn dialog_timeout_surfaces_error() {
    let result = workflows::run_with_dialog(ASK_PAUSE_WF, "ask_pause.nodus", None, TimeoutProvider)
        .expect("run");
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code == "NODUS:DIALOG_TIMEOUT"),
        "timeout must surface NODUS:DIALOG_TIMEOUT; errors: {:?}",
        result.errors
    );
}

#[test]
fn dialog_rejection_surfaces_error() {
    let result = workflows::run_with_dialog(ASK_PAUSE_WF, "ask_pause.nodus", None, RejectProvider)
        .expect("run");
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code == "NODUS:DIALOG_REJECTED"),
        "rejection must surface NODUS:DIALOG_REJECTED; errors: {:?}",
        result.errors
    );
}

// ─── Capability-manifest derivation ────────────────────────────────────

#[test]
fn manifest_requires_dialog_role_without_default() {
    let ast = Parser::parse(ASK_PAUSE_WF).expect("parse");
    let manifest = CapabilityManifest::from_workflow(&ast);
    assert!(
        manifest.roles().contains(&ExtensionRole::Dialog),
        "a non-defaulted dialog requires the Dialog role: {manifest:?}"
    );
}

#[test]
fn manifest_omits_dialog_role_with_default() {
    let ast = Parser::parse(ASK_DEFAULT_WF).expect("parse");
    let manifest = CapabilityManifest::from_workflow(&ast);
    assert!(
        !manifest.roles().contains(&ExtensionRole::Dialog),
        "a defaulted dialog is host-free and needs no Dialog role: {manifest:?}"
    );
}

// ─── Pinned-generation digest ──────────────────────────────────────

/// Captures the `RunManifest` `run_complete` delivers — the `RecordingProvider`
/// shape from `tests/observability.rs`, trimmed to just the manifest.
#[derive(Clone)]
struct ManifestCapture {
    manifests: Arc<Mutex<Vec<RunManifest>>>,
}

impl ManifestCapture {
    fn new() -> Self {
        ManifestCapture {
            manifests: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl AuditProvider for ManifestCapture {
    fn record_event(&self, _event: ExecutionEvent) {}

    fn run_complete(&self, manifest: RunManifest) {
        self.manifests.lock().unwrap().push(manifest);
    }
}

#[test]
fn resume_descriptor_digest_agrees_with_repro_recipe_digest() {
    // Both digests are built a few lines apart in `executor.rs`'s same
    // function, from the same `ast`, but by two independent construction
    // sites — `ResumeDescriptor` and `ReproRecipe` are never compared to each
    // other in production code. Proving they agree here is what makes the
    // definition-pinning claim meaningful rather than coincidental.
    let capture = ManifestCapture::new();
    let result = workflows::run_with_dialog_and_audit(
        ASK_PAUSE_WF,
        "ask_pause.nodus",
        None,
        DefaultDialogProvider,
        capture.clone(),
        "run-pinned-digest",
        "2026-01-01T00:00:00Z",
    )
    .expect("the run returns a result");

    assert_eq!(result.status, Status::Paused, "errors: {:?}", result.errors);
    let resume = result
        .resume
        .expect("a paused run carries a resume descriptor");

    let manifests = capture.manifests.lock().unwrap();
    assert_eq!(manifests.len(), 1, "run_complete must fire exactly once");
    assert_eq!(
        resume.workflow_digest, manifests[0].repro.workflow_digest,
        "ResumeDescriptor and ReproRecipe must agree on the pinned generation"
    );
}
