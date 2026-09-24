# Nodus DSL Testing — Rust Implementation

**Version:** 1.4.0
**Status:** Stable
**Layer:** implementation
**Implements:** [l1-nodus-testing.md](l1-nodus-testing.md)

## Overview

This document specifies the Rust implementation of the nodus `@test:` block testing contract
defined in `l1-nodus-testing.md`. It covers the concrete types in `crates/nodus`, the
execution model wired in `workflows.rs`, the assertion evaluator, and the validator
diagnostics for test-related issues.

## Related Specifications

- [l1-nodus-testing.md](l1-nodus-testing.md) — parent contract; defines NT-1…NT-11 invariants
- [l2-nodus-runtime.md](l2-nodus-runtime.md) — Rust runtime module structure and `Executor`
- [l1-nodus-language.md](l1-nodus-language.md) — `@test:` block as a language-level declaration; **NL-6** dual-representation (compact → human → compact must be AST-equal), the invariant §10.4's round-trip rule realizes for `TestBlock`
- [l2-nodus-control-flow.md](l2-nodus-control-flow.md) — its §3 NL-6 row governs the same round-trip guard for control-flow statements; §10.5 records why `@test:` bodies were initially outside that guard's assertion scope
- [l1-nodus-portability.md](l1-nodus-portability.md) — [ADDED v1.3.0] NT-11 differential parity is the executable form of LP-3 (the two-conforming-host requirement); §11 records why the crate has no second path for it to diff
- [l2-nodus-portability.md](l2-nodus-portability.md) — [ADDED v1.3.0] its §4.8 LP-3 admission records are a *different* two-host comparison (a satisfying host vs. a non-satisfying one, asserting **divergence**) from NT-11's parity harness (two conforming hosts, asserting **equivalence**) — not reusable for this invariant

## 1. Module Map

| Module | Role |
| --- | --- |
| `crates/nodus/src/ast.rs` | `TestBlock` AST node — structured fields after parsing |
| `crates/nodus/src/parser.rs` | `parse_test_block()` + `parse_test_body()` |
| `crates/nodus/src/workflows.rs` | `test()`, `test_with_tags()`, assertion evaluator, `TestReport`/`TestResult` |
| `crates/nodus/src/validator.rs` | `E015` duplicate-name check; `W009` no-expected advisory; `W015` non-conforming pair separator (§10.3) |
| `crates/nodus/src/executor.rs` | `RunResult.vars` — final variable environment exposed for assertions |
| `crates/nodus/src/transpiler.rs` | `@test:` block compact-form emission — governed by the NL-6 round-trip rule (§10.4) |

## 2. TestBlock AST Node

```rust
pub struct TestBlock {
    pub name: String,
    pub input: Vec<(String, String)>,    // field_name → raw_value_string
    pub expected: Vec<(String, String)>, // variable_name → raw_expected_string
    pub tags: Vec<String>,               // tag identifiers
    pub raw_lines: Vec<String>,          // backward-compat for transpiler
}
```

<!-- [MODIFIED] v1.2.0 — the previous description of this field was factually wrong. -->
`raw_lines` is the **lexed token stream of the block body**, and the structured
`input`/`expected`/`tags` fields are a **derived view** of it: `parse_test_block` calls
`collect_braced_raw_lines()` and then hands that same vector to `parse_test_body()`. They
are therefore **not alternative representations**, and both are populated for every parsed
block regardless of how the body was written. A structured field is empty only when nothing
in the token stream matched that section's pair grammar (§10.2/§10.3) — not because the body
used one style rather than another.

> Prior to v1.2.0 this section stated that "for old-style inline bodies the structured
> fields are empty and `raw_lines` holds the tokens". No parse produces that state. The
> transpiler's emission branch was written against that non-existent state and consequently
> never reached its own `raw_lines` path (§10.4).

`raw_lines` is the authoritative round-trip source (§10.4) and additionally has two live
readers in the validator — `w006_route_test_coverage` (NT-10) and the smoke-tag heuristic
both scan it textually — so it is not a transpiler-only artifact and cannot be dropped.

