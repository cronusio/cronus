//! Integration tests for the portability extension seam.
//!
//! Verifies the LP-4 vocabulary-isolation contract: builtin constants are
//! never mutated; host commands extend the Schema value only. Tests cover:
//! - `run_with_schema` recognizes host-declared commands (LP-4)
//! - Without schema extension, host commands are silently parsed as text and
//!   never appear in the execution log (LP-4 isolation gate)
//! - `NoopPolicyProvider` compiles and satisfies its trait without executor
//!   wiring (LP-2 no-op contract)
//! - `InMemoryStorageProvider` round-trips within one invocation and shares
//!   no state across instances (LP-2 built-in sufficiency, §4.1/§4.11)
//! - `PolicyProvider` gates a `ModelCall` and a `Deferred` effect before it
//!   runs, non-halting on denial, byte-for-byte unchanged with no policy
//!   supplied (LP-11, §4.9)

use nodus::{
    ast::CommandCall,
    executor::{DefaultDialogProvider, DialogOutcome, DialogProvider, Status, Value},
    observability::{AuditProvider, DialogProvenance, ExecutionEvent, RunManifest},
    portability::{
        BuiltinSchemaProvider, CapabilityManifest, ExtensionRole, HostCapabilities,
        InMemoryStorageProvider, NoopPolicyProvider, NoopSettlementRail, PolicyProvider,
        SchemaProvider, SettlementRail, StorageProvider,
    },
    workflows,
};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

// ─── Workflow fixture ─────────────────────────────────────────────────────────

const HOST_CMD_WF: &str = r#"§wf:host_cmd_test v1.0
§runtime: { core: schema.nodus }
@in: { query? }
@out: $out
@err: ESCALATE(human)
@steps:
  1. CUSTOM_CMD($in.query) → $out
  2. LOG($out)
"#;

// ─── SchemaProvider implementation used in tests ─────────────────────────────

struct TestSchemaProvider;

impl SchemaProvider for TestSchemaProvider {
    fn host_commands(&self) -> &[&str] {
        &["CUSTOM_CMD"]
    }

    fn host_reserved_variables(&self) -> &[&str] {
        &[]
    }
}

// ─── LP-4 vocabulary extension gate ──────────────────────────────────

#[test]
fn host_schema_extends_builtin() {
    // run_with_schema recognizes CUSTOM_CMD as a valid command call.
    // The command executes (hits the UNKNOWN_COMMAND fallthrough in the executor
    // which returns Value::Null without setting an error), so status is Ok.
    let result = workflows::run_with_schema(
        HOST_CMD_WF,
        "host_cmd_test.nodus",
        None,
        &TestSchemaProvider,
    )
    .expect("run_with_schema must succeed when CUSTOM_CMD is registered");

    assert_eq!(
        result.status,
        Status::Ok,
        "registered host command must not cause a non-Ok status; errors: {:?}",
        result.errors
    );

    // CUSTOM_CMD must appear in the execution log, proving it was dispatched.
    assert!(
        result.log.iter().any(|e| e.command == "CUSTOM_CMD"),
        "CUSTOM_CMD must appear in the execution log; log: {:?}",
        result.log
    );
}

#[test]
fn an_unregistered_command_is_a_validation_error_not_a_silent_skip() {
    // Without schema extension, CUSTOM_CMD is an unknown ALL_CAPS identifier.
    // The parser keeps such a line as raw text, which used to drop it out of
    // the run: the workflow finished `Ok` without ever having done the step.
    // An unknown command has to fail at validation (NL-1), before anything runs.
    let diagnostics = workflows::run(HOST_CMD_WF, "host_cmd_test.nodus", None)
        .expect_err("a workflow using an unregistered command must not run");

    let unknown: Vec<_> = diagnostics.iter().filter(|d| d.code == "E021").collect();
    assert_eq!(
        unknown.len(),
        1,
        "one E021 for the one command: {diagnostics:?}"
    );
    assert!(
        unknown[0].message.contains("CUSTOM_CMD"),
        "the diagnostic names the command: {}",
        unknown[0].message
    );
}

#[test]
fn validation_reports_the_unknown_command_without_running_anything() {
    let report = workflows::validate(HOST_CMD_WF, "host_cmd_test.nodus").expect("parses");
    assert!(report.has_errors);
    assert!(report.diagnostics.iter().any(|d| d.code == "E021"));
}

// ─── Noop-provider compilation test ──────────────────────────────────

#[test]
fn noop_policy_and_schema_compile() {
    // Verify NoopPolicyProvider and BuiltinSchemaProvider satisfy their traits
    // without any executor wiring. Exercises LP-2: every extension point ships
    // with a built-in implementation sufficient for in-process use.
    let policy = NoopPolicyProvider;

    // evaluate always returns true
    assert!(policy.evaluate("spend_cap", &Value::Null));
    assert!(policy.evaluate("tool_access", &Value::Map(Vec::new())));

    // BuiltinSchemaProvider returns empty slices
    let bs = BuiltinSchemaProvider;
    assert!(bs.host_commands().is_empty());
    assert!(bs.host_reserved_variables().is_empty());
}

// ─── InMemoryStorageProvider built-in conformance ─────────────────────────────

