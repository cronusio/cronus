//! The caller's input is checked against the workflow's declared `@in:` contract
//! before anything runs (NL-9): presence, type, undeclared names, and names the
//! runtime owns.

use nodus::{
    Executor, ModelProvider, NoopPolicyProvider, Status, StubProvider, Value, parser::Parser,
    workflows,
};

const WF: &str = "\
§wf:contract v1.0
§runtime: { core: schema.nodus }
@in: { query: str, limit?: int, ratio?: float, flag?: bool, tags?: list, meta_info?: obj, anything?: any, shade?: hue, tone?=warm }
@out: $out
@err: ESCALATE(human)
@steps:
  1. GEN($in.query) → $out
";

const NO_IN_WF: &str = "\
§wf:no_contract v1.0
§runtime: { core: schema.nodus }
@out: $out
@err: ESCALATE(human)
@steps:
  1. GEN(hello) → $out
";

const DECLARES_USER_WF: &str = "\
§wf:declares_user v1.0
§runtime: { core: schema.nodus }
@in: { user: str }
@out: $out
@err: ESCALATE(human)
@steps:
  1. GEN($in.user) → $out
";

fn map(entries: &[(&str, Value)]) -> Option<Value> {
    Some(Value::Map(
        entries
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect(),
    ))
}

fn text(s: &str) -> Value {
    Value::Text(s.to_string())
}

/// The E022 messages a run rejected with, or none when it ran.
fn rejections(source: &str, name: &str, input: Option<Value>) -> Vec<String> {
    match workflows::run(source, name, input) {
        Ok(_) => Vec::new(),
        Err(diagnostics) => diagnostics
            .into_iter()
            .filter(|d| d.code == "E022")
            .map(|d| d.message)
            .collect(),
    }
}

// ─── Presence ────────────────────────────────────────────────────────────────

#[test]
fn a_required_field_left_out_is_rejected_before_the_run() {
    let found = rejections(WF, "contract.nodus", None);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("query"), "{}", found[0]);
}

