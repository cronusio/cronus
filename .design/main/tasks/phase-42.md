---
phase: 42
name: "Diagnostic Log"
status: Todo
subsystem: "crates/core crates/cli crates/store-local crates/activation-os apps/desktop/tauri"
requires: [13, 27, 38]
provides: []
key_files:
  created: []
  modified: []
patterns_established: []
duration_minutes: ~
---

# Stage 42 Tasks — Diagnostic Log

**Phase:** 42
**Status:** Todo
**Strategic Goal:** Realize `l2-diagnostic-log` 1.0.0 — the first log event the product ever writes. One `tracing` event model; a sink installed as the first statement of every binary; rotating JSON-lines files per process kind under `<logs>`; a session header and a panic hook; redaction, payload policy and folding at every projection; first events at the state changes worth knowing a day later; and the `log show / tail / path / status` inspect verbs with a budgeted feed for agents. Today no crate emits a single event, so every later phase — the execution sandbox first — gets somewhere to write its partial failures.

## Atomic Checklist

Track A — Event model and sink (`crates/core/src/diagnostics/`)

- [ ] [T-42A01] Size-rotating JSON-lines writer and the `tracing` dependencies
- [ ] [T-42A02] `diagnostics::install`: line format, filter, non-blocking writer, session header, panic hook
- [ ] [T-42A03] Redaction, payload policy and folding at every projection

Track B — Install and emit

- [ ] [T-42B01] Install the sink first in every product binary
- [ ] [T-42B02] First events at the state-change sites of the facade and adapter tiers
- [ ] [T-42B03] Domain-tier emission

Track C — The `log` verbs

- [ ] [T-42C01] `log show / tail / path / status` as `Inspect` installation verbs
- [ ] [T-42C02] The agent feed: `log tail --for-agent --budget`

Track T — Validation

- [ ] [T-42T01] Invariant sweep and the usage-simulation scenario
- [ ] [T-42T02] Full-cycle validation

## Detailed Tracking

### [T-42A01] Size-rotating JSON-lines writer and the `tracing` dependencies

- **Spec:** l2-diagnostic-log.md §3 DL-5, DL-8; §2 (dependency added); l2-technology-stack.md (logging row)
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo test -p cronus-core diagnostics::rotate` passes unit tests that (1) write past `max_size` and find `<kind>.jsonl` plus `<kind>.1.jsonl`; (2) exceed `max_files` rotations and find the oldest file gone; (3) point the writer at a directory that cannot be created, write, and get `Ok` back with no panic; (4) make the directory creatable and see the next write land in the file. `cargo clippy -p cronus-core --all-targets -- -D warnings` clean.
- **Handoff:** T-42A02 wraps this writer.
- **Notes:** Add `tracing`, `tracing-subscriber` (features `json`, `env-filter`, `registry`, `fmt`) and `tracing-appender` to `[workspace.dependencies]`; `cronus-core` takes all three. `tracing-appender` rotates by time, not size, so DL-5's 10 MiB / 5 files needs a writer of our own: a std-only `Write` implementation in `crates/core/src/diagnostics/rotate.rs` that `tracing_appender::non_blocking` wraps in T-42A02. Rotation renames `<kind>.jsonl` → `<kind>.1.jsonl` and shifts older ones up; `max_size` and `max_files` are parameters (settings `log.max_size`, `log.max_files` arrive with T-42A02). A failed open or rotation is remembered and retried on the next write; the writer reports success to its caller and counts the loss (DL-8: logging never fails the work it describes). No `unwrap()` on this path.

### [T-42A02] `diagnostics::install`: line format, filter, non-blocking writer, session header, panic hook

- **Spec:** l2-diagnostic-log.md §3 DL-2, DL-3, DL-4, DL-8, LL-1, LL-2, LL-3, LL-6, LL-9; §4.1, §4.2, §4.3
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo test -p cronus-core diagnostics` with the subscriber pointed at an in-memory writer: (1) one `info!` from a `cronus_core` target produces exactly one line whose keys include `v` (= 1), `ts`, `level`, `target`, `kind`, `session`, `msg` and the event's own fields; (2) with no override, a `debug!` from `cronus_core` and an `info!` from a non-`cronus` target are both absent; (3) with `CRONUS_LOG=cronus=debug`, the `debug!` appears and the non-`cronus` `info!` is still absent (the dependency floor is a separate value); (4) the first line after `install` is `session.start` carrying OS, architecture, product version, process kind, working directory, arguments, state root and start time, with an argument passed as `--token x` rendered `<redacted>`; (5) a panic in a spawned thread leaves a `panic` event with message and location before the default hook runs; (6) saturating a 1-slot queue then draining it yields one `log.dropped` event with a `count` field.
- **Handoff:** T-42A03 (projections), T-42B01 (callers).
- **Notes:** `pub fn install(kind: ProcessKind, logs_root: &Path) -> DiagnosticsGuard` in `crates/core/src/diagnostics/mod.rs`; `ProcessKind` is `Cli | Tui | Engine | Desktop` (`Engine` has no process of its own until a serving entry exists — declare it, do not invent a caller). The guard holds the non-blocking worker and flushes on drop. Filter: `cronus*=info`, everything else `warn`; `CRONUS_LOG` (same `EnvFilter` syntax) overrides; a `reload` handle lets the `log.level` setting apply once configuration resolves, because DL-2 puts the sink ahead of configuration. Messages are stable text; values go in fields (LL-2), never interpolated. The console projection is a second layer: one line per event (level, target, message, key fields) to standard error at `warn` and above for one-shot commands; the TUI kind installs **no** console layer, because it owns the terminal and a stray line would corrupt the screen. Session id: a random per-process id (reuse whatever id source the endpoint already uses rather than adding a crate). Secret-marked arguments for the header: a flag whose name matches the LL-7 denylist, plus any argument of an invocable whose descriptor has `journal_raw_input == false`.

