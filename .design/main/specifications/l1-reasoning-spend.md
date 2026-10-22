# Reasoning Spend

**Version:** 1.0.1
**Status:** Stable
**Layer:** concept

## Overview

Reasoning spend is the accounting contract for **generation the principal pays for and never receives**: the model's internal deliberation, produced under the same generation call as the answer, billed at generation rates, consuming the same capacity, and — on most providers — never delivered to the caller at all.

The system already owns the *dial* (turn-classified effort modulation) and the *length budget* (reserve small, escalate on truncation). What neither models is that the thing the dial moves is a **real, metered, often dominant quantity that leaves no trace in the output**. Fold it into "output tokens" and four things quietly break: the price is a class-blend of two units that behave nothing alike, the length budget can be fully consumed before a single visible token appears, a provider that reports nothing is indistinguishable from a turn that thought nothing, and every per-outcome cost figure is understated by exactly the amount nobody can see.

This spec names the class, states how it shares capacity with the visible answer, and fixes the honest-degradation rules for the case where the provider will not tell you how much you spent.

## Related Specifications

- [l1-generation-shaping.md](l1-generation-shaping.md) — GSH-3 floored effort/reasoning-depth modulation is the **control**; this spec is the **meter** that control moves. GS decides how much deliberation a turn warrants; nothing there says what the deliberation costs or how it is accounted.
- [l1-generation-budget.md](l1-generation-budget.md) — GB governs output **length**. RSN-2 states the capacity GB caps is **shared** with reasoning and pre-empted by it, which splits GB-2's truncation signal into two causes with opposite remedies.
- [l1-cost-rating.md](l1-cost-rating.md) — CRT-2 rates per unit class and forbids one blended rate; reasoning is the class its list was missing. CRT-4's fail-visible unknown rate is the pattern RSN-3/RSN-10 apply to an unreported **count**.
- [l1-usage-allowance.md](l1-usage-allowance.md) — UA-1/UA-2: reasoning consumes the provider allowance like any other generated token, so a readout that omits it appears to lose balance without spending it.
- [l1-outcome-attributed-cost.md](l1-outcome-attributed-cost.md) — OAC-1 attributes every unit of accounted spend to a durable output or explicitly to none; RSN-6 keeps invisible spend inside that denominator, where it is most likely to be dropped and most distorting when it is.
- [l1-model-benchmarking.md](l1-model-benchmarking.md) — MB-3's three-dimensional scorecard reports tokens separately from quality and time; RSN-7 requires reasoning to be a **separately reported** token figure, because two candidates with identical answers and a tenfold reasoning gap are not the same proposition.
- [l1-inference-cache.md](l1-inference-cache.md) — IC's prefix reuse amortizes **input** across turns; RSN-4 states reasoning is not amortized by it, so warmth-driven cost decay does not apply to the deepest turns.
- [l1-fidelity-variants.md](l1-fidelity-variants.md) — reasoning depth is a fidelity axis: moving it changes cost and quality together, so figures across depths are not comparable without declaring the depth (RSN-5).
- [l1-inner-monologue.md](l1-inner-monologue.md) — the system's **own** background reflection channel, with its own IM-3 budget; demarcated in §4.5 (that is work the system schedules and can read; this is generation the provider performs inside one model call).
- [l1-tokenization-boundary.md](l1-tokenization-boundary.md) — TB-6: counts under different encoders are different quantities. A reasoning count is subject to the same rule as any other count.
- [l1-claim-verification.md](l1-claim-verification.md) — where reasoning content **is** visible, it is a deliberation trace, not a record of what happened; RSN-8 forbids citing it as evidence that an action occurred.
- [l1-observation-retention.md](l1-observation-retention.md) — OR-3's *a gap is a value*: an unmeasured reasoning count records a marker, never a zero.
- [../../nodus/specifications/l1-nodus-environment.md](../../nodus/specifications/l1-nodus-environment.md) — NE-15 is the evaluation-substrate realization: a declared token budget must record **which generated classes it counts**, or two candidates halted at the same nominal budget ran under different ones.

## 1. Motivation

A model that deliberates before answering produces two streams from one call. One is returned; the other is billed and discarded. On models with extended deliberation the discarded stream is routinely larger than the returned one, and on hard turns it can exceed it by an order of magnitude.

Every part of the system that touches tokens was designed against the assumption that generated tokens are the tokens you get back. Each inherits a specific defect from that assumption:

- **Pricing** blends two units with different behavior into one figure, so nobody can see that the bill moved because deliberation deepened rather than because answers got longer.
- **The length budget** reserves capacity for the expected answer. Deliberation is produced first and consumes that reservation, so an undersized cap yields a **truncated-to-nothing response at full price** — and the truncation signal looks identical to an answer cut off midway, whose remedy (continue from the partial) is exactly wrong here.
- **Allowance readouts** show consumption the user cannot connect to anything they received, which reads as a metering bug rather than as the honest fact it is.
- **Per-outcome cost** understates by the invisible share — and understates *most* on precisely the expensive, difficult, low-yield work the attribution exists to surface.
- **Measurement** compares candidates on a single token number, hiding the candidate that reaches the same answer by thinking ten times as long.

There is also a reporting asymmetry no other class has: some providers return the deliberation content, some return only a count, some return nothing. A system that treats "not reported" as zero will confidently report that its most expensive turns were free.

## 2. Constraints & Assumptions

- **The system does not control whether deliberation happens.** It can request a depth (GSH-3) and choose a model; the production of internal reasoning is the provider's, inside a single call.
- **Visibility is a provider property, not a setting the system can rely on.** All three states — content, count-only, neither — are normal and must be representable.
- **Reasoning is generated output.** It is produced by the same generation pass, priced at generation rates, and drawn from the same capacity; it is not an input-side or context-side quantity.
- **The count, where reported, is authoritative over any local estimate**, on the same basis as every other provider-reported figure.
- **This spec accounts; it does not optimize.** Deciding how much deliberation a turn warrants belongs to shaping (GS); deciding which model to route to belongs to routing.

## 3. Core Invariants

Rules every Layer 2 implementation MUST NOT violate. They are technology-neutral.

- **RSN-1 (A distinct accounting class):** generation the principal is billed for but never receives is a **first-class unit class** in metering, rating, allowance, and attribution — never folded into the delivered-output class. It extends the per-class rate discipline (CRT-2) rather than reusing a neighbouring class's rate, because the two behave differently in every dimension that matters: one is readable and one is not, one is amortizable by prefix reuse and one is not (RSN-4), one is governed by a length target and one by a depth dial (RSN-5). A single blended figure cannot answer "did the bill rise because answers got longer or because the model started thinking harder", which is the first question anyone asks.

- **RSN-2 (Shared capacity, deliberation first):** the generation capacity a turn is granted is **shared** between deliberation and the visible answer, and deliberation is produced **first**. A cap sized for the expected answer alone can therefore be **fully consumed before one visible token exists**, producing an empty or stub response at full cost. Truncation detection (GB-2) MUST distinguish the two causes — *answer cut off mid-production* versus *capacity exhausted before the answer began* — because their remedies are opposite: the first continues from the preserved partial (GB-3), the second must **raise the cap or lower the depth**, and continuing from a partial that does not exist silently re-spends the whole budget on deliberation again.

- **RSN-3 (Three visibility states, never conflated):** the class is recorded in exactly one of three honest states — **content available**, **count only**, **not reported** — and no implementation may collapse them. *Not reported* is a gap marker (OR-3), never zero, and never an omitted field that a consumer will read as zero. The distinction is load-bearing: a turn that did not deliberate and a turn whose deliberation the provider declined to report are the same number and opposite facts.

- **RSN-4 (Per-turn recurring, not amortized by prefix reuse):** deliberation is produced afresh on each turn and does **not** become a reusable cached prefix; prefix-addressed reuse (IC-1) reduces the cost of re-ingesting context, never the cost of thinking again. A cost model that projects spend decaying with cache warmth MUST exclude this class from that decay, or it will under-forecast exactly on the deep-deliberation turns where the error is largest and least affordable.

- **RSN-5 (Depth is a declared fidelity axis):** reasoning depth changes **cost and quality together**, so it is a fidelity variant (FV) of the same request, not a free efficiency dial. Any cost, quality, or latency figure compared across turns MUST carry the depth it was produced at; comparing a shallow turn's cost to a deep turn's outcome is a category error that reliably concludes the cheaper setting was better.

- **RSN-6 (Attributed, especially when nothing was produced):** reasoning spend is attributed to the same work unit as the turn that incurred it, under the ordinary attribution rules (OAC-1), **including turns that produced no durable output**. A turn that deliberated at length and yielded nothing is the case most likely to be dropped from a denominator and most distorting when it is: dropping it makes an unproductive session look efficient. Spend-with-no-output remains a measured category carrying no implicit verdict (OAC-4) — exploration and a tenth failed attempt produce the same number and mean opposite things.

- **RSN-7 (Separately reported under measurement):** wherever token consumption is reported for comparison — benchmarks, evaluation suites, cost dashboards — the reasoning class is reported **as its own figure** beside delivered output, never summed into one number (MB-3 discipline). Two candidates that produce the same answer with a tenfold difference in deliberation are economically different; a combined total makes the difference invisible at exactly the moment the comparison exists to expose it.

