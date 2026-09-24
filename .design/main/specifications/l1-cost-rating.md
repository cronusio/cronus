# Cost Rating & Pricing

**Version:** 1.1.1
**Status:** Stable
**Layer:** concept

## Overview

The layer that turns a **metered usage record** into a **monetary figure** — the
*rating* step of the cost pipeline, distinct from *metering* (counting the units) and
*charging* (comparing spend to a budget). It owns the rate card (per-model,
per-unit-class prices), the resolution of a model to its rate, the derivation of a cost
from usage × rate, and — above all — the honesty of that number: which token/modality
class was billed at which rate, whether the figure is a local estimate or a
provider-reported billed amount, whether the rate is fresh or stale, and what happens
when a model has no known rate. It exists because many consumers — the budget engine,
the usage-allowance windows, the dashboard, the router's cost signal, model
benchmarking's `derived_cost`, and the workflow runtime's per-run cost — each assume a
"catalog price" exists and is trustworthy, with no single contract governing that trust.
This spec is that contract: a priced number has exactly one producer, and it is never
silently wrong.

## Related Specifications

- [l2-budget-engine.md](l2-budget-engine.md) — the enforcement/roll-up consumer: it ingests the priced cost events this layer produces and enforces thresholds. Rating derives the number; the budget engine acts on it. Never conflated.
- [l1-usage-allowance.md](l1-usage-allowance.md) — consumes a priced cost for cost-denominated allowance windows; UA-2 (provider-reported vs locally-estimated, honest about which) is the allowance-side parallel of CR-5.
- [l1-cache-stable-context.md](l1-cache-stable-context.md) — CSC-11 separates `cache_read` / `cache_creation` / fresh-`input` / `output` token *counts*; CR-2 prices each of those classes at its own rate (a cache-read unit is billed at a fraction of a fresh-input unit). Rating is the pricing complement of that count-separation.
- [l1-model-benchmarking.md](l1-model-benchmarking.md) — MB-3's `derived_cost from catalog pricing` is a consumer of this layer; CR owns the catalog pricing MB-3 assumes, and MB-7 profile staleness is the fitness-profile parallel of CR-6 rate staleness.
- [l1-optimization-integrity.md](l1-optimization-integrity.md) — OI-4 efficacy parity needs a trustworthy per-stage cost to prove a saving; CR-4 (fail-visible unknown rate) is the cost-side of OI-8 (unverified is never rendered as verified), and CR-5 basis-labeling is OI-4's estimate-vs-measured honesty applied to price.
- [l1-outcome-attributed-cost.md](l1-outcome-attributed-cost.md) — [ADDED v1.0.1] the downstream consumer that closes the loop rating leaves open: rating derives *what was spent*, attribution answers *what the spend produced and whether it lasted*. CR-5 basis labeling and CR-8 auditable inputs travel through attribution unchanged — nothing there re-prices anything, and rating deliberately knows nothing about work items or outcomes.
- [l1-operational-health.md](l1-operational-health.md) — OH-6 cost/usage accounting is fed priced records; a mispricing (stale card, blended-rate regression) surfaces there via CR-8, not only on the provider invoice.
- [l1-routing.md](l1-routing.md) — the router consumes rated cost as a selection signal (RTG cost weighting); an unpriced or stale-priced lane is a signal the router must be able to see as such (CR-4/CR-6), not read as free.
- [l1-telemetry.md](l1-telemetry.md) — the only egress path for cost metrics, opt-in and program-data-only; native amounts and rate provenance never leave the device except through it.
- [l1-nodus-observability.md](../../nodus/specifications/l1-nodus-observability.md) — HO-8 records per-class token counts `counts-only`, deliberately unpriced; those counts are the units this layer rates host-side (see nodus-relevance mapping).

## 1. Motivation

Every part of the system that reasons about money assumes a price exists. The budget
engine ingests "cost events." The dashboard sums a `session_cost_usd`. Benchmarking
reports a `derived_cost`. The router weighs a lane's expense. The usage-allowance
windows count down a cost budget. But nothing owns *how that number is produced*, and
that gap is where cost silently goes wrong:

