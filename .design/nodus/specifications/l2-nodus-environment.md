# Nodus Environment and Evaluation Implementation (Rust)

**Version:** 1.3.1
**Status:** Stable
**Layer:** implementation
**Implements:** l1-nodus-environment.md

## Overview

Concrete Rust realization of the Environment/Evaluation contract in `crates/nodus`. It maps each `NE`-invariant to its enforcing mechanism: an `EnvironmentProvider` trait following the established `ModelProvider`/`DialogProvider` pattern, a built-in deterministic `StubEnvironment`, the closed `open/reset/step/evaluate/release` lifecycle, a typed `Reward` and a read-side `Trajectory` projected onto the existing `AuditProvider` stream (no new `ExecutionEvent` variant, HO-6 preserved), the `ExtensionRole::Environment` capability-manifest binding, a `run_with_environment` frozen-boundary combinator, the closed grading-mode set, the NE-13 profile budget, and the NE-12 archivable candidate tuple. It adds one extension role, no new command, no new `Status`, and no new error category beyond reusing the existing capability-unmet code.

## Related Specifications

- [l1-nodus-environment.md](l1-nodus-environment.md) — the contract this implements (NE-1…NE-14).
- [l2-nodus-runtime.md](l2-nodus-runtime.md) — runtime crate extended here: `executor` (provider seam), the `run_with_*` family, `RunResult`.
- [l2-nodus-portability.md](l2-nodus-portability.md) — the LP-8 `ExtensionRole`/`CapabilityManifest`/`HostCapabilities` taxonomy this adds `Environment` to; [ADDED v1.1.0] NE-14's fail-fast shape is LP-8 kinship (a missing measure is a missing capability) without being a new `ExtensionRole` — the check is profile-shaped, not role-shaped.
- [l2-nodus-observability.md](l2-nodus-observability.md) — the `AuditProvider`/`ExecutionEvent`/`FieldDescriptor`/`RunManifest` the trajectory projects onto (NE-3); [ADDED v1.2.0] also owns `ReproRecipe.workflow_digest`, whose `digest_ast` computation `CandidateResult.workflow_digest` now reuses (§4.4) rather than the two fields disagreeing under one name.
- [l2-nodus-dialog.md](l2-nodus-dialog.md) — the sibling extension role whose `step`-suspend shape a `step` reuses (a machine turn vs. a human turn).
- [l2-nodus-errors.md](l2-nodus-errors.md) — [ADDED v1.1.0] owns `NODUS:ENV_MEASURE_UNKNOWN` (NE-14), registered beside `CAPABILITY_UNMET` in the same pre-run rejection family.
- [../../main/specifications/l1-tokenization-boundary.md](../../main/specifications/l1-tokenization-boundary.md) — [ADDED v1.1.0] TB-6/TB-7, the measurement contract NE-14 realizes: the measure is the receiving model's own encoder, and an unidentifiable encoder fails loudly rather than defaulting.

## 1. Motivation

`l1-nodus-environment.md` defines a host-neutral way to run a workflow against a gradable world, but the crate has no environment seam, no reward type, and no graded-run combinator. This spec records the minimal additions that realize the contract while leaving the executor's run-to-completion model intact: a provider trait with a built-in no-I/O stub, a reward carried alongside (never inside) the run, a trajectory reconstructed from the audit stream rather than a second store, and a frozen-boundary run combinator that guarantees `evaluate` observes only a completed run.

## 2. Constraints & Assumptions

- No new external dependency (LP-1): the environment backend is an in-tree trait with a built-in deterministic resolver; the NE-12 content address uses a std-library stable digest, and any cryptographic content-hash is host-supplied (mirroring the LP-9 no-crypto-in-core rule).
- Additive to the LP-8 enum: `ExtensionRole::Environment` joins the existing `Model/Audit/Storage/Policy/Vocabulary/Dialog` set; `validate_manifest` already handles new roles generically.
- No new `Status` and no new command vocabulary: a `step` reuses the dialog suspend/resume machinery; `reset`/`step`/`evaluate` are provider calls the run combinator sequences, not workflow commands.
- The trajectory is **not a new store**: entries ride as optional side-band descriptors on existing `ExecutionEvent`s and run-boundary structures, so a host recording events already records trajectories and a no-op audit sink simply keeps none (NE-3, HO-6 preserved).
- `Observation` and `Action` are opaque to the crate: their shape is the environment's `profile()` concern, validated host-side exactly as host-schema commands are (LP-4).

