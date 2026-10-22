# Fidelity Variants

**Version:** 1.0.1
**Status:** Stable
**Layer:** concept

## Overview

The runtime already knows how to trade **speed** for **capacity**: when a model does not fit
the fastest placement, a named degradation tier keeps it running more slowly, and that tier
is required to change speed and **never outputs**. That invariant draws a line and then
points across it — a configuration that *does* alter results is explicitly declared to be
"a different configuration", not a degradation, and is handed off to the recipe.

Nothing on the other side of that line has an owner. And the other side is where the most
consequential everyday choices live: a smaller or more compressed model, a shorter
generation, a coarser retrieval pass, a lower rendering resolution, a summarizer running at
a shallower depth. Each is the **same capability at a lower cost and a different output**.

**Fidelity variants** is the discipline for that axis. A capability that exists at several
fidelity points declares them as an ordered set with their resource profiles and their
*measured* quality effects; selection is explicit or policy-driven and never opportunistic;
the variant that produced an artifact travels **with** the artifact; measurements compare
only within a variant; and each capability declares the floor below which the honest answer
is a refusal rather than a cheap result.

The failure this prevents is quiet and expensive: a result produced at a lower fidelity,
unlabelled, compared against results produced at a higher one — after which every
benchmark, every regression judgement, and every cost-per-outcome figure is measuring two
different products and reporting one number.

## Related Specifications

- [l1-model-runtime.md](l1-model-runtime.md) — the **boundary this spec was invited across**: MR-15's degradation tier trades throughput for capacity and MUST NOT alter outputs, explicitly classifying a result-altering placement as *a different configuration*. This spec owns that other side. MR-7's feasibility gate and MR-16's running feasibility are what *force* a variant choice; FV governs how the choice is declared, made, recorded, and measured.
- [l1-competitive-execution.md](l1-competitive-execution.md) — CPE-10 is the **worked special case**: run the N attempts at a reduced declared fidelity to *select*, then re-produce the winner once at full fidelity. FV-3 and FV-9 supply the general rules that case obeys; this spec does not restate it.
- [l1-model-benchmarking.md](l1-model-benchmarking.md) — MB-5 already folds configuration (including quantization) into the identity of the model under test; FV-5 generalizes that comparability rule from *model benchmarks* to **every** measurement over a variant-bearing capability.
- [l1-agent-coevaluation.md](l1-agent-coevaluation.md) — ACE-5's frozen comparability contract is the same instinct; ACE-4's per-slice reporting is why FV-8 requires the quality effect to be sliced rather than averaged.
- [l1-reproduction-recipe.md](l1-reproduction-recipe.md) — RR-2's ambient layer is where the selected variant lands (FV-4); an artifact whose recipe omits its fidelity cannot be re-derived and cannot be honestly compared.
- [l1-outcome-attributed-cost.md](l1-outcome-attributed-cost.md) — OAC-7's per-unit figures are meaningless across mixed variants: cheap outputs and full-fidelity outputs in one denominator produce a cost-per-outcome number that describes neither.
- [l1-usage-allowance.md](l1-usage-allowance.md) — UA-8 sheds *capability* under budget pressure in a declared order; FV supplies the finer move available before shedding — the same capability at a lower point on its own axis, with FV-6's floor bounding how far that can go.
- [l1-generation-shaping.md](l1-generation-shaping.md) — GSH-4's correctness floor is the ancestor of FV-6: shaping may never trade correctness for brevity, and a fidelity variant may never fall below fit-for-purpose.
- [l1-outcome-confidence.md](l1-outcome-confidence.md) — a lower variant legitimately lowers confidence in the outcome; the variant is one of the contributors an honest estimate reads.
- [l1-workflow-language.md](l1-workflow-language.md) — the nodus projection (§4.6): the declared budget measure and the run record already carry what a variant needs; the config surface declares the choice.

## 1. Motivation

**Unlabelled fidelity contaminates every measurement downstream.** A benchmark, an
evaluation suite, a regression check, and a cost-per-outcome figure all assume their inputs
are comparable. One artifact produced at a lower fidelity, unrecorded, silently turns each
of those into a comparison between two products — and the resulting number looks exactly
like a valid one.

**Opportunistic substitution is invisible by construction.** A system that quietly drops to
a cheaper variant when memory is tight, or when a lane is busy, produces two consecutive
results whose difference has no recorded cause. The user reports "it got worse"; nothing in
the record explains why, because the thing that changed was never treated as a change.

**"Slightly worse" is an assumption, and it is often wrong in a specific place.** Lower
fidelity rarely degrades uniformly. It degrades a *slice* — long inputs, one language, one
category, the edge cases — while the average barely moves. A single aggregate quality delta
hides exactly the failure the variant will actually produce.

