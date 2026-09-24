//! Integration tests for the `§config` declarative-configuration surface
//! (NL-20).
//!
//! NL-20 shape-check coverage, the secret-neutrality gate, the LP-8
//! fail-fast path, and the full declaration → proposed → acceptance → run
//! happy path.

use nodus::ast::{ConfigDecl, ConfigField, FieldConstraint};
use nodus::executor::{Status, Value};
use nodus::observability::{AuditProvider, ExecutionEvent, RunManifest};
use nodus::portability::{
    CapabilityManifest, ConfigOutcome, ConfigProvider, DefaultConfigProvider, ExtensionRole,
    HostCapabilities, validate_manifest,
};
use nodus::validator::{ConfigReason, check_config_values};
use nodus::workflows::{self, run_with_config, run_with_config_and_audit};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

// ─── Fixtures ─────────────────────────────────────────────────────────────────

const CONFIG_WF: &str = r#"§wf:configured_greeting v1.0
§runtime: { core: schema.nodus }
@in: { query? }
@out: $out
@err: ESCALATE(human)
@steps:
  1. GEN($in.query) → $out
  2. LOG($out)
"#;

fn sample_decl() -> ConfigDecl {
    ConfigDecl {
        header: None,
        fields: vec![
            ConfigField {
                name: "max_retries".to_string(),
                type_name: "int".to_string(),
                default: Some("3".to_string()),
                constraint: Some(FieldConstraint::Range {
                    lo: "1".to_string(),
                    hi: "10".to_string(),
                }),
                describe: Some("Maximum retry attempts".to_string()),
                ..Default::default()
            },
            ConfigField {
                name: "level".to_string(),
                type_name: "str".to_string(),
                default: Some("medium".to_string()),
                constraint: Some(FieldConstraint::OneOf(vec![
                    "low".to_string(),
                    "medium".to_string(),
                    "high".to_string(),
                ])),
                ..Default::default()
            },
            ConfigField {
                name: "api_key".to_string(),
                type_name: "str".to_string(),
                required: true,
                secret: true,
                describe: Some("External API credential".to_string()),
                ..Default::default()
            },
        ],
    }
}

/// Audit sink that counts every event and run-complete callback it receives —
/// proves a rejected config run emits nothing (observer neutrality), mirroring
/// `tests/portability.rs`'s `CountingAudit`.
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

// ─── NL-20 shape-check coverage (via check_config_values directly) ───────────

#[test]
fn shape_check_unknown_field() {
    let decl = sample_decl();
    let proposed = vec![
        ("api_key".to_string(), Value::Text("k".to_string())),
        ("nonexistent".to_string(), Value::Int(1)),
    ];
    let violations = check_config_values(&decl, &proposed).expect_err("must reject");
    assert!(
        violations
            .iter()
            .any(|v| v.field == "nonexistent" && v.reason == ConfigReason::UnknownField)
    );
}

#[test]
fn shape_check_missing_required() {
    let decl = sample_decl();
    let violations = check_config_values(&decl, &[]).expect_err("must reject");
    assert!(
        violations
            .iter()
            .any(|v| v.field == "api_key" && v.reason == ConfigReason::MissingRequired)
    );
}

#[test]
fn shape_check_type_mismatch() {
    let decl = sample_decl();
    let proposed = vec![
        ("api_key".to_string(), Value::Text("k".to_string())),
        ("max_retries".to_string(), Value::Bool(true)),
    ];
    let violations = check_config_values(&decl, &proposed).expect_err("must reject");
    assert!(
        violations
            .iter()
            .any(|v| v.field == "max_retries" && v.reason == ConfigReason::TypeMismatch)
    );
}

#[test]
fn shape_check_out_of_range() {
    let decl = sample_decl();
    let proposed = vec![
        ("api_key".to_string(), Value::Text("k".to_string())),
        ("max_retries".to_string(), Value::Int(999)),
    ];
    let violations = check_config_values(&decl, &proposed).expect_err("must reject");
    assert!(
        violations
            .iter()
            .any(|v| v.field == "max_retries" && v.reason == ConfigReason::OutOfRange)
    );
}

#[test]
fn shape_check_not_in_enum() {
    let decl = sample_decl();
    let proposed = vec![
        ("api_key".to_string(), Value::Text("k".to_string())),
        ("level".to_string(), Value::Text("critical".to_string())),
    ];
    let violations = check_config_values(&decl, &proposed).expect_err("must reject");
    assert!(
        violations
            .iter()
            .any(|v| v.field == "level" && v.reason == ConfigReason::NotInEnum)
    );
}

