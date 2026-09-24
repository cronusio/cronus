# Plugin Hook System

**Version:** 1.0.4
**Status:** Stable
**Layer:** implementation
**Implements:** l1-extensions.md

## Overview

The plugin hook system is the runtime complement to the extension registry. While the registry handles install-time lifecycle (discover → permit → activate), hooks are invoked at runtime at defined actor lifecycle and bus event boundaries. A plugin registers a `Hooks` object when it initializes; the runtime calls the relevant hook functions at each boundary, passing mutable output structs that plugins fill in to influence behavior.

## Related Specifications

- [l1-extensions.md](l1-extensions.md) - The unified extension model that hooks implement.
- [l2-extension-registry.md](l2-extension-registry.md) - Registry that activates plugins, making their Hooks available.
- [l2-agent-session.md](l2-agent-session.md) - Turn lifecycle; preStop/postStop hooks fire around actor completion.
- [l2-agent-registry.md](l2-agent-registry.md) - `ActorMatcher` may reference agent types defined in the registry.
- [l2-agent-autonomy.md](l2-agent-autonomy.md) - Subagent progress checker (built-in hook) uses autonomy context; the gate every hook-cleared call still passes.
- [l2-security.md](l2-security.md) - Trust registry and fingerprint check that gate project hook files (§4.6).
- [l2-execution-sandbox.md](l2-execution-sandbox.md) - Confinement of command hooks; the coverage row that labels in-process hook code unconfined; the environment plan the env file obeys.
- [l1-action-gating.md](l1-action-gating.md) - AG-11: an automated check can add friction, never grant — the rule behind hook aggregation.

## 1. Motivation

Extension authors need to observe and influence agent behavior without forking the core. Hooks provide defined injection points — before an actor stops, after it delivers its result, on every bus event — with a fail-forward contract that protects the core loop from misbehaving plugins.

## 2. Constraints & Assumptions

