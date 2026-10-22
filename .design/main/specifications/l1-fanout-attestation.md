# Fan-Out Attestation

**Version:** 1.0.1
**Status:** Stable
**Layer:** concept

## Overview

Fan-out attestation is the contract that makes **concurrency a claim backed by evidence** rather than a story told about a run. When a coordinator reports that it ran N units in parallel, that statement is, today, unfalsifiable: a coordinator that launched one worker, waited for it, then launched the next produces the *same* artifacts, the *same* logs, and the *same* completion report as one that genuinely fanned out. The difference is invisible in the record and surfaces only as wall-clock nobody attributes correctly.

The mechanism is a **launch barrier**: a fan-out is recorded as a bounded episode that must *open* with its exact member set declared, record a **host-issued launch handle for every member**, and be *sealed* — all before the coordinator performs its first wait. A result accepted into an unsealed episode is refused. This ordering is what makes the serial-in-disguise pattern structurally unrecordable as parallel: launch-then-wait-then-launch cannot produce a sealed episode, because it must wait while members are still unlaunched.

This spec is deliberately **orthogonal** to the coordination family. Parallel staffing, competitive execution, and deliberation each decide *what* is fanned out and *how fan-in resolves*; none of them governs the launch act itself, and all three are equally able to claim a concurrency they did not perform. This concept owns that one act, and the honest bound on what attesting it does and does not prove.

## Related Specifications

- [l1-parallel-staffing.md](l1-parallel-staffing.md) — the throughput fan-out (disjoint partitions, integrate-all); its fan-out step is subject to this barrier, and PS-2 disjointness is the ownership precondition of a launch episode.
- [l1-competitive-execution.md](l1-competitive-execution.md) — the best-of-N fan-out; CPE-7 bounded width is the same width this spec partitions into episodes, and CPE-9 observable contest is what an attested episode makes true rather than asserted.
- [l1-deliberation.md](l1-deliberation.md) — the synthesis fan-out; independent perspectives are only independent if they actually ran independently, which is what the barrier records.
- [l1-orchestration.md](l1-orchestration.md) — the delegating side that opens episodes; ORC-5 context isolation, ORC-7 budget bound, ORC-12 transparent coordination.
- [l1-completion-verification.md](l1-completion-verification.md) — CMP-3: a recorded return is scheduler completion, never verification; the two acts are separate and this spec never substitutes for the other.
- [l1-work-liveness.md](l1-work-liveness.md) — WL-1 exclusive claim and WL-9 exact-once fan-out; a member is claimed before it is launched, and an episode is the launch-side record of the fan-out WL-9 keys.
- [l1-tool-receipts.md](l1-tool-receipts.md) — the same honesty shape one layer down: a receipt authenticates that an *action* occurred; a launch handle authenticates that a *start* was accepted. The TR-8 honest-coverage discipline is FAN-5.
- [l1-execution-locus.md](l1-execution-locus.md) — where a member actually executes; the locus decides whether a native concurrent facility exists at all (FAN-8/FAN-9).
- [l1-telemetry.md](l1-telemetry.md) — the episode transitions and timestamps are ordinary observable events; concurrency reporting reads them rather than inferring overlap.
- [l1-outcome-attributed-cost.md](l1-outcome-attributed-cost.md) — cost attribution needs a member identity the office can see; a detached process farm (FAN-9) breaks it.
- [../../nodus/specifications/l1-nodus-language.md](../../nodus/specifications/l1-nodus-language.md) — NL-24 is the workflow-language realization: `~PARALLEL` is a scheduling hint, so the run record states the **realized** mode rather than the declared one.

## 1. Motivation

Every fan-out mode in the coordination family assumes its members ran at the same time. The budget model assumes it (N attempts cost N× tokens but 1× wall-clock). The isolation argument assumes it (rival attempts do not contaminate each other *because they were concurrent and separated*). The client-facing projection asserts it (three workers visibly on one unit). Nothing checks it.

The failure is not exotic; it is the default failure of an actor implementing fan-out for the first time. Launching a worker and reading its result is the natural shape of a single call, and repeating that shape in a loop reads exactly like fanning out. The coordinator is not lying — it believes it fanned out — and no artifact in the run disagrees. What is lost is precisely the property the fan-out was purchased for: the wall-clock is N× the plan, the deadline is missed, and the postmortem finds a correct-looking record of a parallel run.

A second, subtler failure sits beside it. Where a host offers no non-blocking launch, an actor under pressure to *report* parallelism will describe a sequential run as parallel rather than declare the limitation. The remedy for both is the same, and it is structural rather than exhortative: make the launch act leave a record that a serial run cannot produce, and bound what that record is allowed to prove.

