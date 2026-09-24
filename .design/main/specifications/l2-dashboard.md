# Dashboard

**Version:** 1.0.3
**Status:** Stable
**Layer:** implementation
**Implements:** l1-dashboard.md

## Overview

The concrete dashboard: which metrics it computes, the state it reads them from, the per-office and home building-aggregate views, where view layout persists, and the `dashboard` command.

## Related Specifications

- [l1-dashboard.md](l1-dashboard.md) - The model this implements.
- [l2-kanban-board.md](l2-kanban-board.md) - Board state for work metrics.
- [l2-app-ui.md](l2-app-ui.md) - The Dashboard surface in the app shell.
- [l2-cli.md](l2-cli.md) - Command grammar standard.
- [l2-navigation.md](l2-navigation.md) - The canonical navigation this surface belongs to, and its two facets (NV-10).
- [l2-budget-engine.md](l2-budget-engine.md) - The per-call cost events usage analytics are recorded from.

## 1. Motivation

The model needs concrete metric definitions and sources so the dashboard is a faithful, live projection that costs nothing to keep consistent.

## 2. Constraints & Assumptions

- Metrics are computed on read from existing state; only cosmetic layout is stored under `<ws>/dashboard/`.
- The home aggregate reads across `<state>/workspaces/*` read-only.
- The frontend renders; metric computation is a core call (INV-2).

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| DSH-1 Projection | Metrics computed from board, sessions, cost ledger, schedules, memory; nothing authoritative stored. |
| DSH-2 Live | Recomputed on state-change events / refresh. |
| DSH-3 Per-office + building | `dashboard show` for the active office; in home, an aggregate across all offices. |
| DSH-4 Observational | Read-only; no office operations from the dashboard. |
| DSH-5 Privacy | Metrics local; sharing only via telemetry opt-in. |
| DSH-6 Isolation | Office dashboard reads only its workspace; building aggregate reads across offices read-only. |

## 4. Detailed Design

### 4.1 Metrics and sources

| Metric | Source |
| --- | --- |
| cards by state, throughput, cycle time, blocked | `<ws>/kanban/` |
| active agents, running tasks, recent sessions | roster + `<ws>/sessions/` |
| cost/budget usage | cost ledger (per office/agent budgets) |
| schedule status (upcoming/overdue, heartbeat) | `<ws>/schedules/` |
| memory size by scope, recent learnings | memory stores |

### 4.2 Layout storage

```plaintext
<ws>/dashboard/
└── layout.json   # cosmetic widget arrangement; presentation only
```

### 4.3 Command surface

| Action | CLI | TUI | Library (no code) |
| --- | --- | --- | --- |
| show office dashboard | `cronus dashboard` | `/dashboard` | `dashboard.show() -> Dashboard` |
| building aggregate (home) | `cronus dashboard building` | `/dashboard building` | `dashboard.building() -> Dashboard` |

### 4.4 Facets

The dashboard is one surface of the canonical navigation, and its sub-navigation is the facet pair the navigation model fixes for it (`l1-navigation-model` §4.5, `l2-navigation` NV-10): **Agent Statistics** and **Token Usage**. It does not host a navigation of its own. The active facet persists in `layout.json` as a preference.

```text
[REFERENCE]
Agent Statistics — cards by state, throughput, cycle time, blocked work; active agents,
                   running tasks, recent sessions; the per-agent health score (§4.5) and
                   alerts (§4.6); next scheduled run; message / session / call trends.
Token Usage      — model spend by model and by date, and usage trends (§4.8).
```

Every panel is read-only (DSH-4). Where a panel concerns something the user can act on — a skill, a knowledge page, a scheduled job, a setting — it links to the surface that owns that action (Wiki, Memory, Schedule, Automation, Settings); the dashboard itself runs, ingests, pauses, and edits nothing.

### 4.5 Session health score

The dashboard computes a health score [0–100] for each active role/agent instance. The score is a read-only diagnostic — it is never stored; always recomputed from live state.

```text
[REFERENCE]
health_score(profile) -> u8:
  score = 100

  // Error log pressure
  if error_lines > 100: score -= 15
  elif error_lines > 50: score -= 10
  elif error_lines > 20: score -= 5

  // Memory/process pressure
  if memory_mb > 300: score -= 20
  elif memory_mb > 200: score -= 10

  // Engagement (messages per session ratio)
  if session_count > 0 AND (message_count / session_count) < 5:
    score -= 10

  // Activity trend (last 7 days vs prior 7 days)
  if trend_window >= 14 days:
    recent_avg = avg(daily_sessions, last 7 days)
    prior_avg  = avg(daily_sessions, days 8–14)
    if prior_avg > 0 AND (recent_avg / prior_avg) < 0.5:
      score -= 15

  return clamp(score, 0, 100)

Color coding:
  score >= 80  → green
  score >= 60  → amber
  score <  60  → red
```