## 3. RunResult Variable Environment

`executor::RunResult` exposes the full post-execution variable environment:

```rust
pub struct RunResult {
    pub workflow: String,
    pub status: Status,
    pub out: Value,
    pub log: Vec<LogEntry>,
    pub errors: Vec<RuntimeError>,
    pub flags: Vec<String>,
    pub vars: HashMap<String, Value>, // key without '$': "out", "confidence", etc.
}
```

`vars` is populated from `ExecutionContext.variables` after the last step executes.
It is the authoritative source for `expected:` assertions (NT-3).

## 4. Public API

```rust
// Run all @test: blocks in declaration order. Returns an empty report for
// workflows with no test blocks (not an error).
pub fn test(source: &str, filename: &str) -> Result<TestReport, Error>

// Like test(), but only runs blocks whose tags list intersects tag_filter.
// If tag_filter is empty, all blocks run (NT-6).
pub fn test_with_tags(source: &str, tag_filter: &[&str]) -> Result<TestReport, Error>
```

Both functions are re-exported from the crate root.

**Validation first (NT-9).** Both entry points validate before any block runs, exactly as every `run*` entry point does. `test` validates under the file name it is given, so the name-matches-file rule applies; `test_with_tags` is given none and validates under the name the workflow declares. Any error-severity diagnostic returns `Error::Validate` — the first code, with every error message — and no block executes, so a workflow that could not run in production cannot pass its tests either. `E015` and the `@test:`-specific `E023` (§7) are among the errors that gate the file.

### TestReport / TestResult

```rust
pub struct TestReport {
    pub results: Vec<TestResult>, // declaration order (NT-7)
    pub passed: usize,
    pub failed: usize,
}

pub struct TestResult {
    pub name: String,  // @test: block name
    pub passed: bool,
    pub message: String, // "ok" or first-failing-assertion description
}
```

`TestReport.passed + TestReport.failed == TestReport.results.len()` is a structural
invariant enforced by `TestReport::from_results`.

## 5. Execution Protocol (NT-1…NT-5)

`test_with_tags` iterates over parsed `WorkflowFile.tests` in declaration order. Per block:

1. **Build input (NT-2)**: `build_test_input(ast, &tb.input)` seeds the `@in:` declared defaults,
   then overlays the block's `input:` key-value pairs. Every `input:` key is a declared `@in:`
   field by the time this runs: an undeclared key is `E023` at validation (§7), so a misspelled
   key fails the file instead of running the block on the field's default. Returns
   `Value::Map(...)` passed to `Executor::with_stub().execute()`.

2. **Fresh executor (NT-1 / NT-5)**: `Executor::with_stub()` creates a new executor with a
   `StubProvider` instance — no state from prior blocks, no real I/O or network access.

3. **Execute**: `executor.execute(ast, Some(input))` runs the full `@steps:` body and returns
   `RunResult` including `vars` (NT-8: same schema as production runs).

4. **Evaluate (NT-3 / NT-4)**: `evaluate_test_block(&run_result.vars, &run_result.status, &tb.expected)`.

<!-- [ADDED] v1.1.0 -->
<!-- [ADDED] v1.1.0 -->
**Parallel-safe stub (NT-5 extension).** `StubProvider` is stateless and input-keyed (`Send + Sync`), so it would stay deterministic under concurrent branch scheduling. The executor schedules none: `~PARALLEL` branches run sequentially in declared order (`l2-nodus-runtime.md` §4.4), so a `@test:` block containing `~PARALLEL` exercises that sequential realization — a failing branch ends the block (fail-fast) and its `~JOIN` target is the map of branch results in declared order. Block-level isolation is unchanged: one fresh executor per block, blocks themselves run in declaration order.

## 6. Assertion Evaluator

```rust
fn evaluate_test_block(
    vars: &HashMap<String, Value>,
    status: &Status,
    expected: &[(String, String)],
) -> (bool, String)
```

Semantics:

- Empty `expected`: passes iff `status == Status::Ok`; fails with `"execution failed with status …"` otherwise.
- Non-empty `expected`: first checks `Status` is `Ok`; then for each `(var, val)` pair:
  - Strip leading `$` from `var` to get the key in `vars`.
  - If key absent from `vars` → fail with `"… is not in the execution context"` (NT-3).
  - `parse_expected_value(val)` parses the raw string into `Value`.
  - Compare with `PartialEq` on `Value` (structural recursive equality per NL-7 / §4.3).
  - If mismatch → fail with `"assertion failed: $var expected … got …"`.
  - Continue to next assertion only if current passes.
- Returns `(true, "ok")` when all assertions pass.

**Pass condition (NT-4).** A block passes only when every assertion holds **and** the run ended `Ok`, as L1 §4.2 step 5 and §4.4 require. `Partial` means non-fatal errors occurred, so a regression that makes a clean run start failing steps cannot stay green because the asserted variables still match. Asserting that a block *expects* a degraded run needs a status assertion, which belongs to the richer-assertion amendment L1 §5 defers.

### Value Parsing

`parse_expected_value(s: &str) -> Value`:

| Input | Parsed as |
| --- | --- |
| `null` | `Value::Null` |
| `true` / `false` | `Value::Bool` |
| Valid `i64` | `Value::Int` |
| Valid `f64` | `Value::Float` |
| `"quoted"` or bare text | `Value::Text` (quotes stripped) |

The lexer already strips quotes from `StringLit` tokens, so the raw string arriving in
`TestBlock.expected` is unquoted. Bare words that are not numbers or booleans become `Text`.

**Known gap — literal kind is lost (pending).** Because the quotes are gone before this parser
runs, `"42"` and `42` reach it identically, and a quoted literal is re-typed from its
spelling: `"007"` becomes `Int(7)`, `"true"` becomes `Bool(true)`. Two consequences follow.
For `input:`, the same parser ignores the `@in:` field's declared `type_name`, contrary to
NT-2's type contract — a `text` field given `"007"` receives `Int(7)`. For `expected:`, a
workflow that regresses from producing `Text("42")` to `Int(42)` still passes an assertion
written as `"42"`. The fix carries each value's token kind (string literal or bare) from the
lexer into the `TestBlock` pairs, reads an `input:` value as its field's declared type, and
reports a value that cannot be read as that type at validation (NT-9) instead of coercing it.

## 7. Validator Diagnostics

| Code | Severity | Trigger | Rust location |
| --- | --- | --- | --- |
| `E015` | Error | Two `@test:` blocks share the same name within a file | `Validator::e015_no_duplicate_test_names` |
| `E023` | Error | An `@test:` block sets an `input:` key the workflow does not declare in `@in:`, or asserts in `expected:` a variable that is not reserved, not `@out`, not an `@in:` field and not the target of any step — a name that cannot exist (NT-9) | `Validator::e023_test_names_exist` |
| `W006` | Warning | `ROUTE(wf:x)` step with no `@test:` block covering it (NT-10) | `Validator::w006_route_test_coverage` (pre-existing) |
| `W009` | Warning | `@test:` block with no `expected:` section (passes trivially on Status::Ok) | `Validator::w009_test_no_expected` |
| `W015` | Warning | A token run inside `input:`/`expected:` that resembles a key-value pair but uses a separator other than `:` — the pair is skipped by `parse_test_body`, so the assertion never reaches the evaluator (§10.3) | `Validator::w015_test_pair_separator` [ADDED v1.2.0] |

`E015` is a block-class error — workflows with duplicate test names fail the validate-before-run
gate and cannot execute.

`E023` is an error for the same reason: an `input:` override with no `@in:` slot runs the block on the field's default while its author believes the override took, and an `expected:` variable that can never be present can only fail — or be read as passing on a run that never produced it. Both are defects in the test, so they fail the file rather than the block.

`W015` is deliberately warning-severity, not an error: its purpose is to surface assertions
that are being silently ignored in the existing corpus, which an error would instead convert
into a hard validate-before-run failure on files that parse today. Note the interaction with
`W009` — a block whose only `expected:` pairs are all non-conforming has an `expected:` section
in source but an empty one in the AST, so it emits **both** `W015` (the pairs were dropped) and
`W009` (nothing is asserted). That pairing is the intended signal, not a duplicate report.

