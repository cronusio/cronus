# Role Catalog

**Version:** 1.0.2
**Status:** Stable
**Layer:** implementation
**Implements:** l1-roles.md

## Overview

The concrete realization of roles: the read-only preset catalog in the program tier, hired instances in the state tier, the role definition format, how custom roles are created, the hire/release mechanics, and the `role` command surface.

## Related Specifications

- [l1-roles.md](l1-roles.md) - The role model this implements.
- [l2-filesystem-layout.md](l2-filesystem-layout.md) - Catalog (`program/employees/`) and instances (`state/employees/`).
- [l2-orchestration.md](l2-orchestration.md) - The manager hires/releases and delegates to roles.
- [l2-cli.md](l2-cli.md) - Command grammar standard the `role` commands follow.
- [l2-execution-workspace.md](l2-execution-workspace.md) - Workspaces allocated to agent runs.
- [l2-budget-engine.md](l2-budget-engine.md) - Budget policies scoped to agent instances.

## 1. Motivation

The model needs a concrete place for blueprints versus instances and a uniform definition format so presets and customs are handled identically. File-backed roles keep them inspectable and instances isolated.

## 2. Constraints & Assumptions

- Presets are read-only and shipped under the program tier.
- A hired instance is a mutable copy under the state tier with its own memory.
- Custom roles use the same on-disk shape as presets.
- The frontend renders; hire/fire and definition are core calls (INV-2).

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| ROL-1 Role = specialty | Each catalog entry is a role definition; a hired entry is an agent instance. |
| ROL-2 Preset + custom | Presets in `program/employees/`; customs created under `state/employees/` with `hired_from: custom`. |
| ROL-3 Hire = instantiate | Hiring copies the blueprint to `state/employees/<id>/` with its own `memory/`, `skills/`. |
| ROL-4 Fire = release | Releasing moves the instance's memory to an archive and removes it from the active roster; re-hire restores. |
| ROL-5 Manager-driven | `role hire/fire` are issued by the manager; no client action required. |
| ROL-6 Composition | Each role: `config.json` + `RULES.md` + `skills/` + `skins/`. |
| ROL-7 Catalog integrity | Presets are not edited; "customize a preset" writes a new custom under state (`hired_from: <preset>`). |
| ROL-8 Hierarchy placement | A hired instance's `config.json` records `reportsTo`. |
| ROL-9 Anti-sprawl justification gate | **Pending.** `role create` is to record, in the custom's `config.json`, its justification on at least two independent axes (expertise, parallelism, context isolation, reuse), and to run the corpus-originality admission check against the whole catalog and any pending sibling candidates on neutralized content (`l1-corpus-originality` ORI-1…ORI-5). A near-duplicate is admitted only once made substantively distinct or declared a variant of the preset it resembles (`hired_from: <preset>`); neither half of the gate is loosened to let it through. |

## 4. Detailed Design

### 4.1 Catalog vs instances

```plaintext
<program>/employees/            # read-only PRESET catalog
├── CATALOG.md
└── <role>/                     # blueprint (persona, default config, seed skills)
<state>/employees/              # HIRED instances (mutable)
└── <role-or-custom-id>/
    ├── config.json             # model, budget, reportsTo, hired_from
    ├── RULES.md                # persona
    ├── memory/  skills/  skins/
```

### 4.2 Preset roles (v0.1.0)

Engineering: architect, backend-engineer, frontend-engineer, api-designer, sql-expert.
Quality: code-reviewer, test-writer, debugger, refactorer, performance-optimizer, security-auditor, accessibility-auditor.
Ops & docs: devops-engineer, incident-responder, doc-writer, data-analyst, prompt-engineer.
Memory: archivist.
Business: finance, hr, marketing, support, game-dev.

### 4.3 Custom role creation

A custom role is created under `state/employees/<id>/` with the same shape; `config.json` records `hired_from: custom` (or `hired_from: <preset>` when derived from a preset). Presets are never mutated (ROL-7).

### 4.4 Hire / fire mechanics

```mermaid
graph TD
    HIRE[role hire preset|custom] --> COPY[instantiate into state/employees/id]
    COPY --> PLACE[set reportsTo in office hierarchy]
    FIRE[role fire id] --> ARCHM[archive role memory]
    ARCHM --> REMOVE[remove from active roster]
```

### 4.5 Command surface

Role operations conform to the CLI grammar standard (see `l2-cli.md` §4.4).

