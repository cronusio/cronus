//! The execution-loop runner: re-attempts a fixed task until
//! the oracle says done or the ceiling fires. The actor's own "I'm done" is
//! advisory — only `governor::judge` sets `Verdict.done`.

use crate::budget::BudgetStatus;
use crate::loop_runner::governor::{
    ControlFlow, Mutation, TurnResult, check_ceiling, guard_writes, judge,
};
use crate::loop_runner::spec::{LedgerEntry, LoopOutcome, LoopSpec, Verdict};

/// The seam the domain tier cannot perform itself: a real isolated
/// workspace and a real turn. Tests use a scripted fake; the facade
/// wires a real execution workspace + agent session + budget engine.
pub trait ExecutionBackend {
    /// Current wall-clock time as a unix timestamp (kept out of the pure
    /// runner so it stays testable without a real clock).
    fn current_time(&self) -> u64;

    /// The budget policy's current status, if the loop is budget-bound.
    fn budget_status(&self) -> Option<BudgetStatus>;

    /// Run one iteration attempt. `plan`/`status` are the durable,
    /// reconstructed-fresh-each-time artifacts — never the growing
    /// transcript of prior iterations. Returns the mutations attempted and
    /// the oracle's raw signal for this attempt (already computed by
    /// whichever mechanism the declared `Oracle` names).
    fn run_turn(&mut self, plan: &str, status: &str) -> (Vec<Mutation>, TurnResult);

    /// Discard the attempt (rollback) and return the compact status
    /// note the next iteration reconstructs from.
    fn rollback(&mut self, feedback: Option<&str>) -> String;

    /// Commit the workspace write-back on a done verdict.
    fn finalize(&mut self);
}

/// The outcome of a full execution-loop run: the final result, the
/// append-only mutation ledger, and how many iterations it took.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionReport {
    pub outcome: LoopOutcome,
    pub ledger: Vec<LedgerEntry>,
    pub iterations_run: u32,
}

