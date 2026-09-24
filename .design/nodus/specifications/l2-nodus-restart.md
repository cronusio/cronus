# Nodus Bounded Self-Restart Implementation (Rust)

**Version:** 1.0.2
**Status:** Stable
**Layer:** implementation
**Implements:** l1-nodus-language.md

## Overview

Rust realization of the NL-23 bounded whole-run self-restart: a workflow MAY
restart its **entire run from the first step**, re-reading its `@in` inputs and
`§config`, bounded by a visible carried restart count, requestable only from a
run-boundary step, and reconstructing fresh rather than inheriting the prior
attempt's context. This spec records how the runtime realizes that as a bounded
loop **around** `execute_inner` — which is what makes "fresh reconstruction" fall
out structurally rather than needing to be enforced.

Distinct from three things it resembles: a `~FOR`/`~UNTIL` body loop (which
carries state across iterations), NL-12 suspend/resume (which *continues*, not
restarts), and NL-22 compensation (which *undoes*; a restart does not — see §2).

## Related Specifications

- [l1-nodus-language.md](l1-nodus-language.md) — the language contract: NL-23 (the invariant realized here), NL-5 bounded loops (the `MAX:n` kinship), NL-7 closed value system, NL-8 reserved namespace, NL-20 `§config` (re-read on restart)
- [../../main/specifications/l1-loop-governance.md](../../main/specifications/l1-loop-governance.md) — the host-side contract NL-23 realizes: LG-11 scope-restart authority is the scope's own boundary, LG-6 counted ceiling, LG-5 fresh reconstruction from durable state
- [l2-nodus-config.md](l2-nodus-config.md) — `§config` acceptance; a restart re-reads the accepted set rather than mutating it mid-run
- [l2-nodus-errors.md](l2-nodus-errors.md) — owns the error taxonomy this spec extends with `NODUS:RESTART_LIMIT`; the nested-restart rejection is a bare validator code (`E019`), not a taxonomy entry (§4.6)
- [l2-nodus-observability.md](l2-nodus-observability.md) — HO-7 dense per-run `seq` + `correlation_id`, which forces the one-event-stream-per-attempt decision in §4.5
- [l2-nodus-runtime.md](l2-nodus-runtime.md) — the runtime crate extended here (`vocab`, `parser`, `executor`, `validator`); its §3 records NL-23 as realized by this spec
- [l2-nodus-compensation.md](l2-nodus-compensation.md) — NL-22; the seam that makes a restart over effectful steps safe. Soft reference: neither spec requires the other to be implemented

## 1. Motivation

A workflow that discovers mid-run that its own premises were wrong — bad input
interpretation, a configuration that needs re-reading, a self-correction after a
failed strategy — has no way to start over. The available constructs are all
wrong for it: a body loop carries the very state that needs discarding,
`Status::Paused` resumes *into* the stale context, and re-invoking from outside
is the host's job and loses the bound. Without a bounded primitive, the
workflow-level workaround is mutating its own `§config` mid-iteration, which the
criteria-immutability discipline forbids precisely because it lets a run move its
own goalposts. This spec provides the bounded, counted, boundary-authorized
alternative.

## 2. Constraints & Assumptions

- No new external dependency (LP-1).
- **A restart re-runs; it does not undo** (NL-23(e)). It restores neither state nor already-committed effects. A restart over non-idempotent, non-compensated effectful steps **double-commits** — nodus does not silently un-commit, and this spec does not pretend otherwise. Safety comes from the effects being idempotent or from NL-22 compensation, both host-side.
- **Bounded, never unbounded** (NL-5 kinship / LG-6): the ceiling is declared, and a request past it is refused rather than honored.
- **Fresh reconstruction** (LG-5): each attempt builds a new execution context from `@in` / `§config` / durable state. The prior attempt's variable environment, logs, and accumulated errors are not inherited.
- **Boundary authority** (LG-11): a restart is legal only from a top-level step. A request from inside a `~FOR`/`~MAP`/`~PARALLEL` element or a `?SWITCH` arm is refused, because which context resumes and what of the in-flight siblings survives is undefined.
- No new `Value` variant (NL-7): the request and the count use existing value kinds.
- Restart **pacing/scheduling policy is host-supplied** (LP-2): core names no delay, backoff, or scheduler. Nodus restarts immediately or not at all; a host wanting a delay wraps the call.

## 3. Invariant Compliance

