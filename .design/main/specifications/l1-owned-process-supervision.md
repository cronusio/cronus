# Owned Process Supervision

**Version:** 1.0.0
**Status:** Stable
**Layer:** concept

## Overview

The office starts other programs all the time: a foreign agent runtime, a tool server, a model runtime, a shell command an agent asked for, a build, the shipped binary under a test driver. Each of those starts a lifetime the office is now responsible for ending — on success, on timeout, on cancellation, on its own shutdown — and each ending is an act on the operating system's process table, which the office shares with everything else on the machine.

The corpus bounds what a started program may do (the execution sandbox), how an environment's lifetime ends (environment lifecycle), and who owns a unit of work (work liveness). It does not say how the office **proves a process is its own before signalling it**, what a termination request is and is not evidence of, or what is reported when cleanup does not finish. Those are the moments where a supervisor kills a stranger that inherited a recycled process number, reports "stopped" because a signal call returned, or loses the output of a run because tidying up after it failed.

This specification owns that: **ownership proven by identity, termination as a recorded request, and cleanup reported separately from the work**.

## Related Specifications

- [l1-execution-sandbox.md](l1-execution-sandbox.md) — EXS bounds what confined code may do and EXS-4 terminates a sandbox that exceeds a bound. This specification is how any termination, sandboxed or not, is carried out and reported; it is not a sandbox and confers no confinement (PSV-10).
- [l1-environment-lifecycle.md](l1-environment-lifecycle.md) — EL-5 requires every exit path of an environment to name its capture owner and EL-8 a declared reaper. A reaper that signals processes does it under PSV-1…PSV-6.
- [l1-work-liveness.md](l1-work-liveness.md) — WL-1 owns *work* by atomic claim; this owns the *processes* that carry the work. WL-8's watchdog classifies a silent run; what it then does to the process goes through this contract.
- [l1-foreign-agent-invocation.md](l1-foreign-agent-invocation.md) — FAI-4 takes the deadline from the operation; PSV-4 is what happens when that deadline elapses: a graceful interval, then one forced stop, each recorded.
- [l1-process-monitor.md](l1-process-monitor.md) — PMN-1 observes the application's process tree and controls nothing. PSV's ownership records are the same tree seen by the party that is allowed to act on it.
- [l1-fault-ownership.md](l1-fault-ownership.md) — OWN-1's *undetermined* is a first-class verdict; PSV-2's *unverified* is the same honesty applied to process identity.
- [l1-surface-driver.md](l1-surface-driver.md) — DRV-10 requires safe failure and a bounded life for a driven session; the terminal lane of `l2-surface-driver` realizes its process ending under this contract.
- [l1-observation-comparison.md](l1-observation-comparison.md) — an observation whose cleanup failed keeps its comparable dimensions (PSV-7, OBC-4).

## 1. Motivation

- **The recycled number.** A child exits. Its process number is reused by an unrelated program. The supervisor's timeout fires, and it signals the number it remembers.
- **The returned call.** A termination signal is sent and the call returns success. The run is recorded as stopped. The program had installed a handler, ignored the signal, and kept writing.
- **The final report that never got written.** A job writes its report on shutdown. The deadline sends an immediate forced stop; the report is half-written, and the next step reads it.
- **The lost run.** A capture completes, then removing its temporary directory fails. The whole operation is reported as an error and the observations, which were fine, are discarded.
- **The publish that looked failed.** An atomic write published its file, then the staging cleanup failed. The caller sees an error and assumes nothing was written; a retry overwrites a good result or a downstream step ignores it.
- **The descendant left behind.** The root process exits cleanly; a grandchild it spawned holds a port, and the next run cannot start.

## 2. Constraints & Assumptions

- **Process numbers are reused.** A number and an executable path identify a process only while it is alive and only until the number is recycled.
- **Checking identity and sending a signal are two separate operations.** Nothing makes them atomic; the window between them can be narrowed, not closed.
- **Some processes cannot be inspected.** The host may withhold identity information for processes the office did not start, or change it when a program rewrites its own title.
- **Sampling a process tree misses short-lived descendants.** A tree observed at intervals is a series of snapshots, not an inventory.
- **Cleanup can fail.** Files may be held open, directories may be unremovable, a process may not die.

## 3. Core Invariants

Rules every Layer 2 implementation MUST NOT violate. They are technology-neutral.

