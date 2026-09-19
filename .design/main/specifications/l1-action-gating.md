# Action Gating

**Version:** 1.2.1
**Status:** Stable
**Layer:** concept

## Overview

When an agent takes real-world actions — sending an email to a client, moving a file, booking a meeting, changing a permission, spending money — some of those actions are safe and reversible and some are consequential and irreversible. This spec names the discipline that decides **how much authorization friction each action must pass through**: gating that is **proportional to consequence**. A read-only, reversible, in-boundary action executes immediately; a write or an externally-visible action takes a single lightweight confirmation; an irreversible, high-value, or authority-changing action takes a full approval.

The load-bearing insight is that **uniform friction is the failure mode in both directions**. Gate nothing and dangerous actions slip through. Gate everything and the approver is trained to rubber-stamp — a blanket-approval habit that silently nullifies the gate on the actions that actually mattered. Proportional gating spends the approver's scarce attention only where the consequence warrants it: safe actions stay frictionless, and the friction rises, in a closed ordered ladder, exactly as the blast radius, irreversibility, external visibility, and value at stake rise. This spec is the *how-much-friction* contract; the enforcement mechanism (the runtime guard, the approval gate) and the config governance around it are owned by their own specs.

## Related Specifications

- [l2-tool-security.md](l2-tool-security.md) — the runtime guard mechanics (allow / escalate-to-approval / hard-block) that **enforce** the tier this spec assigns; l2-tool-security is a threat guard (block malicious), this L1 is the friction calibrator (how much gate a *legitimate* action warrants). They compose: the guard hard-blocks threats, action-gating tiers the rest.
- [l1-interception-model.md](l1-interception-model.md) — the decide-class seam (INT-1) that intercepts an effect before it happens and enforces the assigned tier; INT-3 fail-closed is AG-4's fail-safe-to-friction.
- [l1-policy-governance.md](l1-policy-governance.md) — the consequence→tier thresholds are administrator-governable (AG-8, composing PG-4/PG-6); weakening the gate below its floor is a governed escape-hatch.
- [l1-security.md](l1-security.md) — SEC-3 default-deny and the egress gate (external-visibility axis, AG-2/AG-4); SEC-9 learnable permission (AG-6 de-escalation); SEC-10 human-rooted authority (AG-6/AG-8).
- [l2-orchestration.md](l2-orchestration.md) — the approval gate the top tier routes into.
- [l1-operational-ledger.md](l1-operational-ledger.md) — the auditable record of each gate decision, tier, and approver (AG-7).
- [l2-agent-autonomy.md](l2-agent-autonomy.md) — durable allow-rules are the concrete de-escalation mechanism (AG-6).
- [../../nodus/specifications/l1-nodus-portability.md](../../nodus/specifications/l1-nodus-portability.md) — LP-16 effect risk-class declaration is the nodus-workflow realization: an effectful step declares its consequence descriptors so a host graduated gate can tier it.
- [l1-review-checkpoint.md](l1-review-checkpoint.md) — [ADDED v1.0.1] RC-6 defers to this spec for which terminal/expensive step is checkpointed by default; a review decision (approve/revise/reject) is richer than the permit/deny of an effect gate.
- [l1-compensation.md](l1-compensation.md) - [ADDED v1.0.2] a compensating action is a first-class effect and passes the same risk-proportional gate as any other (CO-2).
- [l1-consent-binding.md](l1-consent-binding.md) - [ADDED v1.0.3] the sibling half: AG decides *how much* friction an act deserves, CB decides *what a granted consent covers and when it lapses*. AG-6's scoped, revocable, human-ratified de-escalation rule is the grant CB-1 gives an identity to, and CB-3/CB-11 are what stop it widening by drift.
- [l1-context-provenance.md](l1-context-provenance.md) - [ADDED v1.2.0] CP-1/CP-4 supply the provenance AG-10 reads at a privileged sink (sticky: a command assembled from a fetched page is a fragment of that page); CP-2's neutralization protects a prompt and has no meaning for a command, so at a sink the answer is a gate; CP-7 is why an automated reviewer's own output (AG-11e) is untrusted data to the agent it reviews.
- [l1-provenance-taint.md](l1-provenance-taint.md) - [ADDED v1.2.0] PT-2/PT-5 (untrusted by default, most-untrusted-wins on combination) decide what "untrusted or unknown" means for AG-10's argument test, including recalled memory.
- [l1-confidentiality-flow.md](l1-confidentiality-flow.md) - [ADDED v1.2.0] CF-1 keeps integrity and confidentiality as two axes; CF-4 puts a capacity on every outbound sink for *what may leave*, AG-10 puts a requirement on every privileged sink for *what may steer it*. CF-8 (enforced at the sink by mechanism) is AG-10(b)'s rule too.
- [l1-execution-sandbox.md](l1-execution-sandbox.md) and [l2-execution-sandbox.md](l2-execution-sandbox.md) - [ADDED v1.2.0] gating is friction, confinement is the boundary (SEC-12); AG-10/AG-11 sit in front of the confinement and never substitute for it (ES-9 in that spec's numbering).

## 1. Motivation

Two opposite mistakes make an agent either dangerous or useless. The dangerous one gates too little: it lets the agent send an irreversible external email, or grant a permission, or spend money, with the same zero friction as reading a file — so a hallucinated or hijacked step does real, unrecoverable damage before anyone sees it. The useless one gates too much: it asks the human to approve *everything*, including reading a document and rephrasing a note — so the human, facing a hundred approvals a day, stops reading them and clicks "approve" reflexively. The second mistake is subtler and more corrosive, because it *feels* safe (there's a gate!) while actually being unsafe (nobody reads it) — the gate on the one dangerous action in the hundred is rubber-stamped along with the ninety-nine trivial ones.

Proportional gating is the resolution. Classify each action by its real consequence — can it be undone, whose state does it touch, does it leave the trust boundary, what value is at stake — and match the friction to that consequence. The frictionless majority (safe reversible reads and in-boundary work) runs immediately, so the human's attention is not spent there. The rare consequential action (irreversible, external, high-value, authority-changing) is gated heavily, so the human's attention lands exactly where it matters and is not yet exhausted. And because a proven-safe action can earn its way to a lower tier over time (never the reverse), the friction keeps shrinking toward only what genuinely needs a human.

## 2. Constraints & Assumptions

- Gating decides **how much friction**, not **whether an action is a threat** — malicious actions are hard-blocked by the security guard regardless of tier; gating calibrates friction for *legitimate* actions.
- The consequence classification must be **legible** — a human can see why an action landed in its tier; an opaque score alone is insufficient.
- The safe default for an unclassifiable action is **more** friction, never less (fail-safe-to-friction).
- This spec owns the friction-calibration contract; the gate mechanics, the approval UI, and the config governance are owned by tool-security, orchestration, and policy-governance respectively.
- Layer 1: it names no concrete threshold, score, or approval UI. The tier thresholds and mechanics are Layer-2 / governable.

## 3. Core Invariants

Rules every Layer 2 realization MUST NOT violate. They are technology-neutral.

- **AG-1 (Friction proportional to consequence):** the authorization friction on an action MUST be **proportional to its consequence**. A safe, reversible, in-boundary action passes no gate; a write or externally-visible action takes a lightweight confirmation; an irreversible, high-value, or authority-changing action takes a full approval. **Uniform friction — gating everything or gating nothing — is the failure this forbids.**

- **AG-2 (Consequence classified by explicit, legible axes):** an action's tier is derived from **explicit consequence axes** — **reversibility** (can it be undone?), **blast radius** (self / shared / others' state), **external visibility** (does it cross the trust boundary?), and **value at stake** (money, credentials, permissions). The classification is **legible and attributable** — a human can see *which axes* put an action in its tier — not an opaque number alone.

- **AG-3 (Closed, ordered tier ladder):** gate tiers form a **closed, ordered ladder** of increasing friction — at minimum **auto** (execute immediately), **confirm** (one lightweight acknowledgement), and **approval** (an explicit authorization by an authorized principal, possibly out-of-band). Higher tiers strictly dominate lower; an action cannot reach a lower-friction path by re-routing around its tier.

- **AG-4 (Read-safe frictionless, irreversible always gated, unknown fails to friction):** a purely read-only, side-effect-free, in-boundary action is **auto** by default — friction there is pure cost. An **irreversible** action, or one that **crosses the trust boundary** or **puts value at stake**, is **never auto**: it takes at least confirm, and by AG-1 usually approval. An **unclassifiable** action defaults to the **higher** tier — fail-safe-to-friction, never fail-open.

- **AG-5 (Friction-fatigue is a first-class failure):** **over-gating is a defect, not caution.** Gating everything trains the approver to rubber-stamp, which nullifies the gate on the actions that mattered — a blanket-approval habit is *worse* than calibrated gating because it *looks* safe while being unsafe. The tier ladder exists precisely to spend the approver's attention only where consequence warrants; a realization that gates indiscriminately violates this contract as surely as one that gates too little.

- **AG-6 (Learned de-escalation, never self-escalation-bypass):** a repeatedly-approved, proven-safe action MAY be **de-escalated** to a lower tier through an explicit, **scoped, revocable, human-ratified** allow-rule (composing SEC-9), so friction shrinks over time toward only what still needs a human. But the ladder is **never bypassed upward**: an action can **never lower its own tier**, a de-escalation is always a human-rooted grant (SEC-10), and a hard-forbidden or top-tier-floored action is **non-de-escalatable** (fail-closed). Trust is earned downward, never seized.

- **AG-7 (Every gate decision is auditable):** each gated action records the **tier assigned**, the **consequence axes** that placed it there, the **decision** (auto / confirmed-by-whom / approved-by-whom / denied), and the outcome — so "why did this need approval" and "who authorized this" are always answerable (composing the operational ledger and the interception observe-after). A silent gate, or one whose tier rationale is unrecorded, is a defect.

- **AG-8 (Governable stricter, never silently laxer):** the mapping from consequence to tier — which axes count, the thresholds, the per-tier friction — is **administrator-governable** (composing policy-governance): an operator can **raise** friction. But the gate is a **safety-reducing escape hatch when weakened**, so lowering it below the safe floor is itself a governed act, and the top tier (approval for irreversible / high-value / authority-changing actions) has a floor **no lower tier of policy can silently remove**. You can make gating stricter, never quietly laxer.

- **AG-9 (A gate is only a gate where the answer can be given):** [ADDED v1.1.0] a tier's friction is realized by an **authorization prompt**, and a prompt raised on a channel the **current caller cannot answer** is not friction — it is an indefinite hang wearing a gate's name. The invocation therefore declares the **answering surface** actually available to it (an attended terminal, a visible confirmation surface, a graphical prompt, or none at all), and the gate selects a mechanism that can reach that surface. Where no mechanism can, the correct outcome is a **visible refusal naming what could not be asked** — never an attempt whose failure mode is silence. Two shapes are forbidden: escalating to a mechanism whose prompt appears where nobody is looking (the unattended-agent case, where the work stops forever and the surface shows nothing), and downgrading to a mechanism that skips the question in order to keep moving, which is AG-4's irreversible-always-gated rule defeated by convenience. The choice of mechanism is decided by **who can answer**, never by how sensitive the action is — sensitivity decides the tier (AG-2/AG-3), reachability decides how the tier is delivered. This is REA-5's rule (a human-only precondition is an instruction to the person, never an attempted invocation) applied to the gate itself, and its absence is the most common way an unattended run dies with no diagnosis at all.

- **AG-10 (What steered an act is a consequence axis at privileged sinks):** [ADDED v1.2.0] AG-2's axes describe what an act *does*; this one describes **who chose its arguments**. A **privileged sink** is an effect whose danger lies in its arguments being a program or an authority — executing a command or code, installing a component, spawning a delegate, sending across the trust boundary, or writing into the authority plane (SEC-10) or into a location the agent later loads as standing instruction. When any part of a privileged sink's **operative arguments** — the ones that decide *what runs, from where, to whom, or what future runs will obey*: the command text and argument vector, code, an install source, a delegate's prompt and tool grant, a destination, and the target and content of an authority or instruction write — derives from a fragment that is **untrusted or of unknown provenance** (CP-1, sticky under CP-4 and PT-5 — a command assembled from a fetched page *is* a fragment of that page), the act takes **at least the approval tier for that call**, and no autonomy level, mode, learned de-escalation (AG-6), or automated review (AG-11) may lower it. Only a human decision on that **resolved identity** (CB-1) does, and the approver is shown the resolved form with the untrusted-derived spans **marked** (CB-2) — a preview that hides which part came from outside asks the human to approve a sentence they cannot audit. Three rules keep it honest. **(a) Neutralization is not the answer at a sink:** CP-2's encode/escape/delimit protects a *prompt*; no escaping makes an untrusted string safe to run, so the sink's answer is the gate, not a wrapper. **(b) Tracked by mechanism, never by self-report:** provenance travels with the argument through the runtime (CP-4, CF-8, INT-5); the model is never asked to declare where its own arguments came from, and a sink that cannot establish provenance treats the argument as unknown, hence untrusted (CP-1). Where a model's own generation sits between the outside fragment and the argument, the mechanism is necessarily a heuristic (SEC-12): it can be defeated by paraphrase, which is why this gate is one layer in front of a boundary and never the boundary. **(c) Scoped to privileged sinks, not to every use of untrusted content:** reading, summarizing, and reasoning over untrusted material stay auto (AG-4); friction is spent where the outside actually reaches an effect, and AG-5 still binds — a realization that raises every call touching a fetched page has re-invented uniform friction. **(d) A data slot in a human-authored operation is not steering:** where a person authored the operation and the untrusted value fills a declared data field that cannot change *which* operation runs, where it goes, or what it obeys — a message body in a notification with a fixed destination, a search query — the value is data, CP-2's neutralization is the right treatment, and an automation whose structure a human wrote is not gated once per payload. The moment the untrusted value can choose the operation, the destination, the code, or the standing instruction, it is an operative argument and (a)–(c) apply.

- **AG-11 (An automated risk reviewer is a decide-class participant — it can add friction, never grant):** [ADDED v1.2.0] a second model or classifier MAY be asked whether a proposed act looks dangerous, and the ladder MAY consult it, on strict terms. **(a) One-directional (INT-1 decide class):** it can deny or raise a tier; it can never lower a tier below what the ladder, the floor (AG-6/AG-8), or a human-approved identity assigns, and its "allow" is not a grant — it never creates or widens a durable allow-rule (SEC-9a) and never stands in for an approval-tier act or for a privileged sink carrying untrusted-derived arguments (AG-10). A human-opted "review-assisted" mode MAY let its "allow" stand in for a *confirm*-tier acknowledgement, and nothing higher; that opt-in is itself the scoped, revocable, human-ratified de-escalation AG-6 requires — a setting on the authority plane (SEC-10), never something the reviewer or the agent can turn on. **(b) Floor first, reviewer second:** the deterministic floor — the hard-forbidden set and the non-de-escalatable tiers — is evaluated before and independently of the reviewer, so a reviewer that is wrong, slow, or subverted cannot open what the floor closes; the reviewer is non-deterministic and sits in a security path, which is exactly why it is never the only thing there (it is a heuristic, SEC-12). **(c) Isolated and bounded:** it sees the proposed act in resolved form and the minimum context that judgement needs — not the reviewed agent's tool access, memory, or credentials — under its own budget and deadline. **(d) Fail-closed, and "unavailable" is not "approved" (INT-3):** an error, timeout, malformed answer, or overflow yields *unavailable*, which resolves to the tier the ladder would assign with no reviewer and is reported as such; a caller with no answering surface gets a visible refusal (AG-9), never a silent pass. **(e) Its words are untrusted data to the agent it reviews (CP-7):** the rationale it returns is length-capped, stripped of markup that could pass for structure or instruction, and delivered as a typed refusal reason — a reviewer output that can steer the reviewed agent is a second injection channel opened by the defence itself. **(f) Refusal loops are bounded:** consecutive refusals of the same act family, consecutive *unavailable* results, and total refusals per session are each capped, and reaching a cap routes the next call to a human approval — the agent may not probe a reviewer until it yields, and a reviewer's failure to make progress never becomes permission. **(g) Auditable (AG-7):** which reviewer, a digest of what it was shown, its verdict, and what the ladder did with it.

> L2 specs cannot reach RFC status until all invariants here are addressed in their "Invariant Compliance" section.

## 4. Detailed Design

### 4.1 The tier ladder

| Tier | For | Friction | Examples |
| --- | --- | --- | --- |
| **auto** | reversible, in-boundary, no value at stake | none — executes immediately | read a file, search, draft (unsent), in-scratch edit |
| **confirm** | writes, externally-visible, recoverable | one lightweight acknowledgement | send an internal message, create a calendar hold, write a shared doc |
| **approval** | irreversible, high-value, authority-changing, others' state | explicit authorization by an authorized principal | email an external client, spend money, grant a permission, delete shared data |

The ladder is closed and ordered (AG-3); an action's tier is the **highest** any of its consequence axes demands (AG-2) — one high-value axis lifts the whole action to approval regardless of the others.

### 4.2 Classifying an action

```text
[REFERENCE]
tier(action):
    if not classifiable(action):                 return APPROVAL      // AG-4 fail-safe-to-friction
    t := AUTO
    if action.writes or action.external_visible:  t := max(t, CONFIRM) // AG-2
    if not action.reversible:                     t := max(t, APPROVAL)
    if action.crosses_trust_boundary:             t := max(t, APPROVAL) // e.g. external send
    if action.value_at_stake(money|creds|perms):  t := max(t, APPROVAL)
    steered := action.sink.privileged and any(untrusted_or_unknown(a) for a in action.arguments)
    if steered:                                   t := max(t, APPROVAL)   // AG-10: the outside chose the arguments
    t := apply_allow_rules(action, t, exact_identity_only = steered)     // AG-6 + CB-1: under AG-10 only a human grant on this exact resolved identity de-escalates
    t := max(t, governance_floor(action))          // AG-8: policy may raise, never silently lower
    t := max(t, reviewer_floor(action))            // AG-11: a reviewer may only raise; unavailable changes nothing
    return t
```

The tier is the max across axes (AG-2), raised for a privileged sink whose arguments were steered from outside (AG-10), de-escalated only by a human-ratified allow-rule bound to the exact identity (AG-6), floored by governance (AG-8) and — where one is configured — by an automated reviewer that can only raise it (AG-11). Every branch is recorded (AG-7).

### 4.3 Why friction-fatigue is the load-bearing rule

The subtle failure this spec guards against is not too little gating — that is obvious and everyone builds a gate. It is **too much**. A system that asks for approval on every action produces an approver who has stopped reading, so the gate that fires on the one dangerous action in a hundred is clicked through with the ninety-nine trivial ones. Such a system passes a naive audit ("there's an approval gate!") while being *less* safe than one with no gate, because it manufactures false confidence. AG-5 makes over-gating a defect precisely so a realization cannot buy the appearance of safety with friction that destroys the attention the gate depends on.

### 4.4 Privileged sinks and the automated reviewer [ADDED v1.2.0]

AG-2's axes classify an act by what it does to the world. They are silent on a different failure: the act may be entirely ordinary — run a command, write a file, send a message — while the *content* that chose it came from a web page, an email, or a document the agent merely read. The principal did not decide it; the outside did. That is a confused-deputy shape, and the defence is not to wrap the content (there is no wrapper that makes a command string safe to run) but to notice, at the effect, that its arguments were steered from outside, and to put a human in front of it.

| Privileged sink | Why its arguments are the danger |
| --- | --- |
| Execute a command or code | the argument **is** the program |
| Install or activate a component | the payload is the program that will run later (XM-8, CS-2) |
| Spawn or delegate to another agent | the prompt is the delegate's program; a guard on the parent does not follow unless the sink is guarded (INT-5) |
| Send across the trust boundary | the destination and the body decide what leaves (CF-4, CF-5) |
| Write the authority plane, or a location the agent later loads as standing instruction | the content is what every future run obeys (SEC-10); a persistence vector, not merely a file write |

```text
[REFERENCE]
gate(action):
    t := tier(action)                                   // §4.2 — includes AG-10 and every floor
    if t < APPROVAL and reviewer.configured and action.consequential:
        v := reviewer.assess(resolved(action))          // AG-11(c): isolated, bounded, own deadline
        match v:
            Deny(reason) -> return refuse(sanitize(reason))   // AG-11(e): typed, capped, inert as text
            Unavailable  -> record("reviewer_unavailable")    // AG-11(d): same tier as with no reviewer
            Allow        -> pass                              // never lowers a tier, never a grant
        caps.record(action.family, v)
        if caps.exceeded():   return approval(action)          // AG-11(f): a cap routes to a human
    return deliver(t, answering_surface)                // AG-9: a gate only where it can be answered
```

A reviewer is consulted only for an act the ladder would let through (auto or confirm): an approval-tier act already has a human deciding, and the reviewer has nothing to add that the human cannot see. What the reviewer buys is a second, independent look at the acts that would otherwise pass unseen; what it cannot buy is a lower tier for anything the floor or AG-10 has raised.

## 5. Drawbacks & Alternatives

**Alternative: gate every action (max safety).** Rejected by AG-5 — it manufactures rubber-stamping and nullifies the gate on what matters; calibrated friction is what keeps approvals meaningful.

**Alternative: gate nothing an allow-rule permits.** Rejected by AG-6 — de-escalation must stay scoped, revocable, and human-ratified, and top-tier actions are non-de-escalatable; blanket self-permitting is the dangerous extreme.

**Alternative: a single opaque risk score.** Rejected by AG-2 — a number alone is not legible; the human must see *which* consequence put an action in its tier to trust and tune the gate.

**Risk: mis-classification.** An action mis-tiered too low is dangerous. Mitigation: AG-4 fails unclassifiable actions to the higher tier, and AG-8 lets an operator raise thresholds — the calibration errs toward friction where uncertain.

**Alternative: extend CP-2's neutralization to sink arguments.** Rejected by AG-10(a) — escaping protects a prompt from being read as instruction; a command has no "inert" form, so a wrapper around it is either ignored by the interpreter or breaks the command. The sink answer has to be a decision.

**Alternative: ask the model to declare where its arguments came from.** Rejected by AG-10(b) — the party the gate defends against is the one answering, and a subverted model reports its arguments as its own.

**Alternative: a reviewer model as the primary gate.** Rejected by AG-11(b) and SEC-12 — a non-deterministic classifier in a security path is a heuristic; it fronts the deterministic floor and the confinement and replaces neither. The defences that tried to make it primary needed a circuit breaker, a deterministic pre-filter and a sanitizer for its own output — each a compensation for what it is.

**Risk: AG-10 over-gates.** Bounded by AG-10(c): the raise applies at a privileged sink whose arguments were steered from outside, not to every call that touched untrusted content, and AG-5 (friction-fatigue) still binds a realization that drifts toward raising everything.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[GUARD]` | `.design/main/specifications/l2-tool-security.md` | The runtime guard that enforces the assigned tier (allow / escalate / hard-block) |
| `[SECURITY]` | `.design/main/specifications/l1-security.md` | Egress/default-deny (AG-4), SEC-9 de-escalation (AG-6), SEC-10 authority (AG-8) |
| `[GOVERNANCE]` | `.design/main/specifications/l1-policy-governance.md` | The governable thresholds and the un-disable-able floor (AG-8) |
| `[NODUS]` | `.design/nodus/specifications/l1-nodus-portability.md` | The host-neutral realization: LP-16 effect risk-class declaration |
| `[PROVENANCE]` | `.design/main/specifications/l1-context-provenance.md` | CP-1/CP-4 provenance read at a privileged sink (AG-10); CP-7 for a reviewer's output (AG-11e) |
| `[TAINT]` | `.design/main/specifications/l1-provenance-taint.md` | PT-2/PT-5 — what "untrusted or unknown" means for AG-10 |
| `[SANDBOX]` | `.design/main/specifications/l2-execution-sandbox.md` | The boundary AG-10/AG-11 front and never replace |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.2.1 | 2026-09-19 | Core Team | Clarification (patch): AG-10(b) said provenance is tracked by mechanism through the runtime, which reads as a guarantee. Where a model's own generation sits between an outside fragment and an argument no runtime can follow the value, so the clause now says the mechanism is a heuristic that a paraphrase defeats (SEC-12), and that this gate is a layer in front of a boundary, not the boundary. No invariant added or removed. |
| 1.2.0 | 2026-09-19 | Core Team | Added AG-10 (what steered an act is a consequence axis at privileged sinks) and AG-11 (an automated risk reviewer is a decide-class participant — it can add friction, never grant). AG-2's axes classify an act by what it does; they were silent on the confused-deputy shape in which an ordinary act — a command, a write, a send — is chosen by content the agent merely read. AG-10 lifts a privileged sink whose arguments derive from untrusted or unknown provenance to at least the approval tier for that call, immune to autonomy level, mode, learned de-escalation and automated review, with the untrusted spans marked in what the approver sees; it is tracked by mechanism (never by the model's self-report), scoped to privileged sinks (execution, install, delegation, boundary-crossing send, authority-plane or standing-instruction write), and explicitly not solved by neutralization, which has no meaning for a command. AG-11 admits a second model as a reviewer on one-directional terms — deny or raise, never lower, never a grant — with the deterministic floor evaluated first, isolation and its own deadline, fail-closed "unavailable", its output treated as untrusted data (CP-7), bounded refusal loops that route to a human, and an audit record. New §4.4, extended §4.2 tier function, three rejected alternatives and one bounded risk. Distilled from a cross-check of eight external agent command-line tools against this corpus: an isolated reviewer had three independent implementations whose guard rails (deterministic floor first, fail-closed, sanitized output, bounded refusals) recurred across them, and the provenance-at-the-sink rule had one strong implementation; on a full read of the corpus neither had an owner here. |
| 1.1.0 | 2026-09-04 | Core Team | Added AG-9 — a gate is only a gate where the answer can be given. AG-1…AG-8 specify *how much* friction an action deserves and never *whether the caller can supply it*, leaving the family's most silent failure open: an authorization prompt raised on a channel the current caller cannot reach (an elevation prompt expecting a terminal, raised by a keybinding-launched or agent-launched process) does not gate the action, it hangs the run with nothing on any surface to explain why. The invocation now declares its available answering surface and the gate picks a mechanism that reaches it, refusing visibly and naming what could not be asked when none does. Both escape hatches are closed: escalating to a prompt nobody sees, and downgrading to a mechanism that skips the question to keep moving (AG-4 defeated by convenience). Mechanism follows *who can answer*; tier follows consequence. The gate-side application of REA-5. |
| 1.0.3 | 2026-08-25 | Core Team | Related Specifications extended with `l1-consent-binding` — the question this spec leaves unasked after assigning a tier: *what was the grant for, and when does it stop applying?* AG-7 records that an act was approved and by whom, but not with enough precision to decide whether the next attempt is the same act. CB gives AG-6 de-escalation rules a bound identity (resolved invocation, not a name or category) and a lapse rule, which matters most precisely where AG-5 correctly minimizes prompts. Link-only; no invariant changed. |
| 1.0.0 | 2026-07-09 | Core Team | Initial stable spec — action gating: authorization friction proportional to an action's consequence. Friction proportional to consequence, uniform friction forbidden (AG-1); consequence classified by explicit legible axes — reversibility, blast radius, external visibility, value at stake (AG-2); closed ordered tier ladder auto/confirm/approval (AG-3); read-safe frictionless, irreversible always gated, unknown fails to friction (AG-4); friction-fatigue a first-class failure — over-gating is a defect not caution (AG-5); learned scoped revocable human-ratified de-escalation, never self-escalation-bypass, top tier non-de-escalatable (AG-6); every gate decision auditable with tier + axes + approver (AG-7); governable stricter never silently laxer, top-tier floor un-removable (AG-8). Composes l2-tool-security / l1-interception-model / l1-policy-governance / l1-security / l2-orchestration / l2-agent-autonomy. Distilled from an adoption pass over an external business-operations agent-harness reference (three-tier none/confirm/approval execution gates by action risk). |