## 3. Invariant Compliance

| NE Invariant | Rust Enforcement |
| --- | --- |
| NE-1 Environment as extension role | `EnvironmentProvider` trait (like `ModelProvider`/`DialogProvider`); built-in deterministic `StubEnvironment` ships in-crate; concrete worlds live outside (LP-2). |
| NE-2 Closed lifecycle | The trait exposes exactly `task_ids`/`profile`/`open`/`reset`/`step`/`evaluate`/`release`; `run_with_environment` is the only sequencer. `StubEnvironment` is deterministic per `(task, seed, actions)`; `release` is idempotent (double-release is a no-op). |
| NE-3 Trajectory is an observability projection | Each interaction attaches an optional `EnvInteraction` side-band descriptor to existing events — a `step` to the workflow step's `StepStart`/`StepEnd`, `reset` to the run-open boundary, `evaluate` to the terminal `RunManifest` — **no new `ExecutionEvent` variant** (HO-6). `Trajectory` is a read-side projection assembled by filtering the stream; it inherits HO-2 attribution, the §4.4 data-safety boundary (`FieldDescriptor`, no raw `Observation`/`Action` text), and HO-3 append-only immutability. |
| NE-4 Frozen evaluation | `run_with_environment` executes the workflow to a terminal state, marks the run frozen, then calls `evaluate` — which receives an immutable view and cannot re-enter `execute`. Two `evaluate` calls over one frozen instance return the same `Reward` (enforced by construction: `evaluate` reads the frozen trajectory only). |
| NE-5 Reward is typed data | `Reward` is returned in `EnvRunResult` **alongside** `RunResult`, never bound to a workflow variable or branched on mid-run; it has no effect on `Status` (a low reward is not `Failed`, a high reward is not `Ok`). |
| NE-6 Addressable catalog + profile | `task_ids() -> Vec<TaskId>` and `profile() -> EnvironmentProfile` (observation/action shape descriptors, orthogonal task labels, optional `Budget`, declared grading mode) published before any run; selection is by stable `TaskId`. |
| NE-7 Instance isolation | `open` returns a fresh `Instance` per run; instances share no mutable state. `release` is mandatory (called in a `finally`/drop guard) and idempotent; a non-released instance surfaces a liveness fault to the host, never silently reclaimed mid-run. |
| NE-8 Function-scoped auxiliary roles | `EnvRole { Policy, Environment, Context, Summary }` tags every trajectory entry; auxiliary roles (context/summary/judge scoring) resolve through function-scoped `ModelProvider` bindings, economical by default — never the policy route (reaffirms RTG-9). **Pending:** no function-scoped binding exists — the executor holds one `ModelProvider`, and `run_with_environment*` fixes it to `StubProvider` (§4.5). |
| NE-9 Host-supplied metric neutrality | `evaluate` delegates scoring to the host `EnvironmentProvider`; `StubEnvironment` (and any host supplying no scorer) returns `Reward { score: None, metadata: {} }` and the run stays valid. |
| NE-10 Capability-declared, fail-fast | `CapabilityManifest::from_workflow` adds `ExtensionRole::Environment` when a workflow declares an environment need; `validate_manifest` rejects fail-fast (reusing `NODUS:CAPABILITY_UNMET`) if the active host omits it. `HostCapabilities::builtin()` **provides** `Environment` via `StubEnvironment` — a deliberate contrast with `Dialog` (whose default resolver is incomplete): the stub is a complete trivial world, so a manifest-declaring workflow stays runnable in-process; a host constructed without `Environment` triggers the fail-fast. |
| NE-11 Declared grading mode | `GradingMode { Automated, Judge, Hybrid }` is declared on the task/profile (not per run). `evaluate` applies the mode: `Automated` runs a deterministic checker; `Judge` scores via a function-scoped auxiliary model over a published rubric; `Hybrid` runs the checker as a floor first and takes `min(checker, judge)` so a judge may lower but never rescue a checker-failed run. |
| NE-12 Archivable candidate result | `EnvRunResult::candidate()` yields `CandidateResult { workflow_digest, reward, trajectory, budget }` — a content-addressable tuple. `workflow_digest` is a std-library stable digest over the **parsed AST** of the workflow the host supplies as `workflow_source` [IMPLEMENTED v1.2.0, Phase 32 — was the raw source string], falling back to the raw-text hash only if the source fails to parse; a host wanting a cryptographic content-address computes it over the exposed source (LP-2). The candidate space/mutation/search/frontier stay host-side; the crate holds no optimizer. |
| NE-13 Budget-normalized graded runs | `EnvironmentProfile::budget: Option<Budget>` (`wall_clock_ms`/`max_steps`/`max_tokens`, any subset). `run_with_environment` halts the run uniformly when any declared limit is reached; a budget halt is a **normal outcome** (`Status::Partial`, not an error) with `budget_halted: true` recorded, and `evaluate` runs over what was achieved. The budget is part of the profile identity and copied into `CandidateResult.budget`; the workflow cannot read, extend, or evade it (enforced by the run loop, LP-10 kinship). Steps/tokens are deterministic; wall-clock is host-measured and recorded. A profile with no budget behaves as today. |
| NE-14 Declared budget measure [ADDED v1.1.0] | **Implemented [v1.1.1].** `EnvironmentProfile::token_measure: Option<String>` — the identity of the host encoder `max_tokens` is denominated in (§4.4). `run_with_environment_impl` validates the profile immediately after `env.profile()` (both public entry points share this one function): a `budget.max_tokens.is_some()` with `token_measure.is_none()` is a fail-fast `NODUS:ENV_MEASURE_UNKNOWN`, returned as an ordinary `Ok(EnvRunResult{result: RunResult{status: Failed, ..}, ..})` mirroring `capability_rejection`'s exact shape (**not** a `Diagnostic` — that channel is parse/validate-only) — never a mid-run failure, never a silent default encoder. `token_measure` is opaque to the crate (LP-2 — no tokenizer/counting rule in core) and is copied into `CandidateResult` alongside `budget` (NE-12), so a host optimizer partitions its frontier by `(profile, budget, measure)`. A profile with a wall-clock/step-only budget (no `max_tokens`) carries no measure and is unaffected. |
| NE-15 Declared budget scope | **Pending.** The profile is to carry a `budget_scope` — the generated classes a `max_tokens` count includes (for instance deliberation that is billed but not delivered) — as part of its identity: copied into `CandidateResult` beside `budget` and `token_measure`, so a host optimizer partitions its frontier by `(profile, budget, measure, scope)`. A profile declaring `max_tokens` with no resolvable scope is to fail fast pre-run exactly as a missing measure does (`NODUS:ENV_MEASURE_UNKNOWN` shape), never defaulting to delivered output only; where the host's provider does not report an in-scope class, the run halts on a declared lower bound and the candidate records the count as incomplete. The classes and their counting stay host-supplied (LP-2). Today the profile carries `token_measure` only. |

