# Context Degradation & Health

**Version:** 1.0.1
**Status:** Stable
**Layer:** concept

## Overview

The failure-model that the context-economy family exists to prevent — and the discipline
that keeps a long, tool-heavy context *healthy* rather than merely small. As a working
context grows, model quality erodes in predictable, silent ways well before any hard
token limit: information in the low-attention middle is under-recalled (lost-in-the-middle),
an honestly-wrong fragment contaminates everything that references it (poisoning),
irrelevant bulk drowns the signal (distraction), mixed tasks blur (confusion), and
contradictory content yields inconsistent reasoning (clash). None of these throws an
error; the only symptom is worse answers. This concept names those degradation modes,
makes context assembly *position-aware* (load-bearing content in the high-attention
head/tail, never buried mid-context), makes non-adversarial poisoning a first-class
recoverable failure (excise to a verified state, do not correct-in-place), and routes
each mode to the *matched* mitigation from the existing toolkit — never one blunt
"shrink it." It is the phenomenology side of the context economy: the compression,
disclosure, cache, and memory specs are the *responses*; this is the *contract for the
degradation they respond to*.

## Related Specifications

- [l2-context-management.md](l2-context-management.md) — owns the selection (trim) and summarization (compaction) *mechanisms*; this spec owns the degradation model that decides *when and why* to invoke them, plus the placement (CD-2) and poisoning-recovery (CD-4) they do not cover.
- [l1-context-compression.md](l1-context-compression.md) — the *Compress* response; CD-6 routes volume-driven degradation to it, and CC-10 memory-safe reduction is the *Write* backstop CD-4 excision relies on.
- [l1-progressive-disclosure.md](l1-progressive-disclosure.md) — the *Select/defer* response; PD-5 anti-rot is the matched mitigation for distraction and confusion (CD-6).
- [l1-cache-stable-context.md](l1-cache-stable-context.md) — the frozen-prefix head and the live-zone tail *are* the high-attention zones CD-2 places load-bearing content into; salience placement and cache stability are the same seam seen from two sides.
- [l1-orchestration.md](l1-orchestration.md) — ORC-5 isolation is the *Isolate* response: partitioning mixing/parallel work across sub-agents bounds each context's degradation (CD-6).
- [l1-memory-intelligence.md](l1-memory-intelligence.md) — MI-4 memory-conflict adjudication is the memory-side counterpart of CD-7 working-context clash reconciliation; and the memory store is where verified content survives a CD-4 excision.
- [l1-context-provenance.md](l1-context-provenance.md) — demarcation: provenance defends the trust boundary against *adversarial* instruction-poisoning structurally (CP-1/CP-2); CD-3/CD-4 recover from *non-adversarial* poisoning — content that is simply wrong, not malicious. Complementary, non-overlapping.
- [l1-claim-verification.md](l1-claim-verification.md) — grounds a single produced output; CD detects and recovers when the *context the agent reasons over* has become contaminated, upstream of any one output.
- [l1-operational-health.md](l1-operational-health.md) — OH-5 tracks length as a context-pressure signal; CD-1/CD-5 feed a richer *degradation-risk* signal (behavioral, not size-only).
- [l1-optimization-integrity.md](l1-optimization-integrity.md) — sibling silent-failure guard: OI guards silent degradation of *optimizations*, CD guards silent degradation of the *context content itself*; both catch a failure that produces valid-looking output.

## 1. Motivation

A long-running agent accumulates context, and more context is not more capability — past
a point it is *less*. The degradation is silent: the model still answers, still runs, still
passes functional checks; it just answers a little worse, and then worse, until a wrong
result ships. Three facts make this a first-class concern rather than a tuning detail:

- **Attention is positionally non-uniform.** A model attends most strongly to the start
  and the most-recent tail of its context and least to the middle. Load-bearing content
  buried in the middle is under-recalled — so *where* content sits in the assembled
  context is a correctness property, not incidental ordering. Nothing in the system
  states this today.
- **Honest errors poison, not just attacks.** The security specs defend the trust
  boundary against *malicious* instructions hidden in content. But a factually-wrong tool
  result, a stale retrieved doc, or a model's own hallucinated summary enters through an
  *honest* path and then contaminates every downstream step that references it —
  persisting even after an explicit correction, because the poisoned references remain.
  This is a distinct failure the injection defenses do not address.
