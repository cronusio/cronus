# Workflow Runtime

**Version:** 1.3.2
**Status:** Stable
**Layer:** implementation
**Implements:** l1-workflow-language.md

## Overview

The concrete realization of the workflow language: a Rust runtime **crate inside the Cronus monorepo** (`crates/nodus`) — lexer, parser, validator (lint), executor, and transpiler — that the core depends on and links **in-process**, so it runs everywhere the core runs (desktop and mobile) with no external language process. It is kept as a self-contained crate (not fused into the core) so it can be **extracted to a standalone crate later** if it outgrows Cronus; for now it is vendored in-tree because no other consumer needs it. The core wires its step handlers to Cronus subsystems. Execution is schema-driven, validated, and bounded.

## Related Specifications

- [l1-workflow-language.md](l1-workflow-language.md) - The language model this runtime implements.
- [l2-core-library.md](l2-core-library.md) - The core depends on this runtime crate and binds its steps.
- [l2-source-layout.md](l2-source-layout.md) - Where this in-monorepo crate sits in the Cronus workspace.
- [l2-orchestration.md](l2-orchestration.md) - Delegated work / `/goal` loops execute workflows.
- [l2-model-router.md](l2-model-router.md) - Generation/analysis steps route models here.
- [l2-cli.md](l2-cli.md) - Command grammar standard for `workflow` commands.

## 1. Motivation

The language must run on every Cronus target — including the mobile thin client and the always-on hub — without a heavy external interpreter. Implementing the runtime in the Rust core (rather than embedding a separate language runtime) keeps it embeddable, fast, and dependency-free, satisfying the hub-and-spoke and mobile constraints.

## 2. Constraints & Assumptions

- The runtime is an in-monorepo Rust crate the core depends on; it links in-process (no external language runtime is bundled, no separate process).
- A formal grammar drives the parser; a schema is loaded before execution.
- The port preserves behavior parity with the upstream language specification (same sample workflows produce equivalent validation, execution, and transpilation results). The crate currently implements the **v0.4.6** vocabulary generation; the upstream specification has advanced to **v0.7**, and the outstanding parity surface is the implementation target tracked in §4.9.
- Steps call core subsystems through internal interfaces; the runtime owns no domain logic of its own beyond control flow.

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| WFL-1 Dual representation | The transpiler converts compact ↔ human losslessly; both parse to the same AST. |
| WFL-2 Schema contract | The validator loads the schema first; unknown vocabulary fails validation. |
| WFL-3 Hard constraints | The executor enforces declared hard constraints; a violation halts and escalates, regardless of caller. |
| WFL-4 Preferences soft | Preferences are advisory inputs to steps; never override hard constraints. |
| WFL-5 Validate before run | `run` invokes the validator (lint rules) first; parse/undefined-var errors halt. |
| WFL-6 Bounded execution | The executor enforces max-iteration/budget limits and honors halt/pause. |
| WFL-7 Subsystem-bound | Command handlers dispatch to memory, HITL, orchestration, quality, and the model router. |
| WFL-8 Result contract | Every run returns a structured result (success/failure) and runs the declared error handler. |
| WFL-9 Human view | The client surface renders the human form via the transpiler. |

## 4. Detailed Design

### 4.1 Pipeline (all in the Rust core)

```mermaid
graph LR
    SRC[workflow source] --> LEX[lexer] --> PARSE[parser -> AST]
    PARSE --> VAL[validator + lint]
    VAL --> EXEC[executor]
    PARSE --> TRANS[transpiler: compact <-> human]
    EXEC --> RES[structured result]
```

A formal grammar specification drives the parser; porting from the reference implementation's grammar is the starting point. Lint rules (errors/warnings/info) run in the validator.

### 4.2 Step binding

```mermaid
graph TD
    STEP[workflow step] --> DISP{command category}
    DISP -->|generate/analyze| MR[model router]
    DISP -->|remember/recall/forget| MEM[memory subsystem]
    DISP -->|ask/confirm/escalate| HITL[human-in-the-loop]
    DISP -->|route| ORCH[orchestration]
    DISP -->|validate| QA[quality gates]
    DISP -->|fetch/read/log/publish| SVC[core services]
```

