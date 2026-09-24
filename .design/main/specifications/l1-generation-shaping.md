# Generation Shaping

**Version:** 1.0.1
**Status:** Stable
**Layer:** concept

## Overview

The output-side member of the token economy concerned with the *character and effort* of what the model generates — not how long it is allowed to run. Where the generation budget governs output **length** (minimal reservation, escalate-on-truncation, continue-from-partial), generation shaping governs output **cost per turn** through two levers keyed on a classification of the turn: **verbosity steering** (suppress ceremony, preamble, and restated context so the model writes the answer, not the packaging) and **effort modulation** (dial the model's reasoning depth down on routine turns and keep it full on turns that need it). Both levers are gated by one non-negotiable floor — shaping never trades correctness for brevity or speed — and because their effect is inherently unobservable (the system never sees what the model *would* have written), any saving they claim is an honest estimate with a confidence interval, or a measured number from a deliberately-held-out control group, never a fabricated figure.

## Related Specifications

- [l1-generation-budget.md](l1-generation-budget.md) - The sibling output-economy axis: budget governs *length/truncation recovery*, shaping governs *character/effort per turn*. Orthogonal and composable — a turn can be both length-budgeted and shaped (GS-1 vs GB-7).
- [l1-cache-stable-context.md](l1-cache-stable-context.md) - The verbosity-steering directive is placed at a cache-stable position so injecting it does not bust the provider KV cache (GS-5 composes CSC-2's frozen-prefix discipline).
- [l1-inner-monologue.md](l1-inner-monologue.md) - The private reasoning channel whose *depth* effort modulation tunes; shaping decides how much of it a turn warrants, it does not change what the channel is.
- [l1-deliberation.md](l1-deliberation.md) - The complementary direction on the same effort knob: deliberation raises effort on hard/ambiguous decisions; shaping lowers it on routine turns. Both classify the turn; neither may lower effort where correctness needs it.
- [l1-user-model.md](l1-user-model.md) - The source of a *learned* verbosity level: a terseness preference inferred from implicit behavioral signals (UM-2, provenance-tracked), with an explicit user statement overriding it (UM-4 → GS-8).
- [l1-solution-frugality.md](l1-solution-frugality.md) - FR-10 measured-not-asserted / honest-counterfactual is the general discipline GS-6 applies to the specifically-unobservable output-saving case.
- [l1-operational-health.md](l1-operational-health.md) - Consumes the accounted shaping signal (GS-9) as a cost/quality indicator alongside the budget and compression signals.

## 1. Motivation

Everything on the input side — selection, summarization, compression — shrinks the prompt the agent *sends*. But an agent also pays for every token the model *writes back*, and on the most capable models output is several times more expensive per token than input. A large fraction of that output is waste that costs money and latency without adding information:

- **Ceremony and restatement.** "Great, let me help with that…" preambles, re-printing a file the agent just showed the model, narrating the plan before doing it, and closing summaries the user will not read. This is packaging, not answer.
- **Reasoning spent where none is needed.** A turn that is merely the model resuming after a routine tool result — a file read that succeeded, a test that passed, a `cd` that worked — rarely needs deep deliberation. Spending full reasoning effort on it burns the most expensive tokens on the least demanding step.

The generation budget already handles output *length*: it reserves little and escalates on truncation. But length is not character: a response can be well within budget and still be half ceremony, and a routine turn can be short yet still trigger a full, expensive reasoning pass. Shaping is the distinct lever that addresses *what kind* of output a turn should produce, and *how hard the model should think* to produce it.

Two things make shaping dangerous if done naively, and they are exactly what the invariants below constrain. First, terseness and reduced effort must **never** cost correctness — a turn that is actually hard, ambiguous, or an error must get full treatment regardless of any global "be terse" setting. Second, the saving is **counterfactual**: the system never observes the un-shaped response, so it cannot simply subtract "after" from "before." A shaping layer that reports a confident exact saving is fabricating it. Honest measurement — an estimate with a stated confidence range, or a measured number from a held-out control group — is part of the contract, not a nicety.

## 2. Constraints & Assumptions

- The concrete verbosity levels, the effort tiers, and the turn-classification rules are tuning parameters; this concept constrains the *behavior* (turn-classified, correctness-floored, cache-stable, counterfactually-measured, opt-in, accounted), not the specific thresholds.
- Shaping acts on the *generation request* (a steering directive in the prompt, an effort/reasoning parameter on the model call); it does not post-edit or truncate the model's produced output — trimming a produced answer is a different, lossy act this concept does not perform.
- Shaping composes with, and is independent of, the length budget (GB-*) and the input economy — relieving one does not relieve the others.
- Turn classification is a heuristic over observable turn context (is this a resume-after-tool-result? a fresh user question? an error?); it is allowed to be imperfect *because* the correctness floor (GS-4) makes a misclassification degrade cost, never correctness.

## 3. Core Invariants

Rules every Layer 2 implementation MUST NOT violate:

- **GS-1 (Turn-classified modulation):** shaping decisions are made per turn, from a classification of what the turn *is* (e.g. a routine resume after a successful tool result, a fresh question, an error, an ambiguous or high-stakes request). A single global setting that shapes every turn identically, blind to turn type, is forbidden — the whole safety of the mechanism rests on treating a routine turn and a hard turn differently.
- **GS-2 (Verbosity steering, not output editing):** the agent MAY steer the model toward terse, answer-first output — suppressing preamble, self-narration, and restatement of context already visible — by instructing the *generation*, never by post-hoc trimming of what was produced. Steering shapes what the model chooses to write; it does not delete tokens after the fact (which would be lossy output editing, out of scope).
- **GS-3 (Effort modulation, floored):** the agent MAY lower the model's reasoning/thinking depth on turns classified as routine and MUST keep it at full depth on turns classified as new, erroneous, ambiguous, or high-stakes. Effort is only ever lowered *toward* a floor for demonstrably-routine work; it is never lowered on a turn whose correctness depends on reasoning.
- **GS-4 (Correctness supremacy):** shaping MUST NOT trade correctness, completeness of a required answer, or safety for brevity or speed. On any signal that a turn is hard, failing, ambiguous, or safety-relevant, shaping steps aside and the turn runs unshaped. Brevity and low effort are conveniences; a correct, complete, safe answer is not negotiable — this floor outranks every other invariant here.
- **GS-5 (Cache-stable placement):** a persistent steering directive is injected at a position that does not destabilize the provider's cacheable prefix — appended at a stable seam (e.g. the tail of the system prompt) rather than spliced into frozen leading content — so shaping never busts the KV cache it is trying to make cheaper (composes `l1-cache-stable-context.md` CSC-2). A shaping change that would invalidate a large cached prefix is itself a cost regression and is disallowed.
- **GS-6 (Counterfactual measurement honesty):** because the un-shaped output is never observed, a reported output-token saving MUST be either (a) an explicit *estimate* carrying a confidence range and labelled as estimated, or (b) a *measured* number derived from a deliberately-held-out control group (a declared fraction of turns left unshaped for comparison). Reporting a shaped saving as a confident exact figure is forbidden — it fabricates a counterfactual the system cannot have. (The specific application of the general honest-counterfactual discipline, `l1-solution-frugality.md` FR-10.)
- **GS-7 (Opt-in, reversible, live-reconfigurable):** shaping is off by default and explicitly enabled; it is fully reversible (disabling it restores unshaped behavior with no residue); and its settings are read live per request, so enabling, disabling, or retuning it takes effect without discarding a warm process or a warm cache. A shaping layer that can only be changed by a cold restart (dropping caches and in-flight work) violates this.
- **GS-8 (Learned level is overridable):** the verbosity/effort *level* MAY be inferred from implicit behavioral signals (a user who consistently interrupts long replies, or moves on before a long answer could be read, is showing a terseness preference) rather than only set statically — but any inferred level is provisional and is overridden by an explicit user instruction (via the user model, UM-2/UM-4). Inference sets a default; the user's stated preference always wins.
- **GS-9 (Accounted & observable):** which turns were shaped, the verbosity/effort decision taken, and the estimated-or-measured output saving are recorded as a distinct signal — separate from the input-compression and length-budget signals — so the effect is auditable, tunable, and legible to the user, and so a shaping regression (e.g. terseness that started dropping needed content) is detectable rather than silent.

> L2 specs cannot reach RFC status until all invariants here are addressed in their "Invariant Compliance" section.

## 4. Detailed Design

### 4.1 The output-economy axes

Generation shaping is the third distinct output-side concern, orthogonal to the other two:

| Concern | Governs | Members | Owner |
| --- | --- | --- | --- |
| Output budget | how *long* the model may generate | minimal reservation, escalate-on-truncation, continue-from-partial | l1-generation-budget |
| **Output shaping** | the *character & effort* of a turn's generation | verbosity steering, effort modulation | **this spec** |
| Output contract | the *shape/validity* of the produced result | schema, size-bounding, validation retry | l1-output-contracts |

A single generation can be length-budgeted, shaped, and contract-validated independently. Shaping reduces the tokens produced and the reasoning spent; budgeting caps and recovers length; the contract governs form.

### 4.2 Turn classification

Shaping keys every decision on a cheap classification of the turn (GS-1). A representative, non-exhaustive taxonomy:

| Turn class | Typical signal | Verbosity (GS-2) | Effort (GS-3) |
| --- | --- | --- | --- |
| Routine resume-after-tool | prior step was a tool result that succeeded (file read, passing test, successful command) | terse | lowered toward floor |
| Continuation of an in-progress mechanical task | mid-sequence step with a clear next action | terse | moderate |
| Fresh user question | a new request opening a turn | default | full |
| Error / failure recovery | prior step failed, an exception surfaced, a validator rejected | default (never suppress diagnostics) | full |
| Ambiguous / high-stakes | under-specified request, destructive or irreversible action, safety-relevant topic | default | full |

The classifier is a heuristic and is *allowed* to be wrong, because GS-4 makes a wrong guess a cost inefficiency, never a correctness failure: misclassifying a hard turn as routine that then produces a wrong answer is a bug in the classifier's *cost model*, and the correctness floor is what keeps that bug from becoming a wrong answer — anything that looks hard, failing, or ambiguous escalates to full treatment regardless of the initial class.

### 4.3 The two levers

```text
[REFERENCE]
shape(turn):
    class := classify(turn)                       // GS-1 cheap turn classification
    if class in {ERROR, AMBIGUOUS, HIGH_STAKES, FRESH_QUESTION}:
        return unshaped(turn)                       // GS-4 correctness floor — step aside

    req := turn.request
    if verbosity_enabled:
        req := req.with_steering(TERSE_DIRECTIVE)   // GS-2 appended at a cache-stable seam (GS-5)
    if effort_enabled and class in {ROUTINE_RESUME, MECHANICAL_CONTINUATION}:
        req := req.with_effort(lower_toward_floor)  // GS-3 reasoning depth down for routine work
    account(class, decisions, estimate_saving)      // GS-9
    return req
```

The verbosity directive is *content the model reads* (a short "answer first, don't restate context, skip preamble" note); the effort setting is a *parameter on the model call* (a lower reasoning/thinking tier). Neither touches the produced output — shaping is entirely a property of the *request*, which is what keeps it lossless with respect to what the model decides to say (GS-2) and reversible (GS-7).

### 4.4 Cache-stable steering placement

A steering directive that changes per turn, or that is spliced into the frozen leading content, would defeat itself: it would invalidate the cached prefix and cost more than it saves. GS-5 requires the persistent directive to sit at a stable seam — appended to the tail of the system prompt, after the frozen content — so the large cacheable prefix stays byte-identical and only the small trailing note varies (and, ideally, stays constant across turns so even it caches). This is the same frozen-prefix / volatile-suffix discipline the cache-stability contract defines, applied to the shaping directive.

### 4.5 Honest counterfactual measurement

The system never sees the response the model *would* have produced without shaping, so a naive "before − after" is not available for output savings. GS-6 permits exactly two honest reports:

```text
[REFERENCE]
// (a) estimate — default: model the saving, never assert it
report := { reduction: 31.7%, ci: [27.7%, 35.7%], basis: "estimated" }

// (b) measured — opt-in holdout control group
//     leave a declared fraction of turns UNSHAPED; compare shaped vs held-out
//     realized output-token counts on comparable turns
report := { reduction: 29.4%, basis: "measured", holdout: 0.1 }
```

The estimate is the default because it needs no sacrifice of savings; the holdout upgrades to a measured number at the cost of leaving some turns unshaped as a control. Either way the number carries its basis label (GS-9), so a downstream dashboard or health signal never presents a modeled figure as if it were observed ground truth — the honest-counterfactual rule (`l1-solution-frugality.md` FR-10) applied to the one place where the counterfactual is structurally unobservable.

## 5. Drawbacks & Alternatives

- **Classifier error costs savings, not correctness.** A mis-tagged turn either wastes the saving (a routine turn treated as hard) or is caught by the floor (a hard turn treated as routine escalates the moment it shows difficulty). The design deliberately makes the failure mode *cost*, never *wrongness* (GS-4) — but a poor classifier leaves savings on the table, so GS-9 accounting exists to tune it.
- **Terseness can drop something the user wanted.** Aggressive verbosity steering risks omitting a caveat or context a user valued. Mitigated by GS-4 (never suppress diagnostics or needed content), GS-8 (the level is learnable and user-overridable), and GS-9 (a terseness regression is observable, not silent). The safe default is off (GS-7).
- **Estimated savings are not measured savings.** GS-6's default is an estimate, which some will distrust. That is the honest state of a counterfactual quantity; the holdout path (GS-6b) is offered for anyone who wants a measured number and will pay for it with a control group. Fabricating an exact figure to look precise is the disallowed alternative.
- **Alternative — fold into the generation budget.** Rejected: the budget governs *length and truncation recovery*; shaping governs *character and reasoning effort per turn* (GS-1 vs GB-7). They are orthogonal — a turn can be terse yet still truncate, or verbose yet short — so folding shaping into the budget would under-specify both, exactly as compression earned its own concept beside selection/summarization.
- **Alternative — post-trim the produced output.** Rejected: deleting tokens after generation is lossy output editing (it can cut a half-finished sentence or a needed caveat) and it does not save the reasoning tokens at all. Shaping the *request* saves both the ceremony and the reasoning, losslessly, because the model never generates the waste in the first place (GS-2).
- **Alternative — always shape everything.** Rejected: a global unconditional "be terse, think less" setting is precisely the unsafe design GS-1/GS-4 forbid — it lowers effort on the hard turns where it matters most.

## nodus-relevance mapping

A workflow step that invokes a model (a `GEN`/`REFINE` step) has the same shaping opportunity at the step grain — but it needs **no new language invariant**, because the seams already exist and shaping is a host concern (nodus LP-1/LP-2):

| Element | nodus seam | Note |
| --- | --- | --- |
| Verbosity steering (GS-2) | host-composed prompt, volatile suffix (NL-15) | The terse directive is host-supplied trailing content; it belongs in the volatile suffix so it never destabilizes the byte-stable prefix (NL-15) — which *is* GS-5 seen from inside the DSL. |
| Effort modulation (GS-3) | `ModelProvider` call options per step | Reasoning depth is a call parameter the host sets; a routine step (a `~FOR` mechanical iteration, a resume after a tool result) may carry a lower tier, the host deciding — no effort vocabulary enters core. |
| Turn classification (GS-1) | host-side, over the run's step context | Which step is "routine" is a host judgement over the observable step context; core neither classifies nor names a turn class. |
| Correctness floor (GS-4) | `@err:` typed handling + `!!` invariants unshaped | An error step or an invariant-bearing step runs at full treatment; shaping never applies to a step whose correctness the workflow's own `!!` rules assert. |
| Counterfactual accounting (GS-6/GS-9) | `AuditProvider` cost classes (HO-8) | Output-token counts and the estimated/measured basis ride the existing observability cost-attribution channel; no new event type. |

The nodus workspace owns any realization; this records the mapping. The workflow side is fully covered by NL-15 (cache-stable composition), the `ModelProvider` option seam, `@err:`/`!!` correctness handling, and HO-8 cost classes — so no new NL invariant is warranted (additive; a host that shapes nothing behaves exactly as today).

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[BUDGET]` | `.design/main/specifications/l1-generation-budget.md` | The sibling output-economy axis (length/truncation) this concept is orthogonal to. |
| `[CACHE-STABLE]` | `.design/main/specifications/l1-cache-stable-context.md` | CSC-2 frozen-prefix discipline the steering-directive placement (GS-5) composes. |
| `[USER-MODEL]` | `.design/main/specifications/l1-user-model.md` | UM-2/UM-4 inferred-and-overridable preference the learned verbosity level (GS-8) draws from. |
| `[FRUGALITY]` | `.design/main/specifications/l1-solution-frugality.md` | FR-10 honest-counterfactual discipline GS-6 applies to the unobservable output-saving case. |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.1 | 2026-09-24 | Core Team | Consistency pass (2026-09-24): Bare `LP-n` citations of the nodus portability contract are now written `nodus LP-n`: this workspace's `l1-lookahead-planning` defines LP-1…LP-6 as well, so the bare form pointed a reader at the wrong invariant. No requirement changed. |
| 1.0.0 | 2026-07-13 | Core Team | Initial spec — generation shaping as the third output-economy concern beside the length budget and the output contract: turn-classified modulation (GS-1), verbosity steering as request-instruction never post-trim (GS-2), floored effort/reasoning-depth modulation — down on routine, full on new/error/ambiguous (GS-3), correctness supremacy as the load-bearing floor (GS-4), cache-stable steering placement (GS-5), counterfactual measurement honesty — estimate+CI or held-out control group, never a fabricated exact figure (GS-6), opt-in/reversible/live-reconfigurable (GS-7), learned-and-overridable level from implicit behavioral signals (GS-8), accounted & observable as a distinct signal (GS-9); nodus-relevance mapping needing no new NL invariant (host-side over NL-15 + ModelProvider options + @err/!! + HO-8). Distilled from an adoption pass over an external agent context-optimization reference whose input-side mechanics (content-routed reversible compression, cache-aligned live-zone compression, cross-agent memory, failure-mining) were already realized by l1-context-compression (CC-1…11 + §4.5 CCR), l1-cache-stable-context, l1-inference-cache, the memory cluster, and l2-learning-loop — GS captures the one genuine delta, the output-side character/effort modulation none of those covered. |
