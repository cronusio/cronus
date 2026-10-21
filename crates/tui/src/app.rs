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
use std::sync::{Arc, mpsc};

use cronus_core::invocable::StreamItem;
use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend as RatatuiCrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::chat_actions::ChatAction;
use crate::command::{self, CommandOutcome, CommandSpec, SlashCommand};
use crate::dispatch;
use crate::installation::{self, Placement, Question, Setting, Step};
use crate::pane_actions::{self, PaneAction};
use crate::terminal::{CrosstermBackend, Key, TermEvent, TerminalBackend, Tui};
use crate::view::{
    self, Addressee, BoardColumn, BoardCursor, BoardView, ChatView, Focus, OfficeView, Projection,
    ResultBlock, Screen, SessionList, SessionRow, SessionsView, StatusView, TranscriptView,
};
use crate::work::{CommandResult, Finished, Kind, Work};
use cronus_contract::{
    ArgValue, ArgValues, Dispatched, Invocable, InvocableId, Invocation, Outcome, OutcomeValue,
    Surface,
};
use cronus_core::invocable::{Dispatcher, InvocableRegistry};
use cronus_domain::{Capabilities, Engine};

/// How long the loop blocks for input before ticking again. Bounds redraw
/// latency for state changes that arrive without a terminal event.
const TICK: Duration = Duration::from_millis(50);

/// What a line meets between a change that asked for a relaunch and the
/// relaunch: nothing new starts on a composition that is about to be replaced.
const RESTARTING: &str = "restarting onto the changed installation — one moment";

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
    /// The sessions list the Sessions screen shows.
    pub session_list: Projection<SessionList>,
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
    /// The result of the last submitted command (error / ack), shown beside
    /// the prompt until the next keystroke.
    pub command_feedback: Option<String>,
    /// A result with more to say than one line — the discovery listing, an
    /// installation verb's answer — shown over the lower part of the view
    /// until the next keystroke in the command bar.
    pub result_block: Option<ResultBlock>,
    /// Who the line being typed is addressed to (shown in the input block).
    pub addressee: Addressee,
    /// Which screen fills the body.
    pub screen: Screen,
    /// The Kanban screen's selection.
    pub board_cursor: BoardCursor,
    /// The Sessions screen's selection (an index into the list).
    pub session_cursor: usize,
    /// The conversation of the selected session, with the key it was read
    /// under (id and entry count, so it is re-read when the journal grows).
    pub transcript: Option<(String, Projection<TranscriptView>)>,
    /// The Chat screen: its conversation, model and unanswered message.
    pub chat: ChatView,
}

/// What a reply being written reports back to the loop.
enum TurnEvent {
    /// A piece of the reply, as it arrives.
    Chunk(String),
    /// The turn settled.
    Done(Outcome),
}

