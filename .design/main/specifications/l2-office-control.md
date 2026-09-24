# Office Control

**Version:** 1.0.1
**Status:** Stable
**Layer:** implementation
**Implements:** l1-office-control.md

## Overview

The concrete realization of office control in `crates/core`: an `OfficeState` machine that drives start/pause/resume/hibernate transitions, a cooperative drain-and-checkpoint protocol, the token-exhaustion hibernation ladder (model substitution before hibernation, automatic resource-recovery wake), and per-subsystem pause toggles. All logic lives in the core; frontends (Tauri master switch, TUI/CLI verbs) are thin bindings that call the control service and render its state (INV-2, INV-3).

## Related Specifications

- [l1-office-control.md](l1-office-control.md) — the model this implements (OC-1…OC-5).
- [l2-budget-engine.md](l2-budget-engine.md) — an office-scope hard stop and its reset or raise are one source of the `quota-exhausted` / `quota-recovered` events (§4.3).
- [l2-model-error-recovery.md](l2-model-error-recovery.md) — the other source: a provider's `Billing` / `RateLimit` classification once rotation and fallback are exhausted.
- [l2-model-router.md](l2-model-router.md) — fallback cascade queried for a substitute model before hibernation (OC-3).
- [l2-orchestration.md](l2-orchestration.md) — coordinates worker drain to a safe checkpoint (OC-1).
- [l2-scheduler.md](l2-scheduler.md) — cron suppression on Paused/Hibernating; reschedules from last-triggered-at on wake.
- [l2-session-checkpoint.md](l2-session-checkpoint.md) — checkpoint format reused for drain-state persistence (OC-1/OC-2).
- [l2-app-ui.md](l2-app-ui.md) — renders the master switch and OfficeState status icon (OC-5).

## 1. Motivation

The model requires clean pause/resume with no lost work and an automatic response to resource exhaustion. A core-owned state machine keeps the transition logic single-sourced across all three frontends; a cooperative drain (rather than a kill) satisfies the no-work-lost contract; delegating substitution to the model-router avoids re-implementing fallback logic here.

## 2. Constraints & Assumptions

- The control service is the sole writer of `OfficeState`; frontends read it and request transitions, never set it directly.
- Drain is cooperative: the service signals workers and waits for atomic-step acknowledgement, bounded by `DRAIN_TIMEOUT_MS` (default 30_000) after which a `PARTIAL` checkpoint is written. The timeout bounds the wait, never the step (OC-1, §3).
- Hibernation subscribes to `quota-exhausted` / `quota-recovered` events — raised by the budget engine (an office-scope hard stop; a reset or raise) and by error recovery (a provider's `Billing` / `RateLimit` classification after rotation and fallback are exhausted, recovering at the provider's reset time) — and never polls a model provider directly, apart from the §3 OC-4 backstop.
- A paused/hibernating office still answers status queries; it rejects new task intake with a typed `OFFICE_NOT_ACCEPTING` result.

## 3. Invariant Compliance (Layer 2)

| L1 Invariant | Implementation |
| --- | --- |
| OC-1 Safe-checkpoint before freeze | `pause()`/`hibernate()` broadcast a drain signal via the orchestration bus; each worker completes its current atomic step (nodus executor step boundary), writes a `session-checkpoint`, then acks. The service transitions to the frozen state after all acks or at `DRAIN_TIMEOUT_MS`, whichever is first. The timeout bounds the wait, not the step: from that moment no new step starts and intake stops, while a step that has not acked runs to its boundary and then writes its checkpoint, replacing that unit's `PARTIAL` marker. No step is interrupted mid-execution. |
| OC-2 Exact-state resume | `resume()` restores each worker from its checkpoint and re-enqueues the interrupted unit at its recorded step. A per-unit `claim` (work-liveness WL-1) prevents duplicate resume; the checkpoint's `run_id` guards against re-start-from-scratch. |
| OC-3 Model degradation before hibernation | On a `quota-exhausted{model}` event, the service calls `model_router.substitute(model, budget)` **before** any hibernation. The substitute comes only from the router's eligible candidates (`l2-model-router` selection pipeline): a quota event never moves work off-device that privacy routing keeps local, or onto a provider the office has not authorized. A viable substitute → swap + `tracing::info!` `"[OC] model {m} → {m'}"` + stay Active. No substitute within budget → proceed to hibernate. An office-scope budget hard stop has no model to substitute and hibernates directly. |
| OC-4 Automatic resource-recovery wake | The service subscribes to `quota-recovered`; on receipt for a hibernation-causing resource it auto-invokes `resume()` with no user action. A recovery monitor also polls at `RECOVERY_POLL_MS` (default 900_000) as a backstop for providers that emit no recovery event. |
| OC-5 State always visible | Every transition emits an `OfficeStateChanged{office_id, from, to, at}` event on the event mesh **before** the transition is considered complete; the transition function returns only after the emit succeeds. Status icons (nav NV-3) and dashboards subscribe; there is no silent transition. |

