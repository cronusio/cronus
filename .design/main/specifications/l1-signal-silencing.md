# Signal Silencing

**Version:** 1.0.1
**Status:** Stable
**Layer:** concept

## Overview

A long-running system that watches things produces recurring signals, and some of them a person has already judged and does not want to hear again. Every such system therefore grows a way to make one of them stop — a mute, a snooze, a do-not-disturb switch, a per-rule silence. That control is treated as a convenience and specified nowhere, which is how it becomes the quietest hole in an observability stack: **the one control whose whole purpose is to stop telling you things is also the one nobody verifies is working.**

The fault-lifecycle contract already draws the line this spec stands on. FL-9 says archival suppresses a fault's *notification* and never its recording, and it explicitly distinguishes that act from "silencing an alert rule", which "withholds a chosen rule's output for a stated time and belongs to the alerting layer." That layer is named there and specified nowhere. This is it.

The failure this discipline exists to close is not noise. It is a silence that **looks applied and is not** — a key that matches nothing, so every future occurrence is delivered while the record says the signal was muted — and its mirror, a silence that is **broader than the person asked for**, where one name stands for a whole class and the rest of the class goes quiet unannounced. Both report success. Neither is discoverable from the outside, because the evidence that something is wrong is precisely the thing that was suppressed.

## Related Specifications

- [l1-fault-lifecycle.md](l1-fault-lifecycle.md) — FL-9 names this layer and hands it off: archival is a judgement about *one fault* that travels with it forever; silencing withholds *a chosen rule's* output for a stated time. This spec owns the second act. FL-9's "suppresses notification, never recording" is the parent of SIL-1, and FL-8's escalation (a known fault whose rate rose is re-surfaced) is the one signal that must survive a silence.
- [l1-error-reporting.md](l1-error-reporting.md) — ERR-3 de-duplicates *reports*; this spec withholds *deliveries*. A de-duplicated report is still delivered once; a silenced signal is delivered zero times and recorded anyway.
- [l1-attention-steering.md](l1-attention-steering.md) — AST-6 meters an agent's *acquisition* of a person's attention. This spec governs the person's own standing refusal of it. Both treat attention as scarce and metered; they point in opposite directions and never overlap.
- [l1-anomaly-consensus.md](l1-anomaly-consensus.md) — ANC-2's unanimity gate and ANC-5's assistance-not-paging exist so a detector does not manufacture the noise a person would then have to silence. This spec assumes that work failed sometimes anyway.
- [l1-action-gating.md](l1-action-gating.md) — AG-5 names friction-fatigue as a first-class failure. A silence is what a person reaches for when fatigue wins; SIL-6 keeps it from becoming the answer to a problem that had a fix.
- [l1-cooperative-status-projection.md](l1-cooperative-status-projection.md) — ASP governs how a status *reaches* a surface; this spec governs a standing decision that it must not. Silencing is a property of the signal and its subject, never of one renderer.
- [l1-diagnostic-log.md](l1-diagnostic-log.md) — level scoping decides what is *written*; silencing decides what is *delivered*. A silenced signal is still written at full fidelity.

## 1. Motivation

Left unspecified, every implementation converges on the same four defects, and three of them are silent:

- **A silence that matches nothing.** The key is derived from a truncated or normalized form of the subject's name — a process name clipped to a fixed width, a path reduced to a basename, an identifier lowercased by one layer and not another. The control accepts it, reports success, lists it among the active silences, and matches no occurrence, ever. The person believes they have handled it and hears the signal again with no explanation for why the mute failed.
- **A silence broader than the request.** The only key available is coarser than the thing the person named: an interpreter standing in for every program it runs, a rule id standing for every subject it fires on, a host standing for every service on it. Accepting the request silently silences the rest of the class. The next real failure in that class is the one that is not delivered.
- **A silence that destroys the record.** The signal is not delivered *and* not written, so "what did I miss while this was off" has no answer, and the fault's own rate-tracking — the input to escalation — is starved by the mechanism that was supposed to affect only delivery.
- **A silence used in place of a fix.** The signal was correct, the defect was within reach, and the cheapest available act was the one that made the symptom invisible. This is the only one of the four that is visible at the time, and it is the one most likely to be chosen anyway.

