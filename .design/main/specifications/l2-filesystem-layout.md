# Filesystem Layout (OS-native)

**Version:** 1.6.0
**Status:** Stable
**Layer:** implementation
**Implements:** l1-storage-model.md, l1-state-authority.md, l1-artifact-reach.md

## Overview

The concrete on-disk realization of the storage model: OS-native install locations for the two tiers, the directory trees of the program and state tiers, the placement of databases (SQLite + sqlite-vec), and the mapping of memory levels to paths. A visualization stub of both trees may exist in the repository as a temporary discussion sandbox; it carries no authority (§4.5). §4.6 maps each kind of state in the tier to its single authority.

## Related Specifications

- [l1-storage-model.md](l1-storage-model.md) - The model this layout implements.
- [l2-technology-stack.md](l2-technology-stack.md) - SQLite + sqlite-vec; optional libSQL/PostgreSQL sync.
- [l2-core-library.md](l2-core-library.md) - The core resolves these paths and owns persistence.
- [l2-skill-system.md](l2-skill-system.md) - The two-tier skill stores rooted at `<program>/skills/` and `<state>/skills/`.
- [l1-state-authority.md](l1-state-authority.md) - [ADDED v1.3.0] the contract §4.6 realizes: one authority per kind of state, three authority classes, recovery from authority to projection only.
- [l1-multi-device-sync.md](l1-multi-device-sync.md) - [ADDED v1.3.0] SY-10: which classes may leave the device over which transport; §4.3's optional tracked subset is its realization point.
- [l2-backup.md](l2-backup.md) and [l2-doctor.md](l2-doctor.md) - [ADDED v1.3.0] the two tools that consult §4.6 by kind: backup includes every authority, the doctor re-derives projections only.
- [l1-artifact-reach.md](l1-artifact-reach.md) - [ADDED v1.4.0] the contract §4.7 realizes: every kind declares how far it travels — project-shared, personal, machine-local, never-egress — and tools decide by the declaration, not by the path.
- [l2-program-lifecycle.md](l2-program-lifecycle.md) - [ADDED v1.6.0] Installation routes, the per-user program default and the update's kept previous release (§4.1, §4.2).
- [l2-diagnostic-log.md](l2-diagnostic-log.md) - [ADDED v1.6.0] What is written under `<logs>`.

## 1. Motivation

The model demands two separated tiers and scoped memory; this spec pins exactly where they live per OS and how the directories are shaped, so implementation and packaging are unambiguous.

## 2. Constraints & Assumptions

