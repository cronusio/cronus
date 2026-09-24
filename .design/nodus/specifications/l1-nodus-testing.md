# Nodus DSL Testing Contract

**Version:** 1.1.2
**Status:** Stable
**Layer:** concept

## Overview

Nodus provides an inline testing facility through `@test:` blocks — named test cases embedded directly in a workflow file. Each block declares an independent invocation of the parent workflow with a custom `input:` override and one or more `expected:` assertions against pipeline outputs. This specification governs the structure, execution semantics, isolation guarantees, and reporting contract for all conforming test runners.

The testing facility is language-level: it is syntax that any conforming implementation must recognise and honour, not a host-specific extension. A workflow's `@test:` blocks are a first-class part of its specification, not a secondary tooling concern.

## Related Specifications

- [l1-nodus-authoring.md](l1-nodus-authoring.md) — [ADDED v1.1.1] the other subject. NT asserts a *workflow's* outputs under fixed inputs; NA exercises *nodus itself* under an author who has not been told what to write, and is the only instrument that can see an intent the language cannot express or a diagnostic that names no recovery. NT-5's provider stubbing is the precedent NA-5 extends; NT-10's route coverage is per-workflow where NA-4's is over the diagnostic taxonomy.
- [l1-nodus-language.md](l1-nodus-language.md) — parent language spec; defines `@test:` block as a section declaration (§4.2)
- [l2-nodus-runtime.md](l2-nodus-runtime.md) — Rust runtime; `Executor`, `RunResult`
- [l2-nodus-testing.md](l2-nodus-testing.md) — Rust implementation of this spec; `TestBlock`, `test()`/`test_with_tags()`, assertion evaluator, NT-1…NT-11 compliance table
- [l1-nodus-observability.md](l1-nodus-observability.md) — observability contract; test runs emit execution events under the same protocol
- [l1-nodus-portability.md](l1-nodus-portability.md) — LP-3 two-host generalisation; NT-11 differential parity is its executable form (one workflow, two conforming paths, equal results)

## 1. Motivation

Workflow correctness is difficult to verify without executable test cases collocated with the workflow definition. Separate test harnesses require maintaining two files in sync, duplicating the `@in:` schema declaration, and relying on conventions that vary across hosts. Inline `@test:` blocks solve these problems:

- Tests are parsed alongside the workflow, so they always reflect the current schema and input contract.
- Assertions are checked against the same runtime that executes the workflow in production.
- Test isolation (NT-1) prevents accidental state sharing while keeping tests co-located.
- Tag-based filtering (NT-6) allows CI and manual runs to select subsets without modifying the source.

## 2. Constraints & Assumptions

- `@test:` blocks are optional; a workflow with no test blocks is valid per NL-4.
- A test block does not alter the workflow's runtime semantics — no step executes differently because a test block exists.
- `expected:` assertions are against workflow output variables, not intermediate step values (unless those values are explicitly bound to named variables via `→ $name`).
- Test blocks may not define new `!!NEVER` / `!!ALWAYS` rules or override the parent `§runtime` schema.
- A conforming test runner must execute each block independently; the execution order of blocks within a file is unspecified but must be deterministic per invocation.
- The testing facility applies only to `§wf:` files; `§schema:` and `§config:` files contain no executable steps and therefore no testable behaviour.

## 3. Core Invariants

Rules that Layer 2 implementations MUST NOT violate:

