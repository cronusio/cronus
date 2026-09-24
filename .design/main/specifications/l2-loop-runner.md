# Loop Runner

**Version:** 1.0.1
**Status:** Stable
**Layer:** implementation
**Implements:** l1-loop-governance.md

## Overview

The engine-side mechanic that turns the loop-governance concept into a running
component: a **loop runner** that drives an autonomous loop turn after turn, plus a
**loop governor** that enforces the declared class, mutation manifest, oracle, and
ceiling. It is the single place where "run an agent in a loop" is implemented, so every
loop in the system — execution or evolution — inherits the same governance instead of
each subsystem re-deriving its own retry-until-done logic.

The runner does not re-implement isolation, budgeting, task selection, or model routing.
It *composes* the existing engine subsystems behind one governed loop contract:
execution workspaces for isolation, the task graph for work, the budget engine for the
ceiling, the model router for oracle/judge lineage, version control for rollback, and
session checkpoints for state externalization.

Placement follows the decomposed crate topology (contract ← domain ← adapters + facade):
the pure loop logic — the runner, the governor, and the `LoopSpec`/`MutationManifest`/
`Oracle`/`Ceiling` types — is I/O-free domain logic in `crates/domain`; the cross-crate
types the CLI and facade name live in `crates/contract`; the adapter-touching
composition that hands the runner real subsystems (an actual execution workspace, a live
budget handle, a model-router binding, a session-checkpoint store) is wired in
`crates/core` (the facade/composition root), the same seam split used for other
adapter-composing defaults. The portable provider traits it leans on live in
`crates/nodus`. The CLI/TUI expose thin bindings; the library method is the source of
truth.

## Related Specifications

- [l1-loop-governance.md](l1-loop-governance.md) - The L1 concept this implements (LG-1…LG-12).
- [l2-execution-workspace.md](l2-execution-workspace.md) - Worktree isolation + finalize write-back; the per-iteration sandbox and the rollback substrate for the immutable set.
- [l2-budget-engine.md](l2-budget-engine.md) - Hierarchical cost ceiling feeding LG-6 termination.
- [l2-agent-autonomy.md](l2-agent-autonomy.md) - Action caps + approval gate; the human-oracle path and an independent ceiling source.
- [l2-model-router.md](l2-model-router.md) - Distinct-lineage binding selection for the independent-judge oracle (LG-4).
- [l2-agent-session.md](l2-agent-session.md) - The turn skeleton each iteration runs; loop/stop seams.
- [l2-session-checkpoint.md](l2-session-checkpoint.md) - Plan/status externalization and fresh-context reconstruction (LG-5).
- [l2-orchestration.md](l2-orchestration.md) - The /goal+judge+budget loop and wave executor that become callers of the runner.
- [l2-workflow-runtime.md](l2-workflow-runtime.md) - nodus executor (`~UNTIL MAX:n`, `AuditProvider`, validators) the runner binds to.

## 1. Motivation

Today the engine has several independent loops: the orchestration /goal loop, the
kanban execution loop, the harness evolution loop, the deep-research loop, the heartbeat.
Each enforces termination, retries, and (sometimes) verification its own way, and none of
them declares whether it is allowed to mutate its own plan or criteria. Consolidating the
loop mechanic gives three concrete wins: a single audited place where ceilings and
oracles are enforced; a structural guarantee that no loop can edit the criteria that
judge it; and a uniform run record so any loop's iterations, mutations, and verdicts are
inspectable the same way.

## 2. Constraints & Assumptions

- The runner never pushes to a git remote; iteration isolation and rollback use the
  local worktree / virtual-staging machinery (no-remote-git contract).
- `criteria` is not representable in a `MutationManifest`'s mutable set — LG-3 holds by
  construction, not by a runtime check that could be bypassed.
- The ceiling is owned by the runner and is enforced even if the actor never yields a
  stop; a model-produced tool call cannot raise it.
