//! Environment & Evaluation — the `EnvironmentProvider` extension role.
//!
//! Lets a workflow run against a task world that produces a graded outcome: a
//! reproducible reset/step/evaluate lifecycle, a typed [`Reward`], and a
//! frozen-evaluation boundary that keeps grading read-only over a completed
//! run. The built-in [`StubEnvironment`] satisfies the interface with no I/O,
//! matching the [`crate::executor::StubProvider`] / [`crate::executor::DefaultDialogProvider`]
//! convention.
//!
//! # Scope (v1)
//!
//! `run_with_environment` sequences exactly one `open → reset → execute →
//! evaluate → release` cycle per call: the reset [`Observation`] seeds the
//! workflow's `$in`, the whole workflow runs as the graded unit, and `evaluate`
//! scores the completed run. [`EnvironmentProvider::step`] is a complete,
//! independently-testable part of the trait contract, but no nodus syntax
//! exists yet to bind a workflow step to a mid-run environment action, so the
//! automatic combinator does not call it — the same "interface present, host
//! wiring pending" scoping already used for `StorageProvider`/`PolicyProvider`.
//! A host MAY drive `step` directly against its own `EnvironmentProvider` outside
//! the combinator.

use crate::executor::{RunResult, Value};
use std::collections::BTreeMap;

// ─── Core types ────────────────────────────────────────────────────────────────

/// Stable identifier for one task in an environment's catalog (NE-6).
pub type TaskId = String;

/// Deterministic seed for `open`/`reset` (NE-2).
pub type Seed = u64;

/// What the environment hands back after `reset`/`step`. Opaque to nodus core —
/// its shape is the environment's `profile()` concern (LP-4).
#[derive(Debug, Clone)]
pub struct Observation(pub Value);

/// What a caller submits to `step`. Opaque to nodus core, mirroring [`Observation`].
#[derive(Debug, Clone)]
pub struct Action(pub Value);

/// An isolated per-run environment handle (NE-7). Each `open()` call returns a
/// fresh, independent value — no shared mutable state is reachable through it.
/// Constructed by an [`EnvironmentProvider`] implementation inside `open`; hosts
/// writing their own provider use [`Instance::new`].
#[derive(Debug, Clone)]
pub struct Instance {
    task: TaskId,
    seed: Seed,
}

impl Instance {
    /// Construct a fresh instance for `task`/`seed`. Called by an
    /// [`EnvironmentProvider::open`] implementation.
    pub fn new(task: TaskId, seed: Seed) -> Self {
        Instance { task, seed }
    }

    /// The task this instance was opened for.
    pub fn task(&self) -> &TaskId {
        &self.task
    }

    /// The seed this instance was opened with.
    pub fn seed(&self) -> Seed {
        self.seed
    }
}

// ─── Reward ───────────────────────────────────────────────────────────────────

/// Typed, non-control grading outcome (NE-5). Never bound to a workflow
/// variable and never branched on mid-run; a low score is not a run failure
/// and a high score is not a run success — grading and run status are
/// orthogonal axes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reward {
    /// `None` = ungraded (NE-9 no-op default — a host that supplies no scorer
    /// still produces a valid, honestly-absent reward).
    pub score: Option<f64>,
    /// Host-defined breakdown. Data-safety bounded like [`crate::observability::FieldDescriptor`] —
    /// callers should not place raw user content here.
    pub metadata: BTreeMap<String, String>,
}

impl Reward {
    /// The NE-9 no-op reward: absent score, empty metadata.
    pub fn no_op() -> Self {
        Reward::default()
    }
}

// ─── Grading mode (NE-11) ───────────────────────────────────────────────────────

/// Closed set of ways `evaluate` may produce its [`Reward`] (NE-11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GradingMode {
    /// A deterministic checker over the frozen trajectory / final state.
    #[default]
    Automated,
    /// A model scores against a published rubric (function-scoped auxiliary
    /// role, NE-8 — never the workflow's own policy model).
    Judge,
    /// The checker runs first as a floor; a judge runs only where the checker
    /// passed and may lower but never rescue a checker-failed result.
    Hybrid,
}

