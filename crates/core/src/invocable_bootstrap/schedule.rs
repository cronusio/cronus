use std::sync::Arc;

use cronus_contract::{Binder, BinderKind, Invocable, Locus, Outcome, OutcomeValue, Stability};
use cronus_domain::invocable::{Dispatcher, InvocableRegistry, Registrant};
use cronus_domain::scheduler::{RecurrencePreset, Schedule, ScheduleAction, Scheduler};

use super::{core_id, opt_text_arg, text_arg};

/// A recurring schedule automates a specific project's workflows — resolves
/// against the current workspace, same as every other project-scoped
/// semantic verb.
fn sched_dir() -> std::path::PathBuf {
    cronus_domain::paths::resolve_workspace_root().join("schedules")
}

fn open_scheduler() -> Result<Scheduler, String> {
    Scheduler::new(sched_dir()).map_err(|e| e.to_string())
}

pub(super) fn register(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    register_list(registry, dispatcher);
    register_add(registry, dispatcher);
    register_delete(registry, dispatcher);
    register_run(registry, dispatcher);
}

fn register_list(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("schedule.list");
    let invocable = Invocable {
        id: id.clone(),
        name: "List",
        summary: "List all schedules.",
        group: "schedule",
        locus: Locus::Semantic,
        binders: Vec::new(),
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:schedule.list registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|_args| {
            // A store-open failure and a genuinely empty schedule list both
            // render as an empty list here — the exact pre-existing
            // behavior (a known residual, recorded, not fixed under this
            // task's own pressure).
            let Ok(sched) = open_scheduler() else {
                return Outcome::Value(OutcomeValue::List(Vec::new()));
            };
            let items = sched.list().unwrap_or_default();
            Outcome::Value(OutcomeValue::List(
                items
                    .into_iter()
                    .map(|s| {
                        OutcomeValue::Record(vec![
                            ("id".to_string(), OutcomeValue::Text(s.id)),
                            ("name".to_string(), OutcomeValue::Text(s.name)),
                            (
                                "kind".to_string(),
                                OutcomeValue::Text(s.kind.as_str().to_string()),
                            ),
                        ])
                    })
                    .collect(),
            ))
        }),
    );
}

fn register_add(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("schedule.add");
    let invocable = Invocable {
        id: id.clone(),
        name: "Add",
        summary: "Add a recurring schedule.",
        group: "schedule",
        locus: Locus::Semantic,
        binders: vec![
            Binder {
                name: "id",
                kind: BinderKind::Text,
                optional: false,
            },
            Binder {
                name: "name",
                kind: BinderKind::Text,
                optional: false,
            },
            Binder {
                name: "preset",
                kind: BinderKind::NamedText,
                optional: true,
            },
        ],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:schedule.add registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let sched_id = text_arg(args, "id");
            let name = text_arg(args, "name");
            // The default `--preset` the pre-migration CLI's own
            // `#[arg(default_value = "daily")]` declared, and the same
            // silent-fallback-to-Daily behavior for an unrecognized value.
            let preset = match opt_text_arg(args, "preset").unwrap_or("daily") {
                "weekdays" => RecurrencePreset::Weekdays,
                "weekends" => RecurrencePreset::Weekends,
                _ => RecurrencePreset::Daily,
            };
            let sched = match open_scheduler() {
                Ok(s) => s,
                Err(e) => return Outcome::Unavailable { reason: e },
            };
            let s = Schedule::recurring(sched_id, name, &preset, ScheduleAction::Routine);
            match sched.add(&s) {
                Ok(()) => Outcome::Value(OutcomeValue::Record(vec![(
                    "id".to_string(),
                    OutcomeValue::Text(sched_id.to_string()),
                )])),
                Err(e) => Outcome::Unavailable {
                    reason: e.to_string(),
                },
            }
        }),
    );
}

fn register_delete(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("schedule.delete");
    let invocable = Invocable {
        id: id.clone(),
        name: "Delete",
        summary: "Delete a schedule.",
        group: "schedule",
        locus: Locus::Semantic,
        binders: vec![Binder {
            name: "id",
            kind: BinderKind::Text,
            optional: false,
        }],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry.register(&Registrant::core(), invocable).expect(
        "core:schedule.delete registers cleanly at bootstrap — a duplicate id here is a bug",
    );
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let sched_id = text_arg(args, "id");
            let sched = match open_scheduler() {
                Ok(s) => s,
                Err(e) => return Outcome::Unavailable { reason: e },
            };
            match sched.delete(sched_id) {
                Ok(()) => Outcome::Value(OutcomeValue::Record(vec![(
                    "id".to_string(),
                    OutcomeValue::Text(sched_id.to_string()),
                )])),
                Err(e) => Outcome::Unavailable {
                    reason: e.to_string(),
                },
            }
        }),
    );
}

fn register_run(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("schedule.run");
    let invocable = Invocable {
        id: id.clone(),
        name: "Run",
        summary: "Fire a schedule immediately.",
        group: "schedule",
        locus: Locus::Semantic,
        binders: vec![
            Binder {
                name: "id",
                kind: BinderKind::Text,
                optional: false,
            },
            Binder {
                name: "prompt",
                kind: BinderKind::NamedText,
                optional: true,
            },
        ],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:schedule.run registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let sched_id = text_arg(args, "id");
            // The default `--prompt` the pre-migration CLI's own
            // `#[arg(default_value = "run scheduled task")]` declared.
            let prompt = opt_text_arg(args, "prompt").unwrap_or("run scheduled task");
            let sched = match open_scheduler() {
                Ok(s) => s,
                Err(e) => return Outcome::Unavailable { reason: e },
            };
            match sched.fire(sched_id, prompt) {
                Ok(session) => Outcome::Value(OutcomeValue::Record(vec![(
                    "session_key".to_string(),
                    OutcomeValue::Text(session.session_key),
                )])),
                Err(e) => Outcome::Unavailable {
                    reason: e.to_string(),
                },
            }
        }),
    );
}