- **PSV-1 (Ownership is proven by an identity bound at start, never by a number or a path):** when the office starts a process it binds an **ownership identity** at that moment — the process's start identity as the operating system reports it, together with a token the office placed in the process's launch context, or the strongest equivalent the host offers — and checks that identity **before and after** reading it, so that a change during validation is detected. A process number, an executable path, or a name **alone never establishes ownership**, and an identity that changed is never adopted as a replacement.

- **PSV-2 (A process the office cannot prove it owns is never signalled):** before every termination signal, the retained identity is **re-checked**. Where the identity cannot be established or no longer matches, the process is **left untouched** and recorded as **unverified** with its number and the reason. There is no fallback to signalling a bare number. Ownership uncertainty is preserved and reported, never resolved by acting.

- **PSV-3 (A termination request is recorded with its delivery result, and no delivery result proves exit):** each signal attempt is recorded in order with the time it was sent and one of: **delivered** (identity matched and the call returned), **gone** (no live process matched), **identity changed** (the number belongs to another process, which was not signalled), **unverified** (identity unavailable, the call failed, or delivery did not settle within a bound). None of these is evidence that the process **exited**; exit is recorded only when it is observed, with the code or signal actually observed, never inferred.

- **PSV-4 (A deadline may grant a finalization interval; cancellation is never delayed by it):** a supervised run may declare a **finalization interval**: when its deadline elapses, it receives a graceful termination request, continues to be observed, and receives **at most one** forced stop if it has not exited when the interval ends. Without a declared interval the deadline stops it at once. A **cancellation** by the caller is not a deadline: it stops the process immediately, including during a finalization interval, and is recorded as cancelled. The recorded end reason keeps the **initiating** cause (deadline, idle deadline, cancellation), and an exit observed during finalization records its own code beside it, so "timed out, then exited cleanly" is distinguishable from "timed out and was killed".

