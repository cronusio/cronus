//! Inspecting the workspace registry reads it and nothing else: with no
//! registry yet, `workspace list` and `workspace check` answer from that fact
//! and leave no file behind.
//!
//! The registry lives under the state tier, which `CRONUS_PORTABLE_DIR`
//! redirects; that variable is process-global, so the test takes a lock.

use cronus_contract::{
    ArgValue, ArgValues, Dispatched, InvocableId, Invocation, Outcome, OutcomeValue, Surface,
};
use cronus_core::Engine;
use cronus_core::invocable_bootstrap::bootstrap;
use cronus_core::paths::{Paths, Root};

static PORTABLE_DIR_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn dispatch(id: &str, args: ArgValues) -> Outcome {
    let (registry, dispatcher) = bootstrap(Engine::new());
    let invocation = Invocation {
        id: InvocableId::new(id).expect("well-formed invocable id"),
        args,
        caller: Surface::Cli,
    };
    match dispatcher.dispatch(&registry, &invocation) {
        Dispatched::Ran(outcome) => outcome,
        Dispatched::Unknown => panic!("expected {id} to resolve, got Unknown"),
    }
}

#[test]
fn listing_and_checking_with_no_registry_answer_without_creating_one() {
    let _env = PORTABLE_DIR_ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let base = std::env::temp_dir().join(format!("cronus-ws-read-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    // SAFETY: serialized by `PORTABLE_DIR_ENV_LOCK`.
    unsafe { std::env::set_var("CRONUS_PORTABLE_DIR", &base) };
    let registry_file = Paths::os_native()
        .resolve(Root::State)
        .join("workspaces.db");

    let listed = dispatch("core:workspace.list", ArgValues::new());
    let mut args = ArgValues::new();
    args.insert("id", ArgValue::Text("some-workspace".to_string()));
    let checked = dispatch("core:workspace.check", args);
    let created = registry_file.exists();

    unsafe { std::env::remove_var("CRONUS_PORTABLE_DIR") };
    let _ = std::fs::remove_dir_all(&base);

    assert_eq!(
        listed,
        Outcome::Value(OutcomeValue::List(Vec::new())),
        "no registry means no workspaces"
    );
    let Outcome::Value(OutcomeValue::Record(status)) = checked else {
        panic!("expected a status record, got {checked:?}");
    };
    assert!(
        status
            .iter()
            .all(|(_, value)| *value == OutcomeValue::Boolean(false)),
        "{status:?}"
    );
    assert!(
        !created,
        "inspecting must not bring a registry file into being"
    );
}