#[test]
fn in_memory_storage_round_trips_within_invocation() {
    // (a) store -> load returns the equal value, satisfying L1 §4.1's
    // in-memory built-in mandate (LP-15) that NoopStorageProvider could not.
    let storage = InMemoryStorageProvider::new();
    storage.store("key", &Value::Text("hello".to_string()));
    assert_eq!(storage.load("key"), Some(Value::Text("hello".to_string())));
}

#[test]
fn in_memory_storage_overwrites_on_repeated_store() {
    // (b) storing twice on the same key overwrites rather than accumulating.
    let storage = InMemoryStorageProvider::new();
    storage.store("key", &Value::Int(1));
    storage.store("key", &Value::Int(2));
    assert_eq!(storage.load("key"), Some(Value::Int(2)));
}

#[test]
fn in_memory_storage_absent_key_returns_none() {
    // (c) load on a key that was never stored still returns None.
    let storage = InMemoryStorageProvider::new();
    storage.store("key", &Value::Bool(true));
    assert!(storage.load("missing").is_none());
}

#[test]
fn in_memory_storage_instances_share_no_state() {
    // (d) two separate provider instances are isolated — the property that
    // makes the built-in safe for in-process testing (LP-2's stated purpose).
    let a = InMemoryStorageProvider::new();
    let b = InMemoryStorageProvider::new();
    a.store("key", &Value::Text("only in a".to_string()));
    assert!(b.load("key").is_none());
}

// ─── LP-8 capability manifest fixtures ───────────────────────────────────────

const MANIFEST_WF: &str = r#"§wf:manifest_test v1.0
§runtime: { core: schema.nodus }
@in: { query? }
@out: $out
@err: ESCALATE(human)
@steps:
  1. GEN($in.query) → $out
  2. LOG($out)
"#;

/// Audit sink that counts every event and run-complete callback it receives.
/// Used to prove a rejected run emits nothing (observer neutrality).
struct CountingAudit {
    events: Arc<AtomicUsize>,
}

impl AuditProvider for CountingAudit {
    fn record_event(&self, _event: ExecutionEvent) {
        self.events.fetch_add(1, Ordering::SeqCst);
    }

    fn run_complete(&self, _manifest: RunManifest) {
        self.events.fetch_add(1, Ordering::SeqCst);
    }
}

// ─── Pre-run fail-fast capability gate ───────────────────────────────────────

#[test]
fn run_with_manifest_rejects_unmet_capability() {
    // A workflow needing Storage, run against the builtin host (no Storage), is
    // rejected before any step executes.
    let manifest = CapabilityManifest::new().require_role(ExtensionRole::Storage);
    let host = HostCapabilities::builtin();

    let result =
        workflows::run_with_manifest(MANIFEST_WF, "manifest_test.nodus", None, &manifest, &host)
            .expect("the manifest gate returns a RunResult, not a parse error");

    assert_eq!(
        result.status,
        Status::Failed,
        "an unsatisfiable manifest must fail the run"
    );
    assert!(
        result.log.is_empty(),
        "no step may execute on a rejected run; log: {:?}",
        result.log
    );
    assert!(
        result
            .errors
            .iter()
            .any(|e| { e.code == "NODUS:CAPABILITY_UNMET" && e.reason.contains("Storage") }),
        "rejection must name the missing Storage capability; errors: {:?}",
        result.errors
    );
}

#[test]
fn run_with_manifest_runs_when_satisfiable() {
    // A model-only workflow's derived manifest is satisfied by the builtin host.
    let manifest = CapabilityManifest::from_workflow(
        &nodus::parser::Parser::parse(MANIFEST_WF).expect("parse"),
    );
    let host = HostCapabilities::builtin();

    let result =
        workflows::run_with_manifest(MANIFEST_WF, "manifest_test.nodus", None, &manifest, &host)
            .expect("run");

    assert_eq!(
        result.status,
        Status::Ok,
        "the builtin host satisfies a model-only manifest; errors: {:?}",
        result.errors
    );
    assert!(
        !result.log.is_empty(),
        "steps must execute when satisfiable"
    );
}

// ─── LP-3 two-host substitution ──────────────────────────────────────────────

#[test]
fn manifest_lp3_two_host_substitution() {
    // The LP-3 reduction: portability ⇔ "does host B satisfy the same manifest
    // host A satisfied?"
    let manifest = CapabilityManifest::new().require_role(ExtensionRole::Storage);

    // Host A provides Storage → satisfiable → runs to completion.
    let host_a = HostCapabilities::builtin().with_role(ExtensionRole::Storage);
    let result_a =
        workflows::run_with_manifest(MANIFEST_WF, "manifest_test.nodus", None, &manifest, &host_a)
            .expect("host A run");
    assert_eq!(
        result_a.status,
        Status::Ok,
        "host A satisfies the manifest; errors: {:?}",
        result_a.errors
    );

    // Host B lacks Storage → the same manifest is unsatisfiable → rejected.
    let host_b = HostCapabilities::builtin();
    let result_b =
        workflows::run_with_manifest(MANIFEST_WF, "manifest_test.nodus", None, &manifest, &host_b)
            .expect("host B run");
    assert_eq!(
        result_b.status,
        Status::Failed,
        "host B does not satisfy the manifest"
    );
    assert!(
        result_b.errors.iter().any(|e| e.reason.contains("Storage")),
        "host B rejection must name Storage; errors: {:?}",
        result_b.errors
    );
}