**Without a floor, degradation has no stopping point.** Under enough resource pressure, a
chain of individually reasonable reductions arrives at an output that is cheap and useless.
The honest answer at that point is a refusal, and refusal only exists as an option if
someone declared where the floor is.

**Silent upgrades are substitutions too.** Producing at a higher fidelity than requested
changes cost and can change results. A user who selected the cheap variant selected it, and
a system that "helpfully" improves on that decision has spent their budget on a choice they
did not make.

## 2. Constraints & Assumptions

- A variant axis exists only where the **same capability** genuinely has multiple operating
  points. A different capability is a different capability, not a variant of one.
- The axis is **ordered**: variants are comparable as higher/lower on cost and fidelity.
  Choices that are merely *different* (a different style, a different algorithm with its own
  strengths) are configurations, not variants.
- Variants are assumed to affect **output**. Anything that preserves outputs exactly is a
  placement or performance concern and belongs to the runtime's degradation tiers.
- The concept defines no new store and no new selector: variants ride the existing
  definition, the existing recipe, and the existing policy surface.
- Measuring a variant's quality effect requires an evaluation instrument; where none exists,
  FV-8's *declared unknown* is the honest state rather than an assumed small delta.

## 3. Core Invariants

Rules every Layer 2 implementation MUST NOT violate:

- **FV-1 A fidelity variant changes the output; a placement tier does not — and the two are
  never conflated**: reducing throughput to fit hardware while producing an identical result
  is a **placement** concern. Producing a *different, generally lesser* result at lower cost
  is a **fidelity** concern. Presenting a fidelity choice as a performance setting is how a
  quality regression enters a system with nobody accountable for it: it arrives through the
  door reserved for changes that were promised not to alter anything.

- **FV-2 The variant set is declared, ordered, and profiled**: a capability offering variants
  declares them as an **ordered set on a named axis**, each carrying its resource profile
  (what it costs) and its quality effect (what it gives up). An undeclared "fast mode" is a
  second product shipped without a name, and no consumer can reason about a set they cannot
  enumerate.

- **FV-3 Selection is explicit or policy-driven, never opportunistic**: the variant in force
  is chosen by a **declared rule** — a user's choice, a stated policy, or a feasibility gate
  — and never swapped silently per request to fit whatever is convenient at that instant.
  Opportunistic substitution produces consecutive results whose difference has no recorded
  cause, which is indistinguishable to the user from the system becoming unreliable.

- **FV-4 The variant travels with the artifact**: the fidelity that produced an artifact is
  recorded **on** it, as a recipe field rather than an operational log line. An artifact whose
  fidelity is unrecorded cannot be re-derived, cannot be honestly compared, and will
  eventually be compared anyway.

- **FV-5 Measurement is within-variant**: any comparison — a benchmark, an evaluation, a
  regression check, an A/B, a cost-per-outcome figure — is valid only across runs at the
  **same** variant, and a mixed-variant comparison is reported as invalid rather than
  averaged. The model-benchmarking discipline already folds configuration into the identity
  of the thing under test; this generalizes that rule to every variant-bearing capability and
  every instrument that measures one.

- **FV-6 A declared floor, below which the answer is refusal**: each capability declares the
  **lowest variant still fit for its purpose**. Resource pressure may walk the axis down to
  that floor and no further; below it the honest response is a stated refusal with the
  shortfall, never a cheaper result presented as an answer. Without a declared floor, a chain
  of individually reasonable reductions terminates in output that is cheap and useless, and
  nothing in the system objects.

- **FV-7 No silent upgrade either**: producing at a **higher** fidelity than selected is also
  a substitution — it spends budget the requester did not authorize and can change results
  that were expected to be stable. A choice of the cheap variant is a choice, and improving on
  it without saying so is the same defect as degrading without saying so, pointed the other
  way.

- **FV-8 The quality effect is measured and sliced, never assumed monotone**: each variant
  carries a **measured** quality delta against the reference variant, reported **per slice**
  rather than as one average, because lower fidelity typically degrades a specific slice
  severely while barely moving the mean. Where no measurement exists, the effect is declared
  **unknown** — an unknown delta is a usable fact, an assumed small one is not.

- **FV-9 A named reference variant anchors the axis**: exactly one variant is the
  **reference** — the default in the absence of policy, and the baseline every other
  variant's delta is stated against. Without a named reference, "the fast one" and "the good
  one" drift into unrelated products with no common yardstick, and FV-5 loses the thing it
  compares to.

> L2 specs cannot reach RFC status until all invariants here are addressed in their "Invariant Compliance" section.

## 4. Detailed Design