/// The seam the loop reads core state through.
///
/// The core exposes no event/observe bus yet, so the production source
/// ([`CapabilitySource`]) *polls* — the poll-snapshot fallback the spec
/// mandates. Polling never waits: the reads run on a thread of their own and
/// a poll hands over whatever has finished. An event-driven source can implement the same
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
        registry: &Arc<InvocableRegistry>,
        dispatcher: &Arc<Dispatcher>,
    ) -> Option<CoreSnapshot>;

    /// What the snapshot shows has probably changed (a command just ran):
    /// read again at the next chance instead of waiting out the interval.
    fn invalidate(&mut self) {}
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
    /// Whether the loop should rebuild the composition and carry on in a
    /// fresh [`App`] onto the same view: a change that was applied by
    /// relaunch has left the one it was running on out of date. Never set
    /// together with `quit`, and never while anything is still running.
    pub relaunch: bool,
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
    registry: Arc<InvocableRegistry>,
    /// Runs a resolved invocation for real, masking secrets at the boundary.
    dispatcher: Arc<Dispatcher>,
    /// The reply being written on a background thread, when there is one: a
    /// model turn takes seconds, and the screen must stay alive meanwhile.
    turn: Option<mpsc::Receiver<TurnEvent>>,
    /// Where everything else that leaves the render thread reports back.
    work: Work,
    /// A command is running; a second one waits for it to finish.
    command_running: bool,
    /// The conversation key of the Sessions screen's read in flight.
    transcript_reading: Option<String>,
    /// A finished reply is waiting for its conversation to be read back
    /// before the pending message is cleared.
    settling: bool,
    /// Stop requests in flight.
    stops: usize,
    /// What the snapshot shows may have changed since it was read.
    stale: bool,
    /// A change waiting for the person's answer; until it comes, the keys
    /// answer it and do nothing else.
    awaiting: Option<Question>,
    /// Where this frontend stands, and how a recomposing change is asked
    /// about: the engine can relaunch, a spoke leaves that to the engine.
    setting: Setting,
    /// A change applied by relaunch was carried out; the loop is asked to
    /// rebuild the composition as soon as nothing is running on this one.
    relaunch_pending: bool,
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
        Self::shared(initial, Arc::new(registry), Arc::new(dispatcher), catalog)
    }

    /// The same, over a registry and dispatcher that something else holds too:
    /// an engine serves the very pair this app dispatches through, so the work
    /// a spoke sends and the work typed here run through one dispatcher.
    pub fn shared(
        initial: ViewModel,
        registry: Arc<InvocableRegistry>,
        dispatcher: Arc<Dispatcher>,
        catalog: Vec<CommandSpec>,
    ) -> Self {
        Self {
            view: initial,
            should_quit: false,
            pending_dispatch: None,
            registry,
            dispatcher,
            turn: None,
            work: Work::new(),
            command_running: false,
            transcript_reading: None,
            settling: false,
            stops: 0,
            stale: false,
            awaiting: None,
            setting: Setting::live(Placement::Engine),
            relaunch_pending: false,
            catalog,
        }
    }

    /// The same app, standing where `setting` says: as the engine of its state
    /// root or as a spoke of another process's.
    pub fn placed(mut self, setting: Setting) -> Self {
        self.setting = setting;
        self
    }

    /// The view this app leaves for the one that replaces it after a
    /// relaunch: the same screen, focus and conversation, with the answer to
    /// the change that caused it still showing.
    pub fn into_view(self) -> ViewModel {
        self.view
    }

    /// This app as the continuation of one that was relaunched: everything it
    /// shows that was read from the installation is read again, because the
    /// change may have replaced it.
    pub fn after_relaunch(mut self) -> Self {
        self.stale = true;
        self.view.transcript = None;
        if self.view.chat.session.is_some() {
            self.refresh_chat_transcript();
        }
        self
    }

    /// Whether a change is waiting for the person's yes or no.
    pub fn is_awaiting_consent(&self) -> bool {
        self.awaiting.is_some()
    }

    /// Whether work started off the render thread — a command, a read of a
    /// conversation, a stop request — is still running. A reply being written
    /// is not counted: it is shown as it arrives, and has its own marker.
    pub fn is_busy(&self) -> bool {
        self.command_running || self.transcript_reading.is_some() || self.settling || self.stops > 0
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
        //     rendered failure, so feedback is cleared exactly
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
            let chat_action = command
                .args
                .first()
                .and_then(|sub_verb| ChatAction::from_slash(&command.verb, sub_verb));
            match (pane_action, chat_action) {
                (_, Some(action)) => self.apply_chat_action(action, &command),
                (Some(action), None) => {
                    self.dispatch_pane_action(action);
                    self.view.command_feedback = None;
                }
                (None, None) => self.run_command(command),
            }
            needs_redraw = true;
        }

        // 1c) A reply finished on its background thread.
        if self.poll_turn() {
            needs_redraw = true;
        }

        // 1d) Whatever else was running beside the loop has finished.
        if self.poll_work() {
            needs_redraw = true;
        }
        if std::mem::take(&mut self.stale) {
            source.invalidate();
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
            if self.view.screen == Screen::Sessions {
                self.sync_transcript();
            }
            needs_redraw = true;
        }

        // 3) Coalesce to exactly one redraw per changed tick.
        if needs_redraw {
            renderer.draw(&self.view)?;
        }

        Ok(TickResult {
            redrawn: needs_redraw,
            quit: self.should_quit,
            // A reply still being written, or any work still running beside the
            // loop, finishes first: it was started on this composition and is
            // not cut off from it.
            relaunch: self.relaunch_pending
                && !self.should_quit
                && !self.is_busy()
                && self.turn.is_none(),
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
        if self.awaiting.is_some() {
            return self.consent_key(key);
        }
        match key {
            Key::Tab => self.dispatch_pane_action(PaneAction::FocusNext),
            Key::BackTab => self.dispatch_pane_action(PaneAction::FocusPrev),
            Key::F(n) => match pane_actions::fkey(n) {
                Some((_, action)) => self.dispatch_pane_action(action),
                None => false,
            },
            Key::Char('/') if self.view.focus != Focus::CommandBar => {
                self.dispatch_pane_action(PaneAction::FocusCommandBar)
            }
            Key::Left | Key::Right | Key::Up | Key::Down | Key::Enter
                if self.view.screen == Screen::Kanban && self.view.focus != Focus::CommandBar =>
            {
                self.kanban_key(key)
            }
            Key::Up | Key::Down
                if self.view.screen == Screen::Sessions && self.view.focus != Focus::CommandBar =>
            {
                self.sessions_key(key)
            }
            // Esc leaves a full-width screen for the overview before it can
            // quit the session.
            Key::Esc
                if self.view.screen != Screen::Overview && self.view.focus != Focus::CommandBar =>
            {
                self.view.screen = Screen::Overview;
                true
            }
            _ if self.view.focus == Focus::CommandBar => self.handle_command_key(key),
            Key::Esc => self.dispatch_pane_action(PaneAction::Quit),
            _ => false,
        }
    }

    /// Keys while a change waits for the person's answer. Only the answer
    /// does anything: yes runs the invocation the question named, no (or
    /// Esc) leaves it undone, and every other key is ignored, so a stray
    /// keystroke can neither agree nor wander off with the question open.
    fn consent_key(&mut self, key: Key) -> bool {
        match key {
            Key::Char('y' | 'Y') | Key::Enter => {
                let Some(question) = self.awaiting.take() else {
                    return false;
                };
                self.view.result_block = None;
                self.command_running = true;
                self.view.command_feedback = Some(format!("running {} …", question.title));
                self.work.spawn(
                    Kind::Command,
                    &self.registry,
                    &self.dispatcher,
                    move |registry, dispatcher| {
                        let installation::Carried { block, relaunch } =
                            installation::confirmed(registry, dispatcher, &question);
                        Finished::Command(CommandResult::Applied { block, relaunch })
                    },
                );
                true
            }
            Key::Char('n' | 'N') | Key::Esc => {
                let Some(question) = self.awaiting.take() else {
                    return false;
                };
                self.view.result_block = None;
                self.view.command_feedback = Some(format!("left undone: {}", question.title));
                true
            }
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
                    self.leave_full_screen();
                    true
                }
                PaneAction::FocusPrev => {
                    self.view.focus = self.view.focus.prev();
                    self.leave_full_screen();
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
                PaneAction::FocusBoard => {
                    self.view.focus = Focus::Board;
                    self.view.screen = Screen::Kanban;
                    true
                }
                PaneAction::FocusOffice => {
                    self.view.focus = Focus::Office;
                    self.view.screen = Screen::Overview;
                    true
                }
                PaneAction::FocusSessions => {
                    self.view.focus = Focus::Sessions;
                    self.view.screen = Screen::Sessions;
                    self.sync_transcript();
                    true
                }
                PaneAction::Help => {
                    self.show_help();
                    true
                }
                PaneAction::FocusChat => {
                    self.view.focus = Focus::CommandBar;
                    self.view.screen = Screen::Chat;
                    self.refresh_addressee();
                    true
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
                self.clear_result();
                self.refresh_addressee();
                true
            }
            Key::Backspace => {
                self.view.command_input.pop();
                self.clear_result();
                self.refresh_addressee();
                true
            }
            Key::Enter => {
                self.submit_command();
                true
            }
            // Esc clears what is typed; with nothing typed on the Chat screen it
            // stops a reply being written, and failing that leaves the screen.
            Key::Esc if self.view.command_input.is_empty() && self.view.screen == Screen::Chat => {
                if self.view.chat.pending.is_some() {
                    self.stop_turn();
                } else {
                    self.view.screen = Screen::Overview;
                    self.view.focus = Focus::Board;
                }
                true
            }
            Key::Esc => {
                self.view.command_input.clear();
                self.clear_result();
                self.refresh_addressee();
                true
            }
            _ => false,
        }
    }

    /// Moving focus by cycling lands on a panel of the overview, so a
    /// full-width screen gives way to it (focus on the command bar keeps the
    /// screen: a command is typed about what is on screen).
    fn leave_full_screen(&mut self) {
        if self.view.focus != Focus::CommandBar {
            self.view.screen = Screen::Overview;
        }
    }

    /// Keys on the Kanban screen: arrows move the selection, Enter shows the
    /// card under it through the ordinary `/board show` command.
    fn kanban_key(&mut self, key: Key) -> bool {
        let Projection::Available(board) = &self.view.snapshot.board else {
            return false;
        };
        let at = self.view.board_cursor.clamped(board);
        let in_column = |column: usize| board.cards_in(BoardColumn::ALL[column]).count();
        let next = match key {
            Key::Left => BoardCursor {
                column: at.column.saturating_sub(1),
                row: at.row,
            },
            Key::Right => BoardCursor {
                column: (at.column + 1).min(BoardColumn::ALL.len() - 1),
                row: at.row,
            },
            Key::Up => BoardCursor {
                row: at.row.saturating_sub(1),
                ..at
            },
            Key::Down => BoardCursor {
                row: (at.row + 1).min(in_column(at.column).saturating_sub(1)),
                ..at
            },
            Key::Enter => {
                let Some(card) = at.card(board) else {
                    return false;
                };
                self.pending_dispatch = Some(SlashCommand {
                    verb: "board".to_string(),
                    args: vec!["show".to_string(), card.id.clone()],
                });
                return true;
            }
            _ => return false,
        };
        let next = next.clamped(board);
        let moved = next != self.view.board_cursor;
        self.view.board_cursor = next;
        moved
    }

    /// Keys on the Sessions screen: up and down move the selection and read
    /// the newly selected session's conversation.
    fn sessions_key(&mut self, key: Key) -> bool {
        let Projection::Available(list) = &self.view.snapshot.session_list else {
            return false;
        };
        let last = list.rows.len().saturating_sub(1);
        let at = self.view.session_cursor.min(last);
        let next = match key {
            Key::Up => at.saturating_sub(1),
            Key::Down => (at + 1).min(last),
            _ => return false,
        };
        let moved = next != self.view.session_cursor;
        self.view.session_cursor = next;
        self.sync_transcript();
        moved
    }

    /// Make the transcript match the selected session: read it when the
    /// selection or the journal's length differs from what was read, drop it
    /// when there is nothing selected.
    ///
    /// The read runs beside the loop. Until it answers, the conversation of
    /// the same session stays on screen (it only grew); that of another
    /// session does not — it would sit under the wrong header.
    fn sync_transcript(&mut self) {
        let Projection::Available(list) = &self.view.snapshot.session_list else {
            self.view.transcript = None;
            return;
        };
        let Some(row) = list.rows.get(
            self.view
                .session_cursor
                .min(list.rows.len().saturating_sub(1)),
        ) else {
            self.view.transcript = None;
            return;
        };
        let key = format!("{}:{}", row.id, row.entries);
        let id = row.id.clone();
        if self
            .view
            .transcript
            .as_ref()
            .is_some_and(|(read, _)| *read == key)
        {
            return;
        }
        if self
            .view
            .transcript
            .as_ref()
            .is_some_and(|(read, _)| !read.starts_with(&format!("{id}:")))
        {
            self.view.transcript = None;
        }
        if self.transcript_reading.as_deref() == Some(key.as_str()) {
            return;
        }
        self.transcript_reading = Some(key.clone());
        let mut args = ArgValues::new();
        args.insert("id", ArgValue::Text(id));
        self.work.spawn(
            Kind::Transcript,
            &self.registry,
            &self.dispatcher,
            move |registry, dispatcher| Finished::Transcript {
                key,
                projection: dispatch_projection(
                    registry,
                    dispatcher,
                    "core:session.show",
                    args,
                    parse_transcript,
                ),
            },
        );
    }

    /// Run a slash command beside the loop. An installation verb is not
    /// addressed to any agent; its answer is a block, not the bar's one line.
    /// Anything else is an ordinary command. One command runs at a time: a
    /// second line is held back with a note, never queued silently.
    fn run_command(&mut self, command: SlashCommand) {
        if self.command_running {
            self.view.command_feedback = Some("a command is still running".to_string());
            return;
        }
        if self.relaunch_pending {
            self.view.command_feedback = Some(RESTARTING.to_string());
            return;
        }
        self.command_running = true;
        // Only the verb and its first word: what follows may be a secret the
        // person typed, and the line has already left the input block.
        self.view.command_feedback = Some(match command.args.first() {
            Some(sub_verb) => format!("running /{} {sub_verb} …", command.verb),
            None => format!("running /{} …", command.verb),
        });
        let setting = self.setting;
        self.work.spawn(
            Kind::Command,
            &self.registry,
            &self.dispatcher,
            move |registry, dispatcher| {
                Finished::Command(
                    match installation::prepare(registry, dispatcher, &command, &setting) {
                        Some(Step::Done(block)) => CommandResult::Block(block),
                        Some(Step::Ask(question)) => CommandResult::Ask(question),
                        None => CommandResult::Feedback(dispatch::dispatch_command(
                            registry, dispatcher, &command,
                        )),
                    },
                )
            },
        );
    }

    /// Apply every job that has finished since the last tick; returns whether
    /// anything on screen changed.
    fn poll_work(&mut self) -> bool {
        let mut changed = false;
        while let Some(finished) = self.work.next() {
            changed |= self.apply(finished);
        }
        changed
    }

    fn apply(&mut self, finished: Finished) -> bool {
        match finished {
            Finished::Command(result) => {
                self.command_running = false;
                self.stale = true;
                match result {
                    CommandResult::Block(block) => {
                        self.view.command_feedback = None;
                        self.view.result_block = Some(block);
                    }
                    CommandResult::Applied { block, relaunch } => {
                        self.view.command_feedback = None;
                        self.view.result_block = Some(block);
                        self.relaunch_pending |= relaunch;
                    }
                    CommandResult::Feedback(feedback) => self.view.command_feedback = feedback,
                    CommandResult::Ask(question) => {
                        self.view.command_feedback = None;
                        self.view.result_block = Some(question.block().clone());
                        self.awaiting = Some(question);
                    }
                }
                true
            }
            Finished::Transcript { key, projection } => {
                if self.transcript_reading.as_deref() == Some(key.as_str()) {
                    self.transcript_reading = None;
                }
                let wanted = match &self.view.snapshot.session_list {
                    Projection::Available(list) => list
                        .rows
                        .get(
                            self.view
                                .session_cursor
                                .min(list.rows.len().saturating_sub(1)),
                        )
                        .map(|row| format!("{}:{}", row.id, row.entries)),
                    Projection::Unavailable { .. } => None,
                };
                if wanted.as_deref() == Some(key.as_str()) {
                    self.view.transcript = Some((key, projection));
                } else {
                    // The selection moved on while this was being read.
                    self.sync_transcript();
                }
                true
            }
            Finished::ChatTranscript(projection) => {
                self.view.chat.transcript = Some(projection);
                self.finish_reply();
                true
            }
            Finished::Stopped(reason) => {
                self.stops = self.stops.saturating_sub(1);
                match reason {
                    Some(reason) => {
                        self.view.command_feedback = Some(reason);
                        true
                    }
                    None => false,
                }
            }
            Finished::Failed(kind) => {
                match kind {
                    Kind::Command => self.command_running = false,
                    Kind::Transcript => self.transcript_reading = None,
                    Kind::ChatTranscript => self.finish_reply(),
                    Kind::Stop => self.stops = self.stops.saturating_sub(1),
                }
                self.view.command_feedback =
                    Some("background work ended without an answer".to_string());
                true
            }
        }
    }

    /// The line in the bar as a slash command. Everywhere but the Chat screen
    /// the slash is implicit; on it the line is typed whole, and a plain one
    /// is a message.
    fn typed_line(&self) -> String {
        if self.view.screen == Screen::Chat && self.view.command_input.starts_with('/') {
            self.view.command_input.clone()
        } else {
            format!("/{}", self.view.command_input)
        }
    }

    /// Apply a Chat screen command: choose the model, or leave the
    /// conversation for a new one.
    fn apply_chat_action(&mut self, action: ChatAction, command: &SlashCommand) {
        match action {
            ChatAction::Model => match command.args.get(1) {
                Some(name) => {
                    self.view.chat.model = Some(name.clone());
                    self.view.command_feedback = Some(format!("chat model: {name}"));
                }
                None => {
                    self.view.command_feedback = Some("usage: /chat model <name>".to_string());
                }
            },
            ChatAction::New => {
                if self.view.chat.pending.is_some() {
                    self.view.command_feedback =
                        Some("a reply is still being written — Esc stops it first".to_string());
                } else {
                    self.view.chat.session = None;
                    self.view.chat.transcript = None;
                    self.view.command_feedback = Some("new conversation".to_string());
                }
            }
        }
    }

    /// Send `text` to the model as the next message of the conversation. The
    /// turn runs through the shared `session say` verb on a background
    /// thread; the screen shows the message as pending until it finishes.
    fn send_message(&mut self, text: String) {
        if self.relaunch_pending {
            self.view.command_feedback = Some(RESTARTING.to_string());
            return;
        }
        if self.view.chat.pending.is_some() {
            self.view.command_feedback =
                Some("a reply is still being written — Esc stops it".to_string());
            return;
        }
        let Some(model) = self.view.chat.model.clone() else {
            self.view.command_feedback = Some("no model chosen: /chat model <name>".to_string());
            return;
        };
        let session = self
            .view
            .chat
            .session
            .clone()
            .unwrap_or_else(cronus_domain::session::journal::new_session_id);
        let mut args = ArgValues::new();
        args.insert("text", ArgValue::Text(text.clone()));
        args.insert("model", ArgValue::Text(model));
        args.insert("session", ArgValue::Text(session.clone()));
        let invocation = Invocation {
            id: InvocableId::new("core:session.say")
                .expect("a literal invocable id must be well-formed"),
            args,
            caller: Surface::Tui,
        };
        let (registry, dispatcher) = (Arc::clone(&self.registry), Arc::clone(&self.dispatcher));
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let outcome = match dispatcher.dispatch(&registry, &invocation) {
                // A turn answers with a stream: forward each piece as it
                // arrives, then the settled result.
                Dispatched::Ran(Outcome::Stream(handle)) => match dispatcher.subscribe(&handle) {
                    Some(mut subscription) => loop {
                        match subscription.next_item() {
                            Some(StreamItem::Chunk(OutcomeValue::Text(piece))) => {
                                if sender.send(TurnEvent::Chunk(piece)).is_err() {
                                    return;
                                }
                            }
                            Some(StreamItem::Chunk(_)) => {}
                            Some(StreamItem::End(outcome)) => break outcome,
                            None => {
                                break Outcome::Unavailable {
                                    reason: "the reply was lost".to_string(),
                                };
                            }
                        }
                    },
                    None => Outcome::Unavailable {
                        reason: "the reply cannot be read".to_string(),
                    },
                },
                Dispatched::Ran(outcome) => outcome,
                Dispatched::Unknown => Outcome::Unavailable {
                    reason: "core:session.say is not registered".to_string(),
                },
            };
            let _ = sender.send(TurnEvent::Done(outcome));
        });
        self.turn = Some(receiver);
        self.view.chat.session = Some(session);
        self.view.chat.pending = Some(text);
        self.view.chat.partial.clear();
    }

    /// Collect a finished reply, if there is one: clear the pending message,
    /// say what went wrong when something did, and re-read the conversation
    /// from its journal. Returns whether anything changed.
    fn poll_turn(&mut self) -> bool {
        let Some(receiver) = &self.turn else {
            return false;
        };
        let mut changed = false;
        let outcome = loop {
            match receiver.try_recv() {
                Ok(TurnEvent::Chunk(piece)) => {
                    self.view.chat.partial.push_str(&piece);
                    changed = true;
                }
                Ok(TurnEvent::Done(outcome)) => break outcome,
                Err(mpsc::TryRecvError::Empty) => return changed,
                Err(mpsc::TryRecvError::Disconnected) => {
                    break Outcome::Unavailable {
                        reason: "the reply was lost: its thread ended without an answer"
                            .to_string(),
                    };
                }
            }
        };
        self.turn = None;
        self.stale = true;
        // A recorded failure is told here; the journal holds it too.
        self.view.command_feedback = match outcome {
            Outcome::Value(_) => None,
            Outcome::Unavailable { reason } => Some(reason),
            other => dispatch::render_outcome(other),
        };
        self.refresh_chat_transcript();
        true
    }

    /// Read the conversation back from its session's journal, beside the
    /// loop. The message and the reply stay on screen until the read answers,
    /// so the reply never blinks out between the stream's end and its record.
    fn refresh_chat_transcript(&mut self) {
        let Some(session) = self.view.chat.session.clone() else {
            self.view.chat.transcript = None;
            self.finish_reply();
            return;
        };
        self.settling = true;
        let mut args = ArgValues::new();
        args.insert("id", ArgValue::Text(session));
        self.work.spawn(
            Kind::ChatTranscript,
            &self.registry,
            &self.dispatcher,
            move |registry, dispatcher| {
                Finished::ChatTranscript(dispatch_projection(
                    registry,
                    dispatcher,
                    "core:session.show",
                    args,
                    parse_transcript,
                ))
            },
        );
    }

    /// The turn is over and its conversation is on screen: nothing is pending.
    fn finish_reply(&mut self) {
        self.settling = false;
        self.view.chat.pending = None;
        self.view.chat.partial.clear();
    }

    /// Stop the reply being written: the turn is cut short and recorded as
    /// such, with whatever had arrived.
    fn stop_turn(&mut self) {
        let Some(session) = self.view.chat.session.clone() else {
            return;
        };
        let invocation = Invocation {
            id: InvocableId::new("core:session.stop")
                .expect("a literal invocable id must be well-formed"),
            args: {
                let mut args = ArgValues::new();
                args.insert("id", ArgValue::Text(session));
                args
            },
            caller: Surface::Tui,
        };
        self.stops += 1;
        self.work.spawn(
            Kind::Stop,
            &self.registry,
            &self.dispatcher,
            move |registry, dispatcher| {
                Finished::Stopped(match dispatcher.dispatch(registry, &invocation) {
                    Dispatched::Ran(Outcome::Unavailable { reason }) => Some(reason),
                    _ => None,
                })
            },
        );
    }

    /// Put the command listing on screen as a result block.
    fn show_help(&mut self) {
        self.clear_result();
        self.view.result_block = Some(ResultBlock {
            title: "/help".to_string(),
            lines: command::help_lines(&self.catalog),
        });
    }

    /// Recompute who the line being typed is addressed to: an installation
    /// line is addressed to no agent, and the input block says so while it
    /// is being typed.
    fn refresh_addressee(&mut self) {
        let chat = self.view.screen == Screen::Chat;
        let input = &self.view.command_input;
        if chat && !input.is_empty() && !input.starts_with('/') {
            self.view.addressee = Addressee::Model;
            return;
        }
        let line = self.typed_line();
        self.view.addressee = match command::parse(&line) {
            Ok(parsed) if installation::is_installation_command(&self.registry, &parsed) => {
                Addressee::Installation
            }
            _ => Addressee::Command,
        };
    }

    /// Drop whatever the last command left on screen — its one-line feedback
    /// and its result block — as the next keystroke begins a new line.
    fn clear_result(&mut self) {
        self.view.command_feedback = None;
        self.view.result_block = None;
    }

    /// Classify the current command line. Help and errors resolve inline; a
    /// recognized command is queued for the loop to dispatch (the loop holds the
    /// registry/dispatcher and performs the real call + masking).
    fn submit_command(&mut self) {
        self.clear_result();
        let chat = self.view.screen == Screen::Chat;
        let typed = self.view.command_input.clone();
        if chat && !typed.trim().is_empty() && !typed.starts_with('/') {
            self.view.command_input.clear();
            self.view.addressee = Addressee::Command;
            self.send_message(typed.trim().to_string());
            return;
        }
        let line = self.typed_line();
        match command::classify(&line, &self.catalog) {
            CommandOutcome::Help => self.show_help(),
            CommandOutcome::Run(command) => self.pending_dispatch = Some(command),
            CommandOutcome::Error(message) => self.view.command_feedback = Some(message),
        }
        self.view.command_input.clear();
        self.view.addressee = Addressee::Command;
    }
}

