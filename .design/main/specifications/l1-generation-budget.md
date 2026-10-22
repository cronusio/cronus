# Output Generation Budget

**Version:** 1.1.1
**Status:** Stable
**Layer:** concept

## Overview

The output-side member of the token economy: how many tokens the model is allowed to *generate* per request, how that allowance is reserved efficiently, and how a cut-off (truncated) generation is recovered into a complete result. Where the input-side economy (selection, summarization, compression) governs what *enters* the model context, this concept governs what *leaves* it. Its core move is "low default, escalate on truncation": reserve only the modest output allowance the common case needs, and reach the large allowance by escalation only when a response is actually cut off — then, if still cut off, continue from the partial answer rather than discarding it, and only as a last resort force the agent to decompose the oversized output. The load-bearing correctness property is that a truncated generation is never accepted, applied, or committed as if it were complete.

## Related Specifications

- [l1-context-compression.md](l1-context-compression.md) - The input-side token-economy member; this concept is its output-side symmetric counterpart (generation, not ingestion).
- [l2-context-management.md](l2-context-management.md) - Owns input selection/summarization to fit the *context* budget; this concept owns the *generation* budget and truncation recovery — a distinct axis.
- [l1-output-contracts.md](l1-output-contracts.md) - Governs output *shape/validity* and result size-bounding with validation retry; this governs output *generation length* and truncation continuation — composes, never conflated.
- [l1-model-runtime.md](l1-model-runtime.md) - Model placement/fit (MR-7); minimal output reservation is what makes constrained local placements concurrent and affordable.
- [l2-model-error-recovery.md](l2-model-error-recovery.md) - Recovers from API *errors*; a length-truncation is a successful-but-incomplete response, recovered here by escalation/continuation rather than error retry.
- [l1-agent-tool-ergonomics.md](l1-agent-tool-ergonomics.md) - ATE-5 monotonic *tool*-output budgets; this concept is the model-*generation* output budget — a different surface.
- [l1-reasoning-spend.md](l1-reasoning-spend.md) - [ADDED v1.1.0] The accounting for the share of this budget's capacity that is generated, billed, and never delivered; RSN-2 is the invariant GB-2's two-cause truncation split reads from.
- [l1-generation-shaping.md](l1-generation-shaping.md) - [ADDED v1.1.0] GSH-3's reasoning-depth dial is the remedy for the second truncation cause: where the answer never started, lowering depth frees capacity that raising the cap alone may not.
- [l1-operational-health.md](l1-operational-health.md) - Consumes the accounted default/escalation/continuation/realized-output signals (GB-8) as cost and reliability indicators.

## 1. Motivation

Every model request reserves generation capacity proportional to its declared output cap — a GPU output slot on a local/self-hosted model, or billed headroom on a hosted one. The naive default reserves for the worst case (a very long response), but the overwhelming majority of responses are short. Reserving the worst case on every call over-commits capacity several-fold, throttling concurrency and inflating cost — a direct hit to "runs profitably on any model," and especially punishing on the constrained local hardware the model-runtime must schedule onto.

The fix is to default the output allowance *low* and grow it only on demand. But growing on demand introduces a second problem the system must handle correctly: **truncation**. When the model hits the output cap mid-answer, the response is cut off — not failed, not finished, but incomplete. Three things must then be true and currently are not specified anywhere:

- **Truncation must be detected and escalated, not accepted.** A response cut off at the cap is not the answer; silently treating it as complete ships a half-written result. The allowance must escalate toward the model's real output limit and re-attempt.
- **An over-cap answer must be continued, not discarded.** Some legitimate outputs exceed even the escalated cap. Throwing away the partial answer and starting over wastes the work and may never converge; the partial must be kept and generation continued from where it stopped, bounded so it cannot loop forever.
- **A truncated artifact must never be applied.** The worst failure is a half-applied file write or edit from a truncated generation. When continuation is exhausted, the safe move is to refuse the partial artifact and force the agent to decompose the output (skeleton first, then incremental fills) — turning one un-completable generation into several completable ones.

These are the output-side analogue of the input-side token economy, and they are a distinct, cross-cutting axis: every model generation — chat turn, tool argument synthesis, workflow step, subagent run — is subject to them.

## 2. Constraints & Assumptions

- The concrete default cap, escalation ceiling, and continuation count are tuning parameters; this concept constrains the *behavior* (minimal-reserve, escalate-on-truncation, continue-from-partial, decompose-on-exhaustion, truncation-safety, accounting), not the numbers.
- A length-truncation (the generation hit the output cap) is distinct from an API error and from a natural stop; the system must be able to tell them apart.
- This axis composes with, and is independent of, the input-context budget — relieving one does not relieve the other.
- Continuation assumes the model can resume coherently from a preserved partial; where it cannot, the decomposition fallback (GB-5) is the safety net.

## 3. Core Invariants

Rules every Layer 2 implementation MUST NOT violate:

- **GB-1 (Minimal default reservation):** the per-request output allowance defaults to a modest cap sized to the common case, not the worst case, so a request does not reserve generation capacity far beyond what typical responses use. The large allowance is reached by escalation (GB-2), never reserved upfront by default.
- **GB-2 (Detect truncation, escalate — never accept as complete):** a generation cut off by the output cap (a length-truncation finish, distinct from an error or a natural stop) MUST be detected and the output allowance escalated toward the model's full output limit, then re-attempted. A truncated response is never silently treated as a finished answer.
  [MODIFIED v1.1.0] The capacity this cap governs is **not exclusively the visible answer's**. On a model that deliberates before answering, the same allowance is consumed **first** by generation the caller never receives, so a truncation has **two distinct causes with opposite remedies**: an answer *cut off mid-production* (the case above — escalate and continue from the preserved partial, GB-3), or the allowance *exhausted before the answer began* (escalate the cap or lower the reasoning depth via `l1-generation-shaping` GSH-3 — there is no partial to continue from, and attempting GB-3 here re-spends the entire budget on deliberation a second time). The two are mechanically separable: a truncation with **zero delivered output** is the second case. The accounting for the consumed-but-undelivered class, including the state where the provider does not report it at all, is `l1-reasoning-spend` (RSN-2).
- **GB-3 (Continue from partial, never discard):** when even the escalated allowance truncates, recovery preserves the partial output and continues generation from where it stopped, assembling the complete result across turns. The partial work is kept and stitched — never thrown away, never silently lost.
- **GB-4 (Bounded recovery):** escalation and continuation are both bounded — a finite continuation count and a hard output ceiling — so a pathological or non-terminating generation cannot loop or consume unbounded budget. Exhaustion advances to the fallback (GB-5); it does not retry forever.
- **GB-5 (Decomposition fallback):** when continuation is exhausted, the system MUST NOT keep retrying the monolithic generation. It falls back to guidance that forces the agent to decompose the oversized output — produce a skeleton/outline first, then fill incrementally — converting one un-completable generation into completable smaller ones.
- **GB-6 (Truncation-safe artifacts):** a truncated generation MUST NOT be applied to any durable artifact as if complete. A partial file write or edit is rejected, never half-applied; an artifact is committed only from a complete (naturally-stopped, escalated, or stitched) generation. Artifact integrity outranks producing *something*.
- **GB-7 (Distinct output-economy axis):** output-generation budgeting is a distinct axis from input-context budgeting and from the output *contract*. It governs how many tokens the model may *produce* and how truncation is recovered; it composes with — and is never conflated with — input selection/summarization/compression or result shape/validity/size-bounding.
- **GB-8 (Accounted & observable):** the default cap, each escalation, each continuation attempt, and the realized output token count are recorded, so the efficiency saving and the truncation/recovery rate are measurable and tunable, and surface as operational-health cost/reliability signals.

> L2 specs cannot reach RFC status until all invariants here are addressed in their "Invariant Compliance" section.

## 4. Detailed Design

### 4.1 The token-economy axes

| Axis | Governs | Members | Owner |
| --- | --- | --- | --- |
| Input economy | what enters the context | selection/trim, summarization/compaction, compression | l2-context-management + l1-context-compression |
| **Output economy** | what the model generates | minimal reservation, escalation, continuation, decomposition fallback | **this spec** |
| Output contract | the shape/validity of the result | schema, size-bounding, validation retry | l1-output-contracts |

The three are orthogonal: a request can be input-compressed, output-budgeted, and contract-validated independently.

### 4.2 The escalation–continuation ladder

```text
[REFERENCE]
generate(request):
    resp := call(request, out_cap = DEFAULT_LOW)          // GB-1 minimal reservation
    if not truncated(resp): return resp                    // common case, cheap

    // Layer 1 — escalate (GB-2): re-attempt at the model's real ceiling
    resp := call(request, out_cap = max(FLOOR, model.output_limit))
    if not truncated(resp): return resp

    // Layer 2 — continue from partial (GB-3, bounded GB-4)
    acc := resp.partial
    for attempt in 1..MAX_CONTINUATIONS:
        cont := call(request + acc + "resume from where you stopped", out_cap = ceiling)
        acc := stitch(acc, cont)
        if not truncated(cont): return acc

    // Layer 3 — decomposition fallback (GB-5); never commit a truncated artifact (GB-6)
    return force_decompose("output too large: produce a skeleton, then fill incrementally")
```

`truncated()` distinguishes a length-cap finish from an error (handled by model-error-recovery) and from a natural stop (done). Every branch is accounted (GB-8).

### 4.3 Truncation safety for artifacts

A truncated generation that would write or edit a file is the dangerous case (GB-6): committing it leaves a syntactically broken, half-applied artifact. The rule is strict — a tool call whose arguments or content came from a truncated generation is rejected, not partially applied; the recovery ladder (escalate → continue → decompose) runs first, and only a complete generation is allowed to mutate a durable artifact.

