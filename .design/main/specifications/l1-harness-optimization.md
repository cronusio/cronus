# Harness Optimization

**Version:** 1.2.1
**Status:** Stable
**Layer:** concept

## Overview

An agent's realized capability depends not only on the model but on the **harness** —
the code that decides what context to collect, store, retrieve, and show. cronus
already knows how to *evolve* one harness against evidence (`l1-harness-engineering`)
and how to *measure* a model×harness pair (`l1-agent-coevaluation`). What it does not
yet own is the third, unifying activity: **searching a space of harness candidates
toward a frontier** — an outer-loop optimizer that proposes candidates, evaluates them,
and keeps the non-dominated best, instead of hand-tuning or hill-climbing a single
lineage.

This spec defines that optimizer. The harness is a point in a **candidate space**; an
**outer loop** (propose → evaluate → compare → select) runs above an **inner runtime**
(execute a candidate on tasks, write its archive). Candidates are generated from a
declared **mutation space**, scored by the existing evaluation machinery, and the
optimizer maintains a durable **frontier** of non-dominated candidates — partitioned by
comparability so only like-for-like candidates compete. A baseline may be reused rather
than rerun, but only when a **task-selection hash** proves it comparable; and a
candidate is accepted only when it improves *without* regressing beyond a bound and the
gain survives on held-out tasks. It is the *optimization* layer; it composes, and never
re-implements, evolution, scoring, and attestation.

## Related Specifications

- [l1-harness-engineering.md](l1-harness-engineering.md) — evolves ONE candidate along a lineage (HE-4/HE-5); this spec searches MANY over a space and composes HE-3 frozen pipeline, HE-6 held-out transfer, HE-8/HE-9 artifact-rich context.
- [l1-agent-coevaluation.md](l1-agent-coevaluation.md) — scores + comparability (ACE-5) + slices the optimizer's candidates read; measurement, which this spec turns into a search signal.
- [l1-evaluation-suites.md](l1-evaluation-suites.md) — grading machinery (ES-5 weighted metrics, ES-9 baseline/regression, ES-14 quality-tier) the acceptance rule and objective vector reuse.
- [l1-attestation.md](l1-attestation.md) — content-addressed candidate/config hashes (AT-2) the archive and frontier key on.
- [l1-staged-rollout.md](l1-staged-rollout.md) — [ADDED v1.2.0] the release contract HX-12 defers to: an accepted candidate reaches live work behind a named revertible switch at a declared exposure fraction, with a holdout on the prior default.
- [l1-nodus-environment.md](../../nodus/specifications/l1-nodus-environment.md) — a graded run (reward + trajectory) is one candidate's evaluation; NE-12 makes it an archivable candidate result.
- [l2-self-improvement.md](l2-self-improvement.md) — best-vs-current rollout gating; a scalar-best special case of the frontier here.
- [l1-model-adaptation.md](l1-model-adaptation.md) - [ADDED v1.2.1] A model-plane adaptation passes the same held-out, regression-bounded acceptance (HX-8) before adoption (MA-6); offline acceptance is necessary, never sufficient (HX-12).

## 1. Motivation

The three activities are genuinely distinct, and only two are specified:

- **Evolve one harness** (`l1-harness-engineering`) is a hill-climb on a single lineage:
  make an evidence-backed, single-component change; keep it if it helps; append to
  history. It never explores a *space* or keeps a *set* of alternatives.
- **Measure a pair** (`l1-agent-coevaluation`) is a diagnostic matrix: run (model,
  harness) over labeled tasks, read the gaps and slices. It tells you *where* a harness
  is weak, not *how to search* for a better one.
- **Search the space** — the missing piece — treats the harness as one point among
  many, generates candidates from a mutation space, and maintains a frontier of the
  non-dominated best. Its value over a hill-climb: it escapes local optima (a set, not
  a point), it reuses comparable baselines instead of paying to rerun them, and it makes
  "better" an honest, regression-bounded, held-out-verified judgment rather than a raw
  scalar max. Its value over measurement: it *acts* on the scores to move the frontier.

Without this concept, harness improvement is a single-lineage hill-climb that can stall
in a local optimum, re-pay for baselines it already has, and credit a "win" that
overfits the tasks it was tuned on. The invariants below make the search principled.

