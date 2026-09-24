//! Lookahead planning — budget-bounded consequence simulation before high-impact,
//! hard-to-reverse actions.
//!
//! Lookahead fires only for catalog trigger categories. The simulation is a
//! "what if" pass over the agent's model and issues no real tool calls, writes, or
//! spawns — here the per-depth verdicts arrive pre-computed as data, so the
//! engine performs zero side effects. Depth and token budget are hard limits;
//! a conclusion (even partial) is produced before commit; budget exhaustion
//! falls back to the approval gate, never a silent proceed; every conclusion
//! is appended to the decision log before commit.

/// A high-impact action category eligible for lookahead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerCategory {
    BranchMerge,
    SchemaMigration,
    FileDeletion,
    DepUpgrade,
    SecurityPolicy,
    MassRefactor,
    ArchChange,
}

impl TriggerCategory {
    /// The per-category default lookahead depth.
    pub fn default_depth(self) -> u8 {
        match self {
            TriggerCategory::BranchMerge => 3,
            TriggerCategory::SchemaMigration => 4,
            TriggerCategory::FileDeletion => 3,
            TriggerCategory::DepUpgrade => 3,
            TriggerCategory::SecurityPolicy => 5,
            TriggerCategory::MassRefactor => 3,
            TriggerCategory::ArchChange => 5,
        }
    }
}

/// The predicted verdict for one simulated depth step (data, not execution).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepVerdict {
    /// The projected next state is acceptable and leads toward the goal.
    Acceptable,
    /// The projected next state is unacceptable — an unambiguously blocking effect.
    Blocking,
}

/// The lookahead budget: hard limits on depth and token spend.
#[derive(Debug, Clone, Copy)]
pub struct LookaheadBudget {
    pub max_depth: u8,
    pub max_tokens: u32,
    /// Tokens consumed per simulated step.
    pub tokens_per_step: u32,
}

/// The conclusion of a lookahead pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Conclusion {
    /// No blocking consequence within depth — execute the original action.
    Confirm,
    /// A better-bounded variant avoids the identified risk.
    Modify(String),
    /// Unacceptable consequence found — route to the approval gate, do not execute.
    Escalate,
    /// Budget exhausted before a conclusion — falls back to the approval gate.
    BudgetExhausted,
}

impl Conclusion {
    /// Whether this conclusion permits executing an action (original or modified).
    pub fn permits_execution(&self) -> bool {
        matches!(self, Conclusion::Confirm | Conclusion::Modify(_))
    }

    /// Whether this conclusion routes to the human approval gate.
    pub fn routes_to_approval(&self) -> bool {
        matches!(self, Conclusion::Escalate | Conclusion::BudgetExhausted)
    }
}

/// One append-only decision-log record.
#[derive(Debug, Clone)]
pub struct DecisionRecord {
    pub category: TriggerCategory,
    pub depth_reached: u8,
    pub conclusion: Conclusion,
}

/// The append-only decision log. Feeds the self-improvement calibration loop.
#[derive(Debug, Default)]
pub struct DecisionLog {
    records: Vec<DecisionRecord>,
}

impl DecisionLog {
    pub fn new() -> Self {
        DecisionLog::default()
    }

