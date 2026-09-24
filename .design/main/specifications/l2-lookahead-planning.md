# Lookahead Planning

**Version:** 1.0.1
**Status:** Stable
**Layer:** implementation
**Implements:** l1-lookahead-planning.md

## Overview

The concrete lookahead engine in `crates/core`: a trigger detector that recognizes high-impact action categories, a budget-bounded consequence simulator that reasons over the orchestrator's context model without executing anything real, a conclusion dispatcher (CONFIRM / MODIFY / ESCALATE / BUDGET_EXHAUSTED), and an append-only decision-log writer. The engine is invoked by the orchestrator before high-impact delegation and falls back to the standard approval gate on budget exhaustion.

## Related Specifications

- [l1-lookahead-planning.md](l1-lookahead-planning.md) — the model this implements (LP-1…LP-6).
- [l2-orchestration.md](l2-orchestration.md) — invokes lookahead before high-impact delegation; hosts the ORC-9 approval-gate fallback.
- [l2-kanban-board.md](l2-kanban-board.md) — high-impact card operations are trigger sources.
- [l2-execution-workspace.md](l2-execution-workspace.md) — the worktree state the simulator reads (never mutates).
- [l2-self-improvement.md](l2-self-improvement.md) — consumes decision-log entries for calibration.

## 1. Motivation

The model requires a cheap simulation pass before expensive/irreversible actions, scoped to declared triggers and bounded by budget. Running the simulation over the orchestrator's existing context model (plus static analysis) avoids a separate simulation-engine process; the decision log doubles as calibration input for self-improvement.

## 2. Constraints & Assumptions

- The simulation reasons about likely consequences; it is not a deterministic external-system simulator.
- It runs in the orchestrator context and issues no real tool calls, writes, spawns, or network requests (LP-2).
- Depth N and token budget are per-trigger-category defaults, overridable in Local Settings → Office.
- Budget exhaustion falls back to the ORC-9 approval gate, never silent proceed (LP-5).
- Lookahead is a check in front of the existing gates, never a substitute for one: an automated reviewer can add friction, never grant (AG-11). A `CONFIRM` means the simulation found no reason to stop; the action still passes every approval gate, sandbox rule, and consent check that applies to it.
- The trigger detector is a heuristic (SEC-12). An action it fails to recognize skips only the lookahead — never the gates and confinement that apply to the action regardless.

## 3. Invariant Compliance (Layer 2)

| L1 Invariant | Implementation |
| --- | --- |
| LP-1 Trigger-scoped | A `TriggerCatalog` enumerates the eligible high-impact categories (§4.2); the detector matches a proposed action against it and only then invokes the engine. Non-catalog actions bypass lookahead entirely. |
| LP-2 No real execution | The simulator operates on a read-only state snapshot; the tool-dispatch path is disabled inside the simulation context — a simulated `action` yields a predicted-consequence record, never a side effect. |
| LP-3 Budget-bounded depth | `LookaheadBudget { max_depth, max_tokens }` is enforced as a hard limit; the projection loop halts at N or on token exhaustion and returns the partial conclusion. |
| LP-4 Conclusion before commit | The orchestrator awaits a `Conclusion` (even partial) before committing the action; commit is gated on the conclusion existing. |
| LP-5 Graceful fallback | `BUDGET_EXHAUSTED` routes to the ORC-9 approval gate; there is no code path where a triggered action proceeds unchecked after a failed simulation. |
| LP-6 Log persistence | Every conclusion (incl. partial) appends `{trigger, depth_reached, conclusion, action_taken}` to the append-only decision log before commit. |

## 4. Detailed Design

### 4.1 Engine flow

```text
[REFERENCE]
lookahead(action A, state S0):
  if not TriggerCatalog.matches(A): return Bypass          // LP-1
  budget := category_budget(A)                              // LP-3
  snapshot := extract_readonly(S0)
  for d in 1..=budget.max_depth:
     pred := simulate_step(A, snapshot_d)                   // LP-2 no real exec
     snapshot_{d+1} := project(pred)
     if unacceptable(snapshot_{d+1}): return conclude(ESCALATE|MODIFY)   // early term
     if tokens_spent > budget.max_tokens: break             // LP-3
  concl := conclude(...)                                    // CONFIRM|MODIFY|ESCALATE|BUDGET_EXHAUSTED
  decision_log.append(trigger=A, depth_reached, concl)      // LP-6 before commit (LP-4)
  return concl
```

### 4.2 Trigger catalog

| Category | Default depth |
| --- | --- |
| Branch merge / PR creation | 3 |
| Schema migration | 4 |
| File / directory deletion | 3 |
| Dependency version upgrade | 3 |
| Security policy change | 5 |
| Mass refactor (>30% files) | 3 |
| Office architecture change | 5 |

Per-category depth/budget overridable in Local Settings → Office.

### 4.3 Conclusions

`CONFIRM(A)` → A proceeds to the gates that apply to it. `MODIFY(A→A')` → A' proceeds in place of A, with the original logged; A' re-enters the trigger detector like any proposed action — a modification never exempts itself from lookahead — and if its own lookahead proposes a further modification, the action goes to the ORC-9 gate instead of looping. `ESCALATE` → ORC-9 gate, no execution. `BUDGET_EXHAUSTED` → ORC-9 gate (LP-5). Early termination fires ESCALATE the moment a step yields an unambiguously blocking consequence.

A category in the authority plane — a security policy change — is never executed on a lookahead conclusion at all: only the human principal authors it (SEC-10), so its lookahead informs the human's decision and always ends at the human gate.

### 4.4 Cost control

Trigger scoping (LP-1) + small depth (2–5) + early termination bound the token cost. The simulator reuses the orchestrator's context model and static analysis (e.g. codegraph reference counts for a deletion), so no separate engine process is spawned.

## 5. Implementation Notes

1. `simulate_step` reuses the orchestrator context model + codegraph static analysis; no separate simulation runtime.
2. Default N=3 captures most impactful consequences; deeper N is linear cost for diminishing returns.
3. The decision log feeds self-improvement: overridden conclusions become calibration data.

## 6. Drawbacks & Alternatives

**Alternative — lookahead on every action**: prohibitive cost. Rejected; LP-1 scopes to high-impact only.

**Alternative — always escalate high-impact to humans**: breaks the autonomous model. Lookahead resolves many cases before a human is needed — a `MODIFY` can turn an action that would require approval into one that does not — and escalation is the fallback, not the default. It never waives a gate the action itself still requires.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[MODEL]` | `.design/main/specifications/l1-lookahead-planning.md` | Invariants LP-1…LP-6 |
| `[ORCH]` | `.design/main/specifications/l2-orchestration.md` | ORC-9 approval-gate fallback (LP-5) |
| `[SELF-IMPROVE]` | `.design/main/specifications/l2-self-improvement.md` | Decision-log calibration consumer |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.1 | 2026-09-23 | Core Team | Consistency pass (2026-09-23): A `CONFIRM` conclusion executed high-impact actions (including a security policy change) with no further gate, making an automated reviewer a grantor — lookahead now only adds friction (AG-11), every applicable gate still applies, and an authority-plane change always ends at the human (SEC-10). A `MODIFY` executed the modified action without it ever being checked — it re-enters the detector, and a second modification escalates. The trigger detector is labeled a heuristic whose miss skips only the lookahead (SEC-12). |
| 1.0.0 | 2026-07-03 | Core Team | Initial implementation spec — trigger detector, budget-bounded no-real-exec simulator, conclusion dispatcher, append-only decision log; maps LP-1…LP-6. |
