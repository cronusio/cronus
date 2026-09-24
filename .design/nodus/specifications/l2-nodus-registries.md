# Nodus Closed Vocabulary Registries Implementation (Rust)

**Version:** 1.1.3
**Status:** Stable
**Layer:** implementation
**Implements:** l1-nodus-language.md

## Overview

The crate treats `~flag` analysis extractors, `^validator` rules, and `@in`
field types as free-form text: any token lexes and parses without a vocabulary
check. `l1-nodus-language.md` §4.6 closes this by declaring **closed registries**
for analysis flags, validators, and primitive types that a conforming
implementation should validate against, strengthening NL-1 (schema-first
vocabulary). This spec specifies the Rust realization: the three registries as
`vocab` data, the `Schema` query surface that consults them, and the advisory
validator diagnostics emitted for tokens outside the registries.

The concrete registry contents are the v0.7 sets enumerated in
`l2-nodus-runtime.md` §4.7(f); this spec does not restate them as prose — it
specifies how they are stored and checked.

## Related Specifications

- [l1-nodus-language.md](l1-nodus-language.md) — NL-1 schema-first vocabulary contract this spec strengthens; §4.6 declares the closed registries
- [l2-nodus-runtime.md](l2-nodus-runtime.md) — the runtime crate this extends; `vocab` constants, `Schema` query methods, the validator; §4.7(f) enumerates the registry contents
- [l2-nodus-errors.md](l2-nodus-errors.md) — error/diagnostic conventions; registry checks are advisory (warning severity), distinct from the `NODUS:*` runtime taxonomy
- [l2-nodus-commands.md](l2-nodus-commands.md) — [ADDED v1.1.3] what a registered `^validator` name means at run time: three core-evaluated rules, the rest the host's, an unevaluable rule failing closed (§4.5 there)

## 1. Motivation

Free-form flags and validators let a typo (`~sentimet`, `^reqired`) pass
validation silently and only surface as a no-op at run time, contradicting the
schema-first promise of NL-1. Closing the registries gives the validator the
data to flag an unknown extractor, validator, or type before execution, while
keeping the check advisory so existing workflows that lean on host-specific
extensions are not hard-broken.

## 2. Constraints & Assumptions

- No new external dependency (LP-1): the registries are in-tree `&[&str]` constants beside `KNOWN_COMMANDS`.
- The registries follow the LP-4 isolation rule: they are builtin constants, never mutated; a host extends vocabulary through the `SchemaProvider` seam, not by editing these.
- Validators may carry a colon-delimited argument (`len:n`, `lang:code`, `sentiment:op:n`); the registry holds the validator **name** (the segment before the first `:`), and the check validates the name, not the argument.
- Registry checks are **advisory** (warning severity): an unknown flag/validator/type is reported but does not, by itself, block execution. This avoids a breaking change to workflows using host extensions; promotion to a hard error is a future amendment.
- The checks run in the existing validator pass; no new pipeline stage is introduced.

## 3. Invariant Compliance

| L1 Invariant | Rust Enforcement |
| --- | --- |
| NL-1 Schema-first | The validator now consults closed registries for `~flag`, `^validator`, and `@in` field types, so an out-of-vocabulary extractor/validator/type is surfaced at validation rather than silently ignored at run time — extending NL-1 beyond commands. |
| NL-7 Closed value type system | The primitive-type registry is the finite type set (`str/int/float/bool/list/obj/url/ts/null/any`); a field type outside it is flagged, reinforcing the closed value space. |
| NL-9 Typed I/O contract | `@in` field types are validated against the primitive registry, so the public input contract is declared in a known type vocabulary. This checks the declared type *names* only; the values bound to those fields at run time are not type-checked (`l2-nodus-runtime.md` §3, NL-9 row). |
| NL-27 Exclusive bindings displaced explicitly [ADDED v1.1.0, RE-CONFIRMED Phase 33] | **Vacuous at this spec's surface — by the shape of the registries, not by an omission — and unaffected by Phase 33's Track A/B closes.** NL-27 names host-contributed command, flag and validator entries among the classes admitting exactly one holder. Of the three registries this spec owns, none is host-extensible: `is_known_flag`, `is_known_validator` and `is_known_type` consult `KNOWN_FLAGS` / `KNOWN_VALIDATORS` / `PRIMITIVE_TYPES` alone, and `SchemaProvider` contributes only commands and reserved variables. With exactly one contributor there is no second declaration to detect, so the rule has nothing to bind here and would acquire content only if these registries ever became host-extensible — a phase closing the `§config`-field and macro-name classes elsewhere in the crate changes nothing about that. The live half of NL-27's LP-4 clause sits on the one vocabulary surface that *is* extensible — `Schema::with_provider`'s silent collision discard, still open — and is carried in [l2-nodus-runtime.md](l2-nodus-runtime.md) §3.1 (now **Partially realized**) rather than split across two specs. |
| NL-28 A validation result declares what it did not check [ADDED v1.1.0] | **Pending — this spec owns one of the unchecked regions and none of the reporting surface.** §4.3's three diagnostics are advisory warnings that never set `has_errors`, so an out-of-vocabulary flag, validator or field type leaves a run permitted and a verdict clean; under NL-28 that combination is precisely a region whose non-blocking status must be *stated* rather than inferred from severity. The registries are also closed against a `~flag`, `^validator` or `@in` type the host understands and the core does not — an unreachable region with a different owning authority, which is NL-28's authority-and-schedule case. Neither can be reported until `Validator::validate` gains a place to put coverage; that return-shape change is the general obligation, carried in [l2-nodus-runtime.md](l2-nodus-runtime.md) §3.1. What belongs here once it exists is the region declaration itself: *vocabulary conformance checked against the closed builtin registries; host-understood extensions outside them not checked here*. |

