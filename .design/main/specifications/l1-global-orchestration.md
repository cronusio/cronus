# Global Orchestration

**Version:** 1.1.0
**Status:** Stable
**Layer:** concept

## Overview

Global orchestration is the building-level coordination layer that operates above individual offices. While office-level orchestration (`l1-orchestration.md`) governs how a single office decomposes and delegates work, global orchestration governs how offices relate to one another, how the building manager routes client requests across offices, and how the system maintains phase-awareness: knowing what to build at each stage of a project lifecycle so that cross-cutting concerns (localization, security, telemetry) are integrated progressively rather than bolted on retroactively.

The global orchestrator is embodied by the default home workspace manager — the "CEO of the building."

## Related Specifications

- [l1-orchestration.md](l1-orchestration.md) — per-office orchestration protocol; global orchestration is the layer above it
- [l1-workspace-lifecycle.md](l1-workspace-lifecycle.md) — office (workspace) creation, routing, and lifecycle
- [l1-office-model.md](l1-office-model.md) — the office entity that global orchestration governs
- [l1-acp.md](l1-acp.md) — the protocol by which inter-office messages are routed at the building level
- [l2-orchestration.md](l2-orchestration.md) — the concrete delegation mechanics that global orchestration builds on
- [l1-intent-resolution.md](l1-intent-resolution.md) — [ADDED v1.1.0] IR-9: the question classes and the one budget of the client's attention that GO-7 applies across offices
- [l1-navigation-model.md](l1-navigation-model.md) — [ADDED v1.1.0] the Inbox's Poll/Clarify facet, where held questions stay visible and answerable

## 1. Motivation

Two problems motivate global orchestration:

**Problem 1 — Cross-office coordination**: as a user runs multiple offices (different projects, environments, or contexts), they implicitly form a system. Without a building-level coordinator, offices operate in isolation: they cannot share knowledge, delegate sub-tasks to sibling offices, or give the user a unified view of all ongoing work.

**Problem 2 — Phase-awareness**: when building a complex system (like Cronus itself), the order of implementation matters as much as the implementation. A coding agent that is unaware of implementation phases may defer cross-cutting concerns (localization, accessibility, observability) to a future "cleanup" pass — but cleanup is exponentially more expensive than integration. Global orchestration encodes phase-awareness: what must be built early, what can be deferred, and how to detect when a phase-level concern is being inadvertently deferred.

## 2. Constraints & Assumptions

- The global orchestrator does not perform specialist work; it coordinates between offices and enforces phase-awareness policies. Specialist work remains in individual offices.
- Cross-office delegation is always mediated by ACP (`l1-acp.md`); the global orchestrator never reaches into an office's internal state directly.
- Phase-awareness policies are declared in the building's global configuration, not hardcoded; they evolve as projects mature.
- Global orchestration is an opt-in capability: a single-office user operating without a home workspace manager receives no global orchestration.

## 3. Core Invariants

- **GO-1 One global orchestrator**: the building has exactly one global orchestrator (embodied by the home workspace manager). Peer offices do not coordinate directly — all cross-office routing passes through the global orchestrator or the ACP relay.
- **GO-2 Non-intrusive coordination**: the global orchestrator MUST NOT interrupt or re-delegate work that an office has already started. It may propose, route, and escalate — it may not unilaterally cancel or override an office's active orchestration.
- **GO-3 Phase-awareness enforcement**: before any office begins implementation of a new component, the global orchestrator MUST check whether the current implementation phase mandates any cross-cutting concerns that must be addressed during this component's implementation. A cross-cutting concern cannot be declared "deferred to cleanup" if it is in the current phase's mandatory list.
- **GO-4 Unified visibility**: the global orchestrator maintains a live aggregate view of all offices' states (OfficeState, kanban summary, budget consumption, active sessions). This view is read-only toward individual offices; mutation happens through each office's own orchestration path.
- **GO-5 ACP routing**: cross-office message routing is always mediated by the ACP relay (ACP cross-office routing, `l1-acp.md §4.5`). The global orchestrator is the ACP relay's decision layer — it determines which office should receive a given message.
- **GO-6 Escalation authority**: the global orchestrator is the escalation target when an office's orchestrator cannot resolve a conflict or requires cross-office resources. The global orchestrator may request a deliberation round (`l1-deliberation.md`) across representatives from multiple offices.
- **GO-7 One attention budget across offices**: [ADDED v1.1.0] the client is one person with one attention, however many offices run. Questions from every office reach the client through the global orchestrator's single dispatch (`l1-intent-resolution` IR-9): it **merges** questions that ask the same thing, **orders** blocking questions ahead of the rest, **paces** interruptions so a burst from several offices does not become a barrage, and **never drops** a held question — each stays in its office's queue, visible and answerable (`l1-intent-resolution` IR-6). The dispatch reads queues and routes presentation; it neither answers an office's question nor suppresses one (GO-2), and the office remains the owner of its questions' content and bound.

## 4. Detailed Design

### 4.1 Building Structure

```text
[REFERENCE]
Building (home workspace / global orchestrator)
  ├── Office A (project-1)       ← ACP endpoint
  ├── Office B (project-2)       ← ACP endpoint
  ├── Office C (environment: staging)  ← ACP endpoint
  └── Office D (environment: prod)     ← ACP endpoint (restricted access)

Global orchestrator visibility:
  - OfficeState of each office (Active/Idle/Paused/Hibernating/Error/Offline)
  - Kanban summary per office (active + blocked card count)
  - Budget consumed today per office
  - Active sessions per office
```

### 4.2 Phase-Awareness Protocol

Phase-awareness is enforced at the start of every new implementation component within any office. The protocol runs automatically when an office's orchestrator initiates a new kanban card for a new component.

