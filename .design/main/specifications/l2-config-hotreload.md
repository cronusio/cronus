# Configuration Hot-Reload

**Version:** 1.0.4
**Status:** Stable
**Layer:** implementation
**Implements:** l1-doctor.md, l1-architecture.md

## Overview

Watches the workspace configuration file for changes and applies them without restarting the process. A prefix-keyed rule table classifies every changed config path into `restart`, `hot`, or `none` (no-op), emitting a reload plan that subsystem services consume to invalidate or restart selectively. The watcher auto-recovers from OS-level errors (file-descriptor exhaustion, inotify quota) with bounded backoff and a polling fallback.

## Related Specifications

- [l1-doctor.md](l1-doctor.md) - Self-healing recovery that hot-reload extends (avoiding forced restarts).
- [l1-architecture.md](l1-architecture.md) - Long-running daemon architecture that hot-reload enables.
- [l2-extension-registry.md](l2-extension-registry.md) - Plugin registry reloaded on config change.
- [l2-scheduler.md](l2-scheduler.md) - Cron service restarted when scheduler config paths change.
- [l2-plugin-hooks.md](l2-plugin-hooks.md) - File hooks reloaded when hook config paths change.
- [l2-security.md](l2-security.md) - Secrets fields in config are never logged; redaction applies to reload diagnostics; config integrity shields catch authority-plane drift (§4.6).
- [l2-office-control.md](l2-office-control.md) - The drain-and-checkpoint a safe restart performs before respawning.

## 1. Motivation

A long-running agent daemon accumulates state across sessions. Restarting it every time a user changes a channel token or a cron schedule is expensive — it drops active sessions and resets in-memory state. A prefix-keyed reload plan lets most config changes take effect in milliseconds by restarting only the affected subsystem rather than the whole process.

## 2. Constraints & Assumptions

- Only the on-disk config file is watched. In-process config mutations are not detected here.
- A changed path is any JSON key-path in the config document that differs between old and new snapshots (diffed as a flat key-path set).
- Subsystem services are responsible for consuming reload events; the reload planner does not call them directly.
- Secrets fields (`password`, `token`, `key`, `secret`) are never included in reload log messages.
- File watcher errors are retried with backoff; after all retries are exhausted the watcher degrades to polling mode rather than disabling reloads entirely.

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| HEAL-2 Safe self-repair | Applying a user's config edit in place is the low-blast-radius path; a full restart is the escalation. A change that fails to parse or validate is never applied — the running config stays in force and the error is reported (§4.7). |
| HEAL-5 Traceable | Every reload plan (hot or restart) is logged with changed paths and chosen actions. |
| HEAL-8 Build-parity skew | A hot action that loads code (`reload-plugins`, `reload-hooks`) records the content identity it loaded, so the doctor's build-parity probe can report a process still holding an older build than the one on disk, instead of leaving it to surface as a mystery error. |
| SEC-5 No secret leakage in output | Changed config paths are never logged in full; only the key-path suffix is emitted, never the value. |
| SEC-10 Authority self-containment | Keys and files on the authority plane — sandbox policy, permission rules, autonomy level, egress and routing authorization, credential grants — are not applied from a watched file change. They take effect through the human-principal path that writes them; a watched change to them that the host did not author is drift, reported by the config integrity shields (`l2-security` §4.6) and never applied (§4.2). |
| INV-1 Embeddable core | Inherited, not realized here: this is a module inside the embeddable core, not the library boundary itself; it adds no frontend or platform dependency that would compromise embeddability. |
| INV-2 Logic in core only | This subsystem IS core logic and lives in the core; reconfiguration behavior is exposed only through the core, never pushed into a frontend. |
| INV-3 Frontend interchangeability / command parity | N/A: reload is an internal doctor operation, not a user-facing capability with a per-frontend surface. Were it ever surfaced manually, parity would be the frontend's concern, not this module's. |
| INV-4 Hub-and-spoke autonomy | N/A directly: reload runs wherever the engine runs, reconfiguring the always-on process on a hub and the foreground one on a spoke with identical logic. |
| INV-5 Durable, restartable state | N/A: reload re-reads configuration into a running process; configuration persistence and restart-recovery are owned by the storage/state layer, not by the reload planner. |
| INV-6 Graceful capability scaling | N/A: reload changes configuration, not the exposed capability set; a host's capability subset is decided by the core contract. |
| INV-7 Security of client data | Addressed by the SEC-5 row above: changed config paths are logged by key-path suffix only, never values, so reload never leaks a secret — the `l1-security` realization of INV-7's no-secret-egress rule for this subsystem. |
| INV-8 Single-deployable modular monolith | Reload and its restart escalation act on the single deployable: the "restart" path bounces the one core daemon process, not an orchestrated set of services — there is no service to roll independently. Config reload is an in-process reconfiguration of the modular monolith. |
| INV-9 Shipped-surface honesty | N/A: this module publishes no frontend verb; surface honesty is a frontend-spec concern. |
| INV-10 Representation isolation at the inward seam | Mostly N/A: the `ConfigReloadPlan` is a domain-side value; reading the config source is the adapter's job and its file/format representation stays on that side. This module names no storage or wire representation. |

