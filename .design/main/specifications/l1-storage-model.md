# Storage & State Model

**Version:** 1.2.0
**Status:** Stable
**Layer:** concept

## Overview

The technology-agnostic model for how Cronus stores everything on a user's machine. It defines two tiers — an **immutable program** tier and a **mutable state** tier — and a **multi-level memory** model (global / workspace / employee / session). It is the "start from the end" view: what lands on the user's device and how it changes over time.

## Related Specifications

- [l1-architecture.md](l1-architecture.md) - Refines INV-5 (durable state) and INV-7 (security) in the storage domain.
- [l1-office-model.md](l1-office-model.md) - Office-per-project isolation (OFF-1) and persistent learning (OFF-9) realized as state scopes.
- [l2-filesystem-layout.md](l2-filesystem-layout.md) - Concrete OS-native paths and directory trees.
- [l2-technology-stack.md](l2-technology-stack.md) - Storage technology (SQLite + sqlite-vec, optional remote sync).
- [l1-state-authority.md](l1-state-authority.md) - [ADDED v1.2.0] which copy of a kind of state is the truth: STO-8's text-first rule is one of three authority classes (SAU-3), and backup, restore and repair decide by the declared class (SAU-5).

## 1. Motivation

Designing "from the end" — the installed footprint — forces clarity about what is shipped versus what is grown. Separating an immutable program from mutable state makes updates safe (replace the program, keep the state), makes backups trivial (copy one tier), and makes the system's learning durable (OFF-9). A scoped memory model lets the agent remember at the right granularity (the human, the office, the role, the conversation) and forget cleanly when a scope disappears.

## 2. Constraints & Assumptions

- The program tier is read-only at runtime; the runtime writes only into the state tier.
- State is local-first and self-contained; remote synchronization is optional, never required.
- Secrets are part of the state tier but are excluded from backups, exports, and version control.
- Some state is regenerable (caches, indices) and may be discarded without data loss.

## 3. Core Invariants (Layer 1 only)

Rules every Layer 2 implementation MUST NOT violate:

- **STO-1 (Two-tier separation):** program artifacts are immutable at runtime; all mutable state lives in a separate tier. Updating or reinstalling the program MUST NOT modify or require the state tier.
- **STO-2 (Durable, restartable state):** all runtime-produced state persists durably and the system resumes from it after a restart, with no loss (consistent with architecture INV-5).
- **STO-3 (Catalog vs instance):** blueprints (role and workspace templates, the role catalog) are read-only in the program tier. Using a blueprint creates a mutable **instance** in the state tier; an instance MUST NOT mutate its blueprint.
- **STO-4 (Multi-level memory):** memory is partitioned into scopes — **global**, **workspace**, **employee**, **session**. Retrieval resolves most-specific-first (employee → workspace → global); a write targets the scope the fact belongs to.
- **STO-5 (Scope-bound lifecycle):** deleting a scope (office, role, or session) deletes the memory owned by that scope. Only the global level outlives all others. Session memory decays and is pruned over time.
- **STO-6 (Secret isolation):** secrets are confined to the state tier and MUST be excluded from backups, exports, and version control (consistent with architecture INV-7).
- **STO-7 (Restore-by-copy):** the mutable state tier (excluding secrets and regenerable caches) is self-contained and restorable by copying it; no hidden external dependency is required to resume.
- **STO-8 (Human-inspectable state):** durable state SHOULD be human-readable and editable where practical (text/Markdown). [MODIFIED v1.2.0] Where text is the **authority** of its kind, databases and indices over it are derived from it rather than being the sole source of truth. Where a kind is store-authoritative (volume or concurrency a file cannot carry) or record-authoritative (an append-only audit record), the store is the truth and human inspectability is provided by an **export** — a projection that is never an input to rebuilding the store. Which applies is each kind's declaration (SAU-3), not a property of a file type or of a database.

- **STO-9 (Versioned state with forward migration):** [ADDED v1.1.0] every durable state artifact whose shape can evolve carries an explicit **schema version**. On load the runtime validates the structure and, if the on-disk version is older than the program's, applies a **forward, one-way migration** to the current version before use — it never silently loads a shape it does not understand, and never writes a newer shape a prior program version could then misread without a version marker. A destructive rewrite is preceded by a **timestamped backup**, backups are rotated by age, and cross-installation transfer uses an explicit **non-destructive merge** import keyed by record identity (never a blind overwrite). This makes STO-2 durability and STO-7 restore-by-copy survive program upgrades that change the state shape.

> L2 specs cannot reach RFC status until all invariants here are addressed in their "Invariant Compliance" section.

## 4. Detailed Design

### 4.1 The two tiers

