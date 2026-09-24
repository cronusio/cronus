# Core Library (Foundation)

**Version:** 1.2.2
**Status:** Stable
**Layer:** implementation
**Implements:** l1-architecture.md

## Overview

Architectural layer 1: the **embeddable core library** that is the foundation of Cronus. It contains all domain logic and exposes a stable programmatic contract consumed by every frontend (CLI, TUI, application) and, optionally, by third-party host programs. It has no presentation dependencies.

## Related Specifications

- [l1-architecture.md](l1-architecture.md) - Concept this layer implements.
- [l2-technology-stack.md](l2-technology-stack.md) - Technology choices for the core.
- [l2-cli.md](l2-cli.md) - A consumer of this core.
- [l2-tui.md](l2-tui.md) - A consumer of this core.
- [l2-app-ui.md](l2-app-ui.md) - A consumer of this core.

## 1. Motivation

A reusable foundation lets every surface share one behavior and lets the engine be embedded elsewhere. The core owns agent orchestration, memory, scheduling, model routing, Kanban state, and persistence so that no frontend has to.

## 2. Constraints & Assumptions

- Written in **Rust** as a library crate; exposes a **C-ABI/FFI** surface for embedding (Tauri backend, mobile static lib, external hosts).
- No UI, terminal, or windowing dependencies (INV-1).
- Synchronous by design, with no async runtime in the core: concurrency is bounded worker threads owned by the component that needs them, and streaming is delivered through pull or callback surfaces (`l2-model-runtime` §2). Long-running work never blocks a host's UI thread — it runs on those threads.
- Local-first: the default datastore is an embedded file (SQLite + sqlite-vec); remote/sync is optional.

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| INV-1 Embeddable core | Rust library crate with C-ABI; compiles for desktop and iOS/Android targets; linkable into external programs. |
| INV-2 Logic in core only | All domain modules (orchestration, memory, router, scheduler, board, persistence) live here; frontends hold none. |
| INV-3 Command parity | The core contract is the single definition of each capability; frontends bind to it, guaranteeing parity. |
| INV-4 Hub-and-spoke autonomy | The autonomous loop is a core service runnable headless on a hub; on spokes the same crate runs in foreground/sync-only mode. |
| INV-5 Durable, restartable state | Persistence module writes durable state (SQLite file); engine reconstructs sessions/memory/tasks on restart. |
| INV-6 Graceful capability scaling | The contract is capability-flagged; a host enables the subset it can support. |
| INV-7 Security of client data | Secret handling and telemetry separation are enforced in the core, not delegated to frontends. |
| INV-8 Single-deployable modular monolith | The core library IS the modular monolith: all domain modules link into one deployable unit and communicate by in-process function calls, never network calls; no orchestration platform is required. Its only sanctioned outward boundary is the frontend↔core contract, and its only sanctioned in-process security boundary is the sandbox for untrusted code. `l2-crate-topology` partitions this library into compiler-enforced crates without introducing any process boundary. |
| INV-9 Shipped-surface honesty | INV-9 is enforced on the frontends; the core's bearing is to be the honest referent they bind against. Each capability is exposed through the core's stable contract (INV-2), and a capability the core has not shipped is simply absent from that contract — so a frontend has nothing to bind an unshipped verb to and is never forced into a "not implemented" stub. The core never publishes a half-capability. |
| INV-10 Representation isolation at the inward seam | The core owns the seam INV-10 governs: its domain tier depends only on contract types, while adapter representations (a persistence row, a wire DTO, a keychain record) stay private to the adapter and are mapped at the trait seam. Realized concretely in `l2-crate-topology` (`cronus-domain` → `cronus-contract`; `cronus-store-local` maps its `rusqlite` row to/from `MemoryEntry`). The core compiles and unit-tests against in-memory providers precisely because no adapter shape reaches the domain. |

## 4. Detailed Design

### 4.1 Core modules (domain)

