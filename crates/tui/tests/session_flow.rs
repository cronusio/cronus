//! Black-box integration coverage for the TUI surface: every existing
//! test in `crates/tui/src/*.rs` drives `App`/`dispatch` over a synthetic or
//! partial registry (an empty one, a hand-built `board.list` fixture, a
//! `test_catalog()`) — none of them combine the *real* production wiring
//! [`app::run`] actually performs: bootstrapping the real core registry
//! (`cronus_core::invocable_bootstrap::bootstrap`), registering this
//! surface's own actions (`pane_actions::register`), and deriving the
//! catalog from the result (`command::build_catalog`), all driven through
//! `App::tick` by a realistic key/command sequence, exactly as a live
//! session would. This file is that missing seam — a second OS process
//! from every other test binary in the workspace, so it can freely
//! isolate state (`CRONUS_PORTABLE_DIR`) without racing anything outside it.
//!
//! Only the crate's public API is used here (no `#[cfg(test)]` internals),
//! the same boundary an external consumer of `cronus_tui` would see.

use std::io;
use std::path::PathBuf;

use cronus_contract::Invocable;
use cronus_core::invocable::{Dispatcher, InvocableRegistry};
use cronus_tui::{
    App, CommandSpec, Key, Renderer, SnapshotSource, TermEvent, ViewModel, command, pane_actions,
};

// `CRONUS_PORTABLE_DIR` is process-global; only one test below touches
// workspace-scoped state (`/board list`), but the lock is held around it
// anyway — the same defensive pattern `crates/domain/tests/portable_dir_env.rs`
// and `crates/core/tests/memory_dispatch.rs` establish, so a future test
// added to this file that also touches it is safe by construction rather
// than by accident.
static PORTABLE_DIR_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock_portable_dir_env() -> std::sync::MutexGuard<'static, ()> {
    PORTABLE_DIR_ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn isolated_portable_dir(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "cronus-tui-session-flow-{tag}-{}",
        std::process::id()
    ))
}

/// A [`SnapshotSource`] that never has a snapshot ready — isolates these
/// tests to the command-bar/key-handling path without also exercising
/// `CapabilitySource`'s own board/office polling (that seam is `app.rs`'s
/// own concern; see its `dispatch_board`/`dispatch_office` unit tests).
struct NullSource;

impl SnapshotSource for NullSource {
    fn poll_snapshot(
        &mut self,
        _registry: &InvocableRegistry,
        _dispatcher: &Dispatcher,
    ) -> Option<cronus_tui::CoreSnapshot> {
        None
    }
}

/// A [`Renderer`] that discards every frame — these tests assert on
/// `App::view()` after each tick, not on drawn buffer content.
struct NullRenderer;

impl Renderer for NullRenderer {
    fn draw(&mut self, _view: &ViewModel) -> io::Result<()> {
        Ok(())
    }
}

/// The exact composition [`cronus_tui::app::run`] performs, minus the real
/// terminal/backend: bootstrap the real core registry, register this
/// surface's own pane actions through the same door, derive the catalog
/// from the result. A test built on anything less than this would not be
/// proving the production wiring — only a hand-picked subset of it.
fn real_app() -> App {
    let (mut registry, mut dispatcher) =
        cronus_core::invocable_bootstrap::bootstrap(cronus_core::Engine::new());
    pane_actions::register(&mut registry, &mut dispatcher);
    let invocables: Vec<&Invocable> = registry.all().collect();
    let catalog: Vec<CommandSpec> = command::build_catalog(&invocables);
    App::new(ViewModel::default(), registry, dispatcher, catalog)
}

/// Feed one command-bar line (without the leading `/`) followed by Enter,
/// then tick, returning the one-line feedback the loop produced.
fn submit_line(app: &mut App, line: &str) -> Option<String> {
    submit(app, line);
    app.view().command_feedback.clone()
}

/// The same, returning the multi-line result block the loop produced.
fn submit_block(app: &mut App, line: &str) -> Option<cronus_tui::ResultBlock> {
    submit(app, line);
    app.view().result_block.clone()
}

