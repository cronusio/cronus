# Optimization Integrity

**Version:** 1.0.1
**Status:** Stable
**Layer:** concept

## Overview

The verification discipline for **silently-degradable optimizations** — the class of
performance and cost transforms (context compression, cache-prefix stability, output
shaping, ranked recall, inference-cache warmth) whose breach produces *no error and
valid-looking output*, degrading only the bill, the latency, or the answer quality.
Because such a failure trips no exception and passes every functional test — a busted
cache still returns correct answers, at ten times the input cost — it is invisible
until an invoice audit or a quality complaint. This concept states the contract that
makes the invisible failure visible: an optimization that can fail silently MUST carry
an explicit integrity check that (a) proves it still preserves outcomes, (b) proves its
claimed saving is real and honestly measured, (c) detects a live violation the moment
it happens rather than at audit time, and (d) guards its own verification layer against
cost blow-ups and forged signals. It is the **verify-it-didn't-quietly-rot** layer,
orthogonal to the transforms it watches — `l1-context-compression` decides *what to
shrink*, `l1-cache-stable-context` decides *where mutation is allowed*, this decides
*how we know either is still working*.

## Related Specifications

- [l1-operational-health.md](l1-operational-health.md) — the measurement/alerting surface these integrity signals feed; operational health scores the *trend* of the runtime, this asserts the *invariant-level integrity* of specific silently-degradable optimizations. OI-7 hands remediation onward exactly as OH-7 does.
- [l1-cache-stable-context.md](l1-cache-stable-context.md) — the canonical silent-failure case (CSC: "correct answers at 10× cost, nothing throws, the only symptom is the bill"). CSC-4 byte-faithful passthrough is the contract OI-2 realizes as a *runtime* violation sensor; CSC-11 cache-cost observability is the per-surface instance of OI-4.
- [l1-context-compression.md](l1-context-compression.md) — CC-2 fidelity bound and CC-6 accounting are the compression-surface instances of OI-3 (correctness) and OI-4 (efficacy); this spec generalizes them into a cross-cutting discipline.
- [l1-generation-shaping.md](l1-generation-shaping.md) — GS-6 counterfactual measurement honesty (estimate+CI or held-out control) is the output-side instance of OI-4; OI-4 states it as the general rule for any counterfactual saving.
- [l1-retrieval-evaluation.md](l1-retrieval-evaluation.md) — RE-6 regression gate ("a silent recall regression must not ship") is the ranked-recall instance of OI-5; OI-5 generalizes it to guard the *tail* of any distribution-shaped quality/economy metric, not just a central metric.
- [l1-inference-cache.md](l1-inference-cache.md) — IC-7 observability and its "silent warmth regression" risk are the storage-cache instance of OI-1/OI-4.
- [l1-model-benchmarking.md](l1-model-benchmarking.md) — MB-3 three-dimensional scorecard and MB-9 honest failure accounting are precedent disciplines OI-3/OI-4 reuse; benchmarking measures *model fitness*, this measures *optimization integrity* — different subject, shared honesty rules.
- [l1-log-legibility.md](l1-log-legibility.md) — LL-6 stable attribution is the substrate OI-6 constrains: an integrity signal's dimensions are a closed, code-fixed vocabulary.
- [l1-context-provenance.md](l1-context-provenance.md) — CP-1/CP-2 untrusted-by-default neutralization is why OI-6 forbids untrusted input from ever becoming an observation dimension.
- [l1-tool-receipts.md](l1-tool-receipts.md) — TR-4 absence-is-signal is the kinship OI-8 draws on: an optimization whose integrity check did not run is *unverified*, never implicitly trusted.

## 1. Motivation

Every optimization in the token economy shares a dangerous property: **when it breaks,
nothing breaks.** A cache-prefix mutation drops the hit rate to zero and the answers
stay correct — the bill silently decuples. A compression transform that over-drops a
field still returns a plausible answer — just a subtly wrong one. A recall tweak that
sinks the right episode by two ranks surfaces *something* — just not the best thing. An
output-shaping directive that started suppressing needed caveats produces shorter,
confident, incomplete answers. None of these throws. None fails a functional test,
because the output is well-formed. The only witnesses are the invoice, the latency
graph, and the slow erosion of answer quality — all of which arrive far too late.