## 4. Detailed Design

### 4.1 EnvironmentProvider trait and StubEnvironment

```text
[REFERENCE]
pub type TaskId = String;
pub type Seed   = u64;

pub trait EnvironmentProvider {
    fn task_ids(&self) -> Vec<TaskId>;
    fn profile(&self) -> EnvironmentProfile;
    fn open(&self, task: &TaskId, seed: Seed) -> Instance;      // NE-2/NE-7
    fn reset(&self, inst: &mut Instance) -> Observation;         // NE-2 deterministic
    fn step(&self, inst: &mut Instance, action: Action) -> Observation;  // NE-2
    fn evaluate(&self, inst: &Instance) -> Reward;              // NE-4 read-only
    fn release(&self, inst: Instance);                          // NE-2/NE-7 idempotent
}

/// Built-in, deterministic, no I/O (mirrors StubProvider / DefaultDialogProvider).
pub struct StubEnvironment;   // task_ids = ["__stub__"], empty profile,
                              // step echoes the action, evaluate = no-op Reward (NE-9)
```

`Observation` and `Action` are opaque wrappers over `Value`; the crate never inspects their domain semantics (LP-4).

### 4.2 Reward, Trajectory and the side-band projection (NE-3)

```text
[REFERENCE]
pub struct Reward { pub score: Option<f64>, pub metadata: BTreeMap<String, Value> }

pub enum EnvRole { Policy, Environment, Context, Summary }        // NE-8
pub enum EnvKind { Reset, Step, Evaluate }

pub struct EnvInteraction {          // OPTIONAL side-band field on existing events
    pub seq: u64,                    // reuses the HO-7 monotonic run sequence
    pub kind: EnvKind,
    pub role: EnvRole,
    pub action: Option<FieldDescriptor>,       // descriptor only — never raw Action (NE-3/HO)
    pub observation: Option<FieldDescriptor>,  // descriptor only — never raw Observation
    pub reward: Option<Reward>,                // present on Evaluate
    pub step_index: Option<u32>,               // HO-2 back-pointer
}
```