// ─── Rejection precedes side effects (observer neutrality) ───────────────────

#[test]
fn manifest_rejects_before_side_effects() {
    let manifest = CapabilityManifest::new().require_role(ExtensionRole::Storage);

    // Rejected run: the audit sink must record nothing.
    let host = HostCapabilities::builtin();
    let counter = Arc::new(AtomicUsize::new(0));
    let result = workflows::run_with_manifest_and_audit(
        MANIFEST_WF,
        "manifest_test.nodus",
        None,
        &manifest,
        &host,
        CountingAudit {
            events: counter.clone(),
        },
        "run-rejected",
        "2026-06-26T00:00:00Z",
    )
    .expect("audited manifest run");

    assert_eq!(result.status, Status::Failed);
    assert_eq!(
        counter.load(Ordering::SeqCst),
        0,
        "a rejected run must emit no audit events (observer neutrality)"
    );

    // Control: a satisfiable audited run DOES emit events — proves the sink counts.
    let host_ok = HostCapabilities::builtin().with_role(ExtensionRole::Storage);
    let counter_ok = Arc::new(AtomicUsize::new(0));
    let _ = workflows::run_with_manifest_and_audit(
        MANIFEST_WF,
        "manifest_test.nodus",
        None,
        &manifest,
        &host_ok,
        CountingAudit {
            events: counter_ok.clone(),
        },
        "run-ok",
        "2026-06-26T00:00:00Z",
    )
    .expect("audited ok run");
    assert!(
        counter_ok.load(Ordering::SeqCst) > 0,
        "a real run must emit audit events"
    );
}

// ─── LP-11 per-effect authorization gate ──────────────────────────────────────

const DEFERRED_WF: &str = r#"§wf:deferred_test v1.0
§runtime: { core: schema.nodus }
@in: { query? }
@out: $out
@err: ESCALATE(human)
@steps:
  1. ASK(question) +default=yes → $out
"#;

struct AllowAllPolicy;

impl PolicyProvider for AllowAllPolicy {
    fn evaluate(&self, _gate: &str, _context: &Value) -> bool {
        true
    }
}

struct DenyAllPolicy;

impl PolicyProvider for DenyAllPolicy {
    fn evaluate(&self, _gate: &str, _context: &Value) -> bool {
        false
    }
}

#[test]
fn policy_permits_model_call_effect() {
    let result =
        workflows::run_with_policy(MANIFEST_WF, "manifest_test.nodus", None, AllowAllPolicy)
            .expect("run_with_policy must succeed when the effect is permitted");

    assert_eq!(
        result.status,
        Status::Ok,
        "a permitted GEN step must not degrade status; errors: {:?}",
        result.errors
    );
    // `$out` is always present (seeded `Value::Null` at context construction) —
    // the real assertion is that GEN actually ran and overwrote it.
    assert_ne!(
        result.vars.get("out"),
        Some(&Value::Null),
        "the permitted step's pipeline target must be bound to GEN's real output; vars: {:?}",
        result.vars
    );
}

#[test]
fn policy_denies_model_call_effect() {
    let result =
        workflows::run_with_policy(MANIFEST_WF, "manifest_test.nodus", None, DenyAllPolicy)
            .expect("run_with_policy returns a RunResult, not a parse error, on denial");

    assert_eq!(
        result.status,
        Status::Partial,
        "a denied effect must degrade status to Partial, not Failed (non-halting); errors: {:?}",
        result.errors
    );
    assert_eq!(
        result.vars.get("out"),
        Some(&Value::Null),
        "a denied step's pipeline target must stay at its seeded default, never GEN's output; vars: {:?}",
        result.vars
    );
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code == "NODUS:POLICY_DENIED" && e.reason.contains("model_call")),
        "denial must record a typed POLICY_DENIED error naming the gate; errors: {:?}",
        result.errors
    );
}

#[test]
fn policy_permits_deferred_effect() {
    let result =
        workflows::run_with_policy(DEFERRED_WF, "deferred_test.nodus", None, AllowAllPolicy)
            .expect("run_with_policy must succeed when the effect is permitted");

    assert_eq!(
        result.status,
        Status::Ok,
        "a permitted ASK step must resolve via +default and not degrade status; errors: {:?}",
        result.errors
    );
    assert_eq!(
        result.vars.get("out"),
        Some(&Value::Text("yes".to_string())),
        "the permitted dialog's pipeline target must be bound to its +default answer; vars: {:?}",
        result.vars
    );
}

