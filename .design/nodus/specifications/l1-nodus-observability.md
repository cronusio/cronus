# Nodus Execution Observability

**Version:** 1.12.1
**Status:** Stable
**Layer:** concept

## Overview

Execution observability is the foundation that enables harness evolution: a workflow run must produce a structured, queryable trace of every step — not just a pass/fail score. This spec defines the observability contract that any conforming nodus implementation must honour. Structured traces allow an outer evolution loop to identify failure patterns, attribute causes to specific harness components, and propose evidence-backed amendments.

The observability layer is designed to be *observer-neutral*: its presence or absence must not change execution semantics. It is a read-only witness to what the executor already does.

## Related Specifications

- [l1-nodus-language.md](l1-nodus-language.md) — language whose execution this spec observes (step syntax, error codes)
- [l2-nodus-runtime.md](l2-nodus-runtime.md) — Rust implementation; must provide the AuditProvider hook points
- [l1-nodus-portability.md](l1-nodus-portability.md) — extension-point taxonomy; AuditProvider is one registered role
- [../../main/specifications/l1-observation-retention.md](../../main/specifications/l1-observation-retention.md) — [ADDED v1.8.0] host-side multi-resolution retention that rolls emitted measurements up; HO-14 is the source-side guarantee its aggregates depend on (OR-2/OR-3)
- [../../main/specifications/l1-anomaly-consensus.md](../../main/specifications/l1-anomaly-consensus.md) — [ADDED v1.8.0] host-side anomaly verdict HO-16 carries; nodus computes none (AC-1/AC-6)
- [../../main/specifications/l1-change-attribution.md](../../main/specifications/l1-change-attribution.md) — [ADDED v1.8.0] host-side window ranking that requires a step to be one comparable series across runs (HO-15, CA-2)
- [../../main/specifications/l1-staged-rollout.md](../../main/specifications/l1-staged-rollout.md) — [ADDED v1.10.0] host-side exposure switches whose resolved values HO-18 records in the manifest, making a trace attributable to exactly one arm
- [../../main/specifications/l1-fault-lifecycle.md](../../main/specifications/l1-fault-lifecycle.md) — [ADDED v1.11.0] host-side fault identity and lifecycle; HO-19 supplies the stable, message-independent identity contribution its ladder (FL-3) groups on, nodus performing no grouping itself
- [../../main/specifications/l1-reproduction-recipe.md](../../main/specifications/l1-reproduction-recipe.md) — [ADDED v1.12.0] host-side contract for an artifact-embedded reproduction recipe; HO-20 makes the RunManifest a workflow's re-execution recipe, embedding and replay staying host concerns

## 1. Motivation

A single pass/fail score is insufficient for harness evolution: it identifies *that* a workflow failed but not *where*, *why*, or *which component* to change. Structured per-step traces:

- Enable attribution: each failure links to a specific step, command, or constraint
- Enable root-cause analysis: patterns across many runs reveal systematic harness weaknesses
- Enable evidence-backed amendments: proposed workflow changes can cite concrete trace excerpts
- Enable replay-based validation: a proposed amendment can be evaluated on previously-recorded trace inputs
- Preserve history: append-only traces create an immutable audit log of every harness generation

## 2. Constraints & Assumptions

- The observability layer is purely additive — it emits events it never modifies; execution branches, outputs, and variable bindings are unaffected by whether an AuditProvider is attached
- Per-step events are emitted synchronously within the execution; the AuditProvider is called before returning control to the next step
- The AuditProvider may be absent (no-op); conforming implementations must not block on its presence
- Trace records are technology-agnostic: the schema uses named fields and primitive types only — no host-project-specific types
- Append-only is a contract: an implementation that allows trace mutation violates HO-3, regardless of whether mutation actually occurs

## 3. Core Invariants

Rules that Layer 2 implementations MUST NOT violate:

- **HO-1 Trace-first output**: a workflow run produces two outputs — the domain result (values in `@out` / `@err`) and the execution trace. The trace is not optional metadata; it is a primary output. A run that produces a result without a trace is incomplete.

- **HO-2 Per-step attribution**: each trace record identifies the step that generated it (step index, step command name, and step label if present). No event is attributable only to "the run" — it is always attributable to a specific step, constraint check, branch decision, or loop iteration.

- **HO-3 Append-only immutability**: once a trace record is emitted, it is immutable. Subsequent steps may not modify, overwrite, or backfill prior records. A replay of the same input may generate new records but must not alter existing ones.

- **HO-4 Frozen boundary**: the validation and invariant-enforcement pipeline (all NL-invariant checks, E013, E014 and equivalent error codes) is the frozen evaluation layer. Trace analysis may identify amendment candidates; it must never alter how the validator or executor applies invariants. The observability layer is a witness, not a judge.

- **HO-5 Observer neutrality**: attaching an AuditProvider to a workflow run must not change the run's output values, timing-dependent branch outcomes, or error codes. The same workflow with no AuditProvider and with a fully-instrumented AuditProvider must produce semantically identical `@out` and `@err` results on deterministic inputs.

- **HO-6 Structured event taxonomy**: the set of observable event types is closed and versioned. An implementation must not emit ad-hoc string events; it must use the canonical taxonomy defined in §4.2. New event types require a spec amendment (minor version bump).

- **HO-7 Monotonic sequence & correlation** [ADDED v1.1.0]: every emitted event carries (a) a run-monotonic **sequence number** assigned in strict emission order and (b) a **correlation id** shared by all events of the same logical run. Consumers order and deduplicate a multiplexed or reordered event stream by `(correlation_id, sequence)` alone — never by arrival order or wall-clock time (reinforces HO-3 append-only ordering without imposing a transport). Sequence is dense and gap-free within a run; a gap signals dropped events. This makes ordering an explicit contract rather than an emergent property of synchronous emission, so an async or buffered audit sink remains correct.

- **HO-8 Cost-attribution token classes** [ADDED v1.2.0]: a `model_response` event carries a **token-class breakdown** — fresh `input`, `output`, and, when the host provider reports them, `cache_read` and `cache_creation` — as distinct fields, not a single opaque total. Because a cached-prefix read is billed at a fraction of fresh input, collapsing these into one number makes cost un-attributable and a cache regression invisible. The classes are optional (a provider that reports only a total leaves the cache fields absent, never zero-faked) and remain within the data-safety boundary (§4.4 — counts only, never content). This lets a host compute per-run and per-step cost from the trace alone, and lets an outer loop detect a cache-warmth regression from telemetry rather than from an invoice.

- **HO-9 Execution-authenticity receipt** [ADDED v1.3.0]: a step's execution event MAY carry a **host-supplied, model-unforgeable receipt** binding the step's identity and its observed result, so a downstream verifier can tell a genuine step result from a fabricated one and a narration cannot claim a step ran that did not. The signing/verification mechanism is **host-supplied** (LP-2 style — no cryptographic dependency in the nodus core, mirroring the LP-9 attestation seam); the receipt is an **opaque, secret-free token** that crosses the data-safety boundary (§4.4) like any other counts-only descriptor — the signing secret is never placed in a trace, a prompt, or the model's context. Receipts are optional and additive: a host that supplies no receipt provider emits events unchanged, so observer neutrality (HO-5) is preserved. Like HO-8, this extends existing event records with an optional field rather than adding an event type (HO-6). This is the nodus realization of the main `l1-tool-receipts` execution-authenticity contract — the workflow-side witness that a step's result is real, host-supplied exactly as the LP-9 attestation witness is.