## 4. Detailed Design

### 4.1 ConfigReloadPlan

The output of the reload planner — a structured decision about what needs to change:

```text
[REFERENCE]
ConfigReloadPlan {
  changed_paths:         Vec<String>,    // flat key-paths that differ
  restart_daemon:        bool,           // true iff any path maps to "restart"
  restart_reasons:       Vec<String>,    // human-readable reasons for restart
  hot_reasons:           Vec<String>,    // human-readable reasons for each hot action
  actions:               Vec<ReloadAction>,
  noop_paths:            Vec<String>,    // paths mapped to "none" — tracked for completeness
}

ReloadAction:
  | "reload-hooks"
  | "restart-cron"
  | "restart-heartbeat"
  | "restart-health-monitor"
  | "reload-plugins"
  | "dispose-mcp-runtimes"
  | "restart-channel:<channel_id>"
```

A plan with `restart_daemon = true` causes the daemon to perform a safe restart (drain active sessions, persist state, then respawn). Hot actions are dispatched in the order listed.

### 4.2 Reload rule table

Every config key-path is matched against a priority-ordered table of `ReloadRule` entries. The first matching prefix wins.

```text
[REFERENCE]
ReloadRule {
  prefix:  String,           // key-path prefix (e.g. "hooks", "agents.defaults.heartbeat")
  kind:    "restart" | "hot" | "none",
  actions: Vec<ReloadAction>, // only meaningful when kind = "hot"
}
```

Built-in rules (evaluated in order):

| Prefix | Kind | Actions |
| --- | --- | --- |
| `daemon.reload` | none | — |
| `daemon.remote` | none | — |
| `diagnostics.stuckSessionWarnMs` | none | — |
| `diagnostics.stuckSessionAbortMs` | none | — |
| `diagnostics.memoryPressureSnapshot` | hot | — |
| `hooks` | hot | `reload-hooks` |
| `agents.defaults.heartbeat` | hot | `restart-heartbeat` |
| `agents.defaults.models` | hot | `restart-heartbeat` |
| `agents.defaults.model` | hot | `restart-heartbeat` |
| `models.pricing` | restart | — |
| `models` | hot | — |
| `skills` | hot | *(skills snapshot invalidation — see §4.4)* |
| `channels.<id>` | hot | `restart-channel:<id>` |
| `mcp` | hot | `dispose-mcp-runtimes` |
| `plugins` | hot | `reload-plugins` |
| `cron` | hot | `restart-cron` |
| `scheduler` | hot | `restart-cron` |
| *(no match)* | restart | — |

Paths not matched by any prefix default to `restart`. This is the safe fallback: unknown config changes trigger a full restart rather than silent misapplication.

Authority-plane paths sit outside this table altogether. Whatever their prefix, a watched change to one is compared with what the host last wrote; if it differs, the plan carries it as `drift`, the integrity shields report it, and nothing is applied — hot or by restart. A restart must not be a way to load an authority change the agent could have written to disk.

### 4.3 Skills snapshot invalidation

Config paths under the `skills` prefix require special handling beyond a simple subsystem restart: every active session caches a tool-set snapshot derived from the skills config. When any `skills.*` path changes, the reload system bumps a monotonic `skills_snapshot_version` counter. Sessions compare their cached version on the next turn and rebuild their tool catalog if the version has advanced.

```text
[REFERENCE]
SKILLS_INVALIDATION_PREFIXES = ["skills"]

shouldInvalidateSkillsSnapshot(changed_paths: Vec<String>) -> bool:
  changed_paths.any(|p| p == "skills" OR p.starts_with("skills."))

// Called after plan is built, before dispatching hot actions:
if shouldInvalidateSkillsSnapshot(plan.changed_paths):
  global_skills_snapshot_version.fetch_add(1, Relaxed)
```

### 4.4 No-op detection

A plan where no action needs to be taken is detected before any subsystem is notified:

```text
[REFERENCE]
isNoopPlan(plan: ConfigReloadPlan) -> bool:
  !plan.restart_daemon
  AND plan.hot_reasons.is_empty()
  AND plan.actions.is_empty()
```

A no-op plan is logged at DEBUG and discarded; no subsystem events are emitted.

### 4.5 File watcher lifecycle