### [T-42A03] Redaction, payload policy and folding at every projection

- **Spec:** l2-diagnostic-log.md §3 LL-4, LL-5, LL-7; l2-invocable-registry.md §4.12, §4.13
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo test -p cronus-core diagnostics::project` unit tests: a field named `api_token`, `client_secret`, `password` or `authorization` renders `<redacted>` in the file and console layers; a field value equal to a secret in the supplied list renders masked through `cronus_domain::redact::redact`; under the default `log.payload = descriptor` a 4 KiB body field renders as its size, and under `excerpt` as a prefix carrying `truncated_from = 4096`; three identical consecutive events render as one line with `repeat = 3`, while a different event between them breaks the fold.
- **Handoff:** T-42C01 reads lines this layer produced; T-42C02 reuses the folding.
- **Notes:** Redaction happens where a projection is rendered, not where an event is emitted, so a library crate cannot forget it. The secret list comes from the same source the dispatch path uses — which `l2-invocable-registry` §4.12 records as **empty everywhere today**; say so in the module documentation and do not count the live-list half of LL-7 as realized. The protections that work today are the field-name denylist and the `journal_raw_input` declaration. Folding compares level, target, message and fields, and emits the folded line when a different event arrives or on flush.

### [T-42B01] Install the sink first in every product binary

- **Spec:** l2-diagnostic-log.md §3 DL-2, DL-3; §4.1; l2-filesystem-layout.md §4.1 (`<logs>`)
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** New test in `crates/cli/tests/cli_smoke.rs`: with an isolated `CRONUS_PORTABLE_DIR`, `cronus status --format json` exits 0, its stdout is byte-identical to the same command's output before this task (compare against a captured fixture) and `<portable>/logs/cli.jsonl` exists with `session.start` and `"kind":"cli"` on its first line. `cargo check --manifest-path apps/desktop/tauri/Cargo.toml` and `cargo clippy --manifest-path apps/desktop/tauri/Cargo.toml -- -D warnings` pass (the desktop host is its own workspace, not a member of the root one) — the desktop host's tests cannot run on this machine (missing WebView loader, recorded since Phase 26), so a build is the check. `cargo test -p cronus-cli` green.
- **Handoff:** T-42B02, T-42C01.
- **Notes:** First statement of `main` in `crates/cli/src/main.rs`, before the installation declaration is built: kind `Tui` when `launch_mode` is `DefaultComposition` or the verb is `tui`, else `Cli`. `logs_root` is `cronus_domain::paths::Paths::resolve(Root::Logs)` (portable mode already maps it under the portable directory). The desktop host installs kind `Desktop` in `apps/desktop/tauri/src/main.rs` before `run()`. `cronus-sim` is a development tool and is not a product binary — leave it alone. **Seam with Phase 33:** its task T-33E04 later puts engine self-hardening at the top of the same `main`; the sink stays first so hardening's partial results have somewhere to go, and `CRONUS_LOG` is read before any environment scrub. Record this ordering in a comment at the call site, in plain language. Tests elsewhere set `CRONUS_PORTABLE_DIR`; a test that does not would write into the developer's real logs directory — grep the test suites for `Command::new(env!("CARGO_BIN_EXE_cronus"))` without that variable and fix any found.

### [T-42B02] First events at the state-change sites of the facade and adapter tiers

- **Spec:** l2-diagnostic-log.md §3 DL-1, DL-2, DL-6, LL-2, LL-6; §4.3
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `git grep -n tracing_subscriber -- crates apps` lists only `crates/core/src/diagnostics/` (library crates emit, never install). New test in `crates/cli/tests/`: in an isolated world, `cronus backup create` leaves a `backup created` event with `backup.id`, `bytes` and `duration_ms` fields in `cli.jsonl`. A unit test in `crates/activation-os` asserts a failed helper call produces a `warn` event whose fields carry the helper's name and its captured standard error. `cargo test -p cronus-core -p cronus-store-local -p cronus-activation-os` green.
- **Handoff:** T-42T01 sweeps these sites.
- **Notes:** Emit with `tracing` macros only, from `cronus-core`, `cronus-store-local`, `cronus-model-local`, `cronus-activation-os` and `cronus-tui`; each takes the `tracing` facade alone. Sites at `info`: store schema migrated, engine seat taken and released (`crates/core/src/endpoint`), backup created and restored (the facade's carry-out, not `cronus_domain::backup`, which is held by T-42B03), extension activated and deactivated, workspace created. `warn`: a fallback taken or an item skipped (where such a branch exists today — find them, do not invent them). `error`: a failed chat turn in the session runner, with the session and model as fields. DL-2's forwarded subprocess output: the only processes the product starts today are the activation adapter's OS helpers, whose captured standard error becomes a field of the failure event under the helper's name; no model-server process is started by the product, so there is nothing else to forward. A log line may *mention* a receipt or journal identifier (LL-6) and is never the record of one (DL-1).

### [T-42B03] Domain-tier emission

- **Spec:** l2-diagnostic-log.md §2 (every library crate emits); l2-crate-topology.md §4.3
- **Status:** Blocked [!]
- **Assignment:** Agent
- **Verify:** `node scripts/check-domain-boundary.mjs` exits 0 with `tracing` on the allowlist, and at least the scheduler, the backup planner and the kanban state machine emit one `debug` event each, checked by unit tests capturing to an in-memory subscriber.
- **Handoff:** Unblocked by a `/magic.spec` amendment to `l2-crate-topology` §4.3, then set to `Todo`.
- **Notes:** Blocked by a specification gap: `l2-crate-topology` §4.3 admits exactly `blake3`, `chrono` and `cron` to `cronus-domain`, so `tracing` (an emit-only facade with no I/O) is not admissible as written, while `l2-diagnostic-log` §2 says every library crate emits. The same guard already fails on `master`: `cronus-domain` has depended on `serde` and `serde_json` since 2026-09-10 and `scripts/check-domain-boundary.mjs` reports both as forbidden, so the CI dependency gate has been red since then. One amendment should settle all three entries. Until then the domain's state changes are logged where the facade calls them (T-42B02).

### [T-42C01] `log show / tail / path / status` as `Inspect` installation verbs

- **Spec:** l2-diagnostic-log.md §3 DL-6, DL-7, LL-9; §4.5; l2-invocable-registry.md §4.8.1 (installation declaration); l2-tui.md §4.7
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo test -p cronus-core installation` — the declaration grows by four `log.*` verbs, each `LiveEffect::Inspect`, each registered with a handler; smoke tests in `crates/cli/tests/cli_smoke.rs` under an isolated `CRONUS_PORTABLE_DIR`: `cronus log path` prints the logs root; `cronus log show --level warn --format json` returns only `warn` and `error` lines from a seeded file spanning two rotations and two schema versions (`v` 1 and a synthetic `v` 2 line, which is shown as raw text rather than dropped); `cronus log status --format json` names each file family, its size, the dropped count and `native_crash_capture: "unavailable"`; all four answer with no composition (they run in the installation half of `main`). A TUI test asserts `/log status` resolves through the projected catalog. `cronus backup create` without `--include-logs` still excludes `<logs>` (existing test stays green, DL-6).
- **Handoff:** T-42C02 adds the agent form of `tail`.
- **Notes:** Declare the group in `crates/core/src/installation.rs` beside `status` and `doctor`; handlers in `installation/carry_out` read `<logs>/<kind>*.jsonl` oldest rotation first. Filters: `--level` (that level and above), `--grep` (substring on message and field values, after redaction), `--since` (a duration such as `10m`, `2h`, `1d`), `--kind`. `tail` follows the file through the streamed-result path the registry already carries; it ends when the caller stops reading. Because installation verbs are projected into the terminal UI as a set, `/log …` should appear there without TUI code — confirm rather than assume. Never echo a secret: the projection layer of T-42A03 has already masked lines on disk, and `--grep` runs over masked text.

