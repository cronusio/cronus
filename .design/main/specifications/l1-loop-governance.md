# Loop Governance

**Version:** 1.4.0
**Status:** Stable
**Layer:** concept

## Overview

Every autonomous process in the system is some agent running *in a loop*: re-invoked
turn after turn until a condition says stop. This spec governs those loops. It makes
two things first-class that the existing harness specs only imply:

1. **Loop class** — every autonomous loop is one of two kinds, and the kinds have
   *opposite* rules. An **execution loop** re-attempts a *fixed* task until an oracle
   says it is done; nothing about the task or its success criteria is meant to change.
   An **evolution loop** *deliberately* changes the artifacts the agent works from
   (plan, knowledge, even the harness itself) between iterations so the agent improves.
   Treating an evolution loop as an execution loop wastes iterations (no learning is
   captured); treating an execution loop as an evolution loop invites uncontrolled drift.

2. **The right to mutation** — the load-bearing safety property. Each loop declares,
   from a closed taxonomy, exactly which artifacts an iteration may change. Everything
   not declared mutable is immutable. The one artifact that is *never* in the mutable
   set of the loop it governs is the **success criteria / the oracle definition**: an
   actor must never hold the right to edit the contract that decides whether it
   succeeded. Granting that right produces *criteria drift* — the agent quietly rewrites
   the goalposts and declares victory.

This is the governance keystone above the loop-related concepts: it classifies and
constrains the loops that [l1-harness-engineering.md](l1-harness-engineering.md)
(the evolution loop), [l1-dynamic-harness.md](l1-dynamic-harness.md) (within-run
adaptation), and [l1-agent-framework-skeleton.md](l1-agent-framework-skeleton.md) §5.7
(the autonomous heartbeat) each realize. It introduces no new subsystem; it names the
loop-class axis, the mutation-rights ladder, and the oracle-ownership contract, and
defers to the concrete specs for everything they already own.

## Related Specifications

- [l1-harness-engineering.md](l1-harness-engineering.md) — the canonical **evolution loop** (EVALUATE→ANALYZE→IMPROVE); HE-3 frozen evaluation is the oracle this spec generalizes, HE-7 append-only history is the mutation ledger, HE-9 fresh-context is LG-5.
- [l1-dynamic-harness.md](l1-dynamic-harness.md) — within-run adaptation; DH-3 bounded interception is the run-time analogue of LG-2, DH-8 falsification ledger of LG-8, DH-10 promotion gate of LG-7, DH-11 verification independence of LG-4.
- [l1-agent-framework-skeleton.md](l1-agent-framework-skeleton.md) — AFS-5 bounded coordination (LG-6), AFS-6 durable boundaries (LG-5), AFS-13 anti-collapse novelty (LG-7); §5.7 the heartbeat loop this spec classifies.
- [l1-execution-graph.md](l1-execution-graph.md) — the substrate an execution loop iterates over (supersteps, interrupt/resume, step budget).
- [l1-task-graph-model.md](l1-task-graph-model.md) — TG-5 next-unit selection and TG-13/TG-14 coordinator/executor split with guarded autonomous delivery; the work source of an execution loop.
- [l1-output-contracts.md](l1-output-contracts.md) — inline schema/criteria validators; a deterministic-or-judge oracle for the inner check (LG-4, LG-9).
- [l1-orchestration.md](l1-orchestration.md) — the /goal+judge+budget loop and ORC-11 error containment; one host of an execution loop.
- [l1-version-control.md](l1-version-control.md) — VC-4 virtual-staging atomicity; the rollback mechanism behind an enforced immutable set.
- [l2-loop-runner.md](l2-loop-runner.md) — the Layer-2 realization of this concept in the engine crates.
- [l1-context-compression.md](l1-context-compression.md) — CC-9 protected regions and CC-10 memory-safe lossy reduction are the context-side mechanisms LG-10 composes so a continuous-session loop's objective survives compaction.
- [l1-development-workflow.md](l1-development-workflow.md) — DW-5 (after a compaction event the durable ledger, not agent memory, is authoritative) is the progress-authority LG-10 generalizes from the dev workflow to any standing-goal loop.
- [l1-nodus-language.md](../../nodus/specifications/l1-nodus-language.md) - [ADDED v1.1.0] the workflow-runtime realization of the scope-restart loop (LG-11): a bounded, counted whole-run self-restart directive requestable only from the run-boundary stages, never a per-item step (NL-23).
- [l2-config-hotreload.md](l2-config-hotreload.md) - [ADDED v1.1.0] a scope-restart re-reads inputs/config, the safe alternative to mutating a running loop's own configuration mid-iteration (LG-11).
- [l1-acceptance-oracle.md](l1-acceptance-oracle.md) - [ADDED v1.2.0] the complement to LG-4 oracle **ownership**: LG owns *who may change* the oracle (never the actor being judged), AO owns *whether the oracle is worth obeying*. The two failures are independent — a properly owned, correctly frozen oracle that cannot fail freezes an empty contract, and an execution loop whose oracle always says *done* terminates on its first iteration forever.