- **The unknown-model zero.** The most common defect: a model with no catalog entry is
  priced at zero. Every aggregate built on it then under-reports true spend — and the
  more a new, unpriced model is used, the more confidently wrong the total becomes. A
  zero cost is indistinguishable from a real zero, so nothing flags it.
- **The blended rate.** Applying one average price to total tokens ignores that input,
  output, and cached input are billed at very different rates (a cache-read unit is a
  fraction of a fresh-input unit). Correct token *counts* still yield a wrong *cost*.
- **The estimate passed off as a bill.** A locally-computed `rate × tokens` is an
  estimate; the provider's actual billed amount can differ (rounding, minimums,
  discounts, tiered pricing). Presenting the estimate as the billed truth is a
  quiet inaccuracy that only the invoice contradicts, too late.
- **The stale card.** Provider prices change and models get revised; a rate table
  captured last quarter drifts. A rate trusted as current when it is a months-old
  default silently misprices everything.

These are the same class of *silent, correct-looking-but-wrong* failure the cache and
optimization-integrity layers already guard on their own surfaces — here on the price.
The fix is one L1 that names the rating step, prices per class, resolves rates
honestly, refuses the silent zero, labels estimate vs billed, and tracks rate staleness
— a contract every cost consumer cites instead of re-assuming.

## 2. Constraints & Assumptions

- This layer *derives* cost; it never *meters* usage (token/unit counts come from the accounting/observability layers) nor *enforces* a budget (the budget engine does). One clean responsibility: usage + rate card → an honest monetary figure.
- The rate card is deployment data, not code logic: which models cost what is host/operator-owned and changes without a code change. This spec constrains the *contract* the card must satisfy (per-class, resolvable, provenance-dated, fail-visible), not the values.
- The canonical stored amount is in one currency; display conversion is a separate, explicit, provenance-marked transform (CR-7).
- Where the provider reports an authoritative billed amount, that supersedes the local estimate; absent it, the estimate stands, labeled as such. The system never fabricates precision it does not have.

## 3. Core Invariants

Rules every Layer 2 implementation MUST NOT violate. They are technology-neutral.

- **CR-1 (Rating is derivation — one producer):** cost rating converts a metered usage record (per-class token counts; image/audio/request units) into a monetary figure by applying a rate card. It is distinct from metering (counting the units) and from budgeting/enforcement (comparing spend to a limit). A priced number has exactly one producer — this layer — so that its honesty rules (CR-2…CR-8) are enforced in one place, not re-implemented per consumer.

- **CR-2 (Per-class rates, never one blended rate):** the rate card prices each billable unit class at its own rate — fresh input, output, cached/discounted input, cache-creation, and each modality unit (image, audio, per-request) — and total cost is the sum over classes of `units(class) × rate(class)`. Applying a single blended rate to total tokens is forbidden: it misprices every request whose class mix differs from the blend, most visibly by ignoring that a cache-read unit bills at a fraction of a fresh-input unit. Composes the separated token-class counts of `l1-cache-stable-context` CSC-11 and `l1-nodus-observability` HO-8.
  [MODIFIED v1.1.0] The class list includes **generation that is billed but never delivered** — a deliberating model's internal reasoning, produced under the same call as the answer and frequently larger than it. It is a class in its own right, not a variant of `output`: it is unreadable, is not amortized by prefix reuse, is governed by a depth dial rather than a length target, and is not reported at all by some providers. Pricing it as `output` makes the two indistinguishable, so a bill that moved because deliberation deepened is indistinguishable from one that moved because answers got longer — the first question anyone asks. Where the count is unavailable the class is **unmeasured** and every aggregate containing it is a declared lower bound, the count-side twin of CR-4's fail-visible unknown **rate**; see `l1-reasoning-spend` (RSN-1/RSN-3/RSN-10).