The runtime is the scripting layer; each command handler calls the owning subsystem (WFL-7), so workflows compose existing capabilities rather than duplicating them.

### 4.3 Embeddability

Because the runtime is a Rust crate the core links in-process, it executes on desktop and mobile alike — there is no separate language process on any target. The always-on hub runs workflows for autonomous routines/goals; the mobile thin client can validate/preview and run foreground workflows. Keeping it a self-contained crate (not fused into the core) preserves a clean seam for later extraction while still linking statically into the core build.

### 4.4 Command surface

Workflow operations conform to the CLI grammar standard (see `l2-cli.md` §4.4).

| Action | CLI | TUI | Library (no code) |
| --- | --- | --- | --- |
| scaffold | `cronus workflow new <name>` | `/workflow new <name>` | `workflows.scaffold(name) -> Workflow` |
| validate | `cronus workflow validate <file>` | `/workflow validate <file>` | `workflows.validate(ref) -> Report` |
| run | `cronus workflow run <file>` | `/workflow run <file>` | `workflows.run(ref, input) -> Result` |
| transpile | `cronus workflow transpile <file> --to <compact\|human>` | `/workflow transpile …` | `workflows.transpile(ref, mode) -> string` |
| test | `cronus workflow test [<file>]` | `/workflow test …` | `workflows.test(ref?) -> Report` |

### 4.5 Port architecture & strategy

The crate is a behavior-preserving port of the reference implementation (~5k lines across six modules) into Rust. Modules and their responsibilities:

| Module | Responsibility | Reference scope |
| --- | --- | --- |
| `lexer` | tokenize the compact form | ~tokens + symbols/operators |
| `parser` → `ast` | build the AST per the formal grammar | grammar-driven; largest module |
| `validator` | structure + lint rules (errors/warnings/info) | the lint catalog |
| `executor` | step dispatch, control flow, bounded execution | command handlers + control keywords |
| `transpiler` | compact ↔ human, lossless | rendering both forms |

Schema and grammar are **data, not code**: the vocabulary schema and the formal grammar ship as resources the crate loads (so updating the language does not require recompiling logic).

**Incremental order (vertical slice first):**

1. `lexer` + `parser` + `ast` — parse a sample workflow to an AST.
2. `transpiler` — compact ↔ human round-trip on that AST (proves WFL-1).
3. minimal `executor` — a couple of commands (`log`, `generate`) end-to-end (proves WFL-7/8).
4. `validator` + full lint rules (proves WFL-5).
5. full command set + control flow (`?if`/`?switch`/`~retry`/`~map`/`!halt`/`!pause`).

**Parity testing:** the reference implementation's sample workflows + lint cases form a golden corpus; the Rust crate must produce equivalent validation verdicts, execution results, and transpilation output. The corpus lives in `crates/nodus/tests/fixtures/` as the normative fixture set every parity and round-trip test reads.

### 4.6 Step-file architecture for disciplined workflow execution

Long multi-phase workflows are decomposed into step files — small, self-contained instruction documents, one per execution step. This architecture prevents context overflow, enforces sequential discipline, and keeps the executing agent focused on one unit of work at a time.

#### JIT (just-in-time) loading

Only the current step file is loaded into the agent's context at any moment. The full workflow is not pre-loaded:

```text
[REFERENCE]
JIT loading rules:
  - Load step N only when the agent is ready to begin step N.
  - Unload (or deprioritize) step N-1 once step N begins.
  - Never load step N+1 while step N is in progress.
  - This keeps context token cost proportional to one step, not the whole workflow.
```

Loading the entire workflow upfront risks context saturation on long workflows and tempts the agent to "skip ahead" to later steps — both are failure modes this pattern prevents.

#### Sequential enforcement

Steps are executed in strict declared order. No skipping is allowed, even when a step appears to be a no-op for the current situation:

