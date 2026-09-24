# Scheduler

**Version:** 1.0.5
**Status:** Stable
**Layer:** implementation
**Implements:** l1-scheduler-model.md

## Overview

The concrete scheduler: a friendly recurrence model (alarm-clock style presets) with an optional raw cron expression for power users, per-workspace storage, the timezone/firing semantics, and the schedule command surface across CLI / TUI / library. Board de-duplication for routine fires is deferred per the model.

## Related Specifications

- [l1-scheduler-model.md](l1-scheduler-model.md) - The model this implements.
- [l2-filesystem-layout.md](l2-filesystem-layout.md) - The `schedules/` location within a workspace.
- [l2-kanban-board.md](l2-kanban-board.md) - Where `routine` fires may place cards.
- [l2-cli.md](l2-cli.md) - Command grammar standard the schedule commands follow.
- [l2-security.md](l2-security.md) - Prompt injection scanning and sandbox constraints applied at fire time.
- [l2-agent-autonomy.md](l2-agent-autonomy.md) - The autonomy gate every fired job still passes; its unattended rows refuse what they cannot ask (AG-9).
- [l2-trigger-triage.md](l2-trigger-triage.md) - Intake of cron, webhook and event fires; a fire's declared action is routed, never re-classified (SCH-3).
- [l1-security.md](l1-security.md) - SEC-10: a webhook trigger opens an ingress path and is therefore human-authored.

## 1. Motivation

The model wants recurrence-first scheduling that a non-technical client can use, while still allowing exact control. A friendly schedule object covers the common cases; an optional cron field serves the rest. File-backed per-workspace storage keeps schedules isolated and inspectable.

## 2. Constraints & Assumptions

- Per-workspace storage under `<ws>/schedules/`.
- Friendly recurrence and raw cron are mutually exclusive on a single schedule.
- Firing uses the host clock and the schedule's timezone (default: host timezone).
- The frontend holds no logic; scheduling is a core service (INV-2).

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| SCH-1 Two kinds | `kind: recurring\|oneshot`; one-shot sets`delete_after_fire: true` and is removed after firing. |
| SCH-2 Recurrence expressiveness | `recurrence` presets (weekdays/weekends/daily/days/interval + times) OR a raw `cron` string. |
| SCH-3 Single declared action | `action: heartbeat\|routine\|reminder` — exactly one per schedule. |
| SCH-4 Wake produces no card | `heartbeat` fires call only the office wake entry point; no board API is invoked. |
| SCH-5 Workspace-scoped | Schedule files live under `<ws>/schedules/`; firing targets that office only. |
| SCH-6 Autonomous & durable | Schedules persist as files; the scheduler service reloads and re-arms them on restart. |
| SCH-7 Lifecycle control | `enabled` flag plus create/edit/delete operations; disabling stops firing without deletion. |
| SCH-8 Bounded catch-up window | Optional per-schedule `catch_up_window` (§4.7): a missed occurrence is eligible for catch-up only while `recovery_time ≤ fire_at + catch_up_window`; absent = zero width, so every missed occurrence is dropped whatever the `catchUpPolicy`. A catch-up run is recorded `fired_late: true` with its intended `fire_at_ms` (§4.9 run log). Pending realization. |

## 4. Detailed Design

### 4.1 Schedule object (conceptual)

```text
[REFERENCE]
{
  id, name,
  kind: "recurring" | "oneshot",
  action: "heartbeat" | "routine" | "reminder",
  recurrence: {                 // friendly model (mutually exclusive with cron)
    preset: "weekdays" | "weekends" | "daily" | "days" | "interval",
    days?: ["mon","tue", ...],  // for preset "days"
    times?: ["09:00", "18:30"], // local times
    interval?: "15m"            // for preset "interval" (heartbeat cadence)
  },
  cron?: "0 9 * * 1-5",         // advanced: raw cron, instead of recurrence
  at?: "2026-07-01T09:00",      // for oneshot
  start_at?, end_at?,           // optional window (when the schedule as a whole is active)
  catch_up_window?,             // SCH-8 per-occurrence lateness bound (§4.7); absent = drop-if-missed
  timezone,                     // default: host
  enabled: true,
  delete_after_fire: false,     // true for oneshot (SCH-1)
  repeat?: {                    // bounded recurrence; absent = unlimited
    times: u32 | null,          // total run cap; null = unlimited
    completed: u32              // runs completed so far (runtime-managed, not user-set)
  },
  deliver?: String[],           // ordered delivery target IDs resolved at fire time (§4.9)
  skills?: String[],            // skill IDs injected into the session context at fire time
  script?: String               // optional script/workflow run instead of the LLM prompt
}
```