## 8. NT-1…NT-11 Compliance Table

| Invariant | Status | Implementation |
| --- | --- | --- |
| NT-1 Block isolation | **Implemented** | Fresh `Executor::with_stub()` per block in `run_test_block` |
| NT-2 Input override | **Partial** | `build_test_input` overlays block `input:` over `@in:` defaults, and a key the workflow does not declare is rejected at validation (`E023`); values are still typed from their spelling rather than from the field's declared type, so the `@in:` type contract is not yet honoured (§6, literal kind) |
| NT-3 Expected assertion binding | **Implemented** | `evaluate_test_block` checks `vars` by key; absent variable = fail |
| NT-4 Assertion failure semantics | **Implemented** | Blocks continue regardless; first-failing-assertion message. A block passes only when every assertion holds and the run ended `Ok` (§6) |
| NT-5 Provider neutrality | **Implemented** | `StubProvider` per block; no real I/O |
| NT-6 Tag metadata | **Implemented** | `test_with_tags` filters by tag intersection; skipped blocks absent from report |
| NT-7 Ordered reporting | **Implemented** | Iterator preserves `WorkflowFile.tests` declaration order |
| NT-8 Schema inheritance | **Implemented** | `Executor::with_stub().execute(ast, ...)` uses the same `ast` (same `§runtime`) |
| NT-9 Parse-time validation | **Partial** | The test entry points validate first (§4), so `E015` duplicate names and the forward-reference variable checks (`E014`) gate the file; `E023` rejects an undeclared `input:` key and an `expected:` variable that can never exist; `W015` covers the silent-drop case NT-9's "not a silent assertion-miss" clause targets (§10.3). Two clauses remain open: a full `@test:`-specific forward-reference check is deferred, and an `input:` value that cannot be read as its field's declared type is coerced rather than reported (§6, literal kind) |
| NT-10 Route coverage advisory | **Implemented** | `W006` emitted by pre-existing `Validator::w006_route_test_coverage` |
| NT-11 Differential parity [ADDED v1.3.0] | **Vacuous in core** | See §11 — the crate has one execution path, so §4.7's `check_parity(fixture, path_B)` has no second `path_B` to run against `path_A`'s recorded fixture |

## 9. Test Coverage

| Test type | Location | Count |
| --- | --- | --- |
| `parse_test_body_*` unit tests | `parser.rs #[cfg(test)]` | 3 |
| `parse_expected_value_*` unit tests | `workflows.rs #[cfg(test)]` | 9 |
| `evaluate_test_block_*` unit tests | `workflows.rs #[cfg(test)]` | 5 |
| `test_*` integration unit tests | `workflows.rs #[cfg(test)]` | 10 |
| E015 / W009 validator unit tests | `validator.rs #[cfg(test)]` | 4 |
| NT-1…NT-7 integration tests | `tests/testing.rs` | 7 |

## 10. Body Grammar Conformance & NL-6 Round-Trip [ADDED v1.2.0]

### 10.1 Canonical grammar

`l1-nodus-testing.md` §4.1 defines the body as one `{key}: {value}` pair per line beneath a
section header, and §4.3's worked example uses the same shape. That is the only form the
parent contract defines, and the only form this implementation treats as canonical:

```text
[REFERENCE]
@test: smoke {
  input:
    query: "hello"
  expected:
    $out: "hello"
  tags: [smoke]
}
```

### 10.2 The tolerated inline-brace form

`parse_test_body` scans a flat token vector for `{key} : {value}` triples and skips any token
it does not recognize. A body written inline — `input: { query: "hello", tone: "warm" }` —
therefore also parses: the `{`, `,`, and `}` tokens are simply skipped between triples.

This form is **accepted but not canonical**: it appears nowhere in `l1-nodus-testing.md` §4.1.
It is retained because corpus files use it, and it MUST NOT be extended — no nesting, no
alternative separators, no reliance on the brace structure carrying meaning. It survives a
round-trip only because `raw_lines` preserves it verbatim (§10.4), not because the grammar
models it.

