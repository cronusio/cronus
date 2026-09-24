# Interception Model

**Version:** 1.3.1
**Status:** Stable
**Layer:** concept

## Overview

The technology-agnostic model of how cross-cutting behaviour attaches to an agent's execution — the discipline by which observation, permission decisions, and data rewriting are inserted at the boundaries of a turn, a model call, a tool call, or a delegated sub-run. Many existing concerns already ride these boundaries (permission policy, provenance neutralization, execution receipts, telemetry), but each realizes its own ad-hoc injection point with its own fail-behaviour. This spec names the *shared* contract those realizations should obey so that a guard is capability-bounded, correctly ordered around the effect it guards, fails in a safe direction, is inherited by delegated work, and is honest about what it does not cover.

The central discipline is a **capability taxonomy**: an interceptor is one of exactly three kinds — it *observes* (read-only, cannot block or change anything), it *decides* (read-only, may veto but not change), or it *transforms* (may rewrite the thing it guards). The kind is declared and structurally enforced, not a matter of convention. From that one distinction everything else follows: deciders must run *before* the effect and observers *after* it (so a check cannot be mistaken for a use), a guarding interceptor that errors must fail *closed* while an observer that errors must fail *forward*, and a denied capability stays denied no matter which agent in a delegation tree attempts it.

This concept sits *above* the concrete hook/guard/guardrail realizations in the implementation layer and unifies their currently-divergent fail-behaviours under one principled contract. It is orthogonal to *what* any particular guard checks — it governs *how* every guard attaches.

## Related Specifications

