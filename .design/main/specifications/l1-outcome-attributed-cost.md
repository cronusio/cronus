# Outcome-Attributed Cost

**Version:** 1.0.1
**Status:** Stable
**Layer:** concept

## Overview

The cost family answers *what was spent*: metering counts the units, rating turns them
into money, the allowance model says how much of the window is left, and the budget engine
enforces a ceiling. Every one of those is about the **outflow**.

None of them answers the question a principal actually asks: **what did the money produce,
and did it last?**

Today a session that cost a lot and ended in a merged change and a session that cost the
same and ended in work that was reverted the next morning are *indistinguishable in every
readout the system has*. The totals match, the token counts match, the model mix matches.
The only difference is the one that matters.

**Outcome-attributed cost** closes that loop: every unit of accounted spend is attributed
to the output it contributed to — or explicitly to none — and each output carries a later,
separately-observed verdict on whether it **survived**. Two properties make the result
trustworthy rather than merely suggestive. Survival is a *later* fact, never inferred at
production time; and the outcome vocabulary distinguishes work that was **discarded by
design** — the losing branches of a deliberate best-of-N — from work that was **reverted**,
because an office that runs competitive execution and cannot tell those apart will read its
own working strategy as its largest source of waste.

## Related Specifications

- [l1-cost-rating.md](l1-cost-rating.md) — the upstream producer: rating derives the monetary figure this layer attributes. CRT-5's estimate-vs-authoritative labeling and CRT-8's auditable inputs travel through attribution unchanged; nothing here re-prices anything.
- [l1-usage-allowance.md](l1-usage-allowance.md) — the forward-looking counterpart: UA governs *how much is left* and UA-8 the order in which capability sheds; this governs *what the spend bought*, retrospectively. Neither gates the other (OAC-9).
- [l1-competitive-execution.md](l1-competitive-execution.md) — the invariant that makes the vocabulary necessary: best-of-N deliberately discards N−1 attempts, and those attempts are **discarded-by-design**, never *reverted* (OAC-3). Conflating them prices a working mechanism as waste.
- [l1-version-control.md](l1-version-control.md) — VC-3's card-aligned commit boundary is what makes attribution reconstructable at all: a commit that maps to one work unit is the join between spend and produced artifact (OAC-5).
- [l1-change-attribution.md](l1-change-attribution.md) — **different question, adjacent name**: that spec asks *what moved together with a regression* (statistical blame); this asks *what did this spend produce, and did it hold*. No overlap in mechanism.
- [l1-practice-analytics.md](l1-practice-analytics.md) — the natural consumer: PA-11 findings become materially more actionable when a habit carries the money it costs, and PA-15's prioritized plan is ordered by exactly this figure.
- [l1-artifact-derived-observation.md](l1-artifact-derived-observation.md) — how outcomes are observed for work the office did **not** run; the coverage figure ADO-11 requires is what keeps a cross-tool cost-per-outcome honest.
- [l1-operational-ledger.md](l1-operational-ledger.md) — the record attribution is reconstructed *from* (OAC-5), never a parallel bookkeeping kept beside it.
- [l1-value-settlement.md](l1-value-settlement.md) — **distinct concern**: settlement pays an outside counterparty for value received; this measures what the office's own spend produced. Same word "value", opposite direction.
- [l1-user-model.md](l1-user-model.md) — UM-8's service-not-manipulation boundary is the principal that OAC-7 applies to a measure whose obvious misuse is an individual productivity score.
- [l1-workflow-language.md](l1-workflow-language.md) — the nodus projection (§4.5): the run record already carries cost and receipt annotations; the delta is a declared outcome slot and a later survival verdict, both host-side.

## 1. Motivation

**Spend without an outcome link cannot answer the only question worth asking.** "We spent
X this week" invites exactly one follow-up, and the system has no answer to it. Worse, the
absence of the answer defaults to the wrong one: with no outcome dimension, cheap looks
good and expensive looks bad, when the expensive run that shipped is the one that was
worth it.

**Production and survival are different events, and the gap between them is where the
information is.** An artifact exists the moment it is produced; whether it *lasts* is only
knowable later — after review, after a merge, after the morning someone reverts it. A
system that records only production has recorded the cheerful half of the story, and it
records it at exactly the moment when the pessimistic half is unknowable.

**Competitive execution makes naïve waste accounting actively wrong.** An office that runs
several attempts and keeps the best one *discards most of what it produces on purpose*.
Measured without a vocabulary for that, its deliberate strategy shows up as its largest
waste category — and the natural correction is to stop doing the thing that was working.

