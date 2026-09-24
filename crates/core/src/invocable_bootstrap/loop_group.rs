use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use cronus_contract::{Binder, BinderKind, Invocable, Locus, Outcome, OutcomeValue, Stability};
use cronus_domain::invocable::{Dispatcher, InvocableRegistry, Registrant};
use cronus_domain::io_message;
use cronus_domain::loop_runner::{LoopOutcome, run_execution};

use crate::loop_bootstrap::{
    FileExistsBackend, file_exists_spec, read_report, read_spec, write_report, write_spec,
};

use super::{core_id, opt_text_arg, text_arg};

pub(super) fn register(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    register_run(registry, dispatcher);
    register_evolve(registry, dispatcher);
    register_log(registry, dispatcher);
    register_show(registry, dispatcher);
}

fn new_run_id() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("run-{secs}-{}", std::process::id())
}

fn register_run(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("loop.run");
    let invocable = Invocable {
        id: id.clone(),
        name: "Run",
        summary: "Run an execution loop over a unit until its oracle says done or the ceiling \
                   fires.",
        group: "loop",
        locus: Locus::Semantic,
        binders: vec![
            // `--file <path>`, not positional — the pre-migration grammar
            // declared it `#[arg(long)]`.
            Binder {
                name: "file",
                kind: BinderKind::NamedText,
                optional: false,
            },
            Binder {
                name: "max-iter",
                kind: BinderKind::NamedText,
                optional: true,
            },
        ],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:loop.run registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let file = std::path::PathBuf::from(text_arg(args, "file"));
            // `--max-iter` was a clap-level `value_parser!(u32)` before
            // this migration (a malformed value was a usage failure,
            // exit 2); as a `NamedText` it is hand-parsed here instead, so
            // a malformed value is now an application-level refusal
            // (exit 1) — the same disclosed usage→application shift
            // `activation enable --mode` already established, applied
            // here rather than invented under this task's own pressure.
            // The default (1) matches the pre-migration
            // `#[arg(default_value_t = 1)]`.
            let max_iter = match opt_text_arg(args, "max-iter") {
                None => 1,
                Some(s) => match s.parse::<u32>() {
                    Ok(n) => n,
                    Err(_) => {
                        return Outcome::Unavailable {
                            reason: format!("--max-iter must be a non-negative integer, got {s:?}"),
                        };
                    }
                },
            };

            let spec = file_exists_spec(max_iter);
            let mut backend = FileExistsBackend { target_path: file };
            let report = run_execution(&spec, &mut backend, "");
            let run_id = new_run_id();

            if let Err(e) = write_report(&run_id, &report) {
                return Outcome::Unavailable {
                    reason: format!("failed to persist ledger: {}", io_message::describe(&e)),
                };
            }
            if let Err(e) = write_spec(&run_id, &spec) {
                return Outcome::Unavailable {
                    reason: format!("failed to persist spec: {}", io_message::describe(&e)),
                };
            }

            match &report.outcome {
                LoopOutcome::Done(_) => Outcome::Value(OutcomeValue::Record(vec![
                    ("run_id".to_string(), OutcomeValue::Text(run_id)),
                    ("status".to_string(), OutcomeValue::Text("done".to_string())),
                    (
                        "iterations".to_string(),
                        OutcomeValue::Integer(report.iterations_run as i64),
                    ),
                ])),
                LoopOutcome::Stopped(reason) => Outcome::Value(OutcomeValue::Record(vec![
                    ("run_id".to_string(), OutcomeValue::Text(run_id)),
                    (
                        "status".to_string(),
                        OutcomeValue::Text("stopped".to_string()),
                    ),
                    ("reason".to_string(), OutcomeValue::Text(reason.clone())),
                ])),
            }
        }),
    );
}

fn register_evolve(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("loop.evolve");
    let invocable = Invocable {
        id: id.clone(),
        name: "Evolve",
        summary: "Run an evolution loop over a harness. Unavailable in this workspace: there is \
                   no CLI-nameable harness registry yet.",
        group: "loop",
        locus: Locus::Semantic,
        binders: vec![Binder {
            name: "harness_id",
            kind: BinderKind::Text,
            optional: false,
        }],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:loop.evolve registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|_args| {
            // Shipped-surface honesty: this workspace has no
            // CLI-nameable harness registry yet, so the command is present
            // but marked unavailable — never a silent "not implemented"
            // success stub. The domain-tier `run_evolution` exists and is
            // tested; wiring a real, CLI-nameable harness resource is
            // future work.
            Outcome::Unavailable {
                reason: "cronus loop evolve is unavailable: no harness registry exists in this \
                          workspace yet"
                    .to_string(),
            }
        }),
    );
}

fn register_log(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("loop.log");
    let invocable = Invocable {
        id: id.clone(),
        name: "Log",
        summary: "Inspect a run's mutation ledger.",
        group: "loop",
        locus: Locus::Semantic,
        binders: vec![Binder {
            name: "run_id",
            kind: BinderKind::Text,
            optional: false,
        }],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:loop.log registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let run_id = text_arg(args, "run_id");
            match read_report(run_id) {
                Ok(text) => Outcome::Value(OutcomeValue::Text(text)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Outcome::Unavailable {
                    reason: format!("run '{run_id}' not found"),
                },
                Err(e) => Outcome::Unavailable {
                    reason: io_message::describe(&e),
                },
            }
        }),
    );
}

fn register_show(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("loop.show");
    let invocable = Invocable {
        id: id.clone(),
        name: "Show",
        summary: "Show a run's manifest/oracle/ceiling.",
        group: "loop",
        locus: Locus::Semantic,
        binders: vec![Binder {
            name: "run_id",
            kind: BinderKind::Text,
            optional: false,
        }],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:loop.show registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let run_id = text_arg(args, "run_id");
            match read_spec(run_id) {
                Ok(text) => Outcome::Value(OutcomeValue::Text(text)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Outcome::Unavailable {
                    reason: format!("run '{run_id}' not found"),
                },
                Err(e) => Outcome::Unavailable {
                    reason: io_message::describe(&e),
                },
            }
        }),
    );
}
