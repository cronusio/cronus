//! The single enforcement point every loop runner calls:
//! the ceiling check, the mutation-manifest write guard, and the
//! oracle dispatch. Concentrating all three here means there is exactly one
//! place ceilings and oracles are enforced, not one per subsystem.

use crate::budget::BudgetStatus;
use crate::loop_runner::Ceiling;
use crate::loop_runner::spec::{MutableArtifact, MutationManifest, Oracle, Verdict};

/// The ceiling's verdict for one iteration boundary, independent of
/// what the actor or the oracle reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlFlow {
    Continue,
    Stop(StopReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    MaxIterations,
    BudgetExhausted,
    DeadlinePassed,
    NoProgress,
}

/// Evaluated before every iteration. Checks max-iterations, budget,
/// deadline, and patience — none of which depend on the actor's "I'm done"
/// claim or the oracle's verdict, so a loop can never talk its way past its
/// own ceiling.
pub fn check_ceiling(
    ceiling: &Ceiling,
    iteration: u32,
    budget_status: Option<&BudgetStatus>,
    now: u64,
    no_progress_streak: u32,
) -> ControlFlow {
    if iteration >= ceiling.max_iterations {
        return ControlFlow::Stop(StopReason::MaxIterations);
    }
    if let Some(BudgetStatus::Exhausted { .. }) = budget_status {
        return ControlFlow::Stop(StopReason::BudgetExhausted);
    }
    if let Some(deadline) = ceiling.deadline
        && now >= deadline
    {
        return ControlFlow::Stop(StopReason::DeadlinePassed);
    }
    if no_progress_streak >= ceiling.patience {
        return ControlFlow::Stop(StopReason::NoProgress);
    }
    ControlFlow::Continue
}

/// One artifact write an iteration attempted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mutation {
    pub artifact: MutableArtifact,
    pub summary: String,
}

/// An attempt to write an artifact kind the manifest does not
/// declare mutable. Since `MutableArtifact` has no `Criteria` variant, a
/// criteria write can never even be constructed here — it is unreachable,
/// not merely rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IllegalMutation(pub MutableArtifact);

/// Every mutation's artifact kind must be declared in the manifest's
/// mutable set, or the whole batch is rejected before it reaches disk —
/// a recorded event (the caller ledgers `IllegalMutation`), never a silent
/// drop.
pub fn guard_writes(
    mutations: &[Mutation],
    manifest: &MutationManifest,
) -> Result<(), IllegalMutation> {
    for mutation in mutations {
        if !manifest.allows(mutation.artifact) {
            return Err(IllegalMutation(mutation.artifact));
        }
    }
    Ok(())
}

/// What one turn produced, ready for the oracle to judge. `raw_done` is
/// whatever mechanical check, judge-model call, or human approval the
/// caller already ran for the declared `Oracle` kind — the governor does not
/// perform that call itself, only decides the resulting `Verdict`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnResult {
    pub raw_done: bool,
    pub actor_lineage: String,
    pub feedback: Option<String>,
}