```text
[REFERENCE]
Sequential enforcement rules:
  - The agent must complete (or explicitly mark as skipped with a reason) each step
    before loading the next.
  - A step cannot be deferred — if it cannot be completed, the workflow HALTs.
  - Workflow order is the author's intent; unilateral reordering is an error.
```

The rationale: steps often have side effects or populate context that later steps depend on implicitly. Skipping breaks the append-only chain.

#### State tracking in frontmatter

Workflow state (which steps have completed) is tracked in a YAML frontmatter header on the primary output document, not in conversation history:

```text
[REFERENCE]
Frontmatter state block (on the workflow's primary output document):

---
stepsCompleted:
  - step-01-init
  - step-02-domain-analysis
currentStep: step-03-competitive-landscape
status: in-progress   # draft | in-progress | complete
---
```

The authoritative state is the executor's own durable step journal: a step is recorded complete when its result is accepted, and the frontmatter mirrors that journal so a reader — or an agent resuming after compaction — can see where the run stands without the conversation history, which is not reliable for this purpose. A `stepsCompleted` entry the executing agent writes itself is a claim, not a completion; on resume the executor trusts its journal.

#### Append-only document building

Workflow output documents are built incrementally. Each step appends its section to the document; earlier sections are never overwritten:

```text
[REFERENCE]
Append-only rules:
  - Each step writes exactly the section(s) it owns.
  - Completed sections are read-only; the agent never edits them in a later step.
  - The final document is the accumulation of all appended sections.
  - [ASSUMPTION] tags mark content the agent generated without explicit input —
    flagged for user review, not silently removed.
```

This rule makes partial output recoverable: if the workflow is interrupted mid-run, completed sections are already written and correct.

#### HALT at menus and decision points

When a step requires a choice the agent cannot make unilaterally, execution halts and the agent surfaces the decision to the user:

```text
[REFERENCE]
HALT conditions:
  - User choice required: multiple valid paths exist and the choice is not deterministic.
  - Ambiguous input: a required input is missing or contradictory.
  - External dependency not met: a prerequisite artifact does not exist yet.
  - Constraint conflict: the work would violate a declared constraint.

At a HALT point:
  - The agent states the specific decision or information needed.
  - The agent offers options if there are a small fixed set (≤4 recommended choices).
  - The agent does NOT improvise past the HALT — it waits for the user to respond.
```

A HALT is not a failure — it is the workflow correctly recognizing that the next step needs human intent. The agent should be specific about what it needs, not ask an open-ended question.

### 4.7 Platform-native capability lookup

Before a workflow step generates new code or recommends installing a dependency, the runtime checks a platform-native capability table. If the platform already ships a solution, the step uses it instead of generating custom code or adding a dependency.

#### Lookup principle

The lookup is applied at code-generation time using the decision ladder (see l2-quality-pipeline.md §4.13): standard library and platform-native capabilities are preferred over third-party dependencies, which are preferred over custom implementation. The lookup table makes this concrete — it maps common "thing I think I need" patterns to "what the platform already ships."

#### Lookup table (selected entries by domain)

