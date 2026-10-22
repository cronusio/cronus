# Adjustment Vocabulary

**Version:** 1.0.1
**Status:** Stable
**Layer:** concept

## Overview

When a produced artifact is close but not right, the client has to explain why. Free-form
revision — *"make it feel more premium but not flashy"* — is expensive at both ends: the
client must author a description of a feeling, and the producer must guess which dimension
to move and how far. What comes back usually differs on **every** axis at once, so the next
round begins by rediscovering what was already good. That is a regeneration lottery, and
its cost is paid per round.

An **adjustment vocabulary** replaces the description with a **move**: a small, closed set
of **named directional operations over an existing artifact**, each naming one axis, each
with an opposite, each re-applicable. The client steers rather than re-specifies, and
*quieter* means the same thing to the client and the producer because the name is bound
once rather than interpreted afresh.

The distinction from the neighbouring disciplines is precise. Iterative refinement is an
**automated** loop driven by a grader against a frozen rubric; directability governs
**steering operations on the office's own substrate** (a card, a worker, an automation).
This governs neither: it is the human's cheap, named handle on **a produced artifact's own
dimensions**, and it exists because "revise it" is the most expensive instruction in the
system.

## Related Specifications

- [l1-iterative-refinement.md](l1-iterative-refinement.md) — the **automated** sibling: a grader-driven revise loop against a frozen rubric (ITR-2/ITR-3/ITR-6). An adjustment is the *human-directed* single step — one axis, one move, no grader, no convergence criterion — and ITR-10's human-grader arm is where the two meet.
- [l1-directability.md](l1-directability.md) — **different subject**: DIR steers *operations on the office substrate* (move a card, reassign a worker, rewire an automation) and re-projects the lens. An adjustment steers a **produced artifact's own dimensions**. DIR-5's accepted-steer-as-standing-constraint is the property ADJ-10 feeds.
- [l1-generation-shaping.md](l1-generation-shaping.md) — the same *shape* of mechanism on the office's own output economy: GS's verbosity and effort are named levels on named axes, set by policy. An adjustment vocabulary is the client-facing generalization over a produced artifact, and GSH-4's correctness floor applies unchanged.
- [l1-design-identity.md](l1-design-identity.md) — the domain where the vocabulary is richest: a visual surface's axes (weight, density, colour, motion) are exactly the adjustment axes, and the craft bar remains the floor no adjustment may cross.
- [l1-conversational-control.md](l1-conversational-control.md) — the channel an adjustment arrives on; ADJ-2's closed names are what keep a conversational steer from being re-interpreted per turn.
- [l1-pattern-codification.md](l1-pattern-codification.md) — the ratification pathway ADJ-9 uses to promote a repeatedly-used free-form direction into a named axis.
- [l1-user-model.md](l1-user-model.md) — the adjustment trail is revealed preference (ADJ-10): a client who always steers one direction has a stated default waiting to be recorded, rather than a correction to repeat.
- [l1-conversation-rewind.md](l1-conversation-rewind.md) — the recorded adjustment sequence (ADJ-10) is what makes stepping back to an earlier artifact state a truncation rather than a reconstruction.
- [l1-workflow-language.md](l1-workflow-language.md) — the nodus projection (§4.5): a declared axis on a revise step plus the existing human-in-the-loop dialog channel; no new primitive.

## 1. Motivation

**Describing a feeling is harder than naming a direction.** The client knows the artifact
is too much or too little of something long before they can say which something. A named
axis lets them act on the judgement they actually have — *less of this* — instead of
authoring a specification for a judgement they do not.

**Regeneration destroys accumulated agreement.** By round three, most of the artifact is
settled and one dimension is wrong. A producer that responds by generating afresh throws
away the settled part, and the client spends the round re-approving what they had already
approved. The cost is not the tokens; it is that the negotiation never converges.

**Free-form revision is ambiguous in both directions.** "More premium" moves type, colour,
spacing, copy, and motion in one unattributable step. When the result is worse, neither
side can say which move caused it, so the next instruction is another guess.

**One-way verbs trap the artifact.** A vocabulary that can only add emphasis, add colour,
add motion has no remedy for over-application except a rewrite — and the producer, asked
to fix it, will regenerate. Every axis needs both directions or it is a ratchet.

