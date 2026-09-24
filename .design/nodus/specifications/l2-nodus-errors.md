# Nodus Error Taxonomy Implementation (Rust)

**Version:** 1.4.0
**Status:** Stable
**Layer:** implementation
**Implements:** l1-nodus-language.md

## Overview

Concrete Rust realization of the nodus error model. `l1-nodus-language.md` §4.5
defines the original eleven `NODUS:*` codes and the `@err:` routing contract;
§4.6 expands the canonical taxonomy to twenty-four codes, each carrying a
**severity** (error / warn / info) and a **category** (parse / runtime /
validation / routing / memory / test / control / dialog), and demotes the
catch-all `NODUS:EXECUTION_FAILED` to a non-canonical legacy code superseded by
specific ones.

This spec specifies how `crates/nodus` realizes that taxonomy: the `error_code`
constant set, the severity/category metadata lookup, the supersede rule for the
catch-all code, and the emission points across the validator and executor. It
does not restate each code's trigger — those semantics live in
`l1-nodus-language.md` §4.5/§4.6; this spec maps each code to its runtime
metadata and enforcement site.

## Related Specifications

- [l1-nodus-language.md](l1-nodus-language.md) — the error model this spec implements: §4.5 original codes + `@err:` routing, §4.6 the 24-code taxonomy with severity/category
- [l2-nodus-runtime.md](l2-nodus-runtime.md) — the runtime crate this spec extends; `vocab::error_code`, `Error`, `RuntimeError`, the validator/executor emission sites (§4.7 itemizes the 11 → 24 gap)
- [l1-nodus-dialog.md](l1-nodus-dialog.md) — owns the `DIALOG_TIMEOUT` / `DIALOG_REJECTED` / `PAUSED` subset
- [l2-nodus-portability.md](l2-nodus-portability.md) — `NODUS:CAPABILITY_UNMET` (LP-8) and `NODUS:POLICY_DENIED` (LP-11, §4.9.4) are portability-layer runtime codes beyond the §4.6 language set; also documents that `@err:` handler dispatch (this spec's first bullet above) is unrealized in `executor.rs` today
- [l2-nodus-settlement.md](l2-nodus-settlement.md) — [ADDED v1.2.0] `NODUS:SETTLEMENT_UNACCOUNTED` (LP-17, §4.4) is a further portability-layer runtime code, registering beside `POLICY_DENIED`
- [l2-nodus-environment.md](l2-nodus-environment.md) — [ADDED v1.3.0] `NODUS:ENV_MEASURE_UNKNOWN` (NE-14, §4.4.1) is a further control-category code, registering beside `CAPABILITY_UNMET`
- [l2-nodus-error-dispatch.md](l2-nodus-error-dispatch.md) — [ADDED v1.1.1] realizes the `@err:` handler-dispatch mechanism this spec's NL-9 row names as a separate obligation; dispatches against the taxonomy this spec owns without adding any new code

## 1. Motivation

The crate currently ships the original eleven codes (plus `CAPABILITY_UNMET`
from LP-8) as flat string constants with no severity or category metadata. The
catch-all `EXECUTION_FAILED` swallows distinct failure modes — a model
low-confidence result, a knowledge-base outage, and a counter overflow all
collapse into one opaque code, so callers cannot route or report them
differently. Closing the §4.6 gap requires three things the current code lacks:

- the thirteen additional codes, so each distinct failure is nameable;
- per-code severity and category, so a host can decide what halts, what warns, and what is informational, and can group failures for reporting;
- a supersede rule that retires `EXECUTION_FAILED` from the canonical set without breaking existing matches.

## 2. Constraints & Assumptions

- No new external crate dependency (LP-1): severity/category are in-tree enums and a static lookup table.
- Codes remain stable `NODUS:*` string constants — the wire/report contract; metadata is attached *beside* a code, never by changing its string.
- `EXECUTION_FAILED` is retained as a deprecated constant for backward-compatibility but is excluded from the canonical registry; new emission sites must use a specific code.
- Hard-constraint handling is unchanged: `NODUS:RULE_VIOLATION` still bypasses `@err:` and terminates the run (NL-2); this spec does not alter that path.
- Severity does not by itself decide control flow — the executor's existing status logic does — but it classifies a code for reporting and for the `@err:` vs. continue decision a host applies.

## 3. Invariant Compliance

| L1 Invariant | Rust Enforcement |
| --- | --- |
| NL-1 Schema-first | **Realized by the validator's lint codes, not by this taxonomy.** An unknown command is the blocking validation error `E021` (`l2-nodus-runtime.md` §3, NL-1 row), so a misspelled command stops the run before it starts. None of the validation-category `NODUS:*` codes (`UNDEFINED_CMD`, `UNDEFINED_VAR`, `NO_SCHEMA`, `SCHEMA_MISMATCH`, `RULE_CONFLICT`, `VALIDATION_FAILED`) has an emission site anywhere in the crate: the validator reports its own `E0xx`/`W0xx` diagnostics instead, and `E021` stands where `UNDEFINED_CMD` would. |
| NL-2 Hard constraints absolute | `RULE_VIOLATION` retains its dedicated path: emitted by the executor's rule check, bypasses `@err:`, forces `Status::Failed`. Severity `error`, category `runtime`; metadata never reclassifies it as catchable. |
| NL-4 Validate-before-run | Any `error`-severity, `validation`-category code in the validation report blocks execution (the existing `has_errors` gate); execution-stage codes are unreachable on an invalid workflow. |
| NL-9 Typed I/O / `@err:` contract | **Taxonomy realized; dispatch implemented [v1.1.2].** Every runtime error carries a canonical `NODUS:*` code and surfaces in `RunResult.errors`; the taxonomy is a real, typed error surface — this document's own scope. **Dispatch itself** — invoking the declared `@err:` handler — is implemented per `l2-nodus-error-dispatch.md` (Phase 26): any non-fatal error a step returns with no `Signal` reaches a dispatch check in the main loop, `$error` is populated, and the handler runs via `execute_command` before the run ends; `RULE_VIOLATION` keeps its own dedicated fatal path unchanged (this document's NL-2 row) and is structurally excluded from dispatch, never an exception carved out by name. **`UNHANDLED_ERROR` is emitted** for the case L1 assigns it to: an error that ends its step with no `@err:` handler declared ends the step sequence and is recorded as `NODUS:UNHANDLED_ERROR`, with a `StepError` trace event naming the failed step and its cause; the triggering error stays in `RunResult.errors` beside it (`l2-nodus-error-dispatch.md` §4.5). "Routed to `@err:`" means both halves at once: a typed code reaches `RunResult.errors`, **and** the declared handler actually runs. |

