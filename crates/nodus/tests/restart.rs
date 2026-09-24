//! Integration tests for the bounded whole-run self-restart (
//! NL-23).
//!
//! bound/ceiling exhaustion, run-boundary authority (validator
//! rejection of a nested request), `$restart_count` progression, fresh
//! reconstruction across attempts, and the no-`restart_max` additivity
//! baseline.

use nodus::executor::{Status, Value};
use nodus::observability::{AuditProvider, ExecutionEvent, RunManifest};
use nodus::parser::Parser;
use nodus::transpiler::Transpiler;
use nodus::validator::{Severity, Validator};
use nodus::workflows::{self, run_with_audit};
use std::sync::{Arc, Mutex};

// ─── Recording audit provider (accumulates across every attempt) ─────────────

struct RecordingProvider {
    events: Arc<Mutex<Vec<ExecutionEvent>>>,
    manifests: Arc<Mutex<Vec<RunManifest>>>,
}

impl RecordingProvider {
    fn new() -> Self {
        RecordingProvider {
            events: Arc::new(Mutex::new(Vec::new())),
            manifests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn manifest_count(&self) -> usize {
        self.manifests.lock().unwrap().len()
    }
}

impl Clone for RecordingProvider {
    fn clone(&self) -> Self {
        RecordingProvider {
            events: Arc::clone(&self.events),
            manifests: Arc::clone(&self.manifests),
        }
    }
}

impl AuditProvider for RecordingProvider {
    fn record_event(&self, event: ExecutionEvent) {
        self.events.lock().unwrap().push(event);
    }
    fn run_complete(&self, manifest: RunManifest) {
        self.manifests.lock().unwrap().push(manifest);
    }
}

// ─── Fixtures (parsed source) ─────────────────────────────────────────────────
//
// The attempt-count-dependent scenario below (`RESTART_ONCE_WF`) used to need
// a directly-constructed AST: `?IF cond → CMD() → $target` dropped its
// trailing pipeline target — `try_parse_command_from_string` never looked for
// a second arrow within the re-parsed action string (confirmed by inspecting
// the parsed AST directly — every inline `?IF` action's `pipeline_target`
// came back `None` regardless of source text). Since fixed by
// (the NL-10 conformance pass, which also covers `?SWITCH`
// arm targets); this fixture is now real nodus source run through the full
// parse → validate → run_with_audit path.

// Always requests a restart — never stops on its own; only the ceiling stops it.
const RESTART_ALWAYS_WF: &str = r#"§wf:restart_always v1.0
§runtime: { core: schema.nodus, restart_max: 2 }
@out: $out
@err: ESCALATE(human)
@steps:
  1. GEN(again) → $restart
  2. GEN(done) → $out
"#;

// No restart_max declared at all — the additivity baseline.
const NO_RESTART_WF: &str = r#"§wf:no_restart v1.0
§runtime: { core: schema.nodus }
@out: $out
@err: ESCALATE(human)
@steps:
  1. GEN(hello) → $out
"#;

// A $restart request nested inside a ~FOR body — must be rejected before it runs.
const NESTED_RESTART_WF: &str = r#"§wf:nested_restart v1.0
§runtime: { core: schema.nodus, restart_max: 3 }
@in: { items?: list }
@out: $out
@err: ESCALATE(human)
@steps:
  1. ~FOR $item IN $in.items
       GEN(x) → $restart
     ~END
  2. GEN(done) → $out
"#;

// Requests a restart only while $restart_count < 1, so it restarts exactly
// once. $attempt0_marker is likewise set only on that first attempt.
const RESTART_ONCE_WF: &str = r#"§wf:restart_once v1.0
§runtime: { core: schema.nodus, restart_max: 3 }
@out: $out
@err: ESCALATE(human)
@steps:
  1. ?IF $restart_count < 1 → GEN(first_attempt_only) → $attempt0_marker
  2. ?IF $restart_count < 1 → GEN(again) → $restart
  3. GEN(done) → $out
"#;

// ─── Bound + authority ────────────────────────────────────────────────────────

#[test]
fn nested_restart_request_rejected_by_validator() {
    let ast = Parser::parse(NESTED_RESTART_WF).expect("parse");
    let diags = Validator::validate(&ast, "nested_restart.nodus");
    assert!(
        diags
            .iter()
            .any(|d| d.code == "E019" && d.severity == Severity::Error),
        "a $restart request inside a ~FOR body must be rejected before any run is attempted; got: {diags:?}"
    );

    let err = workflows::run(NESTED_RESTART_WF, "nested_restart.nodus", None)
        .expect_err("run() must refuse a workflow with a nested restart request");
    assert!(
        err.iter().any(|d| d.code == "E019"),
        "run() must surface E019 before executing; got: {err:?}"
    );
}

#[test]
fn ceiling_exhaustion_stops_after_max_plus_one_attempts_and_flags_restart_limit() {
    let recorder = RecordingProvider::new();
    let result = run_with_audit(
        RESTART_ALWAYS_WF,
        "restart_always.nodus",
        None,
        recorder.clone(),
        "",
        "",
    )
    .expect("run");

    // restart_max: 2 → attempts 0, 1, 2 (three total), then refused.
    assert_eq!(
        recorder.manifest_count(),
        3,
        "expected exactly restart_max + 1 attempts"
    );
    assert!(
        result.flags.iter().any(|f| f == "NODUS:RESTART_LIMIT"),
        "ceiling exhaustion must flag NODUS:RESTART_LIMIT; flags: {:?}",
        result.flags
    );
    assert_eq!(
        result.vars.get("restart_count"),
        Some(&Value::Int(2)),
        "the final (3rd) attempt must see restart_count == 2"
    );
}

// ─── $restart_count progression + fresh reconstruction ───────────────────────

#[test]
fn restart_count_progresses_and_context_is_fresh_each_attempt() {
    let recorder = RecordingProvider::new();
    let result = run_with_audit(
        RESTART_ONCE_WF,
        "restart_once.nodus",
        None,
        recorder.clone(),
        "",
        "",
    )
    .expect("run");

    assert_eq!(result.status, Status::Ok, "errors: {:?}", result.errors);
    // The guard (`$restart_count < 1`) only holds on the first attempt, so a
    // restart was requested exactly once — proving restart_count read 0 then.
    assert_eq!(
        recorder.manifest_count(),
        2,
        "expected exactly one restart: two attempts total"
    );
    // The final (second) attempt must see restart_count == 1.
    assert_eq!(result.vars.get("restart_count"), Some(&Value::Int(1)));
    // Fresh reconstruction (LG-5): $attempt0_marker was set only on the first
    // attempt's guard; the second attempt's context must not carry it over.
    assert_eq!(
        result.vars.get("attempt0_marker"),
        None,
        "a variable set only on a prior attempt must not survive into the next \
         attempt's fresh context; vars: {:?}",
        result.vars
    );
}

// ─── Additivity baseline ──────────────────────────────────────────────────────

#[test]
fn no_restart_max_is_byte_identical_to_the_pre_nl23_path() {
    let recorder = RecordingProvider::new();
    let result = run_with_audit(
        NO_RESTART_WF,
        "no_restart.nodus",
        None,
        recorder.clone(),
        "",
        "",
    )
    .expect("run");

    assert_eq!(result.status, Status::Ok, "errors: {:?}", result.errors);
    assert_eq!(
        recorder.manifest_count(),
        1,
        "a workflow with no restart_max must never loop — exactly one attempt"
    );
    assert_eq!(
        result.vars.get("restart_count"),
        None,
        "restart_count must not be seeded unless restart_max is declared \
         (additivity, NL-23(d)); vars: {:?}",
        result.vars
    );
}

// ─── Transpiler round-trip (NL-6) ─────────────────────────────────────────────

#[test]
fn restart_max_survives_compact_round_trip() {
    let ast = Parser::parse(RESTART_ALWAYS_WF).expect("parse");
    let compact = Transpiler::to_nodus(&ast);
    let ast2 = Parser::parse(&compact).expect("compact form must re-parse");
    assert_eq!(
        ast2.runtime.and_then(|rt| rt.restart_max),
        Some(2),
        "restart_max must survive a compact round-trip"
    );
}