- [l1-security.md](l1-security.md) — a permission/authority check is a *decide*-class interceptor; SEC-10 authority-self-containment is the principal that INT-5 transitive enforcement operationalizes (a guard the agent cannot rewrite is applied to every effect beneath it).
- [l1-context-provenance.md](l1-context-provenance.md) — CP-2 boundary neutralization of untrusted content is a *transform*-class interceptor; INT-2 ordering places it before the model consumes the fragment.
- [l1-tool-receipts.md](l1-tool-receipts.md) — a per-action receipt is emitted by an *observe*-class interceptor at the TR-1 effect boundary; INT-2 guarantees it observes the real post-effect result.
- [l1-policy-governance.md](l1-policy-governance.md) — PG-4 governs *which* interceptors load (managed-only lockdown); this spec governs *how* a loaded interceptor behaves.
- [l1-extensions.md](l1-extensions.md) / [l2-plugin-hooks.md](l2-plugin-hooks.md) — the plugin hook system is one realization of the taxonomy; today it is un-typed and fail-forward, and is a candidate to adopt this contract.
- [l2-tool-security.md](l2-tool-security.md) — the runtime tool guard, ToolPolicy, and guardrail pipeline are realizations; §4.5 `disabled_tools` vs advertised-list removal is a concrete instance of the INT-6 strip-vs-deny axis.
- [l1-orchestration.md](l1-orchestration.md) — INT-5 rides delegation boundaries; ORC-12 transparency requires interception itself be observable, never a hidden back-channel.
- [l1-scheduler-model.md](l1-scheduler-model.md) / [l2-trigger-triage.md](l2-trigger-triage.md) — a trigger/schedule-initiated turn is the canonical INT-8 coverage gap: a turn that does not enter through the guarded user-send path.
- [l1-convergence-gate.md](l1-convergence-gate.md) — [ADDED v1.1.0] the placement discipline INT-9 points to: guard the single boundary all paths converge on rather than each entrance, so source-independence is structural and INT-8's coverage gap disappears.
- [l1-agent-tool-ergonomics.md](l1-agent-tool-ergonomics.md) — INT-6 strip (remove from the model's affordance set) interacts with ATE-8 lean-surface-by-pick-rate; ergonomics and restriction are different reasons to shrink the surface. [ADDED v1.2.0] ATE-13's *no such target* vs *target exists, wrong invocation* is the distinction INT-10 keeps honest: a guard placed post-resolution-matched cannot answer *denied* where the truth is *no such target*.
- [l1-input-binding.md](l1-input-binding.md) — [ADDED v1.2.0] the adjacent, deliberately separate discipline: a binder **supplies a value** and may only reject, an interceptor **guards an effect** and may veto or rewrite. INT-10 fixes the phase each attaches at; a binder that acquires veto power becomes an undeclared guard outside this taxonomy (IB §4.5).
- [l1-scoped-capability-layers.md](l1-scoped-capability-layers.md) - [ADDED v1.3.0] SCL-3 decides *which* interceptors an event admits (the scope question); INT-11 decides how the admitted set runs relative to each other (the dispatch question). SCL-5 is why a scope's own restrictions are not inherited while INT-5's guard enforcement is transitive.

## 1. Motivation

Cross-cutting behaviour keeps being re-invented at every boundary. One subsystem adds a "run this before each tool call" hook; another adds a "wrap external content before the model reads it" step; a third adds a "record what happened after the call" observer. Each picks its own answer to three questions the others already answered differently: *what may this interceptor do* (only look, or also veto, or also rewrite), *when does it run relative to the effect*, and *what happens when it errors*. Left unstated, those answers drift apart — and the drift is not cosmetic, it is a safety hole.

Three concrete failure modes recur. First, an **un-typed interceptor**: a single hook that can observe, veto, *and* rewrite is impossible to reason about — a reviewer cannot tell from its type whether it is a passive logger or an active gate, and a logging hook that gains the power to change data becomes an unaudited rewrite path. Second, **inconsistent fail-direction**: when a permission gate errors, "log it and continue" silently *allows* the very action it was meant to stop, while "abort the whole turn" turns a flaky *observer* into a denial-of-service. The right direction depends on the interceptor's kind, and if the kind is not modelled, the direction cannot be principled. Third, the **time-of-check/time-of-use gap**: if a gate validates the arguments but an observer that "confirms" the call actually ran before the effect completed, the system records a check as if it were a use — a classic TOCTOU vulnerability.

Two further gaps are about composition and honesty. A guard that stops the *parent* agent from running a dangerous command is worthless if the parent can simply spawn a sub-agent to run it — unless the guard is **inherited transitively** by every delegated effect. And a pre-turn guard that only sits on the user-send path silently fails to guard turns that arrive by another route (a background completion, a scheduled fire); if that **coverage gap is not named**, the system looks guarded when it is not.

Capturing these as invariants gives every guard, hook, and guardrail in the system one shared contract — capability-bounded, correctly ordered, fail-safe by class, transitively enforced, and honest about coverage — and lets the divergent realizations converge on it instead of each carrying its own footgun.

## 2. Constraints & Assumptions

- **The agent is a string generator, not a trusted enforcer.** An interceptor's guarantees must hold structurally (by where and how it attaches), never because the model was *told* to respect them — instructions are low-salience and injection-vulnerable (consistent with [l1-context-provenance.md](l1-context-provenance.md)).
- **An interceptor is untrusted code too.** A guard, hook, or guardrail can be buggy, slow, or hostile; the model must contain a misbehaving interceptor (a hung observer must not wedge the agent; a crashing gate must not silently open).
- **Effects are the unit guarded.** The guarded thing is an *effect* — a turn, a model request, a tool call, a delegated sub-run, or any host-defined side-effecting operation. This spec does not enumerate effect kinds; it constrains how any of them is bracketed.
- **Availability and safety trade against each other, per class.** Observation must never reduce the agent's availability; guarding must never trade safety for availability. The taxonomy exists precisely so the trade-off is resolved per kind, not per hook.
- **Ordering is nominal, not wall-clock.** "Before" and "after" mean causal ordering relative to the effect (the decision is settled before the effect is permitted; the observation reflects the completed effect), independent of concurrency within a phase.
- **This is not the permission model.** *What* is allowed, denied, or must be approved lives in [l1-security.md](l1-security.md) / [l1-policy-governance.md](l1-policy-governance.md). This spec is the *attachment discipline* those policies ride on.

## 3. Core Invariants

Layer 2 implementations MUST NOT violate these. They are technology-neutral.

- **INT-1 — Capability-bounded interceptor taxonomy.** Every interceptor declares exactly one capability class from the closed set: **observe** (read-only, non-blocking — may neither veto nor mutate), **decide** (read-only, may veto the effect but not mutate its data), **transform** (may return a rewritten form of the guarded data, and may veto). The runtime bounds each interceptor to its declared class *by construction* — an observe interceptor is given no channel to block or change anything; a decide interceptor is given a veto channel but no mutation channel. Capability is declared, not discovered at runtime, so an interceptor's maximum authority is legible from its kind alone.

- **INT-2 — Check-before-use ordering (TOCTOU-safe).** For any guarded effect, every **decide** (and the veto arm of every **transform**) is evaluated and settled *before* the effect occurs, and every **observe** interceptor runs *after* the effect against its real completed outcome. A guard therefore validates the state that will actually be used, and an observation reflects what actually happened — closing the time-of-check-to-time-of-use gap in which a check on one state is recorded as a use of another. An observer is never positioned where its running could be mistaken for the effect having occurred.

- **INT-3 — Fail-direction is a property of the class.** A guarding interceptor — **decide** or **transform** — that errors, times out, or otherwise fails to produce a valid result fails **closed**: the guarded effect is aborted, never silently permitted. An **observe** interceptor that fails fails **forward**: its failure is recorded but neither aborts nor alters the effect. Fail-direction is fixed by the interceptor's class, not chosen per instance — availability-biased failure is reserved for the class that cannot affect safety, and safety-biased failure is mandatory for the classes that can.

- **INT-4 — Scoped interception state with one-way visibility.** Interceptors share state through a nested scope chain (session ⊃ turn ⊃ operation). A broader scope's state is readable from a narrower scope; a narrower scope's state is never visible to a broader one, and each narrower scope is released at its boundary. State an interceptor keeps for guarding/observation is isolated from the state the guarded effect's own logic carries (e.g. a tool's working state) — the two planes do not implicitly cross-read, preventing accidental coupling and leakage across the guard/effect boundary.

