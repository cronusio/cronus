# Kanban Model

**Version:** 1.2.0
**Status:** Stable
**Layer:** concept

## Overview

The technology-agnostic model of the work board through which an office runs its tasks. Each office has exactly one canonical Kanban board whose fixed ordered set of states forms the mandatory semantic backbone. Cards (work units) move through the pipeline driven by the office (manager and agents), not by the client. Offices MAY layer custom columns and custom board views on top of the canonical pipeline as mapped extensions (KAN-8). Completed work is archived automatically, keeping the active board lean while preserving history.

## Related Specifications

- [l1-office-model.md](l1-office-model.md) - The managed work lifecycle (OFF-7) this board realizes; client-not-managing (OFF-5).
- [l1-workspace-lifecycle.md](l1-workspace-lifecycle.md) - One board belongs to one workspace (office).
- [l2-kanban-board.md](l2-kanban-board.md) - Concrete board storage, transitions, auto-archival, and commands.

## 1. Motivation

An autonomous office needs a single, legible representation of "what work exists and where it stands," and it must stay readable over a long-running project without the client having to tidy it. A fixed canonical pipeline gives a shared vocabulary across offices; office-driven movement honors maximum automation; automatic archival keeps the board lean while never losing the record of what was done.

## 2. Constraints & Assumptions

- One board of record per office; boards do not span offices.
- The canonical state set is fixed and non-removable; custom columns/boards extend it as mapped views, never replace it (KAN-8).
- The client may view the board but is not required to manage it.
- Completed work must remain recoverable after it leaves the active board.

## 3. Core Invariants (Layer 1 only)

Rules every Layer 2 implementation MUST NOT violate:

- **KAN-1 (Canonical pipeline):** every office has exactly one board of record with the fixed, ordered states `triage → todo → ready → running → blocked → done`. The canonical states always exist, keep their semantics, and cannot be removed or renamed; custom structures extend them, never replace them (KAN-8).
- **KAN-2 (Office-managed, not client-managed):** cards are created and moved by the manager and agents. The client MAY view the board but MUST NOT be required to manage it (consistent with OFF-5).
- **KAN-3 (Auto-archival terminal):** `done` cards are archived automatically by a defined condition — by default a fixed age after the card entered `done`, and at once on project closure. [ADDED v1.2.0] A card may be **pinned**, which exempts it from the age condition (never from project closure). Archival is an automatic action, not a manual board column.
- **KAN-4 (Non-destructive archive):** archived cards are retained as durable history; archival MUST NOT destroy the record of completed work.
- **KAN-5 (Card = unit of work):** a card represents one task/work unit; its state reflects progress. A `blocked` card MUST record the reason for the block.
- **KAN-6 (One board per office / isolation):** exactly one board exists per workspace and never spans offices (consistent with OFF-1).
- **KAN-7 (Traceable transitions):** forward flow is the norm; backward moves (e.g. `blocked → ready`) are permitted but explicit, and every state change is traceable (who/when/why).
- **KAN-8 (Custom boards map to canon):** an office MAY define custom columns and custom board views beyond the canonical pipeline. Every custom column MUST anchor to exactly one canonical state; for every cross-cutting consumer (archival, analytics, building-level projections, convergence relations) a card in a custom column is in its anchor state. Custom board views are filtered/scoped projections over the same card set — the canonical board remains the single board of record (KAN-6). Custom structures MUST NOT bypass KAN-2–KAN-7: movement through custom columns stays office-managed and traceable, and the canonical board view remains available at all times.

> L2 specs cannot reach RFC status until all invariants here are addressed in their "Invariant Compliance" section.

## 4. Detailed Design

### 4.1 The pipeline

```mermaid
graph LR
    TRIAGE[triage] --> TODO[todo] --> READY[ready] --> RUNNING[running] --> DONE[done]
    RUNNING --> BLOCKED[blocked]
    BLOCKED --> READY
    DONE -.->|auto-archive| ARCHIVE[(archive store)]
```

