use std::sync::Arc;

use cronus_contract::{Binder, BinderKind, Invocable, Locus, Outcome, OutcomeValue, Stability};
use cronus_domain::invocable::{Dispatcher, InvocableRegistry, Registrant};
use cronus_domain::roles::{PRESET_CATALOG, RoleManager};

use super::{core_id, flag_arg, opt_text_arg, text_arg};

/// A hired role instance staffs a specific project's active work — resolves
/// against the current workspace, same as every other project-scoped
/// semantic verb. The preset *catalog* (`PRESET_CATALOG`) stays a compiled-in
/// constant regardless — only hired instances live under this path.
fn state_dir() -> std::path::PathBuf {
    cronus_domain::paths::resolve_workspace_root()
}

fn open_manager() -> RoleManager {
    let state = state_dir();
    // `RoleManager`'s own methods already join "employees" onto
    // `state_dir` wherever they need that subdirectory (`hire`, `fire`,
    // `get`, …) — passing `state.join("employees")` here doubled it
    // (`employees/employees/<id>/`). `state_dir` is the workspace root
    // itself, same as every other manager this bootstrap layer opens.
    RoleManager::new(state.clone(), state)
}

pub(super) fn register(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    register_list(registry, dispatcher);
    register_hire(registry, dispatcher);
    register_show(registry, dispatcher);
    register_create(registry, dispatcher);
    register_fire(registry, dispatcher);
}

fn register_list(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("role.list");
    let invocable = Invocable {
        id: id.clone(),
        name: "List",
        summary: "List hired instances, or the preset catalog with --presets.",
        group: "role",
        locus: Locus::Semantic,
        binders: vec![Binder {
            name: "presets",
            kind: BinderKind::Flag,
            optional: true,
        }],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:role.list registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|args| {
            if flag_arg(args, "presets") {
                return Outcome::Value(OutcomeValue::List(
                    PRESET_CATALOG
                        .iter()
                        .map(|r| {
                            OutcomeValue::Record(vec![
                                ("id".to_string(), OutcomeValue::Text(r.id.to_string())),
                                ("name".to_string(), OutcomeValue::Text(r.name.to_string())),
                            ])
                        })
                        .collect(),
                ));
            }
            let mgr = open_manager();
            // A store failure and a genuinely empty roster render the same
            // way here — the exact pre-existing behavior (recorded
            // residual, not fixed under this task's pressure).
            let instances = mgr.list_hired().unwrap_or_default();
            Outcome::Value(OutcomeValue::List(
                instances
                    .into_iter()
                    .map(|inst| {
                        OutcomeValue::Record(vec![
                            ("id".to_string(), OutcomeValue::Text(inst.id)),
                            (
                                "display_name".to_string(),
                                OutcomeValue::Text(inst.display_name),
                            ),
                        ])
                    })
                    .collect(),
            ))
        }),
    );
}

fn register_hire(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("role.hire");
    let invocable = Invocable {
        id: id.clone(),
        name: "Hire",
        summary: "Hire a role from the preset catalog.",
        group: "role",
        locus: Locus::Semantic,
        binders: vec![
            Binder {
                name: "preset",
                kind: BinderKind::Text,
                optional: false,
            },
            Binder {
                name: "name",
                kind: BinderKind::NamedText,
                optional: true,
            },
        ],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:role.hire registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let preset = text_arg(args, "preset");
            let name = opt_text_arg(args, "name");
            let mgr = open_manager();
            match mgr.hire(preset, name) {
                Ok(inst) => Outcome::Value(OutcomeValue::Record(vec![
                    ("id".to_string(), OutcomeValue::Text(inst.id)),
                    (
                        "display_name".to_string(),
                        OutcomeValue::Text(inst.display_name),
                    ),
                ])),
                Err(e) => Outcome::Unavailable {
                    reason: e.to_string(),
                },
            }
        }),
    );
}

fn register_show(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("role.show");
    let invocable = Invocable {
        id: id.clone(),
        name: "Show",
        summary: "Show a hired role instance.",
        group: "role",
        locus: Locus::Semantic,
        binders: vec![Binder {
            name: "id",
            kind: BinderKind::Text,
            optional: false,
        }],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:role.show registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let role_id = text_arg(args, "id");
            let mgr = open_manager();
            match mgr.get(role_id) {
                Ok(Some(inst)) => Outcome::Value(OutcomeValue::Record(vec![
                    ("id".to_string(), OutcomeValue::Text(inst.id)),
                    (
                        "display_name".to_string(),
                        OutcomeValue::Text(inst.display_name),
                    ),
                ])),
                Ok(None) => Outcome::Unavailable {
                    reason: format!("hired instance not found: {role_id}"),
                },
                Err(e) => Outcome::Unavailable {
                    reason: e.to_string(),
                },
            }
        }),
    );
}

fn register_create(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("role.create");
    let invocable = Invocable {
        id: id.clone(),
        name: "Create",
        summary: "Create a custom role.",
        group: "role",
        locus: Locus::Semantic,
        binders: vec![
            Binder {
                name: "id",
                kind: BinderKind::Text,
                optional: false,
            },
            Binder {
                name: "display_name",
                kind: BinderKind::Text,
                optional: false,
            },
        ],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:role.create registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let role_id = text_arg(args, "id");
            let display_name = text_arg(args, "display_name");
            let mgr = open_manager();
            match mgr.create_custom(role_id, display_name) {
                Ok(inst) => Outcome::Value(OutcomeValue::Record(vec![
                    ("id".to_string(), OutcomeValue::Text(inst.id)),
                    (
                        "display_name".to_string(),
                        OutcomeValue::Text(inst.display_name),
                    ),
                ])),
                Err(e) => Outcome::Unavailable {
                    reason: e.to_string(),
                },
            }
        }),
    );
}

fn register_fire(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("role.fire");
    let invocable = Invocable {
        id: id.clone(),
        name: "Fire",
        summary: "Fire (remove) a hired role.",
        group: "role",
        locus: Locus::Semantic,
        binders: vec![Binder {
            name: "id",
            kind: BinderKind::Text,
            optional: false,
        }],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:role.fire registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let role_id = text_arg(args, "id");
            let mgr = open_manager();
            match mgr.fire(role_id) {
                Ok(()) => Outcome::Value(OutcomeValue::Record(vec![(
                    "id".to_string(),
                    OutcomeValue::Text(role_id.to_string()),
                )])),
                Err(e) => Outcome::Unavailable {
                    reason: e.to_string(),
                },
            }
        }),
    );
}
