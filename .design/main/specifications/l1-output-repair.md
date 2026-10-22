# Output Repair

**Version:** 1.0.1
**Status:** Stable
**Layer:** concept

## Overview

Output repair is the **deterministic post-generation correction layer**: the small, named transformations a system applies to raw model output to fix *form* defects it has learned that models produce — leaked tool-call syntax emitted as prose, an internal field that escaped into user-facing text, a marker placed after the sentence it should precede, a JSON envelope with a trailing comma.

Every mature model-consuming system grows this layer, and almost every one grows it **anonymously** — a `.replace()` here, a regex there, buried in a parser, added the day someone noticed the bug and never looked at again. That is the failure this spec addresses. The repairs themselves are legitimate and cheap; what is missing is that they are **evidence**. Each one encodes a claim that a specific model misbehaves in a specific way, and the rate at which it fires answers questions nothing else can: is this model version worse than the last, is this correction still needed, is the prompt actually broken.

An anonymous repair converts a measurable model defect into invisible maintenance. A named, counted one turns the same line of code into an instrument.

## Related Specifications

- [l1-output-contracts.md](l1-output-contracts.md) — the **validate-and-retry** path for *semantic* defects (wrong content, unmet criteria). Repair is its deterministic complement for *form* defects, and RPR-8 draws the line: repairing a semantic defect is fabrication, retrying a form defect is waste.
- [l1-tool-call-transport.md](l1-tool-call-transport.md) — TCT-8's typed malformed-call containment re-injects for a repair-retry; this layer sits **before** that, correcting decodable-after-repair form so the retry is not spent on a fixable comma. TCT-5's reasoning isolation and TCT-10's provenance-gated decode are the boundaries RPR-10 forbids a repair from crossing.
- [l1-negative-specification.md](l1-negative-specification.md) — NEG-4 places the exclusion **before** generation (cheaper); a repair is the after-the-fact net for what got through. NEG-10's *firing rate measured, never-fires and always-fires both actionable* is the same discipline, and RPR-5 states it for repairs.
- [l2-model-error-recovery.md](l2-model-error-recovery.md) — provider/transport failures (retry, rotate, fall back). A repaired output is a **successful** call whose text needed correcting; the two paths never substitute for each other.
- [l1-claim-verification.md](l1-claim-verification.md) — post-hoc faithfulness checking, advisory and never silently editing (CV-6). Repair silently edits **form** by design; the demarcation in §4.4 is what keeps CV-6 intact.
- [l1-tokenization-boundary.md](l1-tokenization-boundary.md) — TB-4/TB-5: control symbols are structurally unreachable by encoding content, and control-looking content defaults to refusal. RPR-10 forbids a repair from becoming the path by which control-looking text is promoted to control.
- [l1-telemetry.md](l1-telemetry.md) — the counter channel RPR-4 feeds; counts and enums only, never the offending text.
- [l1-model-benchmarking.md](l1-model-benchmarking.md) — a candidate's repair rate is a fitness signal in its own right, and MB-10's rule applies: repairs are part of the measured harness and must be identical across candidates.
- [l1-scoped-generalization.md](l1-scoped-generalization.md) — SG's argument applied to corrections: a repair earned against one model is a hypothesis about the next (RPR-9).
- [l1-log-legibility.md](l1-log-legibility.md) — LL-5's honest reduction: a repaired output is a modified record and is flagged as such (RPR-6).

## 1. Motivation

Models produce output that is *nearly* right in shape and needs a small, mechanical fix. The alternatives to fixing it are all worse: re-asking costs a full round trip for a stray character; failing the turn punishes the user for a provider quirk; passing it through hands a defect to a parser, a UI, or another model.

So the fix gets applied. And then, in every system that has not thought about it, four things happen:

- **Nobody can tell whether the model improved.** A new model version ships. Did it stop leaking tool markup? The system silently keeps stripping it either way, and the only record of the answer is a line of code that has been running for a year.
- **Dead repairs accumulate.** A correction for a provider nobody uses anymore, or for a defect fixed upstream, stays forever — untested at the only thing that matters (does it still fire), and load-bearing in the imagination of the next reader.
- **Real regressions hide.** A repair that used to fire on 2% of calls now fires on 40% because a prompt change broke a format instruction. Nothing surfaces; the output is still correct, just increasingly manufactured.
- **The streaming path is silently unprotected.** The repair is written against a complete string. The same output, streamed, never passes through it, so the defect the team believes is handled reaches the user on the path they actually ship.