### 4.4 Why low-default beats high-default

Reserving the worst case on every call is simple but over-commits capacity for the >99% of small responses, starving concurrency and raising cost. Low-default + escalate pays a small extra-round-trip cost only on the rare truncation, while reclaiming the bulk of reserved capacity for everything else. The saving is real and measurable (GB-8); the rare escalation preserves output quality for the genuinely long response.

## 5. Drawbacks & Alternatives

- **Extra round-trips on truncation.** Escalation and continuation add latency to long responses. Justified: truncation is rare (the default is sized to the common case), and the alternative — reserving the worst case always — taxes every request to spare the few. GB-4 bounds the worst case.
- **Continuation coherence.** A model may not resume a partial perfectly. Mitigated by GB-5: when continuation cannot converge, the decomposition fallback turns the problem into smaller, individually-completable generations, and GB-6 guarantees nothing broken is committed in the meantime.
- **Alternative — always reserve the full output limit.** Rejected: it over-commits generation capacity several-fold for the common small response, the exact inefficiency this concept removes; it also still needs a truncation story for genuinely over-limit outputs.
- **Alternative — fold into l2-context-management or l2-model-error-recovery.** Rejected: context-management is the *input* budget (a different axis, GB-7), and error-recovery handles *failed* calls — a length-truncation is a successful-but-incomplete response with its own escalate/continue/decompose recovery. Folding it into either under-specifies the output axis (exactly as compression earned its own input-side concept).
- **Alternative — accept truncation and let the agent notice.** Rejected: it ships half-written artifacts and relies on the model to detect its own cut-off, the unreliable case; GB-2/GB-6 make detection and safety structural.

## nodus-relevance mapping

A workflow step that invokes a model has the same generation-budget concern, at the step grain.

| Element | nodus seam | Note |
| --- | --- | --- |
| Minimal default + escalation (GB-1/GB-2) | `ModelProvider` call options per step | Default low out-cap; escalate on a length-finish before failing the step. |
| Continuation from partial (GB-3/GB-4) | step retry/continuation with `~UNTIL MAX:n` bound | The partial is preserved in step state and stitched; the ceiling is the step's bound. |
| Decomposition fallback (GB-5) | step error code (`NODUS:OUTPUT_TOO_LARGE`) → planner splits the step | Converts one over-cap step into a skeleton-then-fill sequence. |
| Truncation-safe writes (GB-6) | output-contract validator gate before a write step commits | A truncated payload fails validation; nothing partial is persisted. |
| Accounting (GB-8) | `AuditProvider` event with default/escalations/continuations/realized | Output-economy telemetry on the step event stream. |

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[COMPRESSION]` | `.design/main/specifications/l1-context-compression.md` | The input-side token-economy member this concept mirrors on the output side. |
| `[CONTEXT]` | `.design/main/specifications/l2-context-management.md` | Input-budget owner; distinct axis from the generation budget (GB-7). |
| `[OUTPUT]` | `.design/main/specifications/l1-output-contracts.md` | Output shape/validity/size-bounding the truncation-safety gate composes with (GB-6). |
| `[MODEL-RT]` | `.design/main/specifications/l1-model-runtime.md` | Placement/fit that minimal reservation (GB-1) makes concurrent and affordable. |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.1.1 | 2026-10-22 | Core Team | Citations updated for the requirement-ID prefix renames that make each prefix name one specification (RULES §1, 1.8.0, the person's decision of 2026-10-22): `GS-n` of `l1-generation-shaping` is now `GSH-n`. No requirement was added, removed or reworded. |
| 1.1.0 | 2026-08-06 | Core Team | GB-2 amended — the capped capacity is **not exclusively the visible answer's**. On a deliberating model the same allowance is consumed first by generation the caller never receives, so a truncation has **two causes with opposite remedies**: an answer cut off mid-production (escalate, then GB-3 continue from the preserved partial) or the allowance **exhausted before the answer began** (escalate the cap or lower reasoning depth per GSH-3 — there is no partial to continue from, and running GB-3 here re-spends the whole budget on deliberation a second time). The two are mechanically separable: a truncation with **zero delivered output** is the second case. Accounting for the consumed-but-undelivered class, including the provider-does-not-report state, moves to the new `l1-reasoning-spend` (RSN-2). Additive diagnostic refinement; GB-1/GB-3…GB-8 unchanged. |
| 1.0.0 | 2026-06-26 | Core Team | Initial spec — output-side token economy: minimal default output reservation (GB-1), detect-and-escalate on length-truncation never accepting it as complete (GB-2), continue-from-partial never discarding (GB-3), bounded escalation/continuation (GB-4), decomposition fallback on exhaustion (GB-5), truncation-safe artifacts never half-applied (GB-6), distinct from input economy and output contract (GB-7), accounted/observable (GB-8); the symmetric output-side counterpart to context-compression; nodus-relevance mapping. |