- **PSV-5 (Descendants are the supervisor's responsibility, and settlement is reported as sampled):** the office supervises the started process **and its descendants** as one owned group where the host allows, and after the root ends it reports **settlement** — whether the observed group quiesced, needed cleanup, or could not be verified. Because sampling can miss short-lived or detached descendants, settlement is reported as **observed at the times sampled**, never as "no descendant existed". Unrelated processes whose identity the host withholds are recorded as such and **do not** count against a verified cleanup of the owned group; related or unexplained ones do.

- **PSV-6 (Cleanup failure is its own outcome and leaves ownership in place):** when cleanup does not complete, the result is **cleanup incomplete**, listing the **owned resource kinds that remain** (processes, temporary directories, mounts, locks). Resources whose cleanup failed **stay owned** — tracked, retried or reported — rather than being forgotten. Removing shared locations by pattern ("everything under the temp directory that looks like ours") is never a cleanup strategy; only exact owned paths are removed.

- **PSV-7 (Observations survive cleanup failure):** what a run observed before it ended — output, exit, files, events, partial results of a failed or cancelled run — is **retained and returned** independently of whether cleanup succeeded. A cleanup failure is reported **beside** the observation or the primary failure, never instead of it.

- **PSV-8 (A completed publication is reported as completed, even when its tidy-up failed):** where a result is published atomically (written to a staging location, then moved into place), and the move succeeded but cleaning the staging location did not, the outcome carries a **publication receipt** — published, where, how many bytes — alongside the cleanup failure. A caller can always tell a write that committed from one that never did.

- **PSV-9 (Cancellation is checked before publication, so a cancelled run never publishes a success):** long computations check for cancellation **before** they publish a result. A cancellation that arrives first produces *cancelled*, with any completed facts retained as partial; it never races with, and is never overtaken by, a success that is published afterwards. A caller that stopped waiting has not thereby established that the work stopped; whether it did is reported by the supervisor.

- **PSV-10 (Supervision is not confinement):** supervising a process — owning its lifetime, observing its files and output — **restricts nothing** it can do. Paths chosen for observation select what is recorded, not what the process may touch; the environment it inherits is not recorded merely because it was inherited, and its influence stays **unknown** unless declared. Any claim of confinement belongs to the execution sandbox, never to supervision.

> An L2 implementation cannot reach RFC until every invariant above is addressed in its Invariant Compliance section.

## 4. Detailed Design

### 4.1 Ending a supervised run

```
deadline elapsed ──► finalization interval declared?
     │ no  → re-check identity → forced stop (PSV-2, PSV-3)
     │ yes → re-check identity → graceful request; keep observing
     │        interval ends, not exited → re-check identity → one forced stop
cancel  ──► re-check identity → forced stop now (PSV-4), even mid-interval
any end ──► record: initiating reason, each attempt + delivery result,
            observed exit (or none), settlement of the owned group (PSV-5),
            cleanup outcome with remaining owned kinds (PSV-6),
            observations retained regardless (PSV-7)
```

The two signal deliveries in a finalization interval are independent: the forced stop does not wait for the graceful request's identity check to settle, because cancellation must never wait on a slow check.

### 4.2 Delivery results (PSV-3)

| Result | Meaning | Proves exit? |
| --- | --- | --- |
| delivered | identity matched; the signal call returned | no |
| gone | no live process matched the identity | no (it may have exited, or never matched) |
| identity changed | the number now belongs to another process; not signalled | no |
| unverified | identity unavailable, call failed, or delivery did not settle in bound | no |

### 4.3 Failure modes named

| Mode | Closed by |
| --- | --- |
| Recycled number signalled | PSV-1, PSV-2 |
| Returned call read as exit | PSV-3 |
| Report cut off by immediate kill | PSV-4 |
| Lost run on cleanup failure | PSV-7 |
| Publish read as failed | PSV-8 |
| Success published after cancel | PSV-9 |
| Descendant holds a resource | PSV-5, PSV-6 |
| Supervision mistaken for a sandbox | PSV-10 |

## 5. Implementation Notes

- Where the host offers a kernel-level grouping of a process and its descendants that ends them together, it is the preferred owned group; where it does not, the group is built from the bound identity and periodic sampling, and settlement says so.
- A host on which descendant cleanup cannot be verified for some kind of launch (for example, a pseudo-terminal adapter that cannot enumerate its children) reports that launch kind **unavailable** rather than running it with unverifiable cleanup.
- Bounded waits on identity checks (a delivery that does not settle within a stated bound is *unverified*) keep a stuck inspector from stalling cancellation; a late result after that bound is not added back to the record.

## 6. Drawbacks & Alternatives

- **Unverified processes are left running.** Accepted: the alternative is occasionally killing something that is not ours, which is not recoverable.
- **More outcomes than "ok" and "error".** Accepted: cleanup incomplete with a publication receipt is a different situation from a failed write, and callers act differently on each.
- **Alternative — kill by process number after a short grace:** rejected (PSV-1, PSV-2).
- **Alternative — treat cleanup failure as failure of the operation:** rejected (PSV-7, PSV-8); it discards good results to report a tidiness problem.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[SANDBOX]` | `.design/main/specifications/l1-execution-sandbox.md` | EXS — confinement, which supervision is not |
| `[ENVLIFE]` | `.design/main/specifications/l1-environment-lifecycle.md` | EL-5, EL-8 — exit paths and reapers |
| `[FOREIGN]` | `.design/main/specifications/l1-foreign-agent-invocation.md` | FAI-4 — deadlines from the operation |
| `[ENDPOINT]` | `.design/main/specifications/l2-engine-endpoint.md` | An existing record binding a process to its start time |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.0 | 2026-10-22 | Core Team | Initial concept — **the lifetime of processes the office starts**: ownership proven by identity, termination as a recorded request, cleanup reported apart from the work. Mined from an external reverse-engineering toolkit's process capture and provider supervision, which never signals a process it cannot prove it owns and records every termination attempt with its delivery result. Ten invariants: ownership bound at start by start identity plus a launch token, never by number or path (PSV-1); no signal without a re-checked identity, unverified left untouched (PSV-2); delivery results recorded, none proving exit (PSV-3); an optional finalization interval with one forced stop, cancellation never delayed, the initiating reason kept (PSV-4); descendants supervised, settlement reported as sampled (PSV-5); cleanup incomplete as its own outcome with remaining owned kinds, ownership retained, no pattern deletion (PSV-6); observations survive cleanup failure (PSV-7); a publication receipt when an atomic publish succeeded and tidy-up did not (PSV-8); cancellation checked before publication (PSV-9); supervision is not confinement, inherited environment not recorded and its influence unknown (PSV-10). Concept-only; no L2 yet. |
