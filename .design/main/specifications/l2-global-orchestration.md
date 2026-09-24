# Global Orchestration

**Version:** 1.0.1
**Status:** Stable
**Layer:** implementation
**Implements:** l1-global-orchestration.md

## Overview

The concrete building-level coordinator in `crates/core`, embodied by the home workspace manager: an aggregate office-state view fed by a building event bus, an ACP relay router that decides which office receives a cross-office message, a phase-awareness enforcer that annotates new-component cards with mandatory cross-cutting concerns, and a building-level escalation path (resolve / cross-office deliberation / HITL). It coordinates offices via ACP only — it never reaches into an office's internal state.

## Related Specifications

- [l1-global-orchestration.md](l1-global-orchestration.md) — the model this implements (GO-1…GO-6).
- [l2-acp.md](l2-acp.md) — the cross-office relay this routes over (GO-5).
- [l2-orchestration.md](l2-orchestration.md) — per-office delegation mechanics this builds on.
- [l2-office-control.md](l2-office-control.md) — OfficeState events feeding the aggregate view (GO-4) and routing decisions.
- [l2-deliberation.md](l2-deliberation.md) — cross-office deliberation rounds (GO-6).
- [l2-workspace-management.md](l2-workspace-management.md) — the home workspace hosting the global orchestrator.

## 1. Motivation

The model requires one building-level coordinator that routes across offices, enforces phase-awareness, and escalates — without overriding an office's active work. Subscribing to each office's OfficeState events gives the aggregate view without polling; routing over the ACP relay keeps coordination on the sanctioned cross-office boundary; annotating cards with phase concerns makes "we'll add it later" an invariant violation, not a hope.

## 2. Constraints & Assumptions

- The global orchestrator does no specialist work; it coordinates and enforces policy.
- Cross-office delegation is always ACP-mediated (GO-5); it never reads/mutates office internals directly.
- Phase-awareness concern catalog + phase numbering come from the building global config + the office's own plan artifacts (the machine-readable phase frontmatter under `<ws>/planning/`), not hardcoded.
- Global orchestration is opt-in — absent a home workspace manager, offices run without it. The phase-concern check (GO-3) does not lapse with it: an office without a coordinator applies the same catalog to its own cards.
- A cross-office send is an outbound sink of the sending office: it carries the confidentiality capacity the user granted that office pair (default below the private label), and content over that capacity is blocked by the runtime at the relay using side-band labels (CF-4, CF-8, CF-10) — without the coordinator reading the content (GO-5).

## 3. Invariant Compliance (Layer 2)

| L1 Invariant | Implementation |
| --- | --- |
| GO-1 One global orchestrator | The home workspace manager is the sole coordinator; peer offices never route directly — all cross-office traffic passes through it or the ACP relay. |
| GO-2 Non-intrusive | The coordinator may propose/route/escalate but has no code path to cancel or re-delegate an office's active orchestration; it only reads office state and sends ACP messages. |
| GO-3 Phase-awareness enforcement | Before a new-component card may move to `running`, the office asks the coordinator (over ACP) for `check_phase_concerns(phase, component)`; the office's own orchestrator adds each mandatory concern to the card as a non-optional acceptance criterion. The coordinator never writes the card (GO-2, GO-4), and because the check is a gate the office runs before implementation starts, it cannot race with it. |
| GO-4 Unified visibility | A building event bus subscribes to every office's `OfficeStateChanged` + kanban-summary + budget + session events into a read-only aggregate view; mutation only via each office's own path. |
| GO-5 ACP routing | Cross-office messages route over the l2-acp relay; the coordinator is the relay's decision layer — it inspects the envelope (target, session) but never message content. |
| GO-6 Escalation authority | An office escalation resolves directly, requests a cross-office deliberation round (l2-deliberation) among affected offices' orchestrators, or escalates to the user (HITL, ORC-9). |

## 4. Detailed Design

### 4.1 Aggregate view

A `BuildingView` subscribes to the building event bus: per office `{OfficeState, kanban_summary (active+blocked), budget_today, active_sessions}`. Read-only toward offices — the coordinator never writes an office's state; it forwards routing/escalation as ACP messages.

### 4.2 Phase-awareness protocol

```text
[REFERENCE]
before office moves new-component card C (component P, phase n) to running:
  concerns := coordinator.check_phase_concerns(n, P)  // GO-3, over ACP;
                                                      // the office's own catalog copy if no coordinator
  if concerns: office adds them to C.acceptance_criteria (non-optional)   // office's own path
  C proceeds under normal office orchestration
```

The concern catalog (localization / observability / security / accessibility / error-handling / budget-safety …) is per-project building config; phase numbering is read from the machine-readable phase frontmatter.

### 4.3 Cross-office delegation & routing

```text
[REFERENCE]
Office A → [ACP] → Global orchestrator
   route_decision := by(declared_capabilities, OfficeState≠Paused/Hibernating, budget)
Global orchestrator → [ACP relay] → Office B → response streams back → Office A
```

Paused/Hibernating offices are bypassed in routing. A message whose only eligible target is paused or hibernating is returned to the sender as undeliverable for now, naming the target's state — or held durably until the target wakes when the sender asked for that — never dropped. The relay layer forwards verbatim (no content inspection).

### 4.4 Escalation

An office deadlock / multi-office conflict escalates to the coordinator, which resolves within its authority, requests a cross-office deliberation round (deliberation engine) with affected orchestrators as participants, or escalates to the user (HITL). No cross-cutting decision is made unilaterally by one office.

## 5. Implementation Notes

1. GO-4 is a building event-bus subscription over office OfficeState events — no polling.
2. GO-3 reads the office's machine-readable phase frontmatter for phase structure and the building config for mandatory concerns; the office applies the result to its own card.
3. GO-5 relay inspects only the envelope (target office id, session context), never content.

## 6. Drawbacks & Alternatives

**Alternative — peer-to-peer cross-office comms**: a bilateral mesh with no unified routing/visibility/escalation. Rejected for multi-office setups (GO-1).

**Alternative — no phase-awareness ("add it later")**: the main source of architectural debt; retrofit costs 3–10×. GO-3 makes progressive integration an invariant.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[MODEL]` | `.design/main/specifications/l1-global-orchestration.md` | Invariants GO-1…GO-6 |
| `[ACP]` | `.design/main/specifications/l2-acp.md` | Cross-office relay (GO-5) |
| `[OFFICE-CTRL]` | `.design/main/specifications/l2-office-control.md` | OfficeState feed for GO-4 + routing |
| `[DELIB]` | `.design/main/specifications/l2-deliberation.md` | Cross-office deliberation (GO-6) |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.1 | 2026-09-23 | Core Team | Consistency pass (2026-09-23): GO-3 had the coordinator write annotations into an office's card, contradicting GO-2/GO-4 and its own "never mutates office internals", and it reacted to card creation, racing the office's start — it is now a gate the office runs before a new-component card moves to running, with the office applying the concerns itself (and applying the catalog on its own when no coordinator exists). A message to a paused or hibernating office was "bypassed" — returned or held, never dropped. Cross-office sends are outbound sinks subject to confidentiality capacity checked on side-band labels (CF-4/CF-10). Phase numbering now reads the office's plan artifacts, not an ambiguous `PLAN.md`. |
| 1.0.0 | 2026-07-03 | Core Team | Initial implementation spec — home-manager coordinator, event-bus aggregate view, ACP relay router, phase-awareness card annotation, building-level escalation with cross-office deliberation + HITL; maps GO-1…GO-6. |