**The producer needs steering to be distinguishable from failure.** A revision request read
as a defect report changes behaviour: the producer starts defending, explaining, or
over-correcting. *Quieter* is a preference, not a bug report, and the system should be able
to tell the difference structurally rather than by tone.

## 2. Constraints & Assumptions

- The subject is a **produced artifact** — a surface, a document, a plan, a piece of copy,
  a diagram. Not the office's own state, and not a running process.
- The vocabulary is **per artifact kind** (§ADJ-7): axes are domain properties, not
  universal ones.
- An adjustment is a **client-directed** act. Nothing here proposes that the office steer
  itself along these axes; that is what shaping policy and refinement loops already do.
- The floor rules still apply: an adjustment may not push an artifact below a quality,
  craft, accessibility, or correctness bar. Steering moves within the acceptable region.
- The concept defines no new store: the adjustment trail rides the artifact's existing
  version history.

## 3. Core Invariants

Rules every Layer 2 implementation MUST NOT violate:

- **ADJ-1 An adjustment edits the existing artifact; it never regenerates it**: the
  artifact keeps its identity and everything not on the named axis is **preserved**.
  Responding to an adjustment with a freshly-generated artifact discards the agreement
  already reached and restarts the negotiation — the single most common and most expensive
  failure this concept exists to prevent.

- **ADJ-2 The vocabulary is closed, named, and shared**: a small, closed set of stable
  names, bound to the **same** meaning by the client and the producer. The value is
  precisely that the term is not re-interpreted per turn: an open or per-session vocabulary
  is free-form revision with extra ceremony.

- **ADJ-3 Every axis is bidirectional and every verb has an opposite**: an adjustment names
  a **direction on an axis** whose other direction is also available. A one-way vocabulary
  is a ratchet: over-application has no remedy but a rewrite, which violates ADJ-1 and
  returns the client to the lottery.

- **ADJ-4 One axis per adjustment**: an operation moves **one** dimension and leaves the
  others as they were. An adjustment that moves several axes is indistinguishable from a
  regeneration and destroys the client's ability to attribute the difference to the move
  they asked for.

- **ADJ-5 Re-applicable, bounded, and honest at the limit**: the same adjustment may be
  applied repeatedly to travel further along its axis, and each axis declares a **limit**.
  At the limit the producer **says so** rather than changing something else to appear
  responsive. A producer that keeps moving *something* so the client sees a difference has
  converted a bounded axis into an unattributable rewrite.

- **ADJ-6 Reversible along the axis**: applying the opposite adjustment moves the artifact
  **back** along that axis. It need not restore a byte-identical prior state, but the
  direction must actually reverse — an "opposite" that produces a third, unrelated result
  is not an axis, and the client can no longer trust any move.

- **ADJ-7 Axes are declared per artifact kind, never universal**: each kind declares its own
  vocabulary, because its dimensions are properties of *it* — a visual surface, a written
  document, a plan, and a code change do not share axes. A single global set either fits
  none of them or collapses into *more* and *less*, which is where the discipline started.

- **ADJ-8 An adjustment carries no verdict on the previous version**: a steer is a
  **preference move**, not a defect report, and is recorded and presented as such.
  Conflating the two teaches the producer to treat steering as failure — after which it
  defends its work, over-corrects, or apologizes, all of which cost a round and none of
  which moves the axis.

- **ADJ-9 Free-form direction remains available and is never removed**: the closed set
  covers the common moves; anything outside it is expressed in prose and handled as an
  ordinary revision. A prose direction used repeatedly is a **candidate** for promotion into
  the vocabulary through the ratification pathway — vocabularies grow from observed use, and
  a vocabulary that forbids what it does not name is a cage.

- **ADJ-10 Every adjustment is recorded with axis, direction, and resulting version**: the
  trail is legible, replayable, and truncatable, and it is **revealed preference** — a
  client who steers the same direction on every artifact has a standing default waiting to
  be recorded rather than a correction to be repeated forever. The record composes the
  artifact's existing version history; it is not a second store.

> L2 specs cannot reach RFC status until all invariants here are addressed in their "Invariant Compliance" section.

## 4. Detailed Design

