---
phase: 34
name: "Command Seam & Labelled Simulation (l2-nodus-commands §4.1–§4.4, §4.8, §4.10)"
status: Todo
subsystem: "crates/nodus/src (commands.rs, executor.rs, portability.rs, observability.rs, vocab.rs, workflows.rs, lib.rs), crates/nodus/tests"
requires: []
provides: []
key_files:
  created: []
  modified: []
patterns_established: []
duration_minutes: ~
---

# Stage 34 Tasks — Command Seam & Labelled Simulation

**Phase:** 34
**Status:** Todo
**Strategic Goal:** Give a host a way to execute the commands the crate cannot, and stop the crate
reporting success for the ones it does not. After this phase every command outside the core-owned set
and the three existing roles reaches a `CommandProvider`; the executor's stub table lives behind it as
the labelled built-in `SimulatedCommands`, so a stand-in is recorded as one (`Simulated`, a
`SIMULATED:<command>` flag, an honest execution mode); an unclassified command is gated as
`tool_use`; and a host-declared command can finally do something. Validators (Phase 35) and effect
identity with command-scoped retry (Phase 36) build on this seam and are out of scope here.

## Scope note (read before starting)

Facts checked against source at planning time.

**1. The stub table is one `match` in `Executor::dispatch` (`executor.rs`).** `VALIDATE`, `PUBLISH`,
`NOTIFY`, `STORE`, `REMEMBER` and `FORGET` answer `Bool(true)`; `SCORE` answers `Float(0.85)`;
`QUERY_KB` and `APPEND` an empty list; `LOAD`, `RECALL`, `WAIT`, `DEBUG` and every other known command
`Null`; `COMPARE` a two-entry map; `TRANSLATE`/`SUMMARIZE` bracketed text; `FETCH` a `_stub` map;
`ESCALATE`/`ROUTE` push `ESCALATE:<target>`/`ROUTE:<target>` flags; `REFINE` reads the `$draft`
variable regardless of its arguments; an unknown name pushes `UNKNOWN_COMMAND:<name>`. `LOG`,
`ASSIGN`, `MERGE`, `TONE` and `RUN` touch the run's own state and stay in the executor. **Capture
this table from the code before T-34B01 edits it** — it is the golden the built-in must reproduce.
A request carries *resolved* arguments and no run state, so three answers change on purpose, each
the spec's own statement (`l2-nodus-commands.md` §4.3): `REFINE` answers its first resolved
argument instead of `$draft`; `FETCH`'s stub map and the `ESCALATE:`/`ROUTE:` targets, which echo
the raw first argument today, echo its resolved text instead. With a literal argument the two
coincide, so the golden test uses literals and separate tests pin the three changes.

**2. `effect_class_of` returns `None` for every name it does not list**, and its unit test
`effect_class_of_non_effectful_command_is_none` (`portability.rs`) asserts exactly that for `COUNTER`
and `FETCH`. That assertion is the behaviour this phase changes on purpose: rewrite it, do not route
around it.

**3. `Executor` is built as a struct literal at ten sites** (`ExecutorBuilder::build` and the nine
constructors `new` through `with_settlement_and_audit`, `with_boxed_audit` included), and
`ExecutorBuilder::default` is an eleventh literal of the builder itself. The field must be added at
each; the compiler proves it. `RunOptions` already holds an `ExecutorBuilder`, so `RunOptions::commands`
is one line beside `RunOptions::settlement`.
`ExtensionRole` and `Determinism` have no exhaustive `match` in the crate or in its consumers, so a
new variant cannot break one.

