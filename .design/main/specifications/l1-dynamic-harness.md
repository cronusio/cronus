# Dynamic Harness Pattern

**Version:** 1.0.1
**Status:** Stable
**Layer:** concept

## Overview

A *dynamic harness* is a harness that changes its own behaviour **at run time**, in contrast to the offline evolution loop where a frozen harness is improved between discrete generations. This spec defines the dynamic harness pattern as a universal protocol and positions it as the run-time complement to the (offline) harness engineering pattern.

"Dynamic" has two orthogonal senses, and a conforming system may use either or both:

1. **Runtime adaptation** — a single run reshapes its own execution as it proceeds: it compacts its context when it grows too large, fails over to another provider on error, bounds an oversized tool result, or terminates a loop that is not converging. The harness specification stays fixed; the *execution* adapts.
2. **Runtime substitution** — the same harness specification executes over **interchangeable underlying runtimes** (different agent execution backends or model lineages), swapped or combined per role, under one uniform contract and one policy surface.

Both senses are governed by the invariants below. Neither may weaken the frozen evaluation contract of the offline pattern when used inside an evolution session.

## Related Specifications

- [l1-harness-engineering.md](l1-harness-engineering.md) — the offline sibling; this spec is its run-time complement and reuses HE-3 (frozen evaluation), HE-4 (evidence-backed amendment), HE-6 (transfer validity), and HE-8 (artifact extraction)
- [l1-agent-framework-skeleton.md](l1-agent-framework-skeleton.md) — paradigm-neutral agent primitives and the *generational* self-improvement loop; dynamic harness governs *within-run* adaptation, not generational lineage
- [l1-execution-graph.md](l1-execution-graph.md) — typed state channels, supersteps, dynamic spawn (EG-9), interrupt/resume; the substrate a run-time interceptor chain observes and reshapes
- [l1-output-contracts.md](l1-output-contracts.md) — inline output validation + retry; a specific case of bounded run-time adaptation (DH-5)
- [l1-orchestration.md](l1-orchestration.md) — office-model orchestration; the dynamic harness runs inside one executor of it
- [l2-model-router.md](l2-model-router.md) — an existing L2 realization of DH facets: dynamic routing, bandit exploration, promotion-by-score, circuit breaker
- [l2-context-management.md](l2-context-management.md) — an existing L2 realization of DH-5 context-shaping adaptation
- [l2-self-improvement.md](l2-self-improvement.md) — a related but non-conformant precedent: durable calibration/mistake/template learning from live operation, but unconditionally additive rather than gated — no DH-10 promotion-gate-with-margin step and no DH-8 machine-checkable predicted-flip scoring; a DH-8/DH-10 realization would need its own design, not an extension of this one
- [../../nodus/specifications/l1-nodus-observability.md](../../nodus/specifications/l1-nodus-observability.md) — the read-only observer contract (`AuditProvider`) that DH-2/DH-7 build upon
- [../../nodus/specifications/l1-nodus-portability.md](../../nodus/specifications/l1-nodus-portability.md) — the extension-point contract (LP-1…LP-7) that the nodus integration (§4.7) must satisfy

## 1. Motivation

The offline harness engineering pattern improves a harness *between* runs: evaluate a frozen harness over a task set, analyze the traces, amend one component, repeat. That loop is rigorous and reproducible, but it is blind to two realities:

- **A run can fail for run-local reasons the offline loop cannot pre-empt** — a context window fills mid-task, a provider rate-limits, a single tool emits a megabyte of output, a loop fails to converge. These need a decision *now*, inside the run, not a code change three generations later.
- **No single underlying runtime or model is best at everything** — planning, bulk code generation, careful security review, and adversarial verification have different sweet spots. Binding a harness to one runtime leaves reliability on the table; being able to substitute or combine runtimes per role captures it.

A dynamic harness addresses both while preserving the discipline of the offline pattern:

- Run-time adaptation is made **explicit, bounded, and observable** — it is a declared interceptor chain with declared triggers and budgets, not hidden control flow.
- Run-time substitution is made **contract-preserving** — the substrate changes, the input/output/error/policy contract does not.
- Improvements remain **falsifiable and auditable** — each change carries a machine-checkable prediction that the next evaluation scores automatically, and each harness version has a structural signature whose drift is measurable without re-running anything.

## 2. Constraints & Assumptions

- Run-time adaptation must never silently change a run's declared output contract, branch outcomes, or error codes outside the aspects an interceptor explicitly declares it may modify (DH-3).
- Inside an evolution session, the frozen evaluation pipeline (HE-3) remains immutable; dynamic adaptation operates *within* a scored run, it does not alter the scoring signal.
- Online adaptation requires a governing metric and a declared margin/regression bound before any learned change can be durably adopted; absent those, learned changes stay provisional.
- Runtime substitution assumes underlying runtimes are addressable behind a uniform contract; a runtime that cannot honour the contract is rejected before execution, not silently degraded.
- The pattern is host-agnostic. The nodus integration (§4.7) is one realization and must satisfy the portability contract (nodus LP-1…LP-7); it must not require host-specific types in any library interface.

## 3. Core Invariants

Rules any conforming dynamic harness implementation MUST honour. They add to — never relax — the offline pattern's HE-invariants.

- **DH-1 Two declared modes**: a dynamic harness declares which mode(s) it uses — *runtime adaptation*, *runtime substitution*, or both. The declaration is part of the harness specification; an undeclared mode is not exercised at run time.

- **DH-2 Observer / interceptor separation**: every run-time component is classified as either an **observer** (may only read execution state; provably cannot alter run outcomes) or an **interceptor** (may alter execution). A component MUST NOT be silently both. Observers follow the observer-neutrality rule; interceptors follow DH-3.

- **DH-3 Bounded interception**: each interceptor declares the execution aspects it may modify — drawn from a fixed taxonomy (context contents, tool availability, model/runtime selection, retry/loop control, error handling, environment injection) — and MUST NOT modify any aspect outside that declaration. Interception is a no-op with respect to declared-immutable state.

- **DH-4 Ordered, inspectable chain**: run-time adaptation is expressed as an ordered chain of *named* interceptors with deterministic composition order. The active chain and each interceptor firing are recoverable from the run trace. Adaptation is auditable, never hidden control flow.

- **DH-5 Explicit, budgeted triggers**: each adaptation fires only from an explicitly declared trigger (e.g. a token threshold, an error class, a round/step count, a validation failure) and consumes from a declared budget. No adaptation fires on undeclared conditions, and none fires unboundedly.

- **DH-6 Contract-preserving substitution**: when the same harness specification runs over a different underlying runtime, its input/output/error contract and its policy constraints are preserved; only the execution substrate changes. A runtime that cannot honour the contract is rejected before execution.

- **DH-7 Experience observability**: raw execution traces are distilled into **layered** digests (a cross-run summary plus per-unit detail) in which **every claim is traceable to the originating raw trace** (run identifier + step index). Analysis consumes digests by default but can always reach ground truth. This is the structured, provenance-bearing *input* to analysis — distinct from the HE-8 analysis *artifact* that is carried forward.

- **DH-8 Falsification ledger**: every harness change is recorded as an append-only manifest carrying the four HE-4 evidence fields **plus a machine-checkable predicted-flip set**. The next evaluation automatically scores predicted versus actual flips and assigns a verdict — *keep*, *revert*, or *partial*. A change whose predictions fail is reverted (or partially reverted) by an explicit, recorded action; a silent keep is forbidden.

- **DH-9 Structural genome & drift**: a harness has a computable **structural signature** derived from its components, independent of run outcomes. Two harness versions are comparable by structural distance, and drift from a declared baseline is detectable **without re-running the evaluation**. Structural drift is an advisory governance signal — it never acts as an automatic gate.

- **DH-10 Promotion-gated online adaptation**: a change learned during live operation (a routing preference, a memory entry, a retrieved exemplar) is **provisional** until it passes an explicit promotion gate — improvement on the governing metric beyond a declared margin with no regression beyond declared bounds. Un-promoted adaptations remain reversible and are never treated as durable harness state.