/// Compose a checker result and an optional judge result per `mode` (NE-11).
///
/// `checker_passed` is the host's own explicit pass/fail verdict — nodus never
/// infers pass/fail from `checker.score` (NE-9 metric neutrality: nodus owns no
/// scoring semantics, so it cannot invent a numeric pass threshold).
///
/// - `Automated` returns `checker` unchanged; `judge` is ignored.
/// - `Judge` returns `judge` (or the no-op reward if absent); `checker` is ignored.
/// - `Hybrid`: a failed checker is returned as-is (the judge cannot rescue it);
///   otherwise the combined score is the lower of the two (the judge may only
///   lower), taking the judge's metadata.
pub fn grade(
    mode: GradingMode,
    checker: Reward,
    checker_passed: bool,
    judge: Option<Reward>,
) -> Reward {
    match mode {
        GradingMode::Automated => checker,
        GradingMode::Judge => judge.unwrap_or_else(Reward::no_op),
        GradingMode::Hybrid => {
            if !checker_passed {
                return checker;
            }
            match judge {
                None => checker,
                Some(j) => {
                    let score = match (checker.score, j.score) {
                        (Some(c), Some(js)) => Some(c.min(js)),
                        (Some(c), None) => Some(c),
                        (None, Some(js)) => Some(js),
                        (None, None) => None,
                    };
                    Reward {
                        score,
                        metadata: j.metadata,
                    }
                }
            }
        }
    }
}

// ─── Budget (NE-13) ─────────────────────────────────────────────────────────────

/// Fixed resource ceiling a graded run is uniformly halted at (NE-13). Any
/// subset of fields may be set; `None` fields impose no limit.
///
/// `max_tokens` is accepted here for profile-identity completeness (NE-13
/// requires the declared budget to travel with the archived candidate even
/// when a component is unenforced) but is **not yet enforced** by
/// `run_with_environment` — no token-accounting seam exists on
/// [`crate::executor::ModelProvider`] today. This is a documented gap, the
/// same "interface declared, host integration pending" precedent already used
/// for `StorageProvider`/`PolicyProvider`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Budget {
    pub wall_clock_ms: Option<u64>,
    pub max_steps: Option<u32>,
    pub max_tokens: Option<u64>,
}

// ─── Profile (NE-6) ─────────────────────────────────────────────────────────────

/// What an environment publishes before any run: the interchangeability
/// contract two environments must share to be substitutable for the same
/// workflow (NE-6).
#[derive(Debug, Clone)]
pub struct EnvironmentProfile {
    /// Orthogonal slice labels (e.g. `capability`, `complexity`) — declared on
    /// the task, not inferred, so a slice is stable across runs (§4.7).
    pub labels: BTreeMap<String, String>,
    /// How `evaluate` grades a run against this profile (NE-11).
    pub grading: GradingMode,
    /// Fixed resource ceiling, if any (NE-13). `None` behaves as today.
    pub budget: Option<Budget>,
    /// Identity of the host encoder `budget.max_tokens` is denominated in
    /// (NE-14). Opaque to the crate — no tokenizer or counting rule in core
    /// (LP-1/LP-2). A profile whose budget carries no `max_tokens` needs no
    /// measure and is unaffected by leaving this `None`.
    pub token_measure: Option<String>,
}

impl EnvironmentProfile {
    /// The empty profile the built-in [`StubEnvironment`] publishes: no
    /// labels, automated (no-op) grading, no budget, no measure.
    pub fn empty() -> Self {
        EnvironmentProfile {
            labels: BTreeMap::new(),
            grading: GradingMode::Automated,
            budget: None,
            token_measure: None,
        }
    }
}