### 4.1 The adjustment

```text
[REFERENCE]
Adjustment {
  artifact  : the existing artifact (identity preserved — ADJ-1)
  axis      : one declared axis of that artifact kind      // ADJ-4, ADJ-7
  direction : one of the axis's two poles                  // ADJ-3
  magnitude : implicit (one step) — repeat to go further   // ADJ-5
}

apply(adjustment):
    if at_limit(artifact, axis, direction):
        return AtLimit(axis)                                // ADJ-5 — say so, change nothing
    return edit(artifact, axis, direction)                  // never regenerate — ADJ-1
```

`magnitude` is deliberately not a parameter. A named step the client can repeat is easier
to reason about than a number whose scale nobody shares, and repetition gives the producer
the signal that the previous step was insufficient — information a magnitude argument
hides.

### 4.2 What an axis looks like

| Artifact kind | Example axes (both poles named) |
| --- | --- |
| Visual surface | emphasis (bolder ↔ quieter) · density (fuller ↔ sparser) · colour (more ↔ less) · motion (more ↔ stiller) |
| Written document | length (fuller ↔ tighter) · register (warmer ↔ drier) · detail (deeper ↔ higher-level) |
| Plan | scope (broader ↔ narrower) · granularity (finer ↔ coarser) · caution (safer ↔ faster) |
| Code change | abstraction (more ↔ less) · defensiveness (stricter ↔ leaner) |

The table is illustrative, not normative — ADJ-7 puts the axis set in each domain's hands.
What is normative is the shape: **a named axis with two named poles**, never a one-way verb
and never an adjective without an opposite.

### 4.3 Why bidirectionality is load-bearing (ADJ-3)