/// Dispatch to the declared oracle kind and stamp `reduced_confidence`
/// when the oracle's lineage matches the actor's — a permitted but weaker
/// termination, recorded rather than hidden. Only a `Judge` oracle has a
/// lineage to compare; `Deterministic` and `Human` are never reduced.
pub fn judge(oracle: &Oracle, turn: &TurnResult) -> Verdict {
    let reduced_confidence = match oracle {
        Oracle::Judge { lineage } => lineage == &turn.actor_lineage,
        Oracle::Deterministic { .. } | Oracle::Human => false,
    };
    Verdict {
        done: turn.raw_done,
        reduced_confidence,
        feedback: turn.feedback.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loop_runner::spec::MutationManifest;

    fn ceiling(max_iterations: u32, deadline: Option<u64>, patience: u32) -> Ceiling {
        Ceiling {
            max_iterations,
            budget_ref: None,
            deadline,
            patience,
        }
    }

    // --- The ceiling stops independent of actor/oracle -----------------

    #[test]
    fn check_ceiling_stops_on_max_iterations() {
        let c = ceiling(3, None, 10);
        assert_eq!(
            check_ceiling(&c, 3, None, 0, 0),
            ControlFlow::Stop(StopReason::MaxIterations)
        );
        assert_eq!(check_ceiling(&c, 2, None, 0, 0), ControlFlow::Continue);
    }

    #[test]
    fn check_ceiling_stops_on_exhausted_budget() {
        let c = ceiling(100, None, 10);
        let exhausted = BudgetStatus::Exhausted {
            spent: 10.0,
            limit: 10.0,
        };
        assert_eq!(
            check_ceiling(&c, 0, Some(&exhausted), 0, 0),
            ControlFlow::Stop(StopReason::BudgetExhausted)
        );
        let ok = BudgetStatus::Ok {
            spent: 1.0,
            remaining: 9.0,
        };
        assert_eq!(check_ceiling(&c, 0, Some(&ok), 0, 0), ControlFlow::Continue);
    }

    #[test]
    fn check_ceiling_stops_when_the_deadline_has_passed() {
        let c = ceiling(100, Some(1_000), 10);
        assert_eq!(
            check_ceiling(&c, 0, None, 1_000, 0),
            ControlFlow::Stop(StopReason::DeadlinePassed)
        );
        assert_eq!(check_ceiling(&c, 0, None, 999, 0), ControlFlow::Continue);
    }

    #[test]
    fn check_ceiling_stops_on_patience_exhausted_no_progress() {
        let c = ceiling(100, None, 2);
        assert_eq!(
            check_ceiling(&c, 0, None, 0, 2),
            ControlFlow::Stop(StopReason::NoProgress)
        );
        assert_eq!(check_ceiling(&c, 0, None, 0, 1), ControlFlow::Continue);
    }

    #[test]
    fn the_ceiling_fires_even_when_nothing_about_the_actor_or_oracle_is_involved() {
        // check_ceiling takes no actor/oracle input at all — its signature
        // proves independence structurally, not just by these assertions.
        let c = ceiling(1, None, 10);
        assert_eq!(
            check_ceiling(&c, 1, None, 0, 0),
            ControlFlow::Stop(StopReason::MaxIterations)
        );
    }

    // --- Illegal writes are rejected, never silently dropped -----

    #[test]
    fn guard_writes_rejects_a_mutation_outside_the_manifest() {
        let manifest = MutationManifest::new(1, [MutableArtifact::Plan]);
        let mutations = [Mutation {
            artifact: MutableArtifact::Prompt,
            summary: "rewrote the system prompt".to_string(),
        }];
        assert_eq!(
            guard_writes(&mutations, &manifest),
            Err(IllegalMutation(MutableArtifact::Prompt))
        );
    }

    #[test]
    fn guard_writes_allows_every_mutation_the_manifest_declares() {
        let manifest =
            MutationManifest::new(2, [MutableArtifact::Plan, MutableArtifact::Knowledge]);
        let mutations = [
            Mutation {
                artifact: MutableArtifact::Plan,
                summary: "updated the plan".to_string(),
            },
            Mutation {
                artifact: MutableArtifact::Knowledge,
                summary: "added an exemplar".to_string(),
            },
        ];
        assert_eq!(guard_writes(&mutations, &manifest), Ok(()));
    }

    // --- Oracle dispatch + lineage-matched reduced_confidence ---------

    #[test]
    fn judge_stamps_reduced_confidence_only_when_the_judge_shares_the_actors_lineage() {
        let turn = TurnResult {
            raw_done: true,
            actor_lineage: "worker-model".to_string(),
            feedback: None,
        };
        let same_lineage = Oracle::Judge {
            lineage: "worker-model".to_string(),
        };
        let different_lineage = Oracle::Judge {
            lineage: "reviewer-model".to_string(),
        };
        assert!(judge(&same_lineage, &turn).reduced_confidence);
        assert!(!judge(&different_lineage, &turn).reduced_confidence);
    }

    #[test]
    fn deterministic_and_human_oracles_are_never_reduced_confidence() {
        let turn = TurnResult {
            raw_done: false,
            actor_lineage: "worker-model".to_string(),
            feedback: Some("2 tests still failing".to_string()),
        };
        let deterministic = Oracle::Deterministic {
            validator: "cargo test".to_string(),
        };
        let human = Oracle::Human;
        assert!(!judge(&deterministic, &turn).reduced_confidence);
        assert!(!judge(&human, &turn).reduced_confidence);
        assert!(!judge(&deterministic, &turn).done);
    }
}
