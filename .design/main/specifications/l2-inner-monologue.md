# Inner Monologue

**Version:** 1.0.1
**Status:** Stable
**Layer:** implementation
**Implements:** l1-inner-monologue.md

## Overview

The concrete inner-monologue cycle in `crates/core`: a heartbeat-triggered background process that assembles a read-only office state snapshot, runs a token-bounded reflection pass, parses the output into typed intentions, logs every intention (including NoAction) to the Pulse log before dispatch, and routes non-NoAction intentions through standard subsystem interfaces. The Pulse log lives in the inbox SQLite database in a table of its own; the reflection prompt is a harness-engineered nodus step.

## Related Specifications

- [l1-inner-monologue.md](l1-inner-monologue.md) — the model this implements (IM-1…IM-5).
- [l2-scheduler.md](l2-scheduler.md) — the heartbeat that fires the cycle (Active/Idle only).
- [l2-inbox.md](l2-inbox.md) — the SQLite database the Pulse log lives in, in its own table (§4.4).
- [l2-office-control.md](l2-office-control.md) — Heartbeat/Pulse subsystem pause toggle (IM-5) suppresses the cycle.
- [l2-automation-pipeline.md](l2-automation-pipeline.md) — AutomationTrigger intention dispatch target.
- [l2-navigation.md](l2-navigation.md) — the Pulse sidebar tab surfacing the log.

## 1. Motivation

The model requires unsolicited, budget-bounded reflection that never interrupts foreground work and never mutates state directly. Gating the heartbeat on foreground turn state satisfies non-interruption; routing all intentions through existing subsystem APIs keeps the monologue a proposer, not a writer; reusing the inbox store avoids a parallel persistence layer.

## 2. Constraints & Assumptions

- The cycle runs in the orchestrator context; workers have no independent monologue.
- Heartbeat default is 15 min when Active/Idle; suppressed in Paused/Hibernating and when the Pulse subsystem is individually paused (IM-5).
- The snapshot is read-only (board/scheduler/memory-staleness/session-excerpt/worker-list); no real-time step streaming.
- Per-cycle token budget is declared before the cycle starts (IM-3).

## 3. Invariant Compliance (Layer 2)

| L1 Invariant | Implementation |
| --- | --- |
| IM-1 Non-interrupting | A scheduler guard fires the heartbeat only when the foreground session turn state is `Waiting`/`Idle` with no blocking work; an active turn defers the cycle. |
| IM-2 Log-before-act | The cycle writes ALL parsed intentions to the Pulse log first, then dispatches; the dispatcher refuses any intention lacking a prior log row. |
| IM-3 Bounded budget | `CycleBudget.max_tokens` caps the reflection pass; exhaustion concludes with partial intentions + `truncated=true`, never spilling into the next slot. |
| IM-4 No direct mutation | Intentions dispatch through chat/automation/memory-curator/kanban APIs; the monologue holds no store handles, so standard approval/safety gates apply to every effect. |
| IM-5 Pauseable | When the Heartbeat/Pulse `SubsystemPause` bit (office-control §4.4) is set, the scheduler does not fire the cycle; foreground conversation/tasks are unaffected. |

## 4. Detailed Design

### 4.1 Cycle

```text
[REFERENCE]
on heartbeat (Active/Idle, foreground Waiting/Idle):        // IM-1
  snapshot := assemble_readonly(board, scheduler, memory_staleness, session_excerpt, workers)
  reflect  := nodus_step(monologue_prompt, snapshot, budget) // IM-3 token-bounded
  intents  := parse(reflect) -> [ProactiveMessage|AutomationTrigger|MemoryWriteProposal|TaskProposal|NoAction]
  pulse_log.append_all(intents)                              // IM-2 before dispatch
  for i in intents where i != NoAction:
     dispatch(i) via standard subsystem API                  // IM-4
```

Dispatch is async; the cycle completes after LOG. The reflection prompt is a harness-engineered nodus workflow step, evolvable under the harness loop — not a hardcoded system-prompt section.

Because dispatch is asynchronous, the IM-1 check is repeated at dispatch time: a `ProactiveMessage` whose cycle started in an idle window is held if a foreground turn has begun since, and delivered after that turn ends — or dropped as stale and logged as such — so a cycle that began idle cannot interrupt a conversation that began afterwards.

### 4.2 Intention types

`ProactiveMessage` (→ Chat, user-visible, threshold-gated §4.5), `AutomationTrigger` (→ automation, configurable visibility), `MemoryWriteProposal` (→ memory curator), `TaskProposal` (→ kanban, orchestrator reviews), `NoAction` (logged, no dispatch).

### 4.3 Reflection focus areas

Work state / schedule health / memory freshness / office health / user engagement — weighted by recency (areas not recently covered get higher weight) within the token budget.

### 4.4 Pulse log

Stored in the inbox SQLite database, in its own table `pulse_log` — not as inbox message rows, which are deleted on delivery and pruned after `GC_TTL_MS` (`l2-inbox` §4.6): a log row the dispatcher must find before dispatching (IM-2) cannot live in a table built to delete delivered rows. One row per cycle: `{cycle_id, started_at, duration_ms, tokens_used, focus_areas, intentions[], dispatched[], truncated}`. A suppressed proactive message is logged with `dispatched.status = suppressed` (IM-2). Rows are rotated only by an explicit, stated age horizon shown in the Pulse tab — never by the inbox's delivery-driven deletes.

### 4.5 Proactivity threshold

Before dispatching a `ProactiveMessage`, a threshold (Local Settings → Office) weighs estimated user value, time since last proactive message, and current engagement; below threshold → suppress (still logged).

## 5. Implementation Notes

1. The monologue prompt is a nodus workflow step (harness-evolvable), not a hardcoded prompt.
2. IM-1 enforcement is a scheduler guard on foreground turn state.
3. The Pulse log reuses the inbox SQLite database in its own `pulse_log` table — no parallel store, and untouched by the inbox's drain and TTL garbage collection.

## 6. Drawbacks & Alternatives

**Alternative — always-on streaming monologue**: disproportionate token cost, no clean audit boundary. Rejected — discrete budget-bounded cycles.

**Alternative — fully hidden logs**: breaks transparency/trust. Rejected — the Pulse tab gives opt-in visibility without cluttering Chat.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[MODEL]` | `.design/main/specifications/l1-inner-monologue.md` | Invariants IM-1…IM-5 |
| `[SCHED]` | `.design/main/specifications/l2-scheduler.md` | Heartbeat trigger |
| `[INBOX]` | `.design/main/specifications/l2-inbox.md` | SQLite database hosting the Pulse log table |
| `[OFFICE-CTRL]` | `.design/main/specifications/l2-office-control.md` | Pulse subsystem pause (IM-5) |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.1 | 2026-09-23 | Core Team | Consistency pass (2026-09-23): The Pulse log, which the dispatcher must find before dispatching (IM-2), was stored as inbox message rows that the inbox deletes on delivery and prunes by age — it now has its own table with an explicit rotation horizon. IM-1 was checked only when the cycle started, so an asynchronous proactive message could interrupt a conversation begun during the cycle — re-checked at dispatch. |
| 1.0.0 | 2026-07-03 | Core Team | Initial implementation spec — heartbeat-gated cycle, read-only snapshot, token-bounded reflection, typed intentions, log-before-dispatch, Pulse log over the inbox store, proactivity threshold; maps IM-1…IM-5. |
