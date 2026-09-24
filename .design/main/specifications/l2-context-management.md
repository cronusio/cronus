# Context Management

**Version:** 1.1.1
**Status:** Stable
**Layer:** implementation
**Implements:** l1-orchestration.md

## Overview

Concrete context-window management for agent sessions: adaptive input-token budget that auto-scales to a model's discovered context window, a trim cascade that drops low-priority content before high-priority content, and an LLM-driven compaction step that summarizes older conversation turns when utilization exceeds 85%. A message `_protected` field prevents active documents from being trimmed.

## Related Specifications

- [l1-orchestration.md](l1-orchestration.md) - ORC-5 context-isolated execution that this spec concretizes.
- [l2-tool-security.md](l2-tool-security.md) - The untrusted-context preamble the trim cascade must never drop, and the provenance rule a compaction summary carries forward (§4.6 there).
- [l2-agent-session.md](l2-agent-session.md) - TurnContext and IterationBudget; context management runs as a prologue step.
- [l2-model-router.md](l2-model-router.md) - Context window size is a routing signal; this spec reads the discovered window.
- [l2-context-router.md](l2-context-router.md) - Memory and rules injection; these system messages are subject to trim cascade.
- [l1-evidence-archive.md](l1-evidence-archive.md) - [ADDED v1.1.0] EA-1 archive-before-reduce, EA-2 artifacts-carry-handles, EA-3 expansion-on-demand; the concept this spec's reduction cascade must satisfy before it removes anything.

## 1. Motivation

A production agent system must run profitably on any model — a 4k local model and a 1M cloud model need different budgets. Without adaptive scaling, a 1M-context model silently uses only 6k tokens (the safe default), never leveraging its capacity. Without a trim cascade and compaction, long sessions fail with `context_length_exceeded` at provider boundaries. This spec prevents both failure modes.

## 2. Constraints & Assumptions

- The input token budget is a soft limit: the session loop trims before sending, not by truncating the actual API call.
- Context window size is discovered from model metadata; when unknown, conservative defaults apply.
- Compaction summarizes via the same LLM (or a cheaper utility model); if the summary call fails, the session continues without compaction (degrade gracefully).
- Messages marked `_protected` are never dropped regardless of budget pressure.
- System messages that are `research_spinoff_from` (seeded research reports) are treated as essential alongside the first system message.

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| ORC-5 Context-isolated execution | Trim and compact are session-scoped; no cross-session context leaks, and a delegated unit's context is reduced inside its own session, never merged into the orchestrator's. |
| EA-1 Archive-before-reduce | [ADDED v1.1.0] Every reduction path — trim cascade (§4.2), tool-output truncation (§4.7/§4.9), compaction (§4.3) — archives the affected range before removing it. An archive-write failure aborts that reduction and leaves the content raw; it never fails the turn (EA-12). |
| EA-2 Artifacts are addresses | [ADDED v1.1.0] The summary message metadata carries the archived-range handle alongside `{compacted: true, summarized_count: n}`; the truncation annotation carries the handle for the omitted remainder. A reduction artifact without a resolvable handle is malformed. |
| EA-3 Expansion on demand | [ADDED v1.1.0] A handle in context resolves to the original range through an ordinary expansion call, so re-reading a truncated tool output no longer requires re-running the tool — and a range whose producing command is non-deterministic or no longer reproducible stays recoverable at all. |
| EA-10 Evidence, never ambient | [ADDED v1.1.0] Archived content is never re-injected automatically; the cascade's budgets are computed over live context only, and an expansion is an explicit act that re-enters the budget like any other content. |

## 4. Detailed Design

### 4.1 Adaptive input token budget

```text
[REFERENCE]
compute_input_token_budget(
  configured: u32,         // user-configured value (or default 6000)
  context_length: u32,     // discovered model context window (0 = unknown)
  explicit: bool,          // true if user set a NON-default value
  default: u32 = 6000,
  headroom: f64 = 0.85,
  hard_max: u32 = 200_000
) -> u32
```

Rules:

- **Explicit user budget:** honoured exactly, only clamped to the model's window when that window is known. Never further capped by `hard_max` (user's deliberate choice wins).
- **Auto (not explicit, window known):** `min(floor(context_length * headroom), hard_max)` — fills up to 85% of the model's window, capped at 200k so very large windows don't send pathologically large prompts.
- **Auto (not explicit, window unknown):** use `configured` if set, else `default` (conservative 6k). **Do not scale off an unproven window.**

The "materialized default" problem: the settings-save path writes the default into the config file, making a stored default look like an explicit choice. Guard against this: a stored value equal to `DEFAULT_BUDGET` is still treated as auto.

### 4.2 Trim cascade

When estimated token count exceeds `context_length - reserve_tokens`, apply in order — stop when under budget. The reserve is the response headroom of §4.8 (`reserveTokens`, default 16 384): the same boundary compaction guards, so trimming applies when compaction is skipped, fails, or leaves the list still over it — and the input never crowds out the response:

```text
Priority (highest = drop LAST):
  0. _protected messages — never dropped
  1. First system message (preset prompt) and the untrusted-context preamble
     (l2-tool-security §4.6) — never dropped; the preamble is never truncated either
  2. Research-primer system messages — never dropped
  3. Recent 10 conversation turns — dropped last
  4. Extra system messages (memory, RAG injections) — dropped first
  5. Truncate first system message (> 2000 chars → truncate + notice)
  6. Drop older conversation turns (keep recent 10)
  7. Truncate current user message (never drop; add head+tail notice)
  8. Truncate tool_call arguments in oversized assistant messages
```

Step 4 is tried before step 6 because injected memory/RAG messages are cheaper to lose than conversation turns — the user's intent history matters more than auxiliary context.

**Tool message sanitization** (runs before trim):

- Drop `role: "tool"` messages with no preceding `assistant.tool_calls` (orphans from front-trimming).
- Drop or strip `tool_calls` from assistant messages whose tool responses were all trimmed away (dangling calls trigger provider validation errors).

### 4.3 Compaction

When session utilization reaches `COMPACT_THRESHOLD = 85%` of context window:

```text
[REFERENCE]
maybe_compact(session, endpoint_url, model, messages) -> (messages, context_length, was_compacted)
```

Algorithm:

1. Estimate token usage. If `usage / context_length < 0.85`, skip.
2. If fewer than 4 conversation turns, skip (not worth compacting).
3. Split at the turn boundary where the preserved recent tail begins (the effective tail of §4.7): `recent` = that tail, `older` = everything before it.
4. Build compaction text from `older` (truncate each message to 2000 chars).
5. Call the utility model (or the session model if no utility model is configured) with the self-summary prompt.
6. On success: replace `older` with a labeled summary message; keep `recent` intact. The summary is context, not instruction: it is **not** placed in the system role, and a passage it condensed from untrusted content keeps that content's provenance (wrapped, `l2-tool-security` §4.6, CP-4) — summarizing a fetched page must not promote the page into the operator's voice.
7. On failure: return original messages unchanged (`was_compacted = false`).

Self-summary format (structured, dense, ≤1000 tokens):

```text
[REFERENCE]
## Conversation Summary
**Turns summarized:** {count}  |  **Compactions so far:** {n}

### User Goal
<one sentence>

### What Was Done
- <completed actions, specific values: paths, names, versions, URLs>

### Current State
<last discussed topic, current system/code state>

### Pending / Next Steps
- <remaining work, blockers, open questions>

### Key Context
- <constraints, preferences, specific values not to lose>
```

[MODIFIED v1.1.0] Step 6 is preceded by a mandatory archive write: the `older` range is
committed to the evidence archive **before** it is replaced (EA-1). The write is a
precondition, not a side effect — if it fails, compaction aborts and returns the original
messages unchanged, exactly as the step-7 failure path already does, and the session
reports unrelieved pressure rather than dropping content it could not preserve.

The live session history is then updated in place; the summary message carries
`{compacted: true, summarized_count: n}` **plus the handle of the archived range it stands
for** (EA-2). "In place" now describes only the live message list — the replaced turns
persist in the archive and expand back on demand (EA-3). Note also that step 4's 2000-char
per-message truncation happens on the way *into* the summarization prompt: what the
utility model sees is already lossy, which is precisely why the unabridged range must be
archived first rather than reconstructed from the summary afterwards.

### 4.4 Message protection