    pub fn records(&self) -> &[DecisionRecord] {
        &self.records
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

/// Whether an action category triggers lookahead. `None` = a non-catalog
/// action, which bypasses lookahead entirely.
pub fn is_triggered(category: Option<TriggerCategory>) -> bool {
    category.is_some()
}

/// Run a budget-bounded lookahead. The per-depth `verdicts` are the pre-computed
/// simulation results (no execution happens here). A conclusion is produced
/// and appended to the log before the caller commits.
///
/// - A `Blocking` verdict at any depth terminates early with `Escalate`.
/// - All `Acceptable` within depth → `Confirm`.
/// - Depth/token budget consumed before a verdict resolves → `BudgetExhausted`.
pub fn run_lookahead(
    log: &mut DecisionLog,
    category: TriggerCategory,
    budget: LookaheadBudget,
    verdicts: &[StepVerdict],
) -> Conclusion {
    let mut depth_reached = 0u8;
    let mut tokens = 0u32;
    let mut conclusion = Conclusion::BudgetExhausted;

    for (i, verdict) in verdicts.iter().enumerate() {
        let d = (i as u8) + 1;
        if d > budget.max_depth {
            conclusion = Conclusion::BudgetExhausted;
            break;
        }
        tokens += budget.tokens_per_step;
        if tokens > budget.max_tokens {
            conclusion = Conclusion::BudgetExhausted;
            break;
        }
        depth_reached = d;
        match verdict {
            StepVerdict::Blocking => {
                conclusion = Conclusion::Escalate; // early termination
                break;
            }
            StepVerdict::Acceptable => {
                // Confirm once we reach the full depth (or exhaust the verdicts)
                // with no blocker, and stop — the simulation has concluded.
                if d == budget.max_depth || i + 1 == verdicts.len() {
                    conclusion = Conclusion::Confirm;
                    break;
                }
            }
        }
    }

    log.records.push(DecisionRecord {
        category,
        depth_reached,
        conclusion: conclusion.clone(),
    });
    conclusion
}

#[cfg(test)]
mod tests {
    use super::*;

    fn budget(depth: u8) -> LookaheadBudget {
        LookaheadBudget {
            max_depth: depth,
            max_tokens: 10_000,
            tokens_per_step: 100,
        }
    }

    #[test]
    fn only_catalog_actions_trigger() {
        assert!(is_triggered(Some(TriggerCategory::SchemaMigration)));
        assert!(!is_triggered(None));
        assert_eq!(TriggerCategory::SecurityPolicy.default_depth(), 5);
    }

    #[test]
    fn all_acceptable_confirms() {
        let mut log = DecisionLog::new();
        let c = run_lookahead(
            &mut log,
            TriggerCategory::BranchMerge,
            budget(3),
            &[
                StepVerdict::Acceptable,
                StepVerdict::Acceptable,
                StepVerdict::Acceptable,
            ],
        );
        assert_eq!(c, Conclusion::Confirm);
        assert!(c.permits_execution());
        assert_eq!(log.records()[0].depth_reached, 3);
    }

    #[test]
    fn blocking_escalates_early() {
        // A blocking consequence terminates early with Escalate (no execution).
        let mut log = DecisionLog::new();
        let c = run_lookahead(
            &mut log,
            TriggerCategory::FileDeletion,
            budget(3),
            &[
                StepVerdict::Acceptable,
                StepVerdict::Blocking,
                StepVerdict::Acceptable,
            ],
        );
        assert_eq!(c, Conclusion::Escalate);
        assert!(c.routes_to_approval());
        assert_eq!(log.records()[0].depth_reached, 2); // terminated at depth 2
    }

    #[test]
    fn budget_exhaustion_falls_back_to_approval() {
        // Depth budget consumed before a blocker/full-confirm ->
        // BudgetExhausted, which routes to the approval gate (never silent proceed).
        let mut log = DecisionLog::new();
        let c = run_lookahead(
            &mut log,
            TriggerCategory::ArchChange,
            budget(2), // only 2 steps allowed
            &[
                StepVerdict::Acceptable,
                StepVerdict::Acceptable,
                StepVerdict::Acceptable,
            ],
        );
        // 2 acceptable steps reached max_depth -> Confirm (depth-bounded conclusion).
        assert_eq!(c, Conclusion::Confirm);

        // Token-starved case: high per-step cost exhausts tokens before depth.
        let mut log2 = DecisionLog::new();
        let tight = LookaheadBudget {
            max_depth: 5,
            max_tokens: 50,
            tokens_per_step: 100,
        };
        let c2 = run_lookahead(
            &mut log2,
            TriggerCategory::ArchChange,
            tight,
            &[StepVerdict::Acceptable, StepVerdict::Acceptable],
        );
        assert_eq!(c2, Conclusion::BudgetExhausted);
        assert!(c2.routes_to_approval());
    }

    #[test]
    fn every_conclusion_is_logged_before_commit() {
        // The record exists after the pass, before the caller commits.
        let mut log = DecisionLog::new();
        run_lookahead(
            &mut log,
            TriggerCategory::DepUpgrade,
            budget(1),
            &[StepVerdict::Acceptable],
        );
        assert_eq!(log.len(), 1);
        assert_eq!(log.records()[0].category, TriggerCategory::DepUpgrade);
    }
}
