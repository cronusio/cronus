//! Registers this frontend's real projection against the shared conformance
//! corpus. Drives every fixture through the exact functions the running
//! surface itself calls — [`command::is_projected`], a real
//! `InvocableRegistry`, and a real `Dispatcher::dispatch` — never a
//! restatement of the fixture data, so a divergence here is a divergence in
//! this surface's actual behavior, not in a test double.
//!
//! Compiled only under `#[cfg(test)]`: the corpus is something a surface's
//! own test target drives against its real projection, not product runtime
//! code.
//!
//! This task's own scope is the surface-set family and the declared-locus-
//! difference mechanism — proving `DeclaredExclusion` is
//! load-bearing, not merely documented in prose. The full corpus run
//! (schema + outcome families, the accepted-residual assertion, and finding
//! F-2's repayment) is a separate, later obligation this registration is
//! built to satisfy without rework.

use std::sync::Arc;

use cronus_conformance::{FIXTURE_IDENTITY, SECRET_VALUE, SurfaceProjection, corpus};
use cronus_contract::{
    ArgValue, ArgValues, Binder, Dispatched, Invocable, InvocableId, Invocation, Outcome,
    OutcomeValue, Resolved, Surface,
};
use cronus_core::invocable::{CONTRIBUTE_GRANT, Dispatcher, InvocableRegistry, Registrant};

use crate::command;

/// This surface's real projection, built from a registry holding **only**
/// the corpus's own fixtures — never the production catalog. The corpus
/// proves the generic bind→dispatch machinery behaves correctly against an
/// adversarial, purpose-built input set; mixing in the real shipped groups
/// (or this surface's own `pane.*` actions) would compare against those
/// plus every real one, for no added proof.
struct TuiProjection {
    registry: InvocableRegistry,
    dispatcher: Dispatcher,
}

impl TuiProjection {
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

        TuiProjection {
            registry,
            dispatcher,
        }
    }
}

impl SurfaceProjection for TuiProjection {
    /// Real production filtering (`command::is_projected`) over this
    /// registration's own registry — the identical predicate `build_catalog`
    /// consumes, never a restatement of it.
    fn exposed(&self) -> Vec<Invocable> {
        self.registry
            .all()
            .filter(|invocable| command::is_projected(invocable))
            .cloned()
            .collect()
    }

    /// This surface's real dispatch path (`dispatch::dispatch_command`,
    /// `bind_args`) reads a binder set from `registry.resolve(&id)` directly
    /// — there is no separate generated-grammar artifact for it the way the
    /// CLI's clap tree is, so reading the registry's own descriptor back
    /// here is not the tautology it would be on that sibling surface: it is
    /// the exact same data this surface's real dispatch already binds
    /// against, not a second, independently-derived copy of it.
    fn advertised_binders(&self, id: &InvocableId) -> Option<Vec<Binder>> {
        match self.registry.resolve(id) {
            Resolved::Found(descriptor) => Some(descriptor.binders.clone()),
            Resolved::Unknown => None,
        }
    }

    fn invoke(&self, id: &InvocableId, args: &ArgValues) -> Dispatched {
        let invocation = Invocation {
            id: id.clone(),
            args: args.clone(),
            caller: Surface::Tui,
        };
        self.dispatcher.dispatch(&self.registry, &invocation)
    }
}

fn fixture_id(tail: &str) -> InvocableId {
    InvocableId::new(format!("{FIXTURE_IDENTITY}:{tail}")).expect("well-formed fixture id")
}