## 1. Motivation

Autonomous loops are now everywhere in the system — a task runner that retries until
tests pass, a harness that improves itself across generations, a heartbeat agent
serving a standing goal, a deep-research engine that searches until its questions are
answered. They look similar (an agent, re-invoked, with a stop condition) and are
governed inconsistently. Two specific failures recur and neither is caught by the
existing per-subsystem specs:

- **Class confusion.** An execution loop that is secretly allowed to mutate its plan
  drifts away from the task it was given. An evolution loop run as if it were an
  execution loop re-attempts with no memory and never improves. The fix is to force
  each loop to *declare its class*, because the class dictates the mutation rules.

- **Criteria drift (the "right to mutation" failure).** The most dangerous capability
  is letting the actor change the very thing that judges it. If the agent can edit its
  success criteria, rewrite its tests, or appoint itself the oracle, then "done" becomes
  whatever the agent wants it to be. Tests pass, the metric is green, and the real task
  is unsolved. This is not hypothetical — it is the predictable end state of an
  unconstrained self-improving loop. The guard is structural: the criteria and the
  oracle are *outside* the mutable set of the loop they govern, and the right to declare
  "done" belongs to an authority separated from the actor.

A third, quieter motivation: **the oracle question is usually left implicit.** Who has
the right to say the task is finished — a deterministic check, an independent judge
model, or a human? When it is the same model that did the work, termination is only as
trustworthy as the model's self-assessment, which is exactly when it is least reliable.
Making the oracle explicit and separated turns a hidden assumption into an auditable
contract.

## 2. Constraints & Assumptions

- **Technology-agnostic.** This is a Layer 1 concept. It names no language or library;
  concrete bindings live in [l2-loop-runner.md](l2-loop-runner.md).
- **Additive over existing loops.** Where a concrete spec already governs a loop
  (harness evolution, runtime adaptation, the heartbeat), this spec classifies and
  constrains it; it does not re-specify it. It wins only on the loop-class axis, the
  mutation-rights ladder, and the oracle-ownership contract.
- **The mutation taxonomy is closed.** An iteration may declare mutability only over a
  fixed set of artifact kinds (§4.5). An undeclared kind is immutable by default.
- **The oracle is not the loop budget.** The right to declare *success* (oracle) and the
  right to declare *exhaustion* (the hard ceiling) are distinct authorities; a loop can
  stop without success. Both must exist (LG-4, LG-6).
- **On-device-first.** Loop state and ledgers stay local unless the user authorizes
  egress (inherited from the storage and security concepts).

## 3. Core Invariants

Layer 2 realizations and any conforming loop MUST honour these.

- **LG-1 Declared loop class.** Every autonomous loop declares its class —
  **execution** (re-attempt a fixed task to a done-condition) or **evolution**
  (deliberately change working artifacts to improve across iterations). A loop may
  *compose* both (an evolution loop whose each generation runs an inner execution
  loop), but each level declares its own class. An undeclared class is not run.

- **LG-2 Mutation-rights manifest.** Every loop declares, from the closed artifact
  taxonomy (§4.5), the set of artifact kinds an iteration may change. Anything not in
  the manifest is immutable for that loop. The manifest is part of the loop
  specification and is recoverable from the run record — mutation is never hidden
  control flow.

- **LG-3 Criteria immutability (anti-drift spine).** The success criteria and the
  oracle definition of a loop are NEVER members of that same loop's mutation manifest.
  An actor may not edit the contract that judges it. Changing the criteria of a loop is
  itself an action of a *higher* loop (an evolution loop, §4.6) with its own separated
  oracle — never a side effect inside the loop being judged.

- **LG-4 Oracle ownership.** The authority to declare an iteration "done" belongs to an
  **oracle** that is *separated from the actor*. The oracle is one of: a **deterministic**
  check (tests, exit code, schema/criteria validator), an **independent judge**
  (a model or reviewer of a different lineage than the producer), or a **human**. When
  the actor and the oracle share a lineage (the actor effectively grades itself), that
  is a permitted but **recorded reduced-confidence condition** — it generalizes DH-11
  from verification to the right of termination.

- **LG-5 State externalization.** Any loop state that must survive an iteration boundary
  lives in durable external artifacts (e.g. a plan and a status record), not solely in
  the actor's context window. A new iteration **reconstructs** its working context from
  those artifacts rather than inheriting the accumulated transcript of all prior
  iterations. (Reuses HE-9 fresh-context and AFS-6 durable boundaries.)

- **LG-6 Independent termination ceiling.** Every loop has a hard ceiling —
  max iterations, token/cost budget, or wall-clock — AND a no-progress stop, both
  enforced **independently of the actor's own "I'm done" claim** and independently of
  the oracle. A loop never relies solely on the actor deciding to stop. (Strengthens
  AFS-5 by separating the ceiling authority from the actor and the oracle.)

