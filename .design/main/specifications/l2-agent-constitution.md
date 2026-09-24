# Agent Constitution

**Version:** 1.1.1
**Status:** Stable
**Layer:** implementation
**Implements:** l1-office-model.md, l1-memory-model.md

## Overview

The concrete mechanism for workspace-scoped agent identity and cross-session memory: a small set of plaintext files that give an agent its persona, user profile, tool cheat sheet, and periodic check list. These files let an agent "wake up fresh" each session while retaining accumulated knowledge — the persona is rebuilt from the files, not from conversation history.

## Related Specifications

- [l1-office-model.md](l1-office-model.md) - Agent persona, roles, and client interaction.
- [l1-memory-model.md](l1-memory-model.md) - Memory scopes; workspace-level persistence.
- [l2-filesystem-layout.md](l2-filesystem-layout.md) - Workspace directory layout that hosts the constitution files.
- [l2-memory-store.md](l2-memory-store.md) - Structured memory (vector+FTS); constitution files are unstructured counterpart.
- [l2-scheduler.md](l2-scheduler.md) - Heartbeat action that reads HEARTBEAT.md periodic tasks.
- [l1-action-gating.md](l1-action-gating.md) - [ADDED v1.1.0] AG-10: a write into a file the agent later loads as standing instruction is a privileged sink (§4.25).
- [l2-execution-sandbox.md](l2-execution-sandbox.md) - [ADDED v1.1.0] the confinement keeps these files unwritable to any confined child even inside a writable root (§4.5 there).
- [l2-security.md](l2-security.md) - Workspace trust (§4.8): project-tier customization layers and persistent facts load only in a trusted folder (§4.11).
- [l1-security.md](l1-security.md) - SEC-9(a): a standing answer is an explicit human act — why approvals are excluded from question tuning (§4.21).

## 1. Motivation

A fresh session context has no memory of prior interactions. Structured memory (the vector/FTS store) covers factual recall; the constitution files cover *identity* — the agent's name, persona, working style, and accumulated knowledge about the user. Constitution files are plain Markdown, human-readable and user-editable, with a lightweight YAML frontmatter header that controls when each file is read. They need no migration and work without a database.

## 2. Constraints & Assumptions

- Constitution files live under `<ws>/constitution/`; the workspace controls which set is active.
- Each file has YAML frontmatter: `summary` (one-line, for index display) and `read_when` (list of load conditions).
- BOOTSTRAP.md is a first-run ritual: it is read on first session, guides initial setup, then is **deleted** — its own last instruction is to delete itself. Subsequent sessions find it absent and do nothing.
- HEARTBEAT.md is optional: an empty file (or one containing only comments) suppresses all heartbeat calls — the agent never proactively pings if it has nothing to check.
- The agent may update PROFILE.md and MEMORY.md over time; SOUL.md and HEARTBEAT.md have a stable workspace-level template and are typically user-edited. `[MODIFIED v1.1.0]` SOUL.md and HEARTBEAT.md are human-written only — the agent has no write path to them (SEC-10) — and an agent write to PROFILE.md or MEMORY.md that derives from untrusted content is staged for human acceptance instead of applied (§4.25).

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| OFF-1 Office-per-project isolation | Constitution files are per-workspace; employees of different offices carry different profiles, and no office reads another's files. |
| OFF-9 Persistent, compounding capability | PROFILE.md and MEMORY.md carry what the office learned across sessions; they are rebuilt into every session rather than reset (§4.7). |
| MEM-1 Multi-scope | Constitution files are workspace-scope; global-scope preferences live in the home workspace. |
| SEC-1 Secret isolation | Constitution files never store secrets (secrets go to `.env` / keychain). MEMORY.md may store aliases, not credentials. |
| SEC-10 Authority self-containment | SOUL.md and HEARTBEAT.md have no agent write path; the agent may only *request* a change, and the request is data (§4.25). |
| AG-10 (l1-action-gating) Provenance at privileged sinks | A write to a standing-instruction file derived from untrusted content is staged for human acceptance, never applied silently (§4.25). |

## 4. Detailed Design

### 4.1 File inventory

```plaintext
<ws>/constitution/
├── SOUL.md        # Core persona principles — defines how the agent behaves
├── PROFILE.md     # Agent identity + accumulated user profile
├── MEMORY.md      # Agent's personal cheat sheet (aliases, tool settings)
├── HEARTBEAT.md   # Periodic check task list (empty → no heartbeat)
└── BOOTSTRAP.md   # First-run ritual (deleted after completion)
```

All files use the same frontmatter schema:

```yaml
---
summary: "One-line description — shown in constitution index"
read_when:
  - condition that triggers loading this file
---
```

### 4.2 SOUL.md — persona principles

Read at the start of every session. Contains the agent's working principles: collaborative stance, transparency, when to ask vs. act, how to handle uncertainty, confidentiality expectations. Not identity-specific data — the same SOUL.md template applies across a role type, whereas PROFILE.md is instance-specific.

```yaml
---
summary: "Core persona principles for this agent role"
read_when:
  - Session start
---
```

Key principle themes:

- Be genuinely helpful, not performative.
- Have opinions; give recommendations, not just options.
- Be resourceful before asking; search before interrupting.
- Earn trust incrementally; ask for access as needed.
- Treat access as intimacy — never accumulate permissions beyond the current task.
- Sessions start fresh, but the files are the memory — always read them before acting.

### 4.3 PROFILE.md — identity and user profile

```yaml
---
summary: "Agent identity and user profile"
read_when:
  - Bootstrapping a workspace manually
  - First session of the day
---
```

Two sections:

**Agent identity** — set during bootstrap and refined by the user:

```plaintext
## Identity
- Name:      (assigned during bootstrap)
- Role type: (e.g. engineer, researcher, analyst)
- Vibe:      (e.g. sharp + methodical, warm + curious)
```

**User profile** — built over time by the agent as it learns about the user:

```plaintext
## User Profile
- Name:
- What to call them:
- Notes:

### Context
(What do they care about? What projects are they working on?
 What annoys them? What helps them most? Updated after sessions.)
```

The agent appends to `### Context` as it learns; it never overwrites the identity block without permission.

### 4.4 MEMORY.md — tool cheat sheet

```yaml
---
summary: "Agent long-term memory — tool setup and lessons learned"
read_when:
  - Session start
---
```

Free-form cheat sheet the agent builds: SSH aliases, workspace-specific paths, known environment quirks, patterns that have proven useful. Not credentials — those stay in `.env`.

Example entries:

```plaintext
### SSH
- home-server → 192.168.1.100, user: admin

### Known Quirks
- The build system caches in ~/.local/share/<app>; clearing it fixes stale artifacts.
```

### 4.5 HEARTBEAT.md — periodic check list

```yaml
---
summary: "Workspace template for HEARTBEAT.md"
read_when:
  - Bootstrapping a workspace manually
---
```

A list of tasks for the agent to check proactively on each heartbeat schedule fire. **An empty file (or comments only) suppresses all heartbeat calls** — the scheduler skips the heartbeat action entirely when HEARTBEAT.md is empty.

When populated, entries are simple task lines:

```plaintext
- Check for stalled tasks in the kanban board
- Summarize unread messages
- Verify connectivity to configured services
```

The heartbeat scheduler fires these on the workspace's `heartbeat`-action schedule.

### 4.6 BOOTSTRAP.md — first-run ritual

```yaml
---
summary: "First-run ritual — guides initial workspace setup"
read_when:
  - First session (file is absent after completion)
---
```

Read exactly once per workspace. Guides the agent through the initial setup ceremony:

1. Learn the agent's name, role, and vibe from the user.
2. Update `PROFILE.md` identity block with learned values.
3. Open and explain `SOUL.md` together with the user.
4. **Delete `BOOTSTRAP.md`** — this is the last instruction; its absence on future sessions signals bootstrap is complete.

The agent must not skip the deletion step. If BOOTSTRAP.md is present at session start, bootstrap is incomplete.

### 4.7 Load order

When loading a session, the agent reads constitution files as follows:

1. Check for BOOTSTRAP.md — if present, run bootstrap ritual (§4.6) and stop normal load.
2. Load SOUL.md (always).
3. Load PROFILE.md and MEMORY.md (when `read_when` conditions match current context).
4. Check HEARTBEAT.md to determine whether a heartbeat schedule is meaningful.

Each file's `read_when` list guides lazy loading; files not matching conditions are skipped for that session.

### 4.8 Persona wizard

When a new agent workspace is first bootstrapped interactively, BOOTSTRAP.md guides a 3-question wizard to populate the persona fields before the agent's first active session. The wizard runs through the CLI or TUI bootstrap ritual; non-interactive installs accept `--agent-name`, `--voice`, and `--focus` flags instead.

```text
[REFERENCE]
Wizard prompts (in order):
  1. "What should we call this assistant?"        → PROFILE.md Identity.Name
  2. "Voice style (one word: direct / warm / clear / playful / analytical)"
                                                  → PROFILE.md Identity.Vibe
  3. "Primary job-to-be-done (one line)"          → SOUL.md Focus header

Behavior rules:
  - Existing values are displayed as defaults; Enter accepts them.
  - A blank answer keeps the existing default — never clears it.
  - After the wizard completes, PROFILE.md and SOUL.md are updated atomically (write-then-rename).
  - The wizard exit line mirrors the result: "Persona set: <Name>, <Voice>, <Focus>"
```

The wizard also offers an optional GitHub backup for the constitution directory — if the user answers yes and the `gh` CLI is present, the workspace constitution folder is pushed to a private repository. This is opt-in and skipped silently when `gh` is absent.

### 4.9 Communication contract