/// Wires one handler per corpus fixture, each producing exactly what
/// `fixtures::corpus()`'s own `outcome_fixtures` declares as `expected` —
/// this is this surface's real dispatch path being driven, so a mismatch
/// here is this surface's own divergence, not the corpus's. Mirrors the
/// sibling CLI frontend's own `attach_handlers` byte-for-byte: the
/// `SurfaceProjection` trait deliberately names no `Dispatcher` type (so the
/// desktop's IPC-bridged projection can implement it too), which is exactly
/// what forces each in-process surface to attach its own handlers rather
/// than sharing one — a boundary, not an oversight.
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
    use super::*;
    use cronus_conformance::{ConformanceReport, Corpus, DeclaredExclusion, check_surface_set};
    use cronus_contract::{Locus, Stability};

    /// A local corpus (never the shared `fixtures::corpus()` itself) that
    /// adds exactly one synthetic `Installation`-locus canonical member to
    /// the real shared fixture set — an installation verb has no meaning
    /// inside a live session, so this surface must never expose it, and
    /// that must be a **declared** omission the corpus can check, not a
    /// silent one.
    fn corpus_with_an_installation_only_member() -> (Corpus, InvocableId) {
        let mut data = corpus();
        let id = InvocableId::new("conformance:installation-only")
            .expect("well-formed synthetic fixture id");
        data.canonical.push(Invocable {
            id: id.clone(),
            name: "Installation only",
            summary: "Meaningful only before a live session exists.",
            group: "conformance",
            locus: Locus::Installation,
            binders: Vec::new(),
            stability: Stability::Shipped,
            journal_raw_input: true,
        });
        (data, id)
    }

    /// The positive half: with the exclusion declared, the surface-set
    /// family reports nothing — the locus difference is accounted for, not
    /// merely true by accident.
    #[test]
    fn a_declared_installation_exclusion_produces_zero_surface_set_divergence() {
        let (data, installation_only) = corpus_with_an_installation_only_member();
        let projection = TuiProjection::new();
        let exclusions = [DeclaredExclusion::new(
            installation_only,
            "an installation verb has no meaning inside a live session",
        )];

        assert_eq!(
            check_surface_set(&projection, &data, &exclusions),
            Vec::new()
        );
    }

    /// The negative half, and the point of this task: remove the exclusion
    /// and the same real projection now reports the installation-only id as
    /// missing — proving the declaration is load-bearing, not a statement
    /// that could never fail. A declared exclusion nobody checks is the same
    /// shape as the deleted hand-copied catalog mirror.
    #[test]
    fn removing_the_declared_exclusion_reports_the_installation_only_id_as_missing() {
        let (data, installation_only) = corpus_with_an_installation_only_member();
        let projection = TuiProjection::new();

        let reports = check_surface_set(&projection, &data, &[]);
        match reports.as_slice() {
            [
                ConformanceReport::SurfaceSet {
                    missing,
                    unexpected,
                },
            ] => {
                assert_eq!(missing, &vec![installation_only]);
                assert!(unexpected.is_empty());
            }
            other => panic!("expected exactly one SurfaceSet report, got {other:?}"),
        }
    }

    /// Sanity check against the real, unmodified shared corpus (all
    /// `Semantic`, this surface's own shared vocabulary): the surface-set
    /// family reports zero divergence with no exclusions declared at all.
    #[test]
    fn the_shared_semantic_corpus_needs_no_declared_exclusion() {
        let data = corpus();
        let projection = TuiProjection::new();
        assert_eq!(check_surface_set(&projection, &data, &[]), Vec::new());
    }

    /// This surface registers against the full corpus — every
    /// assertion family, not only the surface-set family the two tests
    /// above prove. The harness is expected to fail on any real surface's
    /// first run (the harness's own callout); this test makes that expectation
    /// precise rather than vague: this dispatcher is built exactly like
    /// production's own `run()`, which never calls `Dispatcher::set_secrets`
    /// — the already-disclosed, project-wide residual that every surface's
    /// redaction is fed an empty secret list (accepted debt, seeded finding
    /// F-5). The two fixtures that residual affects (`secret-bearing`,
    /// `boundary-crossing`) are therefore the *only* divergence this run may
    /// report; anything else is a genuine, previously-unknown finding this
    /// test must catch, not wave through as "the corpus is expected to
    /// fail". This is finding F-2's own `consumer_registered` condition,
    /// satisfied by a real, running consumer.
    #[test]
    fn this_surface_registers_against_the_full_shared_conformance_corpus() {
        let data = corpus();
        let projection = TuiProjection::new();
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