/// What the background read of the core produces: the three projections
/// dispatched through the registry.
#[derive(Default)]
struct Reads {
    board: Projection<BoardView>,
    office: Projection<OfficeView>,
    session_list: Projection<SessionList>,
}

impl Reads {
    /// What stands in for a read whose thread ended without an answer.
    fn lost() -> Self {
        fn gone<T>() -> Projection<T> {
            Projection::Unavailable {
                reason: "the background read ended without an answer".to_string(),
            }
        }
        Self {
            board: gone(),
            office: gone(),
            session_list: gone(),
        }
    }
}

/// Production [`SnapshotSource`] over the core capability surface.
///
/// The version and status come from the core handle each poll — a cheap call.
/// The board, the roster and the sessions list are dispatched through the
/// registry, which touches the disk (journals are parsed in full to be
/// summarized), so they are read on a thread of their own, about twice a
/// second or sooner when a command has just run. A poll never waits for the
/// read: it returns the snapshot made of the last finished one, and
/// [`App::tick`] dedupes by equality so an unchanged read triggers no redraw.
pub struct CapabilitySource<C: Capabilities> {
    core: C,
    /// The last finished background read.
    latest: Reads,
    /// The read in flight, if any.
    reading: Option<mpsc::Receiver<Reads>>,
    /// When the last read finished.
    read_at: Option<std::time::Instant>,
    /// The next poll should start a read whatever the interval says.
    stale: bool,
}

/// How long a finished read is reused before another is started.
const REFRESH: Duration = Duration::from_millis(500);

impl<C: Capabilities> CapabilitySource<C> {
    /// Wrap a core handle as a snapshot source.
    pub fn new(core: C) -> Self {
        Self {
            core,
            latest: Reads::default(),
            reading: None,
            read_at: None,
            stale: false,
        }
    }
}

impl<C: Capabilities> SnapshotSource for CapabilitySource<C> {
    fn poll_snapshot(
        &mut self,
        registry: &Arc<InvocableRegistry>,
        dispatcher: &Arc<Dispatcher>,
    ) -> Option<CoreSnapshot> {
        if let Some(receiver) = &self.reading {
            match receiver.try_recv() {
                Ok(reads) => {
                    self.latest = reads;
                    self.reading = None;
                    self.read_at = Some(std::time::Instant::now());
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.latest = Reads::lost();
                    self.reading = None;
                    self.read_at = Some(std::time::Instant::now());
                }
            }
        }
        if self.reading.is_none()
            && (self.stale || self.read_at.is_none_or(|at| at.elapsed() >= REFRESH))
        {
            self.stale = false;
            let (sender, receiver) = mpsc::channel();
            let (registry, dispatcher) = (Arc::clone(registry), Arc::clone(dispatcher));
            std::thread::spawn(move || {
                let _ = sender.send(Reads {
                    board: dispatch_board(&registry, &dispatcher),
                    office: dispatch_office(&registry, &dispatcher),
                    session_list: dispatch_session_list(&registry, &dispatcher),
                });
            });
            self.reading = Some(receiver);
        }
        Some(CoreSnapshot {
            status: Projection::Available(StatusView {
                version: self.core.version().to_string(),
                status: self.core.status(),
            }),
            board: self.latest.board.clone(),
            office: self.latest.office.clone(),
            sessions: sessions_panel(&self.latest.session_list),
            session_list: self.latest.session_list.clone(),
        })
    }

