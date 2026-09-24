// Golden parity corpus: verifies semantic equivalence between our Rust runtime
// and the reference workflow-language implementation for the same input corpus.
// Each test loads a .nodus fixture and checks validation verdicts, execution
// results, or transpilation shape against expected golden values.
//
// Conventions:
//  - `codes()` extracts diagnostic codes from a `Vec<Diagnostic>` for easy assertion.
//  - Execution tests use the built-in stub provider (real model calls are not made).
//  - Transpilation tests check structure and section presence, not exact byte output.

use nodus::{
    parser::Parser,
    transpiler::Transpiler,
    validator::{Diagnostic, Severity, Validator},
    workflows::{self, TranspileMode},
};

// ── Fixtures (embedded at compile time) ──────────────────────────────────────

const SIMPLE_LOG: &str = include_str!("fixtures/simple_log.nodus");
const TICKET_TRIAGE: &str = include_str!("fixtures/ticket_triage.nodus");
const LINT_MISSING_RUNTIME: &str = include_str!("fixtures/lint_missing_runtime.nodus");
const LINT_PUBLISH_BEFORE_VALIDATE: &str =
    include_str!("fixtures/lint_publish_before_validate.nodus");
const LINT_NAME_MISMATCH: &str = include_str!("fixtures/lint_name_mismatch.nodus");
const UNTIL_QUALITY_LOOP: &str = include_str!("fixtures/until_quality_loop.nodus");
const CONDITIONAL: &str = include_str!("fixtures/conditional.nodus");
const FOR_LOOP: &str = include_str!("fixtures/for_loop.nodus");
const PARALLEL_JOIN: &str = include_str!("fixtures/parallel_join.nodus");
const MACRO_EXPAND: &str = include_str!("fixtures/macro_expand.nodus");
const MAP_TRANSFORM: &str = include_str!("fixtures/map_transform.nodus");
const SWITCH_DISPATCH: &str = include_str!("fixtures/switch_dispatch.nodus");
const RETRY_BOUNDED: &str = include_str!("fixtures/retry_bounded.nodus");
const HALT_PAUSE: &str = include_str!("fixtures/halt_pause.nodus");

/// The normative fixture corpus swept by the NL-6 AST-equality harness
/// — every well-formed fixture above, excluding the three
/// deliberately-invalid `lint_*` fixtures (they exist to trigger a specific
/// validator diagnostic, not to represent round-trippable content).
const NORMATIVE_CORPUS: &[(&str, &str)] = &[
    ("simple_log.nodus", SIMPLE_LOG),
    ("ticket_triage.nodus", TICKET_TRIAGE),
    ("until_quality_loop.nodus", UNTIL_QUALITY_LOOP),
    ("conditional.nodus", CONDITIONAL),
    ("for_loop.nodus", FOR_LOOP),
    ("parallel_join.nodus", PARALLEL_JOIN),
    ("macro_expand.nodus", MACRO_EXPAND),
    ("map_transform.nodus", MAP_TRANSFORM),
    ("switch_dispatch.nodus", SWITCH_DISPATCH),
    ("retry_bounded.nodus", RETRY_BOUNDED),
    ("halt_pause.nodus", HALT_PAUSE),
];

// ── Helpers ───────────────────────────────────────────────────────────────────

fn codes(diags: &[Diagnostic]) -> Vec<&str> {
    diags.iter().map(|d| d.code.as_str()).collect()
}

fn parse_and_validate(source: &str, filename: &str) -> Vec<Diagnostic> {
    let ast = Parser::parse(source).expect("fixture must parse");
    Validator::validate(&ast, filename)
}

// ── Validation parity ─────────────────────────────────────────────────────────

mod support;

mod validation {
    use super::*;

