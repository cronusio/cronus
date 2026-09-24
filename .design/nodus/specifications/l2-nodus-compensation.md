# Nodus Compensation Seam Implementation (Rust)

**Version:** 1.0.2
**Status:** Stable
**Layer:** implementation
**Implements:** l1-nodus-language.md

## Overview

Rust realization of the NL-22 compensation seam: an effectful step MAY declare a
**compensating action** — a host-supplied *forward* action that undoes the step's
business effect — and when the compensation scope fails or is aborted, the
compensations of the scope's **successfully-completed** steps run in **reverse
order of completion** (LIFO). This spec records how the
lexer, AST, parser, executor, validator, and transpiler realize that contract, and
which parts of NL-22's stated composition are **vacuous in core today** and
therefore host obligations rather than silent omissions.

The structural precedent already exists: `Step.retry: Option<u32>` (from
`~RETRY:n`) is an optional per-step modifier parsed as a `~`-keyword clause, and
the compensation declaration is its direct analog. Nothing here introduces a new
`Value` kind (NL-7) or a new `ExecutionEvent` variant (HO-6).

## Related Specifications

- [l1-nodus-language.md](l1-nodus-language.md) — the language contract: NL-22 (the invariant realized here), NL-2 (`!!` bypass), NL-7 (closed value system), NL-12 (the suspend/resume machinery NL-22 names for crash-resume — not yet realized)
- [../../main/specifications/l1-compensation.md](../../main/specifications/l1-compensation.md) — the host-side contract NL-22 realizes: CO-1 semantic-undo-not-rollback, CO-3 completed-only, CO-4 reverse-order, CO-6 armed-not-automatic, CO-7 fallible-surfaced, CO-8 idempotent-resumable
- [l2-nodus-control-flow.md](l2-nodus-control-flow.md) — established the `~`-keyword step-clause pattern (`~RETRY:n`) this spec reuses, and the `Signal`/`Status::Failed` machinery that arms compensation
- [l2-nodus-errors.md](l2-nodus-errors.md) — owns the error taxonomy this spec extends with `NODUS:COMPENSATION_FAILED`
- [l2-nodus-runtime.md](l2-nodus-runtime.md) — the runtime crate extended here (`lexer`, `ast`, `parser`, `executor`, `validator`, `transpiler`); its §3 records NL-22 as realized by this spec
- [l2-nodus-restart.md](l2-nodus-restart.md) — NL-23; a restart re-runs and does not undo, so a restart over non-idempotent, non-compensated effects double-commits. Soft reference: neither spec requires the other to be implemented

## 1. Motivation

The crate has no compensation machinery of any kind — no way to declare an undo
for an effectful step, no record of which effects completed, and no unwind path
when a run fails partway through a sequence of committed effects. A workflow that
publishes, then charges, then fails has no expressible way to un-publish and
un-charge; the run simply reports `Status::Failed` and leaves both effects live
with nothing in the record distinguishing "was undone" from "was never
undoable". This spec closes that, and makes the un-compensable case *explicit in
the record* rather than an absence to be misread as success.

## 2. Constraints & Assumptions

- No new external dependency (LP-1). The ledger is a `Vec` on the existing execution context; no transaction manager, no persistence crate.
- **Semantic undo, not state rollback** (CO-1): nodus does not restore variables, re-seed `$out`, or unwind the value environment. A compensation is an ordinary forward command the host implements.
- **The run is the compensation scope in this pass.** NL-22 speaks of "a workflow region / subprocess"; nodus has no subprocess concept today (`RUN(@macro)` body expansion is unimplemented) and the lexer emits no indent/dedent tokens, so a declared sub-region form has no structural anchor yet. Scope = the run; a `~SCOPE`-style declared region is a documented deferral (§6), not an omission.
- **The LP-11 `decide → effect → observe` gate composition.** NL-22 requires each compensation to pass that gate "like any effect". When this spec was written no such gate existed; it has since landed (`l2-nodus-portability.md`, LP-11 row), and compensations inherit it as predicted, because they route through the one `execute_command` path every effect takes — no second mechanism was retrofitted. What they inherit includes that gate's composition limit: a run with a real policy runs on the stub model (`l2-nodus-runtime.md` §4.5).
- **Crash-mid-compensation resume is a host obligation today.** NL-22's resumability (CO-8) names the NL-12 suspend/resume machinery, which is unimplemented. Nodus drives each compensation **at-least-once within a single process run** against the ledger; the ledger is the artifact a host persists and replays. A host-supplied compensation is expected idempotent, so re-driving does not double-undo.
- Compensation is **armed, never automatic on success** (CO-6): a scope that completes normally keeps its effects. No implicit rollback on a clean run.
- No new `Value` variant (NL-7) and no new `ExecutionEvent` variant (HO-6).

