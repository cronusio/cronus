use std::sync::{Arc, Mutex, PoisonError};

use cronus_contract::{Binder, BinderKind, Invocable, Locus, Outcome, OutcomeValue, Stability};
use cronus_domain::invocable::{Dispatcher, InvocableRegistry, Registrant};
use cronus_domain::io_message;
use nodus::executor::{Status, Value};
use nodus::validator::Severity;
use nodus::workflows::{self, RunOptions, TranspileMode};
use nodus::{AuditProvider, ExecutionEvent, ExecutionMode, RunManifest, RunResult};

use super::{core_id, flag_arg, opt_text_arg, text_arg};

pub(super) fn register(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    register_scaffold(registry, dispatcher);
    register_validate(registry, dispatcher);
    register_run(registry, dispatcher);
    register_transpile(registry, dispatcher);
}

/// `nodus::executor::Value` is shaped exactly like `OutcomeValue` (Null,
/// Bool, Int, Float, Text, List, Map — the kinds line up one-for-one with
/// `Empty`/`Boolean`/`Integer`/`Float`/`Text`/`List`/`Record`).
fn nodus_value_to_outcome(value: &Value) -> OutcomeValue {
    match value {
        Value::Null => OutcomeValue::Empty,
        Value::Bool(b) => OutcomeValue::Boolean(*b),
        Value::Int(n) => OutcomeValue::Integer(*n),
        Value::Float(f) => OutcomeValue::Float(*f),
        Value::Text(s) => OutcomeValue::Text(s.clone()),
        Value::List(items) => {
            OutcomeValue::List(items.iter().map(nodus_value_to_outcome).collect())
        }
        Value::Map(fields) => OutcomeValue::Record(
            fields
                .iter()
                .map(|(k, v)| (k.clone(), nodus_value_to_outcome(v)))
                .collect(),
        ),
    }
}

fn register_scaffold(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("workflow.scaffold");
    let invocable = Invocable {
        id: id.clone(),
        name: "Scaffold",
        summary: "Generate a new workflow skeleton.",
        group: "workflow",
        locus: Locus::Semantic,
        binders: vec![
            Binder {
                name: "name",
                kind: BinderKind::Text,
                optional: false,
            },
            Binder {
                name: "out",
                kind: BinderKind::NamedText,
                optional: true,
            },
        ],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry.register(&Registrant::core(), invocable).expect(
        "core:workflow.scaffold registers cleanly at bootstrap — a duplicate id here is a bug",
    );
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let name = text_arg(args, "name");
            // `--out` wins. Otherwise the positional is the destination: if it
            // already looks like a `.nodus` file (or a path to one) it is used
            // verbatim, so `scaffold foo.nodus` writes `foo.nodus` rather than
            // `foo.nodus.nodus` and the reported path is the one a later
            // `workflow validate` would name. A bare identifier still becomes
            // `<name>.nodus` in the current directory.
            let dest = match opt_text_arg(args, "out") {
                Some(out) => std::path::PathBuf::from(out),
                None if name.ends_with(".nodus") => std::path::PathBuf::from(name),
                None => std::path::PathBuf::from(format!("{name}.nodus")),
            };
            if dest.exists() {
                return Outcome::Unavailable {
                    reason: format!("file already exists: {}", dest.display()),
                };
            }
            // The workflow's own identity is the file stem, never the path.
            let workflow_name = dest
                .file_stem()
                .and_then(|s| s.to_str())
                .filter(|s| !s.is_empty())
                .unwrap_or(name);
            let ast = workflows::scaffold(workflow_name);
            let source = nodus::transpiler::Transpiler::to_nodus(&ast);
            match std::fs::write(&dest, &source) {
                Ok(()) => {
                    let abs = dest.canonicalize().unwrap_or(dest);
                    Outcome::Value(OutcomeValue::Record(vec![(
                        "path".to_string(),
                        OutcomeValue::Text(cronus_domain::paths::display_clean(&abs)),
                    )]))
                }
                Err(e) => Outcome::Unavailable {
                    reason: io_message::describe(&e),
                },
            }
        }),
    );
}

