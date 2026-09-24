# Inbox (Inter-Actor Messaging)

**Version:** 1.0.2
**Status:** Stable
**Layer:** implementation
**Implements:** l1-office-model.md, l1-orchestration.md

## Overview

The inbox is a lightweight SQLite-backed messaging channel between actors within a workspace. When one actor (a background workflow, a subagent, or the system) wants to notify another actor, it writes a row to the inbox table. The receiver drains pending rows at the start of its next turn, converting them into synthetic user messages that the model sees as additional context. This decouples senders from receivers: a sender writes and returns immediately; the receiver processes at its own pace.

## Related Specifications

- [l1-office-model.md](l1-office-model.md) - Inter-agent communication model.
- [l1-orchestration.md](l1-orchestration.md) - ORC-8 synchronization; briefings and hand-offs between agents.
- [l2-agent-session.md](l2-agent-session.md) - Session turn loop that drains the inbox at the start of each iteration.
- [l2-workflow-runtime.md](l2-workflow-runtime.md) - Workflow completion sends an inbox notification to the parent actor.
- [l1-storage-model.md](l1-storage-model.md) - Inbox rows live in the workspace state-tier SQLite database.
- [l2-tool-security.md](l2-tool-security.md) - §4.6: a delegated agent's result is untrusted data — how a drained row reaches the model.

## 1. Motivation

Agents run as isolated sessions; they cannot share mutable state directly. Yet background workflows need to notify the orchestrator when they complete, and subagents need to return results to a parent. A persisted inbox decouples the notification lifecycle from the receiver's turn schedule: the row survives a receiver that is sleeping, busy, or restarting.

## 2. Constraints & Assumptions

- Inbox rows are workspace-scoped (stored in the workspace SQLite database, not global).
- Delivery is best-effort: if the receiver session is gone, the row is GC'd after the TTL.
- Drain order is ULID ascending (arrival order, monotone time).
- Drain is non-transactional between write-to-session and delete-from-inbox; duplicate delivery on crash is tolerable.
- The inbox is not a guaranteed-delivery queue: rows older than `GC_TTL_MS` are pruned regardless of delivery status.

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| ORC-8 Synchronization | Agents receive briefings and completion notifications via inbox drain (synthetic user message). |
| ORC-12 Transparent, intervenable coordination | The inbox table is the agent mailbox and the orchestrator is the hub for cross-role coordination; every `send` publishes `InboxArrived` on the event bus (§4.7), so no inter-agent message travels off the record. |
| OFF-1 Office-per-project isolation | Rows are workspace-scoped and a receiver must be a registered actor of the same workspace (§4.3 step 1); no office delivers into another. |
| STO-2 Durable, restartable state | Inbox rows persist in the workspace state-tier database and survive a receiver that is sleeping, busy or restarting; they are deleted after delivery or at the §4.6 TTL. |

## 4. Detailed Design

### 4.1 Table schema

```text
[REFERENCE]
InboxRow {
  id:                  String,        // ULID — sortable by arrival time
  receiver_session_id: String,
  receiver_actor_id:   String,
  sender_session_id:   Option<String>,
  sender_actor_id:     Option<String>,
  type:                String,        // "text" | "actor_notification" | ...
  content:             Json,          // { text: String }
  created_at:          i64,           // unix milliseconds
}

Index: (receiver_session_id, receiver_actor_id) for O(1) drain check.
```

### 4.2 Constants

```text
[REFERENCE]
GC_TTL_MS        = 7 * 24 * 60 * 60 * 1000   // 7 days
MAX_DRAIN_PER_TURN = 100
```

### 4.3 send()

```text
[REFERENCE]
send(input: SendInput) -> Result<SendResult, InboxReceiverNotFound>

SendInput {
  receiver_session_id: String,
  receiver_actor_id:   String,
  sender_session_id:   Option<String>,
  sender_actor_id:     Option<String>,
  content:             String,
  type:                Option<String>,  // defaults to "text"
}

Steps:
  1. ESRCH check: receiver actor must exist in ActorRegistry; fail with
     InboxReceiverNotFound if not (prevents orphan rows for already-finished actors).
  2. Insert InboxRow with ULID id and current unix-millisecond timestamp.
  3. Publish InboxArrived bus event: { receiver_session_id, receiver_actor_id,
     sender_session_id?, sender_actor_id?, inbox_id, type }.
  4. Fork-and-forget wake: call sessionPromptRef.loop({ session_id, agent_id })
     so the receiver's run loop is woken if it is sleeping between turns.
     If sessionPromptRef is unavailable (test fixtures, renderer-only paths):
     log a warning; the row is durable and will be drained on the next iteration.
  5. Return { inbox_id }.
```