- **HO-10 Trace-completeness honesty** [ADDED v1.4.0]: a persisted trace is either **complete** — it carries a terminal `RunManifest` (status ∈ {Ok, Error, ConstraintHalt, ValidationError}, delivered by `run_complete`) — or explicitly **truncated**, and a consumer can tell which **from the trace alone**. If the host process is killed, panics, or is otherwise lost mid-run, `run_complete` is never called and the trace is a *headless fragment*: events up to some `seq` with no terminal manifest. Such a fragment MUST be distinguishable from a completed run and from a still-running one, and MUST NOT be mistaken for a whole run by any downstream consumer (the outer evolution loop, health scoring, differential replay). The absence of a terminal manifest is itself the signal "this run did not finish — treat the trace as a fragment." This extends the honesty HO-7 already gives for *dropped* events (a gap in the dense `seq` inside the recorded range) to the *truncated tail* (missing records after the last recorded event), so any trace classifies as complete, gap-damaged, or truncated. Crucially, nodus does **not** attempt to capture the crash that truncated the trace — that is the host's forensic diagnostic-log plane (the nodus realization of the main `l1-diagnostic-log` concept, whose DL-2 owns native-fault and pre-init capture); HO-10 guarantees only that the *semantic* trace never lies about being complete when it was truncated, naming the boundary between what nodus witnessed and what only the host's lower plane could. This is the observability analog of the honest-coverage-boundary discipline (a named gap is not a hidden one): the invariant is a read-side interpretation of manifest presence/absence, adds no new hot-path emission, and leaves a normally-completing run entirely unaffected (purely additive, HO-5 observer-neutrality preserved).

- **HO-11 Single-stream dual legibility** [ADDED v1.5.0]: an `ExecutionEvent` MAY carry a **host-rendered, human-legible one-line message derived from its own structured fields**, so the same single event stream serves both a machine/AI analyzer and a human reader without a second, parallel human log to keep in sync — double-logging both risks divergence and doubles the volume a reader must reconcile. The structured fields remain the **source of truth**: the human rendering MUST NOT introduce a fact absent from the fields nor contradict them (it is a faithful projection, not a new record), and it stays within the data-safety boundary (§4.4) — it renders descriptors and counts, never raw user content. The rendering, and any localization of it, is **host-supplied** (LP-2 style — the nodus core names no rendering or locale vocabulary); a host that supplies no renderer emits events exactly as today, so this is optional and additive and preserves observer neutrality (HO-5). Like HO-8/HO-9, it extends existing event records with an optional field rather than adding an event type (HO-6). This is the nodus realization of the main `l1-log-legibility` single-canonical-event / dual-audience-faithfulness contract (LL-1/LL-2): one stream legible to both a human and a machine, machine-is-truth, instead of two streams that can drift. The broader legibility & economy controls that *bound* a stream — payload policy, compaction, a budget-bounded agent-context feed (LL-4/LL-8) — stay host-side (LP-1/LP-2); nodus already contributes the pieces it owns (HO-6 closed structured taxonomy, §4.4 counts-not-content payload economy at the source, HO-7 sequence for dedup/ordering), and HO-11 adds only the dual-legibility of the single event.

- **HO-12 Execution-mode provenance** [ADDED v1.6.0]: the `RunManifest` declares the run's **execution mode** — `real` versus `simulated` (a modeled/dry-run play-out where effects are mocked or suppressed) — and, when simulated, its fidelity, so a downstream consumer can exclude simulated runs from real-run analytics and never mistake a modeled play-out for production behaviour. A simulated run is one where the **host** has substituted modeled providers (a mock `ModelProvider`, a no-effect tool/host surface) to exercise the workflow's mechanics without real effects; the substitution is host-supplied (host-neutral, LP-1/LP-2), and nodus records which mode the host declared for the run. The one exception is a substitution nodus makes itself: a run executed on the crate's built-in stub model provider is simulated by construction and records itself as simulated — never as `real`, since nothing the host declared made it real. The marker is optional and additive: a manifest without it is `real` (today's behaviour), so observer neutrality (HO-5) and the closed taxonomy (HO-6) are preserved. This is the nodus realization of the main `l1-simulation` legible-fidelity / observable-play-out contract (SIM-3/SIM-6) — the manifest-level witness that keeps a simulated trace from being read as, or polluting the analytics (HO-8 cost accounting, host health scoring) of, a real run. It sits in the same honesty family as HO-10 (a trace never lies about its completeness) and HO-11 (a rendering never lies about its fields): here, a trace never lies about whether it was real.

- **HO-13 Per-item derivation lineage** [ADDED v1.7.0]: an `ExecutionEvent` for a **collection-mapping step** — a `~MAP` element, a `~FOR` iteration, or a pipeline transform over a `List` value — MAY carry an optional **derivation descriptor** mapping each **produced element** to the **source element(s)** it was derived from (a reference mapping: source index/id → produced index/id), so a consumer can reconstruct end-to-end **item lineage** from the trace alone. The descriptor is **side-band metadata on existing events** (like the HO-8 cost fields and the HO-11 message), never a new event type (HO-6) and **never part of the workflow's `Value` payload** — so the closed value-type system (NL-7) is untouched and lineage can neither leak into nor perturb business data (matching the main `l1-data-lineage` LN-5 metadata-not-payload rule). It records **references** — element indices/ids within the mapping domain — **never copies** of element content, staying within the §4.4 data-safety boundary (descriptors/positions, not raw values), so tracing neither duplicates nor leaks payload (LN-8). The mapping is recorded as it truly is: **fan-out** (`split-out`, one input → many outputs), **fan-in** (`~JOIN` collect, many → one), and **filter** (a `~MAP` that drops an element) each record their real correspondence (LN-4). Optional and additive — a host emitting no derivation descriptor behaves exactly as today, so HO-5 observer-neutrality and the HO-6 closed taxonomy are preserved. This is the nodus realization of the main `l1-data-lineage` derivation-traceability contract — per-step immediate links (LN-2) composing into transitive end-to-end lineage (LN-3), captured as side-band metadata (LN-5) referencing not copying (LN-8) — and it sits in the observability plane precisely because lineage is *observational*: nodus contributes the source-side per-element mapping on the trace, and the host walks it to answer "what produced this."