```text
[REFERENCE]
Platform-native capability table (non-exhaustive; extend per project ecosystem):

HTML elements:
  You think you need: date picker widget library
  Platform ships: <input type="date">

  You think you need: accessible modal dialog library
  Platform ships: <dialog> element (built-in open/close, focus trap, ::backdrop)

  You think you need: lazy image loading library
  Platform ships: <img loading="lazy">

CSS capabilities:
  You think you need: responsive typography library
  Platform ships: clamp() (e.g. font-size: clamp(1rem, 2.5vw, 2rem))

  You think you need: dark-mode detection library
  Platform ships: @media (prefers-color-scheme: dark) CSS media query

JavaScript / Browser APIs:
  You think you need: uuid library
  Platform ships: crypto.randomUUID()

  You think you need: deep-clone utility
  Platform ships: structuredClone()

  You think you need: custom event bus
  Platform ships: EventTarget + addEventListener/dispatchEvent

Node.js stdlib:
  You think you need: mkdirp (recursive mkdir)
  Platform ships: fs.mkdirSync(path, { recursive: true })

  You think you need: rimraf (recursive delete)
  Platform ships: fs.rmSync(path, { recursive: true, force: true })

  You think you need: dotenv for loading .env
  Platform ships: --env-file flag (Node ≥ 20.6)

Python stdlib:
  You think you need: requests for simple GET
  Platform ships: urllib.request.urlopen() or http.client

  You think you need: dateutil for ISO date parsing
  Platform ships: datetime.fromisoformat()

  You think you need: path manipulation library
  Platform ships: pathlib.Path

Rust stdlib / ecosystem:
  You think you need: an error-derive crate for error type boilerplate
  Platform ships: std::error::Error + Display impls (Cronus is std-first; no error-derive
                  crate is a dependency)

  You think you need: custom serialization
  Platform ships: serde (already a workspace dependency)

  You think you need: an async runtime
  Platform ships: std::thread with bounded worker threads — the core is synchronous by
                  design and carries no async runtime (l2-core-library §2)

SQL / Database:
  You think you need: manual pagination loop
  Platform ships: LIMIT n OFFSET m

  You think you need: running totals via application code
  Platform ships: SUM() OVER (ORDER BY ...) window function

  You think you need: deduplication via application Set
  Platform ships: SELECT DISTINCT or GROUP BY
```

#### Lookup gate in workflow execution

When a workflow step would install a new dependency or generate more than 20 lines of new code:

```text
[REFERENCE]
Pre-generation check:
  1. Identify the capability being requested (from the step's action field).
  2. Check the platform-native table for the current project's ecosystem.
  3. If a native match is found:
     → Use the native solution. Log: "native: used <feature> instead of custom code."
  4. If no native match, check existing project dependencies.
     → If an installed dependency handles it: use it. Log: "dep: used <pkg>.<method>."
  5. Only if neither check finds a match: generate new code or recommend a new dependency.

The check is advisory — it logs the recommendation but does not block execution.
Workflow authors can mark a step `skip-native-check: true` when a custom implementation
is intentional (e.g., performance-critical hot path with benchmarks to justify it).
```

### 4.8 Nodus language syntax and built-in vocabulary

The nodus crate (`crates/nodus`) is the concrete runtime. A `.nodus` file carries a typed header, a schema-loading runtime block, reactive triggers, hard constraints, soft preferences, I/O declarations, a sequential step body, and optional test and macro blocks. The schema version embedded in the crate is **v0.4.6** (`BUILTIN_SCHEMA_VERSION`). The upstream language specification has since advanced to **v0.7**; this section documents the surface the crate implements today, and §4.9 enumerates the v0.5–v0.7 parity gaps that remain the implementation target.

#### File type sigils

| Sigil | Kind |
| --- | --- |
| `§wf:name vX.Y` | Workflow — the primary executable file type |
| `§schema:name vX.Y` | Vocabulary + rule schema |
| `§config:name vX.Y` | Runtime configuration overlay |

#### Section declarations

| Keyword | Purpose |
| --- | --- |
| `§runtime: { core: … }` | Loads the named schema; `extends:` for overlays; `@needs:` for selective section loading (target — see §4.9d); `agents:` for named model bindings |
| `@ON: cond → action` | Reactive trigger — workflow activates when condition holds |
| `!!NEVER: …` / `!!ALWAYS: …` | Absolute hard constraints; executor halts on violation |
| `!PREF: X OVER Y IF cond` | Soft preference — advisory; never overrides `!!` rules |
| `@in: { field: type }` | Input declaration; `?` suffix marks optional; `= default` provides a fallback |
| `@out: $var` | Output variable |
| `@ctx: [a, b]` | Required context keys |
| `@err: HANDLER` | Error handler invoked for uncaught step errors |
| `@steps:` | Sequential numbered step body |
| `@test: name { … }` | Inline test block with `input:` / `expected:` / `tags:` fields |
| `@macro: name` | Reusable step-sequence macro |