- **The same size degrades differently.** Degradation onset depends on content and model,
  not token count alone, and it is a continuum, not a cliff. Acting only at the hard
  context limit means the agent has already been operating degraded for a long time.

The mitigations already exist and are well-specified — compress, select/disclose,
write-externally, isolate, cache. What is missing is the model that says *what is going
wrong*, *how to see it*, *where to place what matters*, and *how to recover a poisoned
context* — so the right mitigation fires, at the right time, for the right reason,
instead of a reflexive "summarize it" that treats volume when the problem is poison or
clash.

## 2. Constraints & Assumptions

- This concept is the *degradation model and health discipline*; it owns no reduction mechanism. Trimming, summarizing, disclosing, isolating, and caching are owned by their specs — this decides when they fire and adds the two things none of them cover: salience placement and poisoning recovery.
- Attention non-uniformity (strong head/tail, weak middle) is treated as a model-agnostic property; the concrete degradation thresholds per model are tuning data, not part of this contract.
- Poisoning here means *non-adversarial* contamination (wrong / stale / hallucinated content on an honest path). Adversarial instruction-poisoning is out of scope — it is defended structurally by context-provenance and component-scanning.
- Detection is behavioral and best-effort; it surfaces risk to the health layer and never blocks a turn on a false positive. The correctness floor is that a *detected* poisoning is recovered by excision, not that every poisoning is detected.

## 3. Core Invariants

Rules every Layer 2 realization MUST NOT violate. They are technology-neutral.

- **CD-1 (Degradation is a continuum, managed on risk not on the limit):** context quality erodes progressively as the working set grows, well before any hard token limit. Mitigation is triggered against a *degradation-risk signal* that rises with utilization and with the taxonomy indicators (CD-5), not against the token limit alone. A design that acts only at the limit has already been operating degraded.

- **CD-2 (Positional salience — load-bearing content in high-attention zones):** attention over a long context is positionally non-uniform — strongest at the head and the most-recent tail, weakest in the middle. Context assembly places load-bearing content — the current task, its hard requirements, the findings it must act on — in the high-attention head/tail zones, never buried in the low-attention middle. Placement is a deliberate assembly decision, not incidental ordering. (The frozen-prefix head and live-zone tail of `l1-cache-stable-context` are exactly those zones.)

- **CD-3 (Non-adversarial poisoning is a distinct failure mode):** a factually-wrong, outdated, or hallucinated fragment that enters context through an honest path — an erroneous tool result, a stale retrieved document, a model-generated summary that hallucinated — can contaminate all downstream reasoning that references it, persisting even after an explicit correction. This MUST be recognized as its own failure, distinct from adversarial instruction-poisoning (defended at the trust boundary by `l1-context-provenance`): the content is not malicious, it is wrong, and the defense is reasoning hygiene, not a boundary check. Assuming the injection defenses cover it is a coverage gap.

- **CD-4 (Poisoning recovery is excision, not more correction):** once context poisoning (CD-3) is detected, recovery excises the contaminated fragment *and everything derived from it* back to a verified state and continues from there — it does NOT try to repair a poisoned context by appending further corrections, which the surviving poisoned references defeat (the contamination compounds). Only verified or re-grounded content is carried forward; durable knowledge that must survive the excision is the memory subsystem's (CC-10), not the discarded span's.

- **CD-5 (Detection is behavioral, not size-only):** the onset of degradation is detected from *behavioral* signals — a quality drop on tasks the agent previously handled, tool/parameter misalignment, persistent error despite correction, contradictory or inconsistent outputs — because the same token count degrades differently by content and model. These signals feed the health surface so degradation is caught while still recoverable, not after a wrong result ships. (Kin to `l1-optimization-integrity` OI-1: a silent failure needs a signal that is not the size counter.)

- **CD-6 (Mitigation is taxonomy-matched, never one blunt reduction):** each degradation mode is answered by the *matched* response from the existing toolkit — *write* externalizes what must persist, *select/disclose* removes distraction and confusion, *compress* relieves volume, *isolate* partitions mixing or parallel work, *excise* recovers poisoning, *reconcile* resolves clash. Applying volume-reduction (summarize/trim) to a poisoning or clash problem treats the symptom and leaves the cause; the response is chosen to fit the mode, not reflexively.