## 4. Detailed Design

### 4.1 Severity and category types

```text
[REFERENCE]
pub enum ErrorSeverity { Error, Warn, Info }

pub enum ErrorCategory {
    Parse, Runtime, Validation, Routing, Memory, Test, Control, Dialog,
}
```

### 4.2 Canonical code registry (severity × category)

Each canonical code maps to one severity and one category. Triggers are defined
in `l1-nodus-language.md` §4.5/§4.6 and are not restated here.

| Code | Severity | Category |
| --- | --- | --- |
| `RULE_VIOLATION` | error | runtime |
| `PARSE_ERROR` | error | parse |
| `MAX_REACHED` | warn | control |
| `UNDEFINED_VAR` | error | runtime |
| `ROUTE_NOT_FOUND` | error | routing |
| `RULE_CONFLICT` | error | validation |
| `SCHEMA_MISMATCH` | error | validation |
| `NO_SCHEMA` | error | validation |
| `NO_TRIGGER` | warn | routing |
| `UNHANDLED_ERROR` | error | runtime |
| `UNDEFINED_CMD` | error | validation |
| `UNDEFINED_MACRO` | error | validation |
| `VALIDATION_FAILED` | error | validation |
| `ESCALATION_FAILED` | error | routing |
| `CONFIDENCE_LOW` | warn | runtime |
| `KB_UNAVAILABLE` | error | memory |
| `MEMORY_FAILED` | error | memory |
| `TEST_FAILED` | error | test |
| `SWITCH_NO_MATCH` | warn | control |
| `PAUSED` | info | control |
| `COUNTER_OVERFLOW` | error | runtime |
| `GIT_UNAVAILABLE` | error | runtime |
| `DIALOG_TIMEOUT` | error | dialog |
| `DIALOG_REJECTED` | error | dialog |

