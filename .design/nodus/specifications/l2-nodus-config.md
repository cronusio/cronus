# Nodus Declarative Configuration Surface (Rust)

**Version:** 1.0.2
**Status:** Stable
**Layer:** implementation
**Implements:** l1-nodus-language.md

## Overview

Concrete Rust realization of the `§config` declarative-configuration surface.
`l1-nodus-language.md` §4.1 (NL-20) redefines a `§config` file from a flat value
overlay into a **declaration of a configuration surface**: each field declares a
type and, optionally, a default, a range or enumeration, a required marker, a
secret marker, and a human description. A host renders that declaration as an
editing surface and MUST validate a proposed value set against it before those
values become visible to any workflow, in the order **declaration → proposed
values → shape check → host acceptance → run**.

This spec specifies how `crates/nodus` realizes that surface: the config-file
AST, the `§config` parser that replaces today's deferral stub, the pre-run shape
check, the provenance- and secret-aware value model, the host-acceptance
extension seam, and the public run entry points. It does not restate the field
grammar or the host obligations — those live in `l1-nodus-language.md` §4.1; this
spec maps each to its Rust type and enforcement site.

## Related Specifications

- [l1-nodus-language.md](l1-nodus-language.md) — the contract this spec implements: §4.1 (NL-20) field-declaration grammar, validation order, and the secret/provenance boundary
- [l2-nodus-runtime.md](l2-nodus-runtime.md) — the runtime crate this spec extends: `FileType::Config` + `FileHeader` (`ast.rs`), the `Parser::parse` deferral this spec closes, the `Value` model, and the executor boot sequence
- [l2-nodus-portability.md](l2-nodus-portability.md) — the extension-point framework this spec adds a role to: `ExtensionRole`, `HostCapabilities`, `CapabilityManifest`, `validate_manifest` (LP-8); the host-supplied seam pattern (LP-2/LP-15)
- [l2-nodus-errors.md](l2-nodus-errors.md) — owns the canonical `NODUS:*` taxonomy this spec's `CONFIG_INVALID` code registers into (validation category)
- [l2-nodus-dialog.md](l2-nodus-dialog.md) — the precedent this spec mirrors: a host-supplied provider (`DialogProvider`) with a deterministic non-interactive default and an `ExtensionRole` binding
- [../../main/specifications/l1-declarative-configuration.md](../../main/specifications/l1-declarative-configuration.md) — the host-side contract NL-20 realizes (DC-1 owner-published declaration, DC-3/DC-4 owner-side pre-run validation, DC-9 secret write-only, DC-10 config-is-shape-not-authority)
- [../../main/specifications/l2-config-hotreload.md](../../main/specifications/l2-config-hotreload.md) — a host-workspace sibling realization (config hot-reload); the accepted-set-stays-in-force guarantee here is the invariant its reload path depends on

## 1. Motivation

The crate can lex a `§config:` header — `FileType::Config` exists and
`FileHeader` carries it — but `Parser::parse` and `Parser::parse_with_schema`
both reject a `§config` file outright with `"schema/config file parsing is not
yet implemented"`. There is no field-declaration AST, no shape check, no path
from a declared field to a value a workflow can read. NL-20 promotes `§config`
from the deferred §4.6 note it once was to a first-class contract, so the gap is
now a missing first-class surface. Closing it requires what the crate lacks:

- a **field-declaration AST** — a `§config` file parses to a set of typed field declarations, not to a workflow;
- a **shape check** — a proposed value set is validated against the declaration before the run, fail-fast and pre-run, with a rejected set leaving the previously accepted set untouched so a workflow is never partially configured;
- a **secret- and provenance-aware value model** — an accepted value enters the run as an ordinary `Value` carrying provenance (being configuration confers no trust), and a `secret` field is write-only: usable, but never rendered into a model-facing prompt and never entering a trace;
- a **host-acceptance seam** — rendering, editing, storage, and the accept/reject authority are host-supplied; nodus contributes only the declaration, the shape check, and the ordering discipline.

