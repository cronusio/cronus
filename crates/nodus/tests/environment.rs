//! Integration tests for the Environment/Evaluation contract.
//!
//! Covers, end-to-end through the public `workflows::run_with_environment*`
//! surface: deterministic replay, the frozen-evaluation boundary (evaluate
//! strictly after execute, release always last), idempotent/guaranteed
//! release, the manifest fail-fast gate (no instance opened on
//! rejection), the hybrid grading floor, the budget halt, and the
//! candidate digest.

use nodus::environment::{
    Action, Budget, EnvironmentProfile, EnvironmentProvider, GradingMode, Instance, Observation,
    Reward, Seed, StubEnvironment, TaskId, grade,
};
use nodus::executor::Status;
use nodus::portability::{ExtensionRole, HostCapabilities};
use nodus::workflows;
use nodus::{AuditProvider, ExecutionEvent, RunManifest};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

// ─── Fixtures ───────────────────────────────────────────────────────────────

const ENV_WF: &str = r#"§wf:env_test v1.0
§runtime: { core: schema.nodus }
@in: { observation? }
@out: $out
@err: ESCALATE(human)
@steps:
  1. LOG($in.observation) → $out
  2. LOG($out)
  3. LOG($out)
  4. LOG($out)
"#;

/// Records call ordering (`reset`/`evaluate`/`release`) and lets a test assert
/// the frozen boundary — evaluate strictly after every step, release last.
struct InstrumentedEnv {
    calls: Arc<Mutex<Vec<&'static str>>>,
    opened: Arc<Mutex<u32>>,
}

impl InstrumentedEnv {
    fn new() -> Self {
        InstrumentedEnv {
            calls: Arc::new(Mutex::new(Vec::new())),
            opened: Arc::new(Mutex::new(0)),
        }
    }
}

impl EnvironmentProvider for InstrumentedEnv {
    fn task_ids(&self) -> Vec<TaskId> {
        vec!["t1".to_string()]
    }

    fn profile(&self) -> EnvironmentProfile {
        EnvironmentProfile::empty()
    }

    fn open(&self, task: &TaskId, seed: Seed) -> Instance {
        *self.opened.lock().unwrap() += 1;
        Instance::new(task.clone(), seed)
    }

    fn reset(&self, _inst: &mut Instance) -> Observation {
        self.calls.lock().unwrap().push("reset");
        Observation(nodus::executor::Value::Text("seeded".to_string()))
    }

    fn step(&self, _inst: &mut Instance, action: Action) -> Observation {
        self.calls.lock().unwrap().push("step");
        Observation(action.0)
    }

    fn evaluate(&self, _inst: &Instance) -> Reward {
        self.calls.lock().unwrap().push("evaluate");
        Reward {
            score: Some(1.0),
            metadata: Default::default(),
        }
    }

    fn release(&self, _inst: Instance) {
        self.calls.lock().unwrap().push("release");
    }
}

// ─── Deterministic replay ─────────────────────────────────────────────

#[test]
fn deterministic_replay_same_task_seed_same_observation() {
    let env = StubEnvironment;
    let host = HostCapabilities::builtin();

    let a = workflows::run_with_environment(
        ENV_WF,
        "env_test.nodus",
        None,
        &env,
        &host,
        &"__stub__".to_string(),
        7,
    )
    .expect("run a");
    let b = workflows::run_with_environment(
        ENV_WF,
        "env_test.nodus",
        None,
        &env,
        &host,
        &"__stub__".to_string(),
        7,
    )
    .expect("run b");

    assert_eq!(
        a.result.out, b.result.out,
        "identical (task, seed) must reproduce the same reset observation and thus the same $out"
    );
    assert_eq!(a.reward, b.reward);
}

// ─── Frozen-evaluation boundary ───────────────────────────────────────

