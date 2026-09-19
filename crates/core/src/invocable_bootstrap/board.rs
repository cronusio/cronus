use std::sync::Arc;

use cronus_contract::{Binder, BinderKind, Invocable, Locus, Outcome, OutcomeValue, Stability};
use cronus_domain::invocable::{Dispatcher, InvocableRegistry, Registrant};
use cronus_domain::kanban::{Board, CardState};
use cronus_domain::tool_security::now_ms;

use super::{core_id, flag_arg, opt_text_arg, text_arg};

/// A card belongs to a specific project's work, not the machine — resolves
/// against the current workspace (F-02), same as every other project-scoped
/// semantic verb.
fn board_path() -> std::path::PathBuf {
    cronus_domain::paths::resolve_workspace_root().join("kanban")
}

fn open_board() -> Board {
    Board::new(board_path())
}

pub(super) fn register(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    register_list(registry, dispatcher);
    register_show(registry, dispatcher);
    register_add(registry, dispatcher);
    register_move(registry, dispatcher);
    register_block(registry, dispatcher);
    register_done(registry, dispatcher);
    register_archive(registry, dispatcher);
}

fn register_list(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("board.list");
    let invocable = Invocable {
        id: id.clone(),
        name: "List",
        summary: "List board cards, or the archived ones with --archived.",
        group: "board",
        locus: Locus::Semantic,
        binders: vec![Binder {
            name: "archived",
            kind: BinderKind::Flag,
            optional: true,
        }],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:board.list registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let board = open_board();
            let cards = if flag_arg(args, "archived") {
                match board.list_archived_cards() {
                    Ok(cards) => cards,
                    Err(e) => {
                        return Outcome::Unavailable {
                            reason: e.to_string(),
                        };
                    }
                }
            } else {
                // A store failure and a genuinely empty board render the same
                // way here — the exact pre-existing behavior.
                board.list_cards().unwrap_or_default()
            };
            Outcome::Value(OutcomeValue::List(
                cards
                    .into_iter()
                    .map(|c| {
                        OutcomeValue::Record(vec![
                            ("id".to_string(), OutcomeValue::Text(c.id)),
                            (
                                "state".to_string(),
                                OutcomeValue::Text(c.state.as_str().to_string()),
                            ),
                        ])
                    })
                    .collect(),
            ))
        }),
    );
}

fn register_show(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("board.show");
    let invocable = Invocable {
        id: id.clone(),
        name: "Show",
        summary: "Show a card.",
        group: "board",
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
        .expect("core:board.show registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let card_id = text_arg(args, "id");
            let board = open_board();
            match board.get_card(card_id) {
                // F-24: `board show` used to render exactly what `board list`
                // already does (id + state) — no reason a detail view for
                // one card should hold less than the card itself carries.
                Ok(Some(c)) => Outcome::Value(OutcomeValue::Record(vec![
                    ("id".to_string(), OutcomeValue::Text(c.id)),
                    (
                        "state".to_string(),
                        OutcomeValue::Text(c.state.as_str().to_string()),
                    ),
                    ("task_ref".to_string(), OutcomeValue::Text(c.task_ref)),
                    (
                        "reason".to_string(),
                        match c.reason {
                            Some(r) => OutcomeValue::Text(r),
                            None => OutcomeValue::Empty,
                        },
                    ),
                    (
                        "assignee".to_string(),
                        match c.assignee {
                            Some(a) => OutcomeValue::Text(a),
                            None => OutcomeValue::Empty,
                        },
                    ),
                    (
                        "priority".to_string(),
                        match c.priority {
                            Some(p) => OutcomeValue::Text(p.as_str().to_string()),
                            None => OutcomeValue::Empty,
                        },
                    ),
                    (
                        "created_at".to_string(),
                        OutcomeValue::Integer(c.created_at as i64),
                    ),
                    (
                        "updated_at".to_string(),
                        OutcomeValue::Integer(c.updated_at as i64),
                    ),
                    (
                        "history".to_string(),
                        OutcomeValue::List(
                            c.history
                                .into_iter()
                                .map(|t| {
                                    OutcomeValue::Record(vec![
                                        (
                                            "from".to_string(),
                                            OutcomeValue::Text(t.from.as_str().to_string()),
                                        ),
                                        (
                                            "to".to_string(),
                                            OutcomeValue::Text(t.to.as_str().to_string()),
                                        ),
                                        ("actor".to_string(), OutcomeValue::Text(t.actor)),
                                        ("at".to_string(), OutcomeValue::Integer(t.at as i64)),
                                        (
                                            "reason".to_string(),
                                            match t.reason {
                                                Some(r) => OutcomeValue::Text(r),
                                                None => OutcomeValue::Empty,
                                            },
                                        ),
                                    ])
                                })
                                .collect(),
                        ),
                    ),
                ])),
                Ok(None) => Outcome::Unavailable {
                    reason: format!("card not found: {card_id}"),
                },
                Err(e) => Outcome::Unavailable {
                    reason: e.to_string(),
                },
            }
        }),
    );
}