// ─── EnvironmentProvider ────────────────────────────────────────────────────────

/// Host-implemented task environment (NE-1). Nodus core ships exactly one
/// built-in deterministic environment ([`StubEnvironment`]) for in-process
/// testing; concrete worlds live outside the crate (LP-2).
///
/// # Lifecycle (NE-2)
///
/// `open` → `reset` → (optionally `step`, zero or more times) → `evaluate` →
/// `release`. `release` is mandatory and MUST be idempotent — implementations
/// must tolerate a second call on an already-released instance as a no-op.
/// `reset`/`step` are deterministic given `(task, seed, prior actions)`.
pub trait EnvironmentProvider {
    /// The addressable task catalog (NE-6).
    fn task_ids(&self) -> Vec<TaskId>;

    /// The interchangeability contract for this environment (NE-6).
    fn profile(&self) -> EnvironmentProfile;

    /// Open a fresh, isolated instance for `task`/`seed` (NE-2/NE-7).
    fn open(&self, task: &TaskId, seed: Seed) -> Instance;

    /// Produce the initial observation. Deterministic given `(task, seed)` (NE-2).
    fn reset(&self, inst: &mut Instance) -> Observation;

    /// Apply `action`, produce the resulting observation. Deterministic given
    /// `(task, seed, prior actions)` (NE-2). Not called by the v1
    /// `run_with_environment` combinator (see module docs); available for a
    /// host to drive directly.
    fn step(&self, inst: &mut Instance, action: Action) -> Observation;

    /// Grade a completed, frozen run (NE-4). Read-only: MUST NOT mutate `inst`
    /// or the run it grades. Two calls over the same frozen instance MUST
    /// return equal rewards.
    fn evaluate(&self, inst: &Instance) -> Reward;

    /// Release `inst`. Mandatory; MUST be idempotent — a second call on an
    /// already-released instance is a no-op, never a panic (NE-7).
    fn release(&self, inst: Instance);
}

/// Built-in, deterministic, no-I/O environment (NE-1). `task_ids` publishes a
/// single stub task; `reset` produces `Value::Null`; `step` echoes its action
/// back as the observation (a pure function of `action` alone, so determinism
/// holds trivially); `evaluate` always returns the NE-9 no-op reward.
pub struct StubEnvironment;

impl EnvironmentProvider for StubEnvironment {
    fn task_ids(&self) -> Vec<TaskId> {
        vec!["__stub__".to_string()]
    }

    fn profile(&self) -> EnvironmentProfile {
        EnvironmentProfile::empty()
    }

    fn open(&self, task: &TaskId, seed: Seed) -> Instance {
        Instance::new(task.clone(), seed)
    }

    fn reset(&self, _inst: &mut Instance) -> Observation {
        Observation(Value::Null)
    }

    fn step(&self, _inst: &mut Instance, action: Action) -> Observation {
        Observation(action.0)
    }

    fn evaluate(&self, _inst: &Instance) -> Reward {
        Reward::no_op()
    }

    fn release(&self, _inst: Instance) {
        // No resources held; releasing twice is trivially a no-op (NE-7).
    }
}

// ─── Instance guard (NE-7 mandatory + idempotent release) ──────────────────────

/// Guarantees `release` runs exactly once, even if `evaluate` or a caller
/// callback panics between `open` and the end of the run (NE-7).
pub(crate) struct InstanceGuard<'e> {
    env: &'e dyn EnvironmentProvider,
    inst: Option<Instance>,
}

impl<'e> InstanceGuard<'e> {
    pub(crate) fn new(env: &'e dyn EnvironmentProvider, inst: Instance) -> Self {
        InstanceGuard {
            env,
            inst: Some(inst),
        }
    }

    pub(crate) fn get_mut(&mut self) -> &mut Instance {
        self.inst.as_mut().expect("instance already released")
    }