- Default deployment is **OS-native** (tiers in their conventional locations); a portable mode (both under one directory) is also supported.
- Cache and logs are placed in OS-specific cache/state locations, outside the main state tier.
- Databases are created at runtime; the repository ships only empty stubs and READMEs.

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| STO-1 Two-tier separation | Program tier under the OS program location; state tier under the OS app-data location — distinct roots. |
| STO-2 Durable, restartable state | State tier holds SQLite files + text; runtime rehydrates from them on launch. |
| STO-3 Catalog vs instance | `<program>/employees/` (catalog) and `<program>/templates/` are read-only; hiring/init copies into `<state>/employees/` and `<state>/workspaces/`. |
| STO-4 Multi-level memory | Paths per level: global `<state>/memory/`; workspace `<state>/workspaces/<ws>/memory/`+`graph/`; employee `<state>/employees/<role>/memory/`; session `<state>/workspaces/<ws>/sessions/`. |
| STO-5 Scope-bound lifecycle | Deleting an office/role directory removes its memory; sessions pruned in place; global persists. |
| STO-6 Secret isolation | Secrets in `<state>/.env` (template `.env.example`); excluded from backups and version control. |
| STO-7 Restore-by-copy | Copying `<state>/` minus `.env` and cache restores the system. **Partial** for project-local state roots (§4.3): until they are registered with the state tier, a copy of `<state>/` does not include them. |
| STO-8 Human-inspectable state | Config as JSON, rules/notes/STATE as Markdown. Which copy is the truth is declared per kind (§4.6): text-authoritative kinds keep derived indices beside them; store-authoritative and record-authoritative kinds keep a human-readable export, a projection that is never an input to a rebuild. |
| STO-9 Versioned state with forward migration | **Partial.** Session files carry a version and migrate forward on load (`l2-agent-session` §4.15). Every SQLite store records its schema in the database's own `user_version` header, and opening a file does exactly one of four things: a fresh file gets the baseline schema and is stamped; a file from before versioning is adopted as the current version, the baseline being idempotent; an older versioned file runs its ordered migration steps — the path validated before anything is touched, each step in its own transaction together with its stamp — after a consistent copy of the file is taken, since a migration is one-way; and a newer versioned file is refused, never read optimistically or written into. The droppable caches (the project wiki index, the code graph) follow the same refuse-newer rule. The JSON config files carry no version yet — a version marker on each, the refuse-unknown-shape load, and the backup before a destructive rewrite are pending for them. |
| SAU-1 One authority per kind | §4.6 lists each kind of state in the tier with its single authority; the owning specification declares it. |
| SAU-2 Projection or another object | §4.6 names each kind's projections; `wiki.db`, the code-intelligence index and the search and vector indices are marked projections and are never edited as truth. |
| SAU-3 Declared class | The §4.6 *Class* column: text-, store- or record-authoritative, or not an authority. |
| SAU-4 Recovery direction | The §4.6 *Recovery* column. **Pending.** The rule is specified in `l2-doctor` §4.1; checking the repair code against this table is not done. |
| SAU-5 Tools decide by class | `l2-backup` §4.1 includes every authority and excludes projections by this table; `l2-doctor` repairs projections only. **Pending.** Same as SAU-4: the specifications agree, the code has not been checked against the table. |
| SAU-6 The owner declares; the map indexes | Each §4.6 row cites the invariant that declares it; a row whose owner is silent is marked *proposed*, which is a finding against the owner. |
| SAU-7 A move of authority is explicit and swept | The one past move — learned memory from notes to a database (MEM-4) — is reflected in the STO-8 row above and in §4.6; a future move follows the sweep of `l1-state-authority` §4.4. |
| SAU-8 A projection names its derivation | Realized per projection by its owner (`l2-knowledge-store` §4.8, `l2-memory-store` §2, `l2-codegraph` §4.6); the §4.6 map lists the projections it applies to. |
| RCH-1 Every kind declares a reach | §4.7 lists each kind with its reach and declaring source. **Partial:** most rows are *proposed* — their owners declare an authority class but no reach yet. |
| RCH-2 Undeclared is machine-local | The project-local root's exclusion is `*` with named opt-ins (§4.3), so an undeclared kind is already kept out of the tracked set. |
| RCH-3 Reach is not authority | §4.7 is a separate column set from §4.6; no projection row is project-shared, and the code index's wiring is listed apart from the index itself. |
| RCH-4 Portability check | **Pending.** Nothing checks an artifact for secrets, absolute paths, identities or machine-local references before it enters the tracked set; the opted-in names are trusted as written. |
| RCH-5 Arrivals untrusted until trusted | Committed project configuration is loaded only after the workspace trust decision (`l2-security` §4.8), as §4.3 states. **Partial:** skills, agent definitions and workflows declared project-shared in §4.7 are not yet routed through that decision. |
| RCH-6 Consented widening | The tracked-subset opt-in of §4.3 (SY-10). **Pending:** the settings surface (the §4.3 TBD). |
| RCH-7 Tools decide by declared reach | **Pending.** The exclusion `cronus init` writes (`crates/domain/src/state.rs`) is a fixed list (`*`, `!settings.json`, `!commands/`), not generated from §4.7; the target is `local/` plus temporary and cache patterns (§4.3.1). |
| RCH-8 The split is visible | **Pending.** No inspect verb lists the reach of the current project's kinds. |
| RCH-10 Three origins | §4.7 rows: built-in under `<program>/`, user-installed or authored under `<state>/` (converted), project-written under `.cronus/` in source form; resolution order and origin reporting are `l2-skill-system` §4.1's. **Pending** for agent definitions and workflows, which have no project store in code. |
| RCH-11 Source travels, converted form is local | Converted forms live in `.cronus/local/` (§4.3.1) and are never committed. **Pending:** no conversion of project-store artifacts exists yet. |
| RCH-12 Shared folder, excluded local area | §4.3.1. **Pending:** `init` still makes `.cronus/` the state root and writes `*` with opt-ins; the `local/` area, the readme, `.version` and the migration from the shipped layout are not built. |
| RCH-9 A reach change is swept | A reach change edits the owning specification, §4.7 and the exclusion generator in one revision; an existing exclusion file is left untouched by `init` (§4.3), so a changed reach is reported for an existing root rather than silently rewritten. |