fn register_add(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("board.add");
    let invocable = Invocable {
        id: id.clone(),
        name: "Add",
        summary: "Add a card.",
        group: "board",
        locus: Locus::Semantic,
        binders: vec![
            Binder {
                name: "id",
                kind: BinderKind::Text,
                optional: false,
            },
            Binder {
                name: "task_ref",
                kind: BinderKind::Text,
                optional: false,
            },
        ],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:board.add registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let card_id = text_arg(args, "id");
            let task_ref = text_arg(args, "task_ref");
            let board = open_board();
            match board.add_card(card_id, task_ref, now_ms()) {
                Ok(card) => Outcome::Value(OutcomeValue::Record(vec![(
                    "id".to_string(),
                    OutcomeValue::Text(card.id),
                )])),
                Err(e) => Outcome::Unavailable {
                    reason: e.to_string(),
                },
            }
        }),
    );
}

fn register_move(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("board.move");
    let invocable = Invocable {
        id: id.clone(),
        name: "Move",
        summary: "Move a card to a new state (triage, todo, ready, running, blocked, done).",
        group: "board",
        locus: Locus::Semantic,
        binders: vec![
            Binder {
                name: "id",
                kind: BinderKind::Text,
                optional: false,
            },
            Binder {
                name: "state",
                kind: BinderKind::EnumText(CardState::NAMES),
                optional: false,
            },
            Binder {
                name: "actor",
                kind: BinderKind::NamedText,
                optional: true,
            },
        ],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:board.move registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let card_id = text_arg(args, "id");
            let state_str = text_arg(args, "state");
            // The default `--actor` the pre-migration CLI's own
            // `#[arg(default_value = "cli")]` declared.
            let actor = opt_text_arg(args, "actor").unwrap_or("cli");
            let board = open_board();
            let to = match CardState::parse(state_str) {
                Some(s) => s,
                None => {
                    return Outcome::Unavailable {
                        reason: format!(
                            "unknown state '{state_str}' (valid: {})",
                            CardState::NAMES.join(", ")
                        ),
                    };
                }
            };
            match board.move_card(card_id, to, actor, None, now_ms()) {
                Ok(card) => Outcome::Value(OutcomeValue::Record(vec![
                    ("id".to_string(), OutcomeValue::Text(card.id)),
                    (
                        "state".to_string(),
                        OutcomeValue::Text(card.state.as_str().to_string()),
                    ),
                ])),
                Err(e) => Outcome::Unavailable {
                    reason: e.to_string(),
                },
            }
        }),
    );
}

fn register_block(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("board.block");
    let invocable = Invocable {
        id: id.clone(),
        name: "Block",
        summary: "Block a card with a reason.",
        group: "board",
        locus: Locus::Semantic,
        binders: vec![
            Binder {
                name: "id",
                kind: BinderKind::Text,
                optional: false,
            },
            Binder {
                name: "reason",
                kind: BinderKind::Text,
                optional: false,
            },
        ],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:board.block registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let card_id = text_arg(args, "id");
            let reason = text_arg(args, "reason");
            let board = open_board();
            match board.move_card(
                card_id,
                CardState::Blocked,
                "cli",
                Some(reason.to_string()),
                now_ms(),
            ) {
                Ok(card) => Outcome::Value(OutcomeValue::Record(vec![(
                    "id".to_string(),
                    OutcomeValue::Text(card.id),
                )])),
                Err(e) => Outcome::Unavailable {
                    reason: e.to_string(),
                },
            }
        }),
    );
}

fn register_done(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("board.done");
    let invocable = Invocable {
        id: id.clone(),
        name: "Done",
        summary: "Mark a card as done.",
        group: "board",
        locus: Locus::Semantic,
        binders: vec![
            Binder {
                name: "id",
                kind: BinderKind::Text,
                optional: false,
            },
            Binder {
                name: "actor",
                kind: BinderKind::NamedText,
                optional: true,
            },
        ],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:board.done registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let card_id = text_arg(args, "id");
            let actor = opt_text_arg(args, "actor").unwrap_or("cli");
            let board = open_board();
            match board.move_card(card_id, CardState::Done, actor, None, now_ms()) {
                Ok(card) => Outcome::Value(OutcomeValue::Record(vec![(
                    "id".to_string(),
                    OutcomeValue::Text(card.id),
                )])),
                Err(e) => Outcome::Unavailable {
                    reason: e.to_string(),
                },
            }
        }),
    );
}

fn register_archive(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("board.archive");
    let invocable = Invocable {
        id: id.clone(),
        name: "Archive",
        summary: "Archive all eligible done cards.",
        group: "board",
        locus: Locus::Semantic,
        binders: Vec::new(),
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:board.archive registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|_args| {
            let board = open_board();
            match board.archive_done_cards() {
                Ok(n) => Outcome::Value(OutcomeValue::Record(vec![(
                    "count".to_string(),
                    OutcomeValue::Integer(n as i64),
                )])),
                Err(e) => Outcome::Unavailable {
                    reason: e.to_string(),
                },
            }
        }),
    );
}
