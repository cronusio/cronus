# Filesystem Layout (OS-native)

**Version:** 1.3.0
**Status:** Stable
**Layer:** implementation
**Implements:** l1-storage-model.md, l1-state-authority.md

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

## 4. Detailed Design

### 4.1 OS-native locations

| Tier | Windows | macOS | Linux (XDG) |
| --- | --- | --- | --- |
| Program (immutable) | `%ProgramFiles%\Cronus\` | `/Applications/Cronus.app/Contents/Resources/` | `/opt/cronus/` or `/usr/local/lib/cronus/` |
| State (mutable) | `%APPDATA%\Cronus\` | `~/Library/Application Support/Cronus/` | `~/.local/share/cronus/` |
| Cache (regenerable) | `%LOCALAPPDATA%\Cronus\Cache\` | `~/Library/Caches/Cronus/` | `~/.cache/cronus/` |
| Logs / runtime | `%LOCALAPPDATA%\Cronus\Logs\` | `~/Library/Logs/Cronus/` | `~/.local/state/cronus/` |

A single path resolver in the core maps the abstract roots (`<program>`, `<state>`, `<cache>`, `<logs>`) to these per-OS paths; portable mode overrides them to one chosen directory.

### 4.2 Program tier tree

```plaintext
<program>/
├── bin/            # cronus — the one executable: CLI, terminal UI (bare `cronus`), always-on engine (`cronus serve`)
├── app/            # core engine library + desktop application shell
├── templates/      # employee/ , workspace/ (blueprints copied on init)
├── employees/      # read-only role catalog (CATALOG.md + role blueprints)
├── skills/         # [ADDED] read-only preset skill store (canonical packages)
├── languages/  themes/
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
tier, or — for a project initialized in place with `cronus init` — the project's own
`.cronus/` directory, found as the nearest ancestor of the working directory that holds
one, falling back to the OS tier (as shipped). Wherever it lives, the office's planning and
run state — missions, checkpoints, proposals, clarifications, training runs — stays inside
the state root, never scattered among the project's tracked files. What an office
deliberately delivers *into* a project (code, documents) is the project's; the records of
how it got there are the office's. The one exception is an artifact a specification
deliberately makes part of the project's version-controlled history and names as such —
the development workflow's design documents and progress ledger (DW-5), committed to the
feature branch.

A project-local state root carries three obligations the OS-native tier meets by location
alone. It is excluded from the project's version control, since office state is not project
content and a checkout must not carry sessions or memory with it. It never holds a secret —
those stay in the OS state tier's secret store (STO-6). And it is registered with the OS
state tier, so a backup can enumerate it; otherwise copying `<state>/` would silently leave
it behind (STO-7). **Partial:** `cronus init` creates the root and writes its version-control
exclusion — a self-ignoring `.gitignore` (`*`, so `init` leaves nothing in `git status`) that opts
`settings.json` and `commands/` back in by name and is left untouched when it already exists;
registering the root with the OS state tier is pending. Project *configuration*
committed on purpose (`.cronus/settings.json`, commands, skills) is a different thing, loaded
only after the workspace trust decision (`l2-security` §4.8).

**Tracking a subset (`l1-multi-device-sync` SY-10).** The exclusion is the default, and configuration committed on purpose is already a tracked subset. A user may widen it: track a *declared subset* of office state in the project's own history so that a remote the user names carries it. Only kinds whose reconciliation a text-merging transport can carry faithfully are eligible — the reviewable-merge classes first, such as plans and decision records; a CRDT class rides it only as per-replica operation files — and secrets and live sessions never are. The consent names the remote and the subset, is visible and revocable, and lapses when either changes. The office commits locally at most; the push is the user's act or a standing consent they recorded for that remote, never a workspace's (the no-remote-git contract, `l2-execution-workspace` §4.4). <!-- TBD: the settings surface for the opt-in and the initial eligible subset (decision records, plan files, exports of authored memory) -->

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