Twenty-four canonical codes. `CAPABILITY_UNMET` (category `control`, severity
`error`) is an additional implemented runtime code introduced by LP-8
(`l2-nodus-portability.md`); it sits beside this set as a portability-layer code.
`POLICY_DENIED` (category `runtime`, severity `error`) is a further portability-layer
code, specified by LP-11's call-site design (`l2-nodus-portability.md` §4.9.4) — same
classification as `RULE_VIOLATION`, since a denied effect is a runtime-stage failure.
`SETTLEMENT_UNACCOUNTED` (category `runtime`, severity `error`) is the LP-17 portability-layer
code, specified by `l2-nodus-settlement.md` §4.4 — same classification as `POLICY_DENIED`,
for a permitted settlement whose rail produced no verifiable receipt (VS-7).
`ENV_MEASURE_UNKNOWN` (category `control`, severity `error`) is the NE-14 environment-layer
code, specified by `l2-nodus-environment.md` §4.4.1 — same classification as
`CAPABILITY_UNMET`, since both are pre-run structural rejections rather than runtime-stage
effect denials: a profile declaring a token budget with no identified encoder is rejected
before `env.open`, never mid-run.
Four further layer codes sit in the same lookup table, each owned by its realization spec:
`CONFIG_INVALID` (category `validation`, severity `error` — `l2-nodus-config.md`),
`RESTART_LIMIT` (category `control`, severity `warn` — `l2-nodus-restart.md`) and
`COMPENSATION_FAILED` (category `runtime`, severity `error` — `l2-nodus-compensation.md`) and
`MODEL_CALL_FAILED` (category `runtime`, severity `error` — `l2-nodus-runtime.md` §4.5): a model call that
produced no answer, raised by the executor from the fallible `ModelProvider` surface, so a failed
generation reaches `@err:` dispatch as a typed step error rather than as a short answer.

### 4.3 Metadata lookup

A single static table maps each canonical code string to its metadata; a lookup
returns `None` for a non-canonical code (including the legacy `EXECUTION_FAILED`),
which lets callers detect non-canonical usage.

```text
[REFERENCE]
pub fn error_meta(code: &str) -> Option<(ErrorSeverity, ErrorCategory)>;
```

The table is the single source of truth; `error_code` string constants and this
table are kept in lockstep (a test asserts every canonical constant has metadata
and every metadata row names an existing constant).

### 4.4 Superseding the catch-all

`EXECUTION_FAILED` is retained as a `#[deprecated]` constant so existing string
matches still compile, but it is excluded from the canonical registry (§4.2) and
returns `None` from `error_meta`. Each former `EXECUTION_FAILED` emission site is
reassigned to a specific code:

| Former site | Replacement |
| --- | --- |
| Unknown command dispatched | `UNDEFINED_CMD` |
| `RUN(@x)` with undefined macro | `UNDEFINED_MACRO` |
| `^validator` failed | `VALIDATION_FAILED` |
| `ESCALATE` target unreachable | `ESCALATION_FAILED` |
| Model confidence below threshold | `CONFIDENCE_LOW` |
| `QUERY_KB` backend unavailable | `KB_UNAVAILABLE` |
| `REMEMBER`/`RECALL` failure | `MEMORY_FAILED` |

A site with no more specific code keeps a generic runtime failure but must select
the closest category rather than the retired catch-all.

**Emission status (pending).** The table names sites the executor does not have yet. Its
dispatcher is a stub interpreter (`l2-nodus-runtime.md` §4.5): `^validator`s are never
evaluated, and `ESCALATE`, `QUERY_KB`, `REMEMBER` and `RECALL` return fixed values without a
backend that could fail. An unknown command no longer reaches the dispatcher at all — the
validator rejects it as `E021` before the run starts — but that is a lint code, so
`UNDEFINED_CMD` is still not emitted. The retirement of `EXECUTION_FAILED` holds — it is
emitted nowhere — but none of the seven replacements is emitted either.

