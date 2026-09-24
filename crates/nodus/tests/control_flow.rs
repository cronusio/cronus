//! Integration tests for the v0.7 control-flow constructs.
//!
//! Covers the `!HALT` (fatal stop) and `!PAUSE` (suspension) action flags on a
//! conditional branch: a taken `!HALT` ends the run failed and stops later
//! steps; a taken `!PAUSE` suspends with a resume descriptor and runs no later
//! step. The not-taken case proves neither flag fires spuriously.

use nodus::{
    executor::{DialogOutcome, DialogProvider, Status, Value},
    workflows,
};

// A dialog backend that always times out — a deterministic, repeatable per-step
// runtime error used to exercise `~RETRY:n` exhaustion.
struct TimeoutProvider;
impl DialogProvider for TimeoutProvider {
    fn ask(&self, _p: &str, _m: &[(String, String)]) -> DialogOutcome {
        DialogOutcome::Timeout
    }
    fn confirm(&self, _c: &str, _m: &[(String, String)]) -> DialogOutcome {
        DialogOutcome::Timeout
    }
}

// A taken branch carrying `!HALT` alongside the required escalation.
const HALT_WF: &str = r#"§wf:guard_halt v1.0
§runtime: { core: schema.nodus }
@out: $out
@err: ESCALATE(human)
@steps:
  1. ?IF 1 > 0 → ESCALATE(human) !HALT
  2. GEN(go) → $out
"#;

// A branch whose `!HALT` is never reached (condition false).
const HALT_NOT_TAKEN_WF: &str = r#"§wf:guard_skip v1.0
§runtime: { core: schema.nodus }
@out: $out
@err: ESCALATE(human)
@steps:
  1. ?IF 0 > 1 → ESCALATE(human) !HALT
  2. LOG(done) → $out
"#;

// A taken branch carrying `!PAUSE`; the action itself does not suspend and
// does not lock the output, so a `Paused` status proves the flag fired.
const PAUSE_WF: &str = r#"§wf:guard_pause v1.0
§runtime: { core: schema.nodus }
@out: $out
@err: ESCALATE(human)
@steps:
  1. ?IF 1 > 0 → ANALYZE(state) !PAUSE
  2. GEN(after) → $scratch
"#;

#[test]
fn halt_branch_ends_failed_and_stops_run() {
    let result = workflows::run(HALT_WF, "guard_halt.nodus", None).expect("run");
    assert_eq!(
        result.status,
        Status::Failed,
        "a taken !HALT branch must fail the run; errors: {:?}",
        result.errors
    );
    assert!(
        !result.log.iter().any(|e| e.command == "GEN"),
        "no step after !HALT may run; log: {:?}",
        result.log
    );
}

#[test]
fn halt_not_taken_runs_to_completion() {
    let result = workflows::run(HALT_NOT_TAKEN_WF, "guard_skip.nodus", None).expect("run");
    assert_eq!(
        result.status,
        Status::Ok,
        "an untaken !HALT branch must not fail the run; errors: {:?}",
        result.errors
    );
}

// ?SWITCH fixtures — scrutinee seeded via an `@in` default. SWITCH_MATCH_WF,
// SWITCH_DEFAULT_WF and SWITCH_NO_MATCH_WF below declare no arm target at
// all — their three tests passing unmodified after the parser fix
// (try_parse_command_from_string) is this suite's non-regression evidence
// for the no-target case.
const SWITCH_MATCH_WF: &str = r#"§wf:switch_match v1.0
§runtime: { core: schema.nodus }
@in: { category?=urgent }
@out: $out
@err: ESCALATE(human)
@steps:
  1. ?SWITCH $in.category:
    urgent → ANALYZE(crisis)
    spam → GEN(reply)
  ~END
"#;

const SWITCH_DEFAULT_WF: &str = r#"§wf:switch_default v1.0
§runtime: { core: schema.nodus }
@in: { category?=mystery }
@out: $out
@err: ESCALATE(human)
@steps:
  1. ?SWITCH $in.category:
    urgent → ANALYZE(crisis)
    * → GEN(reply)
  ~END
"#;