#### Step-line syntax

```text
N. COMMAND(args) +modifier=value ^validator ~flag → $pipeline_target
```

- `+key=value` — named modifier passed to the command
- `^name` — output-validator rule attached to the step result
- `~name` — flag extractor; populates a named sub-variable from the result
- `→ $target` — pipeline target: step output stored as `$target` for downstream steps

#### Control flow

| Construct | Syntax |
| --- | --- |
| Conditional | `?IF cond → action` / `?ELIF cond → action` / `?ELSE → action` |
| Iterator loop | `~FOR $var IN $collection … ~END` |
| Bounded loop | `~UNTIL cond \| MAX:n … ~END` |
| Parallel block | `~PARALLEL … ~JOIN → $target` |

Conditional branches accept `!BREAK` (exit the enclosing loop), `!SKIP` (skip the current iteration), and `!OVERRIDE` (suppress a soft preference for this branch only).

#### Built-in command vocabulary (schema v0.4.6, current crate surface)

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

#### Reserved variables

`$in`, `$out`, `$error`, `$meta`, `$raw`, `$draft`, `$ctx`, `$user`, `$session`, `$log`, `$flags`, `$quality`, `$sentiment`, `$confidence`, `$memory`, `$kb_results`

User-defined step pipeline targets (`→ $name`) must not shadow reserved names.

#### Tone values (`+tone=` modifier)

`warm` · `neutral` · `formal` · `casual` · `urgent` · `empathetic` · `brand`

#### Runtime value types

The executor represents all step results with a closed `Value` enum:

| Variant | Rust type | Notes |
| --- | --- | --- |
| `Null` | — | Unset or absent |
| `Bool` | `bool` | |
| `Int` | `i64` | |
| `Float` | `f64` | |
| `Text` | `String` | |
| `List` | `Vec<Value>` | Ordered, heterogeneous |
| `Map` | `Vec<(String, Value)>` | Preserves insertion order (not `HashMap`) |

`Map` uses a `Vec` of key-value pairs rather than `HashMap` to preserve declaration order across round-trips.

#### Runtime error codes

| Code | Meaning |
| --- | --- |
| `NODUS:RULE_VIOLATION` | An `!!` absolute rule was violated at run time |
| `NODUS:PARSE_ERROR` | Source file failed to parse |
| `NODUS:MAX_REACHED` | A `~UNTIL` loop exhausted its `MAX:n` bound |
| `NODUS:EXECUTION_FAILED` | A step failed during execution |
| `NODUS:UNDEFINED_VAR` | A variable was referenced before assignment |
| `NODUS:ROUTE_NOT_FOUND` | A `ROUTE(wf:name)` target does not exist |
| `NODUS:RULE_CONFLICT` | Two `!!` rules contradict each other |
| `NODUS:SCHEMA_MISMATCH` | Schema version does not match the workflow header |
| `NODUS:NO_SCHEMA` | Workflow executed without a loaded schema |
| `NODUS:NO_TRIGGER` | No `@ON:` trigger matched the current input |
| `NODUS:UNHANDLED_ERROR` | A step error reached no `@err:` handler |

These 11 codes are the current crate surface. The upstream **v0.7** registry defines 24 severity- and category-tagged codes; the 13 not yet implemented — and the non-canonical `NODUS:EXECUTION_FAILED` they supersede — are listed in §4.9e.

#### Library API (public surface)

| Function | Signature | Purpose |
| --- | --- | --- |
| `scaffold` | `(name: &str) -> WorkflowFile` | Return a minimal valid AST with one `GEN` step |
| `validate` | `(source, filename) -> Result<ValidationReport>` | Parse + lint; all diagnostics regardless of severity |
| `run` | `(source, filename, input?) -> Result<RunResult, Diagnostics>` | Validate then execute with the stub provider — a test and development entry point; fast-fail on block errors (WFL-5) |
| `transpile` | `(source, mode: TranspileMode) -> Result<String>` | `Compact` = lossless round-trip · `Human` = one-way prose |
| `test` | `(source, filename) -> Result<TestReport>` | Execute all `@test:` blocks and aggregate pass/fail |
| `run_with_provider` | `(source, filename, input?, provider) -> Result<RunResult, …>` | Like `run` but with a custom `ModelProvider` |

