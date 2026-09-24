# Nodus Settlement Effect Seam (Rust)

**Version:** 1.0.2
**Status:** Stable
**Layer:** implementation
**Implements:** l1-nodus-portability.md

## Overview

Concrete Rust realization of LP-17: a workflow step MAY declare an **outbound value
settlement** (`SETTLE(payee, amount, purpose)`), gated exactly like any other effect through
the existing LP-11 machinery, then handed to a host-supplied **settlement rail** that
actually moves value and returns a receipt. This spec adds the crate's first new
extension-point *trait* since `l2-nodus-config` — every prior LP-11-family addition
(LP-16, NL-9) reused existing machinery end to end; settlement cannot, because deciding
whether a payment may proceed and actually moving value are two different kinds of
operation (a boolean permit vs. an action that returns a receipt), and `PolicyProvider`
was designed only for the former.

Grounded against `main`'s `l1-value-settlement.md` (VS-1…VS-8, the source invariants LP-17
names as its main-workspace counterpart) as well as this workspace's own L1 §4.13: the
**decide** half (VS-2 envelope, VS-4 allowlist/tier, VS-5 fail-closed) is entirely host-side
— it collapses into the *already-shipped* LP-11 gate, exactly as LP-16's tier-computation
did, costing nodus nothing new. Only the **act** half (VS-3 bounded custody, VS-7 receipt)
needs a new seam, because nothing in the crate today can return a value from an effect
decision — `PolicyProvider::evaluate` is boolean by design.

## Related Specifications

- [l1-nodus-portability.md](l1-nodus-portability.md) — defines LP-17 (§4.13); this spec
  realizes its Rust shape, the twelfth of the twelve LP-9…LP-20 invariants added after
  §3's original table and the fourth to move past "vacuous in core" (after LP-11, LP-15,
  LP-16)
- [l2-nodus-portability.md](l2-nodus-portability.md) — owns the LP-11 gate (§4.9) this spec
  reuses unchanged for the decide half, and the `ExtensionRole`/`CapabilityManifest`/
  `HostCapabilities` machinery (§4.2/§4.7) this spec extends with a ninth role
- [l2-nodus-errors.md](l2-nodus-errors.md) — owns the `NODUS:*` severity/category registry
  `SETTLEMENT_UNACCOUNTED` registers into, beside the frozen 24-code set, matching the
  `CAPABILITY_UNMET` (LP-8) / `POLICY_DENIED` (LP-11) precedent
- [l2-nodus-error-dispatch.md](l2-nodus-error-dispatch.md) — a denied or unaccounted
  settlement is a `Signal`-free `RuntimeError`, so it reaches NL-9's `@err:` dispatch check
  automatically, with no code in this spec needed to wire that up
- [../../main/specifications/l1-value-settlement.md](../../main/specifications/l1-value-settlement.md) —
  the source invariants (VS-1…VS-8) this seam realizes; names this file as "the
  nodus-workflow realization" (its own §Related Specifications)
- [../../main/specifications/l1-interception-model.md](../../main/specifications/l1-interception-model.md) —
  the decide→effect→observe seam VS-5/INT-3 fail-closed composes; the same host-side
  contract LP-11's own §4.8.1 admission record already cites

## 1. Motivation

`l1-nodus-portability.md` §4.13 has named LP-17 since it was added, and `l2-nodus-portability`
§3.1 has carried it as "vacuous in core" — blocked on LP-11's own call site — since that row
was first written. LP-11 (Phase 24) and LP-16 (Phase 25) are both now real, and grounding
this pass confirmed that landing does *not* automatically realize LP-17: settlement needs a
genuinely new capability (moving value and returning proof of it) that no existing trait in
the crate expresses. This spec is that capability's Rust shape.

## 2. Constraints & Assumptions

- The core names no currency, wallet, rail, or payment protocol (VS-8, LP-2). `amount` is an
  opaque string the workflow declares and the host-supplied rail interprets; nodus never
  parses, compares, or reasons about it.
- The **decide** half (is this payment allowed right now, given the envelope and the
  recipient) is entirely the host's `PolicyProvider::evaluate` implementation's business —
  this spec adds no new decision vocabulary, no `SpendingEnvelope` type, and no allowlist
  concept to the core (LP-1, LP-2).
- The **act** half (actually settle, and prove it) needs a new trait because
  `PolicyProvider::evaluate` returns only `bool` — it has no channel for a receipt.
