//! Drives the shared corpus through one surface's real projection and
//! reports every divergence. Three independent passes, one per
//! assertion family — a surface that fails one family still runs the other
//! two, so a single run surfaces everything wrong at once rather than one
//! defect per invocation.

use std::collections::HashSet;

use cronus_contract::InvocableId;

use crate::fixtures::Corpus;
use crate::projection::SurfaceProjection;
use crate::report::{ConformanceReport, DeclaredExclusion};

/// Run every assertion family against `projection`. An empty result is the
/// only passing one; a non-empty result is handed to the caller to assert
/// on directly (a single `assert!(reports.is_empty(), "{reports:#?}")` in
/// the registering surface's own test) or to fold into the finding
/// inventory the sibling task owns.
pub fn run(
    projection: &dyn SurfaceProjection,
    corpus: &Corpus,
    declared_exclusions: &[DeclaredExclusion],
) -> Vec<ConformanceReport> {
    let mut reports = check_surface_set(projection, corpus, declared_exclusions);
    reports.extend(check_schema(projection, corpus, declared_exclusions));
    reports.extend(check_outcomes(projection, corpus));
    reports
}

/// Surface set: exposed invocables must equal the
/// canonical shipped set minus this surface's own declared exclusions —
/// no more, no less.
pub fn check_surface_set(
    projection: &dyn SurfaceProjection,
    corpus: &Corpus,
    declared_exclusions: &[DeclaredExclusion],
) -> Vec<ConformanceReport> {
    let excluded: HashSet<&InvocableId> = declared_exclusions.iter().map(|e| &e.id).collect();
    let expected: HashSet<InvocableId> = corpus
        .canonical
        .iter()
        .map(|invocable| invocable.id.clone())
        .filter(|id| !excluded.contains(id))
        .collect();
    let actual: HashSet<InvocableId> = projection
        .exposed()
        .into_iter()
        .map(|invocable| invocable.id)
        .collect();

    let mut missing: Vec<InvocableId> = expected.difference(&actual).cloned().collect();
    let mut unexpected: Vec<InvocableId> = actual.difference(&expected).cloned().collect();
    if missing.is_empty() && unexpected.is_empty() {
        return Vec::new();
    }
    missing.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    unexpected.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    vec![ConformanceReport::SurfaceSet {
        missing,
        unexpected,
    }]
}

/// Schema: each canonical invocable's advertised argument
/// schema on this surface must match its declared binders exactly — the
/// same fact, rendered from the same source, never restated by hand.
pub fn check_schema(
    projection: &dyn SurfaceProjection,
    corpus: &Corpus,
    declared_exclusions: &[DeclaredExclusion],
) -> Vec<ConformanceReport> {
    let excluded: HashSet<&InvocableId> = declared_exclusions.iter().map(|e| &e.id).collect();
    corpus
        .canonical
        .iter()
        .filter(|invocable| !excluded.contains(&invocable.id))
        .filter_map(|invocable| {
            let advertised = projection.advertised_binders(&invocable.id);
            if advertised.as_deref() == Some(invocable.binders.as_slice()) {
                None
            } else {
                Some(ConformanceReport::Schema {
                    id: invocable.id.clone(),
                    declared: invocable.binders.clone(),
                    advertised,
                })
            }
        })
        .collect()
}

