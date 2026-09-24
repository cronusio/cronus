//! Oracle selection: which oracle an execution loop's inner
//! done-check uses, and how a `Human` oracle behaves where no one can
//! actually approve anything.

use crate::loop_runner::spec::Oracle;

/// Prefer the cheapest trustworthy oracle. A declared deterministic
/// validator is used for the inner done-check whenever one exists;
/// judge/human are reserved for conditions that cannot be checked
/// mechanically. Advisory — this is a preference, not a hard gate, so the
/// caller may still choose `fallback` deliberately; it is never silently
/// overridden.
pub fn select_oracle(deterministic: Option<Oracle>, fallback: Oracle) -> Oracle {
    deterministic.unwrap_or(fallback)
}

/// Pick a judge binding whose lineage differs from the actor's, out of
/// whatever bindings are available. If none is distinct, degrade to the
/// actor's own lineage rather than failing — `governor::judge` then stamps
/// `reduced_confidence` for this oracle, a recorded weakness, not a silent
/// one.
pub fn select_judge_binding(actor_lineage: &str, available_lineages: &[String]) -> Oracle {
    let lineage = available_lineages
        .iter()
        .find(|candidate| candidate.as_str() != actor_lineage)
        .cloned()
        .unwrap_or_else(|| actor_lineage.to_string());
    Oracle::Judge { lineage }
}

/// Whether the current execution context can actually present an approval
/// prompt to a person.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalContext {
    Interactive,
    Background,
}

/// A `Human` oracle's resolved behavior for the current context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HumanFallback {
    UseHuman,
    FallBackTo(Oracle),
    Stop,
}

/// A `Human` oracle in a `Background` context (cron, unattended) cannot show
/// a prompt. It falls back to a deterministic check when one is declared, or
/// stops the loop outright — it never silently self-approves on the
/// actor's own say-so.
pub fn resolve_human_oracle(
    context: ApprovalContext,
    deterministic_fallback: Option<Oracle>,
) -> HumanFallback {
    match context {
        ApprovalContext::Interactive => HumanFallback::UseHuman,
        ApprovalContext::Background => match deterministic_fallback {
            Some(oracle) => HumanFallback::FallBackTo(oracle),
            None => HumanFallback::Stop,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loop_runner::governor::{TurnResult, judge};

    // --- Deterministic is preferred over judge/human when declared ----

    #[test]
    fn a_deterministic_oracle_is_preferred_over_the_fallback_when_declared() {
        let deterministic = Oracle::Deterministic {
            validator: "cargo test".to_string(),
        };
        let chosen = select_oracle(Some(deterministic.clone()), Oracle::Human);
        assert_eq!(chosen, deterministic);
    }

    #[test]
    fn the_fallback_is_used_when_no_deterministic_validator_is_declared() {
        let chosen = select_oracle(None, Oracle::Human);
        assert_eq!(chosen, Oracle::Human);
    }

    // --- Judge lineage selection + reduced_confidence composition ----

    #[test]
    fn a_distinct_lineage_binding_is_selected_when_available() {
        let oracle = select_judge_binding(
            "worker-model",
            &["worker-model".to_string(), "reviewer-model".to_string()],
        );
        assert_eq!(
            oracle,
            Oracle::Judge {
                lineage: "reviewer-model".to_string()
            }
        );
        let turn = TurnResult {
            raw_done: true,
            actor_lineage: "worker-model".to_string(),
            feedback: None,
        };
        assert!(!judge(&oracle, &turn).reduced_confidence);
    }

    #[test]
    fn degrading_to_the_actors_own_lineage_is_recorded_as_reduced_confidence() {
        let oracle = select_judge_binding("worker-model", &["worker-model".to_string()]);
        assert_eq!(
            oracle,
            Oracle::Judge {
                lineage: "worker-model".to_string()
            }
        );
        let turn = TurnResult {
            raw_done: true,
            actor_lineage: "worker-model".to_string(),
            feedback: None,
        };
        assert!(judge(&oracle, &turn).reduced_confidence);
    }

    // --- A Human oracle never silently self-approves in the background

    #[test]
    fn an_interactive_context_uses_the_human_oracle_directly() {
        let resolved = resolve_human_oracle(ApprovalContext::Interactive, None);
        assert_eq!(resolved, HumanFallback::UseHuman);
    }

    #[test]
    fn a_background_context_falls_back_to_a_declared_deterministic_check() {
        let deterministic = Oracle::Deterministic {
            validator: "exit_code".to_string(),
        };
        let resolved =
            resolve_human_oracle(ApprovalContext::Background, Some(deterministic.clone()));
        assert_eq!(resolved, HumanFallback::FallBackTo(deterministic));
    }

    #[test]
    fn a_background_context_with_no_fallback_stops_rather_than_self_approving() {
        let resolved = resolve_human_oracle(ApprovalContext::Background, None);
        assert_eq!(resolved, HumanFallback::Stop);
    }
}
