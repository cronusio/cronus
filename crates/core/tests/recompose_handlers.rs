// The handlers registered for the installation changes that recompose the
// office are gated by construction: whoever dispatches one — a terminal UI that
// asked first, a spoke forwarding to an engine, a surface that asked nothing —
// meets the work gate, because the gate is in the handler and not only in each
// surface's own earlier check.
//
// One test in its own test binary: the portable-directory variable is
// process-global.

use cronus_contract::{ArgValue, ArgValues, Dispatched, InvocableId, Invocation, Outcome, Surface};
use cronus_core::endpoint::ledger::WorkKind;
use cronus_core::endpoint::{begin_work, worklock};

fn dispatch(
    registry: &cronus_core::invocable::InvocableRegistry,
    dispatcher: &cronus_core::invocable::Dispatcher,
    id: &str,
    args: &[(&str, ArgValue)],
) -> Outcome {
    let mut values = ArgValues::new();
    for (name, value) in args {
        values.insert(*name, value.clone());
    }
    match dispatcher.dispatch(
        registry,
        &Invocation {
            id: InvocableId::new(id).expect("well-formed"),
            args: values,
            caller: Surface::Tui,
        },
    ) {
        Dispatched::Ran(outcome) => outcome,
        Dispatched::Unknown => panic!("{id} is not registered"),
    }
}

fn text(value: &str) -> ArgValue {
    ArgValue::Text(value.to_string())
}

#[test]
fn every_recomposing_handler_waits_for_running_work_and_names_it() {
    let base = std::env::temp_dir().join(format!("cronus-handlers-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("creatable");
    // SAFETY: this binary holds exactly one test.
    unsafe { std::env::set_var("CRONUS_PORTABLE_DIR", &base) };
    let root = cronus_core::paths::resolve_workspace_root();

    let (registry, dispatcher) =
        cronus_core::invocable_bootstrap::bootstrap(cronus_core::Engine::new());

    let changes: [(&str, Vec<(&str, ArgValue)>); 9] = [
        ("core:registry.enable", vec![("name", text("plan"))]),
        ("core:registry.disable", vec![("name", text("plan"))]),
        ("core:archetype.set", vec![("clear", ArgValue::Flag)]),
        (
            "core:ext.activate",
            vec![("id", text("demo/x")), ("yes", ArgValue::Flag)],
        ),
        ("core:ext.deactivate", vec![("id", text("demo/x"))]),
        ("core:restore", vec![("backup", text("backup-1"))]),
        ("core:dev.admit", vec![]),
        ("core:dev.revoke", vec![]),
        ("core:doctor", vec![("fix", ArgValue::Flag)]),
    ];

    // Work is running over the state root, and is named.
    let running = begin_work(&root, WorkKind::AgentTurn, "s-7").expect("work begins");
    for (id, args) in &changes {
        match dispatch(&registry, &dispatcher, id, args) {
            Outcome::Unavailable { reason } => {
                assert!(reason.starts_with("not applied —"), "{id}: {reason}");
                assert!(reason.contains("an agent turn (s-7)"), "{id}: {reason}");
            }
            other => panic!("{id} was applied over running work: {other:?}"),
        }
    }
    drop(running);

    // With a change already being applied, none of them starts a second.
    let applying = worklock::exclusive(&root).expect("nothing is running");
    for (id, args) in &changes {
        match dispatch(&registry, &dispatcher, id, args) {
            Outcome::Unavailable { reason } => {
                assert!(reason.starts_with("not applied —"), "{id}: {reason}");
            }
            other => panic!("{id} was applied during another change: {other:?}"),
        }
    }
    drop(applying);

    // With nothing running they are not held up by the gate (they may answer for
    // their own reasons — a backup that is not there, an extension nobody added).
    for (id, args) in &changes {
        if let Outcome::Unavailable { reason } = dispatch(&registry, &dispatcher, id, args) {
            assert!(
                !reason.starts_with("not applied —"),
                "{id} is still held up with nothing running: {reason}"
            );
        }
    }

    unsafe { std::env::remove_var("CRONUS_PORTABLE_DIR") };
    let _ = std::fs::remove_dir_all(&base);
}