- Exactly one built-in ships (LP-2): `NoopSettlementRail`, which never settles anything
  (`None`, always `SETTLEMENT_UNACCOUNTED` if reached). It is **not** included in
  `HostCapabilities::builtin()`'s role set — the same precedent as `Storage`/`Policy`/
  `Dialog`, since there is no meaningful in-process payment.
- No new dependency (LP-1): the mechanism is one trait, one command, one error code, and a
  ninth `ExtensionRole` variant — all patterns the crate already has four/eight/twenty-five
  times over, respectively.
- VS-6's payment-required handshake (negotiate price → check envelope → gate → settle →
  retry the original request) is a **workflow-authoring or host-tool-internal** concern, not
  something this seam's `SETTLE` dispatch loops on itself — see §4.6.

## 3. Invariant Compliance

| L1 Invariant | Implementation |
| --- | --- |
| LP-17 Settlement effect seam | **Implemented [v1.0.1] — but the two halves cannot meet in one run (Partial, pending).** `run_with_settlement*` pairs a real rail with the permit-everything `NoopPolicyProvider`, `run_with_policy*` pairs a real policy with `NoopSettlementRail`, and `run_with_manifest*` runs on both built-ins; no public entry point takes a policy and a rail together. So every settlement a real rail performs is ungated — the envelope, allowlist and fail-closed checks (VS-2, VS-4, VS-5) this spec places in the host's policy never run — and a host that declares the `Settlement` role to the manifest gate still settles through the no-op rail. The tests exercise each half through a different entry point, which is how the gap stayed invisible. Closing it is the composed entry point `l2-nodus-runtime.md` §4.5 records; until it exists a host must not wire a real rail. As built: a new `EffectClass::Settlement` (§4.1) reuses the existing LP-11 gate (§4.2) for the decide half — no new decision call, no envelope/allowlist vocabulary in core. A new `SettlementRail` trait + `NoopSettlementRail` built-in (§4.3) supply the act half: `Executor::handle_settlement` (§4.4) dispatches a permitted `SETTLE` step to the rail and binds its `Option<Value>` receipt to the pipeline target, or pushes `NODUS:SETTLEMENT_UNACCOUNTED` (VS-7) on `None` — a `Signal`-free error, so an already-shipped mechanism (NL-9 dispatch) picks it up for free. `ExtensionRole::Settlement` (§4.5) extends the LP-8 capability manifest; `HostCapabilities::builtin()` does not provide it, so a manifest-gated workflow needing settlement is rejected pre-run without a host-supplied rail. `run_with_settlement`/`run_with_settlement_and_audit` (§4.6) mirror the `run_with_policy`/`run_with_dialog` combinator shape. Tests: 4 unit tests (`portability.rs`) + 6 integration tests (`tests/portability.rs`) covering settle-and-bind, gate-denial short-circuit (never reaches the rail), rail-returns-`None` unaccounted, both denial paths reaching NL-9 `@err:` dispatch automatically, positional `context.args` verbatim, manifest pre-run rejection with no step executed, and byte-for-byte regression for `SETTLE`-free workflows. |

## 4. Detailed Design

### 4.1 `EffectClass::Settlement` — the third variant

```text
[REFERENCE]
pub enum EffectClass { ModelCall, Deferred, Settlement }

const SETTLEMENT_COMMANDS: &[&str] = &["SETTLE"];

pub fn effect_class_of(command: &str) -> Option<EffectClass> {
    if MODEL_COMMANDS.contains(&command) { Some(EffectClass::ModelCall) }
    else if DIALOG_COMMANDS.contains(&command) { Some(EffectClass::Deferred) }
    else if SETTLEMENT_COMMANDS.contains(&command) { Some(EffectClass::Settlement) }
    else { None }
}

impl EffectClass {
    pub fn as_gate_str(self) -> &'static str {
        match self {
            EffectClass::ModelCall => "model_call",
            EffectClass::Deferred => "deferred",
            EffectClass::Settlement => "settlement",
        }
    }
}
```

