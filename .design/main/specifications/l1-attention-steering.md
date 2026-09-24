# Attention Steering

**Version:** 1.0.1
**Status:** Stable
**Layer:** concept

## Overview

Attention steering is the contract for the **inverse of directability**: not the human steering the office, but the office **moving the human's live view** — putting a specific artifact in front of the person who is watching, annotating it in place, and narrating a walkthrough through the surface the human already has open.

It rests on one structural precondition the rest of the corpus assumes without ever stating: a live human-facing surface must be an **addressable object with an out-of-band control channel**, not an interactive terminal that an agent could only influence by pretending to be the user's hands. Once a surface is addressable, "explain this change to me" stops being a wall of text in a chat pane and becomes: read the structure, move the view to the place that matters, leave the note beside the thing it explains, and say the sentence that connects them.

The scope is the *steering* — addressing, reading, moving, annotating, replacing, and the economy that governs how often any of that is allowed. What the annotations are anchored to is [l1-semantic-addressing.md](l1-semantic-addressing.md); whether the surfaces agree about what they show is [l1-surface-parity.md](l1-surface-parity.md).

## Related Specifications

- [l1-directability.md](l1-directability.md) — the mirror direction. Directability governs the human steering autonomous work; this spec governs autonomous work steering the human's *view*. Both are overlays on an office that runs without either.
- [l1-human-intervention.md](l1-human-intervention.md) — HI-1's out-of-band human edit and AST-2's out-of-band agent steering are the two halves of one co-presence: each party acts on the shared artifact through its own channel, and neither impersonates the other.
- [l1-review-checkpoint.md](l1-review-checkpoint.md) — a checkpoint asks a human for a bounded decision; AST-4/AST-6 are how the system puts the *subject* of that decision in front of them before asking. AST-7's withdrawal rule is what happens when the subject is replaced while the question is open.
- [l1-semantic-addressing.md](l1-semantic-addressing.md) — every steering act names its target with an address from that grammar; a steering channel that addressed by screen position would break on the first reload.
- [l1-progressive-disclosure.md](l1-progressive-disclosure.md) / [l1-agent-tool-ergonomics.md](l1-agent-tool-ergonomics.md) — AST-5's structure-first read is progressive disclosure applied to a *live* surface; ATE-3's absence-as-signal is what AST-11 applies when no surface is attached.
- [l1-tool-composition.md](l1-tool-composition.md) — TC-8's effect classes are how an unattended run refuses steering effects that have no audience.
- [l1-navigation-model.md](l1-navigation-model.md) / [l1-office-visualization.md](l1-office-visualization.md) — the surfaces steering acts upon; steering moves within their existing structure and never invents a mode of its own.
- [l1-security.md](l1-security.md) / [l1-context-provenance.md](l1-context-provenance.md) — the control channel is a local, authenticated seam, and an annotation arriving through it is agent-attributed content, not human intent.
- [l1-notes.md](l1-notes.md) — the durable artifact tier AST-10 distinguishes the live projection from.

## 1. Motivation

**A running interface is the one thing an agent cannot currently reach.** Every other artefact the office touches — files, cards, memories, documents — is addressable. The window the human is actually looking at is not: it exists only as pixels and an input loop. So an agent that has just understood something worth showing has exactly one move available, which is to describe it in words and hope the human finds it. That is a translation from a spatial fact ("this line, in this file, in this hunk") into prose and back again, performed by the party least equipped to do it.

**The workaround is worse than the gap.** Absent a control channel, the tempting fix is to have the agent drive the interactive surface as if it were the user — synthesizing keystrokes, typing into the same input the human is typing into. That races with the human's own input, produces acts with no attribution, cannot be refused, and leaves nothing in the trace. A surface an agent can *impersonate a user on* is strictly worse than one it cannot touch.

**Attention is the scarcest resource in the system and the only one nothing meters.** Token budgets, allowances, and cost ratings are all specified. Yet a steering act spends something none of them measure: it takes the human's place away. Ten annotations, each individually reasonable, jump the view ten times and destroy the reading the human was doing. Without an explicit economy, a helpful agent becomes an interruption engine — and the pathology is invisible in every metric the system already collects.

**Replacement is a different act from movement, and conflating them loses decisions.** Moving the view is cheap and reversible: the human can go back. Replacing what the view is *showing* discards their position, and — the part that actually breaks — silently strands any question the system had already asked them about the old content. A pending "approve this?" answered after the subject was swapped is an answer to a question nobody asked.