- **INT-5 — Transitive enforcement across delegation.** An interceptor registered at a boundary applies to *every* nested effect beneath it, including effects performed by delegated sub-agents. A capability the guard denies is denied regardless of which agent in the delegation tree attempts it — a sub-agent cannot escape its parent's guard envelope by performing the effect itself. Enforcement is by the *mechanism* (the guard sits on the nested effect), never by re-injecting an instruction into the sub-agent's prompt and hoping it complies. (Operationalizes [l1-security.md](l1-security.md) SEC-10: authority the agent cannot rewrite is applied to all work it spawns; composes [l1-orchestration.md](l1-orchestration.md) delegation.)

- **INT-6 — Two restriction axes: strip vs deny (model-visibility is a design choice).** A capability may be withheld two ways with deliberately different visibility to the model. **Strip** (static, before context is built): the capability is removed from the model's affordance set — the model never sees it, cannot attempt it, and spends no tokens on it; correct for capabilities the agent should never need. **Deny** (dynamic, per attempt): the capability stays visible and each attempt is evaluated — optionally against the attempt's own arguments — and may be refused with a legible reason the model can adapt to; correct for conditional or argument-dependent restrictions. Strip trades expressiveness (no per-attempt logic, the model cannot learn *why*) for zero surface and zero cost; deny trades cost (a refused attempt still spent tokens) for conditionality and a model that understands the refusal. Choosing the axis is an explicit design decision, not an accident of which layer happens to hold the check.

