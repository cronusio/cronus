# Automation Canvas

**Version:** 1.0.1
**Status:** Stable
**Layer:** implementation
**Implements:** l1-automation-canvas.md

## Overview

The concrete visual automation canvas in the desktop shell (React 19 + Tauri): a three-panel flow-graph editor/inspector that is a pure projection of the automation-pipeline engine. It renders explicit pipelines (read-write) and implicit `@ON:` pipelines (read-only) as node graphs, streams live execution state from the AuditProvider trace, requests engine-side pinned partial runs (never executing itself), and persists layout + explicit definitions to the office local store. Presentation-only: all execution and routing stay in the engine (INV-2, AC-3).

## Related Specifications

- [l1-automation-canvas.md](l1-automation-canvas.md) — the model this implements (AC-1…AC-8).
- [l2-automation-pipeline.md](l2-automation-pipeline.md) — the engine this canvas projects; node taxonomy, execution, dev runs (AP-13), observers (AP-15).
- [l2-app-ui.md](l2-app-ui.md) — desktop UI host + state authority.
- [l2-office-view.md](l2-office-view.md) — spatial office view the canvas may embed as a panel.
- [l2-extension-registry.md](l2-extension-registry.md) — extension-backed action nodes appear in the palette.

## 1. Motivation

The model requires a legible, faithful projection that adds no runtime. Building the canvas as a serialization layer over the canonical pipeline definition (never its own representation) guarantees projection fidelity; consuming the AuditProvider stream for live state keeps the canvas a display consumer, not a second observer.

## 2. Constraints & Assumptions

- The canvas defines no node/trigger/execution semantics — it renders the engine's model.
- Explicit pipelines are read-write; implicit `@ON:` pipelines are read-only (edits go to the workflow file or a convert-to-explicit action).
- Canvas state (layout, explicit defs) persists to the office local store; never egressed (AC-5).
- Live state and traces come only from the AuditProvider stream (AC-3).

## 3. Invariant Compliance (Layer 2)

| L1 Invariant | Implementation |
| --- | --- |
| AC-1 Projection fidelity | The graph is rendered from the engine's live pipeline registry; an `EngineEvent` subscription auto-refreshes on add/remove/state-change so canvas and engine never diverge. |
| AC-2 Read-only implicit | `@ON:`-sourced pipelines render as read-only node groups; there is no write path to workflow files. "Convert to explicit" creates a new definition, never mutating the original. |
| AC-3 No runtime divergence | The canvas has no node-execution code; every run is delegated to the engine and observed via the trace stream. |
| AC-4 Data safety | Rendered traces show descriptors only (field names/types/counts), never verbatim payload content — the canvas reads only what the AuditProvider exposes. |
| AC-5 Local persistence | Layout + explicit defs persist to the office local state store; the backup mechanism covers them; nothing is transmitted externally. |
| AC-6 Graceful degradation | If the engine is unreachable, the canvas shows last-known state + a staleness badge and still allows viewing/editing defs; edits queue for sync on reconnect. Each queued edit carries the definition version it was made against, and a definition that changed meanwhile is never overwritten on reconnect — the user resolves the conflict (base fingerprint, `l1-change-merge`). |
| AC-7 Pinned partial re-run requested | Pinning a node marks it; the canvas requests an engine dev run (AP-13) from a chosen node and renders the result from the trace — it never executes; runs are labelled development, actions dry-run unless opted live. |
| AC-8 Observer scope visible | An `observer` node renders a kind badge (error/status/completion) and a dashed scope overlay on selection; scoped vs catch-all (*unhandled*) are visually distinct; selecting any node surfaces which observer catches its failure. |

## 4. Detailed Design

### 4.1 Three-panel layout

```text
[REFERENCE]
┌ Pipeline Selector (left) ─────────────────────────────┐
│  Explicit (editable) · Implicit per role (read-only)  │
├ Flow Graph (center) ──────────┬ Node Inspector (right)┤
│  nodes + edges + live state   │  config · trace · err │
├ Execution Log (bottom, collapsible) ──────────────────┤
│  per-run trace; filter by run_id / node / time        │
└───────────────────────────────────────────────────────┘
```

State authority is the app-shell store; the canvas reads engine state via the IPC bridge and writes explicit defs back in the canonical pipeline format.

### 4.2 Node rendering

Each node type has a distinct glyph + live-state indicator (trigger last-fired, filter pass/drop, action dispatched/pending/error, delay remaining, aggregate window, loop iteration, `subpipeline` drill-in, `observer` kind badge + scope overlay). Edges carry payload descriptors (names/types, not values).

### 4.3 Editing & validation

Explicit editing offers a node palette (extension action nodes auto-appear), live connection-rule validation (trigger outbound-only, action leaf-unless-error-branch), a per-node config inspector, and inline AP-2/AP-3/AP-7 violation feedback. Each save creates a new pipeline version (non-destructive); prior versions accessible from the selector.

### 4.4 Inspection, dev runs, observers

Run list + per-node run trace + replay (re-display history, no execution) + live mode (highlight active node from the trace). Pin-and-partial-run (AC-7) requests an engine dev run and renders it, distinct from replay. Observer scope view (AC-8) draws the dashed overlay and surfaces per-node error-handler attribution.

### 4.5 Implicit surfaces

`@ON:` pipelines render as collapsed read-only per-role groups; "Convert to explicit" extracts the blocks into a standalone definition and registers it with identical semantics. The canvas never writes the workflow file (AC-2): the user removes the `@ON:` block through the workflow's own editing path, and the converted pipeline is created **disabled** until that block is gone — otherwise the same reaction would fire twice, once from each definition.

## 5. Implementation Notes

1. Ship inspection-only first (trace stream + run display), then add editing — validates the data path before write.
2. The editor is a serialization layer over the canonical pipeline definition; the canvas stores no private representation.
3. The node palette is driven by the engine's node registry — a new node type needs only a rendering entry.
4. Live mode reuses the AuditProvider stream; the canvas is a display consumer, not an additional observer.

## 6. Drawbacks & Alternatives

**Alternative — canvas-only (no implicit mode)**: breaks the autonomy promise for routine per-worker reactions. Rejected.

**Alternative — text-only DSL editor**: fails the legibility goal for non-technical users. Both views can coexist over one definition; the canvas is primary.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[MODEL]` | `.design/main/specifications/l1-automation-canvas.md` | Invariants AC-1…AC-8 |
| `[ENGINE]` | `.design/main/specifications/l2-automation-pipeline.md` | Pipeline engine this projects |
| `[APP-UI]` | `.design/main/specifications/l2-app-ui.md` | Desktop UI host |
| `[OFFICE-VIEW]` | `.design/main/specifications/l2-office-view.md` | Office surface host (the core's interaction graph; no spatial view in core, OVZ-9) |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.1 | 2026-09-24 | Core Team | Consistency pass (2026-09-24): "Convert to explicit" removed the `@ON:` source after confirmation although AC-2 says the canvas has no write path to workflow files, and the converted pipeline fired alongside the original — the user removes the block through the workflow's own path and the converted pipeline stays disabled until then. Queued offline edits could overwrite a definition changed meanwhile — they carry their base version and conflicts are resolved, not overwritten. A canonical reference to a spatial view no longer in core corrected. |
| 1.0.0 | 2026-07-03 | Core Team | Initial implementation spec — three-panel flow-graph projection, node rendering with live state, explicit editing + validation, read-only implicit surfaces, dev-run requests (AC-7), observer scope view (AC-8), local persistence + graceful degradation; maps AC-1…AC-8. |