    fn invalidate(&mut self) {
        self.stale = true;
    }
}

/// Dispatch `core:session.list` and project its rows.
fn dispatch_session_list(
    registry: &InvocableRegistry,
    dispatcher: &Dispatcher,
) -> Projection<SessionList> {
    dispatch_projection(
        registry,
        dispatcher,
        "core:session.list",
        ArgValues::new(),
        |value| {
            let OutcomeValue::List(items) = value else {
                return None;
            };
            let mut rows = Vec::with_capacity(items.len());
            for item in items {
                let OutcomeValue::Record(fields) = item else {
                    return None;
                };
                let count = |name: &str| match fields.iter().find(|(k, _)| k == name) {
                    Some((_, OutcomeValue::Integer(n))) => usize::try_from(*n).ok(),
                    _ => None,
                };
                rows.push(SessionRow {
                    id: record_text(fields, "id")?.to_string(),
                    name: record_text(fields, "name").map(str::to_string),
                    turns: count("turns")?,
                    entries: count("entries")?,
                    problem: record_text(fields, "problem").map(str::to_string),
                });
            }
            Some(SessionList { rows })
        },
    )
}

/// The overview's Sessions panel: one line per session, from the same list
/// the Sessions screen shows — unavailable when the list is.
fn sessions_panel(list: &Projection<SessionList>) -> Projection<SessionsView> {
    match list {
        Projection::Available(list) => {
            let mut view = SessionsView::default();
            // Oldest first, so the newest session is the last line.
            for row in list.rows.iter().rev() {
                view.push(format!(
                    "{} · {} · {} turns",
                    row.id,
                    row.name.as_deref().unwrap_or("(unnamed)"),
                    row.turns
                ));
            }
            Projection::Available(view)
        }
        Projection::Unavailable { reason } => Projection::Unavailable {
            reason: reason.clone(),
        },
    }
}

/// Parse `core:session.show`'s record into a transcript.
fn parse_transcript(value: &OutcomeValue) -> Option<TranscriptView> {
    let OutcomeValue::Record(fields) = value else {
        return None;
    };
    let omitted = match fields.iter().find(|(k, _)| k == "omitted") {
        Some((_, OutcomeValue::Integer(n))) => usize::try_from(*n).ok()?,
        _ => 0,
    };
    let Some((_, OutcomeValue::List(items))) = fields.iter().find(|(k, _)| k == "messages") else {
        return None;
    };
    let mut messages = Vec::with_capacity(items.len());
    for item in items {
        let OutcomeValue::Record(message) = item else {
            return None;
        };
        // A reply that did not complete says how in its header, and one that
        // produced nothing shows why instead of a blank.
        let role = record_text(message, "role")?;
        let text = record_text(message, "text")?;
        let label = match record_text(message, "stop") {
            Some(stop) if stop != "stop" => format!("{role} · {stop}"),
            _ => role.to_string(),
        };
        let text = match (text.is_empty(), record_text(message, "error")) {
            (true, Some(error)) => format!("({error})"),
            _ => text.to_string(),
        };
        messages.push((label, text));
    }
    Some(TranscriptView {
        id: record_text(fields, "id")?.to_string(),
        messages,
        omitted,
    })
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
                        title: record_text(fields, "task_ref")
                            .unwrap_or_default()
                            .to_string(),
                        priority: record_text(fields, "priority")
                            .unwrap_or_default()
                            .to_string(),
                        assignee: record_text(fields, "assignee")
                            .unwrap_or_default()
                            .to_string(),
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
                    column: view::BoardColumn::Archive,
                    ..Default::default()
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

    view::render_tabs(areas.tabs, buf, view.focus);

    match view.screen {
        Screen::Overview => {
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
        }
        Screen::Chat => {
            view::render_chat_screen(areas.body, buf, &view.chat, view.focus == Focus::CommandBar)
        }
        Screen::Sessions => view::render_sessions_screen(
            areas.body,
            buf,
            &snapshot.session_list,
            view.session_cursor,
            view.transcript.as_ref().map(|(_, transcript)| transcript),
            view.focus == Focus::Sessions,
        ),
        Screen::Kanban => view::render_kanban(
            areas.body,
            buf,
            &snapshot.board,
            view.board_cursor,
            view.focus == Focus::Board,
        ),
    }

    if let Some(block) = &view.result_block {
        // Anchored above the ticker, below the tab bar.
        let below_tabs = Rect::new(
            area.x,
            areas.tabs.bottom(),
            area.width,
            area.height.saturating_sub(areas.tabs.height),
        );
        view::render_result_block(below_tabs, buf, areas.ticker, block);
    }

    view::render_ticker(areas.ticker, buf, view.command_feedback.as_deref());
    view::render_input(
        areas.input,
        buf,
        &view.command_input,
        view.addressee,
        view.screen != Screen::Chat,
        view.focus == Focus::CommandBar,
    );
    view::render_fkeys(areas.fkeys, buf, pane_actions::fkey_for_focus(view.focus));
    view::render_status_line(areas.status_line, buf, &snapshot.status);
}

/// A composition this frontend runs in: the registry and dispatcher it
/// dispatches through, the catalog derived from them, the source its snapshots
/// are read through, and where it stands in relation to the engine.
pub struct Hosted<S> {
    pub registry: Arc<InvocableRegistry>,
    pub dispatcher: Arc<Dispatcher>,
    pub catalog: Vec<CommandSpec>,
    pub source: S,
    pub setting: Setting,
    /// What makes this process the engine of its state root or a spoke of
    /// another's — the seat, the link. It is held for as long as the
    /// composition runs and let go when a relaunch replaces it, so the next
    /// composition takes the role afresh.
    pub keep: Box<dyn std::any::Any>,
}

/// Compose this frontend over the live installation: the core's registry, this
/// surface's own actions registered through the same door, the catalog derived
/// from the result — and the role over the state root.
pub fn compose_live() -> io::Result<Hosted<CapabilitySource<Engine>>> {
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
    crate::chat_actions::register(&mut registry, &mut dispatcher);
    let invocables: Vec<&Invocable> = registry.all().collect();
    let catalog = command::build_catalog(&invocables);

    // One engine owns a state root; this terminal UI is either that engine
    // (and serves the composition it just built, so other processes can reach
    // it) or a spoke of the one that is (and its engine-run handlers are
    // re-pointed there). Either way it holds the role until it exits or is
    // relaunched.
    let joined = cronus_core::endpoint::join_composition(
        &cronus_core::paths::resolve_workspace_root(),
        registry,
        dispatcher,
        cronus_contract::Surface::Tui,
        cronus_core::endpoint::DEFAULT_PATIENCE,
    )
    .map_err(|err| io::Error::other(err.to_string()))?;
    let placement = match &joined.role {
        cronus_core::endpoint::Role::Engine(_) => Placement::Engine,
        cronus_core::endpoint::Role::Spoke(_) => Placement::Spoke,
    };

    Ok(Hosted {
        registry: joined.registry,
        dispatcher: joined.dispatcher,
        catalog,
        source: CapabilitySource::new(Engine::new()),
        setting: Setting::live(placement),
        keep: Box::new(joined.role),
    })
}

/// Run the TUI against the real terminal and the live engine.
pub fn run() -> io::Result<()> {
    run_with(
        CrosstermBackend::new(),
        RatatuiRenderer::new()?,
        compose_live,
    )
}

