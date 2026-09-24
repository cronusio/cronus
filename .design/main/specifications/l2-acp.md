# Agent Client Protocol (ACP)

**Version:** 1.0.1
**Status:** Stable
**Layer:** implementation
**Implements:** l1-acp.md

## Overview

The concrete ACP server in `crates/core`: session lifecycle store, the ordered streaming event bus, capability declaration, trust-level enforcement, the pure protocol-projection adapters, cross-office relay, and the live-steering queue. The Streamable HTTP transport (`POST /acp`, Server-Sent Events) is owned by [l2-agent-session.md](l2-agent-session.md); this spec owns the protocol semantics beneath that transport — session state, capability registry, trust gating, relay routing, and the steering/interrupt control verbs.

## Related Specifications

- [l1-acp.md](l1-acp.md) — the protocol model this implements (ACP-1…ACP-10).
- [l2-agent-session.md](l2-agent-session.md) — hosts the `/acp` Streamable HTTP endpoint and the turn loop this server drives.
- [l2-security.md](l2-security.md) — authentication identity that resolves a caller's trust level (§4.4).
- [l2-orchestration.md](l2-orchestration.md) — the internal protocol ACP sessions invoke inside the office boundary.
- [l1-work-liveness.md](l1-work-liveness.md) — WL-4 wake-coalescing realized by the steering queue (ACP-10).
- [l2-inbox.md](l2-inbox.md) — same-session message serialization backing the steering queue.

## 1. Motivation

The model fixes the external contract; this spec makes it concrete without duplicating the transport already built in the agent-session daemon. Keeping session lifecycle, capability declaration, and trust enforcement in one core service means every projection (peer-agent, completion, UI-event) and every transport observes the same ordered stream and the same authority gate.

## 2. Constraints & Assumptions

- Sessions are core state keyed by `session_id`; the transport is stateless and re-attaches to a session by id.
- Every streamed event carries a per-session monotonic `seq`; the bus assigns it in emission order (ACP-8).
- Trust level is resolved once per session at authentication and cached; adapters cannot widen it. A revoked credential ends the sessions it opened rather than leaving them on the cached level.
- The steering queue is bounded (`STEER_QUEUE_MAX`, default 16); overflow rejects visibly.

## 3. Invariant Compliance (Layer 2)

| L1 Invariant | Implementation |
| --- | --- |
| ACP-1 Session-oriented | `SessionStore` (SQLite) keyed by `session_id`, holding conversation history ref, in-progress task state, and remaining budget. Every request resolves a session or is rejected. |
| ACP-2 Capability declaration | `capabilities(session)` builds a `Capabilities` doc (§4.3) from the office's active roles, tool categories, budget, and the caller's trust level. Served before any task interaction. |
| ACP-3 Streaming event protocol | Turns emit typed events (`thinking`/`tool_call`/`text_delta`/…) onto the session bus; the stream always terminates with `done`/`error`/`budget_exhausted`/`interrupted`. |
| ACP-4 Tool delegation boundary | `client_tool_request` is emitted only when the session's `tool_delegation_opt_in` flag is set at creation; otherwise server-side tools run locally. |
| ACP-5 Idempotent session creation | `create_session` upserts on a unique `session_id` index; a racing duplicate returns the winner's state to both callers. Idempotency is scoped to the creating principal and office: a create naming an existing id returns that session only to its owner, and is a conflict that discloses nothing to anyone else — otherwise a known or guessed id would hand over another caller's session. |
| ACP-6 Graceful interrupt | `interrupt(session)` sets a fence; the turn loop finishes its current atomic step (ORC-5), emits `interrupted` + `checkpoint_id`, and leaves the session resumable. |
| ACP-7 Budget transparency | Every terminal event embeds `remaining_budget`; a `budget_exhausted` terminal is emitted when the budget governing the session hits zero and names its scope. When that budget is the office's, the office is hibernating (`l2-office-control`) and wakes by itself on a reset or top-up (OC-4); a session-scoped exhaustion says so, so a client does not wait for a wake that will not come. |
| ACP-8 Monotonic event ordering | The bus assigns a per-session `AtomicU64` `seq` at emission; clients order/dedup by `seq`. A gap surfaces as `EVENT_GAP`, never silently ignored. |
| ACP-9 Pure adapters | Each projection implements `ProjectionAdapter::translate(event) -> wire` — a total, side-effect-free shape map. Adapters cannot add/drop/reorder events, hold logic, or fork state; all bind to the same bus. |
| ACP-10 Live steering | `steer(session, msg)` enqueues into the session-scoped bounded steering queue; the turn loop polls at safe boundaries, cancels not-yet-started planned actions as `action_skipped`, injects the message, and continues. Same-session messages serialize (WL-4); overflow → `STEER_REJECTED`. |

