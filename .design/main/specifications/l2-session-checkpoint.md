# Session Checkpoint

**Version:** 1.0.2
**Status:** Stable
**Layer:** implementation
**Implements:** l1-orchestration.md, l1-memory-model.md

## Overview

The session checkpoint system persists enough task context across LLM context-window boundaries to resume long-running autonomous turns without losing progress. Three files under the session directory capture different granularities of state: the structured checkpoint (recent action log + task reference), session-scoped memory consolidation, and a working notepad. A fork-agent writes these files using the parent's frozen LLM prefix, so the provider's KV cache for the unchanged prefix stays warm.

## Related Specifications

- [l1-orchestration.md](l1-orchestration.md) - ORC-10 resumable: goal/plan state persists and reloads on restart.
- [l1-memory-model.md](l1-memory-model.md) - Session-scoped memory tier that memory.md populates.
- [l2-agent-session.md](l2-agent-session.md) - Turn lifecycle; checkpoint is written at context-window boundaries.
- [l2-agent-registry.md](l2-agent-registry.md) - `checkpoint-writer` fork-agent definition and fork contract.
- [l2-memory-store.md](l2-memory-store.md) - Session memory entries that memory.md consolidates.
- [l2-filesystem-layout.md](l2-filesystem-layout.md) - Path layout for session-scoped files.
- [l2-security.md](l2-security.md) - Secrets-tier patterns the snapshot store never captures (SNAP-5).
- [l2-execution-workspace.md](l2-execution-workspace.md) - Where shell mutations are recovered, since snapshots cannot precede them (§4.10).

## 1. Motivation

When a model's context window fills during a multi-step autonomous task, naive truncation loses the history of what was attempted and why. A structured checkpoint lets the model resume mid-task with accurate knowledge of its current position, recent actions, and remaining work — without replaying the full message history.

## 2. Constraints & Assumptions

- Checkpoint files are session-scoped (under `<state>/sessions/<session_id>/`).
- The checkpoint-writer agent shares the parent's system-prompt + tool-schema prefix; their bytes must be identical (prefix-cache parity requirement).
- Section-budgeted reads enforce token limits per file section to avoid bloating the injected context.
- Checkpoint writes are best-effort; a write failure does not fail the parent turn.
- The fork-agent pattern is the only mechanism that satisfies prefix-cache parity; alternatives that rebuild context from scratch incur a full re-encode cost.

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| ORC-10 Resumable | checkpoint.md captures the current task state; resumed turns load it before the first model call. |
| MEM-5 Scope-aware decay & prune | memory.md entries are session-scoped and ephemeral: they are wiped when the session ends. |
| ORC-5 Context-isolated execution | The checkpoint-writer is an isolated fork; it writes state files without contaminating the parent session's message history. |

## 4. Detailed Design

### 4.1 File hierarchy

```text
[REFERENCE]
<state>/sessions/<session_id>/
  checkpoint.md    — structured task state: current focus, recent action log, open questions
  memory.md        — session-scoped memory consolidation from the memory-writer agent
  notes.md         — agent working notepad (scratch space; ephemeral between context windows)
```

Each file is created from a default template on first access (if missing). Template creation is idempotent.

### 4.2 Section budgets

Section-budgeted reads enforce a per-section token limit so that checkpoint injection does not itself consume most of the context window:

| Section (checkpoint.md) | Default token budget |
| --- | --- |
| Task spec | 2 000 |
| Progress entries (recent N) | 4 000 |
| Open questions | 500 |
| Last action log | 1 000 |

| Section (memory.md / notes.md / global memory) | Default token budget |
| --- | --- |
| Session memory | 1 000 |
| Notes | 1 500 |
| Global memory | 1 000 |

A `readBudgetedSectionAware` reader truncates each section independently at its budget, preserving section headers. Total injected tokens across all files should not exceed approximately 10 000 tokens (controlled by the caller, not enforced per-file).

### 4.3 CheckpointContext

The checkpoint context is computed at the time of writing and injected into the resumed turn's system:

```text
[REFERENCE]
CheckpointContext {
  focus_task_id:      Option<String>,    // active task ID from the task registry
  discovered_titles:  Vec<String>,       // previously generated session titles (dedup list)
  last_message_info:  LastMessageInfo,   // role, model, agent of the last real message
  progress_diff:      Option<String>,    // diff of spec vs progress.md (see §4.5)
}
```

### 4.4 Fork agent (prefix-cache parity)

The checkpoint-writer agent operates under the **fork agent contract**:

```text
[REFERENCE]
At spawn time (tryStartCheckpointWriter):
  1. Capture parent's LLM request prefix:
       ForkContext {
         system:             Vec<String>,        // frozen system prompt blocks (byte-identical)
         tools:              Vec<ToolDefinition>, // frozen serialized tool schema
         parent_permission:  Ruleset,            // parent's permission profile for tool calls
         inherited_messages: Vec<Message>,       // messages up to watermark_msg_id
         watermark_msg_id:   MessageID,
       }
  2. Store ForkContext in the actor service's in-memory map (keyed by fork actor ID).

At checkpoint-writer run time:
  3. Load ForkContext from the in-memory map instead of recomputing from agent definition.
  4. Submit system + tools + messages (from ForkContext) to the provider.
     — prefix bytes are IDENTICAL to the parent's last submission → KV-cache hit.
  5. Write checkpoint.md / memory.md / notes.md via the allowed write tools.
  6. Return; ForkContext is evicted from the map.
```

The boundary invariant (§4.6) ensures that `inherited_messages` ends at a structurally valid position for the provider's message API.

### 4.5 Progress reconcile

When a focus task is active, the checkpoint writer runs a progress reconcile before writing:

```text
[REFERENCE]
buildProgressDiff(spec: String, progress_entries: Vec<String>) -> String:
  // Compare task spec requirements to the latest progress.md entries.
  // Returns a compact diff summary: what was completed, what remains,
  // what was attempted but is still open.
  // Used as the "progress_diff" field in CheckpointContext.
```

The diff summary is injected into checkpoint.md so the model has a concrete "where I left off" reference, without replaying the full action history.

### 4.6 Boundary invariant

The LLM's message API forbids a turn ending with a dangling `tool_result` without its paired `tool_use`. Before writing a checkpoint, `adjustBoundaryForApiInvariants` scans the message slice backwards and trims to the nearest valid boundary:

```text
[REFERENCE]
adjustBoundaryForApiInvariants(messages: Vec<Message>) -> Vec<Message>:
  // Walk backwards from the end.
  // If the last message is a tool_result, scan back to find its paired tool_use user turn.
  // Trim to just before that user turn (keep the pair out if the result is incomplete).
  // The returned slice always ends on a complete assistant or user turn.
```

### 4.7 System injections

The session loop injects reminder blocks into the context at specific lifecycle points to guide the model:

```text
[REFERENCE]
autonomousLoopReminder():
  "<system-reminder>
   You are mid-loop in an autonomous task. Continue your work loop:
   respond to the tool results below and proceed to the next iteration.
   </system-reminder>"

stopReminder(focus_task_id: Option<String>):
  "<system-reminder>
   The previous assistant turn ended with a stop. Before stopping again,
   consult [focus task's progress.md head section].
   Compare the Task spec to the latest Progress entries.
   If the task is incomplete, proceed to the next concrete step.
   Only stop when the spec is genuinely satisfied or you need user input you cannot infer.
   </system-reminder>"

toolResultContinueReminder():
  "<system-reminder>
   Tool results above are real history from the autonomous loop.
   Process them and continue to the next iteration. Do not pause to summarize.
   </system-reminder>"
```

Injections are role: "user" messages (preserving KV-cache stability; they never modify the system prompt).

### 4.8 Auto-memory triggers

After each turn completes, the session evaluates whether a background memory consolidation agent should be spawned:

- `shouldAutoDream()` — true when the session has accumulated enough new content for memory consolidation (e.g., conversation depth threshold reached, significant tool output observed).
- `shouldAutoDistill()` — true when notepad (notes.md) content has grown beyond a threshold and warrants distillation into durable memory entries.

Both predicates gate spawning of the `memory-writer` background agent (see l2-agent-registry.md §4.2). The auto-memory agent runs fire-and-forget; its results update memory.md asynchronously.

### 4.9 Command surface

| Action | CLI | TUI | Library (no code) |
| --- | --- | --- | --- |
| view checkpoint | `cronus session checkpoint [--session-id <id>]` | `/checkpoint` | `checkpoint.read(session_id) -> CheckpointContext` |
| clear checkpoint | `cronus session checkpoint clear [--session-id <id>]` | `/checkpoint clear` | `checkpoint.clear(session_id) -> void` |

### 4.10 File-level snapshot contract

Sections §4.1–§4.9 describe conversation-level checkpointing (what was said and attempted). This section defines a complementary **file-level snapshot** mechanism: before any tool that mutates the file system executes, the runtime captures the current state of all touched files in a shadow repository. This lets the user restore exact file content independently of the conversation history.

#### Shadow repository layout

The shadow repository is a bare-git-style store isolated from the project's own `.git` directory:

```text
[REFERENCE]
<state>/snapshots/<project_hash>/
  .git/        — shadow git store (bare objects, HEAD branch = "snapshots")
  <named-refs> — one ref per snapshot (see naming convention below)

project_hash = SHA-256(canonical_project_root_path)[0..16]  // hex, 16 chars
```

The shadow repo is created on first use; its existence is transparent to the project's own git history. The project's `.gitignore`, hooks, and branches are not read or modified.

#### Snapshot naming convention

Each snapshot is named to be human-readable and sortable:

```text
[REFERENCE]
<ISO8601_compact>-<sanitized_filename>-<tool_name>

Examples:
  20260620T143201Z-main.rs-write_file
  20260620T144500Z-config.toml-replace
  20260620T144900Z-tests-run_shell

Rules:
  ISO8601_compact = UTC timestamp, format YYYYMMDDTHHMMSSZ, no separators
  sanitized_filename = first affected file's basename; "/" replaced with "_"; max 32 chars
  tool_name = the name of the tool that triggered the snapshot
```

For tools affecting multiple files, `sanitized_filename` uses the first file alphabetically. The full list of affected files is stored in the snapshot's commit message.

#### Snapshot trigger

A snapshot is created immediately before a tool's `PreToolUse` hook chain completes, when the tool's `mutates_filesystem` flag is true. The snapshot precedes the tool execution; if the tool fails, the snapshot still exists and can be used to confirm the pre-failure state.

```text
[REFERENCE]
tools with mutates_filesystem = true (initial set):
  write_file, replace, edit, delete_file, move_file, rename_file
```

Third-party tools may opt in by setting `mutates_filesystem: true` in their tool definition, which requires them to declare the paths they will touch before they run.

`run_shell` is **not** covered: the files a command will touch are known only after it has run, and a snapshot must precede the mutation it protects. Shell mutations are recovered through the execution workspace instead — they happen in an isolated workspace and reach the project only at its finalize write-back (`l2-execution-workspace` §4.5). `snapshot list` states this gap rather than implying shell changes are snapshotted.

#### Snapshot commit

The snapshot commit captures:

1. The full text of every file the tool will touch (tracked files only; binary files are excluded with a manifest entry).
2. A commit message with structured metadata:

```text
[REFERENCE]
snapshot: <tool_name> @ <ISO8601_compact>

Files: <comma-separated list of relative paths>
Session: <session_id>
Actor: <actor_id>
Tool input (sanitized): <JSON of tool input with secret fields redacted>
```

Secret fields (`password`, `token`, `api_key`, `secret`, `credential`) are replaced with `[REDACTED]` in the commit message.

#### Restore command

