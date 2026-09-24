# Nodus Runtime (Rust)

**Version:** 1.5.2
**Status:** Stable
**Layer:** implementation
**Implements:** l1-nodus-language.md

## Overview

Concrete Rust implementation of the Nodus DSL: a self-contained crate (`crates/nodus`) comprising a lexer, parser (→ AST), validator (lint), executor, and transpiler. Vendored in the Cronus monorepo; the crate boundary keeps future extraction to a standalone repository low-cost. The current built-in schema version is **v0.4.7** (`BUILTIN_SCHEMA_VERSION`).

## Related Specifications

- [l1-nodus-language.md](l1-nodus-language.md) — Language invariants this crate implements
- [../../main/specifications/l2-workflow-runtime.md](../../main/specifications/l2-workflow-runtime.md) — Cronus integration layer (subsystem dispatch, step binding, platform constraints)
- [l2-nodus-error-dispatch.md](l2-nodus-error-dispatch.md) — [ADDED v1.3.1] realizes NL-9's `@err:` handler-dispatch half, attaching to this spec's main step loop (§4.4)

## 1. Motivation

A Rust implementation links in-process with any host (desktop, mobile, server) without a separate language process. Rust's type system enforces the closed value type system (NL-7) at compile time, and `std`-only dependencies keep the crate embeddable on constrained targets. The `ModelProvider` trait decouples the language runtime from any specific AI provider.

## 2. Constraints & Assumptions

- No runtime dependencies beyond `std`; model integration is injected via the `ModelProvider` trait.
- `StubProvider` ships with the crate for tests and early development; real providers are wired at the host layer.
- Schema and grammar are data, not code: `BUILTIN_SCHEMA_VERSION = "0.4.7"` is the baseline; adding a command requires updating `vocab.rs` and bumping the version constant.
- `Map` values use `Vec<(String, Value)>` (not `HashMap`) to preserve declaration order across transpiler round-trips (NL-6).

## 3. Invariant Compliance

| L1 Invariant | Implementation |
| --- | --- |
| NL-1 Schema-first | **Partial.** The schema is loaded before validation, but an unknown command is neither a validation error nor a runtime one. The lexer types an ALL_CAPS word outside the schema as an identifier, and the step parser keeps a line it cannot parse as a `Stmt::Comment` — so a misspelled or unregistered command silently drops out of the run, which completes `Ok` without it. `E002` is an ordering check the parser guarantees, not an unknown-command diagnostic, and the registered `NODUS:UNDEFINED_CMD` has no emission site. Pending: a step line whose leading word is not a schema command is a blocking diagnostic. |
| NL-2 Hard constraints absolute | `Executor` internalizes `!!` rules in boot step 2; a violation pushes `RULE_VIOLATION` and returns `Signal::Break`, forcing `Status::Failed` — caught by the main loop's `Signal::Break` match arm, so it never reaches the `@err:` dispatch check (`l2-nodus-error-dispatch.md` §4.1) [CORRECTED: previously named a nonexistent `Status::RuleViolation` variant; the real variant is `Status::Failed`] |
| NL-3 Soft preferences advisory | `!PREF` rules are parsed and kept apart from `!!` rules, and `!OVERRIDE` parses as a per-branch flag. The executor consumes neither: boot step 3 records no preference context and no command reads one, so a `!PREF` has no effect on a run today — advisory in the strongest sense, and never able to override a hard rule (NL-2). |
| NL-4 Validate-before-run | `run()` and `run_with_provider()` call `Validator::validate` first; any `has_errors = true` returns `Err(diagnostics)` before `Executor::execute` is reached. Every `run*` entry point validates first; the test entry points do not — `test`/`test_with_tags` parse and execute without validating (`l2-nodus-testing.md` §4, pending) — and `Executor::execute` is public, so a host holding a parsed AST can execute it without validation at all. NL-4 calls that an implementation error; the crate leaves it reachable. |
| NL-5 Bounded loops | Validator checks every `~UNTIL` AST node for a `MAX:n` bound; absence emits `Severity::Error` code `E010` |
| NL-6 Dual representation | `Transpiler::to_nodus` (compact, lossless) and `Transpiler::to_human` (one-way prose); round-trip test in `workflows.rs` verifies AST equality after compact re-parse |
| NL-7 Closed value types | `executor::Value` is a closed Rust enum: `Null / Bool / Int / Float / Text / List / Map` — extension requires a source change and a breaking version bump |
| NL-8 Reserved namespace | `vocab::RUNTIME_OWNED_VARIABLES` (10-element subset of `RESERVED_VARIABLES`, `$restart_count` included); validator emits `Severity::Error` code `E013` if a pipeline target (`→ $name`) names a runtime-owned variable. **Partial:** the guard covers pipeline targets only. Boot step 4 copies every key of the caller's input map into the variable environment, so an input naming `user`, `session`, `ctx`, `memory`, `kb_results`, `flags`, `error` or `meta` sets that runtime-owned variable before step 1 — the reserved seeding inserts only when absent and does not overwrite it. Where input can originate outside the host (a trigger payload, a channel message), a workflow branching on `$user` can be handed a forged identity. Pending: boot binds caller input to declared `@in` fields only and rejects a runtime-owned name. |
| NL-9 Typed I/O contract | **Partial — the input half is unenforced.** NL-9 requires input presence and type to be enforced at run time, and neither is. Nothing checks that a required `@in` field (no `?` suffix, no default) was supplied — `E009` checks the declaration, not the call — so a missing input simply resolves as absent inside the run; and no field's declared `type_name` is read at run time, so a value of any kind binds to any field. Declared defaults are bound as `Text` whatever the field's type, while the test runner re-types the same default from its spelling (`l2-nodus-testing.md` §6), so a test and a production run of one workflow can see differently typed inputs. [CORRECTED v1.5.2 — this row previously claimed the executor verified required inputs before step 1; no such check exists.] The `@err:` handler-dispatch half of NL-9 is **implemented [v1.3.2]** by `l2-nodus-error-dispatch.md` (Phase 26): any non-fatal error a step returns with no `Signal` reaches a dispatch check in the main loop, `$error` is populated, and the declared handler runs via the ordinary `execute_command` path before the run ends. With no handler declared the run keeps executing instead of terminating with `NODUS:UNHANDLED_ERROR` (`l2-nodus-error-dispatch.md` §4.5, pending) |
| NL-10 Sequential pipeline | AST `steps` field is `Vec<Step>` in declaration order; validator emits `Severity::Error` code `E014` when a variable is referenced before any prior step declares it via `→ $name` |