`SETTLEMENT_COMMANDS` follows `MODEL_COMMANDS`/`DIALOG_COMMANDS`'s exact shape — a private
`const` slice reused by both `effect_class_of` (§4.2's gate) and `CapabilityManifest::
from_workflow` (§4.5), so the classification lives in one place. `SETTLE` is added to
`KNOWN_COMMANDS` (`vocab.rs`), `BUILTIN_SCHEMA_VERSION` bumps `"0.4.6"` → `"0.4.7"`. Grammar:
an ordinary three-positional-argument command, `SETTLE(payee, amount, purpose) → $target` —
no new lexer, parser, or transpiler work; `CommandCall`'s existing shape already carries
three raw args.

### 4.2 The decide half reuses the LP-11 gate unchanged

`execute_command`'s existing gate (§4.9.2/§4.9.3 of `l2-nodus-portability.md`) already
handles *any* `effect_class_of`-classified command uniformly — it does not special-case
`ModelCall`/`Deferred` today, so `Settlement` falls through it for free:

```text
[REFERENCE]
if let Some(class) = effect_class_of(&cmd.name) {
    let gate = class.as_gate_str();                 // "settlement" for SETTLE
    let context = /* existing command+args (+LP-16 descriptors) builder, unchanged */;
    if !self.policy.evaluate(gate, &context) {
        /* existing POLICY_DENIED push + None return, unchanged */
    }
}
```

The host's own `evaluate("settlement", context)` implementation is where VS-2 (envelope),
VS-4 (allowlist, tier), and VS-5 (fail-closed) all live — `context.args` already carries
`[payee, amount, purpose]` positionally (the same raw-args convention §4.9.2 established),
so a conforming host reads `context.args[0]`/`[1]`/`[2]` to check its own envelope and
allowlist, exactly as it would compute tier-then-friction for LP-16's descriptors. **No
change to this call site's own code is needed** — `Settlement` is handled by construction
the moment `effect_class_of` recognizes `SETTLE`.

### 4.3 `SettlementRail` — the act half

```text
[REFERENCE]
pub trait SettlementRail {
    /// Attempt to settle a permitted payment. `cmd.args` carries
    /// `[payee, amount, purpose]` raw, exactly as declared — nodus parses
    /// none of them. `None` means unaccounted (VS-7): the rail could not
    /// produce a verifiable receipt, and the payment MUST NOT be treated as
    /// settled.
    fn settle(&self, cmd: &CommandCall) -> Option<Value>;
}

pub struct NoopSettlementRail;

impl SettlementRail for NoopSettlementRail {
    fn settle(&self, _cmd: &CommandCall) -> Option<Value> {
        None   // no rail wired — every settlement is unaccounted (VS-8: cannot pay)
    }
}
```

The receipt is an opaque `Value` — nodus does not define a fixed receipt shape (LP-2); a
conforming rail returns whatever `Value::Map` fields prove the payment (recipient, amount,
time, an unforgeable signature/reference — VS-7), and nodus's only obligation is to treat
`None` as unaccounted and `Some(v)` as the step's ordinary return value. `SettlementRail`
takes `&CommandCall`, not three typed parameters, for the same reason `PolicyProvider::
evaluate` takes a generic `context: &Value` — the trait names no currency/amount type,
keeping the core's zero-vocabulary posture (LP-1/LP-2) rather than inventing a `Payment`
struct that would just duplicate `CommandCall.args`.

### 4.4 Call-site integration: `handle_settlement`

Mirrors `handle_dialog`'s existing shape — a dedicated branch after the LP-11 gate, before
the ordinary `dispatch()` match:

```text
[REFERENCE]
fn execute_command(&self, ctx, cmd, step_num) -> Option<Signal> {
    /* NL-2 rule check, unchanged */
    /* LP-11 gate over effect_class_of(&cmd.name) — §4.2, unchanged */

    if cmd.name == "ASK" || cmd.name == "CONFIRM" {
        return self.handle_dialog(ctx, cmd, step_num);
    }
    if cmd.name == "SETTLE" {
        return self.handle_settlement(ctx, cmd, step_num);
    }
    /* ordinary dispatch() for every other command, unchanged */
}

fn handle_settlement(&self, ctx: &mut ExecutionContext, cmd: &CommandCall, step_num: u32) -> Option<Signal> {
    self.emit(ctx, |seq, cid| ExecutionEvent::StepStart { .. });   // same shape as any command
    let receipt = self.settlement.settle(cmd);
    match receipt {
        Some(value) => {
            ctx.log_step(step_num, &cmd.name, value.clone());
            if let Some(target) = &cmd.pipeline_target { ctx.set_var(target, value); }
        }
        None => {
            ctx.errors.push(RuntimeError {
                code: vocab::error_code::SETTLEMENT_UNACCOUNTED.to_string(),
                step: step_num,
                reason: format!("settlement for '{}' produced no verifiable receipt", cmd.args.first().map(String::as_str).unwrap_or("?")),
            });
            // pipeline_target stays at its seeded default — VS-5/VS-6: no value
            // transferred, no paid action taken.
        }
    }
    self.emit(ctx, |seq, cid| ExecutionEvent::StepEnd { .. });
    None   // non-halting either way — Signal-free, so a denied/unaccounted
           // settlement reaches NL-9 @err: dispatch exactly like POLICY_DENIED
}
```

Both exits return bare `None` (no `Signal`) — a gate denial (POLICY_DENIED, from §4.2's
unchanged LP-11 check) and an unaccounted settlement (`SETTLEMENT_UNACCOUNTED`, from this
handler) are both ordinary `Signal`-free `RuntimeError`s, so NL-9's structural dispatch check
(`l2-nodus-error-dispatch.md` §4.1) reaches either one automatically — this spec adds no
dispatch-adjacent code at all, the eligibility rule already covers it by construction.

### 4.5 `ExtensionRole::Settlement` and the LP-8 manifest

```text
[REFERENCE]
pub enum ExtensionRole {
    Model, Audit, Storage, Policy, Vocabulary, Dialog, Environment, Config,
    Settlement,   // NEW
}
```

`CapabilityManifest::from_workflow` gains one arm, mirroring `MODEL_COMMANDS`'s:

```text
[REFERENCE]
if SETTLEMENT_COMMANDS.contains(&name) {
    manifest.roles.insert(ExtensionRole::Settlement);
}
```

`HostCapabilities::builtin()` is **not** extended to provide `Settlement` — the same
precedent as `Storage`/`Policy`/`Dialog`: there is no meaningful in-process payment, so a
workflow that declares (or auto-derives) a `Settlement` requirement and runs through
`run_with_manifest` against the plain builtin host is rejected pre-run with `NODUS:
CAPABILITY_UNMET` (LP-8 fail-fast), never reaching a step that would silently produce
`SETTLEMENT_UNACCOUNTED` at run time. A host that wires a real rail adds `.with_role(
ExtensionRole::Settlement)` explicitly.

### 4.6 Public API and the payment-required handshake boundary

```text
[REFERENCE]
impl Executor {
    pub fn with_settlement(rail: impl SettlementRail + 'static) -> Self;
    pub fn with_settlement_and_audit(
        rail: impl SettlementRail + 'static,
        audit: impl AuditProvider + 'static,
    ) -> Self;
}

pub fn run_with_settlement(
    source: &str, filename: &str, input: Option<Value>,
    rail: impl SettlementRail + 'static,
) -> Result<RunResult, Vec<Diagnostic>>;

pub fn run_with_settlement_and_audit(
    source: &str, filename: &str, input: Option<Value>,
    rail: impl SettlementRail + 'static, audit: impl AuditProvider + 'static,
    run_id: &str, started_at: &str,
) -> Result<RunResult, Vec<Diagnostic>>;
```

Additive: a caller using any existing `run_with_*` variant gets `NoopSettlementRail`'s
always-unaccounted behavior, unreachable byte-for-byte unless a workflow actually declares
`SETTLE` (LP-16's own §4.9.5 purity guarantee, restated).

**VS-6's payment-required handshake** ("counterparty signals payment required → negotiate →
check envelope → gate → settle → retry the original request") is **not** something this
seam's `SETTLE` dispatch implements as a loop. `handle_settlement` settles once and returns
— it does not know what "the original request" was, does not retry anything, and does not
speak any payment-required protocol (that is host-tool-internal, e.g. inside a `FETCH`
adapter that gets a 402 and negotiates before nodus ever sees a step). Realizing the
handshake **as workflow-visible control flow** — "try the paid resource, on refusal `SETTLE`,
then retry the resource" — is an ordinary composition a workflow author writes today with
`?IF`/`~RETRY:n` over existing constructs; it needs no new nodus primitive, the same
"drives a host subsystem, introduces no new language primitive" pattern
`l1-content-segmentation.md`/`l1-document-understanding.md` (main workspace) each record for
their own host-bound pipelines.

## 5. Implementation Notes

1. `SettlementRail` is a fifth extension-point trait (after `ModelProvider`, `AuditProvider`,
   `DialogProvider`, `PolicyProvider`/`SchemaProvider`/`StorageProvider`/`ConfigProvider`/
   `EnvironmentProvider`) — the crate's ninth `ExtensionRole` overall.
2. No `Value` kind changes (NL-7 holds) — a receipt is an ordinary `Value::Map` a host
   populates however it wants.
3. `SETTLEMENT_UNACCOUNTED` registers beside the frozen 24-code set exactly as
   `CAPABILITY_UNMET`/`POLICY_DENIED` did — `(Error, Runtime)` classification, same category
   as `POLICY_DENIED` (a runtime-stage failure, not a validation-stage one).
4. Existing tests are unaffected — no fixture in the normative corpus declares `SETTLE`
   today, so this phase is purely additive with nothing to review for behavior change (unlike
   Phase 26's NL-9 dispatch, which retroactively activated dormant behavior for every
   existing `@err:`-declaring fixture).

## 6. Drawbacks & Alternatives

**Extending `PolicyProvider::evaluate` to return an enum with an embedded receipt, instead of
a new trait.** Rejected: it would conflate two different questions (may this proceed? / did
it happen, and with what proof?) into one call, and would force every non-settlement
`evaluate` caller (LP-11/LP-16's existing ModelCall/Deferred paths) to handle a receipt
variant that never applies to them — the same reasoning §6 of `l2-nodus-error-dispatch.md`
used to keep `$error` population separate from the gate's own boolean return.

**A typed `Payment { payee, amount, purpose }` struct instead of raw `&CommandCall`.**
Rejected: nodus has no currency/amount type to put in it (LP-1/LP-2), and `CommandCall.args`
already carries the three raw strings — a wrapper struct would be a renaming exercise with
no new information, the same reasoning that kept LP-11's `context` args raw and unresolved.

**Implementing the VS-6 payment-required retry loop inside `handle_settlement`.** Rejected
(§4.6): nodus's `SETTLE` dispatch has no notion of "the original request" it would retry,
and building one would require a new control-flow primitive (`l1-nodus-language.md` owns
that surface, not this spec) for a pattern an ordinary workflow can already express with
existing constructs.

**Adding `Settlement` to `HostCapabilities::builtin()`'s role set.** Rejected: it would make
`NoopSettlementRail`'s always-unaccounted behavior look like a satisfied capability to the
LP-8 manifest gate, silently converting a pre-run rejection (the honest outcome — "you asked
for a capability nothing here provides") into a per-step run-time `SETTLEMENT_UNACCOUNTED`
surprise. The `Storage`/`Policy`/`Dialog` precedent — a real built-in that still isn't
"provided" for manifest purposes — applies unchanged.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[PORTABILITY]` | `crates/nodus/src/portability.rs` | `EffectClass`, `ExtensionRole`, `CapabilityManifest`, `HostCapabilities` — where every addition in this spec lands |
| `[EXECUTOR]` | `crates/nodus/src/executor.rs` | `execute_command`, `handle_dialog` (the precedent `handle_settlement` mirrors), `RuntimeError` |
| `[VOCAB]` | `crates/nodus/src/vocab.rs` | `KNOWN_COMMANDS`, `BUILTIN_SCHEMA_VERSION`, `error_code`/`error_meta` — where `SETTLE` and `SETTLEMENT_UNACCOUNTED` register |
| `[ERR-DISPATCH]` | `.design/nodus/specifications/l2-nodus-error-dispatch.md` | The structural NL-9 mechanism that picks up a denied/unaccounted settlement automatically |
| `[VS-SOURCE]` | `.design/main/specifications/l1-value-settlement.md` | The main-workspace source invariants (VS-1…VS-8) this seam realizes |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.2 | 2026-09-24 | Core Team | Consistency pass (2026-09-24): LP-17 → Partial: no public entry point takes a policy and a rail together — `run_with_settlement*` pairs a real rail with the permit-everything policy — so every real settlement is ungated and the envelope, allowlist and fail-closed checks never run. The tests exercise each half through a different entry point. Test count dropped from the row. |
| 1.0.1 | 2026-08-01 | Core Team | **Implemented the LP-17 seam designed in v1.0.0 — Phase 27.** `EffectClass::Settlement` + `SETTLEMENT_COMMANDS` + `SETTLE` in `KNOWN_COMMANDS` (`BUILTIN_SCHEMA_VERSION` 0.4.6 → 0.4.7); `SettlementRail`/`NoopSettlementRail` beside `PolicyProvider`/`NoopPolicyProvider` in `portability.rs`; ninth `ExtensionRole::Settlement` + `from_workflow` derivation, absent from `builtin()`; `Executor.settlement` field (default `NoopSettlementRail`) across all constructors + `with_settlement`/`with_settlement_and_audit`; `handle_settlement` dispatches after the existing LP-11 gate exactly like `handle_dialog`, both exits (`POLICY_DENIED`, `SETTLEMENT_UNACCOUNTED`) Signal-free; `run_with_settlement`/`run_with_settlement_and_audit` in `workflows.rs`, re-exported from `lib.rs`. **No plan-time or implementation-time scope correction needed** — this spec was authored directly against the real `execute_command`/`handle_dialog`/`Executor` constructor code in the same session, so unlike Phase 24 (the first LP-11 build, which found several real spec/code divergences), this build matched every `[REFERENCE]` block's structural claim; the only adjustments were filling in illustrative elisions (exact `FieldDescriptor` construction, exact match-arm shape), not corrections. **One empirical catch during test-writing**: the first draft of the settlement fixture declared its pipeline target as `@out: $receipt` (a non-reserved name) and asserted it defaulted to `Some(Value::Null)` on denial/unaccounted — both failed, because only *reserved* variables (`out`/`error`/`meta`/…) are pre-seeded at context construction; an ordinary declared name starts absent from `vars` until actually bound. Fixed by using the reserved `$out` binding throughout the fixture, matching every sibling LP-11/LP-16/NL-9 fixture's own convention. 462 tests pass (was 452, +10); clippy clean; fmt clean after one auto-fix (import line wrap); `Cargo.toml`/`Cargo.lock` diff empty (LP-1 preserved); the one new `.expect()` (a unit test's `Parser::parse(src).expect("parse")`) sits inside `#[cfg(test)]`, matching the crate's own existing precedent for that exact pattern. §3's LP-17 row updated to Implemented. |
| 1.0.0 | 2026-07-31 | Core Team | Initial spec. Designed LP-17's Rust shape: a third `EffectClass::Settlement` variant reusing the *already-shipped* LP-11 gate unchanged for the decide half (VS-2/VS-4/VS-5 collapse entirely into the host's own `PolicyProvider::evaluate`, exactly as LP-16's tier computation did) — the crate's first genuinely new extension-point *trait*, `SettlementRail`, is needed only for the act half (VS-3/VS-7), since `PolicyProvider::evaluate` is boolean by design and has no channel for a receipt. **Self-corrected a prediction made at plan time**: the computed DA-6 next step assumed LP-17 would follow LP-16's exact low-footprint pattern (reuse an existing DSL surface, extend `context`, add nothing structural); grounding against `main`'s `l1-value-settlement.md` (VS-1…VS-8) and this crate's own extension-point precedents found that assumption wrong — settlement's act half is categorically different from a descriptor (an action with a return value, not a richer boolean input), so a new trait is genuinely required. Recorded here rather than silently adjusted, matching the session's standing discipline for a wrong prediction. `SettlementRail::settle` takes raw `&CommandCall` (not a typed `Payment`), matching LP-11's own raw-`context` precedent and keeping the core's zero-currency-vocabulary posture. `NODUS:SETTLEMENT_UNACCOUNTED` is a `Signal`-free error exactly like `POLICY_DENIED`, so it reaches NL-9's `l2-nodus-error-dispatch.md` dispatch check automatically — a real, unplanned synergy between the two most recent phases, found while designing this one, not assumed. `ExtensionRole::Settlement` is the ninth role and, like `Storage`/`Policy`/`Dialog`, is deliberately absent from `HostCapabilities::builtin()`'s provided set — a manifest-gated workflow with no real rail is rejected pre-run (LP-8), not left to discover unaccounted settlements one step at a time. VS-6's payment-required handshake is scoped explicitly to workflow-authoring / host-tool-internal territory (§4.6) — this seam settles once and returns, it does not retry an "original request" it has no notion of. Design only — nothing landed in `crates/nodus` yet. |
