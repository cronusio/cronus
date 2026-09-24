# Agent Registry

**Version:** 1.0.1
**Status:** Stable
**Layer:** implementation
**Implements:** l1-roles.md, l1-orchestration.md

## Overview

The agent registry is the runtime catalog that resolves agent names to concrete definitions: system prompt, permission profile, tool allowlist, model reference, and iteration budget. It is distinct from the role catalog (which is the business layer — what a role does) and from the tier hierarchy (which is a static classification). The registry is the lookup table consulted at spawn time: "what exact configuration should this agent run with?"

## Related Specifications

- [l1-roles.md](l1-roles.md) - Roles as specialties that hire agent instances.
- [l1-orchestration.md](l1-orchestration.md) - Orchestrator delegates work to agents resolved here.
- [l2-role-catalog.md](l2-role-catalog.md) - Role definitions; a hired role instance resolves to an agent definition at runtime.
- [l2-orchestration.md](l2-orchestration.md) - `AgentTier` classification and `AgentDefinition` format extended here.
- [l2-tool-security.md](l2-tool-security.md) - `Ruleset` permission type shared with the registry's permission merge.
- [l2-extension-registry.md](l2-extension-registry.md) - Skill directories consulted when building the `external_directory` allowlist.
- [l2-model-router.md](l2-model-router.md) - `model_ref` resolution (tier/group names → concrete provider/model).

## 1. Motivation

The orchestrator and spawn machinery need a single source of truth for "what configuration does agent X run with?" Without a registry, each call site would hardcode model, permissions, and toolsets — making consistent updates impossible. The registry also provides the override mechanism: workspace config can disable, rename, or reprogram any built-in agent without touching source code.

## 2. Constraints & Assumptions

- Built-in agents are loaded first; user config applies on top (highest precedence).
- A user config entry for an unknown name creates a new custom agent; an unknown built-in name is a config error.
- Permission merge is last-key-wins (later entries in the Ruleset override earlier ones for the same key pattern).
- `model_ref` resolution is never-throw: an unknown group falls back to the run-default model.
- The registry is rebuilt when the workspace config changes (extension install, manual edit).

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| ROL-2 Preset + custom | Built-in agents form the preset catalog; user config entries form the custom catalog. |
| ROL-4 Non-destructive release | Disabling a built-in agent (config `disable: true`) removes it from the active set; enabling reverts. No agent data is deleted. |
| ROL-7 Catalog integrity | A config entry for a built-in name patches the resolved definition at load time; the built-in definition itself is never edited. |
| ORC-1 One orchestrator | The orchestrator is always the "all" mode agent at depth 0; its definition is in the registry. |
| ORC-9 Approval gate | Agents with destructive toolsets must declare it; the gate evaluates the resolved permission profile. |

## 4. Detailed Design

### 4.1 AgentDefinition schema

```text
[REFERENCE]
AgentMode: "primary" | "subagent" | "all"

AgentDefinition {
  name:            String,
  description:     Option<String>,
  mode:            AgentMode,
  native:          bool,              // true = built-in; false = user-defined
  hidden:          bool,              // excluded from user-facing lists when true
  temperature:     Option<f32>,
  top_p:           Option<f32>,
  color:           Option<String>,    // display color hint (hex or named)
  permission:      Ruleset,           // merged permission profile (see §4.3)
  model:           Option<ModelSpec>, // { provider_id, model_id } — explicit pin
  model_ref:       Option<String>,    // tier/group alias (e.g. "lite"); resolved via model-router
  variant:         Option<String>,    // provider-specific variant identifier
  prompt:          Option<String>,    // system prompt override (appended after environment block)
  steps:           Option<u32>,       // max LLM steps; overrides tier default
  tool_allowlist:  Option<Vec<String>>, // whitelist tool names; None = inherit all
  options:         HashMap<String, Value>, // provider-specific pass-through options
}
```

`mode` governs where the agent appears:

- `primary` — selectable by the user as the main interactive agent.
- `subagent` — spawnable only by the orchestrator or other agents; never directly selectable.
- `all` — usable both ways (custom agents default to this).

### 4.2 Built-in agents