- **RSN-8 (Visible reasoning is a trace, not a record):** where the provider returns deliberation content, it is **model-authored generated text** and carries the same provenance and confidentiality treatment as any other generation — untrusted as input, secret-safe on every projection, and a **disclosure decision** to surface rather than an automatically shown field. It is a record of what the model *considered*, never evidence that an action *occurred*: a claim MUST NOT be verified against it (the acting record is the tool/effect record). Reasoning that describes an action the system has no effect record for is a claim, not a receipt.

- **RSN-9 (Counted against the external allowance):** reasoning consumes the provider allowance and quota window like any other generated token (UA-1). A readout that meters only delivered output will show a balance falling faster than its own accounting explains — the honest presentation attributes the consumption to this class rather than leaving an unexplained gap that reads as a defect in the meter.

- **RSN-10 (Honest degradation, declared lower bounds):** when the count is unavailable (RSN-3 *not reported*), the class is recorded as **unmeasured**, and every aggregate that includes an unmeasured turn is published as a **declared lower bound** — never a confident total, never a silent zero. This is CRT-4's fail-visible discipline applied to the count rather than the rate: an under-reported spend corrupts every figure derived from it, and does so most on the providers that reveal least.

## 4. Detailed Design

### 4.1 The class beside its neighbours

| Class | Delivered to caller | Amortizable by prefix reuse | Governed by | Reported by every provider |
| --- | --- | --- | --- | --- |
| Fresh input | n/a (consumed) | partially (cached prefix) | context budget | yes |
| Cache read / creation | n/a | that *is* the amortization | cache policy | usually |
| Delivered output | **yes** | no | length budget (GB) | yes |
| **Reasoning** | **no** | **no** (RSN-4) | **depth dial (GSH-3)** | **no** (RSN-3) |

The last row is the one the cost pipeline never had. Every column differs from the row above it, which is the whole argument against a blended figure.

### 4.2 The shared cap, drawn out

```
granted generation capacity
├─────────────── deliberation ───────────────┤├──── visible answer ────┤
                                              ▲
                             everything to the left is billed and discarded

failure mode: capacity ends before this point
  → response empty or stub, full cost, and GB-2 reports "truncated"
  → GB-3 continuation has no partial answer to continue from
  → correct remedy: raise the cap, or lower the depth (GSH-3)
```

Distinguishing the two truncation causes is mechanical, not a judgment call: a truncation with **zero delivered output tokens and non-zero (or unmeasured-but-deep-configured) reasoning** is capacity exhaustion before the answer; a truncation with substantial delivered output is an answer cut off. The second case is GB's existing continuation path and is unaffected by this spec.

### 4.3 What "unmeasured" costs downstream

An unmeasured turn propagates as a lower bound, not as an absence:

```
turn (unmeasured reasoning)
  → session total: "≥ N tokens"        (never "N")
  → session cost:  "≥ $X"              (CRT-4 pattern)
  → per-outcome:   denominator flagged incomplete (OAC-7 dispersion + abstention)
  → benchmark:     candidate's token dimension marked unmeasured, not zero (MB-9 kinship)
```

The rule is uniform: an unmeasured input never becomes a confident output. A system that quietly substitutes zero produces figures that are wrong in a consistent direction — always understating — which is worse than noisy, because it survives sanity checks.

### 4.4 Where each consumer changes

| Consumer | Change |
| --- | --- |
| Metering | records the class separately, with its visibility state |
| Rating (CRT-2) | rates it at its own rate; unpriced → `UNKNOWN`, not $0 |
| Generation budget (GB-2) | splits truncation into two causes (§4.2) |
| Allowance (UA-3) | attributes the consumption the user cannot see |
| Attribution (OAC-1) | keeps invisible spend inside the denominator |
| Benchmarking (MB-3) | reports it as its own dimension |
| Shaping (GSH-3) | unchanged — it moves the dial; this spec says what the dial spends |

### 4.5 Demarcation — the system's own reflection is not this

The inner-monologue channel is background cognition the **system** schedules, budgets (IM-3), logs before acting on (IM-2), and can read in full. Reasoning spend is generation the **provider** performs inside a single model call, which the system requests a depth for and frequently cannot see at all. They are separate quantities with separate budgets: an inner-monologue cycle is itself a model call, and that call has its own reasoning spend accounted under this contract. Conflating them double-counts one and hides the other.

## 5. Implementation Notes

