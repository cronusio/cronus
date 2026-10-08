use std::sync::Arc;

use cronus_contract::{Binder, Invocable, LiveEffect, Locus, Outcome, OutcomeValue, Stability};
use cronus_domain::invocable::{Dispatcher, InvocableRegistry, Registrant};
use cronus_domain::{Capabilities, Engine};

use super::core_id;

pub(super) fn register(
    registry: &mut InvocableRegistry,
    dispatcher: &mut Dispatcher,
    engine: Arc<Engine>,
) {
    let id = core_id("version");
    let invocable = Invocable {
        id: id.clone(),
        name: "Version",
        summary: "Show the engine/product version.",
        group: "status",
        // Reports on the product's own installation, not the user's work —
        // reclassified from `Semantic` once `Locus::Installation` existed
        // to fit it, and read-only, so it may run at any moment. Unlike
        // `core:status` (held in the installation declaration beside the
        // other installation verbs), the product version is genuinely a
        // fact any surface might want, so it stays registered here rather
        // than moving with them.
        locus: Locus::Installation {
            effect: LiveEffect::Inspect,
        },
        binders: Vec::<Binder>::new(),
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:version registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(move |_args| Outcome::Value(OutcomeValue::Text(engine.version().to_string()))),
    );
}
