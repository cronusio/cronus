//! `board.list` through the real dispatch path: each card arrives with what
//! it is, not only its identity and state, so a surface listing the board can
//! put a name on every card without asking for each one separately.
//!
//! The board lives in the workspace's state root, which `CRONUS_PORTABLE_DIR`
//! redirects; that variable is process-global, so the tests here take a lock.

use cronus_contract::{
    ArgValue, ArgValues, Dispatched, InvocableId, Invocation, Outcome, OutcomeValue, Surface,
};
use cronus_core::Engine;
use cronus_core::invocable_bootstrap::bootstrap;

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

fn text_args(pairs: &[(&str, &str)]) -> ArgValues {
    let mut args = ArgValues::new();
    for &(name, value) in pairs {
        args.insert(name, ArgValue::Text(value.to_string()));
    }
    args
}

fn field<'a>(fields: &'a [(String, OutcomeValue)], name: &str) -> Option<&'a OutcomeValue> {
    fields.iter().find(|(k, _)| k == name).map(|(_, v)| v)
}

#[test]
fn a_listed_card_carries_its_task_and_says_plainly_what_it_does_not_have() {
    let _env = PORTABLE_DIR_ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let base = std::env::temp_dir().join(format!("cronus-board-list-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    // SAFETY: serialized by `PORTABLE_DIR_ENV_LOCK`.
    unsafe { std::env::set_var("CRONUS_PORTABLE_DIR", &base) };

    let added = dispatch(
        "core:board.add",
        text_args(&[("id", "card-1"), ("task_ref", "write the proposal")]),
    );
    let listed = dispatch("core:board.list", ArgValues::new());

    unsafe { std::env::remove_var("CRONUS_PORTABLE_DIR") };
    let _ = std::fs::remove_dir_all(&base);

    assert!(matches!(added, Outcome::Value(_)), "{added:?}");
    let Outcome::Value(OutcomeValue::List(cards)) = listed else {
        panic!("expected a list of cards, got {listed:?}");
    };
    let [OutcomeValue::Record(card)] = cards.as_slice() else {
        panic!("expected exactly the one card added, got {cards:?}");
    };
    assert_eq!(
        field(card, "id"),
        Some(&OutcomeValue::Text("card-1".to_string()))
    );
    assert_eq!(
        field(card, "task_ref"),
        Some(&OutcomeValue::Text("write the proposal".to_string()))
    );
    assert_eq!(
        field(card, "priority"),
        Some(&OutcomeValue::Empty),
        "a card with no priority says so; it is not given an invented one"
    );
    assert_eq!(field(card, "assignee"), Some(&OutcomeValue::Empty));
}