- **CD-7 (Clash is surfaced and reconciled, never silently averaged):** contradictory content co-resident in the working context — two sources disagreeing, an updated fact beside its stale version — is detected and reconciled to a single consistent basis (the newer/verified one, the superseded one dropped or explicitly marked), never left for the model to silently average into an inconsistent answer. (The working-context counterpart of the memory-conflict adjudication `l1-memory-intelligence` MI-4; versioning prevents stale-vs-current clash.)

> L2 specs cannot reach RFC status until all invariants here are addressed in their "Invariant Compliance" section.

## 4. Detailed Design

### 4.1 Degradation taxonomy → matched mitigation

| Mode | Cause | Behavioral detection (CD-5) | Matched response (CD-6) |
| --- | --- | --- | --- |
| Lost-in-middle | positional attention falloff | recall of mid-context requirements drops | **place** load-bearing content head/tail (CD-2) |
| Poisoning | wrong/stale/hallucinated fragment referenced downstream | quality drop on previously-successful work; persistent error despite correction | **excise** to verified state (CD-4) |
| Distraction | irrelevant bulk overwhelms signal | off-topic drift; a single distractor degrades output | **select / disclose** (progressive disclosure PD-5) |
| Confusion | multiple tasks mixed in one context | wrong tool/parameter; requirements bleed across tasks | **isolate** across sub-agents (ORC-5) |
| Clash | contradictory content co-resident | inconsistent / self-contradicting outputs | **reconcile** to one basis (CD-7, MI-4) |
| Volume pressure | sheer accumulated size | rising utilization toward the risk band | **compress** (context-compression) + **write** (CC-10) |

The rule is *detect the mode, apply its response* — not summarize everything and hope.

### 4.2 Positional salience in assembly (CD-2)

```text
[REFERENCE]
assemble(task, requirements, context, findings):
    return [ head:  task + hard_requirements ]      // high attention — what to do + must-honor
         + [ mid:   supporting_context ]             // low attention — details, references
         + [ tail:  key_findings + current_step ]    // high attention — what to act on now
// load-bearing content is never placed in `mid`; the head maps to the cache-stable
// frozen prefix and the tail to the live zone — the two high-attention zones (CSC-2)
```

### 4.3 Poisoning excision, not correction (CD-4)

```text
[REFERENCE]
on_poisoning_detected(context, poison_span):
    verified := context up to before(poison_span)        // CD-4 excise the poison AND its derivations
    secure_durable_knowledge(verified)                    // CC-10 — keep what must survive in memory
    return continue_from(verified)                        // resume clean; do NOT append "actually, ignore that"
// appending a correction leaves the poisoned references in place; the contamination
// persists and compounds — only removal to a verified state recovers reasoning quality
```

## 5. Drawbacks & Alternatives

- **Detection is imperfect.** Behavioral degradation signals (CD-5) can false-positive or miss. Mitigated by never blocking a turn on detection and by making the correctness floor *recovery-on-detection* (CD-4), not *detect-everything*; the health surface treats the signal as risk, not verdict.
- **Excision discards work.** CD-4 throws away the contaminated span and its derivations. Accepted: continuing from poisoned context produces confidently-wrong output that costs more to unwind, and CC-10 ensures durable knowledge is preserved in memory before the discard — the excision loses detail, not knowledge.
- **Alternative — fold into `l2-context-management`.** Rejected: context-management owns the trim/summarize *mechanisms*; it does not model *why* context degrades, does not place content by attention position, and does not recover poisoning. Those are cross-cutting (assembly, memory, orchestration, health all participate), so this is a shared contract, not one cascade's private detail — the same reason compression and cache-stability earned their own concepts.
- **Alternative — treat poisoning as an injection problem.** Rejected: injection defenses guard against *malicious* content at the boundary and would neither detect nor recover an *honest* wrong tool result already trusted and referenced. CD-3/CD-4 are a different axis (reasoning hygiene), complementary to `l1-context-provenance`.
- **Alternative — just cap context size.** Rejected: a hard cap treats volume only, ignores that the same size degrades differently by content/model, and does nothing for poisoning, clash, or mid-context burial. CD-1 manages the continuum on a risk signal, and CD-6 matches the response to the actual mode.

