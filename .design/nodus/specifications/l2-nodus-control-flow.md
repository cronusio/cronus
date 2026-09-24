# Nodus Control-Flow Constructs Implementation (Rust)

**Version:** 1.0.3
**Status:** Stable
**Layer:** implementation
**Implements:** l1-nodus-language.md

## Overview

Rust realization of the v0.7 control constructs that extend the v0.4 control flow
(`?IF`/`~FOR`/`~UNTIL`/`~PARALLEL`). `l1-nodus-language.md` §4.6 defines five new
constructs; this spec records how the lexer, AST, parser, and executor realize
them: `?SWITCH` multi-branch dispatch, `~MAP` collection transform, `~RETRY:n`
step-level retry, `!HALT` fatal stop, and `!PAUSE` suspension. The supporting
machinery already exists — `Status::Failed`/`Status::Paused` and `Signal::Pause`
(Phase 10), and the `SWITCH_NO_MATCH`/`PAUSED` error codes (Phase 8) — so this
spec mostly adds parsing and dispatch.

## Related Specifications

- [l1-nodus-language.md](l1-nodus-language.md) — the language contract: §4.4 base control flow, §4.6 the constructs added here; NL-5 bounded loops
- [l2-nodus-runtime.md](l2-nodus-runtime.md) — runtime crate extended here: `lexer`, `ast`, `parser`, `executor`, `transpiler`, `validator`; §4.7(b) itemizes the gap
- [l2-nodus-errors.md](l2-nodus-errors.md) — owns `SWITCH_NO_MATCH` and `PAUSED`
- [l2-nodus-dialog.md](l2-nodus-dialog.md) — established `Status::Paused` + `Signal::Pause`, which `!PAUSE` reuses

## 1. Motivation

The crate parses only the v0.4 control set; `?SWITCH`/`~MAP`/`~RETRY` are not
tokenized (`~MAP`/`~RETRY` currently mis-lex as a `Flag`), and `!HALT`/`!PAUSE`
have no token or AST flag. Workflows needing multi-way dispatch, a one-line
collection transform, bounded retry, a hard stop, or a human pause cannot be
expressed. This spec closes that, reusing the pause/halt machinery already in the
executor.

## 2. Constraints & Assumptions

- No new external dependency (LP-1).
- NL-5 (bounded loops) extends to `~RETRY`: `n` is mandatory and capped (max 10); a missing or over-cap bound is a validation error.
- `?SWITCH` is first-match-wins with no fallthrough; an unmatched scrutinee with no `* →` default emits `NODUS:SWITCH_NO_MATCH` (warning) and continues.
- `~MAP` over an empty or non-list collection yields an empty list and never errors; a collection that is present but not a list is additionally flagged `MAP_SOURCE_NOT_A_LIST:<name>` on the run.
- `!HALT` requires an `ESCALATE()` in the same step (validation-checked); it sets `Status::Failed` and stops the run.
- `!PAUSE` reuses the dialog suspension path: `Signal::Pause` → `Status::Paused` + `ResumeDescriptor`.

## 3. Invariant Compliance

| L1 Invariant | Rust Enforcement |
| --- | --- |
| NL-5 Bounded loops | `~RETRY:n` requires a declared `n` ≤ 10; the validator rejects a missing/over-cap bound with `E017`, mirroring the `~UNTIL MAX:n` rule (`E010`). |
| NL-6 Dual representation | The transpiler round-trips each new construct (compact ⇄ human) so `compact → human → compact` stays AST-equal. |
| NL-7 Closed value system | `~MAP` produces a `Value::List`; `?SWITCH` compares scalars within the closed value set; no new value kinds. |
| NL-10 Sequential pipeline | `~MAP`'s `→ $out` target follows the existing pipeline rule; `?SWITCH` arm actions bind their targets in declaration order. |

## 4. Detailed Design

### 4.1 Lexer tokens

