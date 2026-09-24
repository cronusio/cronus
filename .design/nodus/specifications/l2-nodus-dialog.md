# Nodus Human-in-the-Loop Dialog Implementation (Rust)

**Version:** 1.4.3
**Status:** Stable
**Layer:** implementation
**Implements:** l1-nodus-dialog.md

## Overview

Concrete Rust realization of the dialog contract. Maps each `DG`-invariant from
`l1-nodus-dialog.md` to its enforcing mechanism in `crates/nodus`: the `ASK` and
`CONFIRM` commands and their modifiers, the `Status::Paused` run state and its
resume descriptor, the `DialogProvider` extension point with a built-in
synchronous resolver, the `ExtensionRole::Dialog` capability-manifest binding,
and the executor wiring that makes a dialog step block-then-resolve. The dialog
error codes (`DIALOG_TIMEOUT` / `DIALOG_REJECTED` / `PAUSED`) already exist in
the runtime taxonomy; this spec specifies their emission.

## Related Specifications

- [l1-nodus-dialog.md](l1-nodus-dialog.md) — the dialog contract this implements (DG-1…DG-11)
- [l2-nodus-runtime.md](l2-nodus-runtime.md) — runtime crate extended here: `vocab` (commands, `Status`), `executor`, the `run_with_*` family
- [l2-nodus-portability.md](l2-nodus-portability.md) — LP-2 extension-point pattern + the LP-8 `ExtensionRole` taxonomy this adds `Dialog` to; [ADDED v1.2.0] §4.10's LP-16 `+reversible`/`+external` declaration surface is what makes §4.8's placement advisory decidable; [ADDED v1.3.0] §3.2 tracks LP-22(c) — designed here in §4.3 as `ResumeDescriptor.workflow_digest` — as this document only records the realization verdict, not the field shape
- [l2-nodus-commands.md](l2-nodus-commands.md) — [ADDED v1.4.3] the stable effect key that gives DG-4's replay-recognition half a mechanism (§4.6 there)
- [l2-nodus-errors.md](l2-nodus-errors.md) — owns the `DIALOG_TIMEOUT`/`DIALOG_REJECTED`/`PAUSED` codes and their severity/category
- [l2-nodus-observability.md](l2-nodus-observability.md) — [ADDED v1.1.0] owns `EventAnnotations`, the HO-9/11/16/17 carrier §4.7 extends with a fifth field for DG-9's provenance signal; [ADDED v1.3.0] also owns `ReproRecipe.workflow_digest`, whose exact `digest_ast` computation §4.3's new `ResumeDescriptor.workflow_digest` (LP-22(c)) reuses rather than defining a second one

## 1. Motivation

`l1-nodus-dialog.md` defines a host-neutral dialog contract, but the crate has no
`ASK`/`CONFIRM` commands, no `Paused` run state, and no dialog extension point.
This spec records the minimal additions that realize the contract while keeping
the executor's run-to-completion model intact for the common (default-resolved)
case: a dialog backend trait following the established `ModelProvider`/
`AuditProvider` pattern, a built-in synchronous resolver, and a `Paused` signal
for the case a host must handle out-of-band.

## 2. Constraints & Assumptions

- No new external dependency (LP-1): the dialog backend is an in-tree trait with a built-in no-I/O resolver.
- `ASK`/`CONFIRM` are added to the schema vocabulary; existing `KNOWN_COMMANDS` and reserved variables are not mutated (LP-4 — additions only).
- The built-in resolver is synchronous and never blocks: it resolves a dialog from `+default` or signals `Status::Paused` (DG-2/DG-6).
- True cross-invocation suspend/resume is a host concern over the `Status::Paused` signal; the executor does not serialise or restore mid-run state itself.
- Raw human answers bound to variables are user data; the dialog audit events carry only typed/length descriptors, never the raw text (DG-7, reusing the observability `FieldDescriptor`).

## 3. Invariant Compliance