- **HO-14 Aggregation-safe measurements** [ADDED v1.8.0]: every numeric measurement an `ExecutionEvent` carries — elapsed time, the HO-8 token classes, iteration and element counts — is either a **real measurement** or an explicit **unavailable** marker distinguishable from every real value **including zero**. Emitting `0` for a quantity that was not measured, silently omitting the field, and carrying the previous value forward are each forbidden. The rule distinguishes **not applicable** from **unavailable** and applies only to the latter: a field the taxonomy does not define for a given event type is simply absent (it was never in scope), whereas a field that *is* defined for that event and whose value could not be obtained MUST carry the unavailable marker. In particular the optional HO-8 cache classes on `model_response` are *unavailable* — never `0` — when the provider reports no cache accounting, so a host cannot mistake "the provider said nothing" for "the cache returned nothing". The reason is downstream and structural: a host that rolls events up into a lower-resolution retained record computes averages, minima, maxima, and coverage from these numbers, and a fabricated zero corrupts all four at once — it drags a mean toward zero, produces a minimum that is not a minimum, and makes an interval in which nothing could be measured indistinguishable from an interval in which nothing happened. The host owns retention, tiering, and aggregation entirely (LP-2 — the core names no tier, no window, and no aggregate); nodus owns only the guarantee that the values entering that machinery never lie about their own availability. This is the **measurement-grain** member of the same honesty family as HO-10 (a trace never lies about its completeness), HO-11 (a rendering never lies about its fields), and HO-12 (a trace never lies about being real): here, a number never lies about having been taken. Purely additive and read-side for existing consumers — a host that ignores the marker sees today's fields.
- **HO-15 Stable cross-run step identity** [ADDED v1.8.0]: every step event carries a **step identity that is stable across runs of the same workflow** — the same step in two runs yields the same identity — derived from the step's position and name in the workflow definition rather than from any per-run allocation (a per-run counter, a memory address, a fresh id). HO-7's `(correlation_id, seq)` makes events *within* one run orderable; HO-15 makes **the same step across many runs** one comparable series, which is precisely what a host needs to compare a window of recent runs against a baseline window of earlier ones, or to detect that one step's cost or latency regressed. A definition edit that genuinely changes what a step *is* changes its identity; a re-run, a retry, a resumed run (NL-12), and a recursive child invocation (NL-18, which additionally carries its parent/root correlation) do not. Any re-mapping of identities across a definition edit is host-supplied (LP-2) — nodus guarantees stability, not migration. Deterministic (NL-6): the same definition yields the same identities.
- **HO-16 Optional anomaly annotation** [ADDED v1.8.0]: an `ExecutionEvent` MAY carry an optional **host-supplied boolean** marking the event's measurement as anomalous relative to that step's own history, and — distinctly — an optional **unscored** marker meaning *no verdict was available*. Nodus computes no verdict, holds no model, stores no history, and names no algorithm, threshold, window, or sensitivity (LP-2, entirely host-supplied); it reserves only the **carrier**, so a verdict travels on the same stream as the measurement it qualifies rather than in a parallel record that can drift out of alignment with it. The annotation is a flag, never content, and so stays inside the §4.4 data-safety boundary. *Unscored* is never *normal*: the absence of a verdict is emitted as absence, matching HO-14's unavailable marker and HO-10's truncation marker. Like HO-8/HO-9/HO-11/HO-13, it is an optional field on existing events — never a new event type (HO-6) — so a host that annotates nothing behaves exactly as today and HO-5 observer neutrality is preserved (purely additive).

- **HO-17 Transient-versus-durable stream separation** [ADDED v1.9.0]: the observation stream MAY carry **transient** events — incremental deltas of a model response still being produced, live progress ticks — that are explicitly **not part of the durable trace**: never persisted, never assigned a position in the durable sequence, and merged into and then **superseded by** the durable event they were producing (extending HO-7's streaming chunk-merge from a merge rule into a durability boundary). Three guarantees follow, and each closes a way a consumer could be misled. **Reconstructibility**: the durable trace MUST be complete without any transient event — a consumer that subscribed late, dropped every transient, or never subscribed at all holds exactly the same record as one that saw them all, so a live view is an *affordance* and never a source of record. **Non-interference with the integrity invariants**: HO-3 append-only immutability governs durable events only (a transient is not an amendment of the record), HO-7's monotonic sequence numbers the durable stream only (so a missed transient is never a detected gap), and HO-10's complete/gap-damaged/truncated classification reads durable events only (so a transient tail cut off by a disconnect never makes a completed run look truncated). **Labelled at consumption**: a transient event MUST be distinguishable from a durable one at the point it is received — a consumer must never have to guess whether what it just read will still exist afterwards, because a consumer that persists a transient has silently forked the record. The §4.4 data-safety boundary applies to transient events exactly as to durable ones (a live view is not a licence to stream raw content). Optional and additive: a host emitting no transient events behaves exactly as today, and HO-5 observer neutrality is preserved.

- **HO-18 Variant provenance in the run manifest** [ADDED v1.10.0]: where the host resolved one or more **named exposure switches** for a run (a staged behaviour change, an A/B arm, a reserved holdout), the `RunManifest` records the **resolved switch names and the values the run actually executed under**, so a trace is attributable to exactly one arm. Two consequences, and both are the point. **Partitionability**: a consumer can split recorded runs by arm and compare like with like — without this, a comparison silently mixes runs from both sides of a change and reports the difference as noise, which is the failure mode that quietly invalidates every measurement built on the trace. **Non-straddling**: because the manifest records *one* value per switch for the whole run, a run that straddled two variants is unrepresentable — the host resolves each switch once at run start and freezes it for the run's lifetime (LP-19), so there is no way to record, and therefore no way to produce, a run that was half one thing and half another. Nodus computes no assignment, holds no rollout state, and names no fraction, hash, subject, or targeting rule (LP-2, entirely host-supplied); it records only the values the host declared. The record is names and values within the §4.4 data-safety boundary — never a targeting rule, a subject identifier, or a payload secret. Optional and additive: a manifest without switch provenance describes a run under the prevailing defaults, exactly today's behaviour, and HO-5 observer neutrality is preserved. Same manifest-level honesty family as HO-10 (a trace never lies about its completeness) and HO-12 (a trace never lies about whether it was real): here, a trace never lies about which variant produced it. Composes HO-15 — a stable cross-run step identity is what makes the same step comparable *across* arms, and HO-18 is what makes the arms distinguishable.

- **HO-19 Stable, message-independent fault-identity contribution** [ADDED v1.11.0]: a `step_error` event carries a **stable identity contribution** for the failure it records, composed of the step identity (HO-15), the typed `NODUS:*` error code, and — where the workflow declares one — an explicit workflow-supplied discriminator, which takes precedence. The contribution is **never derived from rendered message text**. This is the load-bearing negative: a message routinely carries interpolated values (an identifier, a path, a count, a timestamp) that differ on every occurrence, so a host grouping failures by message text shatters one recurring failure into thousands of singletons and produces a record containing no recognizable problems at all — only events. Because the contribution is derived from the workflow *definition* rather than from a run, it is stable across runs, retries, resumes (NL-12), and recursive children (NL-18) exactly as HO-15 is, and it is deterministic (NL-6). Nodus performs **no grouping**: it computes no equivalence, holds no fault record, and names no rule, ladder, or configuration version (LP-2 — grouping is entirely host-supplied); it guarantees only that the *inputs* the host groups on are stable and content-independent. Optional and additive — a host that groups by its own means is unaffected — and the contribution stays inside the §4.4 data-safety boundary (identifiers and a typed code, never rendered content). The nodus realization of the new main `l1-fault-lifecycle` identity contract: its layered ladder (FL-3) and declared-outranks-computed rule (FL-4), supplied from the source side.

- **HO-20 The run manifest is a re-execution recipe** [ADDED v1.12.0]: the `RunManifest` records what a **re-execution of this run** requires — the workflow definition's content identity, the resolved schema/vocabulary units actually loaded (NL-16 `@needs`), the capability set that satisfied the LP-8 manifest, the resolved exposure-switch values (HO-18), the execution mode (HO-12), and the nodus version — so a run is re-executable **from the manifest alone**, without the host reconstructing what was in force. Two honesty rules govern it. **Re-executable is not reproducible**: nodus's own evaluation is deterministic (NL-6), but a run containing model calls is not, so the manifest states which of the two it supports rather than letting a reader infer exactness from the presence of a recipe — a recipe that implies a reproducibility it cannot deliver teaches its readers to distrust every recipe. **A field that could not be captured is marked unavailable, never omitted** (the HO-14 rule at manifest grain): a silently short manifest reads as a complete one, and the reader's belief that nothing was missing is exactly the failure. Nodus records only what it resolved; it embeds nothing into produced artifacts, names no file format, and performs no replay — embedding, transport, and re-execution are host concerns (LP-1/LP-2). Optional and additive: a host reading no manifest recipe behaves exactly as today, and the record stays inside the §4.4 data-safety boundary (identities, versions, and names — never rendered content or a secret's value). The nodus realization of the new main `l1-reproduction-recipe` contract — its three layers (RR-2: the workflow is the request, the resolved vocabulary and capabilities are the components, the switch values and mode are the ambient configuration), content identity over names (RR-3), the producing version (RR-4), declared omissions (RR-6), and determinism stated rather than implied (RR-8).

