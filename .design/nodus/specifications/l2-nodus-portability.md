# Nodus Portability Implementation (Rust)

**Version:** 1.11.2
**Status:** Stable
**Layer:** implementation
**Implements:** l1-nodus-portability.md

## Overview

Concrete Rust implementation of the nodus portability and extension contract.
Maps each LP-invariant from `l1-nodus-portability.md` to its enforcing mechanism in
`crates/nodus`. Specifies the `SchemaProvider` vocabulary-extension seam, the
`InMemoryStorageProvider` built-in (conformant per LP-2/LP-15, though its executor wiring
stays unconsulted — §3.1) and the `PolicyProvider` per-effect authorization gate, wired
into `Executor::execute_command` for `ModelCall`/`Deferred` effects (§4.9, LP-11), plus the
optional consequence descriptors (§4.10, LP-16) that ride the same gate via the existing
`+modifier=value` grammar, and documents the extraction artifacts that deliver LP-6
compliance.

§3 maps LP-1…LP-8, the invariants that existed when this spec was written. §3.1
records the realization status of LP-9…LP-20, added to the L1 by later additive
refinement, against a four-way axis that distinguishes a seam with nothing to
attach to from one that ships but is never consulted.

## Related Specifications

- [l1-nodus-portability.md](l1-nodus-portability.md) — portability contract this spec implements
- [l2-nodus-runtime.md](l2-nodus-runtime.md) — runtime module structure this spec extends
- [l2-nodus-observability.md](l2-nodus-observability.md) — AuditProvider extension point (LP-2 example); HO-10 completeness anchors LP-20's reporting half
- [l1-nodus-language.md](l1-nodus-language.md) — NL-invariants that LP-invariants complement
- [l2-nodus-config.md](l2-nodus-config.md) — [ADDED v1.2.0] `ConfigProvider` role; already carries LP-2/LP-15 and LP-10 rows, and is the seam LP-19 should extend rather than parallel
- [l2-nodus-environment.md](l2-nodus-environment.md) — [ADDED v1.2.0] `EnvironmentProvider` role; NE-3/NE-7/NE-10 discharge the realized part of LP-18
- [l2-nodus-dialog.md](l2-nodus-dialog.md) — [ADDED v1.2.0] `DialogProvider` role; its `Status::Paused` + `ResumeDescriptor` lifecycle is the NL-12 substrate LP-14 waits on
- [l2-nodus-compensation.md](l2-nodus-compensation.md) — [ADDED v1.2.0] establishes the "vacuous in core" recording vocabulary §3.1 uses, and records the LP-11 gate as vacuous by composition
- [../../main/specifications/l1-interception-model.md](../../main/specifications/l1-interception-model.md) — [ADDED v1.3.0] the host-side decide-class contract LP-11 realizes; its Document History records the plugin-hook ↔ tool-security fail-behaviour divergence that §4.8.1 cites as LP-3 independence evidence
- [l2-nodus-errors.md](l2-nodus-errors.md) — [ADDED v1.4.0] owns the `NODUS:*` severity/category registry `POLICY_DENIED` (§4.9.4) registers into, beside the frozen 24-code set, matching the `CAPABILITY_UNMET` (LP-8) precedent; also documents that `@err:` dispatch is unrealized (NL-9), the finding §4.9.3 surfaced
- [l2-nodus-settlement.md](l2-nodus-settlement.md) — [ADDED v1.7.0] specifies LP-17's Rust shape: reuses this spec's own LP-11 gate unchanged for the decide half, adds a new `SettlementRail` trait + `ExtensionRole::Settlement` for the act half — following the `l2-nodus-compensation`/`l2-nodus-restart` precedent of a dedicated spec file for an invariant architecturally bigger than a `context`-extension

## 1. Motivation

`l1-nodus-portability.md` defines the invariants that govern how nodus stays decoupled
from any specific host — seven when this spec was written, twenty-three today (LP-1…LP-23) —
but the current `l2-nodus-runtime.md` only documents the runtime from an execution
perspective. No spec records:

- exactly which Rust constructs enforce each LP-invariant,
- the planned vocabulary-extension seam (`SchemaProvider`) documented in `vocab.rs` as
  a future refinement,
- the lifecycle of `StorageProvider` and `PolicyProvider` before they satisfy LP-3,
- or the extraction artifacts (CI workflow, `EXTRACTION.md`) as first-class portability
  deliverables.

This spec closes those gaps without duplicating the runtime implementation detail already
in `l2-nodus-runtime.md`.

## 2. Constraints & Assumptions

- All new traits introduced here follow LP-2: one built-in no-op implementation ships with
  the crate; concrete host implementations live outside the library.
- `SchemaProvider` may NOT modify `KNOWN_COMMANDS` or `RESERVED_VARIABLES` constants —
  it builds an extended `Schema` value on top of the builtin baseline.
- `StorageProvider`'s executor integration (hook points, run-parameter variants) is deferred
  to a future phase — its own LP-3 record (§4.8.2) is not satisfied. `PolicyProvider`'s is
  **not** deferred: LP-3 is satisfied for it (§4.8.1) and its call site is implemented
  (§4.9, LP-11) — this line described both traits identically before v1.5.0 shipped one of
  them; corrected here rather than left stale [CORRECTED v1.6.0].
- No new external crate dependencies may be introduced by any trait in this spec
  (LP-1 + LP-5 constraint).
- Elapsed-time measurement, run IDs, and timestamps follow the same conventions established
  in `l2-nodus-observability.md` (caller-supplied `run_id`; `std::time::Instant` for
  elapsed; wall-clock string for manifests).

## 3. Invariant Compliance

Explicit mapping of LP-1…LP-8 to its Rust enforcement mechanism. LP-9…LP-20 were added
to the L1 after this table was written; their realization status is §3.1. LP-21…LP-23
were added after §3.1 was closed; their realization status is §3.2.

| LP Invariant | Rust Enforcement |
| --- | --- |
| LP-1 Host neutrality | `crates/nodus/Cargo.toml` lists zero `[dependencies]` beyond `std`. The workspace `Cargo.toml` uses `workspace = true` for metadata fields only; no workspace crate is imported. Verified by extraction audit (Phase 3) and by CI's `cargo check --no-default-features`. |
| LP-2 Extension via abstract interfaces | Every integration point is a named Rust `trait`. Built-in implementations (`StubProvider`, `NoopAuditProvider`, `InMemoryStorageProvider`, `NoopPolicyProvider`) satisfy the interface with no I/O. Host implementations live outside the crate boundary. |
| LP-3 Two-host generalisation rule | Enforced by the spec amendment process: a new trait or vocabulary command reaches the library only after a `/magic.spec nodus` amendment documents two independent host-usage contexts, recorded as an **admission record** per L1 §4.14 (§4.8 here). Dispositions are per seam: **`PolicyProvider` is satisfied** (§4.8.1 — the runtime tool guard and the plugin-hook interception, two divergent host decision shapes) and may be wired; **`StorageProvider` is not** (§4.8.2 — one context only), so its executor hook points stay held. |
| LP-4 Vocabulary isolation | `KNOWN_COMMANDS` and `RESERVED_VARIABLES` are compile-time constants in `vocab.rs`. `SchemaProvider` builds an extended `Schema` value at runtime without mutating these constants. Host-specific commands are schema artifacts, never constants. |
| LP-5 Composable extension | **Partial.** The original pair composes — `run_with_provider`, `run_with_audit` and `run_with_provider_and_audit` take the model and audit providers in orthogonal parameters — and there is no global mutable state and no inheritance; combinators are pure functions in `workflows.rs`. Every seam added since (schema, manifest, dialog, policy, settlement, environment, config) arrived as its own entry point that fixes all the others to their built-ins, `StubProvider` included, so no two of them — and none of them with a real model — can be used in one run. The seams are independent units but not independently *composable*, which is the half LP-5 requires. Closing it is the composed entry point `l2-nodus-runtime.md` §4.5 records. |
| LP-6 Semantic versioning contract | Published via `crates/nodus/Cargo.toml` with `version = "{semver}"`. Breaking changes follow the notice protocol in `l1-nodus-portability.md §4.5`. CI enforces `cargo semver-checks` (planned; currently manual). `EXTRACTION.md` documents the release checklist. |
| LP-7 Feedback loop lifecycle | Operationalised by the `/magic.spec nodus` → `/magic.task nodus` → `/magic.run nodus` pipeline. Discovery and distillation steps happen in `l1-nodus-portability.md §4.2`; spec amendment is the Proposal step; the run pipeline is Implementation; `CHANGELOG.md` + version bump is Release. |
| LP-8 Capability manifest | **Implemented (§4.7).** `portability.rs` defines `CapabilityManifest` (roles / host commands / named capabilities), `HostCapabilities`, and the pure `validate_manifest` resolver. `workflows.rs` runs the gate in `run_with_manifest` / `run_with_manifest_and_audit` after lint validation and before the executor boots, rejecting fail-fast with a `NODUS:CAPABILITY_UNMET` error that names the missing-capability set and emitting no audit events (the executor is never invoked). `CapabilityManifest::from_workflow` derives the manifest from invoked commands. The same resolver is the machine-checkable LP-3 host-substitution test. |

### 3.1 Invariants added after v1.0.0 — realization status [ADDED v1.2.0]

`l1-nodus-portability.md` has grown LP-9 … LP-20 through additive refinement since the
§3 table was written. Every one of those invariants' Document History entries closes with
the same sentence — *"l2-nodus-portability carries LP-{N} as a pending Invariant-Compliance
obligation reconciled at magic.task"* — so they are listed here explicitly: an obligation
tracked only in a plan document is not tracked in the place its own contract points to.

These twelve are host-supplied seams, so the binary *Realized / Pending* axis used by
`l2-nodus-runtime.md` §3.1 would misreport them. A four-way axis is used instead:

- **Satisfied structurally** — enforced by what the crate contains and, more often, by what
  it deliberately lacks. Verifiable today; the standing check is a negative one.
- **Vacuous in core** — the construct the invariant gates does not exist in the crate, so the
  seam has no call site. Nothing is owed until that construct lands, and the obligation
  attaches to whoever builds it. (Vocabulary established by `l2-nodus-compensation.md` §2.)
- **Seam declared, wiring pending** — the trait ships and is re-exported from `lib.rs`, but no
  code path consults it: a host implementing it today observes **no behaviour change**. This
  is published API that silently does nothing, not deferred work. (The crate's own phrasing
  for this state is "interface-declared, wiring pending".)
- **Partially realized** — some of the invariant's rules are discharged, named individually.