    #[test]
    fn simple_log_no_block_errors() {
        let diags = parse_and_validate(SIMPLE_LOG, "simple_log.nodus");
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "expected no block-class errors for simple_log; got: {errors:?}"
        );
    }

    #[test]
    fn ticket_triage_no_block_errors() {
        let diags = parse_and_validate(TICKET_TRIAGE, "ticket_triage.nodus");
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "expected no block-class errors for ticket_triage; got: {errors:?}"
        );
    }

    #[test]
    fn missing_runtime_yields_e001() {
        let diags = parse_and_validate(LINT_MISSING_RUNTIME, "lint_missing_runtime.nodus");
        assert!(
            codes(&diags).contains(&"E001"),
            "expected E001 for missing §runtime; got: {diags:?}"
        );
    }

    #[test]
    fn e001_absent_when_runtime_present() {
        let diags = parse_and_validate(SIMPLE_LOG, "simple_log.nodus");
        assert!(
            !codes(&diags).contains(&"E001"),
            "unexpected E001 when §runtime is present"
        );
    }

    #[test]
    fn publish_before_validate_yields_e005() {
        let diags = parse_and_validate(
            LINT_PUBLISH_BEFORE_VALIDATE,
            "lint_publish_before_validate.nodus",
        );
        assert!(
            codes(&diags).contains(&"E005"),
            "expected E005 for PUBLISH before VALIDATE; got: {diags:?}"
        );
    }

    #[test]
    fn ticket_triage_validate_before_publish_no_e005() {
        let diags = parse_and_validate(TICKET_TRIAGE, "ticket_triage.nodus");
        assert!(
            !codes(&diags).contains(&"E005"),
            "unexpected E005: ticket_triage has VALIDATE before PUBLISH"
        );
    }

    #[test]
    fn name_mismatch_yields_e012() {
        // The fixture declares §wf:lint_name_mismatch but the filename says otherwise.
        let diags = parse_and_validate(LINT_NAME_MISMATCH, "wrong_filename.nodus");
        assert!(
            codes(&diags).contains(&"E012"),
            "expected E012 for name/filename mismatch; got: {diags:?}"
        );
    }

    #[test]
    fn correct_filename_no_e012() {
        let diags = parse_and_validate(LINT_NAME_MISMATCH, "lint_name_mismatch.nodus");
        assert!(
            !codes(&diags).contains(&"E012"),
            "unexpected E012 when workflow name matches filename"
        );
    }

    #[test]
    fn no_err_handler_yields_w001() {
        // lint_missing_runtime.nodus has no @err block.
        let diags = parse_and_validate(LINT_MISSING_RUNTIME, "lint_missing_runtime.nodus");
        assert!(
            codes(&diags).contains(&"W001"),
            "expected W001 for missing @err handler"
        );
    }

    #[test]
    fn w001_absent_when_err_handler_present() {
        let diags = parse_and_validate(SIMPLE_LOG, "simple_log.nodus");
        assert!(
            !codes(&diags).contains(&"W001"),
            "unexpected W001 when @err: ESCALATE(human) is declared"
        );
    }

    #[test]
    fn no_test_blocks_yields_w002() {
        let diags = parse_and_validate(SIMPLE_LOG, "simple_log.nodus");
        assert!(
            codes(&diags).contains(&"W002"),
            "expected W002 for workflow with no @test blocks"
        );
    }

    #[test]
    fn w002_absent_when_tests_present() {
        let diags = parse_and_validate(TICKET_TRIAGE, "ticket_triage.nodus");
        assert!(
            !codes(&diags).contains(&"W002"),
            "unexpected W002 when @test blocks exist"
        );
    }

    #[test]
    fn conditional_no_block_errors() {
        let diags = parse_and_validate(CONDITIONAL, "conditional.nodus");
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "expected no block-class errors for conditional; got: {errors:?}"
        );
    }

    #[test]
    fn for_loop_no_block_errors() {
        let diags = parse_and_validate(FOR_LOOP, "for_loop.nodus");
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "expected no block-class errors for for_loop; got: {errors:?}"
        );
    }

    #[test]
    fn parallel_join_no_block_errors() {
        let diags = parse_and_validate(PARALLEL_JOIN, "parallel_join.nodus");
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "expected no block-class errors for parallel_join; got: {errors:?}"
        );
    }

    #[test]
    fn map_transform_no_block_errors() {
        let diags = parse_and_validate(MAP_TRANSFORM, "map_transform.nodus");
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "expected no block-class errors for map_transform (E004 must not flag ~MAP's implicit $it); got: {errors:?}"
        );
    }

    #[test]
    fn switch_dispatch_no_block_errors() {
        let diags = parse_and_validate(SWITCH_DISPATCH, "switch_dispatch.nodus");
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "expected no block-class errors for switch_dispatch; got: {errors:?}"
        );
    }

    #[test]
    fn retry_bounded_no_block_errors() {
        let diags = parse_and_validate(RETRY_BOUNDED, "retry_bounded.nodus");
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "expected no block-class errors for retry_bounded; got: {errors:?}"
        );
    }

    #[test]
    fn halt_pause_no_block_errors() {
        let diags = parse_and_validate(HALT_PAUSE, "halt_pause.nodus");
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "expected no block-class errors for halt_pause; got: {errors:?}"
        );
    }

    #[test]
    fn macro_expand_no_block_errors() {
        let diags = parse_and_validate(MACRO_EXPAND, "macro_expand.nodus");
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "expected no block-class errors for macro_expand; got: {errors:?}"
        );
    }

    #[test]
    fn severity_ordering_error_lt_warning_lt_info() {
        // The partial order must match the reference: Error < Warning < Info
        // (lower = more severe).
        assert!(
            Severity::Error < Severity::Warning,
            "Error must order strictly before Warning"
        );
        assert!(
            Severity::Warning < Severity::Info,
            "Warning must order strictly before Info"
        );
    }
}