- **LG-7 Tier-escalation gate.** Raising a loop to a higher mutation tier (§4.2) —
  especially to **self-evolving**, where prompt, tools, or (via a higher loop) criteria
  become mutable — requires an explicit promotion gate: a separated oracle, an external
  source of novelty (new tasks/inputs/feedback, not the loop's own outputs), and a
  measured improvement that holds on held-out work. Higher tier demands stronger
  independent verification, not less. (Composes DH-10 promotion gate and AFS-13
  anti-collapse.)

- **LG-8 Auditable mutation ledger.** Every between-iteration mutation is recorded in an
  append-only ledger: what artifact changed, why, and — where the loop is an evolution
  loop — a machine-checkable prediction the next iteration scores. Prior states are
  never overwritten. Drift from a declared baseline is therefore detectable after the
  fact. (Reuses HE-7 append-only history and DH-8 falsification ledger.)

- **LG-9 Cheapest trustworthy oracle preferred.** Where a deterministic,
  machine-checkable validation exists for the done-condition, it is preferred over a
  judge-model or human oracle for the *inner* termination check; model and human oracles
  are reserved for conditions that genuinely cannot be checked mechanically. This is an
  advisory preference (it lowers cost and raises trust), not a hard gate — some
  done-conditions are irreducibly judgmental.

- **LG-10 Objective persistence across in-session reduction.** [ADDED v0.2.0] LG-5
  externalizes loop state and opens a *fresh context* each iteration. A continuous-session
  loop that compacts **in place** — one long session whose transcript is summarized or
  evicted mid-run rather than discarded at an iteration boundary — needs the complementary
  guarantee: the **standing objective and its progress-state MUST be re-projected into
  every turn from the durable slot**, so mid-session compaction or eviction can never drop
  the north-star. The objective is a *per-turn projection*, not a transcript element a
  reduction may lose. Two obligations follow: **(a) compaction-immune presence** — the
  objective block is re-materialized each turn (a protected region per `l1-context-compression`
  CC-9, and re-projected if evicted), never left to survive only as un-reduced history; and
  **(b) idempotent, resumable objectives** — the objective is phrased, and its progress
  tracked, so a turn that re-reads it *after* the supporting detail was compacted away
  **resumes** correctly rather than restarting or redoing completed work. After any
  reduction the authoritative progress source is the durable ledger, not the compacted
  transcript (`l1-development-workflow` DW-5), and durable progress is captured *before* the
  lossy reduction runs (`l1-context-compression` CC-10). This is the persistent-session
  counterpart to LG-5's fresh-context-per-iteration: it makes an indefinitely-running
  standing-goal loop safe under continuous compaction.

- **LG-11 Scope-restart authority is the scope's own boundary.** [ADDED v1.1.0] A loop
  whose *body is a whole scope re-run from the top* — the entire workflow/run restarted
  from its first stage, re-reading its own inputs and configuration, rather than a step
  re-attempted with carried state — is a legitimate loop class (self-correction: re-run
  everything after the final stage learned something; self-reconfiguration: reload changed
  config/definition and start over). It is governed like any loop — a declared class (LG-1),
  a hard restart ceiling **counted and visible** (LG-6, the restart count is the bound, not
  an unbounded self-restart), externalized state so each restart **reconstructs** from durable
  artifacts rather than inheriting the prior run's context (LG-5), and an append-only restart
  ledger (LG-8). The addition LG governs here is **who may request it**: the authority to
  restart a scope belongs **only to that scope's own boundary** (its run-level open/close),
  **never to a nested context inside it** — a per-item, per-branch, or child stage MUST NOT
  restart the enclosing whole-scope run. The reason is not policy but coherence: a restart
  requested from inside one item's context is undefined about *which* context resumes and
  *what* of the in-flight siblings survives, so the request is refused at the nested site and
  legal only at the boundary that owns the whole scope. A restart re-reads inputs/config
  (composing `l2-config-hotreload`) so it is the safe alternative to mutating a running loop's
  own configuration mid-iteration, which LG-3 forbids. A restart restores neither state nor the prior run's
  **already-committed effects**: re-running effectful steps from the top would re-commit them (send the email twice,
  charge again), so a scope-restart loop is safe only where its effectful steps are **idempotent or compensated** —
  the prior run's committed effects are harmless to repeat or reconciled through effect-compensation before the
  restart, never silently double-committed. A restart is a re-run, not an undo; undo is a separate concern.

- **LG-12 Termination on a best-so-far objective, not only on a count.** [ADDED v1.3.0] LG-6's hard ceiling bounds a loop that never converges; it says nothing about one that has **stopped improving**, and a repair loop can burn an entire ceiling making changes that neither fix nor worsen the objective. A loop whose iterations act against a **measurable objective** (an error count, a defect list, a score) therefore also declares a **stagnation bound**: it records the **best value reached so far**, and terminates when a declared number of consecutive iterations fail to reach a **new** best — not when they fail to improve on the *previous* iteration, which a loop oscillating between two states satisfies forever. Three rules bind it. The objective is the **same one the oracle owns** (LG-4), so the loop cannot terminate itself on a metric it chose; the two bounds are **independent and both live** (the count ceiling still applies, and whichever fires first ends the loop); and exhaustion by stagnation is a **reportable outcome, not a failure to conceal** — the loop reports the best state reached and **the specific unresolved items truthfully**, since a repair loop that stops with three known defects and says so is a better result than one that spends its whole ceiling and says nothing.

- **LG-13 Review backpressure.** [ADDED v1.4.0] A loop whose output is a change awaiting
  human review **does not start producing the next one while its previous output is still
  waiting**. The check runs at the start of every scheduled run, before any work, and a
  pending review stops the run unchanged. A stalled review therefore halts production instead
  of letting unreviewed changes queue behind it — the queue a reviewer trusts is the scarce
  resource, and a loop that outruns it converts *skim and approve* into *ignore*, at which point
  every later output costs more than it delivers. (Composes `l1-review-checkpoint` RC-10: the
  pending review is a visible, owned state; this makes it a gate on the loop that produced it.)

- **LG-14 Steering and memory are separate channels with different authority.** [ADDED v1.4.0]
  A long-running loop is steered through a durable file it reads at the start of each run, and
  it also records what it learned there. The two are kept apart. The **steering** part — what
  to work on now, what is off limits — is human-owned and outside the loop's mutation manifest
  (LG-2, extending LG-3 from the criteria to the focus): a loop that rewrites its own
  instructions and then obeys them is a loop with no human in it; a change it wants is proposed
  in its output for a person to make. The **memory** part, which runs append to, is trusted
  asymmetrically: an entry is **binding as an exclusion** — skip this target, this approach
  failed, this finding is a false positive — because obeying a wrong exclusion costs one wasted
  run; but any **factual claim** in it that a run is about to rely on — a count, *this failure
  is pre-existing*, *this area is clean* — is re-verified first and corrected when stale,
  because once written a run's unreviewed conclusion is indistinguishable from a person's entry
  (`l1-operational-ledger` OL-3 places agent memory last). Entries are dated, and the memory has
  a size budget enforced by consolidating findings in the same reviewable change — never by
  trimming the steering part or any instruction to make room.

- **LG-15 Unattended conduct is the only conduct.** [ADDED v1.4.0] A loop that may run with
  nobody watching behaves the same way whether or not a person happens to be present, because
  what was tried by hand must be what runs on the schedule. Hence: it **never asks** — a target
  that needs a human decision is abandoned for the next one; it **never reads silence as
  approval** — a fact it cannot look up is a reason to abandon, not to guess; it **leaves the
  person's workspace as a precondition, not a mess to clean** — uncommitted work in its way
  stops the run unchanged, it is never stashed, reset or cleaned, and a checkout moved for the
  run is restored afterwards; each attempt starts from the same clean base and a failed attempt
  is **reverted completely** before the next, since a half-reverted attempt yields a change
  nobody can explain; attempts per run are bounded (LG-6); and a run in which every attempt
  failed produces **no outward artifact** — no change request, no issue, no comment — and
  records why in its own run record (LG-8), because an unlucky hour must not become a
  notification storm.

## 4. Detailed Design

### 4.1 The two loop classes

| | Execution loop | Evolution loop |
| --- | --- | --- |
| Iterates to | finish a *fixed* task | *improve* the agent/harness |
| Re-runs | the same task spec each attempt | a changed candidate each generation |
| Deliberately changes between iterations | nothing about the task (only attempt-local artifacts: logs, scratch, partial work) | declared working artifacts (plan, knowledge, harness components) |
| Stays fixed | task spec **and** success criteria | the **evaluation pipeline / oracle** (HE-3) |
| Typical oracle | deterministic (tests/exit-code) or independent judge | the frozen evaluation pipeline over a task set |
| Typical termination | done-signal from oracle, or ceiling | target score reached, patience exhausted, or budget (HE §4.5) |
| Canonical realization | [l2-loop-runner.md](l2-loop-runner.md) execution runner over the task graph | [l1-harness-engineering.md](l1-harness-engineering.md) |

The classes **compose** but never collapse: an evolution loop may run an inner
execution loop each generation (evaluate the candidate by *executing* it on the task
set). The outer loop changes the harness; the inner loop changes nothing about its task.
Each level declares its own class (LG-1) and its own manifest (LG-2). The autonomous
heartbeat (AFS §5.7) is an evolution loop at the *generation* grain wrapping an
execution loop at the *turn* grain.

### 4.2 The mutation-rights ladder

Five tiers, ordered by what an iteration is permitted to mutate. The ladder makes the
"right to mutation" explicit and graduated; the criteria-immutability spine (LG-3) holds
at every tier below self-evolving, and even there the criteria move only via a *higher*
loop with its own oracle.

| Tier | Name | Mutable set (between iterations) | Immutable | Dominant risk |
| --- | --- | --- | --- | --- |
| 0 | **Same-prompt** | nothing | prompt, tools, plan, criteria | context bloat / no learning; oracle is often the model itself (LG-4 reduced-confidence) |
| 1 | **Externally-verified** | attempt-local artifacts (logs, scratch, status) | prompt, plan, criteria | weak oracle lets a wrong result pass |
| 2 | **Artifact-evolving** | plan, accumulated knowledge, validation *inputs* | prompt, criteria, oracle | criteria-adjacent drift if validation and criteria are conflated |
| 3 | **Artifact-evolving + separated verifier** | plan, knowledge — prompt **assembled programmatically**, criteria **immutable**, unauthorized changes rolled back | criteria, oracle, prompt-assembly rule | summarization loss; narrow oracle |
| 4 | **Self-evolving** | prompt, tools, and (only via a higher loop, §4.6) criteria | — within a single loop, criteria are still immutable (LG-3) | over-fitting to the benchmark; loss of safety; criteria drift if the higher loop lacks a separated oracle |

Tier 3 is the recommended default for autonomous work that must improve: it captures
learning (plan/knowledge evolve) while structurally preventing criteria drift (criteria
immutable, prompt assembled rather than hand-edited, unauthorized writes reverted via
the version-control rollback of VC-4). Tier 4 is reachable only through the
LG-7 escalation gate.

### 4.3 Oracle ownership & termination authority

The oracle is the holder of the *right to declare success*. It is distinct from the
*ceiling* (the right to declare exhaustion, LG-6) and from the *actor* (which performs
the work). Three oracle kinds, in descending order of trust-per-cost where a choice
exists (LG-9):

```text
[REFERENCE]
Oracle kinds:
  deterministic  — tests pass / exit code 0 / schema+criteria validator succeeds.
                   Highest trust, lowest cost. Preferred when it exists (LG-9).
  independent    — a judge model or reviewer of a DIFFERENT lineage than the producer,
                   used when the done-condition is not mechanically checkable.
  human          — a person approves "done"; used for irreversible or low-confidence
                   terminations (composes the human-gate concept).

Separation rule (LG-4):
  actor.lineage != oracle.lineage   → full-confidence termination
  actor.lineage == oracle.lineage   → permitted, but recorded as
                                      reduced_confidence = true in the run record.
```

A loop that has no oracle other than the actor's own claim is a Tier-0 same-prompt loop
and is always recorded reduced-confidence. The point of LG-4 is not to forbid
self-assessment but to make its weakness *visible and auditable* rather than implicit.

### 4.4 State externalization & fresh-context iteration

LG-5 separates *what carries forward* from *how it carries forward*. What carries: a
small set of durable artifacts — minimally a **plan** (what to do / what is left) and a
**status** record (what was done, what failed, what was tried). How: a new iteration
opens a fresh context and *reconstructs* from those artifacts; it does not inherit the
full transcript of every prior turn. This is the mechanism that lets a long-running loop
avoid context rot — signal lives in compact artifacts, not in an ever-growing history.

The evolution-loop variant of this is HE-8/HE-9 (the ANALYZE artifact loaded into a
fresh context). The execution-loop variant is the plan/status pair reconstructed each
attempt. Both are the same invariant at different grains.

**Two realizations of externalized state (LG-5 vs LG-10).** There are two ways an
externalized objective survives context reduction, and a loop uses whichever its runtime
shape dictates:

| Realization | When | Mechanism |
| --- | --- | --- |
| **Fresh-context per iteration** (LG-5) | discrete iteration boundaries (harness generations, task retries) | discard the transcript; reconstruct working context from plan+status artifacts |
| **Continuous-session re-projection** (LG-10) | one long session that compacts in place (a standing-goal heartbeat, an indefinitely-running chat) | keep the compacting session; re-project the standing objective + progress into *every turn* from the durable slot, so compaction/eviction never drops it |

They are the same principle — signal lives in durable artifacts, never only in the
mutable transcript — applied to opposite runtime shapes. LG-10's continuous-session case
is what makes "compaction cannot hide the goal" a contract rather than a hope: the
objective is re-materialized every turn (CC-9 protected + re-projected), progress is
captured before any lossy reduction (CC-10), and the durable ledger outranks the
compacted transcript after the fact (DW-5).

### 4.5 Mutation manifest & ledger

The **manifest** (LG-2) is a declaration over a closed artifact taxonomy:

```text
[REFERENCE]
artifact kinds (closed taxonomy):
  scratch          — attempt-local working files, never carried forward
  plan             — the task decomposition / what-is-left
  knowledge        — accumulated facts, memory entries, exemplars
  validation_input — the data a validator runs against (NOT the validator's pass rule)
  prompt           — the actor's system/instruction text
  tools            — the actor's available capabilities
  criteria         — the success definition / oracle rule   ← NEVER mutable here (LG-3)

MutationManifest {
  loop_class:  execution | evolution
  tier:        0..4
  mutable:     subset of {scratch, plan, knowledge, validation_input, prompt, tools}
  // 'criteria' is structurally excluded — it is not expressible in `mutable`.
}
```

The **ledger** (LG-8) records each applied mutation append-only: `(iteration, artifact
kind, what changed, why)`, plus — for evolution loops — a machine-checkable
`predicted_flip` set the next iteration scores into a `keep | revert | partial` verdict
(this is exactly the DH-8 falsification ledger; an execution loop's ledger omits the
prediction field). Because the ledger is append-only and the manifest is declared,
structural drift from a baseline is computable without re-running anything (DH-9).

### 4.6 Tier-escalation promotion gate

Moving a loop up the ladder — and in particular making criteria mutable, which is only
ever done by a *higher* evolution loop acting on a lower loop's spec — passes the LG-7
gate:

```text
[REFERENCE]
escalate(loop, from_tier, to_tier):
  require separated_oracle(loop)            // actor.lineage != oracle.lineage
  require external_novelty(loop)            // new tasks/inputs/feedback, not self-output
  candidate = apply_higher_loop_change(loop)
  measure on HELD-OUT work (not the search set):
      promote IFF delta_metric >= margin AND regression <= bound
  on promote: record in ledger (LG-8); on fail: revert, record the attempt.
```

This is the same shape as DH-10 promotion and HE-6 transfer validity, lifted to the
governance level: a criteria change or a self-modification is never trusted because it
helped on the cases the loop just saw — only because it holds on work the loop did not
get to influence. AFS-13 supplies the reason the novelty requirement is non-optional:
without external novelty a self-referential loop narrows into self-repetition rather
than improving.

### 4.7 Loop design checklist

A conforming loop answers all six before it runs (the checklist is the practical form of
LG-1…LG-9 and doubles as a review gate):

1. **Class?** Execution or evolution (or a declared composition). *(LG-1)*
2. **Oracle?** Who holds the right to say "done" — deterministic, independent judge, or
   human — and is it separated from the actor? *(LG-4)*
3. **Criteria location?** Where do the success criteria live, and are they outside this
   loop's mutable set? *(LG-3)*
4. **Mutable set?** Which artifact kinds may an iteration change; what carries forward
   and how? *(LG-2, LG-5)*
5. **Ceiling?** What hard limit and no-progress stop end the loop independently of the
   actor? *(LG-6)*
6. **Cheap validation?** Is there a deterministic check for the done-condition, and is it
   used for the inner check where it exists? *(LG-9)*

## 5. Implementation Notes

Recommended order, each step verifiable before the next:

1. **Loop spec + manifest first** (LG-1, LG-2) — the declaration (class, tier, mutable
   set) is the contract everything else enforces; with `criteria` structurally
   inexpressible in the mutable set, LG-3 holds by construction.
2. **Oracle abstraction** (LG-4, LG-9) — deterministic / independent-judge / human,
   with the lineage-comparison that sets `reduced_confidence`.
3. **Ceiling enforcement** (LG-6) — iteration/budget/time + no-progress, owned by the
   runner, not the actor.
4. **State externalization** (LG-5) — the plan/status reconstruct-not-inherit step.
5. **Mutation ledger** (LG-8) — append-only; add the predicted-flip field for evolution
   loops.
6. **Escalation gate** (LG-7) — last, because it depends on a separated oracle, a
   held-out set, and an external-novelty source all being wired.

## 6. Drawbacks & Alternatives

- **Declaration overhead.** Forcing every loop to declare class, tier, oracle, and
  manifest adds ceremony to what is sometimes a three-line retry. Mitigation: a
  same-prompt Tier-0 loop's declaration is nearly empty (mutable set `{}`, oracle =
  self, recorded reduced-confidence); the ceremony scales with the power claimed.
