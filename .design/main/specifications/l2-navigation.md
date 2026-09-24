# Navigation

**Version:** 1.1.1
**Status:** Stable
**Layer:** implementation
**Implements:** l1-navigation-model.md

## Overview

The concrete rendering of the four-layer building navigation in the Tauri v2 + React 19 desktop shell: the Building frame (L0 menu + Providers/ACP + Process Monitor + project file-tree dock + command palette), the floor tab bar (L1) with lazy loading and live OfficeState icons, the canonical subsystem sidebar (L2, a primary run + a foot utility group), recursive mechanism sub-navigation (L3+), the two-tier settings surface (in-sidebar Local tier + a full-screen Global settings overlay), and the "Open in IDE" launcher. Navigation is presentation over core state: the sidebar catalog is a fixed frontend constant, but floor load/unload, office state, and settings persistence are core calls (INV-2).

## Related Specifications

- [l1-navigation-model.md](l1-navigation-model.md) — the model this renders (NV-1…NV-10); §4.6 fixes the L0 menu leaves, §4.5 the L3 facet catalog.
- [l1-application-shell.md](l1-application-shell.md) — AS-9 workbench composition (the file-tree dock is an edge dock) and AS-10 delegated selection surfaces (the command palette).
- [l2-app-ui.md](l2-app-ui.md) — the frontend runtime shape (state authority, workbench) this composes onto.
- [l2-design-system.md](l2-design-system.md) — the token contract every navigation surface renders from (no hardcoded visual values).
- [l2-office-control.md](l2-office-control.md) — the OfficeState source for live tab status icons (NV-3).
- [l2-workspace-management.md](l2-workspace-management.md) — floor creation/deletion and `workspace_root` binding (NV-8).
- [l2-config-hotreload.md](l2-config-hotreload.md) — two-tier settings persistence and live reload (NV-4).
- [l1-process-monitor.md](l1-process-monitor.md) — the Building-level process view (NV-7).

## 1. Motivation

The model guarantees a consistent, memorable structure across platforms. Realizing it as a fixed frontend catalog plus core-backed floor/state calls keeps the navigation identical on every build while the live data (which floors exist, their state, their settings) stays single-sourced in the core.

## 2. Constraints & Assumptions

- The sidebar catalog and its order are a frontend constant; users may pin shortcuts above it but cannot reorder/hide the canonical set (NV-1).
- Floor lazy loading calls the core `office.is_loaded` / `office.load` / `office.unload` capability; the home floor is never unloaded.
- Status icons subscribe to the OfficeState event stream — no polling (NV-3).
- Local settings live with the office's configuration in its state root (`<ws>/config.json`, `l2-filesystem-layout` §4.3), never among the project's tracked files. They hold machine-specific paths and preferences — never secrets, which live only in the state tier's secret store (SEC-1).

## 3. Invariant Compliance (Layer 2)

| L1 Invariant | Implementation |
| --- | --- |
| NV-1 Canonical sidebar order | Two frozen arrays — `SIDEBAR_PRIMARY` (Dashboard…Wiki) then `SIDEBAR_UTILITY` (Channels, Security, Providers/ACP, Settings) — render in fixed order, the utility run visually separated at the sidebar foot; pinned shortcuts render in a separate strip above, never mutating either array (§4.3). |
| NV-2 Office tab lazy loading | On start the shell loads the home floor + most-recently-active project floor via `office.load`; other tabs render a placeholder until activated. A closed tab with no running tasks calls `office.unload`. |
| NV-3 Live status indicator | Each floor tab subscribes to `OfficeStateChanged` (office-control §4.1); the icon re-renders on each event, never from a snapshot older than one cycle. |
| NV-4 Two-tier settings | The Settings tab renders the **Local** tier in-place (active office config) and opens the **Global** tier as a full-screen overlay (§4.8), reached also from L0 File▸Settings; each writes through `config.set(scope, …)` and reloads via config-hotreload. A tier label separates them. |
| NV-5 IDE integration | The floor settings dropdown's "Open in IDE" reads `workspace_root` (local) + `configured_ide` (global) and spawns the IDE executable with the root as an argument vector entry — never through a shell (§4.5). |
| NV-6 Strict layer nesting | The React tree mirrors Building ⊃ Floor ⊃ Subsystem ⊃ Mechanism; each level's router is scoped to one parent instance and cannot address a sibling's subtree. |
| NV-7 Building frame & app menu | The L0 frame hosts File/Edit/View/Help (leaf lists per l1-navigation-model §4.6; rendered as an in-window burger menu, or a platform-native menu bar where available) plus Providers/ACP, Process Monitor, a toggleable right-edge project **file-tree dock** (§4.7), and a **command palette** (§4.6); these act across all floors. |
| NV-8 Floor = disk-bound tab | Floor creation resolves through three affordances (File menu, "+" control, folder drag-drop) all calling `workspace.create`; a project floor binds `workspace_root` at creation, stable for its life. |
| NV-9 Default pinned home floor | The first tab is pinned, non-closable, loaded on start; its dropdown omits close/delete; its files resolve to the state tier, not a disk project. |
| NV-10 Recursive sub-navigation | A subsystem with facets renders an L3 sub-tab strip scoped to that subsystem, from the l1-navigation-model §4.5 facet catalog (Dashboard→Agent Statistics/Token Usage, Inbox→Messages/Poll-Clarify, Schedule→Cron/Pulse, Office→Home/Project, Kanban→boards, Automation→flows, Channels→per-channel, Settings→Global/Local); a flat subsystem renders none. |

