//! The declared contract a loop constructs before it runs.
//!
//! `MutableArtifact` has no `Criteria` variant: the success criteria that
//! judge a loop can never be named as mutable through this type. That is the
//! anti-drift spine — enforced by the compiler, not by a runtime check
//! a compromised actor could bypass.

use std::collections::HashSet;

/// Every autonomous loop declares which of the two kinds it is. An
/// evolution loop may nest an execution loop for candidate scoring, but each
/// level declares its own class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopClass {
    /// Re-attempts a fixed task to a done-condition; nothing about the task
    /// or its success criteria changes between iterations.
    Execution,
    /// Deliberately changes the artifacts it works from between iterations
    /// to improve across a task set.
    Evolution,
}

/// The closed taxonomy of artifact kinds an iteration may
/// change. There is deliberately no `Criteria` variant — the success
/// criteria of a loop are structurally unreachable through this type, so a
/// `MutationManifest` can never declare them mutable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MutableArtifact {
    /// Attempt-local working files, never carried forward.
    Scratch,
    /// The task decomposition / what is left.
    Plan,
    /// Accumulated facts, memory entries, exemplars.
    Knowledge,
    /// The data a validator runs against (not the validator's pass rule).
    ValidationInput,
    /// The actor's system/instruction text.
    Prompt,
    /// The actor's available capabilities.
    Tools,
}

/// A loop's declared mutation rights — the artifact kinds an iteration
/// may change. Anything not in `mutable` is immutable for that loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationManifest {
    pub tier: u8,
    pub mutable: HashSet<MutableArtifact>,
}

impl MutationManifest {
    /// Construct a manifest declaring the given mutable artifact kinds.
    pub fn new(tier: u8, mutable: impl IntoIterator<Item = MutableArtifact>) -> Self {
        MutationManifest {
            tier,
            mutable: mutable.into_iter().collect(),
        }
    }

    /// Tier 0 (same-prompt): nothing is mutable between iterations.
    pub fn same_prompt() -> Self {
        MutationManifest {
            tier: 0,
            mutable: HashSet::new(),
        }
    }

    /// Whether an iteration may change the given artifact kind.
    pub fn allows(&self, artifact: MutableArtifact) -> bool {
        self.mutable.contains(&artifact)
    }

    /// The manifest is part of the loop specification and is
    /// recoverable from the run record — a plain-data projection with no
    /// information loss.
    pub fn to_record(&self) -> (u8, Vec<MutableArtifact>) {
        let mut mutable: Vec<MutableArtifact> = self.mutable.iter().copied().collect();
        mutable.sort_by_key(|a| *a as u8);
        (self.tier, mutable)
    }

    /// Reconstruct a manifest from its run-record form.
    pub fn from_record(tier: u8, mutable: Vec<MutableArtifact>) -> Self {
        MutationManifest {
            tier,
            mutable: mutable.into_iter().collect(),
        }
    }
}

/// The independent termination ceiling. Evaluated before every
/// iteration, regardless of the actor's or the oracle's state.
#[derive(Debug, Clone, PartialEq)]
pub struct Ceiling {
    pub max_iterations: u32,
    /// Reference to the budget policy this loop draws from (budget-engine).
    /// The real handle is bound by the facade; the domain tier holds only
    /// the reference.
    pub budget_ref: Option<String>,
    /// Unix timestamp deadline, if any. A plain integer (not a clock type)
    /// keeps this tier free of wall-clock I/O.
    pub deadline: Option<u64>,
    /// Consecutive no-progress iterations tolerated before stopping.
    pub patience: u32,
}

/// The standing objective and its progress cursor for a
/// continuous-session loop that compacts in place. Re-projected into every
/// turn from a durable slot so mid-session compaction can never drop it. A
/// `LoopSpec` with no `ObjectiveSlot` is a discrete-iteration loop governed
/// by alone (fresh context per iteration, nothing to re-project).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectiveSlot {
    pub objective: String,
    pub progress: String,
}

/// Who holds the right to declare an iteration "done". `Judge` and
/// `Human` compare lineage against the actor to decide whether a termination
/// is full-confidence or reduced-confidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Oracle {
    /// A mechanical check: tests, exit code, schema/criteria validator.
    Deterministic { validator: String },
    /// An independent judge model or reviewer; `lineage` names its binding
    /// so the governor can compare it against the actor's.
    Judge { lineage: String },
    /// A human approves "done" through the approval gate.
    Human,
}

/// The single kind of isolated workspace a loop iteration runs in today.
/// A one-variant enum, not a speculative multi-backend abstraction: the
/// engine has exactly one execution-workspace mechanism (the git worktree),
/// and this field exists so the facade knows which one to allocate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceKind {
    Worktree,
}

/// The full declared contract a loop runs under.
#[derive(Debug, Clone, PartialEq)]
pub struct LoopSpec {
    pub class: LoopClass,
    pub manifest: MutationManifest,
    pub oracle: Oracle,
    pub ceiling: Ceiling,
    pub workspace_kind: WorkspaceKind,
    pub objective_slot: Option<ObjectiveSlot>,
}

