//! The evolution-loop runner: wraps
//! EVALUATE→ANALYZE→IMPROVE, nesting the execution-loop runner to score each
//! candidate. The evaluation pipeline (the oracle) stays frozen across every
//! generation — it is never a member of the loop's own mutation manifest;
//! only an *enclosing* loop, through the escalation gate, can ever change it.

use std::collections::HashSet;

use crate::loop_runner::execution::{ExecutionBackend, run_execution};
use crate::loop_runner::governor::{ControlFlow, Mutation, check_ceiling, guard_writes};
use crate::loop_runner::spec::{LoopOutcome, LoopSpec};

/// The seam an evolution loop composes: scoring the harness under
/// improvement one task at a time (the nested execution loop) and producing
/// bounded candidates. `H` is opaque to the runner — whatever artifact
/// family is being evolved (a prompt bundle, a harness config, ...).
pub trait EvolutionBackend<H> {
    /// Build the execution backend that scores `harness` against one task
    /// in the task set — the inner loop. Its class stays `Execution` and it
    /// changes nothing about its own task, regardless of the outer
    /// evolution loop.
    fn execution_backend_for(&mut self, harness: &H, task: &str) -> Box<dyn ExecutionBackend>;

    /// IMPROVE: produce a candidate harness, the mutations it represents
    /// (bounded by the declared manifest), and the task ids it predicts
    /// will newly pass.
    fn improve(&mut self, harness: &H) -> (H, Vec<Mutation>, HashSet<String>);
}

/// A generation's keep/revert/partial verdict (an evolution loop
/// scores a predicted-flip set against what actually happened).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationVerdict {
    Keep,
    Revert,
    Partial,
}

/// Score a candidate's predicted flips against what actually flipped
/// relative to the baseline. An empty prediction is scored `Revert` — an
/// IMPROVE step that predicts nothing has nothing to confirm.
pub fn score_generation(
    predicted: &HashSet<String>,
    actual: &HashSet<String>,
) -> GenerationVerdict {
    if predicted.is_empty() {
        return GenerationVerdict::Revert;
    }
    let confirmed = predicted.intersection(actual).count();
    if confirmed == predicted.len() {
        GenerationVerdict::Keep
    } else if confirmed == 0 {
        GenerationVerdict::Revert
    } else {
        GenerationVerdict::Partial
    }
}

/// One generation's ledger entry (an evolution loop's ledger carries
/// the predicted-flip field an execution loop's ledger omits).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationRecord {
    pub generation: u32,
    pub predicted_flips: Vec<String>,
    pub verdict: GenerationVerdict,
}

/// The outcome of a full evolution-loop run.
pub struct EvolutionReport<H> {
    pub harness: H,
    pub ledger: Vec<GenerationRecord>,
    pub generations_run: u32,
}

/// EVALUATE: nest `run_execution` per task in the task set (nested
/// composition) to derive the set of tasks the harness currently passes.
fn evaluate<H>(
    backend: &mut dyn EvolutionBackend<H>,
    inner_spec: &LoopSpec,
    harness: &H,
    task_set: &[String],
) -> HashSet<String> {
    let mut passed = HashSet::new();
    for task in task_set {
        let mut exec_backend = backend.execution_backend_for(harness, task);
        let report = run_execution(inner_spec, exec_backend.as_mut(), task);
        if matches!(report.outcome, LoopOutcome::Done(_)) {
            passed.insert(task.clone());
        }
    }
    passed
}