#[test]
fn shape_check_duplicate_field() {
    // NL-27: a field name declared twice is a validation error, not a
    // silently-resolved ambiguity — regardless of whether the two
    // declarations otherwise agree.
    let decl = ConfigDecl {
        header: None,
        fields: vec![
            ConfigField {
                name: "api_key".to_string(),
                type_name: "str".to_string(),
                secret: true,
                ..Default::default()
            },
            ConfigField {
                name: "api_key".to_string(),
                type_name: "str".to_string(),
                secret: false,
                ..Default::default()
            },
        ],
    };
    let violations = check_config_values(&decl, &[]).expect_err("must reject");
    assert!(
        violations
            .iter()
            .any(|v| v.field == "api_key" && v.reason == ConfigReason::DuplicateField),
        "violations: {violations:?}"
    );
}

#[test]
fn shape_check_accepts_defaults_when_nothing_proposed_for_optional_fields() {
    let decl = sample_decl();
    let proposed = vec![("api_key".to_string(), Value::Text("k".to_string()))];
    let accepted = check_config_values(&decl, &proposed).expect("valid set must pass");
    assert_eq!(accepted.get("max_retries"), Some(&Value::Int(3)));
    assert_eq!(
        accepted.get("level"),
        Some(&Value::Text("medium".to_string()))
    );
}

// ─── Secret-neutrality gate ───────────────────────────────────────────────────

#[test]
fn secret_field_is_usable_but_excluded_from_in_config() {
    let decl = sample_decl();
    let provider = DefaultConfigProvider;
    let proposed = vec![(
        "api_key".to_string(),
        Value::Text("sk-live-secret".to_string()),
    )];

    // Usable: the shape check + provider accept it, and it is retrievable.
    let candidate = check_config_values(&decl, &proposed).expect("shape check passes");
    match provider.accept(&decl, candidate) {
        ConfigOutcome::Accepted(accepted) => {
            assert_eq!(
                accepted.get("api_key"),
                Some(&Value::Text("sk-live-secret".to_string()))
            );
            assert!(accepted.is_secret("api_key"));
            // Excluded from the merged $in.config projection.
            let names: Vec<&str> = accepted.non_secret_fields().map(|(n, _)| n).collect();
            assert!(!names.contains(&"api_key"));
        }
        ConfigOutcome::Rejected(v) => panic!("expected Accepted, got Rejected({v:?})"),
    }

    // End-to-end via run_with_config: the run succeeds (api_key was usable to
    // satisfy `required`), and no construct in this workflow could have
    // rendered it into a prompt — GEN($in.query) never references
    // $in.config.api_key because run_with_config never puts it there.
    let result = run_with_config(
        CONFIG_WF,
        "configured_greeting.nodus",
        None,
        &decl,
        &proposed,
        &provider,
    )
    .expect("run_with_config");
    assert_eq!(result.status, Status::Ok, "errors: {:?}", result.errors);
}

// ─── Pre-run fail-fast: rejection precedes side effects ──────────────────────

#[test]
fn rejected_config_run_emits_no_audit_events() {
    let decl = sample_decl();
    let provider = DefaultConfigProvider;
    let counter = Arc::new(AtomicUsize::new(0));

    // Missing required api_key -> shape check fails -> CONFIG_INVALID, no boot.
    let result = run_with_config_and_audit(
        CONFIG_WF,
        "configured_greeting.nodus",
        None,
        &decl,
        &[],
        &provider,
        CountingAudit {
            events: counter.clone(),
        },
        "run-rejected",
        "2026-07-24T00:00:00Z",
    )
    .expect("run_with_config_and_audit returns a RunResult, not a parse error");

    assert_eq!(result.status, Status::Failed);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code == "NODUS:CONFIG_INVALID"),
        "errors: {:?}",
        result.errors
    );
    assert!(result.log.is_empty(), "no step may execute on rejection");
    assert_eq!(
        counter.load(Ordering::SeqCst),
        0,
        "a rejected config run must emit no audit events (observer neutrality)"
    );

    // Control: a satisfiable audited run DOES emit events — proves the sink counts.
    let counter_ok = Arc::new(AtomicUsize::new(0));
    let proposed = vec![("api_key".to_string(), Value::Text("k".to_string()))];
    let _ = run_with_config_and_audit(
        CONFIG_WF,
        "configured_greeting.nodus",
        None,
        &decl,
        &proposed,
        &provider,
        CountingAudit {
            events: counter_ok.clone(),
        },
        "run-ok",
        "2026-07-24T00:00:00Z",
    )
    .expect("audited ok run");
    assert!(
        counter_ok.load(Ordering::SeqCst) > 0,
        "a real run must emit audit events"
    );
}

