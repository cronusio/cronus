# Workspace Management

**Version:** 1.0.1
**Status:** Stable
**Layer:** implementation
**Implements:** l1-workspace-lifecycle.md

## Overview

The concrete realization of the workspace lifecycle on the desktop application: a tab bar with a pinned, non-deletable home tab and one tab per project; an "add" control that opens a creation form; name normalization to a kebab-case identifier; instantiation from the workspace blueprint into the state tier; and a default manager bootstrapped on creation.

## Related Specifications

- [l1-workspace-lifecycle.md](l1-workspace-lifecycle.md) - The lifecycle this implements.
- [l2-app-ui.md](l2-app-ui.md) - The application shell that hosts the tab bar.
- [l2-filesystem-layout.md](l2-filesystem-layout.md) - Template source and state destination paths.
- [l2-core-library.md](l2-core-library.md) - Performs instantiation, manager bootstrap, and staffing.

## 1. Motivation

The lifecycle model demands a one-gesture office creation, an always-present home, editable metadata, and an immediately-staffed manager. The desktop tab metaphor delivers all of this with a familiar UX; the filesystem flow makes each office an isolated, copyable directory.

## 2. Constraints & Assumptions

- Desktop frontend (the application shell); the tab bar is the primary navigation.
- Workspace identifiers are filesystem-safe kebab-case derived from the user-provided name.
- The frontend holds no logic: creation/edit/delete and bootstrap are core calls (consistent with INV-2).

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| WSL-1 Home singular & permanent | Leftmost tab is pinned, shows a home/star icon, has no close control; the core refuses to delete it. |
| WSL-2 One manager per office | Each workspace's `config.json` records its `manager`; the home workspace's manager carries cross-workspace oversight. |
| WSL-3 Project lifecycle | Project tabs support create (add control), edit (settings panel), and close/delete; home tab exposes none of delete. |
| WSL-4 Instantiation from blueprint | Copy `<program>/templates/workspace/` → `<state>/workspaces/<id>/`; `<id>` is the normalized name. |
| WSL-5 Default manager bootstrap | On create, the core instantiates the office manager before returning the open office. |
| WSL-6 Bidirectional staffing | The manager calls core hire/release operations against the role catalog as needs change. |
| WSL-7 Editable metadata | A settings panel writes name/description/local path back to `<ws>/config.json` without recreating the office. |
| WSL-8 Isolation & clean deletion | Each office is its own directory; deleting a project removes `<state>/workspaces/<id>/` only — never the project's own directory on disk; home is exempt. Deletion is a confirmed user act (§4.4). |

## 4. Detailed Design

### 4.1 Tab bar

```text
[ ★ Home ] [ project-alpha ] [ project-beta ] [ + ]
   pinned        closable        closable      add
```

- Home tab: leftmost, pinned, home/star icon, no close button; selecting it opens the organizer office.
- Project tabs: one per `<state>/workspaces/<id>` (excluding home); closable (close ≠ delete — see §4.4).
- Add control (`+` / "Add"): opens a new tab hosting the creation form.

### 4.2 Creation form

Fields (all editable later via the settings panel):

| Field | Required | Notes |
| --- | --- | --- |
| Name | yes | human-readable; drives the identifier |
| Description | no | free text |
| Local path | no | project location on disk |

### 4.3 Name normalization (identifier)

```text
[REFERENCE]
normalize(name):
  lower-case
  replace runs of non [a-z0-9] with single "-"
  trim leading/trailing "-"
  truncate to 64 characters (at a "-" boundary where possible)
  if empty -> "workspace"                   // a name with no Latin letters or digits
  on collision with an existing workspace id, or with a name the OS reserves for a device
  (con, prn, aux, nul, com1…com9, lpt1…lpt9) -> append "-2", "-3", ...
# "My Game Dev!" -> "my-game-dev"
# "Мой проект"   -> "workspace" (or "workspace-2", …); the name itself is kept as typed
```

Only lowercase letters, digits, and `-` as separator (per WSL-4). The identifier only names the directory: the human-readable name is stored as given in `config.json`, so a name in any script survives normalization intact even when little or nothing of it reaches the identifier.

### 4.4 Create / edit / delete flows

