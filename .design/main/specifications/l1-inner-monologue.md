# Inner Monologue

**Version:** 1.0.1
**Status:** Stable
**Layer:** concept

## Overview

The inner monologue is a background cognitive process that runs autonomously during each office heartbeat cycle. It is not a response to a user message or an external event; it is the agent's unstructured internal reasoning — a continuous stream of thought that shapes intentions, surfaces tasks, and updates the agent's internal model of the office's state.

The inner monologue is private by default: its content is not shown directly in the main Chat surface. It is logged in the Pulse sidebar tab, making it inspectable for debugging and oversight without cluttering the user-facing conversation.

## Related Specifications

- [l1-scheduler-model.md](l1-scheduler-model.md) — heartbeat mechanism that triggers the inner monologue cycle
- [l1-office-control.md](l1-office-control.md) — Heartbeat/Pulse subsystem is individually pauseable (§4.4)
- [l1-automation-pipeline.md](l1-automation-pipeline.md) — automation rules that the inner monologue can trigger
- [l1-navigation-model.md](l1-navigation-model.md) — Pulse sidebar tab where inner monologue logs surface (Tab 6)
- [l1-office-model.md](l1-office-model.md) — orchestrator role that hosts the inner monologue process

## 1. Motivation

Reactive agents only act when prompted. A sufficiently autonomous agent needs a mechanism for unsolicited reflection — to notice that a scheduled job is about to fire, that a kanban card has been blocked for too long, that a memory item needs consolidation, or that a proactive message to the user would be useful. The inner monologue provides this reflection channel without polluting the user-facing conversation with the agent's internal deliberation.

## 2. Constraints & Assumptions

- The inner monologue runs in the context of the office orchestrator; individual workers do not have independent inner monologue cycles.
- Heartbeat frequency is configurable (default: once per 15 minutes when the office is Active/Idle); during Paused or Hibernating state the heartbeat is suppressed.
- The inner monologue does NOT produce a user-visible response unless it decides to escalate (surface a message in Chat or trigger an automation).
- The inner monologue has access to the office's current state snapshot: kanban board, scheduler, memory, session log, active workers. It does not have real-time streaming access to ongoing task steps.
- Token budget for a single inner monologue cycle is bounded and declared before the cycle starts.

## 3. Core Invariants

- **IM-1 Non-interrupting**: the inner monologue MUST NOT interrupt an in-progress user conversation or a running task. It runs only during idle windows (Active/Idle state with no blocking foreground work).
- **IM-2 Log-before-act**: any action produced by the inner monologue (automation trigger, proactive message, memory write, task proposal) MUST be written to the inner monologue log BEFORE the action is dispatched. A log entry with no corresponding action is acceptable; an action with no prior log entry is a protocol violation.
- **IM-3 Bounded budget**: each cycle runs within a declared token budget. If the budget is exhausted, the cycle concludes with whatever conclusions were reached; it does not extend into the next heartbeat slot.
- **IM-4 No permanent state mutation from thought alone**: the inner monologue MAY propose memory writes, task proposals, and automation triggers, but these are dispatched through the standard subsystem interfaces — the monologue itself does not write directly to any store. Standard approval and safety gates apply.
- **IM-5 Pauseable**: when the Heartbeat/Pulse subsystem is individually paused (from Local Settings, per OC-subsystem table), the inner monologue cycle does not fire. The pause does not affect user-facing conversation or running tasks.

## 4. Detailed Design

### 4.1 Cycle Lifecycle

```text
[REFERENCE]
Trigger: heartbeat fires (configurable interval; default 15 min; Active/Idle only)

1. SNAPSHOT   — orchestrator assembles a read-only state snapshot:
                 current kanban board / scheduler queue / memory staleness signal /
                 recent session log excerpt / active worker list / time since last cycle
2. REFLECT    — orchestrator runs the inner monologue prompt against the snapshot
                 (token-bounded; IM-3)
3. DECIDE     — monologue output is parsed into zero or more typed intentions:
                 ProactiveMessage | AutomationTrigger | MemoryWriteProposal | TaskProposal | NoAction
4. LOG        — ALL intentions (including NoAction) are written to the Pulse log (IM-2)
5. DISPATCH   — non-NoAction intentions are dispatched through standard subsystem interfaces
                 (chat message API / automation pipeline API / memory curator API / kanban API)
```

Total wall-clock time per cycle is bounded by the token budget (IM-3) plus dispatch latency. Dispatch is asynchronous; the inner monologue cycle itself completes after LOG.

### 4.2 Intention Types