The config file is watched using the OS-native inotify/FSEvents/kqueue mechanism. Watcher errors (e.g. `EMFILE`, `ENOSPC`) are handled with bounded backoff and a polling fallback:

```text
[REFERENCE]
WATCHER_RECREATE_MAX_RETRIES = 3
WATCHER_RECREATE_BACKOFF_MS  = [500, 2_000, 5_000]

WatcherStatus: "active" | "polling" | "disabled"
  // "active"   — native OS watcher running
  // "polling"  — OS watcher failed; falling back to interval polling
  // "disabled" — all retries exhausted; reloads are no longer detected (operator must restart)
```

Recovery loop:

1. On watcher `error` event: log warning, close the watcher.
2. Wait `WATCHER_RECREATE_BACKOFF_MS[attempt]`; attempt re-creation.
3. If re-creation succeeds → `status = "active"`, reset retry counter.
4. If attempt fails and `attempt < WATCHER_RECREATE_MAX_RETRIES` → increment attempt, back to step 2.
5. If all retries exhausted → switch to polling mode (`status = "polling"`). Poll every `WATCHER_POLL_INTERVAL_MS` (default 5 000 ms) by diffing the config file mtime or hash.
6. If polling mode also fails → `status = "disabled"`. Log an ERROR with a human-readable alert; reloads will not fire until the daemon is restarted.

The environment variable `CONFIG_WATCHER_POLL=1` forces polling mode from startup, bypassing the native watcher (useful in container or test environments with limited inotify quotas).

### 4.6 Hot-reload status

The reload status is exposed through the health and diagnostics APIs:

```text
[REFERENCE]
HotReloadStatus {
  watcher_status: WatcherStatus,   // "active" | "polling" | "disabled"
  last_reload_at?: Timestamp,      // when the most recent reload plan was dispatched
  last_plan?: {
    changed_paths: Vec<String>,    // redacted: values omitted
    actions:       Vec<String>,
    restart:       bool,
  }
}
```

### 4.7 Reload sequence

When the watcher detects a file change:

1. Read new config from disk; deserialize and validate. If either fails, keep the running config, record the error (surfaced by the doctor's `[config]` probe), and stop — a half-read file is never applied.
2. Diff against the in-memory current config snapshot → `changed_paths`.
3. Set aside authority-plane paths as drift (§4.2); match each remaining path against the rule table → build `ConfigReloadPlan`.
4. If `isNoopPlan(plan)` → log DEBUG and return.
5. If `restart_daemon` → initiate the safe restart: drain and checkpoint as `l2-office-control` describes for a pause (office-control OC-1), then respawn.
6. Otherwise:
   a. Emit `skills snapshot invalidation` if applicable (§4.3).
   b. Dispatch hot actions to subsystem service bus in order.
   c. Log INFO with `changed_paths` (key-paths only, no values) and `actions`.
   d. Update `last_reload_at` and `last_plan` in `HotReloadStatus`.

## 5. Drawbacks & Alternatives

- **Last-match rule table vs first-match:** using first-match means a more specific prefix must appear before a broader one in the table. The built-in table is ordered correctly; custom rules added at the front take precedence.
- **Polling fallback is reactive, not proactive:** polling catches changes on the next interval, not immediately. Polling is an emergency fallback; the native watcher handles normal operation.
- **Alternative — full restart on every config change:** simpler but drops sessions. Hot-reload is worth the complexity for a production daemon.
- **Alternative — apply all changes without a rule table:** unsafe; some fields (e.g. model pricing) can only be correctly applied at startup.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[DOC]` | `.design/main/specifications/l1-doctor.md` | Self-healing principles |
| `[ARCH]` | `.design/main/specifications/l1-architecture.md` | Daemon architecture |
| `[HOOKS]` | `.design/main/specifications/l2-plugin-hooks.md` | File hook reload trigger |
| `[SCHED]` | `.design/main/specifications/l2-scheduler.md` | Cron service restart trigger |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.4 | 2026-09-24 | Core Team | Consistency pass (2026-09-24): A bare `OC-1` citation of the office-control invariant now names it. |
| 1.0.3 | 2026-09-23 | Core Team | Consistency pass (2026-09-23): Compliance cited DOC-2/DOC-4 (the doctor prefix is HEAL) — now HEAL-2/HEAL-5, plus HEAL-8 for code-loading hot actions. Security: a watched change to any config path was applied, including authority-plane keys an agent able to write the file could change (SEC-10) — those are held back as drift for the integrity shields, never applied hot or by restart. An unparseable or invalid file is never applied. The safe-restart reference pointed at the doctor's runbook — it is the office-control drain and checkpoint. A foreign `hooks.gmail` rule removed. |
| 1.0.2 | — | Core Team | Last version before this section was added; earlier revisions are recorded in version control. |
