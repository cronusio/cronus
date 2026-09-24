use std::sync::Arc;

use cronus_contract::{Binder, BinderKind, Invocable, Locus, Outcome, Stability};
use cronus_domain::invocable::{Dispatcher, InvocableRegistry, Registrant};

use super::core_id;

/// `BudgetEngine` (`cronus_domain::budget`) is a real, tested foundation —
/// policy CRUD, cost ingestion, hard-stop enforcement — but it is
/// in-memory-only by its own doc comment ("SQLite-backed in production");
/// no persistent store or kanban-seam wiring exists yet. Every verb here
/// used to construct a fresh engine per call and discard it on return,
/// so `set` followed by `show` in the next invocation always reported the
/// engine's own zero default — a command that *looked* wired reporting
/// nothing was ever recorded. Shipped-surface honesty (matching
/// `core:loop.evolve`'s own precedent): present and documented, but
/// answering `Unavailable` with the reason, never a silent success stub.
const BUDGET_UNAVAILABLE: &str =
    "cronus budget is unavailable: no persistent budget store exists in this workspace yet";

pub(super) fn register(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    register_show(registry, dispatcher);
    register_set(registry, dispatcher);
    register_reset(registry, dispatcher);
}

fn register_show(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("budget.show");
    let invocable = Invocable {
        id: id.clone(),
        name: "Show",
        summary: "Show current budget usage. Unavailable in this workspace: no persistent \
                   budget store exists yet.",
        group: "budget",
        locus: Locus::Semantic,
        binders: Vec::new(),
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:budget.show registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|_args| Outcome::Unavailable {
            reason: BUDGET_UNAVAILABLE.to_string(),
        }),
    );
}

fn register_set(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("budget.set");
    let invocable = Invocable {
        id: id.clone(),
        name: "Set",
        summary: "Set a workspace budget limit (USD). Unavailable in this workspace: no \
                   persistent budget store exists yet.",
        group: "budget",
        locus: Locus::Semantic,
        binders: vec![Binder {
            name: "limit",
            kind: BinderKind::Float,
            optional: false,
        }],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:budget.set registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|_args| Outcome::Unavailable {
            reason: BUDGET_UNAVAILABLE.to_string(),
        }),
    );
}

fn register_reset(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("budget.reset");
    let invocable = Invocable {
        id: id.clone(),
        name: "Reset",
        summary: "Reset budget counters. Unavailable in this workspace: no persistent budget \
                   store exists yet.",
        group: "budget",
        locus: Locus::Semantic,
        binders: Vec::new(),
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:budget.reset registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|_args| Outcome::Unavailable {
            reason: BUDGET_UNAVAILABLE.to_string(),
        }),
    );
}
