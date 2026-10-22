# Context Transition

**Version:** 1.1.0
**Status:** Stable
**Layer:** concept

## Overview

A working session accumulates something no artifact it produces can reconstruct: the reasoning **as it happened** — the alternatives weighed, the dead ends walked, the reason the chosen option won. That live state is a **primary source**. Every disposition of it except staying put replaces it with a **secondary source**: a summary, a portable document, a delegated report, a compressed remainder.

The project already specifies each of those transforms in detail — how compression re-encodes, how summarization tiers, how a derived artifact travels, how a delegated unit reports. What no spec states is the thing an operator actually has to decide: **which disposition, and when taking one is legitimate at all.**

That gap has a characteristic failure, and it is not exotic. The most *available* move — reduce in place and carry on — gets reached for first, because it is the one that always applies. It is also the one that most often lands a fresh state confidently wrong about a decision the reduction flattened. This concept fixes three things: transitions are evaluated **only at a work boundary**, they are evaluated **in a fixed order whose first admissible option wins**, and that order is **cost-ordered, not preference-ordered** — staying put is examined first because it is the only move that loses nothing.

## Related Specifications

- [l1-context-compression.md](l1-context-compression.md) — CC specifies *how* a dense re-encoding preserves fidelity; this specifies *when* reduction is the right disposition at all. CC-7's ordering (compress before evict) is a within-reduction ordering; CXT-3 is the ordering **above** it.
- [l1-hierarchical-summarization.md](l1-hierarchical-summarization.md) — HS-5's rule that a summary is never passed off as a primary source is the artifact-grain statement of CXT-1; this supplies the session-grain one.
- [l1-derived-artifact-handoff.md](l1-derived-artifact-handoff.md) — the externalize disposition's mechanism. DAH governs the artifact's integrity and transport; CXT-6 governs whether producing one is warranted.
- [l1-execution-locus.md](l1-execution-locus.md) — where delegated work runs; CXT-7 decides whether to delegate, not where the delegate executes.
- [l1-session-reinforcement.md](l1-session-reinforcement.md) — what must survive a reduction; CXT-11's record of what a transition dropped is the input a reinforcement pass needs.
- [l1-context-degradation.md](l1-context-degradation.md) — the behaviour of an actor operating with less than it needs; CXT-10's sharp-reasoning band is the upstream cause this spec refuses to walk into.
- [l1-context-provenance.md](l1-context-provenance.md) — every transition is a provenance event; a reduced context whose reduction is unrecorded is indistinguishable from one that never held the material.
- [l1-evidence-archive.md](l1-evidence-archive.md) — the durable substrate that makes a reduction recoverable rather than terminal; CXT-11 records the *decision*, the archive holds the *content*.
- [l1-completion-verification.md](l1-completion-verification.md) — CXT-9's boundary rule is the mechanism behind hiding post-completion work; without a real boundary the hiding is nominal.
- [l1-recursive-decomposition.md](l1-recursive-decomposition.md) — the delegate disposition composes decomposition; CXT-7 adds the admissibility test decomposition does not state.
- [l2-session-checkpoint.md](l2-session-checkpoint.md) — `[ADDED v1.1.0]` §4.11 stores the CXT-13 handoff record with the work unit and loads it under CXT-14.
- [l1-state-authority.md](l1-state-authority.md) — `[ADDED v1.1.0]` the authorities a handoff record is checked against on load (CXT-14).

## 1. Motivation

Every transform in the token-economy family is well specified and individually correct. Put together they leave the operator with a menu and no ordering, and a menu with no ordering is resolved by availability. Five failures follow directly:

- **Reduction as a first reach.** Compaction is the move that always applies, so it becomes the move that is always taken. The cost is invisible at the time and expensive later: the next stretch of work proceeds from a flattened account of a decision, confidently, with no signal that anything was flattened.
- **Reduction mid-chunk.** A reduction fired by a budget threshold lands in the middle of work that is still using the material being reduced. The actor loses the thread it was holding, and the loss gets attributed to the model rather than to the timing.
- **Discarding a relevant state.** Emptying the window is cheap, instant, and the one move whose loss is total. It gets taken because nothing obviously argued for keeping the state — which is not the same as evidence that the state was disposable.
- **Externalizing for no crossing.** A portable artifact is produced when nothing is travelling: same actor, same location, same work. The result is a secondary source written for an audience identical to its author.
- **Continuing past sharpness.** Space remaining is read as capacity remaining. Work continues into the band where reasoning has already degraded, and produces material no later transition can repair — a reduction of degraded work is degraded work, compressed.