## 3. Invariant Compliance

| L1 Invariant | Rust Enforcement |
| --- | --- |
| NL-22(a) Only completed effects compensate | A step enters the ledger **only** after its action returns without a runtime error. A never-started, still-running, or already-failed step is never in the ledger, so it cannot be compensated. Cancellation of a running step stays the ordinary interrupt path (`Signal`), structurally distinct from the unwind. |
| NL-22(b) Each compensation is a fallible gated effect | A compensation runs through the same `execute_command` path as any command, so it is subject to the same rule checks and emits the same events. A compensation that errors pushes `NODUS:COMPENSATION_FAILED` onto `ctx.errors`, leaving the original effect **live** — never swallowed, never reported as undone. |
| NL-22(c) Idempotent and resumable | The ledger is the driving record: each entry is popped and driven at-least-once as the unwind proceeds. Re-driving a persisted ledger is safe because the host-supplied compensation is expected idempotent. Nodus adds no dedupe of its own — it does not silently skip an entry it cannot prove ran. |
| NL-22(d) Armed, never automatic | The unwind fires only on `Status::Failed` / `Status::Aborted`. A `Status::Ok` / `Status::Partial` run performs no unwind — there is no separate explicit-compensate-request trigger. **Deviation (pending):** L1 (d) composes the failure signal from `!!`/`!HALT` *and `@err:` routing*, but the unwind reads only the final status, and an error routed through `@err:` — like every other non-halting error: a policy denial, a rejected or timed-out dialog, an unaccounted settlement, an exhausted retry — ends the run as `Partial`. So the failures an effectful workflow is most likely to meet leave its completed effects live with no unwind; only a rule violation, `!HALT` or `!BREAK` arms it. Alignment: a run whose step sequence ended on an uncaught error arms the unwind, whether or not a handler ran. |
| NL-22 reverse order (CO-4) | The ledger is append-on-completion and drained **back-to-front**, so later effects are undone before the earlier effects they were built on. Order is a correctness contract, enforced by popping the ledger rather than re-reading step order. |
| NL-2 Hard constraints absolute | A `!!` rule violation still bypasses `@err:` and fails the run; the unwind then runs over whatever completed before the violation. Compensation never rescues a rule violation into success. |
| NL-7 Closed value system | The compensation declaration is a `CommandCall` (existing AST type); its outcome is an ordinary `Value`. No new value kind. |
| HO-6 Closed event taxonomy | Compensation reuses the existing `StepStart`/`StepEnd`/`StepError` emissions of the command path; no new `ExecutionEvent` variant. |
| LP-1 Zero dependency | Ledger is `Vec`; no new crate. |
| LP-2 Host-supplied mechanism | The compensating action, whether it reaches an external system, and how it undoes are entirely the host's. Core names no transaction protocol, external API, or undo mechanism. |

## 4. Detailed Design

### 4.1 Lexer token

```text
[REFERENCE]
// lexer.rs — one new TokenType variant, mirroring TildeRetry/TildeMap
TildeCompensate,   // ~COMPENSATE
```

`~COMPENSATE` must be matched as a keyword **before** the generic `~identifier`
flag rule, exactly as `~MAP`/`~RETRY` are — otherwise it mis-lexes as a `Flag`
(the concrete bug `l2-nodus-control-flow` §1 records for `~MAP`/`~RETRY`).

### 4.2 AST

```text
[REFERENCE]
// ast.rs — Step gains one optional field, mirroring `retry: Option<u32>`
pub struct Step {
    // ... existing fields ...
    pub retry: Option<u32>,
    /// `~COMPENSATE: CMD(args)` — the host-supplied undo for this step's
    /// business effect. `None` means the step is honestly un-compensable.
    pub compensation: Option<CommandCall>,
}
```

### 4.3 Parser

`~COMPENSATE: CMD(args)` is a **trailing same-line clause** on the step:

```text
[REFERENCE]
3. PUBLISH($doc) → $url ~COMPENSATE: NOTIFY($url)
```

Same-line, not an indented sub-clause: the lexer emits no indent/dedent tokens,
so an indentation-anchored form has nothing to parse against (the Phase-13
`§config` lesson). The clause is parsed after the pipeline target, terminating at
end-of-line.

### 4.4 Completed-effect ledger

```text
[REFERENCE]
// executor.rs — on ExecutionContext
struct CompletedEffect {
    step_number: u32,
    compensation: CommandCall,
}

// ctx.compensations: Vec<CompletedEffect>   // append in completion order
```

A step's action completing without a runtime error appends to `compensations`
**only when** `Step.compensation` is `Some` — a step with no declared
compensation is simply never entered into the ledger at all. There is no
parallel `uncompensable` record and no per-entry outcome field: nothing reads
either (no accessor exists, and no external test can observe
`ExecutionContext`'s internals), and NL-22(a)'s honesty property — an
un-compensable committed effect is never silently treated as undone — already
holds by construction, since the ledger only ever contains what a step
explicitly declared. See §6 for why the originally-specified parallel record
and outcome enum were dropped rather than built unread.

### 4.5 Executor — the unwind

```text
[REFERENCE]
Arming condition (after the step loop finishes):
    status is Failed or Aborted
  → drain ctx.compensations back-to-front (Vec::pop, LIFO falls out for free):
        for each entry (last → first):
            errors_before = ctx.errors.len()
            run entry.compensation through execute_command
            ctx.errors grew ⇒ push RuntimeError { code: COMPENSATION_FAILED,
                                                   step: entry.step_number, .. }
                              CONTINUE to the next entry (see §6)
    status Ok / Partial ⇒ no unwind (armed, not automatic)
```

A compensation's success or failure is detected by comparing `ctx.errors.len()`
before and after running it through `execute_command` — there is no per-entry
outcome field to set (§4.4); the entry is simply popped either way, and a grown
error count is what triggers the `COMPENSATION_FAILED` record.

**A failed compensation does not abandon the remaining ones.** NL-22 fixes the
order and requires each failure to be surfaced, but does not say whether a
failure aborts the unwind. Nodus continues: it cannot know whether the failure
invalidates the earlier undos (that is host domain), and abandoning would leave
*more* effects live with no attempt recorded. Either way the record is complete.
The alternative is weighed in §6.

### 4.6 Validator

- `~COMPENSATE` with no command, or naming an unknown command, → error (reuses the existing unknown-command rule).
- A `~COMPENSATE` clause on a step whose action is not effectful is **not** an error — nodus has no effectfulness classifier, and inventing one would be a host-policy judgement in core (LP-2).

### 4.7 Transpiler

`~COMPENSATE` gains compact and human emitters so NL-6 round-trip holds; the
human form reads as "compensate by …".

### 4.8 Error code

```text
[REFERENCE]
// vocab.rs — one new code, following the Phase-13 CONFIG_INVALID precedent
pub const COMPENSATION_FAILED: &str = "NODUS:COMPENSATION_FAILED";
// error_meta: (Error, Runtime)
```

The lockstep test that guards constant↔metadata sync must be extended with it.

## 5. Implementation Notes

Vertical-slice order (each slice compiles and is independently verifiable):

1. Lexer token + AST field + parser clause + transpiler round-trip — the declaration surface, no behavior yet.
2. Error code + `error_meta` row + lockstep-test extension.
3. Ledger population on step completion (both vectors), still with no unwind — provable by asserting ledger contents after a clean run.
4. The arming condition + LIFO drain + failure recording.

## 6. Drawbacks & Alternatives

- **Compensation as automatic state rollback**: rejected — NL-22 and CO-1 are explicit that this is *semantic* undo of a business effect, not a value-environment restore. Rolling back variables would also silently contradict the `$out`-lock and pipeline rules.
- **Abort the unwind on the first failed compensation**: rejected as the default (§4.5). It is the safer choice *if* one assumes an earlier undo depends on a later one having succeeded — but nodus cannot verify that dependency, and the cost of being wrong is strictly worse (more live effects, fewer attempts recorded). A host that wants abort-on-first-failure can express it by making its compensation actions fail closed.
- **An effectfulness classifier in core**: rejected — which commands have external effects is host knowledge (LP-2); a core classifier would be wrong for every host that extends the vocabulary.
- **A declared `~SCOPE ... ~END` compensation region**: deferred, not rejected. It needs a structural anchor nodus lacks (no subprocess, no indent tokens). Run-as-scope is the honest subset; the region form composes additively when `RUN(@macro)` body expansion lands.
- **A parallel `uncompensable` ledger plus a per-entry `CompensationOutcome` enum** (the original design in earlier drafts of §4.4): dropped during implementation. Nothing reads either — no accessor exists on `ExecutionContext`, and no external test can observe its internals — and NL-22(a)'s honesty property (an un-compensable committed effect is never silently treated as undone) already holds by construction, since the ledger only ever contains what a step explicitly declared. Building unread fields would have been the exact over-engineering this project's own discipline rules out. Compensation success/failure is instead read off the `ctx.errors` length delta around each `execute_command` call (§4.5).
- **An explicit "compensate now" request as a second arming trigger alongside `Status::Failed`/`Status::Aborted`** (as earlier drafts of §3/§4.5 described): not built. No mechanism for a workflow or host to request compensation outside of a failed/aborted run exists in the crate; the unwind is armed by run status alone.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[AST]` | `crates/nodus/src/ast.rs` | `Step` — the struct gaining `compensation`; `CommandCall` — the declaration's type |
| `[LEXER]` | `crates/nodus/src/lexer.rs` | `TokenType` + the keyword-before-generic-flag ordering `~COMPENSATE` must join |
| `[PARSER]` | `crates/nodus/src/parser.rs` | step-clause parsing; `~RETRY:n` is the shape to mirror |
| `[EXEC]` | `crates/nodus/src/executor.rs` | `ExecutionContext` (ledger home), `execute_command` (the one effect path), the step loop and status resolution that arm the unwind |
| `[VOCAB]` | `crates/nodus/src/vocab.rs` | `error_code` + `error_meta` + the lockstep test |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.0 | 2026-07-30 | Core Team | Initial spec — Rust realization of NL-22 (compensation seam): `TildeCompensate` token, `Step.compensation: Option<CommandCall>` mirroring `Step.retry`, `~COMPENSATE: CMD(args)` trailing same-line clause, completed-effect ledger + parallel un-compensable record, LIFO drain armed on `Failed`/`Aborted`/explicit request, `NODUS:COMPENSATION_FAILED`. Three compositions NL-22 names are recorded as **vacuous in core today** rather than silently dropped: the LP-11 `decide → effect → observe` gate (no gate exists — compensations route through the one existing effect path, inheriting the seam for free when it lands), crash-mid-compensation resume (needs NL-12; nodus drives at-least-once per process run, the ledger being the host's replay artifact), and the declared sub-region scope (no structural anchor — run-as-scope is the honest subset). Resolves one point NL-22 leaves open: a failed compensation **continues** the unwind rather than aborting it (§4.5, alternative weighed in §6). |
| 1.0.1 | 2026-07-30 | Core Team | Reconciled to Phase 19's as-built implementation (no design/status change) — §4.4/§4.5 rewritten: `CompletedEffect` is only `{ step_number, compensation }`, with no `step_identity` field, no `CompensationOutcome` enum, and no parallel `uncompensable` vector (nothing read any of them — NL-22(a)'s honesty property already holds by construction). §3/§4.5's "or an explicit compensate request" arming trigger removed — only `Status::Failed`/`Status::Aborted` arm the unwind; no such request mechanism was built. §4.3's canonical example (`UNPUBLISH`) replaced with `NOTIFY`, a real `KNOWN_COMMANDS` entry, matching the crate's own test fixtures. Fixed three dangling `§7` cross-references (the document has no §7) to `§6`. §6 gained two Drawbacks entries recording why the outcome enum/uncompensable record and the explicit-request trigger were dropped. |
| 1.0.2 | 2026-09-24 | Core Team | Consistency pass (2026-09-24): NL-22(d): L1 composes the failure signal from `!!`/`!HALT` and `@err:` routing, but the unwind reads only the final status, and every non-halting error — the `@err:`-routed ones included — ends the run `Partial`, so the common failures leave completed effects live. Recorded as a pending deviation. §2's statement that no LP-11 gate exists is updated: it landed, and compensations inherit it. |