None of these is fixed by writing better repairs. They are fixed by treating each repair as a **named, counted hypothesis about a specific model's behaviour** — which costs almost nothing at the point of writing and is impossible to reconstruct later.

## 2. Constraints & Assumptions

- **Repair is cheap and local.** A repair is a pure transformation over the produced output, not a model call, not a retry, not a lookup.
- **The defect class is form, and form is decidable.** If deciding whether the output is defective requires judgment, it is not a repair candidate (RPR-8).
- **Model behaviour changes underneath the layer.** Repairs are written against observed behaviour of specific models at specific versions, and that is a moving target by construction.
- **Both a complete-string path and an incremental path exist.** Any output surface that streams has two paths through which the same defect can arrive.
- **Counting is local.** Recording that a repair fired is an on-device act; whether any of it leaves the device is governed by the telemetry consent contract, not by this spec.

## 3. Core Invariants

Rules every Layer 2 implementation MUST NOT violate. They are technology-neutral.

- **RPR-1 (Named, declared repairs — no anonymous fixups):** every correction applied to raw model output is a **named transformation** with a declared defect it addresses and the site it applies at. An inline, unnamed adjustment buried in a parser is forbidden, because it cannot be counted (RPR-4), tested at its trigger, retired (RPR-5), or attributed to a model (RPR-9) — and its absence from any inventory means nobody knows the layer exists until it misbehaves.

- **RPR-2 (Form only, never meaning):** a repair corrects the **shape** of the output — stray markup, a misplaced marker, an internal field that leaked into user-facing text, an unparseable envelope. It MUST NOT change a value, a decision, a claim, or an action the output expresses. A transformation that alters what the output *says* is not a repair; it is an unreviewed edit made by a component with no authority to make it, and it destroys the property every downstream consumer assumes — that the model's content is the model's.

- **RPR-3 (Deterministic, idempotent, order-declared):** repairs are pure functions of the output. Applying one twice equals applying it once. Where two repairs can both match, either their composition is order-independent or the order is **declared** — an emergent order that happens to work is a defect waiting for the day someone reorders the list.

- **RPR-4 (Every firing is counted and attributed):** each application is recorded against **(repair, provider, model, model version)**. The rate is the product of this layer. A repair that fires without being counted converts a measurable model defect into invisible maintenance — the fix ships, the knowledge does not, and the question "did the new model get better" becomes unanswerable by construction.

- **RPR-5 (Both rate extremes are actionable):** a repair that **never fires** is dead code encoding a guarantee nobody is checking — retire it, or discover that the path no longer reaches it, which is the more interesting finding. A repair that **always fires** is an upstream defect being papered over — a prompt, a schema, or a contract that is simply wrong, and the repair is hiding the evidence. Neither extreme is a healthy steady state, and a layer that reports no rates cannot distinguish either from a repair that is working. (NEG-10 discipline.)

- **RPR-6 (Silent to the model, never silent to the record):** the **repaired** output is what the next turn and the downstream consumer see — re-injecting a defect the system already knows how to fix teaches nothing and costs tokens. But the fact that a repair fired, and which one, is in the trace (LL-5 honest reduction: a modified record is flagged as modified). A repair that leaves no trace makes a corrected output indistinguishable from a correct one, which is precisely the information this layer exists to produce.

- **RPR-7 (No streaming counterpart means no protection on the streaming path):** a repair defined over a **complete** output does **not** apply when the same output is delivered incrementally. Any repair whose defect can appear mid-stream MUST either declare a **streaming form** — one that buffers enough to catch a pattern split across chunk boundaries, and whose scope resets per generation so suppression in one round does not silence a later one — or **declare the streaming path unprotected**. Silence here is the dangerous option: the team believes the defect is handled and ships the path where it is not.

- **RPR-8 (Repair is for decidable form; semantics belong to the contract):** where the defect is **semantic** — wrong answer, missing requirement, unmet criterion — the output contract's validate-and-retry path owns it (OCT-1…OCT-5), and repairing it is **fabrication**: a component with no view of the requirement inventing what the model should have said. Conversely, spending a retry on a defect a deterministic transformation resolves is waste. The discriminator is whether deciding "is this defective, and what is the correct form" requires judgment.