- **CR-3 (Honest rate resolution — exact, then family, marked):** a model's rate is resolved by exact identity first. Where no exact entry exists, a family/base rate MAY be inferred by normalizing away dated or channel suffixes — but an inferred rate is marked *inferred*, never presented as the model's exact published rate, and resolution never silently substitutes a *different* model's rate as authoritative.

- **CR-4 (Fail-visible unknown rate — never a silent zero):** a usage record for which no rate resolves (neither exact nor family) is priced as **unknown**, not as zero. An unknown-cost call is counted and surfaced as unknown-cost; it never contributes a silent zero that under-reports true spend and corrupts every aggregate built on it. Downstream, an aggregate that includes unknown-cost calls discloses that it is a lower bound, not a complete total. (The cost-side of the fail-visible discipline — `l1-optimization-integrity` OI-8.)

- **CR-5 (Estimate vs authoritative, labeled — authoritative supersedes):** a cost derived locally from `rate × usage` is an **estimate**; where the provider reports the actual billed amount (or authoritative usage-and-price), that figure supersedes the estimate for the same call. Every reported cost carries its basis (`estimated` | `provider-reported`). An estimate is never presented as a billed fact, and an authoritative report is never overwritten by a later re-estimate.

- **CR-6 (Dated, sourced, staleness-flagged rates):** every rate carries provenance — its source (provider list price, negotiated/contract rate, subscription-effective rate, or local default) and an as-of date. A rate older than a configured horizon, or one whose source is a local default rather than a confirmed provider price, degrades to *indicative*, is flagged for refresh, and is never silently treated as current authoritative pricing. (Parallels `l1-model-benchmarking` MB-7 profile staleness.)

- **CR-7 (Currency explicit and native amount immutable):** every monetary figure carries its currency, and the recorded amount is stored in one canonical currency. A conversion to a display currency is an explicit, dated, rate-sourced transform that never mutates the recorded native amount; presentation may convert (provenance-marked), but the ledger figure is not re-denominated in place. A hardcoded, undated, unsourced conversion factor is forbidden.

- **CR-8 (Rating is observable and auditable):** each priced record retains the inputs to its price — the resolved rate (with provenance and basis), the per-class unit counts, and the resulting per-class and total cost — so a price is reconstructable and a mispricing (wrong rate, stale card, blended-rate regression, silent-zero leak) is detectable from the record itself, not only from the provider invoice. Feeds `l1-operational-health` OH-6 and the `l1-optimization-integrity` OI-4 efficacy accounting.

> L2 specs cannot reach RFC status until all invariants here are addressed in their "Invariant Compliance" section.

## 4. Detailed Design

### 4.1 The cost pipeline: meter → rate → charge

| Step | Question | Owner |
| --- | --- | --- |
| Meter | how many units of each class did this call use? | usage accounting / `l1-nodus-observability` HO-8 / CSC-11 |
| **Rate** | **what does that usage cost, honestly?** | **this spec** |
| Charge | is that spend within the budget? | `l2-budget-engine` |

Rating is the missing middle: metering already separates the classes and charging
already enforces the limit, but the honest conversion between them was assumed, not
owned.

### 4.2 Per-class rating with fail-visible resolution

```text
[REFERENCE]
rate_call(model_id, usage):                      // usage = per-class unit counts (CR-2)
    card := resolve_rate(model_id)                // CR-3
    if card is UNKNOWN:
        return { cost: UNKNOWN, basis: "unpriced", model: model_id }   // CR-4 — NOT 0
    cost := 0; breakdown := {}
    for class in usage.classes:                    // input, output, cache_read, cache_creation, image, ...
        c := usage[class] * card.rate(class)        // each class at ITS OWN rate — never a blend
        breakdown[class] := c; cost += c
    return { cost, currency: card.currency, basis: "estimated",         // CR-5 default
             rate_provenance: card.source, rate_as_of: card.as_of,      // CR-6
             breakdown }                                                 // CR-8 reconstructable

resolve_rate(model_id):                           // CR-3
    if exact := card_lookup(model_id):  return exact
    if family := card_lookup(base_of(model_id)):  return family.as_inferred()   // marked inferred
    return UNKNOWN                                 // CR-4 — never a substitute model's rate
```

