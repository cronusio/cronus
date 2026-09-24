//! The event-driven render loop and the view-model it drives.
//!
//! The loop is split into a pure step function ([`App::tick`]) and a thin driver
//! ([`run_with`]) that feeds it terminal events. This keeps the loop's logic
//! testable without a TTY: tests call `tick` directly with scripted events and a
//! stub state source.
//!
//! [`App`] holds nothing but a [`ViewModel`] snapshot and a
//! view-local quit flag. All durable state lives in the core and is read through
//! the [`SnapshotSource`] seam — the loop never mutates domain state.
//!
//! Command dispatch (`crate::dispatch`) goes through the real, shared
//! `cronus_core::invocable::Dispatcher` — the same one every surface uses —
//! never a local re-implementation. `App` holds the registry and dispatcher
//! directly rather than behind a trait object: there is exactly one dispatch
//! mechanism now, not a swappable one, so the indirection a trait bought
//! before this migration buys nothing here.

use std::io::{self, Stdout};
use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend as RatatuiCrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::{Paragraph, Widget};

use crate::command::{self, CommandOutcome, CommandSpec, SlashCommand};
use crate::dispatch;
use crate::pane_actions::{self, PaneAction};
use crate::terminal::{CrosstermBackend, Key, TermEvent, TerminalBackend, Tui};
use crate::view::{self, BoardView, Focus, OfficeView, Projection, SessionsView, StatusView};
use cronus_contract::{
    ArgValue, ArgValues, Dispatched, Invocable, InvocableId, Invocation, Outcome, OutcomeValue,
    Surface,
};
use cronus_core::invocable::{Dispatcher, InvocableRegistry};
use cronus_domain::{Capabilities, Engine};

/// How long the loop blocks for input before ticking again. Bounds redraw
/// latency for state changes that arrive without a terminal event.
const TICK: Duration = Duration::from_millis(50);

/// An immutable projection of durable core state at one instant.
///
/// The TUI renders from this and never mutates it. Every field is a
/// [`Projection`]: `status` comes from the core capability surface;
/// `board`/`office` are dispatched through the shared registry (`core:board
/// .list`/`core:role.list`) — the same door the command bar uses — never a
/// path opened directly by this crate (which would re-derive the domain fact
/// of where the board/roster live, the exact class of defect finding F-4
/// already names on the sibling surface). `sessions` stays the neutral,
/// genuinely-empty default until the core exposes a durable activity log to
/// read from.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CoreSnapshot {
    /// Engine/product version + status line.
    pub status: Projection<StatusView>,
    /// Kanban board projection (cards by column).
    pub board: Projection<BoardView>,
    /// Office projection (agents and their current tasks).
    pub office: Projection<OfficeView>,
    /// Sessions/log projection (bounded tail of recent activity).
    pub sessions: Projection<SessionsView>,
}

/// View-only state the panels render from.
///
/// A pure function of the last [`CoreSnapshot`] plus terminal-local view state
/// (currently the size). It accumulates no domain state, so feeding the same
/// snapshot repeatedly is idempotent.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ViewModel {
    /// The latest core snapshot the view reflects.
    pub snapshot: CoreSnapshot,
    /// Terminal size as `(cols, rows)`.
    pub size: (u16, u16),
    /// Which panel currently has keyboard focus. View state, not domain state.
    pub focus: Focus,
    /// In-progress command-bar text (the part after the `/` prompt). View state.
    pub command_input: String,
    /// The result of the last submitted command (help summary / error / ack),
    /// shown beside the prompt until the next keystroke.
    pub command_feedback: Option<String>,
}

/// The seam the loop reads core state through.
///
/// The core exposes no event/observe bus yet, so the production source
/// ([`CapabilitySource`]) *polls* a fresh snapshot each tick — the poll-snapshot
/// fallback the spec mandates. An event-driven source can implement the same
/// trait unchanged if the core later grows a subscription. Takes the loop's own
/// `registry`/`dispatcher` so a real source can dispatch `board`/`office`
/// reads through the identical mechanism the command bar uses — one dispatch
/// mechanism, never a second, direct path into the domain tier.
pub trait SnapshotSource {
    /// Return the latest snapshot if one is available, else `None`.
    ///
    /// `None` models a snapshot still being produced (a slow core call in
    /// flight). The loop must stay responsive to input when this happens.
    fn poll_snapshot(
        &mut self,
        registry: &InvocableRegistry,
        dispatcher: &Dispatcher,
    ) -> Option<CoreSnapshot>;
}

/// Draws a single frame from the immutable view-model.
///
/// Taking `&ViewModel` (never `&mut`) enforces render-from-state: a renderer
/// cannot smuggle mutable state back into the loop.
pub trait Renderer {
    /// Render one frame. Called at most once per [`App::tick`].
    fn draw(&mut self, view: &ViewModel) -> io::Result<()>;
}

/// Outcome of a single [`App::tick`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickResult {
    /// Whether a frame was drawn this tick.
    pub redrawn: bool,
    /// Whether the loop should exit after this tick.
    pub quit: bool,
}

/// The render loop's state: the view-model, a quit flag, and the command-dispatch
/// machinery. Still no domain state — the registry/dispatcher pair reaches the
/// core only through the same public composition door every surface uses, and
/// redaction happens once, inside the shared dispatcher, never here.
pub struct App {
    view: ViewModel,
    should_quit: bool,
    /// A command submitted this tick, awaiting dispatch by the loop.
    pending_dispatch: Option<SlashCommand>,
    /// The shared invocable catalog — resolves a slash command's candidate
    /// identity and its declared binders before dispatch.
    registry: InvocableRegistry,
    /// Runs a resolved invocation for real, masking secrets at the boundary.
    dispatcher: Dispatcher,
    /// The slash-command catalog, built once at construction from the core's
    /// invocable registry (`command::build_catalog`) — never a hand-maintained
    /// list. Held on `App` rather than re-derived per keystroke, since it
    /// changes only if the registry itself does.
    catalog: Vec<CommandSpec>,
}

impl App {
    /// Construct an app over a real (or test-constructed) registry/dispatcher
    /// pair and catalog. An empty registry with nothing attached is a valid,
    /// inert choice for scenarios where dispatch behavior is irrelevant —
    /// every slash command then resolves to `Dispatched::Unknown`, exactly
    /// as a genuinely unbound verb would.
    pub fn new(
        initial: ViewModel,
        registry: InvocableRegistry,
        dispatcher: Dispatcher,
        catalog: Vec<CommandSpec>,
    ) -> Self {
        Self {
            view: initial,
            should_quit: false,
            pending_dispatch: None,
            registry,
            dispatcher,
            catalog,
        }
    }

    /// The current view-model the panels render from.
    pub fn view(&self) -> &ViewModel {
        &self.view
    }