- **RPR-9 (Repairs are model-scoped hypotheses, not permanent truths):** a repair encodes a **specific model's** observed misbehaviour. It records the models and versions it was observed against, and a provider, model, or version change **re-opens** the question rather than silently inheriting the correction. Applying a repair earned on one model to another is the same unearned generalization SG-3 forbids for learned patterns — with the extra hazard that a repair matching output the new model produces *legitimately* corrupts correct output.

- **RPR-10 (A repair never widens trust or promotes content to control):** repaired output carries **exactly** the provenance the raw output carried. Stripping leaked control-looking markup does not make the surrounding content trusted, and a repair MUST NOT be the mechanism by which content that *looks* like a control construct is turned into one — the boundary's refusal default stands (TB-5), and a "helpful" repair that reconstructs a malformed control token into a valid one has performed the injection the boundary exists to prevent.

## 4. Detailed Design

### 4.1 What a declared repair carries

| Field | Why |
| --- | --- |
| Name | The unit of counting, retirement, and conversation (RPR-1) |
| Defect described | Lets a reader decide whether it still applies without reverse-engineering the pattern |
| Site / stage | Where in the pipeline it runs; determines what it can and cannot see |
| Streaming form | Present, or explicitly absent with the path declared unprotected (RPR-7) |
| Observed against | Models and versions the defect was seen on (RPR-9) |
| Firing count | Per provider/model/version (RPR-4) |

### 4.2 The rate is the deliverable

```
repair firing rate over time, per (model, version)

  ▲
  │        ╭──────  always fires → upstream defect, fix the prompt/contract (RPR-5)
  │   ╭────╯
  │───╯                    healthy: fires sometimes, tracks a real model quirk
  │
  │_______________________ never fires → dead code, or an unreachable path (RPR-5)
  └────────────────────────────────────────────▶ model versions
```

A step change at a version boundary is the signal the layer exists to produce: it says the model's behaviour moved, in which direction, and on exactly which defect — a question no benchmark asks and no user reports.

### 4.3 Where repair sits

```
model call
   │
   ├─ transport/provider error ────▶ error recovery (retry, rotate, fall back)
   │
   └─ successful response
         │
         ├─ FORM defect (decidable) ─▶ REPAIR ─┐   deterministic, counted, no round trip
         │                                      │
         ├─ malformed structured call ─────────▶│─▶ typed containment → repair-retry (TCT-8)
         │                                      │
         └─ SEMANTIC defect (judgment) ────────▶ validate → retry with verdict (OCT-4/OCT-5)
                                                │
                                          downstream consumer
                                                │
                                          claim verification (advisory, post-hoc)
```

### 4.4 Why silent form-editing does not contradict the no-silent-edit rule

Claim verification is explicitly **non-authoritative and never silently edits** (CV-6), and this layer silently edits by design. The two coexist because they act on different things: CV-6 protects the output's **content** — what it claims, decides, and asserts — which RPR-2 also forbids a repair from touching. Repair acts only on **form**, where the output's meaning is invariant under the transformation. The moment a repair changes meaning it has stopped being a repair and become the unreviewed edit CV-6 forbids; RPR-2 is the boundary and RPR-6's trace is what makes a violation detectable rather than a matter of trust.

### 4.5 Nodus relevance

None new. A workflow's model call goes through the host-supplied model provider seam, so the repair layer is entirely host-side: nodus names no provider, no output format, and no defect pattern, and adding a repair vocabulary to the language would import exactly the host-specific coupling the portability contract exists to prevent. This is the same disposition the tool-call transport contract reached for its own wire concerns.

## 5. Implementation Notes

- Declare the repair inventory in one place. The most common failure is not a bad repair but a repair nobody knew was there; an inventory makes RPR-5's retirement review a five-minute activity instead of an archaeology project.
- Record the firing at the site, buffer it, and attribute it at the **call boundary** where provider and model are known — attributing at the site either duplicates that knowledge everywhere or loses it.
- Counting must never be able to fail the call. A repair that raises because its counter was unavailable has traded a cosmetic defect for an outage.
- Test each repair at its **trigger**, not only at its transformation: a repair whose pattern no longer matches the defect passes every unit test of its own logic and protects nothing.
- When adding a repair, write down the model and version it was observed on **in the same change**. It is the field nobody can reconstruct later and the one RPR-9 depends on entirely.