    pub(crate) fn get(&self) -> &Instance {
        self.inst.as_ref().expect("instance already released")
    }
}

impl Drop for InstanceGuard<'_> {
    fn drop(&mut self) {
        if let Some(inst) = self.inst.take() {
            self.env.release(inst);
        }
    }
}

// ─── Candidate result (NE-12) ───────────────────────────────────────────────────

/// Archivable, content-addressable outcome of one graded run (NE-12). Nodus
/// supplies only this substrate; the candidate space, mutation, search
/// strategy, and frontier are entirely host-side (LP-1/LP-2).
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateResult {
    /// Deterministic digest of the canonical workflow source. A `std`-library
    /// digest (zero-dep, LP-1) — stable within one build, **not** guaranteed
    /// stable across Rust versions/platforms. A host requiring durable
    /// cross-version archival stability computes its own cryptographic digest
    /// over the exposed source (LP-2), the same pattern as LP-9 attestation.
    pub workflow_digest: String,
    /// The reward this candidate earned.
    pub reward: Reward,
    /// The caller's own run-tracking identifier (the runtime holds no separate
    /// trajectory store to reference — the trajectory was already delivered to
    /// the caller's own `AuditProvider`).
    pub trajectory_ref: String,
    /// The budget this reward was earned under, if any. Rewards earned under
    /// different budgets are not comparable — a host optimizer MUST partition
    /// its frontier by `(profile, budget)` (NE-13).
    pub budget: Option<Budget>,
    /// The measure `budget.max_tokens` was denominated in, if any. Rewards
    /// earned under different measures are not comparable either — a host
    /// optimizer MUST partition its frontier by `(profile, budget, measure)`,
    /// never `(profile, budget)` alone (NE-14).
    pub token_measure: Option<String>,
}

/// Digest of raw `source` text (NE-12), under the same versioned scheme as
/// [`crate::executor::digest_ast`]. Used only when `source` does not parse into
/// a workflow, so there is no AST to take an identity from.
fn digest_source(source: &str) -> String {
    crate::executor::digest_text(source)
}

// ─── EnvRunResult ───────────────────────────────────────────────────────────────

/// The result of one `run_with_environment` cycle: the workflow's own
/// [`RunResult`] plus the [`Reward`], returned alongside — never bound into
/// `RunResult.vars` (NE-5).
#[derive(Debug)]
pub struct EnvRunResult {
    pub result: RunResult,
    pub reward: Reward,
    /// `true` when the run was uniformly halted by the profile's [`Budget`]
    /// rather than reaching a natural terminal state — a normal graded
    /// outcome (NE-13), reflected in `result.status == Status::Partial`.
    pub budget_halted: bool,
}