#[test]
fn evaluate_runs_strictly_after_execute_release_runs_last() {
    let env = InstrumentedEnv::new();
    let host = HostCapabilities::builtin();

    let result = workflows::run_with_environment(
        ENV_WF,
        "env_test.nodus",
        None,
        &env,
        &host,
        &"t1".to_string(),
        1,
    )
    .expect("run");

    assert_eq!(result.result.status, Status::Ok);
    let calls = env.calls.lock().unwrap().clone();
    // v1 scope: no mid-run `step` calls (see crate::environment module docs) —
    // exactly reset, then evaluate (after the whole workflow ran), then release.
    assert_eq!(calls, vec!["reset", "evaluate", "release"]);
}

#[test]
fn two_evaluate_calls_over_one_frozen_instance_agree() {
    // Direct trait-level check: evaluate is a pure read-only projection.
    let env = StubEnvironment;
    let inst = env.open(&"__stub__".to_string(), 1);
    let r1 = env.evaluate(&inst);
    let r2 = env.evaluate(&inst);
    assert_eq!(r1, r2);
    env.release(inst);
}

// ─── manifest fail-fast — no instance opened on rejection ─────────────

#[test]
fn missing_environment_role_rejects_before_open() {
    let env = InstrumentedEnv::new();
    let host = HostCapabilities::new().with_role(ExtensionRole::Model); // no Environment

    let result = workflows::run_with_environment(
        ENV_WF,
        "env_test.nodus",
        None,
        &env,
        &host,
        &"t1".to_string(),
        1,
    )
    .expect("the manifest gate returns an EnvRunResult, not a parse error");

    assert_eq!(result.result.status, Status::Failed);
    assert!(
        result.result.log.is_empty(),
        "no step may execute on a rejected run"
    );
    assert!(
        result
            .result
            .errors
            .iter()
            .any(|e| e.code == "NODUS:CAPABILITY_UNMET" && e.reason.contains("Environment")),
        "rejection must name the missing Environment role; errors: {:?}",
        result.result.errors
    );
    assert_eq!(
        *env.opened.lock().unwrap(),
        0,
        "env.open must never be called when the manifest is unsatisfiable"
    );
    assert!(
        env.calls.lock().unwrap().is_empty(),
        "no lifecycle call may occur on a rejected run"
    );
}

#[test]
fn builtin_host_satisfies_environment_role() {
    let env = StubEnvironment;
    let host = HostCapabilities::builtin();
    let result = workflows::run_with_environment(
        ENV_WF,
        "env_test.nodus",
        None,
        &env,
        &host,
        &"__stub__".to_string(),
        1,
    )
    .expect("run");
    assert_eq!(
        result.result.status,
        Status::Ok,
        "builtin() provides Environment via StubEnvironment; errors: {:?}",
        result.result.errors
    );
}

// ─── hybrid grading floor (composition, exercised via public `grade`) ──

#[test]
fn hybrid_floor_end_to_end() {
    let failing_checker = Reward {
        score: Some(0.0),
        metadata: Default::default(),
    };
    let generous_judge = Reward {
        score: Some(1.0),
        metadata: Default::default(),
    };
    let out = grade(
        GradingMode::Hybrid,
        failing_checker.clone(),
        false,
        Some(generous_judge),
    );
    assert_eq!(
        out, failing_checker,
        "a checker-failed run must not be rescued by a lenient judge"
    );
}

// ─── budget halt is a normal outcome, not an error ─────────────────────