### 3.1 Invariants added after v1.0.0 — realization status [ADDED v1.3.0, EXTENDED v1.4.0, EXTENDED v1.5.0]

`l1-nodus-language.md` has grown NL-11 … NL-28 through additive refinement since
this spec's table was written (NL-24 … NL-26 added at v1.4.0, from the wave that
closed at NL-23 when this section was first written). Each of those invariants' Document History names
*this* spec as the carrier of its pending obligation, so they are listed here
explicitly: an obligation tracked only in a plan document is not tracked in the
place its own contract points to. `Realized by` names the spec that defines the
Rust shape; `pending` means no such spec exists yet, so no verifiable task can be
authored against it.

| L1 Invariant | Realization status |
| --- | --- |
| NL-11 Provenance-safe interpolation | **Pending** — no L2 realization spec. A phase-scoped subset landed with `§config` secret-omission (`l2-nodus-config`), but crate-wide `Value` provenance tagging remains an open system-wide obligation. |
| NL-12 Deferred / external step execution | **Pending** — no L2 realization spec. The `Status::Paused` + `ResumeDescriptor` lifecycle exists (`l2-nodus-dialog`) but is human-answer-shaped; generalizing the resume trigger to any host-supplied completion is unrealized. Named as a dependency by NL-22's resumability rule. |
| NL-13 Competitive parallel selection (`~PARALLEL +select` / `~PICK`) | **Pending** — no L2 realization spec. |
| NL-14 Grader-gated iterative refinement (`~UNTIL +grade`) | **Pending** — no L2 realization spec. |
| NL-15 Cache-stable prompt composition | **Pending** — no L2 realization spec. |
| NL-16 Selective vocabulary disclosure (`@needs`) | **Pending** — no L2 realization spec; additionally blocked on external-schema loading, which does not exist. |
| NL-17 Origin-taint provenance | **Pending** — no L2 realization spec; layers on the unrealized NL-11. |
| NL-18 Bounded recursive decomposition (`RUN` + `MAX_DEPTH:n`) | **Pending** — no L2 realization spec; additionally blocked on `RUN(@macro)` body expansion, which is unimplemented. |
| NL-19 Unforgeable frame markers & seam-anchored segmentation | **Pending** — no L2 realization spec. |
| NL-20 `§config` declarative configuration surface | **Realized by** [l2-nodus-config.md](l2-nodus-config.md) — implemented. |
| NL-21 Confidentiality-flow labeling | **Pending** — no L2 realization spec. |
| NL-22 Compensation seam | **Realized by** [l2-nodus-compensation.md](l2-nodus-compensation.md) — specified; awaiting its implementation phase. |
| NL-23 Bounded whole-run self-restart | **Realized by** [l2-nodus-restart.md](l2-nodus-restart.md) — specified; awaiting its implementation phase. |
| NL-24 Attested execution mode [ADDED v1.4.0] | **Partially realized — and what is missing is the record, not the behaviour.** `Executor::execute_parallel` is a plain `for branch in &pb.branches { self.execute_node(…) }`: every `~PARALLEL` block runs its branches one after another, unconditionally, and LP-1's zero-dependency constraint means the crate contains no scheduler, pool or thread facility that could ever do otherwise. Three of the four rules follow from that. **(a) Realized, never declared** is satisfiable trivially, because the realized mode is a constant. **(b) No attestation without per-branch launch identity** is satisfied vacuously — with no host concurrency facility the attested value can never legitimately be *concurrent*, and the honest third value *host-reported* has no producer either. **(c) Determinism unaffected** is satisfied structurally: NL-7's value space is closed and no `Value` kind is mode-shaped, so flow logic cannot branch on the mode even once it is recorded. What is absent is **(d)** and the record itself — no event, annotation or manifest field states a per-block execution mode, and `observability.rs` does not mention parallelism at all. So the trace does not over-report concurrency (a checkable negative that holds today) but it does not report anything either, and a reader cannot distinguish *"sequential, because this runtime always is"* from *"nothing was recorded"*. **Correction [gap is larger than a carrier choice] — `~PARALLEL` has no emission of its own to carry anything on.** `execute_node`'s dispatch (`executor.rs`) wraps only `Stmt::Command` with `StepStart`/`StepEnd` (inside `execute_command`); `Stmt::Parallel`/`Conditional`/`ForLoop`/`UntilLoop`/`Switch`/`Map` all recurse straight into their bodies with no event of their own — a `~PARALLEL` block itself never appears in the trace as a discrete unit, only its branches' individual commands do. So `EventAnnotations` has no event to attach a per-block mode *to* (there is no "the block's emission"), and `RunManifest` cannot hold a per-block record in its current flat-field shape without a `Vec<(block_identity, ExecutionMode)>`-style addition, which in turn needs a stable per-block identity the crate has no source for today (`step_identity`, HO-15, is defined per-*step*, not per-block). Closing NL-24 therefore is not "pick `EventAnnotations` or `RunManifest`" — it needs one of: (i) a new per-block identity scheme plus a `RunManifest` collection field, or (ii) extending step-level event wrapping to non-`Command` `Stmt` variants generally, a materially larger change to the executor's emission model that would affect every construct's trace shape, not only `~PARALLEL`'s. Neither is a small residue; NL-24 stays **Partially realized** with (d) and the record itself as the open, and now more precisely scoped, gap. Standing consequence for NL-13: `first-success` resolving to declaration order under sequential execution is a materially different contest from a race, and until the record exists no trace can say which one happened. |
| NL-25 Generated-value branch contract [ADDED v1.4.0] | **Pending** — no L2 realization spec, and unusually the pre-structured special case its L1 leans on is absent too: NL-14's `+grade` verdict, which would satisfy (a) anchoring and (b) totality by construction, has no realization anywhere in the crate (`grade` occurs in no source file), so it cannot be used as the escape hatch. The gap is concrete at two shipped sites rather than theoretical. `execute_switch` resolves its scrutinee and compares whole `Value`s for equality, so an **anchored** read of a declared position is not expressible and a branch over a `GEN` result compares against the entire generated text — the scan-shaped behaviour (a) forbids. And its no-match path falls through to the `*` default when one exists and otherwise records `SWITCH_NO_MATCH` as a **warning, then continues**: both outcomes are exactly what (b) forbids — a default arm nobody authored and a silent miss — where the invariant requires a typed `NODUS:*` extraction error routed to `@err:`. That routing is now real ([l2-nodus-error-dispatch.md](l2-nodus-error-dispatch.md)), so the error half has a landing place it lacked one wave ago, which makes NL-25 nearer than its Pending verdict suggests. Rule (d) — a tainted value must not select an arm whose effects exceed what its origin would be granted directly — inherits the NL-11 and NL-17 blockers above and cannot precede them. |
| NL-26 A suspended external step declares its bound [ADDED v1.4.0] | **Partially realized — and, as of this row's correction, independent of `ResumeDescriptor`'s field shape.** The dialog case is discharged: DG-5's `+timeout` yields `DialogOutcome::Timeout` and a typed `NODUS:DIALOG_TIMEOUT` routed to `@err:` ([l2-nodus-dialog.md](l2-nodus-dialog.md) §3), which is precisely the behaviour NL-26 says a general external step must copy. The general case is **vacuous in core**, blocked on NL-12 alone: NL-12 generalized deferred execution is **Pending** above, so there is no non-dialog external step whose completion could fail to arrive. **Corrected reasoning for why the residue is vacuous, not merely unimplemented**: NL-26's own rule presupposes an *asynchronous* boundary — a correlation token that a completion can arrive against *after* the step has moved on, which the elapsed deadline then invalidates. `DialogProvider::ask`/`confirm` (`crate::executor::DialogProvider`) is **synchronous** — the entire interaction resolves, one way or another, within one call, and returns before the step continues — so there is no "later" for a late completion to occur *in*. A correlation token has nothing to correlate against until NL-12 gives a step a suspend/complete boundary that actually spans two separate calls into the crate; adding one to today's dialog path would be inert data, the exact "seam declared, wiring pending" trap this workspace has already hit twice (LP-11 pre-Phase-24, LP-15's `store`/`load`). **Retraction**: a prior version of this row proposed the correlation token as a field alongside LP-22 (c)'s pinned generation on the same `ResumeDescriptor`, framing both as one decision. They are not one decision — LP-22 (c) is now fully designed ([l2-nodus-dialog.md](l2-nodus-dialog.md) §4.3 v1.3.0, `workflow_digest`) and needed no correlation-token question at all; NL-26 remains blocked strictly on NL-12's own design, unrelated to any field on `ResumeDescriptor`. See [l2-nodus-portability.md](l2-nodus-portability.md) §3.2's matching correction. |
| NL-27 Exclusive bindings displaced explicitly [ADDED v1.5.0, UPDATED Phase 33] | **Partially realized — two of the three live-and-violated classes are now closed; one live class, one live-and-inverted class, two vacuous classes and the entire stated-displacement half remain open.** NL-27 names six classes of name admitting exactly one holder. **Closed (Phase 33).** *`§config` fields* (NL-20): `check_config_values` now scans `decl.fields` for repeated names **before** the accept loop runs and rejects the set with a new `ConfigReason::DuplicateField` — one violation per repeated name — so a field declared twice (one `secret`, one not) can no longer reach `AcceptedConfig` at all, closing the value/secrecy accessor-disagreement path this row previously named. *Macro names*: a new `e020_no_duplicate_macro_names` validator rule (modelled directly on `e015_no_duplicate_test_names`) rejects a workflow declaring two `@macro` blocks of one name. Both are blocking (`Severity::Error`), per NL-27's own wording that a second declaration **is a validation error**, not a warning. **Still live and violated (one class).** *Error-dispatch handlers*: `error_decl` is an `Option<ErrorDecl>` (`ast.rs`) and the parser's header loop assigns `wf.error_decl = Some(self.parse_error_decl())` (`parser.rs`) — a second `@err:` block **silently overwrites the first**, which is last-write-wins exactly as NL-27 describes it: costs nothing to implement, undiagnosable to read. Left open deliberately: `Parser::parse` returns `Result<WorkflowFile>` with no advisory channel a validator-style scan could report through, so closing this needs either retaining the second occurrence in the AST or promoting it to a hard parse error — the second changes acceptance of workflows that parse today, which is a design decision this phase's Scope note declined to make unilaterally. **Still live and inverted (one class).** *Host vocabulary* (LP-4): `Schema::with_provider` (`vocab.rs`) filters out host commands and reserved variables that collide with the builtin constants, so the failure is not silent *replacement* but silent *discard*. Its doc comment frames this as preserving LP-4 (builtin constants are never mutated), which it does; what it does not do is tell the host its declaration did nothing, which is NL-27's phantom-declaration half (the LAY-4 composition) seen from the other side. There is also no dedup *within* `host_commands`. Left open: `Schema::with_provider` returns a bare `Schema` with nowhere to carry a discard report — closing this is a shape decision about the vocabulary-extension surface, not an added check. **Still vacuous (two classes).** *Imported bundle units* (LP-13): `extends: Vec<String>` is parsed and transpiled but never resolved — `validator.rs` still carries the literal line `// W010: extends resolve filesystem check — deferred` — so nothing is ever imported and no second holder can arrive. *Host-contributed flags / validators / types*: `is_known_flag`, `is_known_validator` and `is_known_type` read the builtin constants alone, and `SchemaProvider` contributes only commands and reserved variables, so those three registries have no second contributor by construction (see `l2-nodus-registries.md` §3, which owns this half). **What remains.** The *stated-displacement* half is untouched by Phase 33 and is larger than any detection pass: the grammar has no form in which a declaration names the holder it removes, so two visible acts rather than one invisible one cannot currently be written, and the effective-binding-set report that names what was displaced by whom has no surface to be reported on — a language change, not an added check. |
| NL-28 A validation result declares what it did not check [ADDED v1.5.0] | **Pending — and the obstacle is the return shape, not a missing check.** `Validator::validate(ast, filename) -> Vec<Diagnostic>` returns a flat diagnostic list, so a clean result is an empty `Vec` and there is no field in which coverage could be stated: *this surface is valid* and *the part the validator reaches is valid* are literally the same value, which is the conflation NL-28 exists to break. The crate already demonstrates the failure in the precise form the invariant names — a genuinely unchecked region recorded **apart from the verdict**: `validator.rs`'s check registry carries the line `// W010: extends resolve filesystem check — deferred`, so `extends:` targets are never resolved and nothing in any result says so. NL-28's wording for this is that a coverage boundary documented elsewhere is a boundary nobody reads at the moment it matters; here it is a source comment, read by whoever edits the validator and by no one who runs it. Three regions are unchecked today and each is a different one of NL-28's own categories. *Later-resolved imports* (LP-13): `extends`, above. *Host-supplied schema regions* (LP-4): `Schema::is_command` admits a host command by **name only** — the crate holds no flag grammar for host commands, so their arguments are unvalidated by construction, and this is the one region where NL-28's authority-and-schedule clause is expressible today (the host owns it, at effect time). *A second validation surface entirely*: `check_config_values` returns `Result<AcceptedConfig, Vec<ConfigViolation>>` and likewise carries no coverage, and NL-28 binds every validation surface rather than the main one. **What closing it needs**: a return-type change on both surfaces — a verdict paired with a named set of unreached regions and their owning authority — plus the per-region wiring to populate it. No new *check* is required, and determinism (NL-6) is untouched, since coverage describes the check and never the run. |