## 2. Constraints & Assumptions

- No new external crate dependency (LP-1): the field model, the shape check, and the default acceptor are in-tree types and functions.
- Additive: a workflow with no `§config` file is unchanged; every new entry point is a sibling of the existing `run_with_*` family, not a modification of `run`.
- The declaration is the single source of truth (DC-1). A rendered form and the accepted values cannot diverge because both derive from the same `ConfigDecl`.
- Validation is pre-run and fail-fast (DC-3/DC-4), in the LP-8 family — it never surfaces as a mid-run failure. A rejected value set aborts acceptance; it never partially applies.
- A `secret` field is write-only (DC-9): the value model permits its use as a step input but exposes no construct that renders it into a model-facing prompt (GEN/REFINE) and no path that writes it to an audit trace. This composes NL-11 and the §4.4 data-safety boundary; it is a structural property of the value, not a redaction pass.
- Configuration confers no trust: an accepted value carries provenance (NL-11) and MAY carry an origin taint (NL-17) when the host populates it from an external or synced source. Unclassified config defaults to untrusted, exactly like any other externally-sourced value.
- A workflow reads its configuration but can never author, widen, or self-grant it (LP-10 / DC-10). `§config` declares a shape; it never declares an authority.
- Determinism: the same declaration and the same accepted value set produce the same in-run configuration; the built-in acceptor performs no I/O.

## 3. Invariant Compliance

| L1 Invariant | Rust Enforcement |
| --- | --- |
| NL-20 Declarative configuration surface | A `§config` file parses to a `ConfigDecl` (ordered `ConfigField` set). `check_config_values(&ConfigDecl, proposed)` runs the shape check pre-run; the `ConfigProvider` seam supplies host acceptance; `run_with_config` sequences declaration → proposed → shape check → host acceptance → run. The check is pure and `DefaultConfigProvider` accepts a programmatically supplied set with no I/O, so a non-interactive run is reproducible from (declaration, proposed set). |
| NL-1 Schema-first / NL-9 typed I/O | The shape check is a validation-category gate: type match, range/enumeration, required-present, and unknown-field rejection are decided before the executor boots, never first surfaced at run time. A `§config` field type reuses the primitive-type registry (`l2-nodus-registries.md`). |
| NL-11 Provenance-safe interpolation | An accepted value enters as a `Value` with provenance; unclassified/host-external config is `untrusted` and neutralized-as-data at any model-facing render. A `secret` field additionally has no prompt-rendering construct at all (write-only), the neutralize-by-removing-the-capability form of NL-11. |
| NL-17 Origin-taint provenance | The `ConfigProvider` MAY attach a host-supplied origin taint when a value is populated from an external/synced source; the taint rides the value and never upgrades it untrusted → trusted. |
| LP-2 Extension via abstract interfaces / LP-15 Host-supplied durable state | Rendering, editing, persistence, and accept/reject authority live behind `ConfigProvider`; nodus core ships only `DefaultConfigProvider` (programmatic acceptance, no store, no UI). |
| LP-8 Capability manifest | A workflow that consumes a `§config` surface requires `ExtensionRole::Config`; `validate_manifest` rejects fail-fast with `NODUS:CAPABILITY_UNMET` when the host does not provide it. `HostCapabilities::builtin()` provides `Config` via the default acceptor. |
| LP-10 No self-granted authority | The parser accepts only field *declarations* in a `§config` file; there is no construct by which a workflow widens a range, clears `required`, or promotes its own value set. Acceptance authority is the host's alone. |

## 4. Detailed Design

### 4.1 The gap today

`ast.rs` defines `FileType { Workflow, Schema, Config }` and a `FileHeader`
carrying the type; the lexer tokenizes the `§config:` sigil. `parser.rs`
short-circuits both `parse` and `parse_with_schema` on a `§config:`/`§schema:`
header with `Error::Parse { message: "schema/config file parsing is not yet
implemented" }`. This spec replaces the `§config` branch of that stub with a real
parser and adds the AST, shape check, value model, provider seam, and public API
below. The `§schema` branch is out of scope and remains deferred.