    /// Process one loop iteration.
    ///
    /// Order matters: input is handled first so a pending snapshot (a slow core
    /// call) can never starve the keyboard. The snapshot is then pulled cheaply;
    /// if it changed, the view-model updates. Exactly one redraw is issued when
    /// anything changed this tick.
    pub fn tick<S: SnapshotSource, R: Renderer>(
        &mut self,
        events: &[TermEvent],
        source: &mut S,
        renderer: &mut R,
    ) -> io::Result<TickResult> {
        let mut needs_redraw = false;

        // 1) Input first — never blocked by snapshot work.
        for event in events {
            match event {
                TermEvent::Resize(cols, rows) => {
                    self.view.size = (*cols, *rows);
                    needs_redraw = true;
                }
                TermEvent::Key(key) => {
                    if self.handle_key(*key) {
                        needs_redraw = true;
                    }
                }
            }
        }

        // 1b) Dispatch a submitted command through the shared registry/dispatcher.
        //     Resolution, binding, dispatch, and secret masking all happen inside
        //     `dispatch::dispatch_command` — no raw secret value reaches the
        //     view-model or the screen buffer. `None` means the line resolved
        //     to no invocable (`Dispatched::Unknown`) — ordinary input, not a
        //     rendered failure (l2-tui v1.2.0), so feedback is cleared exactly
        //     as it would be for any other line with nothing to report.
        //
        //     A line naming one of this surface's own pane actions (e.g.
        //     `/pane focus-next`) is routed through `dispatch_pane_action`
        //     instead: that is the one place a real `Dispatched::Ran` answer
        //     is turned into the actual view mutation (focus change, quit),
        //     the same effect a direct Tab/Esc key press applies. Rendering
        //     only the dispatch's own stateless `Outcome::Value(Empty)` text
        //     — the prior behavior — left the command-bar form of `/pane`
        //     dispatching successfully while doing nothing a user could see.
        if let Some(command) = self.pending_dispatch.take() {
            let pane_action = command
                .args
                .first()
                .and_then(|sub_verb| PaneAction::from_slash(&command.verb, sub_verb));
            match pane_action {
                Some(action) => {
                    self.dispatch_pane_action(action);
                    self.view.command_feedback = None;
                }
                None => {
                    self.view.command_feedback =
                        dispatch::dispatch_command(&self.registry, &self.dispatcher, &command);
                }
            }
            needs_redraw = true;
        }

        // 2) Snapshot poll — may be `None` (slow call in flight); the loop has
        //    already handled input above, so it stays responsive regardless.
        //    Passes this loop's own registry/dispatcher through so a real
        //    source dispatches board/office reads the same way the command
        //    bar does — never a second, direct path into the domain tier.
        if let Some(snapshot) = source.poll_snapshot(&self.registry, &self.dispatcher)
            && snapshot != self.view.snapshot
        {
            self.view.snapshot = snapshot;
            needs_redraw = true;
        }

        // 3) Coalesce to exactly one redraw per changed tick.
        if needs_redraw {
            renderer.draw(&self.view)?;
        }

        Ok(TickResult {
            redrawn: needs_redraw,
            quit: self.should_quit,
        })
    }

    /// Route a key press by the focused region; returns whether a redraw is
    /// needed. Tab / Shift+Tab cycle focus from anywhere, and `/` jumps
    /// straight to the command bar from any other panel. Outside the command
    /// bar, Esc quits; inside it, keys edit the command line (Esc cancels, and
    /// `/` is ordinary text).
    ///
    /// Which key triggers which action is decided right here — key bindings
    /// stay local presentation. What does NOT
    /// stay local is what each action *is*: every one of these is a real
    /// `ClientLocal` invocable ([`pane_actions`]), and this match only ever
    /// names which one a key requests, never what running it does.
    fn handle_key(&mut self, key: Key) -> bool {
        match key {
            Key::Tab => self.dispatch_pane_action(PaneAction::FocusNext),
            Key::BackTab => self.dispatch_pane_action(PaneAction::FocusPrev),
            Key::Char('/') if self.view.focus != Focus::CommandBar => {
                self.dispatch_pane_action(PaneAction::FocusCommandBar)
            }
            _ if self.view.focus == Focus::CommandBar => self.handle_command_key(key),
            Key::Esc => self.dispatch_pane_action(PaneAction::Quit),
            _ => false,
        }
    }

    /// Dispatch one of this surface's own actions through the shared
    /// registry/dispatcher — the only path from a key press to a view
    /// mutation. `action` is applied only once the real dispatch answers
    /// `Dispatched::Ran`; an action that never registered (or lost its
    /// handler) answers `Dispatched::Unknown`, exactly like a genuinely
    /// unbound verb, and nothing happens here — there is no second,
    /// key-code-to-behaviour path that could run anyway.
    fn dispatch_pane_action(&mut self, action: PaneAction) -> bool {
        let invocation = Invocation {
            id: action.id(),
            args: ArgValues::new(),
            caller: Surface::Tui,
        };
        match self.dispatcher.dispatch(&self.registry, &invocation) {
            Dispatched::Unknown => false,
            Dispatched::Ran(_outcome) => match action {
                PaneAction::FocusNext => {
                    self.view.focus = self.view.focus.next();
                    true
                }
                PaneAction::FocusPrev => {
                    self.view.focus = self.view.focus.prev();
                    true
                }
                PaneAction::FocusCommandBar => {
                    self.view.focus = Focus::CommandBar;
                    true
                }
                PaneAction::Quit => {
                    self.should_quit = true;
                    false
                }
            },
        }
    }

    /// Command-bar text entry. Esc cancels the in-progress line rather than
    /// quitting the app.
    fn handle_command_key(&mut self, key: Key) -> bool {
        match key {
            Key::Char(c) => {
                self.view.command_input.push(c);
                self.view.command_feedback = None;
                true
            }
            Key::Backspace => {
                self.view.command_input.pop();
                self.view.command_feedback = None;
                true
            }
            Key::Enter => {
                self.submit_command();
                true
            }
            Key::Esc => {
                self.view.command_input.clear();
                self.view.command_feedback = None;
                true
            }
            _ => false,
        }
    }

    /// Classify the current command line. Help and errors resolve inline; a
    /// recognized command is queued for the loop to dispatch (the loop holds the
    /// registry/dispatcher and performs the real call + masking).
    fn submit_command(&mut self) {
        let line = format!("/{}", self.view.command_input);
        match command::classify(&line, &self.catalog) {
            CommandOutcome::Help => {
                self.view.command_feedback = Some(format!(
                    "commands: {}",
                    command::names(&self.catalog).collect::<Vec<_>>().join(" ")
                ));
            }
            CommandOutcome::Run(command) => self.pending_dispatch = Some(command),
            CommandOutcome::Error(message) => self.view.command_feedback = Some(message),
        }
        self.view.command_input.clear();
    }
}

/// Production [`SnapshotSource`] over the core capability surface.
///
/// Reads `version`/`status` each tick — a cheap call — and hands the loop a fresh
/// snapshot every time; [`App::tick`] dedupes by equality so identical reads do
/// not trigger redraws. Heavier future snapshots can be produced off-thread and
/// delivered through this same poll seam without changing the loop.
pub struct CapabilitySource<C: Capabilities> {
    core: C,
}

impl<C: Capabilities> CapabilitySource<C> {
    /// Wrap a core handle as a snapshot source.
    pub fn new(core: C) -> Self {
        Self { core }
    }
}

impl<C: Capabilities> SnapshotSource for CapabilitySource<C> {
    fn poll_snapshot(
        &mut self,
        registry: &InvocableRegistry,
        dispatcher: &Dispatcher,
    ) -> Option<CoreSnapshot> {
        Some(CoreSnapshot {
            status: Projection::Available(StatusView {
                version: self.core.version().to_string(),
                status: self.core.status(),
            }),
            board: dispatch_board(registry, dispatcher),
            office: dispatch_office(registry, dispatcher),
            // No durable activity log exists in the domain tier yet to read
            // from — the neutral, genuinely-empty starting point, never a
            // fabricated `Unavailable` with no real call behind it.
            sessions: Projection::Available(SessionsView::default()),
        })
    }
}