Any message with `_protected: true` is excluded from the trim cascade entirely. This flag is set on:

- The "active document" message injected when the user has a document open for editing.
- Other caller-defined must-keep injections.

Protected messages are subtracted from the available budget first, then the remaining budget governs trim decisions.

### 4.5 Command surface

Context management runs automatically as a prologue step in the session loop; there are no user-facing commands. Observability:

| Signal | Where |
| --- | --- |
| Compaction occurred | Structured log at INFO: `{used} -> {new_used} tokens, {older_count} messages summarized` |
| Trim applied | Structured log at INFO: `Trimmed to {n} tokens ({m} messages)` |
| Budget computed | Per-turn metric: `{configured, context_length, effective_budget}` |

### 4.6 Context engine registry

The context management system is pluggable. The built-in compaction engine (§4.3) is always registered as the default; plugins may register alternative engines that implement a richer assembly strategy.

#### Engine host capabilities

Before an engine can serve an operation, the host advertises which capabilities it supports. An engine declares the subset it requires:

```text
[REFERENCE]
ContextEngineHostCapability:
  | "bootstrap"                   // initial context assembly at session start
  | "assemble-before-prompt"      // pre-turn assembly before each model call
  | "after-turn"                  // post-turn hook after model response
  | "maintain"                    // long-running background maintenance
  | "compact"                     // summarization / compaction
  | "runtime-llm-complete"        // engine may make LLM calls internally
  | "thread-bootstrap-projection" // persistent backend thread reuse (see ContextEngineProjection)

ContextEngineHostRequirements {
  required_capabilities: Vec<ContextEngineHostCapability>,
  unsupported_message?:  String,   // guidance shown if host cannot satisfy requirements
}
```

If the host cannot satisfy an engine's `required_capabilities`, the engine selection falls back to the built-in compaction engine and a WARNING is logged.

#### AssembleResult

The return value of the engine's `assemble()` call:

```text
[REFERENCE]
AssembleResult {
  messages:          Vec<Message>,  // ordered messages sent to the model
  estimated_tokens:  u32,

  // Controls what the pre-turn overflow precheck uses as the authoritative token count.
  // "assembled"              (default) — use estimated_tokens of the assembled list.
  // "preassembly_may_overflow" — take max(assembled_estimate, raw_session_history_estimate).
  //   Use when the engine's assembled view can hide an overflow still present in the
  //   underlying transcript (e.g. a windowed engine where older turns are compressed
  //   but the full history would overflow if replayed verbatim).
  prompt_authority?: "assembled" | "preassembly_may_overflow",

  // Optional context projection lifecycle.
  context_projection?: ContextEngineProjection,

  // Optional system-prompt additions from the engine (prepended to runtime system prompt).
  system_prompt_addition?: String,
}
```

#### Context engine projection

```text
[REFERENCE]
ContextEngineProjection {
  // "per_turn" (default) — rebuild and send full context on every turn.
  // "thread_bootstrap"    — inject assembled context once for this epoch, then reuse the
  //                         backend thread until epoch changes.
  mode: "per_turn" | "thread_bootstrap",

  epoch?:       String,   // stable identifier; backend thread is rotated when epoch changes
  fingerprint?: String,   // optional diagnostic fingerprint of the projected context payload
}
```

`"thread_bootstrap"` is for provider backends that maintain a persistent processing thread (e.g. stateful inference servers). The host injects the assembled context once when the epoch is first seen, then reuses the thread — avoiding full-context re-encoding on every turn. The epoch must change whenever the assembled context changes materially (e.g. after compaction, after a tool-set rebuild).

#### Runtime modes

```text
[REFERENCE]
ContextEngineRuntimeMode: "normal" | "fallback" | "degraded"
  // "normal"    — selected engine running as configured
  // "fallback"  — configured engine failed; built-in compaction engine substituted
  // "degraded"  — even built-in engine is partially disabled (trim-only, no LLM compaction)
```

The current mode is exposed in `HotReloadStatus` (see `l2-config-hotreload.md §4.6`) and the health diagnostics endpoint.

#### Registration and initialization