### 10.3 Non-conforming pairs are diagnosed, never silently dropped

Only `:` separates a key from its value. A pair written with any other separator — in the
current corpus, `expected: { status = SUCCESS }` — matches no triple, so its tokens are
skipped and **the assertion never reaches the evaluator**. An `expected:` section reduced to
empty this way then takes the evaluator's empty-`expected` path (§6), which passes on
`Status::Ok`. The net effect is that a declared assertion silently becomes no assertion:
the block reports `passed` without ever having checked what it claims to check.

NT-9 requires that a defective test block surface "as a validation error, not a silent
assertion-miss at run time". A dropped pair is that same failure class, so it is diagnosed as
`W015` (§7).

This resolves the question of the `=` form's legality: it is **not** legal — L1 §4.1 admits
only `:` — and the defect was never that the parser rejected it, but that it rejected it
without saying so.

### 10.4 Round-trip rule (NL-6)

`l1-nodus-language.md` NL-6 requires `parse → to_nodus → parse` to be AST-equal, and
`TestBlock` — including its `raw_lines` field — is part of that AST.

Two facts constrain the emission. First, the lexer strips quotes from `StringLit` tokens
(§6), so every value held in `raw_lines` and in the structured fields is **unquoted**, and a
value emitted bare does not always re-lex to the single token it came from. Both failure
shapes are observed in the current corpus: a value containing whitespace
(`When is my invoice due?`) splits on its spaces, and a value containing a token-splitting
character (`T-001`) splits at that character — in each case the re-parse keeps only the
first fragment (`When`, `T`) as the pair's value. Second, emitting the
canonical §10.1 form for a body that was written in the §10.2 inline form produces a
**different `raw_lines`** on re-parse (the `{`/`,`/`}` tokens are gone), which breaks
AST-equality even if every value were quoted correctly.

The rule therefore has two parts:

> **(a) Source selection.** When `raw_lines` is non-empty it is the emission source, because
> it is the only representation that preserves the author's **token sequence** — including
> the `{`/`,`/`}` of the §10.2 inline form, which the structured fields discard. It does not
> preserve line breaks: `collect_braced_raw_lines` drops `Newline` tokens, so a multi-line
> body re-emits as a single flat line. That is sufficient for NL-6, which requires
> AST-equality and not source identity, and re-emitting the line breaks would in fact
> **break** it by changing `raw_lines` on re-parse. The structured fields are the fallback,
> used for `TestBlock` values constructed programmatically rather than parsed.
>
> **(b) Value re-quoting.** A value MUST be emitted so that re-lexing it yields exactly one
> token whose value equals the value emitted. Where the bare form would not, the value is
> re-quoted. (Tags and section keywords are identifiers and need no quoting.)

Part (a) is a correction to the *branch order*, not a new mechanism: the emitter already has
a `raw_lines` path, but selects it only when the structured fields are all empty — a state
no parse produces (§2), so the path is unreachable for every parsed block. Inverting the
condition makes §2's long-standing claim that "`raw_lines` is retained for the transpiler's
round-trip path" true for the first time.

Part (b) is what makes AST-equality achievable **without** storing quoting information in the
AST. NL-6 requires equality of the *parsed AST*, not of the source text; quoting is invisible
to the AST because the lexer strips it on the way back in. A conservative always-quote
emission satisfies the rule, as does quoting only those values whose bare form would not
re-lex to a single equal token.

### 10.5 Scope note

`@test:` bodies were outside the corpus-wide NL-6 round-trip guard when that guard was first
built: it asserts equality of `WorkflowFile.steps` rather than of the whole file, precisely
because the gap described here was open. Closing §10.4 is what allows that assertion to be
widened to the full `WorkflowFile`, which is the observable acceptance signal for this
section.

## 11. Differential Parity (NT-11) [ADDED v1.3.0]