**Live is not durable, and an agent will conflate them.** What an agent can enumerate from a live surface is whatever that session happens to be holding. Presented as "the review", it becomes a false completeness claim: annotations added through other channels, from earlier sessions, or dropped by a reload are simply not in it. The failure is not the incompleteness — it is presenting an incomplete list as a complete one.

## 2. Constraints & Assumptions

- **Co-presence is optional.** The office runs fully without any human watching (OFF-5/OFF-8). Steering is an overlay that exists only while a surface is attached, and its absence is never an error.
- **The channel is local-first.** Control of a surface is on-device and session-brokered by default; remote steering is a separate, explicitly granted capability, not a consequence of the surface being addressable.
- **The human keeps the surface.** Steering moves the view and adds agent-attributed content. It never takes the input focus, never types for the user, and never removes their ability to go somewhere else.
- **Surfaces differ in what they can be steered to.** A board, a document, a graph, and a diff expose different unit vocabularies. This spec governs the shape of steering, not a universal set of targets.
- **The steering agent may be remote to the surface's process.** The contract is a protocol boundary, not an in-process call, and must survive the surface restarting underneath it.

## 3. Core Invariants

Rules every Layer 2 implementation MUST NOT violate. They are technology-neutral.

- **AST-1 (A live surface is an addressable object with a declared scope):** every human-facing live surface registers itself under a **stable session identity** carrying what it currently shows and what it is rooted at, discoverable by an authorized external actor without the human doing anything. A surface that is not addressable MUST NOT be steered by proxy: the correct behavior when nothing is registered is AST-11, never simulation of the user's input.

- **AST-2 (Steering is out-of-band; keystroke impersonation is prohibited):** an actor steers a surface **only** through its declared control surface, never by injecting input into the interaction path the human is using. Synthesizing the user's own input races their typing, produces acts with no attribution, cannot be refused by the surface, and leaves nothing auditable. Every steering act is a named request with an actor, a target, and a recorded outcome.

- **AST-3 (Targeting resolves explicitly, auto-resolves only when unique, and refuses ambiguity):** a steering act names its surface by session identity or by scope; when exactly **one** surface matches, it MAY auto-resolve; when several match, the act is **refused with the candidates named**. Picking one for the caller is prohibited — steering the wrong window is invisible to the actor and instantly visible to the human.

- **AST-4 (Read, then move, then annotate — in that order):** an actor reads the surface's current structure and focus before moving it, and moves the view to a target before annotating that target. Annotating a place the human is not looking at produces a note discovered later out of context; moving without reading produces a jump to a target that is not there. The ordering is a contract, not advice.

- **AST-5 (Structure first, payload by explicit request):** the default read of a live surface returns its **structure** — the units present, their addresses, their spans, the current focus — and not their content. The expensive body is a per-call opt-in. A read surface that returns everything by default makes the cheap orienting call unaffordable, so it stops being made (composing ATE-5 and progressive disclosure).

- **AST-6 (Focus is a metered act, requested per act, never a side effect):** moving the human's attention is **explicitly requested each time** and is never an implicit consequence of writing an annotation. The value bar for spending it is *the human would not have found this themselves*; annotating every unit is a defect and not thoroughness. Steering frequency is observable, because an interruption pathology is invisible in every other metric the system collects.

- **AST-7 (Replacing content is a higher authority than moving within it, and it withdraws open questions):** *navigation* moves within what is loaded; *replacement* changes what is loaded and discards the human's position. Replacement requires a distinct grant, is attributed with a reason, and — the load-bearing half — every **outstanding request for a human decision about the replaced content is withdrawn**, not carried forward and not auto-answered. An answer that arrives about a subject that has been swapped is an answer to a question nobody asked.

- **AST-8 (Authorship asymmetry — agents may not author as the human):** on a shared surface, an actor may create and modify only **its own attributed** content. Human-authored content it may enumerate, and may remove **on explicit instruction**, but never author, edit, or re-attribute. Attribution travels with the artifact and is never inferred from which channel it arrived on. (The authority-preserving half of HI-3, applied to the live surface.)

- **AST-9 (A batch lands whole or not at all):** a multi-item steering payload is **validated completely before any of it mutates the live surface**. Partial application in front of a watching human produces a surface that is neither the old state nor the intended one, and no way to tell which items landed. A rejection names the offending item and changes nothing.

