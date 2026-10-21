use std::sync::Arc;

use cronus_contract::{Invocable, Locus, Outcome, OutcomeValue, Stability};
use cronus_domain::constitution::{IDENTITY_FILES, identity_paths};
use cronus_domain::invocable::{Dispatcher, InvocableRegistry, Registrant};

use super::core_id;

pub(super) fn register(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    register_constitution(registry, dispatcher);
    register_status(registry, dispatcher);
}

fn register_constitution(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("agent.constitution");
    let invocable = Invocable {
        id: id.clone(),
        name: "Constitution",
        summary: "Show agent identity files and their presence on disk.",
        group: "agent",
        locus: Locus::Semantic,
        binders: Vec::new(),
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry.register(&Registrant::core(), invocable).expect(
        "core:agent.constitution registers cleanly at bootstrap — a duplicate id here is a bug",
    );
    dispatcher.attach(
        id,
        Arc::new(|_args| {
            let workspace = crate::paths::caller_dir();
            let paths = identity_paths(&workspace);
            Outcome::Value(OutcomeValue::List(
                IDENTITY_FILES
                    .iter()
                    .zip(paths.iter())
                    .map(|(name, path)| {
                        OutcomeValue::Record(vec![
                            ("file".to_string(), OutcomeValue::Text((*name).to_string())),
                            (
                                "path".to_string(),
                                OutcomeValue::Text(path.display().to_string()),
                            ),
                            ("exists".to_string(), OutcomeValue::Boolean(path.exists())),
                        ])
                    })
                    .collect(),
            ))
        }),
    );
}

fn register_status(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("agent.status");
    let invocable = Invocable {
        id: id.clone(),
        name: "Status",
        summary: "Show agent runtime status.",
        group: "agent",
        locus: Locus::Semantic,
        binders: Vec::new(),
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:agent.status registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|_args| {
            // No agent session concept exists on this surface yet — the
            // exact pre-existing "no active session" fact, not a stub
            // pretending to have more to say.
            Outcome::Value(OutcomeValue::Text("no active session".to_string()))
        }),
    );
}