| Action | CLI | TUI | Library (no code) |
| --- | --- | --- | --- |
| list (catalog + hired) | `cronus role list [--catalog\|--hired]` | `/role list …` | `roles.list({scope?}) -> Role[]` |
| hire | `cronus role hire <preset> [--as <name>]` | `/role hire <preset> …` | `roles.hire(preset, opts) -> Role` |
| create custom | `cronus role create <name> [--from <preset>]` | `/role create <name> …` | `roles.create(name, opts) -> Role` |
| show | `cronus role show <id>` | `/role show <id>` | `roles.get(id) -> Role` |
| fire | `cronus role fire <id>` | `/role fire <id>` | `roles.fire(id) -> void` |

### 4.6 Agent adapter protocol and config revisions

Hired agents may be backed by external runtimes (a locally spawned process, a remote API, or a sandboxed binary). The adapter protocol defines three integration levels, each building on the previous:

#### Integration levels

| Level | Capability | Required interface |
| --- | --- | --- |
| Callable | Accept a task, return a result. | `run(payload) -> result` |
| Status-reporting | Callable + emit progress events during execution. | Callable + `status(runId) -> RunStatus` |
| Fully instrumented | Status-reporting + heartbeat, cost events, structured logs. | Status + `heartbeat()` + `report_cost(event)` + structured log sink |

The orchestration layer treats a callable adapter as a black box — it cannot monitor progress or enforce a budget mid-run. A fully-instrumented adapter enables real-time budget enforcement, liveness checks, and fine-grained audit logs. New adapters should target fully instrumented; callable is the minimum for third-party integration.

Because a callable adapter cannot be bounded mid-flight, it is bounded at dispatch: every run carries a wall-clock deadline, and its budget slice is reserved in full before the call and charged whether or not the adapter reports cost — so an uninstrumented runtime can never spend past the cap it was given (ORC-7). A status-reporting adapter gets the same deadline; only a fully instrumented one is metered live.

#### Context delivery: fat payload vs thin ping

```text
[REFERENCE]
ContextDelivery: "fat_payload" | "thin_ping"
```

`fat_payload` — the full task context (briefing, constraints, tools, conversation history) is bundled into the initial `run()` call. The adapter needs no further calls to the control plane to begin work. Best for remote adapters with high latency per round-trip.

`thin_ping` — only a task identifier is sent. The adapter fetches the full context from the control plane on demand. Best for local adapters where a round-trip is cheap and context may be large.

The `config.json` for a hired instance records which delivery mode the adapter requires. The orchestration layer selects accordingly.

#### Config revisions and rollback

Every change to a hired agent's `config.json` is recorded as a revision, enabling rollback. A revision is written by the manager the instance reports to or by the user — never by the instance itself: the file carries the instance's model and budget, and an agent that could patch them would widen its own authority (SEC-10; budget changes also follow the cascade of `l2-budget-engine` §4.4). A rollback that would restore a wider grant than the current one is subject to the same rule.

```text
[REFERENCE]
AgentConfigRevision {
  id,
  agentId,
  revisionNumber: u32,          // monotonically increasing
  beforeConfig: JsonObject,     // snapshot of config before this change
  afterConfig: JsonObject,      // snapshot of config after this change
  changedKeys: String[],        // which top-level keys changed
  source: "patch" | "rollback", // how this revision was created
  rolledBackFromRevisionId?,    // set when source = "rollback"
  createdAt
}
```

Rollback procedure: the orchestration layer reads `beforeConfig` from the target revision and applies it as a new `patch` revision (source = `"rollback"`, `rolledBackFromRevisionId` = target). This means rollback itself is recorded — there is no destructive edit of history.

Revision records are stored in `<state>/employees/<id>/config-revisions/` as append-only JSON files (one per revision).

## 5. Drawbacks & Alternatives

- **Copy-on-hire duplication:** each instance copies the blueprint; acceptable for isolation and per-instance learning.
- **Preset upgrades vs live instances:** when a preset updates with the program, existing instances do not auto-migrate. <!-- TBD: policy for propagating preset updates to already-hired instances -->
- **Alternative — shared role definitions (no copy):** rejected; it couples instances and breaks per-role memory isolation.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[ROLES]` | `.design/main/specifications/l1-roles.md` | Invariants this catalog satisfies |
| `[LAYOUT]` | `.design/main/specifications/l2-filesystem-layout.md` | Catalog and instance locations |
| `[CLI]` | `.design/main/specifications/l2-cli.md` | Command grammar standard |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.2 | 2026-09-23 | Core Team | Consistency pass (2026-09-23): ROL-9 (anti-sprawl justification gate, including its corpus-originality half added to the L1 in 1.2.0) was unmapped — Pending row added. A callable adapter could spend without bound because it "cannot enforce a budget mid-run" — it is bounded at dispatch (deadline plus a budget slice reserved and charged in full). Config revisions could be written by the instance itself, whose config carries its own model and budget — only the manager above it or the user writes them (SEC-10, budget cascade). |
| 1.0.1 | — | Core Team | Last version before this section was added; earlier revisions are recorded in version control. |