```text
[REFERENCE]
// Built-in engine always available; never requires explicit registration.
ensureContextEnginesInitialized():
  if already_initialized: return
  register("legacy", BuiltInCompactionEngine)   // safe fallback for all operations
  initialized = true

// Plugin API: called during plugin load
api.registerContextEngine(id: String, factory: ContextEngineFactory)
```

Registration is once-per-process (idempotent init guard). The built-in `"legacy"` engine is always the fallback; it cannot be unregistered.

### 4.7 Tool-output compaction constants

When the compaction engine rebuilds a message list, tool outputs are the single largest
consumer of context tokens. The following constants govern tool-output handling during
compaction and the size of the preserved recent-message tail.

```text
[REFERENCE]
// Tool-output pruning (runs before the compaction LLM call, over older tool outputs only)
PRUNE_MINIMUM         = 20_000   // a prune pass runs only when it would free at least this many tokens
PRUNE_PROTECT         = 40_000   // the most recent 40 000 tokens of tool output are never pruned
TOOL_OUTPUT_MAX_CHARS =  2_000   // max characters per older tool output in the compacted list

PRUNE_PROTECTED_TOOLS = ["skill"]
// Tool outputs from listed tools are NEVER truncated during compaction.
// Skill outputs provide structural agent instructions — truncating them silently
// breaks the agent's operating model for the remainder of the session.

// The preserved recent tail — ONE definition, used by §4.3, §4.8 and the pass below
MIN_PRESERVE_RECENT_TOKENS = 2_000
effective_tail = min(keepRecentTokens,                                  // §4.8 setting, default 20 000
                     max(MIN_PRESERVE_RECENT_TOKENS, floor(usable_tokens * 0.25)))
// The requested tail, capped at a quarter of the usable window and never below 2 000:
// a 200k window keeps the full 20 000; a 16k window keeps 4 000 — a small model never
// spends most of its window on the tail. The tail is turn-aligned (never splits a turn).
```

#### Tool-output truncation format

When a tool output exceeds `TOOL_OUTPUT_MAX_CHARS`, it is truncated with a standard
annotation so the agent knows exactly how much was omitted and can request the full
content via a re-read tool call if needed. [MODIFIED v1.1.0] The full output is archived
before truncation (EA-1) and the annotation carries the archived-range handle (EA-2), so
the omitted remainder is recovered by **expanding the handle** rather than by re-running
the tool — which matters most for the outputs a re-read cannot reproduce: a
non-deterministic command, a one-shot network fetch, a build whose inputs have since
changed:

```text
[REFERENCE]
truncateToolOutput(text, maxChars = TOOL_OUTPUT_MAX_CHARS):
  if text.length <= maxChars:
    return text
  omitted = text.length - maxChars
  return text[:maxChars] + "\n[Tool output truncated for compaction: omitted "
         + omitted + " chars]"
```

Tools listed in `PRUNE_PROTECTED_TOOLS` bypass this function entirely.

#### Compaction pass algorithm (turn-aware)

1. **Completed compactions:** scan the message list for messages flagged `summary: true`
   with no error marker — these are previously committed summary messages. They are
   included in the preserved tail verbatim.
2. **Turn partitioning:** partition the remaining messages into turns at each user-message
   boundary that carries no compaction marker (`turns()`).
3. **Recent tail selection:** binary-search the turn list from the tail inward
   (`splitTurn()`) to find the largest turn-aligned tail slice that fits within
   `effective_tail`. Preserve that slice intact.
4. **Older portion compaction:** [MODIFIED v1.1.0] archive the older portion in full
   first (one write for the whole range — the same content is not archived again by each
   stage that touches it, EA-12); then, for each message in that portion, truncate tool
   outputs to `TOOL_OUTPUT_MAX_CHARS` (skip protected tools) and feed the result to the
   compaction LLM call (§4.3).
5. **Reassembly:** the compaction summary replaces the older portion; the recent tail is
   appended unchanged to produce the final message list.

### 4.8 Compaction trigger and file-operation tracking

#### Compaction trigger

```text
[REFERENCE]
shouldCompact(contextTokens, contextWindow, settings):
  if not settings.enabled: return false
  return contextTokens > contextWindow - settings.reserveTokens

CompactionSettings {
  enabled:           bool,    // default: true
  reserveTokens:     u32,     // default: 16_384  — headroom for the response
  keepRecentTokens:  u32,     // default: 20_000  — the requested recent tail (capped by §4.7's effective_tail)
}
```