fn submit(app: &mut App, line: &str) {
    // The command bar only accepts text input while it has focus; cycle
    // into it first through the real, registered `pane.focus-next` action
    // — one Tab, one tick, re-checking the *updated* focus each time
    // (never pre-building a fixed-length event list against a focus value
    // that has not moved yet), with a hard cap as a safety valve rather
    // than a loop that could run away.
    let mut guard = 0;
    while app.view().focus != cronus_tui::Focus::CommandBar {
        app.tick(
            &[TermEvent::Key(Key::Tab)],
            &mut NullSource,
            &mut NullRenderer,
        )
        .expect("ticking an in-memory app never fails");
        guard += 1;
        assert!(
            guard <= FOCUS_ORDER_LEN,
            "focus never reached the command bar after {FOCUS_ORDER_LEN} Tabs"
        );
    }

    let mut events: Vec<TermEvent> = line.chars().map(|c| TermEvent::Key(Key::Char(c))).collect();
    events.push(TermEvent::Key(Key::Enter));
    app.tick(&events, &mut NullSource, &mut NullRenderer)
        .expect("ticking an in-memory app never fails");
}

/// Safety-valve bound for the Tab-cycling loop in [`submit_line`] — the
/// number of panels `Focus::ORDER` declares, so a real regression (focus
/// stuck, or the command bar dropped from the cycle) fails fast with a
/// clear message instead of spinning.
const FOCUS_ORDER_LEN: usize = cronus_tui::Focus::ORDER.len();

#[test]
fn a_known_group_and_verb_dispatches_for_real_against_an_empty_workspace() {
    let _env = lock_portable_dir_env();
    let base = isolated_portable_dir("board-list");
    // SAFETY: serialized by `PORTABLE_DIR_ENV_LOCK` — no concurrent
    // reader/writer of `CRONUS_PORTABLE_DIR` within this process.
    unsafe { std::env::set_var("CRONUS_PORTABLE_DIR", &base) };

    let mut app = real_app();
    let feedback = submit_line(&mut app, "board list");

    unsafe { std::env::remove_var("CRONUS_PORTABLE_DIR") };

    assert_eq!(
        feedback.as_deref(),
        Some("no results"),
        "a real core:board.list dispatch against a genuinely empty, isolated \
         workspace must render the same 'no results' text `render_outcome` \
         gives any other genuinely-empty list"
    );
}

#[test]
fn an_unregistered_verb_in_a_known_group_is_silently_ignored() {
    // No filesystem access: `core:board.does-not-exist` has no attached
    // handler, so the dispatcher answers `Dispatched::Unknown` from a plain
    // lookup miss — `open_board()` is never called. No env isolation needed.
    let mut app = real_app();
    let feedback = submit_line(&mut app, "board does-not-exist");

    assert_eq!(
        feedback, None,
        "an unrecognized verb inside a known group must clear feedback \
         silently — the same deliberate behavior app.rs's own \
         doc comment on `pending_dispatch` handling describes, now proven \
         through the real bootstrapped registry rather than a stub"
    );
}

#[test]
fn an_unrecognized_group_surfaces_an_inline_catalog_error() {
    let mut app = real_app();
    let feedback = submit_line(&mut app, "totally-unknown-cmd");

    assert_eq!(
        feedback.as_deref(),
        Some("unknown command: /totally-unknown-cmd (try /help)"),
        "a line whose first token names no catalog group must be rejected \
         at classification, before dispatch is ever attempted"
    );
}

#[test]
fn help_lists_the_real_bootstrapped_catalog_with_installation_verbs_set_apart() {
    let mut app = real_app();
    let block = submit_block(&mut app, "help").expect("/help always produces a result block");

    assert_eq!(block.title, "/help");
    let position = |prefix: &str| {
        block
            .lines
            .iter()
            .position(|line| line.starts_with(prefix))
            .unwrap_or_else(|| panic!("/help must list {prefix:?}; got: {:?}", block.lines))
    };
    let heading = position("Installation");
    for work in ["/board", "/pane"] {
        assert!(
            position(work) < heading,
            "{work} is a command on the user's work and is listed before the installation heading"
        );
    }
    for installation in ["/status", "/doctor", "/backup", "/ext", "/workspace"] {
        assert!(
            position(installation) > heading,
            "{installation} is an installation verb and is listed under its own heading"
        );
    }
}