| State | Meaning |
| --- | --- |
| triage | captured, not yet assessed |
| todo | assessed, queued |
| ready | ready to be picked up |
| running | work in progress (WIP) |
| blocked | halted by an impediment (reason recorded) |
| done | completed |
| (archive) | terminal store for completed cards; reached automatically, not a board column |

### 4.2 Who moves cards

The office manager triages incoming work and routes it; agents pull `ready` work into `running` and push to `done` or `blocked`. The client observes; the system does the managing (KAN-2, OFF-5). Cards originate from the office's plans/tasks (OFF-7).

### 4.3 Archival

When a `done` card meets the archival condition, it is moved out of the active board into the archive store, preserved for history and learning (KAN-3, KAN-4). The active board therefore shows only live and recently-finished work. The age is counted from the card's last entry into `done` — a card that leaves and re-enters it starts again — and a pinned card stays on the board until its pin is removed or the project closes.

### 4.4 Custom columns & boards

A custom column is a named refinement of a canonical state (e.g. a `review` column anchored to `running`, or an `icebox` column anchored to `todo`). The anchor is declared when the column is created and is part of the column's identity; re-anchoring is an explicit, traceable change (KAN-7). A card placed in a custom column reports its anchor state to every consumer outside the office — archival conditions, analytics, and the building-level projection read canonical states only, so the shared vocabulary across offices is preserved.

A custom board is a saved view (filter/scope/grouping) over the office's single card set. Views carry no state of their own and cannot hold cards the board of record does not; deleting a view never touches cards (KAN-4 analog: views are disposable, records are not).

## 5. Drawbacks & Alternatives

- **Fixed states reduce flexibility:** resolved in v1.1.0 — the canonical pipeline stays the mandatory semantic backbone, and offices layer custom columns/boards on top as mapped extensions (KAN-8). The universal vocabulary is preserved because every custom column anchors to a canonical state.
- **Alternative — freeform user-defined state sets (no canonical backbone):** rejected; cross-office projections, archival semantics, and convergence relations would lose their shared vocabulary, and every consumer would need per-office mapping tables.
- **Alternative — manual archive column:** rejected; it would force the client to tidy the board, violating KAN-2/OFF-5.
- **Alternative — no archive:** rejected; loses the record of completed work needed for learning (OFF-9).
- **Default archival condition — resolved (KAN-3):** age after `done`, plus an immediate sweep at project closure. Age alone would leave a finished project's board cluttered until its last card aged out; closure alone would let a long-running project's board grow without bound. The age threshold itself is a realization setting.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[OFFICE]` | `.design/main/specifications/l1-office-model.md` | Work lifecycle (OFF-7) and client-not-managing (OFF-5) |
| `[BOARD]` | `.design/main/specifications/l2-kanban-board.md` | Concrete board realization |

## Document History

| Version | Date | Notes |
| --- | --- | --- |
| 1.0.0 | 2026-06-24 | Initial stable spec — canonical pipeline, office-managed movement, auto-archival, KAN-1…KAN-7 |
| 1.1.0 | 2026-07-03 | KAN-8 added — custom columns/boards legitimized as mapped extensions: every custom column anchors to exactly one canonical state, custom boards are views over the single card set; KAN-1 rephrased (canonical states non-removable rather than "not user-redefinable"); §4.4 mapping model added; resolves the contradiction with l1-office-model §4 ("custom columns allowed") and l2-app-ui ("+ custom boards"). Additive — L1 stays Stable; L2 kanban-board reconciled in the same batch. |
| 1.2.0 | 2026-10-03 | KAN-3 amended — the open §5 question of the default auto-archival condition is resolved: age after `done`, plus an immediate sweep at project closure (age alone leaves a finished project's board cluttered, closure alone lets a long-running board grow without bound), and a card may be **pinned** to stay past the age condition (never past closure). The age counts from the card's last entry into `done`. The threshold is a realization setting. Additive — L1 stays Stable (C9); `l2-kanban-board` carries the default and the `pinned` field in the same pass. |