| L1 Invariant | Rust Enforcement |
| --- | --- |
| NL-23(a) Bounded by a visible carried count | The ceiling is declared in `§runtime:` as `restart_max: n` (a run-level property, so a run-level home). The attempt loop refuses to iterate past it and pushes `NODUS:RESTART_LIMIT`. The count is readable by flow logic as the reserved variable `$restart_count`, seeded fresh each attempt. |
| NL-23(b) Run-boundary authority only | The validator statically rejects a restart request nested inside a `~FOR`/`~MAP`/`~PARALLEL` body or `?SWITCH` arm (bare code `E019`, §4.4/§4.6). This is a validator-only gate — the executor never re-checks nesting at run time, relying on every caller having validated first (§4.2). That holds for the `run*` entry points; the test entry points and a host calling the public `Executor::execute` directly skip validation (`l2-nodus-testing.md` §4), so on those paths a nested request is not rejected. |
| NL-23(c) Fresh reconstruction, not inherited transcript | The restart loop wraps `execute_inner`, which constructs a new `ExecutionContext` on entry. Re-running it *is* the fresh reconstruction — there is no state-clearing routine that could be forgotten or drift out of sync with new context fields. `@in` and the accepted `§config` are re-read per attempt. |
| NL-23(d) Deterministic and additive | Same input + same restart decisions ⇒ same attempt chain (NL-6 unaffected: the restart constrains control flow, not rendering). A workflow declaring no `restart_max` and never requesting a restart takes exactly today's path — one attempt, byte-identical result. |
| NL-23(e) Re-runs, does not undo | No unwind, no state restore. The un-compensated double-commit consequence is documented (§2) and left to NL-22 / host idempotency, not papered over. |
| NL-5 Bounded loops | `restart_max` is the run-grain analog of `~UNTIL MAX:n`: mandatory to enable the feature, and the refusal on exhaustion mirrors loop exhaustion. |
| NL-7 Closed value system | `$restart_count` is an `Int`; the request is an existing value kind. No new variant. |
| NL-8 Reserved namespace | `$restart_count` joins `RESERVED_VARIABLES` **and** `RUNTIME_OWNED_VARIABLES`, so a user pipeline target naming it is an E013 error — the count cannot be forged by the workflow it informs. Caller input can still name it (`l2-nodus-runtime.md` §3, NL-8), but on the restart path the executor's own seeded value is applied after the caller's entries and wins. |
| NL-20 `§config` re-read | Each attempt re-reads the host-accepted config set. A restart does not re-open acceptance or let the run edit its own configuration — it re-reads, never re-authors (LP-10). |
| HO-7 Dense per-run `seq` | Each attempt is its own event stream with its own `correlation_id`, dense `seq`, and manifest (§4.5), so `event_count == highest seq + 1` holds per attempt instead of being broken by a restart. |
| LP-1 Zero dependency | A loop counter and an existing `Vec`; no new crate. |
| LP-2 Host-supplied policy | Pacing, backoff, and whether a restart is *desirable* are host concerns; core provides only bound + authority + freshness. |

## 4. Detailed Design

### 4.1 Declaring the ceiling

```text
[REFERENCE]
§runtime: { core: schema.nodus, restart_max: 3 }
```

Absent `restart_max`, self-restart is **disabled**: a request is refused with
`NODUS:RESTART_LIMIT`. Opt-in by declaration keeps the feature additive — no
existing workflow changes behavior.

### 4.2 Requesting a restart

The request is an ordinary typed directive value, not a new `Value` kind: a step
sets the reserved variable `$restart` (an existing kind — `Bool` true, or a `Map`
carrying host-meaningful detail nodus does not interpret). No new `Signal`
variant realizes this — the executor reads `$restart` from the attempt's final
variable state once `execute_inner` returns, the same as any other post-attempt
`RunResult.vars` inspection. Whether the request originated from a nested
per-item context never needs a runtime check: the validator's static rule
(§4.4) rejects any workflow containing such a request before it can run at all,
so every program the executor ever executes already satisfies the boundary-
authority constraint (LG-11) by construction — refused where checkable, and
nothing is left needing a runtime fallback.

### 4.3 The attempt loop

