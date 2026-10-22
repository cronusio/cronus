# Output Contracts

**Version:** 1.1.1
**Status:** Stable
**Layer:** concept

## Overview

The model by which a task or node declares a **contract** for its expected output — a typed schema combined with one or more inline **validators** — and by which the executor enforces that contract automatically, retrying the producing node if validation fails. Output contracts sit between the node that produces a result and the node (or channel) that consumes it; they fire during execution, not as a post-task quality gate. This allows structural and semantic constraints to be declared once, close to the work unit, and enforced consistently without burdening downstream nodes.

This spec governs inline output validation only. Post-task toolchain quality gates (linters, test suites, type-checkers) are defined in the quality standards spec. Human-reviewed approval gates are defined in the orchestration spec.

## Related Specifications

- [l1-quality-standards.md](l1-quality-standards.md) — Post-task quality gates (toolchain checks, definition of done). Output contracts are an upstream, inline complement.
- [l1-orchestration.md](l1-orchestration.md) — ORC-11 (error containment): validation failures from output contracts are classified as retryable failures at the delegation boundary.
- [l1-execution-graph.md](l1-execution-graph.md) — Validation fires at the node's output phase; on failure, the node is re-entered within the same graph run.
- [l1-kanban-model.md](l1-kanban-model.md) — A card with an output contract: the contract is recorded on the card and its status updated when validation passes.
- [l2-quality-pipeline.md](l2-quality-pipeline.md) — Concrete quality toolchain; output contracts complement it with schema and semantic checks that run before toolchain gates.
- [l1-input-binding.md](l1-input-binding.md) — [ADDED v1.0.1] the **twin on the consumer side** of the same work unit: OC governs the value a unit *produces* (schema + validators + retry-with-verdict); input binding governs the values it *consumes* (declared binders + typed located rejections + bind-before-invoke). The machinery is deliberately opposite — the producer side retries with feedback, the consumer side rejects without retry, since re-binding an unchanged invocation yields the same rejection.

## 1. Motivation

Tasks produce outputs consumed by other tasks. Without a declared contract, the consuming side discovers malformed output at use time — often after significant work has already built on the broken result. Two failure modes are common: (a) structural: the output is missing required fields or has wrong types; (b) semantic: the output is structurally valid but violates a declared invariant (e.g., "summary must be under 500 words", "plan must contain at least one action item").

Post-task gates (linters, tests) catch these failures late, after the artifact is committed. Inline validation catches them immediately, while the producing node is still in context and can retry with the validation feedback. This is analogous to a type error caught at compile time versus a panic at runtime.

## 2. Constraints & Assumptions

- A contract is declared on the producing node/task, not on the consuming channel.
- Contract validation occurs after the node's output is staged, before it is committed to channels and before the next node begins.
- A validation failure does not immediately abort the graph run; it triggers a retry of the producing node within the declared retry budget.
- After the retry budget is exhausted, the failure escalates as a retryable-or-fatal error to the delegation boundary (per ORC-11).
- Validators are side-effect-free: they observe the output and return a verdict; they do not modify channels or external state.
- Contracts are declarative: they are part of the node/task definition, not executable code embedded in the output itself.

## 3. Core Invariants

Rules every Layer 2 implementation MUST NOT violate:

- **OCT-1 (Schema contract):** a node may declare a typed output schema. The executor MUST validate that the node's output conforms to the schema before committing it to channels. Schema mismatch is a validation failure and triggers a retry.
- **OCT-2 (Callable validator):** a node may declare one or more callable validators. Each callable receives the output and returns a verdict: `(valid: bool, reason: string | None)`. `valid=false` is a validation failure; `reason` is injected into the node's retry context so the producing agent can correct the specific issue.
- **OCT-3 (Criteria validator):** a node may declare one or more criteria expressed in natural language. A criteria validator is evaluated against the output using the system's reasoning capability (not a hard-coded rule). `valid=false` with a generated reason triggers a retry identical to a callable validator failure.
- **OCT-4 (Retry budget):** each node declares a maximum number of validation retries. On each retry, the executor re-invokes the node with the original input plus all accumulated validation verdicts as additional context. The retry budget is consumed independently of the step budget (EG-8); budget exhaustion escalates the failure.
- **OCT-5 (Verdict injection):** on retry, the executor MUST supply the accumulated validation verdicts to the node's context. Verdicts include: which validator failed, the reason string, and the attempt number. This ensures the node can progressively correct its output rather than repeating the same mistake.
- **OCT-6 (Escalation on budget exhaustion):** when the retry budget is exhausted without a passing verdict, the failure is classified per ORC-11: retryable (if the cause is transient), fatal-isolated (if the node repeatedly produces invalid output), or escalation (if the criteria cannot be mechanically verified). The executor MUST NOT silently commit a contract-failing output.