## nodus-relevance mapping

A workflow that composes a prompt for a `GEN`/`REFINE` step and threads intermediate
results between steps has the same degradation exposure — but it needs **no new language
invariant**, because the seams already exist (nodus LP-1/LP-2):

| Element | nodus seam | Note |
| --- | --- | --- |
| Positional salience (CD-2) | host-composed prompt over the NL-15 frozen-prefix / volatile-suffix | Load-bearing step inputs go in the byte-stable prefix (head) and the volatile suffix (tail); the host places them, core defines no attention model. |
| Poisoning excision (CD-4) | re-run the poisoned subgraph from a verified checkpoint (execution-graph / session-checkpoint), reusing the pinned-partial-re-execution seam | A bad intermediate feeding downstream steps is recovered by re-executing from the last verified step output, not by threading a correction forward. |
| Clash reconcile (CD-7) | `!!` invariants + host reconciliation of contradictory step inputs | A step asserting its own consistency rejects contradictory inputs; the host reconciles, core carries no reconciliation vocabulary. |
| Degradation risk (CD-1/CD-5) | `AuditProvider` signals over the run | Per-step context size and behavioral anomalies ride the existing observability channel; no new event type. |

The host owns any realization; core places nothing and detects nothing on its own. A host
that does none of this behaves exactly as today — so the mapping is additive and warrants
no new NL invariant.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[CTX-MGMT]` | `.design/main/specifications/l2-context-management.md` | The trim/summarize mechanisms CD-1/CD-6 trigger and complement. |
| `[CACHE-STABLE]` | `.design/main/specifications/l1-cache-stable-context.md` | The frozen-prefix/live-zone high-attention zones CD-2 places into. |
| `[PROVENANCE]` | `.design/main/specifications/l1-context-provenance.md` | The adversarial-poisoning boundary defense CD-3/CD-4 are the honest-error complement of. |
| `[MEMORY-INTEL]` | `.design/main/specifications/l1-memory-intelligence.md` | MI-4 conflict adjudication (CD-7 counterpart) and the store that survives a CD-4 excision. |
| `[OPS-HEALTH]` | `.design/main/specifications/l1-operational-health.md` | The surface CD-1/CD-5 degradation-risk signals feed. |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.1 | 2026-09-24 | Core Team | Consistency pass (2026-09-24): Bare `LP-n` citations of the nodus portability contract are now written `nodus LP-n`: this workspace's `l1-lookahead-planning` defines LP-1…LP-6 as well, so the bare form pointed a reader at the wrong invariant. No requirement changed. |
| 1.0.0 | 2026-07-15 | Core Team | Initial spec — context degradation & health as the failure-model behind the context-economy family: degradation is a continuum managed on a risk signal not the token limit (CD-1), positional salience placing load-bearing content in the high-attention head/tail never the low-attention middle composing cache-stable frozen-prefix/live-zone (CD-2), non-adversarial poisoning (wrong/stale/hallucinated honest-path content contaminating downstream reasoning) as a distinct failure from adversarial injection (CD-3), poisoning recovery by excision-to-verified-state not correction-in-place (CD-4), behavioral not size-only detection feeding operational-health (CD-5), taxonomy-matched mitigation — write/select/compress/isolate/excise/reconcile — never one blunt reduction (CD-6), working-context clash surfaced and reconciled never silently averaged as the MI-4 counterpart (CD-7); §4.1 taxonomy→matched-mitigation table, §4.2 salience assembly, §4.3 excision-not-correction; nodus-relevance mapping needing no new NL invariant (host-side salience over NL-15 + re-run-from-verified-checkpoint over the execution-graph/pinned-partial seam). Owns the degradation phenomenology the mitigation specs (l1-context-compression, l2-context-management, l1-progressive-disclosure, l1-orchestration ORC-5) each answer without a unifying model; the content-side sibling of l1-optimization-integrity (silent degradation of optimizations) and the honest-error complement of l1-context-provenance (adversarial poisoning). Distilled from an adoption pass over an external context-engineering reference whose four-bucket mitigation (write/select/compress/isolate), progressive disclosure, caching, and memory architecture were already realized across the context-economy family — CD captures the unowned delta: the degradation taxonomy, positional salience, and non-adversarial poisoning recovery. |
