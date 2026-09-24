//! Registers this shell's real projection against the shared conformance
//! corpus — the first time the corpus runs from a workspace detached from
//! the main engine workspace at all (the reason the corpus is a
//! library, not one central suite, finally proven rather than only
//! asserted).
//!
//! Drives every fixture through the exact seam the real shell crosses:
//! [`Bridge::catalog`]/[`Bridge::invoke`] — never a raw `InvocableRegistry`
//! or `Dispatcher` read directly, which this crate does not otherwise touch
//! at all outside this test-only registration. A divergence here is a
//! divergence in this shell's real bridge, not in a test double standing in
//! for it.
//!
//! Compiled only under `#[cfg(test)]`: the corpus is something a surface's
//! own test target drives against its real projection, not product runtime
//! code.

use std::collections::HashMap;
use std::sync::Arc;

use cronus_conformance::{FIXTURE_IDENTITY, SECRET_VALUE, SurfaceProjection, corpus};
use cronus_contract::{
    ArgValue, ArgValues, Binder, Dispatched, Invocable, InvocableId, Outcome, OutcomeValue,
};
use cronus_core::Engine;
use cronus_core::invocable::{CONTRIBUTE_GRANT, Dispatcher, InvocableRegistry, Registrant};

use crate::bridge::{Bridge, CoreBridge};

/// This shell's real projection, built over a [`Bridge`] whose registry
/// holds **only** the corpus's own fixtures — never the production catalog.
/// The corpus proves the generic bind→dispatch machinery behaves correctly
/// against an adversarial, purpose-built input set; mixing in the real
/// shipped groups would compare against those plus every real one, for no
/// added proof.
struct DesktopProjection {
    bridge: CoreBridge,
}

impl DesktopProjection {
    fn new() -> Self {
        let data = corpus();
        let mut registry = InvocableRegistry::new();
        let registrant = Registrant::extension(FIXTURE_IDENTITY, "conformance-corpus-test")
            .with_grant(CONTRIBUTE_GRANT);
        for invocable in &data.canonical {
            registry.register(&registrant, invocable.clone()).expect(
                "every corpus fixture registers cleanly — a bug in the fixture set or this \
                 registration, not a finding",
            );
        }

        let mut dispatcher = Dispatcher::new();
        attach_handlers(&mut dispatcher);

        // Empty secret list, matching `core_bridge()`'s own production shape
        // exactly (the same disclosed residual every surface's composition
        // carries) — the two redaction fixtures below are expected to
        // diverge for exactly this reason, not silently masked by a secret
        // list this run invented for itself.
        let bridge = Bridge::new(Engine::new(), Vec::new(), registry, dispatcher);
        DesktopProjection { bridge }
    }
}

impl SurfaceProjection for DesktopProjection {
    /// The real bridge's own `catalog()` — the identical call the shell's
    /// IPC command (`capability_catalog`) answers with, already filtered to
    /// `Semantic`+`ClientLocal`+`Shipped` (`Invocable::is_projected`) by
    /// `Bridge` itself.
    fn exposed(&self) -> Vec<Invocable> {
        self.bridge.catalog()
    }

    /// Reads the same catalog `exposed()` does — the desktop shell has no
    /// second, separately-generated schema artifact to diverge from (unlike
    /// the CLI's clap tree); the catalog IS this surface's one schema
    /// source, on both sides of the real IPC seam.
    fn advertised_binders(&self, id: &InvocableId) -> Option<Vec<Binder>> {
        self.bridge
            .catalog()
            .into_iter()
            .find(|invocable| &invocable.id == id)
            .map(|invocable| invocable.binders)
    }

    /// Drives one call through `Bridge::invoke` — the real function
    /// `capability_invoke` answers with, taking the same flat wire shape
    /// (`id` + a raw argument map) the IPC seam actually carries, not the
    /// domain-tier `ArgValues` this trait's own signature is expressed in
    /// (a ports-tier type every surface, including this one, can share).
    fn invoke(&self, id: &InvocableId, args: &ArgValues) -> Dispatched {
        let wire_args: HashMap<String, ArgValue> = args
            .iter()
            .map(|(name, value)| (name.to_string(), value.clone()))
            .collect();
        match self.bridge.invoke(id.as_str().to_string(), wire_args) {
            Ok(None) => Dispatched::Unknown,
            Ok(Some(outcome)) => Dispatched::Ran(outcome),
            Err(reason) => {
                panic!("a conformance fixture id is always well-formed by construction: {reason}")
            }
        }
    }
}

fn fixture_id(tail: &str) -> InvocableId {
    InvocableId::new(format!("{FIXTURE_IDENTITY}:{tail}")).expect("well-formed fixture id")
}