- **AST-10 (The live view is a projection, never the record):** what the surface holds is session-scoped. Any enumeration obtained from it MUST be **presented as what it is** — the content this session currently holds — and never as the complete record of the artifact. The durable record lives in the artifact tier; a live enumeration that arrives labelled as complete converts a projection into a false completeness claim.

- **AST-11 (No attached surface is a first-class answer):** where nothing is registered, the actor **says so and stops**. It does not launch a surface on the human's behalf, does not fall back to driving the interactive path, and does not proceed as if the steering acts had landed. Absence is the one signal that cannot be misread; launching the human's windows for them is an act with no consent behind it.

## 4. Detailed Design

### 4.1 The steering loop

```
                 ┌── nothing registered ──▶ say so, stop (AST-11)
attach ──────────┤
  (discover)     └── one or several ──▶ resolve or refuse (AST-3)
                                            │
                    ┌───────────────────────┘
                    ▼
             read structure (AST-5)           ← cheap, repeatable
                    │
                    ▼
             move the view (AST-4/AST-6)      ← metered, explicit
                    │
                    ▼
             annotate in place (AST-8/AST-9)  ← attributed, atomic
                    │
                    ▼
             narrate the connection           ← the sentence that pays for the jump
```

Replacement (AST-7) enters this loop only from outside it: it is not a step in the walkthrough, it is the decision to walk through something else.

### 4.2 Movement versus replacement

| | Navigation | Replacement |
| --- | --- | --- |
| Changes | where the view points | what the view holds |
| Human's position | recoverable | discarded |
| Open decision requests | unaffected | **withdrawn** (AST-7) |
| Authority | ordinary steering grant | distinct grant, attributed reason |
| Failure mode | a jump the human did not want | a decision answered about the wrong subject |

The asymmetry is why they are separate acts rather than one act with a flag. A flag makes the expensive case reachable by a typo.

### 4.3 The attention economy (AST-6)

Every steering act draws on a budget with no meter. Three rules keep it solvent:

1. **Steering is opt-in per act.** Writing an annotation does not move the view. An actor that wants both asks for both, which makes the second one a decision.
2. **The bar is discovery, not coverage.** The act is justified when the human would not have found the thing themselves. Walking every unit in order is the surface's own job, and the human already has it.
3. **Order by the story, not by the structure.** A walkthrough visits units in the order that explains them, which is rarely the order they are stored in. This is the whole reason steering exists — a list the human could sort themselves needs no agent.

### 4.4 Why live enumeration is not the record (AST-10)

An actor asking a live surface "what annotations exist here" receives an honest answer to a narrow question and will treat it as an answer to a broad one. The narrow question is *what is this session currently holding*. Content added through another channel, content from a previous session, and content a reload remapped or dropped are all outside it.

The remedy is at the source, not at the reader: the answer states its own scope. An enumeration that names itself "this session's current content" cannot be quoted as "the complete record" without the quote being visibly wrong, and an actor reasoning from it will reach for the durable tier when completeness actually matters.

### 4.5 Nodus relevance

**No new language invariant.** A steering act is an ordinary effectful host command bound at a step, and NL-9's typed I/O already carries its target address and its outcome. Two existing mechanisms carry the important parts:

- **Effect class** ([l1-tool-composition.md](l1-tool-composition.md) TC-8): steering is an *attention* effect. An unattended run — a scheduled routine, a background loop, a graded environment run — declares `!!NEVER` over that class, so a workflow written for an attended walkthrough degrades to silence rather than steering a surface nobody is watching. This is exactly the kind of constraint NL-2 exists to make absolute.
- **Capability-declared, fail-fast** ([l1-nodus-environment.md](../../nodus/specifications/l1-nodus-environment.md) NE-10): a workflow whose value depends on an attached surface declares that requirement and fails at validation rather than half-executing into AST-11.

Adding a steering vocabulary to the language would name host-specific surface kinds the portable core must not know about.

## 5. Implementation Notes