// ── Transpilation parity ──────────────────────────────────────────────────────

mod transpilation {
    use super::*;

    fn to_compact(source: &str) -> String {
        let ast = Parser::parse(source).expect("fixture must parse");
        Transpiler::to_nodus(&ast)
    }

    fn to_human(source: &str) -> String {
        let ast = Parser::parse(source).expect("fixture must parse");
        Transpiler::to_human(&ast)
    }

    #[test]
    fn compact_round_trip_preserves_name() {
        let compact = to_compact(SIMPLE_LOG);
        let ast2 = Parser::parse(&compact).expect("compact form must re-parse");
        assert_eq!(
            ast2.header.as_ref().unwrap().name,
            "simple_log",
            "workflow name must survive compact round-trip"
        );
    }

    #[test]
    fn compact_includes_runtime_block() {
        let compact = to_compact(SIMPLE_LOG);
        assert!(
            compact.contains("§runtime:"),
            "compact output must include §runtime: block"
        );
    }

    #[test]
    fn compact_includes_steps_section() {
        let compact = to_compact(SIMPLE_LOG);
        assert!(
            compact.contains("@steps:"),
            "compact output must include @steps: section"
        );
    }

    #[test]
    fn human_includes_workflow_label() {
        let human = to_human(SIMPLE_LOG);
        assert!(
            human.contains("WORKFLOW"),
            "human form must include WORKFLOW section header"
        );
    }

    #[test]
    fn human_includes_steps_label() {
        let human = to_human(SIMPLE_LOG);
        assert!(
            human.contains("STEPS"),
            "human form must include STEPS section header"
        );
    }

    #[test]
    fn compact_ticket_triage_round_trip() {
        let compact = to_compact(TICKET_TRIAGE);
        let ast2 = Parser::parse(&compact).expect("compact form must re-parse");
        assert_eq!(
            ast2.header.as_ref().unwrap().name,
            "ticket_triage",
            "ticket_triage name must survive compact round-trip"
        );
    }

