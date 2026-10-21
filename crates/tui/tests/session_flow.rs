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
use std::sync::Arc;

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
        _registry: &Arc<InvocableRegistry>,
        _dispatcher: &Arc<Dispatcher>,
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
    cronus_tui::chat_actions::register(&mut registry, &mut dispatcher);
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
    settle(app);
}

/// Tick until the work the loop started beside itself has been applied.
fn settle(app: &mut App) {
    for _ in 0..2000 {
        if !app.is_busy() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
        app.tick(&[], &mut NullSource, &mut NullRenderer)
            .expect("ticking an in-memory app never fails");
    }
    panic!("the work started beside the loop never finished");
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

/// A verb that recomposes by relaunch is asked about like any other change,
/// naming what it overwrites and that the screen restarts; saying no restores
/// nothing and restarts nothing.
#[test]
fn a_restore_is_asked_about_naming_what_it_overwrites_and_declining_changes_nothing() {
    let _env = lock_portable_dir_env();
    let base = isolated_portable_dir("restore-decline");
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("creatable");
    unsafe { std::env::set_var("CRONUS_PORTABLE_DIR", &base) };

    let mut app = real_app();
    submit(&mut app, "restore backup-1");
    let asked = app.view().result_block.clone().expect("a question");
    let waiting = app.is_awaiting_consent();
    let result = app
        .tick(
            &[TermEvent::Key(Key::Char('n'))],
            &mut NullSource,
            &mut NullRenderer,
        )
        .expect("ticking an in-memory app never fails");
    settle(&mut app);

    unsafe { std::env::remove_var("CRONUS_PORTABLE_DIR") };
    let _ = std::fs::remove_dir_all(&base);

    assert!(waiting, "{:?}", asked.lines);
    assert_eq!(asked.title, "confirm /restore backup-1");
    assert!(
        asked.lines[0].contains("applied by relaunch"),
        "{:?}",
        asked.lines
    );
    assert!(
        asked
            .lines
            .iter()
            .any(|l| l.contains("over the state root")),
        "{:?}",
        asked.lines
    );
    assert!(
        asked
            .lines
            .iter()
            .any(|l| l == "will run: cronus restore backup-1"),
        "{:?}",
        asked.lines
    );
    assert!(!result.relaunch, "a declined change relaunches nothing");
    assert_eq!(
        app.view().command_feedback.as_deref(),
        Some("left undone: /restore backup-1")
    );
}

/// Press one key and let whatever it started finish.
fn press(app: &mut App, key: Key) {
    app.tick(&[TermEvent::Key(key)], &mut NullSource, &mut NullRenderer)
        .expect("ticking an in-memory app never fails");
    settle(app);
}

/// A change this surface carries out is asked about first, naming the exact
/// invocation, and happens only on a yes: a stray key neither agrees nor
/// closes the question, no leaves it undone, and yes makes the backup where it
/// was told to.
#[test]
fn a_backup_asked_for_inside_is_made_only_after_the_person_says_yes() {
    let _env = lock_portable_dir_env();
    let base = isolated_portable_dir("backup-consent");
    let dest = isolated_portable_dir("backup-consent-dest");
    let _ = std::fs::remove_dir_all(&base);
    let _ = std::fs::remove_dir_all(&dest);
    std::fs::create_dir_all(&base).expect("creatable");
    // SAFETY: serialized by `PORTABLE_DIR_ENV_LOCK`.
    unsafe { std::env::set_var("CRONUS_PORTABLE_DIR", &base) };
    // What the backup takes: the state root this composition resolves.
    let root = cronus_core::paths::resolve_workspace_root();
    assert!(root.starts_with(&base), "{root:?} is outside {base:?}");
    std::fs::create_dir_all(&root).expect("creatable");
    std::fs::write(root.join("data.txt"), "state").expect("writable");

    let mut app = real_app();
    let line = format!("backup create --to {}", dest.display());
    submit(&mut app, &line);

    let asked = app.view().result_block.clone().expect("a question");
    assert!(app.is_awaiting_consent());
    assert_eq!(asked.title, format!("confirm /{line}"));
    assert!(
        asked
            .lines
            .iter()
            .any(|l| l.starts_with("will run: cronus backup create --to ")),
        "{:?}",
        asked.lines
    );
    assert!(!dest.exists(), "nothing is done before the answer");

    press(&mut app, Key::Char('x'));
    assert!(app.is_awaiting_consent(), "a stray key answers nothing");
    assert_eq!(app.view().result_block, Some(asked.clone()));

    press(&mut app, Key::Char('n'));
    assert!(!app.is_awaiting_consent());
    assert_eq!(app.view().result_block, None);
    assert_eq!(
        app.view().command_feedback.as_deref(),
        Some(format!("left undone: /{line}").as_str())
    );
    assert!(!dest.exists(), "no leaves it undone");

    submit(&mut app, &line);
    assert!(app.is_awaiting_consent(), "the same line asks again");
    press(&mut app, Key::Char('y'));
    let answer = app.view().result_block.clone().expect("the answer");

    let made = dest.join("data.txt").is_file();
    unsafe { std::env::remove_var("CRONUS_PORTABLE_DIR") };
    let _ = std::fs::remove_dir_all(&base);
    let _ = std::fs::remove_dir_all(&dest);

    assert!(!app.is_awaiting_consent());
    assert_eq!(answer.title, format!("/{line}"));
    assert!(
        answer.lines.iter().any(|l| l.starts_with("id: backup-")),
        "{:?}",
        answer.lines
    );
    assert!(made, "yes makes the backup where it was told to");
}

/// Ask for a change, say yes, and return the block that answers it.
fn agree(app: &mut App, line: &str) -> cronus_tui::ResultBlock {
    submit(app, line);
    assert!(app.is_awaiting_consent(), "{line} asks first");
    press(app, Key::Char('y'));
    assert!(!app.is_awaiting_consent());
    app.view().result_block.clone().expect("the answer")
}

/// A workspace's whole life from inside: created, switched to, and deleted,
/// each only after a yes, each seen afterwards in the surface's own listing —
/// and the one the office runs around is not deletable from here.
#[test]
fn a_workspace_can_be_created_switched_to_and_deleted_inside() {
    let _env = lock_portable_dir_env();
    let base = isolated_portable_dir("workspace-life");
    let place = isolated_portable_dir("workspace-life-place");
    for dir in [&base, &place] {
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir).expect("creatable");
    }
    unsafe { std::env::set_var("CRONUS_PORTABLE_DIR", &base) };

    let mut app = real_app();
    let created = agree(
        &mut app,
        &format!("workspace create alpha --path {}", place.display()),
    );
    agree(
        &mut app,
        &format!("workspace create bravo --path {}", place.display()),
    );
    let switched = agree(&mut app, "workspace switch bravo");
    let listed = submit_block(&mut app, "workspace list").expect("a listing");

    // alpha is not the active one: deleting it is an ordinary change.
    let deleted = agree(&mut app, "workspace delete alpha");
    // bravo is: deleting it changes what is running, so it is refused.
    let refused = submit_block(&mut app, "workspace delete bravo").expect("a block");
    let asked_about_refused = app.is_awaiting_consent();
    let after = submit_block(&mut app, "workspace list").expect("a listing");

    unsafe { std::env::remove_var("CRONUS_PORTABLE_DIR") };
    let _ = std::fs::remove_dir_all(&base);
    let _ = std::fs::remove_dir_all(&place);

    assert!(
        created.lines.iter().any(|l| l == "id: alpha"),
        "{:?}",
        created.lines
    );
    assert!(
        switched.lines.iter().any(|l| l == "id: bravo"),
        "{:?}",
        switched.lines
    );
    assert!(
        listed
            .lines
            .iter()
            .any(|l| l.contains("bravo") && l.contains("active: true")),
        "the switched-to workspace is marked active: {:?}",
        listed.lines
    );
    assert!(
        deleted.lines.iter().any(|l| l == "id: alpha"),
        "{:?}",
        deleted.lines
    );
    assert!(!asked_about_refused, "a refusal asks nothing");
    assert!(
        refused
            .lines
            .iter()
            .any(|l| l.contains("changes what is running")),
        "{:?}",
        refused.lines
    );
    assert!(
        refused
            .lines
            .iter()
            .any(|l| l.contains("cronus workspace delete bravo")),
        "{:?}",
        refused.lines
    );
    assert!(
        !after.lines.iter().any(|l| l.contains("alpha"))
            && after.lines.iter().any(|l| l.contains("bravo")),
        "{:?}",
        after.lines
    );
}