The `ModelProvider` trait is the extension point for real model integration; the built-in `StubProvider` is used for tests and early development. The product's `workflow run` is to go through `run_with_provider` with the host's inference bridge (`l2-model-runtime` §4.1–§4.2), and stub output is to be labelled as stub output in the run's result and in its manifest's execution mode (`l1-nodus-observability` HO-12) — never presented as a model's answer (INV-9). **Partial:** as shipped, `workflow run` calls `run`, so every model step returns stub text, the verb reports the run as `ok` (a `Partial` run included), and the manifest records the default `Real` mode — only the text's own `[STUB …]` prefix tells a reader the answer is not a model's. Wiring the bridge also meets a nodus limit: `run_with_provider` fixes a permit-everything policy, so no current entry point combines a real model with the host's policy gate (`l2-nodus-runtime` §4.5).

#### Executor boot sequence

When `run` or `run_with_provider` fires, the executor performs these steps in order:

1. Load schema (from `§runtime.core`).
2. Internalize `!!` absolute rules (violations halt immediately).
3. Note `!PREF` soft preferences (advisory; yielded to `!!` rules — nothing consumes them yet).
4. Register `@in` defaults and overlay the caller's input.
5. `@ON` trigger matching belongs to the host; the executor performs none.
6. Execute `@steps` sequentially; thread `→` pipeline targets between steps.

The authoritative sequence is `l2-nodus-runtime.md` §4.4, which also records what step 4 does not yet do: it neither checks that a required input was supplied nor reads a field's declared type, and it copies every input key — runtime-owned names included — into the environment. A host that feeds trigger payloads or channel messages into `input` therefore filters them to the declared `@in` fields itself until the runtime does (`l2-nodus-runtime.md` §3, NL-8/NL-9).

### 4.9 Upstream parity gaps (schema v0.4.6 → v0.7)

<!-- [ADDED] v1.3.0 · [MODIFIED] v1.3.1 -->

> **Authoritative ownership:** the language/crate parity gap is owned by the **nodus workspace** — `l1-nodus-language.md` §4.6 (language design) and `l2-nodus-runtime.md` §4.7 (crate implementation). This integration spec mirrors it for `main`-workspace readers (consistent with the §4.8 vocabulary mirror); the host-binding consequences (HITL, lifecycle, scheduler) are this spec's contribution.

The crate implements the **v0.4** vocabulary generation; the upstream language specification has advanced through **v0.5–v0.7**. Each item below is **unimplemented in the current crate** and is the implementation target required to honor WFL-2/WFL-6/WFL-7 against the upstream surface. Most are already declared at the concept level (see `l1-workflow-language.md` §4.1/§4.2 and WFL-6/WFL-7), so closing them realigns the runtime with its own L1 parent.

#### (a) Control constructs

| Construct | Semantics | What to add |
| --- | --- | --- |
| `?SWITCH $v:` with `arm → action` lines and an optional `* → default` | multi-branch dispatch on a scalar; top-to-bottom, first match wins, no fallthrough; no match and no `*` → `NODUS:SWITCH_NO_MATCH` (warn, continue) | lexer token, AST node, executor branch |
| `~MAP $coll: CMD($it) → $out` | single-line collection transform; implicit `$it`; empty collection → `[]`, never errors | lexer keyword, AST node, executor |
| `~RETRY:n` (+`backoff=int`, +`retry_on=error\|null\|both`) | step-level retry up to `n` (mandatory, max 10); after `n` failures → triggers `@err` normally | step modifier on the command call |
| `!HALT` | fatal stop; status `FAILED`; requires `ESCALATE()` in the same step; no auto-resume | control keyword + status |
| `!PAUSE` | suspend; status `PAUSED`; resumes only on explicit human re-trigger; emits `NODUS:PAUSED` | control keyword + `Status::Paused` |