| DG Invariant | Rust Enforcement |
| --- | --- |
| DG-1 Host neutrality | The backend is a `DialogProvider` trait; the executor never names a UI. Built-in `DefaultDialogProvider` ships; interactive backends live outside the crate (LP-2). |
| DG-2 Blocking progression | The executor resolves a dialog step before advancing: it calls the provider, and on no resolution returns `Status::Paused` without running later steps. |
| DG-3 Typed binding | `ASK` coerces the answer to its `+type` (str/bool/confirm/choice/multi_choice) into a `Value`, then binds it to the pipeline target; a `+validate` failure re-prompts or routes per the validator. |
| DG-4 Suspend/resume | An unresolved blocking dialog and `!PAUSE` set `Status::Paused` and produce a `ResumeDescriptor` (workflow id + var snapshot + step index + generation digest, §4.3). "Resumption" is host re-invocation of `run`/`run_with_dialog` **from the top** with the same `source`/`input`, not a crate-side continuation from the snapshot — `paused_at`/the descriptor are never read back by any nodus function (verified against every `run`/`run_with_*` signature). It "continues from the suspension point" (L1) by **deterministic replay**: NL-6/HO-4 guarantee the prior trajectory re-derives identically, and a `DialogProvider` recognizing the already-answered prompt/action signature returns `DialogOutcome::Remembered` (DG-9 §4.7) instead of pausing again — behaviourally equivalent to continuation, without a continuation mechanism existing. **Caveat already tracked, not new**: replay is sound only when the pre-suspension segment is `Determinism::Deterministic` (`l2-nodus-observability.md` §4.7); a run whose prior steps included `GEN`/`ANALYZE` is `ContainsModelCalls`-flagged, and replaying it may not reproduce the same trajectory the pause recorded — an existing reproducibility limitation, not one this correction introduces. **Replay also re-performs effects (pending).** Re-invocation from the top re-executes every step before the pause, not only its model calls: a settlement, a store, a notification or a host write that ran before the dialog runs again on resume, and only the dialog answer is memoized (DG-9). Resume-by-replay is therefore safe only when every pre-suspension effect is idempotent or recognized as already done by its own provider, and nothing in the core checks that. **The recognition half now has a specified mechanism [v1.4.3].** Every command the command-execution seam answers carries a stable effect key that is identical when the host re-invokes with the same `run_id`, so a provider that honours "at most once per key" recognizes a pre-pause effect as already done (`l2-nodus-commands.md` §4.6). The core still cannot check that a provider does; specified, not built. |
| DG-5 Typed failures | `+timeout` → `NODUS:DIALOG_TIMEOUT`; `+strict` `CONFIRM` rejection → `NODUS:DIALOG_REJECTED`; both are `RuntimeError`s routed to `@err:`. |
| DG-6 Default-on-absence | `DefaultDialogProvider` resolves a dialog with a `+default` and otherwise signals pause; it never blocks (a non-interactive run completes or pauses, never hangs). |
| DG-7 Trace data-safety | Dialog events reuse `observability::FieldDescriptor` (type + length), never raw answer text. |
| DG-8 Capability declaration | `ExtensionRole::Dialog` is added to the manifest taxonomy; `CapabilityManifest::from_workflow` requires it for any `ASK`/`CONFIRM` whose step lacks a `+default`, so a host with no dialog backend is rejected fail-fast. |
| DG-9 Memoizable approval [ADDED v1.1.0] | **Implemented [v1.1.1] — Phase 28.** `+remember` needs no new grammar (§2's existing `+modifier` pass-through already carries it to `DialogProvider::ask`/`confirm`, unread by nodus, exactly like every other undeclared modifier). Nodus's own obligation — a distinct trace provenance on a memoized resolution — realizes as a new `DialogOutcome::Remembered(Value)` variant (§4.7), bound identically to `Answer` but tagged `DialogProvenance::Remembered` on the step's `StepEnd` annotations, vs. `Answered` for the ordinary path. The host's own `ask`/`confirm` implementation performs the entire recall / governance / fit-check pseudocode of L1 §4.6 internally (it already receives the raw `+remember` modifier and returns whichever outcome applies) — no new `DialogProvider` trait method needed. 467 tests pass (was 462, +5). |
| DG-10 Promotable remembered decision [ADDED v1.1.0] | **Realized for free by DG-9 — no separate mechanism [confirmed, Phase 28].** L1 §4.7's `recurrence`/`consider_promotion` pseudocode is entirely host-side: a host's `DialogProvider` already owns the durable store behind DG-9 (it is the one that decided to return `Remembered`), so it already knows a dialog identity's occasion count without nodus reporting anything further. DG-9's per-event `DialogProvenance::Remembered` tag *is* the recurrence signal DG-10 builds on — the same "rides an existing signal, no enumerated list" shape this session's NL-9 `@err:` dispatch and LP-17 `SETTLEMENT_UNACCOUNTED` also use. No code exists for DG-10 specifically, by design — nothing to implement. |
| DG-11 Latest-reversible placement & decision-ready payload [ADDED v1.2.0] | **Advisory — specified in §4.8.** The first dialog invariant that constrains *authoring* rather than runtime behaviour, and its L1 says so: both properties are ones a validator MAY advise on, and neither touches the `Paused`/resume contract (DG-4) or memoization (DG-9). Realized as two `Severity::Warning` validator advisories over data the validator already collects — `W016` for a dialog placed earlier than its own dependencies require, `W017` for a prompt that inlines a produced artifact instead of referring to it. Neither blocks a run; the brief-quality half of the payload rule is declared out of core scope (§4.8.3), because judging prose is a model act and this crate is model-free. |

## 4. Detailed Design

### 4.1 Vocabulary & Status additions

```text
[REFERENCE]
// vocab.rs — added to KNOWN_COMMANDS
"ASK", "CONFIRM"

// executor.rs — Status gains a variant (l2-nodus-runtime §4.7(h))
pub enum Status { Ok, Partial, Failed, Aborted, Paused }
```

`ASK`/`CONFIRM` modifiers (`+type`, `+options`, `+hint`, `+default`, `+validate`,
`+timeout`, `+strict`, `+actions`) reuse the existing `CommandCall.modifiers`
vector — no grammar change beyond recognising the two command names.

### 4.2 DialogProvider extension point

```text
[REFERENCE]
pub enum DialogOutcome {
    Answer(Value),   // resolved (by human or +default)
    Pause,           // no resolution available; caller should suspend
    Timeout,         // +timeout elapsed → DIALOG_TIMEOUT
    Rejected,        // +strict CONFIRM rejected → DIALOG_REJECTED
}

pub trait DialogProvider {
    fn ask(&self, prompt: &str, modifiers: &[(String, String)]) -> DialogOutcome;
    fn confirm(&self, content: &str, modifiers: &[(String, String)]) -> DialogOutcome;
}

/// Built-in synchronous resolver: returns the `+default` as an `Answer`,
/// otherwise `Pause`. Never performs I/O, never blocks (DG-6).
pub struct DefaultDialogProvider;
```

`DefaultDialogProvider` mirrors `StubProvider`/`NoopAuditProvider`: it satisfies
the interface with no I/O and is the provider the plain `run`/`run_with_*` paths
use.

### 4.3 Resume descriptor

```text
[REFERENCE]
pub struct ResumeDescriptor {
    pub workflow: String,                 // "wf:<name>", a display label — never a resolution key (LP-22(c))
    pub vars: HashMap<String, Value>,     // environment snapshot at suspension
    pub step_index: u32,                  // suspended step
    pub workflow_digest: String,          // content identity of the pinned definition — LP-22(c) [IMPLEMENTED v1.4.0]
}
```

`RunResult` carries `Option<ResumeDescriptor>`, populated only when
`status == Paused`. The runtime defines the descriptor; the host persists it for
its own bookkeeping (LP-1) — **no nodus function accepts a `ResumeDescriptor` as
input** (verified: `run`/`run_with_*` take only `source`/`filename`/`input` plus
one provider parameter each; `paused_at` is written at suspension and never read
back). "Resume" is therefore not a continuation the crate performs — it is the
host calling `run`/`run_with_dialog` **again from the top**, with a
`DialogProvider` whose `ask`/`confirm` recognizes the already-answered dialog by
its prompt/action signature and returns `DialogOutcome::Remembered` (DG-9 §4.7)
instead of pausing a second time. `vars`/`step_index` are diagnostic snapshots
for that bookkeeping (audit, UI, "what was this run waiting on"), not inputs to
a re-entry API that does not exist. See §3's DG-4 row for why this satisfies the
L1's "resumption continues from the suspension point" without literal
continuation. The descriptor never includes raw prompt text (DG-7).

**`workflow_digest` (LP-22(c)) [IMPLEMENTED v1.4.0].** `l1-nodus-portability.md` LP-22(c)
requires a resume descriptor to name the **pinned generation**, not the
launch-time name — and `workflow: String` is exactly a launch-time name
(`"wf:<name>"`, the `§wf:` header's declared identifier), which is the one thing
LP-22(c) says must not be used alone. Because resume is host-orchestrated
re-invocation (§4.3 above) rather than a crate-internal continuation, the core
has no call site at which to *enforce* generation pinning — the obligation is
necessarily discharged as **declared, verifiable data**: `workflow_digest` is
the exact `digest_ast(ast)` already computed for `ReproRecipe.workflow_digest`
(`l2-nodus-observability.md` §4.7, `crate::observability::ReproRecipe`) — no new
hashing scheme, the same `std`-only `DefaultHasher` over `format!("{ast:?}")`,
zero-dep (LP-1). A host that persists workflow definitions by name and
re-resolves them at resume time compares the descriptor's `workflow_digest`
against a fresh digest of whatever source it is about to resume with; a
mismatch means the definition changed since suspension and resuming would
violate LP-22(c) — the host's decision what to do about that (refuse, warn,
proceed anyway) is out of core scope (LP-2), matching every other host-enforced
seam in this document. **Content identity, not byte identity**: reusing
`digest_ast` (hashes the parsed AST) rather than `l2-nodus-environment.md`'s
sibling `digest_source` (hashes the raw source string, `CandidateResult.workflow_digest`)
is deliberate — a purely cosmetic edit (reformatting, a comment change) does not
change what the run *is* for pinning purposes, and `ReproRecipe` already
establishes AST-level identity as this crate's reproducibility grain. **A
pre-existing naming collision worth flagging, not fixed here**: two fields
already share the name `workflow_digest` (`ReproRecipe` here, `CandidateResult`
in `l2-nodus-environment.md`) computed by two *different* functions — `digest_ast`
vs. `digest_source` — so a `ReproRecipe.workflow_digest` and a
`CandidateResult.workflow_digest` for the byte-identical file are not guaranteed
equal, and `l2-nodus-observability.md` §4.7's own inline comment ("the NE-12
precedent") reads as if they were. This third `workflow_digest` deliberately
follows the `digest_ast` precedent rather than adding a third computation; the
two-function inconsistency is a separate, pre-existing finding recorded in
`l2-nodus-portability.md` §3.2.

**The digest is a versioned, specified value.** Every `workflow_digest` is `nd1:` followed by the 16-hex-digit FNV-1a (64-bit) of the workflow's canonical text — its compact form, which round-trips to an equal AST (NL-6). The algorithm is fixed and published, so one definition digests to the same value on every build, platform and Rust release, and the canonical form changes only when the workflow does, not whenever an AST type gains a field. The `nd1` tag names the scheme: a digest under another tag is *not comparable* rather than merely unequal, and the tag is bumped whenever the algorithm or the canonical form changes, so a host comparing a descriptor after an upgrade can tell a scheme change from a real edit. It is a 64-bit non-cryptographic hash — fit for detecting an accidental change, not proof against a definition crafted to collide; a host that must resist that computes its own digest over the canonical text (LP-2). The same value backs `ReproRecipe.workflow_digest` and `CandidateResult.workflow_digest`, and the source-byte fallback for an unparseable source (`l2-nodus-environment.md` §4.4) uses the same scheme.

**Implemented [Phase 31].** Field added at `ResumeDescriptor`'s one construction
site (`executor.rs`), populated by the identical `digest_ast(ast)` call
`ReproRecipe`'s own construction makes a few lines below it. A unit test
(`executor.rs`) asserts the descriptor's digest equals `digest_ast(&ast)` for
the exact `ast` it parsed; an integration test (`tests/dialog.rs`, a capturing
`AuditProvider`) asserts it agrees with the independently-built
`ReproRecipe.workflow_digest` surfaced through `run_complete` for the same
paused run — the two construction sites are never compared to each other in
production code, so this is the assertion that makes the pinning claim
meaningful rather than coincidental. 484 tests pass (was 482, +2); clippy/fmt
clean; `Cargo.toml`/`Cargo.lock` diff empty (LP-1 preserved); no
`unwrap`/`panic!`/`expect` added to any production path.

### 4.4 Executor wiring

The executor gains a `DialogProvider` alongside its `ModelProvider`/
`AuditProvider`. `execute_command` dispatches `ASK`/`CONFIRM` to the provider:

- `Answer(v)` → bind `v` to the pipeline target (DG-3) and continue.
- `Pause` → stop the step loop, set `Status::Paused`, emit `NODUS:PAUSED` (info), and build the `ResumeDescriptor` (DG-2/DG-4).
- `Timeout` / `Rejected` → push the matching `RuntimeError`, route to `@err:` (DG-5).

`!HALT` sets `Status::Failed`; `!PAUSE` sets `Status::Paused` with a descriptor.
Dialog events (`ModelCall`-style, but dialog-tagged) carry `FieldDescriptor`
summaries only (DG-7).

### 4.5 Manifest integration

```text
[REFERENCE]
// portability.rs — ExtensionRole gains a variant
pub enum ExtensionRole { Model, Audit, Storage, Policy, Vocabulary, Dialog }
```

`CapabilityManifest::from_workflow` adds `Dialog` when it encounters an
`ASK`/`CONFIRM` whose step declares no `+default`; `HostCapabilities::builtin()`
does **not** provide `Dialog` (the default resolver only handles `+default`
dialogs), so a workflow needing real interaction is rejected fail-fast against a
non-interactive host (DG-8).

### 4.6 Public API

```text
[REFERENCE]
pub fn run_with_dialog(
    source, filename, input, dialog: &dyn DialogProvider,
) -> Result<RunResult, Vec<Diagnostic>>;

pub fn run_with_dialog_and_audit(
    source, filename, input, dialog, audit, run_id, started_at,
) -> Result<RunResult, Vec<Diagnostic>>;
```

Consistent with the orthogonal `run_with_*` combinator family (LP-5).

### 4.7 Memoizable & promotable approval (DG-9/DG-10) [ADDED v1.1.0]

```text
[REFERENCE]
pub enum DialogOutcome {
    Answer(Value),      // resolved fresh (by human or +default)
    Remembered(Value),  // resolved from a host durable prior decision (DG-9)
    Pause,
    Timeout,
    Rejected,
}

// observability.rs — EventAnnotations gains a fifth optional field,
// alongside message/anomaly/receipt/durability (HO-11/16/9/17); None for
// every non-dialog event and for a dialog step's non-resolution outcomes
// (Pause/Timeout/Rejected carry no provenance — they are not a decision).
pub enum DialogProvenance { Answered, Remembered }
pub struct EventAnnotations {
    // ...existing fields unchanged...
    pub dialog_provenance: Option<DialogProvenance>,
}
```

`handle_dialog` binds `Remembered(v)` through the exact same path as `Answer(v)`
(`ctx.set_var`, `ctx.log_step`) — memoization changes *where* the value came from,
never *how* it is bound, which is what keeps DG-9's "never turns a rejection into
an approval, never relaxes `+strict`, never binds a value failing `+type`/
`+validate`" discipline true by construction: a `Remembered` value flows through
the identical typed-binding path an `Answer` does, so it is subject to the same
`+type`/`+validate` handling, not a bypass. The only difference reaching the trace
is the `StepEnd` event's `annotations.dialog_provenance` — `Some(Answered)` on
`Answer`, `Some(Remembered)` on `Remembered`, `None` on `Pause`/`Timeout`/
`Rejected` (they resolve nothing to have a provenance).

**[CONFIRMED Phase 28]** `dialog_provenance` is the first `EventAnnotations` field the
crate's *own* dispatch logic populates directly, rather than a host annotating the stream
after the fact — every other field (`message`/`anomaly`/`receipt`/`durability`) has no
current writer inside the crate at all. `EventAnnotations`'s own doc comment (`observability.rs`)
was updated to say so explicitly, since only `handle_dialog` — not an external observer —
knows which `DialogOutcome` variant resolved a step. This does not change HO-6's closed
event-taxonomy discipline (no new `ExecutionEvent` variant, one field on the existing
carrier) — it only means "host-supplied" no longer describes every field on that carrier.

The host's `ask`/`confirm` implementation is where all of DG-9's real logic lives:
recall the durable prior decision keyed by the resolved prompt/action signature,
check governance permits reusing it, confirm the remembered value still fits the
step's `+type`/`+validate`, and return `Remembered` — or fall through to an
ordinary prompt and return `Answer` (fail-safe to asking, per DG-9's own
discipline). None of that needs a new `DialogProvider` method: `ask`/`confirm`
already receive the full raw `modifiers` slice, so a host checks for `+remember`
itself exactly as it would check for any other modifier it cares about.

**LP-6 note**: adding a `DialogOutcome` variant is a breaking change for any host
matching the enum exhaustively without a wildcard arm — acceptable pre-1.0 (the
`NoopStorageProvider` → `InMemoryStorageProvider` rename precedent), and the only
realistic shape for a closed 5-variant outcome taxonomy that must distinguish
*why* a dialog resolved without inventing a second orthogonal signal.

### 4.8 Latest-reversible placement & decision-ready payload (DG-11) [ADDED v1.2.0]

DG-11 differs in kind from DG-1…DG-10: it constrains how an author *positions and
composes* a dialog, not what the runtime does when one executes. Its L1 text settles the
realization question directly — both properties are ones "a validator MAY advise on", and
neither alters the `Paused`/resume contract (DG-4) or the memoization semantics (DG-9). So
this section specifies **validator advisories**: never errors, never runtime checks, and
never an automatic reordering of the author's steps. A run that ignores both advisories is
a conforming run.

#### 4.8.1 Why the validator can decide this at all

The placement rule needs three facts about a step, and the crate already produces all
three — this is why DG-11 is realizable now rather than after some future mechanism.

1. **Is this step a dialog?** `DIALOG_COMMANDS` (`portability.rs`) is the closed set
   `["ASK", "CONFIRM"]` — the same set `EffectClass::Deferred` already classifies on.
2. **Is a following step safe to run before the question?** LP-16's declaration surface
   (`l2-nodus-portability.md` §4.10) carries `+reversible` and `+external` as ordinary
   `CommandCall.modifiers`, parsed today with no new grammar. A step declaring
   `+external=true` or `+reversible=false` is exactly the boundary DG-11 says a dialog
   must never move past.
3. **Does a following step depend on the answer?** `collect_vars_stmt` already walks every
   statement's variable reads and writes for `E004`/`E014`. The dialog's pipeline target
   (`→ $answer`) is a write; the first later step that reads it closes the movable window,
   which is DG-11's second bound — deferring a question past the work it gates does not
   defer it, it answers it by assumption.

No new AST node, no new grammar, no new provider call, and no runtime data: the advisory is
computable at validation time, which NL-4 requires it to be.

#### 4.8.2 `W016` — dialog asked earlier than its own dependencies require

Emitted at `Severity::Warning` on a dialog step `D` when **all** of the following hold:

```plaintext
(a) D.command ∈ DIALOG_COMMANDS
(b) ∃ S after D in the same block, where
       S.command ∉ DIALOG_COMMANDS
       S declares +reversible=true
       S does not declare +external=true
(c) no step in (D, S] reads D's pipeline target variable
```

The message names the **last** such `S` — the step `D` could be moved past — and states the
reason in the author's terms: the human is being asked before work the run would have
settled on its own. `W016` never blocks a run.

**Condition (b) requires a positive declaration, and that is the design decision of this
section.** The alternative — firing on any following step that has *not* declared itself
irreversible — would give far higher recall and would be wrong in exactly the case DG-11
treats as the hard bound. LP-16's descriptors are *omitted, never defaulted* (§4.10.2 of
`l2-nodus-portability.md`): an absent `+reversible` means **not declared**, not "reversible".
An advisory that can recommend moving a confirmation past an irreversible effect is worse
than one that stays quiet, so recall is deliberately traded for soundness. The consequence is
a proportionate one: `W016`'s reach grows with LP-16 adoption, and declaring risk classes buys
placement advice — the two invariants compose rather than merely coexist.