### 4.1 The two axes, kept apart (FV-1)

| | Placement / degradation tier | Fidelity variant |
| --- | --- | --- |
| Trades | speed, memory locality | output quality |
| Output | **identical** | **different** |
| Honest label | a performance setting | a different product on one axis |
| Recorded as | an operational note on the run | a **recipe field** on the artifact |
| Comparable across? | yes | **no** (FV-5) |

The rows in bold are the whole reason for the separation. A system that files a fidelity
choice in the left column has recorded a quality change as an operational detail, and the
next person to compare two results will have no reason to suspect anything.

### 4.2 The variant declaration

```text
[REFERENCE]
VariantAxis {
  capability   : what this is a variant of
  reference    : the variant deltas are stated against          // FV-9
  floor        : lowest variant still fit for purpose           // FV-6
  variants     : ordered [ { id, resource_profile,
                             quality_delta: per-slice | UNKNOWN } ]   // FV-2, FV-8
}
```

`quality_delta: UNKNOWN` is a first-class value, not an omission. A variant whose effect
nobody measured is usable — with its uncertainty visible — and that is strictly better than
the alternative the omission invites, which is everyone assuming the effect is small.

### 4.3 Selection and the floor (FV-3 / FV-6)

```text
[REFERENCE]
select(capability, constraints):
    v := policy_or_user_choice(capability)      // FV-3 — declared, never opportunistic
    while not feasible(v, constraints):
        if v == capability.floor:  return Refuse(shortfall)   // FV-6 — the floor is real
        v := next_lower(v)
        disclose(v)                              // the walk down is stated, not silent
    return v
```

Two properties are load-bearing. The walk down is **disclosed** — the user learns they are
getting the lower variant before or with the result, not by noticing. And the loop
terminates at a declared floor rather than at the bottom of the list: the lowest *available*
variant and the lowest *acceptable* variant are different values, and only the second one is
a product decision.

### 4.4 Why the delta must be sliced (FV-8)

A single averaged quality delta describes a variant that degrades uniformly. Real variants
do not: they hold up on the common case and collapse on a specific slice — the long input,
the unusual language, the dense document, the rare category. Averaged, that reads as a small
regression; sliced, it reads as "unusable for this class of work", which is the fact a
selector actually needs.

This is why the reference variant (FV-9) matters operationally as well as definitionally:
without a fixed baseline, per-slice deltas cannot be stated at all.

### 4.5 The one case where a cheap variant is legitimate for something else

Where a decision needs only a **ranking** — which of several candidates to keep — the
ranking may be produced at a reduced variant and the chosen candidate then produced once at
the reference variant. That case is already specified by the competitive-execution
discipline and is not restated here; what this spec adds is that the reduced runs are
**declared variants** subject to FV-2/FV-4, so the record shows what the selection was
actually made on. The cheap output itself is never delivered as the result.

### 4.6 nodus projection

No new language primitive is required:

1. **The choice is a configuration field.** The validated declarative configuration surface
   already carries typed, defaulted, range-checked fields validated before any value becomes
   visible to a workflow; a variant selection is an ordinary member of it, and an id that no
   longer exists resolves to the declared reference (FV-9) rather than failing obscurely.
2. **The run record already carries what FV-4 needs.** Per-run cost, receipt, and lineage
   annotations ride the observability record; the selected variant is one more annotation,
   and it is what makes a later comparison able to refuse itself (FV-5).
3. **Comparability is already an environment concern.** The environment/evaluation contract
   requires a graded run to partition its frontier by profile, budget, and the declared
   budget measure — precisely FV-5's within-variant rule, already expressed in the language's
   own terms. Adding the variant to that partition key is the whole of the change, and it is
   host-side.

## 5. Implementation Notes

1. Put the variant in the artifact's recipe at production time (FV-4); reconstructing it
   later from logs is exactly the reconstruction that will be skipped under time pressure.
2. Make `UNKNOWN` the default for an unmeasured `quality_delta` rather than a placeholder
   number — a placeholder is indistinguishable from a measurement six months later.
3. Declare the floor with the capability, not with the policy that consumes it (FV-6): the
   capability's owner knows what is fit for purpose; a resource policy does not.
4. Wire the mixed-variant refusal into the measurement path itself (FV-5). A rule that
   depends on an analyst noticing is a rule that holds until the first busy week.

## 6. Drawbacks & Alternatives

- **Declaring and measuring variants is real work.** Accepted, and bounded by FV-8's
  `UNKNOWN`: a variant may ship with its effect undeclared as long as the uncertainty is
  visible. The work is in measuring, and it is optional; the honesty is not.