#[test]
fn policy_denies_deferred_effect() {
    let result =
        workflows::run_with_policy(DEFERRED_WF, "deferred_test.nodus", None, DenyAllPolicy)
            .expect("run_with_policy returns a RunResult, not a parse error, on denial");

    assert_eq!(
        result.status,
        Status::Partial,
        "a denied deferred effect must degrade status to Partial, not Failed; errors: {:?}",
        result.errors
    );
    assert_eq!(
        result.vars.get("out"),
        Some(&Value::Null),
        "a denied dialog's pipeline target must stay at its seeded default — the dialog must never resolve; vars: {:?}",
        result.vars
    );
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code == "NODUS:POLICY_DENIED" && e.reason.contains("deferred")),
        "denial must record a typed POLICY_DENIED error naming the gate; errors: {:?}",
        result.errors
    );
}

#[test]
fn no_policy_supplied_is_byte_for_byte_unchanged() {
    // Guardrail 5: every existing run_with_* variant must keep NoopPolicyProvider's
    // allow-all behaviour now that Executor carries a policy field.
    let via_plain = workflows::run(MANIFEST_WF, "manifest_test.nodus", None).expect("plain run");
    let via_explicit_noop =
        workflows::run_with_policy(MANIFEST_WF, "manifest_test.nodus", None, NoopPolicyProvider)
            .expect("run_with_policy with the explicit noop");

    assert_eq!(via_plain.status, via_explicit_noop.status);
    assert_eq!(via_plain.vars, via_explicit_noop.vars);
    assert_eq!(
        via_plain.status,
        Status::Ok,
        "must remain unaffected by LP-11's addition"
    );
}

// ─── LP-16 effect risk-class descriptors ──────────────────────────────────────

const RISK_DECORATED_WF: &str = r#"§wf:risk_decorated_test v1.0
§runtime: { core: schema.nodus }
@in: { query? }
@out: $out
@err: ESCALATE(human)
@steps:
  1. GEN($in.query) +reversible=true +external=true +value=money → $out
"#;

/// Policy that permits everything but records the last `context` it was asked
/// to evaluate, so a test can inspect exactly what the gate passed through.
struct CapturingPolicy {
    seen: Arc<Mutex<Option<Value>>>,
}

impl PolicyProvider for CapturingPolicy {
    fn evaluate(&self, _gate: &str, context: &Value) -> bool {
        *self.seen.lock().unwrap() = Some(context.clone());
        true
    }
}

fn context_pairs(context: Value) -> Vec<(String, Value)> {
    match context {
        Value::Map(pairs) => pairs,
        other => panic!("expected the captured context to be a Value::Map, got {other:?}"),
    }
}

#[test]
fn risk_descriptors_reach_context_when_declared() {
    let seen = Arc::new(Mutex::new(None));
    let policy = CapturingPolicy { seen: seen.clone() };
    let result =
        workflows::run_with_policy(RISK_DECORATED_WF, "risk_decorated_test.nodus", None, policy)
            .expect("run_with_policy must succeed when the effect is permitted");

    assert_eq!(
        result.status,
        Status::Ok,
        "a permitted, decorated GEN step must not degrade status; errors: {:?}",
        result.errors
    );

    let captured = seen
        .lock()
        .unwrap()
        .clone()
        .expect("the policy must have been consulted for the gated GEN step");
    let pairs = context_pairs(captured);
    let get = |key: &str| pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
    assert_eq!(
        get("reversible"),
        Some(Value::Text("true".to_string())),
        "declared +reversible=true must reach context verbatim; pairs: {pairs:?}"
    );
    assert_eq!(
        get("external"),
        Some(Value::Text("true".to_string())),
        "declared +external=true must reach context verbatim; pairs: {pairs:?}"
    );
    assert_eq!(
        get("value"),
        Some(Value::Text("money".to_string())),
        "declared +value=money must reach context verbatim; pairs: {pairs:?}"
    );
}

#[test]
fn undeclared_risk_descriptors_are_absent_from_context_not_defaulted() {
    let seen = Arc::new(Mutex::new(None));
    let policy = CapturingPolicy { seen: seen.clone() };
    let result = workflows::run_with_policy(MANIFEST_WF, "manifest_test.nodus", None, policy)
        .expect("run_with_policy must succeed when the effect is permitted");

    assert_eq!(result.status, Status::Ok, "errors: {:?}", result.errors);

    let captured = seen
        .lock()
        .unwrap()
        .clone()
        .expect("the policy must have been consulted for the gated GEN step");
    let pairs = context_pairs(captured);
    for key in ["reversible", "external", "value"] {
        assert!(
            pairs.iter().all(|(k, _)| k != key),
            "an undecorated step's context must omit '{key}' entirely, never default it \
             to Null/false/\"none\"; pairs: {pairs:?}"
        );
    }
}

#[test]
fn risk_descriptors_are_inert_without_a_policy_provider() {
    // Regression guard: decorating a step with
    // +reversible/+external/+value must not change behaviour when no
    // PolicyProvider is present to consult them.
    let via_plain =
        workflows::run(RISK_DECORATED_WF, "risk_decorated_test.nodus", None).expect("plain run");
    let via_explicit_noop = workflows::run_with_policy(
        RISK_DECORATED_WF,
        "risk_decorated_test.nodus",
        None,
        NoopPolicyProvider,
    )
    .expect("run_with_policy with the explicit noop");

    assert_eq!(via_plain.status, via_explicit_noop.status);
    assert_eq!(via_plain.vars, via_explicit_noop.vars);
    assert_eq!(
        via_plain.status,
        Status::Ok,
        "must remain unaffected by LP-16's addition"
    );
}