- **OCT-8 (Exhaustion resolves to a named disposition, never to a loop):** [ADDED v1.1.0] an escalation raised under OCT-6 MUST terminate in **exactly one disposition drawn from a closed set** — *retry with a different producer*, *decompose the unit into smaller units*, *revise the approach or the contract itself*, *accept the current output with its limitations recorded*, or *defer the unit*. A sixth option — "try again the same way" — does not exist at this point, because the budget it would consume has already been spent proving it does not work. The escalation carries the material the choice requires: the **full attempt history** (what each attempt produced and which verdict it failed on), the **root-cause reading** of why the attempts converged on failure, and the **blast radius** (what downstream work is blocked). *Accept with limitations* is a legitimate outcome and is the one that most needs recording: the limitation travels with the artifact, so a knowingly-accepted gap is never later mistaken for a passing result. An escalation that ends without a recorded disposition is an unresolved failure wearing the appearance of a handled one.
- **OCT-7 (Non-blocking absence):** if a node declares no output contract, the executor proceeds without validation. Contracts are always opt-in; their absence never blocks execution.

## 4. Detailed Design

### 4.1 Contract Declaration

```plaintext
[REFERENCE]

OutputContract:
  schema      : TypeSchema | None        -- structural shape (OCT-1)
  validators  : [Validator]              -- ordered list (OCT-2 / OCT-3)
  max_retries : int                      -- default 3; 0 disables retry (OCT-4)

Validator =
  | CallableValidator { fn: (output) → (bool, string | None) }
  | CriteriaValidator { criteria: string }   -- natural-language criterion
```

Multiple validators are evaluated in declared order. The executor stops at the first failure and triggers a retry; it does not aggregate all failures before retrying (fail-fast per retry attempt).

### 4.2 Validation Lifecycle

```mermaid
graph TD
    A[Node produces output] --> B{Schema valid?}
    B -->|no| F[Validation failure]
    B -->|yes| C[Run validator 1]
    C -->|fail| F
    C -->|pass| D[Run validator 2 … N]
    D -->|all pass| E[Commit to channels → proceed]
    D -->|any fail| F
    F --> G{Retry budget remaining?}
    G -->|yes| H[Inject verdicts into context → re-invoke node]
    H --> A
    G -->|no| I[Escalate per ORC-11]
```

### 4.3 Verdict Injection Format

On each retry, the executor prepends accumulated verdicts to the node's context:

```plaintext
[REFERENCE]
ValidationContext:
  attempt       : int             -- current attempt number (1-based)
  verdicts      : [Verdict]       -- one per failed validator, all attempts
  Verdict:
    attempt     : int
    validator   : string          -- name or index of the validator that failed
    reason      : string | None   -- human-readable failure explanation
```

Implementations MUST pass `ValidationContext` to the node so the node's reasoning is informed by exactly what was wrong with previous attempts.

### 4.4 Validator Types Compared

| Type | Evaluation | Strengths | Limitations |
| --- | --- | --- | --- |
| `Schema` | Static type check | Deterministic, zero LLM cost | Structural only; cannot catch semantic errors |
| `Callable` | Deterministic function | Fast, cheap, precise | Requires the constraint to be mechanically expressible |
| `Criteria` | Reasoning evaluation | Handles semantic, nuanced, natural-language constraints | Non-deterministic; higher cost; retry if verdict unclear |

### 4.5 Failure Escalation

When the retry budget is exhausted, the executor classifies the failure (OCT-6) and surfaces it to the delegation boundary. The canonical ORC-11 classification applies:

- **Retryable**: validator raised an infrastructure error (e.g., LLM timeout during criteria evaluation) — the delegation boundary may schedule a fresh attempt.
- **Fatal-isolated**: the node consistently produces invalid output — the card moves to Blocked; other delegations continue.
- **Escalation**: the criteria are ambiguous or contradictory and cannot be mechanically resolved — the human-in-the-loop gate fires.

### 4.6 Interaction with Quality Gates

Output contracts and post-task quality gates are complementary, not redundant:

| Dimension | Output Contract | Quality Gate |
| --- | --- | --- |
| **Timing** | During execution (inline) | After task completion |
| **Subject** | The node's immediate output | The accumulated codebase / artifact |
| **Retry scope** | Re-invoke the producing node | Block card from advancing |
| **Focus** | Schema + semantic correctness of one result | Cross-cutting concerns (tests, lint, security) |
| **Cost** | Paid per failing attempt | Paid once per completion |

A task should use an output contract when its correctness can be evaluated from the output alone, without running a test suite or inspecting other files.

## 5. Implementation Notes

1. Evaluate schema first (cheapest check) before callable validators, and callable validators before criteria validators (most expensive).
2. Criteria validators that call an LLM should have their own timeout and a fail-open policy if the evaluator is unavailable — default to `valid=true` with a warning rather than blocking execution.
3. Store retry history with each graph run checkpoint (EG-6) so that on resumption after a crash the retry count is not reset.
4. The `reason` string from a failing callable validator is the primary correction signal; implementations should ensure it is concise and actionable (max ~200 characters).

## 6. Drawbacks & Alternatives

- **Alternative — post-task gate only:** simpler, no retry complexity. Rejected because the cost of re-running an agent from scratch after a gate failure is much higher than an inline retry with targeted feedback; and because post-task gates examine the whole artifact, not just one task's output.
- **Alternative — explicit correction step:** instead of retry-with-feedback, a dedicated "corrector" node refines the output. Valid design but requires an extra node per contract; inline retry is lower overhead for common structural/semantic mismatches.
- **Criteria validator cost:** LLM-evaluated criteria add latency and cost per retry. Implementations should cache criteria verdicts keyed by output hash across runs to avoid redundant evaluation.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[QS]` | `.design/main/specifications/l1-quality-standards.md` | Post-task quality gates — the downstream complement to output contracts |
| `[ORC]` | `.design/main/specifications/l1-orchestration.md` | ORC-11: how validation failures escalate at delegation boundaries |
| `[EG]` | `.design/main/specifications/l1-execution-graph.md` | Execution graph: where validation fires in the node output phase |

## Document History

| Version | Date | Summary |
| --- | --- | --- |
| 1.1.1 | 2026-10-22 | Requirement-ID prefix renamed `OC` → `OCT` so that it names one specification in the workspace (the person's decision, design-layer audit 2026-10-22; RULES §1 as amended in 1.8.0): every definition of this spec's invariants and every citation of them across the corpus now reads `OCT-n`. No requirement was added, removed or reworded. |
| 1.1.0 | 2026-08-06 | OCT-8 added — **exhaustion resolves to a named disposition, never to a loop**. OCT-6 classified the failure but left open what happens next, and the observed default in that gap is an unbounded re-attempt cycle or a quiet abandonment that reads downstream as success. The escalation now terminates in exactly one of a **closed** set — different producer / decompose / revise approach or contract / accept-with-recorded-limitations / defer — with "try again the same way" structurally absent, since that budget was already spent proving it fails. The escalation carries the full attempt history, the root-cause reading, and the blast radius, because a disposition chosen without them is a guess. *Accept with limitations* is explicitly legitimate **and** explicitly recorded, so a knowingly-accepted gap never later passes for a clean result. Additive to OCT-6; no existing classification changed. |
| 1.0.1 | 2026-08-05 | Related Specifications extended with `l1-input-binding`, the consumer-side twin closing the input/output asymmetry: same unit, opposite machinery (producer retries with verdicts, consumer rejects without retry). Link-only; no invariant changed. |
| 1.0.0 | 2026-06-24 | Initial stable spec — schema + callable + criteria validators, retry budget, verdict injection, escalation model, interaction with quality gates |