/// Wires one handler per corpus fixture, each producing exactly what
/// `fixtures::corpus()`'s own `outcome_fixtures` declares as `expected` —
/// this is this shell's real dispatch path being driven (via `Bridge`), so
/// a mismatch here is this surface's own divergence, not the corpus's.
/// Mirrors the CLI's and terminal UI's own `attach_handlers` byte-for-byte:
/// the `SurfaceProjection` trait deliberately names no `Dispatcher` type (so
/// an IPC-bridged projection like this one can implement it too), which is
/// exactly what forces each registering surface to attach its own handlers
/// rather than sharing one — a boundary, not an oversight.
fn attach_handlers(dispatcher: &mut Dispatcher) {
    dispatcher.attach(
        fixture_id("empty-result"),
        Arc::new(|_args| Outcome::Value(OutcomeValue::Empty)),
    );
    dispatcher.attach(
        fixture_id("single-element"),
        Arc::new(|_args| {
            Outcome::Value(OutcomeValue::List(vec![OutcomeValue::Text(
                "only".to_string(),
            )]))
        }),
    );
    dispatcher.attach(
        fixture_id("zero-count-list"),
        Arc::new(|_args| Outcome::Value(OutcomeValue::List(Vec::new()))),
    );
    dispatcher.attach(
        fixture_id("oversized-input"),
        Arc::new(|args| {
            let text = match args.get("value") {
                Some(ArgValue::Text(s)) => s.clone(),
                _ => String::new(),
            };
            Outcome::Value(OutcomeValue::Text(text))
        }),
    );
    dispatcher.attach(
        fixture_id("absent-required"),
        // Unreachable in this corpus's own run: the fixture supplies no
        // value for its one required `id` binder at all, so `bind()`
        // rejects before this handler could ever be called. Attached
        // anyway for uniformity with every other fixture.
        Arc::new(|_args| Outcome::Unavailable {
            reason: "unreachable: bind() rejects this fixture before dispatch".to_string(),
        }),
    );
    dispatcher.attach(
        fixture_id("unavailable-resource"),
        Arc::new(|_args| Outcome::Unavailable {
            reason: "conformance fixture: resource unavailable".to_string(),
        }),
    );
    dispatcher.attach(
        fixture_id("secret-bearing"),
        Arc::new(|_args| Outcome::Value(OutcomeValue::Text(format!("token={SECRET_VALUE} ready")))),
    );
    dispatcher.attach(
        fixture_id("boundary-crossing"),
        Arc::new(|_args| {
            Outcome::Value(OutcomeValue::Record(vec![(
                "tags".to_string(),
                OutcomeValue::List(vec![OutcomeValue::Text(format!("token={SECRET_VALUE}"))]),
            )]))
        }),
    );
}

#[cfg(test)]
mod tests {
    use cronus_conformance::ConformanceReport;

    use super::*;

    /// This shell registers against the full corpus — every
    /// assertion family, driven through its real `Bridge`. The harness is
    /// expected to fail on any real surface's first run (the harness's own
    /// callout); this test makes that expectation precise rather than
    /// vague: this bridge is composed exactly like production's own
    /// `core_bridge()`, which never populates a non-empty secret list — the
    /// already-disclosed, project-wide residual every surface's redaction
    /// carries (accepted debt, seeded finding F-5). The two fixtures that
    /// residual affects (`secret-bearing`, `boundary-crossing`) are
    /// therefore the *only* divergence this run may report; anything else
    /// is a genuine, previously-unknown finding this test must catch, not
    /// wave through as "the corpus is expected to fail". This is finding
    /// F-1's own missing third-site `consumer_registered` condition, and
    /// F-7's own, both satisfied by this real, running consumer.
    #[test]
    fn this_shell_registers_against_the_full_shared_conformance_corpus() {
        let data = corpus();
        let projection = DesktopProjection::new();
        let reports = cronus_conformance::run(&projection, &data, &[]);

        let disclosed_residual = |report: &ConformanceReport| {
            matches!(
                report,
                ConformanceReport::Outcome {
                    fixture: "secret-bearing" | "boundary-crossing",
                    ..
                }
            )
        };
        let unexpected: Vec<&ConformanceReport> = reports
            .iter()
            .filter(|report| !disclosed_residual(report))
            .collect();
        assert!(
            unexpected.is_empty(),
            "conformance divergence beyond the two disclosed redaction-residual fixtures: \
             {unexpected:#?}"
        );
        assert_eq!(
            reports.len(),
            2,
            "expected exactly the two disclosed redaction-residual fixtures to diverge, got: \
             {reports:#?}"
        );
    }
}