**"Cost with nothing shipped" is a real category, not an error.** Research, exploration,
reading, a dead end that ruled something out — all legitimately produce no durable
artifact. The category must exist, be counted, and carry **no implicit verdict**, because
the same number means opposite things for an exploration turn and for the tenth attempt at
one bug fix.

**The measure has an obvious misuse.** Cost-per-shipped-change is one small step from a
per-person productivity ranking, and that step destroys the measure: once it scores people,
it is gamed, and a gamed measure is worse than no measure because it still looks like
evidence.

## 2. Constraints & Assumptions

- This layer **derives**; it meters nothing and prices nothing. It consumes priced records
  and existing work/change history.
- "Outcome" means a **durable produced artifact** — a change, a document, a delivered
  answer — not a subjective assessment of quality.
- Survival is observed over a **declared window**; inside it the verdict is *unresolved*,
  and unresolved is a real state rather than a pessimistic default.
- The office is **local-first**: attribution runs on-device over records already kept, and
  its outputs follow the existing consent gate like any other derived analytic.
- The measure is **retrospective**. Nothing here participates in choosing a model, a route,
  or a budget in flight.

## 3. Core Invariants

Rules every Layer 2 implementation MUST NOT violate:

- **OAC-1 Spend is attributed to a produced output, or explicitly to none**: every unit of
  accounted spend carries an attribution to the durable output it contributed to, or is
  explicitly classified **unattributed**. There is no third, implicit state. A large
  unattributed share is not a gap in the report — it *is* the finding, and it is reported
  as one rather than quietly excluded from the denominator.

- **OAC-2 Survival is a later, separately-observed fact**: whether a produced output
  **lasted** is determined by an observation made *after* production, over a **declared
  window**, and MUST NOT be inferred at production time. Until the window closes the
  verdict is **unresolved** — an honest state that is neither optimistic nor pessimistic —
  and a system that marks work "shipped" the moment it is produced has recorded an
  intention as an outcome.

- **OAC-3 A closed outcome vocabulary that separates design from failure**: an output's
  verdict comes from a closed set — **kept** (survived the window) · **superseded**
  (replaced by later work, its contribution absorbed) · **reverted** (undone as
  unsatisfactory) · **abandoned** (never completed or delivered) · **discarded-by-design**
  (the losing branches of a deliberate best-of-N or an explicitly exploratory attempt) ·
  **unresolved** (window open). *discarded-by-design* MUST NOT be recorded as *reverted*:
  the first is the declared price of a strategy, the second is a failure of the work, and
  an office that runs competitive execution will misread its own mechanism as its biggest
  waste if the two are fused.

- **OAC-4 Spend with no surviving output is a measured category carrying no implicit
  verdict**: work that produces nothing durable is counted, trended, and reported — and is
  **never** labeled waste by the measure itself. Exploration, reading, and ruling something
  out legitimately produce no artifact. The number's meaning depends entirely on the kind
  of work it describes, so the measure supplies the number and the classification, and
  leaves the judgement to a reader who has the context.

- **OAC-5 Attribution is reconstructed from existing records, never a parallel ledger**:
  the link between spend and output is derived from what the system already keeps — the
  operational ledger, the work item, the change history, the action receipts. A second,
  separately-maintained mapping is forbidden: it drifts from the records it shadows, and a
  drifted attribution is worse than none because it still produces confident numbers.

- **OAC-6 Ambiguous and shared attribution is expressed, never silently resolved**: where
  spend contributed to several outputs, or where the link is uncertain, the record carries
  a **shared or explicitly-uncertain** attribution — never a single confident owner chosen
  by an undeclared tiebreak. The system may distribute a cost across outputs by a **declared
  rule**; it may not pick one and present it as the answer.

- **OAC-7 A per-unit figure is reported with its denominator and its dispersion**: "cost
  per kept change" is meaningless without the count it divides by and the spread behind it
  — a single outlier moves a mean that then misinforms a decision. Central tendency plus
  spread plus n, or the figure is not reported. Minimum-sample abstention applies: below a
  declared count, the honest output is *insufficient data*, not a noisy number.

- **OAC-8 Never a verdict on a person**: the measure describes **work and the system that
  produced it**, and MUST NOT be presented, aggregated, or used as an individual
  performance or productivity score. This is a hard boundary, not a default: a measure that
  scores people is gamed, and a gamed measure is worse than none because it still carries
  the authority of a number.

- **OAC-9 Retrospective, never an in-flight gate**: outcome-attributed cost informs
  decisions **after** the fact. It MUST NOT gate, route, or veto an action in progress —
  routing and the budget make those calls on their own inputs. Wiring a survival statistic
  into live selection creates a loop in which the measure shapes the behaviour it is
  measuring, after which it measures nothing.