/// Dispatch `core:board.list` through the shared registry/dispatcher and
/// project its result — never a store opened directly by this crate, which
/// would re-derive the domain fact of where the board lives (finding F-4's
/// exact defect, on the sibling surface). Two reads: the live cards, then the
/// archived ones (`--archived`) that the separate archive store holds and the
/// Archive column shows. Either read being unavailable makes the whole board
/// unavailable with its reason — the panel is one projection — but a single
/// live card in a state this surface has no column for does not: it is set
/// aside by id in `unmapped` so the cards it can place still render.
fn dispatch_board(registry: &InvocableRegistry, dispatcher: &Dispatcher) -> Projection<BoardView> {
    let live = dispatch_projection(
        registry,
        dispatcher,
        "core:board.list",
        ArgValues::new(),
        |value| {
            let OutcomeValue::List(items) = value else {
                return None;
            };
            let mut cards = Vec::with_capacity(items.len());
            let mut unmapped = Vec::new();
            for item in items {
                let OutcomeValue::Record(fields) = item else {
                    return None;
                };
                let id = record_text(fields, "id")?;
                match board_column(record_text(fields, "state")?) {
                    Some(column) => cards.push(view::BoardCard {
                        id: id.to_string(),
                        // `core:board.list`'s current outcome shape carries no
                        // `task_ref` field — a disclosed limitation of that
                        // invocable's response, not something this panel
                        // derives locally; the same gap the command line's
                        // own output has.
                        title: String::new(),
                        column,
                    }),
                    None => unmapped.push(id.to_string()),
                }
            }
            Some(BoardView { cards, unmapped })
        },
    );
    let Projection::Available(mut board) = live else {
        return live;
    };

    let mut archived_args = ArgValues::new();
    archived_args.insert("archived", ArgValue::Flag);
    let archived = dispatch_projection(
        registry,
        dispatcher,
        "core:board.list",
        archived_args,
        |value| {
            let OutcomeValue::List(items) = value else {
                return None;
            };
            let mut ids = Vec::with_capacity(items.len());
            for item in items {
                let OutcomeValue::Record(fields) = item else {
                    return None;
                };
                ids.push(record_text(fields, "id")?.to_string());
            }
            Some(ids)
        },
    );
    match archived {
        Projection::Available(ids) => {
            board
                .cards
                .extend(ids.into_iter().map(|id| view::BoardCard {
                    id,
                    title: String::new(),
                    column: view::BoardColumn::Archive,
                }));
            Projection::Available(board)
        }
        Projection::Unavailable { reason } => Projection::Unavailable {
            reason: format!("archive: {reason}"),
        },
    }
}

/// Dispatch `core:role.list` (no `--presets` flag: hired instances, not the
/// preset catalog) and project its result the same way [`dispatch_board`]
/// does for the board.
fn dispatch_office(
    registry: &InvocableRegistry,
    dispatcher: &Dispatcher,
) -> Projection<OfficeView> {
    dispatch_projection(
        registry,
        dispatcher,
        "core:role.list",
        ArgValues::new(),
        |value| {
            let OutcomeValue::List(items) = value else {
                return None;
            };
            let mut agents = Vec::with_capacity(items.len());
            for item in items {
                let OutcomeValue::Record(fields) = item else {
                    return None;
                };
                agents.push(view::AgentActivity {
                    agent: record_text(fields, "display_name")?.to_string(),
                    // No live task-tracking exists in the domain layer yet
                    // (`HiredInstance` carries no "current task" field) — an
                    // honest absence, not a locally-derived default; the
                    // panel's own "idle" fallback already renders this
                    // correctly.
                    task: String::new(),
                });
            }
            Some(OfficeView { agents })
        },
    )
}

/// Dispatch one invocable and turn its result into a
/// [`Projection`]: a registry miss or a real `Outcome::Unavailable`/
/// `Rejected`/`Stream` all become `Unavailable` with a legible reason; a
/// `Value` that does not match `parse`'s expected shape is *also*
/// `Unavailable`, never silently treated as empty — an unrecognized shape is
/// exactly the case this crate must not paper over as "nothing to show".
fn dispatch_projection<T>(
    registry: &InvocableRegistry,
    dispatcher: &Dispatcher,
    id: &str,
    args: ArgValues,
    parse: impl FnOnce(&OutcomeValue) -> Option<T>,
) -> Projection<T> {
    let invocation = Invocation {
        id: InvocableId::new(id).expect("a literal invocable id must be well-formed"),
        args,
        caller: Surface::Tui,
    };
    match dispatcher.dispatch(registry, &invocation) {
        Dispatched::Unknown => Projection::Unavailable {
            reason: format!("{id} is not registered"),
        },
        Dispatched::Ran(Outcome::Value(value)) => match parse(&value) {
            Some(parsed) => Projection::Available(parsed),
            None => Projection::Unavailable {
                reason: format!("{id} returned an unrecognized result shape"),
            },
        },
        Dispatched::Ran(Outcome::Rejected(rejection)) => Projection::Unavailable {
            reason: format!(
                "{id} rejected its own call: {} ({:?})",
                rejection.binder, rejection.mode
            ),
        },
        Dispatched::Ran(Outcome::Unavailable { reason }) => Projection::Unavailable { reason },
        Dispatched::Ran(Outcome::Stream(_)) => Projection::Unavailable {
            reason: format!(
                "{id} streamed a result; this surface has no panel renderer for it yet"
            ),
        },
    }
}

/// The inverse of the domain's own `CardState::as_str()` — this crate names
/// no domain type of its own, it only recognizes the same strings
/// `core:board.list`'s handler already emits.
fn board_column(state: &str) -> Option<view::BoardColumn> {
    match state {
        "triage" => Some(view::BoardColumn::Triage),
        "todo" => Some(view::BoardColumn::Todo),
        "ready" => Some(view::BoardColumn::Ready),
        "running" => Some(view::BoardColumn::Running),
        "blocked" => Some(view::BoardColumn::Blocked),
        "done" => Some(view::BoardColumn::Done),
        _ => None,
    }
}

/// Read one named `Text` field out of an `Outcome::Value(Record(..))`'s
/// field list.
fn record_text<'a>(fields: &'a [(String, OutcomeValue)], name: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|(k, _)| k == name)
        .and_then(|(_, v)| match v {
            OutcomeValue::Text(s) => Some(s.as_str()),
            _ => None,
        })
}

/// Production [`Renderer`] drawing the panel layout via ratatui.
///
/// Holds a ratatui terminal over the same crossterm version the [`Tui`] guard
/// drives, so the two share one alternate screen: the guard owns the raw-mode
/// lifecycle while ratatui owns frame diffing. Panels render purely from the
/// view-model; content for each panel arrives in the panel tracks — this
/// renderer establishes the bordered skeleton and the focus highlight.
pub struct RatatuiRenderer {
    terminal: Terminal<RatatuiCrosstermBackend<Stdout>>,
}

impl RatatuiRenderer {
    /// Construct a renderer drawing to the process stdout.
    pub fn new() -> io::Result<Self> {
        let backend = RatatuiCrosstermBackend::new(io::stdout());
        Ok(Self {
            terminal: Terminal::new(backend)?,
        })
    }
}

impl Renderer for RatatuiRenderer {
    fn draw(&mut self, view: &ViewModel) -> io::Result<()> {
        self.terminal
            .draw(|frame| render_view(frame.area(), frame.buffer_mut(), view))?;
        Ok(())
    }
}

/// Render the whole TUI frame into `buf` for `area`, purely from the view-model.
///
/// A pure function of `view`: the same view-model always produces the same buffer
/// (render-from-state). Extracted from the renderer so the render path can
/// be exercised against an off-screen [`Buffer`] without a terminal.
pub fn render_view(area: Rect, buf: &mut Buffer, view: &ViewModel) {
    let areas = view::layout(area);
    let snapshot = &view.snapshot;

    view::render_board(
        areas.board,
        buf,
        &snapshot.board,
        view.focus == Focus::Board,
    );
    view::render_office(
        areas.office,
        buf,
        &snapshot.office,
        view.focus == Focus::Office,
    );
    view::render_status(
        areas.status,
        buf,
        &snapshot.status,
        view.focus == Focus::Status,
    );
    view::render_sessions(
        areas.sessions,
        buf,
        &snapshot.sessions,
        view.focus == Focus::Sessions,
    );

    let bar_text = match &view.command_feedback {
        Some(feedback) if view.command_input.is_empty() => format!("/  {feedback}"),
        _ => format!("/{}", view.command_input),
    };
    Paragraph::new(bar_text)
        .style(view::focus_border_style(view.focus == Focus::CommandBar))
        .render(areas.command_bar, buf);
}