- **DH-11 Verification independence**: when a dynamic harness verifies its own output (review, judging, adversarial check), the verifier SHOULD be drawn from a different runtime or model lineage than the producer wherever substitution (DH-6) makes that available. Identical producer/verifier lineage is permitted but MUST be recorded as a reduced-independence condition.

- **DH-12 Order-dependence disclosure**: where online adaptation makes a run's outcome depend on the order in which work units are processed, that order-dependence MUST be disclosed in the run record. It bounds reproducibility and the transfer-validity claim (HE-6).

## 4. Detailed Design

### 4.1 The Two Modes

| Mode | Unit that changes | Stays fixed | Failure it answers |
| --- | --- | --- | --- |
| Runtime adaptation | the *execution* of one run | the harness specification | run-local conditions (context overflow, provider error, oversized output, non-convergent loop) |
| Runtime substitution | the underlying *runtime/model* | the input/output/error/policy contract | no single runtime is best at every role |

The modes compose: a harness may substitute the runtime for a sub-task *and* adapt that sub-task's execution. The offline evolution loop (HE-pattern) sits *above* both — it changes the harness specification itself, between runs.

### 4.2 Interceptor (Middleware) Chain

Runtime adaptation is realized as an ordered chain of interceptors. Each interceptor is a named unit that observes the execution at a defined seam and may modify only its declared aspects (DH-3). A representative, non-exhaustive taxonomy of adaptation kinds:

| Kind | Trigger (DH-5) | Declared mutable aspect (DH-3) |
| --- | --- | --- |
| Context-shaping | input-token threshold crossed | context contents (compaction / sliding window / selective retention) |
| Provider failover | provider error class | model/runtime selection |
| Output-bounding | tool result exceeds size cap | context contents (truncate / summarize the result) |
| Loop / turn control | round or step count, non-convergence signal | retry/loop control (continue / terminate / escalate) |
| Environment injection | run start / step boundary | context contents (inject environment facts, reminders) |
| Validation-retry | output-contract validator fails | retry/loop control (re-attempt with verdict feedback) — see `l1-output-contracts.md` |

```text
[REFERENCE]
run
 └── interceptor chain (ordered, named):
       [context-shaping] → [output-bounding] → [provider-failover] → [loop-control]
     each fires only on its declared trigger, consumes its declared budget,
     modifies only its declared aspect, and emits a trace event when it fires.
```

The chain is part of the harness specification (and therefore an evolvable harness component in the offline sense). Whether an interceptor fires, and what it changed, is observable — an interceptor that mutated execution invisibly violates DH-4.

### 4.3 Experience Observability Layers

Raw traces from non-trivial runs are large; analysis cannot operate on them directly without losing the signal. DH-7 mandates a **layered, sourced** digestion:

```text
[REFERENCE]
Layer 0 — raw trace            (per-run, per-step; ground truth)
Layer 1 — per-unit detail      (one digest per task/run: what happened, where it failed)
Layer 2 — cross-run summary    (recurring failure classes, frequencies, root-cause hypotheses)
            every Layer-1/2 claim carries a back-pointer (run_id, step_index) into Layer 0.
```

The analysis phase reads Layer 2 by default and drills to Layer 1 or Layer 0 before committing to any change. The **provenance back-pointer is mandatory**: a digest claim that cannot be traced to a raw trace is not admissible evidence under HE-4.

This layer is the structured *input* to the ANALYZE phase; the HE-8 artifact is its *output*. They are different objects: DH-7 compresses-with-provenance, HE-8 carries-forward-as-fresh-context.

### 4.4 Falsification Ledger & Change Manifest

DH-8 turns HE-4's evidence requirement into an automatic, append-only decision record. Each change emits a manifest:

```text
[REFERENCE]
change manifest (append-only):
  - iteration, timestamp, author
  - component changed              (exactly one — HE-5)
  - failure_evidence               (HE-4: failing units + trace excerpts, with back-pointers)
  - root_cause                     (HE-4: why, not just what)
  - targeted_fix                   (HE-4: the diff)
  - predicted_impact               (HE-4) MADE MACHINE-CHECKABLE:
        expected_fixes  = [unit ids predicted to flip fail → pass]
        at_risk         = [unit ids that may regress pass → fail]
  - verification (written by the NEXT evaluation):
        observed_flips, verdict ∈ {keep | revert | partial}
```

Verdict rules:

- **keep** — all `expected_fixes` flipped and no `at_risk` (or unexpected) regression occurred.
- **revert** — an expected fix did not occur, or a regression occurred (any regression of a severity-critical unit forces revert).
- **partial** — some predictions held; revert the parts that failed, keep the parts that held.

The manifest plus the run's append-only history (HE-7) is the audit trail; the verdict is derived mechanically, not asserted by the proposer.

### 4.5 Harness Genome & Drift Governance

DH-9 gives a harness a structural signature computable from its components alone — step/macro counts, schema vocabulary used, declared interceptor chain, policy blocks, input/output/error contract shape. Two properties follow:

- **Structural distance** — two harness versions can be compared without running either, producing a similarity score and a per-component delta.
- **Drift detection** — current structure can be diffed against a stored baseline; a drift report is produced from local comparison only, with **no model calls and no evaluation runs**.

Drift is **advisory** (DH-9): it surfaces "this harness has structurally diverged from its reviewed baseline" as a governance prompt, never as an automatic block. It complements the outcome-based signals (scores, flips) with a structure-based one.

### 4.6 Online Adaptation & Promotion Gates

DH-10 governs anything the harness *learns while serving*. Such a change is admitted in two stages:

```text
[REFERENCE]
1. provisional   — the learned change is applied but tagged provisional and is reversible.
2. promotion gate — measure the governing metric with vs. without the change:
                      promote IFF  delta_metric ≥ margin  AND  regression ≤ bound
                    (the gate may combine multiple criteria, e.g. quality up, cost flat, latency bounded).
3. durable       — only a gate-passing change becomes durable harness state; a failing
                    change is rolled back and the attempt is recorded.
```

This keeps live learning from silently degrading the harness: an exemplar that helped one task but hurts the aggregate never becomes permanent. It is the run-time analogue of HE-6 transfer validity — promotion is the gate that prevents over-fitting to whatever just happened.

### 4.7 Nodus Integration

Nodus is a portable, std-only, zero-dependency workflow DSL library. It already supplies most of what a dynamic harness needs; the gaps are small and expressible as abstract extension points (nodus LP-2), so they do not couple the library to any host (nodus LP-1).