/// Outcome: driving each fixture invocation through this
/// surface's real dispatch path must produce exactly the corpus's declared
/// `Dispatched` — rejection mode and location included, and the
/// empty-versus-unavailable distinction included, since both are just
/// fields of the one value compared here.
pub fn check_outcomes(
    projection: &dyn SurfaceProjection,
    corpus: &Corpus,
) -> Vec<ConformanceReport> {
    corpus
        .outcome_fixtures
        .iter()
        .filter_map(|fixture| {
            let actual = projection.invoke(&fixture.id, &fixture.args);
            if actual == fixture.expected {
                None
            } else {
                Some(ConformanceReport::Outcome {
                    fixture: fixture.name,
                    expected: fixture.expected.clone(),
                    actual,
                })
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use cronus_contract::{ArgValues, Binder, Dispatched, Invocable, Outcome, OutcomeValue};

    use super::*;
    use crate::fixtures::corpus;

    /// A fake projection driven entirely by in-memory maps — enough to
    /// prove the harness's own comparison logic without a real registry or
    /// dispatcher, which this crate does not and must not depend on.
    struct FakeProjection {
        exposed: Vec<Invocable>,
        binders: HashMap<String, Vec<Binder>>,
        outcomes: HashMap<String, Dispatched>,
    }

    impl SurfaceProjection for FakeProjection {
        fn exposed(&self) -> Vec<Invocable> {
            self.exposed.clone()
        }

        fn advertised_binders(&self, id: &InvocableId) -> Option<Vec<Binder>> {
            self.binders.get(id.as_str()).cloned()
        }

        fn invoke(&self, id: &InvocableId, _args: &ArgValues) -> Dispatched {
            self.outcomes
                .get(id.as_str())
                .cloned()
                .unwrap_or(Dispatched::Unknown)
        }
    }

    /// A projection built to agree with the corpus on every fixture —
    /// proving the harness reports nothing when a surface genuinely
    /// conforms, not merely when it is never asked.
    fn conforming_projection() -> FakeProjection {
        let data = corpus();
        let binders = data
            .canonical
            .iter()
            .map(|invocable| (invocable.id.as_str().to_string(), invocable.binders.clone()))
            .collect();
        let outcomes = data
            .outcome_fixtures
            .iter()
            .map(|fixture| (fixture.id.as_str().to_string(), fixture.expected.clone()))
            .collect();
        FakeProjection {
            exposed: data.canonical,
            binders,
            outcomes,
        }
    }

    #[test]
    fn a_fully_conforming_projection_produces_no_reports() {
        let projection = conforming_projection();
        let data = corpus();
        assert_eq!(run(&projection, &data, &[]), Vec::new());
    }

    #[test]
    fn a_missing_invocable_is_reported_by_surface_set() {
        let mut projection = conforming_projection();
        let dropped = projection.exposed.pop().unwrap().id;
        let data = corpus();

        let reports = check_surface_set(&projection, &data, &[]);
        match reports.as_slice() {
            [
                ConformanceReport::SurfaceSet {
                    missing,
                    unexpected,
                },
            ] => {
                assert_eq!(missing, &vec![dropped]);
                assert!(unexpected.is_empty());
            }
            other => panic!("expected exactly one SurfaceSet report, got {other:?}"),
        }
    }

    #[test]
    fn a_declared_exclusion_silences_the_missing_report() {
        let mut projection = conforming_projection();
        let dropped = projection.exposed.pop().unwrap().id;
        let data = corpus();

        let exclusions = [DeclaredExclusion::new(dropped, "host-owned, out of scope")];
        assert_eq!(
            check_surface_set(&projection, &data, &exclusions),
            Vec::new()
        );
    }

    #[test]
    fn an_unexpected_invocable_is_reported_by_surface_set() {
        let mut projection = conforming_projection();
        let extra = Invocable {
            id: InvocableId::new("conformance:not-in-the-corpus").unwrap(),
            name: "Uninvited",
            summary: "Not part of the shared corpus.",
            group: "conformance",
            locus: cronus_contract::Locus::Semantic,
            binders: Vec::new(),
            stability: cronus_contract::Stability::Shipped,
            journal_raw_input: true,
        };
        projection.exposed.push(extra.clone());
        let data = corpus();

        let reports = check_surface_set(&projection, &data, &[]);
        match reports.as_slice() {
            [
                ConformanceReport::SurfaceSet {
                    missing,
                    unexpected,
                },
            ] => {
                assert!(missing.is_empty());
                assert_eq!(unexpected, &vec![extra.id]);
            }
            other => panic!("expected exactly one SurfaceSet report, got {other:?}"),
        }
    }

    #[test]
    fn a_mismatched_binder_is_reported_by_schema() {
        let mut projection = conforming_projection();
        let data = corpus();
        let target = data
            .canonical
            .iter()
            .find(|invocable| !invocable.binders.is_empty())
            .expect("at least one canonical fixture declares a binder");
        // Silently widen the advertised binder from required to optional —
        // exactly the kind of divergence a restated schema produces.
        let mut wrong = target.binders.clone();
        wrong[0].optional = true;
        projection
            .binders
            .insert(target.id.as_str().to_string(), wrong.clone());

        let reports = check_schema(&projection, &data, &[]);
        match reports.as_slice() {
            [
                ConformanceReport::Schema {
                    id,
                    declared,
                    advertised,
                },
            ] => {
                assert_eq!(id, &target.id);
                assert_eq!(declared, &target.binders);
                assert_eq!(advertised, &Some(wrong));
            }
            other => panic!("expected exactly one Schema report, got {other:?}"),
        }
    }

    #[test]
    fn a_surface_that_never_heard_of_an_id_reports_no_advertised_schema() {
        let mut projection = conforming_projection();
        let data = corpus();
        let target = data
            .canonical
            .iter()
            .find(|invocable| !invocable.binders.is_empty())
            .unwrap();
        projection.binders.remove(target.id.as_str());

        let reports = check_schema(&projection, &data, &[]);
        assert!(matches!(
            reports.as_slice(),
            [ConformanceReport::Schema {
                advertised: None,
                ..
            }]
        ));
    }

    #[test]
    fn a_divergent_outcome_is_reported_by_name() {
        let mut projection = conforming_projection();
        let data = corpus();
        let fixture = data
            .outcome_fixtures
            .iter()
            .find(|fixture| fixture.name == "empty-result")
            .unwrap();
        // A surface that quietly turns "empty" into a one-item list — the
        // exact re-derivation class this family exists to catch.
        let wrong = Dispatched::Ran(Outcome::Value(OutcomeValue::List(vec![
            OutcomeValue::Text("not actually empty".to_string()),
        ])));
        projection
            .outcomes
            .insert(fixture.id.as_str().to_string(), wrong.clone());

        let reports = check_outcomes(&projection, &data);
        match reports.as_slice() {
            [
                ConformanceReport::Outcome {
                    fixture: name,
                    expected,
                    actual,
                },
            ] => {
                assert_eq!(*name, "empty-result");
                assert_eq!(expected, &fixture.expected);
                assert_eq!(actual, &wrong);
            }
            other => panic!("expected exactly one Outcome report, got {other:?}"),
        }
    }

    #[test]
    fn an_unavailable_resource_collapsing_into_empty_is_caught() {
        // The residual this fixture exists to guard, exercised directly:
        // a surface reporting an unreachable resource as a success-shaped
        // empty value must fail this family, not pass it.
        let mut projection = conforming_projection();
        let data = corpus();
        let fixture = data
            .outcome_fixtures
            .iter()
            .find(|fixture| fixture.name == "unavailable-resource")
            .unwrap();
        projection.outcomes.insert(
            fixture.id.as_str().to_string(),
            Dispatched::Ran(Outcome::Value(OutcomeValue::Empty)),
        );

        let reports = check_outcomes(&projection, &data);
        assert_eq!(reports.len(), 1);
        assert!(matches!(
            reports[0],
            ConformanceReport::Outcome {
                fixture: "unavailable-resource",
                ..
            }
        ));
    }
}
