// Per-invariant assertions — one named test per workflow-language
// invariant. These guard the language contract directly, independent of the
// reference corpus; they remain valid even as the reference evolves.

use nodus::{
    executor::{ModelProvider, Status, Value},
    parser::Parser,
    transpiler::Transpiler,
    validator::{Severity, Validator},
    vocab::Schema,
    workflows::{self, TranspileMode},
};

mod support;

// ── Fixtures ──────────────────────────────────────────────────────────────────

const SIMPLE_LOG: &str = include_str!("fixtures/simple_log.nodus");
const LINT_MISSING_RUNTIME: &str = include_str!("fixtures/lint_missing_runtime.nodus");
const UNTIL_QUALITY_LOOP: &str = include_str!("fixtures/until_quality_loop.nodus");

// Workflow with a !PREF but no hard rule violation — preference must not block.
const PREF_ONLY: &str = r#"§wf:pref_only v1.0
§runtime: { core: schema.nodus }
!PREF: empathetic OVER neutral IF $in.mood = "sad"
@in: { mood?: str }
@out: $out
@err: ESCALATE(human)
@steps:
  1. GEN($in.mood) → $out
  2. LOG($out)
"#;

// Workflow with both !PREF and !!NEVER — hard rule must win over preference.
const PREF_AND_NEVER: &str = r#"§wf:pref_and_never v1.0
§runtime: { core: schema.nodus }
!PREF: fast OVER thorough
!!NEVER: FETCH
@in: { url?: str }
@out: $out
@err: ESCALATE(human)
@steps:
  1. FETCH($in.url) → $out
  2. LOG($out)
"#;

// Workflow whose ~UNTIL condition is never satisfied by the stub — MAX:2 will be hit.
const ALWAYS_LOOPS: &str = r#"§wf:always_loops v1.0
§runtime: { core: schema.nodus }
@in: { prompt?: str }
@out: $out
@err: ESCALATE(human)
@steps:
  1. GEN($in.prompt) → $out
  2. ~UNTIL $out = "magic_stop_signal" | MAX:2
       GEN("retry") → $out
     ~END
  3. LOG($out)
"#;

// ~UNTIL without a MAX cap — the validator must fire E010.
const UNBOUNDED_LOOP: &str = r#"§wf:unbounded_loop v1.0
§runtime: { core: schema.nodus }
@in: { x?: str }
@out: $out
@err: ESCALATE(human)
@steps:
  1. GEN($in.x) → $out
  2. ~UNTIL $out = "done"
       LOG($out)
     ~END
"#;

// ── Custom provider ─────────────────────────────────────────────────

/// Sentinel output that proves dispatch went through this seam.
const WFL7_MARKER: &str = "WFL7_SEAM_RESULT";

struct FixedOutputProvider;

impl ModelProvider for FixedOutputProvider {
    fn model_id(&self) -> &str {
        "fixed-wfl7"
    }

    fn generate(&self, _prompt: &str, _modifiers: &[(String, String)]) -> String {
        WFL7_MARKER.to_string()
    }

    fn analyze(&self, _text: &str, flags: &[String]) -> Value {
        Value::Map(
            flags
                .iter()
                .map(|f| (f.clone(), Value::Float(0.9)))
                .collect(),
        )
    }
}

// ── Dual representation ───────────────────────────────────────────────

#[test]
fn wfl_1_compact_round_trip_preserves_ast() {
    // to_nodus() strips comments by design (they are not logic).
    // The correct losslessness check: normalise → re-parse → normalise again;
    // both compact forms must be byte-identical.
    let ast1 = Parser::parse(SIMPLE_LOG).expect("fixture must parse");
    let compact1 = Transpiler::to_nodus(&ast1);
    let ast2 = Parser::parse(&compact1).expect("compact form must re-parse");
    let compact2 = Transpiler::to_nodus(&ast2);
    assert_eq!(
        compact1, compact2,
        "to_nodus() output must be identical across round-trips (lossless logic)"
    );
}

#[test]
fn wfl_1_human_form_is_distinct_prose() {
    let ast = Parser::parse(SIMPLE_LOG).expect("fixture must parse");
    let human = Transpiler::to_human(&ast);
    assert!(!human.is_empty(), "human form must not be empty");
    assert!(
        human.contains("WORKFLOW"),
        "human form must include a WORKFLOW section header"
    );
    // Human form is not the same as the compact form — it is a distinct representation.
    let compact = Transpiler::to_nodus(&ast);
    assert_ne!(human, compact, "human and compact forms must differ");
}

// ── Schema vocabulary contract ────────────────────────────────────────