### 4.2 Config AST

A `§config` file parses to a `ConfigDecl` — the ordered set of declared fields.
Field order is preserved for deterministic rendering and stable diagnostics.

```text
[REFERENCE]
pub struct ConfigDecl {
    pub header: FileHeader,        // file_type == FileType::Config
    pub fields: Vec<ConfigField>,  // declaration order preserved
}

pub struct ConfigField {
    pub name: String,
    pub ty: String,                // a primitive-type token (l2-nodus-registries)
    pub default: Option<Value>,
    pub constraint: Option<FieldConstraint>,
    pub required: bool,
    pub secret: bool,
    pub describe: Option<String>,
}

pub enum FieldConstraint {
    Range { lo: Value, hi: Value }, // `range: lo..hi` — ordered primitives
    OneOf(Vec<Value>),              // `one-of: a | b | c`
}
```

`default` and `constraint` are mutually compatible (a default, if present, must
itself satisfy the constraint — checked at parse/validate time, §4.4). `required`
and `default` are not mutually exclusive at the type level, but a `required`
field with a `default` is reported as a redundancy warning (the default makes
the requirement unreachable).

### 4.3 Config-file parsing

```text
[REFERENCE]
impl Parser {
    /// Parse a `§config:` file into its field declarations.
    /// Errors if the source does not open with a `§config:` header.
    pub fn parse_config(source: &str) -> Result<ConfigDecl>;
}
```

`parse_config` reuses `parse_header(FileType::Config)`, then reads field
declarations per the §4.1 grammar: `name : type` followed by any of the optional
clauses (`default:`, `range:` | `one-of:`, `required`, `secret`, `describe:`).
`Parser::parse`/`parse_with_schema` stay typed to `WorkflowFile` — a `§config`
file is not a workflow, so they do not (and cannot) return a `ConfigDecl`.
Their `§config` arm is rewritten from the generic deferral message to a precise
redirect naming `parse_config` as the dedicated entry point; the `§schema` arm
keeps the original deferral message (schema parsing remains unimplemented). The transpiler gains a `§config` round-trip
(`to_nodus` re-emits the declaration; `describe:` and `secret` survive the trip)
so a host editor can serialize an amended declaration back to source.

> `[DR]` Field clauses lex as line-oriented markers within a field block rather
> than as a new token class — keeps lexer churn minimal and mirrors how
> `@in:` field lines are already read. *(Alternative — a dedicated token class per
> clause — rejected: heavier lexer surface for no parser gain.)*

### 4.4 Shape check (pre-run validation)

```text
[REFERENCE]
pub struct AcceptedConfig {                 // opaque; values retrieved by name
    // name -> (Value, provenance, secret flag)
}

pub struct ConfigViolation { pub field: String, pub reason: ConfigReason }
pub enum ConfigReason {
    UnknownField, MissingRequired, TypeMismatch, OutOfRange, NotInEnum, BadDefault,
}

/// Pure, pre-run. Ok => an accepted set ready for host acceptance;
/// Err => the full violation list (fail-fast reports all, applies none).
pub fn check_config_values(
    decl: &ConfigDecl,
    proposed: &[(String, Value)],
) -> std::result::Result<AcceptedConfig, Vec<ConfigViolation>>;
```

The check validates, in one pass: every proposed key names a declared field
(else `UnknownField`); every `required` field without a default is present (else
`MissingRequired`); each value's type matches the field type (`TypeMismatch`);
each value satisfies its `constraint` (`OutOfRange` / `NotInEnum`); and each
declared `default` itself satisfies its constraint (`BadDefault`, a declaration
error surfaced at check time). Missing optional fields resolve to their
`default`. The function is pure and side-effect-free — it is the NL-1/NL-9
validation-category gate for configuration, and it never mutates a prior accepted
set.

### 4.5 Accepted values, provenance, and secrets

