# Foreign Agent Invocation

**Version:** 1.0.2
**Status:** Stable
**Layer:** concept

## Overview

The contract for **driving another vendor's agent as a subordinate executor** — launching a foreign agent runtime headlessly with a work order, holding it to a containment posture, continuing its session across calls, and taking its result as a claim to be re-derived rather than a report to be relayed.

This is the third direction of the foreign-tool relationship, and the only one nothing owns yet. The marketplace governs what comes **in** (a third-party entry resolved, attested, vetted, activated). Host-native rendering governs what goes **out** (a definition materialized into a neighbouring tool's own config format). Artifact-derived observation governs what is **read afterwards** (a foreign tool's leftover artifacts, out-of-band, adapter-per-source). None of them covers the act in the middle: *this system authors the instruction, starts the foreign agent, and owns what it produced.*

The centre of gravity is a single, uncomfortable property: **a foreign agent runtime's failures are predominantly silent, and its defaults are not the defaults its primary documented path advertises.** An invocation that inherits an ambient setting is wider than the one that was reviewed; a continuation addressed by "most recent" targets the wrong session and looks identical to one that targeted the right one; a process that reads an idle input channel waits forever at zero load and never errors. Every one of these produces an outcome shaped exactly like success. That is why this contract is written as invocation discipline rather than as a tool adapter.

## Related Specifications

- [l1-host-native-rendering.md](l1-host-native-rendering.md) — the **outbound** twin. HNR writes *our definitions into* a foreign host's config; this spec *starts* a foreign host and consumes its output. HNR's write is authority-neutral; this one grants a foreign actor authority over a workspace, which is why FAI-2 and FAI-8 exist and HNR needs no counterpart.
- [l1-artifact-derived-observation.md](l1-artifact-derived-observation.md) — the **after-the-fact** twin: ADO reads what a foreign tool left behind, out-of-band, having asked for nothing. Here the system *commissioned* the work, so it holds the correlation, the deadline, and the verification duty ADO deliberately disclaims. ADO-9's untrusted-content rule is inherited verbatim (FAI-13).
- [l1-execution-locus.md](l1-execution-locus.md) — LOC decides *where* a world-touching capability runs. This spec decides *who* runs it when the answer is "an actor this system did not build". A foreign executor is bound to a locus like any other capability (LOC-1) and its handles are locus-bound (LOC-3).
- [l1-completion-verification.md](l1-completion-verification.md) — CMP-3 (delegated success is never relayed) and CMP-8 (a roll-up re-executes) are the general rules; FAI-9 states what they require specifically of a black-box delegate whose only observable effect is a diff.
- [l1-consent-binding.md](l1-consent-binding.md) — CB-1/CB-2 bind consent to the *resolved* invocation. FAI-6 is why resolution must be surfaced here even when nobody asked: the resolved form is assembled from ambient files the principal may not remember writing.
- [l1-execution-sandbox.md](l1-execution-sandbox.md) — ES governs confining code *this system* runs. A foreign agent runtime supplies its own confinement, which is why FAI-2 is about *asserting* a posture through the tool's own surface rather than imposing one.
- [l1-competitive-execution.md](l1-competitive-execution.md) / [l1-orchestration.md](l1-orchestration.md) — CPE-3/ORC-5 isolation is the mechanism FAI-8's baseline requirement rides on; this spec adds that the baseline must be *recorded*, because for a black-box delegate the diff is the only evidence of what happened.
- [l1-capability-reachability.md](l1-capability-reachability.md) — REA-11 is the per-executor form of reachability that FAI-11 depends on.
- [l1-work-liveness.md](l1-work-liveness.md) — WL-8's watchdog classifies a run that has gone quiet. FAI-3 and FAI-4 exist so the common case never reaches the watchdog at all.

## 1. Motivation

Left unspecified, an implementation improvises, and each improvisation is a defect class that presents as success:

- **The wider continuation.** The first call is made with an explicit narrow posture. The continuation call is made through a different subcommand that rejects that flag, so the posture falls back to an ambient configuration file — which may be the widest setting the tool offers. A review loop that was read-only for one round silently gains write authority for the rest.
- **The indefinite quiet wait.** The foreign process reads its standard input in addition to its argument. Under a non-interactive driver there is no terminal to close it, so the process waits at effectively zero load, forever, producing no error and no output.
- **The caller's timeout, not the operation's.** The invoking harness applies its own default ceiling, which is shorter than the foreign agent's ordinary working time. Real work is killed mid-run and reported as a failure of the work.
- **The most-recent continuation.** A continuation is addressed by "the last session" instead of the captured identity. With two runs in flight it attaches to the wrong one, and the transcript that comes back is coherent, plausible, and about something else.
- **The relayed proof.** The delegate reports that the checks passed and pastes their output. The output is text the delegate produced. Nothing ran.
- **The unattributable diff.** The delegate is given write authority over a workspace that already carried uncommitted changes. Its effect can no longer be separated from what was there, which also means it can no longer be reverted.
- **The one-bench assumption.** The plan names a capability that exists on the commissioning side and not on the delegate's, or exists on both but has never been exercised in the non-interactive mode the delegate will actually use. The plan is sound and unbuildable.
- **The silent handoff.** A long delegation finishes while the principal is looking elsewhere. Verification proceeds directly into the transcript, and the moment the work actually completed is never marked.

## 2. Constraints & Assumptions

- **The foreign runtime is not modifiable.** Its flags, its subcommand surface, its defaults and its failure behaviours are given. The contract adapts; it never assumes a fix upstream.
- **The delegate is a black box during execution.** It exposes no internal state, no partial progress the caller can interpret, and no intermediate checkpoint. Its effects are its artifacts.
- **The delegate carries no session context.** It begins with nothing this system knows. The work order is the entire brief.
- **Its surface is versioned and drifts.** Which flags a subcommand accepts, and what a missing flag falls back to, change between releases. Anything relied on is a verified fact about the installed version, not a documented promise.
- **Invocation costs the principal.** Calls consume an external allowance, so a configuration error discovered *after* a call has been paid for is a real loss (UA-1, RSN-9).

## 3. Core Invariants

Rules every Layer 2 implementation MUST NOT violate. They are technology-neutral.

- **FAI-1 (The work order is the entire brief):** a foreign executor starts with **no shared context** — no conversation, no working state, no access to the reasoning that produced the request. Everything it needs is in the order it is launched with: the goal, the frozen source of truth it implements or reviews, the paths it may touch, the constraints, the explicit non-goals, and the shape of the report it must return. An order that assumes the delegate can ask, infer from ambient convention, or recover intent from the workspace is not an order — it is a design request, and design does not delegate.

- **FAI-2 (Containment posture is asserted on every call, never inherited):** the confinement the delegate runs under is established **explicitly at each invocation**, by a mechanism valid for *that* invocation path. Two paths of the same tool — an initial launch and a continuation — MUST be treated as having independent surfaces: a flag accepted by one may be rejected by the other, and what a rejected flag falls back to is an ambient configuration this system does not own and MUST NOT assume is narrow. Where the required posture cannot be asserted through a path's own surface, the call is **refused**, not made and hoped about. The posture asserted is verified against the installed version, because which surface accepts what is a property of the build.

- **FAI-3 (Every idle input channel is explicitly closed at launch):** an invocation supplies a definite end to every input stream the delegate might read, whether or not the contract expects it to read one. The failure this closes is **not an error**: a process waiting on an input that will never end produces no output, no diagnostic, and no load, and is indistinguishable from long work. Where the order itself must be delivered through an input channel, that delivery is what closes it; where it is delivered as an argument, the channel is closed separately.