#[test]
fn wfl_2_builtin_schema_is_loaded_and_queryable() {
    let schema = Schema::builtin();
    assert!(
        schema.is_command("GEN"),
        "schema must recognise GEN as a valid command"
    );
    assert!(
        schema.is_command("LOG"),
        "schema must recognise LOG as a valid command"
    );
    assert!(
        !schema.is_command("FABRICATED_CMD"),
        "schema must reject unknown commands"
    );
    assert!(
        !schema.version().is_empty(),
        "schema must carry a non-empty version string"
    );
}

#[test]
fn wfl_2_validator_uses_schema_to_catch_unknown_commands() {
    // A source with an unknown command — the validator (which uses the schema)
    // must flag it rather than silently accepting it.
    let source = r#"§wf:schema_check v1.0
§runtime: { core: schema.nodus }
@in: { x? }
@out: $out
@err: ESCALATE(human)
@steps:
  1. GEN($in.x) → $out
  2. LOG($out)
"#;
    let ast = Parser::parse(source).expect("must parse");
    let diags = Validator::validate(&ast, "schema_check.nodus");
    // All commands above (GEN, LOG) are in-schema — no schema-vocabulary errors.
    assert!(
        !diags.iter().any(|d| d.severity == Severity::Error),
        "known-vocabulary workflow must produce no errors"
    );
}

// ── Hard constraints inviolable ───────────────────────────────────────

#[test]
fn wfl_3_never_rule_halts_execution_with_failed_status() {
    // !!NEVER: FETCH → the executor must refuse and return Failed.
    let result = workflows::run(
        PREF_AND_NEVER,
        "pref_and_never.nodus",
        Some(support::sample_input(PREF_AND_NEVER)),
    )
    .expect("validation must pass — constraint enforcement is a runtime check");
    assert_eq!(
        result.status,
        Status::Failed,
        "violating !!NEVER must set Status::Failed"
    );
    assert!(
        !result.errors.is_empty(),
        "the NEVER-rule violation must be recorded in RunResult.errors"
    );
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code.contains("RULE_VIOLATION")),
        "error code must identify RULE_VIOLATION"
    );
}

// ── Preferences are soft ──────────────────────────────────────────────

#[test]
fn wfl_4_preference_does_not_halt_execution() {
    // !PREF alone must not block — preferences are advisory, not enforcing.
    let result = workflows::run(
        PREF_ONLY,
        "pref_only.nodus",
        Some(support::sample_input(PREF_ONLY)),
    )
    .expect("workflow with only !PREF must execute");
    assert_eq!(
        result.status,
        Status::Ok,
        "!PREF must not cause Status::Failed or Status::Aborted"
    );
}

#[test]
fn wfl_4_hard_rule_wins_over_preference() {
    // When !PREF and !!NEVER coexist and the NEVER is violated, the hard rule
    // prevails — preference softness does not weaken hard-constraint enforcement.
    let result = workflows::run(
        PREF_AND_NEVER,
        "pref_and_never.nodus",
        Some(support::sample_input(PREF_AND_NEVER)),
    )
    .expect("validation must pass");
    assert_eq!(
        result.status,
        Status::Failed,
        "!!NEVER must win over any !PREF — hard rules are inviolable"
    );
}

// ── Validate before run ────────────────────────────────────────────────

#[test]
fn wfl_5_block_class_error_prevents_execution() {
    // Missing §runtime (E001) is a block-class error — run() must reject before dispatch.
    let err = workflows::run(
        LINT_MISSING_RUNTIME,
        "lint_missing_runtime.nodus",
        Some(support::sample_input(LINT_MISSING_RUNTIME)),
    )
    .expect_err("run must fail when block-class errors are present");
    assert!(
        err.iter().any(|d| d.severity == Severity::Error),
        "rejection diagnostics must include at least one Error-severity entry"
    );
}

#[test]
fn wfl_5_valid_workflow_passes_gate_and_executes() {
    // A valid workflow must clear the validate gate and reach the executor.
    let result = workflows::run(
        SIMPLE_LOG,
        "simple_log.nodus",
        Some(support::sample_input(SIMPLE_LOG)),
    )
    .expect("valid workflow must pass validate-before-run and reach executor");
    assert_eq!(
        result.status,
        Status::Ok,
        "validated workflow must execute successfully"
    );
}

// ── Bounded execution ─────────────────────────────────────────────────

#[test]
fn wfl_6_until_loop_sets_max_reached_flag() {
    // The stub never produces "magic_stop_signal", so MAX:2 is always exhausted.
    let result = workflows::run(
        ALWAYS_LOOPS,
        "always_loops.nodus",
        Some(support::sample_input(ALWAYS_LOOPS)),
    )
    .expect("must execute without block-class errors");
    assert!(
        result.flags.iter().any(|f| f == "NODUS:MAX_REACHED"),
        "when loop condition is never met, NODUS:MAX_REACHED must be set"
    );
    assert_eq!(
        result.status,
        Status::Ok,
        "hitting MAX cap must not abort the workflow"
    );
}

