//! Registers this frontend's real semantic-half projection against the
//! shared conformance corpus — the first surface to do so. Drives every
//! fixture through the exact functions `main.rs` itself calls
//! (`generated::semantic_shipped`, `generated::build_semantic_tree`,
//! `generated::invocation_from_matches`, a real `Dispatcher::dispatch`) —
//! never a restatement of the fixture data — so a divergence here is a
//! divergence in this surface's actual behavior, not in a test double.
//!
//! Compiled only under `#[cfg(test)]`: the corpus is something a surface's
//! own test target drives against its real projection, not product runtime
//! code.

use std::collections::HashMap;
use std::sync::Arc;

use cronus_conformance::{
    ConformanceReport, FIXTURE_IDENTITY, SECRET_VALUE, SurfaceProjection, corpus,
};
use cronus_contract::{
    ArgValue, ArgValues, Binder, BinderKind, Dispatched, Invocable, InvocableId, Invocation,
    Outcome, OutcomeValue, Surface,
};
use cronus_core::invocable::{CONTRIBUTE_GRANT, Dispatcher, InvocableRegistry, Registrant};

use crate::generated;

/// This surface's real projection, built from a registry holding **only**
/// the corpus's own fixtures — never the production ~30-group catalog. The
/// corpus proves the *generic projection machinery* in `generated.rs`
/// behaves correctly against an adversarial, purpose-built input set; mixing
/// in the real shipped groups would compare the corpus's 8 canonical ids
/// against those plus every real one, for no added proof, since the
/// machinery under test does not care which invocables it is given.
struct CliProjection {
    registry: InvocableRegistry,
    dispatcher: Dispatcher,
    /// Every fixture's real, generated `Arg` schema, precomputed once by
    /// walking the exact tree `build_semantic_tree` produces — never read
    /// back from the registry's own descriptor, which would prove only that
    /// the registry stores what it was given, not that this surface's
    /// grammar generation is correct.
    advertised: HashMap<InvocableId, Vec<Binder>>,
}

impl CliProjection {
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

        let semantic = generated::semantic_shipped(&registry);
        let (groups, _) = generated::build_semantic_tree(&semantic);
        let mut advertised = HashMap::new();
        for group in &groups {
            for verb in group.get_subcommands() {
                let Some(invocable) = semantic.iter().find(|i| {
                    i.group == group.get_name() && generated::verb_of(i) == verb.get_name()
                }) else {
                    continue;
                };
                let binders = verb
                    .get_arguments()
                    .filter(|arg| arg.get_id().as_str() != "help")
                    .map(binder_from_arg)
                    .collect();
                advertised.insert(invocable.id.clone(), binders);
            }
        }

        CliProjection {
            registry,
            dispatcher,
            advertised,
        }
    }
}

/// The inverse of `generated::arg_for_with_help`, read back from a real, generated
/// `clap::Arg` rather than from the `Binder` that produced it. Scoped to
/// what the shared corpus actually exercises: every fixture's own binder is
/// `Text` (`fixtures::corpus`'s `value`/`id`) or absent, so distinguishing
/// `Integer`/`Boolean`/`Float` positionals — which would mean inspecting
/// clap's erased `ValueParser` inner type, not exposed for equality — is
/// deliberately out of scope here, not an oversight.
fn binder_from_arg(arg: &clap::Arg) -> Binder {
    let name: &'static str = Box::leak(arg.get_id().as_str().to_string().into_boxed_str());
    let optional = !arg.is_required_set();
    let kind = if arg.get_long().is_some() {
        match arg.get_action() {
            clap::ArgAction::SetTrue => BinderKind::Flag,
            clap::ArgAction::Append => BinderKind::RepeatableNamedText,
            _ => BinderKind::NamedText,
        }
    } else {
        BinderKind::Text
    };
    Binder {
        name,
        kind,
        optional,
    }
}

impl SurfaceProjection for CliProjection {
    fn exposed(&self) -> Vec<Invocable> {
        generated::semantic_shipped(&self.registry)
            .into_iter()
            .cloned()
            .collect()
    }

    fn advertised_binders(&self, id: &InvocableId) -> Option<Vec<Binder>> {
        self.advertised.get(id).cloned()
    }

    fn invoke(&self, id: &InvocableId, args: &ArgValues) -> Dispatched {
        let invocation = Invocation {
            id: id.clone(),
            args: args.clone(),
            caller: Surface::Cli,
        };
        self.dispatcher.dispatch(&self.registry, &invocation)
    }
}

fn fixture_id(tail: &str) -> InvocableId {
    InvocableId::new(format!("{FIXTURE_IDENTITY}:{tail}")).expect("well-formed fixture id")
}

/// Wires one handler per corpus fixture, each producing exactly what
/// `fixtures::corpus()`'s own `outcome_fixtures` declares as `expected` —
/// this is the CLI's real dispatch path being driven, so a mismatch here is
/// this surface's own divergence, not the corpus's.
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
        // anyway for uniformity with every other fixture, rather than the
        // one special-cased as handler-less.
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

/// This surface registers against the corpus — the harness is
/// expected to fail on its very first run against any real surface, and
/// that failure is the initial inventory, not a defect in this test. What
/// this test locks in precisely is *which* failure: this dispatcher is
/// built exactly like production's own `compose()` in `main.rs`, which
/// never calls `Dispatcher::set_secrets` — the already-disclosed,
/// project-wide residual that every surface's redaction is fed an empty
/// secret list (accepted debt, seeded finding F-5/F-6). The two fixtures
/// that residual affects (`secret-bearing`, `boundary-crossing`) are
/// therefore the *only* divergence this run may report; anything else is a
/// genuine, previously-unknown finding this test must catch, not wave
/// through as "the corpus is expected to fail".
#[test]
fn the_semantic_half_registers_against_the_shared_conformance_corpus() {
    let data = corpus();
    let projection = CliProjection::new();
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