## 4. Detailed Design

### 4.1 Module structure

```text
[REFERENCE]
crates/nodus/src/
├── lib.rs           — public re-exports and crate-level doc
├── error.rs         — Error enum, Result alias, Span (line:col)
├── lexer.rs         — tokenizer → Vec<Token>
├── parser.rs        — recursive-descent parser → WorkflowFile AST
├── ast.rs           — all AST node types (FileType, FileHeader, WorkflowFile, Stmt, …)
├── vocab.rs         — Schema, KNOWN_COMMANDS, RESERVED_VARIABLES, RUNTIME_OWNED_VARIABLES, VALID_TONES, error_code
├── validator.rs     — Diagnostic, Severity, Validator
├── executor.rs      — Value, Executor, ModelProvider trait, StubProvider, RunResult, Status
├── observability.rs — AuditProvider trait, ExecutionEvent (10 variants), NoopAuditProvider, RunManifest, FieldDescriptor, RunStatus, LoopType
├── portability.rs   — SchemaProvider trait, BuiltinSchemaProvider; StorageProvider + NoopStorageProvider (pending LP-3); PolicyProvider + NoopPolicyProvider (pending LP-3)
├── transpiler.rs    — Transpiler (to_nodus / to_human)
└── workflows.rs     — public library API (scaffold / validate / run / transpile / test / run_with_provider / run_with_audit / run_with_provider_and_audit / run_with_schema / run_with_schema_and_audit)
```

