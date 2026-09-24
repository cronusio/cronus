# Nodus Observability Implementation (Rust)

**Version:** 1.6.0
**Status:** Stable
**Layer:** implementation
**Implements:** l1-nodus-observability.md

## Overview

Concrete Rust implementation of the nodus execution observability contract.
Adds an `AuditProvider` trait, a closed `ExecutionEvent` enum (10 types), a `RunManifest`
struct, and the built-in `NoopAuditProvider` to `crates/nodus`. Hook points are wired into
`executor.rs` so that every observable execution event is emitted in emission order, synchronously,
before control returns to the next step. The existing `run` and `run_with_provider` API functions
retain their signatures; new `run_with_audit` and `run_with_provider_and_audit` variants accept
an `AuditProvider` implementation from the host.

<!-- [ADDED] v1.2.0 -->
**v1.2.0 — Run-Manifest Identity & Reproducibility (HO-12, HO-15, HO-18, HO-19, HO-20).** The
`RunManifest` and `StepError` are enriched into a cross-run-comparable, arm-partitionable,
re-executable record: a stable per-run execution mode (HO-12), a definition-derived step identity
that is the same step across runs (HO-15), the resolved exposure-switch values a run executed under
(HO-18), a message-independent fault-identity contribution on errors (HO-19), and a re-execution
recipe that states what re-running the workflow requires (HO-20). All additive and optional — a run
that declares none behaves exactly as v1.1.0 (HO-5/HO-6 preserved). This is the intended realization
(the spec-ahead-of-code pattern); it awaits its implementation phase. The per-event-descriptor batch
(HO-7…HO-11, HO-13, HO-14, HO-16, HO-17) is deferred to follow-up passes — its cross-cutting members
HO-7 and HO-14 are now specified in §4.8 below; the rest remain deferred.