#[test]
fn max_steps_budget_halts_with_partial_status() {
    struct BudgetedEnv;
    impl EnvironmentProvider for BudgetedEnv {
        fn task_ids(&self) -> Vec<TaskId> {
            vec!["b".to_string()]
        }
        fn profile(&self) -> EnvironmentProfile {
            EnvironmentProfile {
                labels: Default::default(),
                grading: GradingMode::Automated,
                budget: Some(Budget {
                    max_steps: Some(2),
                    ..Default::default()
                }),
                token_measure: None,
            }
        }
        fn open(&self, task: &TaskId, seed: Seed) -> Instance {
            Instance::new(task.clone(), seed)
        }
        fn reset(&self, _inst: &mut Instance) -> Observation {
            Observation(nodus::executor::Value::Null)
        }
        fn step(&self, _inst: &mut Instance, action: Action) -> Observation {
            Observation(action.0)
        }
        fn evaluate(&self, _inst: &Instance) -> Reward {
            Reward {
                score: Some(0.5),
                metadata: Default::default(),
            }
        }
        fn release(&self, _inst: Instance) {}
    }

    let env = BudgetedEnv;
    let host = HostCapabilities::builtin();
    let result = workflows::run_with_environment(
        ENV_WF, // 4 steps declared; budget caps at 2
        "env_test.nodus",
        None,
        &env,
        &host,
        &"b".to_string(),
        1,
    )
    .expect("run");

    assert!(
        result.budget_halted,
        "a max_steps ceiling below the workflow's step count must halt the run"
    );
    assert_eq!(
        result.result.status,
        Status::Partial,
        "a budget halt is a normal graded outcome, never Failed"
    );
    assert_eq!(
        result.result.log.len(),
        2,
        "exactly max_steps steps may execute; log: {:?}",
        result.result.log
    );
    // evaluate still runs over the partial run and produces a reward.
    assert_eq!(result.reward.score, Some(0.5));
}

#[test]
fn no_budget_behaves_as_today() {
    let env = StubEnvironment;
    let host = HostCapabilities::builtin();
    let result = workflows::run_with_environment(
        ENV_WF,
        "env_test.nodus",
        None,
        &env,
        &host,
        &"__stub__".to_string(),
        1,
    )
    .expect("run");
    assert!(!result.budget_halted);
    assert_eq!(result.result.status, Status::Ok);
    assert_eq!(
        result.result.log.len(),
        4,
        "all four steps must run with no budget set"
    );
}

// ─── candidate digest ───────────────────────────────────────────────────

#[test]
fn candidate_digest_deterministic_and_content_addressed() {
    let env = StubEnvironment;
    let host = HostCapabilities::builtin();
    let result = workflows::run_with_environment(
        ENV_WF,
        "env_test.nodus",
        None,
        &env,
        &host,
        &"__stub__".to_string(),
        1,
    )
    .expect("run");

    let budget = Some(Budget {
        max_steps: Some(10),
        ..Default::default()
    });
    let c1 = result.candidate(ENV_WF, "run-1", budget, None);
    let c2 = result.candidate(ENV_WF, "run-1", budget, None);
    assert_eq!(
        c1.workflow_digest, c2.workflow_digest,
        "the same source must always produce the same digest"
    );
    assert_eq!(c1.budget, budget);
    assert_eq!(c1.trajectory_ref, "run-1");

    let different_source = ENV_WF.replace("env_test", "env_test_renamed");
    let c3 = result.candidate(&different_source, "run-1", budget, None);
    assert_ne!(
        c1.workflow_digest, c3.workflow_digest,
        "a changed workflow source must change the digest"
    );
}

/// Captures the `RunManifest` `run_complete` delivers — the same shape as
/// `tests/dialog.rs`'s `ManifestCapture`, trimmed to just the manifest.
#[derive(Clone)]
struct ManifestCapture {
    manifests: Arc<Mutex<Vec<RunManifest>>>,
}