Each of these is a *decision* failure, not a *transform* failure. The transforms did exactly what they specify. What was missing is a contract over which one to invoke, and when.

## 2. Constraints & Assumptions

- **The live state is genuinely richer than its artifacts.** If everything a session knew were recoverable from what it wrote, this spec would be unnecessary. The rejected alternatives, the reasons, the near-misses are the part that is not written down.
- **Boundaries are recognizable but not mechanical.** A boundary is where one coherent chunk of work finishes. It can be detected with reasonable reliability and cannot be computed exactly; the contract depends on the former, not the latter.
- **Headroom is observable.** The actor, or the system around it, can tell roughly how much usable room remains. Precision is not required; a band is.
- **Delegation is available.** Some dispositions require the ability to run a scoped unit in its own window and receive a report. Where it is absent the ladder still holds, with that rung removed.
- **A transition is not free even when it is right.** Every disposition below *continue* spends something — tokens, latency, fidelity, or a human's attention on a produced artifact. The ordering exists because those costs are not equal.

## 3. Core Invariants

Rules every Layer 2 implementation MUST NOT violate. They are technology-neutral.

- **CXT-1 (Every transition converts a primary source into a secondary one, one-way):** the live session is a **primary source** — full information, high noise, little room to work. Every disposition except *continue* replaces it with a **secondary source** — lossy, less noisy, roomier. The loss is not recoverable by reading what the session produced: an artifact records what was decided, never the alternatives weighed and rejected on the way to deciding it. **The *why* is the part that goes.** This asymmetry is the whole reason the decision needs an order rather than a menu.

- **CXT-2 (The decision belongs at a work boundary, and nowhere else):** a transition is evaluated **only at a boundary** — the point at which one coherent chunk of work is finished and the next has not begun. Mid-chunk there is no decision to make: the only admissible moves are to continue, or to split the *remaining* work into delegated units. A reduction fired mid-chunk removes exactly the material the current work is still using, and the resulting incoherence is a timing defect, never an actor defect. A budget threshold MAY *signal*; it MUST NOT *fire* a transition inside a chunk.

- **CXT-3 (A closed, ordered disposition set; first admissible wins):** the dispositions are exactly **continue**, **discard**, **externalize**, **delegate**, and **compress**, evaluated in that order, and the first one whose admissibility test passes is taken. The order is **cost order, not preference order**: each later rung is strictly more lossy, more expensive, or both, than the rung above it. An implementation MUST NOT reorder the ladder and MUST NOT invent a sixth disposition outside it — a new way of moving context is an amendment to this set, not a private mechanism.

- **CXT-4 (Continue is examined first because it is the only free move):** staying put costs nothing and loses nothing. It is admissible when **either** the next chunk needs this one as a primary source — the reasoning verbatim, not a summary of it — **or** the remaining sharp-reasoning band (CXT-10) fits the next chunk. Reaching past *continue* while either holds pays CXT-1's conversion for no return, and is the most common unnecessary transition there is.

- **CXT-5 (Discard requires positive evidence of irrelevance):** emptying the state is the cheapest reduction and the most dangerous, because its loss is **total** and one-way. It is admissible only on a positive finding that everything held — the exploration, the decisions, the dead ends — is genuinely disposable to what comes next. **Absence of a reason to keep is not evidence of irrelevance**, and doubt resolves *down the ladder*, never into discard.

- **CXT-6 (Externalize only for a real crossing):** producing a portable artifact is justified by **portability and nothing else** — a different host or harness, a different working location, another principal, or a side thread forked out of mid-chunk work. Where nothing crosses, the artifact is a secondary source produced for an audience identical to its author, and it has paid CXT-1's cost to deliver a document no one needed.

- **CXT-7 (Delegate only what can run unattended, and only what returns a report):** a unit sent to its own window MUST be scoped tightly enough to complete **without steering**, and MUST return a report rather than a state to be merged back. This is the one disposition that *adds* a window instead of reducing one: the caller's own state is left intact, which is precisely what makes it cheaper than the rungs below it despite spending more total work.