// ─── NL-9 uncaught-error handler dispatch ─────────────────────────────────────

/// Two declared steps + a real `@err:` handler, so "the second step never ran"
/// is directly observable in `result.log`.
const ERR_HANDLER_WF: &str = r#"§wf:err_handler_test v1.0
§runtime: { core: schema.nodus }
@in: { query? }
@out: $out
@err: ESCALATE(human)
@steps:
  1. GEN($in.query) → $out
  2. LOG($out)
"#;

/// Same shape, but the `@err:` line carries no handler text at all.
const EMPTY_ERR_HANDLER_WF: &str = r#"§wf:empty_err_handler_test v1.0
§runtime: { core: schema.nodus }
@in: { query? }
@out: $out
@err:
@steps:
  1. GEN($in.query) → $out
  2. LOG($out)
"#;

/// No `@err:` line at all.
const NO_ERR_HANDLER_WF: &str = r#"§wf:no_err_handler_test v1.0
§runtime: { core: schema.nodus }
@in: { query? }
@out: $out
@steps:
  1. GEN($in.query) → $out
  2. LOG($out)
"#;

/// `~RETRY:2` GEN step that fails its first attempt then succeeds.
const RETRY_ERR_HANDLER_WF: &str = r#"§wf:retry_err_handler_test v1.0
§runtime: { core: schema.nodus }
@in: { query? }
@out: $out
@err: ESCALATE(human)
@steps:
  1. ~RETRY:2 GEN($in.query) → $out
"#;

struct DialogRejects;

impl DialogProvider for DialogRejects {
    fn ask(&self, _prompt: &str, _modifiers: &[(String, String)]) -> DialogOutcome {
        DialogOutcome::Rejected
    }

    fn confirm(&self, _content: &str, _modifiers: &[(String, String)]) -> DialogOutcome {
        DialogOutcome::Rejected
    }
}

/// Denies exactly the first `evaluate` call, permits every call after —
/// proves a `~RETRY:n` step that fails once then succeeds never reaches
/// `@err:` dispatch (the error left by the first attempt is truncated by
/// `run_step_with_retry` itself once the step succeeds).
struct DenyOncePolicy {
    calls: AtomicUsize,
}

impl DenyOncePolicy {
    fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
        }
    }
}

impl PolicyProvider for DenyOncePolicy {
    fn evaluate(&self, _gate: &str, _context: &Value) -> bool {
        self.calls.fetch_add(1, Ordering::SeqCst) != 0
    }
}

fn empty_error_map() -> Value {
    Value::Map(Vec::new())
}

#[test]
fn err_handler_dispatches_on_policy_denial() {
    let result = workflows::run_with_policy(
        ERR_HANDLER_WF,
        "err_handler_test.nodus",
        None,
        DenyAllPolicy,
    )
    .expect("run_with_policy returns a RunResult, not a parse error, on denial");

    assert_eq!(
        result.status,
        Status::Partial,
        "errors: {:?}",
        result.errors
    );
    let error_pairs = context_pairs(
        result
            .vars
            .get("error")
            .cloned()
            .expect("$error is always seeded"),
    );
    let get = |key: &str| {
        error_pairs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    };
    assert_eq!(
        get("code"),
        Some(Value::Text("NODUS:POLICY_DENIED".to_string()))
    );
    assert_eq!(get("step"), Some(Value::Int(1)));
    assert!(
        result.log.iter().any(|e| e.command == "ESCALATE"),
        "the declared @err: handler must actually dispatch; log: {:?}",
        result.log
    );
    assert!(
        !result.log.iter().any(|e| e.command == "LOG"),
        "the main step sequence must end after dispatch — step 2 must not run; log: {:?}",
        result.log
    );
}

#[test]
fn err_handler_dispatches_on_dialog_denial() {
    let result =
        workflows::run_with_dialog(DEFERRED_WF, "deferred_test.nodus", None, DialogRejects)
            .expect("run_with_dialog returns a RunResult, not a parse error, on rejection");

    assert_eq!(
        result.status,
        Status::Partial,
        "errors: {:?}",
        result.errors
    );
    let error_pairs = context_pairs(
        result
            .vars
            .get("error")
            .cloned()
            .expect("$error is always seeded"),
    );
    let get = |key: &str| {
        error_pairs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    };
    assert_eq!(
        get("code"),
        Some(Value::Text("NODUS:DIALOG_REJECTED".to_string()))
    );
    assert!(
        result.log.iter().any(|e| e.command == "ESCALATE"),
        "the declared @err: handler must actually dispatch; log: {:?}",
        result.log
    );
}

#[test]
fn no_err_handler_declared_is_unchanged() {
    let result = workflows::run_with_policy(
        NO_ERR_HANDLER_WF,
        "no_err_handler_test.nodus",
        None,
        DenyAllPolicy,
    )
    .expect("run_with_policy returns a RunResult, not a parse error, on denial");

    assert_eq!(
        result.status,
        Status::Partial,
        "errors: {:?}",
        result.errors
    );
    assert_eq!(
        result.vars.get("error"),
        Some(&empty_error_map()),
        "$error must stay at its seeded default with no handler declared; vars: {:?}",
        result.vars
    );
    assert!(
        !result.log.iter().any(|e| e.command == "ESCALATE"),
        "no handler is declared, so nothing may dispatch; log: {:?}",
        result.log
    );
}