const SWITCH_NO_MATCH_WF: &str = r#"§wf:switch_nomatch v1.0
§runtime: { core: schema.nodus }
@in: { category?=mystery }
@out: $out
@err: ESCALATE(human)
@steps:
  1. ?SWITCH $in.category:
    urgent → ANALYZE(crisis)
  ~END
  2. LOG(after) → $out
"#;

// Each arm binds a trailing → $target (NL-10:
// "?SWITCH arm actions bind their targets in declaration order"). Two
// distinct target names prove per-arm binding — not a single shared target
// aliased across the whole switch — and the later step reading whichever
// target the taken arm bound proves the target is reachable through
// workflows::run, not just parsed into the AST.
const SWITCH_ARM_TARGETS_WF: &str = r#"§wf:switch_targets v1.0
§runtime: { core: schema.nodus }
@in: { category?=urgent }
@out: $out
@err: ESCALATE(human)
@steps:
  1. ?SWITCH $in.category:
    urgent → GEN(crisis) → $urgent_pick
    spam → GEN(reply) → $spam_pick
  ~END
  2. LOG(done) → $out
"#;

#[test]
fn switch_runs_first_matching_arm() {
    let result = workflows::run(SWITCH_MATCH_WF, "switch_match.nodus", None).expect("run");
    assert_eq!(result.status, Status::Ok, "errors: {:?}", result.errors);
    assert!(
        result.log.iter().any(|e| e.command == "ANALYZE"),
        "the matching arm must run; log: {:?}",
        result.log
    );
    assert!(
        !result.log.iter().any(|e| e.command == "GEN"),
        "a non-matching arm must not run; log: {:?}",
        result.log
    );
}

#[test]
fn switch_falls_through_to_default() {
    let result = workflows::run(SWITCH_DEFAULT_WF, "switch_default.nodus", None).expect("run");
    assert_eq!(result.status, Status::Ok, "errors: {:?}", result.errors);
    assert!(
        result.log.iter().any(|e| e.command == "GEN"),
        "the default arm must run when nothing matches; log: {:?}",
        result.log
    );
    assert!(
        !result.log.iter().any(|e| e.command == "ANALYZE"),
        "no value arm matched, so none should run; log: {:?}",
        result.log
    );
}

#[test]
fn switch_arm_bound_target_reachable_through_run() {
    // Default @in.category = urgent (first arm). Runs through workflows::run
    // (parse → validate → execute), not Executor::execute — proving the
    // arm's → $urgent_pick target is reachable through every validated public
    // entry point, the same bar set earlier for ~MAP's $it.
    let result = workflows::run(SWITCH_ARM_TARGETS_WF, "switch_targets.nodus", None).expect("run");
    assert_eq!(result.status, Status::Ok, "errors: {:?}", result.errors);
    assert_eq!(
        result.vars.get("urgent_pick"),
        Some(&Value::Text("[STUB gen(crisis) tone=brand]".to_string())),
        "the taken arm's own target must be bound; vars: {:?}",
        result.vars
    );
    assert_eq!(
        result.vars.get("spam_pick"),
        None,
        "a non-taken arm's target must never be bound; vars: {:?}",
        result.vars
    );
}

#[test]
fn switch_arm_targets_bind_independently_per_arm() {
    // Overriding @in.category to the second arm must bind THAT arm's own
    // target, not the first arm's — proving each arm's → target is bound to
    // that specific arm (declaration order), not a single target shared
    // across the whole switch regardless of which arm fired.
    let input = Value::Map(vec![(
        "category".to_string(),
        Value::Text("spam".to_string()),
    )]);
    let result =
        workflows::run(SWITCH_ARM_TARGETS_WF, "switch_targets.nodus", Some(input)).expect("run");
    assert_eq!(result.status, Status::Ok, "errors: {:?}", result.errors);
    assert_eq!(
        result.vars.get("spam_pick"),
        Some(&Value::Text("[STUB gen(reply) tone=brand]".to_string())),
        "vars: {:?}",
        result.vars
    );
    assert_eq!(
        result.vars.get("urgent_pick"),
        None,
        "the first arm did not fire, so its target must not be bound; vars: {:?}",
        result.vars
    );
}