The trigger fires when the available response headroom (`reserveTokens`) would be consumed
by the current context. Because `reserveTokens` is a fixed token count and §4.3's threshold
a percentage, which one fires first depends on the window: below roughly 109 000 tokens
(where 15% of the window equals 16 384) the reserve fires first; above it the 85%
threshold does. Compaction runs when either is reached; `reserveTokens` guards the hard
boundary.

`keepRecentTokens` (20 000) is the requested tail: even if shouldCompact fires, the most
recent `effective_tail` (§4.7 — `keepRecentTokens` capped at a quarter of the usable window)
worth of messages are never compacted away — they become the preserved tail without going
through the compaction LLM call.

#### File-operation tracking across compactions

Each compaction pass accumulates a `CompactionDetails` record that tracks which files the
agent has interacted with during the compacted portion of the session. This data is
preserved in the compaction entry and carried forward so later compactions can produce
richer summaries that reference previously-read or previously-modified files.

```text
[REFERENCE]
CompactionDetails {
  readFiles:     Vec<String>,   // files read but NOT modified (read-only access)
  modifiedFiles: Vec<String>,   // files written or edited (union of write + edit ops)
}

// Extraction — called on each assistant message before compaction:
extractFileOpsFromMessage(message):
  for block in message.content where block.type == "toolCall":
    path = block.arguments["path"]
    switch block.name:
      "read"  → fileOps.read.add(path)
      "write" → fileOps.written.add(path)
      "edit"  → fileOps.edited.add(path)

computeFileLists(fileOps):
  modified = fileOps.written ∪ fileOps.edited
  readOnly = fileOps.read - modified   // exclude re-read of modified files
  return { readFiles: sorted(readOnly), modifiedFiles: sorted(modified) }
```

File lists are formatted as XML tags when injected into the summarization prompt:

```text
[REFERENCE]
<read-files>
path/to/file1
path/to/file2
</read-files>
<modified-files>
path/to/file3
</modified-files>
```

### 4.9 Tool output truncation constants

Tool outputs can individually dominate context. Two independent limits apply;
whichever is hit first determines the cut point. Lines are never partially included:

```text
[REFERENCE]
DEFAULT_MAX_LINES    = 2_000        // line count ceiling for any single tool output
DEFAULT_MAX_BYTES    = 50 * 1_024   // 50 KB byte ceiling
GREP_MAX_LINE_LENGTH = 500          // max chars per grep match line (excess is dropped)

TruncationResult {
  content:               String,
  truncated:             bool,
  truncatedBy:           "lines" | "bytes" | null,
  totalLines:            u32,
  totalBytes:            u32,
  outputLines:           u32,
  outputBytes:           u32,
  lastLinePartial:       bool,
  firstLineExceedsLimit: bool,
  maxLines:              u32,
  maxBytes:              u32,
}
```

Two truncation strategies are used depending on the tool:

- **`truncateHead`** (file reads): keep the first N lines/bytes — the beginning of a file is
  typically the most informative (imports, declarations, schema).
- **`truncateTail`** (bash/shell output): keep the last N lines/bytes — shell commands often
  emit a summary or result at the end; earlier lines may be verbose progress output.

#### Summarization serialization constants

When serializing a conversation for the summarization LLM call, tool results are further
truncated to keep the summarization request within a manageable token budget:

```text
[REFERENCE]
TOOL_RESULT_MAX_CHARS = 2_000   // per tool result in serialized summarization input
                                 // (separate from TOOL_OUTPUT_MAX_CHARS used in compaction)

truncateForSummary(text, maxChars = TOOL_RESULT_MAX_CHARS):
  if text.length <= maxChars: return text
  truncated_chars = text.length - maxChars
  return text[:maxChars] + "\n\n[... {truncated_chars} more characters truncated]"

serializeConversation(messages):
  // Converts AgentMessage[] to plain text; prevents the summarization model from
  // treating the content as a conversation to continue.
  [User]: {text}
  [Assistant thinking]: {thinking}
  [Assistant]: {text}
  [Assistant tool calls]: {name}({k}={v}, ...)
  [Tool result]: {truncateForSummary(text)}
```