- **OAC-10 Counterfactual honesty — no fabricated savings**: a claim that a change reduced
  cost is stated against a **declared baseline or held-out comparison**, or it is labeled
  an estimate with its basis named. The un-run alternative was never observed and MUST NOT
  be subtracted from as though it were. The only honest unconditional figures are the ones
  actually counted.

> L2 specs cannot reach RFC status until all invariants here are addressed in their "Invariant Compliance" section.

## 4. Detailed Design

### 4.1 The two-phase record

```text
[REFERENCE]
// phase 1 — at production: what was spent, and on what
Attribution {
  spend_ref   : priced-record id(s)          // OAC-1, from the rating layer, never re-priced
  output_ref  : produced-artifact id | NONE  // NONE is explicit, never implied
  basis       : "direct" | "shared" | "uncertain"   // OAC-6
  share?      : distribution over outputs     // present iff basis = shared, by a declared rule
}

// phase 2 — after the declared window: did it last
Survival {
  output_ref  : produced-artifact id
  verdict     : kept | superseded | reverted | abandoned | discarded_by_design | unresolved
  observed_at : instant                       // OAC-2 — strictly later than production
  window      : declared duration             // what "lasted" was measured over
}
```

The two are separate records on purpose. Fusing them forces a verdict at production time,
which is the one moment it cannot honestly be given.

### 4.2 The outcome vocabulary (OAC-3)

| Verdict | Means | Reads as |
| --- | --- | --- |
| **kept** | survived the declared window | the spend produced something that held |
| **superseded** | replaced by later work that absorbed its contribution | not waste — a step that was built on |
| **reverted** | deliberately undone as unsatisfactory | the failure case; the one worth investigating |
| **abandoned** | never completed or delivered | often a stopped-for-good-reason case; needs context |
| **discarded-by-design** | a losing branch of a declared best-of-N or exploration | **the price of a strategy, not a defect** |
| **unresolved** | the observation window is still open | not yet knowable; never defaulted either way |

The two rows that carry the whole design are *discarded-by-design* and *superseded*. Both
describe work that does not survive as itself and is nonetheless not waste, and both are
what a naïve "did it ship?" boolean would misfile as failure.

### 4.3 Where the join comes from (OAC-5)

```text
[REFERENCE]
attribute(spend_record):
    unit   := work_item_of(spend_record)            // the ledger already knows this
    output := durable_outputs_of(unit)              // change history, receipts, delivered artifacts
    match len(output):
        0 -> (NONE, direct)                          // OAC-1 explicit none; OAC-4 category
        1 -> (output[0], direct)
        n -> distribute(spend_record, output, declared_rule)   // OAC-6 shared, never a silent pick
```

Every input is a record the system keeps for its own reasons. That is the constraint that
makes the measure durable: nothing here needs a human to maintain a mapping, so nothing
here can silently rot into a confident lie.

### 4.4 Reading the numbers honestly

Three readings the measure must make possible, and one it must make impossible:

- *Where did the money go?* — spend by outcome verdict, with the unattributed share visible.
- *What does a surviving unit of work cost?* — a per-unit figure with n and spread (OAC-7).
- *Is the no-durable-output share drifting?* — trended by work kind, uninterpreted (OAC-4).
- *Who is expensive?* — **not answerable by construction** (OAC-8); the measure carries no
  person dimension, so the question has nowhere to attach rather than merely being
  discouraged.

### 4.5 nodus projection

No new language primitive is needed:

1. **The run record already carries the cost half.** Cost, receipt, and lineage annotations
   ride the run's observability record; attribution adds a declared **outcome slot** to that
   record and a later survival verdict attached to the same run identity — host-side,
   additive, and absent by default.
2. **Competitive selection already knows its own discards.** A parallel block with a
   selection discipline binds one winning branch and drops the rest *by declaration*, so the
   losing branches are labeled **discarded-by-design** at the source rather than
   reconstructed later — the language's own construct supplies the distinction OAC-3
   depends on.
3. **Survival is an external, later fact, so it arrives like any other.** The runtime
   already supports a step whose result arrives after the run suspends; a survival verdict
   observed days later is the same shape — a correlated later completion — and needs no new
   mechanism.

## 5. Implementation Notes

1. Record the attribution at production time and the survival verdict on its own schedule;
   a single write that tries to do both will end up guessing the second (OAC-2).
2. Derive the outcome vocabulary's *discarded-by-design* from the mechanism that discarded
   the branch, never from a heuristic over the artifact — the mechanism knows, a classifier
   guesses.
3. Keep the unattributed share on the primary readout, not behind a detail view: it is the
   figure that tells a reader how much to trust everything beside it.
4. The person dimension should be **structurally absent** from the record (OAC-8), not
   filtered at presentation — a field that exists will eventually be grouped by.