## 4. Detailed Design

### 4.1 Component tree

```text
[REFERENCE]
<BuildingFrame>                       // L0 — menu (File/Edit/View/Help) + Providers/ACP + Process Monitor
  <FloorTabBar>                       // L1 — pinned Home + project floors, "+" + drop target
    <BodyRow>                         // L2 sidebar + content + optional right dock
      <SubsystemSidebar>             // L2 — SIDEBAR_PRIMARY run, then SIDEBAR_UTILITY foot group
        <Subsystem tab>             // active subsystem surface
          <MechanismNav?>          // L3+ — sub-tabs, only where earned (NV-10)
      <ContentColumn/>
      <RightDock?>                  // toggleable project file-tree (§4.7)
  <CommandPalette?>                   // floating — delegated selection surface (§4.6, AS-10)
  <GlobalSettingsOverlay?>           // full-screen — L0 File▸Settings / Settings tab Global tier (§4.8)
```

State authority is the app-shell store (AS single-authority); navigation components read floor/state/settings/layout from it and dispatch load/unload/create and layout-toggle actions. Dock visibility and the active subsystem/floor are caller-owned view state (thin runtime — no core round-trip to open the palette or toggle the dock).

### 4.2 Floor tab bar

Each entry renders name, live OfficeState icon (NV-3), and a settings dropdown (rename, open-in-IDE, pause/resume via office-control, close, delete). The pinned home floor omits close/delete. The trailing "+" and a full-bar drop target both invoke `workspace.create`; a dropped folder pre-fills `workspace_root`.

### 4.3 Sidebar catalog

```text
[REFERENCE]
SIDEBAR_PRIMARY = [Dashboard, Chat, Sessions, Inbox, Office, Employees,
                   Schedule, Kanban, Automation, Memory, Wiki]
SIDEBAR_UTILITY = [Channels, Security, Providers/ACP, Settings]   // rendered at the sidebar foot, visually separated
```

Both arrays are frozen order (NV-1); badge counts come from per-subsystem live signals over the bridge. Pinned shortcut tabs render in a strip above `SIDEBAR_PRIMARY` and never mutate either array. UX-stage candidates (Discover/Graph/Process Monitor) slot in additively without reordering the two runs. The old standalone Pulse tab is now an L3 facet of Schedule.

### 4.4 Two-tier settings

Local settings render in-place under the Settings tab (active office config); the Global tier opens as a full-screen overlay (§4.8). Global settings persist to the app global config file; local settings persist with the office's config in its state root. Writes go through the config service so config-hotreload applies them live. Neither tier holds a secret (SEC-1), and neither is written among the project's tracked files.

### 4.5 IDE launch

`open_in_ide(office)` → read `workspace_root` (local) + `configured_ide` (global, default `$EDITOR` → platform default VS Code) → resolve the IDE to an executable and spawn it with `workspace_root` as a separate argument. No shell is involved: a path containing spaces, quotes, or shell metacharacters stays one argument and is never interpreted. `configured_ide` decides what the app will run, so it is changed only through Settings by the user. The editor is external; the app is launcher-only.

### 4.6 Command palette (L0)