#[test]
fn empty_err_handler_is_unchanged() {
    let result = workflows::run_with_policy(
        EMPTY_ERR_HANDLER_WF,
        "empty_err_handler_test.nodus",
        None,
        DenyAllPolicy,
    )
    .expect("run_with_policy returns a RunResult, not a parse error, on denial");

    assert_eq!(
        result.status,
        Status::Partial,
        "errors: {:?}",
        result.errors
    );
    assert_eq!(
        result.vars.get("error"),
        Some(&empty_error_map()),
        "$error must stay at its seeded default when @err: carries no handler text; vars: {:?}",
        result.vars
    );
    assert!(
        !result.log.iter().any(|e| e.command == "ESCALATE"),
        "an @err: line with no handler text must not dispatch anything; log: {:?}",
        result.log
    );
}

#[test]
fn retry_then_succeed_never_dispatches_err_handler() {
    let result = workflows::run_with_policy(
        RETRY_ERR_HANDLER_WF,
        "retry_err_handler_test.nodus",
        None,
        DenyOncePolicy::new(),
    )
    .expect("run_with_policy must succeed once the retried attempt is permitted");

    assert_eq!(
        result.status,
        Status::Ok,
        "a step that succeeds via retry must not degrade status; errors: {:?}",
        result.errors
    );
    assert_eq!(
        result.vars.get("error"),
        Some(&empty_error_map()),
        "retry-then-succeed must never populate $error; vars: {:?}",
        result.vars
    );
    assert!(
        !result.log.iter().any(|e| e.command == "ESCALATE"),
        "retry-then-succeed must never dispatch the @err: handler; log: {:?}",
        result.log
    );
}

// ─── LP-17 settlement effect seam ─────────────────────────────────────────────

const SETTLE_WF: &str = r#"§wf:settle_test v1.0
§runtime: { core: schema.nodus }
@out: $out
@err: ESCALATE(human)
@steps:
  1. SETTLE(vendor, 10.00, invoice) → $out
  2. LOG($out)
"#;

/// Rail that permits everything but records the last `CommandCall` it was
/// asked to settle, so a test can inspect exactly what reached it, and always
/// returns a fixed receipt.
struct CapturingSettlementRail {
    seen: Arc<Mutex<Option<CommandCall>>>,
}

impl SettlementRail for CapturingSettlementRail {
    fn settle(&self, cmd: &CommandCall) -> Option<Value> {
        *self.seen.lock().unwrap() = Some(cmd.clone());
        Some(Value::Text("receipt-123".to_string()))
    }
}

#[test]
fn settlement_permits_and_settles() {
    let seen = Arc::new(Mutex::new(None));
    let rail = CapturingSettlementRail { seen: seen.clone() };
    let result = workflows::run_with_settlement(SETTLE_WF, "settle_test.nodus", None, rail)
        .expect("run_with_settlement must succeed when the effect is permitted and settled");

    assert_eq!(
        result.status,
        Status::Ok,
        "a permitted, settled SETTLE step must not degrade status; errors: {:?}",
        result.errors
    );
    assert_eq!(
        result.vars.get("out"),
        Some(&Value::Text("receipt-123".to_string())),
        "the settled receipt must bind to the pipeline target; vars: {:?}",
        result.vars
    );

    let captured = seen
        .lock()
        .unwrap()
        .clone()
        .expect("the rail must have been consulted for the gated SETTLE step");
    assert_eq!(
        captured.args,
        vec![
            "vendor".to_string(),
            "10.00".to_string(),
            "invoice".to_string()
        ],
        "the rail must see payee/amount/purpose raw and in order"
    );
}

#[test]
fn settlement_gate_receives_positional_args() {
    // The decide half reuses the LP-11 gate unchanged: context.args must
    // already carry [payee, amount, purpose] positionally (§4.2 of the spec).
    // run_with_policy defaults the act half to NoopSettlementRail, so the
    // gate permits but the rail never settles — Partial, not Ok — this test
    // is about what reaches the gate, not the settlement outcome.
    let seen = Arc::new(Mutex::new(None));
    let policy = CapturingPolicy { seen: seen.clone() };
    let result = workflows::run_with_policy(SETTLE_WF, "settle_test.nodus", None, policy)
        .expect("run_with_policy must succeed (non-halting) when the gate permits");

    assert_eq!(
        result.status,
        Status::Partial,
        "the gate permits, but the default NoopSettlementRail leaves it unaccounted; errors: {:?}",
        result.errors
    );

    let captured = seen
        .lock()
        .unwrap()
        .clone()
        .expect("the policy must have been consulted for the gated SETTLE step");
    let pairs = context_pairs(captured);
    let get = |key: &str| pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
    assert_eq!(
        get("command"),
        Some(Value::Text("SETTLE".to_string())),
        "context must name the command; pairs: {pairs:?}"
    );
    assert_eq!(
        get("args"),
        Some(Value::List(vec![
            Value::Text("vendor".to_string()),
            Value::Text("10.00".to_string()),
            Value::Text("invoice".to_string()),
        ])),
        "context.args must carry [payee, amount, purpose] positionally, verbatim; pairs: {pairs:?}"
    );
}