`l1-nodus-testing.md` §4.7 frames NT-11 as "interpreter↔transpiler or two conforming hosts
must produce equal observable results, output + trace shape, on a recorded fixture corpus" —
the executable form of `l1-nodus-portability.md`'s LP-3 two-host requirement. Realizing it
needs a second `path_B` to run `check_parity` against `path_A`'s recorded `Fixture`.

**The crate has no such second path.** `Transpiler::to_nodus(ast: &WorkflowFile) -> String`
(`[TRANSPILER]`) emits nodus source text — it is the inverse of parsing, not an alternative
executor; there is no code path that takes an AST and *runs* it other than `Executor` itself.
So "interpreter↔transpiler" parity is not two conforming paths to diff, it is one path plus a
serializer for it — nodus already asserts the correct property for that pair, which is NL-6
AST-equality (§10.4/§10.5), not NT-11 output/trace-shape equivalence. The two invariants
target different failure classes: NL-6 catches a `parse → emit → parse` structural drift;
NT-11 catches two *executions* of the same AST disagreeing on `vars` or event shape, which
presupposes two executions exist.

**The LP-3 two-host admission record (`l2-nodus-portability.md` §4.8) is not reusable
either**, despite also comparing two hosts: it proves a *satisfying* `PolicyProvider` host and
a *non-satisfying* one **diverge** (`Ok` vs `Failed`, by design — that is what a manifest gate
is for), where NT-11 requires two *conforming* hosts to **agree**. Reusing that harness would
assert the opposite of what NT-11 means.

