# Nodus Uncaught-Error Handler Dispatch (Rust)

**Version:** 1.0.2
**Status:** Stable
**Layer:** implementation
**Implements:** l1-nodus-language.md

## Overview

Concrete Rust realization of NL-9's `@err:` half: a declared `@err:` handler is invoked
exactly once when a non-fatal runtime error escapes ordinary step execution uncaught, and
the run's remaining declared steps do not execute after it. Today `WorkflowFile.error_decl`
is parsed, validated, and transpiled but has **zero call sites in `executor.rs`** — a typed
error code reaches `RunResult.errors` and a host reads it, but the workflow-declared handler
itself is never invoked. This spec closes that gap without inventing new DSL grammar,
AST fields, or error codes: the dispatch mechanism falls out of the executor's *existing*
control-flow structure.

## Related Specifications

- [l1-nodus-language.md](l1-nodus-language.md) — defines NL-9 (`@err:` is part of a
  workflow's public contract) and `error_decl`'s grammar (§4.2)
- [l2-nodus-runtime.md](l2-nodus-runtime.md) — owns the executor's main step loop and boot
  sequence this spec's dispatch check attaches to; its §3 NL-9/NL-2 rows are corrected here
  (both named a `Status` variant — `RuleViolation` / `Error` — that does not exist in
  `executor::Status`, predating most of this crate's evolution and never previously caught)
- [l2-nodus-errors.md](l2-nodus-errors.md) — owns the `NODUS:*` taxonomy this spec dispatches
  against; its NL-9 row already recorded (v1.1.0, during the LP-11 design pass) that
  "building executor-side `@err:` dispatch is a real, separate NL-9 obligation" — this is
  that obligation, discharged
- [l2-nodus-dialog.md](l2-nodus-dialog.md) — `DIALOG_TIMEOUT`/`DIALOG_REJECTED`, two of the
  three codes this spec's dispatch check reaches
- [l2-nodus-portability.md](l2-nodus-portability.md) — `POLICY_DENIED` (LP-11), the third
- [l2-nodus-compensation.md](l2-nodus-compensation.md) — the precedent for reusing a
  triggering step's own `step_number` when dispatching a *second* command on its behalf, and
  for detecting a newly-added error via an `errors.len()` delta

## 1. Motivation

Every fixture in the normative corpus declares `@err: ESCALATE(human)` expecting it to fire
on an uncaught error. It never does. `l2-nodus-errors.md`'s own Overview already states the
as-built model honestly ("the `@err:` vs. continue decision a host applies"), but that model
was never a deliberate design choice — it is the *absence* of a call site, not a decision
that dispatch is unnecessary. NL-13's and NL-14's own Document History entries already
presume a working "`@err:` typed" routing destination for `MAX_REACHED`-class exhaustion
codes; the mechanism they presume does not exist until this spec is realized.

## 2. Constraints & Assumptions

- No new lexer, parser, AST, or transpiler work — `ErrorDecl { handler: Option<CommandCall>,
  raw: String }` already carries everything the dispatch call site needs (`ast.rs:140`).
- No new error code and no `PolicyProvider`/`AuditProvider`/other trait change — dispatch
  reuses whichever `NODUS:*` code the triggering error already carries.
- The dispatch check must not introduce a hardcoded list of "dispatchable" codes. See §4.1:
  eligibility falls out of the existing `Signal` return shape, not an enumerated allowlist,
  so a future non-fatal error code is dispatchable automatically, with no edit here.
- `RULE_VIOLATION`'s dedicated fatal path (`Signal::Break`, forces `Status::Failed`) is
  unchanged — `l2-nodus-errors.md`'s NL-2 row already guarantees this and this spec does not
  touch it.
- Zero dependency (LP-1): the mechanism is a conditional branch and a `Value::Map`
  construction, both already-used patterns.

## 3. Invariant Compliance

| L1 Invariant | Implementation |
| --- | --- |
| NL-9 Typed I/O / `@err:` contract | **Implemented [v1.0.1].** `execute_command`/`handle_dialog` already pushed a `RuntimeError` and returned `None` (no `Signal`) for every non-fatal error; the main step loop's `_ => {}` catch-all arm (§4.1) is the one place every such error necessarily reaches, since a `Signal`-returning error (`RULE_VIOLATION`'s `Signal::Break`) is caught by an earlier match arm and never falls through. When `ast.error_decl.handler` is `Some(cmd)` and this step's execution added an entry to `ctx.errors`, `$error` is populated from that entry (§4.2) and `cmd` is dispatched via the ordinary `execute_command` path (§4.3) — then the main step sequence ends (§4.4). A workflow with no `@err:` handler, or an `@err:` line with no handler text, behaves exactly as before this spec (§4.5) — which is itself a deviation from L1: with no handler the run keeps executing instead of terminating with `NODUS:UNHANDLED_ERROR` (§4.5, pending). |

## 4. Detailed Design

### 4.1 Eligibility is structural, not an enumerated list

The main step loop already distinguishes two shapes of step outcome:

```text
[REFERENCE]
match self.run_step_with_retry(&mut ctx, step) {
    Some(Signal::Halt) => { halted = true; break; }
    Some(Signal::Break) => { abort = true; break; }   // RULE_VIOLATION's own path
    Some(Signal::Pause) => { paused = true; break; }
    _ => { /* dispatch check attaches here */ }
}
```

Every call site that pushes a `RuntimeError` and returns a `Signal` (today: only the `!!`
rule check, via `Signal::Break`) is already caught above the `_` arm and never reaches it.
Every call site that pushes a `RuntimeError` and returns `None` — `POLICY_DENIED` (LP-11),
`DIALOG_TIMEOUT`, `DIALOG_REJECTED` (`l2-nodus-dialog.md`) today — falls into `_`. This is
the definition of "uncaught": nothing else in the language has a per-step catch construct,
so a `None`-returning error has nowhere else to go. `COMPENSATION_FAILED` cannot appear here
at all — it is pushed only during the post-loop compensation drain (`l2-nodus-compensation.md`
§4.5), which runs strictly after this loop has already exited with `Status::Failed` or
`Aborted`. No enumerated code list is needed or maintained; a future non-fatal, `None`-
returning error code is dispatchable automatically.

### 4.2 `$error` population

`$error` is `RESERVED` and `RUNTIME_OWNED` (`vocab.rs`) and seeded to `Value::Map(Vec::new())`
at context construction — reserved for exactly this, and never written to by anything today.
On dispatch, it is populated from the triggering `RuntimeError`:

```text
[REFERENCE]
fn error_to_value(err: &RuntimeError) -> Value {
    Value::Map(vec![
        ("code".to_string(), Value::Text(err.code.clone())),
        ("step".to_string(), Value::Int(err.step as i64)),
        ("reason".to_string(), Value::Text(err.reason.clone())),
    ])
}
```

If a step's execution added more than one entry to `ctx.errors` (retry-then-fail shapes),
`$error` reflects the **last** one added — the one that actually stood when the step gave up
— not the first. Populated **before** the handler dispatches, so a handler command
referencing `$error.reason` (via ordinary `$var` interpolation) sees it.

### 4.3 Dispatch reuses the triggering step's own execution path

The real call site names the parsed AST parameter `ast` (not `wf`), and — since a nested
`if` inside an `if let` is a clippy `collapsible_if` lint — is one let-chain, with no
separate `err_dispatched` flag (nothing else in the function reads one; `break` alone is
sufficient):

```text
[REFERENCE]
_ => {
    if let Some(handler) = ast.error_decl.as_ref().and_then(|d| d.handler.as_ref())
        && ctx.errors.len() > errors_before_this_step
    {
        let triggering = ctx.errors.last().expect("errors grew past errors_before_this_step");
        ctx.variables.insert("error".to_string(), error_to_value(triggering));
        self.execute_command(&mut ctx, handler, step.number);
        break;
    }
}
```

`execute_command` is the same function every ordinary step calls — the handler gets its own
`StepStart`/`StepEnd` events, honors its own `→ $target` pipeline binding if the workflow
declared one (`@err: ESCALATE(human) → $handled` already parses today, since Phase 20's
`try_parse_command_from_string` arrow-splitting fix applies to `@err:` handlers along with
`?IF`/`?SWITCH`), and passes through the LP-11 policy gate and LP-16 descriptors exactly like
any other command — no special-casing. The dispatch is **not** re-entrant: if the handler's
own execution pushes a new error, that error surfaces in `ctx.errors` like any other, but
does not re-trigger `@err:` dispatch (the check above is not applied recursively to the
handler's own command).

### 4.4 The handler is the terminal action

After dispatch, the main step loop ends (`break`) — the workflow's remaining declared
`@steps:` do not execute. This is what "uncaught error handler" means for a language with no
per-step catch construct: reaching `@err:` is the last thing the run does. `Status` computation
is **unchanged** — it already computes `Partial` from `!ctx.errors.is_empty()` regardless of
which steps ran, so the triggering error still surfaces in `RunResult.errors` exactly as
before this spec, and the host still applies its own "read the code and decide" judgment
(`l2-nodus-errors.md`'s Overview). Successfully running the handler does **not** upgrade
`Status` to `Ok` — this spec adds an observable side effect (the handler ran, `$error` was
populated, remaining steps were skipped), not a reinterpretation of what `Partial` means.

### 4.5 No handler declared, or `@err:` declared with no handler text

`ast.error_decl` is `None` (no `@err:` line at all) or `Some(ErrorDecl { handler: None, .. })`:
there is nothing to dispatch, and the main loop's behavior is **byte-for-byte unchanged** from
before this spec — the `_` arm's `if let Some(handler) = ...` simply does not match, and step
execution continues exactly as it does today. `W001`'s existing validator warning ("No @err
handler...") is unaffected and still fires only for the true-absence case.

**Deviation from L1 (pending).** `l1-nodus-language.md` §4.5 states that when no handler is
declared an unhandled error terminates the workflow with `NODUS:UNHANDLED_ERROR`. The shipped
loop instead runs the remaining steps against whatever the failed step left behind, so
omitting `@err:` makes a failure *less* contained than declaring one — the inverse of what
the language promises, and the very outcome §6 rejects for the handler case. `W001` flags the
absence but cannot stop the run. Alignment: when a step adds an error and no handler is
declared, push `NODUS:UNHANDLED_ERROR` and end the step sequence exactly as the dispatch
branch does; `Status` stays `Partial`, as it is after a dispatched handler.

**[CORRECTED v1.0.1]** This section originally also named "handler text that does not parse
into a recognized command" as a second `handler: None` case. Verified against
`try_parse_command_from_string` (`parser.rs`) rather than assumed: the function returns `None`
**only** when its input is empty (checked both before and after splitting off a trailing
`→ $target`) — any other text, including a lowercase free-text sentence with no parentheses,
becomes `Some(CommandCall { name: <the whole raw text>, args: vec![], .. })`. Dispatching that
"command" is harmless (it falls to the executor's existing `UNKNOWN_COMMAND` flag, the same
fallback an unrecognized ordinary step command already gets — `Value::Null`, no error), but it
is **not** the same as `handler: None`. The only real `handler: None` case is an `@err:` line
with no text after it at all.

### 4.6 `l2-nodus-runtime.md` corrections found while grounding this spec

Two stale claims in `l2-nodus-runtime.md` §3, both naming a `Status` variant that does not
exist in `executor::Status` (`Ok`/`Partial`/`Failed`/`Aborted`/`Paused`), corrected here
rather than left for a future pass to rediscover:

- NL-2's row said "violations return `Status::RuleViolation`" — the real variant is
  `Status::Failed`.
- NL-9's row said "missing input returns `Status::Error`" — no such variant exists either;
  the row is rewritten in the same edit to also describe this spec's dispatch mechanism.

## 5. Implementation Notes

1. The dispatch check attaches to the main step loop in `executor.rs`'s `execute_inner` (or
   equivalent) — no new function is required, only a conditional branch inside the existing
   `_ => {}` match arm plus the `error_to_value` helper (§4.2).
2. Existing tests whose fixtures declare `@err: ESCALATE(human)` and trigger a `None`-
   returning error (`policy_denies_model_call_effect`, `policy_denies_deferred_effect`, and
   any workflow-level test hitting `DIALOG_TIMEOUT`/`DIALOG_REJECTED`) will, once this lands,
   also observe `ESCALATE` actually dispatching and any steps after the triggering one no
   longer executing. This is the intended conformance fix, not a regression — review each
   such test's assertions during implementation rather than assuming they still hold
   unchanged (the Phase 17/20/21/23/24/25 precedent: activating previously-inert declared
   behavior is expected to change what a test observes, even when nothing about the test's
   own workflow fixture changed).
3. No task here needs a `PolicyProvider`/`AuditProvider` change, no new `Value` kind (NL-7
   holds), and no new `NODUS:*` code — smallest-surface implementation is one conditional
   branch, one small helper function, and (per Implementation Note 2) a review pass over
   existing dialog/policy tests.

## 6. Drawbacks & Alternatives

**Dispatch at run-end instead of at the point of failure** (collect all errors, then invoke
`@err:` once after every step has run): rejected. It would require every subsequent step to
tolerate running against a `$out`/context shape the failed step never populated, multiplying
edge cases with no compensating benefit — nothing in NL-9 or the fixture corpus's own usage
(`@err: ESCALATE(human)`, a "hand off and stop" action) suggests steps after a failure are
meant to keep running.

**Making the handler dispatch re-entrant** (a handler's own error re-triggers `@err:`):
rejected. Nothing declares a *second* handler for a handler's own failure, so re-entry would
either recurse without a base case or need an arbitrary depth cap invented from nothing;
surfacing the handler's own error in `ctx.errors` like any other command's is sufficient and
requires no new mechanism.

**Enumerating dispatchable codes in a constant** (`DISPATCHABLE_CODES: &[&str] = &[...]`):
rejected in favor of the structural definition (§4.1). An enumerated list would need editing
every time a new non-fatal error code is added elsewhere in the crate — silently stale by
default — where the `Signal`-return-shape criterion is correct by construction and needs no
maintenance.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[RUNTIME-LOOP]` | `crates/nodus/src/executor.rs` | The main step loop, `Signal` enum, and every `ctx.errors.push` call site this spec's eligibility rule (§4.1) is grounded against |
| `[ERR-DECL]` | `crates/nodus/src/ast.rs` | `ErrorDecl { handler, raw }` — already-parsed, already-transpiled, the structure this spec dispatches |
| `[ERR-TAXONOMY]` | `crates/nodus/src/vocab.rs` | `RESERVED_VARIABLES`/`RUNTIME_OWNED_VARIABLES` (`$error`'s reservation) and the `NODUS:*` codes this spec's dispatch check observes without enumerating |
| `[COMPENSATION-PRECEDENT]` | `crates/nodus/src/executor.rs` (compensation drain) | The existing "detect a new error via `errors.len()` delta, dispatch a second command reusing the triggering step's own number" shape this spec's §4.3 mirrors |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.2 | 2026-09-24 | Core Team | Consistency pass (2026-09-24): §4.5 records the deviation from L1 §4.5: with no `@err:` handler declared the run keeps executing after an uncaught error instead of terminating with `NODUS:UNHANDLED_ERROR`, so omitting the handler makes a failure less contained than declaring one. The NL-9 row drops a test count. |
| 1.0.1 | 2026-07-31 | Core Team | **Implemented the dispatch designed in v1.0.0 — Phase 26.** Landed exactly as designed: the eligibility check, `error_to_value`, and dispatch call site in `execute_inner`'s main loop; no `PolicyProvider`/`AuditProvider` change, no new error code. **Self-review reconciling §4.3's pseudocode against the as-built code found two divergences**: the real parameter is named `ast`, not `wf` (the pseudocode's illustrative name); and the pseudocode's separate nested `if let`+`if` collapses to one let-chain in the real code (`clippy::collapsible_if`), with no `err_dispatched` flag (nothing reads one — a bare `break` suffices) — both corrected. **§4.5 also corrected**, a genuine inaccuracy found while writing the "unparseable handler" test: `try_parse_command_from_string` returns `None` **only** for empty input, never for text that merely doesn't look like a command call — any non-empty `@err:` text becomes `Some(CommandCall{name: <the raw text>, ..})`, which dispatches harmlessly through the executor's existing `UNKNOWN_COMMAND` fallback. The only real `handler: None` case is an `@err:` line with no text at all; the "text that does not parse" framing was never really true. **Found and fixed one real regression, not anticipated at spec time**: `tests/control_flow.rs`'s `retry_reruns_failing_step_up_to_bound` (pre-existing, from an earlier phase) asserted "the step after an exhausted retry still runs" — true only because dispatch didn't exist yet; `RETRY_TIMEOUT_WF` already declares `@err: ESCALATE(human)`, and `l1-nodus-language.md`'s own `~RETRY:n` row has always said exhaustion "routes to `@err:`", so the test's old assertion was pinning the very gap this spec closes. Corrected to assert the handler now dispatches and the following step does not run. 452 tests pass (was 447; +5: `err_handler_dispatches_on_policy_denial`, `err_handler_dispatches_on_dialog_denial`, `no_err_handler_declared_is_unchanged`, `empty_err_handler_is_unchanged`, `retry_then_succeed_never_dispatches_err_handler`); reviewed Phase 24's `policy_denies_model_call_effect`/`policy_denies_deferred_effect` (both use fixtures that already declare `@err:`) and confirmed their existing assertions still hold, not vacuously — no changes needed there. `cargo clippy -p nodus --all-targets -- -D warnings`: one `collapsible_if` finding, fixed (the let-chain above); `cargo fmt -p nodus -- --check`: several line-wrap violations in the new tests, fixed (no logic change); `Cargo.toml`/`Cargo.lock` diff empty (LP-1 preserved); the one new `.expect()` on production code (`ctx.errors.last().expect(...)`) is structurally unreachable-as-`None` — it runs only inside the branch that already proved `ctx.errors.len() > errors_before_this_step`, so the vec is provably non-empty. |
| 1.0.0 | 2026-07-31 | Core Team | Initial spec. Designed NL-9's `@err:` dispatch mechanism: eligibility is structural (any `RuntimeError` pushed by a step that returns no `Signal` — never an enumerated code list), `$error` (already `RESERVED`+`RUNTIME_OWNED`, never previously written) populated from the triggering error before dispatch, the handler runs via the ordinary `execute_command` path (LP-11/LP-16 apply unchanged), and the main step sequence ends afterward — `Status` computation untouched. Explicitly out of scope: `RULE_VIOLATION` (already has its own dedicated fatal path, structurally excluded — never reaches the check) and `COMPENSATION_FAILED` (structurally cannot occur inside the main loop this spec attaches to). Found and corrected two stale `l2-nodus-runtime.md` §3 claims naming a nonexistent `Status` variant (`RuleViolation`, `Error`) — predating most of this crate's evolution, never previously caught because only §3.1 (additions after v1.0.0) had been repeatedly reconciled; §3's original rows had not. Named, not silently absorbed: implementing this will retroactively activate `@err:` dispatch for nearly every existing fixture (all of which already declare `@err: ESCALATE(human)`), so existing policy/dialog-denial tests will observe new behavior once built — flagged as the expected conformance-fix pattern, not a regression, in §5. |