### 4.6 Alert thresholds

Alerts are surfaced in the Agent Statistics facet. Each alert has a severity, a human-readable title/detail pair, the metric value, and the threshold that triggered it.

```text
[REFERENCE]
Alert { severity: critical | warning | info, title, detail, metric, current, threshold }

Threshold table:
  memory_mb > 300      → critical  "Memory Pressure"      detail: risk of OOM
  memory_mb > 200      → warning   "High Memory Usage"    detail: monitor closely
  error_lines > 100    → warning   "Elevated Errors"      detail: review for patterns
  session_count == 0   → info      "Idle Agent"           detail: hired but given no work
  activity_drop > 50%  → warning   "Activity Drop"        detail: session avg dropped N%

Sorting: critical first, then warning, then info; same-severity sorted by recency.
Display cap: show up to 10 alerts in the UI; the full list is available via dashboard.alerts().
```

### 4.7 Graceful degradation (engine not running)

When the engine is not running, the dashboard still asks the **core** — never the files. The core library opens the office's stores read-only through its own adapters (the read pool of `l2-technology-stack` §4.7) and returns the same projections; the frontend never parses a database or state file itself (INV-2, INV-10).

```text
[REFERENCE]
Degradation contract:
  - A banner states the engine is not running and the time of the data shown.
  - Every panel renders from the read-only projection; nothing is fabricated.
  - Links to action surfaces stay, and those surfaces show their actions as unavailable
    (disabled, with the reason) until the engine runs.
  - Health score and alerts are suppressed (their inputs are live signals) and replaced
    with "Engine not running — start it for live health data".
```

### 4.8 Usage analytics

Usage analytics are recorded by the **core's usage recorder**, not by the dashboard — the dashboard is a projection and stores nothing but layout (DSH-1). The recorder takes its numbers from the same per-call cost events the budget engine receives (`l2-budget-engine` §4.2), so the dashboard's spend and the budget engine's spend cannot disagree. No user content is recorded — only usage numbers.

```text
[REFERENCE]
Recorded per office, per session, per model:

  Token counters:
    - input_tokens_used, output_tokens_used
    - cache_read_tokens       : tokens served from the provider's prompt cache
    - tool_call_count         : tool invocations issued by agents

  Model distribution:
    - model_id → {input, output, tool_call_count}
    - reveals which roles / task types are dominated by expensive models

  Cost estimate:
    - derived from the cost events; the rate table is configurable
      (Local Settings → Office → Budget); display only, never billing

  Time series (last 100 sessions per office):
    - session_id, started_at, ended_at, total_tokens, total_cost

  Storage:
    - in the office's state (per-office rows; the building aggregate reads across
      offices read-only, DSH-6); flushed at session close and every 30 s while active
    - the doctor and the inner monologue read the recorder, not the dashboard
```

Shown in the Token Usage facet; accessible via `dashboard analytics [--session <id>]`. Feeds: self-improvement calibration data, model-router cost scoring, doctor anomaly detection. Nothing here leaves the device unless the user has opted into telemetry (DSH-5).

## 5. Drawbacks & Alternatives

- **Recompute cost on busy offices:** mitigated by event-driven incremental metric updates.
- **Alternative — persist computed metrics:** rejected; storing derived numbers risks drift (DSH-1). Only layout persists.
- **Analytics storage growth:** mitigated by keeping the last 100 sessions per office; older entries are pruned on flush, and the Token Usage facet states that horizon.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[DASHBOARD]` | `.design/main/specifications/l1-dashboard.md` | Invariants this implements |
| `[BOARD]` | `.design/main/specifications/l2-kanban-board.md` | Work-metric source |
| `[CLI]` | `.design/main/specifications/l2-cli.md` | Command grammar standard |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.3 | 2026-09-24 | Core Team | Consistency pass (2026-09-24): §4.4 defined a private navigation (Today, Skills, Knowledge, Journal, Sources, Automations, Settings) that ran skills, ingested files and paused jobs — contradicting DSH-4 (read-only) and the canonical navigation, whose Dashboard facets are Agent Statistics and Token Usage (NV-10); it also carried another product's vault and bridge concepts. Replaced by the two read-only facets, linking to the surfaces that own actions. The offline mode read the core's files directly from the frontend (INV-2, INV-10) — it now asks the core library for read-only projections. Usage analytics were collected and stored by the dashboard (DSH-1) — they are recorded by the core's usage recorder from the budget engine's cost events, per office. |
| 1.0.2 | — | Core Team | Last version before this section was added; earlier revisions are recorded in version control. |