## 4. Detailed Design

### 4.1 Registries as `vocab` data

Three constants beside `KNOWN_COMMANDS`, populated from `l2-nodus-runtime.md`
§4.7(f):

```text
[REFERENCE]
pub const KNOWN_FLAGS: &[&str];        // analysis extractors (~flag)
pub const KNOWN_VALIDATORS: &[&str];   // validator names (^validator, pre-colon segment)
pub const PRIMITIVE_TYPES: &[&str];    // @in field types
```

### 4.2 `Schema` query surface

```text
[REFERENCE]
impl Schema {
    pub fn is_known_flag(&self, name: &str) -> bool;
    pub fn is_known_validator(&self, name: &str) -> bool; // matches the pre-colon name
    pub fn is_known_type(&self, name: &str) -> bool;
}
```

`is_known_validator` splits the token on the first `:` and matches the leading
name, so `len:32` and `len` both resolve to the `len` validator.

### 4.3 Validator diagnostics (advisory)

The validator gains three advisory (warning-severity) diagnostics, emitted with
the next available `W0xx` codes in `validator.rs`:

| Condition | Diagnostic |
| --- | --- |
| `~flag` not in `KNOWN_FLAGS` | warning — "unknown analysis flag `{name}`" |
| `^validator` name not in `KNOWN_VALIDATORS` | warning — "unknown validator `{name}`" |
| `@in` field type not in `PRIMITIVE_TYPES` | warning — "unknown field type `{name}`" |

Warnings do not set `ValidationReport::has_errors`, so they never block a run
(NL-4 is unaffected). The exact `W0xx` numbers are assigned at implementation
time against the validator's existing code set.

## 5. Drawbacks & Alternatives

- **Hard errors instead of warnings**: rejected for now — it would break workflows that rely on host-specific flags/validators before the `SchemaProvider` path is wired to extend these registries. Advisory first; hard-error promotion is a later amendment once host extension is in place.
- **Storing validators with their argument grammar** (e.g. a parser per validator): rejected — out of scope; the registry validates the name, and argument validation is a richer future feature.
- **Folding into `KNOWN_COMMANDS`**: rejected — flags, validators, and types are distinct token classes with distinct call sites; conflating them would weaken the per-class check.

## 6. Implementation Notes

- The three constants and their `Schema` queries mirror the existing `KNOWN_COMMANDS` / `is_command` pattern exactly — the same "vocabulary as data" shape.
- Host extension of these registries (via `SchemaProvider`) is a natural follow-on, paralleling `host_commands`; it is not required for this advisory baseline.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[VOCAB]` | `crates/nodus/src/vocab.rs` | `KNOWN_FLAGS` / `KNOWN_VALIDATORS` / `PRIMITIVE_TYPES` + `Schema` queries |
| `[VALIDATOR]` | `crates/nodus/src/validator.rs` | advisory registry diagnostics |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.0 | 2026-06-27 | Core Team | Initial spec — closed vocabulary registries (analysis flags, validators, primitive types) as `vocab` data + `Schema` query surface + advisory validator diagnostics; strengthens NL-1/NL-7/NL-9. Contents per `l2-nodus-runtime.md` §4.7(f). |
| 1.1.0 | 2026-09-05 | Core Team | §3 extended with NL-27 and NL-28 verdicts, taken against source. NL-27 is **vacuous at this surface**: none of the three registries this spec owns is host-extensible (`is_known_flag` / `is_known_validator` / `is_known_type` read the builtin constants alone, and `SchemaProvider` contributes only commands and reserved variables), so with one contributor there is no second declaration to detect; the live half of the LP-4 clause is `Schema::with_provider`'s silent collision discard and is carried by `l2-nodus-runtime` §3.1. NL-28 is **Pending**, and this spec owns a region rather than the reporting surface: §4.3's advisory diagnostics never set `has_errors`, so a vocabulary miss leaves the verdict clean, and the closed registries are unreachable against a host-understood extension — both regions NL-28 requires a result to name, once the result has somewhere to name them. No requirement on the implementation changed. |
| 1.1.1 | 2026-09-05 | Core Team | Re-confirmed NL-27's **Vacuous** verdict after `l2-nodus-runtime.md`'s Phase 33 update closed two of NL-27's three live-and-violated classes elsewhere in the crate (`§config` field duplicates, macro-name duplicates) — neither touches this spec's registries, which remain non-host-extensible by construction, so nothing here changed. Cross-reference to `l2-nodus-runtime.md` §3.1 updated to note its verdict is now **Partially realized**. |
| 1.1.2 | 2026-09-24 | Core Team | Consistency pass (2026-09-24): The NL-9 row now states that the registry checks declared type names only; values bound at run time are not type-checked. |
| 1.1.3 | 2026-09-24 | Core Team | Design pass (2026-09-24): Related Specifications gains `l2-nodus-commands.md`, which specifies what a `^validator` name this registry admits means at run time — `len`, `min_len` and `required` evaluated by the core, every other name by the host's provider, an unevaluable rule failing closed. This spec still owns which names exist and checks only the name; no requirement here changed. |