```text
[REFERENCE]
Trigger: office orchestrator creates card C for component P

1. QUERY  — the office notifies the global orchestrator: "starting P in phase {n}"
2. CHECK  — global orchestrator evaluates phase-n mandatory cross-cutting concerns:
              ┌─────────────────────────────────────────────────────────────┐
              │ Phase-aware concern catalog (examples):                     │
              │   localization   — every user-visible string uses i18n      │
              │   observability  — every significant operation is logged    │
              │   security       — new API surface has auth guard           │
              │   accessibility  — UI components have ARIA labels           │
              │   error handling — all failure paths have recovery logic    │
              │   budget-safety  — all LLM calls are wrapped in budget gate │
              └─────────────────────────────────────────────────────────────┘
3. ANNOTATE — if any mandatory concern is relevant to P:
              global orchestrator annotates card C with the concern list
              as non-optional acceptance criteria
4. PROCEED — card C enters normal orchestration; concern list is part of its DoD
```

The concern catalog is configured per project in the building's global configuration. Phase numbering is declared in the project's `PLAN.md`.

### 4.3 Cross-Office Delegation

When an office's orchestrator determines that a task is better performed by a sibling office (different domain expertise, specialized role, different environment), it routes the request through the global orchestrator:

```text
[REFERENCE]
Office A orchestrator → [ACP] → Global orchestrator
  ↓ (routing decision: which office can handle this?)
Global orchestrator → [ACP relay] → Office B

Office B executes → streams response → Global orchestrator → Office A
```

The global orchestrator's routing decision is based on: declared office capabilities, current OfficeState (Paused/Hibernating offices are bypassed), and budget availability.

### 4.4 Building-Level Escalation

When an office's orchestrator encounters a deadlock, an ambiguous multi-office conflict, or a decision that affects multiple projects, it escalates to the global orchestrator. The global orchestrator may:

1. Resolve directly (within its declared authority)
2. Request a cross-office deliberation round (`l1-deliberation.md`) with the orchestrators of affected offices as participants
3. Escalate to the user (HITL gate, consistent with ORC-9)

The escalation chain ensures that no cross-cutting decision is made unilaterally by a single office's orchestrator.

### 4.5 Question dispatch (GO-7)

```text
[REFERENCE]
dispatch_questions(offices):
    pending := union of each office's held questions          // read-only, GO-4
    groups  := merge pending by what is being asked           // duplicates across offices and classes
    ordered := sort groups by (blocking first, then age)      // l1-intent-resolution IR-9
    present := take ordered under the pacing limit            // the rest stay held, visible
    on answer: route the answer to every originating office   // via ACP, GO-5; content untouched
```

The pacing limit is a presentation setting, not a bound on what an office may ask. A held question is never discarded, and a blocking question is never held behind a non-blocking one. Approvals are not questions (`l1-intent-resolution` IR-9 d) and travel on their own gate channel.

## 5. Implementation Notes

1. GO-4 (unified visibility) is implemented as a building-level event bus subscription: each office's OfficeState events are forwarded to the global orchestrator's aggregate view without polling.
2. GO-3 (phase-awareness enforcement) requires the project's `PLAN.md` phase structure to be machine-readable from the building-level configuration — the current frontmatter format in `tasks/phase-{N}.md` provides this.
3. Cross-office ACP relay (GO-5) is a thin routing layer — the global orchestrator inspects the message envelope (target office ID, session context) but does not read or modify message content.

## 6. Drawbacks & Alternatives

**Alternative: peer-to-peer cross-office communication** — offices communicate directly via ACP without a global orchestrator. Simpler for small setups, but creates a mesh of bilateral connections with no unified routing, visibility, or escalation authority. Rejected for multi-office environments.

**Alternative: each office asks the client directly (GO-7)** — simplest, but N offices become N independent interruption streams to one person, with duplicates and no ordering. Questions to a shared human are cross-office traffic like any other, which GO-1 already routes through the global orchestrator.

**Alternative: no phase-awareness — always "we'll add it later"** — the most common approach in practice, and the source of most architectural debt. Localization, observability, and security added retroactively cost 3–10× more than integrated progressively. GO-3 makes this the agent's invariant rather than a hope.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[ORCHESTRATION]` | `.design/main/specifications/l1-orchestration.md` | Per-office protocol that global orchestration builds on |
| `[ACP]` | `.design/main/specifications/l1-acp.md` | Inter-office message routing protocol |
| `[WORKSPACE]` | `.design/main/specifications/l1-workspace-lifecycle.md` | Office creation and routing |
| `[DELIBERATION]` | `.design/main/specifications/l1-deliberation.md` | Cross-office deliberation rounds (GO-6) |
| `[OFFICE-CTRL]` | `.design/main/specifications/l1-office-control.md` | OfficeState taxonomy for routing decisions (GO-5) |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.0 | 2026-06-24 | Core Team | Initial spec — GO-1…GO-6, building structure, phase-awareness protocol with concern catalog, cross-office delegation via ACP, building-level escalation |
| 1.1.0 | 2026-10-03 | Core Team | Added GO-7 + §4.5 — **one attention budget across offices**. With several offices running, each office's questions (intake clarifications, blocking questions, ask-backs) were bounded in isolation, so nothing bounded or ordered what one client saw in total. GO-7 puts the dispatch at the one place that already sees every office (GO-1/GO-4): it merges identical questions, orders blocking ahead of non-blocking, paces interruptions, and never drops a held question; it routes presentation and neither answers nor suppresses an office's question (GO-2). Applies `l1-intent-resolution` IR-9. Related Specifications extended with `l1-intent-resolution` and `l1-navigation-model`; one rejected alternative (each office asks directly). Additive — L1 stays Stable (C9); `l2-global-orchestration` carries GO-7 as a pending compliance row. |