The fixed-control-point case DG-11 explicitly excludes needs no special handling: a dialog
whose position is *not* the author's choice is one whose following step reads its answer, and
condition (c) already excludes it.

Two boundary cases the predicate must not leave to the reader. **A dialog with no pipeline
target** — a bare `CONFIRM` used as a gate, whose only effects are the pause and the
`+strict` rejection path — has an empty dependency set, so condition (c) holds for every
following step and placement is bounded by (b) alone. That is the intended reading: a gate
with no binding constrains nothing downstream, so it is the case where late placement helps
most, not a case to exempt. **The two clauses of (b) are not redundant**: a step may declare
`+reversible=true` *and* `+external=true` — an outward effect that can afterwards be
retracted, such as a message that can be recalled — and DG-11 bounds placement at
irreversible **or** outward, so an outward step stops the dialog moving past it even when it
is reversible.

#### 4.8.3 Payload — the checkable half, and the half that is not core's

DG-11's payload rule has two parts, and honesty about which one a validator can hold is the
substance here.

**Not core-checkable, declared so explicitly.** Whether a prompt is a *decision-ready brief*
— naming what the run produced, what the decision turns on, and what is being asked — is a
judgement about prose. A validator that scored prose would be a model, and this crate is
model-free by LP-1 with a deterministic `StubProvider` built-in. Recording this as out of
scope is preferable to leaving it implicitly unimplemented: the obligation stays with the
author, where DG-11 put it.

