//! Facade-level invariant sweep for the invocable registry bootstrap
//! (EP-12, INV-7) — exercised through the real facade export chain
//! (`cronus_core::invocable_bootstrap::bootstrap`, `cronus_core::invocable::*`),
//! not a stand-in.

use cronus_contract::{
    Dispatched, Invocable, InvocableId, Locus, Outcome, OutcomeValue, Stability,
};
use cronus_core::Engine;
use cronus_core::invocable::{CONTRIBUTE_GRANT, Dispatcher, InvocableRegistry, Registrant};
use cronus_core::invocable_bootstrap::bootstrap;

/// EP-12: core invocables are registered through the same public door a
/// contribution uses — proven by registering a real contribution into the
/// exact registry `bootstrap` populated, alongside the core invocables it
/// already holds, with no separate API involved.
#[test]
fn core_invocables_and_a_contribution_share_one_registration_door() {
    let (mut registry, _dispatcher) = bootstrap(Engine::new());

    // `core:status` is deliberately not asserted here any more: it moved to
    // the CLI frontend's own installation-grammar declaration (installation
    // verbs are declared by the frontend that owns them, not the shared
    // facade), so the facade's own `bootstrap` no longer registers it.
    // `core:version` alone still proves this test's actual point.
    assert!(
        registry
            .resolve(&InvocableId::new("core:version").expect("well-formed invocable id"))
            .is_found(),
        "bootstrap must register core:version"
    );

    let contribution = Invocable {
        id: InvocableId::new("myext:hello").expect("well-formed invocable id"),
        name: "Hello",
        summary: "A contributed invocable proving the shared door.",
        group: "test",
        locus: Locus::Semantic,
        binders: Vec::new(),
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    let registrant = Registrant::extension("myext", "src-1").with_grant(CONTRIBUTE_GRANT);

    // The SAME `register` the bootstrap used for "core:version" accepts
    // this contribution — no privileged, core-only path exists.
    assert!(registry.register(&registrant, contribution).is_ok());
    assert!(
        registry
            .resolve(&InvocableId::new("myext:hello").expect("well-formed invocable id"))
            .is_found()
    );
}

/// INV-7: dispatch output is masked against configured secrets — the single
/// boundary point, proven directly on the real `Dispatcher` rather than a
/// stand-in redaction call.
#[test]
fn dispatch_output_passes_through_the_shared_redaction_path() {
    let mut registry = InvocableRegistry::new();
    let mut dispatcher = Dispatcher::new();

    let id = InvocableId::new("core:echo-secret").expect("well-formed invocable id");
    registry
        .register(
            &Registrant::core(),
            Invocable {
                id: id.clone(),
                name: "Echo secret",
                summary: "Returns a value containing a known secret, for the redaction test.",
                group: "test",
                locus: Locus::Semantic,
                binders: Vec::new(),
                stability: Stability::Shipped,
                journal_raw_input: true,
            },
        )
        .unwrap();
    dispatcher.attach(
        id.clone(),
        std::sync::Arc::new(|_args| {
            Outcome::Value(OutcomeValue::Text("token=sk-LIVE-777 ready".to_string()))
        }),
    );
    dispatcher.set_secrets(vec!["sk-LIVE-777".to_string()]);

    let invocation = cronus_contract::Invocation {
        id,
        args: cronus_contract::ArgValues::new(),
        caller: cronus_contract::Surface::Cli,
    };
    let dispatched = dispatcher.dispatch(&registry, &invocation);

    match dispatched {
        Dispatched::Ran(Outcome::Value(OutcomeValue::Text(text))) => {
            assert!(!text.contains("sk-LIVE-777"), "secret must be masked");
            assert!(text.contains("***"), "masked output must show the mask");
        }
        other => panic!("expected Ran(Value(Text)), got {other:?}"),
    }
}

/// The `memory` verbs resolve and ship. Their store/search/forget round-trip
/// lives in its own test binary (`tests/memory_dispatch.rs`): those handlers
/// now open one persistent database under the state tier, so that test
/// redirects `APPDATA`/`LOCALAPPDATA`, and a process-global env var must not be
/// mutated from a test sharing its process with the disk-touching `exec` /
/// `check` dispatch checks below.
#[test]
fn memory_verbs_are_registered_and_resolve() {
    let (registry, _dispatcher) = bootstrap(Engine::new());
    for id in [
        "core:memory.store",
        "core:memory.search",
        "core:memory.forget",
    ] {
        assert!(
            registry
                .all()
                .any(|i| i.id.as_str() == id && matches!(i.stability, Stability::Shipped)),
            "{id} must be registered and shipped"
        );
    }
}

/// A small helper: dispatch `id` with `args` through a freshly bootstrapped
/// facade, unwrapping the `Ran` case — every test below expects the
/// invocable to resolve, so an `Unknown` here is the test's own setup being
/// wrong, not a case it means to exercise.
fn ran(id: &str, args: cronus_contract::ArgValues) -> Outcome {
    let (registry, dispatcher) = bootstrap(Engine::new());
    let invocation = cronus_contract::Invocation {
        id: InvocableId::new(id).expect("well-formed invocable id"),
        args,
        caller: cronus_contract::Surface::Cli,
    };
    match dispatcher.dispatch(&registry, &invocation) {
        Dispatched::Ran(outcome) => outcome,
        Dispatched::Unknown => panic!("expected {id} to resolve, got Unknown"),
    }
}

/// The second wave of real semantic migrations: `codegraph`, `agent`,
/// `exec`, `check`, `learn` — six groups needing no `Binder` extension,
/// migrated onto the mechanism exactly as built. `role`'s write verbs
/// (`hire`/`create`/`fire`) are deliberately not covered here: their
/// handler resolves the real, un-overridable OS state directory (the same
/// path the pre-migration CLI code already used), and this file's other
/// tests never touch real disk — extending that scope for `role` alone
/// would be new coverage this migration does not need to add, not a gap it
/// leaves behind (the pre-migration test suite never covered `role hire`
/// end-to-end either).
#[test]
fn codegraph_search_on_an_empty_index_is_a_zero_item_list_not_an_empty_result() {
    let mut args = cronus_contract::ArgValues::new();
    args.insert(
        "query",
        cronus_contract::ArgValue::Text("nonexistent".to_string()),
    );
    match ran("core:codegraph.search", args) {
        Outcome::Value(OutcomeValue::List(items)) => assert!(items.is_empty()),
        other => panic!("expected an empty List, not Empty or anything else, got {other:?}"),
    }
}

#[test]
fn agent_constitution_lists_every_identity_file_with_its_existence_bit() {
    match ran("core:agent.constitution", cronus_contract::ArgValues::new()) {
        Outcome::Value(OutcomeValue::List(items)) => {
            assert!(
                !items.is_empty(),
                "the identity file list must be non-empty"
            );
            for item in &items {
                match item {
                    OutcomeValue::Record(fields) => {
                        let names: Vec<&str> =
                            fields.iter().map(|(name, _)| name.as_str()).collect();
                        assert!(names.contains(&"file"));
                        assert!(names.contains(&"path"));
                        assert!(names.contains(&"exists"));
                    }
                    other => panic!("expected a Record per identity file, got {other:?}"),
                }
            }
        }
        other => panic!("expected a List of identity-file records, got {other:?}"),
    }
}

#[test]
fn agent_status_reports_no_active_session() {
    match ran("core:agent.status", cronus_contract::ArgValues::new()) {
        Outcome::Value(OutcomeValue::Text(text)) => assert_eq!(text, "no active session"),
        other => panic!("expected Text(\"no active session\"), got {other:?}"),
    }
}

/// `exec` has no persistent store yet: every verb answers
/// `Unavailable` honestly (INV-9, matching `core:loop.evolve`'s own
/// precedent) rather than a `List([])` that implies a real, empty listing.
#[test]
fn exec_list_is_unavailable_not_a_silent_empty_success() {
    match ran("core:exec.list", cronus_contract::ArgValues::new()) {
        Outcome::Unavailable { reason } => assert!(reason.contains("exec-workspace")),
        other => panic!("expected Unavailable naming why, got {other:?}"),
    }
}

#[test]
fn check_run_reports_the_detected_language_for_the_given_path() {
    let dir = std::env::temp_dir().join(format!(
        "cronus-invariants-check-rust-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();

    let mut args = cronus_contract::ArgValues::new();
    args.insert(
        "card_id",
        cronus_contract::ArgValue::Text("smoke-card".to_string()),
    );
    args.insert(
        "path",
        cronus_contract::ArgValue::Text(dir.display().to_string()),
    );
    match ran("core:check.run", args) {
        Outcome::Value(OutcomeValue::Record(fields)) => {
            let get = |key: &str| {
                fields
                    .iter()
                    .find(|(name, _)| name == key)
                    .and_then(|(_, v)| match v {
                        OutcomeValue::Text(s) => Some(s.as_str()),
                        _ => None,
                    })
            };
            assert_eq!(get("card"), Some("smoke-card"));
            assert_eq!(get("language"), Some("rust"));
        }
        other => panic!("expected a {{card, language}} Record, got {other:?}"),
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn learn_approve_of_an_unknown_id_is_unavailable_not_a_silent_success() {
    let mut args = cronus_contract::ArgValues::new();
    args.insert(
        "id",
        cronus_contract::ArgValue::Text("no-such-proposal".to_string()),
    );
    match ran("core:learn.approve", args) {
        Outcome::Unavailable { reason } => assert!(reason.contains("no-such-proposal")),
        other => panic!("expected Unavailable naming the missing id, got {other:?}"),
    }
}