An `EnvInteraction` rides an existing event, never a new `ExecutionEvent` variant (the HO-8/HO-13 additive-field discipline, so HO-6's closed taxonomy holds): a `step` on the workflow step's `StepStart`/`StepEnd`, `reset` on the run-open boundary, `evaluate` on the terminal `RunManifest`. `Trajectory` is a **read-side projection** a host assembles by filtering the audit stream for these descriptors — not a second store. A no-op audit sink keeps no trajectory; the run is otherwise unaffected.

### 4.3 Frozen-boundary run combinator (NE-4/NE-5)

```text
[REFERENCE]
pub struct EnvRunResult {
    pub result: RunResult,      // the workflow's own run (Status, vars, ...)
    pub reward: Reward,         // NE-5 alongside, never inside
    pub budget_halted: bool,    // NE-13
}

fn run_with_environment(source, filename, input, env, task, seed) -> Result<EnvRunResult, _> {
    let mut inst = env.open(task, seed);                 // NE-7
    let out = (|| {
        let obs = env.reset(&mut inst);                  // NE-2, reset side-band
        let result = execute(workflow, obs, env, &mut inst, budget);  // steps emit interactions (NE-3);
                                                         // budget halt = Status::Partial (NE-13)
        // FROZEN — no further mutation
        let reward = env.evaluate(&inst);                // NE-4 read-only over frozen run
        EnvRunResult { result, reward, budget_halted }
    })();
    env.release(inst);                                   // NE-7 mandatory + idempotent (drop guard)
    out
}
```

`execute` may `step` the environment as the workflow acts, reusing the dialog suspend/resume shape (a machine turn); `evaluate` runs strictly after the frozen point and cannot re-enter `execute`.

### 4.4 Manifest, grading, budget, candidate

```text
[REFERENCE]
// portability.rs — additive variant
pub enum ExtensionRole { Model, Audit, Storage, Policy, Vocabulary, Dialog, Environment }
// HostCapabilities::builtin() inserts Environment (StubEnvironment satisfies it) — unlike Dialog.

pub enum GradingMode { Automated, Judge, Hybrid }                 // NE-11, declared on the task
pub struct Budget { pub wall_clock_ms: Option<u64>,              // NE-13, any subset
                    pub max_steps: Option<u32>,
                    pub max_tokens: Option<u64> }
pub struct EnvironmentProfile { pub labels: BTreeMap<String,String>,  // NE-6 orthogonal slice labels
                                pub grading: GradingMode,
                                pub budget: Option<Budget>,
                                pub token_measure: Option<String> }   // NE-14, opaque encoder identity

pub struct CandidateResult { pub workflow_digest: String,        // NE-12 content address (AST digest — [CORRECTED v1.2.0])
                             pub reward: Reward,
                             pub trajectory_ref: RunId,
                             pub budget: Option<Budget>,          // rewards under different budgets not comparable
                             pub token_measure: Option<String> }  // rewards under different measures not comparable
```

`grade` composes the modes with the hybrid floor: `Hybrid` runs the deterministic checker first and returns it on failure (a judge cannot rescue), else takes `min(checker, judge)`.

**`workflow_digest` unified onto AST identity [CORRECTED v1.2.0].** `candidate()` computed
this by hashing `workflow_source` byte-for-byte (`digest_source`) — a *different* notion of
identity than `l2-nodus-observability.md` §4.7's `ReproRecipe.workflow_digest`, which hashes
the parsed AST (`digest_ast`) so that two sources differing only in whitespace or comments —
neither of which affects execution — share a digest. Both fields carry the identical name
`workflow_digest` and `l2-nodus-observability.md` §4.7's own inline comment had cited this
field as its precedent, as if the two agreed; they did not, for any workflow with a cosmetic
edit between two archived candidates. NE-12's own purpose is comparing *evaluation
candidates* — grouping runs of "the same workflow" for reward/regression tracking — which is
exactly the reproducibility grain `digest_ast`'s own reasoning already states is correct, so
`candidate()` now parses `workflow_source` and hashes the AST via the identical
`crate::executor::digest_ast` function `ReproRecipe` uses (widened to `pub(crate)`, not
duplicated). **Signature unchanged** (`workflow_source: &str`, not `&WorkflowFile`) — the
host already has the raw source at hand (it is what it ran) and rarely holds onto the parsed
AST, which the crate does not return from any `run*` entry point; re-parsing inside a
once-per-archival call is cheap and adds no dependency. **Fallback, not a hard error**: if
`workflow_source` fails to parse (the host passed something that was not actually the source
it ran, or a corrupted string), `candidate()` falls back to the old `digest_source` byte hash
rather than panicking or changing `candidate()`'s infallible return type — `CandidateResult`
is still produced, just under the coarser byte-identity notion for that one call, since a
digest computed on *something* remains more useful than no digest at all. This is the
resolution of `l2-nodus-portability.md` §3.2's "pre-existing finding" note; that note is
updated to point here rather than restating the divergence.

**Implemented [Phase 32], no divergence from the design above.** `digest_ast` widened to
`pub(crate)` in `executor.rs`; `candidate()` matches on `crate::parser::Parser::parse` and
calls `crate::executor::digest_ast(&ast)` on success, `digest_source` on failure, exactly as
designed. `candidate_digest_agrees_with_repro_recipe_digest` (`tests/environment.rs`, a
capturing `AuditProvider` mirroring Phase 31's `ManifestCapture`) proves `CandidateResult`
and `ReproRecipe` agree on the same run's digest; `candidate_digest_falls_back_to_source_hash_when_unparseable`
(`environment.rs`'s own `#[cfg(test)]`) pins the fallback value explicitly — the pre-existing
unparseable-input test never asserted on the digest at all. 486 tests pass (was 484, +2);
clippy/fmt clean; `Cargo.toml`/`Cargo.lock` diff empty (LP-1 preserved); no
`unwrap`/`panic!`/`expect` added to any production path.

### 4.4.1 Measure validation (NE-14) [ADDED v1.1.0, CORRECTED v1.1.1]

```text
[REFERENCE]
fn env_measure_rejection(ast: &WorkflowFile) -> RunResult {
    RunResult {
        status: Status::Failed,
        errors: vec![RuntimeError { code: ENV_MEASURE_UNKNOWN, step: 0, .. }],
        ..  // mirrors capability_rejection's exact shape
    }
}

// inside run_with_environment_impl, immediately after `let profile = env.profile();`:
let wants_tokens = profile.budget.as_ref().and_then(|b| b.max_tokens).is_some();
if wants_tokens && profile.token_measure.is_none() {
    return Ok(EnvRunResult { result: env_measure_rejection(&ast), reward: Reward::no_op(), budget_halted: false });
}
```

**[CORRECTED v1.1.1]** The v1.1.0 text of this section claimed the rejection returns through
"the same pre-run `Result<_, Vec<Diagnostic>>` channel LP-8's `validate_manifest` already
uses" and runs "before `env.open`". Neither holds against the real code, verified directly
against `workflows.rs`'s single shared `run_with_environment_impl` (both public entry points
delegate to it): the adjacent, already-shipped `ExtensionRole::Environment` manifest
rejection returns `Ok(EnvRunResult { result: capability_rejection(&ast, &missing), .. })` — an
ordinary `RunResult { status: Failed, errors: [RuntimeError { code: CAPABILITY_UNMET, .. }] }`
— **never** `Err(Vec<Diagnostic>)`; that channel is reserved for the parse/validate failures
earlier in the same function. `env.profile()` — the only source of `budget`/`token_measure` —
is not called until *after* `env.open`/`env.reset` already ran, both needed to produce the
`Instance`/`Observation` the frozen-boundary reset-observation shape requires before the
workflow's own steps execute. NE-14's check therefore inserts **immediately after
`env.profile()` and before `execute_for_environment`** — no workflow step has executed yet
(satisfying "never mid-run"), even though the `Instance` is technically already open+reset;
`guard`'s `Drop` still releases it correctly on this early return (NE-7 unaffected). The check
is purely structural (does a token budget have an identified measure), not a capability-role
check, so it needs no new `ExtensionRole` and no manifest interaction — a host either returns
a well-formed `profile()` or the run is rejected before the workflow runs, symmetric with how
a workflow can never read or evade the budget itself once a run is underway (NE-13, LP-10
kinship).

### 4.5 Public API

```text
[REFERENCE]
pub fn run_with_environment(
    source, filename, input, env: &dyn EnvironmentProvider, task: &TaskId, seed: Seed,
) -> Result<EnvRunResult, Vec<Diagnostic>>;

pub fn run_with_environment_and_audit(
    source, filename, input, env, task, seed, audit, run_id, started_at,
) -> Result<EnvRunResult, Vec<Diagnostic>>;
```

Consistent with the `run_with_*` combinator family, including its composition limit (`l2-nodus-runtime.md` §4.5): neither entry point takes a `ModelProvider`, so every environment run executes the workflow's model steps on `StubProvider`, and an evaluation of a model-dependent workflow grades stub output until the composed entry point exists. The audit variant is what actually emits the NE-3 trajectory side-bands.

## 5. Drawbacks & Alternatives

- **A dedicated trajectory store** — rejected (violates NE-3): forks observability, duplicates the data-safety boundary, and creates two sources of truth. The side-band-on-existing-events projection keeps one stream.
- **Reward bound to a workflow variable** — rejected (violates NE-4/NE-5): entangles grading with control flow and makes runs non-comparable. Reward lives in `EnvRunResult`, outside the `vars` map.
- **Cryptographic workflow hash in-core** — rejected (LP-1 zero-dep): the crate exposes a std-digest content address and the canonical source; a host needing a cryptographic address computes it (LP-2), exactly as LP-9 attestation is host-supplied.
- **`builtin()` omits `Environment` (mirror Dialog)** — rejected: the L1 (§4.4) makes the stub a complete world, so a manifest-declaring workflow must stay runnable in-process; the fail-fast is exercised by a host that omits the role.
- **A new `ExtensionRole::TokenMeasure` for NE-14** — rejected: the measure is a property of one profile's `budget`, not a host capability a manifest derives from workflow commands; a role would let a host "provide" a generic measure-capability while any specific profile still lacked an actual identified encoder, which is exactly the silent-substitution failure NE-14 forbids. A direct pre-open profile check names the real condition.
- **Defaulting `token_measure` to a fixed built-in encoder** — rejected (the invariant's central rule): an estimate can halt the wrong candidate at the wrong point and produce a reward that looks valid and is not (TB-7); fail-fast is the only response to an unidentifiable measure.

## 6. Implementation Notes

Order that minimizes rework (follows the L1 §5 sequence): (1) `EnvironmentProvider` trait + `StubEnvironment` + lifecycle (NE-1/NE-2); (2) `EnvInteraction` side-band wiring onto existing audit hook points (NE-3, no store); (3) `ExtensionRole::Environment` + `builtin()` + `from_workflow` (NE-10, near-zero cost); (4) `run_with_environment` frozen ordering + `release` drop guard (NE-4/NE-5/NE-7, the hardest correctness property); (5) `EnvironmentProfile`/`Budget`/`GradingMode` (NE-6/NE-11/NE-13); (6) `CandidateResult` digest (NE-12). Each lands with unit tests plus an NE-invariant integration suite in `crates/nodus/tests/environment.rs` (deterministic replay, frozen-boundary immutability, idempotent release, manifest fail-fast, hybrid floor, budget halt).

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[EXEC]` | `crates/nodus/src/executor.rs` | Provider trait pattern; the `EnvironmentProvider` seam and step dispatch |
| `[PORT]` | `crates/nodus/src/portability.rs` | `ExtensionRole::Environment`, `HostCapabilities::builtin`, `validate_manifest` |
| `[AUDIT]` | `crates/nodus/src/observability.rs` | `AuditProvider`/`ExecutionEvent`/`FieldDescriptor`/`RunManifest` the trajectory rides |
| `[API]` | `crates/nodus/src/workflows.rs` | `run_with_environment` combinators alongside the `run_with_*` family |
| `[ERR]` | `crates/nodus/src/vocab.rs` | [ADDED v1.1.0] `error_code`/`error_meta` registry `NODUS:ENV_MEASURE_UNKNOWN` registers into |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.3.1 | 2026-09-24 | Core Team | Consistency pass (2026-09-24): The NE-14 row carried a delivery-phase reference and a test count, both removed. NE-15 (declared budget scope, l1-nodus-environment 1.5.0) had no compliance row — added as Pending: today the profile carries `token_measure` only; scope is to join the profile's identity, fail fast when unresolvable, and record a declared lower bound when the provider does not report an in-scope class. NE-8's function-scoped model bindings do not exist, and `run_with_environment*` takes no `ModelProvider` — every environment run executes model steps on the stub, so evaluating a model-dependent workflow grades stub output (§4.5). |
| 1.3.0 | 2026-09-02 | Core Team | **Phase 32 — NE-12 digest unification implemented exactly per v1.2.0's design, no scope correction.** `digest_ast` widened to `pub(crate)` (`executor.rs`); `candidate()` matches `Parser::parse(workflow_source)` — `Ok(ast)` hashes `digest_ast(&ast)`, `Err(_)` falls back to `digest_source`. `candidate_digest_agrees_with_repro_recipe_digest` (`tests/environment.rs`, a capturing `AuditProvider` mirroring Phase 31's `ManifestCapture`) proves `CandidateResult.workflow_digest` and `ReproRecipe.workflow_digest` agree for the same run — the assertion that makes the unification real, not assumed; `candidate_digest_falls_back_to_source_hash_when_unparseable` (`environment.rs`) pins the fallback value explicitly, closing the gap the prior row noted (the old unparseable-input test asserted nothing about the digest). 486 tests pass (was 484, +2); clippy/fmt clean; `Cargo.toml`/`Cargo.lock` diff empty (LP-1 preserved); no `unwrap`/`panic!`/`expect` added to any production path. |
| 1.2.0 | 2026-09-02 | Core Team | **NE-12's `workflow_digest` unified onto AST identity, resolving `l2-nodus-portability.md` §3.2's "pre-existing finding."** `CandidateResult.workflow_digest` (via `digest_source`, raw-text hash) and `l2-nodus-observability.md`'s `ReproRecipe.workflow_digest` (via `digest_ast`, AST hash) answered a different question under the identical field name; `l2-nodus-observability.md` §4.7's own comment had cited this field as the same precedent, which it was not. `candidate()`'s signature is unchanged (`workflow_source: &str`) — it now parses the source and hashes the AST via `crate::executor::digest_ast` (widened to `pub(crate)`, reused not duplicated), falling back to the prior `digest_source` byte hash only if the given source fails to parse (an honest degrade, not a panic or a signature change). §4.4's `[REFERENCE]` block and the NE-12 row updated. No behavioural change for any currently-passing test — `candidate_digest_deterministic_and_content_addressed`/`candidate_carries_token_measure` (`tests/environment.rs`) exercise valid, parseable sources throughout; the single unparseable-input case (`environment.rs`'s own `#[cfg(test)]`) exercises exactly the fallback path and asserts nothing about the digest's value. |
| 1.1.1 | 2026-08-01 | Core Team | **Implemented the NE-14 seam designed in v1.1.0 — Phase 29, with a real mechanism correction found at plan time and applied here.** `EnvironmentProfile.token_measure`/`CandidateResult.token_measure` landed as designed. **§4.4.1 corrected**: v1.1.0 claimed the rejection used the LP-8 `Diagnostic` channel and ran "before `env.open`" — neither held against the real code (`workflows.rs`'s single shared `run_with_environment_impl`, confirmed both public entry points delegate to it, resolving the plan-time question of whether they had separate bodies). The actual mechanism mirrors the adjacent, already-shipped `ExtensionRole::Environment` rejection exactly: `Ok(EnvRunResult{result: RunResult{status: Failed, errors: [RuntimeError{ENV_MEASURE_UNKNOWN, ..}]}, ..})` via a new `env_measure_rejection` helper (sibling to `capability_rejection`), inserted immediately after `env.profile()` (the only source of the fields being checked) and before `execute_for_environment` — `env.open`/`env.reset` have already run by that point for the frozen-boundary shape, but no *workflow* step has, satisfying "never mid-run"; `guard`'s `Drop` still releases correctly (NE-7 unaffected). This correction was found and recorded honestly at the `/magic.task` planning stage (not silently implemented against the wrong text, the Phase-18 `$restart` precedent) and applied here once the mechanism landed. 471 tests pass (was 467, +4: `max_tokens_with_no_measure_rejects_before_workflow_runs` proving `env.open`/`reset` ran but no workflow step or `evaluate` did, `max_tokens_with_measure_runs_normally`, `no_max_tokens_budget_is_unaffected_by_ne14`, `candidate_carries_token_measure`); clippy/fmt clean; `Cargo.toml`/`Cargo.lock` diff empty (LP-1 preserved); no `unwrap`/`panic!`/`expect` added to any production path. §3's NE-14 row updated to Implemented. |
| 1.1.0 | 2026-07-31 | Core Team | Closes the NE-14 invariant-traceability gap flagged at the v1.29.0 nodus replan (declared budget measure, added to `l1-nodus-environment` v1.4.0, had never gained a §3 row). New `EnvironmentProfile.token_measure: Option<String>` (§4.4) records the opaque host-encoder identity a `max_tokens` budget is denominated in — LP-2, no tokenizer/counting rule in core. New §4.4.1 designs the fail-fast check: `run_with_environment` validates the profile immediately after `env.profile()` and before `env.open`, rejecting `budget.max_tokens.is_some() && token_measure.is_none()` as a pre-run `NODUS:ENV_MEASURE_UNKNOWN` `Diagnostic` through the same channel LP-8's `validate_manifest` already uses — no `Instance` opens for a rejected profile. Deliberately **not** a new `ExtensionRole`: the condition is profile-shaped (does this specific budget have an identified measure), not role-shaped (does the host provide a measure capability in general) — a role would let the exact silent-substitution failure NE-14 forbids hide behind a satisfied capability check. `CandidateResult` gains `token_measure` alongside `budget` so a host optimizer partitions its frontier by `(profile, budget, measure)` per NE-14's own comparability rule. New error code `NODUS:ENV_MEASURE_UNKNOWN` (`Error`, `Control` — the `CAPABILITY_UNMET` precedent, both pre-run structural rejections) registers in `l2-nodus-errors.md`. §3 gains the NE-14 row; §5 gains two rejected alternatives (a new `ExtensionRole`, a default-encoder fallback). Design only — nothing landed in `crates/nodus` this pass. |
| 1.0.0 | 2026-07-10 | Core Team | Initial spec — Rust realization of the Environment/Evaluation contract: `EnvironmentProvider` trait + built-in deterministic `StubEnvironment`, closed `open/reset/step/evaluate/release` lifecycle, typed `Reward` carried in `EnvRunResult` (never in `vars`), `Trajectory` as a read-side projection via optional `EnvInteraction` side-band descriptors on existing events (no new `ExecutionEvent` variant, HO-6 preserved), `ExtensionRole::Environment` manifest binding with `builtin()` providing it via the stub (contrast with `Dialog`), `run_with_environment` frozen-boundary combinator, closed `GradingMode` (automated/judge/hybrid floor), `EnvironmentProfile` budget (NE-13, budget-halt = normal `Status::Partial`), and `CandidateResult` content-addressable tuple (NE-12, std-digest, crypto host-supplied). NE-1…NE-13 compliance table. Adds one extension role, no new command/status/error category. |