```text
[REFERENCE]
// lexer.rs — new TokenType variants
QSwitch,     // ?SWITCH
TildeMap,    // ~MAP
TildeRetry,  // ~RETRY (carries :n like Max)
BangHalt,    // !HALT
BangPause,   // !PAUSE
```

`~MAP`/`~RETRY` must be matched as keywords *before* the generic `~identifier`
`Flag` rule, fixing the current mis-lex.

### 4.2 AST nodes

```text
[REFERENCE]
// ast.rs — new statements + action flags
pub struct SwitchBlock {
    pub scrutinee: String,                       // $var
    pub arms: Vec<(String, CommandCall)>,        // value → action
    pub default: Option<CommandCall>,            // * → action
}

pub struct MapBlock {
    pub collection: String,                      // $coll
    pub command: CommandCall,                    // body using implicit $it
    pub target: Option<String>,                  // → $out
}

pub enum Stmt { /* … */ Switch(SwitchBlock), Map(MapBlock) }

// Conditional gains action flags alongside break_flag/skip_flag:
//   halt_flag: bool, pause_flag: bool
// Step gains: retry: Option<u32>   (from ~RETRY:n)
```

### 4.3 Parser

- `?SWITCH $v:` followed by `value → action` arms and an optional `* → action`, terminated like other blocks (`~END` / dedent per the existing block rules).
- `~MAP $coll: CMD($it) → $out` as a single-line construct; `$it` is the implicit per-element binding.
- `~RETRY:n` as a step modifier attached to the step it precedes (parsed like `MAX:n`).
- `!HALT` / `!PAUSE` as action flags on a conditional/step action, parsed like `!BREAK`.

### 4.4 Executor

```text
[REFERENCE]
Stmt::Switch → evaluate scrutinee; run the first arm whose value compares equal
              (reuse the condition comparator); else run `*` default; else push
              NODUS:SWITCH_NO_MATCH (warn) and continue.
Stmt::Map    → for each element of the Value::List collection, bind $it, run the
              command, collect results into a Value::List bound to the target;
              empty/non-list ⇒ empty list.
~RETRY:n     → run the step; on a runtime error, re-run up to n times; on
              exhaustion route the error to @err (status Partial/Failed).
!HALT        → push the step's error, set Signal::Break with a halt marker ⇒
              Status::Failed.
!PAUSE       → return Signal::Pause ⇒ Status::Paused + ResumeDescriptor (reuses
              the dialog suspension path).
```

`!HALT` status precedence sits with the existing rule-violation/abort logic
(fatal); `!PAUSE` reuses the paused branch added for dialog.

**Retry re-runs the whole step (hazard, recorded).** A retry re-executes the step's action
and every sub-step, including commands that completed in the failed attempt. An
effect-class command — a model call, `SETTLE`, a host effect — that succeeded before a later
command in the same step failed therefore runs again: a double commit, the step-grain twin
of the restart hazard `l2-nodus-restart.md` §2 documents for NL-23(e). The failed attempt's
completed effects also never enter the compensation ledger, which records only a step that
completes cleanly (`l2-nodus-compensation.md`), so a later unwind cannot undo them. Until a
retry is scoped to the failing command, a retried step holds only idempotent effects. The
`+backoff` and `+retry_on` modifiers L1 §4.6 lists are not realized: every runtime error
retries, immediately.

**`~MAP` over a non-list is flagged, not absorbed.** §2 keeps a non-list collection non-halting — the map yields an empty list — but the substitution is no longer silent: the run raises a `MAP_SOURCE_NOT_A_LIST:<collection>` flag, so a check built over the empty result (every element validated, every item delivered) is distinguishable from one built over mapping an empty list. An absent collection is a legitimate empty case and raises no flag.

### 4.5 Validator