/// EVALUATE→ANALYZE→IMPROVE: scores the harness (nesting `run_execution` per
/// task), calls IMPROVE for a manifest-bounded candidate, scores
/// predicted-vs-actual flips, and keeps the candidate only on a
/// `Keep` verdict. An IMPROVE mutation outside the declared manifest is
/// rejected outright — never adopted, never scored. Held-out transfer
/// validation for a criteria change or self-modification is the escalation
/// gate's job, not this ordinary per-generation cycle.
pub fn run_evolution<H: Clone>(
    spec: &LoopSpec,
    inner_spec: &LoopSpec,
    backend: &mut dyn EvolutionBackend<H>,
    mut harness: H,
    task_set: &[String],
) -> EvolutionReport<H> {
    let mut ledger = Vec::new();
    let mut generation: u32 = 0;

    loop {
        if let ControlFlow::Stop(_) = check_ceiling(&spec.ceiling, generation, None, 0, 0) {
            break;
        }

        let baseline = evaluate(backend, inner_spec, &harness, task_set);
        let (candidate, mutations, predicted) = backend.improve(&harness);

        if guard_writes(&mutations, &spec.manifest).is_err() {
            generation += 1;
            continue;
        }

        let candidate_passed = evaluate(backend, inner_spec, &candidate, task_set);
        let actual_flips: HashSet<String> =
            candidate_passed.difference(&baseline).cloned().collect();
        let verdict = score_generation(&predicted, &actual_flips);

        let mut predicted_flips: Vec<String> = predicted.into_iter().collect();
        predicted_flips.sort();
        ledger.push(GenerationRecord {
            generation,
            predicted_flips,
            verdict,
        });

        if verdict == GenerationVerdict::Keep {
            harness = candidate;
        }
        generation += 1;
    }

    EvolutionReport {
        harness,
        ledger,
        generations_run: generation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::BudgetStatus;
    use crate::loop_runner::governor::TurnResult;
    use crate::loop_runner::spec::{
        Ceiling, LoopClass, MutableArtifact, MutationManifest, Oracle, WorkspaceKind,
    };
    use std::collections::HashMap;

    struct FakeExecBackend {
        passes: bool,
    }

    impl ExecutionBackend for FakeExecBackend {
        fn current_time(&self) -> u64 {
            0
        }
        fn budget_status(&self) -> Option<BudgetStatus> {
            None
        }
        fn run_turn(&mut self, _plan: &str, _status: &str) -> (Vec<Mutation>, TurnResult) {
            (
                Vec::new(),
                TurnResult {
                    raw_done: self.passes,
                    actor_lineage: "actor".to_string(),
                    feedback: None,
                },
            )
        }
        fn rollback(&mut self, feedback: Option<&str>) -> String {
            feedback.unwrap_or("").to_string()
        }
        fn finalize(&mut self) {}
    }

    struct FakeEvolutionBackend {
        candidate: String,
        mutations: Vec<Mutation>,
        predicted: HashSet<String>,
        pass_map: HashMap<String, HashSet<String>>,
    }

    impl EvolutionBackend<String> for FakeEvolutionBackend {
        fn execution_backend_for(
            &mut self,
            harness: &String,
            task: &str,
        ) -> Box<dyn ExecutionBackend> {
            let passes = self
                .pass_map
                .get(harness)
                .map(|set| set.contains(task))
                .unwrap_or(false);
            Box::new(FakeExecBackend { passes })
        }

        fn improve(&mut self, _harness: &String) -> (String, Vec<Mutation>, HashSet<String>) {
            (
                self.candidate.clone(),
                self.mutations.clone(),
                self.predicted.clone(),
            )
        }
    }

    fn outer_spec(max_generations: u32) -> LoopSpec {
        LoopSpec {
            class: LoopClass::Evolution,
            manifest: MutationManifest::new(2, [MutableArtifact::Knowledge]),
            oracle: Oracle::Deterministic {
                validator: "n/a — outer evolution loop consults only ceiling+manifest".to_string(),
            },
            ceiling: Ceiling {
                max_iterations: max_generations,
                budget_ref: None,
                deadline: None,
                patience: max_generations,
            },
            workspace_kind: WorkspaceKind::Worktree,
            objective_slot: None,
        }
    }

    fn inner_spec() -> LoopSpec {
        LoopSpec {
            class: LoopClass::Execution,
            manifest: MutationManifest::same_prompt(),
            oracle: Oracle::Deterministic {
                validator: "task done-check".to_string(),
            },
            ceiling: Ceiling {
                max_iterations: 1,
                budget_ref: None,
                deadline: None,
                patience: 1,
            },
            workspace_kind: WorkspaceKind::Worktree,
            objective_slot: None,
        }
    }

    // --- inner-loop class stays fixed regardless of the outer loop ---------

    #[test]
    fn the_nested_execution_loop_stays_execution_class_regardless_of_the_outer_evolution_loop() {
        assert_eq!(outer_spec(1).class, LoopClass::Evolution);
        assert_eq!(inner_spec().class, LoopClass::Execution);
    }

    // --- a candidate is kept only on Keep, reverted otherwise ---------------

    #[test]
    fn a_candidate_is_kept_when_every_predicted_flip_is_confirmed() {
        let mut pass_map = HashMap::new();
        pass_map.insert("gen0".to_string(), HashSet::new());
        pass_map.insert("gen1".to_string(), HashSet::from(["task1".to_string()]));
        let mut backend = FakeEvolutionBackend {
            candidate: "gen1".to_string(),
            mutations: vec![Mutation {
                artifact: MutableArtifact::Knowledge,
                summary: "learned a new exemplar".to_string(),
            }],
            predicted: HashSet::from(["task1".to_string()]),
            pass_map,
        };
        let report = run_evolution(
            &outer_spec(1),
            &inner_spec(),
            &mut backend,
            "gen0".to_string(),
            &["task1".to_string()],
        );

        assert_eq!(report.harness, "gen1");
        assert_eq!(report.ledger.len(), 1);
        assert_eq!(report.ledger[0].verdict, GenerationVerdict::Keep);
    }

    #[test]
    fn a_candidate_is_reverted_when_no_predicted_flip_is_confirmed() {
        let mut pass_map = HashMap::new();
        pass_map.insert("gen0".to_string(), HashSet::new());
        pass_map.insert("gen1".to_string(), HashSet::new()); // nothing actually changed
        let mut backend = FakeEvolutionBackend {
            candidate: "gen1".to_string(),
            mutations: vec![Mutation {
                artifact: MutableArtifact::Knowledge,
                summary: "attempted a fix".to_string(),
            }],
            predicted: HashSet::from(["task1".to_string()]),
            pass_map,
        };
        let report = run_evolution(
            &outer_spec(1),
            &inner_spec(),
            &mut backend,
            "gen0".to_string(),
            &["task1".to_string()],
        );

        assert_eq!(report.harness, "gen0");
        assert_eq!(report.ledger[0].verdict, GenerationVerdict::Revert);
    }

    #[test]
    fn a_generation_that_confirms_only_some_predicted_flips_is_partial() {
        let mut pass_map = HashMap::new();
        pass_map.insert("gen0".to_string(), HashSet::new());
        pass_map.insert("gen1".to_string(), HashSet::from(["task1".to_string()]));
        let mut backend = FakeEvolutionBackend {
            candidate: "gen1".to_string(),
            mutations: vec![Mutation {
                artifact: MutableArtifact::Knowledge,
                summary: "partial fix".to_string(),
            }],
            predicted: HashSet::from(["task1".to_string(), "task2".to_string()]),
            pass_map,
        };
        let report = run_evolution(
            &outer_spec(1),
            &inner_spec(),
            &mut backend,
            "gen0".to_string(),
            &["task1".to_string(), "task2".to_string()],
        );

        assert_eq!(report.ledger[0].verdict, GenerationVerdict::Partial);
        // Partial never adopts the candidate — only a full Keep does.
        assert_eq!(report.harness, "gen0");
    }

    // --- a manifest-bounded IMPROVE mutation is rejected, never adopted -----

    #[test]
    fn an_out_of_manifest_improve_mutation_is_rejected_and_never_adopted() {
        let mut pass_map = HashMap::new();
        pass_map.insert("gen0".to_string(), HashSet::new());
        pass_map.insert("gen1".to_string(), HashSet::from(["task1".to_string()]));
        let mut backend = FakeEvolutionBackend {
            candidate: "gen1".to_string(),
            mutations: vec![Mutation {
                artifact: MutableArtifact::Prompt, // not in the outer manifest (only Knowledge)
                summary: "rewrote the prompt".to_string(),
            }],
            predicted: HashSet::from(["task1".to_string()]),
            pass_map,
        };
        let report = run_evolution(
            &outer_spec(1),
            &inner_spec(),
            &mut backend,
            "gen0".to_string(),
            &["task1".to_string()],
        );

        assert_eq!(report.harness, "gen0");
        assert!(
            report.ledger.is_empty(),
            "a rejected candidate is never even scored"
        );
    }
}