#[test]
fn settlement_denied_by_policy_never_settles() {
    // run_with_policy wires the default NoopSettlementRail as the act half;
    // a denied gate must short-circuit before that rail (or any rail) is
    // ever consulted — observable here as "never settled", since a denied
    // step's pipeline target stays unbound either way.
    let result = workflows::run_with_policy(SETTLE_WF, "settle_test.nodus", None, DenyAllPolicy)
        .expect("run_with_policy returns a RunResult, not a parse error, on denial");

    assert_eq!(
        result.status,
        Status::Partial,
        "a denied settlement must degrade status to Partial, not Failed; errors: {:?}",
        result.errors
    );
    assert_eq!(
        result.vars.get("out"),
        Some(&Value::Null),
        "a denied SETTLE's pipeline target must stay at its seeded default; vars: {:?}",
        result.vars
    );
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code == "NODUS:POLICY_DENIED" && e.reason.contains("settlement")),
        "denial must record a typed POLICY_DENIED error naming the settlement gate; errors: {:?}",
        result.errors
    );
    assert!(
        !result
            .errors
            .iter()
            .any(|e| e.code == "NODUS:SETTLEMENT_UNACCOUNTED"),
        "a gate denial must short-circuit before the rail is ever consulted — no \
         SETTLEMENT_UNACCOUNTED alongside POLICY_DENIED; errors: {:?}",
        result.errors
    );
    assert!(
        result.log.iter().any(|e| e.command == "ESCALATE"),
        "a Signal-free POLICY_DENIED on SETTLE must reach NL-9 @err: dispatch automatically; log: {:?}",
        result.log
    );
}

#[test]
fn settlement_unaccounted_when_rail_returns_none() {
    let result =
        workflows::run_with_settlement(SETTLE_WF, "settle_test.nodus", None, NoopSettlementRail)
            .expect("run_with_settlement must succeed (non-halting) when the rail is unwired");

    assert_eq!(
        result.status,
        Status::Partial,
        "an unaccounted settlement must degrade status to Partial, not Failed; errors: {:?}",
        result.errors
    );
    assert_eq!(
        result.vars.get("out"),
        Some(&Value::Null),
        "an unaccounted SETTLE's pipeline target must stay at its seeded default; vars: {:?}",
        result.vars
    );
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code == "NODUS:SETTLEMENT_UNACCOUNTED"),
        "an unaccounted settlement must record a typed SETTLEMENT_UNACCOUNTED error; errors: {:?}",
        result.errors
    );
    assert!(
        result.log.iter().any(|e| e.command == "ESCALATE"),
        "a Signal-free SETTLEMENT_UNACCOUNTED must reach NL-9 @err: dispatch automatically; log: {:?}",
        result.log
    );
    assert!(
        !result.log.iter().any(|e| e.command == "LOG"),
        "the main step sequence must end after dispatch — step 2 must not run; log: {:?}",
        result.log
    );
}

#[test]
fn run_with_manifest_rejects_settle_without_settlement_role() {
    // A workflow declaring SETTLE, run against the builtin host (which never
    // provides Settlement), is rejected before any step executes — the same
    // shape as run_with_manifest_rejects_unmet_capability.
    let manifest =
        CapabilityManifest::from_workflow(&nodus::parser::Parser::parse(SETTLE_WF).expect("parse"));
    let host = HostCapabilities::builtin();

    let result =
        workflows::run_with_manifest(SETTLE_WF, "settle_test.nodus", None, &manifest, &host)
            .expect("the manifest gate returns a RunResult, not a parse error");

    assert_eq!(
        result.status,
        Status::Failed,
        "an unsatisfiable Settlement manifest must fail the run"
    );
    assert!(
        result.log.is_empty(),
        "no step may execute on a rejected run; log: {:?}",
        result.log
    );
    assert!(
        result
            .errors
            .iter()
            .any(|e| { e.code == "NODUS:CAPABILITY_UNMET" && e.reason.contains("Settlement") }),
        "rejection must name the missing Settlement capability; errors: {:?}",
        result.errors
    );
}

#[test]
fn no_settle_step_is_byte_for_byte_unchanged() {
    // Guardrail: every existing run_with_* variant must keep
    // NoopSettlementRail's always-unaccounted-but-inert behaviour now that
    // Executor carries a settlement field — a workflow with no SETTLE step is
    // completely unaffected.
    let via_plain = workflows::run(MANIFEST_WF, "manifest_test.nodus", None).expect("plain run");
    let via_explicit_noop = workflows::run_with_settlement(
        MANIFEST_WF,
        "manifest_test.nodus",
        None,
        NoopSettlementRail,
    )
    .expect("run_with_settlement with the explicit noop");

    assert_eq!(via_plain.status, via_explicit_noop.status);
    assert_eq!(via_plain.vars, via_explicit_noop.vars);
    assert_eq!(
        via_plain.status,
        Status::Ok,
        "must remain unaffected by LP-17's addition"
    );
}