```text
[REFERENCE]
| Action | CLI | TUI | Library |
| restore file state to snapshot | cronus snapshot restore <name> | /snapshot restore <name> | snapshot.restore(name) -> RestoreResult |
| list snapshots for current project | cronus snapshot list | /snapshot list | snapshot.list() -> Vec<SnapshotMeta> |
| show snapshot details | cronus snapshot show <name> | /snapshot show <name> | snapshot.show(name) -> SnapshotDetail |

RestoreResult { restored_files: Vec<String>, skipped_files: Vec<String>, errors: Vec<String> }
SnapshotMeta { name: String, timestamp: String, tool: String, files: Vec<String> }
```

Restore rewrites the project files to the snapshot state. A file that changed after the snapshot was taken is listed first, and it is overwritten only on confirmation — a restore must not silently discard later edits, including the person's own. It does NOT restore conversation history (that is `cronus session checkpoint`'s domain). After restore, the session emits a `SessionStart` hook event with `source: "restore"` so hooks can re-initialize environment.

#### Non-interference invariants

```text
[REFERENCE]
Invariants:
  SNAP-1 The shadow repo never modifies <project_root>/.git in any way.
  SNAP-2 Snapshot names are monotonically increasing (timestamp-prefixed); no name is ever reused — a name
         that already exists (two snapshots in the same second) takes a "-<n>" counter suffix.
  SNAP-3 Snapshots are retention-limited: keep the 50 most recent snapshots per project; older ones are pruned on each new snapshot creation.
  SNAP-4 Binary files (non-UTF-8) are excluded from content capture; a manifest entry records their path and size for reference.
  SNAP-5 Secrets (matched by field name in tool_input JSON) are redacted in commit messages. Files matching the
         secrets-tier patterns (`.env` and its variants, key and credential files — `l2-security` §4.1) are never
         captured, only recorded by path, like binaries: the shadow store is a copy of project files, and SEC-1/STO-6
         keep secrets out of every copy. Other file content is captured verbatim.
```

## 5. Drawbacks & Alternatives

- **Fork-agent complexity:** the ForkContext in-memory map couples the spawn machinery to the checkpoint writer. The benefit (KV-cache warm hit) is significant for long-running autonomous tasks.
- **Section budgets are fixed defaults:** adaptive budgets (sized by remaining context window) would be more accurate but require knowing the provider's exact context size before writing.
- **Alternative — compact-on-every-turn:** too expensive for short turns; checkpointing only at context-window boundaries keeps the overhead proportional to actual context pressure.
- **Alternative — re-summarize the whole history:** LLM-based summarization loses precise action log detail; the structured checkpoint.md preserves actionable specifics the model needs to resume correctly.
- **File-level snapshots vs conversation checkpoints:** they serve different recovery goals. Snapshots restore disk state; conversation checkpoints restore model context. A full recovery combines both: restore files first, then resume from the conversation checkpoint.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[ORC]` | `.design/main/specifications/l1-orchestration.md` | ORC-10 resumable invariant |
| `[MEM]` | `.design/main/specifications/l1-memory-model.md` | Memory tier model |
| `[SESSION]` | `.design/main/specifications/l2-agent-session.md` | Turn lifecycle |
| `[REGISTRY]` | `.design/main/specifications/l2-agent-registry.md` | checkpoint-writer fork-agent contract |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.2 | 2026-09-23 | Core Team | Consistency pass (2026-09-23): Compliance cited MEM-2 (most-specific-first recall) for session scope and ORC-6 (judged termination) for isolation — now MEM-5 and ORC-5. SNAP-5 captured secret files verbatim into the shadow store ("the user's responsibility"), contradicting SEC-1/STO-6 — secrets-tier files are recorded by path only. `run_shell` was snapshotted "when its output shows file mutations", which cannot precede the mutation — removed from the covered set, with shell changes recovered through the execution workspace and the gap stated. SNAP-2 names could collide within one second — counter suffix. Restore confirms before overwriting files changed after the snapshot. |
| 1.0.1 | — | Core Team | Last version before this section was added; earlier revisions are recorded in version control. |