## 4. Detailed Design

### 4.1 Session store

```text
[REFERENCE]
sessions(
  session_id TEXT PRIMARY KEY,      -- ACP-5 idempotency key
  office_id  TEXT,
  trust      TEXT,                  -- trusted | restricted | anonymous
  history_ref TEXT,                 -- pointer into agent-session history
  budget_remaining INTEGER,
  tool_delegation_opt_in INTEGER,   -- ACP-4
  created_at TEXT, last_seq INTEGER -- ACP-8 high-water mark
)
```

### 4.2 Event bus and ordering

A per-session `EventBus` holds an `AtomicU64` sequence and a broadcast channel. `emit(kind, payload)` stamps `seq`, persists `last_seq`, and fans out to every attached transport and projection. Reconnecting clients replay from `since_seq`. The terminal event carries the turn's highest `seq`.

### 4.3 Capability declaration & trust

`Capabilities { office_id, version, tools[], roles[], trust_level, budget, session_mode, streaming:true }`. Trust is resolved from the authenticated identity (security): `trusted` (the authenticated local principal or an admin), `restricted` (authenticated external), `anonymous` (unauthenticated → only `capabilities`, all else `AUTH_REQUIRED`). A loopback origin is not an identity: it can narrow what a request may do (the local-only policy of `l2-agent-session` §4.10) but never grants trust by itself — any local process, the agent's own sandboxed tools included, can reach loopback. The gate is enforced at the session, beneath every projection — an adapter can never widen it.

### 4.4 Protocol projections (ACP-9)

```text
[REFERENCE]
trait ProjectionAdapter { fn bind(session) -> Endpoint; fn translate(&AcpEvent) -> Wire; }
  peer-agent | client-completion | ui-event  — each a pure translate() over the one bus
```

All adapters consume the same ordered stream; two clients on two protocols observing one session see identical turns in identical order.

### 4.5 Cross-office relay

When a caller address targets a sibling office, the receiving server forwards verbatim to the target's session bus and streams responses back without inspecting or modifying content — the ACP-level mechanism behind global-orchestration routing. Not reading content does not mean not checking it: each send is an outbound sink of the sending office, and its side-band confidentiality label is compared with the capacity granted to that office pair, blocking an over-capacity send with a typed reason (CF-4, CF-10; `l2-global-orchestration` §2).

### 4.6 Live steering & interrupt

The turn loop (agent-session) polls `steering_queue(session)` at: loop start, after every action, after a direct model response, and before finalization. On a hit it emits `action_skipped` for each not-yet-started planned action, injects the steering message, and re-plans — the turn continues. Interrupt (ACP-6) instead fences to a partial terminal and ends the turn. The queue resolves the routed `session_id` before enqueue, so a steer can never land in another session.

## 5. Implementation Notes

1. The transport (`POST /acp` + SSE) stays in agent-session; this server exposes a Rust API the endpoint calls — no HTTP code here.
2. ACP-5 idempotency is the unique PK on `session_id`; concurrent creates resolve via `INSERT … ON CONFLICT` returning the existing row.
3. Steering and interrupt share the turn-loop's safe-boundary poll; they differ only in the terminal outcome (continue vs partial-terminate).

## 6. Drawbacks & Alternatives

**Alternative — logic in each projection adapter**: violates ACP-9; divergent semantics per protocol. Rejected — adapters are pure shape maps.

**Alternative — steering as a new session**: would spawn a concurrent duplicate turn, violating WL-4. Rejected — same-session messages serialize into the one live turn.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[MODEL]` | `.design/main/specifications/l1-acp.md` | Invariants ACP-1…ACP-10 |
| `[SESSION]` | `.design/main/specifications/l2-agent-session.md` | `/acp` transport + turn loop host |
| `[SECURITY]` | `.design/main/specifications/l2-security.md` | Trust-level identity resolution |
| `[ORCH]` | `.design/main/specifications/l2-orchestration.md` | Internal protocol invoked by sessions |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.1 | 2026-09-23 | Core Team | Consistency pass (2026-09-23): ACP-5 idempotent creation returned an existing session to any caller naming its id — scoped to the creating principal and office. `trusted` was granted to "local" callers: loopback is not an identity (the agent's own sandboxed tools can reach it) and now only narrows. A revoked credential ends its sessions instead of running on the cached trust level. ACP-7 tied every `budget_exhausted` to office hibernation — the terminal names its scope. The cross-office relay checks confidentiality labels without reading content (CF-4). |
| 1.0.0 | 2026-07-03 | Core Team | Initial implementation spec — session store, monotonic event bus, capability/trust gate, pure projection adapters, cross-office relay, live-steering + interrupt over the agent-session `/acp` transport; maps ACP-1…ACP-10. |
