//! The `@test:` runner holds a workflow to the same gate an ordinary run passes,
//! and a block that names something that cannot exist fails the file before any
//! block runs instead of running on a default it never meant.

use nodus::{Error, workflows};

const CLEAN: &str = "\
§wf:clean v1.0
§runtime: { core: schema.nodus }
@in: { query?: str }
@out: $out
@err: ESCALATE(human)
@steps:
  1. GEN($in.query) → $out
  2. LOG($out)
@test: smoke {
  input:
    query: hello
  expected:
    $out: hello
}
";

fn validation_failure(source: &str) -> (String, String) {
    match workflows::test(source, "clean.nodus") {
        Err(Error::Validate { code, message }) => (code, message),
        other => panic!("expected a validation failure, got {other:?}"),
    }
}

#[test]
fn a_clean_workflow_still_reports_per_block_results() {
    let report = workflows::test(CLEAN, "clean.nodus").expect("valid");
    assert_eq!(report.results.len(), 1);
    // The stub answers with its own label, so the assertion decides the verdict.
    assert!(!report.results[0].passed);
    assert!(report.results[0].message.contains("$out"));
}

// ─── The ordinary validation gate applies ────────────────────────────────────

#[test]
fn a_workflow_with_a_lint_error_fails_before_any_block_runs() {
    // Two blocks of one name: E015, which blocks an ordinary run.
    let source = CLEAN.replace(
        "@test: smoke {",
        "@test: smoke {\n  tags: [a]\n}\n@test: smoke {",
    );
    let (code, _) = validation_failure(&source);
    assert_eq!(code, "E015");
}

#[test]
fn an_unknown_command_fails_the_test_file_too() {
    let source = CLEAN.replace("1. GEN($in.query) → $out", "1. FLUSHX($in.query) → $out");
    let (code, message) = validation_failure(&source);
    assert_eq!(code, "E021");
    assert!(message.contains("FLUSHX"), "{message}");
}

#[test]
fn the_tag_filtered_entry_point_validates_too() {
    let source = CLEAN.replace("1. GEN($in.query) → $out", "1. FLUSHX($in.query) → $out");
    assert!(matches!(
        workflows::test_with_tags(&source, &["smoke"]),
        Err(Error::Validate { .. })
    ));
}

#[test]
fn the_tag_filtered_entry_point_does_not_judge_the_file_name() {
    // No file name is given, so the name-matches-file rule has nothing to
    // compare against and must stay quiet.
    workflows::test_with_tags(CLEAN, &[]).expect("valid under the name it declares");
}

// ─── Names that cannot exist ───────────────────────────────────────────

#[test]
fn an_input_key_the_workflow_does_not_declare_is_an_error_not_a_silent_default() {
    let source = CLEAN.replace("query: hello", "qurey: hello");
    let (code, message) = validation_failure(&source);
    assert_eq!(code, "E023");
    assert!(message.contains("qurey"), "{message}");
}

#[test]
fn an_expected_variable_that_can_never_exist_is_an_error() {
    let source = CLEAN.replace("$out: hello", "$otu: hello");
    let (code, message) = validation_failure(&source);
    assert_eq!(code, "E023");
    assert!(message.contains("$otu"), "{message}");
}

#[test]
fn expected_may_name_out_a_pipeline_target_an_input_or_a_reserved_variable() {
    let source = "\
§wf:names v1.0
§runtime: { core: schema.nodus }
@in: { query?: str }
@out: $out
@err: ESCALATE(human)
@steps:
  1. GEN($in.query) → $draft
  2. GEN($draft) → $out
@test: names {
  input:
    query: hello
  expected:
    $out: x
    $draft: x
    $query: hello
    $flags: x
}
";
    workflows::test(source, "names.nodus").expect("every asserted name can exist");
}

#[test]
fn a_test_setting_input_on_a_workflow_with_no_in_block_is_an_error() {
    let source = "\
§wf:clean v1.0
§runtime: { core: schema.nodus }
@out: $out
@err: ESCALATE(human)
@steps:
  1. GEN(hello) → $out
@test: setup {
  input:
    query: hello
}
";
    let (code, _) = validation_failure(source);
    assert_eq!(code, "E023");
}