/// The installation set this surface offers: exactly the command line's
/// declaration, minus the verbs this surface declares it does not offer.
#[test]
fn the_catalog_offers_the_command_lines_installation_verbs_less_the_declared_exclusions() {
    let (registry, dispatcher) =
        cronus_core::invocable_bootstrap::bootstrap(cronus_core::Engine::new());
    let declared = cronus_core::installation::declared_invocables();

    let projected: std::collections::BTreeSet<String> = registry
        .all()
        .filter(|i| i.locus.kind() == cronus_contract::LocusKind::Installation)
        .filter(|i| command::is_projected(i))
        .map(|i| i.id.as_str().to_string())
        .collect();
    let expected: std::collections::BTreeSet<String> = declared
        .iter()
        .filter(|i| command::installation_exclusion(&i.id).is_none())
        .map(|i| i.id.as_str().to_string())
        .collect();
    assert_eq!(
        projected, expected,
        "this surface's installation set must be a subset of the command line's, short only by \
         what it declares"
    );

    for id in &projected {
        let id = cronus_contract::InvocableId::new(id.clone()).expect("registered ids are valid");
        assert!(
            dispatcher.has_handler(&id),
            "{id} is offered here but has no handler to run"
        );
    }
    for (excluded, reason) in command::INSTALLATION_EXCLUSIONS {
        assert!(
            !reason.is_empty(),
            "{excluded} is excluded without a reason"
        );
        let id = cronus_contract::InvocableId::new(excluded).expect("excluded ids are valid");
        assert!(
            registry.resolve(&id).is_found(),
            "{excluded} is declared excluded but is not in the catalog — an exclusion of nothing"
        );
    }
}

/// One name per surface: an installation group never shares its name with a
/// group of any other locus, or the slash catalog would hold two commands
/// under one name.
#[test]
fn no_catalog_name_is_held_by_two_commands() {
    let app_catalog = {
        let (mut registry, mut dispatcher) =
            cronus_core::invocable_bootstrap::bootstrap(cronus_core::Engine::new());
        pane_actions::register(&mut registry, &mut dispatcher);
        let invocables: Vec<&Invocable> = registry.all().collect();
        command::build_catalog(&invocables)
    };
    let mut seen = std::collections::HashSet::new();
    for spec in &app_catalog {
        assert!(
            seen.insert(spec.name),
            "two commands share the name /{}",
            spec.name
        );
    }
}

#[test]
fn an_installation_verb_that_only_reads_runs_for_real_and_shows_its_answer_as_a_block() {
    let _env = lock_portable_dir_env();
    let base = isolated_portable_dir("status");
    // SAFETY: serialized by `PORTABLE_DIR_ENV_LOCK`.
    unsafe { std::env::set_var("CRONUS_PORTABLE_DIR", &base) };
    let root = cronus_core::paths::resolve_workspace_root();
    assert!(
        root.starts_with(&base),
        "the workspace root resolved to {root:?}, outside the isolated directory {base:?} — a \
         workspace marker above the test's working directory would be overwritten"
    );
    std::fs::create_dir_all(&root).expect("the isolated state root is creatable");
    std::fs::write(root.join("app.json"), "{}\n").expect("the marker is writable");

    let mut app = real_app();
    let block = submit_block(&mut app, "status");
    let feedback = app.view().command_feedback.clone();

    unsafe { std::env::remove_var("CRONUS_PORTABLE_DIR") };
    let _ = std::fs::remove_dir_all(&base);

    let block = block.expect("an installation verb answers with a block");
    assert_eq!(block.title, "/status");
    assert!(
        block.lines.iter().any(|line| line == "phase: ready"),
        "{:?}",
        block.lines
    );
    assert!(
        block
            .lines
            .iter()
            .any(|line| line.starts_with("workspace: ")),
        "{:?}",
        block.lines
    );
    assert_eq!(
        feedback, None,
        "the one-line feedback is left to ordinary commands"
    );
}