## 4. Detailed Design

### 4.1 OS-native locations

| Tier | Windows | macOS | Linux (XDG) |
| --- | --- | --- | --- |
| Program (immutable) `[MODIFIED v1.6.0]` | `%LOCALAPPDATA%\Programs\Cronus\` (per user, default) or `%ProgramFiles%\Cronus\` (all users) | `~/Applications/Cronus.app/…` (per user) or `/Applications/Cronus.app/Contents/Resources/` | `~/.local/lib/cronus/` (per user) or `/opt/cronus/` |
| State (mutable) | `%APPDATA%\Cronus\` | `~/Library/Application Support/Cronus/` | `~/.local/share/cronus/` |
| Cache (regenerable) | `%LOCALAPPDATA%\Cronus\Cache\` | `~/Library/Caches/Cronus/` | `~/.cache/cronus/` |
| Logs / runtime | `%LOCALAPPDATA%\Cronus\Logs\` | `~/Library/Logs/Cronus/` | `~/.local/state/cronus/` |

A single path resolver in the core maps the abstract roots (`<program>`, `<state>`, `<cache>`, `<logs>`) to these per-OS paths; portable mode overrides them to one chosen directory. `[ADDED v1.6.0]` A per-user program location is the default because it needs no administrator rights to install or update (`l2-program-lifecycle` §4.1); the command `cronus` is registered on the person's search path from there.

### 4.2 Program tier tree

```plaintext
<program>/
├── bin/            # cronus — the one executable: CLI, terminal UI (bare `cronus`), always-on engine (`cronus serve`)
├── app/            # core engine library + desktop application shell
├── templates/      # employee/ , workspace/ (blueprints copied on init)
├── employees/      # read-only role catalog (CATALOG.md + role blueprints)
├── skills/         # [ADDED] read-only preset skill store (canonical packages)
├── extensions/     # [ADDED v1.6.0] read-only preset non-skill extensions: commands (prompt templates), MCP configs, plugins
├── languages/  themes/
├── install.json    # [ADDED v1.6.0] origin, scope, channel, pending switch (l2-program-lifecycle)
├── previous/       # [ADDED v1.6.0] the release kept for a way back during an update's grace period
└── VERSION
```

### 4.3 State tier tree

```plaintext
<state>/
├── .env                  # secrets (excluded from backup/VCS)
├── app.json  config.json  auth.json  channels.json  models.json  routing.json  gateway.json
├── AGENTS.md
├── memory/               # GLOBAL: global.db (SQLite+vec), graph.db, notes/
├── skills/               # [MODIFIED] mutable skill store: user-added + generated (canonical packages)
├── extensions/           # [ADDED v1.6.0] user-installed non-skill extensions: commands/, mcp/, plugins/
├── backups/              # [ADDED v1.6.0] default backup destination (l2-backup)
├── employees/<role>/     # EMPLOYEE: config.json, RULES.md, memory/, skills/, skins/
└── workspaces/<ws>/      # WORKSPACE (office)
    ├── config.json  RULES.md  STATE.md
    ├── memory/           #   workspace.db (SQLite+vec) + notes
    ├── graph/            #   graph.db (people/tasks/decisions/artifacts)
    ├── wiki/             #   wiki.db — client-facing projection CACHE (rebuildable, not source of truth)
    ├── sessions/         #   SESSION (episodic, pruned)
    ├── kanban/  office/  schedules/  hooks/  sandboxes/  snapshots/  dashboard/
    └── missions/  planning/  constitution/  extensions/  skills/