#### (b) Operators & expressions

| Element | Semantics |
| --- | --- |
| `MATCHES` | deterministic PCRE-regex operator in conditions (runtime-evaluated, not via model); `(?i)` prefix for case-insensitive |
| `?.` optional chaining | null-safe path access; short-circuits to `null`; does **not** raise `NODUS:UNDEFINED_VAR` |
| `??` null-coalescing | fallback value when the left side is `null` (e.g. `$user?.tier ?? "free"`) |
| `WHERE` / `FIRST` / `LAST` | inline collection filter/access with implicit `$it`; no match → `[]` (WHERE) or `null` (FIRST/LAST), never errors |
| String interpolation | `$var` / `$obj.field` expand inside string literals before the step runs (runtime-resolved, not by the model); `\$` suppresses; applies in `+msg`, `+hint`, `NOTIFY()`, `ASK()`, `CONFIRM()`, `GEN()` string params |

#### (c) Human-in-the-loop dialog commands

| Command | Semantics |
| --- | --- |
| `ASK(prompt)` | blocking typed question that auto-resumes on answer; `+type=str\|bool\|confirm\|choice\|multi_choice`, `+options`, `+hint`, `+default`, `+validate=<pcre>`, `+timeout` (→ `NODUS:DIALOG_TIMEOUT`) |
| `CONFIRM(content)` | approval decision; `+msg`, `+actions` (returns chosen label), `+default`, `+strict` (reject → `NODUS:DIALOG_REJECTED`) |

Both bind to the HITL subsystem (WFL-7) and realize the "Human interaction" command class of `l1-workflow-language.md` §4.2. Note: the §4.2 step-binding diagram already routes `ask/confirm` to HITL — the commands themselves are simply absent from the crate vocabulary.

#### (d) Selective schema loading — `@needs:`

A `@needs:` directive inside `§runtime:` declares which sections of an `extends:` schema to load — flat (`@needs: [§commands_x, §macros_x]`) or keyed (`@needs: { "x.schema.nodus": [§commands_x] }`). Omit to load the full extension; a schema's `§meta` and `!!` rules always load regardless. Requires a `needs` field on the runtime block plus load-time section filtering — this reduces schema context per execution.

#### (e) Error-code registry — extend 11 → 24

Add the upstream registry's missing codes, each carrying a severity (error/warn/info) and category (parse/runtime/validation/routing/memory/test/control/dialog):

`UNDEFINED_CMD`, `UNDEFINED_MACRO`, `VALIDATION_FAILED`, `ESCALATION_FAILED`, `CONFIDENCE_LOW`, `KB_UNAVAILABLE`, `MEMORY_FAILED`, `TEST_FAILED`, `SWITCH_NO_MATCH`, `PAUSED`, `COUNTER_OVERFLOW`, `GIT_UNAVAILABLE`, `DIALOG_TIMEOUT`, `DIALOG_REJECTED`.

The current catch-all `NODUS:EXECUTION_FAILED` is non-canonical and is superseded by these specific codes.

#### (f) Closed vocabulary registries (model and validate)

The schema declares closed registries the crate currently treats as free-form (so unknown entries pass silently, weakening WFL-2). Load each as data and validate `~flag` / `^validator` / `@in` field types against it:

- **Analysis flags** (`~`): `sentiment`, `intent`, `entities`, `topics`, `lang`, `toxicity`, `urgency`, `formality`, `clarity`, `relevance`, `pii`, `keywords`.
- **Validators** (`^`): `len:n`, `min_len:n`, `no_pii`, `no_toxic`, `lang:code`, `format:type`, `required:keys`, `sentiment:op:n`, `confidence:n`, `no_links`, `brand_voice`, `approved`.
- **Primitive types**: `str`, `int`, `float`, `bool`, `list`, `obj`, `url`, `ts`, `null`, `any` (plus extended object types).