#[test]
fn rejected_config_run_leaves_no_partial_state() {
    // A rejected run's RunResult carries no output and no vars — there is no
    // sense in which the workflow was "partially configured".
    let decl = sample_decl();
    let provider = DefaultConfigProvider;
    let result = run_with_config(
        CONFIG_WF,
        "configured_greeting.nodus",
        None,
        &decl,
        &[],
        &provider,
    )
    .expect("run_with_config");
    assert_eq!(result.out, Value::Null);
    assert!(result.vars.is_empty());
}

// ─── NL-27 regression: a duplicate field can no longer leak a secret ─────────

#[test]
fn duplicate_secret_field_rejects_rather_than_leaking() {
    // Before NL-27 enforcement, a `§config:` name declared twice — once
    // `secret`, once not — produced two entries in `AcceptedConfig` under one
    // name: `is_secret` answered `true` (`.any()` over both), while
    // `non_secret_fields()` (the set `run_with_config` merges into
    // `$in.config`) filtered on the *other* declaration and emitted the value
    // anyway. The assertion here is deliberately on the run's outcome — a
    // rejection — rather than on any surviving two-entry shape, so it stays
    // valid however the accept loop is later refactored: it is impossible for
    // this scenario to produce an `AcceptedConfig` at all.
    let decl = nodus::parser::Parser::parse_config(
        "\
§config:leaky v1.0
api_key : str
  required
  secret
api_key : str
  default: not-a-secret
",
    )
    .expect("parse_config");

    let proposed = vec![("api_key".to_string(), Value::Text("sk-live".to_string()))];
    let violations = check_config_values(&decl, &proposed).expect_err("must reject");
    assert!(
        violations
            .iter()
            .any(|v| v.field == "api_key" && v.reason == ConfigReason::DuplicateField),
        "violations: {violations:?}"
    );

    // End-to-end: the run itself is rejected before boot, so no step — and no
    // path that could read $in.config — ever executes.
    let provider = DefaultConfigProvider;
    let result = run_with_config(
        CONFIG_WF,
        "configured_greeting.nodus",
        None,
        &decl,
        &proposed,
        &provider,
    )
    .expect("run_with_config returns a RunResult, not a parse error");
    assert_eq!(result.status, Status::Failed);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code == "NODUS:CONFIG_INVALID"),
        "errors: {:?}",
        result.errors
    );
    assert!(result.log.is_empty(), "no step may execute on rejection");
}

// ─── LP-8: ExtensionRole::Config fail-fast ────────────────────────────────────

#[test]
fn config_role_satisfied_by_builtin_host() {
    let manifest = CapabilityManifest::new().require_role(ExtensionRole::Config);
    let host = HostCapabilities::builtin();
    assert!(validate_manifest(&manifest, &host).is_empty());
}

#[test]
fn config_role_rejected_by_stripped_host() {
    let manifest = CapabilityManifest::new().require_role(ExtensionRole::Config);
    let host = HostCapabilities::new(); // no roles
    let missing = validate_manifest(&manifest, &host);
    assert_eq!(
        missing,
        vec![nodus::portability::Missing::Role(ExtensionRole::Config)]
    );
}

// ─── Full happy path: declaration → proposed → acceptance → run ──────────────

#[test]
fn full_happy_path_declaration_to_run() {
    let src = "\
§config:app v1.0
max_retries : int
  default: 3
  range: 1, 10
api_key : str
  required
  secret
level : str
  default: medium
  one_of: low | medium | high
";
    let decl = nodus::parser::Parser::parse_config(src).expect("parse_config");
    let provider = DefaultConfigProvider;
    let proposed = vec![("api_key".to_string(), Value::Text("sk-live".to_string()))];

    let result = run_with_config(
        CONFIG_WF,
        "configured_greeting.nodus",
        Some(Value::Map(vec![(
            "query".to_string(),
            Value::Text("hello".to_string()),
        )])),
        &decl,
        &proposed,
        &provider,
    )
    .expect("run_with_config");

    assert_eq!(result.status, Status::Ok, "errors: {:?}", result.errors);
    // The workflow ran with $in.query intact alongside the merged $in.config.
    assert!(
        matches!(&result.out, Value::Text(s) if s.contains("hello")),
        "original $in.query must survive the config merge: {:?}",
        result.out
    );
}

#[test]
fn run_without_config_reference_is_unaffected() {
    // Baseline control: a plain `run` (no config surface at all) behaves
    // exactly as before this feature landed — additive, not a redefinition.
    let result = workflows::run(CONFIG_WF, "configured_greeting.nodus", None).expect("run");
    assert_eq!(result.status, Status::Ok, "errors: {:?}", result.errors);
}
