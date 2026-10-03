# Intent Resolution

**Version:** 1.2.0
**Status:** Stable
**Layer:** concept

## Overview

How the office turns under-specified client intent into correct action **without interviewing the client and without silently guessing**. A non-technical client states a thin intent ("make me a launch page", "sort out my week"); a literal reading leaves gaps a professional must fill. Two failure modes bracket the problem: *interrogate the client* (a barrage of questions a professional office should not need, and the client often cannot answer) or *guess silently* (build confidently on an unstated assumption and ship the wrong thing). This concept defines the middle path the office actually takes: **ground first** from what is already known, **decide ask-or-assume** by a risk/cost gate, and where it does not ask, **proceed on an explicit, recorded, reversible assumption** — never a hidden guess. It operationalizes the office's "act autonomously, clarify only on genuine ambiguity" posture into a concrete resolution discipline, resolving the standing OFF-5↔OFF-6 tension in the autonomy-first direction.

## Related Specifications

- [l1-office-model.md](l1-office-model.md) - OFF-5 (client-not-managing) and OFF-6 (clarify only on genuine ambiguity); this concept is the resolution mechanism that operationalizes them and closes the §-flagged "acting on misread intent" risk.
- [l1-orchestration.md](l1-orchestration.md) - ORC-3 intent→plan→tasks consumes a *resolved* intent; ORC-9 approval gate is where high-impact assumptions escalate.
- [l1-operational-ledger.md](l1-operational-ledger.md) - Asserted ground-truth facts; an assumption is explicitly the *opposite kind* — provisional and reversible, never asserted as fact (OL semantics preserved).
- [l1-user-model.md](l1-user-model.md) - Confirmed recurring assumptions become stated preferences/defaults; a grounding source consulted before asking.
- [l1-memory-model.md](l1-memory-model.md) - Memory and prior decisions are grounding sources resolved before any question.
- [l2-agent-autonomy.md](l2-agent-autonomy.md) - Risk-class gating (read/write/network/install/destructive) the ask-or-assume threshold composes with; destructive always escalates.
- [l1-task-graph-model.md](l1-task-graph-model.md) - TG-9 drift-driven re-planning: correcting an assumption re-plans dependent work rather than letting it drift.
- [l1-global-orchestration.md](l1-global-orchestration.md) - [ADDED v1.2.0] GO-7: the building-level dispatch that applies IR-9's one attention budget across offices.
- [l1-action-gating.md](l1-action-gating.md) - [ADDED v1.2.0] AG-3: an approval is a gate decision on the friction ladder, a consent act and not a question (IR-9 d).
- [l1-navigation-model.md](l1-navigation-model.md) - [ADDED v1.2.0] the Inbox's Poll/Clarify facet, where held questions stay visible and answerable (IR-6, IR-9).
- [l1-honest-solicitation.md](l1-honest-solicitation.md) - [ADDED v1.2.0] how each ask reads: an ask informs the decision and never moves it (HSO).

## 1. Motivation

The office's defining promise is maximum automation under the hood for a client who is not, and need not be, technical (OFF-5). That promise is in direct tension with correctness: a thin brief under-specifies, and something has to fill the gap. The project already names this as an open risk — acting autonomously while rarely asking risks building the wrong thing — but the escape hatch (OFF-6: "ask only on genuine ambiguity") says *when a question is permitted*, not *how the office resolves the dozens of smaller gaps it must not bother the client with*.

A professional firm does not interview a client about every unstated detail; it uses expertise and context to make reasonable, defensible choices, and flags only the few decisions that genuinely need the client. But "make reasonable choices" silently is exactly how the wrong thing gets built confidently. The missing discipline has three parts:

- **Ground before asking.** Most gaps are already answered — by the operational ledger, prior decisions in memory, the user model, the knowledge base, or sensible domain defaults. A question whose answer the office could have looked up is a failure of grounding, not a need for clarification.
- **Ask only when it pays.** A question is worth interrupting the client only when the gap is *blocking* and the expected cost of building on a wrong assumption exceeds the cost of asking. Cheap, reversible gaps are not worth a question.
- **Assume out loud, never in secret.** When the office does not ask, it must not guess silently. It proceeds on an **explicit, recorded, reversible** assumption — visible to the client, attached to the work, correctable cheaply. The difference between a professional default and a silent guess is entirely whether it is written down and reversible.