This is a distinct failure class from the ones the rest of the system already guards.
An exception is loud; a schema violation is caught by a validator; a crash is caught by
the diagnostic log. A *silent economic or quality regression* is caught by none of
them, precisely because the system kept working. Guarding it needs its own discipline,
and today that discipline is **reinvented per surface with gaps**: cache stability has a
byte-faithful *rule* (CSC-4) and a suggested *test*, but a test cannot reproduce a bust
that only emerges from live per-instance state; retrieval has a regression *gate*
(RE-6), but on a single central metric that a tail-only regression slips past; shaping
has *counterfactual honesty* (GS-6), but nothing states it as the rule for every
counterfactual saving; and none of them bounds the cost or trustworthiness of the
observation layer itself, so a malicious client posting a fresh label per request could
blow up the very signal meant to catch the regression.

The fix is one L1 contract every silently-degradable optimization instantiates —
exactly as `l1-cache-stable-context` unified the scattered prompt-cache precautions
into one concept they all cite. It names the silent-failure class, mandates a *runtime*
sensor (not just a test) where the breach is emergent, mandates *measured* (not
asserted) correctness and efficacy, mandates a *tail-aware* gate on distribution-shaped
metrics, and hardens the observation layer against blow-up and forgery.

## 2. Constraints & Assumptions