- Build the **read** path first and make it cheap. Every other act in the loop presumes an orienting call that an actor can afford to make repeatedly; if that call is expensive, actors skip it and start guessing targets, and AST-4 collapses.
- Keep the steering channel's transport distinct from the surface's own input handling in the code, not merely by convention. The moment they share a path, AST-2 becomes a rule enforced by discipline instead of by structure.
- Log steering acts with their target address and their justification alongside ordinary effects; the interesting number is *acts per attended minute*, and nothing else in the trace will reveal it.
- Make the withdrawal in AST-7 an explicit outcome on the pending request (`withdrawn: content replaced`), not a silent cancellation. A human who was mid-decision deserves to see why the question disappeared.
- Test the ambiguity refusal (AST-3) and the batch rejection (AST-9) as behavior. Both are paths that only occur under conditions a happy-path test never creates, and both fail silently and expensively in production.

## 6. Drawbacks & Alternatives

- **A control channel is attack surface.** A surface that can be driven externally can be driven by anything that reaches the channel. Mitigated by locality, session brokering, and authorization — not eliminated; this is why remote steering is a separate grant rather than a consequence of AST-1.
- **The attention economy is unenforceable by construction.** AST-6 makes over-steering observable and nameable; nothing prevents an actor from spending the budget badly. That is the honest limit: this spec converts an invisible pathology into a measurable one.
- **Alternative — let the agent drive the interactive surface as the user:** rejected by AST-2. It appears cheaper (no new channel) and is strictly worse: unattributable, unrefusable, racy against the human's own input, and invisible in the trace.
- **Alternative — no live channel; the agent describes and the human navigates:** rejected. It puts the translation from spatial fact to prose and back on the party least equipped to do it, and it is the status quo this spec exists to replace.
- **Alternative — mirror the surface into the agent's context instead of steering it:** rejected. It scales with content size rather than with interest, and it produces a *second* view that immediately diverges from the one the human is looking at (the failure class [l1-surface-parity.md](l1-surface-parity.md) exists to prevent).
- **Surfaces will want steering vocabulary this spec does not define.** Deliberate: the unit vocabulary belongs to each surface. The risk is each surface inventing an incompatible one, which is why the addresses come from a single grammar (SA-1) even where the units do not.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[ADDRESS]` | `.design/main/specifications/l1-semantic-addressing.md` | The address grammar every steering act targets with |
| `[DIRECT]` | `.design/main/specifications/l1-directability.md` | The mirror direction — human steers work, this steers the view |
| `[INTERVENE]` | `.design/main/specifications/l1-human-intervention.md` | HI-3 authorship authority that AST-8 applies to the live surface |
| `[CHECKPOINT]` | `.design/main/specifications/l1-review-checkpoint.md` | The decision requests AST-7 withdraws on replacement |
| `[ERGONOMICS]` | `.design/main/specifications/l1-agent-tool-ergonomics.md` | ATE-3/ATE-5 — absence-as-signal and budget shape |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.1 | 2026-09-24 | Core Team | Consistency pass (2026-09-24): The NE-10 link pointed at `l1-nodus-environment.md` inside this workspace, where no such file exists; it now resolves to the nodus workspace. |
| 1.0.0 | 2026-08-13 | Core Team | Initial concept: the **inverse of directability** — autonomous work moving a co-present human's live view. A live surface is an **addressable object with a declared scope**, so steering never requires simulating the user (AST-1); steering is **out-of-band**, and keystroke impersonation is prohibited because it races the human's input, carries no attribution, and cannot be refused (AST-2); targeting **auto-resolves only when unique** and refuses ambiguity by naming candidates, since steering the wrong window is invisible to the actor and instant to the human (AST-3); **read → move → annotate** as a contract, not advice (AST-4); **structure first, payload by request**, or the cheap orienting call becomes unaffordable and stops being made (AST-5); **focus is metered and requested per act**, never a side effect of annotating, because the interruption pathology is invisible in every metric the system already collects (AST-6); **replacement is a higher authority than movement and withdraws open decision requests**, since an answer about a swapped subject answers a question nobody asked (AST-7); **authorship asymmetry** — an agent may enumerate and, on instruction, remove human content, never author or re-attribute it (AST-8); a batch **lands whole or not at all** in front of a watching human (AST-9); the live view is a **projection, never the record**, and enumerations state their own scope (AST-10); **no attached surface is a first-class answer** — say so, never launch the human's windows or fall back to driving them (AST-11). §4.2 tabulates movement vs replacement; §4.3 states the attention economy; §4.5 records the nodus disposition — no new invariant, steering is an *attention* effect class an unattended run forbids with `!!NEVER`. |
