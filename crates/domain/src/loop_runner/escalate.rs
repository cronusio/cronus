//! The escalation gate: the only path by which a *lower* loop's
//! criteria — or, at Tier 4, its prompt/tools — become mutable. Never a side
//! effect inside the judged loop; always performed by an *enclosing* loop
//! with its own separated oracle, external novelty, and held-out proof.

use crate::loop_runner::spec::Oracle;

/// A source of novelty external to the loop's own prior output — new tasks,
/// inputs, or feedback the loop did not generate itself: a
/// self-referential loop with no external novelty narrows into
/// self-repetition rather than improving.
pub trait NoveltySource {
    fn has_external_input(&self) -> bool;
}

/// Whether `oracle` is separated from the actor's own lineage. A
/// `Deterministic` or `Human` oracle is always separated by construction; a
/// `Judge` oracle is separated only when its lineage differs from the
/// actor's (the negation of what `governor::judge`'s `reduced_confidence`
/// reports for the same pair).
pub fn is_separated_oracle(oracle: &Oracle, actor_lineage: &str) -> bool {
    !matches!(oracle, Oracle::Judge { lineage } if lineage == actor_lineage)
}

/// Why an escalation was refused outright — a precondition failed before
/// any evaluation ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalReason {
    OracleNotSeparated,
    NoExternalNovelty,
}

/// The result of an escalation attempt.
#[derive(Debug, Clone, PartialEq)]
pub enum EscalationOutcome {
    /// Preconditions were satisfied and the held-out gain cleared the bar.
    Promoted,
    /// Preconditions were satisfied but the held-out result did not clear
    /// the bar — recorded, not silently discarded.
    Rejected,
    /// A precondition (separated oracle / external novelty) was not met —
    /// evaluation never even ran. A hard gate, not advisory.
    Refused(RefusalReason),
}

/// The caller-measured held-out results (never the search set) an
/// escalation bid is judged against.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeldOutMeasurement {
    pub before_metric: f64,
    pub after_metric: f64,
    pub regression: f64,
    pub margin: f64,
    pub regression_bound: f64,
}

/// Attempt to escalate a change (a criteria change, or at Tier 4 a
/// self-modification) for a loop governed by `target_oracle`. Both
/// preconditions are hard gates checked before anything else: a separated
/// oracle (`target_oracle`'s lineage differs from `actor_lineage`) and an
/// external novelty source. `measurement` holds the caller-measured held-out
/// results — this gate decides the precondition check and the
/// promote/reject call, not the evaluation itself.
pub fn escalate(
    target_oracle: &Oracle,
    actor_lineage: &str,
    novelty: &dyn NoveltySource,
    measurement: &HeldOutMeasurement,
) -> EscalationOutcome {
    if !is_separated_oracle(target_oracle, actor_lineage) {
        return EscalationOutcome::Refused(RefusalReason::OracleNotSeparated);
    }
    if !novelty.has_external_input() {
        return EscalationOutcome::Refused(RefusalReason::NoExternalNovelty);
    }
    if measurement.after_metric - measurement.before_metric >= measurement.margin
        && measurement.regression <= measurement.regression_bound
    {
        EscalationOutcome::Promoted
    } else {
        EscalationOutcome::Rejected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeNovelty(bool);
    impl NoveltySource for FakeNovelty {
        fn has_external_input(&self) -> bool {
            self.0
        }
    }

    fn separated_judge() -> Oracle {
        Oracle::Judge {
            lineage: "reviewer-model".to_string(),
        }
    }

    // --- promotes only on a held-out gain within the regression bound -------

    fn measurement(before: f64, after: f64, regression: f64) -> HeldOutMeasurement {
        HeldOutMeasurement {
            before_metric: before,
            after_metric: after,
            regression,
            margin: 0.10,
            regression_bound: 0.05,
        }
    }

    #[test]
    fn promotes_on_a_held_out_gain_within_the_regression_bound() {
        let outcome = escalate(
            &separated_judge(),
            "worker-model",
            &FakeNovelty(true),
            &measurement(0.70, 0.85, 0.02),
        );
        assert_eq!(outcome, EscalationOutcome::Promoted);
    }

    #[test]
    fn rejects_and_records_the_attempt_when_the_held_out_gain_misses_the_margin() {
        let outcome = escalate(
            &separated_judge(),
            "worker-model",
            &FakeNovelty(true),
            &measurement(0.70, 0.72, 0.00), // gain of 0.02, below the 0.10 margin
        );
        assert_eq!(outcome, EscalationOutcome::Rejected);
    }

    #[test]
    fn rejects_when_the_gain_clears_the_margin_but_regression_exceeds_the_bound() {
        let outcome = escalate(
            &separated_judge(),
            "worker-model",
            &FakeNovelty(true),
            &measurement(0.70, 0.90, 0.20), // regression exceeds the 0.05 bound
        );
        assert_eq!(outcome, EscalationOutcome::Rejected);
    }

    // --- hard preconditions: refused, evaluation never runs -----------

    #[test]
    fn refuses_when_the_oracle_shares_the_actors_lineage_even_with_a_winning_metric() {
        let same_lineage_judge = Oracle::Judge {
            lineage: "worker-model".to_string(),
        };
        let outcome = escalate(
            &same_lineage_judge,
            "worker-model",
            &FakeNovelty(true),
            &measurement(0.10, 0.95, 0.00), // would otherwise clearly promote
        );
        assert_eq!(
            outcome,
            EscalationOutcome::Refused(RefusalReason::OracleNotSeparated)
        );
    }

    #[test]
    fn refuses_when_no_external_novelty_source_is_present_even_with_a_winning_metric() {
        let outcome = escalate(
            &separated_judge(),
            "worker-model",
            &FakeNovelty(false),
            &measurement(0.10, 0.95, 0.00),
        );
        assert_eq!(
            outcome,
            EscalationOutcome::Refused(RefusalReason::NoExternalNovelty)
        );
    }

    #[test]
    fn deterministic_and_human_oracles_are_always_separated() {
        let deterministic = Oracle::Deterministic {
            validator: "tests".to_string(),
        };
        let human = Oracle::Human;
        assert!(is_separated_oracle(&deterministic, "worker-model"));
        assert!(is_separated_oracle(&human, "worker-model"));
    }
}
