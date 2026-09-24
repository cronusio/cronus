# Learning Loop

**Version:** 1.1.1
**Status:** Stable
**Layer:** implementation
**Implements:** l1-extensions.md, l1-memory-model.md

## Overview

The concrete mechanism by which Cronus improves itself over time: a post-turn background review spawns an isolated agent fork that audits the session for learnable patterns and writes to memory or skills — without touching the main session's context. A periodic curator maintains the skill library on idle trigger (no continuous cron daemon required), auto-transitioning lifecycle states and never auto-deleting.

## Related Specifications

- [l1-extensions.md](l1-extensions.md) - EXT-7 (skill generation) that this spec concretizes.
- [l1-memory-model.md](l1-memory-model.md) - Memory scopes, curator ownership, lifecycle decay.
- [l2-extension-registry.md](l2-extension-registry.md) - Skill manifest format and lifecycle states.
- [l2-memory-store.md](l2-memory-store.md) - Memory write targets for the review fork.
- [l2-agent-session.md](l2-agent-session.md) - `should_review_memory` flag that gates the post-turn review.
- [l2-skill-system.md](l2-skill-system.md) - The mutable skill store (`<state>/skills/`) and the canonical package/execution stack generated skills must conform to.
- [l1-component-scanning.md](l1-component-scanning.md) - CS-10: a mutation of an admitted component re-opens its verdict — why revisions of active skills wait for review.

## 1. Motivation

Agents that run identical sessions have no institutional memory. The learning loop closes this gap: after sessions, the agent asks itself "what did I learn?" and persists the answer as a skill update or a memory write. A periodic curator keeps the growing skill library coherent — pruning stale entries, promoting candidates after a review gate, consolidating fragmented skills.

## 2. Constraints & Assumptions

- The background review runs in a daemon thread; it never blocks the main response path.
- The review fork inherits the parent's provider/model/credentials to reuse the same prefix cache.
- Writes go only to memory and skill stores; the review fork cannot modify the main conversation.
- The curator runs on inactivity detection, not a continuous cron schedule.
- The curator never auto-deletes — only archives (archival is always reversible).
- Only agent-generated skills (`source: generated`) are subject to auto-transitions; preset, user-added and imported skills are exempt.

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| EXT-7 Skill generation | Post-turn fork distills patterns into candidate skills; curator promotes them after review gate. |
| EXT-3 Default-deny | Generated skills enter as `discovered` (inactive) pending explicit review and activation; a change the fork or curator makes to an **active** skill is a pending revision, reviewed before it replaces the running version (§4.1). |
| MEM-7 Ownership split | The curator is the sole writer to skill lifecycle state; other components only read it. |
| MEM-5 Scope-aware decay & prune | Curator applies `stale_after_days` / `archive_after_days` transitions; never deletes. |
| SEC-3 Default-deny egress | Review fork operates within a tool whitelist with no network tools; its only egress is the model provider the parent session already uses under the same authorization. |
| SEC-6 Sandboxed | Review fork's write permissions are limited to memory and skill stores. |

## 4. Detailed Design

### 4.1 Post-turn background review

After every turn where `TurnContext.should_review_memory == true`, a daemon thread spawns a review fork:

```text
[REFERENCE]
ReviewFork {
  inherits: provider, model, base_url, credentials, cached_system_prompt
  tool_whitelist: [memory_write, memory_read, skill_create, skill_update, skill_view, skills_list]
  max_iterations: 10     // Limited — review, don't solve
  writes_to: memory_store | skill_store
  reads_from: conversation_snapshot (read-only copy)
}
```

The fork never modifies the main conversation's `messages` or system prompt.

Two review passes run (separately or together based on config):

**Pass 1 — Memory review**
"Has the user revealed preferences, persona, working style, or expectations worth remembering?"

- Prefer specific, durable facts over session-transient details.
- Write to the scope the fact belongs to: global (about the user across projects) / workspace (about this office) / employee (about this role).
- If nothing durable surfaces: "Nothing to save." and stop.

**Pass 2 — Skill review (active learning stance)**
"Most sessions produce at least one skill update, even if small. A pass that does nothing is a missed learning opportunity."

Signals that warrant action (any one suffices):

- User corrected style, tone, format, verbosity, or workflow — encode the correction.
- Non-trivial technique, fix, workaround, or debugging path emerged.
- A loaded skill proved wrong, missing a step, or outdated — patch it now.

Preference order (prefer earlier action):

1. **Update a currently-loaded skill** — the skill was in play; it is the right one to extend.
2. **Update an existing umbrella skill** — match by topic from `skills_list`.
3. **Add a support file** under an existing umbrella (`references/`, `templates/`).
4. **Create a new skill** — last resort only, when no existing skill covers the territory.

Target library shape: class-level skills with rich `SKILL.md` and `references/` directory — not a flat list of narrow one-session entries.

