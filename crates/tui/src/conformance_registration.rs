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
    use cronus_contract::{LiveEffect, Locus, Stability};

    /// A descriptor of the `Installation` locus under `id`, for adding to a
    /// local corpus and to the projection's own registry.
    fn installation_member(id: &InvocableId) -> Invocable {
        Invocable {
            id: id.clone(),
            name: "Installation member",
            summary: "Acts on the product's own installation.",
            group: "conformance",
            locus: Locus::Installation {
                effect: LiveEffect::Inspect,
            },
            binders: Vec::new(),
            stability: Stability::Shipped,
            journal_raw_input: true,
        }
    }

    /// A local corpus (never the shared `fixtures::corpus()` itself) and a
    /// projection that both hold one extra `Installation`-locus member under
    /// `id`.
    fn corpus_and_projection_with_an_installation_member(
        id: &InvocableId,
    ) -> (Corpus, TuiProjection) {
        let mut data = corpus();
        data.canonical.push(installation_member(id));

        let mut projection = TuiProjection::new();
        let registrant = if id.qualifier() == "core" {
            Registrant::core()
        } else {
            Registrant::extension(id.qualifier(), "conformance-corpus-test")
                .with_grant(CONTRIBUTE_GRANT)
        };
        projection
            .registry
            .register(&registrant, installation_member(id))
            .expect("the installation member registers cleanly");
        projection.dispatcher.attach(
            id.clone(),
            Arc::new(|_args| Outcome::Value(OutcomeValue::Empty)),
        );
        (data, projection)
    }

    /// This surface takes the `Installation` locus, so an installation member
    /// of the catalog is offered with no exclusion needed — the locus
    /// difference the earlier revision declared is gone, and the corpus
    /// confirms it rather than a comment asserting it.
    #[test]
    fn an_installation_member_is_offered_here_with_no_exclusion_declared() {
        let id = InvocableId::new("conformance:installation-member").expect("well-formed id");
        let (data, projection) = corpus_and_projection_with_an_installation_member(&id);

        assert_eq!(check_surface_set(&projection, &data, &[]), Vec::new());
        assert!(
            projection.exposed().iter().any(|i| i.id == id),
            "the installation member is in this surface's exposed set"
        );
    }

    /// The exclusions this surface really declares (`command::INSTALLATION_EXCLUSIONS`)
    /// are load-bearing in both directions. Positive half: with the exclusion
    /// declared, the surface-set family reports nothing — the omission is
    /// accounted for, not merely true by accident.
    #[test]
    fn a_declared_installation_exclusion_produces_zero_surface_set_divergence() {
        let (excluded, reason) = command::INSTALLATION_EXCLUSIONS[0];
        let id = InvocableId::new(excluded).expect("well-formed excluded id");
        let (data, projection) = corpus_and_projection_with_an_installation_member(&id);

        let exclusions = [DeclaredExclusion::new(id.clone(), reason)];
        assert_eq!(
            check_surface_set(&projection, &data, &exclusions),
            Vec::new()
        );
        assert!(
            projection.exposed().iter().all(|i| i.id != id),
            "the excluded verb is in the catalog but not in this surface's exposed set"
        );
    }

    /// The negative half, and the point of the mechanism: remove the
    /// exclusion and the same real projection reports the excluded id as
    /// missing — proving the declaration is load-bearing, not a statement
    /// that could never fail. A declared exclusion nobody checks is the same
    /// shape as the deleted hand-copied catalog mirror.
    #[test]
    fn removing_the_declared_exclusion_reports_the_excluded_id_as_missing() {
        let (excluded, _) = command::INSTALLATION_EXCLUSIONS[0];
        let id = InvocableId::new(excluded).expect("well-formed excluded id");
        let (data, projection) = corpus_and_projection_with_an_installation_member(&id);

        let reports = check_surface_set(&projection, &data, &[]);
        match reports.as_slice() {
            [
                ConformanceReport::SurfaceSet {
                    missing,
                    unexpected,
                },
            ] => {
                assert_eq!(missing, &vec![id]);
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
