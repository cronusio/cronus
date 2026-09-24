//! Loop-runner invariant acceptance sweep —
//! The closing validation for this subsystem. Each testable invariant maps to
//! one named test, exercised through the **real facade export chain**
//! (`cronus_core::loop_runner::{...}`) — proving the assembled facade
//! re-export composes, not just that each domain module works in isolation.
//!
//! **Invariants deeply covered elsewhere, cited rather than duplicated:**
//! - The full unit-level proof for every invariant lives in
//!   `crates/domain/src/loop_runner/{spec,governor,execution,evolution,
//!   escalate,objective}.rs`'s own 42 tests.
//! - The real (not mocked) CLI/facade wiring — a live `FileExistsBackend`,
//!   a real clock, real state-tier persistence — is proven in
//!   `crates/core/src/loop_bootstrap.rs`'s 4 tests and `crates/cli/tests/
//!   cli_smoke.rs`'s 4 real end-to-end binary-spawning tests.

use cronus_core::context_mgmt::trim_cascade;
use cronus_core::loop_runner::governor::Mutation;
use cronus_core::loop_runner::objective::{
    objective_context_entry, re_project_objective, update_progress,
};
use cronus_core::loop_runner::oracle::select_oracle;
use cronus_core::loop_runner::{
    Ceiling, ControlFlow, EscalationOutcome, ExecutionBackend, HeldOutMeasurement, LoopClass,
    LoopOutcome, LoopSpec, MutableArtifact, MutationManifest, NoveltySource, ObjectiveSlot, Oracle,
    RefusalReason, TurnResult, WorkspaceKind, check_ceiling, escalate, guard_writes, judge,
    run_execution,
};

struct FakeBackend {
    responses: Vec<(Vec<Mutation>, TurnResult)>,
    index: usize,
    statuses_seen: Vec<String>,
}

impl FakeBackend {
    fn new(responses: Vec<(Vec<Mutation>, TurnResult)>) -> Self {
        FakeBackend {
            responses,
            index: 0,
            statuses_seen: Vec::new(),
        }
    }
}

impl ExecutionBackend for FakeBackend {
    fn current_time(&self) -> u64 {
        0
    }
    fn budget_status(&self) -> Option<cronus_core::budget::BudgetStatus> {
        None
    }
    fn run_turn(&mut self, _plan: &str, status: &str) -> (Vec<Mutation>, TurnResult) {
        self.statuses_seen.push(status.to_string());
        let response = self.responses[self.index].clone();
        self.index += 1;
        response
    }
    fn rollback(&mut self, feedback: Option<&str>) -> String {
        feedback.unwrap_or("").to_string()
    }
    fn finalize(&mut self) {}
}

fn turn(raw_done: bool, feedback: Option<&str>) -> TurnResult {
    TurnResult {
        raw_done,
        actor_lineage: "actor".to_string(),
        feedback: feedback.map(|s| s.to_string()),
    }
}