- This concept governs *verification*, not the transforms themselves. It never decides what to compress, where to cache, or how to shape output — it decides how the system knows those decisions are still correct and still paying off. The transforms own their behavior; this owns their proof.
- An integrity check is cheap relative to what it guards: a runtime sensor is a counter comparison, a parity gate runs off the hot path or over a sampled control, an accuracy fixture is small and version-controlled. Verification that cost more than the regression it catches would defeat itself.
- Some savings are *counterfactual* — the un-optimized outcome is never observed, so a saving cannot be computed by subtraction. Honesty about that (estimate-with-range or held-out control) is part of the contract, not a nicety.
- The concrete thresholds, percentiles, control-group fractions, and label vocabularies are tuning parameters; this concept constrains the *behavior* (named-class, runtime-sensed, measured-correctness, measured-efficacy, tail-aware, bounded-cardinality, verify-don't-repair, fail-visible), not the numbers.

## 3. Core Invariants

Rules every Layer 2 implementation MUST NOT violate. They are technology-neutral.

- **OI-1 (Silent-failure class is named and covered):** an optimization whose breach produces no error and well-formed output — degrading only cost, latency, or answer quality — MUST carry an explicit integrity check. It may NOT rely on exceptions, crashes, or functional/schema tests to catch its regression, because a silent breach trips none of them. The defining property is silence: the symptom is the invoice or a quality metric, never a thrown error. Every silently-degradable optimization declares which integrity checks (OI-2…OI-5) cover it.

- **OI-2 (Runtime violation sensor, not only a test):** where an optimization's contract is byte-exact or its violation is emergent from live per-instance state — so a static test cannot reproduce the real failure — the contract MUST be guarded by a *runtime* sensor that fires on the first genuine violation in production, not only by a pre-ship test. The canonical form is a monotonic violation counter expected to stay at zero, alarmed on any non-zero: a path that promised byte-faithful passthrough increments a *bytes-modified* counter on any divergence between what it received and what it forwarded. A test is necessary but not sufficient — the failure the sensor catches is one the test, by construction, cannot stage.

- **OI-3 (Correctness parity — outcomes preserved, measured not asserted):** an optimization that claims to preserve outcomes ("same answer, fewer tokens") MUST have that claim *measured*, never merely asserted. Measurement is either a parity check against a fixed, version-controlled fixture (the outcome *with* the optimization matches the outcome *without* it within a declared tolerance) or a held-out control fraction (a declared slice runs un-optimized for live comparison). An optimization whose outcome-preservation is only asserted is untrusted and MUST be treated as such (OI-8).

- **OI-4 (Efficacy parity — saving real, honestly measured):** the benefit an optimization claims — tokens saved, cache hit, latency cut — MUST be measured, attributed *per stage* (so one stage's saving is never credited to another), and reported honestly. Where the benefit is counterfactual (the un-optimized outcome is never observed), it is reported as an explicit estimate carrying a confidence range, or as a measured number from a deliberately held-out control — never as a fabricated exact figure. An optimization that cannot show its saving cannot be trusted or tuned, and a saving reported as a confident exact number it cannot have is a defect.

- **OI-5 (Tail-aware regression / parity gate):** when a silently-degradable optimization is tuned, changed, or its implementation swapped, the gate that admits the change MUST guard the *distribution* of the affected quality/economy metric — its tail (high/low percentiles) as well as its center — not a single central statistic. A regression confined to a tail slice — a small class of long sessions losing cache hits, a compression strategy that fails to shrink only on outliers — hides behind a median-only check. The gate fails if *any* guarded statistic (e.g. p50, p95, p99, and mean) regresses beyond tolerance against the established baseline, and it is overridable only with an explicit, recorded justification.

- **OI-6 (Bounded-cardinality, code-closed observation):** every dimension (label, key, bucket) of an integrity signal is drawn from a *closed vocabulary fixed in code*, never from untrusted or unbounded input. An observed value outside the vocabulary is mapped to an explicit `other` bucket and surfaced (so wire-format drift is loud), never used raw as a dimension. This keeps the verification layer cheap, un-gameable, and safe: neither an adversary posting a fresh value per request nor an upstream format drift can explode the signal's cardinality (a cost/DoS footgun) or inject a forged dimension into the record the integrity decision reads.

- **OI-7 (Verify, don't repair or hide):** integrity verification only *measures and surfaces*. It raises a detected violation as a signal to operational health and hands remediation to the self-healing subsystem; it never silently repairs a violation (which would erase the evidence that the optimization is broken), and it never suppresses, smooths, or downgrades a detected violation to avoid raising an alert. A silent-degradation, once detected, is escalated — not quietly absorbed.

- **OI-8 (Fail-visible — unverified is never trusted):** when integrity verification itself cannot run — a sensor unavailable, a fixture missing, a control group not sampled, a signal not yet populated — that gap is surfaced as an explicit *unverified* state, never presented as *verified*. An optimization is trusted only where its integrity check is actually running; the absence of a check is the absence of trust, not implicit trust. A dashboard, health score, or gate MUST distinguish "verified holding" from "not verified" and never render the second as the first.

> L2 specs cannot reach RFC status until all invariants here are addressed in their "Invariant Compliance" section.

## 4. Detailed Design

### 4.1 The silent-failure class

An optimization failure is *silent* when the output stays well-formed and the process
keeps running, so no existing guard fires:

| Guard | Catches | Misses a silent optimization regression? |
| --- | --- | --- |
| Exception / crash | a thrown fault | yes — nothing is thrown |
| Schema / output validator | a malformed result | yes — the result is well-formed |
| Functional test | a wrong result on a fixed input | usually — a cache bust or over-drop is state- or data-dependent, not reproduced by the fixed test |
| Diagnostic log | a native crash / fault | yes — there is no fault |
| **Integrity check (this spec)** | **a correct-but-costlier, or plausible-but-worse, result** | **no — this is exactly its target** |

The class members share three traits: (1) the output passes as valid, (2) the cost or
quality moved the wrong way, and (3) the movement is only visible in aggregate or over
time. OI-1 requires each such optimization to declare its coverage against OI-2…OI-5.

### 4.2 Two integrity questions

Every silently-degradable optimization must answer both, measured not asserted:

| Question | Invariant | Measurement |
| --- | --- | --- |
| Did it still preserve the outcome? | OI-3 | parity fixture (with vs without) **or** held-out control |
| Is its claimed saving real? | OI-4 | per-stage accounting; counterfactual → estimate+range **or** held-out control |

A single held-out control fraction can answer both at once — the un-optimized slice is
the ground truth for both the outcome (OI-3) and the saving (OI-4).

### 4.3 Runtime violation sensor (OI-2)

The byte-faithful-passthrough contract is the archetype: a bust depends on live
per-instance state (`l1-cache-stable-context` CSC-10 — a rewrite-to-align aligner
changes the key *because* its output depends on per-request state), so a static test
cannot stage it. A runtime sensor can:

```text
[REFERENCE]
forward(request, path):
    original_len := byte_len(request)
    out := dispatch(request, path)                 // a path that PROMISED byte-faithful passthrough
    if path.promises_passthrough and byte_len(out) != original_len:
        bytes_modified_total{path}.inc(delta)       // OI-2: must-stay-zero counter
        alarm("passthrough integrity violated", path, delta)   // fires on FIRST real bust
    return out
// intentional, sanctioned mutations (e.g. a cache-marker injector) run AFTER this
// check, so they never trip the alarm — the sensor watches only the promised-inert path
```

The counter is expected to sit at exactly zero; any non-zero rate is a live cache bust
caught the instant it happens, not an invoice audit later. The general form: *a
silently-violable inert contract carries a monotonic violation counter, alarmed on
first non-zero.*

### 4.4 Tail-aware parity gate (OI-5)

A quality/economy metric is a *distribution*, not a scalar. Guarding only its center
lets a tail regression ship:

```text
[REFERENCE]
admit_change(metric_series, baseline):
    for stat in { p50, p95, p99, mean }:            // OI-5: the tail, not only the center
        if regressed(stat(metric_series), baseline[stat], tolerance):
            return REJECT(stat)                      // ANY guarded statistic regressing fails the gate
    return ADMIT
// swapping one implementation of a silently-degradable subsystem for another
// (e.g. a rewrite of the compression/cache path) is admitted only on full-distribution
// parity — a regression that only bites the p99 tail (a small class of sessions) would
// pass a median-only check and silently degrade those sessions in production
```

This generalizes `l1-retrieval-evaluation` RE-6 from a single primary metric to the
distribution of any silently-degradable quality/economy metric.

### 4.5 Bounded-cardinality observation (OI-6)

The verification layer must not become the thing that breaks:

```text
[REFERENCE]
label_for(raw_value, vocabulary):                    // vocabulary is a CLOSED, code-fixed set
    if raw_value in vocabulary:  return raw_value
    warn("observation vocabulary drift", raw_value)   // loud: wire-format drift surfaces
    return "other"                                     // never the raw value as a dimension
// a client posting {"tier": "<random-per-request>"} is bucketed to "other";
// it can neither explode signal cardinality (cost/DoS) nor forge a dimension (integrity)
```

Every dimension — strategy, content-type, provider, path, tier, status — is a closed
vocabulary bounded by code, per `l1-log-legibility` LL-6 and `l1-context-provenance`
CP-1 (untrusted input is data, never a control dimension).

### 4.6 Where integrity signals go

Integrity verification is a *producer* of signals, not a repairer or an egress:

- **To operational health** (`l1-operational-health`): a violation counter crossing zero, a parity-gate rejection, an efficacy estimate outside its expected band, and an *unverified* coverage gap (OI-8) each surface as OH signals — scored, alerted, trended. OI-7 mirrors OH-7: measure and hand off, never repair here.
- **To self-healing** (`l1-doctor`): remediation of a detected fault is the doctor's job; integrity verification only reports it.
- **To telemetry** (`l1-telemetry`): integrity metrics may be shared outward *only* under the existing opt-in, program-data-only boundary — never a new egress path.

## nodus-relevance mapping

A workflow step that invokes a model, reads a cached macro result, or dereferences a
`StorageProvider` has the same silent-degradation exposure at the step grain — but it
needs **no new language invariant**, because the seams already exist and integrity
observation is a host concern (nodus LP-1/LP-2):

| Element | nodus seam | Note |
| --- | --- | --- |
| Runtime violation sensor (OI-2) | `AuditProvider` cost/integrity events (HO-8/HO-9) | A step whose provider promises an inert passthrough emits a violation event on divergence; the host owns the counter, core owns no sensor vocabulary. |
| Correctness parity (OI-3) | `!!` invariants + host-run parity fixture over a `RUN(@macro)` | A reused macro result (MI-13 short-circuit) is parity-checkable against a fresh run; the host decides when, core asserts only its `!!` rules. |
| Efficacy parity (OI-4) | `AuditProvider` per-step token/cost classes | Per-step saving rides the existing cost-attribution channel with its estimate/measured basis; no new event type. |
| Tail-aware gate (OI-5) | host-side over recorded step-metric series | Which percentiles gate a workflow change is a host judgement over the audit stream; core neither aggregates nor gates. |
| Bounded cardinality (OI-6) | `AuditProvider` label discipline | Step/label vocabularies are host-fixed closed sets; an untrusted step input never becomes an audit dimension. |

The nodus workspace owns any realization; this records the mapping. The workflow side is
fully covered by `!!` invariants, `RUN(@macro)` reuse gating, and the `AuditProvider`
observability channel (HO-8/HO-9) — so no new NL invariant is warranted (additive; a
host that verifies nothing behaves exactly as today).

## 5. Drawbacks & Alternatives

- **Verification overhead.** Every silently-degradable optimization now carries a check. Mitigated by keeping each check cheap: a runtime sensor is a length/hash comparison (OI-2), an efficacy estimate rides existing accounting (OI-4), a parity gate runs off the hot path or over a sampled control (OI-3/OI-5). A check that cost more than the regression it catches would be self-defeating and is disallowed by the cheapness constraint (§2).
- **Held-out control forgoes some savings.** The measured (rather than estimated) path leaves a declared fraction of work un-optimized (OI-3/OI-4). Accepted and opt-in: the estimate-with-range path needs no sacrifice, and the holdout is offered only for those who want a measured number and will pay a small control cost for it.
- **Threshold and percentile tuning burden.** Tail-aware gates and violation thresholds produce false positives until tuned. Mitigated by sensible defaults, the recorded-override escape hatch (OI-5), and feeding operational health where dedup and severity grading already live.
- **Alternative — fold into `l1-operational-health`.** Rejected: operational health scores the *trend* of the runtime from traces it already has (OH-1); it does not mandate a *runtime violation sensor* on a byte-exact contract, a *parity fixture* proving outcomes unchanged, a *tail-aware* gate on change, or a *bounded-cardinality* rule on the observation dimensions. Those are invariant-level integrity mechanics specific to silently-degradable optimizations; operational health is the *surface* they feed, not their owner.
- **Alternative — leave each optimization to guard itself.** Rejected: cache stability, inference-cache warmth, retrieval, compression, and shaping were each reinventing this discipline with gaps — a runtime sensor missing here, a mean-only gate there, no cardinality bound anywhere. One L1 contract they all cite makes the coverage uniform, exactly as `l1-cache-stable-context` unified the scattered prompt-cache precautions rather than leaving each subsystem to rediscover them.
- **Alternative — trust the provider's own dashboards.** Rejected: a provider bill or console reports cost after the fact and per account, not per-session, per-stage, or in time to gate a change; it also cannot see a local optimization's counterfactual. Integrity must be verifiable on-device, at the seam where the optimization acts.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[OPS-HEALTH]` | `.design/main/specifications/l1-operational-health.md` | The surface integrity signals feed; OH-7 measure-don't-act precedent for OI-7. |
| `[CACHE-STABLE]` | `.design/main/specifications/l1-cache-stable-context.md` | CSC-4/CSC-10/CSC-11 — the archetypal silent-failure contract OI-2 senses at runtime. |
| `[SHAPING]` | `.design/main/specifications/l1-generation-shaping.md` | GS-6 counterfactual honesty — the output-side instance of OI-4. |
| `[RETRIEVAL-EVAL]` | `.design/main/specifications/l1-retrieval-evaluation.md` | RE-6 regression gate OI-5 generalizes to the distribution tail. |
| `[LOG-LEGIBILITY]` | `.design/main/specifications/l1-log-legibility.md` | LL-6 stable attribution — the substrate OI-6 constrains to a closed vocabulary. |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.1 | 2026-09-24 | Core Team | Consistency pass (2026-09-24): Bare `LP-n` citations of the nodus portability contract are now written `nodus LP-n`: this workspace's `l1-lookahead-planning` defines LP-1…LP-6 as well, so the bare form pointed a reader at the wrong invariant. No requirement changed. |
| 1.0.0 | 2026-07-15 | Core Team | Initial spec — optimization integrity as the cross-cutting verification discipline for silently-degradable optimizations: named silent-failure class with mandatory coverage (OI-1), runtime violation sensor for byte-exact/emergent contracts rather than a test alone (OI-2), measured-not-asserted correctness parity via fixture or held-out control (OI-3), honestly-measured per-stage efficacy with counterfactual estimate+range or holdout (OI-4), tail-aware distribution parity/regression gate rather than a central-statistic check (OI-5), bounded-cardinality code-closed observation dimensions with untrusted input never a label (OI-6), verify-don't-repair-or-hide measure-and-surface boundary (OI-7), fail-visible unverified-is-never-trusted coverage honesty (OI-8); §4.3 must-stay-zero passthrough sensor, §4.4 p50/p95/p99/mean parity gate, §4.5 closed-vocabulary label discipline; nodus-relevance mapping needing no new NL invariant (host-side over AuditProvider HO-8/HO-9 + `!!` + `RUN(@macro)` reuse gating). Distilled from an adoption pass over an external agent context-optimization reference whose *transform* mechanics (content-routed reversible compression, cache-aligned live-zone compression, output-side character/effort shaping, cross-agent memory, failure-mining) were already realized by l1-context-compression (CC-1…11 + §4.5 CCR), l1-cache-stable-context, l1-generation-shaping (GS-1…9), the memory cluster, and l2-learning-loop — OI captures the one remaining delta those transform-focused passes left open: the *verification layer* that proves a silently-degradable optimization stays correct-and-effective (runtime passthrough-integrity alarm, full-distribution parity gate, bounded-cardinality observation), generalizing the per-surface guards CSC-4/CSC-11, IC-7, RE-6, and GS-6 into one contract. |