**4. Plan-time correction — each addition lands with its call site.** `l2-nodus-commands.md` §4.3
lists the whole trait and request at once. Shipping it that way would publish members nothing calls:
`validate` (no caller until Phase 35), `world` and the request's `effect_key`/`attempt` (none until
Phase 36) — the *seam declared, wiring pending* state this workspace has met twice and named as
published API that silently does nothing (LP-11 before Phase 24, LP-15's `store`/`load`). The types are
`#[non_exhaustive]` and the trait methods have defaults, so adding each later is a minor change
(LP-6). This phase therefore ships `execute`, `CommandRequest { command, args, modifiers, flags }`, the
four-way outcome, the answer and the failure; **T-34E01 records the staging in the spec.** The answer's
`receipt` *does* have a call site now: the executor puts it on the existing `EventAnnotations::receipt`
of the step's `StepEnd`.

**5. Existing flag assertions are membership tests** (`any(..)`, `contains(..)`) except two
(`tests/input_contract.rs` around lines 319 and 345, read them first), so the new
`SIMULATED:<command>` flag cannot disturb them.

**6. Consumers outside the crate:** `crates/core` (`model_bridge.rs`, `invocable_bootstrap/workflow.rs`)
uses `run_with_provider`, `run_with_options`, `ExecutionMode` and the result's flags. None matches a
changed enum. T-34T03 proves it with a workspace build rather than an argument.

**Build environment.** Run `cargo` through PowerShell; on the rustc `STATUS_STACK_BUFFER_OVERRUN`
class of crash (misleading `E0786`/`E0463`) retry with `-j 2` before suspecting the code.

**What must not happen.** No `Executor::with_commands` and no `run_with_commands*`: a single-seam entry
point pairs a real provider with the permit-everything policy and leaves every tool effect ungated
(`l2-nodus-commands.md` §4.10). No combinator that layers one provider over another (§4.3, LOC-2). No
validator evaluation (Phase 35) and no effect key or retry scoping (Phase 36). No change to what
`ModelProvider`, `DialogProvider` or `SettlementRail` answer. No new dependency (LP-1); `Cargo.toml`
and `Cargo.lock` stay untouched. No `unwrap()`, `panic!()` or `expect()` on a production path.

**Out of scope, and where it goes.** `^validator` evaluation, `VALIDATE`'s real result and the `WITHOUT`
clause → Phase 35. Effect identity, `attempt`, the commit memo, `EventAnnotations::command`, `world()`
→ Phase 36. Model-shaped commands through `ModelProvider`, a durable effect ledger, a per-block
identity → recorded in `l2-nodus-commands.md` §4.12, not planned.

## Atomic Checklist

Track D — Vocabulary

- [ ] [T-34D01] `COMMAND_FAILED` layer code beside `MODEL_CALL_FAILED`

Track A — The seam types (`commands.rs`)

- [ ] [T-34A01] `commands.rs`: the provider trait, the request, the four-way outcome, the answer and the failure
- [ ] [T-34A02] `SimulatedCommands`: the stub table moved behind the trait, answer for answer

Track C — Portability (`portability.rs`)

- [ ] [T-34C01] `EffectClass::ToolUse` fail-safe, `CORE_COMMANDS`, and the `Command` role with its manifest derivation

Track B — Executor (`executor.rs`, `workflows.rs`, `observability.rs`)

- [ ] [T-34B01] The seam path: provider field, `ExecutorBuilder::commands`, `RunOptions::commands`, and all four outcomes in `execute_command`
- [ ] [T-34B02] The record of an answer: the `SIMULATED:` flag, the receipt on `StepEnd`, and two run-level markers
- [ ] [T-34B03] The `tool_use` gate carries the resolved argument values
- [ ] [T-34B04] The end-of-run record: mode from what answered, `Determinism::ContainsHostCommands`

Track E — Specification

- [ ] [T-34E01] Reconcile the specs to what landed, including the staged trait

Track T — Validation

- [ ] [T-34T01] Seam and simulation tests
- [ ] [T-34T02] Gate, manifest, failure, mode and determinism tests
- [ ] [T-34T03] Full quality gates and the consumer check

Order (Sequential): D01 → A01 → A02 → C01 → B01 → B02 → B03 → B04 → T01 → T02 → E01 → T03.

## Detailed Tracking

### [T-34D01] `COMMAND_FAILED` layer code beside `MODEL_CALL_FAILED`

- **Spec:** l2-nodus-commands.md §4.8 · l2-nodus-errors.md §4.2/§4.3
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo test -p nodus error_registry_lockstep` and `cargo test -p nodus error_meta_maps_known_codes`
  green. `error_code::COMMAND_FAILED == "NODUS:COMMAND_FAILED"`; `error_meta` returns
  `Some((ErrorSeverity::Error, ErrorCategory::Runtime))` for it (assert added to
  `error_meta_maps_known_codes`); `error_registry_lockstep` lists `COMMAND_FAILED` and its pinned count
  moves 32 → 33 with the message naming it. `cargo clippy -p nodus --all-targets -- -D warnings` clean.
- **Handoff:** T-34B01 raises it.
- **Notes:** Registers exactly as `MODEL_CALL_FAILED` did (`vocab.rs`): the constant beside it, its
  `error_meta` arm beside line 376's, and the lockstep list. That list already carries the layer codes
  (`MODEL_CALL_FAILED`, `POLICY_DENIED`, …) inside its count of 32, so the count moves — do not leave it
  at 32 and drop the new entry to make the test pass.

### [T-34A01] `commands.rs`: the provider trait, the request, the four-way outcome, the answer and the failure

- **Spec:** l2-nodus-commands.md §4.3 · §4.1
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo check -p nodus` and `cargo clippy -p nodus --all-targets -- -D warnings` clean. New
  `crates/nodus/src/commands.rs`, re-exported from `lib.rs`, defines: `CommandProvider` with one required
  method `execute(&self, &CommandRequest<'_>) -> CommandOutcome`; `CommandRequest<'a>` with `command`,
  `args: Vec<Value>`, `modifiers: &[(String, String)]`, `flags: &[String]`; `CommandOutcome::{Done,
  Simulated, Failed, Unsupported}`; `CommandAnswer { value, flags, receipt }` with `new(value)` and
  chained `flag`/`receipt` setters; `CommandFailure { code, reason }` with `new`. `CommandRequest`,
  `CommandAnswer` and `CommandFailure` are `#[non_exhaustive]`. Unit tests
  `command_answer_builders_chain` and `command_failure_new_keeps_code_and_reason` pass.
- **Handoff:** T-34A02 fills the built-in; T-34B01 installs a provider.
- **Notes:** See scope note 4 — no `validate`, `ValidatorRef`, `ValidatorVerdict`, `world`, `effect_key`
  or `attempt` yet. Match the crate's style for a fallible-provider surface (`ModelError::new`). Doc
  comments state the data-safety rule: a failure reason is short and secret-free, never an argument, a
  credential or a raw response.

### [T-34A02] `SimulatedCommands`: the stub table moved behind the trait, answer for answer

- **Spec:** l2-nodus-commands.md §4.3 · §4.1
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo test -p nodus simulated_commands` green, including the golden test
  `simulated_commands_answer_what_the_stub_table_answered`: every one of the forty-four seam-owned builtin
  names plus one host-declared name answers `CommandOutcome::Simulated`, with the value scope note 1
  records (captured from `Executor::dispatch` before T-34B01 edits it, run with literal arguments);
  `ESCALATE` and `ROUTE` carry `ESCALATE:<first arg or "human">` and `ROUTE:<first arg or "unknown">` in
  the answer's flags, an absent or `Null` first argument taking the default; `REFINE` answers its first
  resolved argument, or `Null` with none; `FETCH`'s map echoes the resolved first argument as text under
  `source`; a host-declared name answers `Null`; the built-in never returns `Done`, `Failed` or
  `Unsupported`. `cargo clippy` clean.
- **Handoff:** T-34B01 routes the seam-owned commands through it.
- **Notes:** `SimulatedCommands` answers by name; it is unit-struct, `Default`, no I/O. The six
  core-owned names and `GEN`/`ANALYZE`/`ASK`/`CONFIRM`/`SETTLE` are not its business and the golden test
  asserts none of them is in its table. The retired `UNKNOWN_COMMAND:<name>` flag is replaced by the
  executor's `SIMULATED:<name>` (T-34B02).

### [T-34C01] `EffectClass::ToolUse` fail-safe, `CORE_COMMANDS`, and the `Command` role with its manifest derivation

- **Spec:** l2-nodus-commands.md §4.2 · §4.10 · l2-nodus-portability.md §4.9.1
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo test -p nodus portability` (the unit tests in `portability.rs` and
  `tests/portability.rs`) green with these changes: `effect_class_of_non_effectful_command_is_none` is
  rewritten to assert `None` only for `LOG`, `ASSIGN`, `MERGE`, `TONE`, `RUN`, `VALIDATE`; a new
  `effect_class_of_tool_shaped_and_unknown_commands_is_tool_use` asserts `Some(EffectClass::ToolUse)` for
  `FETCH`, `COUNTER`, `WRITE` and a name in no constant; `EffectClass::ToolUse.as_gate_str() == "tool_use"`;
  `CapabilityManifest::from_workflow` on a workflow calling a host-declared command yields both
  `ExtensionRole::Command` and `ExtensionRole::Vocabulary`, and on a workflow calling only `PUBLISH`
  yields neither; `HostCapabilities::builtin().provides(ExtensionRole::Command)` is `false`.
  `cargo clippy` clean.
- **Handoff:** T-34B03 uses the gate string; T-34T02 exercises the derivation end to end.
- **Notes:** `CORE_COMMANDS` is `pub(crate)` beside `MODEL_COMMANDS`. `ExtensionRole::Command` goes last so
  the derived `Ord` keeps every existing role's order. The module's doc comment on `EffectClass` ("a
  fourth `ToolUse` class is deliberately absent") is now false and must be rewritten in the same edit.

### [T-34B01] The seam path: provider field, `ExecutorBuilder::commands`, `RunOptions::commands`, and all four outcomes in `execute_command`

- **Spec:** l2-nodus-commands.md §4.4 · §4.8 · §4.10
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo test -p nodus` green with no test edited except those whose assertion this task
  changes on purpose (scope note 1's three answers), and `cargo clippy -p nodus --all-targets -- -D
  warnings` clean. `Executor` and `ExecutorBuilder` carry `commands: Box<dyn CommandProvider>` defaulting to
  `SimulatedCommands`, at the ten `Executor` literals and `ExecutorBuilder::default`;
  `ExecutorBuilder::commands(impl CommandProvider + 'static)` and `RunOptions::commands(..)` exist;
  `grep -rn "with_commands\|run_with_commands" crates/nodus/src` and `grep -rn "UNKNOWN_COMMAND"
  crates/nodus/src` both return nothing. Behaviour, pinned by `tests/commands.rs` (T-34T01 names the
  tests): under the built-in, `PUBLISH($in.x) → $out` binds `Bool(true)` and ends `Status::Ok`; a
  provider answering `Done` binds its value and raises the answer's flags; a host-declared command under
  the built-in answers `Null` and ends `Ok`; `Failed` with a registered code (`KB_UNAVAILABLE`) yields a
  `RuntimeError` carrying exactly that code, a `StepError` event with the matching `fault_identity`, a
  pipeline target left as it was and no `Signal`; `Failed` with an unregistered code yields
  `NODUS:COMMAND_FAILED` whose reason names the proposed code; `Unsupported` yields `NODUS:UNDEFINED_CMD`;
  in each failure the reason begins with the command name and contains no argument value; a step
  declaring `@err:` reaches its handler through the existing dispatch.
- **Handoff:** T-34B02 adds the record of an answer on top of this path.
- **Notes:** The field, its first reader and the failure arms land together on purpose: a `commands`
  field nothing reads would trip `dead_code` under `-D warnings`, and `CommandOutcome` has four arms a
  `match` must cover. Add the field in place at each literal; do not refactor the constructors onto the
  builder unless that shrinks the diff. In `execute_command` keep the order (NL-2 rule check, the LP-11
  gate, the `ASK`/`CONFIRM` and `SETTLE` handlers, then the rest); `dispatch` keeps the core-owned set
  (`LOG`, `ASSIGN`, `MERGE`, `TONE`, `RUN`), `GEN`/`ANALYZE` and `VALIDATE`, which stays the `Bool(true)`
  stub until Phase 35, and sends every other name to a new `handle_command`. Resolve arguments as
  `handle_gen` does (`$var` → its value, literal → `Text`, unbound → `Null`). Mirror
  `record_model_failure` (`executor.rs`) for the failures: the same `StepError` with a `FaultIdentity`,
  then a `RuntimeError` push, and let `execute_command`'s existing "errors grew, so the target stays
  unbound" test carry the rest — the step still emits its `StepEnd` and the run log still records it,
  exactly as a failed `GEN` does today. `error_meta` decides registered versus unregistered, so a
  provider cannot mint a code. `ctx.log_step` stays as it is; the `WITHOUT` clause change is Phase 35.

### [T-34B02] The record of an answer: the `SIMULATED:` flag, the receipt on `StepEnd`, and two run-level markers

- **Spec:** l2-nodus-commands.md §4.9
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo test -p nodus --test commands` green, with these pinned: a `Simulated` answer raises
  `SIMULATED:<command>` exactly once per command name, so two `PUBLISH` steps raise one
  `SIMULATED:PUBLISH` and a `PUBLISH` plus a `NOTIFY` raise two flags; a `Done` answer raises no
  `SIMULATED:` flag; a `Done` answer carrying a `receipt` puts it on that step's `StepEnd`
  `annotations.receipt`, and an answer with none leaves the field `None`; a role-owned or core-owned step
  carries no receipt. `cargo clippy` clean.
- **Handoff:** T-34B04 reads the two markers this task sets: any answer was `Simulated`, any answer was
  `Done`.
- **Notes:** The receipt has to travel from `dispatch` to the `StepEnd` emission in `execute_command`: a
  field on `ExecutionContext` cleared per command, or a returned pair, whichever is the smaller diff. The
  markers are two booleans on the context; they are read once, at run end. A `Failed` or `Unsupported`
  outcome sets neither — nothing was answered.

### [T-34B03] The `tool_use` gate carries the resolved argument values

- **Spec:** l2-nodus-commands.md §4.2
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo test -p nodus --test commands` (T-34T02 names the tests): with a recording
  `PolicyProvider`, a `PUBLISH($in.x)` step's gate `context` contains `command`, the raw `args`
  (`["$in.x"]`) **and** `values` (the resolved list); a `GEN` step's `context` has no `values` key; the LP-16
  descriptors still appear only when declared; a denying policy yields `NODUS:POLICY_DENIED`, the provider
  is never called, and the target stays unbound. `cargo clippy` clean.
- **Handoff:** T-34T02.
- **Notes:** Resolution is a pure read of the run's variables and happens before the gate (spec §4.2).
  Only the `ToolUse` class gains `values`; the other three classes' `context` is byte-for-byte unchanged.

### [T-34B04] The end-of-run record: mode from what answered, `Determinism::ContainsHostCommands`

- **Spec:** l2-nodus-commands.md §4.9 · l2-nodus-observability.md HO-12/HO-20
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo test -p nodus --test observability --test commands` green with these pinned:
  a non-stub model plus a workflow that reaches `PUBLISH` under the built-in records
  `ExecutionMode::Simulated { fidelity: SimFidelity::Structural }`; a non-stub model and a workflow of
  only `LOG`/`GEN` still records `Real`; a provider answering `Done` for `PUBLISH` under a non-stub model
  records `Real` and `Determinism::ContainsHostCommands`; the same provider plus a `GEN` records
  `ContainsModelCalls`; a caller-declared `Simulated { fidelity: Modeled }` is kept as declared. The
  existing `a_run_on_the_built_in_stub_records_itself_as_simulated`,
  `an_explicit_real_declaration_cannot_make_a_stub_run_real` and
  `a_declared_simulation_fidelity_is_kept_as_declared_on_the_stub` stay green unedited.
- **Handoff:** T-34T02 is the acceptance evidence.
- **Notes:** `effective_execution_mode` currently runs at the top of `execute_inner`; the manifest is
  written at the end, so the mode is decided there from the two markers T-34B02 sets. `Determinism` gains
  `ContainsHostCommands` (`observability.rs`, re-exported already): precedence is model calls, then host
  commands, then deterministic — the variant names the first disqualifier, not the full set.
  `Assumed`-verdict simulation joins in Phase 35.

### [T-34E01] Reconcile the specs to what landed, including the staged trait

- **Spec:** l2-nodus-commands.md (all) · l2-nodus-runtime.md §4.5/§4.1 · l2-nodus-portability.md §4.2/§4.9.1 ·
  l2-nodus-errors.md §4.2/§4.4/§4.5 · l2-nodus-observability.md HO-12/HO-20
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `node .magic/scripts/executor.js check-prerequisites --json --workspace=nodus` reports no
  `VERSION_DRIFT`/`STATUS_DRIFT`; each touched spec has a Document History row naming this phase; a scan
  of the touched specs finds no "specified, not built" or "pending build" left on a section this phase
  covers (§4.1–§4.4, §4.8, §4.10 and §4.9's mode, determinism and flag) and still finds it on the sections
  Phases 35–36 own; markdownlint clean (`npx markdownlint-cli2` over the touched files).
- **Handoff:** T-34T03.
- **Notes:** `l2-nodus-commands.md` records the staging (scope note 4): the trait ships with `execute`
  only, `validate`/`ValidatorRef`/`ValidatorVerdict` arrive with Phase 35 and `world`/`effect_key`/`attempt`
  with Phase 36 — a note in §5, not a change to the design. `l2-nodus-portability.md` §4.2's `Command` row
  (currently **Specified**, wired **No**) moves to implemented for the `execute` path and names
  `validate` and the identity members as still pending. `l2-nodus-errors.md`'s emission count:
  `UNDEFINED_CMD` is now emitted; `VALIDATION_FAILED` still waits for Phase 35. The DG-4 row in
  `l2-nodus-dialog.md` and the retry paragraph in `l2-nodus-control-flow.md` §4.4 stay as they are —
  Phase 36 owns them. INDEX version cells in the same edit as each header.

### [T-34T01] Seam and simulation tests

- **Goal:** Verify T-34A01, T-34A02, T-34B01 and T-34B02 against `l2-nodus-commands.md` §4.1, §4.3, §4.8.
- **Method:** New `crates/nodus/tests/commands.rs` plus a `ScriptedCommands` double in
  `tests/support/mod.rs` (a `CommandProvider` that answers from a table of canned outcomes and records the
  requests it saw). Tests: the partition lockstep — `CORE_COMMANDS`, `MODEL_COMMANDS`, `DIALOG_COMMANDS`
  and `SETTLEMENT_COMMANDS` are pairwise disjoint, each is a subset of `KNOWN_COMMANDS` (plus `ASSIGN` for
  the core set), and the remainder — the seam-owned set — has exactly forty-four members, so a command
  added to the vocabulary later cannot be left unowned or owned twice; the four outcomes' binding,
  failure and label behaviour as T-34B01's and T-34B02's Verify lines state it; a request carries
  resolved arguments (a `$var` arrives as its value, a literal as `Text`, an unbound name as `Null`);
  the seam-owned command's `StepStart`/`StepEnd` pair appears once; the built-in's three deliberate
  answer changes (scope note 1) each have a test of their own.
- **Status:** Todo

### [T-34T02] Gate, manifest, failure, mode and determinism tests

- **Goal:** Verify T-34B03, T-34B04 and T-34C01 against `l2-nodus-commands.md` §4.2, §4.9, §4.10.
- **Method:** Extend `tests/commands.rs` and `tests/observability.rs`: a `tool_use` denial never calls the
  provider and `context.values` carries the resolved arguments; a run of only core-owned commands is
  unchanged byte for byte (same `RunResult`, no `SIMULATED:` flag); the mode and determinism cases of
  T-34B04's Verify; `run_with_manifest` against `HostCapabilities::builtin()` rejects a workflow with a
  host-declared command with `NODUS:CAPABILITY_UNMET` naming the `Command` role, and accepts it once
  `.with_role(ExtensionRole::Command)` is added; `RunOptions::commands` installs a provider that answers a
  host-declared command for real.
- **Status:** Todo

### [T-34T03] Full quality gates and the consumer check

- **Goal:** Prove the phase leaves the crate and its consumers green, and that LP-1 held.
- **Method:** In PowerShell: `cargo test -p nodus` (record the new total against the count at phase entry),
  `cargo clippy -p nodus --all-targets -- -D warnings`, `cargo fmt --all -- --check`,
  `cargo check --workspace --all-targets`, and `cargo test -p cronus-core -j 2` for the consumers that
  drive nodus (`model_bridge`, `invocable_bootstrap::workflow`). Then `git diff --stat crates/nodus/Cargo.toml
  Cargo.lock` is empty, and a line-by-line scan of every phase-touched production file finds no new
  `unwrap()`, `panic!()` or `expect()` outside `#[cfg(test)]`. Finally walk the review list in
  `l2-nodus-commands.md` §5 — the `effect_class_of` assertion, the host-vocabulary test, the retry tests,
  the manifest tests, the observability pins — and record for each whether it needed an edit.
- **Status:** Todo