#[test]
fn switch_no_match_warns_and_continues() {
    let result = workflows::run(SWITCH_NO_MATCH_WF, "switch_nomatch.nodus", None).expect("run");
    assert!(
        result.flags.iter().any(|f| f == "NODUS:SWITCH_NO_MATCH"),
        "an unmatched switch with no default must flag SWITCH_NO_MATCH; flags: {:?}",
        result.flags
    );
    assert_eq!(
        result.status,
        Status::Ok,
        "SWITCH_NO_MATCH is advisory; the run continues. errors: {:?}",
        result.errors
    );
    assert!(
        result.log.iter().any(|e| e.command == "LOG"),
        "the step after the switch must still run; log: {:?}",
        result.log
    );
}

// ~RETRY:n — a flaky step retried up to n times.
const RETRY_TIMEOUT_WF: &str = r#"§wf:retry_timeout v1.0
§runtime: { core: schema.nodus }
@out: $out
@err: ESCALATE(human)
@steps:
  1. ~RETRY:3 ASK(question) → $answer
  2. LOG(after) → $out
"#;

#[test]
fn retry_reruns_failing_step_up_to_bound() {
    let result = workflows::run_with_dialog(
        RETRY_TIMEOUT_WF,
        "retry_timeout.nodus",
        None,
        TimeoutProvider,
    )
    .expect("run");
    let timeouts = result
        .errors
        .iter()
        .filter(|e| e.code == "NODUS:DIALOG_TIMEOUT")
        .count();
    assert_eq!(
        timeouts, 3,
        "a ~RETRY:3 step that always fails is attempted 3 times; errors: {:?}",
        result.errors
    );
    // Exhausted retries surface as errors but do not abort the run.
    assert_eq!(
        result.status,
        Status::Partial,
        "errors: {:?}",
        result.errors
    );
    // NL-9: exhausted retries leave an uncaught DIALOG_TIMEOUT behind, and
    // RETRY_TIMEOUT_WF declares `@err: ESCALATE(human)` — the handler now
    // dispatches, which is the terminal action for the run, so step 2
    // (`LOG`) no longer executes. This assertion was written before NL-9
    // dispatch existed, when "routes to @err:" was still
    // documented-but-unbuilt (the language spec's own ~RETRY:n row always
    // said exhaustion "routes to @err:"); updated to match the real behavior
    // rather than the pre-fix absence of it.
    assert!(
        result.log.iter().any(|e| e.command == "ESCALATE"),
        "an exhausted retry's uncaught DIALOG_TIMEOUT must dispatch the declared @err: handler; log: {:?}",
        result.log
    );
    assert!(
        !result.log.iter().any(|e| e.command == "LOG"),
        "dispatch is the terminal action — the step after an exhausted retry must not run; log: {:?}",
        result.log
    );
    let error_map = result
        .vars
        .get("error")
        .cloned()
        .expect("$error is always seeded");
    assert_eq!(
        error_map,
        Value::Map(vec![
            (
                "code".to_string(),
                Value::Text("NODUS:DIALOG_TIMEOUT".to_string())
            ),
            ("step".to_string(), Value::Int(1)),
            (
                "reason".to_string(),
                Value::Text("dialog 'ASK' timed out before an answer".to_string())
            ),
        ]),
        "$error must carry the triggering DIALOG_TIMEOUT before the handler dispatches; vars: {:?}",
        result.vars
    );
}

// ~MAP — transforms a collection element-by-element via the implicit `$it`
// binding. Validated end-to-end through workflows::run: until the NL-10
// E004 conformance fix, every ~MAP workflow was rejected before it ran.
const MAP_WF: &str = r#"§wf:map_transform v1.0
§runtime: { core: schema.nodus }
@in: { items?: list }
@out: $out
@err: ESCALATE(human)
@steps:
  1. ~MAP $in.items: GEN($it) → $out
"#;

