// Every handler that starts long-running work registers it before it starts,
// so a change that recomposes the office is refused while it runs and the work
// itself does not begin while such a change is applied. The way to see that
// from outside is the second half: with a change being applied, none of them
// starts.
//
// One test in its own test binary: the portable-directory variable is
// process-global.

use cronus_contract::{ArgValue, ArgValues, Dispatched, InvocableId, Invocation, Outcome, Surface};
use cronus_core::endpoint::worklock;

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
            caller: Surface::Cli,
        },
    ) {
        Dispatched::Ran(outcome) => dispatcher.settle(outcome),
        Dispatched::Unknown => panic!("{id} is not registered"),
    }
}

fn text(value: &str) -> ArgValue {
    ArgValue::Text(value.to_string())
}

#[test]
fn no_producer_of_work_starts_while_a_change_is_being_applied() {
    let base = std::env::temp_dir().join(format!("cronus-producers-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("creatable");
    // SAFETY: this binary holds exactly one test.
    unsafe { std::env::set_var("CRONUS_PORTABLE_DIR", &base) };
    let root = cronus_core::paths::resolve_workspace_root();
    assert!(root.starts_with(&base), "{root:?} is outside {base:?}");

    let (registry, dispatcher) =
        cronus_core::invocable_bootstrap::bootstrap(cronus_core::Engine::new());
    let workflow = base.join("w.nodus");
    std::fs::write(&workflow, "# not run\n").expect("writable");

    // A change is being applied over the state root.
    let change = worklock::exclusive(&root).expect("nothing is running");

    let attempts: [(&str, Vec<(&str, ArgValue)>); 4] = [
        (
            "core:session.say",
            vec![("text", text("hi")), ("model", text("m"))],
        ),
        (
            "core:workflow.run",
            vec![("file", text(&workflow.display().to_string()))],
        ),
        (
            "core:loop.run",
            vec![("file", text(&workflow.display().to_string()))],
        ),
        (
            "core:codegraph.index",
            vec![("path", text(&base.display().to_string()))],
        ),
    ];
    for (id, args) in &attempts {
        match dispatch(&registry, &dispatcher, id, args) {
            Outcome::Unavailable { reason } => assert!(
                reason.contains("installation change is being applied"),
                "{id}: {reason}"
            ),
            other => panic!("{id} began under a change: {other:?}"),
        }
    }

    // And once the change is done they are free to start again (they may then
    // fail for their own reasons, but not for this one).
    drop(change);
    for (id, args) in &attempts {
        if let Outcome::Unavailable { reason } = dispatch(&registry, &dispatcher, id, args) {
            assert!(
                !reason.contains("installation change is being applied"),
                "{id} is still held up: {reason}"
            );
        }
    }

    unsafe { std::env::remove_var("CRONUS_PORTABLE_DIR") };
    let _ = std::fs::remove_dir_all(&base);
}