**Edits to a skill in use are revisions, not rewrites.** A skill is a standing instruction: whatever it says, every later session that loads it obeys. So options 1–3 never change an active skill in place — the fork writes a **pending revision** beside it, the active version keeps running, and the revision replaces it only when reviewed and granted (EXT-3; a mutation re-opens an admitted component's verdict, `l1-component-scanning` CS-10). A preset skill is never patched at all: the revision is an override copy in the state store, pending like any other (STO-3). A user-authored skill is revised only the same way, for its author to accept. Without this, content the session merely read — a fetched page, a tool result — could be distilled straight into an instruction every future session follows.

### 4.2 Skill package format

Each skill is a directory (package) in `<state>/skills/<name>/` (the mutable skill store — see the skill system spec):

```plaintext
[MODIFIED]
<skill-name>/
├── SKILL.md          # Full spec: triggers, instructions, constraints, examples
├── DESCRIPTION.md    # Lightweight discovery file (name, one-line description, platform tags)
├── references/       # Progressive disclosure: session-specific detail, archived versions
├── templates/        # Reusable templates the skill produces
├── assets/           # Static assets the skill references
└── workflow.nd       # Optional procedure: nodus workflow over built-in commands (canonical stack)
```

`references/`, `templates/`, and `assets/` are **support directories** — never scanned as standalone skill roots. A `SKILL.md` inside `references/` is a preserved old version, not an active skill. Generated skills carry no interpreted scripts: procedural behavior is expressed as a nodus workflow per the canonical execution stack.

Discovery: the registry indexes `SKILL.md` files that are not inside a support directory and not inside an excluded path (VCS metadata, virtualenvs, caches).

### 4.3 Curator — idle-triggered skill library maintenance

The curator fires when the agent has been idle ≥ `min_idle_hours` AND the last curator run was ≥ `interval_hours` ago.

```text
[REFERENCE]
CuratorConfig {
  interval_hours: f32,         // Default 168.0 (7 days)
  min_idle_hours: f32,         // Default 2.0
  stale_after_days: u32,       // Default 30 — active → stale
  archive_after_days: u32,     // Default 90 — stale → archived
  consolidate: bool,           // Default false — opt-in consolidation pass (LLM cost)
  paused: bool,
}
```

Curator responsibilities (in order):

1. **Auto-transitions (deterministic, no LLM):** active → stale when unused ≥ `stale_after_days`; stale → archived when unused ≥ `archive_after_days`. Pinned skills are exempt from all auto-transitions.
2. **Background review fork (LLM):** reviews quality of agent-created skills; archives, or proposes patches and consolidations via `skill_manage` — a patch or consolidation of an active skill is a pending revision (§4.1), never an in-place rewrite.
3. **Persist curator state** (see §4.4).

Hard invariants:

- Only touches agent-generated skills (`source: generated`). Preset (`source: preset`) skills and user-added or imported skills (`source: custom`) are never auto-transitioned — `custom` is the user's and imports' source, not the agent's, and an agent patch never changes a skill's source.
- Never auto-deletes — only archives. Archive is reversible.
- Pinned skills bypass all auto-transitions.
- Uses the auxiliary client — never touches the main session's prompt cache.

### 4.4 Curator state persistence

```text
<state>/skills/.curator_state (JSON)
{
  last_run_at: Option<timestamp>,
  last_run_duration_seconds: Option<f32>,
  last_run_summary: Option<String>,
  last_report_path: Option<String>,
  run_count: u32,
  paused: bool,
}
```

The state file is written atomically after each run. On corruption or absence, the curator starts fresh with default values.

### 4.5 Skill lifecycle states

```text
discovered → (grant activation) → active → (stale_after_days) → stale → (archive_after_days) → archived
                                                ^
                                                | pinned: exempt from auto-transition
```

Generated skills enter at `discovered` (inactive) per EXT-3. User review and explicit activation advance them to `active`. The curator drives stale/archive transitions.

## 5. Drawbacks & Alternatives

- **Review fork cost:** every triggered review turn makes an extra LLM call. Mitigated by the nudge-counter gate (review fires after N idle turns, not every turn) and the limited budget (`max_iterations: 10`).
- **Active learning stance produces noise:** the "most sessions produce an update" bias may generate marginal updates. Mitigated by the preference order (update before create) and the `discovered` gate on generated skills.
- **Curator fires on idle, not continuous:** a very long run without idle gaps delays curator maintenance. Acceptable: library health is not time-critical; degraded library is non-blocking.
- **Alternative — synchronous post-turn review (blocking):** the main response must not wait for review; rejected.
- **Alternative — auto-delete stale skills:** archival is recoverable; deletion is not; rejected.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[EXT]` | `.design/main/specifications/l1-extensions.md` | EXT-7 skill generation invariants |
| `[MEM]` | `.design/main/specifications/l1-memory-model.md` | Curator role and scope lifecycle |
| `[SESSION]` | `.design/main/specifications/l2-agent-session.md` | should_review_memory gate |
| `[REGISTRY]` | `.design/main/specifications/l2-extension-registry.md` | Skill manifest + lifecycle states |
| `[STORE]` | `.design/main/specifications/l2-skill-system.md` | Mutable skill store + canonical package form |

## Document History

| Version | Date | Notes |
| --- | --- | --- |
| 1.0.0 | 2026-06-25 | Initial stable spec — post-turn background review fork, skill package format, idle-triggered curator with lifecycle transitions. |
| 1.1.0 | 2026-07-08 | `[MODIFIED]` Skill package path aligned to the mutable skill store (`<state>/extensions/skills/` → `<state>/skills/`); package tree aligned to the canonical execution stack (`scripts/` support directory replaced by optional `workflow.nd` — generated skills carry no interpreted scripts). Related Specifications + Canonical References link to the skill system spec. History table added with this entry. Path alignment — status remains Stable. |
| 1.1.1 | 2026-09-23 | Consistency pass (2026-09-23): Compliance rows cited MEM-3 (multi-signal recall) and MEM-4 (source of truth) for curator ownership and decay — now MEM-7 and MEM-5. The review fork and curator could rewrite active (incl. user-authored and preset) skills in place — edits to a skill in use are now pending revisions reviewed before they replace it (EXT-3, CS-10, STO-3), closing a path from read content to standing instructions. SEC-3 row corrected (the fork's egress is the parent's own provider). The curator's scope listed `source: generated \| custom` as "agent-created", but `custom` is the user-added and imported source, contradicting the same section's "user-authored skills are never auto-transitioned" — scope is now `source: generated` only, and an agent patch never changes a skill's source (§2 aligned). |