#[test]
fn a_required_field_supplied_as_null_counts_as_left_out() {
    let found = rejections(WF, "contract.nodus", map(&[("query", Value::Null)]));
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn optional_and_defaulted_fields_may_be_omitted() {
    let found = rejections(WF, "contract.nodus", map(&[("query", text("hello"))]));
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_satisfied_contract_runs() {
    let result = workflows::run(WF, "contract.nodus", map(&[("query", text("hello"))]))
        .expect("the contract is satisfied");
    assert_eq!(result.status, Status::Ok, "errors: {:?}", result.errors);
}

// ─── Type ────────────────────────────────────────────────────────────────────

#[test]
fn a_value_of_the_wrong_type_is_rejected_naming_field_and_kind() {
    let found = rejections(
        WF,
        "contract.nodus",
        map(&[("query", text("q")), ("limit", text("ten"))]),
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].contains("limit") && found[0].contains("int"),
        "{}",
        found[0]
    );
    assert!(
        found[0].contains("text"),
        "it says what it got: {}",
        found[0]
    );
}

#[test]
fn every_declared_type_accepts_its_own_kind() {
    let found = rejections(
        WF,
        "contract.nodus",
        map(&[
            ("query", text("q")),
            ("limit", Value::Int(3)),
            ("ratio", Value::Float(0.5)),
            ("flag", Value::Bool(true)),
            ("tags", Value::List(vec![text("a")])),
            ("meta_info", Value::Map(vec![("k".to_string(), text("v"))])),
            ("anything", Value::Int(7)),
        ]),
    );
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_float_field_accepts_a_whole_number() {
    let found = rejections(
        WF,
        "contract.nodus",
        map(&[("query", text("q")), ("ratio", Value::Int(2))]),
    );
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_type_outside_the_registry_is_not_judged_at_run_time() {
    // `hue` is not a registered type; the validator warns (W013) — the input
    // check does not invent a rule for it.
    let found = rejections(
        WF,
        "contract.nodus",
        map(&[("query", text("q")), ("shade", Value::Int(1))]),
    );
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn several_breaches_are_all_reported_together() {
    let found = rejections(
        WF,
        "contract.nodus",
        map(&[("limit", text("x")), ("extra", Value::Int(1))]),
    );
    assert_eq!(
        found.len(),
        3,
        "missing query, wrong-typed limit, undeclared extra: {found:?}"
    );
}

// ─── Names the workflow does not declare ─────────────────────────────────────

#[test]
fn a_key_the_workflow_does_not_declare_is_rejected_when_it_declares_a_contract() {
    let found = rejections(
        WF,
        "contract.nodus",
        map(&[("query", text("q")), ("surprise", Value::Int(1))]),
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("surprise"));
}

#[test]
fn a_workflow_with_no_declared_contract_accepts_ordinary_input() {
    let found = rejections(
        NO_IN_WF,
        "no_contract.nodus",
        map(&[("anything", Value::Int(1))]),
    );
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn input_that_is_not_a_map_is_rejected() {
    let found = rejections(WF, "contract.nodus", Some(text("just a string")));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("map"));
}

// ─── Names the runtime owns ──────────────────────────────────────────────────

#[test]
fn a_runtime_owned_name_cannot_be_supplied_by_the_caller() {
    for owned in [
        "user",
        "session",
        "ctx",
        "memory",
        "kb_results",
        "flags",
        "error",
        "meta",
        "in",
    ] {
        let found = rejections(
            NO_IN_WF,
            "no_contract.nodus",
            map(&[(owned, text("forged"))]),
        );
        assert_eq!(found.len(), 1, "{owned}: {found:?}");
        assert!(found[0].contains("runtime-owned"), "{owned}: {}", found[0]);
    }
}

#[test]
fn a_runtime_owned_name_is_rejected_even_with_a_dollar_sign() {
    let found = rejections(
        NO_IN_WF,
        "no_contract.nodus",
        map(&[("$user", text("forged"))]),
    );
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn a_workflow_that_declares_such_a_name_receives_it_as_its_own_input() {
    let result = workflows::run(
        DECLARES_USER_WF,
        "declares_user.nodus",
        map(&[("user", text("ada"))]),
    )
    .expect("declared on purpose, so it is the workflow's own input");
    assert_eq!(result.status, Status::Ok, "errors: {:?}", result.errors);
}

// ─── Every entry point enforces it ───────────────────────────────────────────

struct Host;

impl ModelProvider for Host {
    fn model_id(&self) -> &str {
        "host"
    }

    fn generate(&self, prompt: &str, _modifiers: &[(String, String)]) -> String {
        format!("answer({prompt})")
    }

    fn analyze(&self, _text: &str, _flags: &[String]) -> Value {
        Value::Null
    }
}

#[test]
fn the_provider_entry_point_enforces_the_contract() {
    let err = workflows::run_with_provider(WF, "contract.nodus", None, Host)
        .expect_err("required field missing");
    assert!(err.iter().any(|d| d.code == "E022"));
}

#[test]
fn the_policy_entry_point_enforces_the_contract() {
    let err = workflows::run_with_policy(WF, "contract.nodus", None, NoopPolicyProvider)
        .expect_err("required field missing");
    assert!(err.iter().any(|d| d.code == "E022"));
}

#[test]
fn the_audit_entry_point_enforces_the_contract() {
    let err =
        workflows::run_with_audit(WF, "contract.nodus", None, nodus::NoopAuditProvider, "", "")
            .expect_err("required field missing");
    assert!(err.iter().any(|d| d.code == "E022"));
}

// ─── A host driving the executor directly ────────────────────────────────────

#[test]
fn the_executor_withholds_runtime_owned_names_and_says_so() {
    let ast = Parser::parse(NO_IN_WF).expect("parse");
    let input = Value::Map(vec![
        ("user".to_string(), text("forged")),
        ("session".to_string(), text("forged")),
        ("topic".to_string(), text("kept")),
    ]);
    let result = Executor::with_stub().execute(&ast, Some(input));

    assert_ne!(
        result.vars.get("user"),
        Some(&text("forged")),
        "a caller must not be able to forge $user"
    );
    assert_ne!(result.vars.get("session"), Some(&text("forged")));
    assert_eq!(
        result.vars.get("topic"),
        Some(&text("kept")),
        "an ordinary key still binds"
    );
    assert!(result.flags.contains(&"INPUT_IGNORED:user".to_string()));
    assert!(result.flags.contains(&"INPUT_IGNORED:session".to_string()));
}

#[test]
fn the_executor_keeps_a_runtime_owned_name_the_workflow_declares() {
    let ast = Parser::parse(DECLARES_USER_WF).expect("parse");
    let input = Value::Map(vec![("user".to_string(), text("ada"))]);
    let result = Executor::new(StubProvider).execute(&ast, Some(input));
    assert_eq!(result.vars.get("user"), Some(&text("ada")));
    assert!(
        result
            .flags
            .iter()
            .all(|f| !f.starts_with("INPUT_IGNORED:"))
    );
}

#[test]
fn a_caller_cannot_forge_the_restart_counter() {
    let source = "\
§wf:restarting v1.0
§runtime: { core: schema.nodus, restart_max: 2 }
@out: $out
@err: ESCALATE(human)
@steps:
  1. GEN(hello) → $out
";
    let ast = Parser::parse(source).expect("parse");
    let input = Value::Map(vec![("restart_count".to_string(), Value::Int(99))]);
    let result = Executor::with_stub().execute(&ast, Some(input));
    assert_eq!(
        result.vars.get("restart_count"),
        Some(&Value::Int(0)),
        "the runtime's own count, not the caller's claim"
    );
    assert!(
        result
            .flags
            .contains(&"INPUT_IGNORED:restart_count".to_string())
    );
}