/// An extension and an agent definition added from inside show up in the
/// surface's own listings of them afterwards.
#[test]
fn an_extension_and_an_agent_added_inside_are_listed_afterwards() {
    let _env = lock_portable_dir_env();
    let base = isolated_portable_dir("catalog-adds");
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("creatable");
    let manifest = base.join("demo.json");
    std::fs::write(
        &manifest,
        r#"{"id":"demo/safe","name":"Safe Demo","version":"1.0.0"}"#,
    )
    .expect("writable");
    unsafe { std::env::set_var("CRONUS_PORTABLE_DIR", &base) };

    let mut app = real_app();
    let added = agree(&mut app, &format!("ext add {}", manifest.display()));
    let extensions = submit_block(&mut app, "ext list").expect("a listing");
    let created = agree(&mut app, "registry create reviewer reviews-diffs");
    let agents = submit_block(&mut app, "registry list").expect("a listing");

    unsafe { std::env::remove_var("CRONUS_PORTABLE_DIR") };
    let _ = std::fs::remove_dir_all(&base);

    assert!(
        added.lines.iter().any(|l| l.contains("result: registered")),
        "{:?}",
        added.lines
    );
    assert!(
        extensions.lines.iter().any(|l| l.contains("demo/safe")),
        "{:?}",
        extensions.lines
    );
    assert!(
        created.lines.iter().any(|l| l == "name: reviewer"),
        "{:?}",
        created.lines
    );
    assert!(
        agents.lines.iter().any(|l| l.contains("reviewer")),
        "{:?}",
        agents.lines
    );
}