Two properties keep it honest: an unpriced model yields `UNKNOWN`, which propagates as a
disclosed lower-bound through every aggregate (CR-4), and a family-inferred rate is
marked so a downstream reader never mistakes it for the model's exact published price
(CR-3).

### 4.3 Estimate superseded by authoritative

```text
[REFERENCE]
finalize_cost(call):
    est := rate_call(call.model, call.usage)                    // basis: "estimated"
    if call.provider_reported_amount is present:                 // CR-5
        return { cost: call.provider_reported_amount, basis: "provider-reported", ... }
    return est                                                    // estimate stands, labeled
// an authoritative amount, once recorded, is never overwritten by a re-estimate
```

### 4.4 Where rated cost flows

A priced record is a producer; it never enforces or egresses. It feeds: `l2-budget-engine`
(threshold enforcement, roll-up), `l1-usage-allowance` (cost-denominated windows),
`l1-dashboard` (priced aggregates), `l1-routing` (cost signal), `l1-operational-health`
OH-6 (cost accounting + mispricing alerts, CR-8), and `l1-optimization-integrity` OI-4
(per-stage efficacy). Egress only via `l1-telemetry`, opt-in.

## nodus-relevance mapping

The workflow runtime already meters per-class token counts on `model_response`
(`l1-nodus-observability` HO-8, `counts-only within the data-safety boundary`) — it
deliberately does not price them. Rating is a **host concern via a provider seam**
(nodus LP-1/LP-2), so it needs **no new language invariant**:

| Element | nodus seam | Note |
| --- | --- | --- |
| Rate card + resolution (CR-1/CR-3) | host-supplied pricing seam (nodus LP-2), alongside `SchemaProvider` / `StorageProvider` / `PolicyProvider` | Rates are deployment data the host owns; core carries no rate table and no currency logic. |
| Per-class rating (CR-2) | over HO-8 `input`/`output`/`cache_read`/`cache_creation` classes | The count-separation HO-8 already provides is exactly what per-class rating consumes; core changes nothing. |
| Fail-visible unknown (CR-4) | host rates the counts; an unpriced model yields an `UNKNOWN`-cost audit annotation, never a silent 0 | A host that prices nothing simply reports HO-8 counts with no monetary figure — today's exact behavior (additive). |
| Estimate vs authoritative (CR-5) | `AuditProvider` cost annotation basis field | Rides the existing observability channel; no new event type. |

nodus's contribution stays *record which classes were used*; the host rates them. A host
that supplies no pricing seam behaves exactly as today — HO-8 counts, no cost — so the
mapping is additive and warrants no new NL invariant.

## 5. Drawbacks & Alternatives