### 4.2 Storage

```plaintext
<ws>/schedules/
└── <schedule-id>.json   # one schedule per file
```

### 4.3 Firing

The scheduler service evaluates due schedules against the host clock + timezone, fires the declared action, and for one-shot schedules deletes the file after firing. On restart it reloads all files and re-arms (SCH-6). Occurrences missed during downtime follow the catch-up rules of §4.7: the SCH-8 `catch_up_window` bounds how late a missed occurrence may still run, and `catchUpPolicy` chooses how many of the eligible ones replay.

### 4.4 Board interaction

`heartbeat` never touches the board (SCH-4). `routine` may create a board card. Overlap between *runs* is governed by the concurrency policy of §4.7 (a new fire while the previous run is still active). Overlap between *cards* — a new fire while the card an earlier, already-finished run created is still open — is not covered by that policy and stays deferred, as the model defers it (`l1-scheduler-model` §5).

### 4.5 Command surface

Schedule operations across all three surfaces, conforming to the CLI grammar standard (verb-first, explicit verbs; see `l2-cli.md` §4.4). The library method is the source.

| Action | CLI | TUI | Library (no code) |
| --- | --- | --- | --- |
| create (friendly) | `cronus schedule create <name> --action <a> --when <preset> [--time HH:MM] [--days mon,tue] [--every 15m]` | `/schedule create …` | `schedule.create(name, spec) -> Schedule` |
| create (one-shot) | `cronus schedule create <name> --action <a> --once --at <datetime>` | `/schedule create … --once` | `schedule.create(name, spec) -> Schedule` |
| create (raw cron) | `cronus schedule create <name> --action <a> --cron "<expr>"` | `/schedule create … --cron …` | `schedule.create(name, spec) -> Schedule` |
| list | `cronus schedule list` | `/schedule list` | `schedule.list() -> Schedule[]` |
| show | `cronus schedule show <id>` | `/schedule show <id>` | `schedule.get(id) -> Schedule` |
| set | `cronus schedule set <id> [--when …] [--cron …] [--time …] [--enabled true\|false]` | `/schedule set <id> …` | `schedule.update(id, patch) -> Schedule` |
| enable | `cronus schedule enable <id>` | `/schedule enable <id>` | `schedule.enable(id) -> Schedule` |
| disable | `cronus schedule disable <id>` | `/schedule disable <id>` | `schedule.disable(id) -> Schedule` |
| delete | `cronus schedule delete <id>` | `/schedule delete <id>` | `schedule.delete(id) -> void` |
| run now | `cronus schedule run <id>` | `/schedule run <id>` | `schedule.runNow(id) -> void` |

### 4.6 Security constraints for fired jobs

Scheduled jobs run in a non-interactive context with no answering surface — there is no human in the loop to catch a bad action, and nothing is auto-approved to keep the run moving: every tool call still passes the autonomy gate, whose unattended rows turn each `Prompt` into a visible refusal and always refuse `destructive` (`l2-agent-autonomy` §4.3, AG-9). Three further constraints are enforced unconditionally at fire time:

**Anti-recursion guard**
A cron-spawned agent always has the `cronjob` toolset disabled. This prevents a fired job from scheduling additional cron jobs — an attack surface for exponential schedule growth via prompt injection in a workflow or skill.

**Fire-time prompt injection scanning**
The fully-assembled prompt is scanned for injection patterns at fire time, not just at create time. A job may load skill content that was not present when the job was created; that content could carry an injection payload. If scanning detects a violation, the job raises `CronPromptInjectionBlocked` and the delivery platform receives a "job blocked" notification instead of executing.

**Per-job toolset scoping**
Toolsets available to a fired job follow this precedence:

1. Per-job `enabled_toolsets` (set at create/update time) — job-scoped override.
2. Platform-level toolset config for the `cron` platform — mirrors gateway behavior.
3. Full default toolset — fallback only.

User-level `disabled_toolsets` from the global agent config is layered on top of all of the above; a per-job override cannot widen past what the global config denies.

The `messaging` and `clarify` toolsets are always disabled in cron context: `messaging` requires a live gateway session; `clarify` blocks waiting for user input — both are incompatible with unattended execution.

