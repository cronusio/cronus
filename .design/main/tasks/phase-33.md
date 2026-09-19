---
phase: 33
name: "Execution Sandbox"
status: Todo
subsystem: "crates/contract crates/domain crates/sandbox-os crates/core crates/cli crates/activation-os"
requires: [9, 13, 18, 27]
provides: []
key_files:
  created: []
  modified: []
patterns_established: []
duration_minutes: ~
---

# Stage 33 Tasks — Execution Sandbox

**Phase:** 33
**Status:** Todo
**Strategic Goal:** Realize `l2-execution-sandbox` 1.0.1 on the development platform first: the port and its closed vocabularies, backend selection that refuses instead of degrading, argument-vector spawning with trust by resolved path, an environment plan that strips secrets by default, path confinement that verifies the handle, a native Windows backend in two profiles, engine self-hardening, an enumerated coverage declaration, and one escape corpus as the acceptance oracle. No shipped verb is wired to `spawn` — none executes tools yet; the deliverables are the boundary, its diagnostics, the one caller-in-waiting placed behind the port, and the evidence.

## Atomic Checklist

Track A — Port and pure policy logic

- [ ] [T-33A01] Sandbox port and closed vocabularies in `cronus-contract`
- [ ] [T-33A02] `ResolvedPolicy` — deny-by-default resolution
- [ ] [T-33A03] Backend selection, probing, refusal and `Unenforceable`
- [ ] [T-33A04] `EnvPlan` — allowlist plan with secret-shaped stripping
- [ ] [T-33A05] `Denial` classification across the five axes

Track B — Adapter crate and spawn discipline

- [ ] [T-33B01] `cronus-sandbox-os` crate scaffold, per-OS stubs and probe plumbing
- [ ] [T-33B02] Executable resolver and pin
- [ ] [T-33B03] Windows command-line quoting and batch-file refusal
- [ ] [T-33B04] Privileged helpers in `cronus-activation-os` resolve to fixed absolute paths

Track C — Confinement primitives

- [ ] [T-33C01] Path confinement: resolve, open, verify the handle
- [ ] [T-33C02] Protected anchors

Track D — Native Windows backend

- [ ] [T-33D01] `compat` profile: restricted token, low integrity, labeled roots, Job Object, spawn
- [ ] [T-33D02] Resource bounds, deadline, output cap and child-tree kill
- [ ] [T-33D03] `strict` profile: AppContainer
- [ ] [T-33D04] Self-disruption protection

Track E — Engine self-hardening

- [ ] [T-33E01] Hardening contract, `scrub_env` and the degradation report
- [ ] [T-33E02] Windows hardening steps
- [ ] [T-33E03] Unix hardening steps (compile-checked; `unverified` on this host)
- [ ] [T-33E04] Apply at the top of `main` and surface in the doctor report

Track F — Coverage, wiring and surface

- [ ] [T-33F01] Coverage union and `PolicyContext` derived from it
- [ ] [T-33F02] Facade wiring
- [ ] [T-33F03] `cronus sandbox status` and `probe`
- [ ] [T-33F04] Quality-gate runner behind a process port

Track T — Validation

- [ ] [T-33T01] Escape corpus on the Windows backend
- [ ] [T-33T02] Full-cycle validation

## Detailed Tracking

### [T-33A01] Sandbox port and closed vocabularies in `cronus-contract`