SOUL.md must include a **Communication contract** section that constrains how the agent presents output. This section is read at every session start (it is part of SOUL.md's `read_when: [Session start]`).

```text
[REFERENCE]
Required subsections within SOUL.md Communication contract:

Style rules:
  - BLUF (bottom line up front) — lead every response with the conclusion, not the buildup.
  - No hedging language ("I think", "you might find", "it seems").
  - Short, declarative sentences. No em-dash abuse.
  - Practitioner voice — describe situations and decisions, not features.

Contradiction handling:
  - Flag contradictions explicitly; do NOT silently resolve them.
  - Record the contradiction in <ws>/contradictions.md with the conflicting sources.
  - Ask for the user's decision rather than picking a side.

Source citation:
  - Cite vault sources with [[wikilink]] syntax.
  - Only fall back to web research when the vault has no relevant material.
  - Search the vault first for any question about the user's domain.

Vocabulary preferences (per workspace):
  - A "use:" list and an "avoid:" list; both are free-form.
  - The lists travel in SOUL.md (user-editable).
  - Example avoid patterns: AI jargon, corporate buzzwords, named AI persona mascots.

Does / does-NOT-do contract:
  - Two short lists in SOUL.md: what the agent actively does, and what it refuses.
  - Does-NOT-do entries are invariants, not suggestions — they require explicit user override to lift.
  - Examples of does-NOT-do invariants:
      "modify reviewed/stable documents without explicit instruction"
      "push to a remote git repository without being asked"
      "fabricate information not present in sources or web research"
      "resolve contradictions silently"
```

The does/does-NOT-do contract is particularly valuable when the agent is running scheduled tasks autonomously — it limits the blast radius of any misfire without requiring the user to be present.

### 4.10 Agent activation sequence

Every agent role (analyst, architect, dev, reviewer, planner) follows the same eight-step activation sequence. This ensures consistent persona adoption, fact loading, and config resolution regardless of which role is activating.

```text
[REFERENCE]
Activation sequence (execute in strict order):

Step 1: Resolve the agent/workflow block
  Run: resolve_customization(skill_root, key="agent"|"workflow")
  On failure, manually merge the three customization files in base → team → user order
  (see §4.11 for merge rules).

Step 2: Execute activation_steps_prepend
  Run each entry in {agent.activation_steps_prepend} in order.
  These steps execute before persona adoption.

Step 3: Adopt persona
  Embody the role's core identity from the SKILL.md overview.
  Layer the resolved customization on top:
    - {agent.role}             → additional responsibility
    - {agent.identity}         → how to present
    - {agent.communication_style} → how to speak
    - {agent.principles}       → operating constraints
  Do not break character until the user explicitly dismisses the persona.

Step 4: Load persistent_facts
  Treat every entry in {agent.persistent_facts} as foundational session context.
  Entries prefixed "file:" are paths/globs under project_root — load referenced contents.
  All other entries are literal facts.

Step 5: Load config
  Load the workspace's agent configuration (<ws>/config.json). Resolve:
    - {user_name}              → name for greetings
    - {communication_language} → all responses in this language
    - {document_output_language} → output artifacts in this language
    - {planning_artifacts}     → output location
    - {project_knowledge}      → additional context paths/globs

Step 6: Greet the user
  Address {user_name} by name in {communication_language}.
  Lead the greeting with {agent.icon} — the icon prefix identifies the active persona
  throughout the session. Continue prefixing every response with {agent.icon}.

Step 7: Execute activation_steps_append
  Run each entry in {agent.activation_steps_append} in order.
  Confirm every prepend and append entry executed before continuing.

Step 8: Dispatch or present menu
  If the user's opening message clearly maps to a menu item, skip the menu and
  dispatch directly. Otherwise render {agent.menu} as a numbered table:
    Code | Description | Action
  Wait for user input. Accept a number, a menu code, or a fuzzy match.
  Clarify only when two or more items are genuinely close — one question, not a ritual.
```

The icon prefix convention is binding for the full session: every response the agent sends opens with `{agent.icon} **{agent.name}:**` so the active persona is unambiguous in multi-agent conversations.

### 4.11 Customization resolution (three-file merge)

Agent and workflow behavior is controlled by a three-layer configuration merge. Each layer narrows or extends the layer below it; user preferences always win over team defaults, which always win over skill defaults.

```text
[REFERENCE]
Merge order (base → team → user):

  1. {skill_root}/customize.toml          — skill defaults (authored by skill developer)
  2. {project_root}/_config/custom/{skill_name}.toml  — team overrides (checked into repo)
  3. {project_root}/_config/custom/{skill_name}.user.toml  — personal overrides (gitignored)

Merge rules:
  - Scalars:   later file wins (override)
  - Tables:    deep-merge (later file adds/overrides keys; other keys preserved)
  - Arrays keyed by "code" or "id":
      matching entry → replace in place
      new entry      → append
  - All other arrays: append
```

A missing file is silently skipped. The merge produces one resolved config block that the agent reads as a single object.

The project-tier layers (the team override checked into the repository, and the project's `persistent_facts` files of §4.10 step 4) are repository content: they load only in a trusted workspace (`l2-security` §4.8), their text reaches the agent as project instructions under the same rule as the other operator-instruction files (`l2-tool-security` §4.8), and they can never relax a trust-sensitive setting or reach the authority plane — a repository does not configure its own reviewer.

#### Four-layer system config resolver

When resolving the full agent registry (all agents/skills in a workspace), the resolver applies a four-layer merge instead of three, separating installer-base from team-wide overrides:

```text
[REFERENCE]
  Layer 1: _config/config.toml          — installer base, team-scoped defaults
  Layer 2: _config/config.user.toml     — installer base, user-scoped defaults
  Layer 3: _config/custom/config.toml   — team overrides (post-install, checked in)
  Layer 4: _config/custom/config.user.toml — personal overrides (post-install, gitignored)

Agent entries are keyed by agent "code":
  { code, name, title, icon, description, module, team }
```

The four-layer structure allows team leads to distribute a baseline in layers 1–2 while allowing project-specific tuning in layer 3 and individual preferences in layer 4 — all without merge conflicts.

### 4.12 Project context document

The project context document (`project-context.md`) is an AI-rules document that lives at the project root. It records **unobvious implementation details** that an LLM cannot infer from reading the code — things that would surprise a capable developer on first contact with the codebase.

```text
[REFERENCE]
project-context.md purpose:
  - Critical patterns that deviate from language/framework defaults
  - Hidden invariants (non-obvious ordering, initialization constraints)
  - Active technical debt that affects adjacent development
  - Tribal knowledge: decisions that look wrong but are intentional
  - Anti-patterns to avoid (with a one-line reason)
  - Environment and toolchain quirks specific to this project

project-context.md is NOT for:
  - Standard library/framework documentation
  - Things evident from reading the code
  - Outdated decisions that have been reversed
```

The project context document is loaded as persistent_facts by all agents at activation time. It is authored collaboratively and updated whenever a non-obvious fact surfaces during development. Each entry should be self-explanatory without reading the referenced code.

### 4.13 Multi-platform rule distribution

Agent constitutions and skill rules must work across multiple host platforms (Claude Code, Codex, IDE extensions, MCP clients). The distribution pattern keeps a single canonical source and deploys thin, platform-specific adapters from it — so a rule change propagates everywhere by updating one file.

#### Canonical source

The authoritative rule content lives in one location:

```text
[REFERENCE]
Canonical source: <ws>/constitution/SOUL.md (for workspace-scoped persona)
                  skills/<skill-name>/SKILL.md (for deployable skill rules)

Properties of the canonical source:
  - Platform-agnostic Markdown — no host-specific syntax
  - Mode-marker annotations for intensity filtering (e.g. [[lite]], [[ultra]])
  - Single source of truth for all adapter copies
  - Human-editable. SOUL.md has no agent write path at all (§4.25) — the agent may only request
    a change; a SKILL.md source changes through a pending revision (l2-learning-loop §4.1)
```

#### Adapter types

Each host platform receives the canonical rules via the appropriate adapter:

| Adapter type | Target host | Mechanism | Example path |
| --- | --- | --- | --- |
| Skill plugin | Plugin-capable hosts | Skills directory + lifecycle hooks | `skills/<name>/SKILL.md` |
| Extension manifest | Extension-based hosts | `extension.json` auto-discovers skills + commands | `gemini-extension.json` |
| Instruction-only copy | Rules-file hosts | Copy rule text to host-specific path | `.cursor/rules/`, `.clinerules/` |
| Steering rules | Steering-file hosts | Copy to steering directory | `.kiro/steering/<name>.md` |
| MCP prompt/tool | MCP clients | Serve rule via MCP prompt or tool interface | `mcp-server/index.js` |
| AGENTS.md | Multi-agent hosts | Drop-in file auto-loaded by host | `AGENTS.md` at repo root |

#### Alignment verification

When the canonical source changes, all adapter copies must be re-synced. An alignment check catches drift before it causes platform-specific behavior divergence:

```text
[REFERENCE]
Alignment check procedure:

1. Compute a hash of the canonical source (SOUL.md or skill SKILL.md).
2. For each adapter copy, compute the same hash on its rule content section
   (stripping host-specific wrapper/frontmatter).
3. Report any adapter whose hash differs from the canonical hash.
4. Copies that cannot be auto-synced (e.g. host-injected wrapper) report drift
   with the changed lines — not a hard failure, but a required human review.

Run: node scripts/check-rule-copies.js
Output: "N adapters in sync. M adapters drifted: [list]"
```

The alignment check runs as part of the pre-commit hook and the CI/CD quality gate. Drifted adapters do not block the commit but produce a visible warning.

#### Mode-marker filtering

The canonical skill file may carry mode-specific sections that get filtered per adapter at load time:

```text
[REFERENCE]
Mode marker syntax in SKILL.md / SOUL.md:

  [[lite]]   — include this line/section only when mode=lite or broader
  [[full]]   — include this line/section only when mode=full or broader (default)
  [[ultra]]  — include this line/section only when mode=ultra

Filtering rules:
  - Lines with no marker: always included (universal rules)
  - Lines with [[lite]]:  included in lite, full, and ultra
  - Lines with [[full]]:  included in full and ultra only
  - Lines with [[ultra]]: included in ultra only
  - Frontmatter (YAML block): always stripped before delivery
```

This allows a single SKILL.md to express the full behavior spectrum without maintaining separate files per mode.

### 4.14 Profile delivery modes

A **profile** selects which capabilities to distribute; a **delivery mode** controls which adapter types receive those capabilities. Together they let a workspace ship exactly the right set of rules to exactly the right tools, without manual per-platform editing.

#### Profiles

A profile is a named collection of skills and workflows. The workspace picks one:

| Profile | Included capabilities |
| --- | --- |
| `minimal` | Core session start only — SOUL.md + PROFILE.md loaded, no skill injection |
| `standard` | Core + workflow commands (propose, plan, apply, verify, archive) |
| `full` | Standard + adversarial review, goal-backward verification, retrospective triggers |
| `custom` | User-selected subset declared in workspace config |

The profile determines which SKILL.md files are compiled into adapter outputs and which workflow commands are generated. Switching profiles regenerates all adapter files from the canonical source.

#### Delivery modes

Delivery mode controls which adapter file types are generated:

| Mode | Generated adapters | Use when |
| --- | --- | --- |
| `skills` | Skills directory files only (`skills/*.md`) | Host supports skill-based context injection (Claude Code, Codex) |
| `commands` | Command prompt files only (e.g., `.github/prompts/*.md`) | Host supports command-palette only (GitHub Copilot) |
| `both` | Skills + commands (default) | Host supports both (most environments) |

#### Installation scope

```text
[REFERENCE]
Installation scope controls where adapter files are written:

  global:  ~/.local/share/cronus/skills/  (user-level, shared across all projects)
           Use when: you want Cronus skills available everywhere

  project: <ws>/skills/  (workspace-local, versioned with the project)
           Use when: project-specific persona or rules must not leak to other workspaces

  both:    Install to project; symlink to global (requires explicit opt-in)
           Use when: want local customization AND global availability
```

#### Profile config schema

```text
[REFERENCE]
Profile configuration (in <ws>/config.json or global ~/.cronus/config.json):

{
  "profile": "standard",           // minimal | standard | full | custom
  "delivery": "both",              // skills | commands | both
  "installScope": "project",       // global | project | both
  "customSkills": [],              // for profile=custom: list of skill IDs to include
  "excludeSkills": []              // for any profile: skill IDs to omit
}
```

#### Regeneration trigger

When profile or delivery changes, all adapter files must be regenerated:

```text
[REFERENCE]
cronus install --profile full --delivery both --scope project
  → Reads canonical sources (SOUL.md, skills/*.md, workflows/*.md)
  → Applies mode-marker filtering for current operation mode
  → Writes all adapter files to the correct locations
  → Runs alignment check (§4.13) to verify no drift

cronus install --check
  → Reads current adapter files
  → Compares against current canonical sources
  → Reports: "N adapters in sync. M adapters drifted."
```

### 4.15 Lifecycle hooks for workflow integration

Adapters (§4.13) and profiles (§4.14) describe what to install, but not when to act. Lifecycle hooks let extensions declare side effects that run before or after core Cronus workflow commands without the core knowing about the extension.

#### Hook declaration

Extensions and profiles declare hooks in their manifest under a `hooks:` key:

```yaml
hooks:
  after_spec:
    command: cronus context refresh
    optional: true
    description: "Refresh agent context file after spec changes."
  before_mission:
    command: cronus git commit --scope=pre-mission
    optional: true
    prompt: "Commit outstanding changes before mission starts?"
```

`optional: true` — skip if the command exits non-zero or is unavailable. `prompt:` — ask the user before running; omit for silent hooks.

#### Available hook points

| Point | Fires |
| --- | --- |
| `before_spec` / `after_spec` | Before/after spec document creation or update |
| `before_plan` / `after_plan` | Before/after plan generation |
| `before_mission` / `after_mission` | Before/after a full mission run |
| `before_task` / `after_task` | Before/after a single task execution |
| `before_review` / `after_review` | Before/after a quality pipeline review pass |
| `before_install` / `after_install` | Before/after `cronus install` |

#### Execution order

Multiple hooks registered for the same point run in declaration order (profile hooks before extension hooks). A non-optional hook that exits non-zero aborts the workflow at that point; optional hooks are logged and skipped.

#### Hook environment

```bash
CRONUS_WORKSPACE=<path>      # current workspace root
CRONUS_MODE=<lite|full|ultra>
CRONUS_HOOK_POINT=<name>     # e.g. "after_spec"
```

Hooks MUST NOT write to `.design/` unless they are engine-improvement tools with explicit write permission.

### 4.16 Managed-marker agent context injection

The multi-platform adapter (§4.13) copies or generates adapter files (CLAUDE.md, AGENTS.md, etc.). A full overwrite destroys user customization. Managed markers solve this: the adapter writes only its section, demarcated by sentinel lines, leaving the rest of the file untouched.

#### Marker format

```
<!-- CRONUS:START -->
[Agent instructions managed by Cronus — do not edit this block manually]
...generated content...
<!-- CRONUS:END -->
```

The sentinel prefix is configurable per adapter in `config.json`:

```json
{
  "adapters": {
    "claude_md": {
      "context_file": "CLAUDE.md",
      "marker_prefix": "CRONUS"
    }
  }
}
```

#### Injection rules

1. **First install**: if the context file does not exist, the adapter creates it with only the managed block.
2. **Update**: the adapter locates `<!-- {prefix}:START -->` and `<!-- {prefix}:END -->`, replaces the content between them, leaves everything outside untouched.
3. **Mismatched sentinels**: if only one sentinel is found, the adapter emits a warning and aborts — no partial writes.
4. **No sentinels in existing file**: the adapter appends the managed block at the end of the file.

#### Manual section policy

Content outside the managed block is the user's responsibility. The adapter NEVER reads, parses, or modifies it. Conflicts between the managed block and user content are the user's responsibility to resolve.

#### Multi-file support

A single install pass may inject into multiple context files (e.g., `CLAUDE.md` and `AGENTS.md`) using separate adapter entries with distinct `marker_prefix` values to avoid collisions.

### 4.17 Agentic readiness checklist

Before the Cronus spec system can run reliably in a workspace, a set of prerequisite artifacts must be in place. The agentic readiness checklist audits the workspace and emits a score with remediation steps.

#### Eight readiness signals

| Signal | Artifact | Severity |
| --- | --- | --- |
| Context file | CLAUDE.md / AGENTS.md / project-context.md present | critical |
| Custom skills | ≥1 `.skill.md` or skill directory entry present | warning |
| Agent definitions | AGENTS.md or persona config present | warning |
| Prompt templates | ≥1 `.prompt.md` template present | info |
| Lifecycle hooks | `hooks:` defined in at least one profile or extension manifest | warning |
| Isolated runtime | `.devcontainer/devcontainer.json` or equivalent sandbox configured | warning |
| Tool integrations | ≥1 MCP server configured | info |
| Freshness | Context file modified within the last 30 days | critical |

#### Score formula

- Each `critical` signal: 20 points (2 signals × 20 = 40 max)
- Each `warning` signal: 12 points (4 signals × 12 = 48 max)
- Each `info` signal: 6 points (2 signals × 6 = 12 max)
- Total: 100 points maximum

| Range | Readiness |
| --- | --- |
| 80–100 | Ready |
| 50–79 | Partial — agentic features may behave unexpectedly |
| <50 | Not ready — core signals missing |

#### Running the check

```text
cronus workspace check
```

Output:

```text
Agentic Readiness Score: 72/100 (Partial)

✓ Context file          (CLAUDE.md)
✗ Custom skills         — add .skill.md files or install a skill profile
✓ Agent definitions     (AGENTS.md)
✗ Prompt templates      — add .prompt.md templates for proposals/clarifications
✓ Lifecycle hooks       (after_spec, before_mission defined)
✗ Isolated runtime      — configure .devcontainer or sandbox
✓ Tool integrations     (3 MCP servers)
✓ Freshness             (CLAUDE.md updated 3 days ago)
```

#### Remediation

Each failed signal includes a one-line fix:

- `custom skills` → `cronus install --profile=minimal`
- `isolated runtime` → `cronus workspace init-devcontainer`
- `prompt templates` → `cronus install --templates=standard`

The checklist runs automatically on `cronus install` and on the first execution in a new workspace.

### 4.18 Cognitive frame activation and preamble tiers

Agent constitutions (§4.8) define persona. Skills define steps. A missing layer is the *cognitive frame* — the mental model a specialist activates before working. Frames scale better than checklists because they invoke latent knowledge; naming a framework activates the hundreds of sub-rules the model already has for it.

#### Cognitive frames vs checklists

| Approach | Example | Problem |
| --- | --- | --- |
| Checklist | "Check for string interpolation in SQL queries" | Misses novel patterns; enumerates the obvious |
| Cognitive frame | "Apply the OWASP Injection lens — what patterns does it teach?" | Activates deep training knowledge; generalizes |

The cognitive frame does not replace steps — it sets the mental posture before the steps run.

#### Cognitive frame format

Each skill or role entry in `AGENTS.md` (or the equivalent adapter) includes a `cognitive_frame:` field:

```yaml
- name: "security-auditor"
  role: "Chief Security Officer"
  cognitive_frame: >
    Internalize the adversarial mindset: what is the real attack surface?
    What patterns do professional penetration testers target first?
    Apply genuine adversarial thinking — not a checklist.

- name: "architecture-reviewer"
  role: "Architecture reviewer"
  cognitive_frame: >
    Internalize: what are the hidden state machines here? Where are the
    implicit assumptions that will break at 10× current load?
    Apply engineering judgment, not a feature checklist.
```

#### Preamble tier hierarchy

Skills run in a defined order based on tier. Lower tiers provide context for higher tiers:

| Tier | Purpose | Examples |
| --- | --- | --- |
| 1 — Foundational | Workspace init, context recovery, config loading | workspace-check, learnings-journal, context-recovery |
| 2 — Specialist | Domain-specific work with full context | mission-clarify, security-audit, arch-review |
| 3 — Orchestration | Sequences tier-2 skills; requires all prior context | mission-run, multi-phase-research, autoplan |

A tier-3 skill that runs before tier-1 completes will miss recovered context. The orchestrator enforces tier ordering.

#### Taste decision gate

Some choices are genuinely subjective (naming, visual design, scope boundaries). The agent flags these rather than resolving them autonomously:

```text
[TASTE] Two approaches are defensible:
  A) Include mobile layout now — consistent with existing pages.
  B) Defer mobile layout — outside this phase's P1 scope.
  Recommendation: A (maintains consistency). Both are valid.
  (Override: state your preference to proceed.)
```

Taste decisions are recorded in CONTEXT.md under `## Taste Decisions` — not in the main D-NN decision log, since they are not architectural.

### 4.19 Writing style protocol

Technical accuracy is necessary but not sufficient — prose that is accurate but jargon-heavy or impact-free fails to communicate. A writing style protocol makes agent prose consistent and actionable.

#### Jargon glossing

Technical terms are glossed on first use **per skill invocation**:

```text
Format: "Term (definition — what it means for you)"

Correct: "Check for N+1 queries (database makes 1+N round-trips instead
         of batching; causes slowdowns under load)."
Incorrect: "Check for N+1 queries."
```

Glossing applies to: AskUserQuestion options, finding descriptions, plan rationale, decision records.  
Glossing is skipped when: the user has demonstrated familiarity in this session; operation mode is `lite`.

#### Outcome framing

Every AskUserQuestion that presents options MUST include one concrete outcome sentence per option:

```text
Correct: "Option A ships 2 weeks faster but skips the migration rollback.
         Option B adds 2 weeks but allows rollback in the first 30 days."
Incorrect: "Do you want option A or B?"
```

Frame choices around **pain avoided** or **capability unlocked**, not around implementation mechanics.

#### User-impact closure

Findings and decisions end with what the user **sees, waits for, loses, or gains**:

```text
Correct: "This fix: users no longer see 'session expired' errors when switching tabs;
         session stays alive for 8 hours."
Incorrect: "This fix improves session handling."
```

#### Tone

Builder-to-builder: concrete, direct, no hype. Active voice. Short sentences. Concrete nouns.

Avoid: *delve*, *crucial*, *robust*, *comprehensive*, *nuanced*, *multifaceted*, *moreover*, *furthermore*, *pivotal*, *landscape*, *underscore*, *foster*, *showcase*, *intricate*, *vibrant*, *fundamental*.

### 4.20 Multi-session context disambiguation

When multiple agent sessions run simultaneously across different terminals or workspaces, clarification prompts lose context — the user cannot immediately tell which project, branch, or task the question belongs to.

**Rule:** When the session count in `.planning/sessions/` (files modified in the last two hours) reaches three or more, every `AskUserQuestion` call prepends a one-line context header to the question text.

**Header format:**

```
[Session {letter}] ./{repo-slug} @ {branch} | {current-task-summary}
```

**Example:**

```
[Session B] ./cronus @ feature-api-redesign | Reviewing data model changes
Which migration approach should be used?
```

**Implementation details:**

- Session files: `.planning/sessions/{ppid}.json` — created at skill start, updated on each tool call.
- Session count: number of `.json` files modified within the last 120 minutes.
- Letter assignment: alphabetical by file modification time (`A` = oldest active session).
- `current-task-summary`: first 60 characters of the active TASKS.md row's `title` field; falls back to the skill name when TASKS.md is absent.

**Threshold:** Three or more concurrent sessions activates context headers. Below three, headers are omitted — they add noise when only one session is active.

### 4.21 Question tuning and preference learning

Repeated clarification prompts on already-decided questions add friction and erode trust in the workflow. A preference store captures recurring decision patterns and suppresses redundant prompts automatically.

**Preference store:** `.planning/question-preferences.jsonl`

**Entry schema:**

```json
{
  "question_key": "checkpoint-mode-confirm",
  "skill": "mission",
  "response_pattern": "always-same",
  "preferred_value": "proceed",
  "occurrences": 4,
  "suppressed": true,
  "suppressed_at": "2026-06-01T10:00:00Z"
}
```

**What may be tuned:** only ordinary clarification and preference questions. A question that is an approval, a consent, or an authority decision — permit an action, relax a setting, trust a folder, accept a staged write — is **never** learned: SEC-9(a) makes a standing grant an explicit human act, never one inferred from repeated answers, so those keys are excluded from the store and always asked.

**Suppression rule:** When the same `question_key` receives an identical response in three or more consecutive occurrences, the entry is marked `suppressed: true`. The preferred value is applied automatically, and a one-line notice is emitted:

```
[Preference applied] "checkpoint-mode-confirm" → "proceed" (4× same response; run `cronus workspace tune` to reset)
```

**Commands:**

- `cronus workspace tune` — interactive review of preference entries; toggle suppression, reset counters, delete entries.
- `cronus workspace tune --reset <key>` — remove suppression for a specific key without interactive mode.
- `--ask-all` flag on any skill invocation: re-asks all suppressed questions for that session only.

**Scope:** Preferences are per-workspace (stored in `.planning/`) and do not cross-contaminate other workspaces. A `global_preferences.jsonl` in the home workspace defines defaults that local workspaces inherit but can override.

### 4.22 Model overlay system

Different AI models have distinct behavioral tendencies — verbosity, tool discipline, uncertainty expression, and interaction style. Hardcoding model-specific instructions in every skill creates maintenance sprawl. A model overlay file centralizes per-model behavioral patches.

**Configuration:** `config.json` → `model_overlays: { "claude": "overlays/claude.md", "gemini": "overlays/gemini.md" }`

**Overlay file structure (`overlays/{model}.md`):**

Three optional sections — include only what differs from default behavior for this model:

```markdown
## Tool Discipline
[model-specific rules for tool selection and ordering]

## Explanation Style
[verbosity and framing adjustments]

## Interaction Style
[question-asking, confirmation, and self-correction behavior]
```

**Activation:** The preamble reads `CRONUS_MODEL` from the environment and injects the matching overlay as a conditional prose block. If no overlay exists for the detected model, the section is silently skipped.

**Precedence:** Model overlay < skill-level instructions < user's inline instruction for the current message.

**Overlay as conditional prose:** The agent reads the overlay and applies it as a behavioral constraint for the session. No runtime branching is required — the prose is the instruction.

### 4.23 Three layers of knowledge (epistemological framework)

Agents applying only conventional patterns miss solutions that are novel, domain-specific, or that require questioning received wisdom. A three-layer reasoning protocol makes the epistemological tier explicit.

**Layer definitions:**

| Layer | Name | Source | Default trust | When to use |
| --- | --- | --- | --- | --- |
| 1 | Tried-and-true | Battle-tested patterns, standard library, documented best practices | High | First resort — stable, low-risk |
| 2 | New-and-popular | Recent posts, current ecosystem trends, community norms | Medium — scrutinize | When Layer 1 has no answer; verify independently before applying |
| 3 | First principles | Original analysis derived from the specific problem constraints | Self-verified | When Layers 1 and 2 conflict or both fail |

**Application:**

- When making an architectural or design decision, the agent labels which layer it draws from.
- Layer 2 reasoning includes an explicit scrutiny note: "This is currently popular — verifying it holds here because [reason]."
- Layer 3 reasoning includes the derivation chain: "From first principles: [constraint A] + [constraint B] → [conclusion]."

**Eureka moments:** When Layer 3 analysis correctly contradicts a Layer 1 or Layer 2 assumption, the finding is logged as a learning entry (§4.13 of `l2-self-improvement.md`) with `pattern_matched: "first-principles-correction"`.

**Anti-pattern:** Quoting Layer 2 sources without scrutiny ("the community recommends X") is flagged in spec review as insufficient reasoning depth.

### 4.24 Proactive drafting posture

Agents that ask questions before acting slow down workflows and transfer cognitive load back to the user. The default posture is to produce a complete draft (or the next concrete artifact) and present it for review, not to request permission before starting.

**Rule:** An agent MUST produce a complete draft before asking a question. Questions are reserved for genuine blockers where the output cannot proceed without user input.

**Blocker conditions** (the only valid reasons to pause before generating):

1. The target is ambiguous and two plausible interpretations would produce incompatible outputs.
2. Multiple contradictory requirements exist and resolving them requires human intent.
3. A required input is absent: missing file, missing credential, undefined variable that has no reasonable default.

**Question budget:** At most two clarifying questions per task. When a third would be needed, the agent makes a reasonable assumption, documents it inline, and proceeds.

**Optimistic default:** When a reasonable interpretation exists, take it. Document the assumption in the output as a single sentence: "Assumed {X} because {Y} — update the input if you meant {Z}." Never more than one such note per artifact.

**Draft-first formats:**

- **Spec or plan:** Write the full section, then append a one-line assumption note if any.
- **Code:** Write the implementation, then a brief rationale comment if the choice is non-obvious.
- **Report:** Write the complete report, then invite feedback at the end.

**Anti-pattern:** Prefacing with "Before I start, I have a few questions…" is always wrong. Generate first; ask only if the above blocker conditions are met after trying.

### 4.25 Standing-instruction files are a persistence sink [ADDED v1.1.0]

The constitution files are read at the start of *every* session and treated as the agent's own working principles, so text written into one of them is obeyed by every future run, long after the conversation that produced it is gone. The strongest form of prompt-injection persistence is therefore not a hostile page in this session — it is a hostile sentence the agent was induced to **save**. A write to one of these files is a **privileged sink** (`l1-action-gating` AG-10).

| File | Who may write | Rule |
| --- | --- | --- |
| SOUL.md, HEARTBEAT.md | the human only | the agent has no write path (SEC-10); a change it wants is a *request* — data, not a write |
| PROFILE.md, MEMORY.md | the agent, per §2 | ordinary while the write's provenance is clean; **staged** when it is not (below) |
| BOOTSTRAP.md | the first-run ritual | deletes itself; not an agent write target thereafter |

```text
[REFERENCE]
write_constitution(file, new_text, actor):
    if file in {SOUL, HEARTBEAT} and actor is the agent:
        return request_change(file, new_text)              // SEC-10: recorded, surfaced to the human, never applied by the agent
    if provenance_of(new_text) is untrusted_or_unknown:    // AG-10: the runtime tracks the derivation (CP-4); the model does not declare it
        stage(<ws>/constitution/.pending/<file>.<n>, new_text)   // the pending shape of l2-learning-loop's skill proposals
        return staged                                      // unattended callers stage without asking (AG-9): no prompt, no hang, no silent write
    write_then_rename(file, new_text); audit(who, when, derived_from)   // AG-7
```

Acceptance of a staged write is a human act on **that exact content** (CB-1): it is shown with its source, and accepting it applies it; a later, different staged text is a new decision. The session-start load (§4.7) never reads `.pending/`. The distribution of rules into *other tools'* instruction files (§4.13) and managed-marker injection (§4.16) are installation-locus actions the human starts from the command line; they are not tools the agent can call.

## 5. Drawbacks & Alternatives

- **Plaintext size vs. structured memory:** PROFILE.md and MEMORY.md grow unbounded. Long-term, entries should be migrated to the structured memory store; the constitution files remain the user-visible identity layer.
- **Concurrent writes (multi-agent):** two agents updating PROFILE.md simultaneously could cause lost updates. Mitigation: the `manager` role (orchestrator) is the designated writer; others read-only unless specifically granted write access.
- **SOUL.md drift:** if the user edits SOUL.md, the agent's behavior changes on next session. This is by design — SOUL.md is a first-class configuration surface.
- **Staging costs a review step (v1.1.0):** a session that has read untrusted material and then wants to remember something must wait for a person. That is the price of not letting a saved sentence outlive its source; clean-provenance updates — the common case — are unaffected, and reading, summarizing and reasoning over untrusted content never stage anything (AG-10(c)).
- **Alternative — store identity in structured DB:** more queryable, but not user-editable without tooling. Plain files win for transparency and auditability.
- **Disclosed simplification (FR-6)** `[ADDED]`: the 8-step activation sequence currently executes as a no-op seam — constitution files are read and validated, but activation performs no session side effects until agent-session wiring lands. Upgrade trigger: binding activation to session start in the session subsystem; the sequence contract is unchanged.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[OFFICE]` | `.design/main/specifications/l1-office-model.md` | Persona and role model |
| `[MEMORY]` | `.design/main/specifications/l1-memory-model.md` | Memory scopes and lifecycle |
| `[LAYOUT]` | `.design/main/specifications/l2-filesystem-layout.md` | Workspace layout |
| `[SCHED]` | `.design/main/specifications/l2-scheduler.md` | Heartbeat schedule action |

## Document History

| Version | Date | Notes |
| --- | --- | --- |
| 1.1.1 | 2026-09-23 | Consistency pass (2026-09-23): Compliance cited a nonexistent `MOD-1` — now OFF-1, with OFF-9 added. §4.10 named a reference framework's own config path — replaced by the workspace configuration. §4.13 let the agent append to SOUL.md, contradicting §4.25 (human-only). §4.21 question tuning auto-applied an answer after three identical replies, which for approvals would be an inferred standing grant (SEC-9(a)) — approvals, consents and authority decisions are excluded. MEMORY.md `read_when` was "bootstrapping" only, so the cheat sheet never loaded — now session start. Project-tier customization layers and persistent facts load only in a trusted workspace (`l2-security` §4.8). |
| 1.1.0 | 2026-09-19 | New §4.25 — standing-instruction files are a persistence sink. These files are loaded as the agent's own principles at every session start, so a sentence saved into one outlives the conversation and the source that induced it; a write to one is a privileged sink under `l1-action-gating` AG-10. SOUL.md and HEARTBEAT.md are human-written only (no agent write path, SEC-10; a wanted change is a request, which is data); an agent write to PROFILE.md or MEMORY.md derived from untrusted content is staged under `.pending/` for human acceptance of that exact content, unattended callers stage without asking (AG-9), and clean-provenance updates are unchanged. Rule distribution into other tools' files (§4.13) and managed-marker injection (§4.16) recorded as human-initiated installation actions the agent cannot call. §2 and §3 updated; SEC-10 and AG-10 rows added. Distilled from a cross-check of eight external agent command-line tools: protecting the files that carry standing instructions from agent writes, as a prompt-injection persistence vector, was found in the most security-mature of them. |
| 1.0.10 | 2026-07-16 | Disclosed simplification (FR-6) recorded in §5: the 8-step activation sequence executes as a no-op seam pending agent-session wiring; upgrade trigger = binding activation to session start. History table added with this entry. |