fn register_validate(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("workflow.validate");
    let invocable = Invocable {
        id: id.clone(),
        name: "Validate",
        summary: "Validate a workflow file and report diagnostics.",
        group: "workflow",
        locus: Locus::Semantic,
        binders: vec![Binder {
            name: "file",
            kind: BinderKind::Text,
            optional: false,
        }],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry.register(&Registrant::core(), invocable).expect(
        "core:workflow.validate registers cleanly at bootstrap — a duplicate id here is a bug",
    );
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let file = std::path::PathBuf::from(text_arg(args, "file"));
            let source = match std::fs::read_to_string(&file) {
                Ok(s) => s,
                Err(e) => {
                    return Outcome::Unavailable {
                        reason: format!(
                            "cannot read {}: {}",
                            file.display(),
                            io_message::describe(&e)
                        ),
                    };
                }
            };
            let filename = file
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown.nodus")
                .to_string();
            let report = match workflows::validate(&source, &filename) {
                Ok(r) => r,
                Err(e) => {
                    return Outcome::Unavailable {
                        reason: format!("parse failed: {e}"),
                    };
                }
            };
            let diag_record = |d: &nodus::validator::Diagnostic| {
                OutcomeValue::Record(vec![
                    ("code".to_string(), OutcomeValue::Text(d.code.clone())),
                    ("message".to_string(), OutcomeValue::Text(d.message.clone())),
                    ("line".to_string(), OutcomeValue::Integer(d.line as i64)),
                ])
            };
            let errors: Vec<OutcomeValue> = report
                .diagnostics
                .iter()
                .filter(|d| d.severity == Severity::Error)
                .map(diag_record)
                .collect();
            let warnings: Vec<OutcomeValue> = report
                .diagnostics
                .iter()
                .filter(|d| d.severity == Severity::Warning)
                .map(diag_record)
                .collect();
            let infos: Vec<OutcomeValue> = report
                .diagnostics
                .iter()
                .filter(|d| d.severity == Severity::Info)
                .map(diag_record)
                .collect();
            Outcome::Value(OutcomeValue::Record(vec![
                (
                    "status".to_string(),
                    OutcomeValue::Text(if report.has_errors {
                        "failed".to_string()
                    } else {
                        "ok".to_string()
                    }),
                ),
                ("errors".to_string(), OutcomeValue::List(errors)),
                ("warnings".to_string(), OutcomeValue::List(warnings)),
                ("infos".to_string(), OutcomeValue::List(infos)),
            ]))
        }),
    );
}

fn register_run(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("workflow.run");
    let invocable = Invocable {
        id: id.clone(),
        name: "Run",
        summary: "Execute a workflow file.",
        group: "workflow",
        locus: Locus::Semantic,
        binders: vec![
            Binder {
                name: "file",
                kind: BinderKind::Text,
                optional: false,
            },
            Binder {
                name: "input",
                kind: BinderKind::NamedText,
                optional: true,
            },
        ],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:workflow.run registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let file = std::path::PathBuf::from(text_arg(args, "file"));
            let source = match std::fs::read_to_string(&file) {
                Ok(s) => s,
                Err(e) => {
                    return Outcome::Unavailable {
                        reason: format!(
                            "cannot read {}: {}",
                            file.display(),
                            io_message::describe(&e)
                        ),
                    };
                }
            };
            let filename = file
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown.nodus")
                .to_string();
            let parsed_input = match opt_text_arg(args, "input") {
                None => None,
                Some(s) => match serde_json::from_str::<serde_json::Value>(s) {
                    Ok(jv) => Some(json_value_to_nodus(jv)),
                    Err(e) => {
                        return Outcome::Unavailable {
                            reason: format!("--input is not valid JSON: {e}"),
                        };
                    }
                },
            };
            run_outcome(&source, &filename, parsed_input)
        }),
    );
}

/// Records whether the model behind a run was real, as the run's own manifest
/// states it. The verb reports this beside the answer so a run answered by the
/// built-in stand-in is never mistaken for one a model produced.
#[derive(Clone, Default)]
struct ModeProbe {
    mode: Arc<Mutex<Option<ExecutionMode>>>,
}

impl ModeProbe {
    /// `real`, `simulated`, or `unreported` when the run never filed a manifest.
    fn label(&self) -> &'static str {
        let mode = self.mode.lock().unwrap_or_else(PoisonError::into_inner);
        match &*mode {
            Some(ExecutionMode::Real) => "real",
            Some(ExecutionMode::Simulated { .. }) => "simulated",
            None => "unreported",
        }
    }
}

impl AuditProvider for ModeProbe {
    fn record_event(&self, _event: ExecutionEvent) {}

    fn run_complete(&self, manifest: RunManifest) {
        *self.mode.lock().unwrap_or_else(PoisonError::into_inner) = Some(manifest.execution_mode);
    }
}