No in-crate second conforming host exists to construct one honestly, and building one solely
to have a `path_B` to diff against would be pure duplication with no product behind it — the
crate would carry two executors maintained in lockstep for a test to pass, not because a
second host has any reason to exist. **NT-11 is therefore Vacuous in core** (§8), on the same
terms `l2-nodus-portability.md` §3.1 already uses for a seam whose gated construct does not
exist: nothing is owed until a real second conforming execution path does (a second in-process
host, or an out-of-process host implementing the same contract) — the obligation attaches to
whoever builds that path, not to this spec inventing one to satisfy the row. Should a genuine
second host appear (e.g. an embedded/out-of-process runtime consuming the same `WorkflowFile`
AST), §4.7's `Fixture`/`check_parity` shape already specifies the harness it would run against
with no further design work here.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[AST]` | `crates/nodus/src/ast.rs` | `TestBlock` — the node whose `raw_lines`/structured-field relationship §2 corrects |
| `[PARSER]` | `crates/nodus/src/parser.rs` | `parse_test_block` / `collect_braced_raw_lines` / `parse_test_body` — the triple-scan that defines which pair shapes are recognized (§10.2/§10.3) |
| `[TRANSPILER]` | `crates/nodus/src/transpiler.rs` | `@test:` emission in `to_nodus` — the branch whose ordering §10.4(a) corrects and the site the §10.4(b) quoting rule applies to |
| `[WORKFLOWS]` | `crates/nodus/src/workflows.rs` | `test` / `test_with_tags` / `evaluate_test_block` / `parse_expected_value` — the execution and assertion path (§4–§6) |
| `[VALIDATOR]` | `crates/nodus/src/validator.rs` | `E015` / `W009` / the new `W015` (§7); `w006_route_test_coverage` and the smoke-tag heuristic, the two non-transpiler `raw_lines` readers named in §2 |
| `[CORPUS]` | `crates/nodus/tests/parity.rs` | the normative fixture corpus and its NL-6 round-trip harness — §10.5's acceptance signal is widening its assertion from `.steps` to the whole `WorkflowFile` |

## Document History

| Version | Date | Change |
| --- | --- | --- |
| 1.4.0 | 2026-09-24 | Realization sync (2026-09-24): The test entry points validate before any block runs; `E023` rejects an `input:` key the workflow does not declare and an `expected:` variable that can never exist (NT-9); a block passes only when its assertions hold and the run ended `Ok` (NT-4). NT-2 and NT-9 stay Partial on one point: literal kind is still lost, so an `input:` value is typed from its spelling rather than its field's declared type. |
| 1.3.1 | 2026-09-24 | Consistency pass (2026-09-24): Four spec-vs-code gaps recorded as pending. The test entry points never run the validator, so a workflow carrying `E015` still executes its blocks. An undeclared `input:` key is dropped rather than rejected (NT-9). Literal kind is lost: a quoted value is re-typed from its spelling and the `@in:` declared type is ignored (NT-2 now Partial). A block with non-empty `expected:` passes on `Status::Partial`, contrary to L1 §4.4. §5's parallel-safe-stub paragraph claimed `~PARALLEL` test blocks exercise real concurrent scheduling; branches run sequentially and `~JOIN` binds an empty map (`l2-nodus-runtime` §4.4). |
| 1.3.0 | 2026-07-31 | Closes the NT-11 invariant-traceability gap flagged at the v1.29.0 nodus replan (differential parity, added to `l1-nodus-testing` v1.1.0, had never gained a §8 row). New §11 grounds and resolves the open fork that replan named: does NT-11 realize as vacuous-in-core, or does an in-crate second conforming host make it meaningful? Confirmed by direct inspection of `transpiler.rs` that `to_nodus(ast) -> String` emits source text, not an executable — nodus has exactly one execution path (`Executor`), so "interpreter↔transpiler parity" is not two paths to diff; that pairing's actual correctness property is NL-6 AST-equality (§10.4), a different invariant already realized. Also ruled out reusing `l2-nodus-portability.md` §4.8's LP-3 two-host admission record: that harness proves a satisfying and a non-satisfying host **diverge**, the opposite of NT-11's two-conforming-hosts-**agree** requirement. §8 gains the NT-11 row (Vacuous in core); Related Specifications gains `l1-nodus-portability`/`l2-nodus-portability` cross-references distinguishing the two two-host comparisons. Design-only finding — no code change, since there is no second path to build a harness against; §4.7's `Fixture`/`check_parity` shape stays specified and ready for whenever a real second conforming host exists. |
| 1.2.1 | 2026-07-30 | Discharges the correction queued when Phase 22 was planned: §10.4(a) claimed `raw_lines` "reproduces the body in the form the author wrote it", which overstates. It preserves the author's **token sequence** (including the §10.2 inline form's `{`/`,`/`}`, which the structured fields discard) but not line breaks — `collect_braced_raw_lines` drops `Newline` tokens, so a multi-line body re-emits flat. Clarified that this is sufficient because NL-6 requires AST-equality rather than source identity, and that re-emitting the line breaks would *break* NL-6 by changing `raw_lines` on re-parse. Text-only precision fix; the two-part rule, `W015`, and every §10 conclusion are unchanged, so the implementation shipped in Phase 22 already matches the corrected wording. |
| 1.2.0 | 2026-07-30 | Added §10 (body grammar conformance & NL-6 round-trip) and `W015`. Corrects §2, which described a `raw_lines`/structured-fields split that no parse produces — `raw_lines` is the lexed token stream and the structured fields are a derived view, both always populated; the transpiler's emission branch was written against the non-existent state and so never reached its own `raw_lines` path. §10.1/§10.2 fix the canonical body grammar to L1 §4.1's line-per-pair colon form and record the inline-brace form as tolerated-but-not-canonical. §10.3 resolves the `=`-separator question — not legal (L1 admits only `:`), the defect being that non-conforming pairs were dropped *silently*, letting a declared assertion pass without ever being checked; now `W015`, warning-severity so existing corpus files still parse. §10.4 states the two-part round-trip rule (emit from `raw_lines` when present; re-quote values that would not re-lex to a single equal token), which achieves NL-6 AST-equality without storing quoting in the AST. §1 module map gains `transpiler.rs`. |
| 1.1.0 | 2026-07-04 | Parallel-safe stub (§5): `StubProvider` documented as `Send + Sync`, so `@test:` blocks containing `~PARALLEL` exercise real concurrent branch scheduling with deterministic assertions (input-keyed stub + declared-order `~JOIN`); realizes the NT-5 host extension the language contract permits |
| 1.0.0 | 2026-06-24 | Initial spec — Rust implementation of NT-1…NT-10; types, API, evaluator, validator diagnostics |