/// "There is no workspace" is a different fact from "the workspace is empty",
/// and the block must say which.
#[test]
fn a_missing_workspace_is_reported_as_unavailable_not_as_an_empty_answer() {
    let _env = lock_portable_dir_env();
    let base = isolated_portable_dir("status-none");
    unsafe { std::env::set_var("CRONUS_PORTABLE_DIR", &base) };

    let mut app = real_app();
    let block = submit_block(&mut app, "status");

    unsafe { std::env::remove_var("CRONUS_PORTABLE_DIR") };
    let _ = std::fs::remove_dir_all(&base);

    let block = block.expect("an installation verb answers with a block");
    assert_eq!(block.lines.len(), 1, "{:?}", block.lines);
    assert!(
        block.lines[0].starts_with("unavailable: No workspace initialized"),
        "{:?}",
        block.lines
    );
}

/// A verb that would change the installation is not carried out from this
/// surface yet; it says so and names the command that does.
#[test]
fn a_verb_that_changes_the_installation_is_refused_with_the_command_line_that_does_it() {
    let mut app = real_app();
    let block = submit_block(&mut app, "backup create --to elsewhere").expect("block");

    assert_eq!(block.title, "/backup create --to elsewhere");
    assert!(
        block.lines[0].starts_with("not run from here"),
        "{:?}",
        block.lines
    );
    assert!(
        block
            .lines
            .iter()
            .any(|line| line.contains("`cronus backup create --to elsewhere`")),
        "{:?}",
        block.lines
    );
}

#[test]
fn diagnostics_inspects_but_diagnostics_with_repair_is_raised_and_refused() {
    let mut app = real_app();
    let repair = submit_block(&mut app, "doctor --fix").expect("block");
    assert!(
        repair.lines[0].contains("recompose, applied by relaunch"),
        "{:?}",
        repair.lines
    );
    assert!(
        repair
            .lines
            .iter()
            .any(|line| line.contains("`cronus doctor --fix`")),
        "{:?}",
        repair.lines
    );
}

/// The one declared exclusion that matters to a person who types it: asking
/// for `/tui` inside the terminal UI is answered, not called unknown.
#[test]
fn an_excluded_installation_verb_is_explained_rather_than_called_unknown() {
    let mut app = real_app();
    let feedback = submit_line(&mut app, "tui").expect("an explained refusal");
    assert!(feedback.contains("not offered here"), "{feedback}");
    assert!(!feedback.contains("unknown command"), "{feedback}");

    let completion = submit_line(&mut app, "completion bash").expect("an explained refusal");
    assert!(completion.contains("cronus completion"), "{completion}");
}

/// The next keystroke begins a new line, so the block of the last one goes.
#[test]
fn the_next_keystroke_dismisses_the_result_block() {
    let mut app = real_app();
    assert!(submit_block(&mut app, "backup create").is_some());
    app.tick(
        &[TermEvent::Key(Key::Char('x'))],
        &mut NullSource,
        &mut NullRenderer,
    )
    .expect("ticking an in-memory app never fails");
    assert_eq!(app.view().result_block, None);
}

#[test]
fn tab_cycles_focus_through_every_panel_via_the_real_pane_actions() {
    let mut app = real_app();
    assert_eq!(app.view().focus, cronus_tui::Focus::Board);

    // `Focus::ORDER` has 5 members; cycling all 5 Tabs returns to the start.
    for _ in 0..5 {
        app.tick(
            &[TermEvent::Key(Key::Tab)],
            &mut NullSource,
            &mut NullRenderer,
        )
        .expect("ticking an in-memory app never fails");
    }
    assert_eq!(
        app.view().focus,
        cronus_tui::Focus::Board,
        "five Tabs through the real, registered `pane.focus-next` action \
         must return focus to its starting panel"
    );
}

#[test]
fn esc_quits_through_the_real_pane_action_when_not_in_the_command_bar() {
    let mut app = real_app();
    assert_eq!(
        app.view().focus,
        cronus_tui::Focus::Board,
        "quitting via Esc from the default focus is what this test proves; \
         if the default focus ever changes this assertion should be revisited"
    );

    let result = app
        .tick(
            &[TermEvent::Key(Key::Esc)],
            &mut NullSource,
            &mut NullRenderer,
        )
        .expect("ticking an in-memory app never fails");

    assert!(
        result.quit,
        "Esc outside the command bar must quit through the real, \
         registered `pane.quit` action — not a local `should_quit = true` \
         that never went through the shared dispatcher"
    );
}
