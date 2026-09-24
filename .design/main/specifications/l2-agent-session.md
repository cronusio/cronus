# Agent Session Loop

**Version:** 1.0.7
**Status:** Stable
**Layer:** implementation
**Implements:** l1-orchestration.md, l1-routing.md

## Overview

The concrete structure of a single agent turn: the per-turn context object that the prologue produces, the iteration budget that caps autonomous tool calls, the context compression thresholds, and the pluggable context engine interface that decides when and how to compact the message list.

## Related Specifications

- [l1-orchestration.md](l1-orchestration.md) - Orchestration model (delegation, judge, budget loop).
- [l2-model-router.md](l2-model-router.md) - Model selection for each turn's API call.
- [l2-context-router.md](l2-context-router.md) - Memory/session context injected in the turn prologue.
- [l2-model-error-recovery.md](l2-model-error-recovery.md) - Error taxonomy and recovery actions per iteration.
- [l2-learning-loop.md](l2-learning-loop.md) - Post-turn review hook gated by `should_review_memory`.
- [l2-agent-autonomy.md](l2-agent-autonomy.md) - `ActionTracker` and approval gate interact with the `action_budget_stop` hook (§4.7).
- [l2-context-management.md](l2-context-management.md) - Owns the context engine registry, the compaction trigger, and the preserved-tail definition the §4.3 interface reads.
- [l2-budget-engine.md](l2-budget-engine.md) - Receives the per-call cost events (§4.7) its hard stop is computed from.
- [l2-acp.md](l2-acp.md) - The ACP server behind the `/acp` endpoint (§4.10): ordered event bus, `EVENT_GAP`, interrupt and steering semantics.
- [l2-tool-security.md](l2-tool-security.md) - The untrusted-content wrapper tool results keep through summarization (§4.6).

## 1. Motivation