## 2. Constraints & Assumptions

- **The host owns scheduling.** The office records launches; it does not create concurrency. Where the execution host provides no concurrent facility, no discipline here manufactures one.
- **A handle is opaque and non-secret.** The identity a host returns for a launched member is a routing value — it carries no prompt, no credential, and no result body, and it is not an authentication token.
- **Attestation is not isolation.** Recording that N members started concurrently says nothing about whether they can write each other's files. Confinement is a separate mechanism.
- **Wall-clock overlap is not directly observable.** The office sees the transitions it recorded, not CPU scheduling; the attestation is therefore about *acceptance ordering*, and its claim is bounded accordingly.
- **Fan-out width is bounded elsewhere.** Budget and policy set how many members may run; this contract governs the launch of whatever set was afforded.

## 3. Core Invariants

Rules every Layer 2 implementation MUST NOT violate. They are technology-neutral.

- **FAN-1 (Concurrency is attested, never assumed):** any record, report, projection, or cost model that states work ran **concurrently** MUST rest on a recorded launch episode for that work. Absent one, the honest record is *sequential* or *unattested* — never parallel. A coordinator's belief that it fanned out is not an observation of fan-out.

- **FAN-2 (The launch barrier — every start precedes the first wait):** a launch episode declares its **exact member set**, records a start for **each** declared member, and is **sealed**, all before the coordinator performs its first wait, join, or result read. Sealing is refused while any declared member lacks a start; accepting a member's return into an unsealed episode is refused. This ordering is the whole mechanism: the serial pattern cannot satisfy it, because it must wait before the remaining members are launched.

- **FAN-3 (Handles are host-issued, never coordinator-authored):** each start record carries the identity the **host scheduler itself returned** from a non-blocking launch. Synthesizing an identity, reusing another member's identity, or recording the result of a blocking/foreground call as a start is a **forged attestation** and MUST be refused rather than accepted. Opening an episode is an execution claim; a coordinator that cannot obtain a real handle has not launched anything.

- **FAN-4 (Partial launch never seals — recover or abandon, never invent):** if a member's launch fails before a handle exists, the episode stays **open**: the coordinator retries that member, or records an explicit **abandonment carrying a non-empty reason**. An abandoned episode is **terminal and non-successful** and MUST NOT be reported as a completed fan-out. Sealing around a missing member, silently shrinking the declared set, or deleting the episode to escape the state are all forbidden — the audit trail of a failed fan-out is worth more than a tidy record of a fictional one.

- **FAN-5 (The attestation bounds its own claim):** a sealed episode proves **exactly one thing** — the host accepted a distinct start for every declared member before any member's result was accepted. It does **not** prove wall-clock overlap, worker honesty, write isolation, filesystem or resource separation, successful completion, or correct integration. A surface MUST NOT present a sealed episode as evidence of any of those, and each of them remains the responsibility of the layer that owns it.

- **FAN-6 (A return is scheduler completion, not verification):** recording a member's return states that the scheduler finished it — **including a failed result**. It never marks the member's work verified, never promotes its parent, and never substitutes for verifying that member's outcome against an observable artifact. The two acts are ordered and distinct: record the return, then verify.

- **FAN-7 (Rolling episodes bounded by host concurrency, reported in declared order):** members exceeding the host's current concurrency limit are **partitioned into later episodes**, never launched and hoped for. When a verified member unblocks others, a **new episode opens for the newly-ready set** without waiting for unrelated in-flight members — each episode carries its own barrier, so rolling dispatch never weakens it. Results are reported and recorded in **declared member order**, not completion order, so two runs of the same fan-out produce comparable records regardless of who finished first.

- **FAN-8 (Declared fallback where the host cannot fan out):** where the execution host offers no non-blocking launch, the coordinator **declares a sequential fallback** in its plan and runs sequentially. It MUST NOT open an episode it cannot honor, and MUST NOT describe the outcome as parallel. An honest sequential run is a correct outcome; a sequential run described as concurrent is a false record that corrupts every downstream estimate built on it.

- **FAN-9 (Native scheduling over detached process farms):** where the host provides a native concurrent-execution facility, fan-out uses it rather than spawning detached copies of the runner. A process farm surrenders the session's own scheduling, cancellation, observability, and cost attribution — the office would then attest a set of workers it cannot see, stop, or bill, which is a worse record than none. Where a native facility exposes **no per-member identity**, no episode is opened: the facility's own progress record is retained as the evidence, and the run states that its concurrency is host-reported rather than attested.