### 4.2 AST node summary

| Node | Key fields |
| --- | --- |
| `WorkflowFile` | `header`, `runtime`, `triggers`, `rules`, `prefs`, `inputs`, `outputs`, `ctx`, `err`, `steps`, `tests`, `macros` |
| `FileHeader` | `file_type` (Workflow/Schema/Config), `name`, `version` |
| `CommandCall` | `name`, `args`, `modifiers`, `validators`, `flags`, `pipeline_target` |
| `Conditional` | `condition`, `action`, `elif_branches`, `else_branch`, `break_flag`, `skip_flag`, `override_flag` |
| `ForLoop` | `variable`, `collection`, `body` |
| `UntilLoop` | `condition`, `max_iterations`, `body` |
| `ParallelBlock` | `body`, `join_target` |
| `AbsoluteRule` | `kind` (Never/Always), `text` |
| `Preference` | `preferred`, `over`, `condition` |

### 4.3 Value type system

```text
[REFERENCE]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    List(Vec<Value>),
    Map(Vec<(String, Value)>),  // ordered Vec, not HashMap — preserves declaration order
}
```

### 4.4 Executor boot sequence

1. Load schema from `§runtime.core` (validates against `BUILTIN_SCHEMA_VERSION`).
2. Internalize `!!` absolute rules — checked before every command; a violation pushes `NODUS:RULE_VIOLATION` and ends the run as `Status::Failed` (§3, NL-2).
3. `!PREF` soft preferences are noted only; no advisory context is built yet (§3, NL-3).
4. Register `@in` defaults, then overlay the caller's input — every key, declared or not (§3, NL-8). A missing required input is not detected (§3, NL-9).
5. `@ON` trigger matching is left to the host; the executor performs none.
6. Execute `@steps` sequentially; thread `→` pipeline targets between steps.

<!-- [CORRECTED] v1.5.2 -->
#### `~PARALLEL` branch execution