- **NT-1 Block isolation**: each `@test:` block executes in a fresh context; variable bindings, flags, and log entries from one block must not carry over into another block's execution environment.
- **NT-2 Input override**: the `input:` section within a test block fully overrides the parent workflow's `@in:` values for that test run; fields not specified in `input:` use the field's declared default, or `Null` for optional fields; the original `@in:` declaration remains the type contract.
- **NT-3 Expected assertion binding**: each entry in the `expected:` section names a workflow output variable (an `@out:` binding or any `→ $name` pipeline target) and the value it must hold after the test run; if the named variable is absent from the execution context at assertion time, the assertion fails.
- **NT-4 Assertion failure semantics**: if any `expected:` assertion evaluates to false, the test block fails; other test blocks in the same file continue executing regardless — a failure is local to its block; the overall `TestReport` accumulates pass/fail counts across all blocks.
- **NT-5 Provider neutrality**: model-calling commands (`GEN`, `REFINE`, `TRANSLATE`, etc.) inside a test run receive stub/mock responses from a test-context provider; test blocks must not trigger real external I/O, network calls, or storage writes; any provider injected at test time must satisfy the no-side-effect requirement.
- **NT-6 Tag metadata**: `tags:` is declarative metadata only — it has no effect on the execution or assertion logic of a block; it exists solely to support selection and filtering by a test runner; an absent `tags:` field is equivalent to an empty tag list.
- **NT-7 Ordered reporting**: a `TestReport` contains one `TestResult` entry per `@test:` block, in their declaration order within the file; each `TestResult` carries the block name, a pass/fail flag, and a human-readable diagnostic message.
- **NT-8 Schema inheritance**: test blocks run under the same vocabulary schema that governs the parent workflow (declared in `§runtime: { core: … }`); a test block cannot extend or replace the schema for its own run.
- **NT-9 Parse-time validation**: a `@test:` block that references a variable that cannot exist in the parent workflow (e.g., a name not declared in `@in:`, `@out:`, or any `→ $name` step) must produce a validation error, not a silent assertion-miss at run time; the block fails before execution.
- **NT-10 Route coverage advisory**: a workflow that declares one or more `ROUTE(wf:name)` steps SHOULD have at least one `@test:` block that exercises each routed target; absence of coverage emits the warning-severity diagnostic `W006` (§4.6); this is advisory and does not block execution.

