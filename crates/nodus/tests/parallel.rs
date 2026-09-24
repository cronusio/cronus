//! `~PARALLEL` semantics on a runtime that runs the branches in sequence: the
//! first failing branch ends the block (fail-fast, `~JOIN` bypassed), and
//! `~JOIN → $target` collects every branch's result in declared order.

use nodus::{Executor, ModelError, ModelProvider, Status, Value, parser::Parser, vocab, workflows};

/// Answers, but fails any generation whose prompt is `boom`.
struct SelectiveModel;

impl ModelProvider for SelectiveModel {
    fn model_id(&self) -> &str {
        "selective"
    }

    fn generate(&self, prompt: &str, _modifiers: &[(String, String)]) -> String {
        format!("answer({prompt})")
    }

    fn analyze(&self, _text: &str, flags: &[String]) -> Value {
        Value::Map(
            flags
                .iter()
                .map(|f| (f.clone(), Value::Float(0.5)))
                .collect(),
        )
    }

    fn try_generate(
        &self,
        prompt: &str,
        modifiers: &[(String, String)],
    ) -> Result<String, ModelError> {
        if prompt == "boom" {
            Err(ModelError::new("this prompt always fails"))
        } else {
            Ok(self.generate(prompt, modifiers))
        }
    }
}

fn run(source: &str) -> nodus::RunResult {
    let ast = Parser::parse(source).expect("parse");
    Executor::new(SelectiveModel).execute(&ast, None)
}

const JOIN_WF: &str = "\
§wf:join_wf v1.0
§runtime: { core: schema.nodus }
@out: $out
@err: ESCALATE(human)
@steps:
  1. ~PARALLEL
       GEN(alpha) → $first
       GEN(beta) → $second
       LOG($first)
     ~JOIN → $results
  2. LOG($results)
";

const FAILING_BRANCH_WF: &str = "\
§wf:failing_branch v1.0
§runtime: { core: schema.nodus }
@out: $out
@err: ESCALATE(human)
@steps:
  1. ~PARALLEL
       GEN(alpha) → $first
       GEN(boom) → $second
       GEN(gamma) → $third
     ~JOIN → $results
  2. LOG($results)
";

const HALTING_BRANCH_WF: &str = "\
§wf:halting_branch v1.0
§runtime: { core: schema.nodus }
!!NEVER: FORGET
@out: $out
@err: ESCALATE(human)
@steps:
  1. ~PARALLEL
       GEN(alpha) → $first
       FORGET(x)
       GEN(gamma) → $third
     ~JOIN → $results
";

// ─── ~JOIN collects the branch results ───────────────────────────────────────

#[test]
fn join_collects_every_branch_result_keyed_by_its_target_in_declared_order() {
    let result = run(JOIN_WF);
    assert_eq!(result.status, Status::Ok, "errors: {:?}", result.errors);

    let Some(Value::Map(joined)) = result.vars.get("results") else {
        panic!(
            "$results must be a map, got {:?}",
            result.vars.get("results")
        );
    };
    let keys: Vec<&str> = joined.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(
        keys,
        ["first", "second", "branch_3"],
        "declared order; an unbound branch is keyed by position"
    );
    assert_eq!(
        joined[0].1,
        Value::Text("answer(alpha)".to_string()),
        "each branch's own result, not a placeholder"
    );
    assert_eq!(joined[1].1, Value::Text("answer(beta)".to_string()));
}

#[test]
fn branch_writes_are_visible_after_the_block() {
    let result = run(JOIN_WF);
    assert_eq!(
        result.vars.get("first"),
        Some(&Value::Text("answer(alpha)".to_string()))
    );
    assert_eq!(
        result.vars.get("second"),
        Some(&Value::Text("answer(beta)".to_string()))
    );
}

#[test]
fn a_block_without_a_join_target_binds_nothing_extra() {
    let source = "\
§wf:no_join v1.0
§runtime: { core: schema.nodus }
@out: $out
@steps:
  1. ~PARALLEL
       GEN(alpha) → $first
       GEN(beta) → $second
     ~JOIN
";
    let result = workflows::run(source, "no_join.nodus", None).expect("runs");
    assert_eq!(result.status, Status::Ok, "errors: {:?}", result.errors);
    assert!(!result.vars.contains_key("results"));
}

// ─── Fail-fast ───────────────────────────────────────────────────────────────

#[test]
fn a_failing_branch_stops_the_later_branches_and_bypasses_the_join() {
    let result = run(FAILING_BRANCH_WF);

    assert_eq!(result.status, Status::Partial);
    let error = result.errors.first().expect("the failure is recorded");
    assert_eq!(error.code, vocab::error_code::MODEL_CALL_FAILED);

    assert_eq!(
        result.vars.get("first"),
        Some(&Value::Text("answer(alpha)".to_string())),
        "the branch that finished before the failure keeps its result"
    );
    assert!(
        !result.vars.contains_key("third"),
        "a branch after the failing one never starts"
    );
    assert!(
        !result.vars.contains_key("results"),
        "~JOIN is bypassed when a branch fails"
    );
    assert!(
        result.log.iter().all(|entry| entry.step == 1),
        "step 2 did not run: {:?}",
        result.log
    );
}

#[test]
fn a_failing_branch_reaches_the_declared_error_handler() {
    let result = run(FAILING_BRANCH_WF);
    assert!(
        result.flags.iter().any(|f| f.starts_with("ESCALATE:")),
        "the @err handler ran: {:?}",
        result.flags
    );
}

#[test]
fn a_control_signal_in_a_branch_ends_the_block_and_the_run() {
    let result = run(HALTING_BRANCH_WF);

    assert_eq!(result.status, Status::Failed, "errors: {:?}", result.errors);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code == vocab::error_code::RULE_VIOLATION)
    );
    assert!(
        !result.vars.contains_key("third"),
        "the branch after the violating one never starts"
    );
    assert!(!result.vars.contains_key("results"));
}
