# Completion Verification

**Version:** 1.5.1
**Status:** Stable
**Layer:** concept

## Overview

The honesty contract for **claims about work state** — done, passing, fixed, succeeded,
requirements-met. It is the missing member of the grounding family: `l1-claim-verification`
grounds claims about *facts* against sources, `l1-tool-receipts` authenticates that an
*action* happened and its result is real — and this grounds a claim that *work is
complete* against fresh evidence from the authoritative check. The failure it closes is
the highest-frequency dishonesty in autonomous work: asserting "tests pass" without
running them this turn, calling a bug "fixed" because the code changed, reporting
"requirements met" because a suite is green, and — the delegation case — relaying a
sub-agent's self-reported "success" upward as a completion without ever looking at what
it actually produced. The rule is one line: **no completion claim without fresh
verification evidence, and a subordinate's reported success is a signal to verify, never
a verdict to relay.**

## Related Specifications

- [l1-claim-verification.md](l1-claim-verification.md) — the faithfulness-of-*facts* sibling (CV-2/CV-4: a claim needs a cited source); this spec is the same discipline for *work-state* claims, which need a run's fresh evidence rather than a source citation.
- [l1-tool-receipts.md](l1-tool-receipts.md) — TR-3/TR-4 authenticate an action's existence and result via a keyed receipt the runtime witnesses; CMP-3 verifies a *delegated actor's reported* success against an observable artifact — the cross-actor / un-witnessed case receipts do not cover.
- [l1-optimization-integrity.md](l1-optimization-integrity.md) — OI-8 fail-visible (an unrun check is `unverified`, never rendered as `verified`); CMP applies exactly that to completion status.
- [l1-quality-standards.md](l1-quality-standards.md) — owns the definition-of-done *gates* (what must be true to be done); this spec owns the behavioral honesty that a green gate may not be *claimed* without being *run and observed* this turn.
- [l1-orchestration.md](l1-orchestration.md) — ORC-5 isolation / ORC-11 error containment: a delegation boundary summarizes upward; CMP-3 requires that summary's *success* be verified against the subordinate's observable output, not passed through on its word.
- [l1-work-liveness.md](l1-work-liveness.md) — a run's terminal state is a CMP-verified outcome (its real result), never a narrated "it worked"; WL owns the ownership/recovery, CMP owns the honesty of declaring it done.
- [l1-loop-governance.md](l1-loop-governance.md) — LG-4 oracle ownership: the check that proves a completion is independent of the actor that produced the work (a generator does not self-certify done), mirroring the verifier-independence discipline.
- [l1-nodus-observability.md](../../nodus/specifications/l1-nodus-observability.md) — a workflow's `RunResult` (typed status + step outcomes, HO-*) is the observable artifact a host verifies (see nodus-relevance mapping).
- [l1-outcome-confidence.md](l1-outcome-confidence.md) — [ADDED v1.1.0] the probabilistic sibling CMP-6 keeps separate: evidence decides *done*, an estimate decides *how hard to look*; neither may overwrite the other.
- [l1-acceptance-oracle.md](l1-acceptance-oracle.md) — [ADDED v1.3.0] the authoring-time head of this chain: CMP-4 forbids a *weaker but real* proxy check, AO forbids an instrument **structurally incapable of failing**. Every guarantee here is conditional on the criterion being meaningful, which is the question AO owns.
- [l1-fanout-attestation.md](l1-fanout-attestation.md) — [ADDED v1.3.0] FAN-6: a recorded return is scheduler completion, never verification. The launch record and the completion evidence are separate acts and neither substitutes for the other (CMP-3).

## 1. Motivation

An agent that has *almost* finished is under the strongest pressure to say it *has*
finished — the work looks done, the last edit "should" compile, the tired end of a long
task wants to be over. So it writes "Done — all tests pass" without running the tests,
"Fixed" because it changed the code that looked wrong, "Phase complete" because the suite
is green even though a required behavior was never built. Each is a confident, plausible,
*false* status — and the cost lands later: a crash from an undefined function, a shipped
feature that was never implemented, a redirect-and-rework cycle, and the trust the human
withdraws the first time "it's done" turns out to be a lie.

The delegation case is worse because it is invisible. An orchestrator hands a task to a
sub-agent; the sub-agent reports "success"; the orchestrator relays "the task is
complete" upward — having never looked at the diff, the output, or the run result. The
sub-agent may have done nothing, done it wrong, or reported success on a partial. A
self-report is the *least* trustworthy evidence of completion precisely where completion
matters most.