A fifth defect arrives from the other side. Where a silence exists, a sender that *wants* attention learns to defeat it, and the first thing it reaches for is the priority field it fills in itself. A gate that reads a self-declared urgency is a gate operated by the party with the strongest interest in opening it.

## 2. Constraints & Assumptions

- **Delivery and recording are separable.** The system can withhold a signal from a person while still writing, counting, and rate-tracking it. Where they are not separable, this contract cannot be honoured and the control must not be offered.
- **The subject of a silence has a name, and the name may not be the identity.** The string a person can type is often a rendered, truncated, or normalized projection of the underlying identity, and the two match by coincidence rather than by construction.
- **Occurrences arrive after the silence is set.** A key can only be validated against what has already been seen; matching future occurrences is an assumption, and it is the assumption that fails.
- **Silences accumulate and outlive their reasons.** A silence set once is rarely revisited, so anything about it that must remain true has to be checked by the system rather than remembered by the person.
- **The emitter may declare a signal ephemeral.** Some signals are genuinely worthless once unseen — a transient progress toast, a confirmation of an act the person just performed. Only the emitter knows this, and only at emission.

## 3. Core Invariants

Rules every Layer 2 implementation MUST NOT violate. They are technology-neutral.

- **SIL-1 (Silencing withholds delivery, never observation):** a silence suppresses **only the delivery of a signal to a person**. Recording, counting, rate-tracking, fault-identity assignment (FL-1) and lifecycle transitions continue **unchanged and at full fidelity** while it is in force, so a silence never alters what the system knows about itself — only what it says. The single exception is a class the **emitter declared ephemeral at emission**, which MAY be dropped rather than recorded; that declaration is the sender's and is made at send time, never a judgement the silencing layer makes on the sender's behalf. A silence that also stops the writing is not a silence, it is deletion with a friendly name.

- **SIL-2 (A key is resolved against real occurrences before the silence is accepted):** setting a silence **resolves its key against the recorded occurrence history and reports what it matched**, and a key matching nothing is **refused or accepted only over an explicit confirmation that names the emptiness**. This closes the worst of the three silent failures: a silence that matches nothing is indistinguishable, from the person's side, from a silence that works — it is listed, it reports success, and it delivers every future occurrence. Where the key must be derived from a rendered name, the derivation is performed **from the identity the matcher will actually use**, never from the display form, and the two are reconciled at set time rather than at match time.

- **SIL-3 (Breadth is disclosed at the moment it is set, in the terms the person used):** where the coarsest available key is **broader than the subject the person named**, the surface **states what else falls silent** before accepting — naming the class, not merely warning that one exists. Silently widening a request is forbidden: the person asked for one thing to stop and got a class, and the class's next genuine failure is the one that goes undelivered. Where a narrower key exists, it is preferred; where none exists, the breadth is part of the decision being consented to, not a footnote discovered later.

- **SIL-4 (Every silence is offered with its reversal, and the active set is listable):** the act that sets a silence **states how to lift it** in the same interaction, and the system exposes a **listing of every silence currently in force** with its subject, breadth, owner, reason, and expiry. A silence a person cannot find is a permanent one regardless of what it was called, and the surface that creates a one-way door has mis-specified a convenience as a commitment.