The sender does not wait for the receiver to process the message (step 4 is fire-and-forget).

### 4.4 drain()

```text
[REFERENCE]
drain(session_id: String, actor_id: String) -> usize

Steps:
  1. SELECT rows WHERE receiver_session_id=session_id AND receiver_actor_id=actor_id
     ORDER BY id LIMIT MAX_DRAIN_PER_TURN.
  2. If empty: return 0 (fast path — common case for idle actors).
  3. Find the last real (non-system) message in the actor's message slice:
       real_msg = slice.find_last(m => m.role in ["user","assistant"]
                                       AND m.model.provider_id != "system"
                                       AND m.agent != "system")
  4. If no real message found: defer — return 0; rows stay for the next iteration.
     (No prior turn exists yet to anchor the synthetic message's agent+model metadata.)
  5. Create a synthetic user message (MessageID.ascending()) inheriting agent and model
     from real_msg.info.
  6. Append one text part per drained row, rendered by renderInboxRow(row).
  7. DELETE rows WHERE id IN (drained row IDs).
  8. Return count.
```

The synthetic message uses `role: "user"`, which preserves the KV-cache-stability invariant from agent-session §4.6 (dynamic context injected as user messages, not by modifying the system prompt). The role is a transport position, not an attribution: `renderInboxRow` labels every part with its sender, and a part from any sender other than the human principal — a subagent's result, a workflow's completion, a system notice carrying tool output — is wrapped as untrusted data (`agent:{actor_id}`, `l2-tool-security` §4.6). An inbox delivery must never read to the model as its principal's own words: a worker that could phrase a request in the user's voice would hold the user's authority over the receiver.

### 4.5 Crash window (known limitation)

Steps 5–6 (write to session) and step 7 (delete from inbox) are not executed in a single transaction. A crash between them causes the same rows to be drained again on the next iteration, resulting in duplicate notifications. This is tolerable: agent turns are idempotent at the application level (a double-notification prompts a status check, not a double-action). Fixing the window would require threading a single transaction across the session and inbox abstraction layers, which is out of scope.

### 4.6 GC

At service initialization, delete all rows where `created_at <= now - GC_TTL_MS`. This is idempotent. No separate GC fiber is started; init-time GC is sufficient given the 7-day TTL.

### 4.7 Event bus

```text
[REFERENCE]
InboxArrived {
  receiver_session_id: String,
  receiver_actor_id:   String,
  sender_session_id:   Option<String>,
  sender_actor_id:     Option<String>,
  inbox_id:            String,
  type:                String,
}
```

Subscribers (e.g., TUI, dashboard) can react to incoming messages for real-time display without polling.

### 4.8 Command surface

The inbox is an implementation detail; it has no direct user-facing commands. Observability is through the session message history (drained rows appear as synthetic user messages) and the audit log.

## 5. Drawbacks & Alternatives

- **Non-transactional drain:** duplicate delivery on crash is acceptable; a transactional approach would require a unified transaction across session + inbox layers.
- **ULID ordering:** monotone time ordering gives arrival-order drain without a separate sequence column.
- **Alternative — in-memory channels:** rejected; durable rows survive receiver restarts. An in-memory channel loses messages on crash.
- **Alternative — separate notification queue (Redis, NATS):** rejected; all-on-SQLite is the storage contract for Cronus (on-device, no external services).

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[OM]` | `.design/main/specifications/l1-office-model.md` | Inter-agent communication model |
| `[ORC]` | `.design/main/specifications/l1-orchestration.md` | ORC-8 synchronization invariant |
| `[SESSION]` | `.design/main/specifications/l2-agent-session.md` | Turn loop that calls drain() |
| `[STORAGE]` | `.design/main/specifications/l1-storage-model.md` | Two-tier storage layout |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.2 | 2026-09-23 | Core Team | Consistency pass (2026-09-23): A drained row reached the model as a synthetic user message "indistinguishable from a real user turn" — a subagent or workflow could speak with the principal's authority; parts are now labeled by sender and non-human senders are wrapped as untrusted data (`l2-tool-security` §4.6), the user role being transport only. Compliance cited nonexistent `OM-3`/`STORE-2` — now ORC-12, OFF-1, STO-2. |
| 1.0.1 | 2026-07-10 | Core Team | Fixed broken Related Specifications link — the storage tier is an L1 concept (l1-storage-model.md); the l2- prefix pointed at a non-existent file (Canonical References already used the correct l1- path) |
| 1.0.0 | 2026-06-24 | Core Team | Initial spec — SQLite-backed inter-actor inbox, ULID ordering, drain-on-turn, synthetic user messages |