## 6. Drawbacks & Alternatives

- **Attribution is imperfect and always will be.** Accepted, and OAC-1/OAC-6 make the
  imperfection visible (explicit *none*, explicit *uncertain*) rather than hiding it in a
  clean-looking total.
- **The survival window delays the answer.** Intended: the delay is the honesty. OAC-2's
  *unresolved* is what a system says while it does not yet know, and a faster answer would
  be an invented one.
- **A cost-per-outcome number invites exactly the misuse OAC-8 forbids.** Real, and the
  reason the boundary is an invariant with a structural remedy (§5.4) rather than a warning
  in a footnote.
- **Alternative — a simple "did it ship?" boolean.** Rejected by OAC-3: it misfiles
  *superseded* and *discarded-by-design* as failure, which is precisely wrong for an office
  that builds iteratively and runs competitive execution.
- **Alternative — fold into the practice-analytics detectors.** Rejected: those detect
  *patterns in traces*; this is an accounting relation between two record families. Analytics
  is the natural **consumer** (it gains a money dimension), not the owner.
- **Alternative — fold into cost rating.** Rejected: rating's job ends when the money is
  derived, and it deliberately knows nothing about work items or outcomes. Mixing them would
  make the pricing layer depend on the change history.
- **Alternative — wire the measure into routing so cheap-and-surviving paths are preferred.**
  Rejected by OAC-9: it closes a loop in which the measure shapes what it measures, and the
  statistic stops describing anything.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[RATING]` | `.design/main/specifications/l1-cost-rating.md` | The upstream producer of the priced records attributed here. |
| `[ALLOWANCE]` | `.design/main/specifications/l1-usage-allowance.md` | The forward-looking counterpart; UA-8 shedding order. |
| `[COMPETITIVE]` | `.design/main/specifications/l1-competitive-execution.md` | Source of the discarded-by-design verdict (OAC-3). |
| `[VCS]` | `.design/main/specifications/l1-version-control.md` | VC-3 commit boundary — the join that makes attribution reconstructable. |
| `[LEDGER]` | `.design/main/specifications/l1-operational-ledger.md` | The existing records attribution derives from (OAC-5). |
| `[ANALYTICS]` | `.design/main/specifications/l1-practice-analytics.md` | The consuming layer; PA-15 ordering gains a money dimension. |
| `[OBSERVATION]` | `.design/main/specifications/l1-artifact-derived-observation.md` | How outcomes are observed for work the office did not run. |
| `[WORKFLOW-LANG]` | `.design/main/specifications/l1-workflow-language.md` | The nodus surface the discipline projects onto (§4.5). |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.0 | 2026-08-05 | Core Team | Initial spec — outcome-attributed cost as the closing half of the cost family, which measured outflow (meter → rate → allowance → enforce) and could not distinguish an expensive session that shipped from an equally expensive one reverted the next morning: spend attributed to a produced output or explicitly to none, with a large unattributed share being the finding rather than a gap (OAC-1); survival observed **later**, over a declared window, never inferred at production time, with *unresolved* an honest state rather than a default (OAC-2); a closed outcome vocabulary — kept / superseded / reverted / abandoned / **discarded-by-design** / unresolved — whose load-bearing separation is that a deliberate best-of-N's losing branches are the declared price of a strategy and not a failure, so an office running competitive execution does not read its own working mechanism as its largest waste (OAC-3); spend-with-no-durable-output measured as a first-class category carrying **no implicit verdict**, since the same number means opposite things for exploration and for a tenth attempt at one bug (OAC-4); attribution reconstructed from records already kept, never a parallel ledger that drifts into confident lies (OAC-5); shared and uncertain attribution expressed rather than resolved by an undeclared tiebreak (OAC-6); per-unit figures reported with denominator, dispersion, and minimum-sample abstention (OAC-7); never a verdict on a person, structurally rather than by convention, because a measure that scores people is gamed and a gamed measure still carries a number's authority (OAC-8); retrospective and never an in-flight gate, since wiring survival statistics into live selection closes a loop in which the measure shapes what it measures (OAC-9); and counterfactual honesty with no fabricated savings (OAC-10). Nodus projection needs no new primitive — the run record carries the cost half and gains a declared outcome slot, competitive selection labels its own discards at the source, and a later survival verdict arrives through the existing correlated-later-completion path. Concept-only. |
| 1.0.1 | 2026-10-22 | Core Team | Citations updated for the requirement-ID prefix renames that make each prefix name one specification (RULES §1, 1.8.0, the person's decision of 2026-10-22): `CR-n` of `l1-cost-rating` is now `CRT-n`. No requirement was added, removed or reworded. |