/// Runs one workflow source and shapes the result for the renderer.
///
/// Every run goes through the same validation and input gate as any other
/// entry point; a source that fails it never starts and is reported as
/// `failed` with its diagnostics. `partial` is reported as its own status — a
/// run that finished with recorded step errors is not the same claim as one
/// that finished clean — though it still exits 0, as before.
fn run_outcome(source: &str, filename: &str, input: Option<Value>) -> Outcome {
    let probe = ModeProbe::default();
    let options = RunOptions::new().audit(probe.clone());
    match workflows::run_with_options(source, filename, input, options) {
        Err(diags) => {
            let messages: Vec<OutcomeValue> = diags
                .iter()
                .map(|d| {
                    OutcomeValue::Text(format!("[{}] {} (line {})", d.code, d.message, d.line))
                })
                .collect();
            let mut fields = vec![
                (
                    "status".to_string(),
                    OutcomeValue::Text("failed".to_string()),
                ),
                ("diagnostics".to_string(), OutcomeValue::List(messages)),
            ];
            // A breach of the input contract is the one failure the caller can
            // fix without touching the workflow, so say how.
            if diags.iter().any(|d| d.code == "E022") {
                fields.push((
                    "hint".to_string(),
                    OutcomeValue::Text(
                        "the input is checked against the workflow's @in: declaration; \
                         pass it as JSON with --input, for example --input '{\"field\": \"value\"}'"
                            .to_string(),
                    ),
                ));
            }
            Outcome::Value(OutcomeValue::Record(fields))
        }
        Ok(result) => shape_result(&result, probe.label()),
    }
}

fn shape_result(result: &RunResult, mode: &str) -> Outcome {
    // Failed, aborted and paused runs exit non-zero (the renderer recognises
    // those `status` values); ok and partial exit 0. Paused shares the failed
    // bucket rather than getting a third exit code.
    let status_str = match result.status {
        Status::Ok => "ok",
        Status::Partial => "partial",
        Status::Failed | Status::Aborted => "failed",
        Status::Paused => "paused",
    };
    let mut fields = vec![
        (
            "workflow".to_string(),
            OutcomeValue::Text(result.workflow.clone()),
        ),
        (
            "status".to_string(),
            OutcomeValue::Text(status_str.to_string()),
        ),
        ("mode".to_string(), OutcomeValue::Text(mode.to_string())),
        ("out".to_string(), nodus_value_to_outcome(&result.out)),
    ];
    if !result.log.is_empty() {
        fields.push((
            "log".to_string(),
            OutcomeValue::List(
                result
                    .log
                    .iter()
                    .map(|e| {
                        OutcomeValue::Text(format!("{}. [{}] {:?}", e.step, e.command, e.result))
                    })
                    .collect(),
            ),
        ));
    }
    if !result.errors.is_empty() {
        fields.push((
            "errors".to_string(),
            OutcomeValue::List(
                result
                    .errors
                    .iter()
                    .map(|e| {
                        OutcomeValue::Text(format!("[{} step {}] {}", e.code, e.step, e.reason))
                    })
                    .collect(),
            ),
        ));
    }
    Outcome::Value(OutcomeValue::Record(fields))
}