<!-- [ADDED] v1.3.0 -->
**v1.3.0 — Aggregation-Safe Event Stream (HO-7, HO-14).** The two *cross-cutting* members of the
deferred event batch, taken together because both rewrite every `ExecutionEvent` variant: a
run-monotonic dense `seq` plus a run-scoped `correlation_id` on every event (HO-7), and a two-state
`Measurement` replacing every raw numeric that could fail to be obtained (HO-14). Together they make
the stream *statistically trustworthy* — a consumer can order it, detect dropped events, and
aggregate it without a fabricated zero corrupting the result. Batching them does the all-variant
churn once, so the remaining riders (HO-8's token classes especially) are **born** as `Measurement`
rather than added raw and retyped later. Intended realization, awaiting its phase. The remaining
riders — HO-8, HO-9, HO-10, HO-11, HO-13, HO-16, HO-17 — are additive optional fields and read-side
rules that build on this foundation; they are now specified in §4.9 below.

<!-- [ADDED] v1.4.0 -->
**v1.4.0 — Event Annotations, Cost, Lineage & Completeness (Pass 2: HO-8, HO-9, HO-10, HO-11,
HO-13, HO-16, HO-17).** Closes the event batch, and with it **all twenty** HO invariants. Four of the
seven (HO-9 receipt, HO-11 message, HO-16 anomaly, HO-17 durability) are *host-supplied annotations
that may ride any event*; specifying them as four separate fields would churn all ten variants four
times over, so they land as **one `EventAnnotations` carrier field per variant** — the all-variant
churn happens once, and any future annotation becomes a struct field rather than a tenth-variant
edit. The remaining three are targeted: HO-8's token classes attach to `ModelResponse` only (born as
§4.8's `Measurement`), HO-13's derivation descriptor to collection-mapping events only, and HO-10 is
a pure read-side classification with **no field at all**. Intended realization, awaiting its phase.

## Related Specifications

- [l1-nodus-observability.md](l1-nodus-observability.md) — observability contract this spec implements
- [l2-nodus-runtime.md](l2-nodus-runtime.md) — executor and public API this spec extends
- [l1-nodus-portability.md](l1-nodus-portability.md) — `AuditProvider` is the registered Audit extension point
- [l2-nodus-commands.md](l2-nodus-commands.md) — [ADDED v1.6.0] widens HO-12's mode override to command answers and validator verdicts, adds `Determinism::ContainsHostCommands` and the `EventAnnotations::command` record (§4.9 there)

## 1. Motivation

The existing `LogEntry`-based execution log in `executor.rs` records step command and result but
cannot represent control-flow transitions (branches, loops, macro calls) or model invocations as
distinct event types. Harness evolution analysis requires structured, attributable, append-only
events — not just a flat command log. This spec closes the gap by wiring the full 10-type event
taxonomy from `l1-nodus-observability.md` into the executor.

## 2. Constraints & Assumptions

- The `AuditProvider` is on the hot execution path; the built-in `NoopAuditProvider` must cost at
  most one virtual dispatch per event (no allocation, no I/O).
- `run_id` is caller-supplied (UUID or equivalent); the executor accepts it as a parameter and
  never generates one itself, keeping the executor deterministic and testable.
- Elapsed time for `step_end`, `macro_exit`, and `model_response` events is measured using
  `std::time::Instant` (monotonic); wall-clock `started_at` in `RunManifest` is a caller-supplied
  ISO-8601 string.
- The `model_call` and `model_response` events must not include raw user content — only structural
  descriptors (HO-4 data-safety boundary; §4.4 of `l1-nodus-observability.md`).
- Adding an `AuditProvider` to a run must not change `RunResult.out`, `RunResult.status`, or
  `RunResult.errors` — the provider is a write-only side channel (HO-5).
- `LogEntry` and `RunResult` in `executor.rs` are unchanged; they remain the caller-facing output.
  The audit stream is orthogonal to the existing log.

## 3. Invariant Compliance

| L1 Invariant | Implementation |
| --- | --- |
| HO-1 Trace-first output | `run_with_audit` emits all events during execution; `run_complete` is called unconditionally before returning `RunResult`. A run without an attached audit uses `NoopAuditProvider` but the hook calls still occur — every run can be observed |
| HO-2 Per-step attribution | Every `ExecutionEvent` variant carries `step_index: u32` (or `call_step_index` for macros); `constraint_hit` carries `triggering_step_index`. No event is emitted with a zero/unknown step index except boot-sequence errors |
| HO-3 Append-only immutability | `AuditProvider::record_event` is a one-way write call; the trait has no mutation or replay method. The `NoopAuditProvider` discards; concrete providers append. There is no API surface to modify a previously emitted event |
| HO-4 Frozen boundary | `observability.rs` contains no validator or executor logic. `record_event` is called *after* the NL-invariant checks in `validator.rs` complete — the audit layer only witnesses outcomes; it never intercepts or modifies them. `ExecutionEvent::ConstraintHit` records that a rule fired; it does not re-evaluate the rule |
| HO-5 Observer neutrality | `Executor::execute` calls `record_event` on a `&dyn AuditProvider` reference; the return type is `()`. No branch in `executor.rs` inspects provider state or return value. `RunResult` is assembled independently of the provider |
| HO-6 Structured event taxonomy | `ExecutionEvent` is a closed Rust enum with exactly 10 variants matching the L1 taxonomy. Adding a new event type requires amending this spec (minor version bump) and the `ExecutionEvent` enum — there is no catch-all string variant |
| HO-12 Execution-mode provenance <!-- [ADDED] v1.2.0 --> | `RunManifest.execution_mode: ExecutionMode` (`Real` default \| `Simulated { fidelity }`); records the host-declared mode, so a consumer excludes simulated runs from real-run analytics. Absent = `Real`. §4.7. **Implemented:** an executor whose model provider is the built-in stub records `Simulated { fidelity: Structural }` whatever mode the caller declared, so no stub entry point claims a real run (`l2-nodus-runtime.md` §4.5); a mode the caller explicitly declared as simulated is kept as declared. |
| HO-15 Cross-run step identity <!-- [ADDED] v1.2.0 --> | `step_identity(&Step)` derived from the definition (number + command name), carried on `StepStart`/`StepEnd`/`StepError` and the manifest; the *same* step across runs is one comparable series. Stable across retries/resumes (NL-12)/recursive children (NL-18); changes only when the definition changes. Distinct from HO-7's within-run `(correlation_id, seq)`. §4.7 |
| HO-18 Variant provenance <!-- [ADDED] v1.2.0 --> | `RunManifest.exposure_switches: Vec<(String, String)>` — the resolved `(name, value)` pairs the host froze once at run start (LP-19; non-straddling, one value per switch). Empty = prevailing defaults. Names/values only (§4.4). §4.7 |
| HO-19 Fault-identity contribution <!-- [ADDED] v1.2.0 --> | `StepError.fault_identity: FaultIdentity { step_identity, code, discriminator? }` — a stable, message-independent grouping input, **never** derived from `error_detail` rendered text; the optional workflow-declared `discriminator` outranks the code. nodus computes no grouping. §4.7 |
| HO-7 Sequence & correlation <!-- [ADDED] v1.3.0 --> | Every `ExecutionEvent` variant carries `seq: u64` (run-monotonic, dense, gap-free) and `correlation_id: String` (bound once at run construction from the same value as `RunManifest.run_id`). `RunManifest.event_count` doubles as the gap check — it equals `highest seq + 1` for an undamaged trace. Streaming chunk-merge is vacuous in core (the `ModelProvider` returns a complete `String`; no chunks exist) and remains a host obligation. §4.8 |
| HO-8 Cost-attribution token classes <!-- [ADDED] v1.4.0 --> | `ModelResponse` gains `input_tokens`, `output_tokens`, `cache_read_tokens`, `cache_creation_tokens` — all `Measurement` (§4.8). `ModelProvider` exposes no token-accounting seam, so all four are `Unavailable` in core today, **never `0`** — the L1's own canonical example of the HO-14 rule. A host wrapping a real backend supplies `Taken(_)`. §4.9 |
| HO-9 Execution-authenticity receipt <!-- [ADDED] v1.4.0 --> | `EventAnnotations.receipt: Option<String>` — an opaque, secret-free, host-supplied token binding step identity to observed result. No crypto in core (LP-2, the LP-9 attestation precedent); the signing secret never enters a trace, prompt, or context. Populated for step events; `None` everywhere today (no host supplies one). §4.9 |
| HO-10 Trace-completeness honesty <!-- [ADDED] v1.4.0 --> | **No field.** A read-side classifier `classify_trace(events, manifest) -> TraceCompleteness { Complete, GapDamaged, Truncated, Empty }`: a terminal manifest ⇒ `Complete`; events without one ⇒ `Truncated`; a manifest whose `event_count` ≠ highest durable `seq` + 1 ⇒ `GapDamaged` (reusing §4.8's gap check). Adds no hot-path emission — HO-5 untouched. nodus does **not** capture the terminating crash; that is the host's forensic plane. §4.9 |
| HO-11 Single-stream dual legibility <!-- [ADDED] v1.4.0 --> | `EventAnnotations.message: Option<String>` — a host-rendered one-line projection of the event's own structured fields. Faithfulness is a contract, not a hope: it introduces no fact the fields lack and contradicts none, and stays within §4.4 (descriptors and counts, never raw content). No renderer or locale vocabulary in core (LP-2). §4.9 |
| HO-13 Per-item derivation lineage <!-- [ADDED] v1.4.0 --> | `LoopIteration.derivation: Option<Vec<SourceRef>>` where `SourceRef { producing_step: u32, source_index: u32 }` — indices only, never element content (LN-8), recording the true shape (N→N, 1→M, K→1, filter-drop; LN-4). Side-band metadata on an existing event — never a new variant (HO-6) and never inside the workflow's `Value` payload, so NL-7's closed value space is untouched. §4.9 |
| HO-16 Optional anomaly annotation <!-- [ADDED] v1.4.0 --> | `EventAnnotations.anomaly: Option<Anomaly>` with `Anomaly { Anomalous, Normal, Unscored }` — four states counting `None` (no annotation), exactly the L1's set. `Unscored` is **never** `Normal`: an absent verdict is emitted as absence, matching HO-14's `Unavailable`. nodus computes no verdict and names no model, threshold, or window (LP-2) — it reserves only the carrier. §4.9 |
| HO-17 Transient/durable separation <!-- [ADDED] v1.4.0 --> | `EventAnnotations.durability: Durability { Durable, Transient }` (default `Durable`). The load-bearing consequence for this crate: **a transient event must not consume a `seq`** — §4.8's counter numbers the durable stream only, so a missed transient is never a detected gap and a cut-off transient tail never makes a completed run read as truncated (HO-10). nodus emits no transients today (`ModelProvider` returns a complete `String`); the field and the emission rule are reserved for a host-facing streaming path. §4.9 |
| HO-14 Aggregation-safe measurement <!-- [ADDED] v1.3.0 --> | `Measurement { Taken(u64), Unavailable }` replaces every raw numeric whose value could fail to be obtained: `StepEnd`/`MacroExit`/`ModelResponse`/`RunManifest`'s `elapsed_ms` and `LoopIteration.iteration_number`. `Unavailable` is never `0`, never omitted, never carried forward. Concrete bite: `handle_dialog`'s hardcoded `elapsed_ms: 0` (the one fabricated zero in the crate) becomes `Unavailable`. `FieldDescriptor`'s counts stay plain `u32` — obtainable by construction, never a stand-in. §4.8 |
| HO-20 Re-execution recipe <!-- [ADDED] v1.2.0 --> | `RunManifest.repro: ReproRecipe` — workflow content digest, the LP-8 capability set that satisfied the manifest, exposure switches (HO-18), execution mode (HO-12), nodus version, and a stated `determinism`. Uncapturable fields (e.g. resolved `@needs` vocabulary while `@needs` is unimplemented) are `None`, never omitted. nodus embeds nothing and performs no replay. §4.7. The digest is stable only within one build (`DefaultHasher` over the AST's `Debug` form), so a recipe compared across an upgrade can mismatch an unchanged workflow — the recorded `nodus_version` is what scopes a comparison (`l2-nodus-dialog.md` §4.3). |

> **Realization status.** **All twenty HO invariants are now specified.** HO-1…HO-6 are realized in `crates/nodus` (§4.1–§4.6); HO-12/15/18/19/20 per §4.7 (Phase 14); HO-7 and HO-14 per §4.8 (Phase 15). The final seven — **HO-8, HO-9, HO-10, HO-11, HO-13, HO-16, HO-17** — are **specified here as the intended realization (§4.9) and await their implementation phase**, the spec-ahead-of-code pattern this project uses (cf. `l2-nodus-config.md`, §4.7, §4.8). Nothing from `l1-nodus-observability` remains unspecified.

## 4. Detailed Design

### 4.1 New Module: `observability.rs`

All new types live in `crates/nodus/src/observability.rs`. This module has no dependencies
outside the Rust standard library.

```text
[REFERENCE]
// Public types
pub trait AuditProvider {
    fn record_event(&self, event: ExecutionEvent);
    fn run_complete(&self, manifest: RunManifest);
}

pub struct NoopAuditProvider;
impl AuditProvider for NoopAuditProvider {
    fn record_event(&self, _: ExecutionEvent) {}
    fn run_complete(&self, _: RunManifest) {}
}

/// Closed enum — 10 variants matching the L1 taxonomy.
pub enum ExecutionEvent {
    StepStart   { step_index: u32, step_command: String, input_vars: Vec<String> },
    StepEnd     { step_index: u32, step_command: String, output_vars: Vec<String>, elapsed_ms: u64 },
    StepError   { step_index: u32, step_command: String, error_code: String, error_detail: String },
    ConstraintHit  { rule_name: String, triggering_step_index: u32, halt: bool },
    BranchTaken    { step_index: u32, branch_label: String, condition_result: bool },
    LoopIteration  { step_index: u32, loop_type: LoopType, iteration_number: u32, bound_vars: Vec<String> },
    MacroEnter  { macro_name: String, call_step_index: u32 },
    MacroExit   { macro_name: String, call_step_index: u32, elapsed_ms: u64 },
    ModelCall   { step_index: u32, command: String, input_summary: FieldDescriptor },
    ModelResponse { step_index: u32, command: String, output_summary: FieldDescriptor, elapsed_ms: u64 },
}

pub enum LoopType { For, Until }

/// Structural descriptor — no raw user content (HO-5 data-safety).
pub struct FieldDescriptor {
    pub field_count: u32,
    pub type_hints: Vec<String>,   // e.g. ["Text", "Map"]
    pub total_bytes: u32,          // approximate; no content
}

pub struct RunManifest {
    pub workflow_name: String,
    pub schema_version: String,
    pub run_id: String,
    pub started_at: String,        // ISO-8601, caller-supplied
    pub elapsed_ms: u64,
    pub status: RunStatus,
    pub error_code: Option<String>,
    pub total_steps: u32,
    pub event_count: u32,
}

pub enum RunStatus { Ok, Error, ConstraintHalt, ValidationError, Paused }
```

**Status mapping.** The executor maps `Status::Ok` to `RunStatus::Ok`; `Partial` to `Error` when non-fatal
errors were recorded (a budget halt ends `Partial` with no error — a graded outcome, not a fault — and maps to
`Ok`); `Paused` to `Paused`, since a suspended run has not completed although `run_complete` fires for it; and
`Failed` and `Aborted` to `ConstraintHalt`. `ValidationError` is never produced by the executor: a run rejected
at validation returns before `run_complete`, so no manifest is written for it. A manifest therefore never
reports `Ok` for a run that ended with errors or stopped at a pause.

### 4.2 `lib.rs` Changes

Add `pub mod observability;` to `lib.rs`. Re-export the public surface:

```text
[REFERENCE]
pub use observability::{AuditProvider, ExecutionEvent, NoopAuditProvider, RunManifest, RunStatus,
                        LoopType, FieldDescriptor};
```

### 4.3 `executor.rs` Changes

`Executor` gains a second field alongside `provider`:

```text
[REFERENCE]
pub struct Executor {
    provider: Box<dyn ModelProvider>,
    audit:    Box<dyn AuditProvider>,
}

impl Executor {
    pub fn new(provider: impl ModelProvider + 'static) -> Self {
        Executor { provider: Box::new(provider), audit: Box::new(NoopAuditProvider) }
    }

    pub fn with_audit(
        provider: impl ModelProvider + 'static,
        audit:    impl AuditProvider + 'static,
    ) -> Self {
        Executor { provider: Box::new(provider), audit: Box::new(audit) }
    }
}
```

`ExecutionContext` gains `event_count: u32` and `started_at: std::time::Instant`.

The following call sites in `execute_command` / `execute_conditional` / `execute_for` /
`execute_until` / `execute_parallel` / `dispatch` emit the corresponding event variant.
All `record_event` calls use `self.audit.record_event(event)` (synchronous, in-path):

| Call site | Event emitted |
| --- | --- |
| Before `dispatch(ctx, cmd)` | `StepStart` — input_vars = snapshot of `ctx.variables` keys written so far |
| After successful `dispatch` | `StepEnd` — output_vars = pipeline target name(s); elapsed from `Instant::now()` before dispatch |
| In `check_rules` violation path | `ConstraintHit` — halt=true (rule violations always halt) |
| After `ctx.errors.push` in rule-violation path | `StepError` — error_code = `RULE_VIOLATION` |
| In `execute_conditional`, before executing the taken branch | `BranchTaken` — branch_label = "if"/"elif"/"else" |
| At top of each `execute_for` loop body | `LoopIteration` — loop_type=For; bound_vars = [fl.variable] |
| At top of each `execute_until` loop body | `LoopIteration` — loop_type=Until; bound_vars = [] |
| In `dispatch` "RUN" arm, before flag push | `MacroEnter` — macro_name from cmd.args[0] |
| In `dispatch` "RUN" arm, after flag push | `MacroExit` — elapsed from Instant before MacroEnter |
| In `handle_gen`, before `self.provider.generate` | `ModelCall` — input_summary = FieldDescriptor { field_count: 1, type_hints: ["Text"], total_bytes: prompt.len() as u32 clamped to 0..u32::MAX } |
| In `handle_gen`, after `generate` returns | `ModelResponse` — output_summary = FieldDescriptor from result length |
| In `handle_analyze`, before `self.provider.analyze` | `ModelCall` — input_summary = FieldDescriptor from flags.len() |
| In `handle_analyze`, after `analyze` returns | `ModelResponse` |

`execute()` calls `self.audit.run_complete(manifest)` immediately before returning `RunResult`.
The manifest's `elapsed_ms` is measured from an `Instant` taken at the top of `execute()`.

<!-- [ADDED] v1.1.0 -->
#### Buffered sink adapter

For hosts where in-path recording is too costly (high-frequency events, slow sinks), the crate ships a `BufferedAuditProvider` adapter: `record_event` enqueues onto a bounded in-memory channel (`std::sync::mpsc`) and returns; a dedicated writer thread drains the queue into the wrapped provider. Ordering is preserved by the `(correlation_id, seq)` contract — the writer never reorders within a correlation. The bound applies backpressure: when the queue is full, `record_event` blocks rather than drops (completeness over latency). On drop the adapter flushes the queue and joins the writer thread before returning, so `run_complete(manifest)` is always the last delivered event of its run. The default remains the synchronous in-path call; the adapter is opt-in at construction and composes with any inner `AuditProvider`.

### 4.4 `workflows.rs` API Additions

Two new public functions alongside the existing six:

```text
[REFERENCE]
/// Like `run` but with an injected `AuditProvider`.
/// `run_id` is caller-supplied (UUID or equivalent string).
pub fn run_with_audit(
    source:   &str,
    filename: &str,
    input:    Option<Value>,
    audit:    impl AuditProvider + 'static,
    run_id:   &str,
    started_at: &str,
) -> Result<RunResult, Vec<Diagnostic>>;

/// Like `run_with_provider` but also accepts an `AuditProvider`.
pub fn run_with_provider_and_audit(
    source:   &str,
    filename: &str,
    input:    Option<Value>,
    provider: impl ModelProvider + 'static,
    audit:    impl AuditProvider + 'static,
    run_id:   &str,
    started_at: &str,
) -> Result<RunResult, Vec<Diagnostic>>;
```

Both functions:

1. Validate with `Validator::validate` (fast-fail on errors — NL-4).
2. Construct `Executor::with_audit(provider, audit)`.
3. Call `executor.execute_with_run_params(ast, input, run_id, started_at)` (extended overload of
   `execute` that threads `run_id` / `started_at` for manifest construction — internal method, not
   public).
4. Return the `RunResult`.

### 4.5 Module Structure After Changes

```text
[REFERENCE]
crates/nodus/src/
├── lib.rs           — adds `pub mod observability;` + re-exports
├── observability.rs — NEW: AuditProvider, ExecutionEvent (10 variants), NoopAuditProvider,
│                          RunManifest, RunStatus, LoopType, FieldDescriptor
├── executor.rs      — MODIFIED: Executor gains `audit` field; hook points wired;
│                          execute() / execute_command / execute_conditional /
│                          execute_for / execute_until / dispatch emit events
├── workflows.rs     — MODIFIED: run_with_audit + run_with_provider_and_audit added
└── (all other modules unchanged)
```

### 4.6 Test Coverage

Unit tests reside in `observability.rs` (`#[cfg(test)] mod tests`) and integration tests in
`crates/nodus/tests/`:

| Test | Location | What it verifies |
| --- | --- | --- |
| `noop_provider_discards_all` | observability.rs | `NoopAuditProvider` accepts all 10 event variants without panic |
| `step_start_end_emitted` | observability.rs | Recording provider receives `StepStart` then `StepEnd` for a GEN step |
| `constraint_hit_recorded` | observability.rs | `ConstraintHit { halt: true }` emitted when `!!NEVER` fires |
| `branch_taken_if` | observability.rs | `BranchTaken { branch_label: "if" }` emitted when condition is true |
| `branch_taken_else` | observability.rs | `BranchTaken { branch_label: "else" }` emitted when condition is false |
| `loop_iteration_for` | observability.rs | `LoopIteration { loop_type: For }` emitted N times for N-element collection |
| `loop_iteration_until` | observability.rs | `LoopIteration { loop_type: Until }` emitted each body entry |
| `macro_enter_exit` | observability.rs | `MacroEnter` + `MacroExit` pair emitted for RUN command |
| `model_call_response_no_raw_content` | observability.rs | `ModelCall.input_summary` has no user text; only `FieldDescriptor` fields |
| `run_complete_manifest` | observability.rs | `run_complete` called once; manifest fields populated |
| `observer_neutrality` | tests/observability.rs | RunResult with NoopAuditProvider == RunResult with RecordingProvider for deterministic inputs |
| `run_with_audit_api` | tests/observability.rs | Public `run_with_audit` function returns correct RunResult; events collected |
| `run_with_provider_and_audit_api` | tests/observability.rs | Public `run_with_provider_and_audit` with StubProvider + RecordingProvider |

The `RecordingProvider` test helper collects events in a `Vec<ExecutionEvent>` behind a
`std::sync::Mutex` — zero external dependencies.

### 4.7 Run-Manifest Identity & Reproducibility [ADDED v1.2.0]

Realization of HO-12, HO-15, HO-18, HO-19, HO-20 — additive `RunManifest` / `StepError` fields that
make a trace cross-run-comparable, arm-partitionable, and re-executable. Every field is
optional/defaulted, so a run declaring none is byte-for-byte v1.1.0 (HO-5 observer neutrality, HO-6
closed taxonomy preserved). All values are host-declared or definition-derived — nodus computes no
statistic, holds no history, mocks nothing, and performs no replay (LP-1/LP-2). Within the §4.4
data-safety boundary: identities, versions, names — never rendered content or a secret's value.

#### Stable step identity (HO-15)

```text
[REFERENCE]
/// Definition-derived, NOT per-run allocated. Same value across runs/retries/resumes (NL-12)/
/// recursive children (NL-18); changes only when the step's definition changes. Deterministic (NL-6).
pub fn step_identity(step: &Step) -> String;   // e.g. "{number}:{command_name}"
```

`StepStart` / `StepEnd` / `StepError` each gain `step_identity: String`. A host comparing "the last
twenty runs" to "the twenty before" groups by this identity; within-run ordering stays on HO-7's
`(correlation_id, seq)` (deferred batch). This is the foundation HO-18 and HO-19 build on.

#### Execution mode (HO-12)

```text
[REFERENCE]
pub enum ExecutionMode {
    Real,                                  // default; an absent marker means Real (today's behaviour)
    Simulated { fidelity: SimFidelity },   // the host substituted modeled providers for this run
}
pub enum SimFidelity { Structural, Modeled, Shadow }
```

`RunManifest` gains `execution_mode: ExecutionMode` (default `Real`). The executor records the host-declared
mode, with one override: when the model provider reports itself a stub (`ModelProvider::is_stub`), a run declared
`Real` is recorded `Simulated { fidelity: Structural }`, because nothing that ran was real. A mode the caller
declared as simulated is kept as declared, and a host that wired modeled providers declares it. Same
manifest-honesty family as HO-10 (completeness): a trace never lies about which mode produced it.

**[UPDATED v1.6.0]** The override reaches past the model. `l2-nodus-commands.md` §4.9 records a run declared `Real`
as `Simulated { fidelity: Structural }` when any command answer was a simulated stand-in or any validator verdict was
assumed, and decides the mode when the manifest is written — not at run start — because that is when the run's
answers are known. Specified, not built: until it is, only a stub model triggers the override.

#### Variant provenance (HO-18)

`RunManifest` gains `exposure_switches: Vec<(String, String)>` — the resolved `(switch_name, value)`
pairs the host froze **once at run start** (LP-19; non-straddling — one value per switch for the whole
run, so a half-one-arm-half-another run is unrepresentable). Empty = the run executed under prevailing
defaults. Composes HO-15: stable step identity makes a step comparable *across* arms, HO-18 makes the
arms distinguishable. nodus computes no assignment and names no fraction/hash/subject (LP-2).

#### Fault identity (HO-19)

```text
[REFERENCE]
pub struct FaultIdentity {
    pub step_identity: String,          // HO-15
    pub code: String,                   // the typed NODUS:* code (l2-nodus-errors)
    pub discriminator: Option<String>,  // optional workflow-declared; when present, outranks the code
}
```

`StepError` gains `fault_identity: FaultIdentity`, composed from stable inputs only and **never**
derived from `error_detail` rendered text (which routinely carries per-occurrence interpolated
values — an id, a path, a count — that would shatter one recurring failure into thousands of
singletons). nodus performs no grouping and holds no fault record; it guarantees only that the inputs
a host groups on are stable and content-independent (FL-3/FL-4, source side).

#### Re-execution recipe (HO-20)

```text
[REFERENCE]
pub struct ReproRecipe {
    pub workflow_digest: String,               // content identity of the definition: `nd1:` + FNV-1a-64 of the canonical compact form (versioned, zero-dep — the NE-12 precedent)
    pub capability_set: Vec<String>,           // the LP-8 roles/commands that satisfied the run's manifest
    pub exposure_switches: Vec<(String, String)>, // HO-18 (mirrored into the recipe)
    pub execution_mode: ExecutionMode,         // HO-12
    pub nodus_version: String,                 // the producing crate version
    pub needs_vocabulary: Option<Vec<String>>, // resolved @needs units — None (Unavailable) until @needs lands
    pub determinism: Determinism,
}
pub enum Determinism { Deterministic, ContainsModelCalls }  // stated, never inferred from the recipe's presence
```

`RunManifest` gains `repro: ReproRecipe`, recording what re-executing the run requires **from the
manifest alone** — the host reconstructs nothing. Two honesty rules from the L1 contract:

- **Re-executable ≠ reproducible.** nodus's own evaluation is deterministic (NL-6), but a run
  containing model calls is not — `determinism` *states* which the run supports rather than letting a
  reader infer exactness from a recipe merely being present (`ContainsModelCalls` whenever a
  `GEN`/`REFINE` ran).
- **Uncapturable → unavailable, never omitted.** A field nodus could not resolve is `None`, never
  silently dropped — a short manifest must not read as a complete one. `needs_vocabulary` is `None`
  until `@needs` selective loading is implemented (an honest declared omission, the HO-14 principle
  applied locally at the manifest grain — the general two-state `Measurement` type is the deferred
  batch's concern).

**[UPDATED v1.6.0]** `Determinism` gains a third statement, `ContainsHostCommands`, specified by
`l2-nodus-commands.md` §4.9: a run whose commands a real `CommandProvider` answered is not stated deterministic,
because a host answer is as external as a model's. The variant names the first disqualifier — model calls, then host
commands — not the full set, which stays in the event stream; a simulated answer disqualifies nothing. Specified,
not built.

nodus embeds nothing into produced artifacts, names no file format, and performs no replay
(embedding/transport/re-execution are host concerns, LP-1/LP-2). Realizes RR-2 (three recipe layers),
RR-3 (content identity over names), RR-4 (producing version), RR-6 (declared omissions), RR-8
(determinism stated).

#### Deferred: event-stream enrichment batch

HO-7 (`(correlation_id, seq)` fields), HO-8 (cost token classes on `ModelResponse`), HO-9 (execution
receipt), HO-10 (trace-completeness classification — read-side), HO-11 (dual-legibility `message`),
HO-13 (per-item derivation lineage), HO-14 (the general two-state `Measurement` type for event
numerics), HO-16 (anomaly annotation), and HO-17 (transient/durable separation) form a distinct
per-event-descriptor batch, reconciled in a follow-up spec pass. This section realizes the
manifest/identity cluster only.

### 4.8 Aggregation-Safe Event Stream [ADDED v1.3.0]

Realization of HO-7 and HO-14 — the two cross-cutting invariants of the deferred event batch. Both
rewrite every `ExecutionEvent` variant, so they land together: one round of churn, and the Pass-2
riders (notably HO-8's token classes) are born already-typed. Together they make the stream
aggregatable: HO-7 lets a consumer order it and detect drops; HO-14 lets it compute a mean, a
minimum, and a coverage ratio without a fabricated zero silently corrupting all three.

#### Sequence & correlation (HO-7)

```text
[REFERENCE]
// Every ExecutionEvent variant gains:
seq            : u64      // run-monotonic, dense, gap-free emission counter
correlation_id : String   // shared by every event of one run (== RunManifest.run_id)
```

`correlation_id` is bound **once**, at run construction, from the same value the manifest reports as
`run_id`; a run entered without an explicit id has one generated at the root before the first event,
so no event is ever emitted uncorrelated. `seq` is assigned at emission, densely — a gap in the
recorded range is exactly the signal that an event was dropped, which is why it must never be
sparse or re-used.

`RunManifest.event_count` becomes the gap check: for an undamaged trace it equals **highest `seq` +
1**. A consumer comparing the two detects in-range loss; combined with manifest presence/absence
(HO-10, Pass 2) this classifies every trace as complete, gap-damaged, or truncated.

> **Emission choke point.** The crate today pairs each of its 20 `record_event` calls with a manual
> `ctx.event_count += 1` — currently correct (verified 20/20) but fragile: `seq` correctness now
> depends on that pairing never drifting. The realization routes emission through a single helper
> that assigns `seq` from the counter and increments it atomically, making a mismatch
> unrepresentable rather than merely absent. This is a refactor of existing call sites, not new
> hot-path work.

**Streaming merge is vacuous in core.** The L1 requires many streamed chunks to merge into one
logical `model_response`. `ModelProvider::generate` returns a complete `String` synchronously — no
chunk ever exists at this layer — so nodus has nothing to merge and adds no merge machinery. A host
wrapping a streaming backend performs the fold on its side before the value reaches the executor;
this is recorded as a host obligation (LP-2), not silently omitted.

#### Aggregation-safe measurement (HO-14)

```text
[REFERENCE]
/// A numeric that was either genuinely measured, or explicitly could not be.
/// `Unavailable` is NEVER rendered as 0, omitted, or carried forward from a
/// previous observation — each of those corrupts a downstream aggregate while
/// looking like data.
pub enum Measurement {
    Taken(u64),
    Unavailable,
}
```

Applied to every defined numeric whose value can fail to be obtained:

| Site | Field | Why it can be `Unavailable` |
| --- | --- | --- |
| `StepEnd` | `elapsed_ms` | the dialog path does not time its step (see below) |
| `MacroExit` | `elapsed_ms` | measured today; typed for uniformity and future host-supplied timings |
| `ModelResponse` | `elapsed_ms` | measured today; a host-substituted provider may not time its call |
| `LoopIteration` | `iteration_number` | typed for uniformity — always taken in-core |
| `RunManifest` | `elapsed_ms` | measured today; typed for uniformity |

**The concrete defect this fixes.** `handle_dialog` currently emits `StepEnd { elapsed_ms: 0 }` — a
hardcoded zero standing in for "this path never measured the duration". That is exactly the
substitution HO-14 forbids: a consumer averaging step durations silently biases toward zero, and a
minimum becomes meaningless. It becomes `Measurement::Unavailable`, which is the honest value.

**What stays a plain number.** `FieldDescriptor`'s `field_count` and `total_bytes` are computed by
construction from a value already in hand — they can never fail to be obtained, so wrapping them
would add ceremony while communicating nothing. *Not applicable* stays distinct from *unavailable*:
a field the taxonomy does not define for an event type is simply absent from that variant, and never
carries the marker.

**Pass-2 coupling.** HO-8's token classes (`input` / `output` / `cache_read?` / `cache_creation?`)
are the L1's own canonical example of this rule — `ModelProvider` exposes no token-accounting seam
(the documented gap `Budget.max_tokens` already records, `l2-nodus-environment`), so those fields
will be `Unavailable`, never `0`, when they land. Introducing `Measurement` first means they are
born correct.

#### Deferred to Pass 2

HO-8 (cost token classes), HO-9 (execution-authenticity receipt), HO-10 (trace-completeness
classification, read-side), HO-11 (dual-legibility `message`), HO-13 (per-item derivation lineage),
HO-16 (anomaly annotation), and HO-17 (transient/durable separation) are additive optional fields
and read-side rules. Each rides on the `seq`/`Measurement` foundation this section lays; none
requires re-touching the variants once §4.8 is realized.

### 4.9 Event Annotations, Cost, Lineage & Completeness [ADDED v1.4.0]

Realization of the final seven HO invariants — HO-8, HO-9, HO-10, HO-11, HO-13, HO-16, HO-17.
Every field below is optional or defaulted, so a host declaring none emits a stream byte-identical to
§4.8's (HO-5 observer neutrality, HO-6 closed taxonomy preserved). All values are host-supplied or
nodus-structural; nodus computes no verdict, holds no history, performs no grouping, and adds no
dependency (LP-1/LP-2).

#### The annotation carrier (HO-9, HO-11, HO-16, HO-17)

Four of the seven are *host-supplied annotations that may ride any event*. Specified as four separate
optional fields they would rewrite all ten `ExecutionEvent` variants four times over — and every
future annotation would rewrite them again. They land instead as **one carrier field per variant**:

```text
[REFERENCE]
// One new field on each of the 10 ExecutionEvent variants:
annotations : EventAnnotations

pub struct EventAnnotations {
    /// HO-11: host-rendered one-line human projection of THIS event's own
    /// structured fields. Adds no fact the fields lack; contradicts none.
    /// Within §4.4 — descriptors and counts, never raw content.
    pub message: Option<String>,
    /// HO-16: host-supplied verdict. `None` = not annotated at all.
    pub anomaly: Option<Anomaly>,
    /// HO-9: opaque, secret-free, host-supplied authenticity token binding
    /// step identity to observed result. Meaningful on step events; `None`
    /// elsewhere and `None` everywhere until a host supplies a provider.
    pub receipt: Option<String>,
    /// HO-17: durable (the record) vs transient (a live affordance).
    pub durability: Durability,
}

pub enum Anomaly { Anomalous, Normal, Unscored }   // + None ⇒ the L1's four states
pub enum Durability { Durable, Transient }          // Default::default() == Durable
```

`EventAnnotations::default()` is all-`None` + `Durable`, so every existing emission site adds one
`Default::default()` and behaves exactly as before. **`Unscored` is never `Normal`** — a detector
with no history yet emits *no verdict*, and the trace says so, matching HO-14's `Unavailable` and
HO-10's truncation marker. nodus names no model, threshold, window, or locale (LP-2); it reserves the
carrier only.

> **Emission rule for transients (HO-17).** §4.8's `seq` numbers the **durable stream only**. A
> transient event therefore **must not consume a `seq`** — it must not increment the counter that
> `RunManifest.event_count` reports. Were a transient to take a position, a consumer that dropped it
> would detect a phantom gap, and a disconnect truncating a transient tail would make a completed
> run read as truncated (HO-10). Concretely: the §4.8 `emit` choke point stays the durable path, and
> a transient path is a *separate* companion that dispatches without touching the counter. nodus
> emits no transients today — `ModelProvider::generate` returns a complete `String`, so no chunk
> exists — but the rule is fixed now so a future host-facing streaming path cannot get it wrong by
> default.

#### Cost-attribution token classes (HO-8)

```text
[REFERENCE]
// ModelResponse gains four fields — and only ModelResponse:
input_tokens           : Measurement
output_tokens          : Measurement
cache_read_tokens      : Measurement
cache_creation_tokens  : Measurement
```

Typed `Measurement` from birth (§4.8) rather than raw numbers retyped later — this is exactly the
coupling §4.8 was sequenced first to buy. `ModelProvider` exposes **no token-accounting seam** (the
same documented gap `l2-nodus-environment`'s `Budget.max_tokens` already records), so all four are
`Measurement::Unavailable` in core today — **never `0`**. This is the L1's own canonical illustration
of HO-14: a host must be able to distinguish *"the provider reported no cache accounting"* from
*"the cache returned nothing"*. Extending `ModelProvider` with a token-reporting method is a
separate, larger change (it touches the extension-point contract) and is **not** in scope here; a
host wrapping a real backend supplies `Taken(_)` once that seam exists.

#### Per-item derivation lineage (HO-13)

```text
[REFERENCE]
// On collection-mapping events (LoopIteration; ~MAP rides the same path):
derivation : Option<Vec<SourceRef>>

pub struct SourceRef {
    pub producing_step: u32,
    pub source_index: u32,
}
```

Indices only — **never element content** (LN-8), staying inside §4.4. Records the true shape as it
actually is (LN-4): `~MAP` N→N gives `[(step, i)]` per produced element; a 1→M split gives
`[(step, 0)]` for each of the M; a K→1 `~JOIN` gives all K sources on the single product; a filter
records the dropped source index while survivors keep their own. Side-band metadata on an existing
event — never a new variant (HO-6), and never part of the workflow's `Value` payload, so NL-7's
closed value-type system is untouched and lineage can neither leak into nor perturb business data.
Walking these references transitively reconstructs end-to-end lineage (LN-3); the walk is host-side.

#### Trace completeness (HO-10) — read-side, no field

```text
[REFERENCE]
pub enum TraceCompleteness { Complete, GapDamaged, Truncated, Empty }

/// Pure classification over what a consumer holds. Adds no emission.
pub fn classify_trace(
    durable_events: &[ExecutionEvent],
    manifest: Option<&RunManifest>,
) -> TraceCompleteness;
```

Rules: a terminal manifest ⇒ `Complete`; events but no manifest ⇒ `Truncated` (a killed, panicked, or
OOM-killed host never reached `run_complete`); neither ⇒ `Empty`; a manifest whose `event_count` ≠
highest durable `seq` + 1 ⇒ `GapDamaged`, reusing §4.8's gap check directly. Reads **durable events
only** (HO-17) — a transient tail severed by a disconnect must never make a completed run classify as
truncated. This is pure interpretation of manifest presence/absence plus the §4.8 identity: no new
hot-path work, so HO-5 is untouched and the change is entirely additive for existing consumers.

**Boundary.** nodus does not capture the fault that truncated a trace. Whether a native traceback
exists is the host's forensic diagnostic-log plane, installed below and earlier than the executor and
surviving faults the executor cannot. nodus's contract stops precisely at *"my trace never lies about
being complete"*; HO-10 forbids it from pretending otherwise.

#### Closing the set

With §4.9 realized, all twenty `l1-nodus-observability` invariants have a specified Rust realization:
HO-1…HO-6 (§4.1–§4.6, live), HO-12/15/18/19/20 (§4.7, live), HO-7/HO-14 (§4.8, live), and
HO-8/9/10/11/13/16/17 (§4.9, pending its phase). No observability invariant remains unspecified.

## 5. Implementation Notes

1. Implement `observability.rs` first (pure types, no executor dependency). All 10 variants and
   `NoopAuditProvider` can be fully tested without touching `executor.rs`.
2. Add the `audit` field to `Executor` and thread it through `with_audit` constructor. Compile-check
   that existing `new()` / `with_stub()` still work (they use `NoopAuditProvider`).
3. Wire hook points in `execute_command` first (covers `StepStart`/`StepEnd`/`StepError`/
   `ConstraintHit`). Run existing tests — they must still pass.
4. Add `BranchTaken` in `execute_conditional`, `LoopIteration` in `execute_for`/`execute_until`,
   `MacroEnter`/`MacroExit` in the `RUN` arm of `dispatch`.
5. Add `ModelCall`/`ModelResponse` in `handle_gen` and `handle_analyze`.
6. Add `execute()` run-complete call and `run_with_audit` API functions last.
7. Write integration test `observer_neutrality` as the final gate — confirms HO-5.

## 6. Drawbacks & Alternatives

- **Shared `Executor` instance across concurrent runs**: `Box<dyn AuditProvider>` is `Send` only
  if the concrete type is. The executor remains one-per-run; host projects that need concurrent
  runs should instantiate one `Executor` per run thread. <!-- [MODIFIED] v1.1.0 --> Within a single
  run, `~PARALLEL` branches execute in sequence (`l2-nodus-runtime.md` §4.4 — the concurrent
  scheduling once described there never shipped), so their events arrive already ordered; the
  `(correlation_id, seq)` fields would resolve interleaving if a runtime ever ran branches
  concurrently.
- **Alternative — emit events to a channel instead of a trait**: using `std::sync::mpsc::Sender`
  avoids the vtable dispatch but couples the executor to the channel type and complicates no-op
  semantics. Rejected in favour of LP-2 (extension via abstract interface).
- **Alternative — integrate `tracing` crate**: provides rich structured logging but adds a
  non-`std` dependency, violating the zero-external-deps constraint from `l2-nodus-runtime.md §2`.
  Hosts may wrap an `AuditProvider` implementation that bridges to `tracing` at the host layer.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[OBSERVABILITY]` | `crates/nodus/src/observability.rs` | Authoritative source for all event types, traits, and manifest struct |
| `[EXECUTOR]` | `crates/nodus/src/executor.rs` | Hook-point implementation — where `record_event` calls are inserted |
| `[WORKFLOWS]` | `crates/nodus/src/workflows.rs` | Public API functions including new `run_with_audit` variants |
| `[TESTS]` | `crates/nodus/tests/observability.rs` | Integration tests for observer neutrality and public API |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.6.0 | 2026-09-24 | Core Team | Design pass (2026-09-24): `l2-nodus-commands.md` §4.9 widens two of this spec's honesty rules to the command seam. HO-12: a run declared `Real` is recorded `Simulated { fidelity: Structural }` when any command answer was a simulated stand-in or any validator verdict was assumed, decided when the manifest is written rather than at run start. HO-20: `Determinism` gains `ContainsHostCommands`, so a run answered by a real provider is not stated deterministic. A new `EventAnnotations::command` record carries the effect key, attempt, world and provenance on a seam-answered step's `StepEnd` (no new event variant, HO-6 intact). Nothing is built: every rule still describes the crate as it is. |
| 1.5.0 | 2026-09-24 | Core Team | Realization sync (2026-09-24): HO-12 implemented: a run answered by the built-in stub records `Simulated { Structural }`. `RunStatus` gains `Paused`; `Partial` with recorded errors maps to `Error`, so a manifest no longer reports `Ok` for a run that ended with errors or stopped at a pause. `workflow_digest` is versioned (`nd1:` + FNV-1a-64 over the canonical compact form). |
| 1.4.1 | 2026-09-24 | Core Team | Consistency pass (2026-09-24): HO-12 → Partial: every entry point except `run_with_provider*` substitutes the stub model yet records `ExecutionMode::Real`. §4.1 records the status mapping: `Partial` and `Paused` runs are reported as `RunStatus::Ok`, and `RunStatus::Error`/`ValidationError` are never produced. HO-20 notes that the digest is stable only within one build. §6 no longer claims concurrent `~PARALLEL` event emission. |
| 1.4.0 | 2026-07-24 | Core Team | Added §4.9 Event Annotations, Cost, Lineage & Completeness — the intended realization (spec-ahead-of-code) of the final seven invariants, **closing all twenty**. Central design decision: HO-9 (receipt), HO-11 (`message`), HO-16 (anomaly), and HO-17 (durability) are all *host-supplied annotations that may ride any event*; as four separate fields they would rewrite all ten `ExecutionEvent` variants four times over, so they land as **one `EventAnnotations` carrier field per variant** (`message`/`anomaly`/`receipt`/`durability`, `Default` = all-`None` + `Durable`) — one churn now, and future annotations become struct fields rather than tenth-variant edits. Targeted realizations: **HO-8** four token classes on `ModelResponse` only, born as §4.8 `Measurement` and `Unavailable`-not-`0` since `ModelProvider` has no token-accounting seam (extending that seam is explicitly out of scope); **HO-13** `Option<Vec<SourceRef>>` (indices only, never content) on collection-mapping events, side-band and outside the NL-7 `Value` space; **HO-10** a pure read-side `classify_trace → { Complete, GapDamaged, Truncated, Empty }` with **no field**, reusing §4.8's `event_count == highest seq + 1` identity as the gap test. Records the load-bearing HO-17 consequence for this crate: **a transient event must not consume a `seq`** — §4.8's counter numbers the durable stream only, so a dropped transient can never register as a gap nor a severed transient tail as a truncated run; nodus emits no transients today (`generate` returns a complete `String`), but the rule is fixed now so a future streaming path cannot default into corrupting the sequence. |
| 1.3.0 | 2026-07-24 | Core Team | Added §4.8 Aggregation-Safe Event Stream — the intended realization (spec-ahead-of-code) of **HO-7** (`seq: u64` run-monotonic dense counter + run-scoped `correlation_id` on every `ExecutionEvent`; `RunManifest.event_count` = highest `seq` + 1 as the gap check; streaming chunk-merge recorded as vacuous in core — `ModelProvider::generate` returns a complete `String`, so no chunk exists to merge, and the fold is a host obligation) and **HO-14** (two-state `Measurement { Taken(u64), Unavailable }` replacing every raw numeric that can fail to be obtained — `elapsed_ms` on `StepEnd`/`MacroExit`/`ModelResponse`/`RunManifest` and `LoopIteration.iteration_number`; `FieldDescriptor`'s counts deliberately stay plain `u32`, obtainable by construction). Batched because both rewrite every variant — one round of churn, and Pass-2's HO-8 token classes are born as `Measurement` rather than added raw and retyped. Records two findings from the current crate: emission must route through a single `seq`-assigning choke point (20 `record_event` calls each paired with a manual `event_count += 1` — correct today, fragile once `seq` depends on it), and `handle_dialog`'s hardcoded `elapsed_ms: 0` is the exact fabricated-zero HO-14 forbids and becomes `Unavailable`. Pass 2 (HO-8, HO-9, HO-10, HO-11, HO-13, HO-16, HO-17) explicitly deferred. |
| 1.2.0 | 2026-07-24 | Core Team | Added §4.7 Run-Manifest Identity & Reproducibility — the intended realization (spec-ahead-of-code) of HO-12 (`execution_mode`), HO-15 (definition-derived `step_identity` on events + manifest), HO-18 (`exposure_switches` resolved-and-frozen variant provenance), HO-19 (`StepError.fault_identity`, message-independent), and HO-20 (`RunManifest.repro: ReproRecipe` — workflow digest, capability set, exposure, mode, nodus version, stated determinism, `None`-not-omitted uncapturable fields). All additive/optional (HO-5/HO-6 preserved); awaits its implementation phase. The per-event-descriptor batch (HO-7…HO-11, HO-13, HO-14, HO-16, HO-17) is explicitly deferred to a follow-up pass. Reconciles the standing pending Invariant-Compliance obligation for HO-12/15/18/19/20. |
| 1.1.0 | 2026-07-04 | Core Team | Added `BufferedAuditProvider` adapter (§4.3): opt-in bounded-channel + writer-thread sink honoring the `(correlation_id, seq)` ordering contract; blocking backpressure (never drops); flush-and-join on drop with `run_complete` as the final delivered event; synchronous in-path recording remains the default |
| 1.0.0 | 2026-06-24 | Core Team | Initial spec — HO-1…HO-6 compliance table, `observability.rs` type system, executor hook-point mapping, `workflows.rs` API additions, full test plan |