- **Rate-card maintenance burden.** Prices change and models churn, so the card goes stale. Accepted and made honest rather than hidden: CR-6 dates and sources every rate and degrades a stale one to *indicative* with a refresh flag, so the burden is visible instead of silently mispricing.
- **Estimate imprecision.** A local `rate × tokens` estimate can differ from the billed amount (minimums, tiers, rounding, discounts). CR-5 labels it `estimated` and lets an authoritative provider amount supersede it — the estimate is honest about being one, never dressed as a bill.
- **Alternative — leave pricing inside `l2-budget-engine`.** Rejected: the budget engine *enforces* against a number it is given; making it also *derive* that number would bury the rate card, the per-class rule, and the fail-visible/estimate-honesty guarantees inside one enforcement path, where the router, dashboard, benchmarking, and the workflow runtime — all of which also need a priced number — could not cite them. Rating is cross-cutting; it is a shared contract, not the budget engine's private detail (the same reason cache stability became its own L1 rather than a compression detail).
- **Alternative — trust the provider's reported cost only.** Rejected: many local and self-hosted models report no price, streaming/tool calls often expose only token counts, and a routing or pre-flight decision needs a cost *estimate before* the call — so a local rate card is required. CR-5 keeps the provider's authoritative figure supreme *when present* without making the system blind when it is absent.
- **Alternative — price unknown models at zero and move on.** Rejected outright: this is the exact silent-underreporting defect CR-4 exists to forbid. An unknown cost is `UNKNOWN`, disclosed as a lower bound — never a zero that quietly corrupts the total.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[BUDGET]` | `.design/main/specifications/l2-budget-engine.md` | The enforcement/roll-up consumer of the priced cost events this layer produces. |
| `[ALLOWANCE]` | `.design/main/specifications/l1-usage-allowance.md` | Cost-denominated windows; UA-2 source-honesty is the allowance parallel of CR-5. |
| `[CACHE-STABLE]` | `.design/main/specifications/l1-cache-stable-context.md` | CSC-11 separated token-class counts that CR-2 prices each at its own rate. |
| `[BENCHMARKING]` | `.design/main/specifications/l1-model-benchmarking.md` | MB-3 `derived_cost from catalog pricing` consumer; MB-7 staleness parallels CR-6. |
| `[OPT-INTEGRITY]` | `.design/main/specifications/l1-optimization-integrity.md` | OI-4 efficacy accounting and OI-8 fail-visible discipline CR-4/CR-5 supply the cost-side of. |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.1.1 | 2026-09-24 | Core Team | Consistency pass (2026-09-24): Bare `LP-n` citations of the nodus portability contract are now written `nodus LP-n`: this workspace's `l1-lookahead-planning` defines LP-1…LP-6 as well, so the bare form pointed a reader at the wrong invariant. No requirement changed. |
| 1.1.0 | 2026-08-06 | Core Team | CR-2 amended — the per-class rate list gains **generation that is billed but never delivered** (a deliberating model's internal reasoning, produced under the same call as the answer and frequently larger than it). It is a class in its own right, not a variant of `output`: unreadable, not amortized by prefix reuse, governed by a depth dial rather than a length target, and unreported by some providers. Pricing it as `output` makes a bill that moved because deliberation deepened indistinguishable from one that moved because answers got longer. Where the count is unavailable the class is **unmeasured** and containing aggregates are declared lower bounds — the count-side twin of CR-4's fail-visible unknown *rate*. Full contract in the new `l1-reasoning-spend`. |
| 1.0.1 | 2026-08-05 | Core Team | Related Specifications extended with `l1-outcome-attributed-cost` — the consumer that attributes priced records to the outputs they produced and to a later survival verdict, closing the loop this layer deliberately leaves open. Link-only; no invariant changed. |
| 1.0.0 | 2026-07-15 | Core Team | Initial spec — cost rating as the owned middle step (meter → **rate** → charge) between usage metering and budget enforcement: single-producer derivation distinct from metering and charging (CR-1), per-class rates never one blended rate composing CSC-11/HO-8 class separation (CR-2), honest exact-then-family-marked rate resolution (CR-3), fail-visible unknown rate never a silent zero — the cost-side of OI-8 (CR-4), estimate-vs-provider-reported basis labeling with authoritative supersession (CR-5), dated/sourced/staleness-flagged rates paralleling MB-7 (CR-6), explicit currency with immutable native amount and provenance-marked conversion (CR-7), reconstructable/auditable priced record feeding OH-6 + OI-4 (CR-8); §4.2 fail-visible per-class rating pseudocode, §4.3 estimate-superseded-by-authoritative; nodus-relevance mapping needing no new NL invariant — HO-8 already meters per-class counts `counts-only`, the host rates them via an LP-2 pricing seam (additive; a host that prices nothing behaves as today). Owns the `catalog pricing` that l1-model-benchmarking MB-3, l2-dashboard, l2-model-router, and l2-deep-research each assumed without a governing contract. Distilled from an adoption pass over an external LLM cost-tracking reference whose per-call token/latency/success accounting, provider breakdown, and budget-alert surface were already realized by l2-budget-engine / l1-usage-allowance / l1-operational-health / l1-dashboard — CR captures the one unowned delta: the honest rating layer, with the reference's silent-zero-on-unknown-model and blended/hardcoded-currency assumptions inverted into fail-visible invariants. |