The existing honesty specs do not close this. Claim-verification grounds *factual*
claims against *sources* — a completion claim has no source, it has a *check*.
Tool-receipts authenticate an action the runtime *witnessed* — a subordinate's narrated
success is exactly the un-witnessed, cross-actor case receipts leave open (TR-8). What is
missing is the contract that a claim about work being *done* is backed by fresh evidence
from the check that proves it, and that a delegated success is verified against an
observable artifact the reporter did not author. Built once, it serves every surface that
declares work complete — a status line, a hand-off, a commit message, an orchestrator's
roll-up — instead of each improvising a trust-the-report shortcut.

## 2. Constraints & Assumptions

- This spec governs *claims about work state*, not the work itself. It never decides whether the gates are the right gates (quality-standards) or how work is recovered (work-liveness) — only that a completion status must be earned by fresh evidence before it is asserted.
- "Fresh" means *this turn*: evidence from the authoritative check run now, over the current state — not a prior run, not an extrapolation, not "it should."
- The authoritative check is the one that actually proves the specific claim; a proxy (a linter for a build, a build for tests, one test for a regression fix) is not it.
- A subordinate actor's self-report is input, never evidence; the evidence is the observable artifact it produced (diff, output, run result), read by the delegator.
- Where verification genuinely cannot run, the honest output is an explicit *unverified* state, never a completion claim — degrading honestly, never optimistically.

## 3. Core Invariants

Rules every Layer 2 realization MUST NOT violate. They are technology-neutral.

- **CMP-1 (Fresh evidence before a completion claim):** a claim that work is complete, passing, fixed, or succeeded MUST be backed by verification evidence gathered *this turn* from the authoritative check for that claim — the test run's output and exit code, the build's exit code, the original symptom re-run and now passing. Extrapolation, a prior run, "should pass," confidence, or a partial check is not evidence. Absent fresh evidence, the honest report is the *actual verified* state, not the intended one.

- **CMP-2 (The claim names its check):** every verifiable completion claim is paired with the specific check that proves it — the command, its full output, its exit/failure count — so the claim is reconstructable and falsifiable rather than a bare assertion. A claim whose proving check cannot be named or run is surfaced as *unverified* (composing `l1-optimization-integrity` OI-8), never asserted as done.

- **CMP-3 (Delegated success is verified independently, never relayed on report):** when work is delegated to a subordinate actor — a sub-agent, a tool, an external worker — that actor's *self-reported* success is NOT a completion the delegator may pass upward. The delegator verifies the outcome against an observable artifact the subordinate did not author the report of (the actual diff/changed files, the actual output, the actual run result) before treating the work as done. A subordinate's "success" is a signal to verify, not a verdict to relay.

- **CMP-4 (The check proves the claim, not a weaker proxy):** the check backing a completion claim MUST actually prove *that* claim, not a cheaper stand-in for it — a passing linter does not prove the build compiles, a passing build does not prove the tests pass, one passing test does not prove a regression is fixed (a regression fix is proven only by the symptom failing *before* and passing *after*), and a green suite does not prove the *requirements* are met. A "requirements met" claim is checked against the requirements themselves, item by item, with gaps reported — never inferred from a passing suite. A proxy check reported as if it were the real one is a false completion.

- **CMP-5 (No unverified completion status is surfaced as done):** no surface — a status line, a report, a hand-off, a commit or PR message, an orchestrator's roll-up — presents work as complete or passing on the strength of an unverified assumption or a relayed self-report. The completion status a consumer reads is one the system actually verified; an unverified status is labeled *unverified*, never rendered as done. This is the execution-completion sibling of `l1-claim-verification` CV-2/CV-4 (facts) and `l1-tool-receipts` TR-4 (actions).
- **CMP-6 (A confidence estimate is never evidence, in either direction):** [ADDED v1.1.0] a probabilistic estimate of how likely an outcome is to hold up — however well calibrated, however produced — is **not verification evidence** and MUST NOT satisfy, replace, weaken, shorten, or waive any check CMP-1…CMP-5 require. A high estimate is not a reason to skip a check; a check's absence is not excused by a high estimate. The two answer different questions and compose in one direction only: **evidence decides whether the work is done; an estimate decides how hard to look and whom to involve** — so a *low* estimate may legitimately demand *more* checking than the minimum, while a high one may never demand less. Nor does the converse hold: a passing check does not license suppressing a low estimate from the record or the hand-off, because the estimate's value lies precisely in flagging the gaps no check was written for (silent incompleteness). Both travel with the outcome; neither is permitted to overwrite the other.

