# Automation Pipeline

**Version:** 1.1.1
**Status:** Stable
**Layer:** implementation
**Implements:** l1-automation-pipeline.md

## Overview

The runtime automation engine in `crates/core`: a trigger dispatcher fed by trigger-triage, a topological node executor over the typed node taxonomy, the deduplication window shared by both authoring modes, scoped node/pipeline state with pluggable persistence backends, in-graph lifecycle observers, the control plane, portable bundles, and pipeline composition. The engine is a single implementation behind both the implicit (`@ON:` workflow blocks) and explicit (canvas) surfaces — AP-1 parity is structural, not a convention.

## Related Specifications

- [l1-automation-pipeline.md](l1-automation-pipeline.md) — the model this implements (AP-1…AP-16).
- [l2-trigger-triage.md](l2-trigger-triage.md) — event intake + dedup window feeding the trigger dispatcher.
- [l2-scheduler.md](l2-scheduler.md) — `schedule` trigger source.
- [l2-orchestration.md](l2-orchestration.md) — `action` nodes delegate through the orchestration bus.
- [l2-extension-registry.md](l2-extension-registry.md) — external-service action/trigger nodes cross the MCP boundary here.
- [l1-automation-canvas.md](l1-automation-canvas.md) — the explicit-mode visual surface over this engine.
- [../../nodus/specifications/l1-nodus-observability.md](../../nodus/specifications/l1-nodus-observability.md) — AuditProvider trace contract (AP-6).

## 1. Motivation

The model requires one engine behind two surfaces, deterministic evaluation, and observable runs. A single `crates/core` executor reused by both modes guarantees AP-1 parity by construction; delegating action dispatch to existing subsystems (kanban, orchestration, inbox) avoids re-implementing effect logic; binding AuditProvider at every node boundary makes every run replayable.

## 2. Constraints & Assumptions

- The pipeline graph is a validated DAG; iteration is a bounded `loop` node, never a graph cycle.
- Node evaluation is deterministic given a fixed event + rule set; side effects occur only in `action` nodes.
- Event payloads are schema-validated and content-excluded (AP-4) — never raw user text, session context, credentials, or memory-store contents.
- Scoped state binds to a named persistence backend from a registry (volatile / durable) with a configured default.

## 3. Invariant Compliance (Layer 2)

| L1 Invariant | Implementation |
| --- | --- |
| AP-1 Dual-mode parity | One `PipelineEngine`; the `@ON:` binder and the canvas both produce the same `PipelineDef`. No mode-only node type. |
| AP-2 Trigger uniqueness | The dedup window (trigger-triage) keys on `(trigger_id, event_key)` within `DEDUP_WINDOW_MS`; duplicate firings suppressed, distinct triggers fire independently. |
| AP-3 Step atomicity | Each node returns `Ok(payload)` or `Err(structured)`; no partial node state. On error the run halts at the node unless an error branch/observer handles it. |
| AP-4 Event payload isolation | `EventPayload` is a typed struct; a schema validator rejects any `data` field carrying excluded content classes before propagation. |
| AP-5 Security boundary | A run executes under the owning role's permission set, never the emitter's, resolved when each run starts — not frozen at bind time, so a permission revoked after binding stops the next run instead of surviving in the binding; escalation attempts fail closed. |
| AP-6 Observable execution | The executor emits AuditProvider start/end/error + elapsed at every node boundary; a run with no trace is rejected as incomplete. |
| AP-7 Office isolation | Every payload carries `office_id`; cross-office reads/mutations require an explicit permission gate + user approval before activation. |
| AP-8 Durable node memory | Nodes may declare a schema-bounded, office-scoped, resettable memory map; `transform` declares none and is guaranteed retryable. |
| AP-9 Control plane | Data edges carry payloads; control edges carry `enable`/`disable`/`trigger` verbs; the two graphs are stored and traversed separately. |
| AP-10 Event lifecycle | Per-node retention policy + GC (keeping the one most-recent change-detector event); per-edge immediate/deferred propagation; each node exposes last-evaluated/received/error + working? |
| AP-11 Portable bundles | `AutomationBundle` serializes definitions + edges only; import is non-destructive, credential-rebinding, re-validates AP-5/AP-7 via the staged agent-migration apply. |
| AP-12 Pipeline composition | `subpipeline` node invokes a pipeline by identity under the caller's scope; depth-guarded and acyclic across the composition graph. |
| AP-13 Pinned partial re-execution | Dev run pins a node's output, re-runs only the downstream subgraph, reuses the §4.9 dry-run quarantine, marked development — never a production run. |
| AP-14 Scoped state | node-private / pipeline-shared scopes bind to a named volatile/durable backend from a registry with a default + per-scope override; all office-scoped + content-excluded. |
| AP-15 Lifecycle observers | `observer` node subscribes to error/status/completion of a declared scope; scoped precedes catch-all; unhandled errors propagate outward across `subpipeline` to the caller, else AP-3 stop-on-failure stands. |
| AP-16 External-source change classification | **Pending.** A change-detector over an `external_event` source is to compare a declared canonical projection (volatile regions — reflow, rotating embeds, timestamps, nonces — removed), not raw bytes, and emit a typed status `new` / `same` / `changed` / `removed`, with `removed` distinct from a transient fetch error and comparison lineages namespaced by tag. Today a change-detector compares raw payloads. |

## 4. Detailed Design

### 4.1 Engine structure