- **Spec:** l2-execution-sandbox.md §4.1, §4.8, §4.9; l1-execution-sandbox.md ES-1, ES-8 (cited in that spec's numbering)
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo test -p cronus-contract sandbox` with new unit tests: (a) `DenialAxis`, `LaunchPath` and `Confinement` are closed sets whose `ALL` arrays have the expected lengths (5, 7, 3) and are matched exhaustively; (b) a `compile_fail` doctest proves `SpawnSpec` has no constructor taking a single command string — `argv` is a `Vec<OsString>`; (c) `CoverageDeclaration::merge` keeps the *least confined* verdict per launch path. `cargo clippy -p cronus-contract --all-targets -- -D warnings` clean.
- **Handoff:** Hard prerequisite for every other task in the phase.
- **Notes:** New module `crates/contract/src/sandbox.rs`, declared and re-exported from `lib.rs` the way the model-transport port was added. Contents: `trait SandboxBackend` (`kind`, `probe`, `prepare`, `spawn`, `classify`, `coverage`), `BackendKind`, `Probe`, `SpawnSpec`, `EnvPlan` (entries, recorded pass-throughs, stripped *names*), `Limits`, `Denial`, `CoverageDeclaration`, `PrepareError { Unenforceable { axis }, .. }`, `SandboxUnavailable { tried }`. Contract-tier rule: zero dependencies, no `serde`.

### [T-33A02] `ResolvedPolicy` — deny-by-default resolution

- **Spec:** l2-execution-sandbox.md §4.1, §4.5, §4.6; l2-sandbox-policy.md §4.1–§4.6; ES-1
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo test -p cronus-domain sandbox::resolved_policy`: an empty policy resolves to no writable roots and no network grants; adding one entry adds exactly that grant with its policy origin recorded; a relative root or an empty path is a typed refusal; a nested root is retained as declared (no silent merging); `isolation_compatibility: best_effort` with an empty `accept_unconfined` list changes nothing.
- **Handoff:** T-33A03.
- **Notes:** New directory module `crates/domain/src/sandbox/` (`mod.rs`, `resolved_policy.rs`), declared in `crates/domain/src/lib.rs`. Reads the existing `SandboxPolicy` / `FilesystemPolicy` / `PolicyTier` in `sandbox_policy.rs`; the domain tier stays no-I/O. The `accept_unconfined` field is new in `l2-sandbox-policy` 1.1.0.

### [T-33A03] Backend selection, probing, refusal and `Unenforceable`

- **Spec:** l2-execution-sandbox.md §4.1 (enforceability), §4.2; ES-8; l1-policy-governance.md PG-6
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo test -p cronus-domain sandbox::select` with fake backends: (1) a configured backend is preferred over the platform default; (2) a failed probe falls through to the next and is listed in `tried` when all fail; (3) no available backend is a typed `SandboxUnavailable` — never a fallback to unconfined; (4) `sandbox.enabled = false` yields `Unconfined` with the persistent marker and an audit entry; (5) `prepare()` returning `Unenforceable { Net }` is refused unless `Net` is in the human's `accept_unconfined`, and an accepted axis is stamped on the selection and audited; (6) selection accepts settings only as a `HumanSandboxSettings` value whose sole public constructor is the policy-file loader (T-33F02) — a test proves an ad-hoc settings struct built by a caller cannot be passed to `select()`.
- **Handoff:** T-33F02 consumes `select()`.
- **Notes:** `select()` takes the resolved policy, a `HumanSandboxSettings` value and the candidate backends as trait objects; probing is the only I/O and it lives behind the port, so the domain stays pure. Audit entries: `sandbox_selected`, `sandbox_axis_accepted_unconfined`.

### [T-33A04] `EnvPlan` — allowlist plan with secret-shaped stripping

- **Spec:** l2-execution-sandbox.md §4.4; l1-process-integrity.md PI-3, PI-5, PI-8; l1-security.md SEC-1, SEC-5
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo test -p cronus-domain sandbox::env_plan`: (a) an innocuously named variable holding a private-key block is stripped; (b) `MY_SERVICE_TOKEN` is stripped by name; (c) `PATH`, `TEMP` and the home-directory variable pass when declared needed; (d) a declared pass-through is kept and recorded with its reason; (e) loader/preload and interpreter-startup vectors are removed even when declared; (f) `format!("{report:?}")` contains variable *names* and no value substring; (g) an undeclared, unknown variable is dropped (safe by omission).
- **Handoff:** T-33B01's Windows spawn (T-33D01) consumes the plan; T-33E01 reuses the injection-vector constant defined here — define it once.
- **Notes:** Pure function in `crates/domain/src/sandbox/env_plan.rs`. Value shapes: private-key block, provider-key prefixes, bearer/session tokens, JWT shape, credential embedded in a URL. The engine's own credential names never pass by default.

### [T-33A05] `Denial` classification across the five axes

- **Spec:** l2-execution-sandbox.md §4.8; l2-sandbox-policy.md §4.9; l1-security.md SEC-11
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo test -p cronus-domain sandbox::denial` table-driven: a permission-denied message naming a path is `Fs` with that target; a non-zero exit with unrelated stderr is `None` (no false confinement blame); a backend `ResourceExceeded(memory)` signal is `Resource`; evidence longer than the cap is truncated; a seeded secret in stderr never appears in `Denial.evidence`.
- **Handoff:** T-33D02 and T-33F03 consume it.
- **Notes:** `crates/domain/src/sandbox/denial.rs`. Reuses `classify_access_failure` from `sandbox_policy.rs` for the `Net` axis rather than duplicating it; scrubs evidence through `crate::redact`.

### [T-33B01] `cronus-sandbox-os` crate scaffold, per-OS stubs and probe plumbing

- **Spec:** l2-execution-sandbox.md §4.14; l2-crate-topology.md §4.4(a)
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo build -p cronus-sandbox-os` and `cargo test -p cronus-sandbox-os` green on this host (PowerShell); `cargo metadata --format-version 1` shows the crate depending on `cronus-contract` only (plus `windows-sys` under `cfg(windows)`); a test asserts the non-Windows stub reports `Probe::Unavailable` with a reason string, and `cargo check -p cronus-sandbox-os --target x86_64-unknown-linux-gnu` is recorded as run or as `unverified — target not installed`.
- **Handoff:** Hard prerequisite for T-33B02, T-33B03, T-33C01, T-33C02, T-33D01…D04, T-33E02, T-33E03.
- **Notes:** New crate `crates/sandbox-os` (`cronus-sandbox-os`), workspace member plus a `[workspace.dependencies]` entry, mirroring `cronus-activation-os`. Minted by the crate-minting rule §4.4(a): it needs a platform-bindings dependency the domain tier may not hold. `default_backends()` returns the backends built for this OS; on Windows the native backend, elsewhere an honest `Unavailable` stub — never a pretend success. **The scaffold declares every module the later tasks fill** (`resolve`, `confine`, `anchors`, `windows::{compat, strict, cmdline, harden}`, `unix::harden`) as stub files, and the **union of the `windows-sys` features** the phase needs (foundation, security and authorization, isolation/AppContainer, job objects, threading, file system, environment), so the OS tasks touch disjoint files and never contend over `lib.rs` or the manifest.

### [T-33B02] Executable resolver and pin

- **Spec:** l2-execution-sandbox.md §4.3; l1-consent-binding.md CB-1, CB-2; l2-sandbox-policy.md §4.4, §5
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo test -p cronus-sandbox-os resolve` (Windows host): with a controlled search-directory list, a lookalike executable resolves only in the order the list states and the order is reported; a bare name is **not** resolved from the current directory; replacing the file after resolution makes `verify_pin` fail; a batch-file target comes back flagged `BatchFile`; every returned path is absolute, canonical and free of `..` (verbatim `\\?\` prefixes normalized consistently).
- **Handoff:** T-33B03 and T-33D01; the guard analysis in a later phase consumes it through the port.
- **Notes:** `crates/sandbox-os/src/resolve.rs`: `resolve_executable(name_or_path, search_dirs) -> Resolved { path, identity }` and `verify_pin`. `identity` = size, modified time and the volume file id. No shim or alias directories by default; extension resolution on Windows is explicit.

### [T-33B03] Windows command-line quoting and batch-file refusal

- **Spec:** l2-execution-sandbox.md §4.3 (argument vectors only)
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo test -p cronus-sandbox-os cmdline`: `build_command_line` round-trips through a reference implementation of the standard argument-splitting rules for a corpus of hostile arguments (embedded quotes, trailing backslashes, spaces, an empty argument, non-ASCII) plus a deterministic pseudo-random generator (no new dependency); a NUL is refused; a batch-file target with `& calc`, `%COMSPEC%` or `^` in an argument is refused (`BatchFileRefused`) unless explicitly allowed *and* every argument passes a strict character allowlist.
- **Handoff:** T-33D01.
- **Notes:** `crates/sandbox-os/src/windows/cmdline.rs`, pure and compiled on every OS so it is testable everywhere. This is the argument-injection class specific to Windows launchers: a batch file re-parses its arguments through `cmd`, so quoting that is correct for the target program is not enough.

### [T-33B04] Privileged helpers in `cronus-activation-os` resolve to fixed absolute paths

- **Spec:** l2-execution-sandbox.md §4.3 ("the launcher is not found by search"); l2-service-activation.md
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo test -p cronus-activation-os` green on this host including a new test that the Windows helper path for `schtasks` is absolute and equals the system directory's copy; `rg -n 'Command::new\("' crates/activation-os/src` returns no bare-name spawn; the Linux and macOS variants are compile-checked with `cargo check -p cronus-activation-os --target <triple>` where installed, otherwise recorded `unverified`.
- **Handoff:** None — independent of the rest of the phase.
- **Notes:** The audit that opened this phase found `systemctl`, `pkexec`, `loginctl` (`linux_calls.rs`), `launchctl`, `sudo` (`macos_calls.rs`) and `schtasks` (`windows_calls.rs`) started by bare name — the elevation helpers are exactly the PATH-shadowing shape. New `crates/activation-os/src/helpers.rs`: `helper_path(Helper) -> Result<PathBuf, HelperError>` from a fixed per-OS candidate list, checked to exist as a regular file (and on Unix not group- or world-writable). Errors are typed, never panics.

### [T-33C01] Path confinement: resolve, open, verify the handle

- **Spec:** l2-execution-sandbox.md §4.5; l1-interception-model.md INT-2; l2-tool-security.md §4.2 path containment
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo test -p cronus-sandbox-os confine` (Windows host), each attempt expecting its named `EscapeError`: `..\outside`; a junction inside the root pointing outside (created in the test via the system `cmd.exe` at its absolute path); a symlink (reported `unverified` when the privilege is absent, never skipped silently); a hard link to an outside file followed by a write; a drive-relative path; `\\?\C:\Windows` and `\\.\PhysicalDrive0`; `file.txt:stream`; `NUL`; the short-name alias of an outside directory; a case-variant of the root accepted as inside and a case-variant outside refused; and a swap test — a check-then-swap-junction race fails to redirect the already-open handle.
- **Handoff:** T-33C02, T-33D01.
- **Notes:** `crates/sandbox-os/src/confine.rs`: `open_confined(root, path, intent) -> Result<ConfinedHandle, EscapeError>`. Open with reparse-point inspection, take the final path *from the handle*, compare after normalization (case-insensitive on Windows volumes, verbatim prefix, short-name expansion). The decision binds to the object that will be used, not to a string checked earlier.

### [T-33C02] Protected anchors

- **Spec:** l2-execution-sandbox.md §4.5; l1-security.md SEC-10; l1-action-gating.md AG-10 (standing-instruction sink)
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo test -p cronus-sandbox-os anchors`: inside a writable root, writes to `.git\config`, a `constitution\SOUL.md`, the sandbox policy file path, and rename or delete of the root itself are refused with `AnchorViolation`; a sibling ordinary file is writable; every path is built from a `CRONUS_PORTABLE_DIR` layout so no test touches real user state.
- **Handoff:** T-33D01.
- **Notes:** `crates/sandbox-os/src/anchors.rs`: `ProtectedSet::from_layout(&Paths, extras)` from the existing path model (`cronus_domain::paths`), consulted by `open_confined` for write, create, delete and rename intents.

### [T-33D01] `compat` profile: restricted token, low integrity, labeled roots, Job Object, spawn

- **Spec:** l2-execution-sandbox.md §4.1 (Windows `compat`), §4.3, §4.4; ES-1…ES-3, ES-5 of l1-execution-sandbox
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo test -p cronus-sandbox-os windows::compat -- --test-threads=1` (Windows host): a confined shell command runs and its output is captured; the child cannot create a file in a medium-integrity directory (typed `Denial { Fs }`) and can create one inside the labeled root; the child token reports low integrity and no enabled non-default privilege; a policy with a network grant makes `prepare()` return `Unenforceable { Net }`; the child's environment equals the `EnvPlan` (secret-shaped variable absent).
- **Handoff:** T-33D02, T-33D03, T-33D04, T-33T01.
- **Notes:** `crates/sandbox-os/src/windows/compat.rs`. `probe()` runs a self-test spawn under the profile. `prepare()` labels writable roots low-integrity and builds the restricted token; `spawn()` starts the child suspended with the command line from T-33B03, assigns it to the Job Object, then resumes it. Highest-risk task in the phase — split `.1` token and labels / `.2` spawn and job if it outgrows one sitting.

### [T-33D02] Resource bounds, deadline, output cap and child-tree kill

- **Spec:** l2-execution-sandbox.md §4.7; ES-4 of l1-execution-sandbox
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo test -p cronus-sandbox-os windows::bounds`: a child that spawns children until the limit returns `Denial { Resource(processes) }`; a memory hog under the per-process limit is terminated; an output flood is truncated at the cap with an explicit marker; a long sleep is killed at the deadline and the job reports no surviving member; the engine (test) process is untouched throughout.
- **Handoff:** T-33T01.
- **Notes:** Job Object limits (active process count, per-process and job memory, CPU time), a supervising deadline that terminates the whole job, output capture with a byte cap. Uses only system tools already present on Windows (the shell, `ping`, PowerShell) as test children — no helper binary.

### [T-33D03] `strict` profile: AppContainer

- **Spec:** l2-execution-sandbox.md §4.1 (Windows `strict`, enforceability), §4.6
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo test -p cronus-sandbox-os windows::strict` (Windows host): a confined child cannot connect to a loopback port the test is listening on and cannot read a file under the user's documents, but reads and writes inside the granted root; after the test no access entry for the profile identity remains on the temporary root; a network grant makes `prepare()` return `Unenforceable { Net }`; if the profile cannot be created on this host `probe()` is `Unavailable` with the reason and the tests report `unverified`, never `passed`.
- **Handoff:** T-33T01.
- **Notes:** A persistent, named, capability-less AppContainer profile created idempotently; the AppContainer identity is granted rights on the writable roots only; spawn carries the security-capabilities attribute and the Job Object of T-33D01. The proxy route on Windows needs a one-time elevated loopback exemption and is deferred by design.

### [T-33D04] Self-disruption protection

- **Spec:** l2-execution-sandbox.md §4.10
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo test -p cronus-sandbox-os windows::self_disruption`: a confined child running the system task-kill utility against the engine (test) process id fails and the engine stays alive; the same by image name; an attempt to write over the engine's own executable path is an `AnchorViolation`; a grandchild started by the child is still a member of the job.
- **Handoff:** T-33T01.
- **Notes:** Largely a verification task: a low-integrity or AppContainer child cannot open a medium-integrity process for termination, and the job forbids breakaway. Anything found missing is a finding recorded before it is fixed.

### [T-33E01] Hardening contract, `scrub_env` and the degradation report

- **Spec:** l2-execution-sandbox.md §4.11; l1-process-integrity.md PI-1…PI-6
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo test -p cronus-domain sandbox::hardening`: `scrub_env` removes each vector and leaves every other variable untouched; a `HardeningReport` distinguishes `Applied`, `Partial { remaining }` and `Unsupported { reason }` per step; its `Display` and `Debug` never include a variable value.
- **Handoff:** T-33E02, T-33E03, T-33E04.
- **Notes:** Types in `crates/contract/src/sandbox.rs` (closed `HardeningStep`: crash dumps, tracer attach, environment), pure functions in `crates/domain/src/sandbox/hardening.rs`. The injection-vector list is the constant defined in T-33A04, not a second copy.

### [T-33E02] Windows hardening steps

- **Spec:** l2-execution-sandbox.md §4.11; PI-1, PI-2, PI-3
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo test -p cronus-sandbox-os windows::harden` (Windows host): after `apply()`, a second process (the test binary re-run in a child mode) cannot open the engine process with memory-read rights (access denied); the report marks crash dumps `Partial` with what remains stated (the operating system's crash reporting may still write per system policy); the loader/preload-style variables are gone from the process environment.
- **Handoff:** T-33E04.
- **Notes:** `crates/sandbox-os/src/windows/harden.rs`: error-mode flags that suppress fault dialogs, a process-object descriptor that removes memory-read/write, thread-creation and handle-duplication rights from same-user callers, and the environment scrub. Windows has no exact tracer-attach denial; the honest outcome is `Partial`, per PI-6.

### [T-33E03] Unix hardening steps (compile-checked; `unverified` on this host)

- **Spec:** l2-execution-sandbox.md §4.11; PI-1, PI-2
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo check -p cronus-sandbox-os --target x86_64-unknown-linux-gnu` and `--target aarch64-apple-darwin` where installed; otherwise the task records `unverified — target not installed` and its `cfg(unix)` tests are **not** counted as passed.
- **Handoff:** T-33E04.
- **Notes:** `crates/sandbox-os/src/unix/harden.rs`: not-dumpable and zero core-size limit on Linux; deny-attach and zero core-size limit on macOS. Adds a `libc` dependency under `cfg(unix)` — justified as a direct binding to OS calls the standard library does not expose; state that in the commit rationale per the dependency policy.

### [T-33E04] Apply at the top of `main` and surface in the doctor report

- **Spec:** l2-execution-sandbox.md §4.11; PI-4, PI-6; l2-doctor.md
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo test -p cronus-cli` with a new smoke test: `cronus doctor --format json` (isolated state) contains a hardening section listing each step's outcome; a unit test in `crates/domain/src/doctor.rs` maps `Partial` and `Unsupported` to warnings and an unexpected failure to critical; `cronus status` and `cronus init` behave exactly as before.
- **Handoff:** T-33T02.
- **Notes:** `crates/cli/src/main.rs` runs the hardening sequence before configuration or secrets are touched (PI-4) and keeps the report in a `OnceLock` for the doctor. The doctor gains one check that reads it.

### [T-33F01] Coverage union and `PolicyContext` derived from it

- **Spec:** l2-execution-sandbox.md §4.9; l1-security.md SEC-12(b), SEC-12(d); l2-sandbox-policy.md §4.8
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo test -p cronus-domain sandbox::coverage` and `cargo test -p cronus-domain sandbox_policy`: with a fake backend declaring only child processes confined, `PolicyContext::support_boundaries` lists in-process tools, hooks and plugin code as unconfined; a launch path missing from the declaration renders as unconfined ("not listed means unconfined"); a JSON snapshot of the seven-row table is stable; no rendered string calls an unconfined path "sandboxed".
- **Handoff:** T-33F02, T-33F03.
- **Notes:** Until a tool-execution path routes hooks and tool servers through the backend, those rows are honestly `UnconfinedGated` — the coverage table is a statement of what is enforced today, not of what is intended.

### [T-33F02] Facade wiring

- **Spec:** l2-execution-sandbox.md §4.2, §4.14; l2-crate-topology.md §4.5
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo test -p cronus` (crates/core) with a new integration test `tests/sandbox_selection.rs` under `CRONUS_PORTABLE_DIR`: default settings on this host select the native Windows backend; a policy file with `sandbox.enabled = false` yields `Unconfined` with the marker; a file naming an unavailable backend yields a refusal listing it; an unreadable or corrupt policy file **fails closed** (refusal, not unconfined).
- **Handoff:** T-33F03.
- **Notes:** `crates/core/src/sandbox_bootstrap.rs` mirrors `activation_bootstrap.rs` and `pub use cronus_sandbox_os as sandbox_os;` mirrors the activation adapter re-export. A JSON policy-file loader (the JSON form of the `l2-sandbox-policy` schema, written by the host, read here) maps the DTO into `SandboxPolicy`; fields with no Windows meaning (`run_as_user`, `run_as_group`) are accepted and reported as not applicable. `serde_json` is already a workspace dependency; YAML stays out of scope.

### [T-33F03] `cronus sandbox status` and `probe`

- **Spec:** l2-execution-sandbox.md §4.15; l2-cli.md §4.1.1; INV-9
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo test -p cronus-cli` with new smoke tests using `isolated_state()`: `cronus sandbox status` prints the selected backend and the coverage table's unconfined rows; `--format json` parses and carries `selected`, `unconfined_axes`, `backends`, `coverage` and `policy_digest`; `cronus sandbox probe` exits 0 on this host; with `enabled = false` in the policy file `status` prints the persistent unconfined marker; `cronus sandbox explain` is absent from `--help` (INV-9); the conformance corpus registration test stays green.
- **Handoff:** T-33T02.
- **Notes:** An **installation-locus** group (diagnostics), declared once in `installation.rs` and handled by a new `sandbox_cmd` module in `commands.rs`; the TUI does not project it and the conformance registration declares that exclusion. `explain` is deferred until a run path records denials — a verb that could only answer "unavailable" does not ship (INV-9).

### [T-33F04] Quality-gate runner behind a process port

- **Spec:** l2-execution-sandbox.md §4.3, §4.9; l2-crate-topology.md §4.3 (the domain tier performs no I/O); l1-security.md SEC-6
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** `cargo test -p cronus-domain quality` with a fake runner: (1) each gate builds an argument vector (`["cargo", "test"]`, never a joined string) and asks the runner to start it in the project root; (2) a runner refusal (`SandboxUnavailable`, `Unenforceable`) becomes `GateStatus::Fail` with a `sandbox_refused:` output prefix — never `Pass`, never `Skipped`, and never an unconfined fallback; (3) an unknown language stays `Skipped` exactly as before; `rg -n 'std::process' crates/domain/src` returns no match. A core-level test on this host runs a trivial command through the real runner in a temporary workspace and gets `Pass`.
- **Handoff:** Depends on T-33A03, T-33B02, T-33D01 and T-33F02 (the real-runner test needs a working backend and the facade selection). `check run` still invokes no tools; enabling it is a separate product decision.
- **Notes:** `quality::run_gate` in `crates/domain/src/quality.rs` is dormant (no caller) and is the workspace's one piece of domain-tier process I/O: it starts `cargo`, `pnpm`, `pytest`, `go` and the rest by bare name, in the project directory, with the engine's whole environment. This task takes a `ProcessRunner` port (contract tier) as an argument, deletes the `std::process` import from the domain, and implements the port in the facade over the selected backend (resolver, `EnvPlan`, `spawn`). Whoever later wires `check run` to execute gates inherits confinement instead of having to add it.

### [T-33T01] Escape corpus on the Windows backend

- **Goal:** One corpus of attempts, run against both Windows profiles, as the acceptance evidence for the phase — because a sandbox nobody attacks proves nothing.
- **Method:** `crates/sandbox-os/tests/escape_corpus.rs`: write outside the root; junction swap between check and use; hard link to an outside file; rename across the boundary; a secret-shaped variable and an engine key in the parent environment (the child prints its own environment); a direct socket connect; a request to a metadata address; a fork bomb, a memory hog and an output flood; signalling the engine; opening the policy file and a standing-instruction file for write; batch-file argument injection. Each attempt yields `Passed`, `Failed` or `Unverified { reason }`; the test prints the table and asserts no `Failed`, that every `Unverified` carries a reason, and that the attempt count per profile equals the expected number so the corpus cannot silently shrink. An attempt the `compat` profile is *declared* not to confine (the network) is recorded `unconfined-by-declaration`, not `passed`. Per the conformance-corpus precedent the **first run is expected to find defects**: convert each into a finding (`record-diagnostic`) before repairing anything.
- **Verify:** `cargo test -p cronus-sandbox-os --test escape_corpus -- --nocapture` prints the table and exits 0.
- **Status:** Todo

### [T-33T02] Full-cycle validation

- **Goal:** Verify Phase 33 as a whole against `l2-execution-sandbox` 1.0.1 and the process-integrity invariants it realizes.
- **Method:** `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace` (PowerShell; `-j 2` on this host if rustc crashes) green across 2 consecutive runs — the known simulation flake is a recorded diagnostic, not a reason to rerun until green; `rg` sweep for `unwrap\(\)|panic!\(|\.expect\(` over every touched production file; every `unsafe` block in `crates/sandbox-os` carries a `// SAFETY:` comment (grep); by-hand run of `cronus sandbox status`, `cronus sandbox probe` and `cronus doctor` against the freshly built binary; containment sweep of the added lines for SDD-layer references (§6); the coverage table printed by `sandbox status` compared line by line with `l2-execution-sandbox` §4.9.
- **Status:** Todo

## Planning Audit and Instruction Review

**Planner audit (optimism, hidden dependencies, cascade).** *Optimism:* 25 tasks for one L2 whose cost is dominated by Windows debugging (tokens, integrity labels, AppContainer, Job Objects); T-33D01 and T-33C01 are the two tasks most likely to outgrow a sitting and may split `.1` / `.2`; T-33B04 and T-33E03 cannot be verified on this host and say so. *Hidden dependencies:* every OS task shares `crates/sandbox-os` — resolved by T-33B01 declaring every module and the union of platform features up front; T-33A04's injection-vector constant is shared with T-33E01 and defined once; T-33F04's real-runner test needs D01 and F02. *Cascade:* T-33A01 blocks every track and T-33B01 blocks every OS task, so a wrong port shape reworks D, E and F — read the port adversarially before D01 starts.

**Coverage against the spec (what the phase deliberately does not build).** §4.6 the egress proxy and every non-Windows backend; the container backend and its image pin (ES-6); routing hook commands and tool-server launches through `spawn` (§4.9 rows stay honestly *unconfined-but-gated*); §4.13 unattended callers, because nothing unattended executes yet; `cronus sandbox explain`. Every other section of `l2-execution-sandbox` 1.0.1 is mapped to at least one task above.

**Instruction quality (six dimensions).** Verdict: PASS-WITH-REWRITES, applied — T-33A03 (6) replaced a vague "loader stub" with a checkable construction rule; T-33B01 gained the file-independence mechanism; T-33F04 states its real handoff. No contradictions between Verify lines and the spec found; every Verify names a command or an assertable behaviour.