The summarization system prompt is a fixed constant injected as the system message
for the summarization LLM call:

```text
[REFERENCE]
SUMMARIZATION_SYSTEM_PROMPT =
  "You are a context summarization assistant. Your task is to read a conversation
   between a user and an AI assistant, then produce a structured summary following
   the exact format specified.

   Do NOT continue the conversation. Do NOT respond to any questions in the
   conversation. ONLY output the structured summary."
```

This prevents the summarization model from mistakenly generating the next turn of the
conversation instead of a summary.

## 5. Drawbacks & Alternatives

- **Compaction loses exact tool-call history:** the summary captures what happened but not the raw tool inputs/outputs. Acceptable — the summary preserves the facts needed to continue; exact replay is not a goal.
- **85% compaction threshold may trigger too early on rapid-fire turns:** a single very long turn can cross 85% before the older history is worth summarizing. Mitigation: the "< 4 turns" guard prevents premature compaction.
- **Utility model for compaction may have different quality:** if the utility model is much smaller than the session model, summaries may be coarser. Fallback: use session model if no utility model configured.
- **Disclosed simplification (FR-6)** `[ADDED]`: the shipped compactor is a no-op returning a fixed placeholder summary — automatic compaction is inert until a summarization model is bound. Upgrade trigger: a configured utility-model binding (the economical auxiliary route); the compaction contract above is unchanged, only the generator behind it is stubbed.
- **Alternative — fixed sliding window:** discard oldest N turns. Rejected: loses information abruptly and has no awareness of which messages are high/low priority (RAG injection vs user intent).

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[ORC]` | `.design/main/specifications/l1-orchestration.md` | ORC-5 context-isolated execution |
| `[SESSION]` | `.design/main/specifications/l2-agent-session.md` | TurnContext + IterationBudget |
| `[ROUTER]` | `.design/main/specifications/l2-model-router.md` | Context window discovery |

## Document History

| Version | Date | Notes |
| --- | --- | --- |
| 1.1.1 | 2026-09-23 | Consistency pass (2026-09-23): The untrusted-context preamble, which `l2-tool-security` §4.6 says is never trimmed, fell under "extra system messages — dropped first" — it is now in the never-dropped tier. A compaction summary built from content that includes untrusted tool output was inserted in the system role — it is now a labeled, provenance-carrying context message. Five incompatible definitions of the preserved recent tail (10 turns, half the turns, 2k–8k tokens, 40k, 20k) reduced to one `effective_tail` (keepRecentTokens capped at a quarter of the usable window, floor 2 000; matches the shipped `keep_recent_tokens` parameter); PRUNE_MINIMUM/PRUNE_PROTECT redescribed as the tool-output pruning parameters they are. Compliance cited ORC-3 (intent→board) and a "tool sandbox" ORC-5 row — now one ORC-5 row. The trim cascade kept a 512-token reserve, leaving the response almost no room — it now uses the §4.8 response headroom (`reserveTokens`), applying when compaction is skipped, fails, or is insufficient. §4.8 claimed the reserve always fires before the 85% threshold, which holds only below about 109 000 tokens of window — restated. |
| 1.1.0 | 2026-08-20 | Compaction is no longer destructive. Added EA-1/EA-2/EA-3/EA-10 compliance rows and reordered the reduction cascade: the affected range is archived **before** trim, tool-output truncation, or LLM compaction removes it (§4.3, §4.7), an archive-write failure aborts that reduction and leaves the content raw rather than dropping what it could not preserve, and every artifact left behind — the summary message metadata, the truncation annotation — carries the handle of the range it stands for. "Updated in-place" now describes only the live message list. Consequences: the omitted remainder of a truncated tool output is recovered by expanding the handle instead of re-running the tool (which a non-deterministic command, a one-shot fetch, or a since-changed build cannot reproduce), and the step-4 2000-char pre-summarization truncation stops being a silent one-way loss. Range archived once per reduction pass, not once per stage (EA-12). |
| 1.0.4 | 2026-07-16 | Disclosed simplification (FR-6) recorded in §5: the shipped compactor is a no-op returning a placeholder summary — compaction inert until a summarization model is bound; upgrade trigger = configured utility-model binding. History table added with this entry. |