- **FAN-10 (Episode state is append-only and refuses impossible states):** transitions (`open` → `start`\* → `seal` → `return`\* → complete, or `abandon`) are **append-only with timestamps**, and a reader MUST **refuse** a state that no legal transition sequence could have produced — a return with no start, a seal with a missing handle, a duplicate handle across members, a terminal timestamp preceding its predecessor. Hand-editing an impossible state fails closed. A record that certifies whatever it is told certifies nothing.

- **FAN-11 (Check concurrency is not actor concurrency):** running several deterministic *checks* in parallel to save wall-clock is a distinct, unattested optimization and MUST NOT open, satisfy, or be reported as a launch episode. It creates no actors, changes no dependency readiness, and carries no handles. Conflating the two lets a run that merely verified quickly claim it worked widely.

> L2 specs cannot reach RFC status until all invariants here are addressed in their "Invariant Compliance" section.

## 4. Detailed Design

### 4.1 The episode lifecycle

```text
[REFERENCE]
fan_out(ready_set):
    members := partition(ready_set, host.concurrency_limit)[0]   // FAN-7 — the rest wait for later episodes
    for m in members: claim(m)                                   // WL-1 — ownership before launch
    ep := open(members)                                          // FAN-2 — exact set declared up front
    for m in members:
        handle := host.launch_nonblocking(m)                     // FAN-3 — the HOST returns it
        if handle is None:
            // FAN-4 — episode stays OPEN; never seal around the gap
            retry(m) or return abandon(ep, reason)               // terminal, non-successful
        record_start(ep, m, handle)
    seal(ep)                                                     // refused unless every member has a distinct handle
    // ---- barrier: only now may the coordinator block ----
    while ep.pending:
        m := host.wait_any(ep)                                   // FAN-2 — never before seal
        record_return(ep, m)                                     // FAN-6 — scheduler completion, not verification
        verify(m)                                                // CMP-3 — separate act, against an observable artifact
    report(ep, order = declared)                                 // FAN-7 — declared order, not completion order
```

The refusals are the design. `seal` refusing an incomplete member set and `record_return` refusing an unsealed episode are not validation niceties — together they are the only thing standing between a serial loop and a parallel-looking record.

### 4.2 Why the barrier catches what nothing else does

| Coordinator behaviour | Artifacts produced | Logs produced | Completion report | Episode record |
| --- | --- | --- | --- | --- |
| True fan-out | N results | N workers' output | "N ran in parallel" | sealed, N distinct handles |
| Launch → wait → launch → wait | N results | N workers' output | "N ran in parallel" | **cannot seal** |
| Sequential by design (declared) | N results | N workers' output | "N ran sequentially" | **no episode, declared fallback** |

Rows one and two are indistinguishable in every column but the last. That is precisely why the record has to exist: the failure has no other symptom until a deadline is missed.

### 4.3 What a sealed episode is worth

A sealed episode is a statement about **acceptance ordering**, and its value comes from being narrow enough to be true:

| Question | Answered by a sealed episode |
| --- | --- |
| Did the host accept a start for every member before any result was taken? | **Yes** — this is the claim |
| Did the members' execution overlap in wall-clock? | No — the office observes acceptances, not the scheduler |
| Did the members avoid writing each other's state? | No — confinement and disjoint ownership own that |
| Did any member do honest work? | No — verification owns that (CMP-3) |
| Did the members' results integrate? | No — the branch/fan-in gate owns that |

Stating the bound is part of the contract (FAN-5). An attestation that quietly implies the other four rows is worse than no attestation, because it retires the questions without answering them.

### 4.4 Abandonment as a first-class terminal outcome

A fan-out that cannot be completed has exactly two honest endings: retry into a sealed episode, or **abandon with a reason** (FAN-4). Abandonment is recorded on the episode, is terminal, and is **non-successful** — it never rolls up as a completed fan-out and never lets a parent report the work as done. The reason text is carried in the episode record for the final handoff; where an abandonment is surfaced into a privileged or automated channel, that channel receives the **identity** of the abandoned episode and its members, never the free-form reason, which is repository-derived text and is bounded and sanitized before it crosses that boundary.

### 4.5 Demarcation within the coordination family

| Concept | Decides | This spec adds |
| --- | --- | --- |
| l1-parallel-staffing | *what* is split (disjoint partitions) and that all are integrated | that the partitions were actually launched together |
| l1-competitive-execution | *what* rivals (whole attempts) and that exactly one is selected | that the rivals actually ran as rivals |
| l1-deliberation | *what* diverges (perspectives) and that they are synthesized | that the perspectives were actually independent in time |
| l1-orchestration | *who* does what, under what budget and isolation | the evidentiary record of the delegation's launch act |