## 4. Detailed Design

### 4.1 State machine

```text
[REFERENCE]
enum OfficeState { Active, Idle, Paused, Hibernating, Error, Offline }

transitions (guarded):
  Active|Idle   --master pause-->      drain → checkpoint → Paused
  Paused        --master resume-->     restore → Active (queued) | Idle (empty)
  Active|Idle   --quota-exhausted-->   substitute? Active : (drain → Hibernating)
  Hibernating   --quota-recovered-->   restore → Active | Idle           (OC-4 auto)
  Hibernating   --master resume-->     restore → Active | Idle           (early manual)
  *             --unrecoverable-->      Error   (requires user ack)
  tab unload    ------------------->    Offline
```

`OfficeControl` owns the current state behind a `RwLock`; `transition(to)` is the only mutator, runs the guard, performs drain/restore side effects, emits `OfficeStateChanged`, then commits. Concurrent transition requests serialize on the lock; a request whose guard fails returns `TransitionRejected{from, requested}` without side effects.

### 4.2 Master switch

Exposed as a core capability `office.pause` / `office.resume`, bound identically by the CLI verb (`cronus office pause`), the TUI slash command (`/office pause`), and the Tauri settings-dropdown control. `Error` and `Offline` are inert to the switch (different recovery paths). The library method is the source of truth; frontends never branch on state themselves beyond rendering.

### 4.3 Token-exhaustion hibernation

```text
[REFERENCE]
on quota-exhausted{model M}:
  sub := model_router.substitute(M, remaining_budget)
  if sub: swap workers M→sub; emit ModelSubstituted; stay Active/Idle
  else:   broadcast drain; await acks (≤ DRAIN_TIMEOUT_MS → PARTIAL);
          scheduler.suppress_all(); transition Hibernating

on quota-recovered{resource R} where R caused this hibernation:
  restore checkpoints; scheduler.reschedule_from_last_triggered();
  transition Active (queued) | Idle (empty)
```

Scheduler suppression records the suppression instant so recurring jobs reschedule from `last_triggered_at` (no burst-fire on wake).

### 4.4 Per-subsystem toggles

A `SubsystemPause` bitset (scheduler, kanban-autorun, automation, heartbeat) persists in local settings. Individually-paused subsystems stay paused across a master resume; `resume()` restarts only subsystems not individually paused. Toggles are set from Local Settings and via `office.subsystem <name> pause|resume`.

## 5. Implementation Notes

1. OC-1 drain reuses the existing orchestration delegation bus; no new transport.
2. OC-3 substitution is entirely `model_router.substitute` — office-control holds no fallback logic.
3. OC-4 recovery subscribes to `quota-recovered` from both sources; the poll backstop exists only for providers that never emit one.

## 6. Drawbacks & Alternatives

**Alternative — kill on pause**: violates OC-1/OC-2, loses work. Rejected.

**Alternative — poll the provider for recovery**: couples control to provider internals and wastes calls. Rejected in favor of budget-engine events with a bounded poll backstop.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[MODEL]` | `.design/main/specifications/l1-office-control.md` | Invariants OC-1…OC-5 this realizes |
| `[BUDGET]` | `.design/main/specifications/l2-budget-engine.md` | Quota exhaustion/recovery events |
| `[ROUTER]` | `.design/main/specifications/l2-model-router.md` | Substitute-model selection (OC-3) |
| `[SCHED]` | `.design/main/specifications/l2-scheduler.md` | Cron suppression / reschedule on wake |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.1 | 2026-09-23 | Core Team | Consistency pass (2026-09-23): OC-1: after `DRAIN_TIMEOUT_MS` the office entered the frozen state while an unacknowledged step still ran, so the spec both froze and did not interrupt — the timeout now bounds the wait, not the step: no new step starts, and a late step completes and replaces its `PARTIAL` marker. The budget engine was named the source of `quota-exhausted`/`quota-recovered` events it does not emit — sources are now the budget engine's office-scope hard stop/reset/raise and error recovery's Billing/RateLimit classification. OC-3 substitution confined to the router's eligible candidates. A build-plan phase reference removed from the implementation notes. |
| 1.0.0 | 2026-07-03 | Core Team | Initial implementation spec — OfficeState machine, cooperative drain-and-checkpoint, token-exhaustion hibernation ladder (substitute-before-hibernate, auto-recovery wake), per-subsystem toggles; maps OC-1…OC-5. |