**Checkable: the reference-not-inlined half — realized against the whole-argument reference
model nodus actually has, not an interpolation scanner [CORRECTED at implementation].**
`W017`, `Severity::Warning`, fires when an `ASK`/`CONFIRM` argument is a bare `$var` —
nodus has no string-interpolation scanner; every variable reference the validator recognizes
elsewhere (`collect_vars_stmt`'s own `arg.starts_with('$')` test) is a whole positional
argument token, and this is the same model — whose root was written by a command in
`MODEL_COMMANDS` (`GEN` / `ANALYZE`) anywhere in the file: the dialog is inlining a produced
artifact into the prompt rather than referring to it, which is precisely what DG-11 says a
brief must not do. The producer is established by a **dedicated single-pass walker**
(`w017_collect_producers`), not the `collect_vars_stmt` walk §4.8.1 relies on for `W016`:
that function tracks *declared*/*used* variable-root sets for `E004`/`E014` and has no notion
of *which* command produced a target, so a purpose-built pass records `target-root →
producing command name` instead — no value is inspected either way, and the check remains a
pure validation-time property (NL-4). The message names the variable and the producing
**command** (e.g. "an earlier `GEN` step"), not a step number — `wf.steps` order is
irrelevant to this check, since a variable used before its producer is already a separate
`E014` finding.

Deliberately **not** a size heuristic: a length threshold would need the value, which the
validator cannot have (validation precedes execution, NL-4), and measuring the source text
instead measures the wrong thing — a short interpolation of a large artifact is the case
DG-11 is about.

**Code allocation.** `W001`…`W015` are in use in `validator.rs`; `W016` and `W017` occur
nowhere in the crate and are free.

#### 4.8.4 What this section does not settle

- **No runtime enforcement of either property.** DG-4's `Paused`/resume contract and DG-9's
  memoization are untouched, exactly as the L1 requires; nothing here changes what a run does.
- **`W016` sees one block at a time — precisely the sequence-bearing containers, not every
  block-shaped construct [CORRECTED at implementation].** A dialog inside `?IF`/`?ELIF`/
  `?ELSE` or `~FOR`/`~UNTIL` is scoped to its own body, and movement across that boundary is
  deliberately not advised: moving a dialog out of a conditional changes *whether* it is
  asked, not only when — a different decision from the one DG-11 governs, and not one a
  validator should nudge. `~PARALLEL` branches are walked for scopes nested further inside
  them but are never paired against each other: the language guarantees branches no order —
  `~PARALLEL` is a scheduling hint, even though this runtime runs them in sequence
  (`l2-nodus-runtime.md` §4.4) — so there is no "runs before it" relation between siblings
  for (b) to test. `?SWITCH` arms/default and
  `~MAP`'s command are a single `CommandCall`, not a sequence — a dialog can appear only as a
  whole arm's or map's action, which by construction has no following sibling to advise
  moving past, so it can never contribute a `W016` finding regardless of block type.
- **`W016`'s recall is bounded by LP-16 adoption, on purpose** (§4.8.2).
- **The brief's quality stays with the author** (§4.8.3), and no future core mechanism is
  implied by leaving it there.

## 5. Drawbacks & Alternatives

- **Mid-run coroutine suspension** (capture/restore the executor stack): rejected — it rewrites the run-to-completion model for every workflow to serve a minority of dialog steps. The `Status::Paused` + resume-descriptor signal achieves durable hand-off without that cost.
- **Folding dialog into `ModelProvider`**: rejected — conflates inference with human interaction; violates LP-5 (a host needing only one would be forced to supply both).
- **Blocking built-in resolver** (read stdin): rejected — it makes tests and headless runs hang; the default-or-pause resolver keeps non-interactive runs deterministic (DG-6).
- **New `DialogProvider::recall`/`may_memoize` trait methods** (DG-9): rejected — `ask`/`confirm` already receive the full `modifiers` slice, so a host can implement recall/governance entirely inside its own method body; a second trait method would duplicate a capability the existing signature already grants, the same reasoning `l2-nodus-settlement.md` §6 used to keep `SettlementRail::settle` on raw args rather than a typed wrapper.
- **A separate `DialogResolved` event instead of an `EventAnnotations` field** (DG-9): rejected — HO-6's closed event taxonomy is extended via optional fields on existing events (the HO-8/9/10/11/13/16/17 precedent), not new variants; dialog already reuses `StepStart`/`ModelCall`/`StepEnd` rather than minting dialog-specific events (DG-7), and provenance is no exception.
- **A nodus-side recurrence counter for DG-10**: rejected — the host's own durable store already knows a dialog identity's occasion count (it decided to return `Remembered`), so nodus tracking a parallel count would be redundant state that could drift from the host's own; DG-10 is fully discharged by DG-9's per-event provenance tag.

## 6. Implementation Notes

- Order: vocabulary + `Status::Paused` first (smallest, unblocks everything), then `DialogProvider` + `DefaultDialogProvider`, then executor dispatch, then manifest `Dialog` role, then the `run_with_dialog` combinators.
- `ExtensionRole::Dialog` is additive to the LP-8 enum; the `error_meta` registry and `validate_manifest` resolver already handle new roles generically.
- The dialog error codes are already canonical (`l2-nodus-errors.md`); this phase only adds their emission sites.
- **[ADDED v1.1.0]** DG-9/DG-10 add no new command, no new modifier registry entry (modifiers are already unconstrained free-form pass-through — confirmed no `KNOWN_MODIFIERS` registry exists, the same finding `l2-nodus-portability.md` §4.10 made for LP-16), and no new `DialogProvider` trait method — the smallest realization in this spec besides the original vocabulary addition: one `DialogOutcome` variant, one `EventAnnotations` field, one `handle_dialog` match arm sharing `Answer`'s binding logic.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[VOCAB]` | `crates/nodus/src/vocab.rs` | `ASK`/`CONFIRM` commands; error codes |
| `[EXEC]` | `crates/nodus/src/executor.rs` | `Status::Paused`, dialog dispatch, `ResumeDescriptor`, `handle_dialog`'s `DialogOutcome` match (§4.7) |
| `[PORT]` | `crates/nodus/src/portability.rs` | `ExtensionRole::Dialog`, manifest derivation |
| `[API]` | `crates/nodus/src/workflows.rs` | `run_with_dialog` combinators |
| `[OBS]` | `crates/nodus/src/observability.rs` | [ADDED v1.1.0] `EventAnnotations` — the `dialog_provenance` field §4.7 adds |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.4.3 | 2026-09-24 | Core Team | Design pass (2026-09-24): DG-4's "replay re-performs effects (pending)" gains the mechanism its recognition half was missing — the stable effect key `l2-nodus-commands.md` §4.6 gives every command the seam answers, identical across a re-invocation with the same `run_id`. The core still cannot check that a provider honours it; nothing is built. |
| 1.4.2 | 2026-09-24 | Core Team | Realization sync (2026-09-24): The workflow digest is a specified, versioned value (`nd1:` + FNV-1a-64 over the canonical compact form), stable across builds and toolchains; the limitation recorded as pending is closed. |
| 1.4.1 | 2026-09-24 | Core Team | Consistency pass (2026-09-24): DG-4: resume-by-replay re-performs every pre-pause effect, not only model calls — only the dialog answer is memoized. §4.3: `workflow_digest` is stable only within one build (`DefaultHasher` over the AST's `Debug` form) and carries no version, and it is a 64-bit non-cryptographic hash. The `W016` note no longer claims `~PARALLEL` branches run concurrently. |
| 1.1.1 | 2026-08-01 | Core Team | **Implemented the DG-9/DG-10 seam designed in v1.1.0 — Phase 28.** `DialogOutcome::Remembered(Value)` + `DialogProvenance{Answered,Remembered}` (`observability.rs`, beside `EventAnnotations`) + the `dialog_provenance` field landed exactly as designed; `handle_dialog`'s single `outcome` match now also computes the provenance for the one `StepEnd` emit, no second emit call. **Confirmed rather than assumed**: `dialog_provenance` is the first `EventAnnotations` field the crate's own dispatch logic populates directly (every sibling field has no in-crate writer at all) — `EventAnnotations`'s own doc comment updated to say so; recorded in §4.7 rather than silently adjusted. DG-10 needed zero code, confirming the v1.1.0 prediction. **One real test-writing finding**: asserting "no dialog provenance on rejection" against `DEFERRED_WF` (which declares `@err: ESCALATE(human)`) initially expected one `StepEnd`, but `DIALOG_REJECTED` is `Signal`-free so NL-9 dispatch fires automatically — two `StepEnd` events land (the rejected `ASK`, then the dispatched `ESCALATE`), both correctly carrying no provenance; the test was corrected, not the code. §3's DG-9/DG-10 rows updated to Implemented. 467 tests pass (was 462, +5: 1 unit test in `observability.rs`, 4 integration tests in `tests/portability.rs`); clippy/fmt clean; `Cargo.toml`/`Cargo.lock` diff empty (LP-1 preserved); no `unwrap`/`panic!`/`expect` added to any production path. |
| 1.4.0 | 2026-09-02 | Core Team | **Phase 31 — LP-22(c) implemented exactly per v1.3.0's design, no scope correction.** `ResumeDescriptor.workflow_digest: String` added at the struct's one construction site, populated by `digest_ast(ast)` — the identical call `ReproRecipe`'s own construction makes a few lines below it in `executor.rs`. Unit test (`executor.rs`) confirms the descriptor's digest equals `digest_ast(&ast)` for the parsed workflow; integration test (`tests/dialog.rs`, a capturing `AuditProvider`) confirms it agrees with the independently-built `ReproRecipe.workflow_digest` surfaced through `run_complete` for the same paused run — the assertion that makes the pinning claim real rather than coincidental, since the two construction sites are never cross-checked in production code. 484 tests pass (was 482, +2); clippy/fmt clean; `Cargo.toml`/`Cargo.lock` diff empty (LP-1 preserved); no `unwrap`/`panic!`/`expect` added to any production path. LP-22 overall stays **Partially realized** — (a)/(b) unaffected by closing (c). |
| 1.3.0 | 2026-09-02 | Core Team | Designs LP-22(c) (`l1-nodus-portability.md`) on `ResumeDescriptor`, the struct this spec owns: a `workflow_digest: String` field, computed by the same `digest_ast` function `l2-nodus-observability.md`'s `ReproRecipe.workflow_digest` already uses (`std`-only `DefaultHasher`, zero-dep), so a host can detect whether the definition it is about to resume differs from the one pinned at suspension. Corrects two overclaims found while grounding the design against source, both traced to the same misreading — that a "resume" API exists for the descriptor to feed: §4.3 said the host "re-supplies" the descriptor (to what? — verified against every `run`/`run_with_*` signature and `paused_at`'s read sites: nothing accepts one), and §3's DG-4 row said re-invocation "continues deterministically" as if from a snapshot. Both now state the real mechanism — resume is the host calling `run`/`run_with_dialog` again **from the top**, replaying the deterministic prefix (NL-6/HO-4) until `DialogProvider` recognizes the already-answered prompt via DG-9 memoization — which satisfies the L1's "continues from the suspension point" by replay rather than literal continuation, with the `Determinism::ContainsModelCalls` caveat (`l2-nodus-observability.md` §4.7) cited as the reason replay is not sound across a `GEN`/`ANALYZE`-containing prefix. Flags, without fixing, a **pre-existing** naming collision: `ReproRecipe.workflow_digest` (`digest_ast`, AST-level) and `l2-nodus-environment.md`'s `CandidateResult.workflow_digest` (`digest_source`, raw-text-level) are two different computations sharing one field name — `l2-nodus-observability.md` §4.7's own comment treats them as the same precedent, which they are not. Deliberately follows `digest_ast` here (content identity, not byte identity — a cosmetic edit should not read as a different generation). No code changed — this is spec-level design, decomposable by `/magic.task`. |
| 1.2.1 | 2026-09-01 | Core Team | Implementation-time reconciliation of §4.8.3/§4.8.4 to the as-built `crates/nodus` mechanism, landed alongside Phase 30. §4.8.3: `W017`'s trigger is the whole-argument `$var` reference model nodus actually has, not a string-interpolation scan (the crate has no interpolation scanner), and the producer lookup is a **dedicated** single-pass walker (`w017_collect_producers`, recording `target-root → producing command name`) rather than a reuse of `collect_vars_stmt`, which tracks declared/used sets only and has no producer-identity concept. §4.8.4: named precisely which constructs `W016` treats as their own scope — `?IF`/`?ELIF`/`?ELSE` and `~FOR`/`~UNTIL` bodies — and added the two cases the original text omitted: `~PARALLEL` branches are walked for nested scopes but never paired against each other (concurrent, not sequential — no "runs before it" relation for clause (b)), and `?SWITCH` arms/`~MAP`'s command are a single `CommandCall` with no internal sequence, so a dialog appearing as a whole arm's or map's action can never contribute a finding. No behavioural change — `W016`/`W017` ship exactly as designed; this row narrows how the design is described to match what was built. |
| 1.2.0 | 2026-09-01 | Core Team | Closes the DG-11 invariant-traceability gap found at the v1.40.0 nodus replan. New §4.8 realizes DG-11 as two validator advisories rather than runtime behaviour, following its L1's own "a validator MAY advise on" framing: `W016` for a dialog positioned earlier than its own dependencies require, and `W017` for a prompt that inlines a `GEN`/`ANALYZE`-produced artifact instead of referring to it. Decidable today because all three facts it needs already exist — `DIALOG_COMMANDS`, LP-16's `+reversible`/`+external` modifiers (§4.10 of `l2-nodus-portability`), and `collect_vars_stmt`'s read/write walk — so no new AST, grammar, provider call or runtime data is introduced. The section's load-bearing decision is that `W016` requires a **positive** `+reversible=true` declaration on the following step: LP-16 descriptors are omitted rather than defaulted, so firing on undeclared steps would advise moving a confirmation past a possibly irreversible effect — recall traded for soundness, with reach growing as LP-16 is adopted. The brief-quality half of the payload rule is declared **out of core scope** (judging prose is a model act; the crate is model-free per LP-1) rather than left silently unimplemented. `Paused`/resume (DG-4) and memoization (DG-9) untouched. |
| 1.1.0 | 2026-07-31 | Core Team | Closes the DG-9/DG-10 invariant-traceability gap flagged at the v1.29.0 nodus replan. New §4.7 designs both: DG-9's `+remember` marker needs no grammar change (modifiers are already unconstrained free-form pass-through to `DialogProvider::ask`/`confirm`); its one real nodus-side obligation — a distinct trace provenance on a memoized resolution — realizes as a new `DialogOutcome::Remembered(Value)` variant bound through `Answer`'s exact path, plus a new `dialog_provenance: Option<DialogProvenance>` field on the shared `EventAnnotations` carrier (the HO-8/9/10/11/13/16/17 closed-taxonomy-via-optional-field precedent, not a new event type). No new `DialogProvider` trait method: `ask`/`confirm` already receive the raw `modifiers` slice, so a host's own recall/governance/fit-check logic (L1 §4.6's pseudocode) lives entirely inside its existing method body. **DG-10 found to need no separate mechanism at all** — its recurrence/promotion pseudocode is 100% host-side, operating on the host's own durable store plus the DG-9 provenance tag nodus already emits, the same "rides an existing signal, no enumerated list" shape this session's NL-9 dispatch and LP-17 `SETTLEMENT_UNACCOUNTED` also use. §3 gains both compliance rows; §5 gains three rejected alternatives (a second trait method, a dedicated event type, a nodus-side recurrence counter) each traced to an existing precedent. Design only — nothing landed in `crates/nodus` this pass. |
| 1.0.0 | 2026-06-27 | Core Team | Initial spec — Rust realization of the dialog contract: `ASK`/`CONFIRM` vocabulary, `Status::Paused` + `ResumeDescriptor`, `DialogProvider` + `DefaultDialogProvider` (synchronous default-or-pause), `ExtensionRole::Dialog` manifest binding, executor dispatch, `run_with_dialog` combinators; DG-1…DG-8 compliance table. |