The language defines `~PARALLEL` as a scheduling hint, and a runtime may run the branches one after another. That is what ships: `execute_parallel` runs the branches in declared order over the shared value environment, with no thread, scheduler or width bound (LP-1 leaves the crate no concurrency facility — NL-24's row in §3.1). The v1.2.0 text of this section described scoped-thread concurrency, snapshot isolation, a declared-order merge and fail-fast; none of it shipped.

Two gaps against `l1-nodus-language.md` §4.4 are live, not merely unscheduled:

- **No fail-fast.** Each branch's returned signal is discarded, so an error, `!BREAK`, `!HALT` or `!PAUSE` inside a branch does not end the block — the remaining branches run and the step continues. L1 requires a sequential runtime to apply the same fail-fast semantics: `~JOIN` bypassed and the error forwarded to `@err:`. A `!PAUSE` inside a branch records its suspension point, but the signal that would suspend the run is dropped.
- **`~JOIN` collects nothing.** The join target is bound to an empty map; branch results reach later steps only through the variables each branch wrote into the shared environment. L1 defines `~JOIN → $target` as collecting every branch's result.

Closing both is executor work with no new dependency: propagate the first branch signal (bypassing the join) and bind the join target to the branches' results in declared order.

### 4.5 Public library API

| Function | Signature summary | Contract |
| --- | --- | --- |
| `scaffold` | `(name: &str) -> WorkflowFile` | Returns a minimal valid AST (one `GEN` step); always succeeds |
| `validate` | `(source, filename) -> Result<ValidationReport>` | All diagnostics regardless of severity; `has_errors` true on any `Error`-severity finding |
| `run` | `(source, filename, input?) -> Result<RunResult, Vec<Diagnostic>>` | Fast-fail on validation errors (NL-4); uses `StubProvider` |
| `transpile` | `(source, mode: TranspileMode) -> Result<String>` | `Compact` = lossless round-trip; `Human` = one-way prose |
| `test` | `(source, filename) -> Result<TestReport>` | Executes all `@test:` blocks; aggregates `passed`/`failed` counts. Does not validate first (pending — `l2-nodus-testing.md` §4) |
| `run_with_provider` | `(source, filename, input?, provider) -> Result<RunResult, Vec<Diagnostic>>` | Like `run` but with injected `ModelProvider` |
| `run_with_audit` | `(source, filename, input?, audit, run_id, started_at) -> Result<RunResult, Vec<Diagnostic>>` | Like `run` but with injected `AuditProvider`; forwards run metadata to the manifest |
| `run_with_provider_and_audit` | `(source, filename, input?, provider, audit, run_id, started_at) -> Result<RunResult, Vec<Diagnostic>>` | Full injection: custom `ModelProvider` + `AuditProvider` |
| `run_with_schema` | `(source, filename, input?, schema_provider) -> Result<RunResult, Vec<Diagnostic>>` | Extends the builtin vocabulary from `schema_provider` before parse; host-declared commands lex and execute without error |
| `run_with_schema_and_audit` | `(source, filename, input?, schema_provider, audit, run_id, started_at) -> Result<RunResult, Vec<Diagnostic>>` | Schema extension + `AuditProvider`; run metadata forwarded to the manifest |

The table lists the original entry points. Later specifications add `run_with_manifest`, `run_with_dialog`, `run_with_policy`, `run_with_settlement`, `run_with_environment` and `run_with_config`, each with an `_and_audit` twin and each specified by the spec that owns its seam. The extension seams are `ModelProvider`, `AuditProvider`, `SchemaProvider`, `DialogProvider`, `PolicyProvider`, `SettlementRail`, `EnvironmentProvider` and `ConfigProvider`; `StorageProvider` is declared but not wired. Any host may supply implementations; the built-ins (`StubProvider`, `NoopAuditProvider`, `BuiltinSchemaProvider`, `DefaultDialogProvider`, `NoopPolicyProvider`, `NoopSettlementRail`) cost nothing.

Three properties of this surface are recorded as pending defects, because each lets a run report something other than what happened:

- **Seams do not compose.** Each entry point injects one seam and fixes the rest. Only `run_with_provider*` accepts a real `ModelProvider`, and it fixes `NoopPolicyProvider`, which permits everything; every other entry point — schema, manifest, dialog, policy, settlement, environment, config — runs on `StubProvider`. So no public path runs real model calls under a host policy gate (LP-11), a host vocabulary (LP-4), a capability manifest (LP-8) or a settlement rail (LP-17): the gate that exists governs stub calls only. Closing it needs one entry point that takes every seam at once — a run-options value with the built-ins as defaults — and validates first like every other `run*`.
- **Stub runs are recorded as real.** The stub entry points substitute `StubProvider` for every model call yet record the manifest's default `ExecutionMode::Real`, although HO-12 reserves `Real` for a run whose providers were not substituted (`l2-nodus-observability.md` HO-12 row). Only the output text's own `[STUB …]` prefix marks it. A stub entry point is to declare `Simulated { fidelity: Structural }`.
- **The dispatcher is a stub interpreter, and a provider cannot report failure.** Only `GEN` and `ANALYZE` reach the `ModelProvider`. `VALIDATE` returns `true` unconditionally; `PUBLISH`, `NOTIFY`, `STORE` and `REMEMBER` report success with no effect; `SCORE` returns a constant; and a host-declared command dispatches to a no-op that only raises an `UNKNOWN_COMMAND:<name>` flag. There is no command-execution seam, so a host can extend the vocabulary but cannot give the new commands behaviour. `^validator`s are never evaluated, so `!!NEVER: publish WITHOUT validate` proves that `VALIDATE` ran, never that anything passed, and eighteen of the twenty-four canonical error codes have no emission site (`l2-nodus-errors.md` §4.5). And `ModelProvider::generate` returns `String` (`analyze` returns `Value`): a host whose model call fails can only return partial or empty text, so the step completes `Ok`, `$error` is never populated and `@err:` never runs — a failed generation is indistinguishable from a short one. A fallible provider surface, whose error the executor maps to a typed `NODUS:*` code routed through `@err:` dispatch, closes this; the change is breaking for every provider, so it is recorded here rather than patched in a host bridge.

### 4.6 Vocabulary schema (v0.4.7)

| Category | Commands |
| --- | --- |
| Memory & knowledge | `REMEMBER`, `RECALL`, `FORGET`, `QUERY_KB` |
| Generation | `GEN`, `REFINE`, `TRANSLATE`, `SUMMARIZE`, `FILL`, `GENERATE_DOC` |
| Analysis | `ANALYZE`, `SCORE`, `COMPARE`, `EXTRACT`, `FILTER`, `PARSE`, `PARSE_MD_HEADER`, `PARSE_INDEX` |
| I/O & messaging | `FETCH`, `STORE`, `LOAD`, `APPEND`, `MERGE`, `PUBLISH`, `NOTIFY`, `LOG` |
| Routing & control | `VALIDATE`, `ROUTE`, `ESCALATE`, `WAIT`, `TONE`, `DEBUG` |
| Execution | `EXECUTE`, `SIMULATE`, `EXECUTE_TEST`, `TRANSPILE` |
| Filesystem | `WRITE`, `READ_FILE`, `MKDIR`, `FILE_EXISTS`, `SCAN_DIR`, `MOVE`, `COPY` |
| System & meta | `ENV`, `DATE`, `COUNTER`, `GIT`, `QUERY_GIT`, `HASH`, `VERSION_BUMP` |
| Meta-command | `RUN` |
| Human interaction | `ASK`, `CONFIRM` (`l2-nodus-dialog.md`) |
| Settlement | `SETTLE` (`l2-nodus-settlement.md`) |

Tone values (`+tone=` modifier): `warm` · `neutral` · `formal` · `casual` · `urgent` · `empathetic` · `brand`

Reserved variables: `$in` `$out` `$error` `$meta` `$raw` `$draft` `$ctx` `$user` `$session` `$log` `$flags` `$quality` `$sentiment` `$confidence` `$memory` `$kb_results`

**Runtime-owned variables** (`RUNTIME_OWNED_VARIABLES` — read-only subset of reserved): `$in` `$error` `$meta` `$ctx` `$user` `$session` `$flags` `$memory` `$kb_results` — pipeline targets (`→ $name`) must not name these variables; violation emits E013.

**Vocabulary alignment (v0.4.7):** `KNOWN_COMMANDS` contains **54** commands across the 11 categories above — `ASK`/`CONFIRM` and `SETTLE` joined the original 51, and the settlement work moved `BUILTIN_SCHEMA_VERSION` to `"0.4.7"`. `RUN` is a meta-command — it bypasses domain-command argument and modifier checks; only decorator rules (`+modifier`, `→ $target`) apply.

### 4.7 Upstream parity gaps (v0.4.6 → v0.7)

<!-- [ADDED] v1.1.0 -->

When this section was written `BUILTIN_SCHEMA_VERSION` was **v0.4.6** against an upstream **v0.7**, so the crate implemented the v0.4 generation. The language-design view of these gaps is in `l1-nodus-language.md` §4.6; this section records the concrete implementation work, and each item was absent from the crate at the time.

**Status [UPDATED v1.5.2]:** (a) shipped with `l2-nodus-dialog.md`, (b) with `l2-nodus-control-flow.md`, (e) with `l2-nodus-errors.md` — the registry, not the emission sites (§4.5) — (f) with `l2-nodus-registries.md` as advisory `W011`–`W013`, and (h) with the dialog work. (c), (d) (NL-16), (g) (NL-18) and (i) remain open; the lexer still recognizes only `;;` comments.

#### (a) Vocabulary — commands

`KNOWN_COMMANDS` (51) omits the v0.6 human-interaction commands `ASK` and `CONFIRM`. Add both with their dialog modifiers (`+type`, `+options`, `+hint`, `+default`, `+validate`, `+timeout`, `+strict`, `+actions`), bound to the host HITL surface at the integration layer.

#### (b) Control constructs (`lexer` + `ast` + `executor`)

`?SWITCH` (+ `*` wildcard, `SWITCH_NO_MATCH`), `~MAP` (implicit `$it`), `~RETRY:n` (step modifier with `+backoff` / `+retry_on`), `!HALT` (→ `Status::Failed`), `!PAUSE` (→ new `Status::Paused`). None are tokenized or parsed today — `~MAP` / `~RETRY` currently mis-lex as a `Flag`. Semantics: `l1-nodus-language.md` §4.6.

#### (c) Operators & expressions (`lexer`)

`MATCHES` regex operator, `?.` optional chaining, `??` null-coalescing, `WHERE` / `FIRST` / `LAST` collection expressions, and string interpolation in literals (with `\$` escape) are not recognized as distinct tokens.

#### (d) `@needs:` selective schema loading

Add a `needs` field to the runtime-block AST plus load-time section filtering against `extends:` schemas.

#### (e) Error-code registry (`vocab::error_code`: 11 → 24)

Add `UNDEFINED_CMD`, `UNDEFINED_MACRO`, `VALIDATION_FAILED`, `ESCALATION_FAILED`, `CONFIDENCE_LOW`, `KB_UNAVAILABLE`, `MEMORY_FAILED`, `TEST_FAILED`, `SWITCH_NO_MATCH`, `PAUSED`, `COUNTER_OVERFLOW`, `GIT_UNAVAILABLE`, `DIALOG_TIMEOUT`, `DIALOG_REJECTED`, each with a severity and category. `NODUS:EXECUTION_FAILED` is non-canonical and is superseded.

#### (f) Closed vocabulary registries (new `vocab` data + validation)

The crate treats `~flag` and `^validator` tokens as free-form. Add closed registries and validate `~flag` / `^validator` / `@in` field types against them:

- **Analysis flags**: `sentiment`, `intent`, `entities`, `topics`, `lang`, `toxicity`, `urgency`, `formality`, `clarity`, `relevance`, `pii`, `keywords`.
- **Validators**: `len:n`, `min_len:n`, `no_pii`, `no_toxic`, `lang:code`, `format:type`, `required:keys`, `sentiment:op:n`, `confidence:n`, `no_links`, `brand_voice`, `approved`.
- **Primitive types**: `str`, `int`, `float`, `bool`, `list`, `obj`, `url`, `ts`, `null`, `any`.

#### (g) Macro execution

`@macro:` bodies are parsed (`MacroBlock`) and audited (`MacroEnter` / `MacroExit`) but **not executed**. `RUN(@macro:name)` must expand and run the body — caller params bind as `$in`, the last assigned variable is the implicit return.

#### (h) `Status` enum

Add `Paused` to the `Ok / Partial / Failed / Aborted` set (for `!PAUSE`).

#### (i) Minor lexer parity

Single-character `;` inline comments (only `;;` is recognized) and the `\$` interpolation escape.

## 5. Drawbacks & Alternatives

- **`StubProvider` gap**: the stub returns placeholder values; real AI generation requires a concrete `ModelProvider` wired at the host layer. Tests that rely on stub output cannot verify generation quality.
- **Schema evolution coupling**: bumping `BUILTIN_SCHEMA_VERSION` requires all callers that hard-code a schema name to update their `§runtime.core` declaration — schema updates are breaking by design to prevent silent vocabulary drift.
- **Alternative — Lua/Wren embed**: rejected; an embedded scripting interpreter adds a non-`std` runtime dependency and cannot enforce `!!` constraints at the language level.
- **Alternative — proc-macro DSL**: rejected; a macro-based DSL would couple the workflow language to the Rust compiler and break the portability goal (NL across host languages).

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[CRATE]` | `crates/nodus/` | Source of truth for all module-level details |
| `[VOCAB]` | `crates/nodus/src/vocab.rs` | Authoritative command list, `BUILTIN_SCHEMA_VERSION`, reserved variables |
| `[EXECUTOR]` | `crates/nodus/src/executor.rs` | `Value` type definition, executor boot sequence |
| `[WORKFLOWS]` | `crates/nodus/src/workflows.rs` | Public API function signatures and contracts |
| `[AST]` | `crates/nodus/src/ast.rs` | Full AST node type definitions |
| `[FIXTURES]` | `crates/nodus/tests/fixtures/` | Canonical sample workflows (normative test corpus) |

## Document History

| Version | Date | Change |
| --- | --- | --- |
| 1.0.0 | 2026-06-23 | Initial spec — module structure, AST nodes, Value type, executor boot sequence, public API, vocabulary schema v0.4.5 |
| 1.0.1 | 2026-06-23 | §4.6: documented vocabulary alignment (50 commands verified against `vocab.rs`), added `RUN` meta-command gap note |
| 1.0.2 | 2026-06-24 | §4.6: BUILTIN_SCHEMA_VERSION v0.4.6, 51 commands (RUN added as meta-command), RUNTIME_OWNED_VARIABLES constant documented; §3: NL-8→E013, NL-10→E014 enforced |
| 1.0.3 | 2026-06-24 | §4.1: add `observability.rs` module; §4.5: add `run_with_audit` and `run_with_provider_and_audit` rows; note `AuditProvider` as second extension point |
| 1.0.4 | 2026-06-24 | §4.1: add `portability.rs` module; §4.5: add `run_with_schema` and `run_with_schema_and_audit` rows; update extension-point note to reference `SchemaProvider` |
| 1.1.0 | 2026-06-25 | Added §4.7 upstream parity gaps (v0.4.6 → v0.7): missing `ASK`/`CONFIRM` commands, control constructs (`?SWITCH`/`~MAP`/`~RETRY`/`!HALT`/`!PAUSE`), operators/expressions, `@needs:`, `error_code` 11 → 24, closed flag/validator/type registries, macro execution, `Status::Paused`, lexer parity items |
| 1.2.0 | 2026-07-04 | §4.4: real `~PARALLEL` branch execution via `std::thread::scope` when providers are `Send + Sync` (sequential fallback otherwise); bounded width (`max_parallel_branches`, default cap 4); isolated branch environments with deterministic declared-order `~JOIN`; fail-fast without mid-step cancellation; audit interleaving resolved by `(correlation_id, seq)` |
| 1.3.0 | 2026-07-30 | Added §3.1 — realization status for NL-11 … NL-23, the thirteen invariants added to `l1-nodus-language.md` after this spec's original compliance table was written. Each of their Document History entries names this spec as carrier of their pending obligation, but no row existed here; the obligations were tracked only in `PLAN.md`'s Backlog. §3.1 makes the stated carrier real: NL-20 realized by `l2-nodus-config`, NL-22 by the new `l2-nodus-compensation`, NL-23 by the new `l2-nodus-restart`, the remaining ten explicitly pending with their additional blockers named (NL-16 external-schema loading, NL-18 `RUN` body expansion, NL-17 layering on the unrealized NL-11). |
| 1.3.1 | 2026-07-31 | Corrected §3's NL-2 and NL-9 rows, both naming a `Status` variant that has never existed in `executor::Status` (`RuleViolation`, `Error`) — predating most of this crate's evolution and never previously caught, since only §3.1 (additions after v1.0.0) had been repeatedly reconciled while §3's original rows had not. NL-9's row now also points to the new `l2-nodus-error-dispatch.md`, which realizes the `@err:` handler-dispatch half of NL-9 by attaching to this spec's own main step loop. Related Specifications gains the new sibling spec. |
| 1.3.2 | 2026-07-31 | NL-9's row updated from "realized by" to **Implemented** — `l2-nodus-error-dispatch.md` 1.0.0 → 1.0.1 (Phase 26) landed the dispatch mechanism. |
| 1.4.0 | 2026-09-01 | Extended §3.1 with NL-24…NL-26, the three invariants added to `l1-nodus-language.md` after the section was written, each grounded against `crates/nodus` source. NL-24 **partially realized** — `execute_parallel` is an unconditional sequential `for` loop and LP-1 leaves no scheduler that could do otherwise, so rules (a)/(b)/(c) hold trivially or structurally and the whole gap is the per-block *record*, which does not exist anywhere in `observability.rs`; the carrier fork (`EventAnnotations` vs. a `RunManifest` entry) is left open under HO-6's no-new-variant constraint. NL-25 **pending**, with two concrete shipped counter-examples in `execute_switch` — whole-`Value` equality instead of an anchored read, and a `*` default plus a `SWITCH_NO_MATCH` warning where a typed extraction error is required — and with NL-14's `+grade` escape hatch itself unrealized. NL-26 **partially realized** — the dialog half is discharged by DG-5's `+timeout`, the general half is vacuous behind NL-12, and the residue is the missing correlation token on `ResumeDescriptor`, which should land in the same pass as LP-22 (c)'s pinned generation. |
| 1.4.1 | 2026-09-02 | Corrected NL-26's row: retracted the v1.4.0 claim that its correlation-token residue "should land in the same pass as LP-22 (c)'s pinned generation" — the two are independent (LP-22 (c) is now fully designed in `l2-nodus-dialog.md` §4.3 v1.3.0 and needed no correlation-token decision). Sharpened why NL-26's general case is **vacuous in core** rather than merely unimplemented: `DialogProvider::ask`/`confirm` is synchronous — the interaction resolves within one call — so a correlation token naming a suspend/complete boundary a *later*, separate call could satisfy has nothing to attach to until NL-12 gives the crate an asynchronous external-step boundary at all; adding a token to today's dialog path would be inert data (the LP-11-pre-Phase-24 / LP-15 `store`/`load` trap). |
| 1.4.2 | 2026-09-02 | Corrected NL-24's row: retracted the "carrier fork, open" framing (v1.4.0) that reduced the gap to a two-way choice between `EventAnnotations` and `RunManifest`. Grounding against `executor.rs`'s `execute_node` dispatch showed the gap is prior to that choice — only `Stmt::Command` gets `StepStart`/`StepEnd` wrapping at all; `~PARALLEL` (and every other non-`Command` construct) emits no event of its own to attach a per-block annotation to, and `RunManifest`'s flat-field shape has no per-block collection today and no stable per-block identity source (`step_identity`, HO-15, is per-step). Closing NL-24 needs a new per-block identity scheme or a materially larger extension of the executor's event-wrapping model to non-`Command` statements — not a carrier pick. Verdict unchanged (**Partially realized**, (d) and the record still the open gap), scope corrected. |
| 1.5.0 | 2026-09-05 | §3.1 extended with NL-27 and NL-28 realization verdicts, taken against source as the NL-11…NL-26 rows were. Both **Pending**, for different reasons worth separating. NL-27's six exclusive-name classes split three ways: three are live and already violated (`@err:` last-write-wins through an `Option<ErrorDecl>`; duplicate macro names unchecked; `§config` fields where `get` resolves first-match while `is_secret` answers `any`, so value and secrecy can describe different declarations), one is live but inverted (host vocabulary collisions are silently *discarded* rather than replaced, telling the host nothing), and two are vacuous (imports never resolve; flags/validators/types have no host contribution path). NL-28's obstacle is the return shape — `validate` yields a flat `Vec<Diagnostic>`, leaving no field for coverage — and the crate already shows the exact failure the invariant names, with `extends` resolution deferred in a source comment rather than declared in any result. Neither verdict changes a requirement on the implementation; both record where it stands. Status stays Stable per the v1.3.0 / v1.4.0 precedent for this section. |
| 1.5.1 | 2026-09-05 | NL-27's row updated **Pending → Partially realized** — closes the two of its three live-and-violated classes that were detectable with no shape change (the AST retains both declarations for a `§config` field and a `@macro` name). `check_config_values` gained a pre-accept-loop duplicate-name scan (`ConfigReason::DuplicateField`) closing the accessor-disagreement path this row named; a new `e020_no_duplicate_macro_names` validator rule (modelled on `e015_no_duplicate_test_names`) closes the macro class. Left open with their reasons preserved: `@err:` last-write-wins (no advisory channel exists in `Parser::parse`'s return type), the inverted host-vocabulary discard (`Schema::with_provider` has nowhere to report it), both vacuous classes, and the entire stated-displacement half (no grammar for a declaration naming what it removes). 490 tests pass (was 486, +4); clippy/fmt clean. |
| 1.5.2 | 2026-09-24 | Consistency pass (2026-09-24): Re-grounded against the crate; several rows claimed behaviour that does not exist. NL-1 → Partial: an unknown command is not a validation error — it lexes as an identifier and the step parser keeps the line as a comment, so it silently drops out of the run (`E002` is an ordering check; `UNDEFINED_CMD` has no emission site). NL-3: nothing consumes `!PREF`. NL-4: the test entry points and the public `Executor::execute` skip validation. NL-5 cited `E006` for the `E010` check. NL-8 → Partial: boot copies every caller-input key into the environment, runtime-owned names included, so input can set `$user`/`$session`/`$ctx`. NL-9 → Partial: required-input presence and declared types are not enforced at run time (the row claimed a check that does not exist); with no `@err:` handler the run continues instead of terminating. §4.4: boot steps corrected (no `Status::RuleViolation`, no trigger matching, no missing-input failure); the v1.2.0 `~PARALLEL` concurrency text never shipped — branches run sequentially, branch signals are discarded (no fail-fast) and `~JOIN` binds an empty map. §4.5 records three pending defects: seams do not compose (only `run_with_provider*` takes a real model and it fixes a permit-all policy), stub runs are recorded as `ExecutionMode::Real`, and the dispatcher is a stub interpreter whose provider cannot report failure. Schema constant 0.4.6 → 0.4.7 and 54 commands; §4.7 marks the shipped parity items. Document History reordered to ascending version order. |