- Record the visibility state **at the point of metering**, not at presentation. A field that is absent for two different reasons cannot be repaired downstream.
- The two-cause truncation split (§4.2) is cheap and should be a named condition in the code, not an inference at the reporting layer — by the time a report is written, the delivered-token count and the configured depth may no longer be adjacent.
- Where a provider reports only a count, the count is authoritative and no local estimate should compete with it; where it reports nothing, resist estimating from depth configuration — a fabricated number is worse than a declared gap, because it silently exits the lower-bound discipline of RSN-10.
- Presentation should surface this class by default in any cost readout. Hiding it recreates the exact confusion the spec exists to remove: the bill moved and nothing visible changed.

## 6. Drawbacks & Alternatives

- **Another unit class to carry everywhere.** Metering, rating, allowance, attribution, and reporting each gain a column. That is the price of the question "why did the bill move"; the blended alternative cannot answer it at all.
- **Alternative — treat reasoning as delivered output:** rejected (RSN-1). It is the current implicit design and it is what produces every defect in §1; the two units differ on delivery, amortization, governance, and reportability.
- **Alternative — estimate the count when the provider does not report it:** rejected (RSN-10). An estimate presented as a count converts a known unknown into a confident error, and the error is systematic rather than random.
- **Alternative — exclude unmeasured turns from aggregates:** rejected. It biases every aggregate toward the providers that report, which are not a random sample; a declared lower bound is honest and still usable.
- **The shared-cap rule depends on production order.** RSN-2 assumes deliberation precedes the visible answer, which is the observed behavior of the models this applies to. A provider interleaving them would keep the accounting (RSN-1) and weaken only the §4.2 diagnostic. <!-- TBD: whether an interleaving provider needs a third truncation cause, or whether zero-delivered-output remains a sufficient discriminator -->

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[SHAPING]` | `.design/main/specifications/l1-generation-shaping.md` | GSH-3 depth dial — the control this spec meters |
| `[GENBUDGET]` | `.design/main/specifications/l1-generation-budget.md` | The length budget whose capacity RSN-2 shares |
| `[RATING]` | `.design/main/specifications/l1-cost-rating.md` | CRT-2 per-class rates, CRT-4 fail-visible unknown |
| `[ATTRIBUTION]` | `.design/main/specifications/l1-outcome-attributed-cost.md` | OAC-1 denominator this spend stays inside |
| `[BENCH]` | `.design/main/specifications/l1-model-benchmarking.md` | MB-3 separated dimensions RSN-7 extends |
| `[CACHE]` | `.design/main/specifications/l1-inference-cache.md` | Prefix reuse RSN-4 excludes this class from |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.0 | 2026-08-06 | Core Team | Initial concept: accounting contract for **billed generation the principal never receives**. The system already owned the depth **dial** (GSH-3) and the length **budget** (GB) but never the **meter** between them, so model deliberation — routinely larger than the answer it precedes — was implicitly folded into delivered output. A distinct unit class extending CRT-2's per-class rates, justified by differing on delivery, amortization, governance, and reportability (RSN-1); **shared capacity with deliberation produced first**, splitting GB-2's truncation signal into two causes with opposite remedies — answer-cut-off (continue) versus capacity-exhausted-before-the-answer (raise cap or lower depth), where continuing from a partial that does not exist re-spends the budget on deliberation again (RSN-2); three never-conflated visibility states content / count-only / **not reported**, the last a gap marker never a zero, since a turn that did not think and a turn whose thinking went unreported are the same number and opposite facts (RSN-3); **not amortized by prefix reuse**, so warmth-driven cost decay must exclude it or under-forecast worst on the deepest turns (RSN-4); depth as a declared fidelity axis moving cost and quality together, making cross-depth figures incomparable without the depth (RSN-5); attributed **especially on turns that produced nothing**, the case most likely to be dropped from a denominator and most distorting when it is (RSN-6); separately reported under measurement, since equal answers with a tenfold deliberation gap are economically different (RSN-7); visible reasoning treated as untrusted model-authored **trace, never a record of action** — a claim is never verified against it (RSN-8); counted against the provider allowance so a readout has no unexplained gap (RSN-9); and honest degradation as **declared lower bounds** when unmeasured, CRT-4's fail-visible discipline applied to the count rather than the rate (RSN-10). §4.5 demarcates it from the system's own inner-monologue channel — separate budgets, and an inner-monologue call has its own reasoning spend under this contract. Nodus realization: `l1-nodus-environment` NE-15 (declared budget scope). |
| 1.0.1 | 2026-10-22 | Core Team | Citations updated for the requirement-ID prefix renames that make each prefix name one specification (RULES §1, 1.8.0, the person's decision of 2026-10-22): `CR-n` of `l1-cost-rating` is now `CRT-n`; `GS-n` of `l1-generation-shaping` is now `GSH-n`. No requirement was added, removed or reworded. |