```

#### Path conventions used by other specifications

`<ws>/…` in any specification denotes the office's **state root**, and a bare `.planning/…`
denotes `<ws>/planning/…`. The state root is `<state>/workspaces/<ws>/` in the OS-native
tier, or — for a project initialized in place with `cronus init` — the **local area**
`.cronus/local/` of the project's own `.cronus/` folder, found through the nearest ancestor
of the working directory that holds a `.cronus/`, falling back to the OS tier. `[MODIFIED v1.5.0]`
As shipped the state root is `.cronus/` itself; §4.3.1 is the target layout. Wherever it lives, the office's planning and
run state — missions, checkpoints, proposals, clarifications, training runs — stays inside
the state root, never scattered among the project's tracked files. What an office
deliberately delivers *into* a project (code, documents) is the project's; the records of
how it got there are the office's. The one exception is an artifact a specification
deliberately makes part of the project's version-controlled history and names as such —
the development workflow's design documents and progress ledger (DW-5), committed to the
feature branch.

A project-local state root carries three obligations the OS-native tier meets by location
alone. It is excluded from the project's version control — the local area, not the whole
`.cronus/` folder (§4.3.1) — since office state is not project content and a checkout must
not carry sessions or memory with it. It never holds a secret —
those stay in the OS state tier's secret store (STO-6). And it is registered with the OS
state tier, so a backup can enumerate it; otherwise copying `<state>/` would silently leave
it behind (STO-7). **Partial:** `cronus init` creates the root and writes its version-control
exclusion — a self-ignoring `.gitignore` (`*`, so `init` leaves nothing in `git status`) that opts
`settings.json` and `commands/` back in by name and is left untouched when it already exists;
registering the root with the OS state tier is pending. Project *configuration*
committed on purpose (`.cronus/settings.json`, commands, skills) is a different thing, loaded
only after the workspace trust decision (`l2-security` §4.8).

#### 4.3.1 The project folder `[ADDED v1.5.0]`

Decided by the user (2026-10-22, `l1-artifact-reach` RCH-12): `.cronus/` in a project is the
**team's folder**, committed as a whole, and `.cronus/local/` is the office's working state for
that project, excluded from version control.

```plaintext
<project>/.cronus/
├── README.md          # project-shared: what Cronus is, what this folder is for, notes for collaborators
├── .version           # project-shared: the folder's format version (refuse-newer, migrate-older — STO-9)
├── .gitignore         # project-shared: excludes local/ and temporary/cache files (written by init)
├── settings.json      # project-shared: project settings
├── commands/          # project-shared: project commands
├── skills/            # project-shared: ordinary skills in source form (RCH-10/RCH-11)
├── agents/            # project-shared: agent definitions written for the project
├── workflows/         # project-shared: workflows written for the project
└── local/             # machine-local: the state root — sessions, memory, kanban, planning, indices,
                       #   converted forms of the project's skills/agents/workflows, locks, snapshots