fn exec_spec(max_iterations: u32, mutable: impl IntoIterator<Item = MutableArtifact>) -> LoopSpec {
    LoopSpec {
        class: LoopClass::Execution,
        manifest: MutationManifest::new(1, mutable),
        oracle: Oracle::Deterministic {
            validator: "sweep".to_string(),
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

// --- A loop runs only under its declared class ------------------------

#[test]
fn lg1_a_loop_spec_carries_exactly_the_class_it_declared() {
    let spec = exec_spec(1, [MutableArtifact::Plan]);
    assert_eq!(spec.class, LoopClass::Execution);
    assert_ne!(spec.class, LoopClass::Evolution);
}

// --- Mutation-rights manifest — an out-of-manifest write is recorded --

#[test]
fn lg2_a_write_outside_the_manifest_is_rejected_not_silently_dropped() {
    let manifest = MutationManifest::new(1, [MutableArtifact::Plan]);
    let illegal = [Mutation {
        artifact: MutableArtifact::Prompt,
        summary: "rewrote the prompt".to_string(),
    }];
    let result = guard_writes(&illegal, &manifest);
    assert!(
        result.is_err(),
        "an undeclared artifact kind must be rejected"
    );
}

// --- Criteria are structurally unreachable through MutableArtifact ---

#[test]
fn lg3_the_mutable_artifact_taxonomy_has_no_criteria_variant() {
    // Exhaustive match compiles only because these six variants are the
    // whole enum — a `Criteria` arm would not compile. The guarantee holds by
    // construction through the facade's re-exported type, not by this
    // assertion alone.
    for artifact in [
        MutableArtifact::Scratch,
        MutableArtifact::Plan,
        MutableArtifact::Knowledge,
        MutableArtifact::ValidationInput,
        MutableArtifact::Prompt,
        MutableArtifact::Tools,
    ] {
        match artifact {
            MutableArtifact::Scratch
            | MutableArtifact::Plan
            | MutableArtifact::Knowledge
            | MutableArtifact::ValidationInput
            | MutableArtifact::Prompt
            | MutableArtifact::Tools => {}
        }
    }
}

// --- Oracle ownership — separated vs. same-lineage reduced_confidence

#[test]
fn lg4_a_judge_sharing_the_actors_lineage_is_recorded_reduced_confidence() {
    // `turn()` fixes the actor's lineage at "actor" — the oracle is
    // separated or not relative to that same value.
    let same_lineage = Oracle::Judge {
        lineage: "actor".to_string(),
    };
    let different_lineage = Oracle::Judge {
        lineage: "reviewer-model".to_string(),
    };
    let t = turn(true, None);
    assert!(judge(&same_lineage, &t).reduced_confidence);
    assert!(!judge(&different_lineage, &t).reduced_confidence);
}

// --- State reconstructs fresh each iteration, never inherits ---------

#[test]
fn lg5_the_next_iteration_reconstructs_from_the_compact_status_not_a_transcript() {
    let spec = exec_spec(5, [MutableArtifact::Plan]);
    let mut backend = FakeBackend::new(vec![
        (Vec::new(), turn(false, Some("2 of 5 checks passing"))),
        (Vec::new(), turn(true, None)),
    ]);
    let report = run_execution(&spec, &mut backend, "the plan");
    assert!(matches!(report.outcome, LoopOutcome::Done(_)));
    assert_eq!(backend.statuses_seen[0], "");
    assert_eq!(backend.statuses_seen[1], "2 of 5 checks passing");
}

// --- The ceiling stops independent of actor and oracle --------------

#[test]
fn lg6_the_ceiling_stops_the_loop_regardless_of_actor_or_oracle_state() {
    // check_ceiling's own signature excludes an actor/oracle parameter
    // entirely — independence is structural, exercised here through the
    // facade re-export.
    let ceiling = Ceiling {
        max_iterations: 2,
        budget_ref: None,
        deadline: None,
        patience: 10,
    };
    assert_eq!(
        check_ceiling(&ceiling, 2, None, 0, 0),
        ControlFlow::Stop(cronus_core::loop_runner::StopReason::MaxIterations)
    );
}

// --- Escalation promotes only on held-out gain with hard preconditions

#[test]
fn lg7_escalation_refuses_on_shared_lineage_even_with_a_winning_metric() {
    struct AlwaysNovel;
    impl NoveltySource for AlwaysNovel {
        fn has_external_input(&self) -> bool {
            true
        }
    }
    let same_lineage = Oracle::Judge {
        lineage: "worker-model".to_string(),
    };
    let outcome = escalate(
        &same_lineage,
        "worker-model",
        &AlwaysNovel,
        &HeldOutMeasurement {
            before_metric: 0.10,
            after_metric: 0.95,
            regression: 0.00,
            margin: 0.10,
            regression_bound: 0.05,
        },
    );
    assert_eq!(
        outcome,
        EscalationOutcome::Refused(RefusalReason::OracleNotSeparated)
    );
}

#[test]
fn lg7_escalation_promotes_on_a_real_held_out_gain_with_both_preconditions_met() {
    struct AlwaysNovel;
    impl NoveltySource for AlwaysNovel {
        fn has_external_input(&self) -> bool {
            true
        }
    }
    let separated = Oracle::Judge {
        lineage: "reviewer-model".to_string(),
    };
    let outcome = escalate(
        &separated,
        "worker-model",
        &AlwaysNovel,
        &HeldOutMeasurement {
            before_metric: 0.60,
            after_metric: 0.80,
            regression: 0.01,
            margin: 0.10,
            regression_bound: 0.05,
        },
    );
    assert_eq!(outcome, EscalationOutcome::Promoted);
}

// --- The mutation ledger is append-only ------------------------------

#[test]
fn lg8_every_applied_mutation_appends_to_the_ledger_across_iterations() {
    let spec = exec_spec(5, [MutableArtifact::Plan]);
    let mut backend = FakeBackend::new(vec![
        (
            vec![Mutation {
                artifact: MutableArtifact::Plan,
                summary: "first pass".to_string(),
            }],
            turn(false, Some("keep going")),
        ),
        (
            vec![Mutation {
                artifact: MutableArtifact::Plan,
                summary: "second pass".to_string(),
            }],
            turn(true, None),
        ),
    ]);
    let report = run_execution(&spec, &mut backend, "the plan");
    assert_eq!(
        report.ledger.len(),
        2,
        "both applied mutations must be present"
    );
    assert_eq!(report.ledger[0].summary, "first pass");
    assert_eq!(report.ledger[1].summary, "second pass");
}

// --- The cheapest trustworthy oracle is preferred when present ------

#[test]
fn lg9_a_declared_deterministic_oracle_is_preferred_over_the_fallback() {
    let deterministic = Oracle::Deterministic {
        validator: "tests".to_string(),
    };
    let chosen = select_oracle(Some(deterministic.clone()), Oracle::Human);
    assert_eq!(chosen, deterministic);
}

// --- The objective survives in-session reduction and resumes -------

#[test]
fn lg10_the_objective_survives_aggressive_trimming_and_resumes_with_current_progress() {
    let mut slot = ObjectiveSlot {
        objective: "keep the release green".to_string(),
        progress: "2 of 5 checks passing".to_string(),
    };
    let mut entries = vec![
        objective_context_entry(&slot),
        cronus_core::context_mgmt::ContextEntry::new("user", "chatter", 100),
    ];
    trim_cascade(&mut entries, 0); // force maximum eviction
    assert_eq!(
        entries.len(),
        1,
        "only the protected objective entry survives"
    );
    assert!(entries[0].body.contains("2 of 5 checks passing"));

    // Progress advances before the next lossy reduction (CC-10 timing);
    // a simulated compaction event then drops the entire prior turn.
    update_progress(&mut slot, "5 of 5 checks passing — release green");
    let mut next_turn: Vec<cronus_core::context_mgmt::ContextEntry> = Vec::new();
    re_project_objective(&mut next_turn, &slot);
    assert!(
        next_turn[0].body.contains("5 of 5 checks passing"),
        "resumes with the current progress, not the compacted-away value"
    );
}