### 4.5 Emission points

| Stage | Codes |
| --- | --- |
| Parser | `PARSE_ERROR` |
| Validator | `NO_SCHEMA`, `SCHEMA_MISMATCH`, `RULE_CONFLICT`, `UNDEFINED_CMD`, `UNDEFINED_VAR`, `VALIDATION_FAILED` |
| Executor (control) | `RULE_VIOLATION`, `MAX_REACHED`, `SWITCH_NO_MATCH`, `PAUSED`, `COUNTER_OVERFLOW` |
| Executor (runtime/routing/memory/test/dialog) | `ROUTE_NOT_FOUND`, `NO_TRIGGER`, `ESCALATION_FAILED`, `CONFIDENCE_LOW`, `KB_UNAVAILABLE`, `MEMORY_FAILED`, `TEST_FAILED`, `GIT_UNAVAILABLE`, `DIALOG_TIMEOUT`, `DIALOG_REJECTED`, `UNHANDLED_ERROR` |

`UNDEFINED_VAR` is validator-emitted under NL-1; the optional-chaining operator
`?.` (a separate parity cluster) will later short-circuit it at run time rather
than raise it.

**Emission status (pending).** The table is the target map, not the shipped one. Of the
twenty-four canonical codes, seven have an emission site today — `RULE_VIOLATION`,
`MAX_REACHED`, `SWITCH_NO_MATCH`, `PAUSED`, `UNHANDLED_ERROR`, `DIALOG_TIMEOUT` and
`DIALOG_REJECTED`, all in the executor — plus the layer codes `CAPABILITY_UNMET`, `POLICY_DENIED`,
`SETTLEMENT_UNACCOUNTED`, `ENV_MEASURE_UNKNOWN`, `CONFIG_INVALID`, `RESTART_LIMIT`,
`COMPENSATION_FAILED` and `MODEL_CALL_FAILED`. The validator row is entirely unrealized: undeclared
variables surface as `E004`/`E014` diagnostics and an unknown command as `E021`, not as
`UNDEFINED_VAR` / `UNDEFINED_CMD`. A code with no emission site is a name a host can match on and
never receive, so the lockstep test (§4.3) proves the registry is consistent, not that a failure is
reported.

## 5. Drawbacks & Alternatives

- **Encode severity/category in the code string** (e.g., `NODUS:WARN:SWITCH_NO_MATCH`): rejected — it breaks the stable string contract and forces every matcher to parse structure out of an identifier.
- **One error enum variant per code**: rejected — 24+ variants bloat the `Error` type and couple every match site to the full set; a code string plus a metadata lookup keeps the surface flat and the metadata data-driven (consistent with the "vocabulary as data" property).
- **Delete `EXECUTION_FAILED` outright**: rejected — it is a published constant; deprecation plus registry exclusion retires it without a breaking removal.

## 6. Implementation Notes