    #[test]
    fn compact_map_transform_round_trip() {
        let compact = to_compact(MAP_TRANSFORM);
        let ast2 = Parser::parse(&compact).expect("compact form must re-parse");
        assert_eq!(
            ast2.header.as_ref().unwrap().name,
            "map_transform",
            "map_transform name must survive compact round-trip"
        );
    }

    /// The NL-6 mandate ("compact → human →
    /// compact must produce an AST-equal result") verified over the **whole**
    /// `WorkflowFile`, for the entire normative corpus. The runtime contract
    /// once placed this test in `workflows.rs`; it lives here instead,
    /// alongside the fixture corpus it sweeps — the contract now records the
    /// real location, correcting a claim that didn't match where the code and
    /// tests actually ended up.
    ///
    /// Compares parsed ASTs, never source text: three source-level
    /// differences are expected and AST-stable (`core: x.nodus` truncates to
    /// `x` on the FIRST parse, `mode` defaults to `production` during parsing
    /// if absent, `@err:` raw text is already space-joined by
    /// `consume_rest_of_line` before this test ever sees it) — a string-equal
    /// assertion would fail on all three for reasons unrelated to NL-6.
    ///
    /// This assertion was first scoped to `.steps` because three fields did
    /// not yet round-trip: `tests` (fixed per §10 —
    /// `raw_lines` is now the emission source, with values re-quoted so they
    /// re-lex to the single token they came from) and `macros`/`human_mode`
    /// (previously never emitted at all). Widening
    /// this assertion to the whole file is the decisive acceptance
    /// signal (§10.5): it is unreachable unless all
    /// three are actually fixed, so a regression in any one fails it here.
    #[test]
    fn full_corpus_ast_equal_after_compact_round_trip() {
        for (name, source) in NORMATIVE_CORPUS {
            let ast1 =
                Parser::parse(source).unwrap_or_else(|e| panic!("{name}: parse failed: {e:?}"));
            let compact = Transpiler::to_nodus(&ast1);
            let ast2 = Parser::parse(&compact).unwrap_or_else(|e| {
                panic!("{name}: compact re-parse failed: {e:?}\ncompact:\n{compact}")
            });
            assert_eq!(
                ast1, ast2,
                "{name}: the whole WorkflowFile must be unchanged by a compact round-trip (NL-6)\ncompact:\n{compact}"
            );
        }
    }

    #[test]
    fn transpile_mode_compact_via_library() {
        let out = workflows::transpile(SIMPLE_LOG, TranspileMode::Compact)
            .expect("library transpile compact must succeed");
        assert!(
            out.contains("§wf:simple_log"),
            "library compact output must include §wf header"
        );
    }

    #[test]
    fn transpile_mode_human_via_library() {
        let out = workflows::transpile(SIMPLE_LOG, TranspileMode::Human)
            .expect("library transpile human must succeed");
        assert!(!out.is_empty(), "human output must not be empty");
        assert!(
            out.contains("WORKFLOW"),
            "library human output must include WORKFLOW section"
        );
    }
}

// ── Execution parity ──────────────────────────────────────────────────────────

mod execution {
    use super::*;
    use nodus::executor::Status;

    #[test]
    fn simple_log_executes_ok() {
        let result = workflows::run(
            SIMPLE_LOG,
            "simple_log.nodus",
            Some(crate::support::sample_input(SIMPLE_LOG)),
        )
        .expect("simple_log must execute without block-class errors");
        assert_eq!(
            result.status,
            Status::Ok,
            "simple_log must return Status::Ok"
        );
        assert_eq!(
            result.workflow, "wf:simple_log",
            "workflow identifier must match the declared name"
        );
    }

    #[test]
    fn ticket_triage_executes_ok() {
        let result = workflows::run(
            TICKET_TRIAGE,
            "ticket_triage.nodus",
            Some(crate::support::sample_input(TICKET_TRIAGE)),
        )
        .expect("ticket_triage must execute without block-class errors");
        assert_eq!(
            result.status,
            Status::Ok,
            "ticket_triage must return Status::Ok"
        );
    }