- Actor-level hooks (§4.1–§4.9) are registered in deterministic order (internal plugins first, then file hooks, then external plugins, each group in registration order), and execute sequentially in that order within a group to preserve deterministic behavior. Tool-event hooks (§4.10) are a different family: all matching hooks run in parallel and their outcomes are aggregated by the rule in §4.10 (deny wins).
- Hooks must be fail-forward: an error in one hook is logged and a Session.Event.Error is published, but execution of the remaining hooks continues. For a tool-event hook that can block, an error or timeout is an objection, not a pass — the call resolves to `ask` (fail toward friction, as for the guard's `guard_unavailable`).
- **A hook never grants.** A hook may block, ask, annotate or patch arguments; it can never approve an action or widen what the actor may do. "No objection" from every hook sends the call on to the normal autonomy gate, which decides (`l1-security` SEC-10, `l1-action-gating` AG-11).
- Re-entry loops (preStop/postStop) are capped to prevent infinite continuation.
- Every hook runs under a timeout (§4.10 defaults; actor-level hooks share the command default).
- File hooks are reloaded on config change; other hooks are loaded once at plugin initialization time.

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| EXT-1 Unified model | All hook sources (built-in, file, external npm) register the same `Hooks` interface. |
| EXT-3 Default-deny | Hooks may not grant themselves or the actor additional tool permissions; they operate within the actor's existing Ruleset. A hook's `allow` means "no objection" — the call still passes the autonomy gate — and a `PermissionRequest` hook can deny or annotate but never approve (§4.10). |
| EXT-4 Sandboxed | **Command hooks** run through the sandbox backend by default (`l2-execution-sandbox` §4.9, row "Hook commands"), with network through the egress gate. **Code loaded into a host process** — file hooks (§4.6) and module-based plugin hooks (§4.7) — is unconfined and labeled so (SEC-12(c)); it is held by activation, the trust registry and the fingerprint check instead, never described as sandboxed. |
| EXT-8 Provenance & audit | `HookEvent.Executed` records hook ID, plugin name, duration, and outcome for every invocation. |

## 4. Detailed Design

### 4.1 Hooks interface

```text
[REFERENCE]
Hooks {
  // Fires before the actor's completion is reported to the caller.
  // May trigger a re-entry turn (continue=true). Capped at MAX_PRE_REACT.
  "actor.preStop"?: HookFn<ActorPreStopInput> | HookEntry<ActorPreStopInput>

  // Fires after the actor's result is delivered to the caller.
  // May spawn a new actor turn (continue=true). Capped at MAX_POST_REACT.
  // Result of the new turn is NOT propagated to the original caller.
  "actor.postStop"?: HookFn<ActorPostStopInput> | HookEntry<ActorPostStopInput>

  // Fires for every bus event (fire-and-forget; no output struct).
  "event"?: (EventInput) => void

  // Fires when workspace config loads or reloads.
  "config"?: (Config) => Promise<void>

  // Fires during system-prompt construction; allows transforming the system array.
  "experimental.chat.system.transform"?:
    (SystemTransformInput, SystemTransformOutput) => Promise<void>
}

type HookFn<I> = (input: I, output: ActorStopOutput) => Promise<void>

type HookEntry<I> = {
  run:      HookFn<I>,
  matcher?: ActorMatcher,
}

ActorMatcher: { agent_type: String | Vec<String> }
  // Hook fires only for actors whose agent_type matches.
  // If absent, hook fires for all actors.

ActorStopOutput {
  continue: bool,       // set true to request re-entry
  reason:   Option<String>, // re-entry prompt; required when continue=true
}
```

### 4.2 ActorPreStopInput and ActorPostStopInput

```text
[REFERENCE]
ActorPreStopInput {
  actor_id:    String,
  session_id:  String,
  agent_type:  String,
  final_text:  Option<String>,
  structured:  Option<Value>,
}

ActorPostStopInput {
  actor_id:    String,
  session_id:  String,
  agent_type:  String,
  outcome:     AgentOutcome,   // success | failure | cancelled
}
```

### 4.3 preStop ReAct loop

The preStop loop runs before the actor's completion is reported to its caller:

```text
[REFERENCE]
MAX_PRE_REACT = 3

preStopLoop(input: ActorPreStopInput):
  for iteration in 0..MAX_PRE_REACT:
    decision = aggregateDecision(input, "actor.preStop")
    if !decision.continue:
      break
    if decision.reason is empty:
      log.warn("hook returned continue=true without reason; ignored")
      break
    publish(HookEvent.ReActReentered, { phase="pre", iteration, ... })
    runTurn(reason=decision.reason)   // injects reason as a new user turn
  else:
    publish(HookEvent.ReActMaxReached, { phase="pre", ... })
```

If the loop exhausts MAX_PRE_REACT iterations without a hook returning `continue=false`, the actor stops regardless (cap is a hard ceiling, never overridden by plugins).

### 4.4 postStop ReAct loop

The postStop loop runs after the actor's result is delivered:

```text
[REFERENCE]
MAX_POST_REACT = 3

postStopLoop(input: ActorPostStopInput):
  for iteration in 0..MAX_POST_REACT:
    decision = aggregateDecision(input, "actor.postStop")
    if !decision.continue:
      break
    publish(HookEvent.ReActReentered, { phase="post", iteration, ... })
    newTurn(reason=decision.reason)   // spawns a new actor turn; result NOT propagated
  else:
    publish(HookEvent.ReActMaxReached, { phase="post", ... })
```

The postStop loop can fire additional work (e.g., memory consolidation, status updates) without the original caller waiting for it.

### 4.5 Aggregation across plugins

Multiple plugins may each request re-entry in the same hook cycle. The decision is aggregated:

```text
[REFERENCE]
aggregateDecision(input, event_name) -> ActorStopAggregatedDecision:
  reasons = []
  plugin_names = []
  hook_ids = []
  any_continue = false

  for entry in all_hooks_with_meta:
    if entry.matcher is set AND !matchesActor(entry.matcher, input):
      publish(HookEvent.Executed, outcome="skipped")
      continue

    output = ActorStopOutput { continue: false }
    try:
      entry.hook[event_name](input, output)
    catch err:
      log.error(...)
      publish(Session.Event.Error, ...)
      continue   // fail-forward

    publish(HookEvent.Executed, outcome=..., continue_requested=output.continue, ...)

    if output.continue == true AND output.reason is non-empty:
      any_continue = true
      reasons.push(output.reason)
      plugin_names.push(entry.plugin_name)
      hook_ids.push(entry.hook_id)
    elif output.continue == true:
      log.warn("continue=true without reason; ignored")

  return ActorStopAggregatedDecision {
    continue: any_continue,
    reason: reasons.join("\n\n"),
    contributing_plugin_names: plugin_names,
    contributing_hook_ids: hook_ids,
  }
```

All contributing reasons are joined (separated by double newline) as the re-entry prompt. This lets multiple plugins each contribute context to the model's next turn.

### 4.6 File hooks (no install required)

File hooks are auto-discovered from directories listed in the workspace config:

```text
[REFERENCE]
Pattern: {hook,hooks}/*.{js,ts}
  in each directory from config.directories()

Loading:
  1. Glob match the pattern in each directory.
  2. Import each matched file (ESM import with cache-busting via ?v=timestamp).
  3. Treat module.default ?? module as the Hooks object if it is a non-null object.
  4. Register with plugin_name = "file:<basename_without_ext>".
  5. hook_id_for(event) = "file:<basename>#<event>".

Reloading:
  reloadFileHooks() invalidates the file-hook state and re-runs discovery.
  Called by extension-registry on config change (extension install/uninstall, hook file edit).
```

File hooks do not require npm install or an explicit `plugin_origins` entry. They are a lightweight escape hatch for workspace-local behavior — and because importing one runs its code, a file hook found in a project directory loads only when that directory is trusted and the file's fingerprint is approved (`l2-security` §4.8: safe mode loads no project hooks; a changed file returns to pending). Cloning a repository never makes its hook files run.

### 4.7 External plugins (npm-installable)

```text
[REFERENCE]
Loading (from config.plugin_origins[]):
  1. config.waitForDependencies() — wait for npm install to complete.
  2. PluginLoader.loadExternal({ items: plugin_origins, kind: "server" }) resolves
     each specifier to a module and loads it.
  3. For each loaded module:
       a. Detect v1 plugin: { id, server: fn } → call server(input, options) → Hooks.
       b. Detect legacy plugin: function export → call fn(input) → Hooks.
  4. Register sequentially (deterministic order).

Error stages and behavior:
  install  → log error + publish session error; plugin skipped.
  compat   → log warning + publish session error; plugin skipped.
  entry    → log error + publish session error; plugin skipped.
  load     → log error + publish session error; plugin skipped.
  (Other plugins in the same load batch continue regardless.)
```

### 4.8 Built-in plugins

Built-in plugins are unconditionally loaded before file and external plugins:

| Plugin | Purpose |
| --- | --- |
| Auth plugins (per provider) | OAuth/token refresh, credential handoff via preStop/postStop hooks. |
| CheckpointSplitoverPlugin | Manages context-window boundaries; triggers checkpoint-writer at the right split point. |
| SubagentProgressCheckerPlugin | Monitors idle or stalled subagents; injects a nudge via postStop if a subagent has not made progress within a threshold. |

### 4.9 Observability events

```text
[REFERENCE]
HookEvent.Executed {
  event:               "actor.preStop" | "actor.postStop",
  hook_id:             String,     // "<plugin_name>#<event>"
  plugin_name:         String,
  actor_id:            String,
  agent_type:          String,
  duration_ms:         u64,
  outcome:             "success" | "error" | "skipped",
  continue_requested:  bool,
  reason_length:       usize,
}

HookEvent.ReActReentered {
  phase:                    "pre" | "post",
  actor_id:                 String,
  agent_type:               String,
  iteration:                u32,
  triggered_by_plugins:     Vec<String>,
  reason_preview:           String,    // first 200 chars of the concatenated reason
}

HookEvent.ReActMaxReached {
  phase:      "pre" | "post",
  actor_id:   String,
  agent_type: String,
}
```

All events are published on the workspace event bus; subscribers (TUI, telemetry) may react without coupling to hook execution.

### 4.10 Tool-event hooks (plugin-facing API)

Sections §4.1–§4.9 define hook injection points at **actor-turn granularity** (preStop/postStop ReAct loops, bus events). This section defines a complementary **tool-call granularity** hook API: plugins declare hooks in `hooks/hooks.json` that fire at individual tool calls, session boundaries, and user input events.

#### Event taxonomy

| Event | When | Primary use |
| --- | --- | --- |
| `PreToolUse` | Before a tool executes | Validate, block, ask, modify input |
| `PermissionRequest` | When a tool requests user approval | Deny or annotate the request the human sees — never approve it |
| `PostToolUse` | After a tool completes | Feedback, logging |
| `PreCompact` | Before context compaction | Mark critical context to preserve |
| `PostCompact` | After context compaction completes | Restore context, inject follow-up |
| `Stop` | Before main agent stops | Completeness verification |
| `SubagentStart` | When a subagent starts | Constraint injection, context initialization |
| `SubagentStop` | Before a subagent stops | Subagent task validation |
| `SessionStart` | Session opens | Context loading, env initialization |
| `SessionEnd` | Session closes | Cleanup, state persistence |
| `UserPromptSubmit` | User submits input | Context injection, prompt augmentation |
| `Notification` | Agent emits notification | Logging, reactions |

#### Two hook execution models

**Prompt-based hooks** (preferred for reasoning-intensive checks): an LLM evaluates the hook condition from a natural-language prompt. Supported on `PreToolUse`, `Stop`, `SubagentStop`, `UserPromptSubmit`.

**Command hooks** (preferred for fast, deterministic checks): a script or binary runs to completion and emits a JSON response. Supported on all events.

**Agent hooks** (for complex multi-step logic): spawns a complete agent sub-process that receives the full hook payload and session context. Supported on all events; use when command/prompt cannot encode the required logic. More expensive — prefer command hooks for performance-critical events.

```text
[REFERENCE]
HookDef (command): {
  "type":           "command",
  "command":        String,         // Unix/POSIX shell command
  "commandWindows": String?,        // optional Windows-specific override (defaults to command)
  "timeout":        Number,         // default 60 s
  "async":          bool            // default false; true runs hook without blocking the operation
}

HookDef (prompt):  { "type": "prompt",  "prompt": String,  "timeout": Number }
                   // default timeout: 30 s

HookDef (agent):   { "type": "agent" }
                   // spawns a full agent subprocess; inherits session context
```

All matching hooks for a given event run **in parallel**; design hooks for independence — they do not see each other's output and their execution order within a parallel group is non-deterministic.

#### hooks.json format

```text
[REFERENCE]
hooks/hooks.json:
{
  "description": String,    // optional
  "hooks": {
    "<EventName>": [
      {
        "matcher": String,  // tool matcher; see Matcher syntax below
        "hooks":   [HookDef...]
      }
    ]
  }
}
```

#### Matcher syntax

```text
Exact:    "Write"                  // one tool
OR:       "Read|Write|Edit"        // any listed tool
Wildcard: "*"                      // all tools
Regex:    "mcp__.*__delete.*"      // pattern match (case-sensitive)
```

#### Hook input (JSON on stdin, all events)

```text
[REFERENCE]
{
  session_id:      String,
  transcript_path: String,
  cwd:             String,
  permission_mode: "ask" | "allow",
  hook_event_name: String
  // Event-specific additions:
  // PreToolUse / PostToolUse:  tool_name, tool_input, tool_result (PostToolUse only)
  // UserPromptSubmit:          user_prompt
  // Stop / SubagentStop:       reason
}
```

#### Hook output

```text
[REFERENCE]
// Standard (all events):
{ continue: bool, suppressOutput: bool, systemMessage: String }

// PreToolUse: permission decision
{
  hookSpecificOutput: {
    permissionDecision: "allow" | "deny" | "ask",
    updatedInput?: Object   // optional field-level modification of tool_input
  },
  systemMessage: String
}

// Stop / SubagentStop: completion decision
{ decision: "approve" | "block", reason: String, systemMessage: String }
```

Exit codes: `0` = success (stdout shown in transcript), `2` = blocking error (stderr fed to agent), other = non-blocking error.

**Aggregating a parallel group.** Hooks in one group run in parallel and never see each other, so their outcomes combine by a fixed rule rather than by order:

```text
[REFERENCE]
permissionDecision:  any "deny" → deny;  else any "ask" (or a hook error/timeout) → ask;
                     else → no objection: the call continues to the autonomy gate, which decides.
                     A hook's "allow" is never a grant and never skips the gate or an approval prompt.
updatedInput:        applied only when exactly one hook returns it; two or more competing
                     patches → none applied, the call resolves to ask (the human sees both).
                     A patch is applied before risk classification and consent (CB-1).
Stop "approve":      means "no objection to stopping"; any "block" keeps the actor running (capped).
```

#### Environment variables (command hooks)

```text
$PLUGIN_ROOT           // Plugin directory — use for all intra-plugin file references
$CRONUS_PROJECT_DIR    // Project working directory
$CRONUS_ENV_FILE       // SessionStart only: write "export K=v" to persist env vars
```

Variables persisted through `$CRONUS_ENV_FILE` pass the same sanitization as a child's environment plan (`l2-execution-sandbox` §4.4, PI-3/PI-8): dynamic-loader and preload overrides, interpreter and shell startup hooks, engine-own credentials and secret-shaped values are refused and the refusal is logged by name — a hook cannot re-introduce a vector the engine stripped.

#### Session-load constraint

Tool-event hooks load once at session start. Unlike file hooks (§4.6), they do not hot-reload; a session restart is required after configuration changes.

#### Conditional activation

A hook may no-op when its feature flag is absent:

```text
[REFERENCE]
// Flag-file activation:
if [ ! -f "$PLUGIN_ROOT/.enable-strict-mode" ]; then exit 0; fi

// Config-file activation:
enabled=$(jq -r '.strictMode // false' "$CRONUS_PROJECT_DIR/.cronus/plugin-config.json")
if [ "$enabled" != "true" ]; then exit 0; fi
```

Document the activation mechanism in the plugin README.

### 4.11 Rule evaluation engine

Policy-based command hooks implement a declarative rule engine instead of ad-hoc scripting, providing composable, auditable policy enforcement.

#### Schema

```text
[REFERENCE]
Rule {
  name:         String
  enabled:      bool
  event:        String        // target hook event
  tool_matcher: String?       // optional; §4.10 matcher syntax
  conditions:   Condition[]   // ALL must match (AND semantics)
  action:       "block" | "warn"
  message:      String
}

Condition {
  field:    "command" | "file_path" | "content" | "new_string"
            | "old_string" | "reason" | "transcript" | "user_prompt"
  operator: "regex_match" | "contains" | "equals"
             | "not_contains" | "starts_with" | "ends_with"
  pattern:  String
}
```

#### Evaluation algorithm

```text
[REFERENCE]
evaluate_rules(rules, input) -> HookResponse:
  blocking = rules where enabled AND matches(rule, input) AND action == "block"
  warning  = rules where enabled AND matches(rule, input) AND action == "warn"

  if blocking not empty:
    msg = join("\n\n", ["[{name}]\n{message}" for r in blocking])
    if event == "Stop":
      return { decision:"block", reason:msg, systemMessage:msg }
    elif event in ["PreToolUse","PostToolUse"]:
      return { hookSpecificOutput:{permissionDecision:"deny"}, systemMessage:msg }
    else:
      return { systemMessage:msg }

  if warning not empty:
    return { systemMessage: join("\n\n", warning messages) }

  return {}   // no match → allow

matches(rule, input) -> bool:
  if rule.tool_matcher AND tool not in matcher: return false
  if rule.conditions is empty: return false   // rules require at least one condition
  return ALL(check_condition(c, input) for c in rule.conditions)
```

Compile regex patterns eagerly; LRU-cache 128 slots suffices for typical policy files.

### 4.12 Hook security model

Command hooks execute arbitrary scripts that receive unvalidated external input. Apply defense-in-depth:

- **Validate inputs**: parse stdin with a JSON tool; check field formats before use; emit exit 2 JSON on unexpected values.
- **Path safety**: reject `..` in file paths; deny writes to `.env`, secret files, or paths outside the project root.
- **Quote all bash variables**: unquoted variables allow injection — always `echo "$file_path"`, never `echo $file_path`.
- **Set timeouts**: every hook runs under a timeout — the entry's `"timeout"`, or the §4.10 default when omitted (60 s command, 30 s prompt). Aim for command hooks < 10 s; offload slow work to `PostToolUse` or `SessionEnd`.
- **Never log sensitive data**: hooks must not write user content, credentials, or file contents to stdout/stderr.

### 4.13 Hook state tracking

Each tool-event hook entry (§4.10) may carry a state block that the runtime checks before executing the handler.

```text
[REFERENCE]
HookState {
  enabled:      bool?,    // null = inherit default (enabled); false disables without removing the hook
  trusted_hash: String?,  // SHA-256 hex of the hook file/command content.
                          // When set, the hook fires ONLY when the on-disk handler matches this
                          // hash exactly. A mismatch is logged as WARNING and the hook is skipped
                          // — the operation is not aborted.
}
```

#### trusted_hash usage

The `trusted_hash` field provides a tamper-detection guard: a hook file edited after the hash was recorded will not execute until the operator updates the hash. This prevents a compromised plugin directory from silently replacing a security-critical hook with malicious logic.

```text
[REFERENCE]
Workflow:
  1. Finalize the hook handler file.
  2. Compute its SHA-256: sha256sum hooks/my_hook.sh
  3. Set state.trusted_hash in the hook configuration to the computed value.
  4. On each invocation the runtime verifies the hash before execution.
  5. On intentional update: modify the file, re-compute, update trusted_hash.

Hooks without a trusted_hash execute once their plugin is activated (low-risk hooks) —
EXCEPT model-level hooks (§4.14), which never fire without a matching trusted_hash.
```

Hash verification failures are appended to the audit log with `category: "hook_integrity_failure"` and are visible in the Doctor health report.

### 4.14 Model-level hook events

Sections §4.10–§4.13 operate at tool-call granularity. This section defines three hook events that fire at **LLM-call granularity** — once per provider request, not once per tool execution. These hooks give plugins fine-grained control over the prompt sent to the model and the response received from it, without touching the tool pipeline.

#### Event definitions

| Event | When | Primary use |
| --- | --- | --- |
| `BeforeModel` | Before each LLM request is dispatched to the provider | Modify system prompt, inject context, or provide a synthetic mock response |
| `AfterModel` | After the LLM response arrives, before tool dispatch | Redact, log, or modify the response; block on policy violations |
| `BeforeToolSelection` | After context is assembled but before the model selects tools | Dynamically restrict the available tool list or force a specific tool config |

All three events use the standard hook input/output format (§4.10) and honor the same exit-code contract. They fire in the order `BeforeModel → [LLM call] → AfterModel → BeforeToolSelection → [model selects tools]`.

#### BeforeModel

```text
[REFERENCE]
BeforeModelInput {
  session_id:      String,
  transcript_path: String,
  cwd:             String,
  hook_event_name: "BeforeModel",
  timestamp:       String,
  llm_request: {
    system:   String[],    // system prompt blocks, in order
    messages: Message[],   // conversation history
    tools:    ToolDef[],   // currently registered tools
    model:    String,      // resolved model identifier
  }
}

BeforeModelOutput {
  // Standard fields (continue, suppressOutput, systemMessage)
  hookSpecificOutput?: {
    hookEventName: "BeforeModel",
    // Modify the outgoing request (merged field-by-field):
    llm_request?: {
      system?:   String[],
      messages?: Message[],
    },
    // Inject a synthetic response — the LLM call is skipped entirely when present:
    llm_response?: {
      candidates: [{ content: { role: "model", parts: [{ text: String }] } }]
    }
  }
}
```

Synthetic response injection bypasses the provider call; the runtime behaves as if the model returned that response. Use for testing hooks or cached-response injection. It bypasses the provider, never the policy: a tool call inside a synthetic (or `AfterModel`-replaced) response is a model-produced call like any other — it passes the guard and the autonomy gate, receives no extra authority, and the transcript marks the turn as hook-supplied so no one mistakes it for the model's output.

#### AfterModel

```text
[REFERENCE]
AfterModelInput {
  session_id:      String,
  hook_event_name: "AfterModel",
  timestamp:       String,
  llm_request:     LLMRequest,   // same shape as BeforeModelInput.llm_request
  llm_response: {
    candidates: [{ content: { role: "model", parts: Part[] } }]
  }
}

AfterModelOutput {
  hookSpecificOutput?: {
    hookEventName: "AfterModel",
    // Replace the model response delivered to the tool dispatcher:
    llm_response?: {
      candidates: [{ content: { role: "model", parts: [{ text: String }] } }]
    }
  }
}
```

If `llm_response` is present in the hook output, it replaces the provider's response. If absent, the original response is used. Use for redaction (strip PII from model output before it reaches the conversation), policy enforcement (block specific tool-call patterns), or audit logging.

#### BeforeToolSelection

```text
[REFERENCE]
BeforeToolSelectionInput {
  session_id:      String,
  hook_event_name: "BeforeToolSelection",
  timestamp:       String,
  llm_request:     LLMRequest,   // assembled context for this turn
}

BeforeToolSelectionOutput {
  hookSpecificOutput?: {
    hookEventName: "BeforeToolSelection",
    toolConfig?: {
      mode: "auto" | "any" | "none",
      allowedTools?: String[],   // tool names to allow; absent = all tools allowed
    }
  }
}
```

`toolConfig.mode` mirrors the provider's function-calling mode:

- `"auto"` — model may call any allowed tool or respond with text.
- `"any"` — model must call a tool (forces tool use).
- `"none"` — model may not call any tool (text-only response).

`toolConfig.allowedTools` restricts the tool set visible to the model for this turn without removing tool definitions from the registry; it is intersected with the actor's own tool set, so it can only narrow — naming a tool the actor does not hold does not grant it. Use for role-based tool restrictions, mode-specific tool filtering (e.g. read-only mode removes all write tools), or A/B experiments.

#### Tail tool call request (§4.10 extension)

The `AfterTool` hook output may include a `tailToolCallRequest` that the runtime executes immediately after the original tool completes. The tail call's result **replaces** the original tool's result as seen by the model:

```text
[REFERENCE]
// In AfterToolOutput.hookSpecificOutput:
tailToolCallRequest?: {
  name: String,          // name of the tool to execute
  args: Map<String, Any> // arguments for the tail tool
}
```

Use case example: after a `run_shell` tool completes, a hook executes a lint or test runner as a tail call; the model sees the linter output rather than the raw shell result. Only one tail call per `AfterTool` invocation; chaining is not supported (prevents infinite recursion). The tail call is an ordinary tool call for every purpose but its trigger: it passes the guard and the autonomy gate, is receipted and audited on its own, and the original tool's result stays in the record even though the model sees the tail result.

#### Context clear on AfterAgent

The `AfterAgent` hook output may request context window clearing after the agent turn completes:

```text
[REFERENCE]
// In AfterAgentOutput.hookSpecificOutput (extends §4.10 AfterAgent output):
clearContext?: bool   // when true, the runtime clears the session context window
                      // after this agent turn; next turn starts with a fresh context
```

Use for long-running autonomous agents that periodically flush stale context to stay within token limits while preserving the constitutional files loaded from `<ws>/constitution/`.

## 5. Drawbacks & Alternatives

- **Sequential execution (actor-level hooks):** deterministic but not concurrent. A misbehaving plugin that hangs delays all subsequent hooks; bounded by the per-hook timeout every hook runs under (§2).
- **Fail-forward error contract:** a plugin error is logged but does not fail the actor. This is intentional — a buggy plugin should not take down the whole turn.
- **MAX_PRE_REACT/POST_REACT = 3:** arbitrary but prevents runaway loops. Configurable per workspace.
- **Alternative — event-driven hooks only:** simpler, but preStop/postStop re-entry capability is needed for automatic memory consolidation and subagent health checks without user involvement.
- **Model-level hooks bypass guardrails if misused:** synthetic response injection (§4.14) effectively mocks the LLM; a malicious plugin could inject harmful responses. Mitigation: model-level hooks require the same `trusted_hash` verification (§4.13) as other hooks; any hash mismatch skips the hook rather than allowing the mock.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[EXT]` | `.design/main/specifications/l1-extensions.md` | Unified extension model |
| `[REGISTRY]` | `.design/main/specifications/l2-extension-registry.md` | Install-time lifecycle |
| `[SESSION]` | `.design/main/specifications/l2-agent-session.md` | Turn lifecycle; hook integration points |
| `[SPAWN]` | `.design/main/specifications/l2-orchestration.md` | Spawn machinery that invokes the preStop/postStop loops |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.4 | 2026-09-23 | Core Team | Consistency pass (2026-09-23): Hooks could grant: `PreToolUse` `allow` and `PermissionRequest` "approve" contradicted this spec's own EXT-3 row, SEC-10 and AG-11 — a hook's allow is now "no objection" (the gate still decides) and permission-request hooks can deny or annotate only; parallel tool-event hooks aggregate deny-wins, competing `updatedInput` patches resolve to ask. §2 no longer claims all hooks run sequentially. Timeouts: "mandatory" vs "default 60 s" vs "future work" reconciled. §5 promised trusted_hash for model-level hooks while §4.13 ran unhashed hooks unconditionally — model-level hooks now require it. EXT-4 row no longer claims in-process hook code is sandboxed (`l2-execution-sandbox` §4.9). Project file hooks load only when trusted. Env file sanitized per PI-3/PI-8. Reference product environment-variable names replaced with `CRONUS_*`. Synthetic/replaced model responses and tail tool calls pass the normal gate and are labeled. |
| 1.0.3 | — | Core Team | Last version before this section was added; earlier revisions are recorded in version control. |