- **INT-7 — Deterministic priority resolution for declarative guards.** When multiple declarative restriction rules bear on one attempt, resolution is deterministic by a fixed precedence: **more-specific over wildcard**, and within equal specificity the **safer decision** wins (deny ranks above ask-approval, which ranks above allow). The first matching rule in the winning band decides (short-circuit); a rule whose own match-predicate errors is treated as **matching** (fail-closed), never as skipped. A rule set therefore resolves to exactly one decision independent of authoring order, and a construction-time check rejects a rule set that is unsatisfiable (e.g. an approval-required rule with no approval handler) rather than failing silently at runtime.

- **INT-8 — Honest coverage boundary.** An interception point guards only the entry paths it actually sits on. An effect that enters by a path the guard does not cover — e.g. a turn initiated by a background, scheduled, or externally-delivered completion rather than by the user-send path a pre-turn guard intercepts — is a **named** coverage gap, surfaced as such, never presented as guarded. Absence of coverage is disclosed at the point it matters (in the guard's own contract and in diagnostics), so no downstream reasoning treats an unguarded path as guarded.

- **INT-9 — Prefer a convergence point over per-entrance guards.** [ADDED v1.1.0] Where multiple entry paths lead to the same governed effect **and** they share a single boundary every one of them must cross for the effect to become real (durable, shared, shipped), the deterministic guard SHOULD be placed at that **convergence point** rather than replicated on each entrance. INT-8 makes an incomplete guard *honest*; INT-9 makes it *unnecessary* where a chokepoint exists — a guard at the convergence point is source-independent **by construction** (it cannot be evaded by producing the effect through a different actor or path, because there is no other path to the effect), so it has no coverage gap to name. This does not weaken INT-8: where no single convergence point exists, per-entrance guards with named gaps remain the honest design; and inline per-path guards still compose *on top* of a convergence gate for early, contextual feedback (they are never dropped because the chokepoint guard exists). The full placement discipline — judging the change's delta rather than accumulated state, deferred obligations, and an attributed bypass — is owned by [l1-convergence-gate.md](l1-convergence-gate.md); INT-9 records only that, given the choice, the interceptor belongs at the exit all paths share, not on each entrance.

- **INT-10 — Declared attachment phase relative to resolution.** [ADDED v1.2.0] Where an invocation is first **resolved** to a target (a tool, a command, a handler, a node) and then executed, every interceptor declares its **attachment phase** from a closed set: **pre-resolution** (sees every invocation, including ones that resolve to nothing, and is the *only* phase that may rewrite the address), **post-resolution-any** (sees every invocation after resolution, resolved or not, and MUST NOT alter the address), or **post-resolution-matched** (runs only when the invocation resolved to a target). Three consequences bind it. **A post-resolution interceptor cannot influence resolution** — an attempt to rewrite the address there is a defect, not a no-op to be tolerated, because resolution has already happened and the rewrite silently does nothing. **Observation belongs post-resolution-any by default**: an invocation that resolved to nothing is precisely the signal that the surface is misadvertised (composing ATE-13), and an observer that never sees it cannot report it. **A guard bound to a specific target belongs post-resolution-matched by default**: running an authorization decision against an address that resolves to nothing spends the check on nothing and answers *denied* where the honest answer is *no such target*, disclosing existence through the error class. The phase is declared and structurally enforced, exactly like the capability class (INT-1) — a guard whose phase is implicit is one whose coverage (INT-8) cannot be stated.

- **INT-11 — Declared dispatch discipline, checked against the dispatch site.** [ADDED v1.3.0] Every interception point declares — alongside its capability class (INT-1) and its attachment phase (INT-10) — the **dispatch discipline** its interceptors run under, from a closed set: **broadcast** (all run, none returns a value, and no interceptor's outcome reaches another), **chained** (each receives the guarded value plus an explicit continuation, and either *delegates* — passing a possibly-modified value onward — or *answers* without delegating, ending the chain), **fan-out-awaited** (all run concurrently and the point waits for all), and **ordered-answered** (they run in declared order and the first answer settles). Three consequences bind. **The declaration is machine-checked against every dispatch site** — a point declared broadcast but dispatched expecting an answer is a defect a tool can find, and the check is what keeps the declaration from decaying into a comment. **Delegation is explicit in a chained point**: an interceptor that neither delegates nor answers has *silently swallowed* the guarded operation, so "did not continue" is an outcome the contract names rather than an omission the runtime absorbs. **Short-circuiting is the design for a single-decision point and a defect for an annotating one**: an interceptor that owns the decision answers without delegating, while one that only annotates or observes MUST delegate — and a point whose interceptors disagree about which they are produces an outcome whose author depends on registration order, which is precisely the non-determinism INT-7 forbids for declarative rules, arriving through the programmatic door instead.

> L2 specs cannot reach RFC status until all invariants here are addressed in their "Invariant Compliance" section.

## 4. Concept Detail

### 4.1 The three kinds, and why the type is the guarantee

The taxonomy is not documentation — it is the enforcement. An interceptor's *kind* is its type, and its type is exactly the set of channels it is handed:

| Kind | Reads the data | May veto the effect | May rewrite the data | Blocking | Canonical use |
| --- | --- | --- | --- | --- | --- |
| observe | yes | no | no | no | logging, metrics, receipts, telemetry |
| decide | yes | yes | no | yes | permission checks, guardrails, approval gates |
| transform | yes | yes | yes | yes | sanitization, neutralization, redaction, error shaping |

Because an *observe* interceptor is never handed a veto or mutation channel, it *cannot* silently become a gate or a rewrite path however it is written — the guarantee is structural. A reviewer classifies an interceptor's maximum authority by reading its declared kind, not by auditing its body. Escalating authority (an observer that needs to start deciding) is a visible re-declaration to a stronger kind, never a quiet capability creep.

### 4.2 Ordering and the TOCTOU gap (INT-2)

For a guarded effect the phases are fixed:

```text
[REFERENCE]
guard(effect):
    for d in decide_interceptors:          # INT-2: all settle BEFORE the effect
        verdict = run(d, data)             # INT-3: d errors -> fail CLOSED (abort)
        if verdict.veto: abort(verdict.reason)
    data = apply_transforms(data)          # transform veto arm also pre-effect
    result = perform(effect, data)         # the effect happens exactly once, here
    for o in observe_interceptors:         # INT-2: observers run AFTER, on the real result
        try: run(o, result)                # INT-3: o errors -> fail FORWARD (record, continue)
        except e: record(e)
    return result
```

The gap this closes: if an "observer that confirms the call" were allowed to run *before* `perform`, the record would attest a check, not a use — and an attacker who can influence state between check and use would have the system vouch for an action it never verified in its final form. Placing all deciders strictly before the single `perform`, and all observers strictly after it on the completed `result`, removes that window.

### 4.3 Fail-direction, worked (INT-3)

The two directions are not a preference — each class has exactly one safe direction:

- A **decide** guard that crashes and "fails forward" would *permit the action it exists to stop*. Unacceptable → decide fails **closed**.
- An **observe** logger that crashes and "fails closed" would *abort a legitimate turn because logging hiccuped*. Unacceptable → observe fails **forward**.
- A **transform** sanitizer that crashes and passes the unsanitized data through would defeat itself → transform fails **closed** (the effect does not proceed on un-transformed data).

A single un-typed interceptor cannot honour this, because the right direction depends on which of the three roles it is playing at the moment of failure — which is exactly why INT-1 forbids the un-typed interceptor.

### 4.4 Strip vs deny (INT-6)

The same restriction — "the agent must not delete files" — can be enforced two ways, and the choice is about what the *model* sees:

| Axis | Where it acts | Model sees the capability? | Token cost | Conditional / argument-aware | Model learns *why* |
| --- | --- | --- | --- | --- | --- |
| **strip** | before context assembly | no — removed from affordances | none | no (all-or-nothing) | no |
| **deny** | at the attempt, per call | yes — remains offered | a refused attempt still costs | yes | yes (legible refusal) |

Rule of thumb: **strip** what the agent should *never* need (a read-only researcher has no business seeing a shell tool at all); **deny** what it may need *sometimes* or *conditionally* (a shell tool that is fine except for destructive arguments). Stripping a conditionally-needed capability over-restricts and hides the reason; denying a never-needed capability wastes tokens on attempts the model should never have been able to make. The axes compose: strip the irrelevant, deny the conditional.

### 4.5 Transitive enforcement (INT-5)

Delegation must not be an escape hatch:

```text
[REFERENCE]
# A guard registered at the office/parent boundary denies "run destructive command".
parent.attempt(destructive)          -> denied by the guard
parent.delegate(subagent).attempt(destructive)
                                     -> the SAME guard sits on the sub-agent's effect
                                     -> denied, identically, without re-declaring the guard
```

The guarantee is that the guard rides the *effect*, not the *agent identity* — so the delegation tree inherits one envelope. The anti-pattern it forbids: relying on re-injecting a natural-language rule into each sub-agent's prompt (fragile, forgettable, and defeated by injection). Where a rule genuinely can only be carried as prompt text, INT-8 requires naming that as a coverage limitation rather than treating it as enforcement.

### 4.6 Coverage honesty (INT-8)

A guard that sits on one entry path is silent about the others. The canonical case: a pre-turn guard positioned on the user-send path does not see a turn initiated by a background completion or a scheduled fire, because those enter the agent by a different route. The invariant is not "guard every path" (some paths may legitimately be out of a given guard's reach); it is "**say so**" — the guard's contract and its diagnostics name the paths it does not cover, so a reader never mistakes partial coverage for total coverage. Honesty here is the same discipline as an execution receipt that declines to vouch for a background action until it can (consistent with [l1-tool-receipts.md](l1-tool-receipts.md) TR-8).

## 5. Ideas to Adopt

| Mined mechanic | Adoption in Cronus |
| --- | --- |
| Interceptor split into observe / decide / transform, enforced by type | **[new]** INT-1; the classification every hook/guard/guardrail declares. |
| Decide-before-effect, observe-after-effect as TOCTOU safety | **[new]** INT-2; an ordering *guarantee*, not an incidental sequence. |
| Fail-closed for guards, fail-forward for observers | **[new]** INT-3; unified the fail-behaviours that diverged when this spec was written — `l2-plugin-hooks` (fail-forward), `l2-tool-security` guardrails (then fail-open), and the tool guard (fail-closed) — under one per-class principle. |
| Nested session ⊃ turn ⊃ operation state with one-way visibility | **[new]** INT-4; a shared, leak-resistant interception-state model. |
| Parent guard enforced on every delegated sub-effect | **[new]** INT-5; closes the sub-agent escape hatch — stronger than `l2-tool-security` §4.8 verbatim-reinjection, which is explicitly *not* inherited. |
| Strip-from-context vs runtime-deny as a visibility axis | **[new]** INT-6; names the design axis behind `l2-tool-security` §4.5 `disabled_tools` vs advertised-list removal and `l1-policy-governance` PG-4, as a reusable principle. |
| Specificity × safety priority lattice, first-match, fail-closed predicate | **[new]** INT-7; a deterministic resolution order for any declarative guard rule set, with construction-time validation. |
| Name the paths a guard does not cover | **[new]** INT-8; the coverage-honesty invariant, sibling to `l1-tool-receipts` TR-8. |

Dispositioned as refinement candidates to *other* specs (recorded here, owned there — no duplication):

- **Divergent fail-behaviour reconciliation** → realized in the L2s: `l2-tool-security` §4.7 now declares its fail direction per class (security guardrails fail closed, adapters degrade visibly, observe-only guardrails fail forward), and `l2-plugin-hooks` treats an erroring or timed-out blocking hook as an objection, never a pass. This L1 states the contract; the L2s carry it.
- **Un-typed hook surfaces** → `l2-plugin-hooks` §4.10/§4.14 hooks currently mix decide (`permissionDecision`) and transform (`updatedInput`, `llm_response`) in one output shape; a typed split is an L2 refinement candidate.

## 6. Nodus Relevance

The nodus workspace already carries the *observe* seam (its AuditProvider is a pure observer with observer-neutrality) and a static whole-run capability gate (its pre-run capability manifest). The mechanic it lacks — and adopts under this pass — is the **decide** seam: a host-supplied per-effect authorization gate over effectful steps, evaluated *before* the effect, fail-closed on deny or error, ordered decide → effect → observe with the audit observer running after. The strip-vs-deny axis (INT-6) maps cleanly onto nodus: the static capability manifest is the *strip* form (an effect the host cannot satisfy is rejected pre-run), and the new per-effect gate is the *deny* form (an available effect is attempted per call and may be refused on its arguments).

Realized in the nodus workspace as a portability-contract refinement (a per-effect authorization seam / PolicyProvider), keeping nodus host-neutral: nodus contributes only the *seam and the ordering/fail-closed guarantee*; the policy itself is host-supplied, and a step may declare its effect class for host matching but can never author or relax the policy (consistent with the workflow-has-no-ambient-authority rule). The nodus workspace owns that realization; this records the relevance.

## 7. Drawbacks & Alternatives

- **Three kinds add ceremony.** Forcing every interceptor to declare a kind is more up-front structure than a single "do-anything" hook. The trade is deliberate: the un-typed hook is exactly the object that makes fail-direction unprincipled and authority illegible (§4.1/§4.3). The ceremony buys structural guarantees a comment cannot.
- **Fail-closed guards can wedge availability.** A flaky decide guard that fails closed will block real work. Mitigated by keeping guards fast and simple, by the observe class absorbing everything that does not need to block, and by governance (`l1-policy-governance`) being able to fix which guards run. Fail-*open* is rejected for guards because a guard that opens on error is not a guard.
- **Transitive enforcement has a cost.** Running the parent's guards on every nested sub-effect is more work than trusting a sub-agent. Accepted: the alternative (a delegation that escapes the envelope) is the failure INT-5 exists to close.
- **Alternative — one un-typed interceptor with flags.** Simpler surface; rejected — it collapses the observe/decide/transform distinction that INT-1/INT-3 depend on, and reproduces the divergent-fail-behaviour problem this spec was written to end.
- **Alternative — enforce guards by prompt instruction.** Rejected as the *mechanism*; low-salience and injection-vulnerable. Instructions may complement a structural guard (as in repo-content-as-data) but never replace it, and where only an instruction is possible INT-8 requires disclosing the coverage limit.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[SECURITY]` | `.design/main/specifications/l1-security.md` | Permission/authority model; SEC-10 principal behind INT-5 |
| `[PROVENANCE]` | `.design/main/specifications/l1-context-provenance.md` | CP-2 neutralization as a transform-class interceptor |
| `[RECEIPTS]` | `.design/main/specifications/l1-tool-receipts.md` | Observe-class receipt at the effect boundary; TR-8 coverage honesty |
| `[GOVERNANCE]` | `.design/main/specifications/l1-policy-governance.md` | Governs which interceptors load (PG-4) |
| `[HOOKS]` | `.design/main/specifications/l2-plugin-hooks.md` | A realization of the taxonomy (reconciliation candidate) |
| `[TOOLSEC]` | `.design/main/specifications/l2-tool-security.md` | Tool guard / ToolPolicy / guardrail realizations; §4.5 strip-vs-deny instance |

## Document History

| Version | Date | Notes |
| --- | --- | --- |
| 1.3.1 | 2026-09-24 | Consistency pass (2026-09-24): The disposition list still called `l2-tool-security` guardrails fail-open and deferred the reconciliation to a future build-process step; both L2s already follow INT-3 (security guardrails fail closed; an erroring blocking hook is an objection) — recorded as realized. |
| 1.3.0 | 2026-08-19 | INT-11 added — **declared dispatch discipline, checked against the dispatch site**. INT-1 fixed *what an interceptor may do* and INT-10 *when it attaches*, but neither said how a point's interceptors **run relative to each other**, which is the property that decides whether an outcome is composed or accidental. The discipline is declared per point from a closed set — broadcast, chained (explicit continuation), fan-out-awaited, ordered-answered — and is **machine-checked against every dispatch site**, which is what stops the declaration from decaying into a comment. Two consequences carry the weight: **delegation is explicit in a chained point**, so an interceptor that neither delegates nor answers has silently swallowed the guarded operation and the contract names that outcome rather than the runtime absorbing it; and **short-circuiting is the design for a single-decision point and a defect for an annotating one**, so a point whose interceptors disagree about which they are produces an outcome whose author depends on registration order — exactly the non-determinism INT-7 forbids for declarative rules, arriving through the programmatic door. Links the new `l1-scoped-capability-layers` (SCL-3 — an event's admission set is a scope question, its dispatch discipline is this one). Distilled from an adoption pass over an external plugin-framework-based agent-harness reference. Additive; no existing invariant weakened. |
| 1.2.0 | 2026-08-05 | Added INT-10 (declared attachment phase relative to resolution) — where an invocation is resolved to a target before execution, every interceptor declares its phase from a closed set (pre-resolution / post-resolution-any / post-resolution-matched), with three binding consequences: only pre-resolution may rewrite the address and a post-resolution rewrite attempt is a defect rather than a tolerated no-op; observation defaults to post-resolution-any because an invocation resolving to nothing is exactly the misadvertisement signal an observer must see (composing ATE-13); and a target-bound guard defaults to post-resolution-matched, because deciding against an address that resolves to nothing spends the check on nothing and answers *denied* where the honest answer is *no such target*, disclosing existence through the error class. Phase is declared and structurally enforced like the INT-1 capability class — an implicit phase makes INT-8 coverage unstatable. Related Specifications extended with `l1-input-binding` (the adjacent supply-vs-guard boundary) and an ATE-13 note. |
| 1.1.0 | 2026-07-23 | Added INT-9 (prefer a convergence point over per-entrance guards) — where multiple entry paths lead to the same governed effect and share a single boundary each must cross for the effect to become real, the deterministic guard SHOULD sit at that convergence point rather than being replicated per entrance; INT-8 makes an incomplete guard *honest*, INT-9 makes it *unnecessary* where a chokepoint exists, since a guard there is source-independent by construction (unable to be evaded through a different actor or path because there is no other path to the effect) and thus has no coverage gap to name. Does not weaken INT-8 (per-entrance guards with named gaps remain honest where no single convergence point exists) and does not drop inline per-path guards (they compose on top for early contextual feedback). Full placement discipline — delta-not-state judgement, deferred obligations, attributed bypass — owned by the new l1-convergence-gate; INT-9 records only the placement preference. Related Specifications extended with `l1-convergence-gate`. |
| 1.0.0 | 2026-07-06 | Initial spec — capability-bounded interception discipline mined from an external agent-SDK reference: three-kind interceptor taxonomy (observe/decide/transform) enforced by type (INT-1); check-before-use TOCTOU-safe ordering (INT-2); fail-direction fixed per class — guards fail closed, observers fail forward (INT-3); nested session⊃turn⊃operation interception state with one-way visibility (INT-4); transitive guard enforcement across delegation, no sub-agent escape (INT-5); strip-vs-deny model-visibility axis (INT-6); deterministic specificity×safety priority resolution with construction-time validation (INT-7); honest coverage boundary for unguarded entry paths (INT-8). Unifies the currently-divergent fail-behaviours of the plugin-hook and tool-security realizations under one L1 contract; ideas-to-adopt + nodus-relevance ledgers recorded. Nodus deliverable of the same pass: l1-nodus-portability LP-11 (per-effect authorization seam / PolicyProvider). |