- Severity/category are reporting metadata; the executor's `Status` derivation (Ok/Partial/Failed/Aborted) is unchanged. A host consults `error_meta` to decide presentation and whether a `warn`/`info` code should continue past `@err:`.
- The lockstep test (§4.3) is the guard that keeps the registry honest as future clusters (control-flow, dialog) add their codes.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[CODES]` | `crates/nodus/src/vocab.rs` | `error_code` constants + the metadata table |
| `[ERR]` | `crates/nodus/src/error.rs` | `Error` enum carrying the `NODUS:*` code |
| `[RTERR]` | `crates/nodus/src/executor.rs` | `RuntimeError` emission sites |
| `[VALIDATOR]` | `crates/nodus/src/validator.rs` | validation-category emission sites |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.4.0 | 2026-09-24 | Core Team | Realization sync (2026-09-24): `UNHANDLED_ERROR` is now emitted (seven of the twenty-four canonical codes have an emission site); `MODEL_CALL_FAILED` joins the layer codes; an unknown command is the lint error `E021`, so NL-1 is realized by the validator rather than by an `UNDEFINED_CMD` emission, which remains absent. |
| 1.3.1 | 2026-09-24 | Core Team | Consistency pass (2026-09-24): The NL-1 row claimed the validator emits the validation-category `NODUS:*` codes; none has an emission site, and an unknown command is not diagnosed at all. §4.4/§4.5 gain emission-status notes: six of the twenty-four canonical codes are emitted today, and none of the seven `EXECUTION_FAILED` replacements is. The three further layer codes (`CONFIG_INVALID`, `RESTART_LIMIT`, `COMPENSATION_FAILED`) are now listed beside the others. `UNHANDLED_ERROR`'s L1 role (no handler declared) is cross-referenced to the open deviation. |
| 1.3.0 | 2026-07-31 | Core Team | §4.2 gains a cross-reference for `ENV_MEASURE_UNKNOWN` (category `control`, severity `error`), the new NE-14 environment-layer code specified by `l2-nodus-environment.md` §4.4.1 — same classification as `CAPABILITY_UNMET`, both pre-run structural rejections rather than runtime-stage effect denials. Not added to §4.5's Emission Points table, matching how `CAPABILITY_UNMET`/`POLICY_DENIED`/`SETTLEMENT_UNACCOUNTED` are handled there. Related Specifications gains the new sibling spec. |
| 1.2.0 | 2026-07-31 | Core Team | §4.2 gains a cross-reference for `SETTLEMENT_UNACCOUNTED` (category `runtime`, severity `error`), the new LP-17 portability-layer code specified by `l2-nodus-settlement.md` §4.4 — same classification as `POLICY_DENIED`, for a gate-permitted settlement whose rail returned no receipt. Not added to §4.5's Emission Points table, matching how `POLICY_DENIED`/`CAPABILITY_UNMET` are handled there (portability-layer codes are owned and enumerated by their own realization spec). Because it is `Signal`-free like `POLICY_DENIED`, it reaches `l2-nodus-error-dispatch.md`'s NL-9 dispatch check automatically — no change needed to that spec either. Related Specifications gains the new sibling spec. |
| 1.1.2 | 2026-07-31 | Core Team | NL-9 row updated: dispatch is now **implemented** (Phase 26, `l2-nodus-error-dispatch.md` 1.0.0 → 1.0.1) — "routed to `@err:`" means both a typed code reaching `RunResult.errors` and the declared handler actually running. |
| 1.1.1 | 2026-07-31 | Core Team | NL-9 row updated: dispatch is now **specified** (`l2-nodus-error-dispatch.md`, new sibling spec), not merely flagged as missing. Eligibility for dispatch is structural (any `RuntimeError` returned with no `Signal`) rather than an enumerated code list, so this document needed no new code and no change to its own emission-point table — `RULE_VIOLATION` keeps the dedicated fatal path this document's NL-2 row already describes, unchanged. Related Specifications gains the new sibling spec. |
| 1.1.0 | 2026-07-31 | Core Team | §4.2 gains a cross-reference for `POLICY_DENIED` (category `runtime`, severity `error`), the new LP-11 call-site denial code specified by `l2-nodus-portability.md` §4.9.4 — same classification as `RULE_VIOLATION`, sitting beside the frozen 24-code set exactly as `CAPABILITY_UNMET` does for LP-8. Not added to §4.5's Emission Points table, matching how `CAPABILITY_UNMET` is handled there (portability-layer codes are owned and enumerated by their own realization spec, not restated here). |
| 1.0.0 | 2026-06-27 | Core Team | Initial spec — Rust realization of the 24-code taxonomy (`l1-nodus-language.md` §4.6): `ErrorSeverity`/`ErrorCategory` types, per-code severity×category registry, `error_meta` lookup, `EXECUTION_FAILED` supersede rule + site reassignment, validator/executor emission map; `CAPABILITY_UNMET` noted as the LP-8 portability-layer code. |