An artifact under a one-way vocabulary can only accumulate. The client applies *bolder*
twice, overshoots, and now has no move: the only available response is prose ("undo some of
that"), which the producer resolves by regenerating, which loses everything else. The
opposite pole is not a convenience; it is what keeps the whole interaction inside ADJ-1.

### 4.4 The trail as preference (ADJ-10)

```text
[REFERENCE]
trail := [(axis, direction, version), ...]

// two readings, both cheap:
undo         := apply(opposite(last.direction), last.axis)      // ADJ-6
learned_pref := axis where direction is consistent across artifacts   // → user model
```

The second reading is the compounding one. An office that notices the client steers
*quieter* on every surface can carry that as a stated default and stop producing the
version that will be corrected — turning a repeated correction into a preference, which is
exactly the promotion path the resolution and codification layers already define.

### 4.5 nodus projection

No new language primitive is required:

1. **An adjustment is a revise step over an existing artifact.** The language already
   carries a single evolving artifact through bounded revise iterations; an adjustment is
   one such step whose input is the current artifact and whose parameter is a declared axis
   and direction — a host-supplied vocabulary riding the step's existing modifier grammar.
2. **The dialog channel already exists.** The human-in-the-loop contract that suspends a run
   for a question is the same channel an adjustment arrives on, and the memoizable-decision
   variant is precisely how a repeated steer becomes a standing preference (ADJ-10) without
   asking again.
3. **The trail is already observable.** Per-step trace events with stable cross-run step
   identity give the adjustment record its home; the axis and direction ride as annotations
   rather than as a new store.

## 5. Implementation Notes

1. Declare the axes with the artifact kind, not with the producer — a second producer for
   the same kind must inherit the same vocabulary or ADJ-2 is already broken.
2. Implement `at_limit` honestly before shipping any axis (ADJ-5); the tempting fallback —
   change something adjacent so the client sees movement — is the exact behaviour the
   invariant forbids and is very hard to detect afterwards.
3. Keep the opposite verb in the same declaration as its pole so an axis cannot ship
   one-way by omission (ADJ-3).
4. Record the trail on the artifact's existing version history from the first adjustment;
   reconstructing it later loses the direction, which is the only part that matters.

## 6. Drawbacks & Alternatives

- **A closed vocabulary cannot express everything.** By construction, and ADJ-9 keeps prose
  available as the always-present fallback with a promotion path. The closed set is a
  shortcut for the common case, never a restriction on what may be asked.
- **Naming axes per domain is work, repeated per domain.** Real, and ADJ-7 argues it is
  unavoidable: the alternative is a universal set that degenerates to *more*/*less* and
  buys nothing over free-form revision.
- **A step's size is unstated, so two producers may move differently.** Accepted: ADJ-5's
  repeatability plus ADJ-6's reversibility make a mis-sized step cheap to correct, which is
  a better trade than a shared magnitude scale nobody can calibrate.
- **Alternative — free-form revision only.** Rejected by §1: it is the status quo, it is
  ambiguous in both directions, and it reliably triggers the regeneration that loses
  accumulated agreement.
- **Alternative — a numeric slider per axis.** Rejected: it implies a shared, calibrated
  scale that does not exist between a client and a generator, and it invites moving several
  sliders at once, which is ADJ-4's failure.
- **Alternative — fold into iterative refinement.** Rejected: IR is an automated loop with a
  grader, a frozen rubric, and a convergence budget. An adjustment has no grader, no
  rubric, and no convergence criterion — it is one human-directed move, and forcing it
  through a loop's machinery adds a rubric nobody wrote.
- **Alternative — fold into directability.** Rejected: DIR steers the office's own
  substrate through the operation that owns it, and re-projects the lens. A produced
  artifact is not a lens over a substrate; the axes are properties of the artifact itself.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[REFINEMENT]` | `.design/main/specifications/l1-iterative-refinement.md` | The automated sibling; boundary drawn in §6. |
| `[DIRECTABILITY]` | `.design/main/specifications/l1-directability.md` | The substrate-steering sibling; different subject. |
| `[SHAPING]` | `.design/main/specifications/l1-generation-shaping.md` | The same named-axis mechanism applied to output economy. |
| `[IDENTITY]` | `.design/main/specifications/l1-design-identity.md` | The richest domain for axes; the craft floor no adjustment may cross. |
| `[CODIFICATION]` | `.design/main/specifications/l1-pattern-codification.md` | The promotion path for a repeated free-form direction (ADJ-9). |
| `[USER-MODEL]` | `.design/main/specifications/l1-user-model.md` | Where a consistent steering direction becomes a stated default (ADJ-10). |
| `[WORKFLOW-LANG]` | `.design/main/specifications/l1-workflow-language.md` | The nodus surface the concept projects onto (§4.5). |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.0 | 2026-08-05 | Core Team | Initial spec — an adjustment vocabulary as the client's cheap, named handle on a produced artifact's own dimensions, replacing free-form revision (expensive at both ends, ambiguous in both directions, and reliably answered with a regeneration that discards accumulated agreement): an adjustment **edits** the existing artifact and never regenerates it (ADJ-1); the vocabulary is closed, named, and bound to the same meaning by client and producer, since re-interpretation per turn is free-form revision with ceremony (ADJ-2); every axis is **bidirectional**, because a one-way verb is a ratchet whose only remedy for overshoot is the rewrite ADJ-1 forbids (ADJ-3); one axis per adjustment, or the move is unattributable (ADJ-4); re-applicable and bounded with the producer **saying so at the limit** rather than moving something else to appear responsive (ADJ-5); reversible along the axis, an "opposite" that yields a third unrelated result being no axis at all (ADJ-6); axes declared per artifact kind, since a universal set collapses into *more*/*less* (ADJ-7); an adjustment carries **no verdict** on the previous version, so steering is not read as failure and answered with defence or over-correction (ADJ-8); free-form direction always available with a promotion path into the vocabulary (ADJ-9); and the trail recorded as axis+direction+version — replayable, truncatable, and readable as revealed preference feeding the user model (ADJ-10). Nodus projection needs no new primitive — an adjustment is a revise step with a declared axis, the human-in-the-loop dialog channel carries it, the memoizable-decision path turns a repeated steer into a standing preference, and per-step trace annotations hold the trail. Concept-only. |
| 1.0.1 | 2026-10-22 | Core Team | Citations updated for the requirement-ID prefix renames that make each prefix name one specification (RULES §1, 1.8.0, the person's decision of 2026-10-22): `GS-n` of `l1-generation-shaping` is now `GSH-n`; `IR-n` of `l1-iterative-refinement` is now `ITR-n`. No requirement was added, removed or reworded. |