| Module | Responsibility |
| --- | --- |
| Orchestrator | Hierarchical agent coordination (manager → department agents); task delegation; adaptive hiring of roles on demand. |
| Memory | Tiered memory (working / recall / archival), scope-aware decay + prune, knowledge graph of entities/relations, hybrid retrieval. |
| Model router | Selects local vs cloud models by cost/tokens/capability; fallback cascade; semantic cache. |
| Scheduler | Cron + heartbeat wake queue (coalesced) driving autonomous work without spamming the board. |
| Board | Kanban state machine: `triage → todo → ready → running → blocked → done`, then automatic archival to the archive store (+ custom columns anchored to these states). |
| Persistence | Durable local state (SQLite + sqlite-vec); optional remote sync (libSQL/PostgreSQL). |
| Model runtime | Inference behind the contract's inference trait (`l2-model-runtime`): loopback REST providers on a desktop or server host, llama.cpp through FFI on mobile, and egress-gated cloud clients. |

### 4.2 Contract surface (illustrative)

The core exposes capability operations consumed by frontends; equivalent to the command set in [l1-architecture.md](l1-architecture.md) §4.4. `[REFERENCE]` illustrative shape, not final API:

```text
[REFERENCE]
core.init(config) -> Session
core.idea(text) -> CapturedIntent
core.plan(scope) -> Plan
core.task() -> Tasks
core.run(scope) -> RunHandle
core.status() -> StatusSnapshot
core.memory(query) -> MemoryResult
core.goal(condition) -> GoalHandle    // autonomous loop, judged termination
core.compact() -> void
core.analyze(scope) -> Report
```

### 4.3 Embedding model

```mermaid
graph TD
    HOST[Host program: Tauri / CLI / TUI / 3rd-party] --> FFI[C-ABI boundary]
    FFI --> CORE[Rust core engine]
    CORE --> DB[(SQLite + sqlite-vec)]
    CORE --> AI[Model runtime: loopback REST, mobile FFI, cloud]
```

### 4.4 Autonomy safety rails

- Per-agent budget checks before each wake; overspend auto-pauses (circuit breaker).
- Goal completion gated by an independent judge before the loop may stop.
- Checkpointing + context reconstruction for long-horizon runs.

### 4.5 Manager Pattern & Resource Guards

Stateful subsystems (model inference, session context, history, audio) are organized as **managers**: independent structs wrapped in `Arc<T>`, registered in the application state container at startup, and accessed by any handler via a shared reference.

#### `Arc<T>` manager pattern

```text
[REFERENCE]
// At startup:
let model_manager = Arc::new(ModelManager::new(...));
app.manage(model_manager);

// In any handler:
let mm = app.state::<Arc<ModelManager>>();
mm.load_model(model_id)?;       // synchronous; the manager owns any worker threads
```

Managers are initialized once in a defined order. Later managers may hold `Arc` references to earlier ones. All domain logic lives inside a manager; no domain logic lives in handler glue code.

<!-- [ADDED] v1.2.0 -->
The "defined order" is a dependency DAG, not a chain: managers with no dependency edge between them (datastore open, extension scan, provider probing, config load) initialize concurrently in dependency waves — a manager starts as soon as every manager it holds an `Arc` to is ready. Cold-start latency approaches the longest dependency path instead of the sum of all initializations; per-manager failure semantics are unchanged (a failed manager fails exactly the wave members that depend on it).

#### RAII loading guard

When a manager transitions to a "loading" state, wrap the transition in a guard struct that resets the state on drop — ensuring atomic cleanup even if the work panics or returns early:

```text
[REFERENCE]
struct LoadingGuard<'a>(&'a Manager);

impl Drop for LoadingGuard<'_> {
    fn drop(&mut self) {
        self.0.is_loading.store(false, Ordering::SeqCst);
    }
}

fn load_model(&self, id: &str) -> Result<()> {
    self.is_loading.store(true, Ordering::SeqCst);
    let _guard = LoadingGuard(self);    // cleared on ANY exit path
    // ... loading work ...
    Ok(())
}
```

#### Idle watcher thread

When a manager holds a heavy resource (e.g., a model occupying VRAM), spawn a background thread that polls `last_activity` and releases the resource after a configurable inactivity timeout:

```text
[REFERENCE]
idle_watcher(manager_ref, poll_interval = 10 s):
  loop:
    sleep(poll_interval)
    if resource_loaded AND now - last_activity > inactivity_timeout:
      manager_ref.unload_resource()

InactivityTimeout enum: Never | Immediately | Sec15 | Min2 | Min5 | Min10 | Min15 | Hour1

last_activity is updated by every successful use of the resource.
```

<!-- [ADDED] v1.2.0 -->
When several managers hold idle-released resources, they share **one** watcher — a single timer task multiplexing per-manager deadlines — rather than one polling thread per manager; a fired deadline dispatches that manager's `unload_resource()` and re-arms on next use.

