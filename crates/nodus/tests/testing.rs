// Integration tests for the @test: block contract — NT-1…NT-7 compliance.
//
// Each test maps to one or more invariants from the testing spec. Fixtures are
// inline strings to keep the test file self-contained and immune to fixture
// file changes that do not relate to the tested invariants.

use nodus::workflows::{self};

// ── Shared fixtures ───────────────────────────────────────────────────────────

// Workflow used by block_isolation (NT-1) and expected_assertion_pass (NT-3).
// Two blocks with different query inputs and matching expected values.
const ISOLATION_WF: &str = "\
§wf:isolation_wf v1.0
§runtime: { core: schema.nodus }
@in: { query? }
@out: $out
@err: ESCALATE(human)
@steps:
  1. GEN($in.query) → $out
  2. LOG($out)
@test: block_a {
  input:
    query: alpha
  expected:
    $out: \"[STUB gen(alpha) tone=brand]\"
}
@test: block_b {
  input:
    query: beta
  expected:
    $out: \"[STUB gen(beta) tone=brand]\"
}
";

// Workflow used by input_override (NT-2).
const OVERRIDE_WF: &str = "\
§wf:override_wf v1.0
§runtime: { core: schema.nodus }
@in: { query? }
@out: $out
@err: ESCALATE(human)
@steps:
  1. GEN($in.query) → $out
  2. LOG($out)
@test: override_test {
  input:
    query: overridden
  expected:
    $out: \"[STUB gen(overridden) tone=brand]\"
}
";

// Workflow used by expected_assertion_fail (NT-4).
const FAIL_WF: &str = "\
§wf:fail_wf v1.0
§runtime: { core: schema.nodus }
@in: { query? }
@out: $out
@err: ESCALATE(human)
@steps:
  1. GEN($in.query) → $out
  2. LOG($out)
@test: fail_test {
  input:
    query: hello
  expected:
    $out: \"wrong_expected_value\"
}
@test: sibling_test {
  input:
    query: hello
}
";

// Workflow used by tag_filter (NT-6).
const TAG_WF: &str = "\
§wf:tag_wf v1.0
§runtime: { core: schema.nodus }
@in: { query? }
@out: $out
@err: ESCALATE(human)
@steps:
  1. GEN($in.query) → $out
@test: smoke_block {
  tags: [smoke]
}
@test: integration_block {
  tags: [integration]
}
";

// Workflow used by ordered_report (NT-7).
const ORDER_WF: &str = "\
§wf:order_wf v1.0
§runtime: { core: schema.nodus }
@in: { query? }
@out: $out
@err: ESCALATE(human)
@steps:
  1. GEN($in.query) → $out
@test: first {}
@test: second {}
@test: third {}
";

// ── NT-1: Block isolation ─────────────────────────────────────────────────────

#[test]
fn block_isolation() {
    // Each block executes in a fresh context. If block_a's $out leaked into
    // block_b's context, block_b's assertion would fail (different expected value).
    let report = workflows::test(ISOLATION_WF, "isolation_wf.nodus").expect("test");
    assert_eq!(report.results.len(), 2, "both blocks must appear in report");
    assert!(
        report.results[0].passed,
        "block_a failed: {}",
        report.results[0].message
    );
    assert!(
        report.results[1].passed,
        "block_b failed — possible state leak from block_a: {}",
        report.results[1].message
    );
    assert_eq!(report.passed, 2);
    assert_eq!(report.failed, 0);
}

// ── NT-2: Input override ──────────────────────────────────────────────────────

#[test]
fn input_override() {
    // Block's input: section fully overrides the default @in: value for its run.
    // If the override were ignored, GEN would receive the default (empty or null)
    // instead of "overridden", and the expected assertion would fail.
    let report = workflows::test(OVERRIDE_WF, "override_wf.nodus").expect("test");
    assert_eq!(report.results.len(), 1);
    assert!(
        report.results[0].passed,
        "input override not applied: {}",
        report.results[0].message
    );
}

// ── NT-3: Expected assertion binding ─────────────────────────────────────────

#[test]
fn expected_assertion_pass() {
    // expected: entries whose values match the execution context pass the block.
    let report = workflows::test(ISOLATION_WF, "isolation_wf.nodus").expect("test");
    assert_eq!(report.passed, 2, "both assertion blocks should pass");
}

// ── NT-4: Assertion failure semantics ────────────────────────────────────────

#[test]
fn expected_assertion_fail() {
    // A failing assertion marks only that block as failed; the sibling block
    // still executes and appears in the report.
    let report = workflows::test(FAIL_WF, "fail_wf.nodus").expect("test");
    assert_eq!(
        report.results.len(),
        2,
        "both blocks must execute regardless of failure"
    );

    let fail = &report.results[0];
    assert!(
        !fail.passed,
        "fail_test should fail on wrong expected value"
    );
    assert!(
        fail.message.contains("$out"),
        "failure message must name the failing assertion, got: {}",
        fail.message
    );

    let sibling = &report.results[1];
    assert!(
        sibling.passed,
        "sibling_test should pass independently: {}",
        sibling.message
    );

    assert_eq!(report.failed, 1);
    assert_eq!(report.passed, 1);
}

// ── NT-6: Tag metadata ────────────────────────────────────────────────────────

#[test]
fn tag_filter_skips_unmatched() {
    // Only blocks whose tags satisfy the predicate appear in the report.
    let report = workflows::test_with_tags(TAG_WF, &["smoke"]).expect("test_with_tags");
    assert_eq!(
        report.results.len(),
        1,
        "only the smoke block should run; got {:?}",
        report.results.iter().map(|r| &r.name).collect::<Vec<_>>()
    );
    assert_eq!(report.results[0].name, "smoke_block");
}

#[test]
fn tag_filter_empty_runs_all() {
    // An empty tag filter is equivalent to "run all blocks" (NT-6).
    let report = workflows::test_with_tags(TAG_WF, &[]).expect("test_with_tags empty");
    assert_eq!(
        report.results.len(),
        2,
        "empty filter should run all blocks"
    );
}

// ── NT-7: Ordered reporting ───────────────────────────────────────────────────

#[test]
fn ordered_report() {
    // TestReport.results are in @test: declaration order, not execution order.
    let report = workflows::test(ORDER_WF, "order_wf.nodus").expect("test");
    assert_eq!(report.results.len(), 3);
    assert_eq!(report.results[0].name, "first");
    assert_eq!(report.results[1].name, "second");
    assert_eq!(report.results[2].name, "third");
}