## 4. Detailed Design

### 4.1 AuditProvider Role

The `AuditProvider` is a stateful observer registered once per workflow run. It receives events from the executor in emission order. The built-in no-op implementation accepts all events and discards them, satisfying the interface without any I/O.

```text
[REFERENCE]
Role:          AuditProvider
Interface:     record_event(event: ExecutionEvent) → void
               run_complete(manifest: RunManifest)  → void
Built-in:      NoopAuditProvider  (discards all events)
Scope:         one instance per workflow run
               (re-instantiated for each run; not shared across runs)
```

A host project plugs in an `AuditProvider` implementation that persists events to a file, a database, a streaming pipeline, or any other store. The nodus executor calls `record_event` during execution and `run_complete` once the run terminates (success, error, or hard-constraint halt).

### 4.2 Execution Event Taxonomy

| Event type | When emitted | Required fields |
| --- | --- | --- |
| `step_start` | Before a step's command is dispatched | `step_index`, `step_command`, `input_vars` (snapshot) |
| `step_end` | After a step's command returns | `step_index`, `step_command`, `output_vars` (delta), `elapsed_ms` |
| `step_error` | When a step produces an error code | `step_index`, `step_command`, `error_code` (NODUS:* taxonomy), `error_detail` |
| `constraint_hit` | When a hard (`!!NEVER`/`!!ALWAYS`) rule fires | `rule_name`, `triggering_step_index`, `halt: true/false` |
| `branch_taken` | When a conditional (`?IF`/`?ELIF`/`?ELSE`) resolves | `step_index`, `branch_label`, `condition_result` |
| `loop_iteration` | At the start of each `~FOR`/`~UNTIL` body | `step_index`, `loop_type`, `iteration_number`, `bound_vars` |
| `macro_enter` | When `RUN(@macro_name)` begins execution | `macro_name`, `call_step_index` |
| `macro_exit` | When a macro body completes | `macro_name`, `call_step_index`, `elapsed_ms` |
| `model_call` | Immediately before dispatching to the ModelProvider | `step_index`, `command`, `input_summary` (no raw text — §4.4) |
| `model_response` | Immediately after the ModelProvider returns | `step_index`, `command`, `output_summary`, `elapsed_ms`, token classes `input`/`output`/`cache_read?`/`cache_creation?` (HO-8) |

> New event types added in future minor versions must not reuse existing type names.

### 4.3 Run Manifest

At run completion the executor calls `run_complete` with a `RunManifest`:

```text
[REFERENCE]
RunManifest fields:
  workflow_name   : Text     — §wf: identifier
  schema_version  : Text     — BUILTIN_SCHEMA_VERSION at run time
  run_id          : Text     — unique per run (UUID or equivalent)
  started_at      : Text     — ISO-8601 timestamp
  elapsed_ms      : Int      — total wall-clock duration
  status          : Enum     — Ok | Error | ConstraintHalt | ValidationError
  error_code      : Text?    — NODUS:* code if status ≠ Ok
  total_steps     : Int      — count of steps attempted (excluding unentered branches)
  event_count     : Int      — total events emitted
```

### 4.4 Data Safety Boundary

The `model_call` and `model_response` events must not log raw user-content strings. `input_summary` and `output_summary` are structured descriptors (field names, types, length hints) — never the verbatim content. This boundary:

- Prevents user data from appearing in execution logs
- Keeps traces lightweight enough for large-scale analysis
- Allows trace sharing across team members who may not have access to the underlying data

Any field that would contain user-controlled text must be replaced with a schema-level descriptor.

### 4.5 Frozen-vs-Evolvable Boundary

The observability contract identifies which workflow components are frozen (cannot be changed by harness evolution) and which are evolvable:

| Component | Frozen | Evolvable |
| --- | --- | --- |
| NL-1…NL-10 invariants | Yes | No |
| E013/E014 error codes | Yes | No |
| BUILTIN_SCHEMA_VERSION | Yes (per release) | Per minor release |
| `@steps:` sequence | No | Yes |
| `@macro:` definitions | No | Yes |
| `@ctx:` field shape | No | Yes |
| `!!NEVER`/`!PREF` rules | Frozen once active | Evolvable before activation |
| Schema vocabulary (host extension) | No | Yes |

> The frozen layer is the scoring signal. The evolvable layer is the harness. Evolving the frozen layer is a spec amendment, not a harness improvement.

### 4.6 Sequence, Correlation, and Streaming Merge [ADDED v1.1.0]

Every event and manifest gains two ordering fields, and streamed model output is
collapsed into one logical record:

```text
[REFERENCE]
ExecutionEvent gains:
  seq            : Int      — run-monotonic, dense, gap-free emission counter (HO-7)
  correlation_id : Text     — shared by all events of one run (= RunManifest.run_id)

RunManifest gains:
  event_count    : Int      — already present; now also the highest seq + 1 (gap check)
```