```

The exclusion `init` writes is `local/` plus temporary and cache patterns, generated from the
§4.7 map (RCH-7), replacing today's `*`-with-opt-ins list. A collaborator's clone therefore
carries everything above `local/` and nothing of another machine. Moving an existing
project from the shipped layout (state root at `.cronus/`) to this one is a one-way, backed-up
migration of the state into `local/` keyed by `.version` (STO-9); an existing `.gitignore` is not
rewritten silently — the migration reports it and offers the new rule (RCH-9).

**Tracking a subset (`l1-multi-device-sync` SY-10).** The exclusion is the default, and configuration committed on purpose is already a tracked subset. A user may widen it: track a *declared subset* of office state in the project's own history so that a remote the user names carries it. Only kinds whose reconciliation a text-merging transport can carry faithfully are eligible — the reviewable-merge classes first, such as plans and decision records; a CRDT class rides it only as per-replica operation files — and secrets and live sessions never are. The consent names the remote and the subset, is visible and revocable, and lapses when either changes. The office commits locally at most; the push is the user's act or a standing consent they recorded for that remote, never a workspace's (the no-remote-git contract, `l2-execution-workspace` §4.4). `[MODIFIED v1.6.0]` The opt-in is a project setting, `share.office_state`, listing the kinds to track; turning it on asks for the consent that names the remote and the subset (RCH-6). The initial eligible subset is decision records, plan files and exports of authored memory — reviewable-merge kinds only.

**Where an office keeps its state is a per-project choice** `[ADDED v1.6.0]`. When a project is initialized or attached, `state.location` is chosen: `project` keeps the state root in `.cronus/local/` (the default for a project initialized in place), `user` keeps it in the OS state tier under `<state>/workspaces/<ws>/` and writes into the project only the shared `.cronus/` files the person opted into. Either way the shared part above `local/` is the same, and either way office work reaches collaborators only through the opt-in above. Changing the choice later moves the state root once, after a backup, and is reported; the old location is not left half-populated. **Pending:** the setting and the move.

### 4.4 Database placement (SQLite + sqlite-vec)

| Level | File | Engine |
| --- | --- | --- |
| Global | `<state>/memory/global.db`, `<state>/memory/graph.db` | SQLite + sqlite-vec |
| Workspace | `<state>/workspaces/<ws>/memory/workspace.db`, `<state>/workspaces/<ws>/graph/graph.db` | SQLite + sqlite-vec |
| Workspace wiki | `<state>/workspaces/<ws>/wiki/wiki.db` | SQLite + FTS5 (projection cache; rebuildable, droppable — see l2-project-wiki) |
| Employee | `<state>/employees/<role>/memory/employee.db` | SQLite + sqlite-vec |

Physical consolidation (one file with attached schemas vs separate files per level) is an implementation choice deferred to the build phase. Optional remote sync targets libSQL/PostgreSQL as replicas of the user's own state; the local tier remains the authority of each kind (SAU-1), and a remote is eligible only under `l1-multi-device-sync` SY-10.

### 4.5 Repository visualization stub

```plaintext
<stub-root>/
├── program/   # stub of the immutable program tier
└── state/     # stub of the mutable state tier (example office: workspaces/default)
```

A repository-level stub of both trees may exist as a temporary discussion sandbox; it is not a build artifact and carries no authority — this specification does. Where the stub and this specification disagree, the stub is stale.

### 4.6 State authority map

The path-level index of `l1-state-authority` (SAU-6). Each row is a kind of state, its single authority and class, what is derived from it, how it is recovered, and the invariant that declares it. The map adds no definition: a row whose owner is silent is marked *proposed*, which is a finding against the owner and not a class by default.

| Kind of state | Where | Class | Projections and other objects | Recovery | Declared by |
| --- | --- | --- | --- | --- | --- |
| Work state: the card and its transition history | `<ws>/kanban/cards/` | text-authoritative | board views, plan-unit status, the sprint tracking file, the office view, dashboards | restore from backup; views re-derive | KAN-1, CONV-10 |
| Card event and comment logs, per-attempt run records | `<ws>/kanban/events/`, `comments/`, `runs/` | record-authoritative (append-only) | activity feeds, cost-attribution views | restore; folds re-derive | `l2-kanban-board` §4.6–4.7 |
| Plan structure (planning contexts) | `<ws>/planning/` | text-authoritative | plan-unit statuses read from cards, tracking files | restore | TG-10, TG-11 |
| Learned memory | the per-scope memory databases | store-authoritative | FTS and vector indices; any text export | restore the database, re-derive the indices; never rebuild from the export | MEM-4 |
| Authored quick memory | `MEMORY.md` and `USER.md` per role | text-authoritative | search index | restore the file | MEM-4 |
| Deliberation log | the append-only `deliberation_log` table in the inbox database | record-authoritative | the Channels view | restore; exempt from delivery TTL pruning | DL-4 |
| Schedules | `<ws>/schedules/` | text-authoritative | the armed timer set (re-armed on start) | restore; re-arm | SCH-6 |
| Configuration | the state tier's JSON files | text-authoritative | resolved views | restore defaults or from backup | STO-8 |
| Sessions | `<ws>/sessions/` | record-authoritative (*proposed*; the owning specification is silent on class) | summaries, recall entries | prune by policy; restore | STO-5 |
| Secrets | the secret store, `<state>/.env` | the secret store | never in a projection, export, backup or tracked subset | re-enter | STO-6 |
| Project wiki | `<ws>/wiki/wiki.db` | not an authority | the page index | drop and rebuild on use | PW-3 |
| Code-intelligence index | the code-graph store (`l2-codegraph`) | not an authority | — | drop and re-extract | CI-3 |
| Relationship graph (deferred) | `<ws>/graph/graph.db`, the global `graph.db` | *proposed* — undeclared: whether it is derived from memory items or holds bi-temporal facts of its own is open, so it is treated as an authority and backed up until declared | — | restore from backup | `l2-memory-store` §4.5 (silent on class) |
| Office layout | `<ws>/office/layout.json` | text-authoritative for cosmetic placement only; the rendered graph is a projection | the interaction graph | delete to re-lay-out | OVZ-1, OVZ-7 |

A backup includes every authority and excludes projections by this map, except that a costly-to-rebuild projection may be included for rebuild cost (`l2-backup` §4.1). A repair re-derives projections only (`l2-doctor` §4.1): a damaged authority is reported with its restore path, never regenerated from a projection.

### 4.7 Artifact reach map `[ADDED v1.4.0]`

The path-level index of `l1-artifact-reach` (RCH-1). Reach says how far a kind may travel; it is independent of the §4.6 authority class. *Proposed* marks a reach this layout assigns because the owning specification is silent — a finding against the owner, and machine-local wherever doubt remains (RCH-2).

| Kind | Where (project-local root) | Reach | Declared by |
| --- | --- | --- | --- |
| Project settings | `.cronus/settings.json` | project-shared | §4.3 (opted in by `init`) |
| Project commands | `.cronus/commands/` | project-shared | §4.3 (opted in by `init`) |
| Collaborator readme, folder format version | `.cronus/README.md`, `.cronus/.version` | project-shared | RCH-12 |
| Project skills (source form) | `.cronus/skills/` | project-shared | RCH-10/RCH-11; `l2-skill-system` §4.1 |
| Agent definitions written for the project (source form) | `.cronus/agents/` | project-shared | RCH-10/RCH-11 |
| Workflows written for the project (source form) | `.cronus/workflows/` | project-shared | RCH-10/RCH-11 |
| Converted forms of the project's skills, agents and workflows | `.cronus/local/` | machine-local (a projection of the shared source) | RCH-11 |
| Built-in skills, roles, templates | `<program>/` | part of the release — neither committed nor synced | RCH-10; STO-1 |
| Installed or user-authored skills, agents, workflows (converted) | `<state>/skills/`, `<state>/extensions/`, `<state>/employees/` | personal | RCH-10 |
| Code-index integration wiring | the project's committed configuration | project-shared | CI-18 |
| Design documents and progress ledger | the feature branch | project-shared | DW-5 |
| Decision records, plan files (opt-in subset) | `<ws>/planning/` | personal by default; project-shared only under the SY-10 opt-in | SY-10 |
| Authored quick memory, preferences | `MEMORY.md`, `USER.md`, the state tier's JSON files | personal | MEM-4 (authority); reach *proposed* |
| Learned memory | the per-scope memory databases | personal | MEM-4 (authority); reach *proposed* |
| User-added skills | `<state>/skills/` | personal | `l2-skill-system` (reach *proposed*) |
| Sessions and journals | `<ws>/sessions/` | machine-local | STO-5; reach *proposed* |
| Board, card logs and run records | `<ws>/kanban/` | machine-local unless opted in under SY-10 | KAN-1 (authority); reach *proposed* |
| Every index and cache (wiki, code graph, vectors, FTS) | as §4.4 | machine-local | CI-18, PW-3, SAU-2 |
| Engine seat, endpoint record, locks | the state root | machine-local | `l2-engine-endpoint` |
| Sandboxes, snapshots, backups | `<ws>/sandboxes/`, `snapshots/`, the backup destination | machine-local | STO-7 |
| Secrets and keys | the secret store, `<state>/.env` | never-egress | STO-6 |

Every machine-local row resolves under `.cronus/local/` for a project-local root (§4.3.1). The exclusion file `cronus init` writes is to be generated from this table — `local/` plus temporary and cache patterns — rather than kept as a fixed list (RCH-7).

### 4.8 Reach of the OS state tier `[ADDED v1.4.0]`

The OS-native state tier (§4.3) holds no project-shared kind by construction: it is outside every project checkout. Its personal kinds travel only by multi-device sync or an opted-in backend; its machine-local kinds and secrets do not travel.

### 4.9 Path catalog and renames `[ADDED v1.6.0]`

The trees above are the stable skeleton; names inside them are expected to change while the product is young. Two rules keep a rename cheap:

1. **Every path the product writes is one entry** — an abstract root, a relative path and the specification that owns the kind — resolved by the core's path resolver. No code builds a tier path from string literals outside the resolver, so a rename edits one entry.
2. **A rename is a migration and a sweep.** The entry change ships with a forward migration step (STO-9) that moves the old path to the new one on first open, after the safety copy, and with an edit of every specification that names the path in the same revision — the same discipline a reach change follows (RCH-9). The old name is never read again after the migration, so two names for one kind cannot coexist.

Paths named by other specifications that the trees above do not show, and the open conflicts among them:

| Path | Owner | State |
| --- | --- | --- |
| `<state>/extensions/{commands,mcp,plugins}/`, `<program>/extensions/` | `l2-extension-registry` (EXT-5) | placed above |
| `<state>/backups/` | `l2-backup` | placed above |
| `<program>/install.json`, `<program>/previous/` | `l2-program-lifecycle` | placed above |
| `<logs>/<kind>.jsonl` | `l2-diagnostic-log` | under the `<logs>` root of §4.1 |
| `<state>/execution/`, `<state>/worktrees/<project_id>/` | `l2-execution-workspace` | not yet in the tree |
| `<state>/settings.json`, `<state>/telemetry.json`, `<state>/trusted_workspaces.json` | `l2-security` | not yet in the tree; `settings.json` beside `config.json` is a second configuration file whose split is unstated |
| `<state>/jobs.sqlite` | `l2-self-improvement` | not yet in the tree |
| `<state>/guardrails/` | `l2-tool-security` | not yet in the tree |
| `<state>/global-graph/` | `l2-codegraph` | not yet in the tree; overlaps the global `memory/graph.db` by name only |
| `<state>/router-pool.json` | `l2-model-router` | not yet in the tree |
| `<state>/sessions.json` | `l2-multi-user-auth` | not yet in the tree |
| `<state>/sessions/<session_id>/` | `l2-session-checkpoint` §4.1 | **conflict**: sessions are workspace-scoped (STO-4, §4.3 `<ws>/sessions/`); the checkpoint files belong at `<ws>/sessions/<session_id>/`, and the owner is to be swept |
| `<state>/snapshots/<project_hash>/` | `l2-session-checkpoint` §4.10 | **name collision**: file-level shadow repositories share the word with the workspace's crash-recovery `snapshots/`; a distinct name is due at the next sweep |

The repository's visualization stub (§4.5) differs in three places, and this specification wins in each: its project folder names the local area `data/` (here `local/`, the later decision); it keeps skills under `extensions/skills/` as well as at the tier top (here, and in `l2-skill-system`, only at the top); and it has a state-tier `benches/` directory that no owner declares.

## 5. Drawbacks & Alternatives

- **Per-OS path variance:** four location classes per OS add packaging complexity; mitigated by the central path resolver.
- **Strict XDG split (config/data/cache/state in four roots) vs consolidated state:** this spec consolidates mutable state under the data root for a simpler mental model, splitting only cache/logs. Strict XDG separation remains an option. <!-- TBD: confirm consolidated-state vs strict-XDG for Linux v0.1.0 -->
- **Alternative — portable-only:** simpler paths but poor OS integration; rejected as default, kept as a mode.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[MODEL]` | `.design/main/specifications/l1-storage-model.md` | Invariants this layout satisfies |
| `[STACK]` | `.design/main/specifications/l2-technology-stack.md` | Storage engine choices |
| `[AUTHORITY]` | `.design/main/specifications/l1-state-authority.md` | Authority classes the §4.6 map applies |