- **FAI-4 (The deadline comes from the operation, not from the caller's default):** every foreign invocation runs under an **explicit deadline derived from what that operation actually takes**, never the invoking harness's ambient default. Two symmetric failures are forbidden: a ceiling shorter than the work kills legitimate runs and misreports them as failures of the work, and no ceiling at all converts FAI-3's residual cases into an unbounded silent wait. A deadline that trips is a **failed run** — surfaced with what is known — never a blind re-invocation, because a re-invocation of an operation that may have partially applied its effects is a second uncontrolled write.

- **FAI-5 (A continuation names its session; implicit selection is forbidden):** the identity of a resumable foreign session is **captured at launch, from the value the tool itself issued**, and every continuation names that identity explicitly. A selector meaning "the most recent session" MUST NOT be used, even where only one session is believed to exist: with concurrency it attaches to the wrong run, and a missing or malformed identity may fall back to that same selector rather than erroring. A wrong-target continuation returns a coherent, well-formed result about different work and is **indistinguishable from a correct one** — which is why the rule is absolute rather than conditional on observed concurrency.

- **FAI-6 (The resolved configuration is disclosed before the first billable call):** what the delegate will actually run as — its model or tier, its reasoning depth, its effective posture, the tool build in use — is **resolved and shown before the first call is made**, so an objection costs nothing. The resolution is assembled partly from ambient files on the principal's machine, which means it is not knowledge the principal reliably has (CB-2's resolved-not-authored discipline, at the point where the resolution happens outside this system entirely). Pinning a configuration the invocation surface will reject is itself a resolution failure and is surfaced, not retried.

- **FAI-7 (Success is decided by the produced artifact, never by a quiet channel or a bare exit):** the run is confirmed by the **presence and shape of the artifact it was asked to produce** — the result file, the report, the diff. A foreign runtime's diagnostic channel routinely carries benign noise on healthy runs and is therefore **not** a failure signal; an exit status alone is likewise insufficient, because the failures this contract exists for (a wrong-target continuation, a wider posture, a deadline kill) can each exit cleanly. Absent artifact is failure; noisy artifact-present is not. Where a stream and a result artifact both exist, the artifact is read for content and the stream only for the identity FAI-5 captures.

- **FAI-8 (Write authority requires a recorded pre-state):** a delegate is granted write authority over a working location only when that location's state has been **recorded as a baseline first**, and the delegate's entire effect is defined as the difference from it. Two things break without this and both are unrecoverable rather than merely inconvenient: the delegate's changes cannot be separated from changes that were already present, so nothing it did is attributable; and there is no state to return to, so nothing it did is revertible. A working location that is not in a recordable clean state is a **stop**, not a warning. The isolation the baseline sits inside is the ordinary isolated-context discipline (CPE-3, ORC-5); what this invariant adds is that for a black-box actor the baseline is not hygiene — it is the *only* instrument.