| Name | Mode | Hidden | Purpose |
| --- | --- | --- | --- |
| `work` | primary | no | Default interactive agent; full permission profile; allows planning and question tools. |
| `plan` | primary | no | Planning mode; denies all edit/write tools except plan files; plan_exit allowed. |
| `compose` | primary | no | Workflow composition; orchestrates built-in skills; `skill` tool allowed. |
| `general` | subagent | no | Multi-step delegated tasks; `change_directory` denied. |
| `explore` | subagent | no | Fast read-only codebase search; deny-all except grep/glob/list/bash/read/webfetch/websearch/codesearch. |
| `title` | subagent | yes | Generates session titles; no tools; temperature 0.5. |
| `summary` | subagent | yes | Generates context summaries for compaction; no tools. |
| `compaction` | subagent | yes | Drives context compaction; no tools. |
| `checkpoint-writer` | subagent | yes | Fork agent for session checkpoints; inherits parent tool schema; see §4.5. |
| `memory-writer` | subagent | yes | Background memory consolidation; read/write/edit/glob/grep/memory/bash only. |

The `compose` and `general` agents have access to the full skill catalog. The `explore` agent carries no edit or write tools, so spawning it needs no approval of its own; its `bash` access is **not** read-only by construction — every bash call is classified from the guard's parse (`l2-tool-security` §4.2), read-class commands proceed, and anything else goes to the autonomy gate like any other call. "Read-only" names the tool set's intent, never a guarantee about what a shell can do.

### 4.3 Permission layer stack and defaults

Permission profiles are built in three layers (lowest to highest precedence):

```text
[REFERENCE]
1. defaults: Ruleset = {
     "*":                   "allow",
     "doom_loop":           "ask",
     "external_directory":  { "*": "ask", <skill_dirs/*>: "allow", <truncate_glob>: "allow" },
     "question":            "deny",
     "plan_enter":          "deny",
     "plan_exit":           "deny",
     "read":                { "*": "allow", "*.env": "deny", "*.env.*": "deny", "*.env.example": "allow" },
   }

2. per-agent overlay (built-in or custom; applied on top of defaults)

3. user config layer (workspace config's `agent.<name>.permission`; always last)
```

Merge rule: `Permission.merge(a, b)` → combines the two Rulesets; for a given key pattern, the **last** matching rule wins (`findLast` semantics). The user config layer is always applied last, so workspace policy always has the final say.

What a resolved rule means at call time (the profile is a policy input to the single autonomy gate, `l2-orchestration` §4.7): `allow` defers to the gate's own matrix — it never suppresses a prompt the gate would raise; `ask` raises at least a prompt; `deny` blocks. A `permission` layer read from project-tier configuration may only tighten (add `deny`/`ask`); a loosening entry is ignored and reported, like every other trust-sensitive key (`l2-security` §4.8).

`.env` files and their variants are always-forbidden for the agent upstream of this profile (`l2-agent-autonomy` §4.4 — no approval opens them), so the defaults state `deny` rather than a prompt that could never be granted; `.env.example`, the committed template that carries no secrets, stays readable.

After building the profile, `external_directory[truncate_glob]` is always ensured to be "allow" unless the profile explicitly sets it to "deny" — this preserves access to the tool result truncation path.

### 4.4 User config overrides

Workspace config may include an `agent` block:

```text
[REFERENCE]
agent.<name> {
  disable:       bool,               // remove from active set
  model:         Option<String>,     // "provider/model" literal or model_ref group
  variant:       Option<String>,
  prompt:        Option<String>,
  description:   Option<String>,
  temperature:   Option<f32>,
  top_p:         Option<f32>,
  mode:          Option<AgentMode>,
  color:         Option<String>,
  hidden:        Option<bool>,
  name:          Option<String>,     // display name override
  steps:         Option<u32>,
  tool_allowlist: Option<Vec<String>>,
  options:       Option<HashMap<String, Value>>,
  permission:    Option<Ruleset>,    // merged as the final layer (highest precedence)
}
```

An unknown `<name>` creates a new custom agent with `mode = "all"`, `native = false`, and the global defaults. Known names patch the existing built-in definition.

### 4.5 Checkpoint-writer (fork agent)

The `checkpoint-writer` agent follows a special **fork contract**:

- At spawn time, the parent captures its current LLM request prefix — system prompt bytes + serialized tool schema + messages-to-watermark — into a `ForkContext` snapshot stored in the actor service's in-memory map.
- The checkpoint-writer's run loop reads from that snapshot instead of recomputing a fresh prefix from its own definition.
- **Purpose:** the checkpoint-writer's provider call shares the same prefix bytes as the parent, so the provider's KV cache for those tokens remains warm — only the changed message slice needs re-encoding.
- `tool_allowlist` is **not** set on this agent; the fork runtime restricts available tools to a narrow write-only set via the spawn-time actor.tools whitelist. Omitting `tool_allowlist` from the definition is load-bearing: it ensures the fork's LLM-visible tool schema exactly matches the parent's schema (prompt-cache parity). Permission overrides at the per-call ask level are bounded by the actor.tools whitelist, not by this definition's Ruleset.

### 4.6 generate-from-description

The registry exposes a generation API that creates a new agent definition from a natural-language description:

```text
[REFERENCE]
generate(input: GenerateInput) -> GenerateResult

GenerateInput {
  description: String,
  model:       Option<ModelSpec>,  // defaults to workspace default model
}

GenerateResult {
  identifier:    String,   // snake_case; must not conflict with existing agents
  when_to_use:   String,   // one-line description of when to invoke this agent
  system_prompt: String,   // generated system prompt
}
```

- Temperature: 0.3 (deterministic generation).
- The prompt instructs the model to avoid identifier conflicts with the list of existing agent names (passed in the prompt, not in the tool schema).
- The result is returned to the caller; the caller decides whether to persist it to workspace config.

### 4.7 Default agent resolution

```text
[REFERENCE]
default_agent() -> String:
  if config.default_agent is set:
    agent = agents[config.default_agent]
    if agent is None: error("default agent not found")
    if agent.mode == "subagent": error("default agent is a subagent")
    if agent.hidden == true: error("default agent is hidden")
    return agent.name

  return first visible primary agent (mode != "subagent" AND hidden != true)
         in sorted order: [config.default_agent, "work", "plan", "compose", ...]
```

### 4.8 Command surface

| Action | CLI | TUI | Library (no code) |
| --- | --- | --- | --- |
| list agents | `cronus agent list` | `/agent list` | `registry.list() -> Vec<AgentDefinition>` |
| show agent details | `cronus agent show <name>` | `/agent show <name>` | `registry.get(name) -> AgentDefinition` |
| generate agent | `cronus agent generate --description <text>` | `/agent generate` | `registry.generate(input) -> GenerateResult` |
| set default | `cronus agent use <name>` | `/agent use <name>` | `registry.set_default(name) -> void` |

## 5. Drawbacks & Alternatives

- **Flat permission merge:** three-layer merge (defaults → built-in → user config) is simple to reason about; a more granular per-role-per-tool matrix would be more expressive but harder to audit.
- **Fork agent no-toolAllowlist invariant:** omitting toolAllowlist is counter-intuitive but load-bearing for prefix-cache parity; the comment in the definition is the only guard. A type-level marker would be safer.
- **Alternative — role catalog IS the agent registry:** rejected; roles are business entities (what a role specializes in), agents are runtime entities (what configuration runs). A Coder role can run as a `worker`-tier agent with a custom system prompt; merging the two would couple business and runtime concerns.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[ROLES]` | `.design/main/specifications/l1-roles.md` | Role model this registry supports |
| `[TIER]` | `.design/main/specifications/l2-orchestration.md` | AgentTier classification |
| `[TOOLS]` | `.design/main/specifications/l2-tool-security.md` | Ruleset permission type |
| `[ROUTER]` | `.design/main/specifications/l2-model-router.md` | model_ref resolution |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.1 | 2026-09-23 | Core Team | Consistency pass (2026-09-23): Compliance table cited a nonexistent `ROLE-*` prefix — now ROL-2, ROL-4, ROL-7. `explore` was called read-only while carrying `bash`; its bash calls are classified per call and gated. The `.env` defaults said `ask` while `l2-agent-autonomy` §4.4 makes `.env` always-forbidden — now `deny`. The meaning of a resolved rule is stated (allow defers to the gate, never suppresses a prompt), and a project-tier permission layer may only tighten. |
| 1.0.0 | — | Core Team | Last version before this section was added; earlier revisions are recorded in version control. |