```text
[REFERENCE]
// A bounded loop AROUND execute_inner — not inside it.
attempt = 0
loop:
    seed $restart_count = attempt        // visible to flow logic this attempt
    (result, _) = execute_inner(ast, input, ...)   // fresh ExecutionContext
    if result did not request restart      ⇒ return result
    if attempt + 1 > restart_max          ⇒ push NODUS:RESTART_LIMIT
                                            return result   // refused, not looped
    attempt += 1
```

Wrapping rather than reaching inside `execute_inner` is the load-bearing choice:
freshness is a property of re-entering the function, so no future context field
can be accidentally carried across an attempt.

### 4.4 Validator

- A restart request written inside a `~FOR`/`~MAP`/`~PARALLEL` body or a `?SWITCH` arm → a validator error, bare code `E019` (not a `vocab.rs` `NODUS:*` constant — see §4.6). Statically detectable by the existing AST walk; because this rule runs before any execution, the executor never needs a runtime re-check (§4.2).
- `restart_max` outside `1..=10` → error, mirroring `~RETRY:n`'s bound check (E017's shape) so the run grain inherits the same sanity ceiling as the step grain.

### 4.5 Observability: one event stream per attempt

Each attempt gets its own `correlation_id`, its own dense `seq` space, and its
own `RunManifest`. Forced by HO-7: `RunManifest.event_count == highest emitted
seq + 1` is a per-manifest identity, and a single shared stream across attempts
would either break the identity or make an attempt boundary invisible in the
trace. Per-attempt streams keep each attempt independently auditable — the same
reasoning NL-18 applies to recursive children.

Linking the attempts into one chain in the manifest (a chain id, as NL-18's
parent/root correlation does) is **deliberately not specified here** — it is an
`l2-nodus-observability` concern, and adding a manifest field from this spec
would put a cross-spec change in a restart phase. Today the chain is observable
through `$restart_count` per attempt.

### 4.6 Error codes

```text
[REFERENCE]
// vocab.rs — one new code (Phase-13 CONFIG_INVALID precedent)
pub const RESTART_LIMIT: &str = "NODUS:RESTART_LIMIT";   // (Warn, Control)
```

`RESTART_LIMIT` is a **warning**, mirroring `MAX_REACHED`: hitting the ceiling is
a bounded construct reaching its bound — a normal outcome the run reports, not a
fault. It joins `error_meta` and the lockstep test.

The nested-restart-request rejection (§4.4) is realized as a **bare validator
code** — `"E019"`, not a `vocab.rs` `NODUS:*` constant, and not part of
`error_meta` or the lockstep count. This matches the crate's existing
convention for purely structural, statically-decidable validator rules
(E010/E013/E014/E016/E017 are the same shape): a `vocab.rs` `NODUS:*` code is for
a condition with runtime-facing meaning — something a host or trace consumer
might branch on — while `E019` never reaches a run at all (the validator gate
refuses it first), so it has no place in the runtime-facing registry. §6
records why this was chosen over the originally-specified `Signal::Restart` +
`NODUS:RESTART_SCOPE` design.

## 5. Implementation Notes

Vertical-slice order (each slice compiles and is independently verifiable):

1. `restart_max` parsing in `§runtime:` + the validator bound check — declaration surface only, no behavior.
2. One error code (`RESTART_LIMIT`) + its `error_meta` row + lockstep-test extension. The nested-restart rejection's `E019` is not a `vocab.rs` code and lands in slice 4 (§4.6).
3. `$restart` into `RESERVED_VARIABLES` only (reserved-but-writable, like `$out`/`$draft`); `$restart_count` into **both** `RESERVED_VARIABLES` and `RUNTIME_OWNED_VARIABLES` (NL-8 — the count is unforgeable, the request is not).
4. The static `E019` validator rule rejecting a restart request nested inside a `~FOR`/`~MAP`/`~PARALLEL` body or `?SWITCH` arm. No `Signal::Restart` variant is added — see §6.
5. The attempt loop around `execute_inner`, plus `RESTART_LIMIT` on exhaustion.

Slice 5 last: it is the only slice that changes an existing run's control flow,
and by then every guard it depends on is already provable.

## 6. Drawbacks & Alternatives