- **CMP-7 (A dismissal is a claim and needs the same evidence):** [ADDED v1.2.0] CMP-1…CMP-6 govern the claim *that work succeeded*. The **symmetric** claim — that an observed failure is **not this work's responsibility** ("pre-existing", "unrelated to my change", "flaky", "an environment issue", "it fails on main too") — is equally a factual assertion about the world, and it is the one routinely asserted with no evidence at all. It requires the same standard: the check run against the **baseline** with its output shown. A failure that reproduces on the baseline is pre-existing; a failure that does **not** reproduce there **is** this work's, whatever the reasoning suggests. Where the baseline cannot be run, the honest report is *unverified — may or may not be related*, carried forward as a risk rather than resolved by assertion. The asymmetry this closes is stark and dangerous: a system that demands proof to say *done* while accepting confidence to say *not mine* has left its cheapest escape hatch open, and every unproven dismissal removes a real regression from view precisely when it is cheapest to find.

- **CMP-8 (Evidence does not travel upward — a roll-up re-executes, it does not inherit):** [ADDED v1.3.0] where completion aggregates through a hierarchy — a parent over children, a branch over leaves, an orchestrator over delegated units — the parent's verification MUST **re-execute the child's own authoritative checks**, not read the child's recorded status or replay the child's stored evidence. Inspecting a recorded result is *reporting*, not verifying: it reproduces whatever was true when the child ran, in the child's environment, before every change made since — including the parent's own integration. Two consequences bind. A child that passed and whose oracle no longer passes **returns to unmet** on re-execution, and that demotion is the mechanism working. And a child whose required outcome was **explicitly handed off** — declared impossible within the authorized work and recorded with its reason — is **terminal non-success**: it never aggregates into a complete parent, at any level, however many siblings passed. A set of complete siblings plus one handoff is a handoff. Extending CMP-3 from *the delegate's word* to *the delegate's record*, and closing the aggregation path by which a locally honest failure becomes a globally clean report.

- **CMP-9 (Evidence names the environment it was produced in):** [ADDED v1.3.0] a completion claim's evidence records the **resolved conditions the check ran under** — the executing runtime or interpreter, the working location, the resolution context that decided what its names meant, and the observed termination — because the same authored check, run under different conditions, is a different check and can produce a different verdict. On re-execution (CMP-8) a **material difference in those conditions is a mismatch to resolve, not a pass to accept**: the honest outcome is *unverified — environment differs*, carried forward like any other unverified state, never quietly credited as evidence. This is the environmental axis of CMP-1's freshness rule: CMP-1 forbids evidence that is stale in *time*, CMP-9 forbids evidence that is stale in *place*.