The three fan-out modes are mutually exclusive in their fan-in; this contract composes with all of them and is required by each. It introduces no new fan-out mode and no new coordination decision.

## 5. Implementation Notes

1. **Episode store** — an append-only per-scope record of transitions with timestamps, a validating loader that refuses impossible states (FAN-10), and a reduction that reports an episode as open / sealed / complete / abandoned.
2. **Host adapters** — one adapter per execution host exposing a non-blocking launch that returns an identity, plus a capability probe so the FAN-8 fallback is chosen before the plan is written rather than discovered mid-run.
3. **Refusal points first** — implement the `seal` and `record_return` refusals before the happy path; they are the invariant, and an implementation whose refusals arrive later ships a window during which false attestations are recordable.
4. **Projection** — surface episodes in the office's live view (ORC-12/CPE-9), showing declared members, started members, and seal state, so an unsealed episode is visible while it is still recoverable.
5. **Cost & telemetry** — attribute per-member cost through the member identity (l1-outcome-attributed-cost); an episode that could not be opened for want of handles (FAN-9) marks its cost as host-reported.

## 6. Drawbacks & Alternatives

- **Ceremony on every fan-out.** Three transitions where one loop would do. Accepted: the cost is per-episode and constant, and the failure it closes is invisible by construction — exactly the class where after-the-fact detection does not exist. It is not required for a single delegated unit, only where a run *claims* width.
- **Alternative — infer concurrency from timestamps.** Rejected: overlapping start/end times are consistent with a fast serial run on a fast machine, and non-overlapping times are consistent with genuine fan-out under a busy scheduler. Inference here produces a confident number with no truth value; acceptance ordering is a fact the office can actually observe.
- **Alternative — trust the coordinator's report.** Rejected on the same ground as CMP-3: a self-reported property, produced by the actor whose behaviour it describes, is a claim to verify rather than a verdict to relay.
- **The attestation can be honest and the work still wrong.** By design (FAN-5). This layer is deliberately narrow; it is the launch-side sibling of verification, not a replacement for it.
- **A host that returns no per-member identity gets no attestation.** Accepted and disclosed (FAN-9) rather than substituted with a weaker proxy: the run states that its concurrency is host-reported. An attestation that degraded silently into a guess would defeat the concept on its first day.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[STAFFING]` | `.design/main/specifications/l1-parallel-staffing.md` | Throughput fan-out whose launch act this governs |
| `[COMPETE]` | `.design/main/specifications/l1-competitive-execution.md` | Best-of-N fan-out; bounded width partitioned into episodes |
| `[VERIFY]` | `.design/main/specifications/l1-completion-verification.md` | CMP-3 — a return is not a verification |
| `[LIVENESS]` | `.design/main/specifications/l1-work-liveness.md` | WL-1 claim before launch; WL-9 exact-once fan-out |
| `[RECEIPTS]` | `.design/main/specifications/l1-tool-receipts.md` | The honest-coverage shape (TR-8) this spec applies to launches |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.0 | 2026-08-25 | Core Team | Initial concept — concurrency as an attested claim rather than an assumed property. The **launch barrier**: an episode declares its exact member set, records a host-issued handle per member, and seals before the first wait (FAN-2), which the serial launch-wait-launch pattern structurally cannot satisfy; handles are host-issued and never coordinator-authored (FAN-3); a partial launch keeps the episode open and ends in retry or a reasoned, terminal, non-successful abandonment, never a seal around the gap (FAN-4); the attestation's claim is explicitly bounded to acceptance ordering and proves nothing about overlap, isolation, honesty, or integration (FAN-5); a return is scheduler completion, not verification (FAN-6); rolling episodes bounded by host concurrency with results reported in declared order (FAN-7); a declared sequential fallback where the host cannot fan out, never a parallel description of a serial run (FAN-8); native scheduling preferred over detached process farms that surrender cancellation, observability and cost attribution, with no episode opened where no per-member identity exists (FAN-9); append-only episode state that refuses impossible transition sequences (FAN-10); check-level parallelism explicitly excluded from attestation (FAN-11). Orthogonal to the coordination family — parallel-staffing, competitive-execution and deliberation each decide what is fanned out and how fan-in resolves; none governs the launch act, and all three can claim a concurrency they did not perform. Nodus realization: l1-nodus-language NL-24 realized-execution-mode attestation for `~PARALLEL`. Concept-only. |
| 1.0.1 | 2026-10-22 | Core Team | Citations updated for the requirement-ID prefix renames that make each prefix name one specification (RULES §1, 1.8.0, the person's decision of 2026-10-22): `CE-n` of `l1-competitive-execution` is now `CPE-n`. No requirement was added, removed or reworded. |