fn json_value_to_nodus(jv: serde_json::Value) -> Value {
    match jv {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else {
                Value::Float(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => Value::Text(s),
        serde_json::Value::Array(items) => {
            Value::List(items.into_iter().map(json_value_to_nodus).collect())
        }
        serde_json::Value::Object(map) => Value::Map(
            map.into_iter()
                .map(|(k, v)| (k, json_value_to_nodus(v)))
                .collect(),
        ),
    }
}

fn register_transpile(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("workflow.transpile");
    let invocable = Invocable {
        id: id.clone(),
        name: "Transpile",
        summary: "Transpile a workflow to a different representation.",
        group: "workflow",
        locus: Locus::Semantic,
        binders: vec![
            Binder {
                name: "file",
                kind: BinderKind::Text,
                optional: false,
            },
            Binder {
                name: "human",
                kind: BinderKind::Flag,
                optional: true,
            },
            Binder {
                name: "compact",
                kind: BinderKind::Flag,
                optional: true,
            },
        ],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry.register(&Registrant::core(), invocable).expect(
        "core:workflow.transpile registers cleanly at bootstrap — a duplicate id here is a bug",
    );
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let file = std::path::PathBuf::from(text_arg(args, "file"));
            let human = flag_arg(args, "human");
            let compact = flag_arg(args, "compact");
            // The pre-migration grammar enforced `--human`/`--compact`
            // mutual exclusivity at the clap level (`conflicts_with`) — a
            // usage failure. `Binder` has no cross-binder constraint
            // concept, so this is now an application-level refusal
            // instead — a real, disclosed shift, not an invented
            // shortcut.
            if human && compact {
                return Outcome::Unavailable {
                    reason: "--human and --compact cannot both be given".to_string(),
                };
            }
            let mode = if human {
                TranspileMode::Human
            } else {
                TranspileMode::Compact
            };
            let source = match std::fs::read_to_string(&file) {
                Ok(s) => s,
                Err(e) => {
                    return Outcome::Unavailable {
                        reason: format!(
                            "cannot read {}: {}",
                            file.display(),
                            io_message::describe(&e)
                        ),
                    };
                }
            };
            match workflows::transpile(&source, mode) {
                Ok(output) => Outcome::Value(OutcomeValue::Text(output)),
                Err(e) => Outcome::Unavailable {
                    reason: e.to_string(),
                },
            }
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const GREET: &str = "\
§wf:greet v1.0
§runtime: { core: schema.nodus }
@in: { name: text }
@out: $out
@err: ESCALATE(human)
@steps:
  1. GEN($in.name) → $out
  2. LOG($out)
";

    fn field<'a>(outcome: &'a Outcome, name: &str) -> Option<&'a OutcomeValue> {
        match outcome {
            Outcome::Value(OutcomeValue::Record(fields)) => {
                fields.iter().find(|(k, _)| k == name).map(|(_, v)| v)
            }
            _ => None,
        }
    }

    fn text<'a>(outcome: &'a Outcome, name: &str) -> Option<&'a str> {
        match field(outcome, name) {
            Some(OutcomeValue::Text(s)) => Some(s.as_str()),
            _ => None,
        }
    }

    fn named(name: &str) -> Option<Value> {
        Some(Value::Map(vec![(
            "name".to_string(),
            Value::Text(name.to_string()),
        )]))
    }

    #[test]
    fn a_run_answered_by_the_built_in_stand_in_is_labelled_simulated() {
        let outcome = run_outcome(GREET, "greet.nodus", named("Ada"));
        assert_eq!(text(&outcome, "status"), Some("ok"));
        assert_eq!(
            text(&outcome, "mode"),
            Some("simulated"),
            "no model is wired to this verb, so the answer must not read as a model's"
        );
    }

    #[test]
    fn a_missing_required_input_is_a_failed_run_that_never_started() {
        let outcome = run_outcome(GREET, "greet.nodus", None);
        assert_eq!(text(&outcome, "status"), Some("failed"));
        assert!(text(&outcome, "mode").is_none(), "no run, so no mode");
        let Some(OutcomeValue::List(diagnostics)) = field(&outcome, "diagnostics") else {
            panic!("diagnostics missing: {outcome:?}");
        };
        assert!(
            diagnostics
                .iter()
                .any(|d| matches!(d, OutcomeValue::Text(t) if t.contains("E022"))),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn an_input_breach_tells_the_caller_how_to_supply_the_input() {
        let outcome = run_outcome(GREET, "greet.nodus", None);
        let hint = text(&outcome, "hint").expect("a hint accompanies an input breach");
        assert!(hint.contains("--input"), "{hint}");
    }

    #[test]
    fn a_source_that_fails_validation_is_reported_failed() {
        let outcome = run_outcome("§wf:x v1.0\n@steps:\n  1. GEN(a)\n", "x.nodus", None);
        assert_eq!(text(&outcome, "status"), Some("failed"));
        assert!(field(&outcome, "diagnostics").is_some());
        assert!(
            text(&outcome, "hint").is_none(),
            "the input hint is only for input breaches"
        );
    }

    fn finished(status: Status) -> RunResult {
        RunResult {
            workflow: "wf:greet".to_string(),
            status,
            out: Value::Null,
            log: Vec::new(),
            errors: Vec::new(),
            flags: Vec::new(),
            vars: std::collections::HashMap::new(),
            resume: None,
        }
    }

    #[test]
    fn each_run_status_keeps_its_own_label() {
        for (status, label) in [
            (Status::Ok, "ok"),
            (Status::Partial, "partial"),
            (Status::Failed, "failed"),
            (Status::Aborted, "failed"),
            (Status::Paused, "paused"),
        ] {
            let outcome = shape_result(&finished(status), "real");
            assert_eq!(text(&outcome, "status"), Some(label));
            assert_eq!(text(&outcome, "mode"), Some("real"));
        }
    }

    #[test]
    fn a_probe_that_saw_no_manifest_says_so_instead_of_claiming_real() {
        let probe = ModeProbe::default();
        assert_eq!(probe.label(), "unreported");
    }
}
