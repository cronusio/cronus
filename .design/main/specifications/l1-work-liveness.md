# Work Liveness & Ownership

**Version:** 1.3.1
**Status:** Stable
**Layer:** concept

## Overview

The contract that keeps autonomously-executed work *alive*: never silently abandoned, never executed twice, always recoverable. An always-on office runs work units (board cards) without a human steering them, so three failure modes become structural rather than incidental — a unit owned and "in progress" whose execution path quietly died and nothing will ever pick it back up; two executors both grabbing the same ready unit and doing the work twice; a process that is still technically alive but has stopped making progress, mistaken for healthy. This concept makes the opposite guarantees first-class: in-flight work is claimed *atomically by exactly one owner*; every owned, non-terminal unit *always* has an identifiable next-move drawn from a closed set of typed paths, or is surfaced as stalled; and loss of that path is repaired through a *bounded, owner-preserving* escalation rather than a silent drop or a silent reassignment.

It sits *beneath* delegation and *above* a single run: orchestration decides who does what; the execution graph runs one process to a checkpoint; this concept guarantees the *work item itself* survives crashes, contention, and silence in between.

## Related Specifications

- [l1-kanban-model.md](l1-kanban-model.md) - The board whose `running`/`blocked` states this concept gives ownership and liveness semantics to; KAN-5 (card = unit of work) is the unit this contract governs.
- [l1-orchestration.md](l1-orchestration.md) - Delegation protocol above this layer; ORC-4 (monitor/re-delegate), ORC-10 (resumable), and ORC-11 (retryable/fatal/escalation classification) are the delegation-level analogues of the work-item recovery ladder here.
- [l1-execution-graph.md](l1-execution-graph.md) - EG-6 checkpoint/resume restores a *run*; this concept covers the distinct case where the run record itself is gone and the owned unit is stranded.
- [l1-office-control.md](l1-office-control.md) - OC-2 (exact-state resume: no task dropped, restarted, or duplicated) is the freeze/resume special case this concept generalizes into a continuous guarantee.
- [l1-doctor.md](l1-doctor.md) - HEAL-1/HEAL-6 detect stuck/orphaned work and reconcile on restart; this concept supplies the *affirmative* contract doctor checks against and the typed recovery action it opens.
- [l1-operational-health.md](l1-operational-health.md) - OH-4 anomaly/idle and stuck-work signals *observe* silence (OH-7 measure-don't-act); this concept defines what the system *does* about it.
- [l1-scheduler-model.md](l1-scheduler-model.md) - SCH wakes/heartbeats drive execution; wake and run coalescing is owned here (WL-4), while the card-level de-duplication of a routine's fires that SCH §5 once deferred is SCH-9 (open-card policy).
- [l1-task-graph-model.md](l1-task-graph-model.md) - TG-7 additive/reversible decomposition; WL-9 makes its fan-out exact-once under retry.
- [l1-fanout-attestation.md](l1-fanout-attestation.md) - [ADDED v1.3.0] the launch-side record of the fan-out WL-9 keys: a member is claimed (WL-1) before it is launched, and the launch episode is what makes the resulting concurrency an attested fact rather than an assumed one.
- [l1-loop-governance.md](l1-loop-governance.md) - [ADDED v1.3.0] LG-6 bounded iteration is the loop-grain sibling of WL-11's bounded release; a guard that blocks an actor from stopping is itself a loop and takes the same ceiling.

## 1. Motivation

A human-run task board self-heals socially: if a ticket stalls, someone notices in standup. An autonomous office has no standup. When the office runs unattended for days, every gap in the execution model becomes a class of silent failure:

- **Stranded work.** A unit is assigned and `running`; the process that was advancing it crashed, was killed on restart, or its wake was lost. The board still says `running`. Nobody is coming. Without an explicit guarantee, the unit sits forever, looking healthy, doing nothing. Resuming a *checkpointed run* (the execution-graph's job) does not cover this — the run record is gone; the problem is at the level of *who owns this unit and what will move it next*.
- **Double-work.** Multiple wake paths (a timer fire, a fresh assignment, an on-demand ping) and multiple idle agents can all reach for the same ready unit at the same moment. Without an atomic claim, two runs start, burn budget twice, and may make conflicting changes. "Be careful" is not a guarantee; an exclusive lock is.
- **False liveness.** A run's process is still alive but has produced nothing for a long time — wedged on a prompt, stuck in a retry, deadlocked. Treating "process exists" as "work is progressing" hides the failure; treating silence as "process failed" and killing it throws away possibly-real work. Silence needs to be *classified*, not assumed either way.

The fix is not more checks bolted on after the fact — it is a positive contract the whole system is built to satisfy: owned non-terminal work is *never* without a next move, in-flight work has *exactly one* owner, and when liveness is lost it is restored by a *bounded* escalation that never silently drops the work and never silently hands it to someone else. The invariants below are exactly those guarantees, stated so an implementation cannot drift from them.

## 2. Constraints & Assumptions

- The unit of work is the board card (KAN-5); this concept governs its ownership and liveness, not its content or its place in the pipeline (owned by the kanban model).
- "Atomic claim" and "compare-and-clear" name *required effects* (exactly-one ownership, conflict-not-clobber), not a specific locking mechanism or storage column.
- This layer composes with, and never replaces, run-level resume (EG-6) and freeze/resume (OC-2). It addresses the cases those do not: lost run records, contention, and silence.
- Recovery is conservative by design: it preserves the existing owner and retries a bounded number of times before escalating. It is explicitly **not** an auto-reassignment system.
- Work may be agent-owned (heartbeat-executed) or human-owned; the liveness contract binds agent-owned work. Human-owned work is tracked but not driven by the execution loop.

## 3. Core Invariants

Rules every Layer 2 implementation MUST NOT violate:

- **WL-1 (Exclusive ownership of in-flight work):** a unit in an actively-executing state is owned by exactly one executor at a time, established by an **atomic claim**. Two executors can never both hold the active claim on the same unit; a second claim against a live owner resolves as an explicit ownership conflict, never a silent second execution. (No double-work.)
- **WL-2 (Claim bound to a live run, conflict-not-clobber release):** the active-execution claim is held only while its run is non-terminal. On any terminal outcome the claim is released by a compare-and-clear that clears it **only if it still points at that run** — never clearing a claim a successor run already re-acquired. The *unit owner* (who is responsible) is a distinct fact from the *live claim* (which run is executing right now); a handoff between runs MUST NOT leave the claim pinned to the dead run.
- **WL-3 (Liveness contract — no silently-dead work):** every owned, non-terminal unit MUST, at all times, have at least one identifiable **next-move path** from a closed, typed set: an active run · a queued wake/continuation · a deferred one-shot monitor · a pending human interaction or approval · a human owner · a live blocker-leaf (itself satisfying this contract) · an open recovery action. A unit with none of these is **stalled** and MUST be surfaced as stalled — never left appearing healthy. Free-text alone is not a next-move path.
- **WL-4 (Single active run, wake coalescing):** an executor has at most one active run at a time. Redundant wake requests for a unit/executor that already has a queued or running turn are **coalesced** — recorded (count, latest reason) rather than multiplied into concurrent duplicate runs. (This realizes run-level de-duplication; the card-level counterpart for a routine's fires is the scheduler model's open-card policy, SCH-9.)
- **WL-5 (Stranded-work reconciliation, owner-preserving):** loss of a live path is recovered distinctly from resuming a checkpointed run. A startup-and-periodic reconciliation **reaps** orphaned in-flight runs, **resumes** durably-queued work, and **reconciles** stranded ownership (owned + non-terminal + no live path). Recovery re-establishes a path **for the same owner**; it MUST NOT silently reassign the unit to a different executor.
- **WL-6 (Bounded recovery escalation):** when liveness is lost, recovery escalates through three bounded stages and never loops: **(1) auto-recover** — re-queue a single bounded continuation/dispatch for the same owner when ownership is clear and only execution continuity was lost; **(2) explicit recovery action** — when the system can name the problem but cannot safely complete it; **(3) human escalation** — when the next safe move needs human judgment, budget, or approval. Each stage's exhaustion advances to the next; auto-recovery's retry budget is finite.
- **WL-7 (Typed recovery action):** a recovery action is a structured, durable object naming the affected unit, the recovery owner (and prior/return owner when ownership shifts temporarily), the cause and bounded, redacted evidence, the next action, the wake/monitor/timeout/escalation policy that moves it forward, and a resolution outcome when closed. A comment or system notice may be *evidence* for a recovery action but is never itself the recovery path — it defines no owner, no policy, and no bound.
- **WL-8 (Liveness watchdog — silence is suspect, not proof):** a run whose process is still live but has stopped producing observable progress is **classified** by a watchdog (e.g. healthy / suspect / critical), never assumed either healthy or failed. A suspect/critical classification opens a recovery action — or blocks the unit when correctness requires it — **without unilaterally killing a possibly-productive process**. The watchdog decision (continue / snooze / dismiss / escalate) is explicit and recorded; the silence signal is observed via operational health, the action is taken here.
- **WL-9 (Idempotent fan-out):** decomposing one approved unit into child units is **exact-once**, keyed by the (source unit, approved revision) pair through a durable claim recorded *before* fan-out begins. A retry after partial fan-out continues from the recorded partial result under the same key and MUST NOT create a second set of siblings. Re-approving the same revision authorizes no new decomposition; a later, distinct revision is a new key.

- **WL-10 (Where intake is shared, a result belongs to an interval, never to a message):** [ADDED v1.2.0] when several inputs — queued follow-ups, injected context, steering, scheduled continuations — are drained by one executor into one run, that run's outputs MUST NOT be causally attributed to any individual input. A caller that needs to own a result **declares its interval explicitly** — from its own input's durable intake receipt through the next whole-executor idle — and reports whatever it selects as **interval-wide**, never as caused by its message. The guard cuts both ways, and the two failures are opposites. An automation that reads the executor's busy/idle transition as *its own* message's completion reports someone else's output as its result — plausible, unfalsifiable, and wrong. An automation that waits for a transition which can never occur — because its input was coalesced, cancelled, or discarded before starting — waits forever on a run that will never begin. Every such wait therefore handles the **nothing-to-wait-for** branch explicitly: the absence of a pending transition is an answer, not a reason to keep waiting. WL-4's wake coalescing is what makes shared intake correct; this invariant is what keeps the resulting attribution honest.

- **WL-11 (A guard that blocks completion measures semantic progress and carries a bounded release):** [ADDED v1.3.0] where a mechanism **blocks an actor from stopping** until obligations are met — an unmet gate, an incomplete fan-out, an open recovery action — four rules bind it, and each closes a way the guard turns a stall into a livelock. **(a) Progress is read from resolved obligation state, never from artifact bytes.** Whether the actor made progress is computed from the *state of the obligations themselves* — an obligation met, a required outcome newly unmet, a launch episode sealed — never from whether the files recording them changed. A byte-difference guard counts a reformatted line, an added comment, or the recorder rewriting its own metadata as progress, so it re-arms indefinitely and its release can only ever fire for an actor doing *literally nothing* — the one case least in need of a release. **(b) The release is bounded and diagnostic.** After a declared finite number of consecutive no-progress blocks the guard **releases**, stating why, because an actor that is trying and failing is exactly who an unbounded guard traps hardest. **(c) The budget is per-actor.** One actor's no-progress blocks are never consumed from another's; concurrent evaluations of the guard serialize their own state, and an obligation that disappears clears the state it owned. **(d) The guard observes; it never executes.** A guard evaluated at stop time reads recorded state and MUST NOT run the checks it blocks on — making the act of stopping produce effects inverts the relationship between an actor and the mechanism supervising it, and turns every abandoned attempt into a side effect. A blocked stop still consumes real work each time it fires, so every block states an actionable next move rather than repeating an obligation count.

> L2 specs cannot reach RFC status until all invariants here are addressed in their "Invariant Compliance" section.

## 4. Detailed Design

### 4.1 Four separable concerns

The model keeps apart four things that are easy to conflate; this spec owns the last two.

| Concern | Question | Owned by |
| --- | --- | --- |
| Structure | why does this unit exist (parent/child)? | task-graph (TG-2), kanban |
| Dependency | what must finish before it can proceed (blockers)? | task-graph (TG-4) |
| **Ownership** | who is responsible *and* who holds the live execution claim? | **this spec (WL-1/WL-2)** |
| **Execution liveness** | what moves this forward *next*, and is it actually moving? | **this spec (WL-3/WL-8)** |

A parent/child link is not a dependency, and a dependency is not an execution path: a unit can be structurally a child, depend on nothing, and still be stalled because its execution path died. Conflating them is exactly how silent-dead work hides.

### 4.2 The liveness contract

```text
[REFERENCE]
healthy(unit) :=
  terminal(unit)                                  // done / cancelled — no path needed
  OR human_owned(unit)                            // tracked, not heartbeat-driven
  OR exists next_move(unit) in {
        active_run, queued_wake, deferred_monitor,
        pending_interaction_or_approval,
        live_blocker_leaf, open_recovery_action
     }

stalled(unit) := owned(unit) AND not terminal(unit) AND no next_move(unit)
```

`stalled` is never a resting state: detecting it (doctor HEAL-1, or the reconciliation sweep) triggers the recovery ladder (WL-6). The set of next-move primitives is **closed** — an implementation may not invent a seventh that is really "a comment we hope someone reads."

### 4.3 Atomic claim lifecycle

```text
[REFERENCE]
claim(unit, run):
    if active_claim(unit) points at a LIVE run by another owner:
        return CONFLICT            // WL-1: real owner exists — caller stops, does not retry-spin
    if active_claim(unit) points at a TERMINAL or MISSING run:
        self-heal: clear it        // stale lock from a dead run
    set active_claim(unit) := run  // atomic

release(unit, run):                // on run terminal (succeeded/failed/cancelled/timed_out)
    compare-and-clear active_claim(unit) ONLY IF it still == run   // WL-2
    // never clear a claim a successor run already re-acquired

handoff(old_run -> retry_run):
    set active_claim := retry_run BEFORE old_run finalizes
    // WL-2: must not leave the claim pinned to the failed run
```

A `CONFLICT` from `claim` means a *real live owner* (or an unresolved blocker/gate) — the caller treats it as terminal-for-this-attempt and stops, rather than retry-spinning. Stale-lock self-healing is crash recovery, not a retry loop: claims held by *non-terminal* runs are never cleared or adopted.

**Concrete realization — negated compare-and-set + cooperative cancel** [ADDED v1.1.0]. The atomic claim (WL-1) is realized as a single **compare-and-set that succeeds only if the current state is *not* already the active state** — the transition to `RUNNING` is admitted iff the unit is currently not `RUNNING`. This one atomic operation subsumes both "is anyone running this?" and "take it" without a separate read, closing the check-then-act race two wake paths would otherwise hit (WL-1/WL-4). External **preemption of a healthy run** — distinct from the crash/silence cases WL-5/WL-8 cover — is a first-class next step: a control signal (`STOP`/`PAUSE`/`RESUME`) delivered on a per-unit channel is observed by a listener paired to the run and **cooperatively cancels** it; the producing execution and the consuming stream are decoupled (the run drains through a buffer, so cancellation never corrupts a half-emitted result). On exit the claim writes a terminal state (`STOPPED` on preemption, `FINISHED` on completion, `ERROR` on fault) carried with a **short time-to-live**, so a crashed executor's claim self-expires into the reap path (§4.4 step 1) rather than pinning the unit forever. Preemption is the OC pause/resume special case (`l1-office-control`) at the work-item grain; the resulting `STOPPED` unit re-enters the liveness contract with a queued continuation, not a silent drop.

### 4.4 Reconciliation sweep

On startup and on a periodic loop, in order:

```text
[REFERENCE]
1. reap     orphaned in-flight runs        (process gone, claim points at it)  -> mark terminal, release claim
2. resume   durably-queued runs            (queue survived restart)            -> re-enter execution
3. reconcile stranded ownership            (owned + non-terminal + no path)    -> recovery ladder (4.5)
4. watchdog  silent-but-live runs          (alive, no progress)                -> classify (WL-8)
```

Step 1 (reap) and step 2 (resume) restore *run* continuity — the EG-6 / OC-2 territory. Step 3 (reconcile) is the new guarantee: it closes the gap where unit state survived the crash but the run/wake path did not. Step 4 (watchdog) covers the orthogonal case where a process is alive but wedged.

### 4.5 Recovery escalation ladder

```text
[REFERENCE]
recover(unit):
    if ownership clear AND only execution continuity lost AND auto_budget > 0:
        auto_budget -= 1
        requeue one bounded continuation/dispatch for SAME owner     // WL-6 stage 1
    elif system can name a bounded owner/action:
        open TypedRecoveryAction(unit)                               // WL-6 stage 2 / WL-7
    else:
        escalate_to_human(visible trail)                            // WL-6 stage 3

TypedRecoveryAction {                                                // WL-7
  unit, recovery_owner, prior_owner?,
  cause, evidence (bounded, redacted),
  next_action,
  policy: { wake | monitor | timeout | max_attempts | escalation },
  resolution?: restored | delegated | false_positive | blocked | escalated | cancelled
}
```

The ladder maps onto orchestration's ORC-11 error classes (retryable → auto-recover, fatal-isolated → explicit recovery action, escalation → human) but operates at the **work-item** grain, where the failure is *loss of liveness* rather than a returned error. Recovery preserves the owner (WL-5): a cheap status-only recovery turn may clear bad state, but real work resumes only on the normal-model owner run.

### 4.6 Idempotent fan-out

```text
[REFERENCE]
decompose(source, approved_revision):
    key := (source.id, approved_revision.id)
    claim := durable_claim(key)                      // recorded BEFORE creating any child
    if claim.completed:   return claim.children       // reuse, create nothing (WL-9)
    if claim.in_flight:   resume from claim.partial   // continue same key, keep created ids
    children := create_children(...); persist incrementally under key
    mark claim.completed(children)
```

This makes TG-7 decomposition safe under the crash/retry reality WL-5 assumes: a run that creates two of five children and dies is continued, not restarted into ten.

## 5. Drawbacks & Alternatives

- **Coordination overhead.** Atomic claims, a reconciliation sweep, and typed recovery actions are more machinery than "just run the agent." Justified: every piece exists to prevent a *silent* failure (double-spend, stranded work, false liveness), and silent failures in an unattended office are the expensive kind. The simple cases stay simple — a healthy unit with an active run needs none of the recovery path.
- **Conservatism can feel slow.** Owner-preserving, retry-once-then-escalate recovery will sometimes surface a recovery action where an aggressive system would have silently reassigned and "just fixed it." That is the intended trade: visibility and no double-work over hidden autonomy. Auto-reassignment is deliberately out of scope (it needs judgment the control plane lacks).
- **Watchdog tuning.** Output-silence thresholds produce false positives until tuned (WL-8); mitigated by classification (suspect ≠ failed), explicit snooze/continue decisions, and never killing a live process on silence alone.
- **Alternative — lean entirely on run-level checkpoint/resume (EG-6).** Rejected: checkpoint/resume restores a run whose record survived; it cannot recover a unit whose run record is gone, cannot prevent two runs from claiming one unit, and cannot tell a wedged-but-alive process from a healthy one. Those are this concept's reason to exist.
- **Alternative — fold into the doctor (l1-doctor).** Rejected: doctor is generic check→repair→escalate over many fault kinds; the *affirmative* liveness contract, atomic-claim semantics, and exact-once fan-out are a specific, load-bearing contract that deserves its own concept. Doctor *checks against* it and *opens* its recovery actions; it does not define it.

## nodus-relevance mapping

The portable workflow runtime executes steps that can crash, contend, and wedge exactly as office work units do; the same contract applies at the step grain.

| Liveness element | nodus seam | Note |
| --- | --- | --- |
| Atomic claim (WL-1/WL-2) | `StorageProvider` step-state with a single-owner claim on the running step | Compare-and-clear on step terminal; a second worker gets a conflict, not a re-run. |
| Liveness contract (WL-3) | runner's step status + `~UNTIL`/monitor edges | A non-terminal step always has an active turn, a queued continuation, or a recovery edge. |
| Single active run + coalescing (WL-4) | runner's per-workflow execution lock | The de-dup the scheduler model deferred, realized at the step executor. |
| Reconciliation sweep (WL-5) | `AuditProvider` replay + durable queue resume on runtime restart | Reap/resume/reconcile keyed by `run_id` + `step_index`. |
| Recovery ladder + typed action (WL-6/WL-7) | `PolicyProvider` gate + step-error codes (`NODUS:*`) | Auto-retry budget → typed recovery state → HITL dialog. |
| Watchdog (WL-8) | step heartbeat/progress event on the audit stream | Output-silence classification over the step event timeline. |
| Idempotent fan-out (WL-9) | spawn/map step keyed by `(source_step, revision)` | Map-reduce fan-out continues from the durable claim, never double-spawns. |

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[KANBAN]` | `.design/main/specifications/l1-kanban-model.md` | The board/unit whose `running`/`blocked` states this contract governs. |
| `[ORCH]` | `.design/main/specifications/l1-orchestration.md` | Delegation above this layer; ORC-11 error classification the recovery ladder mirrors at the work-item grain. |
| `[EXEC-GRAPH]` | `.design/main/specifications/l1-execution-graph.md` | Run-level checkpoint/resume (EG-6) this concept layers above. |
| `[DOCTOR]` | `.design/main/specifications/l1-doctor.md` | Self-healing that checks against this contract and opens its recovery actions. |
| `[OPS-HEALTH]` | `.design/main/specifications/l1-operational-health.md` | Observes silence/stuck/idle signals (OH-4/OH-7) the watchdog acts on. |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.3.1 | 2026-10-03 | Core Team | Patch — WL-4's closing parenthetical claimed to realize "the de-duplication the scheduler model deferred", but WL-4 coalesces *wake requests and runs*; the deferred item was the *card* a routine's repeated fire adds, which no run policy touches. The sentence now says what WL-4 realizes (run-level de-duplication) and names the card-level rule (`l1-scheduler-model` SCH-9, added in 1.2.0) so a reader no longer concludes the flooding question was settled here. The Related Specifications line for the scheduler model made the same claim ("the de-duplication SCH §5 deferred is owned here") and is corrected the same way. No invariant changed. |
| 1.3.0 | 2026-08-25 | Core Team | WL-11 added — **a guard that blocks completion measures semantic progress and carries a bounded release**. WL-8 classified a live-but-silent run; what it did not cover is the inverse mechanism — a guard that refuses to let an actor *stop* while obligations remain — which has its own failure mode and reaches it reliably. Four rules: **progress is read from resolved obligation state, never artifact bytes** (a byte-diff guard counts a reflowed line, an added comment, or the recorder rewriting its own metadata as progress, so it re-arms forever and its release can only fire for an actor doing literally nothing — the one case least in need of it); **the release is bounded and diagnostic** after a declared number of consecutive no-progress blocks, since an actor that is trying and failing is who an unbounded guard traps hardest; **the budget is per-actor**, with serialized state updates and clearance when the obligation disappears, so one actor never consumes another's; and **the guard observes, never executes** — a stop-time guard that ran the checks it blocks on would make stopping produce effects and turn every abandoned attempt into a side effect. Each block states an actionable next move, because a blocked stop consumes real work every time it fires. Related Specifications extended with `l1-fanout-attestation` (the launch-side record of the WL-9 fan-out) and `l1-loop-governance` (LG-6 bounded iteration, the loop-grain sibling of the bounded release). Additive; no existing invariant weakened. |
| 1.2.0 | 2026-08-19 | Core Team | WL-10 added — **where intake is shared, a result belongs to an interval, never to a message**. WL-4 already coalesces wakes and holds one active run per executor; what it did not say is what a caller may claim about *its own* input once several inputs drain into one run. Causal attribution is unavailable there, so a caller that must own a result declares its interval explicitly — from its input's durable intake receipt to the next whole-executor idle — and reports what it selects as **interval-wide**. The guard cuts both ways and the two failures are opposites: reading a busy/idle transition as one's own completion reports someone else's output as one's result (plausible, unfalsifiable, wrong), while waiting for a transition that can never occur — the input was coalesced, cancelled, or discarded before starting — waits forever. Every such wait therefore handles the **nothing-to-wait-for** branch explicitly: the absence of a pending transition is an answer. Composes `l1-change-attribution` and `l1-outcome-attributed-cost` (both of which assume an attributable unit; this names when one does not exist). Distilled from an adoption pass over an external plugin-framework-based agent-harness reference. Additive; no existing invariant weakened. |
| 1.1.0 | 2026-07-01 | Core Team | §4.3: added concrete WL-1 realization — negated compare-and-set (transition to active state iff not already active, race-free single op) — and cooperative external preemption of a healthy run (STOP/PAUSE/RESUME on a per-unit channel, producer/consumer decoupled via buffer) with terminal-state-plus-short-TTL so a crashed executor's claim self-expires into the reap path; preemption framed as the OC pause/resume special case at the work-item grain. |
| 1.0.0 | 2026-06-26 | Core Team | Initial spec — autonomous work liveness & ownership: exclusive atomic claim with conflict-not-clobber release (WL-1/WL-2), affirmative liveness contract over a closed next-move set (WL-3), single active run + wake coalescing (WL-4), owner-preserving stranded-work reconciliation distinct from run-resume (WL-5), bounded auto→explicit→human recovery ladder (WL-6) with typed recovery action (WL-7), silence-is-suspect watchdog (WL-8), idempotent exact-once fan-out (WL-9); separates structure/dependency/ownership/liveness; composes with kanban, orchestration, execution-graph, doctor, operational-health; nodus-relevance mapping. |