## 2. Constraints & Assumptions

- The optimizer searches over harness *procedure/config*, never model weights (weight
  tuning is a separate, out-of-scope lever) and never the inner runtime itself (HX-2).
- Evaluation is non-deterministic; the frontier reflects distributions per candidate,
  reusing the trials/stability model of `l1-evaluation-suites` (ES-6). This spec adds
  the search structure, not a new grader.
- Objectives are a *vector* (quality, cost, latency, stability, safety), so "best" is a
  non-dominated *set* (a frontier), not a single number; a scalar objective is the
  1-dimensional special case.
- The search is single-node and local (archive + frontier on disk); this is an
  optimization concept, not a distributed compute concept (INV-8).

## 3. Core Invariants

Rules every Layer 2 implementation MUST NOT violate:

- **HX-1 Harness space is searchable**: the harness is treated as a point in a candidate
  space searched by an outer-loop optimizer — propose, evaluate, compare, select — not
  only a single artifact hand-tuned or hill-climbed. The optimizer's product is a
  frontier of candidates, not one edited harness.

- **HX-2 Outer-loop / inner-runtime separation**: the optimizer (outer loop) is separate
  from the execution backend (inner runtime that runs a candidate on tasks and writes
  its archive). The optimizer proposes and selects over candidates; it MUST NOT mutate
  the inner runtime, and a candidate's execution is frozen while it is scored
  (composes `l1-harness-engineering` HE-3).

- **HX-3 Content-addressed archived candidates**: each candidate is content-addressed (a
  candidate hash over its code + config), resolvable by stable identity, and each
  evaluation writes a durable archive (a manifest, a summary, and per-task results).
  Provenance — candidate hash, config hash, launcher/context — travels with every
  result (composes `l1-attestation` AT-2, `l1-harness-engineering` append-only HE-7).

- **HX-4 Trace-rich proposer**: the candidate proposer has access to prior candidates'
  **source, scores, AND execution traces** through the archive — not only scalar scores
  or short summaries. Richer diagnostic context is a first-class requirement of the
  proposer, not an optional extra (composes HE-8 artifact extraction, HE-9 context
  freshness, `l1-dynamic-harness` DH-7 sourced experience).

- **HX-5 Declared, bounded mutation space**: candidates are generated by mutations drawn
  from a **declared** mutation space; the conservative default wraps a seed candidate
  rather than rewriting its core, bounding blast radius. The space is explicit and its
  mutations reproducible, so a search is repeatable and a variant is traceable to
  `(seed, mutation)` (composes HE-5 single-component change).

- **HX-6 Frontier of non-dominated candidates**: the optimizer maintains a **frontier** —
  the set of candidates not dominated across the objective vector (quality, cost,
  latency, stability, safety) — not a single scalar best. A candidate enters the
  frontier only if it is non-dominated; a dominated candidate is archived but not on the
  frontier. The frontier is both the baseline source and the search target (a scalar
  best with a regression guard is the 1-objective special case).

- **HX-7 Comparability-partitioned frontier and guarded baseline reuse**: candidates
  compete, and a baseline is reused, only **within a comparability partition** — keyed
  by a **task-selection hash** (plus the frozen-contract check of `l1-agent-coevaluation`
  ACE-5). A prior run may serve as the baseline instead of rerunning it *only if* its
  task-selection hash matches the candidate's; otherwise a fresh baseline run is
  required. Comparing candidates that ran on different task sets is forbidden.

- **HX-8 Regression-bounded, held-out-honest acceptance**: a candidate is accepted as
  "better" only if it improves the objective **and** does not regress more than a
  declared bounded fraction of tasks — never a raw scalar maximum. The credited gain
  must survive on **held-out** tasks not used to propose it (train/eval-disjointness);
  and reward-hacking guards reject a gain that games the metric rather than the task. A
  win that regresses too much, overfits the tuning set, or games the grader is not a win
  (composes HE-6 transfer validity, `l1-evaluation-suites` ES-9/ES-13/ES-14).