- **CMP-10 (A completion requiring a counterparty is not satisfied by the actor's own contribution):** [ADDED v1.4.0] CMP-1…CMP-9 govern claims about **work the actor did** — was it verified, was it proxied, was the failure really unrelated, did the roll-up re-execute. A distinct class escapes all of them: an outcome whose definition requires **a second party's participation** — a human's decision, an approval, an interview answer, an external system's acknowledgement — where the actor **supplies that party's side itself** and then reports the outcome. Every prior invariant passes: the work was real, the evidence is fresh, the check proves the claim, the report is honest. What was fabricated is not the report but the **participant**. Such an outcome is complete only when the counterparty's contribution is **present and attributable** — recorded, and naming who or what supplied it — and where the counterparty was unavailable the honest outcome is **not complete**, never complete-with-an-inferred-answer. The detectable shape is specific and worth naming because it is what actually occurs rather than a sophisticated forgery: **a record with one participant, and an outcome attributed to nobody.**

- **CMP-11 (Producing the evidence is not examining it):** [ADDED v1.5.0] where a check's output is an **artifact a person or agent must look at** — a capture, a recording, a rendered page, a generated report, a diff — the completion claim is backed only once the artifact has actually been **examined**, and the claim states **what was looked for and what was seen**. Generating the artifact is the cheap half and is routinely mistaken for the whole: the file exists, its path appears in the report, and nothing in the record distinguishes an inspected capture from an unopened one. CMP-4 forbids a *weaker* check; this forbids the *right* check whose result nobody read. Two corollaries bind it. The **medium must be able to carry the property**: a still image cannot evidence a transition, a duration, or an ordering, so a claim about behaviour over time is examined in a medium that records time. And the **observation must enter at the same layer as the claim**: evidence produced by driving a layer *above* the one under test (synthesizing an event downstream of the mechanism that was supposed to produce it) cannot establish that the mechanism works, and reads as a pass. An artifact that was produced but not examined is an unverified claim with a file attached, and per CMP-5 it is reported as unverified.

> L2 specs cannot reach RFC status until all invariants here are addressed in their "Invariant Compliance" section.

## 4. Detailed Design

### 4.1 The completion gate

```text
[REFERENCE]
claim_complete(work, claim):
    check := authoritative_check_for(claim)         // CMP-4 — the check that PROVES this claim, not a proxy
    if check is None:
        return report(status: UNVERIFIED, reason: "no runnable check")   // CMP-2/CMP-5 — never assert done
    evidence := run(check)                            // CMP-1 — fresh, this turn, over current state
    if not evidence.confirms(claim):                 // read full output, exit code, failure count
        return report(status: evidence.actual_state, proof: evidence)     // honest actual state
    return report(status: DONE, proof: { check, evidence.output, evidence.exit })   // CMP-2 — claim names its check
```

The claim is never made before the gate; the gate's evidence *is* the claim's warrant.

### 4.2 Delegated completion (CMP-3)

```text
[REFERENCE]
accept_delegated(subordinate_report, task):
    // the report's "success" is INPUT, not evidence
    artifact := observe(task)          // the diff / output / run result the subordinate did NOT author
    if artifact is absent:
        return UNVERIFIED("subordinate reported success but produced no observable artifact")  // TR-4 kinship
    return claim_complete(artifact, task.done_criteria)   // verify the artifact, then (only then) roll up
// an orchestrator relays a completion only after this returns DONE — never on the strength
// of the subordinate's word (ORC-5 summarizes upward; CMP-3 makes the summary's success earned)
```

### 4.3 Proxy checks that do not prove the claim (CMP-4)

| Claim | Proves it | Does NOT prove it |
| --- | --- | --- |
| Build compiles | build exit 0 | linter clean |
| Tests pass | test run: 0 failures | build succeeds, "should pass" |
| Bug fixed | original symptom reproduced → now passes | code changed, looks right |
| Regression test works | red→green verified (fails before fix, passes after) | test passes once |
| Requirements met | each requirement checked line-by-line | suite green |
| Delegated task done | the subordinate's observable artifact verified | the subordinate's "success" report |

### 4.4 Relationship to the honesty family

| Spec | Grounds a claim about… | Evidence it demands |
| --- | --- | --- |
| l1-claim-verification | a *fact* | a cited source span (CV-4) |
| l1-tool-receipts | an *action* happened + its result | a keyed runtime receipt (TR-3/TR-4) |
| **this spec** | *work is complete / passing* | a fresh run of the authoritative check (CMP-1) |

The three are one discipline — *no assertion without its proper evidence* — applied to
the three things an agent asserts: what is true, what it did, and what it finished.

## nodus-relevance mapping

A workflow already produces a grounded outcome — a `RunResult` with a typed status
(`Completed` / `Failed`), per-step results, and typed error codes (`@err:`, `!!`
invariants, the error taxonomy) — so the workflow layer is *well-positioned*, not
deficient. CMP is a **host-side consumption discipline** over that outcome and needs
**no new language invariant** (nodus LP-1/LP-2):

| Element | nodus seam | Note |
| --- | --- | --- |
| Fresh evidence (CMP-1) | the `RunResult` of the actual run | The host reads the real terminal status + step outcomes; it never narrates a success the `RunResult` does not show. |
| The claim names its check (CMP-2) | `AuditProvider` run trace (HO-*) | The proving evidence is the recorded run; a completion cites its `RunResult`, not a bare "it worked." |
| Delegated success verified (CMP-3) | a sub-workflow's `RunResult` is the observable artifact | An orchestrating host verifies the sub-run's status, never a narrated claim about it. |
| Right check not a proxy (CMP-4) | `!!` invariants + `@test:` blocks assert the actual claim | A step's own invariants/tests are the authoritative check; a green lint is not a passing run. |

nodus's contribution stays *produce a truthful `RunResult`*; the host must then *not lie
about it*. A host that already trusts the `RunResult` behaves exactly as intended — so the
mapping is additive and warrants no new NL invariant.

## 5. Drawbacks & Alternatives

- **Verification costs a round-trip.** Running the real check before every completion claim adds latency. Accepted: a false "done" costs far more (a crash, a shipped gap, lost trust and rework), and CMP-1 only demands the check that *already* defines done — it adds honesty, not new gates.
- **Some claims have no runnable check.** A genuinely un-checkable completion (a subjective judgment) cannot be mechanically verified. CMP-2/CMP-5 handle this honestly: it is surfaced as *unverified*, never dressed as done — the same fail-visible posture as OI-8.
- **Alternative — trust the actor (self or delegated).** Rejected: an actor's own report is the least reliable evidence of its success, and the delegation case (relaying a sub-agent's "success" unread) is exactly where false completions hide (CMP-3).
- **Alternative — fold into `l1-quality-standards`.** Rejected: quality-standards defines *which gates* make work done; it does not govern the *behavioral honesty* of not claiming a gate green without running it, nor the cross-actor rule that a delegated success is verified independently. Those are a distinct contract — the completion member of the grounding family, sibling to claim-verification and tool-receipts, not a definition-of-done detail.
- **Alternative — fold into `l1-claim-verification`.** Rejected: that spec grounds *factual* claims against *sources*; a completion claim has no source, it has a *check*, and its delegated-verification rule (CMP-3) has no analogue in source-grounding. Same family, different evidence.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[CLAIM-VERIFY]` | `.design/main/specifications/l1-claim-verification.md` | The facts-grounding sibling (CV-2/CV-4) this concept mirrors for work-state claims. |
| `[RECEIPTS]` | `.design/main/specifications/l1-tool-receipts.md` | Action authenticity (TR-3/TR-4); CMP-3 covers the delegated/un-witnessed success receipts leave open. |
| `[QUALITY]` | `.design/main/specifications/l1-quality-standards.md` | The definition-of-done gates CMP requires be *run and observed* before being claimed. |
| `[OPT-INTEGRITY]` | `.design/main/specifications/l1-optimization-integrity.md` | OI-8 fail-visible (unverified never rendered as verified) applied to completion status. |
| `[ORCH]` | `.design/main/specifications/l1-orchestration.md` | ORC-5/ORC-11 delegation boundary CMP-3 makes report-a-success-only-after-verifying. |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.3.0 | 2026-08-25 | Core Team | Added CMP-8 (evidence does not travel upward — a roll-up re-executes, it does not inherit) and CMP-9 (evidence names the environment it was produced in). CMP-8 closes the **aggregation** path the family had left open: CMP-3 required a delegate's *word* be verified, but said nothing about a delegate's *record*, so a parent could satisfy itself by reading a child's recorded status — reporting, not verifying, since a stored result reproduces what was true in the child's environment before every subsequent change including the parent's own integration. A parent therefore re-runs the child's authoritative checks; a child that no longer passes returns to unmet, and that demotion is the mechanism working. It also fixes the roll-up of an **explicit handoff** — a required outcome declared impossible within the authorized work and recorded with its reason is terminal non-success that never aggregates into a complete parent at any level: complete siblings plus one handoff is a handoff, not a completion. CMP-9 adds the **environmental** axis to CMP-1's temporal one: the same authored check under a different interpreter, location, or name-resolution context is a different check, so evidence records the resolved conditions it ran under and a material difference on re-execution is *unverified — environment differs*, a mismatch to resolve rather than a pass to accept. Related Specifications extended with `l1-acceptance-oracle` (the authoring-time head of the chain — CMP-4 forbids a weaker-but-real proxy, AO forbids an instrument that cannot fail) and `l1-fanout-attestation` (FAN-6 — a scheduler return is not a verification). |
| 1.2.0 | 2026-08-05 | Core Team | Added CMP-7 (a dismissal is a claim and needs the same evidence) — closing the family's open asymmetry: CMP-1…CMP-6 hold *success* claims to fresh, named, proving evidence, while the symmetric claim that a failure is **not this work's** ("pre-existing", "unrelated", "flaky", "fails on main too") was accepted on confidence alone. It now requires the same standard — the check run against the **baseline**, output shown; reproduces there = pre-existing, does not reproduce there = **yours**, whatever the reasoning suggests; baseline unrunnable = *unverified, may or may not be related*, carried as a risk rather than resolved by assertion. Demanding proof to say *done* while accepting confidence to say *not mine* leaves the cheapest escape hatch open, and every unproven dismissal hides a real regression exactly when it is cheapest to find. |
| 1.1.0 | 2026-07-23 | Core Team | Added CMP-6 — a probabilistic outcome estimate is **not evidence in either direction**: however well calibrated, it may never satisfy, replace, weaken, shorten, or waive a check CMP-1…CMP-5 require, and a check's absence is never excused by a high estimate. Composition is one-way — evidence decides *whether the work is done*, an estimate decides *how hard to look and whom to involve* — so a low estimate may demand more checking than the minimum while a high one may never demand less. The converse is also closed: a passing check does not license suppressing a low estimate from the record or the hand-off, because the estimate's value is precisely in flagging the gaps no check was written for. Both travel with the outcome; neither overwrites the other. Related Specifications extended with `l1-outcome-confidence`. |
| 1.0.0 | 2026-07-15 | Core Team | Initial spec — completion verification as the work-state member of the grounding/honesty family (beside claim-verification for facts and tool-receipts for actions): fresh same-turn evidence from the authoritative check before any complete/passing/fixed/succeeded claim, actual-state reported otherwise (CMP-1); the claim names its check — command + output + exit/failure count, reconstructable/falsifiable, unrunnable ⇒ unverified not asserted (CMP-2); delegated success verified independently against an observable artifact the subordinate did not author its report of, never relayed on the subordinate's word (CMP-3); the check proves the claim not a weaker proxy — linter≠build, build≠tests, one-pass≠regression-fixed (red-green), green-suite≠requirements-met (checked line-by-line) (CMP-4); no surface presents work as done on an unverified assumption or relayed self-report, unverified is labeled not rendered as done (CMP-5); §4.1 completion gate, §4.2 delegated completion, §4.3 proxy-check table, §4.4 honesty-family relationship; nodus-relevance mapping needing no new NL invariant — the RunResult/@err/!!/@test seams already ground outcomes, CMP is the host-side "don't lie about the RunResult" consumption discipline. Distilled from an adoption pass over an external agent-methodology reference whose brainstorm→spec→plan→subagent-execution pipeline, TDD/YAGNI/DRY gates, and systematic-debugging root-cause discipline were already realized by the magic SDD engine + l1-development-workflow + l1-quality-standards + l1-deliberation/lookahead + l1-orchestration recovery ladders — CMP captures the one unowned delta: the verify-before-claiming-done + verify-delegated-success honesty contract that sat in the gap between claim-verification (facts) and tool-receipts (actions). |
| 1.4.0 | 2026-08-26 | Core Team | Amended — CMP-10: a completion requiring a **counterparty** is not satisfied by the actor's own contribution. CMP-1…CMP-9 all govern claims about work the actor did; a distinct class escapes every one of them — an outcome whose definition requires a second party's participation (a human decision, an approval, an interview answer, an external acknowledgement) where the actor supplies that party's side itself. Every prior invariant passes: the work was real, the evidence fresh, the check proving, the report honest. What was fabricated is the **participant**, not the report. Completion requires the counterparty's contribution to be **present and attributable**, and where the counterparty was unavailable the honest outcome is *not complete*, never complete-with-an-inferred-answer. The detectable shape is named because it is what occurs rather than a sophisticated forgery: **a record with one participant, and an outcome attributed to nobody.** |
| 1.5.0 | 2026-09-04 | Core Team | Amended — CMP-11: producing the evidence is not examining it. CMP-1…CMP-10 hold a claim to fresh, proving, independently-verified evidence and assume that evidence is *read*; the assumption fails for every check whose output is an artifact somebody must look at — a capture, a recording, a rendered page, a generated report — where the file's existence and its path in the report are indistinguishable from an inspection that never happened. The claim now states what was looked for and what was seen. Two corollaries: the **medium must carry the property** (a still frame cannot evidence a transition, a duration, or an ordering, so a temporal claim is examined in a temporal medium), and the **observation enters at the claim's own layer** (evidence produced by synthesizing an event downstream of the mechanism under test cannot establish that the mechanism fired, and reads as a pass). Distinct from CMP-4, which forbids a *weaker* check: this forbids the *right* check whose result nobody read, and routes it to CMP-5's unverified. |
| 1.5.1 | 2026-09-24 | Core Team | Consistency pass (2026-09-24): Bare `LP-n` citations of the nodus portability contract are now written `nodus LP-n`: this workspace's `l1-lookahead-planning` defines LP-1…LP-6 as well, so the bare form pointed a reader at the wrong invariant. No requirement changed. |