- **SIL-5 (Bounded by time or condition; an unbounded silence is an explicit, attributed choice):** every silence carries **either an expiry or a condition for return** (FL-9's condition-for-return generalized to the rule grain). Indefinite silence exists, but it is chosen explicitly, attributed to whoever chose it, and rendered differently from a bounded one. A default-permanent silence is where problems go to be forgotten rather than decided, and it is indistinguishable from a fix.

- **SIL-6 (Silence is never offered in place of an available fix):** where the signal is correct and its cause is **within reach of the actor proposing the silence**, proposing the silence instead is a wrong answer, not a lesser one. Silencing is for a signal that is *understood and still unwanted* — a known third-party defect, a condition already tracked elsewhere, an accepted operating state. It is not a disposition for a signal nobody has explained, and an agent MUST NOT propose one before the diagnosis it would be silencing has been produced (composing OWN-1: the ownership verdict comes first).

- **SIL-7 (Punch-through is decided by attributes the sender cannot profitably forge):** where some signals are permitted through a silence, the rule admitting them **MUST NOT rest on a priority, urgency, or severity value the sender sets itself**. A self-declared priority is a claim by the party with the strongest interest in being heard, and any population of senders sharing one channel converges on declaring the maximum. An admission rule therefore combines the declared level with **provenance the sender cannot set to its own advantage** — which component emitted it, whether it followed a privileged internal path, whether it directly answers an act the person just performed — and the combination, not the declaration, is what opens the gate. The set of punch-through classes is closed, small, and stated.

- **SIL-8 (What was withheld is reviewable, and the review is the point):** a person can read **what a silence withheld** — every non-ephemeral signal suppressed while it was in force, in its own record, addressable by the silence that suppressed it. "What did I miss" is the question a silence creates and is obliged to answer; a suppression whose contents cannot be reviewed afterwards has converted a delivery decision into a data-loss decision.

- **SIL-9 (Escalation survives silence, or the silence declares that it does not):** a silence does **not** by default suppress FL-8 escalation — a known fault whose rate rose above its own baseline is a materially different signal from the one that was silenced, and being silenced must not become a hiding place. Where a silence is intended to cover escalation too, that is an **explicit, separately-consented property of the silence**, not the default, and it is rendered as what it is: an agreement not to be told even if this gets worse.

- **SIL-10 (Instance, rule, and global are three different objects, and the control says which one it sets):** silencing *this fault*, silencing *the rule that produced it*, and silencing *everything* are distinct acts with distinct blast radii and distinct owners, and a surface MUST NOT collapse them into one control whose scope the person infers from context. The instance grain is FL-9's archival and belongs to whoever triages the fault; the rule grain belongs to whoever operates the alerting layer; the global grain is a mode with a visible indicator, because a system that is silent everywhere must look different from one that has nothing to say.

> An L2 implementation cannot reach RFC until every invariant above is addressed in its Invariant Compliance section.

## 4. Detailed Design

### 4.1 The set path

```
person asks to silence <subject>
        │
        ├─ derive key from the MATCHER'S identity, not the display form   (SIL-2)
        ├─ resolve key against recorded occurrences
        │      matched 0 → refuse, or confirm explicitly "this matches nothing yet"
        │      matched a wider class than <subject> → state the class     (SIL-3)
        ├─ is the cause within reach of this actor?  → propose the fix    (SIL-6)
        ├─ bind expiry or return condition                                 (SIL-5)
        ├─ record owner + reason                                           (SIL-9 audit)
        └─ confirm, naming: what falls silent, until when, how to lift    (SIL-4)
```

The ordering is load-bearing. Resolution precedes acceptance because a key that matches nothing must never reach the active set; breadth disclosure precedes acceptance because the person is consenting to the class, not to the name they typed.

### 4.2 The delivery path

A silence is consulted **after** the signal is fully constructed, identified, recorded, and counted — never before. This is what makes SIL-1 structural rather than disciplinary: the suppression point sits downstream of every observation the system performs on its own behalf, so no future change can accidentally route recording through the silence check.

### 4.3 Failure modes named

| Mode | What it looks like | Which invariant closes it |
| --- | --- | --- |
| Phantom silence | Set, listed, reports success, matches nothing, ever | SIL-2 |
| Silent widening | One name typed, a class silenced, no mention | SIL-3 |
| One-way door | No listing, no expiry, no stated reversal | SIL-4, SIL-5 |
| Suppression as data loss | Nothing delivered and nothing kept | SIL-1, SIL-8 |
| Mute-instead-of-fix | The cheapest act made the symptom invisible | SIL-6 |
| Priority inflation | Every sender declares critical to punch through | SIL-7 |
| Hiding place | A worsening fault stays quiet because it was known | SIL-9 |
| Scope confusion | One control, three blast radii, inferred from context | SIL-10 |

## 5. Implementation Notes

- The key derivation in SIL-2 is the single highest-value place to spend implementation care. Where an identity has both a full and a truncated form, the silence stores the form the matcher uses and renders the form the person recognizes; storing the rendered form is the defect.
- Any string a person supplies as a key is data on the way to a matcher, never a fragment of a command, a pattern, or a query the system then evaluates. The subject of a silence is frequently named by a third party (a crashing program, a remote host, an imported rule), so its name is untrusted input by construction.
- SIL-8's review surface reuses the evidence plane rather than creating a second one: what was withheld is already recorded by SIL-1, so the review is a query, not a parallel store.

## 6. Drawbacks & Alternatives

- **SIL-2 makes setting a silence slower and can refuse a request the person is sure about.** Held: the refused case (silencing something not yet seen) is rare and expressible over a confirmation, while the case it prevents is undetectable by design.
- **SIL-3 makes some silences feel heavy-handed.** Accepted. The alternative is a person who believes they muted one program and has muted a hundred, learning this from the failure that was not delivered.
- **SIL-7 means some legitimate urgent senders are not admitted.** Accepted: a closed, small punch-through set is the only form of the rule that survives contact with a population of senders competing for attention.
- **Alternative — silence as a per-renderer setting:** rejected. A signal silenced on one surface and delivered on another is not silenced, and the person's mental model is of the *thing* being quiet, not of one window.
- **Alternative — sensitivity tuning instead of silencing:** rejected as a replacement (it is a complement). Tuning changes what is *detected*, which changes the record; silencing changes what is *delivered*, which must not. ANC-1's per-series discipline covers the first; conflating them is how a delivery preference silently becomes a detection change nobody can audit.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[FAULT]` | `.design/main/specifications/l1-fault-lifecycle.md` | FL-8 escalation, FL-9 archival — the parent contract that hands off this layer |
| `[REPORT]` | `.design/main/specifications/l1-error-reporting.md` | De-duplication of reports, distinct from withholding deliveries |
| `[ATTENTION]` | `.design/main/specifications/l1-attention-steering.md` | The opposite direction of the same scarce resource |
| `[ANOMALY]` | `.design/main/specifications/l1-anomaly-consensus.md` | Detection-side noise control, upstream of this contract |
| `[OWNERSHIP]` | `.design/main/specifications/l1-fault-ownership.md` | SIL-6's precondition — the ownership verdict precedes any silencing proposal |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.0 | 2026-09-04 | Core Team | Initial concept — the alerting/suppression layer FL-9 names and hands off ("silencing withholds a chosen rule's output for a stated time and belongs to the alerting layer"), specified for the first time. Mined from an external desktop-environment distribution with an agent-facing control surface, whose crash-watcher mute is documented with its own traps: a key built from a truncated process name "matches nothing, forever, while looking like it worked", and a key that is a bare interpreter name silences every program run through it. Ten invariants: delivery withheld, observation never (SIL-1, the emitter-declared ephemeral class the only exception); the key resolved against real occurrences before acceptance, because a phantom silence is indistinguishable from a working one (SIL-2); breadth disclosed at set time in the person's own terms, never silently widened (SIL-3); every silence offered with its reversal and the active set listable (SIL-4); bounded by time or condition, unbounded explicit and attributed (SIL-5); never offered in place of a fix within reach, and never before the diagnosis it would suppress (SIL-6); punch-through decided on attributes the sender cannot profitably forge, since a self-declared urgency is a claim by the interested party and a shared channel converges on maximum severity (SIL-7); what was withheld is reviewable, or a delivery decision has become a data-loss decision (SIL-8); escalation survives silence unless the silence explicitly says otherwise, so being known is not a hiding place (SIL-9); instance/rule/global are three objects with three blast radii and are never one control (SIL-10). Concept-only; no L2 yet. |
| 1.0.1 | 2026-10-22 | Core Team | Citations updated for the requirement-ID prefix renames that make each prefix name one specification (RULES §1, 1.8.0, the person's decision of 2026-10-22): `AC-n` of `l1-anomaly-consensus` is now `ANC-n`. No requirement was added, removed or reworded. |