impl EnvRunResult {
    /// Build an archivable candidate tuple (NE-12). `workflow_source` is the
    /// source the caller ran, hashed for `workflow_digest` at the AST level —
    /// the same `digest_ast` computation `ReproRecipe.workflow_digest` uses,
    /// so a `CandidateResult` and a `RunManifest` for the same run agree on
    /// the pinned generation. Falls
    /// back to the raw-text `digest_source` hash only if `workflow_source`
    /// fails to parse (the caller passed something other than what it ran) —
    /// an honest degrade, not a panic; `run_id` is the caller's own tracking
    /// identifier; `budget` should be the profile's declared budget (or
    /// `None`) so different-budget results are never silently compared.
    pub fn candidate(
        &self,
        workflow_source: &str,
        run_id: &str,
        budget: Option<Budget>,
        token_measure: Option<String>,
    ) -> CandidateResult {
        let workflow_digest = match crate::parser::Parser::parse(workflow_source) {
            Ok(ast) => crate::executor::digest_ast(&ast),
            Err(_) => digest_source(workflow_source),
        };
        CandidateResult {
            workflow_digest,
            reward: self.reward.clone(),
            trajectory_ref: run_id.to_string(),
            budget,
            token_measure,
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── StubEnvironment ────────────────────────────────────────────

    #[test]
    fn stub_task_ids_is_single_stub_task() {
        let env = StubEnvironment;
        assert_eq!(env.task_ids(), vec!["__stub__".to_string()]);
    }

    #[test]
    fn stub_step_echoes_action() {
        let env = StubEnvironment;
        let mut inst = env.open(&"__stub__".to_string(), 42);
        let obs = env.step(&mut inst, Action(Value::Text("hello".to_string())));
        assert_eq!(obs.0, Value::Text("hello".to_string()));
    }

    #[test]
    fn stub_evaluate_returns_no_op_reward() {
        let env = StubEnvironment;
        let inst = env.open(&"__stub__".to_string(), 1);
        let reward = env.evaluate(&inst);
        assert_eq!(reward.score, None);
        assert!(reward.metadata.is_empty());
    }

    // ── Lifecycle determinism + isolation ──────────────────────────

    #[test]
    fn reset_step_deterministic_across_two_runs() {
        let env = StubEnvironment;
        let mut inst_a = env.open(&"t1".to_string(), 7);
        let obs_a1 = env.reset(&mut inst_a);
        let obs_a2 = env.step(&mut inst_a, Action(Value::Int(3)));

        let mut inst_b = env.open(&"t1".to_string(), 7);
        let obs_b1 = env.reset(&mut inst_b);
        let obs_b2 = env.step(&mut inst_b, Action(Value::Int(3)));

        assert_eq!(obs_a1.0, obs_b1.0);
        assert_eq!(obs_a2.0, obs_b2.0);
    }

    #[test]
    fn double_release_is_a_no_op() {
        let env = StubEnvironment;
        let inst1 = env.open(&"t1".to_string(), 1);
        let inst2 = env.open(&"t1".to_string(), 1);
        env.release(inst1);
        // A second release (on an independently-opened, isolated instance)
        // must not panic — this is the idempotency contract (NE-7).
        env.release(inst2);
    }

    #[test]
    fn open_returns_isolated_instances() {
        let env = StubEnvironment;
        let inst_a = env.open(&"t1".to_string(), 1);
        let inst_b = env.open(&"t2".to_string(), 2);
        assert_eq!(inst_a.task(), "t1");
        assert_eq!(inst_b.task(), "t2");
        assert_eq!(inst_a.seed(), 1);
        assert_eq!(inst_b.seed(), 2);
    }

    // ── InstanceGuard release-always-runs (NE-7) ─────────────────────────────

    struct CountingEnv {
        released: std::cell::RefCell<u32>,
    }

    impl EnvironmentProvider for CountingEnv {
        fn task_ids(&self) -> Vec<TaskId> {
            vec!["c".to_string()]
        }
        fn profile(&self) -> EnvironmentProfile {
            EnvironmentProfile::empty()
        }
        fn open(&self, task: &TaskId, seed: Seed) -> Instance {
            Instance::new(task.clone(), seed)
        }
        fn reset(&self, _inst: &mut Instance) -> Observation {
            Observation(Value::Null)
        }
        fn step(&self, _inst: &mut Instance, action: Action) -> Observation {
            Observation(action.0)
        }
        fn evaluate(&self, _inst: &Instance) -> Reward {
            Reward::no_op()
        }
        fn release(&self, _inst: Instance) {
            *self.released.borrow_mut() += 1;
        }
    }

    #[test]
    fn guard_releases_exactly_once_on_drop() {
        let env = CountingEnv {
            released: std::cell::RefCell::new(0),
        };
        {
            let inst = env.open(&"c".to_string(), 1);
            let _guard = InstanceGuard::new(&env, inst);
        }
        assert_eq!(*env.released.borrow(), 1);
    }

    // ── Grading modes (NE-11) ──────────────────────────────────────

    fn reward(score: f64) -> Reward {
        Reward {
            score: Some(score),
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn hybrid_failing_checker_judge_cannot_rescue() {
        let checker = reward(0.0);
        let judge = Some(reward(0.9));
        let out = grade(GradingMode::Hybrid, checker.clone(), false, judge);
        assert_eq!(out, checker);
    }

    #[test]
    fn hybrid_passing_checker_lower_judge_wins() {
        let checker = reward(0.9);
        let judge = Some(reward(0.4));
        let out = grade(GradingMode::Hybrid, checker, true, judge);
        assert_eq!(out.score, Some(0.4));
    }

    #[test]
    fn hybrid_passing_checker_no_judge_keeps_checker() {
        let checker = reward(0.8);
        let out = grade(GradingMode::Hybrid, checker.clone(), true, None);
        assert_eq!(out, checker);
    }

    #[test]
    fn automated_mode_never_uses_judge() {
        let checker = reward(0.5);
        let judge = Some(reward(0.99));
        let out = grade(GradingMode::Automated, checker.clone(), true, judge);
        assert_eq!(out, checker);
    }

    #[test]
    fn judge_mode_ignores_checker() {
        let checker = reward(0.1);
        let judge = reward(0.7);
        let out = grade(GradingMode::Judge, checker, true, Some(judge.clone()));
        assert_eq!(out, judge);
    }

    #[test]
    fn judge_mode_absent_judge_is_no_op() {
        let out = grade(GradingMode::Judge, reward(0.5), true, None);
        assert_eq!(out, Reward::no_op());
    }

    // ── Candidate digest (NE-12) ───────────────────────────────────

    #[test]
    fn same_source_same_digest() {
        let a = digest_source("§wf:x v1.0");
        let b = digest_source("§wf:x v1.0");
        assert_eq!(a, b);
    }

    #[test]
    fn different_source_different_digest() {
        let a = digest_source("§wf:x v1.0");
        let b = digest_source("§wf:y v1.0");
        assert_ne!(a, b);
    }

    #[test]
    fn candidate_carries_budget_and_run_id() {
        let result = EnvRunResult {
            result: RunResult {
                workflow: "wf:x".to_string(),
                status: crate::executor::Status::Ok,
                out: Value::Null,
                log: Vec::new(),
                errors: Vec::new(),
                flags: Vec::new(),
                vars: std::collections::HashMap::new(),
                resume: None,
            },
            reward: reward(1.0),
            budget_halted: false,
        };
        let budget = Some(Budget {
            max_steps: Some(5),
            ..Default::default()
        });
        let candidate = result.candidate("source", "run-42", budget, None);
        assert_eq!(candidate.trajectory_ref, "run-42");
        assert_eq!(candidate.budget, budget);
        assert_eq!(candidate.reward, reward(1.0));
    }

    #[test]
    fn candidate_digest_falls_back_to_source_hash_when_unparseable() {
        // NE-12/HO-20 unification: candidate() prefers digest_ast, but an
        // unparseable workflow_source (the caller passed something other than
        // what it actually ran) must degrade to the old digest_source hash
        // rather than panicking or silently producing an empty/placeholder
        // digest. Pin the fallback value explicitly — the pre-unification
        // tests exercised this path but never asserted on the digest itself.
        let result = EnvRunResult {
            result: RunResult {
                workflow: "wf:x".to_string(),
                status: crate::executor::Status::Ok,
                out: Value::Null,
                log: Vec::new(),
                errors: Vec::new(),
                flags: Vec::new(),
                vars: std::collections::HashMap::new(),
                resume: None,
            },
            reward: reward(1.0),
            budget_halted: false,
        };
        let not_a_workflow = "this is not valid nodus source";
        let candidate = result.candidate(not_a_workflow, "run-fallback", None, None);
        assert_eq!(
            candidate.workflow_digest,
            digest_source(not_a_workflow),
            "an unparseable source must fall back to the raw-text digest"
        );
    }
}