### [T-42C02] The agent feed: `log tail --for-agent --budget`

- **Spec:** l2-diagnostic-log.md §3 LL-3, LL-8; §4.4
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** Unit tests in `crates/core/src/installation/carry_out` over a fixture log: with `--budget 200` the output's estimated size is at most 200 tokens and its last line states how many events were left out (the number matches the fixture); `warn` and `error` events are present, payload fields are absent, identical consecutive events appear once with `repeat`; with `--card c-7` the `info` events for card `c-7` are included and those for another card are not; events run oldest first, newest last.
- **Handoff:** T-42T01.
- **Notes:** The token estimate is stated in the code and the help text (characters divided by four is enough; the budget is a ceiling, not a promise of fullness). Cut from the oldest end so the newest events survive. This is the only form in which an agent reads logs (§4.4): no other verb gains an agent mode.

### [T-42T01] Invariant sweep and the usage-simulation scenario

- **Goal:** One test per DL / LL row this phase realizes, and an honest list of the rows it does not, so the sweep cannot claim more than was built; plus the scenario RULES C30 requires for a new user-observable verb group.
- **Method:** `crates/core/tests/diagnostic_log_invariants.rs`: tests for DL-1 (no receipt, journal or telemetry writer reads `<logs>`; grep-style assertion over the source tree), DL-2 (header first, panic event), DL-3, DL-4, DL-5, DL-6 (backup default excludes logs), DL-7 (four verbs, all `Inspect`), DL-8 (drop counting), LL-1…LL-9. A `NOT_REALIZED` constant lists, with reasons, the native-fault handler (DL-2, per OS), domain-tier emission (T-42B03) and the live secret list (§4.12's empty list); a test asserts `log status` reports native crash capture as unavailable rather than implying coverage. New scenario `simulations/short/why-did-that-just-fail.md`: a person whose last command failed finds out why with nothing but the product; obligations are falsifiable (the reason is named on screen; no secret appears; the person learns where the files are) and the route is not prescribed.
- **Verify:** `cargo test -p cronus-core --test diagnostic_log_invariants` green; `cronus-sim world new simulations/short/why-did-that-just-fail.md` accepts the scenario (the parse-time validator runs there and refuses an unfalsifiable obligation) and one recorded run of it is attached to the task's `Changes` as a transcript path.
- **Status:** Todo

### [T-42T02] Full-cycle validation

- **Goal:** Verify Phase 42 as a whole against `l2-diagnostic-log` 1.0.0.
- **Method:** In PowerShell: `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace` green across 2 consecutive runs (`-j 2` if rustc crashes on this host); `rg -n "unwrap\(\)|panic!\(|\.expect\(" crates/core/src/diagnostics` finds nothing on a production path; `node scripts/check-domain-boundary.mjs` — record its result as-is (expected to fail on `serde`/`serde_json` until T-42B03's amendment; this phase adds no domain dependency); by-hand run of `cronus log status`, `cronus log show --since 1h` and `cronus log path` against the freshly built binary with an isolated portable directory; containment sweep of the added lines for SDD-layer references.
- **Verify:** All commands above exit 0 except the boundary guard, whose output matches the recorded baseline exactly (no new offender).
- **Status:** Todo

## Planning Audit and Instruction Review

**Planner audit (optimism, hidden dependencies, cascade).** *Optimism:* ten tasks for a spec with no code behind it; T-42A02 carries the most moving parts (filter reload, non-blocking worker, header, panic hook, console layer) and may split `.1` (format and filter) / `.2` (header, panic hook, drop event). `tracing-appender` does not rotate by size, which DL-5 requires — found while decomposing, so T-42A01 writes the rotating writer instead of assuming the crate does it. *Hidden dependencies:* the sink and Phase 33's self-hardening both claim the top of `main` (T-42B01 records the order); `log.level` is a setting, but the sink must exist before configuration resolves, hence the reload handle; the TUI owns the terminal, so the console layer must not exist there; the live secret list LL-7 relies on is empty today. *Cascade:* T-42A02 blocks every B and C task; a wrong line shape costs C01's reader and the invariant sweep, so the line format is fixed by A02's golden test before C01 starts. T-42B03 is blocked on a specification amendment and nothing else waits on it.

**Coverage against the spec (what the phase deliberately does not build).** DL-2's native-fault handler per OS (the spec itself marks it Pending; `log status` says *unavailable*); the desktop log panel (the GUI track is held for its own design pass; the core functions it would call are built here); domain-tier emission (T-42B03). Every other row of `l2-diagnostic-log` §3 and every subsection of §4 is mapped to a task above.

**Instruction quality (six dimensions).** Verdict: PASS-WITH-REWRITES, applied — T-42B01's Verify gained the byte-identical stdout check, so a sink that leaks onto standard output fails; T-42B02 now says to find existing fallback branches rather than invent them; T-42C01 states what happens to a schema version the reader does not know. Every Verify names a command or an assertable behaviour.