```mermaid
graph TD
    subgraph Program [Program tier - immutable]
        BIN[Executables / launchers]
        APP[Compiled runtime]
        TPL[Templates / role catalog]
        I18N[Languages / themes]
    end
    subgraph State [State tier - mutable]
        CFG[Config + secrets]
        EMP[Hired employees]
        WS[Offices / workspaces]
        MEM[Memory + graph]
    end
    Program -. blueprints copied on init .-> State
    RUNTIME[Runtime] -->|writes| State
    RUNTIME -->|reads only| Program
```

### 4.2 Memory levels

| Level | Scope | Holds | Lifecycle |
| --- | --- | --- | --- |
| Global | The installation / the human client | Cross-project facts, preferences, shared skills | Long-lived |
| Workspace | One office/project | Office knowledge, project graph | Lives with the office |
| Employee | One hired role | Role expertise | Grows with the role (OFF-9) |
| Session | One conversation/run | Episodic dialogue/decisions | Decays, auto-pruned |

```mermaid
graph LR
    Q[Retrieval query] --> E[Employee]
    E --> W[Workspace]
    W --> G[Global]
    note[Most specific wins; write to owning scope]
```

### 4.3 Backup and update flows

- **Update:** replace the program tier; the state tier is untouched (STO-1). If the new program's state schema is newer, a forward migration runs on first load (STO-9), preceded by a timestamped backup.
- **Backup:** copy the state tier minus secrets and caches (STO-6, STO-7). Destructive rewrites auto-snapshot first; snapshots rotate by age.
- **Restore:** drop the copied state tier back in place; the runtime resumes from it (STO-2). A restored artifact with an older schema version is migrated forward before use (STO-9).
- **Transfer/merge:** cross-installation import merges non-destructively by record identity (STO-9) — an incoming record never blindly clobbers a local one; conflicts are resolved by explicit key, not by load order.

### 4.4 State versioning and migration [ADDED v1.1.0]

```text
[REFERENCE]
load(artifact):
    data := read(artifact)
    if data.schema_version is missing or > program.schema_version:
        reject("unknown/newer state shape")        // STO-9: never guess an unknown shape
    if data.schema_version < program.schema_version:
        backup(artifact, timestamp)                 // STO-9: snapshot before rewrite
        data := migrate_forward(data,               // one-way, recorded
                   from=data.schema_version, to=program.schema_version)
        write(artifact, data)
    validate_structure(data)                        // STO-9: fail loud, not silent-partial
    return data

import(incoming, mode=merge):                       // STO-9 non-destructive transfer
    for record in incoming:
        if local.has(record.id):  reconcile_by_key(local[record.id], record)
        else:                      local.add(record)
```

The version marker is what makes STO-2 durability robust across upgrades: without
it, a program that changed its state shape either crashes on old data or silently
loads a half-understood structure. The pre-write snapshot makes every migration
reversible (STO-7), and merge-by-identity makes import safe to repeat.

## 5. Drawbacks & Alternatives

- **Derived indices and exports:** where text is the authority, STO-8 means maintaining derived databases alongside it (sync cost, justified by inspectability and git-friendliness); where a store is the authority, it means maintaining a human-readable export instead (SAU-3). Either way the derived side is rebuilt from the authority and never the reverse (SAU-4).
- **Alternative — single opaque database:** simpler but violates STO-8 and complicates backup/merge; rejected.
- **Alternative — one global memory only:** simplest but breaks office isolation (OFF-1) and clean forgetting (STO-5); rejected in favor of multi-level. <!-- TBD: whether global+workspace+employee share one physical database (attached) or separate files -->

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[ARCH]` | `.design/main/specifications/l1-architecture.md` | State/security invariants this model refines |
| `[OFFICE]` | `.design/main/specifications/l1-office-model.md` | Office isolation and learning realized as scopes |
| `[LAYOUT]` | `.design/main/specifications/l2-filesystem-layout.md` | Concrete realization of this model |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.2.0 | 2026-10-03 | Core Team | STO-8 amended: the text-first rule ("databases derived from human-readable text") is now stated as one of three **authority classes**, not the only one. The learned memory corpus was deliberately made database-authoritative (MEM-4) after STO-8 was written; the blanket sentence survived in this spec and in `l2-filesystem-layout`, and `l2-doctor` still lists a repair ("rebuild the index from source text") that, read literally, rebuilds a truth from its own export. STO-8 now defers the choice to each kind's declaration (`l1-state-authority` SAU-3) and keeps inspectability as an export where the store is the truth. §5's first bullet follows; Related Specifications extended with `l1-state-authority`. No tier, scope or versioning rule changed. L1 stays Stable (C9). |
| 1.1.0 | 2026-07-01 | Core Team | Added STO-9 (versioned state with forward one-way migration, timestamped pre-write backups with age rotation, non-destructive merge-by-identity import) and §4.4 migration flow; §4.3 backup/update/restore/transfer flows extended to reference it. Makes STO-2 durability and STO-7 restore-by-copy survive program upgrades that change the state shape. |
| 1.0.0 | 2026-06-24 | Core Team | Initial spec — STO-1…STO-8, two-tier separation, multi-level memory, backup/update flows. |