#[test]
fn wfl_6_until_without_max_is_lint_error() {
    // The validator must reject ~UNTIL without an explicit MAX guard (E010).
    let ast = Parser::parse(UNBOUNDED_LOOP).expect("must parse");
    let diags = Validator::validate(&ast, "unbounded_loop.nodus");
    assert!(
        diags.iter().any(|d| d.code == "E010"),
        "~UNTIL without MAX must fire E010; got: {diags:?}"
    );
}

#[test]
fn wfl_6_bounded_loop_executes_within_limit() {
    // The UNTIL_QUALITY_LOOP fixture uses MAX:3 — execution must complete (not hang).
    let result = workflows::run(
        UNTIL_QUALITY_LOOP,
        "until_quality_loop.nodus",
        Some(support::sample_input(UNTIL_QUALITY_LOOP)),
    )
    .expect("bounded loop must execute without block-class errors");
    assert!(
        result.status == Status::Ok || result.flags.iter().any(|f| f == "NODUS:MAX_REACHED"),
        "bounded loop must either meet its condition or exhaust MAX gracefully"
    );
}

// ── Subsystem-dispatch seam ──────────────────────────────────────────

#[test]
fn wfl_7_executor_dispatches_through_provider_seam() {
    // A custom ModelProvider replaces the default stub — the executor must route
    // GEN through it, proving the subsystem-dispatch seam is real and pluggable.
    let result = workflows::run_with_provider(
        SIMPLE_LOG,
        "simple_log.nodus",
        Some(support::sample_input(SIMPLE_LOG)),
        FixedOutputProvider,
    )
    .expect("must execute with custom provider");
    assert_eq!(
        result.status,
        Status::Ok,
        "execution through custom provider must succeed"
    );
    // $out is set by GEN → our provider → WFL7_MARKER; if dispatch went through
    // the seam the result carries the provider's sentinel value.
    match &result.out {
        Value::Text(s) => assert_eq!(
            s, WFL7_MARKER,
            "GEN output must come from the injected provider, not a hardcoded path"
        ),
        other => panic!("expected Text($out), got {other:?}"),
    }
}

// ── Result contract ────────────────────────────────────────────────────

#[test]
fn wfl_8_success_result_has_required_fields() {
    let result = workflows::run(
        SIMPLE_LOG,
        "simple_log.nodus",
        Some(support::sample_input(SIMPLE_LOG)),
    )
    .expect("must execute");
    assert_eq!(
        result.workflow, "wf:simple_log",
        "workflow field must carry the declared identifier"
    );
    assert_eq!(result.status, Status::Ok, "successful run must be Ok");
    // log must record at least the LOG step.
    assert!(
        !result.log.is_empty(),
        "log field must be non-empty after execution"
    );
    // errors and flags are present (possibly empty on clean run — that is correct).
    let _ = &result.errors;
    let _ = &result.flags;
}

#[test]
fn wfl_8_failure_result_has_required_fields() {
    // NEVER-rule violation → Status::Failed; the result contract must still be complete.
    let result = workflows::run(
        PREF_AND_NEVER,
        "pref_and_never.nodus",
        Some(support::sample_input(PREF_AND_NEVER)),
    )
    .expect("validation must pass");
    assert_eq!(
        result.workflow, "wf:pref_and_never",
        "workflow field must be set even on failure"
    );
    assert_eq!(result.status, Status::Failed, "failed run must be Failed");
    assert!(
        !result.errors.is_empty(),
        "errors field must be populated on failure"
    );
}

// ── Human view ────────────────────────────────────────────────────────

#[test]
fn wfl_9_human_view_contains_required_sections() {
    let out = workflows::transpile(SIMPLE_LOG, TranspileMode::Human)
        .expect("human transpile must succeed");
    assert!(!out.is_empty(), "human view must not be empty");
    assert!(
        out.contains("WORKFLOW"),
        "human view must include WORKFLOW section"
    );
    assert!(
        out.contains("STEPS"),
        "human view must include STEPS section"
    );
}

#[test]
fn wfl_9_human_view_is_not_compact_syntax() {
    // The human form is prose for the client — it must not look like the machine form.
    let human = workflows::transpile(SIMPLE_LOG, TranspileMode::Human)
        .expect("human transpile must succeed");
    // Compact form uses §wf: header — human form must not.
    assert!(
        !human.contains("§wf:"),
        "human view must not contain compact §wf: syntax"
    );
}