- **HX-9 Budgeted, terminating search**: the search is bounded by an explicit budget
  (candidate count, evaluation count, wall-clock, or cost) and terminates on a declared
  criterion — budget exhausted, frontier stagnation, or target met. An unbounded search
  that never terminates is forbidden; the budget and termination reason are recorded.

- **HX-10 Durable, concurrency-safe, observable frontier**: the frontier and every
  candidate's provenance and scores are persisted durably with concurrency-safe writes
  (no lost updates under parallel evaluation) and are observable, so the search is
  auditable, **resumable** across restarts, and the frontier's evolution is inspectable.

- **HX-11 Budget-normalized evaluation & mutation-invariant metric**: [ADDED v1.1.0]
  when candidates in one search differ in dimensions that affect resource consumption
  (size, structure, strategy), their evaluations MUST be **budget-comparable**: the
  preferred design is **budget normalization** — the evaluation contract fixes the
  **resource budget** (wall-clock, compute, tokens, trials), not the workload, and every
  candidate is measured on **outcome achieved within that fixed budget** — so radically
  heterogeneous candidates compete directly and fairly, and efficiency is absorbed into
  the outcome (a faster candidate does more inside the budget) instead of being a second
  axis to hand-weigh. Where budgets do differ, the difference **partitions the frontier**
  (HX-7, sharpening the comparability contract's a-longer-budget-is-a-different-task
  rule into a design principle): scores earned under different budgets never compete.
  A budget-normalized optimum is honestly **platform-relative** — the best candidate
  *for this platform within this budget* — so the budget and the platform are recorded
  with every score (composing MR-14's estimate-vs-measurement honesty). And the scoring
  **metric MUST be mutation-invariant**: chosen so it remains meaningful across the
  *entire* mutable candidate space — a candidate mutation MUST NOT be able to change
  what the metric measures (a metric a mutation can redefine lets the search optimize
  the metric's blind spot rather than the outcome, a criteria-drift sibling of the
  loop-governance oracle-ownership rule).

- **HX-12 Offline acceptance is necessary, never sufficient — an accepted candidate ships staged**: [ADDED v1.2.0]
  HX-8's held-out, regression-bounded acceptance admits a candidate into the archive; it
  MUST NOT by itself promote that candidate to the default for live work. An accepted
  candidate reaches real work through a **named, instantly revertible switch at a declared
  exposure fraction**, with assignment computed deterministically and a reserved holdout
  left on the prior default — so a regression the held-out set could not see is bounded to
  the exposed fraction and reversible without shipping anything. The reason the offline
  gate cannot carry the whole decision is structural, not a matter of gate strength: the
  held-out set is a *sample of tasks*, while live work is a *distribution that moves* — the
  cases that matter most are the ones no evaluation set contained, and they arrive only in
  production. A candidate promoted straight to 100% on a held-out win has been validated
  against the past and deployed against the future. The exposure fraction, its guardrails,
  and its stopping rule are declared before exposure begins and frozen for its lifetime, so
  the search cannot select the moment its candidate looked best (the live analog of HX-8's
  held-out honesty and of the criteria-immutability discipline HX-11 invokes).

> L2 specs cannot reach RFC status until all invariants here are addressed in their "Invariant Compliance" section.

## 4. Detailed Design

### 4.1 Outer Loop over Inner Runtime

```text
[REFERENCE]
outer loop (this spec)                     inner runtime (l1-harness-engineering backend)
  propose(archive) -> candidate    ──────►  run(candidate, tasks) -> archive
  evaluate(candidate)              ◄──────  (manifest, summary, per-task results, traces)
  compare(candidate, baseline)                                    │
  update_frontier(result)                                         │
  select next / terminate (HX-9)                                  ▼
```

The outer loop never touches how a candidate runs; it only decides which candidates to
try and which to keep (HX-2).

### 4.2 Candidate, Archive, Provenance

```text
[REFERENCE]
Candidate := { id, hash(code ∥ config), source, config, seed?, mutation? }   // HX-3/HX-5
Archive(candidate) := { manifest, summary, tasks[ {name, passed, reward, trace} ] }  // HX-3/HX-4
Provenance := { candidate_hash, config_hash, launcher, task_selection_hash }  // HX-3/HX-7
```

The proposer reads whole archives — code, scores, and traces — not just the summary
(HX-4).

### 4.3 Mutation Space

```text
[REFERENCE]
MutationSpace := closed set of declared mutations               // HX-5
variant(seed, mutation) := wrap(seed, mutation)                  // conservative default
name(variant) := f"{seed}__{mutation}"                          // traceable to (seed, mutation)
```

Mutations are deterministic and reproducible; the default wraps rather than rewrites, so
a variant's blast radius is bounded and its provenance clear.

### 4.4 The Frontier

```text
[REFERENCE]
dominates(a, b) := a ≥ b on every objective AND a > b on at least one   // Pareto
update_frontier(f, c):
    partition := f[c.task_selection_hash]                        // HX-7 comparability partition
    if ∃ x ∈ partition: dominates(x, c):   archive c, do not add    // dominated
    else:                                  add c; drop any x now dominated by c   // HX-6
persist(f)  with an exclusive lock, atomic temp-file-then-rename   // HX-10 no lost updates
```

The frontier is keyed by comparability partition: a candidate is compared and admitted
only against candidates that ran the same task selection (HX-7).

### 4.5 Baseline Reuse Guard

```text
[REFERENCE]
resolve_baseline(candidate, frontier, prior_run?):
    if prior_run and prior_run.task_selection_hash == candidate.task_selection_hash:
        return reuse(prior_run)                                  // HX-7 comparable → cheap
    if frontier.best(candidate.task_selection_hash) exists:
        return frontier_best                                     // frontier-backed baseline
    return REQUIRES_FRESH_RUN                                    // not comparable → rerun
```

Reuse is a cost optimization *gated by comparability*: an incomparable baseline is never
silently used; the search pays for a fresh run instead.

### 4.6 Division of Labour

| Concern | Owner |
| --- | --- |
| Search the candidate space; maintain the frontier; guard reuse; accept honestly | this spec |
| Evolve one candidate (evidence-backed, single-component change) | `l1-harness-engineering` |
| Score a candidate; comparability contract; slices | `l1-agent-coevaluation` / `l1-evaluation-suites` |
| Content-address a candidate | `l1-attestation` |
| Execute one graded run (reward + trajectory) | `l1-nodus-environment` |

This spec adds the *search + frontier* structure; it never re-implements a grader, an
evolver, or a runtime.

## 5. Implementation Notes

1. Key the frontier and every comparison by the comparability partition first (HX-7); a
   comparison drawn across partitions is a silent correctness bug.
2. Persist the frontier under an exclusive lock with atomic replace (HX-10) so parallel
   evaluations never lose an update; make the search resumable from the persisted state.
3. Default the mutation space to wrappers (HX-5); admit core-rewriting mutations only
   behind the same safety gate a single-component change uses.
4. Hold out a task split for acceptance (HX-8); never accept on the split used to
   propose.

## 6. Drawbacks & Alternatives

- **Search cost.** Evaluating many candidates is expensive. Mitigated by comparability-
  guarded baseline reuse (HX-7 — don't rerun a comparable baseline), a budget (HX-9), and
  a conservative mutation space (HX-5 — cheap wrappers before expensive rewrites).
- **Frontier bookkeeping.** Maintaining a non-dominated set is more machinery than a
  single best. Justified: a set escapes local optima a hill-climb cannot, and the
  1-objective case degenerates to a best-with-guard cheaply.
- **Alternative — fold into `l1-harness-engineering`.** Rejected: engineering evolves ONE
  harness along a lineage; the candidate space, mutation space, frontier, and
  comparability-guarded reuse are a distinct *optimization* activity that *drives*
  engineering (it proposes what engineering then refines), the same way coevaluation
  measures what this optimizes.
- **Alternative — scalar best only.** Rejected (HX-6): a single scalar collapses a
  multi-objective trade (a cheaper, slightly-lower-quality harness may be the right pick)
  and hides the frontier a user should choose from.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[HARNESS-ENG]` | `.design/main/specifications/l1-harness-engineering.md` | Per-candidate evolution + frozen pipeline (HE-3/HE-5/HE-6) this optimizer drives. |
| `[COEVAL]` | `.design/main/specifications/l1-agent-coevaluation.md` | Comparability contract (ACE-5) the frontier partitions on. |
| `[SUITES]` | `.design/main/specifications/l1-evaluation-suites.md` | Grading + regression/quality-tier the acceptance rule reuses. |
| `[ATTEST]` | `.design/main/specifications/l1-attestation.md` | Content-addressed candidate hashing (AT-2). |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.2.1 | 2026-07-23 | Core Team | Related Specifications gains `l1-model-adaptation` — a model-plane adaptation passes the same held-out, regression-bounded acceptance (HX-8) before adoption (MA-6). No invariant changed. (Row reconstructed from version control in the 2026-09-24 consistency pass.) |
| 1.2.0 | 2026-07-23 | Core Team | Added HX-12 — **offline acceptance is necessary, never sufficient**: HX-8's held-out regression-bounded acceptance admits a candidate to the archive but may not by itself promote it to the default for live work; an accepted candidate reaches real work behind a named, instantly revertible switch at a declared exposure fraction with deterministic assignment and a reserved holdout on the prior default, so a regression the held-out set could not see is bounded to the exposed fraction and reversible without shipping. The reason is structural rather than a matter of gate strength — the held-out set is a *sample of tasks* while live work is a *distribution that moves*, so a candidate promoted straight to 100% on a held-out win has been validated against the past and deployed against the future. Exposure fraction, guardrails, and stopping rule are declared before exposure and frozen for its lifetime (the live analog of HX-8's held-out honesty and HX-11's criteria-immutability). Related Specifications extended with `l1-staged-rollout`. |
| 1.1.0 | 2026-07-09 | Core Team | Added HX-11 (budget-normalized evaluation & mutation-invariant metric) — when candidates in one search differ in resource-affecting dimensions, evaluations MUST be budget-comparable: preferred design is budget normalization (the evaluation contract fixes the resource budget — wall-clock/compute/tokens/trials — not the workload, every candidate measured on outcome-within-fixed-budget), so radically heterogeneous candidates compete directly, efficiency is absorbed into the outcome rather than hand-weighed as a second axis, and the optimum is honestly platform-relative (budget + platform recorded with every score, composing MR-14 estimate-vs-measurement honesty); differing budgets partition the frontier (HX-7, sharpening the ACE-5 a-longer-budget-is-a-different-task row into a design principle), scores under different budgets never competing; and the scoring metric MUST be mutation-invariant — chosen to remain meaningful across the entire mutable candidate space, a mutation never able to change what the metric measures (a criteria-drift sibling of the loop-governance oracle-ownership rule). Invariant-only addition. Distilled from an adoption pass over an external autonomous-experimentation-loop reference whose core (autonomous modify→run→measure→keep/discard loop, mutable-surface restriction with an immutable oracle, experiment journal) was already realized by HX-1…HX-10 + l1-loop-governance mutation-rights/oracle-ownership + NE-12 — HX-11 captures the genuine delta (fixed-budget comparability + metric invariance). L1 stays Stable (C9, additive); the nodus realization is l1-nodus-environment NE-13. |
| 1.0.0 | 2026-07-02 | Core Team | Initial spec — harness optimization as an outer-loop search over a candidate space, the third activity beside harness-engineering (evolve one) and agent-coevaluation (measure): searchable space (HX-1); outer-loop/inner-runtime separation (HX-2); content-addressed archived candidates with provenance (HX-3); trace-rich proposer reading source+scores+traces (HX-4); declared bounded wrapper-first mutation space (HX-5); frontier of non-dominated candidates across the objective vector, not a scalar best (HX-6); comparability-partitioned frontier + task-selection-hash-guarded baseline reuse or fresh run (HX-7); regression-bounded, held-out-honest, train/eval-disjoint, reward-hacking-guarded acceptance (HX-8); budgeted terminating search (HX-9); durable, concurrency-safe, resumable, observable frontier (HX-10). Composes harness-engineering / agent-coevaluation / evaluation-suites / attestation / nodus-environment; adds the search+frontier layer none owned. HX-8 also formalizes the reward-hacking-filter + train/eval-disjointness mechanic. |