/// The grant of background autonomy puts its consequence in front of the
/// person before anything is registered; declining registers nothing. (Saying
/// yes is not exercised here: it would register with the real operating system.)
#[test]
fn background_activation_discloses_its_consequence_and_declining_registers_nothing() {
    let mut app = real_app();
    submit(&mut app, "activation enable --mode login");

    let asked = app.view().result_block.clone().expect("a question");
    assert!(app.is_awaiting_consent());
    assert!(
        asked.lines.iter().any(|l| l.contains("spend allowance")),
        "{:?}",
        asked.lines
    );
    assert!(
        asked
            .lines
            .iter()
            .any(|l| l.ends_with("--acknowledge-unattended-execution")),
        "{:?}",
        asked.lines
    );

    press(&mut app, Key::Char('n'));
    assert!(!app.is_awaiting_consent());
    assert_eq!(
        app.view().command_feedback.as_deref(),
        Some("left undone: /activation enable --mode login")
    );
}

/// Esc is a no, and while a question is open the other keys do nothing but
/// stay out of the way: Tab does not move focus away from it.
#[test]
fn escape_declines_and_other_keys_leave_the_question_open() {
    let _env = lock_portable_dir_env();
    let base = isolated_portable_dir("backup-decline");
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("creatable");
    unsafe { std::env::set_var("CRONUS_PORTABLE_DIR", &base) };

    let mut app = real_app();
    submit(&mut app, "backup create");
    assert!(app.is_awaiting_consent());
    let focus = app.view().focus;
    press(&mut app, Key::Tab);
    assert!(app.is_awaiting_consent());
    assert_eq!(app.view().focus, focus, "focus stays with the question");
    let result = app
        .tick(
            &[TermEvent::Key(Key::Esc)],
            &mut NullSource,
            &mut NullRenderer,
        )
        .expect("ticking an in-memory app never fails");
    settle(&mut app);

    unsafe { std::env::remove_var("CRONUS_PORTABLE_DIR") };
    let _ = std::fs::remove_dir_all(&base);

    assert!(!result.quit, "Esc answered the question; it did not quit");
    assert!(!app.is_awaiting_consent());
    assert_eq!(
        app.view().command_feedback.as_deref(),
        Some("left undone: /backup create")
    );
}

/// The same verb inspects without its repair flag and, with it, recomposes —
/// which is asked about, naming the relaunch it ends in; saying no repairs
/// nothing.
#[test]
fn diagnostics_with_repair_is_raised_and_asked_about_and_declining_repairs_nothing() {
    let _env = lock_portable_dir_env();
    let base = isolated_portable_dir("doctor-repair");
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("creatable");
    unsafe { std::env::set_var("CRONUS_PORTABLE_DIR", &base) };

    let mut app = real_app();
    submit(&mut app, "doctor --fix");
    let asked = app.view().result_block.clone().expect("a question");
    let waiting = app.is_awaiting_consent();
    press(&mut app, Key::Char('n'));

    unsafe { std::env::remove_var("CRONUS_PORTABLE_DIR") };
    let _ = std::fs::remove_dir_all(&base);

    assert!(waiting, "{:?}", asked.lines);
    assert!(
        asked.lines[0].contains("recompose, applied by relaunch"),
        "{:?}",
        asked.lines
    );
    assert!(
        asked
            .lines
            .iter()
            .any(|l| l == "will run: cronus doctor --fix"),
        "{:?}",
        asked.lines
    );
    assert_eq!(
        app.view().command_feedback.as_deref(),
        Some("left undone: /doctor --fix")
    );
}