#[test]
fn map_transforms_each_element_producing_an_n_element_list() {
    let input = Value::Map(vec![(
        "items".to_string(),
        Value::List(vec![
            Value::Text("a".to_string()),
            Value::Text("b".to_string()),
            Value::Text("c".to_string()),
        ]),
    )]);
    let result = workflows::run(MAP_WF, "map_transform.nodus", Some(input)).expect("run");
    assert_eq!(result.status, Status::Ok, "errors: {:?}", result.errors);
    match result.out {
        Value::List(items) => assert_eq!(
            items.len(),
            3,
            "a 3-element collection must transform to a 3-element list; got: {items:?}"
        ),
        other => panic!("expected $out to be a Value::List; got: {other:?}"),
    }
}

#[test]
fn map_over_empty_collection_yields_empty_list_no_error() {
    let input = Value::Map(vec![("items".to_string(), Value::List(vec![]))]);
    let result = workflows::run(MAP_WF, "map_transform.nodus", Some(input)).expect("run");
    assert_eq!(result.status, Status::Ok, "errors: {:?}", result.errors);
    assert_eq!(
        result.out,
        Value::List(vec![]),
        "an empty collection must yield an empty list, never an error"
    );
}

// The same transform over an untyped source: what the collection actually
// holds is decided at run time, so a non-list value reaches the ~MAP.
const MAP_ANY_WF: &str = r#"§wf:map_any v1.0
§runtime: { core: schema.nodus }
@in: { items?: any }
@out: $out
@err: ESCALATE(human)
@steps:
  1. ~MAP $in.items: GEN($it) → $out
"#;

#[test]
fn map_over_non_list_collection_yields_empty_list_no_error() {
    let input = Value::Map(vec![("items".to_string(), Value::Int(5))]);
    let result = workflows::run(MAP_ANY_WF, "map_any.nodus", Some(input)).expect("run");
    assert_eq!(result.status, Status::Ok, "errors: {:?}", result.errors);
    assert_eq!(
        result.out,
        Value::List(vec![]),
        "a non-list collection must yield an empty list, never an error"
    );
    // The empty result is not silent: it is indistinguishable from mapping an
    // empty list otherwise, and a check built over it would pass vacuously.
    assert!(
        result
            .flags
            .iter()
            .any(|f| f.starts_with("MAP_SOURCE_NOT_A_LIST:")),
        "a present-but-not-a-list source is flagged: {:?}",
        result.flags
    );
}

#[test]
fn a_list_typed_input_field_rejects_a_non_list_value_before_the_run() {
    let input = Value::Map(vec![("items".to_string(), Value::Int(5))]);
    let diagnostics = workflows::run(MAP_WF, "map_transform.nodus", Some(input))
        .expect_err("the declared type is enforced before any step runs");
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == "E022" && d.message.contains("items")),
        "{diagnostics:?}"
    );
}

#[test]
fn map_over_an_absent_collection_is_not_flagged() {
    let result = workflows::run(MAP_WF, "map_transform.nodus", None).expect("run");
    assert_eq!(result.status, Status::Ok, "errors: {:?}", result.errors);
    assert_eq!(result.out, Value::List(vec![]));
    assert!(
        result
            .flags
            .iter()
            .all(|f| !f.starts_with("MAP_SOURCE_NOT_A_LIST:")),
        "nothing to map is a legitimate empty case: {:?}",
        result.flags
    );
}

#[test]
fn map_over_a_real_list_is_not_flagged() {
    let input = Value::Map(vec![(
        "items".to_string(),
        Value::List(vec![Value::Text("a".to_string())]),
    )]);
    let result = workflows::run(MAP_WF, "map_transform.nodus", Some(input)).expect("run");
    assert!(
        result
            .flags
            .iter()
            .all(|f| !f.starts_with("MAP_SOURCE_NOT_A_LIST:")),
        "{:?}",
        result.flags
    );
}

#[test]
fn pause_branch_suspends_with_resume() {
    let result = workflows::run(PAUSE_WF, "guard_pause.nodus", None).expect("run");
    assert_eq!(
        result.status,
        Status::Paused,
        "a taken !PAUSE branch must suspend the run; errors: {:?}",
        result.errors
    );
    let resume = result
        .resume
        .expect("a paused run carries a resume descriptor");
    assert_eq!(resume.workflow, "wf:guard_pause");
    assert!(
        !result.log.iter().any(|e| e.command == "GEN"),
        "no step after !PAUSE may run; log: {:?}",
        result.log
    );
}