    #[test]
    fn until_quality_loop_executes_ok() {
        let result = workflows::run(
            UNTIL_QUALITY_LOOP,
            "until_quality_loop.nodus",
            Some(crate::support::sample_input(UNTIL_QUALITY_LOOP)),
        )
        .expect("until_quality_loop must execute without block-class errors");
        assert_eq!(
            result.status,
            Status::Ok,
            "bounded ~UNTIL loop must not abort the workflow"
        );
    }

    #[test]
    fn conditional_executes_ok() {
        let result = workflows::run(
            CONDITIONAL,
            "conditional.nodus",
            Some(crate::support::sample_input(CONDITIONAL)),
        )
        .expect("conditional must execute without block-class errors");
        assert_eq!(
            result.status,
            Status::Ok,
            "conditional must return Status::Ok"
        );
    }

    #[test]
    fn for_loop_executes_ok() {
        let result = workflows::run(
            FOR_LOOP,
            "for_loop.nodus",
            Some(crate::support::sample_input(FOR_LOOP)),
        )
        .expect("for_loop must execute without block-class errors");
        assert_eq!(result.status, Status::Ok, "for_loop must return Status::Ok");
    }

    #[test]
    fn parallel_join_executes_ok() {
        let result = workflows::run(
            PARALLEL_JOIN,
            "parallel_join.nodus",
            Some(crate::support::sample_input(PARALLEL_JOIN)),
        )
        .expect("parallel_join must execute without block-class errors");
        assert_eq!(
            result.status,
            Status::Ok,
            "parallel_join must return Status::Ok"
        );
    }

    #[test]
    fn macro_expand_executes_ok() {
        let result = workflows::run(
            MACRO_EXPAND,
            "macro_expand.nodus",
            Some(crate::support::sample_input(MACRO_EXPAND)),
        )
        .expect("macro_expand must execute without block-class errors");
        assert_eq!(
            result.status,
            Status::Ok,
            "macro_expand must return Status::Ok"
        );
    }

    #[test]
    fn map_transform_executes_ok() {
        // No input: $in.items is unset, so ~MAP iterates zero elements — this
        // proves the workflow clears the validator gate and runs to
        // completion, matching for_loop/parallel_join's no-input convention.
        let result = workflows::run(
            MAP_TRANSFORM,
            "map_transform.nodus",
            Some(crate::support::sample_input(MAP_TRANSFORM)),
        )
        .expect("map_transform must execute without block-class errors");
        assert_eq!(
            result.status,
            Status::Ok,
            "map_transform must return Status::Ok; errors: {:?}",
            result.errors
        );
    }

    #[test]
    fn run_rejects_missing_runtime_with_e001() {
        let err = workflows::run(
            LINT_MISSING_RUNTIME,
            "lint_missing_runtime.nodus",
            Some(crate::support::sample_input(LINT_MISSING_RUNTIME)),
        )
        .expect_err("run must fail fast when §runtime is absent");
        assert!(!err.is_empty(), "rejection diagnostics must not be empty");
        let got: Vec<_> = err.iter().map(|d| d.code.as_str()).collect();
        assert!(
            got.contains(&"E001"),
            "expected E001 in rejection diagnostics; got: {got:?}"
        );
    }

    #[test]
    fn run_rejects_publish_before_validate_with_e005() {
        let err = workflows::run(
            LINT_PUBLISH_BEFORE_VALIDATE,
            "lint_publish_before_validate.nodus",
            None,
        )
        .expect_err("run must fail fast on PUBLISH before VALIDATE");
        let got: Vec<_> = err.iter().map(|d| d.code.as_str()).collect();
        assert!(
            got.contains(&"E005"),
            "expected E005 in rejection diagnostics; got: {got:?}"
        );
    }
}
