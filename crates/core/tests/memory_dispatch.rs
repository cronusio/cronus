//! `memory.store` / `memory.search` / `memory.forget` round-trip through the
//! real dispatch path.
//!
//! The memory verbs used to open a fresh `MemoryStore::open_in_memory()` per
//! dispatch, so a `store` wrote into a database that was dropped the instant
//! the handler returned and the id it handed back was unreachable by the next
//! `search`/`forget`. They now open one persistent database under the state
//! tier. Every test here redirects the state tier (`APPDATA`/`LOCALAPPDATA`,
//! which `Paths::os_native()` reads) to its own scratch directory —
//! `APPDATA`/`LOCALAPPDATA` are process-global, and `cargo test` runs a
//! binary's tests multi-threaded, so the tests in *this* file that set them
//! must not interleave (same pattern as `mission_mode.rs`'s
//! `CRONUS_MISSION_MODE` lock). This lock serializes them; each holds it
//! across its whole set-env→dispatch→remove-scratch-dir sequence.

use cronus_contract::{
    ArgValue, ArgValues, Dispatched, InvocableId, Outcome, OutcomeValue, Surface,
};
use cronus_core::Engine;
use cronus_core::invocable_bootstrap::bootstrap;

static STATE_TIER_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock_state_tier_env() -> std::sync::MutexGuard<'static, ()> {
    STATE_TIER_ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn dispatch(id: &str, args: ArgValues) -> Outcome {
    let (registry, dispatcher) = bootstrap(Engine::new());
    let invocation = cronus_contract::Invocation {
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
    let mut a = ArgValues::new();
    for &(k, v) in pairs {
        a.insert(k, ArgValue::Text(v.to_string()));
    }
    a
}

#[test]
fn store_search_forget_round_trip_persists_across_dispatches() {
    let _env = lock_state_tier_env();
    let root = std::env::temp_dir().join(format!(
        "cronus-mem-dispatch-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&root).unwrap();
    // SAFETY: serialized by the env lock (see module doc) — no concurrent
    // reader/writer of APPDATA/LOCALAPPDATA within this process.
    unsafe {
        std::env::set_var("APPDATA", &root);
        std::env::set_var("LOCALAPPDATA", &root);
    }

    // store
    let stored_id = match dispatch(
        "core:memory.store",
        text_args(&[("key", "roundtrip fact"), ("value", "the sky is blue")]),
    ) {
        Outcome::Value(OutcomeValue::Record(fields)) => match &fields[0].1 {
            OutcomeValue::Text(id) if !id.is_empty() => id.clone(),
            other => panic!("store returned a non-text / empty id: {other:?}"),
        },
        other => panic!("expected a store Record, got {other:?}"),
    };

    // search finds it — a *separate* dispatch, so this only passes if the
    // write actually hit disk.
    match dispatch("core:memory.search", text_args(&[("query", "roundtrip")])) {
        Outcome::Value(OutcomeValue::List(items)) => {
            let hit = items.iter().any(|item| {
                matches!(item, OutcomeValue::Record(f)
                    if f.iter().any(|(k, v)| k == "id"
                        && matches!(v, OutcomeValue::Text(t) if *t == stored_id)))
            });
            assert!(
                hit,
                "stored entry {stored_id} must be found by search: {items:?}"
            );
        }
        other => panic!("expected a List from search, got {other:?}"),
    }

    // a query that matches nothing is a zero-item List, not Empty
    match dispatch(
        "core:memory.search",
        text_args(&[("query", "no-such-term-xyzzy")]),
    ) {
        Outcome::Value(OutcomeValue::List(items)) => assert!(items.is_empty()),
        other => panic!("expected an empty List, got {other:?}"),
    }

    // forget the real entry -> Ok
    match dispatch("core:memory.forget", text_args(&[("id", &stored_id)])) {
        Outcome::Value(OutcomeValue::Record(fields)) => {
            assert!(matches!(&fields[0].1, OutcomeValue::Text(t) if *t == stored_id));
        }
        other => panic!("expected forget to return the removed id, got {other:?}"),
    }

    // forget an unknown id -> Unavailable naming it
    match dispatch("core:memory.forget", text_args(&[("id", "no-such-entry")])) {
        Outcome::Unavailable { reason } => {
            assert!(reason.contains("no-such-entry"), "reason: {reason}")
        }
        other => panic!("expected Unavailable naming the missing id, got {other:?}"),
    }

    let _ = std::fs::remove_dir_all(&root);
}

/// `memory.store`'s summary used to call this "key-value" storage,
/// implying a second `store` under the same key replaces the first —
/// `MemoryEntry` has no key field to be unique on, and every call adds a
/// genuinely new, separately timestamped entry. Locked in explicitly as the
/// deliberate behavior (a richer append-and-supersede history model, not a
/// mutable key-value map), not an untested accident.
#[test]
fn storing_the_same_key_twice_adds_two_distinct_entries_not_a_replacement() {
    let _env = lock_state_tier_env();
    let root = std::env::temp_dir().join(format!(
        "cronus-mem-dispatch-dup-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&root).unwrap();
    // SAFETY: serialized by the env lock (see module doc) — no concurrent
    // reader/writer of APPDATA/LOCALAPPDATA within this process.
    unsafe {
        std::env::set_var("APPDATA", &root);
        std::env::set_var("LOCALAPPDATA", &root);
    }

    let first_id = match dispatch(
        "core:memory.store",
        text_args(&[("key", "dup-key"), ("value", "first")]),
    ) {
        Outcome::Value(OutcomeValue::Record(fields)) => match &fields[0].1 {
            OutcomeValue::Text(id) => id.clone(),
            other => panic!("expected a text id, got {other:?}"),
        },
        other => panic!("expected a store Record, got {other:?}"),
    };
    let second_id = match dispatch(
        "core:memory.store",
        text_args(&[("key", "dup-key"), ("value", "second")]),
    ) {
        Outcome::Value(OutcomeValue::Record(fields)) => match &fields[0].1 {
            OutcomeValue::Text(id) => id.clone(),
            other => panic!("expected a text id, got {other:?}"),
        },
        other => panic!("expected a store Record, got {other:?}"),
    };

    assert_ne!(
        first_id, second_id,
        "a second store under the same key must not reuse or replace the first entry's id"
    );

    match dispatch("core:memory.search", text_args(&[("query", "dup-key")])) {
        Outcome::Value(OutcomeValue::List(items)) => {
            let ids: Vec<&str> = items
                .iter()
                .filter_map(|item| match item {
                    OutcomeValue::Record(f) => f.iter().find_map(|(k, v)| {
                        if k == "id" {
                            match v {
                                OutcomeValue::Text(t) => Some(t.as_str()),
                                _ => None,
                            }
                        } else {
                            None
                        }
                    }),
                    _ => None,
                })
                .collect();
            assert!(
                ids.contains(&first_id.as_str()) && ids.contains(&second_id.as_str()),
                "both entries must remain independently findable: {ids:?}"
            );
        }
        other => panic!("expected a List from search, got {other:?}"),
    }

    let _ = std::fs::remove_dir_all(&root);
}