- **CXT-8 (Compression is the default, and never the first reach):** the ordered evaluation lands on compression often, and that is correct; **starting** there is the characteristic failure of the whole family. Two obligations bind it. It is **directed** — told what the next chunk will need, so the reduction preserves that rather than optimizing an undirected notion of importance. And it happens **at a boundary** (CXT-2). An undirected compaction at an arbitrary point satisfies neither, and its output is a state that reads complete and is not.

- **CXT-9 (Hiding later work requires a real boundary; an inline call hides nothing):** where the *reason* for a transition is to remove upcoming steps from view — because visible post-completion work pulls attention away from the step in front of it — only a disposition that creates a **genuine context boundary** (delegate, externalize) actually removes them. An inline invocation leaves the later steps resident and clears nothing. A design that claims to hide upcoming work by an inline call is claiming an effect its mechanism does not produce: the behaviour is unchanged while the record says it was addressed.

- **CXT-10 (A declared sharp-reasoning band; the transition is taken before it is exhausted):** the actor's usable window contains a **band inside which its reasoning stays sharp**, narrower than its nominal capacity. Work MUST NOT be pushed past that band on the argument that space remains. The transition is taken at the **nearest boundary before** the band ends, never after it: a reduction of degraded work yields degraded work, compressed, and no downstream transition repairs it. The band is a declared, observable property — a policy value the system holds and reports, never an implicit hope.

- **CXT-11 (Every transition is recorded with what it converted):** each transition records **which disposition** was taken, **at which boundary**, and **what it replaced** — so a later reader can distinguish rationale that was never captured from rationale a reduction flattened. Without this, a missing *why* is indistinguishable from a *why* that never existed, and the loss CXT-1 names becomes undetectable rather than merely irreversible.

- **CXT-12 (The order is the contract; the verdict is not claimed to be mechanical):** the admissibility tests carry judgement, and the same boundary can legitimately resolve two ways on two days. What this contract fixes is that the questions are asked **in order, at a boundary** — not that the answer is computable. An implementation that renders the outcome as an objective determination invites a trust the mechanism cannot support; it surfaces the disposition **and the test that admitted it**, so a human can disagree with a specific step rather than with an opaque result.