/// An oracle's judgment on one iteration.
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    pub done: bool,
    /// Set when the oracle's lineage matches the actor's — a permitted but
    /// weaker termination.
    pub reduced_confidence: bool,
    pub feedback: Option<String>,
}

/// One append-only entry in a loop's mutation ledger.
#[derive(Debug, Clone, PartialEq)]
pub struct LedgerEntry {
    pub iteration: u32,
    pub artifact: MutableArtifact,
    pub summary: String,
    pub why: String,
}

/// The result of a completed or ceiling-stopped execution-loop run.
#[derive(Debug, Clone, PartialEq)]
pub enum LoopOutcome {
    Done(Verdict),
    Stopped(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Criteria are structurally unreachable ------------------------

    #[test]
    fn the_mutable_artifact_taxonomy_is_exactly_the_six_non_criteria_kinds() {
        // Exhaustive match: this compiles only because these six variants are
        // the whole enum. A `Criteria` arm would not compile — the guarantee holds by
        // construction, not by this test.
        let all = [
            MutableArtifact::Scratch,
            MutableArtifact::Plan,
            MutableArtifact::Knowledge,
            MutableArtifact::ValidationInput,
            MutableArtifact::Prompt,
            MutableArtifact::Tools,
        ];
        for artifact in all {
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

    #[test]
    fn a_manifest_declaring_all_six_kinds_allows_each_of_them() {
        let manifest = MutationManifest::new(
            3,
            [
                MutableArtifact::Scratch,
                MutableArtifact::Plan,
                MutableArtifact::Knowledge,
                MutableArtifact::ValidationInput,
                MutableArtifact::Prompt,
                MutableArtifact::Tools,
            ],
        );
        assert!(manifest.allows(MutableArtifact::Scratch));
        assert!(manifest.allows(MutableArtifact::Plan));
        assert!(manifest.allows(MutableArtifact::Knowledge));
        assert!(manifest.allows(MutableArtifact::ValidationInput));
        assert!(manifest.allows(MutableArtifact::Prompt));
        assert!(manifest.allows(MutableArtifact::Tools));
    }

    // --- A declared class is required to construct a spec -------------

    #[test]
    fn constructing_a_loop_spec_requires_a_declared_class() {
        // Omitting `class` from the struct literal is a compile error (a class rule
        // by construction); this test proves the value round-trips once
        // declared.
        let spec = LoopSpec {
            class: LoopClass::Execution,
            manifest: MutationManifest::same_prompt(),
            oracle: Oracle::Deterministic {
                validator: "exit_code".to_string(),
            },
            ceiling: Ceiling {
                max_iterations: 5,
                budget_ref: None,
                deadline: None,
                patience: 2,
            },
            workspace_kind: WorkspaceKind::Worktree,
            objective_slot: None,
        };
        assert_eq!(spec.class, LoopClass::Execution);
    }

    // --- Tier 0 same-prompt has an empty mutable set -------------------------

    #[test]
    fn a_tier_0_same_prompt_manifest_has_an_empty_mutable_set() {
        let manifest = MutationManifest::same_prompt();
        assert_eq!(manifest.tier, 0);
        assert!(manifest.mutable.is_empty());
        assert!(!manifest.allows(MutableArtifact::Plan));
    }

    // --- The manifest recovers from its run-record form ---------------

    #[test]
    fn a_manifest_round_trips_through_its_record_form_without_loss() {
        let original =
            MutationManifest::new(2, [MutableArtifact::Plan, MutableArtifact::Knowledge]);
        let (tier, mutable) = original.to_record();
        let restored = MutationManifest::from_record(tier, mutable);
        assert_eq!(restored, original);
    }

    // --- Oracle lineage shapes are constructible and distinct ---------

    #[test]
    fn oracle_kinds_are_distinct_and_carry_their_lineage_data() {
        let deterministic = Oracle::Deterministic {
            validator: "tests".to_string(),
        };
        let judge = Oracle::Judge {
            lineage: "reviewer-model".to_string(),
        };
        let human = Oracle::Human;
        assert_ne!(deterministic, judge);
        assert_ne!(judge, human);
    }

    // --- An objective slot is optional and carries progress ----------

    #[test]
    fn a_loop_spec_with_no_objective_slot_is_a_discrete_iteration_loop() {
        let spec = LoopSpec {
            class: LoopClass::Execution,
            manifest: MutationManifest::same_prompt(),
            oracle: Oracle::Human,
            ceiling: Ceiling {
                max_iterations: 1,
                budget_ref: None,
                deadline: None,
                patience: 0,
            },
            workspace_kind: WorkspaceKind::Worktree,
            objective_slot: None,
        };
        assert!(spec.objective_slot.is_none());

        let continuous = LoopSpec {
            objective_slot: Some(ObjectiveSlot {
                objective: "keep the release green".to_string(),
                progress: "3 of 5 checks passing".to_string(),
            }),
            ..spec
        };
        assert!(continuous.objective_slot.is_some());
    }
}
