# Office Deliberation

**Version:** 1.0.1
**Status:** Stable
**Layer:** implementation
**Implements:** l1-deliberation.md

## Overview

The concrete deliberation engine in `crates/core`: an orchestrator-initiated round runner that dispatches independent parallel arguments (no cross-reading), synthesizes them into a final decision with attribution, and writes an immutable round entry to the deliberation log. The log lives in the inbox SQLite database, in an append-only table of its own, and surfaces in the Channels sidebar tab. Parallel argument generation reuses the orchestration wave-execution model.

## Related Specifications

- [l1-deliberation.md](l1-deliberation.md) — the model this implements (DL-1…DL-9).
- [l2-orchestration.md](l2-orchestration.md) — wave-based parallel execution + budget protocol for rounds.
- [l2-inbox.md](l2-inbox.md) — the SQLite database the log lives in, in its own append-only table (§4.3).
- [l2-navigation.md](l2-navigation.md) — the Channels tab surfacing the deliberation log.
- [l2-role-catalog.md](l2-role-catalog.md) — preset roles hired on demand for specialty diversity (DL-2).

## 1. Motivation

The model requires independent reasoning, orchestrator finality, and a visible audit trail. Gathering arguments in parallel with no cross-reading enforces independence (DL-1); reusing the inbox store avoids a parallel persistence layer; the append-only log makes the office's reasoning legible in Channels.

## 2. Constraints & Assumptions

- Only the orchestrator initiates a round; workers cannot self-initiate.
- Participation defaults to 3; each round declares a token budget upfront, sized for every stage it will run: the synthesis share is reserved first, the critique round (DL-6) runs only if the remainder covers it, and the arguments share what is left.
- The orchestrator always decides — no voting/majority rule.
- Budget exhaustion truncates arguments with a visible marker; the round still concludes with a decision.

## 3. Invariant Compliance (Layer 2)

| L1 Invariant | Implementation |
| --- | --- |
| DL-1 Independent reasoning | Each participant runs in its own isolated session; arguments dispatch as a parallel wave (orchestration) with no participant reading another's output before all return. |
| DL-2 Specialty diversity | Participant selection prefers active workers with overlapping specialties, else hires preset-catalog roles for the round (released after), and may mix model tiers for perspective breadth. |
| DL-3 Orchestrator finality | After all arguments return, the orchestrator synthesizes and emits the decision; no vote/majority code path exists. |
| DL-4 Append-only log | On close, the full round entry is written immutably to the deliberation log — its own append-only table, outside the inbox's delivery deletes and TTL pruning (§4.3); there is no update/delete path for a round row. |
| DL-5 Budget-bounded | Each round declares `token_budget`, reserving the synthesis share before splitting the rest; an argument exceeding its slice is truncated with a `truncated=true` marker, never silently dropped. |
| DL-6 Blind cross-critique round | **Pending.** The optional critique pass of §4.1 — each participant reviews the others' arguments in one parallel, independent pass — is specified but not built; it runs only when the remaining budget covers it, and is skipped (recorded as skipped) rather than truncated. |
| DL-7 Anonymized, randomized critique | **Pending** with DL-6: critics receive the arguments with identities stripped and in a per-critic shuffled order; identities return only at synthesis. |
| DL-8 Full-stance arguments | **Partial.** Participants may be given opposed stances; the devil's-advocate flag (§4.2) is the one stance specified today. The participant prompt that tells each to argue its angle fully rather than hedge — leaving the balancing to synthesis — is pending. |
| DL-9 Synthesis surfaces disagreement | **Partial.** The synthesis records its decision and reasoning; the required named sections — convergence, genuine clashes, blind spots the critique caught — and the explicit freedom to side with a minority are pending in the synthesis prompt and the log entry (§4.3). |

## 4. Detailed Design

### 4.1 Round runner

```text
[REFERENCE]
run_round(question, n, budget):
  participants := select(question, n)                 // DL-2
  remaining := budget - synthesis_reserve             // synthesis is never starved
  critique := remaining >= critique_cost(n)           // DL-6 — run only if affordable
  arg_budget := critique ? remaining - critique_cost(n) : remaining
  args := parallel_wave(participants, question, arg_budget/n)   // DL-1 no cross-read
  reviews := critique
    ? parallel_wave(participants, anonymized_shuffled(args))    // DL-6/DL-7 one blind pass
    : skipped
  decision := orchestrator.synthesize(args, reviews)  // DL-3/DL-9
  entry := { round_id, opened_at, question, participants, arguments: args,
             reviews, decision, reasoning, token_budget, truncated }
  deliberation_log.append(entry)                      // DL-4 immutable
  return decision
```

Wall-clock ≈ max(argument time) + max(review time, when run) + synthesis, since arguments and reviews each run in parallel.

### 4.2 Participant selection

Priority: (1) active workers with overlapping specialty; (2) preset roles hired for the round then released; (3) mixed model tiers for diversity. The orchestrator MAY designate one participant a **devil's advocate** via a per-round system-prompt constraint (not a separate agent type), skippable for low-stakes rounds.

### 4.3 Log entry

Stored in the inbox SQLite database, in its own append-only table `deliberation_log`: `{round_id, opened_at, question, participants[{role, model}], arguments[{role, position_summary, key_points[], confidence}], reviews?, decision, reasoning, token_budget, truncated}`. It is deliberately **not** an inbox message row: inbox rows are deleted when delivered and pruned after `GC_TTL_MS` regardless of status (`l2-inbox` §4.6), which would erase the audit trail DL-4 requires. The table has no update or delete path and is read directly by the Channels surface, with per-argument attribution, explicit decision+reasoning, and searchable/filterable history. A round's removal, if ever needed, follows the explicit, reported removal rule of the evidence archive (EA-5), never retention pruning.

## 5. Implementation Notes

1. Parallel argument generation uses the orchestration wave-execution model — no new parallelism primitive.
2. Log entries share the inbox SQLite database but live in their own append-only `deliberation_log` table, untouched by the inbox's drain and TTL garbage collection.
3. Devil's-advocate is a per-round flag on one participant's session prompt.

## 6. Drawbacks & Alternatives

**Alternative — open group discussion (cross-reading)**: violates DL-1; premature convergence. Rejected.

**Alternative — majority vote**: discards argument quality; loses minority insight the orchestrator judges significant. Rejected — orchestrator synthesizes (DL-3).

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[MODEL]` | `.design/main/specifications/l1-deliberation.md` | Invariants DL-1…DL-9 |
| `[ORCH]` | `.design/main/specifications/l2-orchestration.md` | Wave-parallel execution + budget |
| `[INBOX]` | `.design/main/specifications/l2-inbox.md` | Log entry SQLite backing |
| `[NAV]` | `.design/main/specifications/l2-navigation.md` | Channels tab surface |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.1 | 2026-09-23 | Core Team | Consistency pass (2026-09-23): The immutable deliberation log (DL-4) was stored as inbox message rows, which the inbox deletes on delivery and prunes after seven days — it now has its own append-only table. DL-6…DL-9 (added to the L1 in 1.1.0) were unmapped — rows added (critique round and anonymization Pending, full-stance and disagreement-surfacing synthesis Partial) and the round runner reserves the synthesis share before splitting the budget, running the critique only when affordable. |
| 1.0.0 | 2026-07-03 | Core Team | Initial implementation spec — orchestrator-initiated round runner, parallel independent arguments, synthesis with attribution, immutable log over the inbox store, Channels surface; maps DL-1…DL-5. |