## 6. Drawbacks & Alternatives

- **Ceremony on a one-line fix.** Naming, registering, and counting a `.strip()` feels disproportionate at the moment of writing. It is; the cost is paid once and the alternative is an unmeasurable layer that grows monotonically for years.
- **Alternative — always retry instead of repairing:** rejected (RPR-8). A round trip for a stray character is expensive, slow, and not even reliable — the model may reproduce the same form defect.
- **Alternative — pass the defect through and let consumers cope:** rejected. It pushes one model's quirk into every downstream surface, each of which handles it differently and none of which counts it.
- **Alternative — count nothing, keep repairs simple:** rejected (RPR-4). This is the current default everywhere and it is exactly what makes model-version regressions invisible; the counting is the point, the correcting is the easy part.
- **Repairs can mask real degradation.** A model getting steadily worse in a repairable way looks fine to users while the repair rate climbs. That is a feature only if someone is watching the rate — which is why RPR-5 makes the always-fires end actionable rather than merely observable. <!-- TBD: whether a rate threshold should raise an operational-health signal automatically, and what baseline it compares against -->

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[CONTRACTS]` | `.design/main/specifications/l1-output-contracts.md` | The validate-and-retry path for semantic defects (RPR-8) |
| `[TRANSPORT]` | `.design/main/specifications/l1-tool-call-transport.md` | TCT-8 malformed-call containment; the decode boundary RPR-10 protects |
| `[NEGATIVE]` | `.design/main/specifications/l1-negative-specification.md` | NEG-4 prevention-before-generation, NEG-10 rate discipline |
| `[VERIFY]` | `.design/main/specifications/l1-claim-verification.md` | CV-6 never-silently-edits, demarcated in §4.4 |
| `[TOKENS]` | `.design/main/specifications/l1-tokenization-boundary.md` | TB-4/TB-5 control-symbol boundary RPR-10 must not breach |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.0 | 2026-08-06 | Core Team | Initial concept: the **deterministic post-generation correction layer** every model-consuming system grows and almost every one grows anonymously. Named declared repairs with no anonymous fixups, since an unnamed correction cannot be counted, tested at its trigger, retired, or attributed (RPR-1); **form only, never meaning** — a transformation that changes what the output *says* is an unreviewed edit by a component with no authority to make it (RPR-2); deterministic, idempotent, order-declared (RPR-3); **every firing counted and attributed to (repair, provider, model, version)** because the rate is the layer's actual product and an uncounted repair turns a measurable model defect into invisible maintenance (RPR-4); **both rate extremes actionable** — never-fires is dead code encoding an unchecked guarantee, always-fires is an upstream defect being papered over while its evidence is hidden (RPR-5, NEG-10 discipline); **silent to the model, never silent to the record**, since a corrected output indistinguishable from a correct one destroys exactly the information the layer produces (RPR-6); **no streaming counterpart means no protection on the streaming path** — a repair written over a complete string silently does not apply to the incremental path a product usually ships, so the streaming form is declared or the gap is (RPR-7); repair for decidable form, contract-retry for semantics, where repairing a semantic defect is fabrication and retrying a form defect is waste (RPR-8); **repairs as model-scoped hypotheses** whose observed-against versions are recorded and whose inheritance across a model change is re-opened, not assumed — SG-3's argument, with the added hazard that a stale repair corrupts output a new model produces legitimately (RPR-9); and **a repair never widens trust or promotes content to control**, so a helpful reconstruction of a malformed control token is the injection the boundary exists to prevent (RPR-10). §4.4 reconciles silent form-editing with CV-6's never-silently-edits by locating the boundary at meaning. §4.5 records the nodus disposition: no new invariant — the repair layer is host-side behind the model-provider seam. |
| 1.0.1 | 2026-10-22 | Core Team | Citations updated for the requirement-ID prefix renames that make each prefix name one specification (RULES §1, 1.8.0, the person's decision of 2026-10-22): `OC-n` of `l1-output-contracts` is now `OCT-n`. No requirement was added, removed or reworded. |