```mermaid
graph TD
    ADD["+ add"] --> FORM[Creation form]
    FORM --> NORM[normalize name -> id]
    NORM --> COPY[copy template -> state/workspaces/id]
    COPY --> BOOT[core bootstraps default manager]
    BOOT --> OPEN[open office tab]
    SET[Settings panel] --> EDITcfg[write config.json metadata]
    CLOSE[Close tab] --> HIDE[hide tab, keep state]
    DEL[Delete project] --> RM[remove state/workspaces/id]
```

- **Close** a tab hides the office without destroying state; reopening relists it.
- **Delete** removes the office directory entirely (WSL-8); the home office offers no delete. It is destructive and irreversible, so it is the user's act only (WSL-3): the surfaces require confirmation (the id typed back, or `--yes` in scripts), and `workspace.delete` is in no agent's tool surface — an office can delete neither itself nor a sibling (OFF-1). An office with work in flight is paused first (drain and checkpoint, `l2-office-control`) so no step is cut off mid-write. Deletion removes the office's state only — never the project directory recorded as its local path.

### 4.5 Default manager bootstrap

On creation the core instantiates the office manager (the boss) per the office model, writes it into `<ws>/config.json` (`manager`), and hands control to it. The manager then drives hire/release against the role catalog as the project's inputs accumulate (WSL-6).

### 4.6 Command surface

Workspace operations across all three surfaces, conforming to the CLI grammar standard (verb-first, explicit verbs; see `l2-cli.md` §4.4). The library method is the source; CLI and TUI are thin bindings (INV-3).

| Action | CLI | TUI | Library (no code) |
| --- | --- | --- | --- |
| list | `cronus workspace list` | `/workspace list` | `workspace.list() -> Workspace[]` |
| create | `cronus workspace create <name> [-d <desc>] [-p <path>]` | `/workspace create <name> …` | `workspace.create(name, {description?, path?}) -> Workspace` |
| open | `cronus workspace open <id>` | `/workspace open <id>` | `workspace.open(id) -> Workspace` |
| info | `cronus workspace info <id>` | `/workspace info <id>` | `workspace.get(id) -> Workspace` |
| set (edit) | `cronus workspace set <id> [--name <v>] [--description <v>] [--path <v>]` | `/workspace set <id> …` | `workspace.update(id, patch) -> Workspace` |
| close | `cronus workspace close <id>` | `/workspace close <id>` | `workspace.close(id) -> void` |
| delete | `cronus workspace delete <id>` | `/workspace delete <id>` | `workspace.delete(id) -> void` (refuses home) |
| home | `cronus workspace home` | `/workspace home` | `workspace.home() -> Workspace` |

- `create` takes the human-readable `<name>` (positional), normalized to `<id>` (§4.3); `--description/-d` and `--path/-p` are optional and editable later via `set`.
- `delete` refuses the home workspace (WSL-1); `close` hides a tab without destroying state (WSL-8).
- Internal helper (not a user command): `workspace.normalizeName(name) -> id`.

## 5. Drawbacks & Alternatives

- **Tabs scale poorly past many offices:** mitigate with grouping/overflow in a later iteration. <!-- TBD: overflow/grouping behavior when offices exceed the tab strip width -->
- **Close vs delete ambiguity:** mitigated by distinct affordances (close = hide, explicit Delete = destroy).
- **Alternative — one window per office:** rejected; tabs keep the building metaphor and a single organizer surface.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[LIFECYCLE]` | `.design/main/specifications/l1-workspace-lifecycle.md` | Invariants this implements |
| `[LAYOUT]` | `.design/main/specifications/l2-filesystem-layout.md` | Template source and state destination |
| `[APP]` | `.design/main/specifications/l2-app-ui.md` | Application shell hosting the tab bar |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.1 | 2026-09-23 | Core Team | Consistency pass (2026-09-23): Name normalization produced an empty identifier for names with no Latin letters or digits (any Cyrillic name) and could produce device names Windows reserves (`con`, `nul`, …) — fallback id, reserved-name suffixing, and a length cap added; the human-readable name is kept as typed. Delete was unconfirmed and unscoped — it is a confirmed user-only act (WSL-3), absent from any agent's tool surface, pauses an office with work in flight first, and never touches the project directory at the recorded local path. |
| 1.0.0 | — | Core Team | Last version before this section was added; earlier revisions are recorded in version control. |