An `AcceptedConfig` yields values a workflow reads by name; each retrieved value
is an ordinary `Value` carrying provenance. Config is not trusted by virtue of
being config: a value the host populated from an external or synced source
carries `untrusted` provenance (NL-11) and MAY carry an origin taint (NL-17),
and is neutralized-as-data at any model-facing render. A field marked `secret`
is **write-only**: `AcceptedConfig` exposes it to step evaluation but provides
**no** accessor that renders it into a model-facing (`GEN`/`REFINE`) prompt and
**no** path that includes it in an `ExecutionEvent`/`RunManifest` field
(`observability.rs` receives only a `FieldDescriptor` for a secret, never its
value — DG-7-style descriptor-only exposure). This is NL-11 neutralization by
removing the capability, realized as a property of the value rather than a
downstream redaction filter.

### 4.6 Host-acceptance seam

Acceptance is the host's authority (LP-2/LP-10). It is expressed as an extension
role mirroring `DialogProvider`/`EnvironmentProvider`:

```text
[REFERENCE]
pub enum ConfigOutcome {
    Accepted(AcceptedConfig),   // host approved the proposed set
    Rejected(Vec<ConfigViolation>), // host declined; prior accepted set stays in force
}

pub trait ConfigProvider {
    /// Given the declaration and a shape-checked candidate set, decide.
    fn accept(&self, decl: &ConfigDecl, candidate: AcceptedConfig) -> ConfigOutcome;
}

pub struct DefaultConfigProvider; // accepts the shape-checked set as-is; no I/O, no store, no UI
```

`ExtensionRole::Config` is added; `HostCapabilities::builtin()` provides it via
`DefaultConfigProvider`. `CapabilityManifest::from_workflow` marks `Config`
required when the workflow reads a config surface. A host that renders a form,
exposes a terminal editor, persists accepted versions, or stores secrets in a
keychain implements `ConfigProvider` over that machinery; none of it lives in
core.

### 4.7 Public API and validation order

```text
[REFERENCE]
pub fn run_with_config(
    workflow: &str,
    decl: &ConfigDecl,
    proposed: &[(String, Value)],
    provider: &dyn ConfigProvider,
) -> Result<RunResult>;

pub fn run_with_config_and_audit(
    workflow: &str,
    decl: &ConfigDecl,
    proposed: &[(String, Value)],
    provider: &dyn ConfigProvider,
    audit: &dyn AuditProvider,
) -> Result<RunResult>;
```

The entry point sequences NL-20's order exactly: parse the declaration → take the
proposed values → `check_config_values` (shape check) → `provider.accept` (host
acceptance) → boot the executor with the accepted set bound as readable
configuration. A shape-check failure or a `Rejected` outcome returns before the
executor boots and **emits no audit events** — the run never starts, and any
previously accepted set is untouched (never partially configured). Accepted
values are read by workflows through the existing variable/context surface;
secret values follow the §4.5 write-only path.

### 4.8 Error taxonomy integration

One canonical code is added to the `l2-nodus-errors.md` registry:

| Code | Severity | Category |
| --- | --- | --- |
| `CONFIG_INVALID` | error | validation |

`CONFIG_INVALID` is the pre-run rejection surface — the shape check emits it
carrying the `ConfigViolation` list, and a host `Rejected` outcome maps to it.
It is validation-category (pre-run, blocks the boot gate like `SCHEMA_MISMATCH`),
and it registers in `error_meta` in lockstep with its constant. No secret value
appears in a `CONFIG_INVALID` payload — only the offending field name and reason.

> `[DR]` One aggregate `CONFIG_INVALID` carrying a structured violation list,
> not a code per reason — mirrors the validation-report pattern and keeps the
> `NODUS:*` surface flat (the l2-nodus-errors "vocabulary as data" property).
> *(Alternative — `CONFIG_TYPE_MISMATCH`/`CONFIG_OUT_OF_RANGE`/… — rejected:
> multiplies codes for one pre-run gate.)*

## 5. Drawbacks & Alternatives