/// Re-attempt `plan`/`status` under `backend` until the oracle judges an
/// iteration done or the ceiling stops the loop. A rejected (out-of-manifest)
/// mutation never reaches the backend's committed state — it is recorded in
/// the ledger and rolled back like any other non-done attempt, never a
/// silent drop.
pub fn run_execution(
    spec: &LoopSpec,
    backend: &mut dyn ExecutionBackend,
    plan: &str,
) -> ExecutionReport {
    let mut status = String::new();
    let mut ledger = Vec::new();
    let mut iteration: u32 = 0;
    let mut no_progress_streak: u32 = 0;

    loop {
        let now = backend.current_time();
        let budget_status = backend.budget_status();
        if let ControlFlow::Stop(reason) = check_ceiling(
            &spec.ceiling,
            iteration,
            budget_status.as_ref(),
            now,
            no_progress_streak,
        ) {
            return ExecutionReport {
                outcome: LoopOutcome::Stopped(format!("{reason:?}")),
                ledger,
                iterations_run: iteration,
            };
        }

        let (mutations, turn_result) = backend.run_turn(plan, &status);

        if let Err(illegal) = guard_writes(&mutations, &spec.manifest) {
            ledger.push(LedgerEntry {
                iteration,
                artifact: illegal.0,
                summary: "rejected: outside the declared mutation manifest".to_string(),
                why: "IllegalMutation".to_string(),
            });
            status = backend.rollback(Some("an illegal mutation was rejected"));
            iteration += 1;
            no_progress_streak += 1;
            continue;
        }

        for mutation in &mutations {
            ledger.push(LedgerEntry {
                iteration,
                artifact: mutation.artifact,
                summary: mutation.summary.clone(),
                why: "applied within the declared manifest".to_string(),
            });
        }

        let verdict: Verdict = judge(&spec.oracle, &turn_result);

        if verdict.done {
            backend.finalize();
            return ExecutionReport {
                outcome: LoopOutcome::Done(verdict),
                ledger,
                iterations_run: iteration + 1,
            };
        }

        if mutations.is_empty() {
            no_progress_streak += 1;
        } else {
            no_progress_streak = 0;
        }
        status = backend.rollback(verdict.feedback.as_deref());
        iteration += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loop_runner::spec::{
        Ceiling, LoopClass, MutableArtifact, MutationManifest, Oracle, WorkspaceKind,
    };

    /// A scripted backend: each call to `run_turn` pops the next scripted
    /// response. Records every `status` it was called with, so a test can
    /// prove the runner reconstructs fresh from the compact note rather than
    /// threading a growing transcript.
    struct FakeBackend {
        responses: Vec<(Vec<Mutation>, TurnResult)>,
        call_index: usize,
        statuses_seen: Vec<String>,
        rollback_count: u32,
        finalized: bool,
    }

    impl FakeBackend {
        fn new(responses: Vec<(Vec<Mutation>, TurnResult)>) -> Self {
            FakeBackend {
                responses,
                call_index: 0,
                statuses_seen: Vec::new(),
                rollback_count: 0,
                finalized: false,
            }
        }
    }

    impl ExecutionBackend for FakeBackend {
        fn current_time(&self) -> u64 {
            0
        }

        fn budget_status(&self) -> Option<BudgetStatus> {
            None
        }

        fn run_turn(&mut self, _plan: &str, status: &str) -> (Vec<Mutation>, TurnResult) {
            self.statuses_seen.push(status.to_string());
            let response = self
                .responses
                .get(self.call_index)
                .cloned()
                .unwrap_or_else(|| {
                    (
                        Vec::new(),
                        TurnResult {
                            raw_done: false,
                            actor_lineage: "actor".to_string(),
                            feedback: None,
                        },
                    )
                });
            self.call_index += 1;
            response
        }

        fn rollback(&mut self, feedback: Option<&str>) -> String {
            self.rollback_count += 1;
            feedback.unwrap_or("no progress").to_string()
        }

        fn finalize(&mut self) {
            self.finalized = true;
        }
    }

    fn deterministic_spec(max_iterations: u32) -> LoopSpec {
        LoopSpec {
            class: LoopClass::Execution,
            manifest: MutationManifest::new(1, [MutableArtifact::Plan]),
            oracle: Oracle::Deterministic {
                validator: "tests".to_string(),
            },
            ceiling: Ceiling {
                max_iterations,
                budget_ref: None,
                deadline: None,
                patience: max_iterations,
            },
            workspace_kind: WorkspaceKind::Worktree,
            objective_slot: None,
        }
    }

    fn turn(raw_done: bool) -> TurnResult {
        TurnResult {
            raw_done,
            actor_lineage: "actor".to_string(),
            feedback: if raw_done {
                None
            } else {
                Some("not there yet".to_string())
            },
        }
    }

    // --- A failed attempt rolls back to a fresh, compact status -------

    #[test]
    fn a_failed_attempt_rolls_back_and_the_next_iteration_reconstructs_from_the_compact_status() {
        let mut backend =
            FakeBackend::new(vec![(Vec::new(), turn(false)), (Vec::new(), turn(true))]);
        let spec = deterministic_spec(5);
        let report = run_execution(&spec, &mut backend, "the plan");

        assert!(matches!(report.outcome, LoopOutcome::Done(_)));
        assert_eq!(backend.rollback_count, 1);
        // The second run_turn call saw only the compact rollback note, not
        // an accumulated transcript of the first attempt.
        assert_eq!(backend.statuses_seen[0], "");
        assert_eq!(backend.statuses_seen[1], "not there yet");
    }

    // --- The ceiling stops the loop independent of the actor ----------

    #[test]
    fn the_ceiling_stops_the_loop_even_though_the_backend_never_reports_done() {
        let mut backend = FakeBackend::new(vec![(Vec::new(), turn(false)); 10]);
        let spec = deterministic_spec(2);
        let report = run_execution(&spec, &mut backend, "the plan");

        assert!(matches!(report.outcome, LoopOutcome::Stopped(_)));
        assert_eq!(report.iterations_run, 2);
        assert!(!backend.finalized);
    }

    // --- Every applied mutation appends to the ledger -----------------

    #[test]
    fn every_applied_mutation_appends_to_the_ledger() {
        let mutation = Mutation {
            artifact: MutableArtifact::Plan,
            summary: "narrowed the plan".to_string(),
        };
        let mut backend = FakeBackend::new(vec![(vec![mutation.clone()], turn(true))]);
        let spec = deterministic_spec(5);
        let report = run_execution(&spec, &mut backend, "the plan");

        assert_eq!(report.ledger.len(), 1);
        assert_eq!(report.ledger[0].artifact, MutableArtifact::Plan);
        assert_eq!(report.ledger[0].summary, "narrowed the plan");
    }

    // --- An out-of-manifest write never reaches the backend's committed state

    #[test]
    fn an_out_of_manifest_write_is_rejected_and_rolled_back_not_finalized() {
        let illegal = Mutation {
            artifact: MutableArtifact::Prompt, // not in the manifest (only Plan is)
            summary: "rewrote the prompt".to_string(),
        };
        let mut backend = FakeBackend::new(vec![
            (vec![illegal], turn(true)), // even though the oracle would say done, the illegal write blocks it
            (Vec::new(), turn(true)),
        ]);
        let spec = deterministic_spec(5);
        let report = run_execution(&spec, &mut backend, "the plan");

        assert!(matches!(report.outcome, LoopOutcome::Done(_)));
        assert_eq!(report.iterations_run, 2);
        assert_eq!(report.ledger.len(), 1);
        assert_eq!(report.ledger[0].why, "IllegalMutation");
        assert_eq!(backend.rollback_count, 1);
    }

    // --- A passing deterministic oracle ends the loop Done -------------------

    #[test]
    fn a_passing_deterministic_oracle_ends_the_loop_done_and_finalizes() {
        let mut backend = FakeBackend::new(vec![(Vec::new(), turn(true))]);
        let spec = deterministic_spec(5);
        let report = run_execution(&spec, &mut backend, "the plan");

        assert!(matches!(report.outcome, LoopOutcome::Done(_)));
        assert!(backend.finalized);
        assert_eq!(report.iterations_run, 1);
    }
}