- **A floor can block work the user would have accepted.** Which is why the floor is declared
  by the capability's owner as *fit for purpose* rather than as a quality preference, and why
  refusal states the shortfall so the user can change the constraint rather than the answer.
- **Within-variant comparison shrinks the comparable set.** True, and the alternative is a
  larger set of incomparable numbers. A smaller honest baseline beats a larger contaminated
  one, and FV-9's named reference keeps at least one axis always comparable.
- **Alternative — treat quantization/compression as a performance setting.** Rejected by
  FV-1: it is precisely the misfiling that lets a quality regression enter through the door
  reserved for changes promised not to alter results.
- **Alternative — always run at the reference variant.** Rejected as a general rule: on
  local-first hardware it converts "slower" into "impossible", which is the failure the
  runtime's degrade-before-refusing discipline exists to prevent. FV makes the cheaper option
  *legible* rather than forbidden.
- **Alternative — fold into the runtime's degradation tiers.** Rejected by that spec's own
  words: a tier changes speed and never outputs, and a result-altering configuration is
  explicitly declared to be something else. This is that something else.
- **Alternative — fold into the reproduction recipe.** Rejected: the recipe *records* the
  variant (FV-4) and has no notion of an ordered axis, a floor, a reference, or a
  within-variant comparison rule. Recording is one of nine obligations here.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[RUNTIME]` | `.design/main/specifications/l1-model-runtime.md` | MR-15's boundary that invited this spec; MR-7/MR-16 force the choice. |
| `[COMPETITIVE]` | `.design/main/specifications/l1-competitive-execution.md` | CPE-10 — the select-cheap-produce-once special case (§4.5). |
| `[BENCHMARK]` | `.design/main/specifications/l1-model-benchmarking.md` | MB-5 — the existing within-variant instance FV-5 generalizes. |
| `[COEVAL]` | `.design/main/specifications/l1-agent-coevaluation.md` | ACE-4 slicing, ACE-5 comparability. |
| `[RECIPE]` | `.design/main/specifications/l1-reproduction-recipe.md` | RR-2 ambient layer — where the variant is recorded. |
| `[OUTCOME-COST]` | `.design/main/specifications/l1-outcome-attributed-cost.md` | Why a mixed-variant denominator is meaningless. |
| `[ALLOWANCE]` | `.design/main/specifications/l1-usage-allowance.md` | UA-8 shedding order; FV is the finer move available first. |
| `[WORKFLOW-LANG]` | `.design/main/specifications/l1-workflow-language.md` | The nodus surface the concept projects onto (§4.6). |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.0 | 2026-08-05 | Core Team | Initial spec — fidelity variants, the axis the runtime's degradation discipline explicitly declines: MR-15 trades speed for capacity and MUST NOT alter outputs, classifying a result-altering configuration as *something else*, and that something else had no owner. A fidelity variant changes the output while a placement tier does not, and conflating them lets a quality regression enter through the door reserved for changes promised not to alter anything (FV-1); the variant set is declared, ordered, and profiled, since an undeclared fast mode is a second product shipped without a name (FV-2); selection is explicit or policy-driven and never opportunistic, because silent substitution yields consecutive results whose difference has no recorded cause — indistinguishable from the system becoming unreliable (FV-3); the variant travels **with** the artifact as a recipe field, since an artifact whose fidelity is unrecorded will be compared anyway (FV-4); measurement is **within-variant**, generalizing the model-benchmark rule that already folds configuration into the identity of the thing under test, with mixed-variant comparisons reported invalid rather than averaged (FV-5); a **declared floor** below which the answer is a stated refusal, because a chain of individually reasonable reductions otherwise terminates in output that is cheap and useless (FV-6); no silent **upgrade** either, spending budget the requester did not authorize being the same defect pointed the other way (FV-7); the quality effect measured and **sliced**, never assumed monotone, since lower fidelity typically collapses one slice while barely moving the mean — with `UNKNOWN` a first-class value rather than an omission (FV-8); and a named **reference** variant anchoring the axis, without which "the fast one" and "the good one" drift into unrelated products (FV-9). CPE-10's select-cheap-produce-once is the worked special case and is referenced, not restated. Nodus projection needs no new primitive — the config surface declares the choice with a stale id resolving to the reference, the run record annotation carries it, and the environment contract's comparability partition already expresses the within-variant rule. Concept-only. |
| 1.0.1 | 2026-10-22 | Core Team | Citations updated for the requirement-ID prefix renames that make each prefix name one specification (RULES §1, 1.8.0, the person's decision of 2026-10-22): `CE-n` of `l1-competitive-execution` is now `CPE-n`; `GS-n` of `l1-generation-shaping` is now `GSH-n`. No requirement was added, removed or reworded. |