/// Run the TUI against the real terminal and the live engine.
pub fn run() -> io::Result<()> {
    // The same public composition door every surface uses
    // (`invocable_bootstrap::bootstrap`) — no private construction path.
    // Secret values to mask are loaded from the core secrets store once that
    // binding lands; the dispatcher is left with none for now, the same
    // disclosed residual the sibling CLI frontend's own composition carries.
    let (mut registry, mut dispatcher) =
        cronus_core::invocable_bootstrap::bootstrap(cronus_core::Engine::new());
    // This surface's own actions register through the same door, before the
    // catalog is built from the result — so `/pane` is a real discoverable
    // group, not a parallel mechanism the catalog never learns about.
    pane_actions::register(&mut registry, &mut dispatcher);
    let invocables: Vec<&Invocable> = registry.all().collect();
    let catalog = command::build_catalog(&invocables);

    run_with(
        CrosstermBackend::new(),
        CapabilitySource::new(Engine::new()),
        RatatuiRenderer::new()?,
        registry,
        dispatcher,
        catalog,
    )
}

/// Run the loop against injected backend / source / renderer / registry /
/// dispatcher.
///
/// The terminal lifecycle is RAII-guarded by [`Tui`], so any exit path — normal,
/// `?` error, or panic — restores the terminal.
pub fn run_with<B, S, R>(
    backend: B,
    mut source: S,
    mut renderer: R,
    registry: InvocableRegistry,
    dispatcher: Dispatcher,
    catalog: Vec<CommandSpec>,
) -> io::Result<()>
where
    B: TerminalBackend,
    S: SnapshotSource,
    R: Renderer,
{
    let mut tui = Tui::new(backend)?;
    let (cols, rows) = tui.size()?;
    let mut app = App::new(
        ViewModel {
            size: (cols, rows),
            ..Default::default()
        },
        registry,
        dispatcher,
        catalog,
    );

    // Initial frame so the screen is populated before the first event.
    renderer.draw(app.view())?;

    loop {
        let mut events = Vec::new();
        if let Some(event) = tui.poll_event(TICK)? {
            events.push(event);
        }
        let result = app.tick(&events, &mut source, &mut renderer)?;
        if result.quit {
            break;
        }
    }

    tui.restore()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Arc;

    use cronus_contract::{
        ArgValue, Binder, BinderKind, InvocableId, Locus, Outcome, OutcomeValue, Stability,
    };
    use cronus_core::invocable::Registrant;

    /// A scripted [`SnapshotSource`]: returns each queued value in turn, so tests
    /// can model "new snapshot ready" (`Some`) and "slow call in flight" (`None`).
    struct ScriptedSource {
        queue: VecDeque<Option<CoreSnapshot>>,
    }

    impl ScriptedSource {
        fn new(script: Vec<Option<CoreSnapshot>>) -> Self {
            Self {
                queue: script.into(),
            }
        }
    }

    impl SnapshotSource for ScriptedSource {
        fn poll_snapshot(
            &mut self,
            _registry: &InvocableRegistry,
            _dispatcher: &Dispatcher,
        ) -> Option<CoreSnapshot> {
            self.queue.pop_front().flatten()
        }
    }

    /// Records every frame drawn so tests can count redraws and inspect content.
    #[derive(Default)]
    struct RecordingRenderer {
        frames: Vec<ViewModel>,
    }

    impl Renderer for RecordingRenderer {
        fn draw(&mut self, view: &ViewModel) -> io::Result<()> {
            self.frames.push(view.clone());
            Ok(())
        }
    }

    fn snap(status: &str) -> CoreSnapshot {
        CoreSnapshot {
            status: Projection::Available(StatusView {
                version: "0.1.0".to_string(),
                status: status.to_string(),
            }),
            ..CoreSnapshot::default()
        }
    }

    /// An empty registry/dispatcher pair — a valid, inert choice for tests
    /// where dispatch behavior is irrelevant: any slash command resolves to
    /// `Dispatched::Unknown`, exactly as a genuinely unbound verb would.
    fn empty_registry_and_dispatcher() -> (InvocableRegistry, Dispatcher) {
        (InvocableRegistry::new(), Dispatcher::new())
    }

    /// The real pane-action registration production `run()` performs — used
    /// by every test that drives Tab/BackTab/Esc and needs the real dispatch
    /// path behind them to actually resolve, not merely the key press.
    fn registry_with_pane_actions() -> (InvocableRegistry, Dispatcher) {
        let mut registry = InvocableRegistry::new();
        let mut dispatcher = Dispatcher::new();
        pane_actions::register(&mut registry, &mut dispatcher);
        (registry, dispatcher)
    }

    /// A minimal catalog naming `test` as a known verb — used by tests
    /// exercising dispatch *wiring* end to end (does a recognized command
    /// reach the registry, bind its args, and dispatch for real), not by
    /// anything asserting on catalog derivation itself (see `command.rs`'s
    /// own `build_catalog` tests for that).
    fn test_catalog() -> Vec<CommandSpec> {
        vec![CommandSpec {
            name: "test",
            summary: "test-only catalog entry".to_string(),
        }]
    }

    /// A real, test-registered `core:test.probe` invocable, one required
    /// `Text` binder, echoing it back as `probe: {value}` — proves dispatch
    /// wiring end to end without depending on any real domain subsystem.
    fn registry_with_probe() -> (InvocableRegistry, Dispatcher) {
        let mut registry = InvocableRegistry::new();
        let mut dispatcher = Dispatcher::new();
        let id = InvocableId::new("core:test.probe").expect("well-formed test id");
        registry
            .register(
                &Registrant::core(),
                Invocable {
                    id: id.clone(),
                    name: "Probe",
                    summary: "Test-only probe invocable.",
                    group: "test",
                    locus: Locus::Semantic,
                    binders: vec![Binder {
                        name: "value",
                        kind: BinderKind::Text,
                        optional: false,
                    }],
                    stability: Stability::Shipped,
                    journal_raw_input: true,
                },
            )
            .expect("test fixture registers cleanly");
        dispatcher.attach(
            id,
            Arc::new(|args| {
                let echoed = match args.get("value") {
                    Some(ArgValue::Text(s)) => s.clone(),
                    _ => String::new(),
                };
                Outcome::Value(OutcomeValue::Text(format!("probe: {echoed}")))
            }),
        );
        (registry, dispatcher)
    }

    /// The same probe invocable, taking no arguments and echoing back a
    /// value containing `secret` in its raw, unmasked output — the
    /// dispatcher's own secret list (set via `set_secrets`) is what should
    /// mask it, never anything in this crate.
    fn registry_with_secret_probe(secret: &str) -> (InvocableRegistry, Dispatcher) {
        let mut registry = InvocableRegistry::new();
        let mut dispatcher = Dispatcher::new();
        let id = InvocableId::new("core:test.probe").expect("well-formed test id");
        registry
            .register(
                &Registrant::core(),
                Invocable {
                    id: id.clone(),
                    name: "Probe",
                    summary: "Test-only probe invocable.",
                    group: "test",
                    locus: Locus::Semantic,
                    binders: Vec::new(),
                    stability: Stability::Shipped,
                    journal_raw_input: true,
                },
            )
            .expect("test fixture registers cleanly");
        let secret_owned = secret.to_string();
        dispatcher.attach(
            id,
            Arc::new(move |_args| {
                Outcome::Value(OutcomeValue::Text(format!("key={secret_owned}")))
            }),
        );
        dispatcher.set_secrets(vec![secret.to_string()]);
        (registry, dispatcher)
    }

    /// The same probe identity, declaring one required binder but never
    /// attaching a handler for it — dispatch always rejects before any
    /// handler could run, letting a test drive a real `Rejected` outcome
    /// end to end through the app's tick loop.
    fn registry_with_required_binder() -> (InvocableRegistry, Dispatcher) {
        let mut registry = InvocableRegistry::new();
        let dispatcher = Dispatcher::new();
        let id = InvocableId::new("core:test.probe").expect("well-formed test id");
        registry
            .register(
                &Registrant::core(),
                Invocable {
                    id,
                    name: "Probe",
                    summary: "Test-only probe invocable.",
                    group: "test",
                    locus: Locus::Semantic,
                    binders: vec![Binder {
                        name: "value",
                        kind: BinderKind::Text,
                        optional: false,
                    }],
                    stability: Stability::Shipped,
                    journal_raw_input: true,
                },
            )
            .expect("test fixture registers cleanly");
        (registry, dispatcher)
    }

    #[test]
    fn render_loop_state_change_schedules_exactly_one_redraw() {
        let (registry, dispatcher) = empty_registry_and_dispatcher();
        let mut app = App::new(ViewModel::default(), registry, dispatcher, Vec::new());
        let mut source = ScriptedSource::new(vec![Some(snap("running"))]);
        let mut renderer = RecordingRenderer::default();

        let result = app
            .tick(&[], &mut source, &mut renderer)
            .expect("tick succeeds");

        assert!(result.redrawn, "a state change must redraw");
        assert_eq!(renderer.frames.len(), 1, "exactly one redraw per change");
        assert_eq!(app.view().snapshot, snap("running"), "view-model updated");
    }

    #[test]
    fn render_loop_unchanged_snapshot_does_not_redraw() {
        let (registry, dispatcher) = empty_registry_and_dispatcher();
        let mut app = App::new(ViewModel::default(), registry, dispatcher, Vec::new());
        // Same snapshot delivered twice across two ticks.
        let mut source = ScriptedSource::new(vec![Some(snap("idle")), Some(snap("idle"))]);
        let mut renderer = RecordingRenderer::default();

        let first = app.tick(&[], &mut source, &mut renderer).unwrap();
        let second = app.tick(&[], &mut source, &mut renderer).unwrap();

        assert!(first.redrawn, "first snapshot is a change");
        assert!(!second.redrawn, "identical snapshot must not redraw");
        assert_eq!(renderer.frames.len(), 1, "no redundant frame");
    }

    #[test]
    fn render_loop_stays_responsive_while_core_call_is_slow() {
        let (registry, dispatcher) = registry_with_pane_actions();
        let mut app = App::new(ViewModel::default(), registry, dispatcher, Vec::new());
        // `None` models a snapshot still being produced — a slow core call.
        let mut source = ScriptedSource::new(vec![None]);
        let mut renderer = RecordingRenderer::default();

        // An Esc keypress arrives during the slow call.
        let result = app
            .tick(&[TermEvent::Key(Key::Esc)], &mut source, &mut renderer)
            .expect("tick succeeds");

        assert!(
            result.quit,
            "input must be handled even when no snapshot is ready"
        );
    }

    /// The other half of the same criterion pane dispatch introduces: with
    /// no pane actions registered at all, Esc must NOT quit — there is no
    /// second, direct key-code-to-behaviour path that could run once the
    /// real dispatch answers `Dispatched::Unknown`.
    #[test]
    fn an_unregistered_quit_action_never_quits_the_app() {
        let (registry, dispatcher) = empty_registry_and_dispatcher();
        let mut app = App::new(ViewModel::default(), registry, dispatcher, Vec::new());
        let mut source = ScriptedSource::new(vec![None]);
        let mut renderer = RecordingRenderer::default();

        let result = app
            .tick(&[TermEvent::Key(Key::Esc)], &mut source, &mut renderer)
            .expect("tick succeeds");

        assert!(
            !result.quit,
            "quit must go through a real dispatch, never a hardcoded key match"
        );
    }

    #[test]
    fn render_loop_resize_updates_view_and_redraws_without_snapshot() {
        let (registry, dispatcher) = empty_registry_and_dispatcher();
        let mut app = App::new(ViewModel::default(), registry, dispatcher, Vec::new());
        let mut source = ScriptedSource::new(vec![None]);
        let mut renderer = RecordingRenderer::default();

        let result = app
            .tick(&[TermEvent::Resize(120, 40)], &mut source, &mut renderer)
            .unwrap();

        assert!(result.redrawn, "resize requests a redraw");
        assert_eq!(app.view().size, (120, 40), "view-model tracks new size");
        assert_eq!(renderer.frames.len(), 1);
    }

    #[test]
    fn layout_focus_tab_key_advances_focus_and_redraws() {
        let (registry, dispatcher) = registry_with_pane_actions();
        let mut app = App::new(ViewModel::default(), registry, dispatcher, Vec::new());
        let mut source = ScriptedSource::new(vec![None]);
        let mut renderer = RecordingRenderer::default();

        assert_eq!(app.view().focus, Focus::Board, "default focus is the board");

        let forward = app
            .tick(&[TermEvent::Key(Key::Tab)], &mut source, &mut renderer)
            .unwrap();
        assert!(forward.redrawn, "focus change requests a redraw");
        assert_eq!(app.view().focus, Focus::Office, "Tab advances focus");

        let mut source2 = ScriptedSource::new(vec![None]);
        app.tick(&[TermEvent::Key(Key::BackTab)], &mut source2, &mut renderer)
            .unwrap();
        assert_eq!(app.view().focus, Focus::Board, "Shift+Tab steps focus back");
    }

    /// The command bar is reachable in one keystroke from any panel: `/`
    /// focuses it, and the key that moved focus is consumed by the move — it is
    /// not typed into the line it just opened.
    #[test]
    fn the_slash_key_focuses_the_command_bar_from_any_panel_without_typing_it() {
        for start in [Focus::Board, Focus::Office, Focus::Status, Focus::Sessions] {
            let (registry, dispatcher) = registry_with_pane_actions();
            let mut app = App::new(
                ViewModel {
                    focus: start,
                    ..Default::default()
                },
                registry,
                dispatcher,
                Vec::new(),
            );
            let mut source = ScriptedSource::new(vec![None]);
            let mut renderer = RecordingRenderer::default();

            let result = app
                .tick(
                    &[TermEvent::Key(Key::Char('/'))],
                    &mut source,
                    &mut renderer,
                )
                .unwrap();

            assert_eq!(app.view().focus, Focus::CommandBar, "from {start:?}");
            assert_eq!(app.view().command_input, "", "the jump key is not text");
            assert!(result.redrawn, "a focus change requests a redraw");
        }
    }

    /// Once the command bar already holds focus, `/` is what it looks like:
    /// an ordinary character in the line being typed.
    #[test]
    fn the_slash_key_is_ordinary_text_once_the_command_bar_has_focus() {
        let (registry, dispatcher) = registry_with_pane_actions();
        let mut app = App::new(
            ViewModel {
                focus: Focus::CommandBar,
                ..Default::default()
            },
            registry,
            dispatcher,
            Vec::new(),
        );
        let mut source = ScriptedSource::new(vec![]);
        let mut renderer = RecordingRenderer::default();

        for c in "/a".chars() {
            app.tick(&[TermEvent::Key(Key::Char(c))], &mut source, &mut renderer)
                .unwrap();
        }

        assert_eq!(app.view().command_input, "/a");
        assert_eq!(app.view().focus, Focus::CommandBar);
    }

    /// The reported defect end to end: a user looking at a panel types `/` and
    /// then a command, and the command runs — previously every key typed
    /// against the rendered prompt was swallowed until focus had been tabbed
    /// onto it.
    #[test]
    fn a_command_can_be_typed_straight_from_a_panel_after_the_slash_key() {
        let (mut registry, mut dispatcher) = registry_with_probe();
        pane_actions::register(&mut registry, &mut dispatcher);
        let mut app = App::new(ViewModel::default(), registry, dispatcher, test_catalog());
        let mut source = ScriptedSource::new(vec![]);
        let mut renderer = RecordingRenderer::default();
        assert_eq!(app.view().focus, Focus::Board);

        for c in "/test probe hello".chars() {
            app.tick(&[TermEvent::Key(Key::Char(c))], &mut source, &mut renderer)
                .unwrap();
        }
        app.tick(&[TermEvent::Key(Key::Enter)], &mut source, &mut renderer)
            .unwrap();

        assert_eq!(app.view().command_feedback.as_deref(), Some("probe: hello"));
    }

    /// The direct-entry key is a registered action like every other pane
    /// action, so an unregistered one leaves focus exactly where it was.
    #[test]
    fn an_unregistered_focus_command_action_never_moves_focus() {
        let (registry, dispatcher) = empty_registry_and_dispatcher();
        let mut app = App::new(ViewModel::default(), registry, dispatcher, Vec::new());
        let mut source = ScriptedSource::new(vec![None]);
        let mut renderer = RecordingRenderer::default();

        let result = app
            .tick(
                &[TermEvent::Key(Key::Char('/'))],
                &mut source,
                &mut renderer,
            )
            .unwrap();

        assert!(!result.redrawn, "no dispatch ran, so nothing changed");
        assert_eq!(app.view().focus, Focus::Board, "focus must not move");
    }

    /// The registry, not the key match, decides whether focus moves at all:
    /// with no pane actions registered, Tab must leave focus exactly where
    /// it was and request no redraw — proving `handle_key` really resolves
    /// through dispatch rather than mutating `view.focus` unconditionally.
    #[test]
    fn an_unregistered_focus_action_never_moves_focus() {
        let (registry, dispatcher) = empty_registry_and_dispatcher();
        let mut app = App::new(ViewModel::default(), registry, dispatcher, Vec::new());
        let mut source = ScriptedSource::new(vec![None]);
        let mut renderer = RecordingRenderer::default();

        let result = app
            .tick(&[TermEvent::Key(Key::Tab)], &mut source, &mut renderer)
            .unwrap();

        assert!(!result.redrawn, "no dispatch ran, so nothing changed");
        assert_eq!(app.view().focus, Focus::Board, "focus must not move");
    }

    /// Types `/test probe hello`, submits it, and asserts the feedback is
    /// exactly the real, registered invocable's own `Outcome` rendered —
    /// end to end through the app's tick loop, the real registry, and the
    /// real dispatcher, not a stub.
    #[test]
    fn command_parse_bar_typing_then_enter_dispatches_a_real_registered_invocable() {
        let (registry, dispatcher) = registry_with_probe();
        let mut app = App::new(
            ViewModel {
                focus: Focus::CommandBar,
                ..Default::default()
            },
            registry,
            dispatcher,
            test_catalog(),
        );
        let mut source = ScriptedSource::new(vec![]);
        let mut renderer = RecordingRenderer::default();

        for c in "test probe hello".chars() {
            app.tick(&[TermEvent::Key(Key::Char(c))], &mut source, &mut renderer)
                .unwrap();
        }
        assert_eq!(app.view().command_input, "test probe hello");

        app.tick(&[TermEvent::Key(Key::Enter)], &mut source, &mut renderer)
            .unwrap();
        assert_eq!(app.view().command_input, "", "input clears on submit");
        assert_eq!(
            app.view().command_feedback.as_deref(),
            Some("probe: hello"),
            "feedback is exactly the dispatched invocable's own rendered Outcome"
        );
    }

    #[test]
    fn command_parse_bar_unknown_errors_and_esc_cancels_without_quitting() {
        let (registry, dispatcher) = empty_registry_and_dispatcher();
        let mut app = App::new(
            ViewModel {
                focus: Focus::CommandBar,
                ..Default::default()
            },
            registry,
            dispatcher,
            Vec::new(),
        );
        let mut source = ScriptedSource::new(vec![]);
        let mut renderer = RecordingRenderer::default();

        for c in "xyz".chars() {
            app.tick(&[TermEvent::Key(Key::Char(c))], &mut source, &mut renderer)
                .unwrap();
        }
        app.tick(&[TermEvent::Key(Key::Enter)], &mut source, &mut renderer)
            .unwrap();
        assert!(
            app.view()
                .command_feedback
                .as_deref()
                .unwrap()
                .contains("unknown command"),
            "an unrecognized verb yields an inline error"
        );

        let result = app
            .tick(&[TermEvent::Key(Key::Esc)], &mut source, &mut renderer)
            .unwrap();
        assert!(!result.quit, "Esc cancels in the command bar, never quits");
        assert_eq!(app.view().command_feedback, None);
    }

    #[test]
    fn render_loop_holds_no_state_beyond_the_snapshot() {
        // Determinism proves the absence of hidden domain state: two apps fed the
        // same event/snapshot script end in identical view-models.
        let script = || ScriptedSource::new(vec![Some(snap("a")), Some(snap("b"))]);

        let (r1, d1) = empty_registry_and_dispatcher();
        let mut app1 = App::new(ViewModel::default(), r1, d1, Vec::new());
        let mut r1 = RecordingRenderer::default();
        let mut s1 = script();
        app1.tick(&[], &mut s1, &mut r1).unwrap();
        app1.tick(&[], &mut s1, &mut r1).unwrap();

        let (r2, d2) = empty_registry_and_dispatcher();
        let mut app2 = App::new(ViewModel::default(), r2, d2, Vec::new());
        let mut r2 = RecordingRenderer::default();
        let mut s2 = script();
        app2.tick(&[], &mut s2, &mut r2).unwrap();
        app2.tick(&[], &mut s2, &mut r2).unwrap();

        assert_eq!(
            app1.view(),
            app2.view(),
            "loop is a pure function of its inputs"
        );
        // The view-model is exactly the last snapshot — nothing accumulated.
        assert_eq!(app1.view().snapshot, snap("b"));
    }

    /// Both halves of the same criterion, driven end to end through the
    /// app's tick loop over one real, registered invocable: an unresolved
    /// slash line renders **no** feedback at all — ordinary input, not a
    /// rendered failure (l2-tui v1.2.0) — and a genuinely `Rejected` outcome
    /// (the identity resolves; its one required binder is never supplied)
    /// still renders as a legible refusal. Proven together so the two stay
    /// distinguishable rather than both silently swallowed.
    #[test]
    fn command_dispatch_distinguishes_an_unresolved_line_from_a_genuine_rejection() {
        let (registry, dispatcher) = registry_with_required_binder();
        let mut app = App::new(
            ViewModel {
                focus: Focus::CommandBar,
                ..Default::default()
            },
            registry,
            dispatcher,
            test_catalog(),
        );
        let mut source = ScriptedSource::new(vec![]);
        let mut renderer = RecordingRenderer::default();

        // "test" is a known catalog group, but "frobnicate" resolves to no
        // registered invocable — an unresolved candidate identity, not a
        // failure to render anything about.
        for c in "test frobnicate".chars() {
            app.tick(&[TermEvent::Key(Key::Char(c))], &mut source, &mut renderer)
                .unwrap();
        }
        app.tick(&[TermEvent::Key(Key::Enter)], &mut source, &mut renderer)
            .unwrap();
        assert_eq!(
            app.view().command_feedback,
            None,
            "an unresolved identity must render no feedback at all, not an error string"
        );

        // "test probe" resolves to a real invocable whose one required
        // binder is never supplied — a genuine Rejected outcome, which must
        // still render, distinguishing it from the unresolved case above.
        for c in "test probe".chars() {
            app.tick(&[TermEvent::Key(Key::Char(c))], &mut source, &mut renderer)
                .unwrap();
        }
        app.tick(&[TermEvent::Key(Key::Enter)], &mut source, &mut renderer)
            .unwrap();

        let feedback = app.view().command_feedback.as_deref().unwrap();
        assert!(
            feedback.contains("value"),
            "the offending binder's own name must reach the view: {feedback:?}"
        );
        assert!(
            feedback.contains("Absent"),
            "the rejection mode must reach the view, not be flattened away: {feedback:?}"
        );
    }

    #[test]
    fn command_dispatch_through_the_bar_never_leaks_a_secret_to_the_view() {
        let (registry, dispatcher) = registry_with_secret_probe("sk-LIVE-7");
        let mut app = App::new(
            ViewModel {
                focus: Focus::CommandBar,
                ..Default::default()
            },
            registry,
            dispatcher,
            test_catalog(),
        );
        let mut source = ScriptedSource::new(vec![]);
        let mut renderer = RecordingRenderer::default();

        for c in "test probe".chars() {
            app.tick(&[TermEvent::Key(Key::Char(c))], &mut source, &mut renderer)
                .unwrap();
        }
        app.tick(&[TermEvent::Key(Key::Enter)], &mut source, &mut renderer)
            .unwrap();

        let feedback = app.view().command_feedback.as_deref().unwrap();
        assert!(
            !feedback.contains("sk-LIVE-7"),
            "the secret never reaches the view-model or the screen buffer"
        );
        assert!(feedback.contains("***"), "the leaked value renders masked");
    }

    /// The literal defect a live simulation pass found: dispatching `/pane
    /// focus-next` through the command bar answered `Dispatched::Ran` (the
    /// stateless `Outcome::Value(Empty)` every pane action returns) and
    /// rendered nothing wrong — but `view.focus` never moved, because only
    /// the direct Tab key path applied the actual mutation. Proven here
    /// through the real, registered pane actions and the real catalog they
    /// build, not a stand-in.
    #[test]
    fn command_bar_pane_action_focus_next_moves_focus_like_the_tab_key_does() {
        let (registry, dispatcher) = registry_with_pane_actions();
        let invocables: Vec<&Invocable> = registry.all().collect();
        let catalog = command::build_catalog(&invocables);
        let mut app = App::new(
            ViewModel {
                focus: Focus::CommandBar,
                ..Default::default()
            },
            registry,
            dispatcher,
            catalog,
        );
        let mut source = ScriptedSource::new(vec![]);
        let mut renderer = RecordingRenderer::default();

        for c in "pane focus-next".chars() {
            app.tick(&[TermEvent::Key(Key::Char(c))], &mut source, &mut renderer)
                .unwrap();
        }
        app.tick(&[TermEvent::Key(Key::Enter)], &mut source, &mut renderer)
            .unwrap();

        assert_eq!(
            app.view().focus,
            Focus::Board,
            "the command-bar form of /pane focus-next must move focus exactly like Tab does"
        );
        assert_eq!(
            app.view().command_feedback,
            None,
            "the effect itself is the feedback, matching the direct-key path's own silence"
        );
    }

    /// The other half of the same defect: `/pane quit` through the command
    /// bar must actually end the session, not merely echo a stateless
    /// acknowledgment while the loop keeps running.
    #[test]
    fn command_bar_pane_action_quit_actually_quits_the_session() {
        let (registry, dispatcher) = registry_with_pane_actions();
        let invocables: Vec<&Invocable> = registry.all().collect();
        let catalog = command::build_catalog(&invocables);
        let mut app = App::new(
            ViewModel {
                focus: Focus::CommandBar,
                ..Default::default()
            },
            registry,
            dispatcher,
            catalog,
        );
        let mut source = ScriptedSource::new(vec![]);
        let mut renderer = RecordingRenderer::default();

        for c in "pane quit".chars() {
            app.tick(&[TermEvent::Key(Key::Char(c))], &mut source, &mut renderer)
                .unwrap();
        }
        let result = app
            .tick(&[TermEvent::Key(Key::Enter)], &mut source, &mut renderer)
            .unwrap();

        assert!(
            result.quit,
            "the command-bar form of /pane quit must quit exactly like Esc does outside the command bar"
        );
    }

    /// A submitted line that merely *looks* like a pane action (right group,
    /// wrong verb) must fall through to the ordinary dispatch path rather
    /// than being silently swallowed by the pane-action shortcut.
    #[test]
    fn command_bar_pane_group_with_an_unknown_verb_falls_through_to_ordinary_dispatch() {
        let (registry, dispatcher) = registry_with_pane_actions();
        let invocables: Vec<&Invocable> = registry.all().collect();
        let catalog = command::build_catalog(&invocables);
        let mut app = App::new(
            ViewModel {
                focus: Focus::CommandBar,
                ..Default::default()
            },
            registry,
            dispatcher,
            catalog,
        );
        let mut source = ScriptedSource::new(vec![]);
        let mut renderer = RecordingRenderer::default();

        for c in "pane bogus".chars() {
            app.tick(&[TermEvent::Key(Key::Char(c))], &mut source, &mut renderer)
                .unwrap();
        }
        app.tick(&[TermEvent::Key(Key::Enter)], &mut source, &mut renderer)
            .unwrap();

        assert_eq!(
            app.view().command_feedback,
            None,
            "an unresolved candidate identity is ordinary input, not a rendered failure"
        );
        assert_eq!(
            app.view().focus,
            Focus::CommandBar,
            "no action ran, so focus must not move"
        );
    }

    /// A representative populated view-model for render-from-state tests.
    fn populated_view() -> ViewModel {
        let mut sessions = SessionsView::default();
        sessions.push("agent started");
        ViewModel {
            snapshot: CoreSnapshot {
                status: Projection::Available(StatusView {
                    version: "1.2.3".to_string(),
                    status: "running | 80%".to_string(),
                }),
                board: Projection::Available(BoardView {
                    cards: vec![view::BoardCard {
                        id: "k1".to_string(),
                        title: String::new(),
                        column: view::BoardColumn::Running,
                    }],
                    ..Default::default()
                }),
                office: Projection::Available(OfficeView {
                    agents: vec![view::AgentActivity {
                        agent: "orchestrator".to_string(),
                        task: "planning".to_string(),
                    }],
                }),
                sessions: Projection::Available(sessions),
            },
            size: (80, 24),
            focus: Focus::Board,
            command_input: String::new(),
            command_feedback: None,
        }
    }

    fn render_to_buffer(view: &ViewModel, area: Rect) -> Buffer {
        let mut buf = Buffer::empty(area);
        render_view(area, &mut buf, view);
        buf
    }

    fn buffer_text(buf: &Buffer, area: Rect) -> String {
        let mut text = String::new();
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                if let Some(cell) = buf.cell((x, y)) {
                    text.push_str(cell.symbol());
                }
            }
        }
        text
    }

    #[test]
    fn render_state_is_a_pure_function_of_the_view_model() {
        let view = populated_view();
        let area = Rect::new(0, 0, 80, 24);

        let first = render_to_buffer(&view, area);
        let second = render_to_buffer(&view, area);

        assert_eq!(
            first, second,
            "the same view-model must render an identical frame"
        );
    }

    #[test]
    fn render_state_differs_when_the_snapshot_changes() {
        let area = Rect::new(0, 0, 80, 24);
        let base = render_to_buffer(&populated_view(), area);

        let mut changed = populated_view();
        changed.snapshot.status = Projection::Available(StatusView {
            version: "1.2.3".to_string(),
            status: "running | 100%".to_string(),
        });
        let after = render_to_buffer(&changed, area);

        assert_ne!(base, after, "a changed snapshot must change the frame");
    }

    // ── Board/office projection dispatch (unavailable vs. empty) ────
    //
    // `dispatch_board`/`dispatch_office` are what `CapabilitySource` and the
    // production `poll_snapshot` call every tick; proven directly here
    // against a real registry/dispatcher, the same way every other dispatch
    // path in this crate is — never through a stand-in that only pretends
    // to be the real mechanism.

    #[test]
    fn dispatch_board_reports_unavailable_when_core_board_list_is_not_registered() {
        let registry = InvocableRegistry::new();
        let dispatcher = Dispatcher::new();
        assert!(matches!(
            dispatch_board(&registry, &dispatcher),
            Projection::Unavailable { .. }
        ));
    }

    /// Registers a fixture `core:board.list` handler with the real
    /// invocable's shape: `(id, state)` records for the live cards and, when
    /// called with the `archived` flag, the archive store's cards or its
    /// failure — so `dispatch_board` is proven against a real dispatch path,
    /// not a stub of it.
    fn register_board_list(
        registry: &mut InvocableRegistry,
        dispatcher: &mut Dispatcher,
        live: Vec<(&'static str, &'static str)>,
        archived: Result<Vec<&'static str>, &'static str>,
    ) {
        let id = InvocableId::new("core:board.list").expect("well-formed test id");
        registry
            .register(
                &Registrant::core(),
                Invocable {
                    id: id.clone(),
                    name: "List",
                    summary: "Test-only board.list fixture.",
                    group: "board",
                    locus: Locus::Semantic,
                    binders: vec![Binder {
                        name: "archived",
                        kind: BinderKind::Flag,
                        optional: true,
                    }],
                    stability: Stability::Shipped,
                    journal_raw_input: true,
                },
            )
            .expect("test fixture registers cleanly");
        dispatcher.attach(
            id,
            Arc::new(move |args| {
                let record = |cid: &str, state: &str| {
                    OutcomeValue::Record(vec![
                        ("id".to_string(), OutcomeValue::Text(cid.to_string())),
                        ("state".to_string(), OutcomeValue::Text(state.to_string())),
                    ])
                };
                if matches!(args.get("archived"), Some(ArgValue::Flag)) {
                    return match &archived {
                        Ok(ids) => Outcome::Value(OutcomeValue::List(
                            ids.iter().map(|cid| record(cid, "done")).collect(),
                        )),
                        Err(reason) => Outcome::Unavailable {
                            reason: reason.to_string(),
                        },
                    };
                }
                Outcome::Value(OutcomeValue::List(
                    live.iter().map(|(cid, state)| record(cid, state)).collect(),
                ))
            }),
        );
    }

    #[test]
    fn dispatch_board_reports_a_genuinely_empty_result_as_available() {
        let mut registry = InvocableRegistry::new();
        let mut dispatcher = Dispatcher::new();
        register_board_list(&mut registry, &mut dispatcher, Vec::new(), Ok(Vec::new()));

        assert_eq!(
            dispatch_board(&registry, &dispatcher),
            Projection::Available(BoardView::default()),
            "a real, genuinely empty result must be Available, never Unavailable"
        );
    }

    #[test]
    fn dispatch_board_maps_a_real_result_into_board_cards() {
        let mut registry = InvocableRegistry::new();
        let mut dispatcher = Dispatcher::new();
        register_board_list(
            &mut registry,
            &mut dispatcher,
            vec![("k1", "running"), ("k2", "todo")],
            Ok(Vec::new()),
        );

        let Projection::Available(board) = dispatch_board(&registry, &dispatcher) else {
            panic!("expected an available board");
        };
        assert_eq!(board.cards.len(), 2);
        assert!(
            board
                .cards
                .iter()
                .any(|c| c.id == "k1" && c.column == view::BoardColumn::Running)
        );
        assert!(
            board
                .cards
                .iter()
                .any(|c| c.id == "k2" && c.column == view::BoardColumn::Todo)
        );
    }

    /// The Archive column reads the separate archive store: archived cards
    /// land in it whatever state they carry, beside the live ones.
    #[test]
    fn dispatch_board_places_archived_cards_in_the_archive_column() {
        let mut registry = InvocableRegistry::new();
        let mut dispatcher = Dispatcher::new();
        register_board_list(
            &mut registry,
            &mut dispatcher,
            vec![("k1", "running")],
            Ok(vec!["a1", "a2"]),
        );

        let Projection::Available(board) = dispatch_board(&registry, &dispatcher) else {
            panic!("expected an available board");
        };
        assert_eq!(board.cards.len(), 3);
        let archived: Vec<&str> = board
            .cards_in(view::BoardColumn::Archive)
            .map(|c| c.id.as_str())
            .collect();
        assert_eq!(archived, ["a1", "a2"]);
        assert!(
            board
                .cards_in(view::BoardColumn::Running)
                .any(|c| c.id == "k1")
        );
    }

    /// One card in a state this surface has no column for must not blank the
    /// cards it can place: the panel stays available, and the card is
    /// named rather than silently dropped.
    #[test]
    fn dispatch_board_sets_aside_one_unrecognized_state_instead_of_failing_the_board() {
        let mut registry = InvocableRegistry::new();
        let mut dispatcher = Dispatcher::new();
        register_board_list(
            &mut registry,
            &mut dispatcher,
            vec![("k1", "running"), ("weird", "sideways")],
            Ok(Vec::new()),
        );

        let Projection::Available(board) = dispatch_board(&registry, &dispatcher) else {
            panic!("one unplaceable card must not make the whole board unavailable");
        };
        assert_eq!(
            board
                .cards
                .iter()
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>(),
            ["k1"]
        );
        assert_eq!(board.unmapped, ["weird"]);
    }

    /// An archive store that cannot be read is unavailable, not an empty
    /// Archive column — an empty column would read as "nothing archived".
    #[test]
    fn dispatch_board_is_unavailable_when_the_archive_cannot_be_read() {
        let mut registry = InvocableRegistry::new();
        let mut dispatcher = Dispatcher::new();
        register_board_list(
            &mut registry,
            &mut dispatcher,
            vec![("k1", "running")],
            Err("archive directory unreadable"),
        );

        let Projection::Unavailable { reason } = dispatch_board(&registry, &dispatcher) else {
            panic!("an unreadable archive must not render as an empty column");
        };
        assert!(
            reason.contains("archive") && reason.contains("unreadable"),
            "{reason}"
        );
    }

    #[test]
    fn dispatch_office_reports_unavailable_when_core_role_list_is_not_registered() {
        let registry = InvocableRegistry::new();
        let dispatcher = Dispatcher::new();
        assert!(matches!(
            dispatch_office(&registry, &dispatcher),
            Projection::Unavailable { .. }
        ));
    }

    #[test]
    fn dispatch_office_maps_a_real_result_into_agent_activity() {
        let mut registry = InvocableRegistry::new();
        let mut dispatcher = Dispatcher::new();
        let id = InvocableId::new("core:role.list").expect("well-formed test id");
        registry
            .register(
                &Registrant::core(),
                Invocable {
                    id: id.clone(),
                    name: "List",
                    summary: "Test-only role.list fixture.",
                    group: "role",
                    locus: Locus::Semantic,
                    binders: vec![Binder {
                        name: "presets",
                        kind: BinderKind::Flag,
                        optional: true,
                    }],
                    stability: Stability::Shipped,
                    journal_raw_input: true,
                },
            )
            .expect("test fixture registers cleanly");
        dispatcher.attach(
            id,
            Arc::new(|_args| {
                Outcome::Value(OutcomeValue::List(vec![OutcomeValue::Record(vec![
                    ("id".to_string(), OutcomeValue::Text("r1".to_string())),
                    (
                        "display_name".to_string(),
                        OutcomeValue::Text("Coder".to_string()),
                    ),
                ])]))
            }),
        );

        let Projection::Available(office) = dispatch_office(&registry, &dispatcher) else {
            panic!("expected an available office");
        };
        assert_eq!(office.agents.len(), 1);
        assert_eq!(office.agents[0].agent, "Coder");
        assert_eq!(
            office.agents[0].task, "",
            "no live task-tracking exists in the domain layer yet — an honest absence"
        );
    }

    #[test]
    fn mask_secrets_dispatched_value_never_reaches_the_buffer() {
        let (registry, dispatcher) = registry_with_secret_probe("sk-LIVE-42");
        let mut app = App::new(
            ViewModel {
                focus: Focus::CommandBar,
                size: (80, 24),
                ..Default::default()
            },
            registry,
            dispatcher,
            test_catalog(),
        );
        let mut source = ScriptedSource::new(vec![]);
        let mut renderer = RecordingRenderer::default();

        for c in "test probe".chars() {
            app.tick(&[TermEvent::Key(Key::Char(c))], &mut source, &mut renderer)
                .unwrap();
        }
        app.tick(&[TermEvent::Key(Key::Enter)], &mut source, &mut renderer)
            .unwrap();

        let area = Rect::new(0, 0, 80, 24);
        let buf = render_to_buffer(app.view(), area);
        let rendered = buffer_text(&buf, area);

        assert!(
            !rendered.contains("sk-LIVE-42"),
            "no secret value reaches the rendered screen buffer"
        );
        assert!(rendered.contains("***"), "the secret renders masked");
    }
}