/// A change that takes effect in place is asked about and, once agreed to,
/// made at once and seen afterwards in the surface's own listing — an agent
/// definition disabled is no longer offered, and enabled again it is.
#[test]
fn an_agent_definition_is_disabled_and_enabled_again_from_inside() {
    let _env = lock_portable_dir_env();
    let base = isolated_portable_dir("registry-toggle");
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("creatable");
    unsafe { std::env::set_var("CRONUS_PORTABLE_DIR", &base) };

    let mut app = real_app();
    agree(&mut app, "registry create reviewer reviews-diffs");
    let before = submit_block(&mut app, "registry list").expect("a listing");
    let disabled = agree(&mut app, "registry disable reviewer");
    let while_disabled = submit_block(&mut app, "registry list").expect("a listing");
    let enabled = agree(&mut app, "registry enable reviewer");
    let after = submit_block(&mut app, "registry list").expect("a listing");

    unsafe { std::env::remove_var("CRONUS_PORTABLE_DIR") };
    let _ = std::fs::remove_dir_all(&base);

    let offers =
        |block: &cronus_tui::ResultBlock| block.lines.iter().any(|l| l.contains("reviewer"));
    assert!(offers(&before), "{:?}", before.lines);
    assert!(
        disabled.lines.iter().any(|l| l.contains("enabled: false")),
        "{:?}",
        disabled.lines
    );
    assert!(!offers(&while_disabled), "{:?}", while_disabled.lines);
    assert!(
        enabled.lines.iter().any(|l| l.contains("enabled: true")),
        "{:?}",
        enabled.lines
    );
    assert!(offers(&after), "{:?}", after.lines);
}