| Area | Current nodus state | Proposed extension (abstract-interface only) |
| --- | --- | --- |
| Run-time interception (DH-2…DH-5) | Only a **read-only** observer seam exists (`AuditProvider`, bound by observer-neutrality). | Add a **write-capable interceptor seam** — the explicit counterpart to `AuditProvider`. An interceptor declares its mutable aspect (a closed enum mirroring DH-3) and fires at declared step seams; the runtime composes an ordered chain. Ships a built-in no-op (nodus LP-2). This is the *named, declared* category DH-2 requires so that "observer" and "interceptor" are never conflated. |
| Runtime substitution (DH-6) | `ModelProvider` already abstracts the model backend; swapping it substitutes the *model* under a fixed contract. | Generalize to a runtime-substitution abstraction *above* `ModelProvider` so a whole agent runtime (not just the model) is interchangeable behind the same `@in`/`@out`/`@err` contract. Substitution failures surface as a contract-violation error code before execution. |
| Experience observability (DH-7) | `ExecutionEvent` (closed taxonomy) + `RunManifest` already provide step-indexed, **content-free** events (`FieldDescriptor` carries shape, never raw text). | No new library type required: the **data-safety boundary is exactly what makes layered digests shareable**. Digestion is a host-side consumer of the event stream keyed by `run_id` + `step_index`; nodus already guarantees the stable addressing DH-7 back-pointers need. |
| Falsification ledger (DH-8) | `RunManifest.run_id` and `ExecutionEvent.step_index` give stable change addressing. | The manifest is a host-side append-only record; nodus supplies the addressing and the per-step pass/fail signal. No library coupling. |
| Genome & drift (DH-9) | The parser/AST already fully describe a workflow's structure. | A **pure function over the AST** → structural signature (step/macro counts, schema vocabulary, interceptor chain, policy blocks, contract shape). Pure, std-only, no I/O — a natural nodus-library addition that satisfies nodus LP-5. |
| Promotion gate & durable provisional state (DH-10) | `PolicyProvider` **already graduated nodus LP-3** — `l2-nodus-portability.md` §4.8.1 records the runtime tool-security guard and the plugin-hook interception as its two independent host contexts, wired since LP-11 [v1.5.0] and consumed by LP-16/LP-17. `StorageProvider` has not: §4.8.2 records only one host context, so its `store`/`load` call sites stay unwired. | The dynamic harness **consumes** the already-wired `PolicyProvider` for the promotion-gate decision — no graduation work needed there. `StorageProvider` (provisional-vs-promoted state, the append-only ledger, the genome baseline) is the one seam the harness could supply a genuine second, independent host context for — but that is a possible *consequence* of building the harness, not a justification for building it: DH-10 does not depend on `StorageProvider` graduating, since a harness may hold its provisional state host-side without ever touching this nodus seam. |

The portability discipline is preserved: every addition is an abstract interface with a built-in no-op, no host type enters the library, and the schema vocabulary baseline is untouched (nodus LP-4).

## 5. Implementation Notes

Recommended order, chosen to keep each step verifiable before the next:

1. **Interceptor seam first** (DH-2…DH-5) — define the write-capable seam, the mutable-aspect enum, and the ordered-chain composition, with a no-op built-in. Nothing run-time-dynamic is possible without it, and it is the riskiest abstraction (it must not be confusable with the observer).
2. **Trace digestion** (DH-7) — build the host-side layered digester over the existing event stream; verify every digest claim carries a back-pointer.
3. **Falsification ledger** (DH-8) — emit manifests with machine-checkable predictions; wire the next-evaluation verdict computation.
4. **Genome function** (DH-9) — pure AST→signature; add distance and drift-from-baseline as local-only comparisons.
5. **Promotion gate + durable state** (DH-10) — consume the already-wired `PolicyProvider`; gate provisional changes before they become durable. Optionally graduate `StorageProvider` if provisional state is held nodus-side rather than host-side.
6. **Runtime substitution** (DH-6) and **verification independence** (DH-11) — last, because they depend on more than one runtime being wired and on the contract being stable.

## 6. Drawbacks & Alternatives

**Drawbacks (honest limits):**

- **Online memory is token-hazardous at scale** — per-task accumulation grows unbounded; beyond modest task volumes it needs retrieval compression or embedding-based selection, or it becomes economically unviable. DH-10's gate bounds *quality*, not *size*.
- **Complexity does not always help** — empirically, the simplest adaptive strategy (a small memory bank) can dominate elaborate harnesses on a cost/quality frontier; added interceptors and substitution must justify their token cost, not be assumed beneficial.
- **Reduced independence with a single lineage** — if producer and verifier share a model lineage (DH-11 not satisfiable), self-verification is weaker than it appears and must be flagged.
- **Order-dependence hurts reproducibility** — online adaptation can make outcomes depend on processing order (DH-12); this bounds the HE-6 transfer claim.
- **Honor-system isolation is not isolation** — if a held-out set is merely "instructed off-limits" rather than technically inaccessible, evolution integrity is social, not enforced. Technical isolation is preferred.

**Alternatives considered:**