- **Restart as a `~UNTIL` loop wrapping every step**: rejected — a body loop carries state across iterations, which is precisely what a restart must discard; it also gives no run-boundary authority and would make the whole workflow one nested block.
- **Reusing `NODUS:MAX_REACHED` for the ceiling**: rejected, narrowly. It has the right severity and semantics, but reusing it would make a run-grain restart refusal indistinguishable from a `~UNTIL` exhaustion in a trace — and the whole point of a counted ceiling is that the chain position is legible. A distinct code costs one registry row.
- **A new `Value::Directive` variant for the request**: rejected — violates NL-7's closed value system for no gain; an existing kind in a reserved variable carries the same signal.
- **Restarting by re-invoking from the host**: rejected as the *only* mechanism — it loses the bound (nothing counts the chain), loses the authority rule (the host cannot see which context asked), and pushes a correctness contract into every host. Available as a complement, not a substitute.
- **Clearing state in place instead of re-entering `execute_inner`**: rejected — a clearing routine must be updated every time the context gains a field, and the failure mode is silent state leakage across attempts.
- **A `Signal::Restart` executor variant plus a `vocab.rs` `NODUS:RESTART_SCOPE` constant** (the original design in earlier drafts of this section): dropped during implementation. Every comparable structural rule already in this crate (E010/E013/E014/E016/E017) is validator-only, with zero runtime re-check, because the executor is never invoked on a workflow the validator gate has not already passed — adding a `Signal`-based interception here would have been this crate's first redundant enforcement layer for a condition the validator already refuses before any run starts. The bare validator code `E019` is the actual realization (§4.4/§4.6); this section and the `INDEX.md` registry entry are corrected to describe it accordingly.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[EXEC]` | `crates/nodus/src/executor.rs` | `execute_with_restart` — the attempt loop; `execute_inner` — the function it wraps; `ExecutionContext` construction (where freshness originates) |
| `[VOCAB]` | `crates/nodus/src/vocab.rs` | `RESERVED_VARIABLES` / `RUNTIME_OWNED_VARIABLES` (NL-8), `error_code` + `error_meta` + lockstep test |
| `[PARSER]` | `crates/nodus/src/parser.rs` | `§runtime:` block parsing — where `restart_max` is read |
| `[VALIDATOR]` | `crates/nodus/src/validator.rs` | the AST walk that detects a nested restart request; `~RETRY:n`'s bound check as the shape for `restart_max` |
| `[AST]` | `crates/nodus/src/ast.rs` | `RuntimeBlock` (gains the ceiling), `WorkflowFile.steps` (what "top level" means structurally) |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.0 | 2026-07-30 | Core Team | Initial spec — Rust realization of NL-23 (bounded whole-run self-restart): `restart_max` in `§runtime:` (opt-in; absent ⇒ disabled), `$restart` request + `$restart_count` exposure as reserved + runtime-owned variables (NL-8 shadow-rejected), `Signal::Restart` raised only from a top-level step, a bounded attempt loop **around** `execute_inner` so fresh reconstruction is structural rather than enforced by a clearing routine, `NODUS:RESTART_LIMIT` (Warn) on ceiling exhaustion and `NODUS:RESTART_SCOPE` (Error) for a request from a nested per-item context. Records the HO-7-forced decision that each attempt is its own event stream / manifest, and deliberately leaves attempt-chain linking to `l2-nodus-observability` rather than changing a manifest field from a restart spec. NL-23(e) double-commit consequence documented, not papered over. |
| 1.0.1 | 2026-07-30 | Core Team | Reconciled to Phase 18's as-built implementation (no design/status change) — §4.2/§4.3/§4.4/§4.6 rewritten: no `Signal::Restart` variant exists; the executor detects a restart request by reading `$restart` from the attempt's final `RunResult.vars` once `execute_inner` returns, relying on the validator's static rule to make nested requests unreachable at run time rather than re-checking at runtime. The nested-restart rejection is a bare validator code `"E019"`, not a `vocab.rs` `NODUS:RESTART_SCOPE` constant — it carries no `error_meta` row and is outside the lockstep count. §5 slice 3 corrected: only `$restart_count` is `RUNTIME_OWNED_VARIABLES`; `$restart` is reserved-but-writable, matching §3 (the authoritative row, which was already correct). §6 records why the `Signal`/`RESTART_SCOPE` design was dropped in favor of this crate's validator-only-enforcement precedent (E010/E013/E014/E016/E017). |
| 1.0.2 | 2026-09-24 | Core Team | Consistency pass (2026-09-24): NL-23(b) relied on every caller validating first; the test entry points and the public `Executor::execute` do not. NL-8 notes that caller input can name `$restart_count`, and that the executor's seeded value wins on the restart path. |
