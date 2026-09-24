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
/// then tick, returning the feedback the loop produced.
fn submit_line(app: &mut App, line: &str) -> Option<String> {
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
    app.view().command_feedback.clone()
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
fn help_lists_the_real_bootstrapped_catalog() {
    let mut app = real_app();
    let feedback = submit_line(&mut app, "help");

    let feedback = feedback.expect("/help always produces feedback");
    assert!(
        feedback.starts_with("commands: "),
        "unexpected /help feedback shape: {feedback:?}"
    );
    for expected in ["board", "pane"] {
        assert!(
            feedback.contains(expected),
            "the real catalog derived from the bootstrapped registry must \
             list '{expected}' as a discoverable group; got: {feedback:?}"
        );
    }
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