- **CXT-13 (Work that continues in a fresh window continues from a handoff record of one shape):** `[ADDED v1.1.0]` when a directed reduction (CXT-8) or an externalization (CXT-6) hands work to a window that does not hold the current one — a new session, a successor actor, the same actor after its window is emptied — what crosses is a **handoff record** with six fixed parts: the **goal** (what the work is for, in its owner's terms); the **current state** (what holds now, verified rather than remembered); the **active artifacts** (the files, cards and documents in play, by reference); the **changes made** (what this window altered, by reference to the changes themselves); the **failed attempts** (what was tried and abandoned, and why — the part every reduction drops first and the next window most needs); and the **next steps** (the concrete next action, then what follows). An empty part is written as empty with its reason, never omitted. The record belongs to the **work**, not to the window that wrote it: it is attached to the work unit, and the next window that takes the unit reads it before acting. A newer record supersedes the older one; the older one stays retrievable.

- **CXT-14 (A handoff record is a secondary source and is checked against its authorities on load):** `[ADDED v1.1.0]` the record states what one window believed at one boundary (CXT-1). Before the next window acts on it, every part that names something with an authority of its own — a card's status, a file's content, a decision's record — is checked against that authority; where they disagree, the authority wins, and the disagreement is reported in the new window rather than silently corrected or silently followed. A record whose work unit is closed is not loaded as live guidance.

## 4. Detailed Design

### 4.1 The source trade

Every rung below *continue* makes the same trade in the same direction:

| Source | Information | Noise | Room to work | Recoverable |
| --- | --- | --- | --- | --- |
| Primary (continue) | Full | High | Little | n/a |
| Secondary (compress, externalize) | Lossy | Low | Large | Content via the archive; the reasoning, no |
| None (discard) | Gone | None | Complete | No |
| Delegated (delegate) | Caller's stays primary | Unchanged | Unchanged for the caller | Report only |

The table is the argument for the ordering. *Delegate* sits above *compress* not because it is cheap — it spends a whole additional window — but because it is the only rung that leaves the caller's primary source intact.

### 4.2 The ladder

Evaluated top to bottom at a boundary; the first admissible rung is taken.

| # | Disposition | Admissible when | What it costs |
| --- | --- | --- | --- |
| 1 | **Continue** | The next chunk needs this one verbatim, **or** the sharp band fits it | Nothing |
| 2 | **Discard** | Positive finding that everything held is disposable to what follows | Total, irreversible loss of the *why* |
| 3 | **Externalize** | Something genuinely crosses: host, location, principal, or a forked side thread | A lossy artifact plus the work of writing it |
| 4 | **Delegate** | The unit is scoped to run unattended and return a report | A whole additional window; caller's state untouched |
| 5 | **Compress** | Everything above failed: relevant state, same actor, same place, human stays in the loop | A flattened account, directed by what comes next |

### 4.3 The boundary test

A boundary is where one coherent chunk of work has finished and the next has not begun — recognizable, per CXT-12, by judgement rather than by rule. Two consequences are not judgement calls:

- **Mid-chunk, the ladder does not run.** The moves are *continue* or *split the remainder into delegated units*. Nothing else is admissible, whatever the headroom says.
- **A threshold signals; it does not fire.** Budget pressure raises a flag that the ladder should run **at the next boundary**. A system that reduces the moment a number is crossed has moved the decision from the boundary to the counter, which is exactly CXT-2's failure.

### 4.4 Directing a reduction

CXT-8's *directed* obligation is small and load-bearing: the reduction is told what the **next** chunk needs, not asked to preserve importance in general. An undirected reduction optimizes a notion of importance derived from the material being reduced; a directed one optimizes for the consumer that has not started yet. The difference shows up precisely on the decisions that mattered to the next step and looked minor in the transcript.

### 4.5 Failure modes named

| Failure | What it looks like | Which invariant closes it |
| --- | --- | --- |
| Compaction reflex | Every boundary resolves to *compress* | CXT-3, CXT-4 |
| Threshold firing | A reduction lands mid-work; the actor loses the thread | CXT-2 |
| Cheap wipe | State discarded because nothing argued for keeping it | CXT-5 |
| Artifact for no one | A handoff written for the same actor, same place | CXT-6 |
| Nominal hiding | Later steps "hidden" by an inline call, still resident | CXT-9 |
| Degraded continuation | Work pushed on because space remains | CXT-10 |
| Invisible loss | A missing rationale nobody can attribute | CXT-11 |

## nodus-relevance mapping

- **Section boundaries are the workflow-grain boundary.** A workflow's declared sections are exactly the boundaries CXT-2 requires: the runtime already knows where one coherent chunk ends, so the ladder has a natural firing point rather than a heuristic one.
- **A checkpoint is a delegate rung with a human on the other end.** The scoped-unit-plus-report shape of CXT-7 is the same shape as a dialog step that hands out a bounded question and takes back an answer; the caller's state is untouched in both cases.
- **The transition record belongs in the execution trace.** CXT-11 is one more event class in the append-only trace the runtime already emits — no new persistence, and it makes a reduced run auditable at the same grain as every other step.

## 5. Implementation Notes

1. **Detect boundaries before wiring thresholds.** A threshold with nowhere legitimate to fire will fire illegitimately. Boundary detection is the prerequisite, not the enhancement.
2. **Make the band a value, not a feeling.** CXT-10 requires a declared number the system holds and reports. Where it is per-actor it belongs with the actor's declaration; where it is global it belongs in policy.
3. **Record the test, not just the choice** (CXT-12). "Compressed" is a log line; "compressed — continue inadmissible: next chunk does not need this verbatim, band exhausted" is a decision a human can dispute.
4. **Archive before you reduce.** CXT-11 records the decision; the durable content lives in the evidence archive. The ordering is archive-then-reduce, matching the compression family's own rule.
5. **Do not build a sixth rung.** A tempting "partial clear" or "selective forget" is either compression with a narrower eligibility set or discard with a smaller scope. Route it to the rung it actually is; the ladder's value is that it is closed.

## 6. Drawbacks & Alternatives

- **The ladder adds a decision where there was a reflex.** Real. Bounded by CXT-4: the first test is usually the one that passes, and it is the cheapest to evaluate. A ladder whose first rung is *do nothing* costs almost nothing on the common path.
- **Boundary detection is imperfect.** Accepted and stated (CXT-12). A missed boundary defers a transition to the next one, which is the safe direction; a false boundary triggers an early ladder run whose first rung will usually be *continue*.
- **The sharp band is a soft number.** Accepted: it is a policy value chosen from observation, and CXT-10 requires only that it be declared and honoured, not that it be exact. A declared approximate bound is strictly better than an undeclared one, which is what "space remains" amounts to.
- **Alternative — decide by remaining headroom alone.** Rejected by CXT-2 and CXT-4: headroom answers *whether there is room*, never *whether the next chunk needs this one verbatim*. The most important admissibility test in the ladder is not a capacity question at all.
- **Alternative — always externalize, so every transition is portable.** Rejected by CXT-6: it converts every boundary into a document written for nobody, pays CXT-1's cost universally, and trains readers to ignore artifacts because most of them carry nothing they needed.
- **Alternative — fold into `l1-context-compression`.** Rejected: CC's subject is a *transform* and its guarantees are about fidelity; this spec's subject is a *choice among five dispositions*, of which compression is the last rung. Folding the choice into one of its options is what produced the compaction reflex in the first place.
- **Alternative — fold into `l1-context-degradation`.** Rejected: degradation describes an actor operating with less than it needs and how it should behave there. This spec governs the moves that decide how much it has, taken before that state is reached; CXT-10 is the seam between them, not a merge point.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[COMPRESS]` | `.design/main/specifications/l1-context-compression.md` | The transform the last rung invokes; fidelity, eligibility, recoverability |
| `[HANDOFF]` | `.design/main/specifications/l1-derived-artifact-handoff.md` | The externalize rung's artifact contract |
| `[DEGRADE]` | `.design/main/specifications/l1-context-degradation.md` | The state CXT-10 refuses to walk into |
| `[ARCHIVE]` | `.design/main/specifications/l1-evidence-archive.md` | Durable substrate that keeps a reduction recoverable |
| `[PROVENANCE]` | `.design/main/specifications/l1-context-provenance.md` | Where CXT-11's transition record lives |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.1.0 | 2026-10-22 | Core Team | Added CXT-13 and CXT-14 from the user's backlog (context rot: a handoff file written during a session, the next session reading it first): work continuing in a fresh window continues from a handoff record with six fixed parts — goal, current state, active artifacts, changes made, failed attempts and next steps — attached to the work unit rather than the window and superseded rather than appended; the record is a secondary source, checked against the authorities it names before the next window acts, the authority winning and the disagreement reported. Where it is stored is the realization's: `l2-session-checkpoint` §4.11 (the work unit's run record, or the session journal for work with no card). |
| 1.0.0 | 2026-08-26 | Core Team | Initial concept — the decision the token-economy family leaves unmade: which disposition of a working context, and when taking one is legitimate at all. Every transition converts a primary source into a secondary one and the loss is the *why*, unrecoverable from the artifacts produced (CXT-1); the decision belongs **at a work boundary and nowhere else** — a threshold signals, never fires, and a mid-chunk reduction removes what the work is still using (CXT-2); a closed, **cost-ordered** disposition set — continue / discard / externalize / delegate / compress — first admissible wins, no reordering and no sixth rung (CXT-3); continue is examined first because it is the only free move, admissible when the next chunk needs this one verbatim or the sharp band fits it (CXT-4); discard requires **positive evidence of irrelevance**, absence of a reason to keep is not evidence, doubt resolves down the ladder (CXT-5); externalize only for a real crossing — host, location, principal, forked side thread (CXT-6); delegate only what runs unattended and returns a report, the one rung that leaves the caller's primary source intact (CXT-7); compression is the default and **never the first reach**, bound by being directed at what the next chunk needs and by happening at a boundary (CXT-8); hiding later work requires a real context boundary — an inline call leaves them resident and clears nothing (CXT-9); a declared sharp-reasoning band, with the transition taken at the nearest boundary **before** it is exhausted (CXT-10); every transition recorded with what it converted, so flattened rationale is distinguishable from rationale never captured (CXT-11); the order is the contract, the verdict is not claimed mechanical, and the admitting test is surfaced so a human can dispute a step rather than a result (CXT-12). Concept-only. |