| Intention | Description | Subsystem | User visible? |
| --- | --- | --- | --- |
| `ProactiveMessage` | The orchestrator decides to surface a message in Chat | Chat | Yes |
| `AutomationTrigger` | An event pattern was matched; fire an automation pipeline | Automation | Configurable |
| `MemoryWriteProposal` | A fact worth remembering was noticed; proposes a memory entry | Memory curator | No (unless curated) |
| `TaskProposal` | A new kanban card should be created; proposes it to the orchestrator | Kanban | No (orchestrator reviews) |
| `NoAction` | Reflection concluded with no action needed | — | No |

### 4.3 Reflection Focus Areas

The inner monologue prompt is structured around recurring focus areas. Each heartbeat cycle covers all areas proportionally within the token budget:

1. **Work state** — are any kanban cards blocked, overdue, or anomalous? Should a card be escalated or deferred?
2. **Schedule health** — are upcoming cron jobs adequately prepared? Did any recent job produce unexpected results?
3. **Memory freshness** — are any memory items stale or inconsistent with recent observed facts? Should consolidation be proposed?
4. **Office health** — any warning signals from the doctor subsystem or model-router that warrant a proactive notification?
5. **User engagement** — has there been no user interaction for an unusually long time? Is there anything useful to surface?

The orchestrator weights each area based on recency (areas not addressed in recent cycles get higher weight).

### 4.4 Pulse Log Format

Each inner monologue cycle produces one log entry in the Pulse tab:

| Field | Content |
| --- | --- |
| `cycle_id` | Unique identifier |
| `started_at` | ISO-8601 timestamp |
| `duration_ms` | Total cycle duration |
| `tokens_used` | Tokens consumed this cycle |
| `focus_areas` | Which areas were covered, with weights |
| `intentions` | List of typed intentions (including NoAction) |
| `dispatched` | Per-intention: `{type, target, status, dispatched_at}` |
| `truncated` | `true` if token budget caused early conclusion |

### 4.5 Proactive Message Control

The orchestrator applies a proactivity threshold before dispatching a `ProactiveMessage`. The threshold is configurable in Local Settings → Office and considers:

- Estimated user value of the message (low → suppress)
- Time since last proactive message (too frequent → suppress)
- Office's current engagement level (user just active → suppress; long idle → lower threshold)

A suppressed proactive message is still logged in the Pulse log (IM-2), with `dispatched.status = suppressed`.

## 5. Implementation Notes

1. The inner monologue prompt is a harness-engineered nodus workflow step, not a hardcoded system prompt section. It evolves under the harness engineering loop.
2. IM-1 non-interrupting enforcement is a scheduler guard: heartbeat fires only when the foreground session is in `Waiting` or `Idle` turn state.
3. The Pulse log is backed by the same SQLite database as the inbox (`l2-inbox.md`), in a table of its own — inbox message rows are deleted on delivery and pruned by age, which a log the dispatcher must consult (IM-2) cannot tolerate.

## 6. Drawbacks & Alternatives

**Alternative: always-on streaming inner monologue** — the orchestrator runs a continuous background thought process rather than discrete heartbeat cycles. Rejected: continuous operation would consume disproportionate API tokens; the discrete cycle allows budget control (IM-3) and a clear audit boundary per cycle.

**Alternative: inner monologue fully hidden** — never expose logs to the user. Rejected: transparency is essential for user trust in an autonomous system. The Pulse tab provides opt-in visibility without intruding on the main conversation.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[HEARTBEAT]` | `.design/main/specifications/l1-scheduler-model.md` | Heartbeat trigger that fires the cycle |
| `[OFFICE-CTRL]` | `.design/main/specifications/l1-office-control.md` | Pause semantics for the Heartbeat/Pulse subsystem (IM-5) |
| `[AUTOMATION]` | `.design/main/specifications/l1-automation-pipeline.md` | AutomationTrigger dispatch target |
| `[INBOX]` | `.design/main/specifications/l2-inbox.md` | SQLite backing for Pulse log entries |
| `[NAV-MODEL]` | `.design/main/specifications/l1-navigation-model.md` | Pulse tab (Tab 6) where log is surfaced |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.1 | 2026-09-23 | Core Team | Consistency pass (2026-09-23): Implementation note placed the Pulse log in inbox message rows, which the inbox deletes on delivery and prunes by age — the log lives in the same database in a table of its own (IM-2). |
| 1.0.0 | 2026-06-24 | Core Team | Initial spec — IM-1…IM-5, cycle lifecycle, 5 intention types, Pulse log format, proactivity threshold, reflection focus areas |