#### Backend enum for polymorphic dispatch

Support multiple interchangeable implementations (e.g., different inference engines) using an enum rather than `dyn Trait`, to avoid heap allocation and retain ownership clarity:

```text
[REFERENCE]
enum LoadedBackend {
    EngineA(EngineAState),
    EngineB(EngineBState),
    // New backend = new variant; callers use match, not dyn dispatch.
}

impl LoadedBackend {
    fn run(&mut self, input: &Input) -> Result<Output> {
        match self {
            Self::EngineA(e) => e.run(input),
            Self::EngineB(e) => e.run(input),
        }
    }
}
```

### 4.6 Typed prefixed ID system

Every entity in the core that requires a stable, sortable identifier uses a **typed prefixed ID**:
a short human-readable type prefix followed by a k-sortable body. The prefix makes the
entity type visible in logs and databases without a schema lookup; the k-sortable body
makes IDs usable as monotonic cursors.

#### Prefix table

| Entity | Prefix | Example |
| --- | --- | --- |
| Session | `ses` | `ses0r7k3mQp2…` |
| Message | `msg` | `msg0r7k3mQp3…` |
| Message part | `prt` | `prt0r7k3mQp4…` |
| Permission | `per` | `per0r7k3mQp5…` |
| Question | `que` | `que0r7k3mQp6…` |
| Event | `evt` | `evt0r7k3mQp7…` |
| Job | `job` | `job0r7k3mQp8…` |
| Worktree | `wrk` | `wrk0r7k3mQp9…` |
| Tool call | `tool` | `tool0r7k3m…` |
| PTY | `pty` | `pty0r7k3mQpA…` |
| Access grant | `grn` | `grn0r7k3mQpB…` |

#### ID structure and generation

```text
[REFERENCE]
ID format: <prefix> <body>   (no separator; prefix is 2–4 lowercase chars, body is 26 chars)

Body encoding:
  timestamp_ms × 0x1000 + monotonic_counter   → BigInt
  BigInt is encoded as base-62 (digits 0–9 A–Z a–z), zero-padded to 26 chars

Ascending (creation-order) ID:
  BigInt = timestamp × 0x1000 + counter   → encodes lexicographically as creation time order

Descending (reverse-order) ID:
  BigInt = MAX_VALUE - (timestamp × 0x1000 + counter)
  → encodes lexicographically as newest-first; useful for "latest items first" queries

Monotonic guarantee:
  lastTimestamp + counter pair ensures no two IDs are identical even within the same
  millisecond (counter resets each time the millisecond advances). The timestamp used is
  max(now, lastTimestamp), so a clock stepped backwards never produces an earlier or a
  repeated ID; when the counter would pass 0xFFF within one millisecond, generation
  continues in the next millisecond rather than letting the counter spill into the
  timestamp bits.

Prefix rule:
  no prefix may be a prefix of another (the body follows with no separator), so a
  prefix always parses unambiguously.

Factory functions:
  ascending(entity_type, given?)  -> ID   // validate prefix if given; else generate new
  descending(entity_type, given?) -> ID   // same; different sort encoding

Validation: if a given ID does not start with the expected prefix, return an error.
  This catches accidental entity-type mixups at the call site rather than the database.
```

#### Branded type usage

IDs are **branded** at the type level so that a `SessionId` cannot be passed where a
`MessageId` is expected:

```rust
// [REFERENCE]
type SessionId  = Branded<String, "SessionId">;
type MessageId  = Branded<String, "MessageId">;

impl SessionId {
    pub fn ascending(given: Option<&str>) -> Result<Self> { /* prefix: "ses" */ }
}
```

Cross-entity operations (e.g., "which messages belong to session X") use the unbranded
string form only at persistence boundaries; all in-memory code uses the branded type.

### 4.7 Thinking level taxonomy

Model calls that support extended reasoning take a `ThinkingLevel` that controls how much
internal computation the model is allowed before producing visible output. The level is
an enum with six ordered steps from disabled to maximum budget:

```text
[REFERENCE]
ThinkingLevel: "off" | "minimal" | "low" | "medium" | "high" | "xhigh"

Mapping to provider-side budget (illustrative — actual token counts may vary by model):
  "off"     — reasoning disabled; no thinking tokens allocated
  "minimal" — very short internal scratchpad (lowest cost, fastest)
  "low"     — brief reasoning pass
  "medium"  — balanced reasoning for most tasks
  "high"    — extended reasoning for complex multi-step problems
  "xhigh"   — maximum reasoning budget; slowest and most expensive
```

The current thinking level is stored per-session as a `ThinkingLevelChangeEntry` in the
session JSONL file (see `l2-agent-session.md §4.15`). On reload, the level is replayed
from the entry sequence so the session continues with the same reasoning mode. Extensions
may subscribe to `thinking_level_select` events to react when the user or another
extension changes the level mid-session.

The thinking level is also a `prepareNextTurn` output (see `l2-agent-session.md §4.14`):
a hook may raise or lower the level for a specific turn based on the complexity of the
task at hand without permanently changing the session-level default. Any level set by an
extension or hook stays inside the effort envelope the user set (`l1-routing` RTG-11) —
reasoning is billed spend, and only the user widens how much of it a session may buy.

#### CustomAgentMessages extensibility

The message taxonomy accepted by the agent loop is extensible without modifying the core
type. An extension declares its custom message variants when it registers — a namespaced
`customType` plus a schema for its payload — and the core validates each custom message
against the declared schema before accepting it:

```text
[REFERENCE]
// Extension registration (declared in its manifest, validated by the core):
custom_messages:
  - customType: "my_extension/status_update"
    schema: { level: string, text: string }
// "my_extension/status_update" is now a valid custom message customType.
// The agent loop passes it through convertToLlm() for provider formatting,
// labeled with the extension that emitted it.
```

The taxonomy lives in the core (INV-2): a frontend package may mirror the declared types
for its own type checking, but it defines none. A payload that fails its declared schema
is rejected with a typed error rather than forwarded; `customType` values the core does
not own are treated as opaque values and forwarded without transformation.

## 5. Drawbacks & Alternatives

- **C-ABI surface is verbose:** mitigated by generating bindings; the embeddability benefit (INV-1) justifies it.
- **Alternative — TypeScript/Node core:** richer AI SDK ecosystem but high memory footprint and poor mobile embedding; rejected for the foundation. <!-- TBD: confirm whether a thin Go alternative core is worth a spike given repo history -->

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[ARCH]` | `.design/main/specifications/l1-architecture.md` | Invariants the core must satisfy |
| `[STACK]` | `.design/main/specifications/l2-technology-stack.md` | Technology choices for the core |

## Document History

| Version | Date | Notes |
| --- | --- | --- |
| 1.2.0 | 2026-07-04 | Manager initialization as dependency waves (§4.5): independent managers initialize concurrently, cold start bounded by the longest dependency path; idle watchers consolidated into one multiplexed timer task. History table added with this entry. |
| 1.2.1 | 2026-07-29 | Completeness fix: extended the §3 Invariant-Compliance table to INV-8/INV-9/INV-10, which entered `l1-architecture` after this table (INV-1…INV-7) was written, restoring the L1 "all invariants addressed" gate. INV-8 — the core library IS the modular monolith (in-process modules, one deployable), partitioned by `l2-crate-topology` without a process boundary. INV-9 — the core is the honest referent frontends bind to (no half-capabilities). INV-10 — the core owns the inward domain↔adapter seam (domain depends on contract types; adapter representations mapped at the seam). No new design or requirement; stays Stable. |
| 1.2.2 | 2026-09-23 | Consistency pass (2026-09-23): §2 specified "Async via Tokio" while the tree has no async runtime and `l2-model-runtime` builds its transport synchronous by design — the core is now specified synchronous with bounded worker threads (the manager example drops `.await`). The "AI gateway: llama.cpp via FFI" row contradicted the model runtime (loopback REST providers on desktop/server, FFI only on mobile) — replaced by the model-runtime row; the board row no longer lists archive as a state (KAN-1). Typed IDs: a clock stepped backwards or more than 4 096 IDs in a millisecond could repeat or reorder IDs — the timestamp is monotonic and the counter never spills; no prefix may prefix another; `grn` added. Thinking-level changes by hooks stay inside the user's effort envelope (RTG-11). The TypeScript declaration-merging taxonomy put the agent loop's message types in a TS package (INV-2) — custom message types are declared in the extension manifest and schema-validated by the core. |