### 4.7 Routine execution policy

Routines (recurring `action: "routine"` schedules) carry execution policy that governs what happens when a new fire is due while a previous run is still active, and what happens to fires that were missed during downtime.

#### Concurrency policy

```text
[REFERENCE]
Schedule.concurrencyPolicy: "coalesce_if_active" | "run_parallel" | "skip_if_active"
```

| Policy | Behavior |
| --- | --- |
| `coalesce_if_active` (default) | If a run is still active, the new fire is merged into it (recorded as a coalesced trigger). No new run is started. |
| `run_parallel` | A new run starts regardless; concurrent runs are allowed for this routine. |
| `skip_if_active` | If a run is still active, the new fire is silently dropped (logged but no action). |

#### Catch-up policy

```text
[REFERENCE]
Schedule.catchUpPolicy: "skip_missed" | "run_once" | "run_all"
```

```text
[REFERENCE]
Schedule.catch_up_window?: Duration   // SCH-8 lateness bound; absent = zero width (drop-if-missed)

on recovery at time T:
  eligible := missed occurrences with fire_at ≤ T ≤ fire_at + catch_up_window
  dropped  := all other missed occurrences            // genuine misses — never fired stale
  apply catchUpPolicy to `eligible` only
```

| Policy | Behavior after downtime (applies only to occurrences still inside their catch-up window) |
| --- | --- |
| `skip_missed` (default) | Every missed occurrence is dropped; only the next scheduled fire runs. |
| `run_once` | One catch-up run is started immediately on recovery for the latest eligible occurrence, then normal cadence resumes. |
| `run_all` | Every eligible occurrence is replayed in sequence. Use with caution — can produce burst load. |

The window bounds lateness and the policy chooses replay breadth (SCH-8): with no window declared, `run_once` and `run_all` have nothing eligible and behave as `skip_missed`, so drop-if-missed stays the default and a catch-up is always an explicit opt-in. A catch-up run is recorded `fired_late: true` and carries its intended `fire_at_ms`; a late reminder says it is late.

#### Idempotency and dispatch fingerprint

To prevent exact-duplicate runs when the scheduler restarts or a cluster has split-brain, each dispatched run carries:

```text
[REFERENCE]
RoutineRun.idempotencyKey: String    // unique per (schedule_id, scheduled_fire_at) pair
RoutineRun.dispatchFingerprint: String // hash of (schedule_id, fire_at, concurrency_policy)
RoutineRun.coalescedIntoRunId?: String // set if this fire was merged into an existing run
```

Before dispatching, the scheduler records the `idempotencyKey` with a single atomic insert-if-absent in the durable run log; if the key is already present, dispatch is a no-op (idempotent re-fire). A `(schedule_id, scheduled_fire_at)` pair therefore dispatches **at most once**, even across restarts. A crash after the key is recorded but before the run finishes is surfaced as an interrupted run on recovery — it is not silently re-dispatched, and it is not reported as delivered.

#### Webhook triggers

Routines may also be triggered by an inbound HTTP request in addition to (or instead of) the cron expression:

```text
[REFERENCE]
ScheduleTrigger {
  kind: "cron" | "webhook",
  cronExpression?: String,       // for kind = "cron"
  publicId?: String,             // public endpoint path component (webhook URL)
  secretId: String,              // signing key stored in the secret store (required for kind = "webhook")
  signingMode: "hmac_sha256",    // the only accepted mode — an unsigned webhook is refused at creation
  replayWindowSec: u32           // reject requests older than this; default 300
}
```

Webhook validation (every step fail-closed; a rejected request never reaches triage):

1. Recompute HMAC-SHA256 over the raw body with the key named by `secretId` and compare it with the `X-Cronus-Signature` header in constant time; a missing or mismatched signature is rejected.
2. Read the timestamp from the signed body and reject it if it is older than `replayWindowSec` seconds or in the future beyond the same tolerance — the signature covers the timestamp, so it cannot be refreshed by a replayer.
3. Dispatch the routine as a one-shot run whose `idempotencyKey = sha256(schedule_id ‖ raw_body)`: a replay of the same signed body inside the window is a no-op, while the time-based key of §4.7 continues to govern cron fires.