- Oracle lineage comparison uses the model router's binding identity; when only one
  lineage is available, the runner records `reduced_confidence` rather than failing.
- The runner is host-agnostic in its core types; nodus provider traits are the only
  abstraction it requires, satisfying the portability contract.

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| LG-1 Declared loop class | `LoopSpec.class: LoopClass {Execution, Evolution}` is required to construct a runner; a spec without a class fails to build. Evolution loops nest an execution `LoopSpec` for candidate evaluation. |
| LG-2 Mutation manifest | `LoopSpec.manifest: MutationManifest`; the governor classifies every write the iteration made and rejects kinds outside `mutable` before anything leaves the isolated workspace (§4.4). Manifest is serialized into the run record. |
| LG-3 Criteria immutability | `MutationManifest.mutable` is a `Vec<MutableArtifact>` whose enum has **no `Criteria` variant**, so criteria cannot be *declared* mutable; and because the oracle's own inputs (a validator's test files, criteria documents) classify as criteria and an unclassifiable write is illegal (§4.4), they cannot be *written* either. A criteria change is only performed by an enclosing evolution runner via the §4.6 escalation path. |
| LG-4 Oracle ownership | `Oracle` enum: `Deterministic(Validator)`, `Judge(ModelBinding)`, `Human(ApprovalGate)`. The governor compares actor vs oracle binding lineage (via model-router identity) and stamps `reduced_confidence` on the iteration record when equal. |
| LG-5 State externalization | Plan + status persisted via the session-checkpoint three-file hierarchy; each iteration calls `reconstruct_context()` (fresh context from artifacts) rather than inheriting the prior transcript. |
| LG-6 Independent ceiling | `Ceiling { max_iterations, budget_ref, deadline, patience }` evaluated by the runner before each iteration; budget drawn from the budget engine, action caps from agent-autonomy. Stop fires regardless of actor/oracle state. |
| LG-7 Tier-escalation gate | `escalate()` requires a separated oracle, an external-novelty source handle, and a held-out evaluation; promotes only on `delta >= margin && regression <= bound`, recording the attempt either way. Reuses the dynamic-harness promotion gate machinery. |
| LG-8 Mutation ledger | Every applied mutation appends an `AuditProvider` event `(run_id, step_index, artifact_kind, summary, why)`; evolution iterations add `predicted_flip` scored into `keep/revert/partial`. Append-only; never overwritten. |
| LG-9 Cheapest trustworthy oracle | The runner prefers a `Deterministic` oracle for the inner done-check when the `LoopSpec` provides a validator; `Judge`/`Human` are used only when no deterministic validator is declared. Advisory — logged, not blocked. |
| LG-11 Scope-restart authority | **Pending.** The runner's loops re-attempt a unit (execution) or a generation (evolution); no whole-scope restart loop exists yet. One added later declares its class, counts restarts against the ceiling, reconstructs each restart from durable artifacts, records restarts in the ledger, accepts a restart request only at the scope's own run boundary (a nested stage's request is refused), and runs only where the scope's effectful steps are idempotent or compensated. |
| LG-12 Best-so-far termination | `Ceiling.patience` counts consecutive iterations that reach no **new best** value of the oracle-owned objective — not iterations that fail to beat the previous one, which an oscillating loop never stops doing. The count ceiling and the stagnation bound are both live and the first to fire ends the loop; a stagnation stop reports the best state reached and the specific unresolved items (§4.2). |
| LG-10 Objective persistence across in-session reduction | Two runtime shapes, one durable-slot principle. **Discrete iterations** (the §4.2 execution runner, §4.3 evolution generations) already reconstruct from the plan/status slot each iteration (LG-5), so a dropped transcript loses nothing. **Continuous-session** loops (a standing-goal heartbeat, an indefinitely-running session that compacts in place) add `LoopSpec.objective_slot: ObjectiveSlot` — the standing objective + a progress cursor persisted in the session-checkpoint durable store. The governor **re-projects it into every turn** as a protected region (`l1-context-compression` CC-9), and re-materializes it if evicted, so in-place compaction can never drop the north-star. Durable progress is written to the LG-8 ledger **before** any lossy reduction runs (CC-10); after a reduction the ledger — not the compacted transcript — is the authoritative progress source (`l1-development-workflow` DW-5). The objective is phrased idempotent/resumable so a turn that re-reads it after supporting detail was compacted **resumes** rather than restarts or redoes completed work. A loop whose `objective_slot` is absent is a discrete-iteration loop governed by LG-5 alone. |

## 4. Detailed Design

### 4.1 Core types

```text
[REFERENCE]  // crates/domain — I/O-free domain types
enum LoopClass { Execution, Evolution }

enum MutableArtifact {            // NOTE: no `Criteria` variant — LG-3 by construction
  Scratch, Plan, Knowledge, ValidationInput, Prompt, Tools,
}

struct MutationManifest {
  class:   LoopClass,
  tier:    u8,                    // 0..=4 (l1-loop-governance §4.2)
  mutable: Vec<MutableArtifact>,
}

enum Oracle {
  Deterministic(Validator),       // tests / exit code / output-contract validator
  Judge(ModelBinding),            // independent lineage via model router
  Human(ApprovalGate),            // l2-agent-autonomy approval path
}

struct Ceiling {
  max_iterations: u32,
  budget_ref:     BudgetHandle,   // l2-budget-engine
  deadline:       Option<Instant>,
  patience:       u32,            // consecutive iterations without a new best value of the
                                  // oracle-owned objective before stop (LG-12)
}

struct ObjectiveSlot {              // LG-10 — continuous-session objective persistence
  objective: String,               // the standing north-star, re-projected every turn
  progress:  ProgressCursor,       // idempotent/resumable progress, persisted to the ledger
}

struct LoopSpec {
  class:    LoopClass,
  manifest: MutationManifest,
  oracle:   Oracle,
  ceiling:  Ceiling,
  workspace_kind: ProviderType,    // l2-execution-workspace (worktree default)
  objective_slot: Option<ObjectiveSlot>, // Some = continuous-session (LG-10 re-projection);
                                   // None = discrete-iteration loop (LG-5 fresh-context only)
}
```

### 4.2 Execution-loop runner

The execution runner re-attempts a fixed unit until the oracle passes or the ceiling
fires. It composes the task graph (work), execution workspace (isolation), session
(the turn), and version control (commit/rollback).

```text
[REFERENCE]
run_execution(spec: LoopSpec, unit: WorkUnit) -> LoopOutcome:
  workspace = allocate_workspace(spec.workspace_kind, unit)     // isolated worktree
  iteration = 0
  loop:
    governor.check_ceiling(spec.ceiling, iteration)             // LG-6 — may stop here
    ctx = reconstruct_context(unit, plan, status)               // LG-5 fresh context
    result = run_turn(ctx)                                      // l2-agent-session
    if governor.guard_writes(result.mutations, spec.manifest) is Err(e):   // LG-2/LG-3
      rollback_to_snapshot(workspace); record_violation(run_id, iteration, e)
      iteration += 1; continue                                  // never judged
    verdict = oracle.judge(result)                              // LG-4
    record_iteration(run_id, iteration, result, verdict)        // LG-8 ledger
    if verdict.done:
      finalize_workspace(workspace)                             // write-back, commit
      return Done(verdict)
    else:
      rollback_to_snapshot(workspace)                           // VC-4 — discard attempt
      append_status(status, verdict.feedback)                   // carried via plan/status
      iteration += 1
```

Key points: the actor's "I think I'm done" is *advisory*; only `oracle.judge` sets
`verdict.done` (LG-4). A failed attempt is rolled back so the next iteration starts from a
clean snapshot, and only the compact status note carries forward (LG-5). The turn's writes
land in the isolated workspace; `guard_writes` runs **before the oracle judges** and before
anything is written back, so an attempt that touched an artifact kind outside the manifest
is rolled back instead of judged (LG-2) — an actor that edited the tests could otherwise be
graded by the tests it edited. Since `Criteria` is not a `MutableArtifact`, criteria writes
are illegal (LG-3). When the ceiling or the stagnation bound stops the loop, the outcome
carries the best state reached and the unresolved items the oracle still reports (LG-12).

### 4.3 Evolution-loop runner

The evolution runner wraps the harness EVALUATE→ANALYZE→IMPROVE loop. It changes harness
components (its manifest's `mutable` set) but holds the evaluation pipeline (the oracle)
frozen, and it nests an execution runner to *score* each candidate.

```text
[REFERENCE]
run_evolution(spec: LoopSpec, harness, task_set, held_out) -> Harness:
  loop:
    governor.check_ceiling(spec.ceiling, gen)                   // LG-6
    scores = for task in task_set:                              // EVALUATE
               run_execution(inner_spec(harness), task)         // nested execution loop
    artifact = analyze(scores)                                  // ANALYZE → durable artifact (HE-8)
    candidate = improve(artifact, harness)                      // IMPROVE — manifest-bounded mutation
    governor.guard_writes(candidate.diff, spec.manifest)        // LG-2 — never touches criteria
    predicted = candidate.predicted_flips
    actual = re-evaluate(candidate, task_set)
    verdict = score(predicted, actual)                          // keep/revert/partial (DH-8)
    record_generation(run_id, gen, candidate, verdict)          // LG-8
    if verdict == keep: harness = candidate
    if target_reached or patience_exhausted: break
  if not transfer_valid(harness, held_out):                     // HE-6 — overfit to the search set
    record_rejection(run_id, harness)
    return best_transferring(run_id)                            // the last harness that held on
                                                                // held-out work, else the input
  return harness
```

The oracle here is the frozen evaluation pipeline (HE-3); it is never in
`spec.manifest.mutable`. Making criteria mutable for a *lower* loop is the only thing that
routes through §4.6.

### 4.4 The governor

A single enforcement object every runner calls, analogous to the autonomy
`gate_decision` single-enforcement-point pattern:

```text
[REFERENCE]
impl Governor {
  check_ceiling(c: Ceiling, n: u32) -> Continue | Stop(reason)
     // iterations / budget / deadline / patience — independent of actor & oracle (LG-6)

  guard_writes(muts: &[Mutation], m: &MutationManifest) -> Result<(), GuardError>
     // each mutation's artifact_kind must be in m.mutable; else GuardError::IllegalMutation
     // (criteria can't even be expressed, so it can't be requested) (LG-2/LG-3).
     // artifact_kind is assigned by path against the loop's artifact map: the oracle's own
     // inputs (validator test files, criteria documents) map to criteria, and a write that
     // maps to no kind is illegal — classification fails closed

  judge(o: &Oracle, r: &TurnResult) -> Verdict
     // dispatches to deterministic/judge/human; stamps reduced_confidence on lineage match (LG-4)
}
```

`GuardError::IllegalMutation` rolls the iteration back via VC-4 and records the
violation in the ledger — an attempt to mutate outside the manifest is a recorded event,
not a silent drop.

### 4.5 Oracle wiring

```text
[REFERENCE]
Deterministic — reuses l1-output-contracts validators: schema, callable (exit code /
                grep / test command), and LLM-criteria (only when no mechanical check
                exists — LG-9 keeps this the last resort).
Judge         — model-router selects a binding whose lineage != actor's; if none is
                available, run with actor's lineage and set reduced_confidence=true.
Human         — routes through the agent-autonomy approval gate (10-min TTL); in
                background/cron context, falls back to deterministic or stops (no silent
                self-approval).
```

### 4.6 Escalation path (criteria change / self-evolution)

A criteria change or a Tier-4 self-modification is performed by an *enclosing* evolution
runner, never inside the judged loop:

```text
[REFERENCE]
escalate(target_loop, change) -> Promoted | Rejected:
  require target_loop.oracle.lineage != actor.lineage           // separated oracle (LG-7)
  require novelty_source.has_external_input()                   // AFS-13 / LG-7
  candidate = apply(change, target_loop.spec)                   // may touch prompt/tools/criteria
  before = evaluate(target_loop, held_out)
  after  = evaluate(candidate,   held_out)                      // held-out, not search set
  if after.metric - before.metric >= margin and regression(after) <= bound:
      record_promotion(run_id, change); return Promoted
  else:
      record_rejection(run_id, change); return Rejected
```

### 4.7 Command surface

| Action | CLI | TUI | Library (no code) |
| --- | --- | --- | --- |
| run an execution loop on a unit | `cronus loop run <unit-id> [--tier N] [--max-iter N]` | `/loop run <unit-id> …` | `loop.run_execution(spec, unit) -> LoopOutcome` |
| run an evolution loop | `cronus loop evolve <harness-id> [--target <score>]` | `/loop evolve …` | `loop.run_evolution(spec, harness, set) -> Harness` |
| inspect a loop's ledger | `cronus loop log <run-id>` | `/loop log <run-id>` | `loop.ledger(run_id) -> Vec<LedgerEntry>` |
| show a loop's manifest/oracle/ceiling | `cronus loop show <run-id>` | `/loop show <run-id>` | `loop.spec(run_id) -> LoopSpec` |

### 4.8 Crate placement

```text
[REFERENCE]
crates/domain/               // pure, I/O-free loop logic + types
  loop/
    spec.rs        // LoopSpec, MutationManifest, MutableArtifact, LoopClass, Ceiling, ObjectiveSlot
    governor.rs    // Governor: check_ceiling / guard_writes / judge
    execution.rs   // run_execution
    evolution.rs   // run_evolution (wraps harness EVALUATE→ANALYZE→IMPROVE)
    oracle.rs      // Oracle enum + lineage comparison
    escalate.rs    // §4.6 promotion gate
crates/core/                 // facade / composition root — adapter-touching wiring only
  loop_bootstrap.rs // hands the runner real subsystems: an execution workspace, a live
                    // budget handle, a model-router binding, a session-checkpoint store
crates/nodus/                // (existing seams, reused — no host types added)
  ModelProvider    // judge-oracle lineage
  AuditProvider    // mutation ledger event stream
  PolicyProvider / StorageProvider   // escalation gate + provisional-vs-promoted state
```

Dependencies point inward: `crates/domain` is I/O-free and depends only on
`crates/contract` types + `crates/nodus` provider traits — never on an adapter crate or
the facade; the adapter-touching composition that instantiates real subsystems lives in
`crates/core`. `nodus` stays std-only and host-agnostic. The runner is a domain consumer
of nodus's provider traits, exercising the same two-host rule the dynamic-harness spec
relies on.

## 5. Implementation Notes

1. `spec.rs` first — the types are the contract; with `MutableArtifact` lacking a
   `Criteria` variant, LG-3 is enforced by the type system before any logic exists.
2. `governor.rs` — the three enforcement methods, unit-tested against illegal-mutation
   and ceiling-overrun cases.
3. `execution.rs` over the existing workspace + session + version-control machinery.
4. `oracle.rs` with the deterministic path first (reuse output-contract validators),
   then judge (model-router lineage), then human (approval gate).
5. `evolution.rs` wrapping the harness loop, nesting `run_execution` for candidate
   scoring; `escalate.rs` last (needs held-out + novelty + separated oracle wired).

## 6. Drawbacks & Alternatives

- **Consolidation churn.** Existing loops (orchestration /goal, kanban execution, deep
  research) must be migrated to call the runner to gain the governance. Mitigation:
  migrate incrementally — each subsystem becomes a *caller* of the runner with its own
  `LoopSpec`; the runner does not change their behavior, only enforces a declared
  contract around it.
- **Ceiling double-counting.** Both the runner and the budget engine track spend.
  Mitigation: the runner holds a `BudgetHandle` into the engine rather than a private
  counter — one source of truth.
- **Judge lineage may be unavailable.** With a single local model, every judge oracle is
  same-lineage. The runner degrades to `reduced_confidence` (LG-4) rather than failing,
  but the limitation is real and surfaced in the run record.
- **Alternative — keep per-subsystem loops.** Rejected: it perpetuates inconsistent
  ceiling/oracle handling and leaves criteria-drift unguarded in any loop that evolves
  its own artifacts.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[LOOP-GOV]` | `.design/main/specifications/l1-loop-governance.md` | The governed contract (LG-1…LG-12) this crate enforces. |
| `[EXEC-WS]` | `.design/main/specifications/l2-execution-workspace.md` | Worktree isolation + finalize + VC-4 rollback substrate. |
| `[BUDGET]` | `.design/main/specifications/l2-budget-engine.md` | Cost ceiling feeding LG-6. |
| `[AUTONOMY]` | `.design/main/specifications/l2-agent-autonomy.md` | Approval gate (human oracle) + independent action caps. |
| `[ROUTER]` | `.design/main/specifications/l2-model-router.md` | Distinct-lineage binding for the judge oracle (LG-4). |
| `[CHECKPOINT]` | `.design/main/specifications/l2-session-checkpoint.md` | Plan/status externalization + fresh-context reconstruction (LG-5). |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.1 | 2026-09-23 | Core Team | Consistency pass (2026-09-23): LG-11 (scope-restart authority, Pending) and LG-12 (best-so-far termination — `patience` now counts iterations without a new best, not without improvement over the previous one) were unmapped. `guard_writes` ran after the turn but was said to reject writes "before they reach disk", and nothing classified an actual file write — writes now land in the isolated workspace, are classified by path with the oracle's own inputs mapped to criteria and unclassifiable writes illegal, and an illegal attempt is rolled back before the oracle judges it (an actor that edited the tests would otherwise be graded by them). The evolution runner ended on an `assert` for held-out transfer — an overfit harness now yields the last transferring one. Range references updated to LG-1…LG-12. |
| 1.0.0 | 2026-07-17 | Core Team | Promoted Draft→Stable via `/magic.spec` in the same pass that promoted its L1 parent `l1-loop-governance` to Stable — the Draft status was held only by the layer constraint (an L2 cannot pass RFC until its L1 parent is Stable), not by incompleteness. Two substantive closures made it Stable-ready: **(1)** added the missing **LG-10 compliance row** (objective persistence across in-session reduction) with a supporting `ObjectiveSlot` type on `LoopSpec` — the L2 was authored at 0.1.0 before LG-10 was added to the L1 at 0.2.0, so its Invariant Compliance table covered only LG-1…LG-9; it now addresses all ten (the L1's own gate for an L2 to reach RFC/Stable). **(2)** reconciled crate placement to the post-decomposition topology (Phase 13): the I/O-free runner/governor/types move from the pre-split `crates/core` to `crates/domain`, with the adapter-touching composition (`loop_bootstrap.rs`: real execution workspace, budget handle, model-router binding, session-checkpoint store) wired in the `crates/core` facade — the same domain/facade seam split the shipped adapters use. spec-critic + prompt-engineer PASS. Now Stable-but-unbuilt → the next `/magic.task` opens its build phase. |
| 0.1.0 | 2026-06-25 | Core Team | Initial Draft — loop runner + governor mechanic for `crates/core` (composing execution-workspace, budget-engine, agent-autonomy, model-router, session-checkpoint, version-control, nodus provider seams); `LoopSpec`/`MutationManifest`/`Oracle`/`Ceiling` types with `MutableArtifact` having no `Criteria` variant (LG-3 by construction); execution + evolution runners; single-point governor (check_ceiling/guard_writes/judge); oracle wiring (deterministic/judge/human with lineage reduced-confidence); escalation path for criteria change / self-evolution; crate placement. Draft pending L1 parent promotion to Stable. |