A single floating selection surface (l1-application-shell AS-10) reached by keybinding (default Ctrl+Shift+J) or the sidebar search affordance. It renders a query input plus grouped results; behaviour is supplied by a **delegate** — the built-in delegate sources *recent offices*, *go-to-subsystem*, *settings*, and *actions* (the named L0 menu leaves and any registered action), fuzzy-ranks, renders a row (icon, label, secondary text, current binding), and confirms by dispatching. It never round-trips to the core to open; results that reference live data read the shell store's projections.

### 4.7 Project file-tree dock (L0)

A toggleable edge dock (l1-application-shell AS-9) on the trailing side, scoped to the active floor's `workspace_root`. It lists the directory tree with name/content filter tabs and reflects git-ignored entries dimmed. It is a read-only projection in this slice — open/reveal actions bind to shell/core capabilities as they ship (INV-9). Dock visibility is caller-owned view state.

### 4.8 Global settings overlay (L0)

`File ▸ Settings…` and the Settings tab's Global tier both open a full-screen overlay above the workbench: its own title bar, a left settings nav (grouped: onboarding, agents & offices, set-up, workflows, interface), and a scrolled content pane. Closing returns to the prior surface without disturbing floor/subsystem state. The overlay writes through the same config service as the in-place Local tier (§4.4).

## 5. Implementation Notes

1. Lazy loading (NV-2) requires the core `office.is_loaded`/`load`/`unload` capability; the shell holds no office state itself.
2. Status icons (NV-3) subscribe to the office-control event stream through the IPC bridge, not a poll timer.
3. Leaf lists (§4.6 palette actions ← l1-navigation-model §4.6) and the L3 facet catalog (l1-navigation-model §4.5) are fixed at the L1; this spec renders them.
4. Palette, file-tree dock, and global-settings overlay are thin-runtime view state — no core round-trip to open, no persisted layout in the core in this slice.

## 6. Drawbacks & Alternatives

**Alternative — user-customizable sidebar**: fragments the cross-platform mental model (NV-1). Rejected.

**Alternative — floor picker dialog instead of a tab bar**: loses glanceable live status across all floors (NV-3). Rejected.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[MODEL]` | `.design/main/specifications/l1-navigation-model.md` | Invariants NV-1…NV-10; §4.5 L3 facet catalog; §4.6 L0 menu leaves |
| `[APP-SHELL]` | `.design/main/specifications/l1-application-shell.md` | AS-9 edge docks (file tree); AS-10 delegated selection surface (command palette) |
| `[APP-UI]` | `.design/main/specifications/l2-app-ui.md` | Rendering host / state authority / theming |
| `[DESIGN-SYS]` | `.design/main/specifications/l2-design-system.md` | Token contract every navigation surface renders from |
| `[OFFICE-CTRL]` | `.design/main/specifications/l2-office-control.md` | OfficeState source for NV-3 icons |
| `[WS-MGMT]` | `.design/main/specifications/l2-workspace-management.md` | Floor creation + workspace_root |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.0 | 2026-07-03 | Core Team | Initial implementation spec — four-layer component tree, floor tab bar with lazy load + live OfficeState icons, frozen sidebar catalog, recursive mechanism sub-nav, two-tier settings, IDE launcher; maps NV-1…NV-10. |
| 1.1.0 | 2026-09-02 | Core Team | Tracks l1-navigation-model v1.3.0. §4.3 splits the catalog into `SIDEBAR_PRIMARY` + a foot `SIDEBAR_UTILITY` run (Pulse removed — now a Schedule L3 facet). §4.1 component tree adds the right-edge file-tree dock, the command-palette overlay (AS-10), and the full-screen global-settings overlay. New §4.6 command palette, §4.7 file-tree dock, §4.8 global-settings overlay. NV-1/4/7/10 compliance rows updated. Palette/dock/overlay are thin-runtime view state — no core layout persistence in this slice. Added `l2-design-system` as the token-contract source every surface renders from. |
| 1.1.1 | 2026-09-24 | Core Team | Consistency pass (2026-09-24): "Open in IDE" built `{ide} {root}` for a shell spawn, so a workspace path or configured command could be interpreted as shell syntax — the IDE is spawned with an argument vector and no shell, and `configured_ide` is changed only through Settings. Local settings were described as git-ignored files carrying "keys" in the project repository — they live in the office's state root and never hold secrets (SEC-1). |