impl ManifestCapture {
    fn new() -> Self {
        ManifestCapture {
            manifests: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl AuditProvider for ManifestCapture {
    fn record_event(&self, _event: ExecutionEvent) {}

    fn run_complete(&self, manifest: RunManifest) {
        self.manifests.lock().unwrap().push(manifest);
    }
}

#[test]
fn candidate_digest_agrees_with_repro_recipe_digest() {
    // Digest unification: CandidateResult.workflow_digest and
    // ReproRecipe.workflow_digest are built from two independent call sites
    // (environment.rs vs. executor.rs) and never cross-checked in production
    // code — this is the test that makes "they agree" a proven fact rather
    // than an assumption from both reusing digest_ast.
    let env = StubEnvironment;
    let host = HostCapabilities::builtin();
    let capture = ManifestCapture::new();
    let result = workflows::run_with_environment_and_audit(
        ENV_WF,
        "env_test.nodus",
        None,
        &env,
        &host,
        &"__stub__".to_string(),
        1,
        capture.clone(),
        "run-ne12",
        "2026-01-01T00:00:00Z",
    )
    .expect("run");

    let candidate = result.candidate(ENV_WF, "run-ne12", None, None);

    let manifests = capture.manifests.lock().unwrap();
    assert_eq!(manifests.len(), 1, "run_complete must fire exactly once");
    assert_eq!(
        candidate.workflow_digest, manifests[0].repro.workflow_digest,
        "CandidateResult and ReproRecipe must agree on the pinned generation"
    );
}

#[test]
fn candidate_carries_token_measure() {
    let env = StubEnvironment;
    let host = HostCapabilities::builtin();
    let result = workflows::run_with_environment(
        ENV_WF,
        "env_test.nodus",
        None,
        &env,
        &host,
        &"__stub__".to_string(),
        1,
    )
    .expect("run");

    let budget = Some(Budget {
        max_tokens: Some(1000),
        ..Default::default()
    });
    let candidate = result.candidate(
        ENV_WF,
        "run-1",
        budget,
        Some("gpt-tokenizer-v1".to_string()),
    );
    assert_eq!(
        candidate.token_measure,
        Some("gpt-tokenizer-v1".to_string()),
        "the profile's measure must carry through to the archived candidate"
    );
}

// ─── declared budget measure ────────────────────────────────────────────

/// Same call-tracking shape as `InstrumentedEnv`, but its `profile()` is
/// configurable so a test can declare a `max_tokens` budget with or without a
/// `token_measure`.
struct MeasureEnv {
    profile: EnvironmentProfile,
    calls: Arc<Mutex<Vec<&'static str>>>,
    opened: Arc<Mutex<u32>>,
}

impl MeasureEnv {
    fn new(profile: EnvironmentProfile) -> Self {
        MeasureEnv {
            profile,
            calls: Arc::new(Mutex::new(Vec::new())),
            opened: Arc::new(Mutex::new(0)),
        }
    }
}

impl EnvironmentProvider for MeasureEnv {
    fn task_ids(&self) -> Vec<TaskId> {
        vec!["t1".to_string()]
    }

    fn profile(&self) -> EnvironmentProfile {
        self.profile.clone()
    }

    fn open(&self, task: &TaskId, seed: Seed) -> Instance {
        *self.opened.lock().unwrap() += 1;
        Instance::new(task.clone(), seed)
    }

    fn reset(&self, _inst: &mut Instance) -> Observation {
        self.calls.lock().unwrap().push("reset");
        Observation(nodus::executor::Value::Text("seeded".to_string()))
    }

    fn step(&self, _inst: &mut Instance, action: Action) -> Observation {
        self.calls.lock().unwrap().push("step");
        Observation(action.0)
    }

    fn evaluate(&self, _inst: &Instance) -> Reward {
        self.calls.lock().unwrap().push("evaluate");
        Reward {
            score: Some(1.0),
            metadata: Default::default(),
        }
    }

    fn release(&self, _inst: Instance) {
        self.calls.lock().unwrap().push("release");
    }
}

#[test]
fn max_tokens_with_no_measure_rejects_before_workflow_runs() {
    let env = MeasureEnv::new(EnvironmentProfile {
        labels: Default::default(),
        grading: GradingMode::Automated,
        budget: Some(Budget {
            max_tokens: Some(1000),
            ..Default::default()
        }),
        token_measure: None,
    });
    let host = HostCapabilities::builtin();

    let result = workflows::run_with_environment(
        ENV_WF,
        "env_test.nodus",
        None,
        &env,
        &host,
        &"t1".to_string(),
        1,
    )
    .expect("the gate returns an EnvRunResult, not a parse error");

    assert_eq!(result.result.status, Status::Failed);
    assert!(
        result.result.log.is_empty(),
        "no workflow step may execute on a rejected run"
    );
    assert!(
        result
            .result
            .errors
            .iter()
            .any(|e| e.code == "NODUS:ENV_MEASURE_UNKNOWN"),
        "rejection must record ENV_MEASURE_UNKNOWN; errors: {:?}",
        result.result.errors
    );
    assert_eq!(
        *env.opened.lock().unwrap(),
        1,
        "env.open already ran for the frozen-boundary reset-observation shape (the check is after profile(), not before open)"
    );
    assert_eq!(
        *env.calls.lock().unwrap(),
        vec!["reset", "release"],
        "reset ran, then the rejection short-circuits before any workflow step or evaluate — \
         but release still fires via the guard's Drop (release unaffected); calls: {:?}",
        env.calls.lock().unwrap()
    );
}

#[test]
fn max_tokens_with_measure_runs_normally() {
    let env = MeasureEnv::new(EnvironmentProfile {
        labels: Default::default(),
        grading: GradingMode::Automated,
        budget: Some(Budget {
            max_tokens: Some(1000),
            ..Default::default()
        }),
        token_measure: Some("gpt-tokenizer-v1".to_string()),
    });
    let host = HostCapabilities::builtin();

    let result = workflows::run_with_environment(
        ENV_WF,
        "env_test.nodus",
        None,
        &env,
        &host,
        &"t1".to_string(),
        1,
    )
    .expect("run");

    assert_eq!(
        result.result.status,
        Status::Ok,
        "an identified measure must not be rejected; errors: {:?}",
        result.result.errors
    );
    assert!(
        env.calls.lock().unwrap().contains(&"evaluate"),
        "the run must proceed all the way to evaluate; calls: {:?}",
        env.calls.lock().unwrap()
    );
}

#[test]
fn no_max_tokens_budget_is_unaffected_by_ne14() {
    // A profile with no token budget at all (or no budget) carries no measure
    // and is unaffected — the regression every other environment test already
    // exercises implicitly; this one asserts it explicitly for the token-measure check.
    let env = MeasureEnv::new(EnvironmentProfile::empty());
    let host = HostCapabilities::builtin();

    let result = workflows::run_with_environment(
        ENV_WF,
        "env_test.nodus",
        None,
        &env,
        &host,
        &"t1".to_string(),
        1,
    )
    .expect("run");

    assert_eq!(result.result.status, Status::Ok);
    assert!(
        env.calls.lock().unwrap().contains(&"evaluate"),
        "an unbudgeted profile must run to completion unaffected by the token-measure check"
    );
}

// ─── Idempotent, guaranteed release — through the public combinator ───

#[test]
fn release_runs_exactly_once_via_combinator() {
    let released = Rc::new(RefCell::new(0u32));

    struct ReleaseCountingEnv {
        released: Rc<RefCell<u32>>,
    }

    impl EnvironmentProvider for ReleaseCountingEnv {
        fn task_ids(&self) -> Vec<TaskId> {
            vec!["r".to_string()]
        }
        fn profile(&self) -> EnvironmentProfile {
            EnvironmentProfile::empty()
        }
        fn open(&self, task: &TaskId, seed: Seed) -> Instance {
            Instance::new(task.clone(), seed)
        }
        fn reset(&self, _inst: &mut Instance) -> Observation {
            Observation(nodus::executor::Value::Null)
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

    let env = ReleaseCountingEnv {
        released: released.clone(),
    };
    let host = HostCapabilities::builtin();
    let _ = workflows::run_with_environment(
        ENV_WF,
        "env_test.nodus",
        None,
        &env,
        &host,
        &"r".to_string(),
        1,
    )
    .expect("run");

    assert_eq!(
        *released.borrow(),
        1,
        "release must fire exactly once per run"
    );
}