**Correlation binding.** The `correlation_id` is set once, at run construction,
from the same identifier as `RunManifest.run_id`. It flows to every event without
the executor threading it through each call site — the audit context holds it for
the run's duration. If a run is entered without an explicit id, the executor
generates one at the root and binds it before the first event, so no event is ever
emitted uncorrelated.

**Streaming merge.** A `model_call`/`model_response` pair may, for a streaming
model, arrive as many incremental chunks. The observability layer merges them into
a **single** logical `model_response` record at completion: chunks are accumulated,
a *finish-reason* predicate detects the terminal chunk, and the merged record
carries the final `output_summary` and total `elapsed_ms`. Individual chunks are
not separate taxonomy events (they would violate HO-6's closed set and inflate the
trace); the merge preserves one attributable record per model turn (HO-2). The
merge is a witness-side fold — it never changes what the model returned to the
workflow (HO-5).

This section is the projection substrate the environment/trajectory contract
(`l1-nodus-environment.md`) relies on: a `TrajectoryEntry.seq` is exactly this
`seq`, so a trajectory is an ordered slice of the correlated event stream.

### 4.7 Trace Completeness & Abnormal Termination [ADDED v1.4.0]

The `RunManifest` (§4.3) is the **completeness witness**. Its presence or absence
classifies any persisted trace without inspecting event contents:

```text
[REFERENCE]
A trace is COMPLETE   iff it carries a terminal RunManifest (run_complete was called).
A trace is TRUNCATED  iff it has ≥1 event but no terminal RunManifest.
A trace is EMPTY      iff it has neither events nor a manifest.

Consumer rule (decidable from the trace alone, HO-10):
  terminal manifest present → COMPLETE  — trust the whole trace
  events but no manifest     → TRUNCATED — treat as a fragment, never a whole run
```

**Why absence is the signal.** A run that panics, is OOM-killed, or whose host
process is lost emits events up to some `seq = N` and then stops; `run_complete` is
never reached, so no manifest is written. The highest recorded `seq` marks *how far*
the run got; the missing manifest marks *that it did not finish*. Together with
HO-7's dense gap-free sequence — which exposes events dropped *inside* the recorded
range — this lets a consumer classify every trace as one of {complete, gap-damaged,
truncated} and never silently mistake a crash-truncated fragment for a whole run.

**Boundary with the host forensic plane.** nodus owns the semantic trace and its
completeness signal; it does **not** own crash capture. Whether a native traceback
of the terminating fault exists is a host concern — the forensic diagnostic-log
plane (the nodus realization of the main `l1-diagnostic-log` concept, DL-2), which
is installed below and earlier than the executor and survives a native fault the
executor cannot. nodus's contract stops precisely at "my trace honestly signals its
own truncation"; reconstructing what happened after the executor lost control is the
host's lower plane, and HO-10 forbids nodus from pretending otherwise.

**No hot-path cost.** HO-10 adds no new emission: a normally-completing run already
writes its manifest via `run_complete`, and an aborted run cannot emit anything more
(it is gone). The invariant fixes only the *interpretation* of a manifest's
presence/absence on the read side, so observer neutrality (HO-5) is untouched.

### 4.8 Single-Stream Dual Legibility [ADDED v1.5.0]

One event serves two readers. An `ExecutionEvent` MAY carry an optional
host-rendered `message` derived from its own fields, so a single `AuditProvider`
stream drives both a human-facing view and a machine/AI analyzer — no second prose
log to keep in sync, and none to drift:

```text
[REFERENCE]
ExecutionEvent gains (optional):
  message : Text?   — host-rendered one-line human rendering DERIVED from this
                      event's own structured fields (HO-11)

Faithfulness rule (LL-1/LL-2):
  `message` asserts nothing the structured fields do not already carry.
  A consumer that ignores `message` loses no fact (the fields are complete);
  a consumer that reads only `message` gets a faithful human summary.

Data-safety (§4.4): `message` renders descriptors and counts, never raw content —
  e.g. "step 4 GEN produced 2 output fields in 812 ms", never the generated text.
```

The structured fields stay the source of truth; the rendering is a projection of
them (LL-1). The renderer — and any localization — is **host-supplied** (LP-2), so
the nodus core carries no format or locale vocabulary and a host that supplies none
emits events exactly as today. The invariant is additive: it only fixes what an
optional `message` field *means* and constrains it to faithfulness plus §4.4, adding
no new hot-path obligation (a host that renders nothing pays nothing) and preserving
observer neutrality (HO-5) and the closed event taxonomy (HO-6).

**Boundary with host-side economy.** HO-11 makes the *single event* legible to both
audiences; it does not govern how a *stream* is bounded. The controls that keep a
stream from overloading a reader — payload policy, compaction/folding, a
budget-bounded feed of the trace back into an agent's own context — are the host's
(main `l1-log-legibility` LL-4/LL-8, LP-1/LP-2). nodus already supplies the
source-side pieces those controls build on: the closed structured taxonomy (HO-6, so
the stream is machine-parseable not ad-hoc prose), counts-not-content descriptors
(§4.4, so the source is payload-economical), and the dense correlation/sequence
(HO-7, so a consumer can order, deduplicate, and window).

### 4.9 Execution-Mode Provenance [ADDED v1.6.0]

A trace should say whether it came from a real run or a simulated play-out, so the
two are never confused and simulated runs never corrupt real analytics:

```text
[REFERENCE]
RunManifest gains (optional):
  execution_mode : Enum   — real | simulated   (absent → real; today's behaviour)
  sim_fidelity   : Text?  — when simulated: structural | modeled | shadow (host-declared)

Consumer rule (HO-12):
  execution_mode = simulated → exclude from real-run analytics (host health scoring,
                               HO-8 cost/usage accounting, the outer evolution loop);
                               the trace is a play-out of the mechanics, not production.
```

nodus does not mock anything. A host runs a workflow *in simulation* by substituting
modeled providers — a mock `ModelProvider`, a no-effect tool/host surface — to
exercise the mechanics without real effects; that modeling, the fidelity choice, and
the bounding are the host's simulation capability (main `l1-simulation` SIM-2/SIM-3,
host-side per LP-1/LP-2). nodus's sole contribution is to *record which mode the host
declared* on the manifest, making the trace self-describing: a consumer distinguishes
a real run from a simulated one from the manifest alone — exactly as HO-10 lets it
distinguish a complete trace from a truncated one. Without this marker, a hundred
simulated runs would be indistinguishable from a hundred real ones and would corrupt
cost and health analytics. Optional and additive: no marker means `real`, so a host
that never simulates is unaffected (HO-5/HO-6 preserved).

### 4.10 Per-Item Derivation Lineage [ADDED v1.7.0]

A collection-mapping step emits, per produced element, a reference to the source
element(s) it derived from — an optional field on the existing event, never a new
record and never in the `Value` payload (HO-13, NL-7 preserved):