## Document History

| Version | Date | Notes |
| --- | --- | --- |
| 1.0.0 | 2026-06-24 | Initial stable spec — OS-native tier locations, program/state trees, database placement, repository visualization stub. |
| 1.1.0 | 2026-07-08 | `[ADDED]` `<program>/skills/` (read-only preset skill store) to the program tier tree; `[MODIFIED]` `<state>/skills/` comment to reflect the mutable skill store (user-added + generated canonical packages); Related Specifications link to the skill system spec. Additive — status remains Stable. |
| 1.2.0 | 2026-07-15 | `[ADDED]` `<state>/workspaces/<ws>/wiki/wiki.db` — the per-office project-wiki projection cache (SQLite + FTS5; rebuildable/droppable, not source of truth) to the workspace tree and the §4.4 database-placement table. Additive — status remains Stable. Realized by the new l2-project-wiki. |
| 1.2.1 | 2026-09-23 | Consistency pass (2026-09-23): STO-9 (versioned state with forward migration) was unmapped — Partial (session files versioned; SQLite and JSON state carry no schema version yet). Other specifications write `<ws>/…` and `.planning/…` for office state that this layout never placed — the convention is now defined (`<state>/workspaces/<ws>/`, planning tree in the state tier, never in the user's repository, per STO-7) and the workspace tree lists missions/, planning/, constitution/, extensions/, skills/. One graph path lacked its `<state>/workspaces/` prefix. The state-tier convention now names its one exception: an artifact a specification deliberately makes part of the project's version-controlled history — the development workflow's design documents and progress ledger (DW-5). |
| 1.2.2 | 2026-09-24 | Consistency pass (2026-09-24): The state-root convention added yesterday placed office state only in the OS tier, but `cronus init` creates a project-local `.cronus/` state root that every verb resolves first (as shipped) — the convention now names both, with the obligations a project-local root carries (excluded from version control, no secrets, registered for backup — Partial), and the STO-7 row states the backup gap. The program tier listed three executables (`cronus`, `cronus-tui`, `cronusd`) although the product ships one binary — the terminal UI and the always-on engine are modes of `cronus` (`l2-cli` §4.2, `l2-service-activation` §2). |
| 1.2.3 | 2026-09-24 | Realization sync (2026-09-24): STO-9: every SQLite store carries a schema version (fresh, adopted, migrated with a safety copy, or refused when newer); the JSON config files remain unversioned. §4.3: `cronus init` writes the version-control exclusion; registering the root with the OS state tier is still pending. |
| 1.3.0 | 2026-10-03 | Realization of `l1-state-authority` (now also in Implements) and a correction. §3: the STO-8 row no longer says "`*.db` are derived indices alongside `notes/`" — true for text-authoritative kinds, false for the learned memory database, the deliberation log and every other store- or record-authoritative kind — and now defers to the per-kind declaration; SAU-1…SAU-7 rows added (SAU-4/SAU-5 Pending: the specifications agree, the repair and backup code has not been checked against the table). New §4.6 authority map: each kind of state in the tier with its location, class, projections, recovery and declaring invariant; sessions and the deferred relationship graph are marked *proposed* because their owners are silent on class. §4.3: the project-local root's exclusion from version control stays the default, configuration committed on purpose is already a tracked subset, and widening it to office state is an opt-in under `l1-multi-device-sync` SY-10 (per remote, per declared subset, lapsing on change, the office never pushing on its own) — open: the settings surface and the initial eligible subset. §4.4: a remote sync target holds replicas and the local tier stays the authority of each kind. §4.5 and the Canonical References no longer name a `.release/` directory that does not exist; the stub is a temporary sandbox with no authority. |
| 1.4.0 | 2026-10-22 | Realization of `l1-artifact-reach` (now also in Implements). New §4.7 reach map: each kind with its reach (project-shared, personal, machine-local, never-egress) and declaring source — most rows *proposed*, because owners declare authority classes but not reach; new §4.8 for the OS tier. §3 gains RCH-1…RCH-9 (RCH-4 portability check, RCH-7 generated exclusion and RCH-8 inspect listing **Pending**; RCH-1 and RCH-5 Partial — skills, agent definitions and workflows have no opt-in line and are not routed through the trust decision) and an SAU-8 row pointing at the projections' owners. Open: where project-shared skills, agent definitions and workflows live (TBD in §4.7). |
| 1.5.0 | 2026-10-22 | The §4.7 TBD resolved by the user's decision: `.cronus/` in a project is the team's folder, committed as a whole, with `.cronus/local/` as the office's state root, excluded from version control. New §4.3.1 lays the folder out (readme for collaborators, `.version`, `.gitignore`, settings, commands, and source-form `skills/`, `agents/`, `workflows/`); the state-root convention points at `local/`; the shipped layout migrates once, backed up, keyed by `.version`. §4.7: project skills, agents and workflows declared project-shared in source form, their converted forms machine-local under `local/`, built-in artifacts part of the release, user-installed or authored ones personal. §3 gains RCH-10…RCH-12, all Pending against code. |
| 1.6.0 | 2026-10-22 | From the user's backlog (the release folder structure, expected to be renamed often; an optional choice to keep Cronus's working files in the project folder for sync through the remote). §4.1: the program tier defaults to a per-user location so installation and updates need no administrator rights. §4.2: `extensions/`, `install.json`, `previous/`. §4.3: `extensions/` and `backups/`; §4.3.1's TBD resolved — `share.office_state` is the opt-in setting, the initial subset decision records, plan files and authored-memory exports; and a per-project `state.location` (`project` or `user`) with a one-time backed-up move. New §4.9: every written path is one resolver entry, a rename is a migration plus a sweep; a catalog of paths other specifications name and the trees did not, with two conflicts recorded (checkpoint files outside the workspace, two meanings of `snapshots`) and the stub's three stale differences. Pending: the setting, the move, the resolver entries. |