| L1 Invariant | Realization status |
| --- | --- |
| LP-9 Extraction attestation | **Vacuous in core.** No bundle-load path exists (zero occurrences of `bundle` / `witness` / `attest`-as-verification), so the verify-before-load hook has no call site. The `EXTRACTION.md` + `ci.yml` artifacts are LP-6 deliverables (§4.6), not an LP-9 witness. Nodus never signs — verification is the host's (LP-2) — so the core's obligation begins only when an import path does. |
| LP-10 Host-granted authority, never self-authored | **Satisfied structurally.** Discharged by absence of vocabulary: `KNOWN_COMMANDS` holds no policy-writing command; `CapabilityManifest` exposes only `require_*` builders (declare) and getters (read) with no grant/relax/widen operation; acceptance authority sits behind `ConfigProvider` (`l2-nodus-config.md` LP-10 row); `Budget` is enforced by the run loop and unreadable by the workflow (`l2-nodus-environment.md` NE-13). Load-bearing in four sibling specs (cited by NL-14 / NL-17 / NL-20 DC-10 / NL-21) while having had no row here — the reason it needs one. |
| LP-11 Per-effect authorization seam | **Implemented [v1.5.0].** `Executor::execute_command` gates every `ModelCall`/`Deferred` effect (`EffectClass{ModelCall,Deferred}`, reusing `MODEL_COMMANDS`/`DIALOG_COMMANDS`; `ToolUse` deliberately unrealized — every tool-shaped builtin command is a fixed stub with no host-swappable seam to gate) through `self.policy.evaluate(gate, context)` before the effect happens — `gate` the effect-class string, `context` a raw pre-resolution `Value::Map` snapshot (§4.9.2). On denial: `ExecutionEvent::StepError` with `NODUS:POLICY_DENIED` (not `ConstraintHit`, which stays scoped to hard `!!`-constraints per §4.4), a `RuntimeError` pushed to `ctx.errors`, and a bare `None` return — non-halting, mirroring `DialogOutcome::Timeout`/`::Rejected`; `RunResult.status` degrades to `Partial`, the pipeline target stays at its seeded default. `Executor` carries a fourth `policy: Box<dyn PolicyProvider>` field (default `NoopPolicyProvider`) across every constructor; `run_with_policy`/`run_with_policy_and_audit` mirror `run_with_dialog`'s exact shape. `POLICY_DENIED` now also reaches the declared `@err:` handler (`l2-nodus-error-dispatch.md`, built after this row was written). **The gate never meets a real effect (pending):** `run_with_policy*` fixes `StubProvider` and the no-op settlement rail, while `run_with_provider*` and `run_with_settlement*` fix `NoopPolicyProvider`, which permits everything — so through the public API the gate governs only stub calls, and every real model call or settlement is ungated (LP-5 row; `l2-nodus-runtime.md` §4.5). |
| LP-12 Imported-bundle admission vetting | **Vacuous in core.** Same absent load path as LP-9, and explicitly sequenced after it (attest → vet → run, §4.8 of the L1). No classifier call site can exist before a bundle can be loaded. |
| LP-13 Addressable versioned import resolution | **Vacuous in core.** No resolver, catalog, pin record, or reference form. `@runtime: { core: <schema-file> }` names a schema file but performs no resolution or pinning. Precedes LP-9/LP-12 in the load order (resolve → attest → vet → run), so the three are one deliverable, not three. |
| LP-14 Verified-peer delegation seam | **Vacuous in core**, with a named upstream blocker: it specializes NL-12 generalized deferred execution, which `l2-nodus-runtime.md` §3.1 records as **Pending** — the `Status::Paused` + `ResumeDescriptor` lifecycle exists but is human-answer-shaped. A peer seam cannot precede the generalized deferred step it is a special case of. |
| LP-15 Host-supplied durable-state seam | **Seam declared, wiring pending — the built-in divergence is RESOLVED.** `StorageProvider` ships `InMemoryStorageProvider`, which round-trips within one invocation and shares no state across instances, matching L1 §4.1's *in-memory* built-in mandate ("non-durable; state discarded between invocations") and LP-2's "sufficient for in-process testing without I/O" — neither of which the prior `NoopStorageProvider` satisfied (`store` discarded, `load` always returned `None`). The **wiring** half remains open: `store`/`load` still have zero call sites, held by the LP-3 record (§4.8.2 — one host context, not two). One naming divergence from the L1 persists, deliberately unaddressed here: L1 §4.11 names `put`/`get`/`delete`; the crate keeps `store`/`load` and no `delete` — changing the trait signature would be a second breaking change with no conformance gain, since the interface was already LP-1-clean (no host type named). |
| LP-16 Effect risk-class declaration | **Implemented [v1.6.1].** `execute_command`'s existing LP-11 `context` literal (§4.9.3) now carries `reversible`/`external`/`value` when the step declares them via the *existing* `+modifier=value` grammar (`CommandCall.modifiers`) — zero new DSL/AST/parser/transpiler work, exactly as designed. `PolicyProvider::evaluate`'s signature is untouched — no LP-6 change, no new failure mode. A descriptor is omitted, never defaulted, when the step carries no matching modifier (verified: an undecorated step's `context` contains no `reversible`/`external`/`value` key at all). 447 tests pass (was 444, +3); no new unit tests needed — there is no new type or function to unit-test, only a loop inside an existing call site. |
| LP-17 Settlement effect seam | **Implemented [v1.7.1]**, with the halves unable to meet in one run — a real rail runs only under the permit-everything policy (`l2-nodus-settlement.md` §3, pending). Realized by [l2-nodus-settlement.md](l2-nodus-settlement.md). Splits on LP-11's own decide/act seam: the decide half needed no design of its own — `EffectClass::Settlement` reuses `self.policy.evaluate("settlement", context)` unchanged, `context.args` carrying `[payee, amount, purpose]` positionally per §4.9.2's existing raw-args shape; the act half needed a genuinely new `SettlementRail` trait, since `PolicyProvider::evaluate` returns only `bool` and has no channel for a receipt (VS-7) — the first LP-9…LP-20 item needing a new extension-point trait rather than reusing one already in the crate. VS-6's payment-required handshake stays explicitly out of nodus-core (host/workflow-authoring territory, not a new control-flow primitive). Realized in its own spec file, mirroring the `l2-nodus-compensation`/`l2-nodus-restart` precedent: architecturally comparable in scope to a new provider trait, not a small `context`-extension like LP-16. |
| LP-18 Environment-liveness seam | **Partially realized.** Rule (c) is discharged — `EnvInteraction` carries environment attribution on existing events (`l2-nodus-environment.md` NE-3), `release` is mandatory + idempotent behind an `InstanceGuard` drop guard (NE-7), and the capability half is complete (NE-10: `ExtensionRole::Environment` + `validate_manifest` fail-fast, provided by `HostCapabilities::builtin()`). Rules (a) and (b) are **not**: `EnvironmentProvider::open` returns `Instance`, not a `Result`, so a vanished or un-openable environment has no typed failure path; no `NODUS:*` code is environment-liveness-shaped; and `ResumeDescriptor` is dialog-shaped, carrying no environment identity, so a resume cannot detect that its environment is gone. Closing (a)/(b) changes a published trait signature — LP-6 **major**, not additive. |
| LP-19 Host-supplied exposure switch seam | **Vacuous in core.** Zero `rollout` / `holdout` vocabulary; the `switch` occurrences are the `?SWITCH` control-flow construct, unrelated. Its nearest neighbour is `§config` (NL-20, `l2-nodus-config.md`), which already shares the resolve-once / host-accepts / workflow-read-only shape but lacks the two rules that make LP-19 what it is: a **declared safe value** and the unresolvable-switch fallback path. When LP-19 lands it should extend the `ConfigProvider` acceptance shape rather than introduce a parallel seam. |
| LP-20 Obligation-gated effect seam | **Vacuous in core.** Zero `obligation` / `discharge` vocabulary. Its gating half reuses the now-wired LP-11 ordering [v1.5.0] but still needs its own obligation-declaration concept before there is anything to gate. Its *reporting* half is nearer: "an obligation open at run end is surfaced in the terminal `RunManifest`" has a real anchor, since `RunManifest` exists and `l2-nodus-observability.md` HO-10 already carries completeness honesty (`TraceCompleteness` / `classify_trace`). |

**Leverage [UPDATED v1.7.1].** Of the twelve, three are **Implemented**: LP-11 (Phase 24,
v1.5.0), LP-16 (Phase 25, v1.6.1), and LP-17 (Phase 27, `l2-nodus-settlement.md` v1.0.1) —
the remaining nine split six vacuous
(LP-9/12/13/14/19/20), one seam-declared-wiring-pending (LP-15), one satisfied
structurally (LP-10), one partially realized (LP-18). **LP-11's call site landing is what
unblocked both LP-16 and LP-17** — LP-16's consequence descriptors ride the exact same
`execute_command` gate via the existing `+modifier=value` grammar (one context-enrichment
change, no new hook); LP-17's decide half reuses the identical gate call unchanged
(`context.args` already carries `[payee, amount, purpose]` positionally), but — unlike
LP-16 — its act half could not be freed by wiring alone, since deciding and settling are
different kinds of operation (a boolean permit vs. an action returning a receipt):
`SettlementRail` is the crate's first genuinely new §3.1 extension-point trait since the
LP-11 gate itself landed. LP-20 remains blocked the LP-16 way, not the LP-17 way — it still
needs its own `obligation`/`discharge` vocabulary designed before anything can carry through
the now-open gate. The two other nearest-to-plannable items stay independent of LP-11
entirely: the LP-15 built-in divergence (small and self-contained, resolved at v1.3.1) and
LP-18 (a)/(b) (needs an L2 design decision first, because a `Result`-returning environment
lifecycle is a breaking change).

**Import triad.** LP-13 → LP-9 → LP-12 are the ordered stages of one absent mechanism.
Recording them as three independent gaps overstates the backlog; they land together or
not at all.

### 3.2 Invariants added after v1.7.1 — realization status [ADDED v1.8.0, LP-22(c) IMPLEMENTED v1.10.0]

Three further invariants — LP-21, LP-22, LP-23 — landed on `l1-nodus-portability.md`
after §3.1's table was closed at LP-20. They are recorded here on the same four-way axis
§3.1 defines — *satisfied structurally* / *vacuous in core* / *seam declared, wiring pending* /
*partially realized* — and for the same reason: an obligation tracked only in a plan document is not tracked in
the place its own contract points to. Each verdict below was taken against `crates/nodus`
source rather than against the L1 text alone, and each names what would have to exist
before the invariant becomes task-authorable.

| L1 Invariant | Realization status |
| --- | --- |
| LP-21 Declared role-provider resolution | **Vacuous in core.** `Executor` holds exactly one provider per role — five `Box<dyn _>` fields (`provider`, `audit`, `dialog`, `policy`, `settlement`), each set by a constructor or a `run_with_*` combinator and each defaulting to its built-in. There is no candidate set, no declaring locus, no declared priority and no registration order, so all four ordering rules have nothing to order: a *second* candidate for one role cannot currently be expressed, let alone resolved between. The obligation attaches to whoever first builds a provider registry, exactly as LP-9/LP-12/LP-13 attach to whoever builds an import path. Its two honesty rules split, and only one is genuinely absent. **Declared refusal** already holds at a coarser grain — an operation a built-in does not perform is a fixed, documented no-op (`NoopAuditProvider`, `NoopPolicyProvider`, `NoopSettlementRail`), never a crash and never a silent fall-through, because with one binding per role there is nothing to fall through *to*. **Observable resolved binding** has no anchor at all: `RunManifest` records execution mode, exposure switches and reproduction recipe (`l2-nodus-observability.md` §4.7) but records nothing about *which* implementation filled which role, so an LP-8 satisfiability claim travels without the fact that would make it checkable. **Correction [v1.11.0] — this half is not the small residue it was described as.** Recording an identity needs one of the identified traits to *have* one first — none of `ModelProvider`/`AuditProvider`/`DialogProvider`/`PolicyProvider`/`SettlementRail` (`Executor`'s five, out of LP-8's full nine-role taxonomy — `StorageProvider`/`SchemaProvider`/`ConfigProvider`/`EnvironmentProvider` are wired through separate `run_with_*` paths `Executor` cannot see at all) exposes a `name()`-shaped method, so adding one is a breaking change to at least five trait definitions and every built-in and host implementation of them, not "one manifest field." And even built, what it would record is hollow: with exactly one binding per role and no candidate set (per this row's own opening), "which implementation filled the role, at which locus, and why it won" degenerates to "the only one there was, trivially" — the invariant's own text presumes a competition to disambiguate, which a single-binding architecture does not have. Genuinely vacuous in core, in full, until a registry exists — no partial win available without one. |
| LP-22 A run is pinned to the definition generation it validated against | **Partially realized**, splitting along its own three consequences rather than resolving to one verdict. **(a) An edit, replacement or deletion disturbs no already-pinned run — satisfied structurally.** Every `run*` entry point parses source text into a `WorkflowFile` and hands *that value* to the executor; the definition is never re-read, re-resolved or re-stat'ed mid-run, so the pinning LP-22 requires falls out of owning the parsed AST rather than a path. Discharged by construction, not by design — worth stating plainly, because nothing would fail if a later change introduced a mid-run re-read: no test asserts the property. **(b) A nested or imported run inherits its parent's generation by reference — vacuous in core.** No import or nested-run path exists (the LP-13 → LP-9 → LP-12 triad), so there is no re-resolution to forbid. **(c) A resume descriptor names the pinned generation — Implemented [Phase 31, v1.10.0].** `ResumeDescriptor { workflow: String, vars, step_index, workflow_digest: String }` — the former `"wf:<name>"`-only shape carried precisely the launch-time *name* LP-22 (c) says must not be used alone. **Correction (recorded at v1.9.0) to this row's own earlier "a resume re-enters whatever that name now resolves to"**: nodus performs no name resolution at all on resume — every `run`/`run_with_*` entry point takes `source: &str` directly and `paused_at` is never read back by any function, so "resume" is host-orchestrated re-invocation (the host supplies source text again), never a crate-side lookup. The risk LP-22 (c) names is therefore entirely on the host side of the boundary — a host that re-resolves a workflow by name from mutable storage may silently supply edited text — which is why the fix is **declared, host-verifiable data**, not a core-side check: `workflow_digest`, computed by the same `digest_ast` function `ReproRecipe.workflow_digest` (`l2-nodus-observability.md` §4.7) already uses, so a host compares the descriptor's digest against a fresh digest of whatever source it is about to resume with. Built and tested exactly per the design (`l2-nodus-dialog.md` §4.3 v1.4.0, which owns and details the field — this document tracks only the realization verdict): a unit test confirms the digest equals `digest_ast` for the parsed workflow, an integration test confirms it agrees with the independently-built `ReproRecipe.workflow_digest` for the same paused run. |
| LP-23 Versioned capability manifest & per-capability degrade | **Partially realized.** The `required` half is discharged by construction and by the L1's own additivity clause: an entry with no declared version and no declared optionality must behave exactly as LP-8 does today, and `validate_manifest`'s fail-fast *is* that behaviour, so the shipped crate is already a conforming implementation of the degenerate case. Neither new dimension exists. **Per-entry `{major, minor}` lines**: `CapabilityManifest` stores three presence-only sets (`roles: BTreeSet<ExtensionRole>`, `commands: BTreeSet<String>`, `capabilities: BTreeSet<String>`) and satisfaction is a presence question against `HostCapabilities`, so a version line per entry changes the **entry type** — a set of names becomes a set of name-plus-line pairs — and with it every `require_*` builder, every getter and the satisfaction predicate. That is not a field addition but an LP-6 decision: whether the existing builders survive as "no line declared" or the type break is taken. **Optional entries with a declared degrade**: *skip* needs a notion of a guarded branch that does not run, and *fallback* must name an alternative step — the core has no step-substitution mechanism of any kind, so the fallback form has no realization path until one exists, while *skip* is the reachable half and sits nearer `?SWITCH`'s arm dispatch than anything in `portability.rs`. **The diagnostic half is nearly free**: `capability_rejection` already names the missing-capability set, so naming both lines and which side must move ("requires Storage 2.x; host supplies 1.4 — upgrade the host, or lower the declared line") is a message change over data the resolver would already hold once lines exist. |

**What is plannable, and what is not [ADDED v1.8.0; LP-22(c) IMPLEMENTED Phase 31, v1.10.0; LP-21 correction v1.11.0].**
LP-21 and LP-23 are both fully vacuous/blocked with no small residue to peel off either one —
**LP-21 has no construct to attach to at all** (the earlier "observable binding is a cheap
partial win" framing did not survive grounding: it needs a `name()`-shaped method added to at
least five trait definitions, a real breaking-change surface, and even built would record a
one-candidate "resolution" the invariant's own competitive framing does not describe), while
LP-23 has a construct but carries an open type or LP-6 decision alongside it. **LP-22 (c) is
done** — `l2-nodus-dialog.md` §4.3 v1.4.0's `workflow_digest` field is built and tested,
host-compared at resume time since the core has no resume-time call site of its own. Neither
LP-21 nor LP-23 is task-authorable until a genuinely new decision is made — a provider
registry for the former, a `CapabilityManifest` entry-type change and a step-substitution
mechanism for the latter's `fallback` degrade — each a larger design act than this workspace's
usual `/magic.spec` pass settles in one sitting.

**Cross-spec note — corrected [v1.9.0]: LP-22 (c) and NL-26 are independent, not a shared
decision.** A prior version of this note treated LP-22 (c)'s pinned generation and NL-26's
correlation-token invalidation (`l2-nodus-runtime.md` §3.1) as two fields on the same struct
needing one joint decision. They do not: LP-22 (c) is now fully designed (above) and needs no
further decision at all, while NL-26's correlation token remains **vacuous in core** for a
reason specific to it and unrelated to `workflow_digest` — it specializes NL-12's generalized
deferred-external-step architecture, which does not exist, and its "an elapsed deadline
invalidates the token so a late completion is rejected" rule presupposes an asynchronous
boundary that `DialogProvider::ask`/`confirm`'s synchronous, single-call shape does not have
(the interaction is fully resolved, one way or another, before `ask` returns — there is no
"later" for a completion to arrive *in*). See `l2-nodus-runtime.md` §3.1 for the full
separation. `ResumeDescriptor` gains exactly one new field from this session's work
(`workflow_digest`, LP-22 (c)); NL-26 gains none, and adds none until NL-12 is designed.

**Finding resolved [RESOLVED v1.10.0, was ADDED v1.9.0]: `workflow_digest` was two different
computations under one name.** `ReproRecipe.workflow_digest` (this crate's `observability.rs`,
via `digest_ast` — hashes the parsed AST's `Debug` form) and `l2-nodus-environment.md`'s
`CandidateResult.workflow_digest` (via `digest_source` — hashes the raw source string)
answered a different question — content identity vs. byte identity — under the identical
field name, and `l2-nodus-observability.md` §4.7's own inline comment cited the latter as
"the NE-12 precedent" for the former, overstating the relationship at the time it was
written: the two were not guaranteed equal for a byte-identical file, let alone a
cosmetically-edited one. `l2-nodus-dialog.md` §4.3's new field deliberately followed
`digest_ast` (content identity is the right grain for a pinning check) rather than adding a
third inconsistent computation, which is what surfaced this. **Implemented [Phase 32] in
`l2-nodus-environment.md` v1.3.0**: `EnvRunResult::candidate()` hashes the parsed AST via the
identical `digest_ast` function (widened to `pub(crate)`, not duplicated), falling back to the
old `digest_source` byte hash only if the given source fails to parse — proven by a test
cross-checking `CandidateResult.workflow_digest` against `ReproRecipe.workflow_digest` for the
same run, not merely assumed from both reusing the same function. `l2-nodus-observability.md`
§4.7's "the NE-12 precedent" comment needed no correction — it is accurate now that the two
fields agree.

## 4. Detailed Design

### 4.1 SchemaProvider — Vocabulary Extension Seam

`Schema`'s public query surface (`is_command`, `is_reserved`, `is_valid_tone`) is the
seam for runtime vocabulary extension noted in `vocab.rs`. `SchemaProvider` formalises
the interface:

```text
[REFERENCE]
pub trait SchemaProvider {
    /// Return the host-declared command names that extend the builtin baseline.
    /// Return an empty slice to use the builtin vocabulary unchanged.
    fn host_commands(&self) -> &[&str];

    /// Return additional reserved variable names beyond RESERVED_VARIABLES.
    /// Return an empty slice to use the builtin set unchanged.
    fn host_reserved_variables(&self) -> &[&str];
}

/// Built-in provider: no host extensions; pure builtin schema.
pub struct BuiltinSchemaProvider;

impl SchemaProvider for BuiltinSchemaProvider {
    fn host_commands(&self) -> &[&str] { &[] }
    fn host_reserved_variables(&self) -> &[&str] { &[] }
}
```

`Schema` gains a companion constructor:

```text
[REFERENCE]
impl Schema {
    /// Build an extended schema merging the builtin baseline with host additions.
    /// Host commands that collide with KNOWN_COMMANDS are silently deduplicated.
    pub fn with_provider(provider: &dyn SchemaProvider) -> Self { ... }
}
```

The validator and executor receive the merged `Schema` value; they do not call the
provider directly. This preserves their existing interface and keeps the seam at the
boundary of schema construction.

### 4.2 Extension Point Registry (Rust)

Full registry of LP-2 extension points and their implementation status in `crates/nodus`.
The rows correspond one-to-one with the `ExtensionRole` variants (§4.7); *Wired* records
whether any code path actually consults the trait, which is the distinction §3.1 turns on:

| Role | Trait | Built-in | Status | Wired |
| --- | --- | --- | --- | --- |
| Model | `ModelProvider` | `StubProvider` | Implemented (`executor.rs`) | Yes |
| Audit | `AuditProvider` | `NoopAuditProvider` | Implemented (`observability.rs`) | Yes |
| Vocabulary | `SchemaProvider` | `BuiltinSchemaProvider` | Implemented (`portability.rs`; `Schema::with_provider`, `run_with_schema`) | Yes |
| Dialog | `DialogProvider` | `DefaultDialogProvider` | Implemented (`executor.rs`; `l2-nodus-dialog.md`) | Yes |
| Environment | `EnvironmentProvider` | `StubEnvironment` | Implemented (`environment.rs`; `l2-nodus-environment.md`) | Yes |
| Config | `ConfigProvider` | `DefaultConfigProvider` | Implemented (`portability.rs`; `l2-nodus-config.md`) | Yes |
| Storage | `StorageProvider` | `InMemoryStorageProvider` | Implemented (`portability.rs`; round-trips within an invocation, LP-2/LP-15 conformant — §4.3) | **No** — zero call sites (LP-15, §3.1) |
| Policy | `PolicyProvider` | `NoopPolicyProvider` | Implemented (`executor.rs`; gates `ModelCall`/`Deferred` effects — §4.9) | **Yes** |

> `Dialog`, `Environment`, and `Config` are roles the implementation added after the L1
> §4.1 taxonomy table was last amended; that table still lists four. L1 §4.1 declares itself
> "the authoritative extension point registry", so the divergence is recorded here and
> reconciled L1-side rather than resolved unilaterally by this L2.

### 4.3 StorageProvider — Built-In Conformant, Wiring Pending LP-3

The trait and its built-in satisfy LP-2/LP-15 today; only executor integration remains
gated:

```text
[REFERENCE]
pub trait StorageProvider {
    /// Persist a named value. `key` is host-defined; nodus treats it as opaque.
    fn store(&self, key: &str, value: &crate::executor::Value);

    /// Retrieve a named value. Returns `None` if the key is absent.
    fn load(&self, key: &str) -> Option<crate::executor::Value>;
}

/// In-memory built-in: round-trips within one process; instances share no
/// state. Mutex-guarded; a poisoned lock degrades to empty rather than panics.
pub struct InMemoryStorageProvider { /* Mutex<Vec<(String, Value)>> */ }

impl StorageProvider for InMemoryStorageProvider {
    fn store(&self, key: &str, value: &crate::executor::Value) { /* insert-or-overwrite */ }
    fn load(&self, key: &str) -> Option<crate::executor::Value> { /* clone if present */ }
}
```

`InMemoryStorageProvider` replaced the prior no-op built-in (`NoopStorageProvider`,
`store` discarded, `load` always returned `None`) because L1 §4.1's Storage row specifies
an **in-memory** default and LP-2 requires every built-in be "sufficient for in-process
testing without I/O" — a discarding store cannot round-trip and so satisfied neither. A
discarding **audit** built-in stays fine, since audit is write-only; the store/audit
asymmetry is why this fix does not generalize to `NoopPolicyProvider` or
`NoopAuditProvider`. This is a public-API rename, acceptable at the crate's pre-1.0
version (LP-6 §5's `cargo semver-checks` gate lands at 1.0.0).

Executor integration (hook points for `STORE`/`LOAD`/`RECALL`/`REMEMBER` commands) remains
deferred — LP-3 not satisfied for this seam (§4.8.2); those commands currently operate
against the in-memory variable environment. LP-3 graduation also triggers the addition of
`run_with_storage` and `run_with_storage_and_audit` API variants to `workflows.rs`.

### 4.4 PolicyProvider — Implemented (LP-11) [MODIFIED v1.5.0]

Wired: `Executor::execute_command` gates every `ModelCall`/`Deferred` effect through
`evaluate` before the effect happens. The trait signature is unchanged from what shipped —
only its callers are new (§4.9):

```text
[REFERENCE]
pub trait PolicyProvider {
    /// Evaluate a named policy gate. Returns `true` if the action is permitted.
    ///
    /// `gate` is the effect-class string (`"model_call"` / `"deferred"`,
    /// `EffectClass::as_gate_str`). `context` is a pre-effect `Value::Map`
    /// snapshot of the command and its unresolved argument strings — §4.9.2.
    fn evaluate(&self, gate: &str, context: &crate::executor::Value) -> bool;
}

pub struct NoopPolicyProvider;

impl PolicyProvider for NoopPolicyProvider {
    fn evaluate(&self, _gate: &str, _context: &crate::executor::Value) -> bool { true }
}
```

The `evaluate` contract is deliberately narrow: boolean permit/deny, no mutation. Spend
tracking, approval workflows, and tool-access lists are host-side concerns; the interface
only passes context to the host's decision function.

**[CORRECTED v1.4.0]** This doc comment previously claimed "the executor emits
`ConstraintHit { halt: false }` and skips the step if `false` is returned". That was
wrong on inspection of `observability.rs`: `ConstraintHit`'s own doc comment scopes it to
"a hard (`!!NEVER`/`!!ALWAYS`) constraint" — a language-level absolute rule, not a
host-side policy decision — and `l2-nodus-errors.md` states the `RULE_VIOLATION`/
`ConstraintHit` path is "unchanged" by any later spec. Reusing that event for a policy
denial would silently conflate two different invariants (NL-2 vs LP-11) under one event
type and violate a standing guarantee. §4.9 specifies the real, distinct mechanism.

### 4.5 Module Additions

Delivered (`SchemaProvider` per §4.1, the manifest per §4.7):

```text
[REFERENCE]
crates/nodus/src/
├── ...
├── vocab.rs          — Schema::with_provider constructor
└── portability.rs    — SchemaProvider  + BuiltinSchemaProvider   (wired)
                        ConfigProvider  + DefaultConfigProvider   (wired, l2-nodus-config)
                        StorageProvider + InMemoryStorageProvider (LP-2/LP-15 conformant; executor call sites still pending — §3.1 LP-15)
                        PolicyProvider  + NoopPolicyProvider      (wired — §4.9, LP-11 [CORRECTED v1.6.0: was stale "interface only"])
                        ExtensionRole / CapabilityManifest / HostCapabilities / validate_manifest
```

`lib.rs` re-exports from `portability` follow the same pattern as `observability`.

### 4.6 Extraction Artifacts as LP-6 Deliverables

LP-6 compliance requires two publishable artifacts already delivered in Phase 3:

| Artifact | Path | LP-6 role |
| --- | --- | --- |
| CI workflow | `crates/nodus/.github/workflows/ci.yml` | Validates `check + test + clippy + fmt + doc` on every push; acts as the regression gate for semantic versioning |
| Extraction procedure | `crates/nodus/EXTRACTION.md` | Seven-step checklist for human extraction to a standalone repository; includes `cargo semver-checks` step |
| Cargo manifest | `crates/nodus/Cargo.toml` | `version`, `description`, `keywords`, `categories`, `homepage`, `documentation`, `docs.rs` config — all required for crates.io publication |

### 4.7 Capability Manifest & Pre-Run Validation (LP-8)

Realizes `l1-nodus-portability.md` §4.6. A workflow's manifest and the host's
provisions are two values; a pure resolver compares them before the executor
boots, so an unsatisfiable run never starts.

The extension-point taxonomy (§4.2) is a closed enum, and the manifest is three
ordered sets — ordered so diagnostics are deterministic. An empty manifest is
satisfied by any host:

```text
[REFERENCE]
pub enum ExtensionRole {
    Model, Audit, Storage, Policy, Vocabulary,   // original five
    Dialog, Environment, Config,                 // added by later L2 work (§4.2)
}

pub struct CapabilityManifest {
    roles:        BTreeSet<ExtensionRole>,
    commands:     BTreeSet<String>,   // host-schema (non-builtin) commands
    capabilities: BTreeSet<String>,   // named finer-grained features of a role
}
```

A manifest may be authored explicitly (`require_role` / `require_command` /
`require_capability`) or derived from a workflow's AST:

```text
[REFERENCE]
impl CapabilityManifest {
    pub fn from_workflow(ast: &WorkflowFile) -> Self;
}
```

`from_workflow` walks every command (descending into conditionals, loops, and
parallel branches): a model-backed command (`GEN`/`ANALYZE`) requires `Model`; a
dialog command (`ASK`/`CONFIRM`) without a `+default` requires `Dialog`; a
command outside the builtin vocabulary requires `Vocabulary` and is recorded as a
required command name; a builtin non-model command requires nothing. `Storage`,
`Policy`, and `Environment` have no corresponding command syntax and are never
derived — a caller requires them explicitly via `require_role` (`run_with_environment`
does so on every call, since calling it is itself the declaration). Explicit DSL
declaration (an `@needs` section) is deferred to the upstream-parity backlog.

`HostCapabilities` is what the active host provides. It is constructed
explicitly, so the same type serves both the built-in in-process configuration
and host-substitution tests (LP-3):

```text
[REFERENCE]
pub struct HostCapabilities { /* roles, commands, capabilities */ }

impl HostCapabilities {
    pub fn builtin() -> Self;                              // Model + Audit + Vocabulary
    pub fn provides(&self, role: ExtensionRole) -> bool;
    pub fn has_command(&self, command: &str) -> bool;
    pub fn satisfies(&self, capability: &str) -> bool;
}
```

`HostCapabilities::builtin()` provides five roles: `Model` (the `StubProvider`),
`Audit` (a sink is always wired), `Vocabulary` (the builtin schema),
`Environment` (`StubEnvironment`), and `Config` (`DefaultConfigProvider`) — the
last two because each built-in is a *complete trivial* implementation, so a
workflow declaring the need stays runnable in-process. It provides neither
`Storage` nor `Policy` (interface-only, §4.3/§4.4), and deliberately not
`Dialog`, whose default resolver is incomplete: a workflow that asks a question
no host can answer must fail its manifest rather than silently proceed. A
model-only or manifest-free workflow therefore runs against the built-in host
with no wiring.

The resolver is pure — no I/O, no events — and returns every unmet item by name:

```text
[REFERENCE]
pub enum Missing { Role(ExtensionRole), Command(String), Capability(String) }

pub fn validate_manifest(
    manifest: &CapabilityManifest,
    host: &HostCapabilities,
) -> Vec<Missing>;   // empty ⇒ satisfiable
```

The gate lives in `workflows.rs` as two combinators, consistent with the existing
orthogonal `run_with_*` family (LP-5):

```text
[REFERENCE]
pub fn run_with_manifest(
    source, filename, input,
    manifest: &CapabilityManifest, host: &HostCapabilities,
) -> Result<RunResult, Vec<Diagnostic>>;

pub fn run_with_manifest_and_audit(
    source, filename, input,
    manifest, host, audit, run_id, started_at,
) -> Result<RunResult, Vec<Diagnostic>>;
```

Both run the resolver after lint validation and before the executor boots. A
non-empty `missing` set yields a fail-fast `RunResult`: `Status::Failed`, zero
steps logged, and one `NODUS:CAPABILITY_UNMET` error naming each missing
capability. On the audited variant the audit sink receives nothing — no events,
no run-complete callback — because the executor is never invoked (observer
neutrality for runs that never start, HO-3/HO-5). A satisfiable manifest delegates
to the stub executor. This is the machine-checkable LP-3 contract: a workflow is
portable to a host exactly when that host satisfies its manifest.

### 4.8 LP-3 Admission Records [ADDED v1.3.0]

Admission records per `l1-nodus-portability.md` §4.14, which makes the LP-3 gate checkable.
Dispositions are per seam; the two open seams reach **different** outcomes, which is the
rule working rather than a half-finished pass.

#### 4.8.1 `PolicyProvider` (LP-11) — **LP-3 SATISFIED**

| Field | Record |
| --- | --- |
| **Context A** | **Runtime tool guard** — `crates/domain/src/tool_security.rs`. Decides: may this tool call execute? Evaluates *every* tool call before execution against named severity-ranked signature rules ("Layer 2 (guard) evaluates every tool call before execution"). Fails by severity classification. |
| **Context B** | **Plugin-hook interception** — `crates/domain/src/hooks.rs`. Decides: does any registered hook block this tool event? Nine `HookEvent` kinds; hooks run **in parallel** and the aggregated decision is deny if *any* returns `Block`. Fails by aggregation. |
| **Independence** | Documented divergence, and not by our reading: the host's own Stable `l1-interception-model.md` exists to unify "the **currently-divergent fail-behaviours of the plugin-hook and tool-security realizations**" (its Document History, INT-3). Two mechanisms that already disagree on fail direction — severity classification vs parallel deny-aggregation — are the §4.14 strongest signal. Neither derives its behaviour from the other. |
| **Host types the seam must not name** | `HookEvent`, the hook rule/matcher types, `tool_security::Severity`, the scanner's signature-rule types, and `contract::*`. Verified against the interface as it stands: `evaluate(&self, gate: &str, context: &Value) -> bool` names only `&str` and nodus's own `Value`, so LP-3's second half is **already satisfied by construction** — the interface needs no change to pass. |
| **Disposition** | **Satisfied.** The seam spans two independent, divergent host decision shapes without naming a host type. `PolicyProvider` may be wired. |

The host-side need is specified rather than hypothetical: `l1-interception-model.md` §165–167
records that nodus "already carries the *observe* seam … the mechanic it lacks — and adopts
under this pass — is the **decide** seam", and assigns the realization to this workspace.

#### 4.8.2 `StorageProvider` (LP-15) — **LP-3 NOT SATISFIED**

| Field | Record |
| --- | --- |
| **Context A** | **Durable local store** — `crates/store-local`. Decides nothing; persists and retrieves. A genuine backend, but a *provider* rather than a second decision shape. |
| **Context B** | **None identified.** No second integration consumes durable cross-invocation workflow state today. |
| **Independence** | Not assessable — one context. |
| **Host types the seam must not name** | Verified clean regardless: `store(&self, key: &str, value: &Value)` / `load(&self, key: &str) -> Option<Value>` name only `&str` and `Value`. LP-3's second half is satisfied; the first is not. |
| **Disposition** | **Not satisfied.** Missing evidence that would settle it: a second integration that would persist workflow state *with a different durability or locality contract* than `store-local` — e.g. a remote-backed store whose egress decision differs (the `l1-deployment-neutrality` DN-3 local-vs-remote axis LP-15 already cites). Until then `StorageProvider` stays interface-only, and its executor hook points (`STORE`/`LOAD`) remain unplannable. |

Note the asymmetry this produces, and that it is correct: the LP-15 **built-in conformance**
fix (an in-memory built-in per L1 §4.1 and LP-2) is *not* gated by this record, because it
touches neither hook points nor run-parameter variants — the scope §2 defers. Only the wiring
is held.

### 4.9 Per-Effect Authorization Call-Site (LP-11) [ADDED v1.4.0, IMPLEMENTED v1.5.0]

LP-3 is satisfied for `PolicyProvider` (§4.8.1); this section specifies the Rust shape and
**is now implemented** in `crates/nodus` exactly as designed — every reference below is the
real, as-built code, not a projection.

#### 4.9.1 EffectClass — realized narrower than the L1 sketch

L1 §4.7 names three classes: `model_call`, `tool_use`, `deferred`. Grounded against what
the crate actually has:

```text
[REFERENCE]
pub enum EffectClass { ModelCall, Deferred }

/// `None` for a command with no effect class — LOG, COUNTER, DATE, and every other
/// purely in-memory builtin are not gated at all; calling PolicyProvider::evaluate
/// for them would authorize nothing (they touch no host-swappable seam).
pub fn effect_class_of(command: &str) -> Option<EffectClass> {
    if MODEL_COMMANDS.contains(&command) { Some(EffectClass::ModelCall) }
    else if DIALOG_COMMANDS.contains(&command) { Some(EffectClass::Deferred) }
    else { None }
}
```

`ModelCall` = `MODEL_COMMANDS` (`GEN`, `ANALYZE`) and `Deferred` = `DIALOG_COMMANDS`
(`ASK`, `CONFIRM`) — the **same two constants** `CapabilityManifest::from_workflow` already
uses (§4.7), reused rather than re-derived so the two seams stay in lockstep.

**`ToolUse` is deliberately not realized.** `tool_access` even appears as `evaluate`'s own
example gate name (§4.4) — but every `tool`-shaped builtin command (`FETCH`, `WRITE`,
`MKDIR`, `GIT`, `NOTIFY`, `PUBLISH`, …) is, on inspection of `executor.rs`, a fixed,
zero-dependency **stub**: `handle_fetch`, for one, returns `Value::Map([("_stub", true),
("source", <arg>)])` unconditionally — there is no host-swappable seam behind it the way
`ModelProvider` backs `GEN` or `DialogProvider` backs `ASK`. Gating a stub would ask a host
to authorize an effect that cannot vary by host and does not reach outside the crate. This
is the same shape as the §3.1 *vacuous in core* entries (LP-9/12/13/14/16/17/19/20): the
construct the invariant gates does not yet exist, so nothing is owed until a generic
host-tool extension point is introduced (a role this spec does not yet define). Host-schema
(non-builtin) commands are also not gated here — they already pass through LP-8's
pre-run manifest check, and a host implementing `SchemaProvider` controls their dispatch
entirely on its own side.

#### 4.9.2 `gate` and `context` at the call site

`PolicyProvider::evaluate`'s shipped signature (§4.4) is `(gate: &str, context: &Value) ->
bool` — narrower than L1 §4.7's illustrative `authorize(effect_class, args)`. The DSL has
no syntax for a step to declare an arbitrary named gate (that would be the unrealized
`@needs`-adjacent declaration surface the LP-16/17/20 Backlog entries also wait on), so
`gate` is the effect-class name as a literal string — the only vocabulary that exists
today and is host-matchable without a parser change:

```text
[REFERENCE]
gate    := "model_call" | "deferred"        // EffectClass, stringified
context := Value::Map([
    ("command", Value::Text(cmd.name.clone())),
    ("args",    Value::List(cmd.args.iter().map(|a| Value::Text(a.clone())).collect())),
])
```

`context` carries **raw, unresolved** argument strings (`CommandCall.args: Vec<String>`) —
the same pre-resolution form the executor holds before `dispatch` looks up `$var`
references. Passing resolved values would require the gate to duplicate dispatch's own
variable-lookup logic; passing raw args keeps the seam a thin, honest snapshot of what is
about to run, consistent with "decide, pre-effect" (L1 §4.7).

#### 4.9.3 Call-site integration in `execute_command`

The gate brackets the same function that already brackets the `!!`-rule check — mirroring
its structural position, not its consequence:

```text
[REFERENCE]
fn execute_command(&self, ctx, cmd, step_num) -> Option<Signal> {
    if let Some(violation) = ctx.check_rules(&cmd.name, &cmd.args) { /* unchanged, NL-2 */ }

    if let Some(class) = effect_class_of(&cmd.name) {
        let gate = class.as_gate_str();                    // "model_call" | "deferred"
        let context = build_context(cmd);                  // §4.9.2
        if !self.policy.evaluate(gate, &context) {          // decide, pre-effect
            let error_detail = format!("policy denied gate '{gate}' for '{}'", cmd.name);
            self.emit(ctx, |seq, cid| ExecutionEvent::StepError {
                step_index: step_num,
                step_command: cmd.name.clone(),
                error_code: vocab::error_code::POLICY_DENIED.to_string(),
                error_detail: error_detail.clone(),
                step_identity: step_identity(step_num, &cmd.name),
                fault_identity: FaultIdentity {
                    step_identity: step_identity(step_num, &cmd.name),
                    code: vocab::error_code::POLICY_DENIED.to_string(),
                    discriminator: None,
                },
                seq, correlation_id: cid, annotations: EventAnnotations::default(),
            });
            ctx.errors.push(RuntimeError {
                code: vocab::error_code::POLICY_DENIED.to_string(),
                step: step_num,
                reason: error_detail,
            });
            return None;    // the effect never runs; pipeline_target stays at its
        }                   // seeded default (e.g. $out : Value::Null); execution
    }                       // continues to the next step (non-halting)

    /* … existing dispatch, StepStart/StepEnd, pipeline_target binding … */
}
```

Three choices here are load-bearing, each grounded against an existing pattern rather than
invented:

- **`ConstraintHit` is not used.** §4.4 already corrects the doc comment that claimed it;
  the shape above emits `StepError` instead — the same event `RULE_VIOLATION` emits
  alongside its own `ConstraintHit`, so a denial is observable through the ordinary
  per-step channel (HO-2) without touching an event scoped to a different invariant.
- **Return `None`, not `Signal::Skip` or `Signal::Break`.** `Signal::Skip` is the `?IF …
  !SKIP` branch flag — at the top level of `execute_inner`'s step loop it is caught by a
  bare `_ => {}` arm, identical to `None`, so using it here would be a
  same-effect-different-name substitution with no real benefit and a misleading label.
  `Signal::Break` aborts the whole run (`Status::Aborted`), which would make every denial
  fatal — wrong for a per-effect, per-attempt gate whose own L1 invariant (§3, LP-11) draws
  it as the *dynamic* complement to LP-8's *static* strip, not a second hard-constraint
  path. The precedent this shape actually follows is `DialogOutcome::Timeout` /
  `::Rejected` (`executor.rs`): push a typed `RuntimeError`, return `None`, let the run
  continue with the target unset. `RunResult.status` degrades to `Partial` via the existing
  `!ctx.errors.is_empty()` branch — the same path every other non-fatal typed error takes.
- **No `@err:` dispatch.** L1 §4.7 (corrected, v1.14.1) no longer claims `route_to(@err)`;
  grounding here confirmed why — `WorkflowFile.error_decl` has zero call sites anywhere in
  `executor.rs`. It is parsed (`parser.rs`), validated (`validator.rs`), and transpiled
  (`transpiler.rs`), but the declared `@err:` handler is never invoked at runtime. `NL-9`'s
  "the `@err:` handler is invoked for uncaught step errors" is therefore an **unrealized**
  part of `l1-nodus-language.md`, predating LP-11 and out of this pass's scope — building it
  is a separate, crate-wide undertaking (it would touch every error-emitting site, not just
  this one), not something a single seam's call site should silently take on. `POLICY_DENIED`
  surfacing in `RunResult.errors` matches the **actual**, as-built NL-9 pattern every other
  code already follows (`l2-nodus-errors.md` §Overview: "the `@err:` vs. continue decision a
  host applies").

#### 4.9.4 `POLICY_DENIED` error code

New portability-layer code, registered the same way `CAPABILITY_UNMET` (LP-8) was — beside
the frozen 24-code canonical registry, not inside it (`l2-nodus-errors.md` §4.2):

```text
[REFERENCE]
pub const POLICY_DENIED: &str = "NODUS:POLICY_DENIED";
// severity: Error, category: Runtime — same classification as RULE_VIOLATION,
// since a denied effect is a runtime-stage failure, not a validation-stage one.
```

#### 4.9.5 `run_with_policy` / `run_with_policy_and_audit`

Orthogonal combinators (LP-5), matching `run_with_dialog` / `run_with_dialog_and_audit`'s
exact shape — `PolicyProvider` becomes a fourth `Executor` field (alongside `provider`,
`audit`, `dialog`), defaulting to `NoopPolicyProvider`:

```text
[REFERENCE]
impl Executor {
    pub fn with_policy(policy: impl PolicyProvider + 'static) -> Self;
    pub fn with_policy_and_audit(
        policy: impl PolicyProvider + 'static,
        audit: impl AuditProvider + 'static,
    ) -> Self;
}

pub fn run_with_policy(
    source: &str, filename: &str, input: Option<Value>,
    policy: impl PolicyProvider + 'static,
) -> Result<RunResult, Vec<Diagnostic>>;

pub fn run_with_policy_and_audit(
    source: &str, filename: &str, input: Option<Value>,
    policy: impl PolicyProvider + 'static, audit: impl AuditProvider + 'static,
    run_id: &str, started_at: &str,
) -> Result<RunResult, Vec<Diagnostic>>;
```

Additive: a caller using any existing `run_with_*` variant continues to get
`NoopPolicyProvider`'s allow-all, byte-for-byte today's behaviour (L1 §4.7's own purity
guarantee).

#### 4.9.6 What this section does not settle

`ToolUse` realization (§4.9.1) waits on a host-tool extension seam this spec does not
define. `gate` values beyond the two effect-class strings — a workflow declaring its own
named gate — wait on DSL surface this spec does not add. Building `@err:` dispatch itself
(§4.9.3) is `l1-nodus-language.md` NL-9's obligation, not LP-11's; flagging it here is this
pass's contribution, not its remedy. **[UPDATED v1.6.0]** The LP-16/17/20 kinship named above
turned out to split on inspection (§4.10) — LP-16 needed no DSL surface at all, only richer
`context`; LP-17/LP-20 still do, each its own.

### 4.10 Effect Risk-Class Declaration Seam (LP-16) [ADDED v1.6.0, IMPLEMENTED v1.6.1]

L1 §4.12 sketches `effect := { class, reversible?, external?, value? }` and a `run_effect`
that maps declared consequence to host-decided friction. Grounded against what the crate
already has for LP-11 (§4.9): realizing LP-16 needed **no new DSL grammar, no new AST field,
and no `PolicyProvider` trait change** — every piece it needed already existed, and **is now
connected** in `crates/nodus` exactly as designed.

#### 4.10.1 Declaration surface — the existing `+modifier=value` grammar, not new syntax

`l1-nodus-language.md` §4.3 already gives every step `+modifier=value` decorators,
"optional, repeatable", with no closed vocabulary constraining modifier *names* (unlike
`~flag`/`^validator`/`@in` types, which `l2-nodus-registries.md` closes — grepping
`vocab.rs` for a modifier-name registry finds none). `CommandCall.modifiers:
Vec<(String, String)>` already carries them losslessly through parse → transpile → reparse
(`ast.rs`; the NL-6 modifier-value quoting fix landed in Phase 21). A workflow can therefore
declare LP-16 consequence **today**, with zero code changes, using grammar that already
exists:

```text
[REFERENCE]
5. GEN("draft a refund email") +reversible=true +external=true +value=money → $draft
```

No lexer, parser, or transpiler change is in scope. `reversible`, `external`, and `value`
(the last closed to `none | money | creds | perms`, per L1 §4.12) are a **convention this
spec names**, not a new registry — the same relationship `l1-nodus-portability.md` already
has to `gate` string values (§4.9.2): nodus carries the vocabulary through without
validating its semantics.

#### 4.10.2 `context` extension at the existing LP-11 call site

The only code change: `execute_command`'s existing `context` literal (§4.9.3) grows a small
loop reading the three modifier keys off the same `cmd.modifiers` it already has in scope,
pushing each only when present. There is no separate `build_context` function — the literal
is inline, exactly like the rest of the gate:

```text
[REFERENCE]
let mut context_pairs = vec![
    ("command".to_string(), Value::Text(cmd.name.clone())),
    ("args".to_string(), Value::List(cmd.args.iter().cloned().map(Value::Text).collect())),
];
for key in ["+reversible", "+external", "+value"] {
    if let Some((_, value)) = cmd.modifiers.iter().find(|(k, _)| k == key) {
        context_pairs.push((key.trim_start_matches('+').to_string(), Value::Text(value.clone())));
    }
}
let context = Value::Map(context_pairs);
```

No new `Value` kind (NL-7 holds). A descriptor is **omitted, not defaulted to a sentinel**,
when the step carries no matching modifier: an unclassified effect is silence in `context`,
never a `false`/`"none"` value a host could mistake for an explicit declaration. L1 §4.12's
"unclassified → most cautious" is host-side policy the host's own `evaluate` body applies —
nodus encodes no default.

#### 4.10.3 `PolicyProvider` is untouched

L1's `run_effect` pseudocode shows the host computing `tier := host.tier_of(step.effect)`
then `decision := host.gate(tier, step.effect)` — but from nodus's side both calls collapse
into the **already-shipped** `self.policy.evaluate(gate, context)` (§4.4): the host's own
`evaluate` implementation is free to inspect the now-richer `context` and compute
tier → friction → decision entirely on its own side. No second trait method, and no `Tier`
or `Friction` type enters the core (LP-2) — the identical call site (§4.9.3) simply receives
a `context` that may now carry up to three more keys. `NODUS:POLICY_DENIED` is the same code
(§4.9.4); LP-16 introduces no new failure mode, only a richer input to an existing one.

#### 4.10.4 Additivity

A workflow with no `+reversible`/`+external`/`+value` modifiers, or a host whose
`PolicyProvider::evaluate` ignores the extra keys, behaves byte-for-byte as it does
today — the same purity guarantee §4.9.5 already states for `NoopPolicyProvider`. Decorating
a command `effect_class_of` does **not** gate (e.g. `LOG +external=true`) is inert: the
modifiers still parse and transpile like any other, but no gate call site ever reads them,
since LP-16 rides the LP-11 gate rather than opening a second one.

#### 4.10.5 What this section does not settle

Whether nodus should validate `value=` against its closed four-token vocabulary (an
advisory `W`-code, mirroring `l2-nodus-registries.md`'s `~flag`/`^validator`/`@in`-type
precedent) is deliberately left open: modifiers as a whole have never been validated (no
`KNOWN_MODIFIERS` registry exists anywhere in the crate), and adding validation for exactly
these three while leaving every other modifier name unchecked would be an unmotivated
asymmetry this pass has no independent reason to introduce. A future pass finding workflows
actually mistyping `value=` tokens in practice is the trigger for a dedicated registry, not
something to pre-build speculatively (LP-2, and this project's own anti-overengineering
discipline). LP-17 and LP-20 are not addressed here — each specializes the LP-11 gate
differently (LP-17 to an outbound value transfer with a spending envelope; LP-20 to an
obligation-discharge check) and needs its own grounding, not an assumption that this
section's pattern — reuse an existing decoration surface, extend `context`, leave the trait
alone — transfers unchanged.

## 5. Implementation Notes

Order of implementation across future phases, revised against the §3.1 findings:

1. **SchemaProvider + capability manifest (LP-8)** — delivered. `Schema::with_provider`, the `run_with_schema` variant, and the `CapabilityManifest` / `validate_manifest` gate (§4.7) are purely additive — zero risk to the existing API.
2. ~~**PolicyProvider call site (LP-11)**~~ — **DONE [v1.5.0].** `Executor::execute_command` gates every `ModelCall`/`Deferred` effect through `self.policy.evaluate`; `NODUS:POLICY_DENIED` (not `ConstraintHit`) is non-halting and degrades `RunResult.status` to `Partial`; `run_with_policy`/`run_with_policy_and_audit` ship. This item passed through all four states the LP-11 arc named: v1.2.0 called it task-authorable while omitting the LP-3 gate (error) → v1.3.0 opened the gate but left the shape undesigned (permitted-not-designed) → v1.4.0 designed it (permitted-and-designed) → v1.5.0 built it. LP-16/LP-17/LP-20 remain blocked behind the same `decide → effect → observe` ordering until their own designs land — unblocking *this* seam does not, by itself, realize theirs.
3. **StorageProvider built-in + call site (LP-15)** — two separable pieces, correctly separated in practice. **[MODIFIED v1.3.1] The built-in half is DONE**: `InMemoryStorageProvider` replaced the no-op, round-trips within an invocation, and was self-contained exactly as predicted — it did not wait on LP-3. Executor hook points for `STORE`/`LOAD` remain the larger piece, still held: `StorageProvider`'s own LP-3 record (§4.8.2) is **not satisfied** — one host context, not two — unlike `PolicyProvider`'s.
4. **Environment liveness (LP-18 a/b)** — requires an L2 design decision before it can be planned: giving `EnvironmentProvider::open` a `Result` return and teaching `ResumeDescriptor` an environment identity are breaking changes to published items, so LP-6 classes them **major**. Amend this spec (or `l2-nodus-environment.md`) before authoring a task.
5. **Import triad (LP-13 → LP-9 → LP-12)** — one deliverable, not three; nothing is owed until a bundle-load path exists. Not plannable today.
6. **`cargo semver-checks` gate** — add to `ci.yml` once the crate reaches `1.0.0`; this is the LP-6 mechanical enforcement.
7. ~~**Effect risk-class declaration (LP-16)**~~ — **DONE [v1.6.1].** Specified in §4.10, implemented in Phase 25: `execute_command`'s `context` literal gained the three optional keys, reusing the existing `+modifier=value` grammar (zero new DSL/AST/parser/transpiler work), `PolicyProvider`'s signature untouched. The smallest item on this list — one loop, no new type, no new error code, no new unit test needed.
8. ~~**Settlement effect seam (LP-17)**~~ — **DONE [v1.0.1] — Phase 27.** `l2-nodus-settlement.md` implemented: a new `EffectClass::Settlement` + `SETTLE` command reuses this spec's own LP-11 gate unchanged for the decide half; a new `SettlementRail` trait + `NoopSettlementRail` built-in supplies the act half (`PolicyProvider::evaluate` has no receipt channel); `NODUS:SETTLEMENT_UNACCOUNTED` is `Signal`-free so `l2-nodus-error-dispatch.md`'s NL-9 mechanism picked it up automatically, with no dispatch-adjacent code needed. `ExtensionRole::Settlement` extends the LP-8 manifest, deliberately absent from `HostCapabilities::builtin()` (`Storage`/`Policy`/`Dialog` precedent). 462 tests pass (was 452, +10).

**[MODIFIED v1.7.1]** Item 8 is now **done** alongside items 2 and 7 — three of twelve
LP-9…LP-20 items now Implemented. Item 3's built-in half
has been done since v1.3.1 (its wiring half stays held by its own unsatisfied LP-3 record,
§4.8.2). Item 4 still needs an L2 decision. Everything in §3.1 marked *Vacuous in core*
(LP-9/12/13/14/19/20) is deliberately absent from this list: an item with no call site
yields no verifiable task.

## 6. Drawbacks & Alternatives

**SchemaProvider as a generic parameter (not trait object)**: a `Schema::with_provider<P: SchemaProvider>(p: &P)` signature avoids one virtual dispatch. Rejected in favour of `&dyn SchemaProvider` to keep `run_with_schema` callable with any boxed host implementation without requiring the caller to specify a type parameter.

**Merging StorageProvider into ModelProvider**: pairing state persistence with model invocation. Rejected: violates LP-5 (composability); storage and model are independent concerns; combining them prevents hosts that need only one from avoiding the other.

**Runtime feature flags for pending extension points**: expose `StorageProvider`/`PolicyProvider` behind `#[cfg(feature = ...)]`. Rejected: violates LP-5 (no feature-flag extension mechanism); the no-op built-in achieves zero-cost absence without compile-time gating.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[VOCAB]` | `crates/nodus/src/vocab.rs` | `KNOWN_COMMANDS`, `RESERVED_VARIABLES`, `Schema` — LP-4 seam |
| `[EXT-POINTS]` | `crates/nodus/src/executor.rs` | `ModelProvider`, `StubProvider` — LP-2 canonical example |
| `[AUDIT-EXT]` | `crates/nodus/src/observability.rs` | `AuditProvider`, `NoopAuditProvider` — LP-2 second example |
| `[PORTABILITY]` | `crates/nodus/src/portability.rs` | `ExtensionRole`, `CapabilityManifest`, `HostCapabilities`, `validate_manifest`; the Schema/Storage/Policy/Config traits — the §3.1 evidence base |
| `[ENV-EXT]` | `crates/nodus/src/environment.rs` | `EnvironmentProvider`, `StubEnvironment`, `Instance` — the LP-18 realized surface |
| `[RUN-VARIANTS]` | `crates/nodus/src/workflows.rs` | The orthogonal `run_with_*` family — LP-5 composability evidence |
| `[GUARD-CTX-A]` | `crates/domain/src/tool_security.rs` | LP-3 host context A (§4.8.1) — runtime tool guard, pre-execution severity-rule evaluation |
| `[GUARD-CTX-B]` | `crates/domain/src/hooks.rs` | LP-3 host context B (§4.8.1) — plugin-hook interception, parallel deny-aggregation |
| `[HOST-EMBED]` | `crates/core/src/model_bridge.rs` | The canonical LP-2 host adaptor: `contract::InferenceBackend` → `nodus::ModelProvider`, kept outside the crate so LP-1 holds |
| `[API-SURFACE]` | `crates/nodus/src/lib.rs` | Public re-export surface — LP-6 versioning scope |
| `[CI]` | `crates/nodus/.github/workflows/ci.yml` | LP-6 regression gate |
| `[EXTRACTION]` | `crates/nodus/EXTRACTION.md` | LP-6 extraction checklist |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.11.2 | 2026-09-24 | Core Team | Consistency pass (2026-09-24): LP-5 → Partial: every seam added after the model/audit pair arrived as its own entry point fixing the rest to built-ins, so no two compose and none composes with a real model. LP-11 notes the gate never meets a real effect and that `@err:` dispatch now exists (the row still called it unbuilt). LP-17 points to the settlement composition gap. |
| 1.7.1 | 2026-08-01 | Core Team | **Reconciled §3.1's LP-17 row to Implemented — Phase 27.** `l2-nodus-settlement.md` 1.0.0 → 1.0.1: `EffectClass::Settlement`/`SettlementRail`/`NoopSettlementRail`/ninth `ExtensionRole::Settlement`/`handle_settlement`/`run_with_settlement[_and_audit]` all landed in `crates/nodus` exactly per the design, with no plan-time or implementation-time scope correction — the spec had been authored directly against the real code in the same session, so every `[REFERENCE]` block's structural claim held (unlike Phase 24's first LP-11 build, which found real divergences). One empirical catch during test-writing, not a spec defect: the first fixture draft used a non-reserved `@out: $receipt` pipeline target and wrongly assumed it would be pre-seeded like `$out` — only reserved variables are, so an ordinary declared name starts absent from `vars`; fixed by using the reserved `$out` binding, matching every sibling fixture's convention. §3.1's LP-17 row, the Leverage paragraph (three of twelve now Implemented), and §5 item 8 all updated to Implemented/Done. 462 tests pass (was 452, +10); clippy clean; fmt clean after one auto-fix; `Cargo.toml`/`Cargo.lock` diff empty (LP-1 preserved); the one new `.expect()` sits inside `#[cfg(test)]`. |
| 1.7.0 | 2026-07-31 | Core Team | **Reconciled §3.1's LP-17 row against the new `l2-nodus-settlement.md` [v1.0.0 Stable]** — moved from *Vacuous in core* to *Realized by [dedicated spec] — specified; awaiting implementation phase*, the same non-axis exception `l2-nodus-runtime.md` §3.1 already uses for NL-22/NL-23. **Self-corrected a prediction this same document made at v1.6.0**: that entry read "LP-17 and LP-20 are *not* freed the same way [by LP-11 landing]: each still needs its own core vocabulary… before either has anything to carry through the now-open gate," grouping LP-17 with LP-20 as equally blocked. Grounding LP-17 directly (this pass) found that framing incomplete: LP-17's **decide** half *is* freed the LP-16 way — `context.args` already carries `[payee, amount, purpose]` positionally, so `self.policy.evaluate("settlement", context)` needed no design at all — only its **act** half (settling and returning a receipt) needed genuinely new vocabulary, because `PolicyProvider::evaluate` is boolean-only and has no channel for a receipt. That distinction is why LP-17 got a dedicated new-trait spec (`SettlementRail`, the crate's first genuinely new §3.1 extension-point trait since LP-11 itself) while LP-16 needed none — recorded honestly as a correction, not silently folded in, matching this document's own v1.5.0/v1.6.0 entries' practice of naming wrong assumptions once grounding reveals them. Updated §3.1's LP-17 row, the Leverage paragraph's invariant counts and LP-16/LP-17 comparison, §5 (new item 8, task-authorable), and Related Specifications (new `l2-nodus-settlement.md` entry). Design only — nothing landed in `crates/nodus` this pass; `magic.task`/`magic.run` build it next. |
| 1.6.1 | 2026-07-31 | Core Team | **Implemented the LP-16 descriptors designed in v1.6.0 — Phase 25.** `execute_command`'s existing LP-11 `context` literal gained a small loop reading `+reversible`/`+external`/`+value` off `cmd.modifiers`, pushing each pair only when present — exactly per §4.10's design, with zero plan-time or implementation-time scope correction (the second phase in the LP-11 arc, after Phase 24, where none was needed). **Self-review reconciling §4.10.2's `[REFERENCE]` pseudocode found one divergence worth correcting**: the pseudocode named a separate `build_context` function for illustrative clarity, but the real call site builds the literal inline (matching §4.9.3's own precedent of showing the real, as-built code rather than a hypothetical helper) — corrected to the exact inline shape. 447 tests pass (was 444; +3, all integration: a decorated `GEN` step's descriptors reach a capturing `PolicyProvider`'s `context` verbatim; an undecorated step's `context` omits all three keys entirely, not defaulted to `Null`/`false`/`"none"`; a `NoopPolicyProvider`/plain-`run` regression proving the modifiers are inert without a consulting host) — no new unit tests, since there is no new type or function to unit-test, only a loop inside an existing call site. `cargo clippy -p nodus --all-targets -- -D warnings`: clean; `cargo fmt -p nodus -- --check`: one violation in the new tests (line-wrap, no logic change), fixed; `git diff --stat` on `Cargo.toml`/`Cargo.lock`: empty (LP-1 preserved); no `unwrap`/`expect`/`panic!` added to `executor.rs`'s production path (the three new integration tests' own `.unwrap()`/`.expect()` calls are test code, not production). §3.1's LP-16 row, its Leverage paragraph's invariant count (now two Implemented: LP-11, LP-16), and §5 item 7 all updated to Implemented/Done. |
| 1.6.0 | 2026-07-31 | Core Team | **Designed the LP-16 effect risk-class declaration — new §4.10, the first §3.1 item LP-11's own landing unblocked.** Grounded against the shipped LP-11 gate rather than L1's illustrative pseudocode alone: realizing LP-16 needs no new DSL grammar, AST field, or `PolicyProvider` signature change, because `l1-nodus-language.md` §4.3's existing `+modifier=value` decorators — already parsed, transpiled, and NL-6 round-trip-safe since Phase 21 — can carry `reversible`/`external`/`value` today with zero code changes; the only realization work is a small extension to the LP-11 call site's `context` builder (§4.9.2) reading those three modifier keys when present. **A genuine simplification found by grounding, not assumed:** L1 §4.12's `run_effect` pseudocode shows the host computing a tier then a gate decision as two conceptual steps, but both collapse into the *already-shipped* `evaluate(gate, context)` call — the host's own implementation decides tier-then-friction internally over the richer context nodus now offers it, so no second trait method or `Tier`/`Friction` type is needed. Corrected two stale surfaces found while writing this section, both dating to before LP-11 shipped: §2's Constraints bullet still said `PolicyProvider`'s executor integration "is deferred… once LP-3 is satisfied" — true of `StorageProvider`, false of `PolicyProvider` since v1.5.0; and §4.5's module map still called it "interface only" after Phase 24 wired it. §3.1's LP-16/LP-17/LP-20 rows updated to state precisely what LP-11 landing did and didn't unblock (LP-16 needed nothing further; LP-17/LP-20 still need their own core vocabulary), and its Leverage paragraph's invariant counts brought current. §5 gains item 7, marked task-authorable with no further design decision needed. **Explicitly left open (§4.10.5) rather than pre-built:** whether `value=` should get closed-vocabulary validation — no modifier name has a registry today, and adding one for just these three would be an unmotivated asymmetry absent evidence of real-world mistyping. `l1-nodus-portability.md` needed no change — its §4.12 pseudocode and properties already matched this realization exactly. Design only — nothing landed in `crates/nodus` this pass; `magic.task`/`magic.run` build it next. |
| 1.5.0 | 2026-07-31 | Core Team | **Implemented the LP-11 call site designed in v1.4.0 — Phase 24.** `Executor::execute_command` now gates every `ModelCall`/`Deferred` effect through `self.policy.evaluate(gate, context)` before it runs, exactly per §4.9's design; `Executor` gained a fourth `policy: Box<dyn PolicyProvider>` field (default `NoopPolicyProvider`) across all six constructors, plus `with_policy`/`with_policy_and_audit` and the `run_with_policy`/`run_with_policy_and_audit` combinators mirroring `run_with_dialog`'s exact shape. `NODUS:POLICY_DENIED` registered in `vocab.rs` beside the frozen 24-code registry (`Error`, `Runtime` — same as `RULE_VIOLATION`), surfaced via the existing `StepError` event and `ctx.errors`, never `ConstraintHit`. 444 tests pass (was 435; +9): `effect_class_of` classification + gate-string unit tests in `portability.rs`, and five integration tests in `tests/portability.rs` — permit/deny on both `ModelCall` and `Deferred`, plus a `NoopPolicyProvider` regression proving every existing `run_with_*` variant stays byte-for-byte unchanged. **Caught two real assertion errors empirically, not assumed correct**: the test's first draft asserted a denied step's pipeline target is *absent* from `RunResult.vars`; it is not — `$out` (and `$error`/`$meta`/etc.) are seeded to defaults (`Value::Null` for `out`) at context construction regardless of whether any step runs, so "unbound" means "still `Value::Null`", not "key absent" — fixed by asserting against the seeded default instead, which also strengthened the *permitted*-effect assertions (previously vacuously true, since the key is always present). **Self-review before closing the task caught two stale doc comments in the shipped crate itself**, not just the spec: `portability.rs`'s module doc and `PolicyProvider`'s own trait doc still said "executor integration is deferred until LP-3 is satisfied" — both corrected to describe the real, wired gate. §4.9's own three `[REFERENCE]` pseudocode blocks were reconciled to the exact as-built code during this pass: `class.as_str()` corrected to the real method name `as_gate_str()`, and the denial `reason` string corrected to the fuller `"policy denied gate '{gate}' for '{cmd}'"` form the real code emits (the pseudocode had shown a shorter placeholder). §4.2's registry row, §4.4's heading/framing, §3.1's LP-11 row, §5 item 2, and the Overview all updated to Implemented; `Related Specifications`/`Canonical References` needed no new entries (existing `[EXT-POINTS]`/`[PORTABILITY]` aliases already cover the touched files generically). `cargo test -p nodus`: 444 passed, 0 failed; `cargo clippy -p nodus --all-targets -- -D warnings`: clean; `cargo fmt -p nodus -- --check`: one violation (line-wrap in the new integration tests), fixed by `cargo fmt`; `Cargo.toml`/`Cargo.lock` diff empty (LP-1 preserved); every `unwrap`/`expect`/`panic!` hit in the touched files falls inside `#[cfg(test)] mod tests`, none on a production path. |
| 1.4.0 | 2026-07-31 | Core Team | **Designed the LP-11 call site — new §4.9, first non-vacuous design closing the highest-leverage item in §5.** `EffectClass{ModelCall,Deferred}` realized narrower than L1 §4.7's three-way sketch: `ModelCall`/`Deferred` reuse the existing `MODEL_COMMANDS`/`DIALOG_COMMANDS` constants (already load-bearing in `CapabilityManifest::from_workflow`, §4.7); `ToolUse` is deliberately left **vacuous** — every `tool`-shaped builtin command (`FETCH`, `WRITE`, `GIT`, `NOTIFY`, …) is, on inspection, a fixed zero-dependency stub with no host-swappable seam behind it, so gating one would authorize nothing real, the same shape as the §3.1 *vacuous in core* entries. `gate`/`context`: since the DSL has no syntax for a step to declare a named gate, `gate` is the effect-class string itself and `context` a raw, pre-resolution `Value::Map` snapshot of the command and its unresolved args. Call site: `execute_command`, mirroring the `!!`-rule check's structural position but not its consequence. **Corrected a real defect in §4.4's own doc comment**, found on inspection of `observability.rs` rather than assumed: it claimed the denial reuses `ConstraintHit { halt: false }`, but that variant's own doc comment scopes it to hard `!!NEVER`/`!!ALWAYS` constraints and `l2-nodus-errors.md` states the `RULE_VIOLATION`/`ConstraintHit` path is "unchanged" by any later spec — reusing it would have silently conflated NL-2 and LP-11 under one event type. The real mechanism: a new `NODUS:POLICY_DENIED` code (registered beside the frozen 24-code registry exactly as `CAPABILITY_UNMET` was for LP-8), emitted via the existing `StepError` event (no new `ExecutionEvent` variant — purely additive), `ctx.errors.push`, and a bare `None` return — mirroring `DialogOutcome::Timeout`/`::Rejected`'s existing non-halting precedent, not `Signal::Skip` (a different, branch-scoped primitive that top-level code treats identically to `None` anyway) or `Signal::Break` (which would make every denial fatal, wrong for a per-attempt gate). **A genuine, previously-unflagged finding surfaced while grounding the denial signal**: `WorkflowFile.error_decl` — the parsed `@err:` handler — has **zero call sites in `executor.rs`**. It is parsed, validated, and transpiled but never dispatched, meaning `@err:` routing is not a mechanism the runtime has today; L1 §4.7's `route_to(@err)` pseudocode described something unbuilt. Corrected there (v1.14.1) rather than silently built here — a full `@err:` dispatch mechanism is `l1-nodus-language.md` NL-9's own obligation, touching every error-emitting site, not a thing this one seam's call site should absorb; flagged as a new, real gap rather than a rediscovery of a known one (checked the Backlog first — it wasn't there). `run_with_policy`/`run_with_policy_and_audit` combinators specified mirroring `run_with_dialog`'s exact shape (`PolicyProvider` becomes a fourth `Executor` field). §3.1's LP-11 row, §4.4 (doc-comment correction), and §5 item 2 (closed out through its third and final state: error → permitted-not-designed → permitted-and-designed) all updated; `l2-nodus-errors.md` gains a one-line cross-reference for the new code, matching its own `CAPABILITY_UNMET` precedent. Design only — no line has landed in `crates/nodus`; `magic.task`/`magic.run` build it next. |
| 1.3.1 | 2026-07-31 | Core Team | Reconciled to the as-built LP-15 fix: `NoopStorageProvider` replaced by `InMemoryStorageProvider` (`Mutex`-guarded, round-trips within an invocation, shares no state across instances), satisfying L1 §4.1's in-memory built-in mandate and LP-2's in-process-sufficiency requirement that the prior no-op met for neither. Updated §4.2's registry row and Overview to Implemented; §4.3 heading and body to record the built-in as conformant while its executor wiring stays held by the unsatisfied LP-15 admission record (§4.8.2); §3.1's LP-15 row split into a resolved built-in half and a still-open wiring half; §5 item 3 marked complete for the built-in and its closing sentence updated to two states (was three, since item 3 no longer shares item 2's "not yet designed" status). Public-API rename (`NoopStorageProvider` → `InMemoryStorageProvider`), acceptable pre-1.0 per LP-6 §5. No new dependency (LP-1); `cargo test -p nodus` 435 passing (was 429), `clippy`/`fmt` clean. |
| 1.3.0 | 2026-07-31 | Core Team | Added §4.8 — the first **LP-3 admission records** under the L1 §4.14 rule authored in the same pass, reaching **different dispositions for the two open seams**, which is the gate doing work rather than a half-finished pass. **`PolicyProvider` (LP-11) — SATISFIED.** Context A: the runtime tool guard (`crates/domain/src/tool_security.rs`), deciding whether a tool call may execute, evaluating every call pre-execution against severity-ranked signature rules. Context B: plugin-hook interception (`crates/domain/src/hooks.rs`), nine `HookEvent` kinds evaluated **in parallel** with deny-if-any-`Block` aggregation. Independence is established by **documented divergence and not by our own reading**: the host's Stable `l1-interception-model.md` exists precisely to unify "the currently-divergent fail-behaviours of the plugin-hook and tool-security realizations" (INT-3), and two mechanisms that already disagree on fail direction — severity classification vs parallel deny-aggregation — are §4.14's strongest signal. LP-3's second half was already satisfied by construction: `evaluate(&self, gate: &str, context: &Value) -> bool` names only `&str` and nodus's own `Value`, so the interface needs no change; the host types it must never name (`HookEvent`, hook matcher/rule types, `tool_security::Severity`, scanner signature-rule types, `contract::*`) are all absent. The host need is specified rather than hypothetical — `l1-interception-model` §165–167 records that nodus "already carries the *observe* seam … the mechanic it lacks … is the **decide** seam" and assigns the realization here. **`StorageProvider` (LP-15) — NOT SATISFIED**: only one context (`crates/store-local`, a provider rather than a second *decision shape*); the missing evidence is named precisely — a second integration persisting workflow state under a different durability/locality contract, the `l1-deployment-neutrality` DN-3 local-vs-remote axis LP-15 already cites — so its `STORE`/`LOAD` hook points stay unplannable while the separate LP-15 **built-in conformance** fix remains ungated, since it touches neither hook points nor run-parameter variants. Updated accordingly: §3's LP-3 row (per-seam dispositions replacing the blanket "both held pending"), §3.1's LP-11 row (**unblocked**, with the four items still needing design named — effect-class notion, `gate` derivation, denial signal, `run_with_policy` combinators), and §5 item 2, whose v1.2.0 text called the call site task-authorable while omitting the gate this same document asserts in §3/§2/§4.4 — corrected from the other side, since the gate is now genuinely open but the seam is *permitted, not yet designed*: the difference between "not allowed" and "not specified". §5's closing sentence rewritten to separate the three states it had collapsed. Related Specifications gains the host-side `l1-interception-model`; Canonical References gain the two divergence-evidence files and the LP-2 host adaptor. |
| 1.2.0 | 2026-07-30 | Core Team | Reconciled the twelve deferred Invariant-Compliance obligations LP-9…LP-20, each of whose L1 Document History entries names *this* spec as their carrier — new §3.1. Replaces the binary Realized/Pending axis of `l2-nodus-runtime` §3.1 with a **four-way** one, because these are host-supplied seams and the binary form misreports them: **Satisfied structurally** (LP-10 — discharged by absence of vocabulary, and load-bearing in four sibling specs while having had no row here), **Vacuous in core** (LP-9/12/13/14/16/17/19/20 — the gated construct is absent, so the seam has no call site and nothing is owed until it lands; vocabulary from `l2-nodus-compensation` §2), **Seam declared, wiring pending** (LP-11 and LP-15 — traits ship and are re-exported but `evaluate` / `store` / `load` have **zero call sites**, so a host implementing them observes no behaviour change: published API that silently does nothing, not deferred work), and **Partially realized** (LP-18 — rule (c) discharged via NE-3/NE-7/NE-10, rules (a)/(b) not, and closing them is an LP-6 *major* change since `EnvironmentProvider::open` returns `Instance` not `Result`). Records two structural findings: **LP-11's absent call site alone blocks LP-16, LP-17, and LP-20**, so four invariants reduce to one hook in `execute_command`; and LP-13 → LP-9 → LP-12 are ordered stages of one absent import mechanism, not three independent gaps. Names an L1 divergence: L1 §4.1 specifies an *in-memory* built-in store while the crate ships `NoopStorageProvider`, which never round-trips within an invocation (also `put`/`get`/`delete` vs `store`/`load`, no `delete`). Corrected three stale surfaces against `portability.rs`: §4.2 registry (5 roles → 8, adding `Dialog`/`Environment`/`Config`, plus a *Wired* column, and un-marking `SchemaProvider` as pending when §5 already recorded it delivered), §4.7 `ExtensionRole` reference enum (5 → 8 variants) and its built-in-host prose (3 → 5 roles provided, with `Dialog`'s deliberate exclusion explained), and `from_workflow`'s derivation rules (adds the `ASK`/`CONFIRM` → `Dialog` rule and the never-derived roles). §1 seven-invariants → twenty; §4.5 module map marked delivered; §5 re-ordered so the only two task-authorable items (LP-11 call site, LP-15 built-in) lead. Flags for L1 reconciliation that §4.1 — self-declared "the authoritative extension point registry" — still lists four roles against the implementation's eight. |
| 1.11.1 | 2026-09-02 | Core Team | **Phase 32 — cross-reference updated from "designed" to "implemented."** §3.2's digest-collision paragraph now points at `l2-nodus-environment.md` v1.3.0 (the implemented, tested reconciliation) rather than v1.2.0 (the design). No new content otherwise. |
| 1.11.0 | 2026-09-02 | Core Team | **Two corrections, no code yet — both retracting this document's own earlier optimism.** LP-21's row: the "observable binding is a cheap partial win" framing (v1.8.0) did not survive grounding — recording it needs a `name()`-shaped method on at least five trait definitions (a real breaking-change surface across `ModelProvider`/`AuditProvider`/`DialogProvider`/`PolicyProvider`/`SettlementRail`), and even built the recorded "resolution" would be hollow with exactly one candidate per role; LP-21 is vacuous in core, in full, with no small residue. "What is plannable" rewritten accordingly — LP-21 and LP-23 are both fully blocked, no cheap entry point remains for either. §3.2's `workflow_digest` naming-collision finding marked **RESOLVED**: `l2-nodus-environment.md` v1.2.0 unified `CandidateResult.workflow_digest` onto the same `digest_ast` computation `ReproRecipe.workflow_digest` already used, closing the divergence this document flagged at v1.9.0. |
| 1.10.0 | 2026-09-02 | Core Team | **Phase 31 — LP-22 (c) reconciled to Implemented**, no scope correction: `l2-nodus-dialog.md` §4.3 v1.4.0 built `workflow_digest` exactly as designed at v1.9.0. §3.2's LP-22 row (c) updated from "designed, not yet built" to Implemented, citing the unit test (digest equals `digest_ast` for the parsed workflow) and integration test (agrees with the independently-built `ReproRecipe.workflow_digest` for the same paused run). The "What is plannable" summary drops LP-22 (c) from its residue list — one residue (LP-21's observable binding) remains. LP-22's overall §3.2 verdict stays **Partially realized**: closing (c) does not change (a) (already satisfied structurally) or (b) (still vacuous behind the absent import triad). |
| 1.9.0 | 2026-09-02 | Core Team | LP-22 (c) fully designed and cross-referenced to `l2-nodus-dialog.md` §4.3 v1.3.0 (owning spec for `ResumeDescriptor`): a `workflow_digest: String` field reusing the existing `digest_ast` computation, host-compared at resume time since the core has no resume-time call site. **Corrects this document's own v1.8.0 row**, which asserted "a resume re-enters whatever that name now resolves to" — grounding against every `run`/`run_with_*` signature and `paused_at`'s read sites showed no name resolution happens in the crate at all; resume is host-supplied source text re-invoked from the top, never a crate-side lookup. **Retracts the v1.8.0 "Cross-spec note"**: LP-22 (c) and NL-26 are independent, not a shared decision — LP-22 (c) needed no design question beyond the digest choice, and NL-26's correlation token is blocked on NL-12 alone, for a reason specific to it (the synchronous `DialogProvider::ask`/`confirm` call shape has no asynchronous boundary for a "late completion" to arrive across). **New finding**: `ReproRecipe.workflow_digest` (`digest_ast`, AST-level) and `l2-nodus-environment.md`'s `CandidateResult.workflow_digest` (`digest_source`, raw-text-level) are two different computations sharing one field name, which `l2-nodus-observability.md` §4.7's own comment treats as equivalent; not unified here, flagged for whichever pass next touches either document. |
| 1.8.0 | 2026-09-01 | Core Team | Added §3.2 — realization status for LP-21…LP-23, the three invariants added to `l1-nodus-portability.md` after §3.1 was closed at LP-20, recorded on the same four-way axis and grounded against `crates/nodus` source. LP-21 **vacuous in core** (one `Box<dyn _>` binding per role on `Executor`; no candidate set to resolve among — its observable-binding half has no anchor and is the nearest-to-plannable residue). LP-22 **partially realized**, split by its own three consequences: (a) satisfied structurally because a run owns its parsed `WorkflowFile` and never re-reads the source, (b) vacuous behind the absent import triad, (c) **not realized** — `ResumeDescriptor` carries `"wf:<name>"`, the launch-time name LP-22 (c) forbids, which is the one LP-22 failure reachable today since `Status::Paused` ships. LP-23 **partially realized** — the `required` case is conforming by construction, but per-entry `{major, minor}` lines change `CapabilityManifest`'s entry *type* (an LP-6 decision, not a field addition) and the `fallback` degrade needs a step-substitution mechanism the core lacks; the two-line diagnostic is the cheapest piece. Names the two small additive residues (observable binding, pinned generation) as the right first phase, and flags that LP-22 (c) and NL-26 both amend the same `ResumeDescriptor` and should land together. |
| 1.1.0 | 2026-06-27 | Core Team | LP-8 implemented: §4.7 capability manifest + pre-run satisfiability gate (`CapabilityManifest`, `ExtensionRole`, `HostCapabilities`, `validate_manifest`, `run_with_manifest` / `run_with_manifest_and_audit`, `NODUS:CAPABILITY_UNMET`); §3 LP-8 row → Implemented; status RFC → Stable |
| 1.0.0 | 2026-06-24 | Core Team | Initial spec — LP-1…LP-7 compliance table; SchemaProvider seam; StorageProvider + PolicyProvider pending LP-3 interfaces; extraction artifacts |