- `~RETRY` without `:n`, or `n > 10`, → error (NL-5 bounded-loop extension).
- `!HALT` without an `ESCALATE()` in the same step → error.
- `?SWITCH` with no arms → warning.
- `~MAP`'s implicit `$it` binding (§4.3) counts as declared for the
  pre-existing "variable used but never assigned" check, on the same terms as
  `~FOR`'s explicit loop variable: the check tracks declarations in one
  file-wide set with no per-construct scope, so a variable a loop introduces
  reads as declared for the rest of the file, not only inside that loop's own
  body — `~MAP` follows this existing precedent rather than introducing new
  scoping behavior. A file containing **no** `~MAP` construct at all still
  flags a bare `$it` reference as undeclared, exactly as before.

### 4.6 Transpiler

Each new node gains compact and human emitters so NL-6 round-trip holds; the
human form reads as "Switch on …", "Map … over …", "Retry up to n", "Halt", "Pause".

## 5. Drawbacks & Alternatives

- **`?SWITCH` as desugared `?IF/?ELIF` chains**: rejected — loses the single-scrutinee intent and the precise `SWITCH_NO_MATCH` diagnostic; a first-class node round-trips cleanly (NL-6).
- **`~MAP` as a `~FOR` with an accumulator**: rejected — `~MAP` is a one-line total transform with implicit `$it`; expressing it as a loop reintroduces the boilerplate it removes.
- **`~RETRY` as an unbounded loop**: rejected — violates NL-5; the mandatory capped `n` keeps it bounded.

## 6. Implementation Notes

Phased order (each a complete vertical slice): (1) `!HALT` / `!PAUSE` action
flags — smallest, reuse `Signal`/`Status`; (2) `?SWITCH` — new node + dispatch;
(3) `~MAP` — new node + dispatch; (4) `~RETRY:n` — step modifier + retry loop.
Each slice carries lexer + AST + parser + executor + transpiler + validator +
tests. `SWITCH_NO_MATCH`/`PAUSED` codes and `Status::Paused`/`Signal::Pause`
already exist; this cluster wires them to syntax.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[LEXER]` | `crates/nodus/src/lexer.rs` | new control tokens |
| `[AST]` | `crates/nodus/src/ast.rs` | `SwitchBlock`/`MapBlock`, action flags, retry |
| `[PARSER]` | `crates/nodus/src/parser.rs` | construct parsing |
| `[EXEC]` | `crates/nodus/src/executor.rs` | dispatch + halt/pause/retry |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.0 | 2026-06-27 | Core Team | Initial spec — Rust realization of the v0.7 control constructs (`?SWITCH`/`~MAP`/`~RETRY`/`!HALT`/`!PAUSE`): lexer tokens, `SwitchBlock`/`MapBlock` AST + action flags + retry, parser/executor/validator/transpiler wiring; reuses `Status::Paused`/`Signal::Pause` and `SWITCH_NO_MATCH`/`PAUSED`. Phased implementation recommended. |
| 1.0.1 | 2026-07-25 | Core Team | §4.5 patch: records that `~MAP`'s implicit `$it` binding (§4.3) must count as declared for the pre-existing variable-declaration check, on the same file-wide-set terms as `~FOR`'s explicit loop variable — closes a conformance gap where the realized check omitted this and rejected every `~MAP` workflow. No design change; status stays `Stable`. |
| 1.0.2 | 2026-09-24 | Core Team | Consistency pass (2026-09-24): §4.4 records two hazards: a retry re-runs the whole step, re-committing effects that succeeded in the failed attempt and leaving them outside the compensation ledger (`+backoff`/`+retry_on` are unrealized); and `~MAP` over a non-list silently yields an empty list, so a check over the result passes vacuously. The NL-5 row names `E017`. |
| 1.0.3 | 2026-09-24 | Core Team | Realization sync (2026-09-24): `~MAP` over a present non-list collection raises a `MAP_SOURCE_NOT_A_LIST:<name>` flag instead of absorbing the type error silently. The retry-repeats-effects hazard remains recorded. |