A webhook trigger opens a new ingress path, which is part of the reachability half of the authority plane (`l1-security` SEC-10): creating or enabling one is a human-authored act, an agent may only request it, and a job running under the scheduler cannot create one (the anti-recursion guard of §4.6 already withholds the `cronjob` toolset). An unsigned mode is not offered: without a signature, anyone able to reach the endpoint could start the routine, and the replay window would bound nothing. The signing key is stored in the OS keychain under the same mechanism as `l2-security.md §4.1`; it is never logged or exported. Webhook triggers share the same concurrency and catch-up policies as cron triggers.

### 4.8 Event-driven task triggers

In addition to time-based schedules, a task can be configured to fire when an observed metric or signal crosses a threshold. Event-driven triggers share the same dispatch path as cron triggers but replace `cronExpression` with an event definition.

#### EventTrigger format

```text
[REFERENCE]
EventTrigger {
  trigger_type: "event",
  event_kind: String,         // named event class, e.g. "card_status_changed"
  event_filter: JSON?,        // optional match predicate applied to the event payload
  trigger_count: u32,         // fire when counter reaches this threshold (default 1)
  counter_key: String?,       // key used to accumulate count; None = each event counts as 1
  reset_on_fire: bool,        // reset counter to 0 after firing (default true)
  dedup_window_sec: u32,      // singleflight window: suppress duplicate fires within this period (default 0 = off)
  dedup_cache_key: String?,   // key for the singleflight cache; defaults to (schedule_id, event_kind)
}
```

The `trigger_count` pattern lets an event accumulate before firing. For example, a task with `trigger_count = 5` on `event_kind = "tool_call_blocked"` fires once per five matching blocks — useful for rate-anomaly detection without firing on every isolated incident. The count is cumulative, not consecutive: a non-matching event neither increments nor resets it.

#### Counter increment

The counter is incremented in the schedule's durable state on each matching event:

```text
[REFERENCE]
on_event(event):
  if not matches(event, trigger.event_filter):
    return
  counter = load_counter(trigger.counter_key ?? schedule_id)
  counter += 1
  if counter >= trigger.trigger_count:
    maybe_fire(schedule, event)
    if trigger.reset_on_fire:
      counter = 0
  store_counter(trigger.counter_key ?? schedule_id, counter)
```

Counter state is persisted in `schedules/<id>/counter.json` alongside the schedule file, using the same atomic-write protocol as auth state.

#### Singleflight dedup cache

When multiple events arrive simultaneously (e.g. a burst of `card_status_changed` events from a batch operation), the dedup cache prevents duplicate fires within the `dedup_window_sec` window:

```text
[REFERENCE]
singleflight_cache: HashMap<String, Instant>
  key = dedup_cache_key ?? "{schedule_id}:{event_kind}"
  if cache[key] exists and elapsed < dedup_window_sec:
    log "Event trigger suppressed (singleflight dedup)"
    return
  cache[key] = now()
  dispatch(schedule)
```

The cache is in-memory, per-process. On process restart, the window resets — a short burst immediately after restart can re-fire. This is acceptable for the use case (dedup is best-effort, not idempotency).

#### Interaction with routine policies

Event-triggered fires reuse the same `concurrencyPolicy` and `catchUpPolicy` fields as cron-triggered fires. An event-driven task with `concurrencyPolicy = "coalesce_if_active"` will merge into an already-running instance rather than spawning a second one.

#### Audit trail

Every event-driven fire appends an entry to the audit log:

```text
[REFERENCE]
{ timestamp, schedule_id, trigger_type: "event", event_kind, counter_value, outcome: "fired" | "dedup_suppressed" | "counter_incremented" }
```

### 4.9 Cron isolated session execution

Each fired scheduled job runs in a **dedicated, isolated session** rather than sharing the agent's main interactive session. This keeps cron execution from interfering with the user's live conversation and allows job-specific model and tool configuration.

#### Session key

```text
[REFERENCE]
cron_session_key(workspace_id: String, schedule_id: String, fire_at_ms: u64) -> String:
  "cron:{workspace_id}:{schedule_id}:{fire_at_ms}"
```

The key is deterministic: the same schedule fired at the same `fire_at_ms` always maps to the same session key. Combined with the idempotency key from §4.7, this prevents duplicate session creation under restart conditions.

#### Model preflight

Before spawning the session, the scheduler verifies that the designated model is reachable:

```text
[REFERENCE]
CronModelPreflightResult: "ok" | "unavailable" | "auth_error"

preflight(job: CronJob) -> CronModelPreflightResult:
  provider = resolveProvider(job.model_override ?? workspace.default_model)
  result = provider.probe(timeout_ms = 5_000)
  if result == ok: return "ok"
  if result is auth failure: return "auth_error"
  return "unavailable"
```