- **Pure offline evolution** (the HE-pattern alone) — simpler and fully reproducible, but cannot react within a run and cannot exploit per-role runtime substitution. Retained as the baseline; the dynamic pattern is additive, not a replacement.
- **Static (non-adaptive) middleware** — a fixed interceptor chain with no triggers. Predictable, but leaves run-local reliability unaddressed; it is the degenerate case of DH-5 with constant triggers.
- **Model fine-tuning** — improve the base model instead. Orthogonal and out of host control; composes with, rather than substitutes for, dynamic harnessing.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[HARNESS-ENG]` | `.design/main/specifications/l1-harness-engineering.md` | The offline sibling; DH reuses HE-3/HE-4/HE-6/HE-8 and must not relax them |
| `[NODUS-OBS]` | `crates/nodus/src/observability.rs` | `AuditProvider` observer-neutrality + `ExecutionEvent`/`FieldDescriptor` content-free taxonomy that DH-2/DH-7 build on |
| `[NODUS-PORT]` | `crates/nodus/src/portability.rs` | `PolicyProvider` (already nodus LP-3-graduated, DH-10 consumes) and `StorageProvider`'s still-open nodus LP-3 seam (DH-10 could supply its second host context for); `ModelProvider` substitution seam for DH-6 |
| `[NODUS-PORT-SPEC]` | `.design/nodus/specifications/l1-nodus-portability.md` | LP-1…LP-7 contract every §4.7 extension must satisfy |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.1 | 2026-09-24 | Core Team | Consistency pass (2026-09-24): Bare `LP-n` citations of the nodus portability contract are now written `nodus LP-n`: this workspace's `l1-lookahead-planning` defines LP-1…LP-6 as well, so the bare form pointed a reader at the wrong invariant. No requirement changed. |
| 1.0.0 | 2026-08-13 | Core Team | RFC→Stable — the adversarial verification the 0.1.0 entry flagged as pending, completed. Post-Update Review (spec-critic) verified every `Related Specifications` and `Canonical References` link resolves and every cited spec is genuinely Stable, then found and fixed two stale/overclaimed technical premises rather than accepting the draft's own framing at face value: (1) §4.7's DH-10 row asserted `PolicyProvider`/`StorageProvider` were *both* "pending LP-3, awaiting a second independent host" and that this harness would be *the* second host graduating both — checked against the current `l2-nodus-portability.md` (v1.7.1, Stable) and found stale: `PolicyProvider` **already graduated** independently (§4.8.1 — the runtime tool-security guard and the plugin-hook interception are its two host contexts, wired since LP-11 [v1.5.0] and consumed by LP-16/LP-17), while `StorageProvider` genuinely remains single-context (§4.8.2, unwired). Corrected §4.7's DH-10 row, §5's Implementation Notes step 5, and the `[NODUS-PORT]` Canonical Reference to state the harness *consumes* the already-wired `PolicyProvider` and only *could* supply `StorageProvider`'s missing second context as a possible consequence, never a justification, of being built. (2) The `l2-self-improvement.md` Related-Specifications entry claimed it as "an existing L2 realization of DH-8/DH-10" — read in full and found non-conformant: its calibration/mistake/template writes are unconditionally additive with no promotion-gate-with-margin step and no machine-checkable predicted-flip scoring, so it is related by theme (learning from live operation) but does not realize either invariant; reworded to state that honestly rather than implying DH-8/DH-10 are already covered by extension. No change to the DH-1…DH-12 invariants themselves — both findings were in the illustrative nodus-integration/related-specs material, not the contract. Version bumped straight to 1.0.0 on first Stable promotion per project convention (`l1-dev-office` 0.1.0→1.0.0, `l1-loop-governance` 0.2.0→1.0.0 precedent), folding the corrections into the same promotion event rather than a separate patch. |
| 0.1.0 | 2026-06-25 | Core Team | Initial RFC — DH-1…DH-12; two-mode model (runtime adaptation / runtime substitution); interceptor chain taxonomy; layered sourced experience observability; falsification ledger with machine-checkable predicted flips; structural genome + advisory drift; promotion-gated online adaptation; nodus integration via write-capable interceptor seam, runtime-substitution abstraction, AST genome function, and LP-3 graduation of PolicyProvider/StorageProvider. Adversarial verification of invariants pending (research pass interrupted by quota) |
