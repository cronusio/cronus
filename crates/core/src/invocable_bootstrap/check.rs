use std::sync::Arc;

use cronus_contract::{Binder, BinderKind, Invocable, Locus, Outcome, OutcomeValue, Stability};
use cronus_domain::invocable::{Dispatcher, InvocableRegistry, Registrant};
use cronus_domain::quality::{GateResultStore, detect_language};

use super::{core_id, opt_text_arg, text_arg};

pub(super) fn register(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    register_run(registry, dispatcher);
    register_show(registry, dispatcher);
    register_history(registry, dispatcher);
}

fn register_run(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("check.run");
    let invocable = Invocable {
        id: id.clone(),
        name: "Run",
        summary: "Run quality gates for a card.",
        group: "check",
        locus: Locus::Semantic,
        binders: vec![
            Binder {
                name: "card_id",
                kind: BinderKind::Text,
                optional: false,
            },
            Binder {
                name: "path",
                kind: BinderKind::NamedText,
                optional: true,
            },
        ],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:check.run registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let card_id = text_arg(args, "card_id");
            let root = opt_text_arg(args, "path")
                .map(|path| crate::paths::from_caller(std::path::Path::new(path)))
                .unwrap_or_else(crate::paths::caller_dir);
            let lang = detect_language(&root);
            // The gate-runner seam itself — no tools invoked yet, matching
            // the exact pre-existing behavior.
            Outcome::Value(OutcomeValue::Record(vec![
                ("card".to_string(), OutcomeValue::Text(card_id.to_string())),
                (
                    "language".to_string(),
                    OutcomeValue::Text(lang.as_str().to_string()),
                ),
            ]))
        }),
    );
}

fn register_show(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("check.show");
    let invocable = Invocable {
        id: id.clone(),
        name: "Show",
        summary: "Show gate results for a card.",
        group: "check",
        locus: Locus::Semantic,
        binders: vec![Binder {
            name: "card_id",
            kind: BinderKind::Text,
            optional: false,
        }],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:check.show registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let card_id = text_arg(args, "card_id");
            // `GateResultStore::new()` is in-memory — the same ephemeral-
            // per-call shape `memory`/`codegraph`/`exec` already carry,
            // preserved exactly rather than fixed here.
            let store = GateResultStore::new();
            Outcome::Value(OutcomeValue::List(
                store
                    .results_for(card_id)
                    .into_iter()
                    .map(|r| {
                        OutcomeValue::Record(vec![
                            (
                                "gate".to_string(),
                                OutcomeValue::Text(r.gate.as_str().to_string()),
                            ),
                            (
                                "status".to_string(),
                                OutcomeValue::Text(r.status.as_str().to_string()),
                            ),
                        ])
                    })
                    .collect(),
            ))
        }),
    );
}

fn register_history(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("check.history");
    let invocable = Invocable {
        id: id.clone(),
        name: "History",
        summary: "Show gate result history for a card.",
        group: "check",
        locus: Locus::Semantic,
        binders: vec![Binder {
            name: "card_id",
            kind: BinderKind::Text,
            optional: false,
        }],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:check.history registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let card_id = text_arg(args, "card_id");
            let store = GateResultStore::new();
            let count = store.results_for(card_id).len();
            Outcome::Value(OutcomeValue::Record(vec![
                ("card".to_string(), OutcomeValue::Text(card_id.to_string())),
                ("count".to_string(), OutcomeValue::Integer(count as i64)),
            ]))
        }),
    );
}