- **Reuse `StorageProvider` for acceptance** instead of a new `ConfigProvider`/`ExtensionRole::Config`: rejected — storage is persistence, acceptance is authority; conflating them would let a "store" imply an accept. The Dialog/Environment precedent is a distinct role per distinct authority.
- **Parse `§config` into the workflow AST** and treat fields as a special `@in:`: rejected — a config surface has declaration-only semantics (range/secret/describe, host acceptance) with no `@steps:`; forcing it through the workflow grammar loses the very properties NL-20 adds.
- **Redact secrets with a post-hoc trace filter**: rejected — a filter is a fallible allow-list; write-only-by-construction (no accessor exists) is the NL-11 "remove the capability" discipline and cannot be bypassed by a new emission site.
- **Validate lazily at first read**: rejected — violates DC-3/DC-4 pre-run fail-fast and NL-20's ordering; a bad set must never reach a running workflow.

## 6. Implementation Notes

1. AST + parser first (`ConfigDecl`/`ConfigField`, `parse_config`, rewire the `parse` deferral branch) — everything else reads the declaration.
2. Shape check (`check_config_values`) next — pure, unit-testable in isolation, no runtime coupling.
3. Value model (provenance + secret write-only path in `AcceptedConfig`) alongside the shape check.
4. `ConfigProvider` + `DefaultConfigProvider` + `ExtensionRole::Config` + `builtin()` wiring (portability), then `run_with_config[_and_audit]` (workflows) + `lib.rs` re-exports.
5. `CONFIG_INVALID` registered in the error registry with a lockstep-test row.
6. Validation task: a secret-neutrality gate (a `secret` field cannot be rendered into a model prompt nor appear in any emitted event) alongside NL-20 shape-check coverage and the LP-8 fail-fast test.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[AST]` | `crates/nodus/src/ast.rs` | `FileType::Config`, `FileHeader`; home of `ConfigDecl`/`ConfigField` |
| `[PARSER]` | `crates/nodus/src/parser.rs` | the `§config` deferral branch this spec replaces; `parse_header` reuse |
| `[VALIDATOR]` | `crates/nodus/src/validator.rs` | the shape-check gate + `ConfigViolation` diagnostics |
| `[PORT]` | `crates/nodus/src/portability.rs` | `ExtensionRole`, `HostCapabilities`, `CapabilityManifest` to extend with `Config` |
| `[WORKFLOWS]` | `crates/nodus/src/workflows.rs` | `run_with_*` family the config entry points join |
| `[CODES]` | `crates/nodus/src/vocab.rs` | `error_code` registry to add `CONFIG_INVALID` |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.2 | 2026-09-24 | Core Team | Consistency pass (2026-09-24): Links to the two main-workspace specifications resolved one directory short. An `NL-6 Deterministic execution` row cited the wrong invariant — NL-6 is dual representation; the determinism statement now sits in the NL-20 row, where the behaviour lives. The LP-2/LP-15 row title now names both invariants as the portability contract defines them. |
| 1.0.1 | 2026-07-24 | Core Team | Patch clarification (no logic change): §4.3 realigned to the actual `Parser::parse`/`parse_with_schema` return type (`Result<WorkflowFile>`) — their `§config` arm cannot return a `ConfigDecl`, so it is rewritten to a precise redirect error naming `parse_config` rather than literally "delegating"; the `§schema` arm is unchanged. |
| 1.0.0 | 2026-07-24 | Core Team | Initial spec — Rust realization of the `§config` declarative-configuration surface (`l1-nodus-language.md` §4.1 / NL-20): `ConfigDecl`/`ConfigField` AST, `parse_config` replacing the parser deferral, pure pre-run `check_config_values` shape check, provenance-carrying + write-only-secret value model, `ConfigProvider`/`ExtensionRole::Config` host-acceptance seam with a deterministic `DefaultConfigProvider`, `run_with_config[_and_audit]` sequencing declaration → proposed → shape check → host acceptance → run, and the `CONFIG_INVALID` validation code. |