- **Criteria immutability can feel rigid.** Real tasks sometimes reveal that the
  original criteria were wrong. LG-3 does not forbid changing them — it forbids the
  *judged actor* from doing so silently; a criteria change is a deliberate act of a
  higher evolution loop through the LG-7 gate. This is the intended friction.
- **Oracle separation is not always available.** With a single model lineage, every
  oracle is reduced-confidence (DH-11 limit). LG-4 degrades gracefully — it records the
  condition rather than blocking — but the weakness is real and surfaced, not hidden.
- **Alternative — one undifferentiated "agent loop."** Let every loop be the same
  retry-until-done primitive. Rejected: it is exactly how class confusion and criteria
  drift arise, with no vocabulary to catch either.
- **Alternative — forbid self-modification entirely.** Ban Tier 4. Rejected: it
  forecloses a legitimate, valuable capability; the LG-7 gate makes Tier 4 *governed*
  rather than *absent*.

## Ideas-to-adopt mapping

What each studied loop pattern contributes and where it lands. Patterns are named by
structural form, not by product.

| Source pattern | Idea worth adopting | Where it lands |
| --- | --- | --- |
| Same-prompt re-run | Simplest possible loop; the model self-declares done. | Tier 0 of the ladder (§4.2); always recorded reduced-confidence (LG-4). |
| Externally-verified re-run | Delegate "done" to an external verifier; compress history between attempts. | Tier 1; LG-4 oracle separation + LG-5 externalization. |
| Artifact-evolving (plan/knowledge mutate) | Capture learning in durable artifacts across iterations. | Tier 2; LG-2 manifest + LG-8 ledger. |
| Artifact-evolving + separated verifier + programmatic prompt assembly + rollback | Keep criteria immutable, assemble the prompt rather than hand-edit it, revert unauthorized changes. | Tier 3 (recommended default); LG-3 spine + VC-4 rollback. |
| Self-evolving agent | An agent may rewrite its own prompt/tools/criteria — under guard. | Tier 4 behind the LG-7 escalation gate; criteria still move only via a higher loop with a separated oracle. |
| Outer-loop "loop engineering" discipline | The developer designs the *outer* loop (triggers, isolation, decomposition, ceiling, memory, maker–checker), not just the prompt. | Already owned: orchestration (/goal+judge+budget), execution-workspace (isolation), task-graph (decomposition), agent-autonomy (ceiling/gates), self-improvement §4.7 (maker–checker). This spec adds the loop-class + mutation-rights contract over them. |
| Oracle-as-explicit-question | Name who has the right to declare "done." | **New** as LG-4 oracle ownership; generalizes DH-11 from verification to termination authority. |