/// Work running beside the change refuses it before the person is asked,
/// names what is running, and queues nothing; once the work has ended the
/// same line is asked about.
#[test]
fn a_recomposing_change_is_refused_while_work_runs_and_asked_about_once_it_has_ended() {
    let _env = lock_portable_dir_env();
    let base = isolated_portable_dir("recompose-refused");
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("creatable");
    unsafe { std::env::set_var("CRONUS_PORTABLE_DIR", &base) };

    let mut app = real_app();
    let running = cronus_core::endpoint::begin_work(
        &cronus_core::endpoint::machine_root(),
        cronus_core::endpoint::ledger::WorkKind::AgentTurn,
        "s-9",
    )
    .expect("work can begin");
    let refused = submit_block(&mut app, "registry disable reviewer").expect("an answer");
    let asked_while_running = app.is_awaiting_consent();
    drop(running);
    submit(&mut app, "registry disable reviewer");
    let asked_afterwards = app.is_awaiting_consent();
    press(&mut app, Key::Char('n'));

    unsafe { std::env::remove_var("CRONUS_PORTABLE_DIR") };
    let _ = std::fs::remove_dir_all(&base);

    assert!(
        !asked_while_running,
        "nobody is asked to agree to a refusal"
    );
    assert!(
        refused.lines[0].starts_with("not applied —"),
        "{:?}",
        refused.lines
    );
    assert!(
        refused.lines[0].contains("an agent turn (s-9)"),
        "{:?}",
        refused.lines
    );
    assert!(
        refused
            .lines
            .iter()
            .any(|l| l.contains("nothing was changed")),
        "{:?}",
        refused.lines
    );
    assert!(
        asked_afterwards,
        "the work ended, so the change can be asked about"
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
    assert!(submit_block(&mut app, "ext remove demo/x").is_some());
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

/// The Sessions screen end to end against real journals: written into an
/// isolated workspace, read through the real bootstrapped registry by the
/// production snapshot source, selected with F4, and shown as a conversation.
#[test]
fn the_sessions_screen_shows_a_real_journal_through_the_production_source() {
    let _env = lock_portable_dir_env();
    let base = isolated_portable_dir("sessions");
    // SAFETY: serialized by `PORTABLE_DIR_ENV_LOCK`.
    unsafe { std::env::set_var("CRONUS_PORTABLE_DIR", &base) };
    let root = cronus_core::paths::resolve_workspace_root();
    assert!(
        root.starts_with(&base),
        "the workspace root resolved to {root:?}, outside the isolated directory {base:?}"
    );
    let sessions = cronus_core::session::journal::sessions_dir(&root);
    std::fs::create_dir_all(&sessions).expect("creatable");
    std::fs::write(
        sessions.join("s-1.jsonl"),
        concat!(
            "{\"type\":\"session\",\"version\":3,\"createdAt\":1700000000}\n",
            "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"what blocks the card?\"}}\n",
            "{\"type\":\"message\",\"message\":{\"role\":\"assistant\",\"content\":\"the gate is red\"}}\n",
        ),
    )
    .expect("writable");

    let mut app = real_app();
    let mut source = cronus_tui::CapabilitySource::new(cronus_core::Engine::new());
    let mut tick = |app: &mut App, key: Option<Key>| {
        let events: Vec<TermEvent> = key.into_iter().map(TermEvent::Key).collect();
        app.tick(&events, &mut source, &mut NullRenderer)
            .expect("ticking never fails");
    };
    // The production source reads on a thread of its own: tick until the
    // sessions list has arrived, then until the conversation has.
    let arrived = |app: &App| {
        matches!(
            &app.view().snapshot.session_list,
            cronus_tui::Projection::Available(list) if !list.rows.is_empty()
        )
    };
    for _ in 0..2000 {
        if arrived(&app) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
        tick(&mut app, None);
    }
    tick(&mut app, Some(Key::F(4)));
    for _ in 0..2000 {
        if !app.is_busy() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
        tick(&mut app, None);
    }
    let transcript = app.view().transcript.clone();

    unsafe { std::env::remove_var("CRONUS_PORTABLE_DIR") };
    let _ = std::fs::remove_dir_all(&base);

    assert_eq!(app.view().screen, cronus_tui::Screen::Sessions);
    let Some((_, cronus_tui::Projection::Available(conversation))) = transcript else {
        panic!("the selected session's conversation is read: {transcript:?}");
    };
    assert_eq!(conversation.id, "s-1");
    assert_eq!(
        conversation.messages,
        vec![
            ("user".to_string(), "what blocks the card?".to_string()),
            ("assistant".to_string(), "the gate is red".to_string()),
        ]
    );
}

/// A conversation end to end: the Chat screen sends a message through the
/// real bootstrapped `session say`, a mock model server answers, the reply is
/// journaled, and the screen reads the conversation back from the journal.
#[test]
fn the_chat_screen_talks_to_a_model_and_shows_the_journaled_conversation() {
    use std::io::{Read, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let url = format!(
        "http://127.0.0.1:{}",
        listener.local_addr().expect("addr").port()
    );
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut acc = Vec::new();
        let mut buf = [0u8; 4096];
        while !acc.windows(4).any(|w| w == b"\r\n\r\n") {
            match stream.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => acc.extend_from_slice(&buf[..n]),
            }
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n")
            .expect("preamble");
        for token in ["the gate ", "is red"] {
            write!(
                stream,
                "data: {{\"choices\":[{{\"delta\":{{\"content\":\"{token}\"}}}}]}}\n\n"
            )
            .expect("token");
        }
        stream.write_all(b"data: [DONE]\n\n").expect("done");
    });

    let _env = lock_portable_dir_env();
    let base = isolated_portable_dir("chat");
    // SAFETY: serialized by `PORTABLE_DIR_ENV_LOCK`.
    unsafe {
        std::env::set_var("CRONUS_PORTABLE_DIR", &base);
        std::env::set_var("CRONUS_CHAT_ENDPOINT", &url);
    }
    assert!(
        cronus_core::paths::resolve_workspace_root().starts_with(&base),
        "the workspace root must resolve inside the isolated directory"
    );

    let mut app = real_app();
    let tick = |app: &mut App, keys: Vec<Key>| {
        let events: Vec<TermEvent> = keys.into_iter().map(TermEvent::Key).collect();
        app.tick(&events, &mut NullSource, &mut NullRenderer)
            .expect("ticking never fails");
    };
    let typed = |line: &str| -> Vec<Key> {
        line.chars()
            .map(Key::Char)
            .chain(std::iter::once(Key::Enter))
            .collect()
    };
    tick(&mut app, vec![Key::F(3)]);
    tick(&mut app, typed("/chat model any-model"));
    tick(&mut app, typed("what blocks the card?"));
    for _ in 0..400 {
        if app.view().chat.pending.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        tick(&mut app, Vec::new());
    }
    let chat = app.view().chat.clone();

    unsafe {
        std::env::remove_var("CRONUS_PORTABLE_DIR");
        std::env::remove_var("CRONUS_CHAT_ENDPOINT");
    }
    let _ = std::fs::remove_dir_all(&base);
    server.join().expect("server thread");

    assert_eq!(chat.pending, None, "the reply arrived");
    let Some(cronus_tui::Projection::Available(conversation)) = chat.transcript else {
        panic!(
            "the conversation is read back from the journal: {:?}",
            chat.session
        );
    };
    assert_eq!(
        conversation.messages,
        vec![
            ("user".to_string(), "what blocks the card?".to_string()),
            ("assistant".to_string(), "the gate is red".to_string()),
        ]
    );
}