// ─── DG-9/DG-10 memoizable & promotable approval ──────────────────────────────

/// Always resolves from a (fake) durable prior decision — never re-prompts.
struct DialogRemembers;

impl DialogProvider for DialogRemembers {
    fn ask(&self, _prompt: &str, _modifiers: &[(String, String)]) -> DialogOutcome {
        DialogOutcome::Remembered(Value::Text("remembered-answer".to_string()))
    }

    fn confirm(&self, _content: &str, _modifiers: &[(String, String)]) -> DialogOutcome {
        DialogOutcome::Remembered(Value::Text("remembered-answer".to_string()))
    }
}

/// Records every event it receives, so a test can inspect an event's
/// `annotations` after the run completes.
struct RecordingAudit {
    events: Arc<Mutex<Vec<ExecutionEvent>>>,
}

impl AuditProvider for RecordingAudit {
    fn record_event(&self, event: ExecutionEvent) {
        self.events.lock().unwrap().push(event);
    }

    fn run_complete(&self, _manifest: RunManifest) {}
}

fn step_end_provenance(events: &[ExecutionEvent]) -> Vec<Option<DialogProvenance>> {
    events
        .iter()
        .filter_map(|e| match e {
            ExecutionEvent::StepEnd { annotations, .. } => Some(annotations.dialog_provenance),
            _ => None,
        })
        .collect()
}

#[test]
fn remembered_binds_identically_to_answer() {
    let result =
        workflows::run_with_dialog(DEFERRED_WF, "deferred_test.nodus", None, DialogRemembers)
            .expect("run_with_dialog must succeed when the dialog resolves from memory");

    assert_eq!(
        result.status,
        Status::Ok,
        "a remembered resolution must not degrade status; errors: {:?}",
        result.errors
    );
    assert_eq!(
        result.vars.get("out"),
        Some(&Value::Text("remembered-answer".to_string())),
        "Remembered must bind to the pipeline target exactly like Answer; vars: {:?}",
        result.vars
    );
}

#[test]
fn step_end_carries_dialog_provenance_answered_and_remembered() {
    let events = Arc::new(Mutex::new(Vec::new()));
    workflows::run_with_dialog_and_audit(
        DEFERRED_WF,
        "deferred_test.nodus",
        None,
        DefaultDialogProvider,
        RecordingAudit {
            events: events.clone(),
        },
        "run-answered",
        "2026-01-01T00:00:00Z",
    )
    .expect("run_with_dialog_and_audit must succeed with the default (+default) resolver");
    let answered_provenance = step_end_provenance(&events.lock().unwrap());
    assert_eq!(
        answered_provenance,
        vec![Some(DialogProvenance::Answered)],
        "a +default-resolved ASK's StepEnd must carry Answered provenance"
    );

    let events = Arc::new(Mutex::new(Vec::new()));
    workflows::run_with_dialog_and_audit(
        DEFERRED_WF,
        "deferred_test.nodus",
        None,
        DialogRemembers,
        RecordingAudit {
            events: events.clone(),
        },
        "run-remembered",
        "2026-01-01T00:00:00Z",
    )
    .expect("run_with_dialog_and_audit must succeed with a memoizing provider");
    let remembered_provenance = step_end_provenance(&events.lock().unwrap());
    assert_eq!(
        remembered_provenance,
        vec![Some(DialogProvenance::Remembered)],
        "a memoized ASK's StepEnd must carry Remembered provenance"
    );
}

#[test]
fn pause_timeout_rejected_carry_no_dialog_provenance() {
    let events = Arc::new(Mutex::new(Vec::new()));
    workflows::run_with_dialog_and_audit(
        DEFERRED_WF,
        "deferred_test.nodus",
        None,
        DialogRejects,
        RecordingAudit {
            events: events.clone(),
        },
        "run-rejected",
        "2026-01-01T00:00:00Z",
    )
    .expect("run_with_dialog_and_audit must succeed (non-halting) on rejection");
    // Two StepEnd events: the rejected ASK itself, then the dispatched
    // @err: ESCALATE handler (DEFERRED_WF declares one, and DIALOG_REJECTED
    // is Signal-free — NL-9 dispatch fires automatically). Neither carries
    // dialog provenance: the ASK never resolved, and ESCALATE isn't a
    // dialog step at all.
    let provenance = step_end_provenance(&events.lock().unwrap());
    assert_eq!(
        provenance,
        vec![None, None],
        "neither the rejected ASK nor the dispatched ESCALATE handler carries dialog provenance"
    );
}

#[test]
fn default_dialog_provider_never_returns_remembered() {
    // Regression: every existing run_with_* variant keeps DefaultDialogProvider's
    // Answer-or-Pause behaviour byte-for-byte — it never memoizes anything.
    let via_plain = workflows::run_with_dialog(
        DEFERRED_WF,
        "deferred_test.nodus",
        None,
        DefaultDialogProvider,
    )
    .expect("run_with_dialog with the default provider");
    assert_eq!(
        via_plain.vars.get("out"),
        Some(&Value::Text("yes".to_string())),
        "DefaultDialogProvider must still resolve via +default, never Remembered"
    );
}