#### (g) Execution semantics

- **Macro execution.** `@macro:` bodies must be parsed structurally and **executed** on `RUN(@macro:name)` — caller params bind as `$in`, the last assigned variable is the implicit return. The current crate parses the body and audits enter/exit but does not run it.
- **`Status` taxonomy.** Add `Paused` (for `!PAUSE`) to the existing `Ok / Partial / Failed / Aborted` set.
- **`@ON` priority.** Support optional `@ON(priority=N):` — lower `N` = higher priority; default is declaration order.

#### (h) Minor lexer parity

Single-character `;` inline comments (the crate recognizes only `;;`) and the `\$` interpolation escape are small lexer items to align with the upstream grammar.

## 5. Drawbacks & Alternatives

- **Porting effort:** re-implementing lexer/parser/validator/executor/transpiler in Rust is real work; mitigated by an existing formal grammar and lint catalog to port from.
- **Schema drift vs runtime:** the runtime must track the schema version it supports. The crate is at **v0.4.6** while the upstream spec is **v0.7**; §4.9 is the authoritative gap list, and a schema-version compatibility check (header `compatible:` range vs crate `BUILTIN_SCHEMA_VERSION`) gates execution, surfacing `NODUS:SCHEMA_MISMATCH` on divergence.
- **Alternative — embed an external interpreter:** rejected; it breaks the embeddable/mobile constraint (no heavy runtime on device).
- **Alternative — standalone crate in its own repository:** deferred; vendored in-tree for now since no other consumer needs it. The self-contained crate boundary keeps later extraction cheap if that changes.

## Document History

| Version | Date | Change |
| --- | --- | --- |
| 1.3.2 | 2026-09-24 | Consistency pass (2026-09-24): The platform-capability table told generated code that `tokio` and `thiserror` were already Cronus dependencies; neither is, and the core is synchronous by design — corrected. Step-file frontmatter was the authoritative state though the executing agent writes it — the executor's durable journal is authoritative and an agent-written completion is a claim. The product's `workflow run` uses the inference bridge, and stub output is labelled as such (INV-9, HO-12). Yesterday's normative sentence that `workflow run` uses the inference bridge described intent, not the shipped verb: it calls the stub `run`, reports `Partial` as `ok`, and records a `Real` manifest — now marked Partial, with the nodus limit that no entry point combines a real model with the host's policy gate. The boot-sequence list now matches the executor (no trigger matching, no advisory preference context) and points to `l2-nodus-runtime` §4.4 for what input registration does not yet check. A TBD asking to extract the reference corpus into shared fixtures is resolved — the corpus is `crates/nodus/tests/fixtures/`. |
| 1.3.1 | 2026-06-25 | §4.9: marked the **nodus workspace** (`l1-nodus-language.md` §4.6, `l2-nodus-runtime.md` §4.7) as authoritative owner of the parity gap; this spec retains the integration/host-binding view. |
| 1.3.0 | 2026-06-25 | Recorded the upstream schema **v0.4.6 → v0.7** parity gap as the implementation target — new §4.9 enumerating control constructs (`?SWITCH`/`~MAP`/`~RETRY`/`!HALT`/`!PAUSE`), operators/expressions (`MATCHES`, `?.`, `??`, `WHERE`/`FIRST`/`LAST`, string interpolation), HITL dialog commands (`ASK`/`CONFIRM`), `@needs:` selective schema loading, the 24-code error registry, closed flag/validator/type registries, macro execution, `Status::Paused`, and `@ON` priority. Corrected the embedded schema version (v0.4.5 → v0.4.6). |

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[LANG]` | `.design/main/specifications/l1-workflow-language.md` | Invariants this runtime implements |
| `[CORE]` | `.design/main/specifications/l2-core-library.md` | The core that hosts the runtime |
| `[CLI]` | `.design/main/specifications/l2-cli.md` | Command grammar standard |