On `"unavailable"`: fall back through the model fallback cascade (see `l2-model-error-recovery.md §4.1`).
On `"auth_error"`: skip this fire and emit `CronModelAuthAlert`; the job is not retried until the next scheduled fire.

#### Run timeout

```text
[REFERENCE]
CronJob.run_timeout_secs: u32   // default 3600 (1 hour); 0 = unlimited
```

If the session is still running after `run_timeout_secs`, the session is aborted and the run is recorded as `timed_out`. The next scheduled fire starts fresh.

#### Run log

Every execution is appended to a per-schedule run log:

```text
<ws>/schedules/<schedule_id>/runs.jsonl
```

Each line (JSON):

```text
[REFERENCE]
{ run_id, session_key, started_at_ms, ended_at_ms?, status: "running"|"ok"|"error"|"timed_out"|"blocked"|"interrupted",
  model, fire_at_ms, fired_late: bool, idempotency_key, error_summary? }
```

The run log is pruned to the last `N` entries (default 100) when the scheduler reloads on startup, preventing unbounded growth.

#### Delivery dispatch

On successful completion, the session's final reply is dispatched to the job's configured delivery targets:

```text
[REFERENCE]
CronDeliveryTarget:
  | { kind: "announce" }                       // no result delivery: the run's outcome is recorded in the run log only
  | { kind: "local" }                          // surface result in the user's active UI session
  | { kind: "session", session_key: String }   // inject result into a named session via the Inbox
```

The `deliver` field on the job (§4.1) is an ordered list of target identifiers. `"local"` and `"announce"` are literal identifiers; `"session:<key>"` maps to `{ kind: "session", session_key: <key> }`. Multiple targets receive the result in order.

The `"session"` target injects the result via the Inbox system (see `l2-inbox.md`), allowing a background cron result to surface in a named conversation session.

#### Failure notification

```text
[REFERENCE]
CronFailurePolicy {
  notify_on: "error" | "timed_out" | "blocked" | "all",
  alert_delivery: CronDeliveryTarget,   // where to send the alert
}
```

When a run ends in a failure state matching `notify_on`, a `CronRunFailedAlert` message is dispatched to `alert_delivery`. The alert includes: `schedule_id`, `schedule_name`, `status`, `error_summary`, `started_at_ms`, `ended_at_ms`.

## 5. Drawbacks & Alternatives

- **Two recurrence representations (friendly + cron):** a small translation/validation cost; justified by serving both audiences (SCH-2).
- **File-per-schedule:** simple and inspectable; if schedules grow large, an index or SQLite-backed store can be introduced later (consistent with STO-8).
- **Card-level de-duplication deferred:** run overlap is settled by `concurrencyPolicy` (§4.7), but a new fire while an earlier run's card is still open can still add a second card — an accepted risk until tuned in real use (§4.4, `l1-scheduler-model` §5).
- **Always-disabled toolsets (cronjob/messaging/clarify):** not configurable by design — the non-interactive execution context makes these toolsets structurally unsafe in cron.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[MODEL]` | `.design/main/specifications/l1-scheduler-model.md` | Invariants this scheduler satisfies |
| `[LAYOUT]` | `.design/main/specifications/l2-filesystem-layout.md` | `schedules/` location in a workspace |
| `[CLI]` | `.design/main/specifications/l2-cli.md` | Command grammar standard |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.5 | 2026-09-23 | Core Team | Consistency pass (2026-09-23): SCH-8 compliance row added and the stale §4.3 missed-fire TBD resolved — `catch_up_window` bounds lateness and `catchUpPolicy` only chooses replay breadth among still-eligible occurrences, so `run_once`/`run_all` can no longer fire stale; catch-up runs are recorded `fired_late`. §4.4/§5 no longer claim the concurrency policy settles card de-duplication (run overlap vs card overlap). §4.6 no longer calls cron an auto-approved context (AG-9 refusal rows of `l2-agent-autonomy`). Webhooks: unsigned mode removed, constant-time HMAC verification and signed-timestamp replay window made explicit, webhook creation is a SEC-10 ingress write; idempotency is an atomic insert-if-absent giving at-most-once dispatch (the exactly-once claim is withdrawn); event counter is cumulative, not consecutive. |
| 1.0.4 | — | Core Team | Last version before this section was added; earlier revisions are recorded in version control. |
