use std::sync::Arc;

use cronus_contract::{Binder, BinderKind, Invocable, Locus, Outcome, Stability};
use cronus_domain::invocable::{Dispatcher, InvocableRegistry, Registrant};

use super::core_id;

/// `ExecWorkspaceManager` (`cronus_domain::exec_workspace`) is a real,
/// tested foundation — id validation, worktree directory lifecycle, the
/// no-remote-git contract — but it is in-memory-only by its own doc comment
/// ("SQLite-backed in production"); no manifest describing a workspace's
/// id/state/timestamps survives on disk, so nothing could reconstruct a
/// listing after the process that created it exits. Every verb here used
/// to construct a fresh manager per call and discard it on return — `create`
/// even wrote a real worktree directory to disk while doing so, an orphaned
/// side effect no later `list`/`finalize`/`discard` could ever see or clean
/// up. Shipped-surface honesty (matching `core:loop.evolve`'s own
/// precedent): present and documented, but answering `Unavailable` with the
/// reason, never a silent success stub — and never touching the filesystem
/// on a call that cannot actually be tracked afterward.
const EXEC_UNAVAILABLE: &str =
    "cronus exec is unavailable: no persistent exec-workspace store exists in this workspace yet";

pub(super) fn register(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    register_list(registry, dispatcher);
    register_create(registry, dispatcher);
    register_finalize(registry, dispatcher);
    register_discard(registry, dispatcher);
}

fn register_list(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("exec.list");
    let invocable = Invocable {
        id: id.clone(),
        name: "List",
        summary: "List execution workspaces. Unavailable in this workspace: no persistent \
                   exec-workspace store exists yet.",
        group: "exec",
        locus: Locus::Semantic,
        binders: Vec::new(),
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:exec.list registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|_args| Outcome::Unavailable {
            reason: EXEC_UNAVAILABLE.to_string(),
        }),
    );
}

fn register_create(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("exec.create");
    let invocable = Invocable {
        id: id.clone(),
        name: "Create",
        summary: "Create an execution workspace for a card. Unavailable in this workspace: no \
                   persistent exec-workspace store exists yet.",
        group: "exec",
        locus: Locus::Semantic,
        binders: vec![
            Binder {
                name: "ws_id",
                kind: BinderKind::Text,
                optional: false,
            },
            Binder {
                name: "card_id",
                kind: BinderKind::Text,
                optional: false,
            },
        ],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:exec.create registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|_args| Outcome::Unavailable {
            reason: EXEC_UNAVAILABLE.to_string(),
        }),
    );
}

fn register_finalize(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("exec.finalize");
    let invocable = Invocable {
        id: id.clone(),
        name: "Finalize",
        summary: "Finalize an execution workspace. Unavailable in this workspace: no persistent \
                   exec-workspace store exists yet.",
        group: "exec",
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
        .expect("core:exec.finalize registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|_args| Outcome::Unavailable {
            reason: EXEC_UNAVAILABLE.to_string(),
        }),
    );
}

fn register_discard(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("exec.discard");
    let invocable = Invocable {
        id: id.clone(),
        name: "Discard",
        summary: "Discard an execution workspace. Unavailable in this workspace: no persistent \
                   exec-workspace store exists yet.",
        group: "exec",
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
        .expect("core:exec.discard registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|_args| Outcome::Unavailable {
            reason: EXEC_UNAVAILABLE.to_string(),
        }),
    );
}