This concept makes those three the contract, so "we didn't ask you, we assumed X — here's where to change it" replaces both the barrage and the silent wrong build.

## 2. Constraints & Assumptions

- The client is not required to be technical and is not managing the work (OFF-5); resolution happens under the hood.
- An assumption is provisional and reversible by construction — it is a distinct kind from an asserted operational fact (operational-ledger), and must never masquerade as one.
- Resolution precedes decomposition: orchestration plans from an intent whose gaps are already resolved (grounded, asked, or assumed).
- This concept governs *intent-gap resolution*, not general dialogue; it neither forbids normal conversation nor mandates ceremony on a fully-specified request.
- The ask-or-assume threshold is configurable but defaults to autonomy-first (assume-and-record), biasing toward a question only as blast radius rises.

## 3. Core Invariants

Rules every Layer 2 implementation MUST NOT violate:

- **IR-1 (Ground before ask):** before any question to the client, the office MUST attempt to resolve the gap from available grounding — operational ledger, memory and prior decisions, the user model, the knowledge base, and declared domain defaults. A question whose answer is already obtainable from grounding is a defect, not a clarification.
- **IR-2 (Ask only when blocking and costly — minimal, non-technical, batched):** a question to the client is the exception (reaffirms OFF-6). It is posed ONLY when the gap is genuinely blocking AND the expected cost of building on a wrong assumption exceeds the cost of asking. Questions are intent-level (never technical), batched into the minimal high-leverage set, and never delivered as an interview, a drip, or a quiz.
- **IR-3 (Assume-and-record, never silently guess):** where a gap is not asked, the office proceeds on an **explicit, recorded assumption** carrying its statement, rationale, and confidence — never a hidden guess. An action taken on an unrecorded assumption is a protocol violation; the recorded assumption is the unit that makes autonomy auditable.
- **IR-4 (Assumption ≠ fact; reversible by default):** a recorded assumption is provisional and reversible, and is stored distinctly from asserted operational facts (it never enters the ledger as ground truth). The client or later evidence can correct or supersede it at any time; the original is not silently mutated (supersede-don't-mutate).
- **IR-5 (Risk-proportional ask-or-assume):** the threshold between asking and assuming scales with blast radius. Irreversible or high-impact gaps — payments, finances, legal/contractual content, destructive actions, external/irrevocable delivery — bias toward ask or an approval gate (composes ORC-9 and the destructive-always-approve rule); low-impact, reversible gaps bias toward assume-and-record. The cost of being wrong, not the mere existence of a gap, sets the bar.
- **IR-6 (Surface at the right gate, not mid-flight):** material assumptions are surfaced to the client at a natural checkpoint — result delivery, an approval gate, or a periodic digest — concise and correctable, not buried and not interrupting flow. The client should be able to see what was assumed and change it cheaply, without being managed during the work (consistent with OFF-5).
- **IR-7 (Correction propagates; assumptions feed learning):** correcting an assumption re-plans the dependent work rather than leaving it to drift (TG-9). A class of assumption the client repeatedly confirms is promoted to a stated default/preference (user model) so the office stops re-deriving it; a class repeatedly corrected becomes a recorded anti-pattern (self-improvement) so the office stops repeating the wrong guess.

- **IR-8 (Interpretation fidelity — confirm a stated intent when its reading is genuinely uncertain):** IR-1…IR-7 resolve what the client left *unsaid*; IR-8 governs the distinct risk of *misreading what they did say*. When the office's confidence in its **interpretation** of a stated idea or requirement is genuinely low — the phrasing admits materially different readings, or the idea is about to be transformed into a durable, high-leverage artifact (a specification, a plan) where a misread would propagate into everything built from it — the office confirms its understanding **before** building on it, by **restating its interpretation back** ("here is what I understood: … — is that right?") for the client to validate or correct. This is not a front-loaded interview (it reaffirms IR-2 / §4.4): it is **uncertainty-gated** — only genuine interpretation-doubt or a high-leverage transform triggers it, never a routine quiz — and it is **lower-friction than a clarifying question**, reflecting the office's own reading back for a cheap confirm/adjust rather than interrogating the client for missing information (that is gap-resolution, IR-2). A confidently-wrong interpretation carried silently into a spec is the failure this prevents; a corrected interpretation propagates and feeds learning exactly as a corrected assumption does (IR-7).

- **IR-9 (Questions come in classes, each with its own bound, and all of them draw on one budget of the client's attention):** [ADDED v1.2.0] IR-2 governs *whether* to ask; this governs how the different kinds of question the office can put to a client relate to each other. The classes are distinct and not interchangeable: (a) an **intake or planning clarification** — a batch put before work is planned from an under-specified brief; (b) a **blocking question mid-work** — a gap that cannot be assumed past (IR-2, IR-5), bounded per task, beyond which the office assumes and records (IR-3); (c) a **post-session ask-back** — a system-drafted question from a session that ended in confusion, bounded per project and never blocking the next task; (d) an **approval** — a gate decision (ORC-9, the action-gating tiers), which is a consent act and is not counted as a question; (e) an **open-clarification measure** — a health figure over a planning artifact, which describes the office's state and is never itself put to the client. The owning specification of each class states its bound, and a bound is a ceiling, never a target: every question still passes IR-1 and IR-2 first. Across classes and across offices the client has **one budget of attention**: whatever a class's own bound, the questions that reach the client are ordered blocking-first, merged when two offices or two classes ask the same thing, and never dropped silently — a question held back is kept, visible and answerable later (IR-6), and one that gains a blocker is promoted. The building-level dispatch that applies this across offices is GO-7. A client-chosen engagement or autonomy profile, if one is offered, modulates only how questions are explained and how often non-blocking ones are batched; it never lowers an approval gate (AG-3) or removes a class bound.

> L2 specs cannot reach RFC status until all invariants here are addressed in their "Invariant Compliance" section.

## 4. Detailed Design

### 4.1 The resolution path

```text
[REFERENCE]
resolve(gap):
    g := ground(gap)                       // IR-1: ledger / memory / user-model / KB / defaults
    if g.resolved: return g.value          // already known — no question, no assumption needed
    if blocking(gap) and cost_if_wrong(gap) > cost_to_ask(gap):   // IR-2 / IR-5
        return ask_client(minimal, intent_level)                 // the exception, batched
    a := assume(gap)                        // IR-3: explicit statement + rationale + confidence
    record(a)                               // visible, reversible, NOT a ledger fact (IR-4)
    return a.value
```

The default branch is `assume-and-record` — the autonomy-first posture. A question is reached only when grounding fails *and* the gap is both blocking and expensive to get wrong.

### 4.2 Grounding precedence (IR-1)

```text
[REFERENCE]
ground(gap) consults, in order, until resolved:
  1. operational ledger   — asserted facts that settle the gap outright
  2. memory / decisions   — what was decided before in this or a related context
  3. user model           — the client's stated/inferred preferences and defaults
  4. knowledge base        — reference material relevant to the gap
  5. domain defaults       — the professional's sensible default for this kind of work
```

Steps 1–3 are *authoritative-ish* (a ledger fact or a stated preference settles it); steps 4–5 inform an assumption rather than eliminate it.

### 4.3 The recorded assumption

```text
[REFERENCE]
Assumption {
  gap         : what was under-specified
  statement   : "assuming the launch page targets desktop-first"
  basis       : "inferred" | "default" | "analogous-prior-work"
  rationale   : why this is the reasonable choice
  confidence  : 0..1
  reversible  : true                 // IR-4 — always; an irreversible choice must ask (IR-5)
  surfaced_at : checkpoint ref       // IR-6 — where the client can see/override it
  status      : "open" | "confirmed" | "corrected" | "superseded"
}
```

An assumption is the audit unit of autonomy: it is what lets the office say, after the fact, exactly which choices it made on the client's behalf and on what basis.

### 4.4 Why not interview

A front-loaded interview is rejected for this office (it is not the client-facing posture): it burdens a non-technical client with questions they often cannot answer, it stalls autonomy, and most of its questions are answerable by grounding (IR-1) or safely assumable (IR-3). The interview's one real value — reaching a buildable spec with no silent guesses — is captured instead by grounding + assume-and-record, with a question reserved for the few blocking, costly gaps (IR-2). The office behaves like an expert firm: it fills gaps with defensible, recorded choices and asks only what genuinely needs the client.

### 4.5 Relationship to validation and automation

Resolution is the *front door*; two adjacent disciplines guard the rest of the lifecycle and are deliberately not re-specified here:

- **Validate-before-build** (proportional human-in-the-loop on high-impact work) is the approval gate (ORC-9) and lookahead; IR-5 feeds it by escalating high-blast-radius gaps rather than assuming them.
- **Promote-to-automation** (turning resolved, repeated work into a skill, then into autonomous operation) is the autonomy ladder and self-improvement promotion; IR-7 feeds it by turning confirmed assumptions into defaults and corrected ones into anti-patterns.

### 4.6 Interpretation fidelity — restate-to-confirm (IR-8)

IR-1…IR-5 resolve what is *missing*; IR-8 guards against misreading what is *present*. The
two are distinct axes and use distinct moves — a gap is *asked or assumed*, a shaky
reading is *reflected back*:

```text
[REFERENCE]
handle_stated_intent(idea, context):
    reading := interpret(idea)
    if confidence(reading) >= threshold(context):      // routine, unambiguous → proceed
        return reading                                   // no confirmation (reaffirms IR-2 / §4.4)
    // genuine interpretation-doubt, OR a high-leverage transform (idea → spec/plan):
    confirmed := restate_and_confirm(reading)            // "here's what I understood: … — right?"
    return confirmed                                     // reflect, don't interrogate; precedes formalizing
// a corrected reading propagates + feeds learning exactly as a corrected assumption does (IR-7)
```

Restate-to-confirm is deliberately *lower-friction* than an IR-2 clarifying question: it
does not ask the client to supply information (that is gap-resolution) — it reflects the
office's own interpretation for a cheap yes/no/adjust, the right move when the doubt is
"did I understand you," not "what did you leave out." It is reserved for genuine
interpretation-uncertainty and for the moment an idea is about to become a durable,
high-leverage artifact where a silent misread would propagate into everything built from
it — the ideation → specification boundary being the canonical trigger.

### 4.7 Question classes and the attention budget (IR-9)

The bounds live with the specifications that own each class, so they can change without this spec changing; the table fixes the classes and what each one is *for*.

| Class | Typical case | Blocks work? | Bound stated by | When held back |
| --- | --- | --- | --- | --- |
| (a) Intake or planning clarification | a batch of scope questions before a plan is written | planning waits for it | `l2-mission-mode` §4.14 | kept in the clarify list, answerable later |
| (b) Blocking question mid-work | which of two incompatible readings to build | that task | `l2-agent-constitution` §4.24 | until the task needs it, then promoted |
| (c) Post-session ask-back | a question drafted after a session that ended in confusion | never | `l2-self-improvement` §4.4 | one pending per project; the rest are not stored |
| (d) Approval | a gate decision before an irreversible act | the gated act | `l1-action-gating`, ORC-9 | not a question — a separate consent channel |
| (e) Open-clarification measure | the count of unanswered planning clarifications in a health score | no | `l2-self-improvement` §4.11 | never put to the client |

Presentation across all of them is blocking-first, merged on identical asks, and never silent about what is held. The Poll/Clarify facet of the inbox (`l1-navigation-model`) is where held questions stay visible, and how each one reads is governed by `l1-honest-solicitation`.

## 5. Drawbacks & Alternatives

- **Recording overhead.** Writing down every assumption is more than guessing. Mitigation: only *material* assumptions need surfacing (IR-6); trivial defaults are recorded cheaply and never shown unless asked. The cost is small against one avoided wrong build (the "40% of the budget spent producing the wrong thing" failure).
- **Wrong assumptions still happen.** Grounding can miss and a default can be wrong. Mitigation: IR-4 reversibility + IR-6 surfacing make a wrong assumption cheap to correct, and IR-7 ensures the same wrong guess is not repeated. The guarantee is correctability, not omniscience.
- **Alternative — interview the client up front.** Rejected for this office: it violates the non-technical, autonomy-first, under-the-hood posture (OFF-5), and most of its questions are answerable without the client.
- **Alternative — pure OFF-6 (ask only on blocking ambiguity, nothing else).** Insufficient alone: it says when to ask but leaves every non-asked gap to a silent guess. IR-3's assume-and-record is the missing half that makes "rarely ask" safe.
- **Alternative — one global question cap (IR-9).** Rejected: the classes differ in what they cost the client and whether work waits (a blocking question and a post-session ask-back are not interchangeable), so a single number would either starve the urgent class or license the idle one. A shared *budget of attention* with per-class ceilings and a blocking-first order keeps both.
- **Alternative — treat assumptions as ledger facts.** Rejected: an assumption is provisional and the ledger is asserted ground truth; conflating them lets soft guesses harden into unquestioned fact (violates OL semantics) — IR-4 keeps them distinct.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[OFFICE]` | `.design/main/specifications/l1-office-model.md` | OFF-5/OFF-6 posture this concept operationalizes. |
| `[ORCH]` | `.design/main/specifications/l1-orchestration.md` | Consumes resolved intent (ORC-3); approval gate (ORC-9) for escalated assumptions. |
| `[LEDGER]` | `.design/main/specifications/l1-operational-ledger.md` | Asserted-fact kind an assumption is deliberately distinct from (IR-4). |
| `[USER-MODEL]` | `.design/main/specifications/l1-user-model.md` | Confirmed assumptions become stated defaults (IR-7). |
| `[AUTONOMY]` | `.design/main/specifications/l2-agent-autonomy.md` | Risk-class gating the ask-or-assume threshold composes with (IR-5). |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.0 | 2026-06-26 | Core Team | Initial spec — autonomous intent-gap resolution without interviewing: ground-before-ask (IR-1), ask-only-when-blocking-and-costly minimal/non-technical/batched (IR-2), assume-and-record-never-silently-guess (IR-3), assumption≠fact/reversible (IR-4), risk-proportional ask-or-assume (IR-5), surface-at-the-right-gate (IR-6), correction-propagates + assumptions-feed-learning (IR-7); operationalizes OFF-5/OFF-6 and resolves the acting-on-misread-intent risk; adapts the external "spec-first / 0%-guesses / interview-me" pattern to the office's autonomy-first, non-technical-client posture (interview deliberately rejected). |
| 1.1.0 | 2026-07-15 | Core Team | Added IR-8 (interpretation fidelity — confirm a stated intent when its reading is genuinely uncertain) + §4.6 — a distinct axis from IR-1…7 gap-resolution: IR-1…7 resolve what the client left *unsaid*, IR-8 guards against *misreading what they did say*. When confidence in the *interpretation* of a stated idea/requirement is genuinely low (phrasing admits materially different readings) or the idea is about to become a durable high-leverage artifact (a spec/plan) where a misread propagates, the office confirms understanding *before* building on it by **restating its interpretation back** ("here's what I understood: … — right?") for validate/correct; uncertainty-gated and lower-friction than an IR-2 clarifying question (it *reflects* the office's reading rather than *interrogating* the client for missing info), reaffirming IR-2/§4.4 anti-interview; a corrected interpretation propagates + feeds learning as a corrected assumption does (IR-7); the ideation → specification boundary is the canonical trigger. Sharpens the "acting-on-misread-intent" claim the 1.0.0 spec made only via gap-resolution. Additive — L1 stays Stable (C9). |
| 1.2.0 | 2026-10-03 | Core Team | Added IR-9 + §4.7 — questions come in **classes**, each with its own bound, and all of them draw on **one budget of the client's attention**. IR-2 governs whether to ask; the corpus nonetheless carried four unrelated question limits in four places (a ceiling on the planning-clarification batch, a per-task budget, a one-pending ask-back per project, a "few open" health threshold) with no shared frame, so nothing said how they combine and nothing bounded the total a client sees across offices. IR-9 names five classes (intake/planning clarification, blocking question mid-work, post-session ask-back, approval — a consent act that is not counted as a question —, and the open-clarification measure — a health figure never put to the client), leaves each bound with its owning specification as a ceiling rather than a target, and states the cross-class rule: questions reach the client blocking-first, merged when identical, never dropped silently, and promoted if they gain a blocker. `l1-global-orchestration` GO-7 applies it across offices. Related Specifications extended with `l1-global-orchestration`, `l1-action-gating`, `l1-navigation-model`, `l1-honest-solicitation`. Additive — L1 stays Stable (C9). |