```text
[REFERENCE]
PipelineEngine {
  registry   : PipelineDef by id (implicit-bound + explicit)
  dispatcher : Trigger → matching pipelines (via trigger-triage intake)
  executor   : topological node runner (per §4.3 of the model)
  state       : ScopeStore (node-private | pipeline-shared) over backend registry
  control     : ControlGraph (enable/disable/trigger edges)
  audit       : AuditProvider sink (AP-6)
}
```

### 4.2 Node executor

Nodes evaluate in topological order: `filter` (short-circuit), `transform` (pure), `branch` (one path), `delay` (durable suspend), `aggregate` (window), `loop` (bounded), `action` (delegates via orchestration/kanban/inbox), `subpipeline` (AP-12), `observer` (AP-15). Each boundary emits an AuditProvider event. `action` dispatch reuses existing subsystem calls — no new dispatch logic.

<!-- [ADDED] v1.1.0 -->
Topological order constrains dependencies, not scheduling: nodes of the same topological rank have no edge between them and MAY evaluate concurrently under a bounded cap, per the superstep semantics of the shared execution-graph model. Side effects occur only in `action` nodes, and an `action`'s dispatch mode (blocking or non-blocking) is per-node configuration unchanged by rank-level concurrency. Two `action` nodes of one rank run concurrently only when their declared targets are disjoint; actions sharing a target (the same card, the same channel, the same file) run one after another in a stable order (node id), so the combined effect does not depend on which finished first. Determinism holds: a node's inputs are fixed by its predecessors' staged outputs, never by sibling completion order, and effects on a shared target are ordered.

### 4.3 Dedup window & trigger dispatch

All trigger types (`schedule`/`state_change`/`kanban_event`/`message_received`/`webhook`/`external_event`/`manual`) funnel through trigger-triage. The engine checks `(trigger_id, event_key)` against the `DEDUP_WINDOW_MS` cache before starting a run (AP-2). Implicit `@ON:` blocks bind at worker activation; explicit definitions load from the office pipeline registry.

### 4.4 Scoped state & backends

```text
[REFERENCE]
ScopeStore:
  scope    : node-private | pipeline-shared
  backend  : registry-resolved { volatile (in-proc) | durable (survives restart) }
  ops      : get/set/reset ; schema-bounded ; office-scoped (AP-7) ; content-excluded (AP-4)
```

A node declaring no scoped state is freely retryable (AP-3). `transform` never declares state.

### 4.5 Control plane, observers, composition

Control edges carry `enable`/`disable`/`trigger`, traversed on a separate `ControlGraph` (AP-9). `observer` nodes receive engine-routed lifecycle events with scoped-then-catch-all precedence and outward error propagation across `subpipeline` boundaries (AP-15). `subpipeline` resolves a target by identity, runs under the caller's scope, and is depth-guarded + acyclic (AP-12).

### 4.6 Bundles & dev runs

`export` serializes definitions + edges only. `import` runs the staged dry-run → instantiate → review → activate apply (reused from agent-migration), rebinds credentials, re-validates AP-5/AP-7. A pinned partial run (AP-13) quarantines `action` dispatch as simulated-and-recorded unless an action is explicitly opted live, and is traced as development.

## 5. Implementation Notes

1. Build trigger-triage intake + the dedup primitive first; wire implicit `@ON:` binding before the canvas to validate the engine with known inputs.
2. Implement `action` nodes by delegating to kanban/orchestration/inbox — never re-implement dispatch.
3. Bind AuditProvider at every node boundary, not only run start/end (AP-6).
4. The canvas (l1-automation-canvas) only requests dev runs and renders results — it never executes.

## 6. Drawbacks & Alternatives

**Alternative — separate engines per mode**: violates AP-1; divergent behavior is undebuggable. Rejected — one engine, two binders.

**Alternative — single edge type (no control plane)**: overloads the payload contract with control semantics (AP-4) and makes "is this active?" unanswerable. Rejected — AP-9 separates governance from data.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[MODEL]` | `.design/main/specifications/l1-automation-pipeline.md` | Invariants AP-1…AP-16 |
| `[TRIAGE]` | `.design/main/specifications/l2-trigger-triage.md` | Trigger intake + dedup window |
| `[ORCH]` | `.design/main/specifications/l2-orchestration.md` | Delegation used by action nodes |
| `[CANVAS]` | `.design/main/specifications/l1-automation-canvas.md` | Explicit-mode visual surface |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.1.1 | 2026-09-24 | Core Team | Consistency pass (2026-09-24): AP-16 (external-source change classification, added to the L1 in 1.4.0) was unmapped — Pending row. Same-rank concurrency claimed determinism while two `action` nodes could race on one target — actions sharing a target run in a stable order. AP-5 froze the owning role's permissions at bind time, so a revoked permission survived in the binding — resolved at each run start. |
| 1.1.0 | 2026-07-04 | Core Team | Concurrent same-rank node evaluation (§4.2): nodes of one topological rank MAY run concurrently under a bounded cap per the shared superstep semantics; effects stay confined to `action` nodes; determinism preserved by staged predecessor outputs. |
| 1.0.0 | 2026-07-03 | Core Team | Initial implementation spec — single PipelineEngine behind both modes, topological node executor, dedup window, scoped state over volatile/durable backends, control plane, lifecycle observers, composition, portable bundles, dev runs; maps AP-1…AP-15. |