- **NT-11 Differential parity across execution paths**: [ADDED v1.1.0] when a workflow can be executed through more than one conforming path — the reference interpreter and a transpiled realization, or two independent hosts that satisfy the same capability manifest — those paths MUST produce **equal observable results** on a recorded fixture corpus: the same `@out:`/`@err:` bindings and the same trace event *shape* (event types, order, per-step attribution), modulo values a fixture explicitly declares non-deterministic. Parity is verified by recording a fixture (input + config + observed output) from one path and replaying it against the other; the determinism this contract already guarantees (block isolation NT-1, side-effect-free provider NT-5, and the observability contract's deterministic ordering) makes exact-match a viable gate. A divergence is a defect in the diverging path — not a test-authoring problem — and is reported against that path, not the fixture. This is the executable form of the portability two-host generalisation rule (`l1-nodus-portability.md` LP-3): a path is a valid realization only if it is parity-equal to the reference on the corpus.

## 4. Detailed Design

### 4.1 Block structure

```text
[REFERENCE]
@test: {name}
  input:
    {field}: {literal-value}
  expected:
    {variable}: {literal-value}
  tags: [{tag}, …]
```

| Field | Required | Description |
| --- | --- | --- |
| `{name}` | Mandatory | Unique test identifier within the file; used in `TestResult.name` |
| `input:` | Optional | Key-value pairs overriding `@in:` fields for this test run |
| `expected:` | Optional | Assertions: variable name → expected value; absence of `expected:` means the test passes if `Status::Ok` |
| `tags:` | Optional | List of string labels for filtering; no semantic effect on pass/fail |

Multiple `@test:` blocks with the same `name` are a validation error (code `E015`).

### 4.2 Execution protocol

1. **Parse and validate**: the workflow file is parsed once and validated exactly as for an ordinary run; an error-severity diagnostic — a duplicate block name (`E015`) or a block naming something that cannot exist (NT-9) — fails the file before any block executes. A test run never executes a workflow that could not run in production. All `@test:` blocks are then collected from `WorkflowFile.tests`.
2. **For each block** (declaration order):
   1. Build a fresh execution context: empty variable environment, no carry-over from prior blocks.
   2. Apply `input:` overrides: merge block `input:` values over the workflow's declared `@in:` defaults, each value read as its field's declared type (NT-2 keeps `@in:` as the type contract — a value's spelling never decides its type). An `input:` key not declared in `@in:` has no schema slot to bind to; it is the NT-9 case, rejected in step 1, so it never reaches the merge — dropping it here instead would run the block on the field's default while the author believes the override applied.
   3. Execute the full `@steps:` body against the overridden inputs using a side-effect-free provider (NT-5).
   4. Evaluate each `expected:` assertion against the final execution context (NT-3).
   5. If all assertions pass and `Status::Ok`, mark the block passed. If any assertion fails or `Status` ≠ `Ok`, mark the block failed with a diagnostic message naming the first failing assertion.
3. **Aggregate**: collect all `TestResult` entries into a `TestReport` (NT-7). Return the report regardless of individual failures — all blocks run (NT-4).

### 4.3 Assertion semantics

An `expected:` entry asserts strict equality between the variable's value and the declared literal. The equality check uses the same semantics as the `Value` type system (NL-7): `Text`, `Int`, `Float`, `Bool`, `Null`, `List`, and `Map` equality are structurally recursive. Type mismatch is an assertion failure, not an error.

```text
[REFERENCE]
@test: smoke
  input:
    query: "hello"
  expected:
    $out: "hello"         — passes if Value::Text("hello") equals run output $out
    $confidence: 0.9      — passes if Value::Float(0.9) equals $confidence
  tags: [smoke, unit]
```

### 4.4 TestReport contract

```text
[REFERENCE]
TestReport {
    results: Vec<TestResult>,  — one entry per @test: block, declaration order
    passed:  usize,            — count of TestResult entries where passed = true
    failed:  usize,            — count of TestResult entries where passed = false
}

TestResult {
    name:    String,           — @test: block name
    passed:  bool,             — true iff all assertions passed and Status::Ok
    message: String,           — human-readable outcome: first failing assertion, or "ok"
}
```

`TestReport.passed + TestReport.failed == TestReport.results.len()` is a structural invariant.

An empty `results` vector (no `@test:` blocks in the file) is a valid report with `passed = 0`, `failed = 0`.

### 4.5 Tag-based filtering

A test runner MAY accept a tag predicate at call time (e.g., `tags: ["smoke"]`) and skip blocks whose `tags:` list does not satisfy the predicate. When a block is skipped by tag filtering, it must NOT appear in the `TestReport.results` list — the report reflects only executed blocks. An empty tag list on a block matches a "run all" predicate.

### 4.6 Validation diagnostics

| Code | Severity | Trigger |
| --- | --- | --- |
| `E015` | Error | Duplicate `@test:` name within the same file |
| `W006` | Warning | `ROUTE(wf:x)` step with no `@test:` block exercising it (NT-10); code pre-exists and is re-used here |
| `W009` | Warning | `@test:` block with no `expected:` section (block will pass trivially on `Status::Ok`) |

### 4.7 Differential Parity Harness [ADDED v1.1.0]

Parity (NT-11) is a *harness-level* check that reuses the same fixtures the inline
`@test:` blocks describe, run through two paths and diffed:

```text
[REFERENCE]
Fixture := { workflow, input, config, output, trace_shape, recorded_at, input_sha256 }

record(path_A, workflow, input, config):                 // capture from the reference path
    result := run(path_A, workflow, input, config)
    return Fixture{ …, output: result.vars, trace_shape: shape(result.events) }

check_parity(fixture, path_B):
    result := run(path_B, fixture.workflow, fixture.input, fixture.config)
    if result.vars != fixture.output:            return Diff(vars, …)      // NT-11 output parity
    if shape(result.events) != fixture.trace_shape: return Diff(trace, …)  // NT-11 trace parity
    return Match
```

Properties: fixtures are content-addressed (`input_sha256`) so a changed input is a
new fixture, never a silent overwrite; a comparator is per-path and reports the first
divergence with both sides; declared non-deterministic fields are excluded from the
diff rather than making parity impossible. The corpus is normative — the same
`crates/nodus/tests/fixtures/` corpus the inline runner uses — so interpreter and
transpiler are held to one ground truth. Parity does **not** replace `@test:` blocks
(which assert *correctness* against authored expectations); it asserts *equivalence*
between realizations of an already-correct workflow.

## 5. Drawbacks & Alternatives

- **Inline vs. external test files**: external test files (e.g., `*.test.nodus`) allow larger test suites without growing the workflow source. Rejected for the base contract because co-location eliminates synchronisation drift and external files would need to import the workflow schema — reintroducing the coupling problem inline tests avoid. External suites are a host-level extension, not a language invariant.
- **Assertion language expressiveness**: `expected:` supports only strict equality. Range checks, regex matches, and partial-map assertions require a richer DSL. Deferred: the base contract covers the 80 % case (smoke tests, contract tests); richer assertions are an amendment to this spec.
- **Parallel block execution in tests**: test blocks that contain `~PARALLEL` steps run their parallel body sequentially in the stub provider context (NT-5). Parallel scheduling behaviour is not testable within the base contract; host implementations that support real parallelism may extend NT-5 with a parallel-safe stub.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[LANG]` | `crates/nodus/src/ast.rs` | `TestBlock` AST node definition |
| `[RUNNER]` | `crates/nodus/src/workflows.rs` | `test()` public API, `TestReport`, `TestResult` |
| `[PARSER]` | `crates/nodus/src/parser.rs` | `parse_test_block()` — how `@test:` body is tokenised |
| `[FIXTURES]` | `crates/nodus/tests/fixtures/` | Canonical sample workflows (normative test corpus) |

## Document History

| Version | Date | Change |
| --- | --- | --- |
| 1.1.2 | 2026-09-24 | Consistency pass (2026-09-24): NT-10 still named `W001` — the code §4.6 already corrected to `W006` in 1.0.1 (`W001` is the error-handler warning). §4.2 step 2 let an undeclared `input:` key be ignored silently, contradicting NT-9 (a name not declared in `@in:` is a validation error, never a silent miss) — such a key is now rejected at validation, and each value is read as its field's declared type (NT-2). §4.2 step 1 now validates before any block runs, so a test run never executes a workflow that could not run in production. Related Specifications counted NT-1…NT-10. |
| 1.1.1 | 2026-09-11 | Patch — cross-reference to `l1-nodus-authoring`, the free-route instrument whose subject is nodus rather than a workflow: NT tests what an author wrote, NA tests whether it could be written and whether the diagnostics taught. Documentation linkage only; no invariant added or changed. |
| 1.1.0 | 2026-07-02 | Added NT-11 (differential parity across execution paths — interpreter↔transpiler or two conforming hosts must produce equal observable results, output + trace shape, on a recorded fixture corpus; the executable form of portability LP-3) and §4.7 differential-parity harness (content-addressed fixtures, per-path comparators, declared-non-determinism exclusion, shared normative corpus). Parity asserts equivalence between realizations, complementary to `@test:` correctness assertions. Related Specifications extended with l1-nodus-portability. |
| 1.0.1 | 2026-06-24 | §4.6: corrected diagnostic codes — W006 (pre-existing ROUTE coverage) replaces spec-proposed W001; W009 (new no-expected advisory) replaces spec-proposed W002 to avoid conflict with existing validator codes |
| 1.0.0 | 2026-06-24 | Initial spec — NT-1…NT-10 invariants, block structure, execution protocol, assertion semantics, TestReport contract, tag filtering, validation diagnostics |