## nodus-relevance mapping

How the portable workflow substrate realizes this governance (the contract surface
already largely exists; the gaps are small):

| Governance element | nodus seam | Note |
| --- | --- | --- |
| Loop class + manifest (LG-1, LG-2) | workflow declaration (`@steps`, control flow `~UNTIL MAX:n`) + a manifest field | The mutable set is a declared property of the workflow spec, schema-validated like any other section. |
| Criteria immutability (LG-3) | `@out`/`@err` contract + validators (`^validator`) | The validator/criteria live in the schema-frozen surface, structurally outside the actor's editable steps. |
| Oracle (LG-4, LG-9) | output-contract validators (deterministic) + `ModelProvider` of a distinct binding (independent judge) + HITL dialog (human) | Lineage comparison sets `reduced_confidence`; reuses the existing provider abstraction. |
| Ceiling (LG-6) | `~UNTIL MAX:n` + step budget + `NODUS:MAX_REACHED` error code | Already present; the runner owns it, not the actor. |
| Mutation ledger (LG-8) | `AuditProvider` event stream keyed by `run_id` + `step_index` | The content-free, step-indexed event taxonomy is the natural append-only ledger substrate (as DH-7/DH-8 already use). |
| Escalation gate (LG-7) | `PolicyProvider` (gate) + `StorageProvider` (provisional-vs-promoted state) | The same pending-LP-3 traits DH-10 graduates; the loop runner is another consumer that exercises the two-host rule. |
| Objective persistence (LG-10) | `@ctx:` section + EG-11 immutable invocation context | **Satisfied by construction**: a nodus run has no in-place transcript compaction and its `@ctx`/invocation context is present to every step unchanged, so the objective can never be "compacted away." LG-10 is a guarantee nodus already provides structurally — no nodus-side invariant needed, recorded here so the mapping is explicit. |

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[HARNESS-ENG]` | `.design/main/specifications/l1-harness-engineering.md` | The evolution-loop realization; HE-3 oracle, HE-7 ledger, HE-9 fresh-context that this spec generalizes. |
| `[DYN-HARNESS]` | `.design/main/specifications/l1-dynamic-harness.md` | DH-8 falsification ledger, DH-10 promotion gate, DH-11 verification independence reused by LG-4/LG-7/LG-8. |
| `[AFS]` | `.design/main/specifications/l1-agent-framework-skeleton.md` | AFS-5/AFS-6/AFS-13 and §5.7 heartbeat — the loops this spec classifies. |
| `[TASK-GRAPH]` | `.design/main/specifications/l1-task-graph-model.md` | TG-5/TG-13/TG-14 work source and guarded autonomous delivery for the execution loop. |
| `[VC]` | `.design/main/specifications/l1-version-control.md` | VC-4 virtual-staging rollback enforcing the immutable set. |
| `[CTX-COMPRESS]` | `.design/main/specifications/l1-context-compression.md` | CC-9 protected regions + CC-10 memory-safe reduction that LG-10 composes. |
| `[DEV-WF]` | `.design/main/specifications/l1-development-workflow.md` | DW-5 ledger-authoritative-after-compaction that LG-10 generalizes. |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.4.0 | 2026-10-22 | Core Team | Added LG-13…LG-15 from an external project's scheduled, unattended maintenance loop that opens one small change per run. LG-13 review backpressure: a pending review of the previous output stops the next run before any work. LG-14 steering vs memory: the focus a person sets is outside the mutation manifest (LG-3 extended from criteria to focus) and changes to it are proposed, never made; run-written memory is binding as an exclusion but its factual claims are re-verified before use, entries dated, budget kept by consolidation and never by trimming instructions. LG-15 unattended conduct: same behaviour attended or not, never ask, never read silence as approval, never clean the person's workspace, full revert between attempts, bounded attempts, and total failure leaves no outward artifact while the run record keeps the reasons. The studied loop discards the notes of a run that landed nothing; that is not taken (LG-8 keeps them). |
| 1.3.0 | 2026-09-04 | Core Team | Added LG-12 — termination on a best-so-far objective, not only on a count. LG-6's hard ceiling bounds a loop that never converges and says nothing about one that has stopped improving, so a repair loop can spend its entire ceiling on changes that neither fix nor worsen the objective. A loop acting against a measurable objective now also declares a **stagnation bound**: it records the best value reached and terminates when a declared number of consecutive iterations fail to reach a **new** best — deliberately not "fail to improve on the previous iteration", which a loop oscillating between two states satisfies forever. The objective is the one the oracle owns (LG-4), so a loop cannot terminate itself on a metric it chose; both bounds stay live and whichever fires first ends the loop; and stagnation exhaustion is a reportable outcome that names the unresolved items truthfully — stopping with three known defects and saying so beats spending the ceiling and saying nothing. From an external artifact-generation tool whose repair loop is bounded exactly this way. |
| 1.2.0 | 2026-08-25 | Core Team | Related Specifications extended with `l1-acceptance-oracle` — the integrity complement to LG-4 oracle ownership. LG closed *criteria drift* (the actor rewriting the contract that judges it) and mandated an oracle outside the mutable set; it assumed that oracle meaningful. AO covers the independent failure where a properly owned, correctly frozen oracle is one that cannot fail — under which an execution loop terminates on its first iteration forever and an evolution loop optimizes against a constant. Link-only; no invariant changed. |
| 1.0.0 | 2026-07-17 | Core Team | Promoted RFC→Stable via `/magic.spec` adversarial review (spec-critic + prompt-engineer PASS) — the governance-claim verification the 0.1.0/0.2.0 history named as the gate to Stable. No content change: LG-1…LG-10, the two loop classes with composition, the five-tier mutation-rights ladder with the criteria-immutability spine (LG-3), the separated-oracle contract, the independent ceiling, the escalation gate, and the objective-persistence guarantee were reviewed as-is and found technology-neutral, complete, and internally coherent. The keystone is now buildable via its L2 realization `l2-loop-runner` (promoted Draft→Stable in the same pass, LG-10 compliance row added, crate placement reconciled to the post-decomposition topology). |
| 1.1.0 | 2026-07-26 | Core Team | Added LG-11 (scope-restart authority is the scope's own boundary) — a loop whose *body is a whole scope re-run from the top* (the entire workflow restarted from its first stage, re-reading its own inputs and configuration, rather than a step re-attempted with carried state) is a legitimate class for self-correction and self-reconfiguration, governed like any loop: declared class (LG-1), a hard **counted, visible** restart ceiling so it is bounded not unbounded self-restart (LG-6), externalized state so each restart reconstructs from durable artifacts rather than inheriting the prior run context (LG-5), and an append-only restart ledger (LG-8). The addition LG governs is **who may request it**: the authority to restart a scope belongs only to that scope's own boundary (its run-level open/close), never to a nested context inside it — a per-item/per-branch/child stage MUST NOT restart the enclosing whole-scope run, because a restart requested from inside one item's context is undefined about which context resumes and what of the in-flight siblings survives, so it is refused at the nested site and legal only at the boundary that owns the scope. A restart re-reads inputs/config (composing l2-config-hotreload), the safe alternative to the mid-iteration self-config-mutation LG-3 forbids. Nodus realization = NL-23. |
| 0.2.0 | 2026-07-02 | Core Team | Added LG-10 (objective persistence across in-session reduction) + §4.4 two-realizations table + nodus-relevance row: LG-5 opens a fresh context per iteration; LG-10 is the persistent-session counterpart — a continuous-session loop that compacts in place MUST re-project the standing objective + progress into every turn from the durable slot so mid-session compaction/eviction can never drop the north-star (compaction-immune presence via CC-9 protected + re-projected-if-evicted), and the objective must be idempotent/resumable so a post-reduction turn resumes rather than restarts/redoes (durable progress captured before the lossy reduction, CC-10; the ledger outranks the compacted transcript after, DW-5). Satisfied by construction in nodus (no in-place compaction; `@ctx`/EG-11 immutable invocation context always present) — no nodus-side invariant. Stays RFC (additive; Stable gate unchanged). |
| 0.1.0 | 2026-06-25 | Core Team | Initial RFC — LG-1…LG-9; two loop classes (execution/evolution) with composition rule; five-tier mutation-rights ladder with criteria-immutability spine; oracle-ownership contract (deterministic/independent/human, lineage-separation reduced-confidence) generalizing DH-11 to termination authority; state externalization; mutation manifest over a closed artifact taxonomy + append-only ledger; tier-escalation promotion gate; six-question loop design checklist; ideas-to-adopt + nodus-relevance mappings. Adversarial verification of the governance claims is the gate to Stable (mirrors the dynamic-harness sibling). |