```text
[REFERENCE]
derivation := [ source_ref ]                 // on a produced element's event
source_ref := (producing_step, source_index) // an index/id, never source content (LN-8)

// shapes (LN-4), recorded as they truly are:
//   ~MAP  N→N : produced[i].derivation = [ (step, i) ]
//   split 1→M : produced[k].derivation = [ (step, 0) ]   for k in 0..M
//   ~JOIN K→1 : produced[0].derivation = [ (step, 0) … (step, K-1) ]
//   filter    : dropped source index recorded; survivors keep their (step, i)
```

Walking these references transitively (produced → source, across steps) reconstructs
the end-to-end lineage of any item (LN-3) using only indices — the walker reads no
element content unless it explicitly dereferences one within the §4.4 boundary. Because
the descriptor is optional side-band metadata, a host that wants no lineage emits none
and the trace is byte-for-byte today's (HO-5). This is the source-side half of the main
`l1-data-lineage` contract; the walk and any dereference are host-side.

### 4.11 Aggregation-Safe Measurements, Cross-Run Identity, and the Anomaly Carrier [ADDED v1.8.0]

Three source-side guarantees that together let a host build a long-horizon, honest picture out of a stream of events, while nodus itself keeps no history and computes no statistic.

**Measurement availability (HO-14).** Every numeric field is a two-state value:

```text
[REFERENCE]
measurement := Taken(number) | Unavailable        // never 0 as a stand-in
// event fields affected: elapsed_ms, HO-8 token classes, iteration/element counts
// not-applicable ≠ unavailable: a field the taxonomy does not define for this
// event type is absent (never in scope); a defined field whose value could not
// be obtained is Unavailable — e.g. HO-8 cache classes when the provider
// reports no cache accounting are Unavailable, never 0
```

What this buys downstream, and why the alternative is not a small inaccuracy:

| Host computes | With `Unavailable` | With `0` substituted |
| --- | --- | --- |
| average | exact over what was taken | biased toward zero, silently |
| minimum | exact | always 0 — the statistic becomes meaningless |
| coverage (taken ÷ expected) | answerable | unanswerable — absence is erased |
| "was this idle or unobserved?" | distinguishable | indistinguishable |

**Cross-run identity (HO-15).** Two different questions need two different identifiers, and conflating them makes the second unanswerable:

```text
[REFERENCE]
(correlation_id, seq)  → ordering and grouping WITHIN one run   (HO-7)
step_identity          → the SAME step ACROSS runs               (HO-15)
                       := derived from definition position + name, not per-run allocation
```

A host comparing "the last twenty runs" to "the twenty before that" is comparing `step_identity` series. Without HO-15 every run produces fresh identifiers and no such comparison exists at all.

**Anomaly carrier (HO-16).** A verdict rides the event it qualifies:

```text
[REFERENCE]
anomaly := Anomalous | Normal | Unscored | (absent)   // host-supplied; absent = today's behaviour
```

`Unscored` is deliberately not `Normal` — a host whose detector has no history yet, or has fallen behind, emits *no verdict*, and the trace says so rather than implying the step was fine. The division of labour is strict: nodus emits the measurement and reserves the field; the host supplies whatever verdict it computes and owns every model, window, and threshold (LP-2).

### 4.12 The Durability Boundary [ADDED v1.9.0]

Two streams share one channel and must never be confused for one another:

| | Durable event | Transient event (HO-17) |
| --- | --- | --- |
| Persisted | Yes | **Never** |
| Position in the HO-7 sequence | Yes — a missing one is a detected gap | **No** — a missing one is not a gap |
| Governed by HO-3 append-only | Yes | No — it is not part of the record to append to |
| Read by HO-10 completeness classification | Yes | **No** — a cut-off transient tail never marks a run truncated |
| Fate | Is the record | Merged into, then **superseded by**, the durable event it was producing |
| Subject to §4.4 data safety | Yes | **Yes** — equally |

```text
[REFERENCE]
// The reconstructibility test HO-17 imposes:
durable_trace(consumer_that_saw_every_transient)
  == durable_trace(consumer_that_saw_none)
// If the two differ, a transient has leaked into the record — a defect.
```

The invariant exists because a live-rendering stream is the easiest thing in an observability design to accidentally promote into evidence. Once a consumer persists what it was shown live, two records exist, they disagree at the edges (a disconnect truncates one and not the other), and no reader can tell which is authoritative. Labelling transients at consumption and forbidding them a sequence position removes the possibility rather than documenting against it.

## 5. Implementation Notes

1. `record_event` is called in the executor's hot path — the built-in no-op implementation costs a single indirect call.
2. The AuditProvider is registered at run construction time (before lexing); an unregistered run uses NoopAuditProvider automatically.
3. `run_id` generation is the caller's responsibility — the executor accepts it as a run parameter, keeping the executor deterministic and testable.
4. `elapsed_ms` is measured by the executor's internal clock, not by wall time; the AuditProvider must not rely on system time for ordering.

## 6. Drawbacks & Alternatives

**Alternative: log-line events** — emit unstructured text logs instead of typed events. Rejected: unstructured logs cannot be queried programmatically; harness analysis requires machine-readable records.

**Alternative: post-run reflection** — reconstruct the trace from final state rather than recording during execution. Rejected: violates HO-2 (per-step attribution) and HO-3 (append-only immutability) because state reconstruction is necessarily lossy for branching/looping execution paths.