/// Run the loop against an injected backend and renderer, over the
/// compositions `compose` builds.
///
/// `compose` is called once to start, and again each time a change applied by
/// relaunch has left the composition out of date: the old one is dropped first
/// — its role over the state root with it — so the next takes the role afresh,
/// and the loop carries on in a new [`App`] onto the same screen, focus and
/// conversation with the answer to that change still showing. The terminal is
/// not left and re-entered between the two.
///
/// The terminal lifecycle is RAII-guarded by [`Tui`], so any exit path — normal,
/// `?` error, or panic — restores the terminal.
pub fn run_with<B, S, R, C>(backend: B, mut renderer: R, mut compose: C) -> io::Result<()>
where
    B: TerminalBackend,
    S: SnapshotSource,
    R: Renderer,
    C: FnMut() -> io::Result<Hosted<S>>,
{
    // The first composition is built before the terminal is taken over, so a
    // failure to compose is reported on the screen the person came from.
    let mut hosted = compose()?;
    let mut tui = Tui::new(backend)?;
    let mut resume: Option<ViewModel> = None;
    loop {
        let Hosted {
            registry,
            dispatcher,
            catalog,
            mut source,
            setting,
            keep,
        } = hosted;
        let (cols, rows) = tui.size()?;
        let mut app = match resume.take() {
            Some(mut view) => {
                view.size = (cols, rows);
                App::shared(view, registry, dispatcher, catalog)
                    .placed(setting)
                    .after_relaunch()
            }
            None => App::shared(
                ViewModel {
                    size: (cols, rows),
                    chat: ChatView {
                        model: std::env::var("CRONUS_CHAT_MODEL")
                            .ok()
                            .filter(|m| !m.is_empty()),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                registry,
                dispatcher,
                catalog,
            )
            .placed(setting),
        };

        // Initial frame so the screen is populated before the first event.
        renderer.draw(app.view())?;

        let relaunch = loop {
            let mut events = Vec::new();
            if let Some(event) = tui.poll_event(TICK)? {
                events.push(event);
            }
            let result = app.tick(&events, &mut source, &mut renderer)?;
            if result.quit {
                break false;
            }
            if result.relaunch {
                break true;
            }
        };
        if !relaunch {
            break;
        }

        resume = Some(app.into_view());
        drop(source);
        drop(keep);
        hosted = compose()?;
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
            _registry: &Arc<InvocableRegistry>,
            _dispatcher: &Arc<Dispatcher>,
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

    /// Tick until the work the loop started beside itself has been applied.
    fn settle(app: &mut App) {
        for _ in 0..2000 {
            if !app.is_busy() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
            app.tick(
                &[],
                &mut ScriptedSource::new(vec![None]),
                &mut RecordingRenderer::default(),
            )
            .unwrap();
        }
        panic!("the work started beside the loop never finished");
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
            installation: false,
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

    /// The function-key bar runs the same registered actions the keyboard
    /// shortcuts do: a bound slot moves focus or quits, an unbound one does
    /// nothing at all.
    #[test]
    fn function_keys_run_their_registered_pane_actions_and_unbound_ones_do_nothing() {
        let tick = |app: &mut App, key: Key| {
            app.tick(
                &[TermEvent::Key(key)],
                &mut ScriptedSource::new(vec![None]),
                &mut RecordingRenderer::default(),
            )
            .unwrap()
        };
        let (registry, dispatcher) = registry_with_pane_actions();
        let mut app = App::new(ViewModel::default(), registry, dispatcher, Vec::new());

        tick(&mut app, Key::F(6));
        assert_eq!(app.view().focus, Focus::Office);
        tick(&mut app, Key::F(4));
        assert_eq!(app.view().focus, Focus::Sessions);
        tick(&mut app, Key::F(7));
        assert_eq!(app.view().focus, Focus::Board);

        let before = app.view().clone();
        let result = tick(&mut app, Key::F(2));
        assert!(!result.redrawn && !result.quit, "F2 has no action yet");
        assert_eq!(app.view(), &before);

        assert!(tick(&mut app, Key::F(10)).quit, "F10 quits");
    }

    /// The Kanban screen is reached by F7, steered by the arrow keys over the
    /// board projection, and left by Esc (before Esc can quit) or by moving
    /// focus to another panel.
    #[test]
    fn the_kanban_screen_is_entered_navigated_shown_and_left() {
        let (mut registry, mut dispatcher) = registry_with_pane_actions();
        register_board_list(
            &mut registry,
            &mut dispatcher,
            vec![
                ("card-1", "todo"),
                ("card-2", "running"),
                ("card-3", "running"),
            ],
            Ok(Vec::new()),
        );
        let board = dispatch_board(&registry, &dispatcher);
        let mut app = App::new(
            ViewModel {
                snapshot: CoreSnapshot {
                    board,
                    ..Default::default()
                },
                ..Default::default()
            },
            registry,
            dispatcher,
            Vec::new(),
        );
        let press = |app: &mut App, key: Key| {
            let result = app
                .tick(
                    &[TermEvent::Key(key)],
                    &mut ScriptedSource::new(vec![None]),
                    &mut RecordingRenderer::default(),
                )
                .unwrap();
            settle(app);
            result
        };

        press(&mut app, Key::F(7));
        assert_eq!(app.view().screen, Screen::Kanban);
        assert_eq!(app.view().focus, Focus::Board);

        // Running is the fourth column; Todo the second.
        for _ in 0..3 {
            press(&mut app, Key::Right);
        }
        assert_eq!(app.view().board_cursor, BoardCursor { column: 3, row: 0 });
        press(&mut app, Key::Down);
        press(&mut app, Key::Down);
        assert_eq!(app.view().board_cursor.row, 1, "stops at the last card");
        press(&mut app, Key::Left);
        assert_eq!(
            app.view().board_cursor,
            BoardCursor { column: 2, row: 0 },
            "an empty column holds the row at 0"
        );

        press(&mut app, Key::Right);
        press(&mut app, Key::Enter);
        let shown = app.view().command_feedback.clone();
        assert!(
            shown.as_deref().is_some_and(|text| text.contains("card-2")),
            "Enter shows the selected card through /board show: {shown:?}"
        );

        let result = press(&mut app, Key::Esc);
        assert!(!result.quit, "Esc leaves the board before it quits");
        assert_eq!(app.view().screen, Screen::Overview);
        assert!(press(&mut app, Key::Esc).quit, "and the next Esc quits");

        press(&mut app, Key::F(7));
        press(&mut app, Key::Tab);
        assert_eq!(
            app.view().screen,
            Screen::Overview,
            "cycling focus leaves it"
        );
        press(&mut app, Key::F(7));
        press(&mut app, Key::F(6));
        assert_eq!(app.view().screen, Screen::Overview);
        assert_eq!(app.view().focus, Focus::Office);
    }

    /// A command is typed about what is on screen: focusing the command bar
    /// keeps the board, and the arrow keys then belong to the line.
    #[test]
    fn focusing_the_command_bar_keeps_the_kanban_screen() {
        let (registry, dispatcher) = registry_with_pane_actions();
        let mut app = App::new(
            ViewModel {
                screen: Screen::Kanban,
                focus: Focus::Board,
                ..Default::default()
            },
            registry,
            dispatcher,
            Vec::new(),
        );
        let press = |app: &mut App, key: Key| {
            let result = app
                .tick(
                    &[TermEvent::Key(key)],
                    &mut ScriptedSource::new(vec![None]),
                    &mut RecordingRenderer::default(),
                )
                .unwrap();
            settle(app);
            result
        };
        press(&mut app, Key::Char('/'));
        assert_eq!(app.view().focus, Focus::CommandBar);
        assert_eq!(app.view().screen, Screen::Kanban);
        let before = app.view().board_cursor;
        press(&mut app, Key::Right);
        assert_eq!(app.view().board_cursor, before);
    }

    #[test]
    fn the_kanban_screen_replaces_the_four_panels_in_the_frame() {
        let mut view = populated_view();
        view.screen = Screen::Kanban;
        let area = Rect::new(0, 0, 120, 30);
        let text = buffer_text(&render_to_buffer(&view, area), area);
        assert!(text.contains("Kanban ·"));
        assert!(!text.contains("Sessions─"), "the overview panels are gone");
        assert!(text.contains("1Help"), "the frame around it stays");
    }

    /// A fixture journal: id, entry count, and `(role, text)` messages.
    type FixtureJournal = (&'static str, i64, Vec<(&'static str, &'static str)>);

    /// Registers a fixture `core:session.list` / `core:session.show` pair
    /// over `(id, entries, messages)` journals, and counts the reads of each
    /// transcript.
    fn register_sessions(
        registry: &mut InvocableRegistry,
        dispatcher: &mut Dispatcher,
        journals: Vec<FixtureJournal>,
    ) -> Arc<std::sync::atomic::AtomicUsize> {
        let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let register = |registry: &mut InvocableRegistry, tail: &str, binders: Vec<Binder>| {
            let id = InvocableId::new(format!("core:{tail}")).expect("well-formed test id");
            registry
                .register(
                    &Registrant::core(),
                    Invocable {
                        id: id.clone(),
                        name: "Fixture",
                        summary: "Test-only session fixture.",
                        group: "session",
                        locus: Locus::Semantic,
                        binders,
                        stability: Stability::Shipped,
                        journal_raw_input: true,
                    },
                )
                .expect("fixture registers cleanly");
            id
        };
        let text = |s: &str| OutcomeValue::Text(s.to_string());

        let list_id = register(registry, "session.list", Vec::new());
        let rows = journals.clone();
        dispatcher.attach(
            list_id,
            Arc::new(move |_args| {
                Outcome::Value(OutcomeValue::List(
                    rows.iter()
                        .map(|(id, entries, _)| {
                            OutcomeValue::Record(vec![
                                ("id".to_string(), OutcomeValue::Text((*id).to_string())),
                                ("name".to_string(), OutcomeValue::Empty),
                                ("entries".to_string(), OutcomeValue::Integer(*entries)),
                                ("turns".to_string(), OutcomeValue::Integer(1)),
                                ("problem".to_string(), OutcomeValue::Empty),
                            ])
                        })
                        .collect(),
                ))
            }),
        );
        let show_id = register(
            registry,
            "session.show",
            vec![Binder {
                name: "id",
                kind: BinderKind::Text,
                optional: false,
            }],
        );
        let counter = Arc::clone(&reads);
        dispatcher.attach(
            show_id,
            Arc::new(move |args| {
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let wanted = match args.get("id") {
                    Some(ArgValue::Text(id)) => id.as_str(),
                    _ => "",
                };
                match journals.iter().find(|(id, _, _)| *id == wanted) {
                    Some((id, _, messages)) => Outcome::Value(OutcomeValue::Record(vec![
                        ("id".to_string(), text(id)),
                        ("omitted".to_string(), OutcomeValue::Integer(0)),
                        (
                            "messages".to_string(),
                            OutcomeValue::List(
                                messages
                                    .iter()
                                    .map(|(role, body)| {
                                        OutcomeValue::Record(vec![
                                            ("role".to_string(), text(role)),
                                            ("text".to_string(), text(body)),
                                        ])
                                    })
                                    .collect(),
                            ),
                        ),
                    ])),
                    None => Outcome::Unavailable {
                        reason: format!("session not found: {wanted}"),
                    },
                }
            }),
        );
        reads
    }

    /// F4 opens the Sessions screen; the selected session's conversation is
    /// read once and again only when the selection or the journal changes;
    /// Esc leaves.
    #[test]
    fn the_sessions_screen_follows_the_selection_and_rereads_only_what_changed() {
        let (mut registry, mut dispatcher) = registry_with_pane_actions();
        let reads = register_sessions(
            &mut registry,
            &mut dispatcher,
            vec![
                ("s-new", 4, vec![("user", "hi"), ("assistant", "hello")]),
                ("s-old", 2, vec![("user", "earlier")]),
            ],
        );
        let list = dispatch_session_list(&registry, &dispatcher);
        assert!(matches!(&list, Projection::Available(l) if l.rows.len() == 2));
        let mut app = App::new(
            ViewModel {
                snapshot: CoreSnapshot {
                    session_list: list,
                    ..Default::default()
                },
                ..Default::default()
            },
            registry,
            dispatcher,
            Vec::new(),
        );
        let press = |app: &mut App, key: Key| {
            let result = app
                .tick(
                    &[TermEvent::Key(key)],
                    &mut ScriptedSource::new(vec![None]),
                    &mut RecordingRenderer::default(),
                )
                .unwrap();
            settle(app);
            result
        };
        let reads_so_far = || reads.load(std::sync::atomic::Ordering::SeqCst);
        let shown_id = |app: &App| app.view().transcript.as_ref().map(|(key, _)| key.clone());

        press(&mut app, Key::F(4));
        assert_eq!(app.view().screen, Screen::Sessions);
        assert_eq!(shown_id(&app).as_deref(), Some("s-new:4"));
        assert_eq!(reads_so_far(), 1);

        press(&mut app, Key::Up);
        assert_eq!(reads_so_far(), 1, "no movement, no re-read");

        press(&mut app, Key::Down);
        assert_eq!(shown_id(&app).as_deref(), Some("s-old:2"));
        assert_eq!(reads_so_far(), 2);
        press(&mut app, Key::Down);
        assert_eq!(app.view().session_cursor, 1, "stops at the last session");

        let result = press(&mut app, Key::Esc);
        assert!(!result.quit);
        assert_eq!(app.view().screen, Screen::Overview);
    }

    #[test]
    fn an_unreadable_journal_shows_up_as_an_unavailable_transcript_not_a_blank_one() {
        let (mut registry, mut dispatcher) = registry_with_pane_actions();
        // The list names a session the show call cannot find.
        register_sessions(&mut registry, &mut dispatcher, vec![("s-1", 1, Vec::new())]);
        let mut list = dispatch_session_list(&registry, &dispatcher);
        if let Projection::Available(l) = &mut list {
            l.rows[0].id = "s-ghost".to_string();
        }
        let mut app = App::new(
            ViewModel {
                snapshot: CoreSnapshot {
                    session_list: list,
                    ..Default::default()
                },
                ..Default::default()
            },
            registry,
            dispatcher,
            Vec::new(),
        );
        app.tick(
            &[TermEvent::Key(Key::F(4))],
            &mut ScriptedSource::new(vec![None]),
            &mut RecordingRenderer::default(),
        )
        .unwrap();
        settle(&mut app);
        assert!(matches!(
            app.view().transcript,
            Some((_, Projection::Unavailable { ref reason })) if reason.contains("s-ghost")
        ));
    }

    #[test]
    fn the_overview_sessions_panel_lists_the_same_sessions_and_inherits_unavailability() {
        let list = Projection::Available(SessionList {
            rows: vec![
                SessionRow {
                    id: "s-new".to_string(),
                    turns: 3,
                    ..Default::default()
                },
                SessionRow {
                    id: "s-old".to_string(),
                    name: Some("first".to_string()),
                    turns: 1,
                    ..Default::default()
                },
            ],
        });
        let Projection::Available(panel) = sessions_panel(&list) else {
            panic!("an available list gives an available panel");
        };
        assert_eq!(
            panel.entries(),
            ["s-old · first · 1 turns", "s-new · (unnamed) · 3 turns"],
            "oldest first, so the newest is the last line"
        );
        let down = Projection::Unavailable {
            reason: "cannot read sessions".to_string(),
        };
        assert_eq!(
            sessions_panel(&down),
            Projection::Unavailable {
                reason: "cannot read sessions".to_string()
            }
        );
    }

    #[test]
    fn a_reply_that_did_not_complete_says_how_in_its_header_and_why_in_its_body() {
        let text = |s: &str| OutcomeValue::Text(s.to_string());
        let message = |role: &str, body: &str, stop: OutcomeValue, error: OutcomeValue| {
            OutcomeValue::Record(vec![
                ("role".to_string(), text(role)),
                ("text".to_string(), text(body)),
                ("stop".to_string(), stop),
                ("error".to_string(), error),
            ])
        };
        let value = OutcomeValue::Record(vec![
            ("id".to_string(), text("s-1")),
            ("omitted".to_string(), OutcomeValue::Integer(0)),
            (
                "messages".to_string(),
                OutcomeValue::List(vec![
                    message("user", "hi", OutcomeValue::Empty, OutcomeValue::Empty),
                    message("assistant", "fine", text("stop"), OutcomeValue::Empty),
                    message("assistant", "", text("error"), text("Timeout")),
                ]),
            ),
        ]);
        let transcript = parse_transcript(&value).expect("a well-formed transcript");
        assert_eq!(
            transcript.messages,
            vec![
                ("user".to_string(), "hi".to_string()),
                ("assistant".to_string(), "fine".to_string()),
                ("assistant · error".to_string(), "(Timeout)".to_string()),
            ]
        );
    }

    #[test]
    fn dispatch_session_list_reports_unavailable_when_the_core_call_is_not_registered() {
        let registry = InvocableRegistry::new();
        let dispatcher = Dispatcher::new();
        assert!(matches!(
            dispatch_session_list(&registry, &dispatcher),
            Projection::Unavailable { .. }
        ));
    }

    /// How the fixture `session.say` behaves.
    #[derive(Clone, Copy)]
    enum Say {
        /// Answers at once.
        Answers,
        /// Fails at once.
        Fails,
        /// Holds until `session.stop` is dispatched, then reports it cut short.
        UntilStopped,
        /// Streams "par", waits for `session.stop` as its release, then streams
        /// "tial" and settles.
        Streams,
    }

    /// Registers fixture `core:session.say`, `.show` and `.stop` over one
    /// shared flag, so the Chat screen is driven against the real dispatch
    /// path without a model. `show` returns the messages `say` was given.
    fn register_chat_core(
        registry: &mut InvocableRegistry,
        dispatcher: &mut Dispatcher,
        behavior: Say,
    ) {
        use std::sync::atomic::{AtomicBool, Ordering};
        let stopped = Arc::new(AtomicBool::new(false));
        let said: Arc<std::sync::Mutex<Vec<(String, String)>>> = Arc::default();
        let hub = dispatcher.streams();
        let register = |registry: &mut InvocableRegistry, tail: &str, binders: Vec<Binder>| {
            let id = InvocableId::new(format!("core:{tail}")).expect("well-formed test id");
            registry
                .register(
                    &Registrant::core(),
                    Invocable {
                        id: id.clone(),
                        name: "Fixture",
                        summary: "Test-only chat fixture.",
                        group: "session",
                        locus: Locus::Semantic,
                        binders,
                        stability: Stability::Shipped,
                        journal_raw_input: true,
                    },
                )
                .expect("fixture registers cleanly");
            id
        };
        let text_binder = |name: &'static str, kind: BinderKind, optional: bool| Binder {
            name,
            kind,
            optional,
        };
        let text_of = |args: &ArgValues, name: &str| match args.get(name) {
            Some(ArgValue::Text(value)) => value.clone(),
            _ => String::new(),
        };

        let say = register(
            registry,
            "session.say",
            vec![
                text_binder("text", BinderKind::Text, false),
                text_binder("model", BinderKind::NamedText, false),
                text_binder("session", BinderKind::NamedText, true),
            ],
        );
        let (flag, log) = (Arc::clone(&stopped), Arc::clone(&said));
        dispatcher.attach(
            say,
            Arc::new(move |args| {
                let (text, model) = (text_of(args, "text"), text_of(args, "model"));
                let mut log = log.lock().expect("lock");
                log.push(("user".to_string(), format!("{text} [{model}]")));
                match behavior {
                    Say::Answers => {
                        log.push(("assistant".to_string(), "an answer".to_string()));
                        Outcome::Value(OutcomeValue::Record(vec![(
                            "session".to_string(),
                            OutcomeValue::Text(text_of(args, "session")),
                        )]))
                    }
                    Say::Fails => Outcome::Unavailable {
                        reason: "cannot reach the model server".to_string(),
                    },
                    Say::Streams => {
                        log.push(("assistant".to_string(), "partial".to_string()));
                        drop(log);
                        let (handle, sink) = hub.open();
                        let release = Arc::clone(&flag);
                        let session = text_of(args, "session");
                        std::thread::spawn(move || {
                            sink.chunk(OutcomeValue::Text("par".to_string()));
                            for _ in 0..400 {
                                if release.load(Ordering::SeqCst) {
                                    break;
                                }
                                std::thread::sleep(std::time::Duration::from_millis(5));
                            }
                            sink.chunk(OutcomeValue::Text("tial".to_string()));
                            sink.end(Outcome::Value(OutcomeValue::Record(vec![
                                ("session".to_string(), OutcomeValue::Text(session)),
                                (
                                    "reply".to_string(),
                                    OutcomeValue::Text("partial".to_string()),
                                ),
                            ])));
                        });
                        Outcome::Stream(handle)
                    }
                    Say::UntilStopped => {
                        drop(log);
                        for _ in 0..400 {
                            if flag.load(Ordering::SeqCst) {
                                return Outcome::Unavailable {
                                    reason: "cancelled".to_string(),
                                };
                            }
                            std::thread::sleep(std::time::Duration::from_millis(5));
                        }
                        Outcome::Unavailable {
                            reason: "the fixture was never stopped".to_string(),
                        }
                    }
                }
            }),
        );

        let stop = register(
            registry,
            "session.stop",
            vec![text_binder("id", BinderKind::Text, false)],
        );
        let flag = Arc::clone(&stopped);
        dispatcher.attach(
            stop,
            Arc::new(move |_args| {
                flag.store(true, Ordering::SeqCst);
                Outcome::Value(OutcomeValue::Empty)
            }),
        );

        let show = register(
            registry,
            "session.show",
            vec![text_binder("id", BinderKind::Text, false)],
        );
        let log = Arc::clone(&said);
        dispatcher.attach(
            show,
            Arc::new(move |args| {
                let messages = log
                    .lock()
                    .expect("lock")
                    .iter()
                    .map(|(role, body)| {
                        OutcomeValue::Record(vec![
                            ("role".to_string(), OutcomeValue::Text(role.clone())),
                            ("text".to_string(), OutcomeValue::Text(body.clone())),
                        ])
                    })
                    .collect();
                Outcome::Value(OutcomeValue::Record(vec![
                    ("id".to_string(), OutcomeValue::Text(text_of(args, "id"))),
                    ("omitted".to_string(), OutcomeValue::Integer(0)),
                    ("messages".to_string(), OutcomeValue::List(messages)),
                ]))
            }),
        );
    }

    /// An app on the Chat screen over the fixture core and the real chat and
    /// pane actions.
    fn chat_app(behavior: Say) -> App {
        let (mut registry, mut dispatcher) = registry_with_pane_actions();
        crate::chat_actions::register(&mut registry, &mut dispatcher);
        register_chat_core(&mut registry, &mut dispatcher, behavior);
        let invocables: Vec<&Invocable> = registry.all().collect();
        let catalog = command::build_catalog(&invocables);
        App::new(ViewModel::default(), registry, dispatcher, catalog)
    }

    fn type_and_enter(app: &mut App, line: &str) {
        let mut events: Vec<TermEvent> =
            line.chars().map(|c| TermEvent::Key(Key::Char(c))).collect();
        events.push(TermEvent::Key(Key::Enter));
        app.tick(
            &events,
            &mut ScriptedSource::new(vec![None]),
            &mut RecordingRenderer::default(),
        )
        .unwrap();
    }

    fn key(app: &mut App, key: Key) {
        app.tick(
            &[TermEvent::Key(key)],
            &mut ScriptedSource::new(vec![None]),
            &mut RecordingRenderer::default(),
        )
        .unwrap();
    }

    /// Tick until the reply being written has been collected.
    fn wait_for_the_reply(app: &mut App) {
        for _ in 0..400 {
            if app.view().chat.pending.is_none() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
            key(app, Key::F(2)); // a bound-to-nothing key: just a tick
        }
        panic!("the reply never arrived");
    }

    #[test]
    fn a_message_needs_a_model_and_a_plain_line_is_addressed_to_it() {
        let mut app = chat_app(Say::Answers);
        key(&mut app, Key::F(3));
        assert_eq!(app.view().screen, Screen::Chat);
        assert_eq!(app.view().focus, Focus::CommandBar);

        for c in "hello".chars() {
            key(&mut app, Key::Char(c));
        }
        assert_eq!(app.view().addressee, Addressee::Model);
        key(&mut app, Key::Enter);
        assert_eq!(
            app.view().command_feedback.as_deref(),
            Some("no model chosen: /chat model <name>")
        );
        assert_eq!(app.view().chat.pending, None, "nothing was sent");

        type_and_enter(&mut app, "/chat model llama3");
        assert_eq!(app.view().chat.model.as_deref(), Some("llama3"));
        for c in "/sta".chars() {
            key(&mut app, Key::Char(c));
        }
        assert_ne!(
            app.view().addressee,
            Addressee::Model,
            "a line starting with a slash is a command"
        );
    }

    #[test]
    fn a_sent_message_is_pending_until_the_reply_is_collected_and_then_read_back() {
        let mut app = chat_app(Say::Answers);
        key(&mut app, Key::F(3));
        type_and_enter(&mut app, "/chat model llama3");
        type_and_enter(&mut app, "what blocks the card?");
        assert_eq!(
            app.view().chat.pending.as_deref(),
            Some("what blocks the card?"),
            "shown as sent, not yet answered"
        );
        assert!(app.view().chat.session.is_some());

        wait_for_the_reply(&mut app);
        assert_eq!(
            app.view().command_feedback,
            None,
            "a good reply needs no comment"
        );
        let Some(Projection::Available(conversation)) = &app.view().chat.transcript else {
            panic!("the conversation was read back: {:?}", app.view().chat);
        };
        assert_eq!(
            conversation.messages,
            vec![
                (
                    "user".to_string(),
                    "what blocks the card? [llama3]".to_string()
                ),
                ("assistant".to_string(), "an answer".to_string()),
            ]
        );
    }

    #[test]
    fn a_second_message_while_a_reply_is_being_written_is_held_back() {
        let mut app = chat_app(Say::UntilStopped);
        key(&mut app, Key::F(3));
        type_and_enter(&mut app, "/chat model m");
        type_and_enter(&mut app, "first");
        type_and_enter(&mut app, "second");
        assert_eq!(app.view().chat.pending.as_deref(), Some("first"));
        assert!(
            app.view()
                .command_feedback
                .as_deref()
                .is_some_and(|f| f.contains("still being written"))
        );
        key(&mut app, Key::Esc);
        wait_for_the_reply(&mut app);
    }

    #[test]
    fn a_reply_shows_as_it_streams_in_and_settles_into_the_journaled_conversation() {
        let mut app = chat_app(Say::Streams);
        key(&mut app, Key::F(3));
        type_and_enter(&mut app, "/chat model m");
        type_and_enter(&mut app, "tell me");

        // The first piece arrives while the reply is still being written.
        for _ in 0..400 {
            if app.view().chat.partial == "par" {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
            key(&mut app, Key::F(2));
        }
        assert_eq!(app.view().chat.partial, "par");
        assert!(app.view().chat.pending.is_some(), "still being written");

        // Let the fixture finish: its release is a stop, then it settles.
        let release = Invocation {
            id: InvocableId::new("core:session.stop").expect("well-formed"),
            args: {
                let mut args = ArgValues::new();
                args.insert(
                    "id",
                    ArgValue::Text(app.view().chat.session.clone().unwrap()),
                );
                args
            },
            caller: Surface::Tui,
        };
        app.dispatcher.dispatch(&app.registry, &release);
        wait_for_the_reply(&mut app);
        assert_eq!(app.view().chat.partial, "", "nothing is pending any more");
        assert_eq!(app.view().command_feedback, None);
        let Some(Projection::Available(conversation)) = &app.view().chat.transcript else {
            panic!("the conversation was read back");
        };
        assert_eq!(
            conversation.messages.last().map(|m| m.1.as_str()),
            Some("partial")
        );
    }

    #[test]
    fn escape_stops_a_reply_being_written_and_the_cut_short_turn_is_told() {
        let mut app = chat_app(Say::UntilStopped);
        key(&mut app, Key::F(3));
        type_and_enter(&mut app, "/chat model m");
        type_and_enter(&mut app, "tell me everything");
        assert!(app.view().chat.pending.is_some());

        key(&mut app, Key::Esc); // empty line, reply pending: stop it
        wait_for_the_reply(&mut app);
        assert_eq!(app.view().command_feedback.as_deref(), Some("cancelled"));
        assert_eq!(
            app.view().screen,
            Screen::Chat,
            "stopping does not leave the screen"
        );
    }

    #[test]
    fn a_failed_turn_is_told_in_the_ticker_and_clears_the_pending_message() {
        let mut app = chat_app(Say::Fails);
        key(&mut app, Key::F(3));
        type_and_enter(&mut app, "/chat model m");
        type_and_enter(&mut app, "anyone there?");
        wait_for_the_reply(&mut app);
        assert_eq!(
            app.view().command_feedback.as_deref(),
            Some("cannot reach the model server")
        );
    }

    fn tick_once(app: &mut App) -> TickResult {
        app.tick(
            &[],
            &mut ScriptedSource::new(vec![None]),
            &mut RecordingRenderer::default(),
        )
        .unwrap()
    }

    /// A relaunch waits for what is still running on the composition it will
    /// replace — a reply being written is finished, not cut off — and nothing
    /// new is started on it meanwhile.
    #[test]
    fn a_pending_relaunch_waits_for_a_reply_being_written_and_takes_nothing_new() {
        let mut app = chat_app(Say::UntilStopped);
        key(&mut app, Key::F(3));
        type_and_enter(&mut app, "/chat model m");
        type_and_enter(&mut app, "tell me everything");
        assert!(app.view().chat.pending.is_some());

        app.relaunch_pending = true;
        assert!(
            !tick_once(&mut app).relaunch,
            "a reply is being written on this composition"
        );
        type_and_enter(&mut app, "and another thing");
        assert_eq!(app.view().command_feedback.as_deref(), Some(RESTARTING));
        assert_eq!(
            app.view().chat.pending.as_deref(),
            Some("tell me everything"),
            "the second message was not sent"
        );

        key(&mut app, Key::Esc);
        wait_for_the_reply(&mut app);
        assert!(tick_once(&mut app).relaunch, "nothing is running now");
    }

    #[test]
    fn a_command_is_held_back_while_a_relaunch_is_pending() {
        let (mut registry, mut dispatcher) = registry_with_probe();
        pane_actions::register(&mut registry, &mut dispatcher);
        let mut app = App::new(ViewModel::default(), registry, dispatcher, test_catalog());
        app.relaunch_pending = true;
        key(&mut app, Key::Char('/'));
        type_and_enter(&mut app, "test probe hello");
        assert_eq!(app.view().command_feedback.as_deref(), Some(RESTARTING));
        assert!(!app.is_busy(), "nothing was started");
        assert!(tick_once(&mut app).relaunch);
    }

    #[test]
    fn leaving_wins_over_a_relaunch_and_nothing_is_asked_for_without_a_change() {
        let mut app = App::new(
            ViewModel::default(),
            InvocableRegistry::new(),
            Dispatcher::new(),
            Vec::new(),
        );
        assert!(!tick_once(&mut app).relaunch, "no change, no relaunch");
        app.relaunch_pending = true;
        app.should_quit = true;
        let result = tick_once(&mut app);
        assert!(result.quit && !result.relaunch);
    }

    /// What replaces an app after a relaunch shows the same screen, focus and
    /// conversation, and the answer to the change that caused it.
    #[test]
    fn the_view_a_relaunch_carries_over_is_the_one_the_person_was_in() {
        let mut app = chat_app(Say::Answers);
        key(&mut app, Key::F(3));
        type_and_enter(&mut app, "/chat model m");
        app.view.result_block = Some(ResultBlock {
            title: "/restore backup-1".to_string(),
            lines: vec!["restarting onto the changed installation".to_string()],
        });
        let carried = app.into_view();

        let (registry, dispatcher) = registry_with_pane_actions();
        let invocables: Vec<&Invocable> = registry.all().collect();
        let catalog = command::build_catalog(&invocables);
        let resumed = App::new(carried, registry, dispatcher, catalog).after_relaunch();
        assert_eq!(resumed.view().screen, Screen::Chat);
        assert_eq!(resumed.view().chat.model.as_deref(), Some("m"));
        assert_eq!(
            resumed
                .view()
                .result_block
                .as_ref()
                .map(|block| block.title.as_str()),
            Some("/restore backup-1")
        );
        assert!(!resumed.is_busy(), "no conversation to read back yet");
    }

    #[test]
    fn escape_on_an_idle_chat_leaves_it_and_a_new_conversation_forgets_the_old() {
        let mut app = chat_app(Say::Answers);
        key(&mut app, Key::F(3));
        type_and_enter(&mut app, "/chat model m");
        type_and_enter(&mut app, "hi");
        wait_for_the_reply(&mut app);
        let first = app.view().chat.session.clone();
        assert!(first.is_some());

        type_and_enter(&mut app, "/chat new");
        assert_eq!(app.view().chat.session, None);
        assert_eq!(app.view().chat.transcript, None);
        assert_eq!(
            app.view().chat.model.as_deref(),
            Some("m"),
            "the model is kept"
        );

        key(&mut app, Key::Esc);
        assert_eq!(app.view().screen, Screen::Overview);
    }

    #[test]
    fn f1_shows_the_command_listing_as_a_block() {
        let (registry, dispatcher) = registry_with_pane_actions();
        let invocables: Vec<&Invocable> = registry.all().collect();
        let catalog = command::build_catalog(&invocables);
        let mut app = App::new(ViewModel::default(), registry, dispatcher, catalog);
        app.tick(
            &[TermEvent::Key(Key::F(1))],
            &mut ScriptedSource::new(vec![None]),
            &mut RecordingRenderer::default(),
        )
        .unwrap();
        let block = app.view().result_block.as_ref().expect("help block");
        assert_eq!(block.title, "/help");
        assert!(block.lines.iter().any(|l| l.starts_with("/pane")));
    }

    /// While an installation line is being typed the input block says the
    /// line is addressed to no agent; deleting it back restores the label.
    #[test]
    fn the_addressee_follows_what_is_being_typed() {
        let mut registry = InvocableRegistry::new();
        let mut dispatcher = Dispatcher::new();
        cronus_core::installation::register(&mut registry, &mut dispatcher);
        let mut app = App::new(
            ViewModel {
                focus: Focus::CommandBar,
                ..Default::default()
            },
            registry,
            dispatcher,
            Vec::new(),
        );
        let mut type_keys = |keys: &[Key]| {
            let events: Vec<TermEvent> = keys.iter().copied().map(TermEvent::Key).collect();
            app.tick(
                &events,
                &mut ScriptedSource::new(vec![None]),
                &mut RecordingRenderer::default(),
            )
            .unwrap();
            app.view().addressee
        };
        let word = |s: &str| s.chars().map(Key::Char).collect::<Vec<_>>();

        assert_eq!(
            type_keys(&word("doc")),
            Addressee::Command,
            "not a verb yet"
        );
        assert_eq!(type_keys(&word("tor")), Addressee::Installation);
        assert_eq!(type_keys(&[Key::Backspace]), Addressee::Command);
        assert_eq!(type_keys(&[Key::Char('r'), Key::Esc]), Addressee::Command);
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
        settle(&mut app);

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
        settle(&mut app);
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
        settle(&mut app);
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
    /// rendered failure — and a genuinely `Rejected` outcome
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
        settle(&mut app);
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
        settle(&mut app);

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
        settle(&mut app);

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
        settle(&mut app);

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
        settle(&mut app);

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
                        column: view::BoardColumn::Running,
                        ..Default::default()
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
                session_list: Projection::default(),
            },
            size: (80, 24),
            focus: Focus::Board,
            command_input: String::new(),
            command_feedback: None,
            result_block: None,
            addressee: Addressee::Command,
            screen: Screen::Overview,
            board_cursor: BoardCursor::default(),
            session_cursor: 0,
            transcript: None,
            chat: ChatView::default(),
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

    /// A result block is part of the view-model, so it is drawn from it: it
    /// appears with its title and lines directly above the command bar, the
    /// panels underneath keep rendering, and clearing it restores the frame.
    #[test]
    fn a_result_block_is_drawn_above_the_command_bar_and_clearing_it_restores_the_frame() {
        let area = Rect::new(0, 0, 80, 24);
        let plain = render_to_buffer(&populated_view(), area);

        let mut with_block = populated_view();
        with_block.result_block = Some(ResultBlock {
            title: "/status".to_string(),
            lines: vec!["workspace: main".to_string(), "phase: ready".to_string()],
        });
        let drawn = render_to_buffer(&with_block, area);
        let text = buffer_text(&drawn, area);
        assert!(text.contains("/status"), "the block's title is drawn");
        assert!(text.contains("workspace: main"));
        assert!(text.contains("phase: ready"));
        assert_ne!(plain, drawn, "the block changes the frame");

        // The block's last border row is the row directly above the ticker.
        let ticker = view::layout(area).ticker;
        let above = Rect::new(0, ticker.y - 1, area.width, 1);
        assert!(
            buffer_text(&drawn, above).contains('└'),
            "the block ends on the row above the ticker"
        );

        with_block.result_block = None;
        assert_eq!(
            render_to_buffer(&with_block, area),
            plain,
            "clearing the block restores the frame exactly"
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

    /// The board projection carries what the core's `board.list` now sends
    /// with each card — its task, priority and assignee — and an absent
    /// priority or assignee is blank, not a placeholder.
    #[test]
    fn dispatch_board_maps_the_task_priority_and_assignee_of_each_card() {
        let mut registry = InvocableRegistry::new();
        let mut dispatcher = Dispatcher::new();
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
            .expect("fixture registers cleanly");
        dispatcher.attach(
            id,
            Arc::new(|args| {
                if matches!(args.get("archived"), Some(ArgValue::Flag)) {
                    return Outcome::Value(OutcomeValue::List(Vec::new()));
                }
                let text = |s: &str| OutcomeValue::Text(s.to_string());
                Outcome::Value(OutcomeValue::List(vec![
                    OutcomeValue::Record(vec![
                        ("id".to_string(), text("card-1")),
                        ("state".to_string(), text("running")),
                        ("task_ref".to_string(), text("budget guard")),
                        ("priority".to_string(), text("P1")),
                        ("assignee".to_string(), text("rhea")),
                    ]),
                    OutcomeValue::Record(vec![
                        ("id".to_string(), text("card-2")),
                        ("state".to_string(), text("todo")),
                        ("task_ref".to_string(), text("saved views")),
                        ("priority".to_string(), OutcomeValue::Empty),
                        ("assignee".to_string(), OutcomeValue::Empty),
                    ]),
                ]))
            }),
        );

        let Projection::Available(board) = dispatch_board(&registry, &dispatcher) else {
            panic!("the board is available");
        };
        let by_id = |id: &str| board.cards.iter().find(|c| c.id == id).expect("card");
        let running = by_id("card-1");
        assert_eq!(
            (
                running.title.as_str(),
                running.priority.as_str(),
                running.assignee.as_str()
            ),
            ("budget guard", "P1", "rhea")
        );
        let todo = by_id("card-2");
        assert_eq!(todo.title, "saved views");
        assert_eq!((todo.priority.as_str(), todo.assignee.as_str()), ("", ""));
    }

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

    /// A `core:test.probe` that holds until `gate` is set, then echoes its
    /// argument — a command that takes as long as the test says.
    fn registry_with_gated_probe(
        gate: &Arc<std::sync::atomic::AtomicBool>,
    ) -> (InvocableRegistry, Dispatcher) {
        let mut registry = InvocableRegistry::new();
        let mut dispatcher = Dispatcher::new();
        let id = InvocableId::new("core:test.probe").expect("well-formed test id");
        registry
            .register(
                &Registrant::core(),
                Invocable {
                    id: id.clone(),
                    name: "Probe",
                    summary: "Test-only gated probe invocable.",
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
        let gate = Arc::clone(gate);
        dispatcher.attach(
            id,
            Arc::new(move |args| {
                for _ in 0..1000 {
                    if gate.load(std::sync::atomic::Ordering::SeqCst) {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                let echoed = match args.get("value") {
                    Some(ArgValue::Text(s)) => s.clone(),
                    _ => String::new(),
                };
                Outcome::Value(OutcomeValue::Text(format!("probe: {echoed}")))
            }),
        );
        (registry, dispatcher)
    }

    #[test]
    fn a_slow_command_does_not_hold_the_loop_and_a_second_line_waits_for_it() {
        let gate = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (registry, dispatcher) = registry_with_gated_probe(&gate);
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
        let mut line = |app: &mut App, text: &str| {
            let mut events: Vec<TermEvent> =
                text.chars().map(|c| TermEvent::Key(Key::Char(c))).collect();
            events.push(TermEvent::Key(Key::Enter));
            app.tick(&events, &mut source, &mut renderer).unwrap();
        };

        // The command is still holding its handler when the tick returns.
        line(&mut app, "test probe one");
        assert!(app.is_busy(), "the command runs beside the loop");
        assert_eq!(
            app.view().command_feedback.as_deref(),
            Some("running /test probe …"),
            "the line says what is running, and not what was typed after it"
        );

        // The keyboard is alive meanwhile, and a second line is told to wait.
        line(&mut app, "test probe two");
        assert_eq!(
            app.view().command_feedback.as_deref(),
            Some("a command is still running")
        );
        app.tick(
            &[TermEvent::Key(Key::Char('x'))],
            &mut ScriptedSource::new(vec![None]),
            &mut RecordingRenderer::default(),
        )
        .unwrap();
        assert_eq!(app.view().command_input, "x", "typing goes on");

        gate.store(true, std::sync::atomic::Ordering::SeqCst);
        settle(&mut app);
        assert!(!app.is_busy());
        assert_eq!(app.view().command_feedback.as_deref(), Some("probe: one"));

        // And the bar takes a line again.
        app.view.command_input.clear();
        line(&mut app, "test probe three");
        settle(&mut app);
        assert_eq!(app.view().command_feedback.as_deref(), Some("probe: three"));
    }

    #[test]
    fn a_command_that_ran_asks_the_snapshot_source_to_read_again() {
        struct Counting {
            invalidated: usize,
        }
        impl SnapshotSource for Counting {
            fn poll_snapshot(
                &mut self,
                _registry: &Arc<InvocableRegistry>,
                _dispatcher: &Arc<Dispatcher>,
            ) -> Option<CoreSnapshot> {
                None
            }
            fn invalidate(&mut self) {
                self.invalidated += 1;
            }
        }
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
        let mut source = Counting { invalidated: 0 };
        let mut renderer = RecordingRenderer::default();
        let mut events: Vec<TermEvent> = "test probe hi"
            .chars()
            .map(|c| TermEvent::Key(Key::Char(c)))
            .collect();
        events.push(TermEvent::Key(Key::Enter));
        app.tick(&events, &mut source, &mut renderer).unwrap();
        for _ in 0..400 {
            if source.invalidated > 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
            app.tick(&[], &mut source, &mut renderer).unwrap();
        }
        assert_eq!(source.invalidated, 1, "once, when the command finished");
    }

    #[test]
    fn the_production_source_never_waits_for_a_slow_read() {
        let gate = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut registry = InvocableRegistry::new();
        let mut dispatcher = Dispatcher::new();
        let id = InvocableId::new("core:board.list").expect("well-formed test id");
        registry
            .register(
                &Registrant::core(),
                Invocable {
                    id: id.clone(),
                    name: "List",
                    summary: "Test-only gated board.list fixture.",
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
        let held = Arc::clone(&gate);
        dispatcher.attach(
            id,
            Arc::new(move |args| {
                for _ in 0..1000 {
                    if held.load(std::sync::atomic::Ordering::SeqCst) {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                if matches!(args.get("archived"), Some(ArgValue::Flag)) {
                    return Outcome::Value(OutcomeValue::List(Vec::new()));
                }
                Outcome::Value(OutcomeValue::List(vec![OutcomeValue::Record(vec![
                    ("id".to_string(), OutcomeValue::Text("card-1".to_string())),
                    ("state".to_string(), OutcomeValue::Text("todo".to_string())),
                ])]))
            }),
        );
        let (registry, dispatcher) = (Arc::new(registry), Arc::new(dispatcher));
        let mut source = CapabilitySource::new(Engine::new());

        // The board read is held, and the poll still answers at once with
        // the status and the neutral starting board.
        let first = source
            .poll_snapshot(&registry, &dispatcher)
            .expect("a snapshot is always handed over");
        assert!(matches!(first.status, Projection::Available(_)));
        assert_eq!(first.board, Projection::Available(BoardView::default()));

        gate.store(true, std::sync::atomic::Ordering::SeqCst);
        for _ in 0..400 {
            let snapshot = source
                .poll_snapshot(&registry, &dispatcher)
                .expect("a snapshot is always handed over");
            if let Projection::Available(board) = &snapshot.board
                && board.cards.len() == 1
            {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("the finished read never reached the snapshot");
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
        settle(&mut app);

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