An agent turn drives prompt construction, the tool-calling loop, error recovery, and post-turn hooks. Without structure, all of this is a single monolithic function. Extracting a `TurnContext` (the prologue's output, consumed by the loop) and an `IterationBudget` (a thread-safe ceiling on tool calls) separates the setup phase from the execution phase, enables independent testing, and makes the orchestration invariants enforceable in code.

## 2. Constraints & Assumptions

- A "turn" is one user message plus all model calls and tool calls needed to produce a response.
- The prologue runs once per turn; the tool-calling loop may run many times.
- Subagents receive independent budgets; a parent agent does not donate its remaining budget to a subagent.
- Context compression may create a new session (resetting `conversation_history`); that side effect is the prologue's last step.
- Input sanitization (removal of unpaired surrogates and other invalid code points) always runs in the prologue, never later. It never removes valid text in any script; untrusted content a turn ingests is wrapped by the tool-security layer (`l2-tool-security` §4.6), not filtered here.

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| ORC-5 Context-isolated execution | Each subagent gets its own `TurnContext`; no shared mutable turn state crosses delegation boundaries. |
| ORC-6 Judged autonomous termination | A goal-driven turn (§4.9) ends as satisfied only on the independent judge's verdict — `goal.evaluate` is the judge call of `l2-orchestration` §4.3, never the acting model's own claim; `MAX_GOAL_REACT` is the hard stop. |
| ORC-7 Budget circuit-breaker | `IterationBudget.consume()` runs before each tool-call loop iteration and every model call's tokens count against the turn's ceiling (§4.2); exhaustion ends the turn with a user-visible message and leaves the session resumable. Autonomous re-entry (§4.9, §4.14) draws on the same budget. Subagent budgets are independent, but each spawn consumes a parent iteration and spawn depth is capped (`l2-orchestration` §4.6), so the total stays bounded. Cost is reported per model call (§4.7), so the budget engine's hard stop sees spend before the next call. |
| RTG-2 Graceful fallback | A failed provider call is classified per iteration (`l2-model-error-recovery`) and advances the router's cascade over eligible candidates; the loop exits only when no fallback remains (§4.5). |
| RTG-7 Bounded & traceable | Every model call in a turn — including a `prepareNextTurn` model switch (§4.14) — is a router decision, recorded with its reason and bounded by the turn's budget. |

## 4. Detailed Design

### 4.1 TurnContext — per-turn prologue output

The prologue builds one `TurnContext` per user message and hands it to the loop. The loop only reads from it (and appends to `messages`). No turn state is re-built mid-loop.

```text
[REFERENCE]
TurnContext {
  user_message: String,              // Sanitized inbound (surrogates stripped)
  original_user_message: Any,        // Clean copy for transcripts and memory queries
  messages: Vec<Message>,            // Working message list (loop appends here)
  conversation_history: Option<Vec<Message>>, // May be reset by compression
  active_system_prompt: Option<String>,        // May be rebuilt by compression
  effective_task_id: String,
  turn_id: String,                   // UUID, unique per turn
  current_turn_user_idx: usize,      // Index of the user message in `messages`
  should_review_memory: bool,        // Whether post-turn learning loop fires
  plugin_user_context: String,       // Context injected by pre_llm_call hooks
  ext_prefetch_cache: String,        // External-memory prefetch, reused across iterations
}
```

### 4.2 IterationBudget — autonomous call ceiling

Each agent instance (parent or subagent) holds one `IterationBudget`. Budgets are independent — total iterations across a parent and all its subagents may exceed the parent's own cap.

```text
[REFERENCE]
IterationBudget {
  max_total: u32,         // Parent default: 90; subagent default: 50
  used: AtomicU32,
  max_tokens: u64,        // token ceiling for the turn
  spent_tokens: AtomicU64,
}
impl IterationBudget {
  consume() -> bool    // True if allowed; increments counter. Returns false when exhausted.
  refund()             // Reclaim one iteration (for non-LLM tool calls, e.g. script execution)
  spend_tokens(n: u64) // Every model call's tokens; never refunded
  remaining() -> u32
}
```

The `refund()` mechanism keeps purely mechanical tool calls (data lookups, script execution, file reads) from depleting the iteration count against conversational or planning turns. A refund returns the iteration, never the tokens: each loop iteration still makes a model call whose tokens count against `max_tokens`, so a run of refundable calls cannot make a turn unbounded (ORC-7). The budget is exhausted when either ceiling is reached.

### 4.3 Context engine interface

One context engine is active per session. Engines are pluggable via the extension registry (kind: `plugin`, interface: `ContextEngine`). The default engine is the built-in compaction engine (`legacy`, LLM-based summarization), which is also the fallback when a configured engine fails; the registry, the compaction trigger, and the preserved tail are owned by `l2-context-management` (§4.6–§4.8) — this interface reads them and defines no threshold of its own.

```text
[REFERENCE]
trait ContextEngine {
  name() -> &str

  // Called after every LLM response with normalized usage data
  update_from_response(usage: Usage)

  // Return true if compaction should fire this turn
  should_compress(prompt_tokens: u32) -> bool

  // Compact the message list and return the shortened version.
  // The engine MAY summarize, build a DAG, or do anything else.
  // The returned list must be a valid message sequence.
  compress(
    messages: Vec<Message>,
    current_tokens: u32,
    focus_topic: Option<&str>,
  ) -> Vec<Message>

  on_session_start()
  on_session_end()

  // Readable by the caller for display and preflight checks; the values come from the
  // context-management settings (l2-context-management §4.7/§4.8)
  reserve_tokens: u32      // response headroom — compaction fires when prompt > context_length − reserve_tokens
  effective_tail: u32      // the preserved recent tail, in tokens (turn-aligned)
  context_length: u32      // 0 = unknown; the conservative budget of l2-context-management §4.1 applies
  compression_count: u32
}
```

The system prompt, `_protected` messages, and the untrusted-context preamble are always preserved in addition to the tail (`l2-context-management` §4.2, §4.4); an engine may change how the older portion is reduced, never these.

### 4.4 Prologue steps (order)

1. Guard stdio for background-thread safety.
2. Sanitize: remove unpaired surrogates and other invalid code points from the user message (valid text in any script is preserved verbatim), and skip tool-use blocks a cancelled turn left abandoned (§4.13).
3. Reset iteration counter; assign `turn_id` (UUID).
4. Restore cached system prompt, or rebuild it if missing or stale.
5. Run `pre_llm_call` plugin hooks; collect `plugin_user_context`.
6. Preflight: if `context_engine.should_compress()`, compress now (may create new session).
7. Prefetch external memory; cache result in `ext_prefetch_cache`.
8. Set `should_review_memory` based on turn flags and memory-nudge counter.
9. Return `TurnContext`.

### 4.5 Loop exit conditions

The tool-calling loop terminates when:

- The model returns a final answer (no pending tool calls).
- `IterationBudget.consume()` returns `false` (budget exhausted).
- An error is classified as non-retryable and no fallback is available.
- A stop hook fires (§4.7): iteration ceiling, action cap, or an unrecoverable text loop (§4.8).
- A stop signal (user interrupt, timeout) is received.

### 4.6 Tool-call loop engine

The tool-calling loop is structured around four pluggable seams that allow testing and extension without modifying the core loop.

#### Seams

```text
[REFERENCE]
ToolSource
  // Provides the tool catalog for the session. Built once at session start.
  // Additions (install/activation) take effect at the next turn boundary.
  // A removal, disable, or revocation takes effect at once: the dispatcher refuses a
  // call to a withdrawn tool even mid-turn; only the catalog the model sees waits for
  // the boundary.
  tools() -> Vec<ToolDefinition>
  reload()  // called by extension registry on activation/deactivation

ProgressReporter
  // Receives streaming tokens and tool events for display.
  on_token(text: &str)
  on_tool_start(tool_name: &str, params: &ToolParams)
  on_tool_end(tool_name: &str, result: &ToolResult)

TurnObserver
  // Post-tool-call observation hook (used by the learning loop).
  observe(tool_name: &str, result_summary: &str, duration_ms: u64)

CheckpointStrategy
  // Decides when to persist turn state to disk.
  should_checkpoint(tool_calls_since_last: u32, elapsed_ms: u64) -> bool
  // Default: every 10 tool calls OR every 60 seconds, whichever comes first.
```

#### KV-cache stability

The system prompt (persona, workspace rules, skill preambles) is built **once per session** at session start and kept identical for all turns. It is never modified after construction. Dynamic context — memory recalls, updated card state, injected tool results — is added as `role: "user"` messages so that the immutable system-prompt prefix remains intact in the provider's KV cache. This avoids full-context re-encoding on every turn.

The `ToolSource` catalog follows the same rule: it is built once and only rebuilt on explicit extension changes, not on each tool call. New tools added mid-session by extension installs take effect at the next turn boundary; a withdrawn tool stops being callable immediately (above) — cache stability never outranks a revocation.

#### Oversized result summarizer

When a tool result exceeds `OVERSIZED_RESULT_THRESHOLD` tokens (default: 8 000), the loop detours to a summarizer sub-agent before passing the result to the main model:

```text
[REFERENCE]
oversized_result_handler(tool_name, result, threshold):
  if token_count(result) <= threshold:
    return result
  summary = summarizer_subagent.run(
    prompt  = "Summarize the following tool result for {tool_name}:",
    content = result,
    budget  = 500 tokens
  )
  return summary
```

Before either reduction, the full result is written to the evidence archive and the summary or truncated result carries its handle (EA-1/EA-2, applied as `l2-context-management` applies them to every reduction path; ATE-12), so the omitted content stays retrievable instead of disappearing. A summary of an untrusted result stays inside the same untrusted-content wrapper (`l2-tool-security` §4.6, CP-4): summarizing does not promote tool output into trusted context. If the archive write fails, the result is not reduced and the turn reports the pressure (EA-12).

**Circuit breaker**: after 3 consecutive summarizer failures (timeout, model error, output exceeds budget), the loop stops calling the summarizer for this turn and instead truncates the raw result to `threshold` tokens with a prepended warning that names the archived handle:

```text
[REFERENCE]
"[RESULT TRUNCATED: summarizer unavailable after 3 attempts. Showing first {threshold} tokens; full result: {handle}.]"
```

The circuit breaker resets at the start of the next turn.

#### Missing-command self-healing

If the model produces a tool call for a name not present in the current `ToolSource` catalog, the loop attempts self-healing before returning an error:

1. Check whether a known skill provides the missing tool name (skill registry lookup).
2. If found: auto-activate the skill, reload the `ToolSource`, re-execute the tool call.
3. If not found: return a structured `ToolNotFoundResult { tool_name, available_tools_hint }` so the model can recover gracefully.

Self-healing is attempted at most once per missing tool name per turn; a second miss for the same name returns the error immediately.

Self-healing is the one mid-turn catalog addition the loop permits, and it stays inside granted scope: the skill must already be installed, enabled, and permitted for this agent (`l2-skill-system`). Activation never installs, enables, or grants anything — the model naming a tool is not a request for new authority — and a call the activated skill makes still passes the approval gate like any other. The activation is recorded on the turn.

### 4.7 Turn lifecycle hooks

#### Stop hooks (inline, before the model sees results)

Stop hooks run synchronously within the tool-calling loop and can terminate the turn early:

```text
[REFERENCE]
StopHook: (context: &TurnContext, budget: &IterationBudget) -> Option<StopSignal>

StopSignal {
  reason:          "budget_exhausted" | "max_iter" | "action_cap_exceeded" | "text_loop"
                 | "goal_react_cap_exceeded" | "interrupt",
  message_to_user: String,    // displayed to the user
  remaining:       Option<u32>,
}
```

Built-in stop hooks:

| Hook | Trigger | Signal reason |
| --- | --- | --- |
| `budget_stop` | `IterationBudget.consume()` returns `false` | `budget_exhausted` |
| `max_iter_stop` | Model-context iteration ceiling derived from context window | `max_iter` |
| `action_budget_stop` | `ActionTracker.record()` returns `ActionCapError` | `action_cap_exceeded` |

When any stop hook fires, the loop breaks immediately and `StopSignal.message_to_user` is returned as the turn's final response.

#### InterruptFence

A shared `Arc<AtomicBool>` is checked synchronously before three operations in each loop iteration:

1. Tool execution (before the tool is called).
2. Sub-agent spawn (before creating the child `TurnContext`).
3. Provider API call (before sending the prompt).

```text
[REFERENCE]
InterruptFence { flag: Arc<AtomicBool> }

impl InterruptFence {
  check() -> Result<(), Interrupted>
    // Returns Err(Interrupted) if the flag is set.
  signal()
    // Set by the user interrupt handler (Ctrl-C / TUI stop button).
}
```

When `check()` returns `Err`, the current operation is abandoned and the turn exits with `Interrupted` status. The fence is **cleared** (reset to `false`) only when a new turn begins from user input, so that turn can proceed normally. An autonomous re-entry — goal re-entry (§4.9) or the follow-up loop (§4.14) — checks the fence before re-entering and ends the run when it is set: an interrupt raised between iterations is never swallowed by the reset.

#### Post-turn hooks (background, after response is sent)

Post-turn hooks run as background tasks after the model's response is delivered. They do not block the turn or affect the response.

| Hook | Function |
| --- | --- |
| `archivist` | Writes a condensed turn summary, including the turn's total cost, to the session archive (`<ws>/sessions/<session_id>/archive.jsonl`) |
| `learning_loop` | Triggers the skill-review pipeline; gated by `TurnContext.should_review_memory` |
| `episodic_indexer` | Extracts episodic memory candidates from the turn and queues them for the memory store |

Post-turn hooks are fire-and-forget; failures are logged at WARN and do not surface to the user. That is acceptable only because nothing the run depends on is written here. **Cost is not a post-turn hook:** each model call reports its cost event to the budget engine as it completes (`l2-budget-engine` §4.2), because the hard stop must see spend before the next call — a turn-end write would let a long turn overrun the cap, and a dropped write would hide spend. A cost event that cannot be recorded stops further autonomous calls in the run and surfaces (fail closed), rather than letting spend go uncounted.

### 4.8 Text loop detection

When a model gets stuck in a repetitive output pattern (e.g., printing the same explanation, calling the same tool with the same arguments), the loop detector identifies the stall and injects a recovery prompt:

```text
[REFERENCE]
TEXT_LOOP_BUFFER_SIZE   = 6    // sliding window of recent assistant steps to compare
TEXT_LOOP_TRIGGER_COUNT = 3    // consecutive near-identical steps that trip detection
TEXT_LOOP_MAX_RECOVERY  = 2    // max recovery injections per turn before giving up

REPEATED_STEP_THRESHOLD = 3    // consecutive identical action signatures → nudge

normalizeForLoopDetection(text: String) -> String:
  // Lowercase, collapse whitespace, strip punctuation variants → canonical form
  // for comparison. Two steps that produce the same normalized form are "identical".

detectTextLoop(recent_steps: Vec<String>) -> bool:
  normalized = recent_steps.map(normalizeForLoopDetection)
  // Check whether the last TEXT_LOOP_TRIGGER_COUNT steps are all equivalent.
  return normalized[−TEXT_LOOP_TRIGGER_COUNT..].all_equal()
```

Recovery behavior:

1. **Mild recovery** (`RECOVERY_PROMPT_MILD`): inject as a new user message asking the model to try a different approach. Fired on first detection.
2. **Strong recovery** (`RECOVERY_PROMPT_STRONG`): more directive prompt instructing the model to stop its current approach and take an explicit alternative step. Fired on subsequent detections.
3. After `TEXT_LOOP_MAX_RECOVERY` recovery injections in a single turn, the loop is treated as non-recoverable; the turn is terminated with a `text_loop` stop signal — distinct from a user interrupt, so the record never shows a stop the user did not make.

Recovery prompts are injected in the user position but are labeled as system-generated steering, never attributed to the user (CP-1).

The `REPEATED_STEP_THRESHOLD` guard is a complementary check for action-signature repetition (same tool name + same argument hash), independent of text normalization.

### 4.9 Goal re-entry cap

When a turn is driven by an active goal (autonomous `/goal` execution), the main-loop re-entry counter bounds the total number of model re-entries per turn to prevent a never-satisfiable goal condition from burning tokens indefinitely:

```text
[REFERENCE]
MAX_GOAL_REACT = 12

goalReentryLoop(goal: Goal, turn_context: &mut TurnContext):
  for react_count in 0..MAX_GOAL_REACT:
    if interrupt_fence.is_set(): return Interrupted          // §4.7 — never swallowed
    outcome = runTurn(turn_context)                          // draws on the turn's budget
    if outcome is Interrupted or Stopped: return outcome     // user stop or stop hook
    verdict = goal.evaluate(outcome)   // the independent judge (l2-orchestration §4.3, ORC-6);
                                       // the executor's own "done" is not a verdict
    if verdict == Met: return Satisfied(goal)
    turn_context.append(nextGoalStep(goal, verdict))
    // continue to next re-entry
  // cap reached
  return HardStop {
    reason: "goal_react_cap_exceeded",
    message_to_user: "Goal not satisfied after {MAX_GOAL_REACT} attempts. Stopping.",
  }
```

`MAX_GOAL_REACT` (12) is higher than the plugin hook's `MAX_PRE_REACT` (3) because main-session goals are structurally larger tasks with more steps.

The cap is per-turn, not per-goal: a goal that spans multiple user turns accumulates re-entries independently across each turn.

### 4.10 Session server mode (daemon)

`cronus serve` starts a daemon that exposes agent sessions over HTTP, allowing remote
clients (IDE extensions, SDKs, web UIs) to drive Cronus without spawning a new process
per connection. The daemon maintains a pool of long-lived agent sessions; clients
connect via **ACP Streamable HTTP transport** — a single `/acp` endpoint mounted
alongside the existing REST session surface.

**Binding and token.** The daemon binds to loopback by default. Binding a non-loopback
interface is an explicit opt-in and requires TLS — a bearer token never crosses a network
in the clear. The token is a secret (SEC-1): it lives in the state tier, is never placed in
an environment a tool inherits, and is not readable from the agent's execution sandbox.
That matters beyond confidentiality: the token authorizes authority-plane changes (the
approval mode through `session/set_config_option`), and an agent able to read it could
call its own daemon to relax its own approvals — exactly what SEC-10 forbids.

#### ACP HTTP endpoint

```text
[REFERENCE]
POST   /acp  — send a JSON-RPC request or notification (response: 200 on initialize;
                202 Accepted for all other methods — response is on the SSE stream)
GET    /acp  — open a long-lived SSE stream (connection-scoped or session-scoped)
DELETE /acp  — terminate this connection and cancel all its owned sessions

Auth: Authorization: Bearer <token> (same middleware as the REST surface).
SSRF guard (l2-security.md §4.4) applies to any outbound URL derived from client input.

JSON-RPC methods accepted on POST /acp:
  initialize               — mint Acp-Connection-Id; return protocol version + capabilities
  session/new              — create an agent session; response on connection stream
  session/load             — restore a saved session
  session/resume           — resume an existing session
  session/prompt           — send a prompt; response streams on the session stream
  session/cancel           — (notification) cancel the in-flight prompt
  session/close            — close a session
  session/list             — enumerate active sessions
  session/set_config_option — change model or approval mode (standard ACP method;
                              configId: "model" | "mode"). A model change is a router
                              request (eligibility applies, l2-model-router); relaxing the
                              approval mode is recorded (SEC-7).
  _cronus/session/context   — get session context status (vendor extension)
  _cronus/session/update_metadata — patch session metadata
  _cronus/workspace/mcp     — MCP server status
  _cronus/workspace/skills  — loaded skills
  _cronus/workspace/env     — environment variable summary: names only, never values
                              (l2-security §4.12)
  _cronus/session/heartbeat — extend the connection idle TTL (vendor extension)
  JSON-RPC response object  — client answer to an agent→client request (e.g. permission)
```

#### Identity layers

Three identity scopes are stacked to correlate connection, session, and message:

```text
[REFERENCE]
Acp-Connection-Id  (HTTP header) — transport binding; minted by the daemon at initialize.
                                   Must be present on all /acp requests after initialization.
Acp-Session-Id     (HTTP header) — required on session-scoped GET and session-scoped POSTs.
                                   Must match sessionId inside the JSON-RPC params.
sessionId          (JSON-RPC param) — inside method params; cross-checked against the header.
                                      Mismatch → INVALID_PARAMS error.

Session ownership: a connection may only subscribe to or prompt sessions it created via
session/new / session/load / session/resume.
Unowned session access → 403 Forbidden or INVALID_PARAMS.
session/load and session/resume attach only sessions that belong to the token's user and
office (l2-multi-user-auth, OFF-1). Connection ownership keeps one user's clients from
cross-talking; it is not the security boundary — the token and that scope check are.

Loopback detection: captured at the TCP layer on each request (127.0.0.0/8, ::1,
::ffff:127.*) and threaded into permission decisions ("local-only" policy).
```

#### Two-tier SSE streams

```text
[REFERENCE]
Connection-scoped stream: GET /acp with Acp-Connection-Id only (no Acp-Session-Id).
  Receives: session/new response, session/load response, connection-level notifications.

Session-scoped stream: GET /acp with Acp-Connection-Id + Acp-Session-Id.
  Receives: session/update notifications (streaming agent output),
            agent→client requests (session/request_permission),
            final prompt result frame ({id, result: {stopReason}}).

Framing: standard SSE format ("data: <JSON-RPC object>\n\n"); events carry a monotonic id.
Resume: Last-Event-ID header resumes from the connection's ring-buffer EventBus
        (same ring-buffer maxQueued = 256 as the REST surface). A Last-Event-ID older
        than the buffer's oldest event is answered with EVENT_GAP (l2-acp, ACP-8) and
        the client re-syncs through session/load — never a silent resume past lost events.

Reconnect: a client may close and re-open the session-scoped SSE stream (e.g., network glitch)
without losing the in-flight prompt. The new stream attaches to the live session binding;
the old stream's onClose does NOT abort promptAbort if a fresh stream is now live.
```

#### Extension namespace

```text
[REFERENCE]
Standard ACP methods are never renamed.
Cronus-specific capabilities without a standard ACP equivalent use vendor-namespaced names:
  _cronus/<area>/<verb>   (ACP spec reserves the _ prefix for extensions)

Extensions are advertised at initialize under:
  agentCapabilities._meta: { "cronus": { ... } }

Clients feature-detect before use; unknown _cronus/* methods return method-not-found (-32601).
```

#### Connection lifecycle

```text
[REFERENCE]
ConnectionRegistry {
  max_connections: usize,   // cap: 64; 503 when exceeded (logged to stderr)
  idle_ttl_ms:     u64,    // default: 30 min; reap idle connections
}

touch() resets the idle clock. Called on:
  - any valid POST from this client
  - each SSE heartbeat write (prevents long-running prompts from being idle-reaped)

On connection reap: log entry + close SSE streams + abort promptAbort on each owned session.

Pre-attach buffer (frames queued before the client opens GET /acp):
  capped at 256 frames, drop-oldest policy. A drop is announced to the client on attach
  (EVENT_GAP), and pending permission requests and the final prompt result are re-sent
  from the registry on attach — a dropped frame never strands a prompt.

AbortController lifecycle for session streams:
  Each GET /acp session stream installs a fresh AbortController — never reused.
  Closing the old stream AFTER the new one is attached; avoids aborting the
  new stream's event pump on reconnect.
```

#### Permission round-trip

```text
[REFERENCE]
When an in-flight prompt requires user approval:

Agent → client (on session-scoped SSE stream):
  { "id": "_cronus_perm_N", "method": "session/request_permission", "params": { ... } }

Client → agent (POST /acp, JSON-RPC response object):
  { "id": "_cronus_perm_N", "result": { "approved": true } }
  OR
  { "id": "_cronus_perm_N", "error": { ... } }

Daemon-allocated ids use the string form "_cronus_perm_N" (N monotonic) to avoid
collisions with client-supplied numeric ids.

Pending permissions are tracked in the connection registry by JSON-RPC id, together
with the connection and session the request was sent on. An answer is accepted only
from that connection for that session; the same id posted by any other connection is
ignored and logged — the ids are sequential, so an id alone would let any token holder's
client approve another client's pending request.
On connection teardown or session close: outstanding permissions are cancelled via
cancelAbandonedPermission — never left blocking an in-flight prompt.
On malformed client vote (result: {}): cancel the permission and release the mediator
so the agent is not permanently stalled.
Only an explicit { "approved": true } approves. A cancellation, an error object, a
malformed vote, or a timeout is a denial, surfaced to the agent as a refusal (AG-9) —
never an approval by default.
```

#### Dual-transport policy

```text
[REFERENCE]
The /acp endpoint is ADDITIVE — it runs alongside the existing REST session routes.
Both transports share one underlying session engine; no state duplication.
A single session may be attached concurrently by REST and /acp clients (multi-client attach
is intentional; the bearer-token + single-workspace bind remains the trust boundary).
Prompts are still serialized by the runner's busy gate (§4.13): a second prompt while one
is in flight receives BusyError on whichever transport sent it.

Disable: CRONUS_SERVE_ACP_HTTP=0 env var prevents mounting /acp routes at startup.

Migration path: once the ACP Streamable HTTP specification ratifies and SDKs ship,
the REST surface may be reframed as a thin compatibility shim over /acp (separate PR).
```

### 4.11 MCP tool session contract

[ADDED] The contract governing how Cronus exposes its capabilities to MCP clients and how
tool responses must be shaped to maintain agent trust across the session lifetime.

#### Server instructions as single source of truth

The `initialize` response carries the primary agent-facing guidance for all tools exposed
by the Cronus MCP server. This text is emitted once at session start and surfaced in the
agent's system prompt by every compliant MCP client — it is the **only** place tool guidance
lives. Instructions must NOT be duplicated in agent-config files (`CLAUDE.md`, `.cursor/rules/`,
`AGENTS.md`, etc.); writing duplicate blocks into those files means updating the server
changes nothing for agents that already have the cached copy.

Two variants are emitted based on workspace state:

```text
[REFERENCE]
ACTIVE   — emitted when a Cronus session is fully initialized and tools are available.
           Contains: tool-selection-by-intent, common chains, anti-patterns, limitations.
           `tools/list` returns the full exposed surface.

INACTIVE — emitted when the session cannot serve tools (e.g. workspace not yet ready,
           missing required configuration). Contains: one short note that the toolset is
           inactive this session plus one action the user can take.
           `tools/list` returns an EMPTY list — absence is the one signal an agent
           cannot misread. A non-empty but uniformly-failing tool list teaches
           abandonment through error accumulation.
```

Inactive variant MUST be ≤ 3 lines. The agent should not consume context explaining a
toolset it cannot use.

#### Error response taxonomy

MCP tool calls return one of two shapes. The choice is permanent for the session: a single
`isError: true` response early in a session observably causes agents to stop calling the
entire toolset for the rest of that session (abandonment learned from negative signal).
Reserve it accordingly.

```text
[REFERENCE]
ToolCallOutcome:
  Success {
    content: Vec<ToolContent>,
    isError: false,     // default; omit in wire format
  }

  GuidedRecovery {
    content: Vec<ToolContent>,   // plain-text guidance what to do instead
    isError: false,              // SUCCESS-SHAPED — agent continues calling tools
  }

  HardRefusal {
    content: Vec<ToolContent>,   // brief explanation (1-2 sentences)
    isError: true,               // reserved for "stop trying" conditions only
  }

GuidedRecovery is correct for:
  - Workspace not yet initialized (guide: user can run `cronus init`)
  - Symbol/entity not found (guide: try a search with fewer terms)
  - Tool parameters out of range (guide: valid range or alternative)

HardRefusal is correct for:
  - Security path refusals (sensitive directories blocked by policy) — no retry guidance;
    abandonment is the intended outcome.
  - Genuine server malfunctions (not recoverable by agent action) — carries a single
    retry-once note (ATE-2) and nothing more.
```

#### Input / output size limits

All tool call handlers enforce bounds before touching any state:

```text
[REFERENCE]
MAX_QUERY_INPUT_CHARS  = 10_000   // free-form text: query, task, symbol name
MAX_PATH_INPUT_CHARS   =  4_096   // path-like inputs: filePath, glob pattern
MAX_OUTPUT_CHARS       = 15_000   // total tool response to prevent context bloat

Inputs exceeding the limit → GuidedRecovery (not HardRefusal):
  "Input exceeds the maximum length of {N} characters. Please shorten the query."

Output exceeding the limit is truncated server-side at the nearest sentence/line
boundary (never inside a character) with a trailing note:
"… [output truncated to {MAX_OUTPUT_CHARS} characters; full output: {handle}]" — the full
output is archived first, so the remainder stays reachable (ATE-12).
```

These bounds protect against hostile or buggy MCP clients sending oversized payloads (OOM,
runaway FTS scans) without treating it as an error that would teach abandonment.

#### Tool surface gating by context size

The number of tools exposed via `tools/list` may be reduced when the estimated prompt-context
cost of the full surface exceeds a threshold. This is the tool-profile concept:

```text
[REFERENCE]
ToolSurfaceProfile:
  full     — every registered tool available (default for interactive sessions)
  core     — 5–8 high-frequency tools only (search + context + recall)
  headless — 3–4 tools only (context retrieval + one write surface)

Switching triggers (automatic, not user-facing):
  full    → core:     accumulated_context_tokens >= PROFILE_DOWNGRADE_THRESHOLD
  core    → headless: tool_call_count >= PROFILE_HEADLESS_THRESHOLD

Constants:
  PROFILE_DOWNGRADE_THRESHOLD =  60_000   // estimated tokens in conversation so far
  PROFILE_HEADLESS_THRESHOLD  = 100       // total tool calls in the session
```

An agent consuming too many tokens on structural lookups gets a narrower surface that
preserves budget for high-value calls. The switch sends the protocol's tool-list-changed
notification so the client refreshes its cached list, and a call to a tool the profile
removed returns GuidedRecovery naming the tools that remain and how to restore the full
profile (ATE-2, ATE-8) — never an error, which would teach abandonment of the whole
surface.

### 4.12 Durable prompt admission

A session that writes only to in-memory state before invoking the model loses work on
crash or restart. The durable prompt admission pattern separates two explicit steps:

1. **Admit**: persist the incoming prompt (user text + metadata) to the durable store
   as a `session_input` row **before** scheduling model execution.
2. **Execute**: schedule the model execution task; on crash, the pending row is visible
   to the startup recovery pass and the task is re-queued automatically.

```text
[REFERENCE]
admit_prompt(input: UserInput) -> SessionInput:
  db.insert(session_input).values({
    session_id: ..., content: input.text, metadata: ..., status: "pending"
  })
  schedule(SessionExecution.wake(session_id))

startup_recovery():
  for row in db.select(session_input).where(status in ["pending", "processing"]):
    if row.attempts >= MAX_RECOVERY_ATTEMPTS:            // 3
      row.status = "failed"; surface to the user         // a prompt that keeps crashing
      continue                                           // the process is not retried forever
    row.attempts += 1
    schedule(SessionExecution.wake(row.session_id))
```

Status lifecycle: `"pending"` → `"processing"` (execution task picks it up) →
`"done"` (model response committed to history), or `"failed"` after
`MAX_RECOVERY_ATTEMPTS` recoveries.

A `"processing"` row found at startup means the process died mid-turn. It is re-queued
like a pending one — otherwise it would be stranded, since nothing else revisits it — and
the tool calls that turn had in flight are marked abandoned first (§4.13), so recovery
re-issues the model call against the committed history and never silently re-executes a
tool whose effect may already have happened.

**One model call per turn**: exactly one `llm.stream(request)` call is issued per
provider turn. No implicit retry loops that would duplicate the `"processing"` write.
If the model call fails, the row remains `"processing"` and the recovery path handles
it on the next startup (for crashes) or the error-recovery path handles it within the
session (for provider errors — see `l2-model-error-recovery.md`).

### 4.13 Per-session runner lifecycle

Each active session has at most one `Runner` — a scoped execution context that owns
the in-flight model call, background jobs, and the interrupt signal for that session.
Runners are stored in a process-level map keyed by session ID.

```text
[REFERENCE]
RunnerMap: Map<SessionId, Runner>

Runner {
  on_idle:      () -> void   // turn complete: delete from RunnerMap; set status "idle"
  on_busy:      () -> void   // turn started: set session status "busy"
  on_interrupt: () -> void   // interrupt fence fired (user cancel)

  ensure_running()           // create and insert runner if absent
  start_shell()              // start the interactive shell subprocess in this runner's scope
  cancel()                   // cancel background jobs + interrupt the runner
  assert_not_busy()          // returns BusyError if a runner is already active
}
```

`assert_not_busy()` is called before accepting a new prompt: two concurrent prompts for
the same session would corrupt the message history. `BusyError` is surfaced to the caller
(CLI/TUI/ACP) without aborting the in-flight turn.

#### Orphaned tool-use cleanup

When a turn is cancelled mid-execution, tool-call blocks that were opened but never
completed are marked abandoned:

```text
[REFERENCE]
abandoned tool_use block:
  state.status   = "error"
  state.metadata = { interrupted: true }
```

The prologue's sanitization pass (§4.4 step 2) detects abandoned blocks and skips them
rather than treating them as pending work, preventing a spurious assistant-prefill request
that would try to complete a cancelled operation.

### 4.14 Two-level agent loop

The agent execution model uses two nested loops that separate stable long-horizon control
(outer) from per-turn steering (inner):

```text
[REFERENCE]
outer loop (follow-up):
  for re_entry in 0..MAX_GOAL_REACT:                  // same cap as goal re-entry (§4.9)
    result = inner_loop(context, config)
    if result is Interrupted or Stopped: break        // user stop or stop hook (§4.7)
    if interrupt_fence.is_set(): break                // checked before any re-entry
    follow_ups = config.getFollowUpMessages(result.messages)
    if follow_ups is None or empty: break
    context.messages += follow_ups   // inject after agent would stop; re-enter inner loop

inner loop (turn + steering):
  while true:
    // every iteration: IterationBudget.consume(), the stop hooks, and the interrupt
    // fence of §4.7 apply before the provider call
    steerings = config.getSteeringMessages(context.messages)
    context.messages += steerings   // inject mid-run before provider call

    assistant_msg = streamAssistantResponse(context, config)

    if config.shouldStopAfterTurn(assistant_msg): break
    if assistant_msg has no tool calls: break

    tool_results = executeToolCalls(assistant_msg.toolCalls, config)
    context.messages += [assistant_msg, tool_results]

    // a tool result may carry `terminate: true` (a tool whose completion ends the turn,
    // such as a final-answer or hand-off tool); the turn ends when every result asks to
    if all(r.terminate for r in tool_results): break
```

The outer loop is bounded twice: by the re-entry cap and by the turn's `IterationBudget`, which every inner-loop iteration draws on — a follow-up hook cannot extend a run past the ceiling ORC-7 requires.

#### Hook responsibilities

| Hook | Phase | Purpose |
| --- | --- | --- |
| `getSteeringMessages` | Before each provider call (inner) | Inject mid-run guidance messages without user interaction |
| `getFollowUpMessages` | After inner loop exits (outer) | Inject next prompt; re-enters inner loop when non-empty |
| `shouldStopAfterTurn` | After each turn (inner) | Extension-supplied stop condition checked after every model response |
| `prepareNextTurn` | Between turns | Replace context, model, or thinking level for the next iteration |
| `transformContext` | Before provider call | Prune or inject messages at the `AgentMessage[]` level |
| `convertToLlm` | Before provider call | Convert `AgentMessage[]` → provider `Message[]` (custom message types) |

`getFollowUpMessages` enables autonomous re-looping (the outer loop): the agent runs to
completion, the hook injects a follow-up, and the inner loop restarts — without the user
sending a new message. Returning `None` or an empty list exits the outer loop.

`prepareNextTurn` receives the outcome of the current turn and may return a new context,
model ref, or thinking level for the next turn. This enables per-turn adaptive model
switching (e.g., upgrading to a reasoning model mid-sequence when complexity increases).
A returned model ref is a request to the router, which applies its eligibility stage
(`l2-model-router` selection pipeline): a hook cannot move a turn off-device that privacy
routing keeps local, or onto a provider the office has not authorized.

Hook-supplied content is bounded the same way. `transformContext` may prune or inject,
but never removes the system prompt, `_protected` messages, or the untrusted-context
preamble (`l2-context-management` §4.2). Messages injected by `getSteeringMessages` and
`getFollowUpMessages` carry their source (the supplying extension) and are never
attributed to the user (CP-1).

#### API key resolution per call

`getApiKey` is resolved once per provider call (not cached at session start). This
handles expiring OAuth tokens: each turn fetches the freshest credentials available —
within the credential lane the router chose for the session (RTG-10), never by switching
lanes.

### 4.15 Session entry taxonomy and tree structure

Sessions are stored as append-only JSONL files where every entry has a stable `id` and
a `parentId` pointing to the preceding entry. This forms a tree that enables branching
(forks), tree navigation, and branch summarization.

#### Entry types

| Type | In LLM context | Description |
| --- | --- | --- |
| `message` | Yes | An `AgentMessage` (user / assistant / tool-result) |
| `custom_message` | Yes (as user msg) | Extension-injected message with `display` flag for TUI rendering |
| `compaction` | Yes (summary) | Compaction result: `summary`, `firstKeptEntryId`, `tokensBefore`, `details?`, `fromHook?` |
| `branch_summary` | Yes | Summary for a pruned branch when navigating the tree |
| `thinking_level_change` | No | Records a user-changed thinking level; replayed to restore state |
| `model_change` | No | Records a user-changed model; replayed to restore state |
| `custom` | No | Extension-private data store (NOT sent to LLM); used to persist extension state across reloads |
| `label` | No | User bookmark: `targetId` + `label` string |
| `session_info` | No | Session metadata (e.g., user-defined display name) |

`CustomEntry` (`type: "custom"`) is explicitly excluded from LLM context. Extensions use it
to persist internal state across session reloads by scanning entries for their `customType`
on startup. `CustomMessageEntry` (`type: "custom_message"`) is included — it is converted
to a user-role message in `buildSessionContext()`, labeled with the extension that wrote
it and never attributed to the user (CP-1).

#### Context reconstruction (buildSessionContext)

Given a leaf entry ID, the runtime walks the tree from leaf to root, collecting the path.
Compaction entries on the path define the context boundary:

```text
[REFERENCE]
buildSessionContext(entries, leafId?):
  path = walk_to_root(leaf)   // ordered root → leaf

  // Extract last known state from path
  thinkingLevel = last thinking_level_change on path (or "off")
  model = last model_change or last assistant message's {provider, modelId}
  compaction = last compaction entry on path (or null)

  // Assemble messages
  if compaction present:
    emit compactionSummaryMessage(compaction.summary)
    emit messages from firstKeptEntryId up to (not including) compaction entry
    emit messages after compaction entry
  else:
    emit all message/custom_message/branch_summary entries on path
```

A restored model is a routing input, not a decision: it passes the router's eligibility
stage again on load, and if the policy no longer admits it (privacy routing or egress
authorization changed since it was recorded), the router chooses anew and the change is
recorded as a `model_change` entry.

#### Session migration

Stored sessions carry a `version` field; the runtime migrates on load:

```text
[REFERENCE]
CURRENT_SESSION_VERSION = 3

v1 → v2: add id/parentId tree structure to all entries
          (assign short random 8-hex ids, chain parentId sequentially)
v2 → v3: rename role "hookMessage" → "custom" in message entries
```

## 5. Drawbacks & Alternatives

- **Independent subagent budgets:** total iterations may exceed the parent's cap. Justified — a subagent solving a delegation should not penalize the parent's own remaining turns; the total stays bounded by spawn depth and per-spawn parent iterations, and spend by the budget engine's per-call hard stop.
- **Token-denominated tail instead of message counts:** a count of preserved messages keeps wildly different amounts of context depending on message size; the tail is defined once, in tokens, by `l2-context-management`, and engines may override it via the interface.
- **Alternative — share budget across parent and all subagents:** makes delegation unpredictable and hard to debug; rejected.
- **Two-level loop vs. single flat loop:** the outer follow-up level enables autonomous chaining without user input; the inner steering level enables per-turn injection without re-entering the outer loop. Separating these two concerns prevents coupling between horizon-length autonomy and within-turn guidance.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[ORC]` | `.design/main/specifications/l1-orchestration.md` | Budget and isolation invariants |
| `[ROUTER]` | `.design/main/specifications/l2-model-router.md` | Model selection per turn |
| `[RECOVERY]` | `.design/main/specifications/l2-model-error-recovery.md` | Error handling per iteration |
| `[LEARN]` | `.design/main/specifications/l2-learning-loop.md` | Post-turn review hook |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.7 | 2026-09-23 | Core Team | Consistency pass (2026-09-23): Compliance cited ORC-4 as the budget and ORC-6 as isolation (now ORC-7 and ORC-5), mapped the memory-review gate to ORC-8 and user-message sanitization to SEC-1 (secret isolation) — replaced by ORC-6 (the goal loop now takes its verdict from the independent judge, never the executor's own claim), RTG-2 and RTG-7. Budget: `refund()` let a run of refundable calls make a turn unbounded — refunds return the iteration, never the tokens (token ceiling as shipped); the follow-up outer loop had no bound — capped like goal re-entry and drawing on the same budget; cost moved from a fire-and-forget post-turn hook to a per-call event the budget engine's hard stop can see, failing closed. The interrupt fence was reset at every turn start, swallowing an interrupt raised between autonomous iterations. Context-engine defaults (75%, first/last N messages) contradicted l2-context-management — the interface now reads its trigger and token tail. Sanitization no longer strips non-ASCII text. A withdrawn tool stops being callable immediately; missing-tool self-healing stays within granted scope; oversized results are archived before summarizing/truncating and keep untrusted provenance. Daemon: loopback by default, TLS for a network bind, bearer token unreadable from the sandbox (it authorizes approval-mode changes, SEC-10); permission answers bound to the requesting connection (sequential ids were forgeable by any client) and anything but an explicit approval is a denial; `session/load`/`resume` scoped to the token's user and office; ring-buffer overrun answered with EVENT_GAP; env summary returns names only. Durable admission stranded `processing` rows at startup — they are re-queued after abandoning in-flight tool calls, with a failure cap. MCP contract: a malfunction refusal carries a retry-once note (ATE-2), profile changes notify the client, truncated output keeps an archived handle. Hook-supplied model switches pass router eligibility; injected messages carry their source. A dangling §4.15 reference replaced by the defined `terminate` flag. |
| 1.0.6 | — | Core Team | Last version before this section was added; earlier revisions are recorded in version control. |