**Alternative: embedded tracing library** — import an external observability library (OpenTelemetry etc.) directly into nodus core. Rejected: violates LP-1 (host neutrality) and LP-2 (extension via abstract interfaces) from `l1-nodus-portability.md` — the host chooses the observability backend.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[EXECUTOR]` | `crates/nodus/src/executor.rs` | The execution engine where AuditProvider hook points must be added |
| `[ERROR-CODES]` | `crates/nodus/src/vocab.rs` | NODUS:* error taxonomy (used in `step_error.error_code`) |
| `[INVARIANTS]` | `crates/nodus/src/validator.rs` | E013/E014 and all NL-invariant enforcement (frozen boundary, HO-4) |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.12.1 | 2026-09-24 | Core Team | Consistency pass (2026-09-24): HO-12 stated that nodus itself mocks nothing, yet the crate's own entry points run on its built-in stub model provider. The invariant now names that exception: a run on the built-in stub is simulated by construction and records itself as simulated, never as real. |
| 1.12.0 | 2026-07-23 | Core Team | Added HO-20 (the run manifest is a re-execution recipe) — the RunManifest records what re-executing the run requires: the workflow definition's content identity, the resolved schema/vocabulary units actually loaded (NL-16 `@needs`), the capability set that satisfied the LP-8 manifest, the resolved exposure-switch values (HO-18), the execution mode (HO-12), and the nodus version, so a run is re-executable **from the manifest alone** without the host reconstructing what was in force. Two honesty rules: **re-executable is not reproducible** — nodus's own evaluation is deterministic (NL-6) but a run containing model calls is not, so the manifest states which of the two it supports rather than letting a reader infer exactness from the presence of a recipe; and **an uncapturable field is marked unavailable, never omitted** (the HO-14 rule at manifest grain), since a silently short manifest reads as a complete one. Nodus records only what it resolved — it embeds nothing into produced artifacts, names no file format, and performs no replay (embedding, transport, and re-execution are host concerns, LP-1/LP-2); optional + additive, within the §4.4 data-safety boundary (identities, versions, names — never rendered content or a secret's value). The nodus realization of the new main l1-reproduction-recipe contract: its three layers (RR-2 — workflow as request, resolved vocabulary/capabilities as components, switch values and mode as ambient configuration), content identity over names (RR-3), producing version (RR-4), declared omissions (RR-6), determinism stated not implied (RR-8). L1 stays Stable (C9); l2-nodus-observability carries HO-20 as a pending Invariant-Compliance obligation reconciled at magic.task (HO-8…HO-19 precedent). |
| 1.11.0 | 2026-07-23 | Core Team | Added HO-19 (stable, message-independent fault-identity contribution) — a `step_error` event carries an identity contribution composed of the step identity (HO-15), the typed `NODUS:*` code, and an optional workflow-declared discriminator that takes precedence, and **never derived from rendered message text**: a message routinely carries interpolated values (identifier, path, count, timestamp) that differ every occurrence, so a host grouping by message text shatters one recurring failure into thousands of singletons and produces a record with no recognizable problems in it, only events. Derived from the workflow *definition* rather than a run, so it is stable across runs, retries, resumes (NL-12), and recursive children (NL-18) exactly as HO-15 is, and deterministic (NL-6). Nodus performs **no grouping** — no equivalence computed, no fault record held, no rule/ladder/configuration version named (LP-2, grouping entirely host-supplied); it guarantees only that the *inputs* the host groups on are stable and content-independent. Optional + additive, within the §4.4 data-safety boundary (identifiers and a typed code, never rendered content). The nodus realization of the new main l1-fault-lifecycle identity contract (FL-3 layered ladder / FL-4 declared-outranks-computed), supplied from the source side. L1 stays Stable (C9); l2-nodus-observability carries HO-19 as a pending Invariant-Compliance obligation reconciled at magic.task (HO-8…HO-18 precedent). |
| 1.10.0 | 2026-07-23 | Core Team | Added HO-18 (variant provenance in the run manifest) — where the host resolved named exposure switches for a run (a staged behaviour change, an A/B arm, a reserved holdout), the RunManifest records the resolved names and the values the run executed under, so a trace is attributable to exactly one arm. **Partitionability**: a consumer splits recorded runs by arm and compares like with like — without it a comparison silently mixes both sides of a change and reports the difference as noise, quietly invalidating every measurement built on the trace. **Non-straddling**: one value per switch for the whole run makes a half-one-variant-half-another run unrepresentable, the host resolving each switch once at run start and freezing it (LP-19). Nodus computes no assignment, holds no rollout state, and names no fraction/hash/subject/targeting rule (LP-2, entirely host-supplied) — it records only what the host declared, as names and values within the §4.4 data-safety boundary, never a targeting rule or subject identifier or payload secret. Optional + additive (a manifest without switch provenance describes a run under prevailing defaults, today's behaviour), HO-5 preserved. Same manifest-level honesty family as HO-10 (completeness) and HO-12 (real-vs-simulated): a trace never lies about which variant produced it. Composes HO-15 — stable cross-run step identity makes the same step comparable *across* arms, HO-18 makes the arms distinguishable. The nodus realization of the new main l1-staged-rollout contract (SR-2 reproducible assignment / SR-8 explainable decisions / SR-11 honest comparison). L1 stays Stable (C9); l2-nodus-observability carries HO-18 as a pending Invariant-Compliance obligation reconciled at magic.task (HO-8…HO-17 precedent). |
| 1.9.0 | 2026-07-23 | Core Team | Added HO-17 (transient-versus-durable stream separation) + §4.12 — the observation stream MAY carry transient events (incremental deltas of an in-progress model response, live progress ticks) that are never persisted, never given a position in the durable sequence, and merged into then superseded by the durable event they were producing, extending HO-7's streaming chunk-merge from a merge rule into an explicit durability boundary. Three guarantees: **reconstructibility** — the durable trace is complete without any transient, so a consumer that subscribed late or dropped every transient holds the identical record and a live view is an affordance never a source of record; **non-interference** — HO-3 append-only, HO-7 sequence-gap detection, and HO-10 complete/gap-damaged/truncated classification all read durable events only, so a transient tail cut off by a disconnect never makes a completed run look truncated and a missed transient is never a detected gap; **labelled at consumption** — a transient is distinguishable from a durable event when received, because a consumer that persists a transient silently forks the record into two disagreeing copies with no way to tell which is authoritative. §4.4 data safety applies to transients equally (a live view is not a licence to stream raw content). Optional + additive (a host emitting no transients behaves exactly as today), HO-5 observer-neutrality preserved. L1 stays Stable (C9); l2-nodus-observability carries HO-17 as a pending Invariant-Compliance obligation reconciled at magic.task (HO-8…HO-16 precedent). |
| 1.8.0 | 2026-07-23 | Core Team | Added HO-14/HO-15/HO-16 + §4.11 — the source-side guarantees a host's long-horizon observational record depends on, with nodus keeping no history and computing no statistic. HO-14 aggregation-safe measurements: every numeric field is a real measurement or an explicit *unavailable* marker distinguishable from zero; emitting 0, omitting silently, or carrying the previous value forward are forbidden because a fabricated zero corrupts mean, minimum, maximum, and coverage simultaneously once a host rolls events into a lower-resolution retained record — the measurement-grain member of the HO-10/HO-11/HO-12 honesty family (a number never lies about having been taken); *not applicable* stays distinct from *unavailable* — a field the taxonomy does not define for an event type is absent, while a defined field whose value could not be obtained carries the marker, so the optional HO-8 cache classes are `Unavailable` and never `0` when a provider reports no cache accounting; retention/tiering/aggregation entirely host-supplied (LP-2). HO-15 stable cross-run step identity: a step's identity is derived from its definition position/name, not per-run allocation, so the same step across many runs is one comparable series — what a baseline-window comparison and a per-step regression check require, and which HO-7's within-run (correlation_id, seq) cannot provide; a definition edit that changes what a step is changes its identity, a re-run/retry/resume (NL-12)/recursive child (NL-18) does not; identity re-mapping across edits host-supplied; deterministic (NL-6). HO-16 optional anomaly annotation: an event MAY carry a host-supplied anomalous boolean plus a distinct *unscored* marker, nodus reserving only the carrier so a verdict travels on the same stream as the measurement it qualifies instead of a parallel record that can drift — no model, history, algorithm, threshold, or window in core (LP-2); a flag never content (§4.4); *unscored* is never *normal*, matching HO-14's unavailable and HO-10's truncation markers; an optional field on existing events like HO-8/HO-9/HO-11/HO-13, never a new event type (HO-6), HO-5 observer-neutrality preserved, purely additive. The nodus realization of the new main l1-observation-retention (OR-2 aggregate sufficiency / OR-3 gap-is-a-value), l1-anomaly-consensus (AC-3 inline flag / AC-6 unscored-is-never-normal), and l1-change-attribution (CA-2 derived baseline needs a comparable cross-run series) contracts. L1 stays Stable (C9); l2-nodus-observability carries HO-14…HO-16 as pending Invariant-Compliance obligations reconciled at magic.task (HO-8…HO-13 precedent). |
| 1.7.0 | 2026-07-09 | Core Team | Added HO-13 (per-item derivation lineage) + §4.10 — an ExecutionEvent for a collection-mapping step (a `~MAP` element, a `~FOR` iteration, a pipeline transform over a List) MAY carry an optional derivation descriptor mapping each produced element to the source element(s) it derived from (a reference mapping source index/id → produced index/id), so a consumer reconstructs end-to-end item lineage from the trace alone; side-band metadata on existing events (like HO-8 cost fields and the HO-11 message), never a new event type (HO-6) and never part of the workflow's Value payload so the closed value-type system (NL-7) is untouched and lineage cannot leak into or perturb business data (matching main l1-data-lineage LN-5 metadata-not-payload); records references (indices/ids within the mapping domain) never copies of element content, within the §4.4 data-safety boundary (LN-8); records the true shape — fan-out split-out (1→M), fan-in ~JOIN (K→1), filter drop — each as it is (LN-4); optional + additive so a host emitting none behaves exactly as today (HO-5 observer-neutrality + HO-6 closed taxonomy preserved). The nodus realization of the new main l1-data-lineage derivation-traceability contract — per-step immediate links (LN-2) composing into transitive end-to-end lineage (LN-3), captured as side-band metadata (LN-5) referencing not copying (LN-8) — sitting in the observability plane because lineage is observational: nodus contributes the source-side per-element mapping on the trace, the host walks it. L1 stays Stable (C9); l2-nodus-observability carries HO-13 as a pending Invariant-Compliance obligation reconciled at magic.task (HO-8…HO-12 precedent). |
| 1.6.0 | 2026-07-07 | Core Team | Added HO-12 (execution-mode provenance) + §4.9 — the RunManifest declares the run's execution mode (real vs simulated, plus fidelity when simulated) so a consumer excludes simulated runs from real-run analytics (health scoring, HO-8 cost accounting, the outer evolution loop) and never mistakes a modeled play-out for production. nodus mocks nothing (host substitutes modeled providers to run in simulation, LP-1/LP-2); it only records the host-declared mode, making the trace self-describing — a real run is distinguishable from a simulated one from the manifest alone, as HO-10 distinguishes complete from truncated. Optional + additive (absent = real, today's behaviour), HO-5/HO-6 preserved. The nodus realization of the new main l1-simulation legible-fidelity / observable-play-out contract (SIM-3/SIM-6); same honesty family as HO-10 (completeness) and HO-11 (legibility). L1 stays Stable (C9); l2-nodus-observability carries HO-12 as a pending Invariant-Compliance obligation reconciled at magic.task (HO-8/HO-9/HO-10/HO-11 precedent). |
| 1.5.0 | 2026-07-07 | Core Team | Added HO-11 (single-stream dual legibility) + §4.8 — an ExecutionEvent MAY carry an optional host-rendered human-legible one-line `message` derived from its own structured fields, so one AuditProvider stream serves both a human reader and a machine/AI analyzer without a second parallel prose log that can drift; the structured fields stay the source of truth (the rendering is a faithful projection that adds/contradicts no fact, LL-1/LL-2) and stays within §4.4 (descriptors/counts, never raw content); renderer + localization host-supplied (LP-2, no format/locale vocabulary in core), optional + additive so a host that renders nothing emits events as today, HO-5 observer-neutrality and HO-6 closed taxonomy preserved (an optional field on existing events, like HO-8/HO-9). Stream-bounding economy (payload policy, compaction, budget-bounded agent feed, LL-4/LL-8) stays host-side; nodus contributes the source-side pieces (HO-6 taxonomy, §4.4 counts-not-content, HO-7 sequence). The nodus realization of the new main l1-log-legibility single-canonical-event / dual-audience-faithfulness contract. L1 stays Stable (C9); l2-nodus-observability carries HO-11 as a pending Invariant-Compliance obligation reconciled at magic.task (HO-8/HO-9/HO-10 precedent). |
| 1.4.0 | 2026-07-07 | Core Team | Added HO-10 (trace-completeness honesty) + §4.7 — a persisted trace is complete (carries a terminal RunManifest via run_complete) or explicitly truncated (events but no manifest), decidable from the trace alone; a headless fragment left by a killed/panicked/OOM'd host process MUST NOT be mistaken for a whole run. Extends HO-7's dropped-event gap detection to the truncated tail (missing records after the last event), so any trace classifies as complete / gap-damaged / truncated. nodus does not capture the terminating crash — that is the host's forensic diagnostic-log plane (nodus realization of the new main l1-diagnostic-log concept, DL-2); HO-10 guarantees only that the semantic trace never lies about its own completeness. Read-side interpretation of manifest presence/absence — no new hot-path emission, HO-5 observer-neutrality preserved (purely additive). L1 stays Stable (C9); l2-nodus-observability carries HO-10 as a pending Invariant-Compliance obligation reconciled at magic.task (HO-8/HO-9 precedent). |
| 1.3.0 | 2026-07-02 | Core Team | Added HO-9 (execution-authenticity receipt) — a step's execution event MAY carry a host-supplied, model-unforgeable receipt binding step identity + observed result, so a verifier distinguishes a genuine step result from a fabricated one; signing mechanism host-supplied (LP-2, no crypto in core, mirroring the LP-9 attestation seam), receipt an opaque secret-free token within the data-safety boundary (signing secret never in trace/prompt/context), optional + additive so HO-5 observer neutrality holds, an optional field on existing events not a new event type (HO-6). The nodus realization of the new main l1-tool-receipts execution-authenticity contract. L1 stays Stable (C9); l2-nodus-observability carries HO-9 as a pending Invariant-Compliance obligation reconciled at magic.task. |
| 1.2.0 | 2026-07-02 | Core Team | Added HO-8 (cost-attribution token classes on model_response — fresh input/output plus optional cache_read/cache_creation as distinct fields, counts-only within the data-safety boundary) so per-run/per-step cost is computable from the trace and a cache-warmth regression is detectable from telemetry, not the invoice. Event taxonomy §4.2 model_response fields extended. |
| 1.1.0 | 2026-07-01 | Core Team | Added HO-7 (monotonic sequence + correlation id ordering contract) and §4.6 (sequence/correlation fields, run-scoped correlation binding, streaming chunk-merge into one logical model_response). Ordering is now an explicit `(correlation_id, seq)` contract, enabling async/buffered audit sinks; underpins the trajectory projection in l1-nodus-environment.md. |
| 1.0.0 | 2026-06-24 | Core Team | Initial spec — HO-1…HO-6, AuditProvider role, event taxonomy, run manifest, data safety boundary, frozen-vs-evolvable boundary |