- **FAI-9 (The delegate's report is a claim, and the delegator re-derives the evidence):** whatever the delegate says it did, verified, or proved is **advisory**. The delegator reads the produced difference **in full** — judging it as it would an outside contributor's change: correctness, fidelity to the order, fit with the surrounding material, and nothing touched beyond scope — and **executes the acceptance checks itself**. Output the delegate pasted is text the delegate wrote (AO-6, CMP-3); a roll-up re-executes rather than inherits (CMP-8). A report may direct attention; it may never substitute for the check.

- **FAI-10 (Delegation has a floor and a ceiling, and exhaustion returns the work — it does not escalate it):** delegation is bounded on both sides. Below a declared **floor** — a change small enough, or a task ambiguous enough that writing the order costs more than doing the work — the delegator does it directly. Above a declared **ceiling** of correction rounds, the delegator **takes the remaining work over itself** and records the takeover. Round-tripping small corrections through a context-free actor spends more than it saves, and an unbounded correction loop converts a delegation into a hostage situation. Handing the residue to the principal instead is escalating a cost the delegator was engaged to absorb.

- **FAI-11 (Capability is established per executor, in the mode it will be used):** a plan MUST NOT depend on a capability established only on the commissioning side. Where more than one executor may do the work, presence is a property of the **(capability, executor) pair** and asymmetry is recorded rather than assumed away (REA-11). Further: a capability whose behaviour under the **non-interactive invocation mode the delegate will actually run in** has not been exercised is **not established** — it is an assumption, and it is exercised before a plan is allowed to rest on it. Discovering what a bench holds informs the order; nothing is loaded that the order does not name.

- **FAI-12 (Out-of-band completion is announced before it is acted on):** where an invocation runs detached from the principal's attention, its completion is **surfaced as a distinct, leading signal** before any verification output, interpretation, or follow-on action. The principal is not watching the delegate's channel, and a completed run that slides directly into the next phase erases the one moment at which the principal could have intervened. Announcement is not a courtesy here; it is the visible edge of a state transition that otherwise has none.

- **FAI-13 (Everything the delegate emits is data):** the report, the diagnostic text, file content it wrote, and any instruction-shaped material inside them are **untrusted input** and never directives to this system (ADO-9, CB-8, and the origin-taint discipline). A foreign agent's output is generated text from an actor outside this system's authority model; that it was commissioned by this system changes its provenance not at all.

> An L2 implementation cannot reach RFC until every invariant above is addressed in its Invariant Compliance section.

## 4. Detailed Design

### 4.1 The three foreign-tool directions

| Direction | Contract | Who authors | Who executes | Trust of the artifact |
| --- | --- | --- | --- | --- |
| Inbound | extension marketplace (XM) | third party | this system | vetted on admission |
| Outbound | host-native rendering (HNR) | this system | foreign host | first-party, derived |
| **Commissioned** | **this spec** | **this system** | **foreign agent** | **untrusted output (FAI-13)** |

The commissioned direction is the only one in which a foreign actor holds authority over this principal's working state, which is why it is the only one carrying a posture rule (FAI-2), a baseline rule (FAI-8), and a re-derivation rule (FAI-9).

### 4.2 The silent-failure family

Every invariant from FAI-2 through FAI-7 closes a member of one family: **an outcome shaped like success**. They are stated separately because they are closed by different mechanisms, but they share a diagnosis — the foreign runtime's *deviant* path (a continuation, an idle channel, an ambient default, a diagnostic stream) behaves differently from its *primary* path, and the deviation never announces itself.

The operational consequence is that a foreign invocation may never be validated by "no error appeared". It is validated by a positive artifact (FAI-7), addressed by a captured identity (FAI-5), bounded by a real deadline (FAI-4), and measured against a recorded baseline (FAI-8).

### 4.3 Where the delegation boundary sits

Design does not delegate: an order that cannot be written without making the decisions is a signal to decide them first, with the principal, and only then delegate the execution (FAI-1, FAI-10's floor). Verification does not delegate either: the actor that produced a thing does not certify it, and a foreign delegate certifying its own work is the same self-endorsement in a different vendor's clothes (FAI-9, and the producer/grader-independence rule ITR-2 of [l1-iterative-refinement.md](l1-iterative-refinement.md)).

What *does* delegate cleanly is bounded execution of a frozen order — a mechanical migration, a refactor with a named shape, a fix with a known reproduction, coverage against an existing surface.

### 4.4 Failure modes named

| Failure | What it looks like | Which invariant closes it |
| --- | --- | --- |
| Widened continuation | A read-only reviewer that can write from round two | **FAI-2** |
| Indefinite quiet wait | A run at zero load, no output, no error | **FAI-3** |
| Caller's-default kill | Real work reported as a failure of the work | FAI-4 |
| Wrong-target continuation | A coherent result about different work | **FAI-5** |
| Surprise configuration | An expensive round paid before anyone could object | FAI-6 |
| Noise read as failure | A healthy run abandoned on benign diagnostics | FAI-7 |
| Unattributable diff | An effect that can be neither isolated nor reverted | **FAI-8** |
| Relayed proof | Pasted check output standing in for a check | FAI-9 |
| Delegation ping-pong | Trivia round-tripped through a context-free actor | FAI-10 |
| One-bench assumption | A sound plan that the delegate cannot execute | FAI-11 |
| Silent handoff | Completion that left no observable moment | FAI-12 |
| Instruction laundering | Delegate output read as direction | FAI-13 |

## nodus-relevance mapping

- **The commissioned invocation is already expressible.** A step that suspends pending a host-supplied external completion, correlated and deterministic, is the language-grain form of this contract's launch/return shape; this spec governs what the *host* owes on the other side of that seam — posture, deadline, identity, baseline, re-derivation — none of which the language should name.
- **The deadline is the one piece the language is missing.** A suspended step with no declared deadline is FAI-4's unbounded case at the workflow grain; a deferred step should declare its own bound and route its elapse as a typed error, exactly as a blocking dialog step already does.
- **Posture belongs to the host, but its *declaration* does not.** Which capability a delegating step requires is already a declared, fail-fast host capability; whether the delegate may write is the same kind of declaration and belongs beside it, not inside the host's discretion.

## 5. Implementation Notes

1. **Verify the surface, don't read about it.** Which subcommand accepts which posture flag, and what a rejection falls back to, is established against the installed build and re-established when it changes (FAI-2).
2. **Capture the session identity from the tool's own emission** at launch and echo it into every continuation visibly, so a wrong-target continuation is at least reviewable after the fact (FAI-5).
3. **Deliver the order through a file, never through inline quoting.** It removes a quoting-defect class and, where delivery uses the input channel, satisfies FAI-3 in the same act.
4. **Treat the result artifact as the read surface and the event stream as identity only** (FAI-7). Parsing content out of a stream that also carries framing is how a partial read becomes a confident wrong answer.
5. **Make the baseline record explicit rather than implied by the isolation** (FAI-8). "It runs in its own workspace" is not the same statement as "its pre-state is recorded".
6. **Put the floor and ceiling in the delegation's own declaration**, not in an operator's judgement at the moment of frustration (FAI-10).

## 6. Drawbacks & Alternatives

- **FAI-2 makes some invocations impossible.** A tool path that cannot express the required posture cannot be used for work needing it. Held: the alternative is an invocation whose authority nobody can state.
- **FAI-9 duplicates work the delegate already did.** Deliberately. The delegate's report and this system's verification are not the same act performed twice; the second is the only one whose evidence this system may stand behind (CMP-3).
- **FAI-8 blocks work in a dirty location.** By design, and it is the invariant most likely to be resented in the moment. The cost of proceeding is discovered later, when a change must be reverted and cannot be.
- **FAI-6 spends a round-trip on disclosure.** It spends the cheapest possible one — the resolution — to avoid burning the expensive one on a configuration nobody chose.
- **Alternative — model the foreign agent as an ordinary tool call.** Rejected: a tool call has a bounded, declared effect surface and returns a value. A foreign agent has an open effect surface, its own confinement model, a resumable session, and an unbounded working time, and every invariant here follows from one of those four properties.
- **Alternative — fold into `l1-artifact-derived-observation`.** Rejected: ADO's defining property is that it asked for nothing (ADO-1, out-of-band by construction) and holds no correlation to the work. This contract commissions, correlates, deadlines, and verifies. Folding them would put a verification duty inside a spec whose whole discipline is *derived, never authoritative*.
- **Alternative — fold into `l1-host-native-rendering`.** Rejected: HNR's write is explicitly authority-neutral — it grants a foreign host nothing the principal had not already granted it. This contract's central problem is granting authority, which HNR never does.
- **Alternative — let each integration carry its own invocation quirks.** Rejected: the quirks are not per-tool trivia, they are one recurring family (§4.2), and per-integration handling means each new one rediscovers the silent-wait and the widened-continuation independently.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[RENDER]` | `.design/main/specifications/l1-host-native-rendering.md` | The outbound twin; the boundary drawn in §4.1 |
| `[OBSERVE]` | `.design/main/specifications/l1-artifact-derived-observation.md` | The after-the-fact twin; ADO-9 inherited by FAI-13 |
| `[COMPLETION]` | `.design/main/specifications/l1-completion-verification.md` | CMP-3/CMP-8, the general form of FAI-9 |
| `[CONSENT]` | `.design/main/specifications/l1-consent-binding.md` | CB-1/CB-2 resolved-invocation discipline behind FAI-6 |
| `[LOCUS]` | `.design/main/specifications/l1-execution-locus.md` | Where a commissioned executor is bound |
| `[REACH]` | `.design/main/specifications/l1-capability-reachability.md` | REA-11, the per-executor rule FAI-11 rests on |
| `[LIVENESS]` | `.design/main/specifications/l1-work-liveness.md` | WL-8's watchdog, the backstop FAI-3/FAI-4 keep unused |
| `[REFINE]` | `.design/main/specifications/l1-iterative-refinement.md` | ITR-2 producer/grader independence, the general form of FAI-9's §4.3 reading |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.0 | 2026-09-01 | Core Team | Initial concept — the **commissioned** direction of the foreign-tool relationship, distinct from inbound admission and outbound rendering: this system authors the order, starts a foreign agent runtime, and owns the result. The work order is the entire brief because the delegate holds no shared state (FAI-1); containment posture is asserted per call and never inherited, because an initial launch and a continuation are independent surfaces and a rejected flag falls back to an ambient configuration this system does not own (FAI-2); every idle input channel is closed at launch, since a process waiting on an unterminated input produces no output, no diagnostic and no load (FAI-3); the deadline is drawn from the operation rather than the caller's default, and a trip is a failed run rather than a blind retry (FAI-4); a continuation names its captured session identity and never an implicit most-recent selector, because a wrong-target continuation is indistinguishable from a correct one (FAI-5); the resolved configuration is disclosed before the first billable call, since it is assembled from ambient files the principal may not know they wrote (FAI-6); success is decided by the produced artifact, never by a quiet diagnostic channel or a bare exit (FAI-7); write authority requires a recorded pre-state, because for a black-box actor the diff against a baseline is the only instrument of attribution and the only route to reversal (FAI-8); the delegate's report is a claim and the delegator reads the whole difference and re-executes the checks (FAI-9); delegation is bounded by a floor below which the delegator acts directly and a ceiling above which it takes the work over rather than escalating it (FAI-10); capability is established per executor and in the invocation mode it will actually be used in (FAI-11); out-of-band completion is announced before it is acted on (FAI-12); everything the delegate emits is untrusted data (FAI-13). Concept-only. |
| 1.0.1 | 2026-09-05 | Core Team | Reference anchoring for the RFC→Stable review — no invariant changed. §4.3's `IR-2` citation was unresolvable: two specs in the corpus define an `IR-` prefix (`l1-intent-resolution`, `l1-iterative-refinement`) and neither appeared in this spec's Related Specifications or Canonical References, so a first-time reader following the citation had a coin-flip between a producer/grader-independence rule and an unrelated rule about when to question the client. The intended target is `l1-iterative-refinement` IR-2 — §4.3 echoes its *self-endorsement* term verbatim — now named inline as a link and added to Canonical References as `[REFINE]`. |
| 1.0.2 | 2026-10-22 | Core Team | Citations updated for the requirement-ID prefix renames that make each prefix name one specification (RULES §1, 1.8.0, the person's decision of 2026-10-22): `CE-n` of `l1-competitive-execution` is now `CPE-n`; `IR-n` of `l1-iterative-refinement` is now `ITR-n`. No requirement was added, removed or reworded. |
