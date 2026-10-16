//! Panel layout and focus navigation — the read-only view-surface skeleton.
//!
//! This module is render-only: it computes screen regions from the terminal area
//! and tracks which region has focus. It holds no domain state and performs no
//! core calls; panels are fed by the [`crate::app::ViewModel`] snapshot.

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Widget};

/// The focusable regions, in tab order.
///
/// `CommandBar` is last so that tabbing past the panels lands on the input line,
/// matching the spatial bottom-of-screen position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Focus {
    /// Kanban board panel.
    #[default]
    Board,
    /// Office (agents → tasks) panel.
    Office,
    /// Status panel.
    Status,
    /// Sessions / log panel.
    Sessions,
    /// Slash-command input line.
    CommandBar,
}

impl Focus {
    /// Tab order driving [`next`](Focus::next) / [`prev`](Focus::prev).
    pub const ORDER: [Focus; 5] = [
        Focus::Board,
        Focus::Office,
        Focus::Status,
        Focus::Sessions,
        Focus::CommandBar,
    ];

    /// The next region in tab order, wrapping after the last.
    pub fn next(self) -> Focus {
        match self {
            Focus::Board => Focus::Office,
            Focus::Office => Focus::Status,
            Focus::Status => Focus::Sessions,
            Focus::Sessions => Focus::CommandBar,
            Focus::CommandBar => Focus::Board,
        }
    }

    /// The previous region in tab order, wrapping before the first.
    pub fn prev(self) -> Focus {
        match self {
            Focus::Board => Focus::CommandBar,
            Focus::Office => Focus::Board,
            Focus::Status => Focus::Office,
            Focus::Sessions => Focus::Status,
            Focus::CommandBar => Focus::Sessions,
        }
    }
}

/// Screen regions: the tab bar on top, the four view panels, and the chrome
/// beneath them — ticker, input block (rule, prompt, rule), function-key bar
/// and status line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelAreas {
    /// Single-row tab bar pinned to the top.
    pub tabs: Rect,
    /// Everything between the tab bar and the ticker — what a full-width
    /// screen fills.
    pub body: Rect,
    /// Board panel (top-left).
    pub board: Rect,
    /// Office panel (top-right).
    pub office: Rect,
    /// Status panel (bottom-left).
    pub status: Rect,
    /// Sessions / log panel (bottom-right).
    pub sessions: Rect,
    /// One row between the panels and the input block: the last command's
    /// one-line feedback.
    pub ticker: Rect,
    /// The three-row input block: top rule, prompt, bottom rule.
    pub input: Rect,
    /// The prompt row inside the input block.
    pub command_bar: Rect,
    /// Single-row function-key bar.
    pub fkeys: Rect,
    /// Single-row status line pinned to the very bottom.
    pub status_line: Rect,
}

impl PanelAreas {
    /// The region owning the given focus, for highlight rendering.
    pub fn for_focus(&self, focus: Focus) -> Rect {
        match focus {
            Focus::Board => self.board,
            Focus::Office => self.office,
            Focus::Status => self.status,
            Focus::Sessions => self.sessions,
            Focus::CommandBar => self.command_bar,
        }
    }
}

/// Split `area` into a tab bar, a 2×2 grid of view panels, and — from the
/// bottom up — a status line, a function-key bar, a three-row input block
/// and a ticker row.
///
/// ratatui's constraint solver clamps to the available space, so this degrades
/// to zero-sized regions at tiny terminal sizes rather than panicking.
pub fn layout(area: Rect) -> PanelAreas {
    let [tabs, body, ticker, input, fkeys, status_line] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);
    // The prompt is the middle row of the block; a block squeezed below three
    // rows keeps whatever it has as the prompt.
    let command_bar = if input.height == 3 {
        Rect::new(input.x, input.y + 1, input.width, 1)
    } else {
        input
    };

    let [top, bottom] =
        Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(body);

    let [board, office] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(top);

    let [status, sessions] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(bottom);

    PanelAreas {
        tabs,
        body,
        board,
        office,
        status,
        sessions,
        ticker,
        input,
        command_bar,
        fkeys,
        status_line,
    }
}

/// Border style for a panel given whether it currently holds focus.
pub fn focus_border_style(focused: bool) -> Style {
    if focused {
        Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(Color::DarkGray)
    }
}

// ── Panel availability ──────────────────────────────────────────────────────

/// A core-supplied panel projection: obtained, or not. `Unavailable`
/// carries why, so the two states stay distinguishable **in the view model
/// itself** — a panel whose projection could not be obtained must not render
/// the same way as one whose projection is legitimately empty, and the
/// distinction has to be a value the code carries, not a formatting
/// accident downstream of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Projection<T> {
    /// The projection was obtained — `T` may still be legitimately empty.
    Available(T),
    /// The projection could not be obtained, and why.
    Unavailable { reason: String },
}

impl<T: Default> Default for Projection<T> {
    /// The neutral starting point before any snapshot has arrived: a
    /// genuine absence of data to report yet, not a failure to obtain it —
    /// the same state a fresh, empty projection would report once obtained.
    fn default() -> Self {
        Projection::Available(T::default())
    }
}

/// Render one panel's bordered block, then either `render_available` (over
/// the block's interior) for `Available`, or the unavailable reason for
/// `Unavailable` — the one place that fallback rendering lives, so it is not
/// restated with four slightly different wordings across the four panels.
fn render_panel<T>(
    area: Rect,
    buf: &mut Buffer,
    title: &'static str,
    focused: bool,
    projection: &Projection<T>,
    render_available: impl FnOnce(Rect, &mut Buffer, &T),
) {
    let block = Block::bordered()
        .title(title)
        .border_style(focus_border_style(focused));
    let inner = block.inner(area);
    block.render(area, buf);

    match projection {
        Projection::Available(value) => render_available(inner, buf, value),
        Projection::Unavailable { reason } => {
            Paragraph::new(format!("unavailable: {reason}")).render(inner, buf);
        }
    }
}

// ── Board projection ────────────────────────────────────────────────────────

/// The Kanban columns the board renders, in pipeline order.
///
/// This is a presentation projection: the core models six live card states plus
/// a separate archive store, while the board *view* shows seven columns. The
/// binding maps live cards by state and archived cards into `Archive`; the TUI
/// owns no transition logic (that stays in the core).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BoardColumn {
    /// Incoming, untriaged work.
    #[default]
    Triage,
    /// Triaged, not yet ready.
    Todo,
    /// Ready to start.
    Ready,
    /// Actively running.
    Running,
    /// Blocked, awaiting resolution.
    Blocked,
    /// Completed.
    Done,
    /// Archived (view-only column; the core archives to storage).
    Archive,
}

impl BoardColumn {
    /// All columns in pipeline order.
    pub const ALL: [BoardColumn; 7] = [
        BoardColumn::Triage,
        BoardColumn::Todo,
        BoardColumn::Ready,
        BoardColumn::Running,
        BoardColumn::Blocked,
        BoardColumn::Done,
        BoardColumn::Archive,
    ];

    /// The column's full name, for the Kanban screen's header.
    pub fn name(self) -> &'static str {
        match self {
            BoardColumn::Triage => "Triage",
            BoardColumn::Todo => "Todo",
            BoardColumn::Ready => "Ready",
            BoardColumn::Running => "Running",
            BoardColumn::Blocked => "Blocked",
            BoardColumn::Done => "Done",
            BoardColumn::Archive => "Archive",
        }
    }

    /// Short header label that fits a narrow column.
    pub fn short(self) -> &'static str {
        match self {
            BoardColumn::Triage => "tri",
            BoardColumn::Todo => "todo",
            BoardColumn::Ready => "rdy",
            BoardColumn::Running => "run",
            BoardColumn::Blocked => "blk",
            BoardColumn::Done => "done",
            BoardColumn::Archive => "arc",
        }
    }
}

/// A card as the board view shows it. Presentation-only — no history or
/// transition rules, which live in the core.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BoardCard {
    /// Stable card id (the token rendered).
    pub id: String,
    /// Short task reference / title shown beside the id.
    pub title: String,
    /// Priority label (`P1` …), empty when the card has none.
    pub priority: String,
    /// Who the card is assigned to, empty when nobody.
    pub assignee: String,
    /// The column the card currently sits in.
    pub column: BoardColumn,
}

/// The board projection: a flat list of cards, each tagged with its column.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BoardView {
    /// Every card currently on the board.
    pub cards: Vec<BoardCard>,
    /// Ids of live cards whose state this surface has no column for. Carried
    /// rather than dropped, so a card the board cannot place stays visible as
    /// a note instead of vanishing from an otherwise successful-looking view.
    pub unmapped: Vec<String>,
}

impl BoardView {
    /// Cards currently in `column`, in insertion order.
    pub fn cards_in(&self, column: BoardColumn) -> impl Iterator<Item = &BoardCard> {
        self.cards.iter().filter(move |c| c.column == column)
    }
}

/// Split a board inner area into its seven equal columns.
pub fn board_columns(inner: Rect) -> [Rect; 7] {
    Layout::horizontal([Constraint::Ratio(1, 7); 7]).areas(inner)
}

/// Render the Board panel: a bordered block whose interior splits into the seven
/// columns, each listing its cards. Pure function of the [`Projection<BoardView>`].
pub fn render_board(area: Rect, buf: &mut Buffer, board: &Projection<BoardView>, focused: bool) {
    render_panel(area, buf, "Board", focused, board, |inner, buf, board| {
        let (columns_area, note_area) = if board.unmapped.is_empty() || inner.height < 2 {
            (inner, None)
        } else {
            let [columns_area, note_area] =
                Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);
            (columns_area, Some(note_area))
        };
        let columns = board_columns(columns_area);
        for (column, column_area) in BoardColumn::ALL.iter().zip(columns) {
            let mut lines = vec![Line::from(column.short()).bold()];
            for card in board.cards_in(*column) {
                let label = if card.title.is_empty() {
                    card.id.clone()
                } else {
                    format!("{} {}", card.id, card.title)
                };
                lines.push(Line::from(label));
            }
            Paragraph::new(lines).render(column_area, buf);
        }
        if let Some(note_area) = note_area {
            let note = format!(
                "{} card(s) in an unrecognized state: {}",
                board.unmapped.len(),
                board.unmapped.join(", ")
            );
            Paragraph::new(note).render(note_area, buf);
        }
    });
}

// ── Office projection ───────────────────────────────────────────────────────

/// One agent and the task it is currently working, as the Office panel shows it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AgentActivity {
    /// Agent / role name.
    pub agent: String,
    /// The task the agent is currently on (empty when idle).
    pub task: String,
}

/// The office projection: the roster of agents and their current tasks.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OfficeView {
    /// Active agents and what each is doing.
    pub agents: Vec<AgentActivity>,
}

/// Render the Office panel: an `agent → task` line per active agent.
pub fn render_office(area: Rect, buf: &mut Buffer, office: &Projection<OfficeView>, focused: bool) {
    render_panel(
        area,
        buf,
        "Office",
        focused,
        office,
        |inner, buf, office| {
            let lines: Vec<Line> = office
                .agents
                .iter()
                .map(|a| {
                    let task = if a.task.is_empty() { "idle" } else { &a.task };
                    Line::from(format!("{} → {}", a.agent, task))
                })
                .collect();
            Paragraph::new(lines).render(inner, buf);
        },
    );
}

// ── Status panel ────────────────────────────────────────────────────────────

/// Version + status line, as the Status panel shows it. Wrapped in
/// [`Projection`] like every other panel (structural compliance): the
/// panel's own doc comment already anticipates richer, potentially fallible
/// position/progress/blockers fields later, and the distinction belongs in
/// the view model before that lands, not retrofitted onto it once it does.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StatusView {
    pub version: String,
    pub status: String,
}

/// Render the Status panel: a mirror of the core `status` capability snapshot.
///
/// Today the capability surface is a single status line plus the version; the
/// panel renders both. Richer position/progress/blockers fields extend this once
/// the core exposes them.
pub fn render_status(area: Rect, buf: &mut Buffer, status: &Projection<StatusView>, focused: bool) {
    render_panel(
        area,
        buf,
        "Status",
        focused,
        status,
        |inner, buf, status| {
            let lines = vec![
                Line::from(format!("version {}", status.version)),
                Line::from(status.status.clone()),
            ];
            Paragraph::new(lines).render(inner, buf);
        },
    );
}

// ── Sessions / log projection ───────────────────────────────────────────────

/// Maximum retained activity lines. The scrollback is bounded so a long-running
/// office cannot grow the view without limit (the panel is a recent-tail view,
/// not the full durable history — that lives in the core).
pub const MAX_SESSION_LINES: usize = 500;

/// The sessions/log projection: a bounded, ordered tail of recent activity.
///
/// This is a last-N *projection* of the core's durable activity log, not view
/// state accumulated across frames — keeping the view a pure function of the
/// snapshot. [`push`](SessionsView::push) drops the oldest line past the
/// cap so growth stays bounded.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionsView {
    /// Recent log lines, oldest first.
    entries: Vec<String>,
}

impl SessionsView {
    /// Append an activity line, dropping the oldest if the cap is exceeded.
    pub fn push(&mut self, entry: impl Into<String>) {
        self.entries.push(entry.into());
        if self.entries.len() > MAX_SESSION_LINES {
            let overflow = self.entries.len() - MAX_SESSION_LINES;
            self.entries.drain(0..overflow);
        }
    }

    /// The retained lines, oldest first.
    pub fn entries(&self) -> &[String] {
        &self.entries
    }
}

/// Render the Sessions/Log panel: the most recent activity lines that fit,
/// oldest of the visible window at the top and newest at the bottom.
pub fn render_sessions(
    area: Rect,
    buf: &mut Buffer,
    sessions: &Projection<SessionsView>,
    focused: bool,
) {
    render_panel(
        area,
        buf,
        "Sessions",
        focused,
        sessions,
        |inner, buf, sessions| {
            let height = inner.height as usize;
            let entries = sessions.entries();
            let start = entries.len().saturating_sub(height);
            let lines: Vec<Line> = entries[start..]
                .iter()
                .map(|e| Line::from(e.clone()))
                .collect();
            Paragraph::new(lines).render(inner, buf);
        },
    );
}

// ── Chrome: tabs, ticker, input block, function keys, status line ───────────

/// The view panels as the tab bar names them, in tab order.
const TAB_NAMES: [(Focus, &str); 4] = [
    (Focus::Board, "Kanban"),
    (Focus::Office, "Office"),
    (Focus::Status, "Status"),
    (Focus::Sessions, "Sessions"),
];

/// Render the tab bar: the product mark, then each panel, the focused one in
/// reverse video.
pub fn render_tabs(area: Rect, buf: &mut Buffer, focus: Focus) {
    let mut spans = vec![Span::styled(
        " ◆ cronus ",
        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )];
    for (panel, name) in TAB_NAMES {
        spans.push(Span::raw("│ "));
        let style = if panel == focus {
            Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else {
            Style::new().fg(Color::Gray)
        };
        spans.push(Span::styled(format!(" {name} "), style));
        spans.push(Span::raw(" "));
    }
    Paragraph::new(Line::from(spans)).render(area, buf);
}

/// Render the ticker: the last command's one line of feedback, or nothing.
pub fn render_ticker(area: Rect, buf: &mut Buffer, feedback: Option<&str>) {
    if let Some(text) = feedback {
        Paragraph::new(format!(" {text}"))
            .style(Style::new().fg(Color::Yellow))
            .render(area, buf);
    }
}

/// A horizontal rule `width` cells wide with `left` set into its left end and
/// `right` into its right end: `── left ───────── right ──`.
fn rule(width: u16, left: &str, right: &str) -> String {
    let width = usize::from(width);
    let left = if left.is_empty() {
        "──".to_string()
    } else {
        format!("── {left} ")
    };
    let right = if right.is_empty() {
        "──".to_string()
    } else {
        format!(" {right} ──")
    };
    let used = left.chars().count() + right.chars().count();
    if used > width {
        return "─".repeat(width);
    }
    format!("{left}{}{right}", "─".repeat(width - used))
}

/// Who the line being typed is addressed to. The label sits in the input
/// block's top rule so an installation line is never mistaken for work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Addressee {
    /// An ordinary command (or nothing typed yet).
    #[default]
    Command,
    /// A line that resolves to an installation verb: addressed to no agent.
    Installation,
    /// A plain message on the Chat screen: addressed to the model.
    Model,
}

impl Addressee {
    fn label(self) -> &'static str {
        match self {
            Addressee::Command => "Command",
            Addressee::Installation => "Installation",
            Addressee::Model => "Model",
        }
    }
}

/// Render the three-row input block: a rule naming the addressee, the prompt
/// line (the `/` is implicit, as in the slash grammar), and a rule with the
/// keys that matter here.
/// `slash` says whether the slash is implicit (a screen of commands) or the
/// line is typed whole (the Chat screen, where plain text is a message).
pub fn render_input(
    area: Rect,
    buf: &mut Buffer,
    text: &str,
    addressee: Addressee,
    slash: bool,
    focused: bool,
) {
    let marker = if slash { "/" } else { "" };
    let cursor = if focused { "█" } else { "" };
    if area.height < 3 {
        Paragraph::new(format!("❯ {marker}{text}{cursor}"))
            .style(focus_border_style(focused))
            .render(area, buf);
        return;
    }
    let style = focus_border_style(focused);
    let hint = if focused {
        "⏎ run  Esc clear"
    } else {
        "/ command"
    };
    let [top, prompt, bottom] = Layout::vertical([Constraint::Length(1); 3]).areas(area);
    Paragraph::new(rule(area.width, addressee.label(), ""))
        .style(style)
        .render(top, buf);
    Paragraph::new(format!("❯ {marker}{text}{cursor}")).render(prompt, buf);
    Paragraph::new(rule(area.width, "", hint))
        .style(style)
        .render(bottom, buf);
}

/// Render the function-key bar: ten equal slots, each a number then a label.
/// The number of the slot whose panel holds focus is bright; slots with no
/// action are a bare dim number.
pub fn render_fkeys(area: Rect, buf: &mut Buffer, active: Option<u8>) {
    if area.width < 20 {
        return;
    }
    let slot = usize::from(area.width) / 10;
    let mut spans = Vec::with_capacity(20);
    for n in 1..=10u8 {
        let digits = n.to_string();
        let number_style = if active == Some(n) {
            Style::new().fg(Color::White).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(Color::DarkGray)
        };
        spans.push(Span::styled(digits.clone(), number_style));
        let room = slot.saturating_sub(digits.len());
        match crate::pane_actions::fkey(n) {
            Some((label, _)) => {
                let label: String = label.chars().take(room).collect();
                let padded = format!("{label:<room$}");
                spans.push(Span::styled(
                    padded,
                    Style::new().fg(Color::Black).bg(Color::Cyan),
                ));
            }
            None => spans.push(Span::raw(" ".repeat(room))),
        }
    }
    Paragraph::new(Line::from(spans)).render(area, buf);
}

/// Render the status line: what the core reports, and its version.
pub fn render_status_line(area: Rect, buf: &mut Buffer, status: &Projection<StatusView>) {
    let (left, right) = match status {
        Projection::Available(view) => (view.status.clone(), format!("v{}", view.version)),
        Projection::Unavailable { reason } => (format!("unavailable: {reason}"), String::new()),
    };
    let width = usize::from(area.width);
    let gap = width
        .saturating_sub(left.chars().count() + right.chars().count() + 2)
        .max(1);
    Paragraph::new(format!(" {left}{}{right} ", " ".repeat(gap)))
        .style(Style::new().fg(Color::Gray).bg(Color::DarkGray))
        .render(area, buf);
}

// ── Kanban screen ───────────────────────────────────────────────────────────

/// Which screen fills the body between the tab bar and the ticker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Screen {
    /// The four-panel overview.
    #[default]
    Overview,
    /// The full-width Kanban board.
    Kanban,
    /// The sessions list with the selected session's conversation.
    Sessions,
    /// A conversation with a model.
    Chat,
}

/// The card the Kanban screen's selection rests on: a column (an index into
/// [`BoardColumn::ALL`]) and a row within it. Clamped against the board when
/// it is used, so a card leaving the board never leaves the cursor dangling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BoardCursor {
    /// Index into [`BoardColumn::ALL`].
    pub column: usize,
    /// Row within the column, counted over its cards in order.
    pub row: usize,
}

impl BoardCursor {
    /// The cursor pulled back onto the board: a column that exists, a row
    /// that exists in it (row 0 for an empty column).
    pub fn clamped(self, board: &BoardView) -> BoardCursor {
        let column = self.column.min(BoardColumn::ALL.len() - 1);
        let in_column = board.cards_in(BoardColumn::ALL[column]).count();
        BoardCursor {
            column,
            row: self.row.min(in_column.saturating_sub(1)),
        }
    }

    /// The card under the cursor, if the column has one there.
    pub fn card(self, board: &BoardView) -> Option<&BoardCard> {
        let at = self.clamped(board);
        board.cards_in(BoardColumn::ALL[at.column]).nth(at.row)
    }
}

/// `text` cut to at most `width` characters.
fn clip(text: &str, width: usize) -> String {
    text.chars().take(width).collect()
}

/// Rows one card takes in a column: its id, its title line, a gap.
const CARD_ROWS: u16 = 3;

/// Render the Kanban screen: one column per board state, each headed by its
/// name and count, its cards listed beneath. The card under the cursor is
/// drawn in reverse video while the board holds focus. A card shows what the
/// board projection carries — its id and, when it has one, its title.
pub fn render_kanban(
    area: Rect,
    buf: &mut Buffer,
    board: &Projection<BoardView>,
    cursor: BoardCursor,
    focused: bool,
) {
    let title = match board {
        Projection::Available(board) => format!("Kanban · {} cards", board.cards.len()),
        Projection::Unavailable { .. } => "Kanban".to_string(),
    };
    let frame = Block::bordered()
        .title(title)
        .title_bottom("←→ column  ↑↓ card  ⏎ show")
        .border_style(focus_border_style(focused));
    let inner = frame.inner(area);
    frame.render(area, buf);

    let board = match board {
        Projection::Available(board) => board,
        Projection::Unavailable { reason } => {
            Paragraph::new(format!("unavailable: {reason}")).render(inner, buf);
            return;
        }
    };
    let columns = BoardColumn::ALL.len() as u16;
    if inner.width < columns * 6 || inner.height < 4 {
        Paragraph::new("terminal too small for the board").render(inner, buf);
        return;
    }
    let cursor = cursor.clamped(board);
    let width = (inner.width - (columns - 1)) / columns;
    let note_rows = u16::from(!board.unmapped.is_empty());
    let visible_cards = usize::from((inner.height - 2 - note_rows) / CARD_ROWS);

    for (index, column) in BoardColumn::ALL.iter().enumerate() {
        let x = inner.x + index as u16 * (width + 1);
        let rect = Rect::new(x, inner.y, width, inner.height);
        if index > 0 {
            for y in inner.y..inner.bottom() - note_rows {
                buf[(x - 1, y)].set_symbol("│");
            }
        }
        let cards: Vec<&BoardCard> = board.cards_in(*column).collect();
        let glyph = if matches!(column, BoardColumn::Triage | BoardColumn::Todo) {
            "○"
        } else {
            "●"
        };
        let header = clip(
            &format!(" {glyph} {} {}", column.name(), cards.len()),
            usize::from(width),
        );
        Paragraph::new(header)
            .style(Style::new().add_modifier(Modifier::BOLD))
            .render(Rect::new(rect.x, rect.y, rect.width, 1), buf);
        Paragraph::new("─".repeat(usize::from(width)))
            .render(Rect::new(rect.x, rect.y + 1, rect.width, 1), buf);

        let selected_here = index == cursor.column;
        let skip = if selected_here {
            (cursor.row + 1).saturating_sub(visible_cards)
        } else {
            0
        };
        for (slot, card) in cards.iter().skip(skip).take(visible_cards).enumerate() {
            let y = rect.y + 2 + slot as u16 * CARD_ROWS;
            let is_selected = focused && selected_here && skip + slot == cursor.row;
            let style = if is_selected {
                Style::new().add_modifier(Modifier::REVERSED)
            } else {
                Style::new()
            };
            // Row one: priority and title. Row two: the id, with the
            // assignee set against the right edge.
            let head = [card.priority.as_str(), card.title.as_str()]
                .iter()
                .filter(|part| !part.is_empty())
                .copied()
                .collect::<Vec<_>>()
                .join(" ");
            let w = usize::from(width);
            let tail = {
                let id = format!(" {}", card.id);
                if card.assignee.is_empty() {
                    id
                } else {
                    let room =
                        w.saturating_sub(id.chars().count() + card.assignee.chars().count() + 1);
                    format!("{id}{}{} ", " ".repeat(room), card.assignee)
                }
            };
            let lines = [
                if head.is_empty() {
                    String::new()
                } else {
                    format!(" {head}")
                },
                tail,
            ];
            for (offset, line) in lines.iter().enumerate() {
                let line = format!("{:<w$}", clip(line, w));
                Paragraph::new(line)
                    .style(style)
                    .render(Rect::new(rect.x, y + offset as u16, rect.width, 1), buf);
            }
        }
        if cards.len() > skip + visible_cards {
            let more = format!(" … +{}", cards.len() - skip - visible_cards);
            Paragraph::new(clip(&more, usize::from(width)))
                .style(Style::new().fg(Color::DarkGray))
                .render(
                    Rect::new(rect.x, rect.bottom() - 1 - note_rows, rect.width, 1),
                    buf,
                );
        }
    }
    if note_rows > 0 {
        let note = format!(
            "{} card(s) in an unrecognized state: {}",
            board.unmapped.len(),
            board.unmapped.join(", ")
        );
        Paragraph::new(clip(&note, usize::from(inner.width)))
            .render(Rect::new(inner.x, inner.bottom() - 1, inner.width, 1), buf);
    }
}

// ── Sessions screen ─────────────────────────────────────────────────────────

/// One session as the list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionRow {
    /// The session's identity.
    pub id: String,
    /// The name it was given, if any.
    pub name: Option<String>,
    /// Assistant messages so far.
    pub turns: usize,
    /// Entries in its journal.
    pub entries: usize,
    /// What is wrong with its journal, when something is.
    pub problem: Option<String>,
}

/// The sessions list projection.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionList {
    /// Most recently written first.
    pub rows: Vec<SessionRow>,
}

/// One session's conversation, as the transcript pane shows it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TranscriptView {
    /// The session it belongs to.
    pub id: String,
    /// `(role, text)` in order.
    pub messages: Vec<(String, String)>,
    /// Earlier messages the core left out of the transcript.
    pub omitted: usize,
}

/// `text` broken into rows of at most `width` characters, honoring its own
/// line breaks.
fn wrapped_rows(text: &str, width: usize) -> Vec<String> {
    let mut rows = Vec::new();
    for line in text.lines() {
        rows.extend(wrap_row(line, width));
    }
    if rows.is_empty() {
        rows.push(String::new());
    }
    rows
}

/// The transcript as display rows: each message a bold-able header row (its
/// role) followed by its wrapped text and a gap.
fn transcript_rows(transcript: &TranscriptView, width: usize) -> Vec<(bool, String)> {
    let mut rows = Vec::new();
    if transcript.omitted > 0 {
        rows.push((
            false,
            format!("… {} earlier message(s) not shown", transcript.omitted),
        ));
    }
    for (role, text) in &transcript.messages {
        let role = if role.is_empty() { "?" } else { role };
        rows.push((true, format!("── {role} ")));
        rows.extend(
            wrapped_rows(text, width)
                .into_iter()
                .map(|row| (false, row)),
        );
        rows.push((false, String::new()));
    }
    rows
}

/// Render the Sessions screen: the list on the left (the session under the
/// cursor in reverse video while the screen holds focus), the selected
/// session's conversation on the right, newest at the bottom.
pub fn render_sessions_screen(
    area: Rect,
    buf: &mut Buffer,
    list: &Projection<SessionList>,
    cursor: usize,
    transcript: Option<&Projection<TranscriptView>>,
    focused: bool,
) {
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(35), Constraint::Percentage(65)]).areas(area);

    let count = match list {
        Projection::Available(list) => format!("Sessions · {}", list.rows.len()),
        Projection::Unavailable { .. } => "Sessions".to_string(),
    };
    let frame = Block::bordered()
        .title(count)
        .title_bottom("↑↓ session")
        .border_style(focus_border_style(focused));
    let inner = frame.inner(left);
    frame.render(left, buf);

    let rows = match list {
        Projection::Available(list) => &list.rows,
        Projection::Unavailable { reason } => {
            Paragraph::new(format!("unavailable: {reason}")).render(inner, buf);
            render_transcript_pane(right, buf, None, None);
            return;
        }
    };
    if rows.is_empty() {
        Paragraph::new("No sessions yet.").render(inner, buf);
        render_transcript_pane(right, buf, None, None);
        return;
    }
    let cursor = cursor.min(rows.len() - 1);
    let width = usize::from(inner.width);
    let per_page = usize::from(inner.height / 3).max(1);
    let skip = (cursor + 1).saturating_sub(per_page);
    for (slot, row) in rows.iter().skip(skip).take(per_page).enumerate() {
        let y = inner.y + slot as u16 * 3;
        let selected = focused && skip + slot == cursor;
        let style = if selected {
            Style::new().add_modifier(Modifier::REVERSED)
        } else {
            Style::new()
        };
        let marker = if skip + slot == cursor { "▶" } else { " " };
        let lines = [
            format!("{marker} {}", row.id),
            format!(
                "   {} · {} turns",
                row.name.as_deref().unwrap_or("(unnamed)"),
                row.turns
            ),
            match &row.problem {
                Some(problem) => format!("   ! {problem}"),
                None => String::new(),
            },
        ];
        for (offset, line) in lines.iter().enumerate() {
            if y + offset as u16 >= inner.bottom() {
                break;
            }
            let text = format!("{:<width$}", clip(line, width));
            let line_style = if offset == 2 && !line.is_empty() {
                style.fg(Color::Yellow)
            } else {
                style
            };
            Paragraph::new(text)
                .style(line_style)
                .render(Rect::new(inner.x, y + offset as u16, inner.width, 1), buf);
        }
    }
    render_transcript_pane(right, buf, rows.get(cursor), transcript);
}

fn render_transcript_pane(
    area: Rect,
    buf: &mut Buffer,
    row: Option<&SessionRow>,
    transcript: Option<&Projection<TranscriptView>>,
) {
    let title = match row {
        Some(row) => match &row.name {
            Some(name) => format!("{} · {name}", row.id),
            None => row.id.clone(),
        },
        None => "Transcript".to_string(),
    };
    let frame = Block::bordered()
        .title(title)
        .border_style(Style::new().fg(Color::DarkGray));
    let inner = frame.inner(area);
    frame.render(area, buf);

    match transcript {
        None => {
            Paragraph::new("Select a session to read its conversation.").render(inner, buf);
        }
        Some(Projection::Unavailable { reason }) => {
            Paragraph::new(format!("unavailable: {reason}")).render(inner, buf);
        }
        Some(Projection::Available(transcript)) => {
            if transcript.messages.is_empty() {
                Paragraph::new("This session has no messages yet.").render(inner, buf);
                return;
            }
            let rows = transcript_rows(transcript, usize::from(inner.width));
            let height = usize::from(inner.height);
            let start = rows.len().saturating_sub(height);
            for (offset, (is_header, text)) in rows[start..].iter().enumerate() {
                let style = if *is_header {
                    Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                } else {
                    Style::new()
                };
                Paragraph::new(text.as_str()).style(style).render(
                    Rect::new(inner.x, inner.y + offset as u16, inner.width, 1),
                    buf,
                );
            }
        }
    }
}

// ── Chat screen ─────────────────────────────────────────────────────────────

/// What the Chat screen shows: the conversation it is in, the model it talks
/// to, and the message whose reply is still being written.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChatView {
    /// The conversation's session, once it has one.
    pub session: Option<String>,
    /// The model chosen for it.
    pub model: Option<String>,
    /// The message just sent, while its reply is still being written.
    pub pending: Option<String>,
    /// The part of that reply that has arrived so far.
    pub partial: String,
    /// The conversation so far, as the session's journal holds it.
    pub transcript: Option<Projection<TranscriptView>>,
}

/// Render the Chat screen: the conversation filling the frame, newest at the
/// bottom, with the sent-but-unanswered message and a marker for the reply
/// being written after it.
pub fn render_chat_screen(area: Rect, buf: &mut Buffer, chat: &ChatView, focused: bool) {
    let title = format!(
        "Chat · {} · {}",
        chat.session.as_deref().unwrap_or("new conversation"),
        chat.model.as_deref().unwrap_or("no model chosen"),
    );
    let hint = if chat.pending.is_some() {
        "Esc stop the reply"
    } else {
        "⏎ send  /chat model <name>  /chat new"
    };
    let frame = Block::bordered()
        .title(title)
        .title_bottom(hint)
        .border_style(focus_border_style(focused));
    let inner = frame.inner(area);
    frame.render(area, buf);
    let width = usize::from(inner.width);

    let mut rows: Vec<(bool, String)> = match &chat.transcript {
        Some(Projection::Available(transcript)) => transcript_rows(transcript, width),
        Some(Projection::Unavailable { reason }) => {
            vec![(false, format!("unavailable: {reason}"))]
        }
        None => Vec::new(),
    };
    if let Some(pending) = &chat.pending {
        rows.push((true, "── user ".to_string()));
        rows.extend(
            wrapped_rows(pending, width)
                .into_iter()
                .map(|row| (false, row)),
        );
        rows.push((false, String::new()));
        rows.push((true, "── assistant ".to_string()));
        if chat.partial.is_empty() {
            rows.push((false, "▌ writing…".to_string()));
        } else {
            // What has arrived, with a cursor where the rest will land.
            let mut shown = wrapped_rows(&format!("{}▌", chat.partial), width);
            rows.extend(shown.drain(..).map(|row| (false, row)));
        }
    }
    if rows.is_empty() {
        rows.push((false, "Say something to start a conversation.".to_string()));
        if chat.model.is_none() {
            rows.push((
                false,
                "No model is chosen yet: /chat model <name>".to_string(),
            ));
        }
    }

    let height = usize::from(inner.height);
    let start = rows.len().saturating_sub(height);
    for (offset, (is_header, text)) in rows[start..].iter().enumerate() {
        let style = if *is_header {
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::new()
        };
        Paragraph::new(clip(text, width)).style(style).render(
            Rect::new(inner.x, inner.y + offset as u16, inner.width, 1),
            buf,
        );
    }
}

// ── Result block ────────────────────────────────────────────────────────────

/// A multi-line result shown over the lower part of the view, just above the
/// command bar: the output of a command that has more to say than the bar's
/// one line of feedback — the discovery listing, an installation verb's
/// answer. Cleared by the next keystroke in the bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultBlock {
    /// What produced it, e.g. `/backup list`.
    pub title: String,
    /// The result, one entry per line.
    pub lines: Vec<String>,
}

/// Break `line` into rows of at most `width` characters, so a long path stays
/// readable instead of being clipped at the right edge.
fn wrap_row(line: &str, width: usize) -> Vec<String> {
    if width == 0 || line.chars().count() <= width {
        return vec![line.to_string()];
    }
    line.chars()
        .collect::<Vec<_>>()
        .chunks(width)
        .map(|chunk| chunk.iter().collect())
        .collect()
}

/// The rows a block shows inside a frame `inner_width` wide and `inner_rows`
/// tall: every line wrapped to the width, then cut to the height with a final
/// row saying how many rows were left out.
fn block_rows(block: &ResultBlock, inner_width: usize, inner_rows: usize) -> Vec<String> {
    let rows: Vec<String> = block
        .lines
        .iter()
        .flat_map(|line| wrap_row(line, inner_width))
        .collect();
    if rows.len() <= inner_rows {
        return rows;
    }
    let Some(shown) = inner_rows.checked_sub(1) else {
        return Vec::new();
    };
    let left_out = rows.len() - shown;
    let mut cut: Vec<String> = rows.into_iter().take(shown).collect();
    cut.push(format!("… {left_out} more row(s)"));
    cut
}

/// Render `block` as a bordered overlay anchored above `command_bar`, as tall
/// as its content needs and no taller than the space above the bar leaves.
/// Pure function of the block and the areas, like every other renderer here.
pub fn render_result_block(area: Rect, buf: &mut Buffer, command_bar: Rect, block: &ResultBlock) {
    let available = command_bar.y.saturating_sub(area.y);
    if available < 3 || area.width < 6 {
        return;
    }
    let inner_width = usize::from(area.width - 2);
    let wanted_rows: usize = block
        .lines
        .iter()
        .map(|line| wrap_row(line, inner_width).len())
        .sum();
    let height = u16::try_from(wanted_rows.saturating_add(2))
        .unwrap_or(u16::MAX)
        .min(available);
    let rect = Rect {
        x: area.x,
        y: command_bar.y - height,
        width: area.width,
        height,
    };
    let rows = block_rows(block, inner_width, usize::from(height - 2));

    Clear.render(rect, buf);
    let frame = Block::bordered()
        .title(format!(" {} ", block.title))
        .border_style(Style::new().fg(Color::Cyan));
    Paragraph::new(rows.into_iter().map(Line::from).collect::<Vec<_>>())
        .block(frame)
        .render(rect, buf);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(lines: &[&str]) -> ResultBlock {
        ResultBlock {
            title: "/probe".to_string(),
            lines: lines.iter().map(|l| l.to_string()).collect(),
        }
    }

    fn full_row(buf: &Buffer, y: u16, width: u16) -> String {
        (0..width)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect::<String>()
    }

    fn text_row(buf: &Buffer, area: Rect) -> String {
        (area.x..area.right())
            .map(|x| buf[(x, area.y)].symbol().to_string())
            .collect()
    }

    fn kcard(id: &str, title: &str, column: BoardColumn) -> BoardCard {
        BoardCard {
            id: id.to_string(),
            title: title.to_string(),
            column,
            ..Default::default()
        }
    }

    fn sample_board() -> BoardView {
        BoardView {
            cards: vec![
                kcard("card-1", "", BoardColumn::Todo),
                kcard("card-2", "Budget guard", BoardColumn::Running),
                kcard("card-3", "", BoardColumn::Running),
                kcard("card-4", "", BoardColumn::Done),
            ],
            unmapped: Vec::new(),
        }
    }

    fn column_index(column: BoardColumn) -> usize {
        BoardColumn::ALL
            .iter()
            .position(|c| *c == column)
            .expect("every column is in ALL")
    }

    fn whole_screen(buf: &Buffer, area: Rect) -> String {
        (area.y..area.bottom())
            .map(|y| text_row(buf, Rect::new(area.x, y, area.width, 1)))
            .collect()
    }

    #[test]
    fn a_card_shows_its_priority_and_title_over_its_id_and_assignee() {
        let area = Rect::new(0, 0, 120, 12);
        let mut buf = Buffer::empty(area);
        let board = BoardView {
            cards: vec![BoardCard {
                id: "card-7".to_string(),
                title: "Budget guard".to_string(),
                priority: "P1".to_string(),
                assignee: "rhea".to_string(),
                column: BoardColumn::Running,
            }],
            unmapped: Vec::new(),
        };
        render_kanban(
            area,
            &mut buf,
            &Projection::Available(board),
            BoardCursor::default(),
            false,
        );
        let rows: Vec<String> = (0..area.height)
            .map(|y| text_row(&buf, Rect::new(0, y, area.width, 1)))
            .collect();
        let title_row = rows.iter().position(|r| r.contains("P1 Budget guard"));
        let id_row = rows.iter().position(|r| r.contains("card-7"));
        assert_eq!(
            id_row.zip(title_row).map(|(id, title)| id == title + 1),
            Some(true),
            "the title row sits directly over the id row: {rows:#?}"
        );
        let id_line = &rows[id_row.expect("id row")];
        let id_at = id_line.find("card-7").expect("id");
        let assignee_at = id_line.find("rhea").expect("assignee on the id row");
        assert!(
            assignee_at > id_at + "card-7".len(),
            "assignee is set to the right"
        );
    }

    #[test]
    fn the_cursor_is_pulled_back_onto_the_board_and_finds_its_card() {
        let board = sample_board();
        let running = column_index(BoardColumn::Running);
        let far = BoardCursor {
            column: 99,
            row: 99,
        };
        assert_eq!(far.clamped(&board).column, BoardColumn::ALL.len() - 1);
        let at = BoardCursor {
            column: running,
            row: 9,
        };
        assert_eq!(at.clamped(&board).row, 1, "last card of a two-card column");
        assert_eq!(at.card(&board).map(|c| c.id.as_str()), Some("card-3"));
        let empty = BoardCursor { column: 0, row: 3 };
        assert_eq!(empty.card(&board), None, "Triage holds no card");
        assert_eq!(empty.clamped(&board).row, 0);
    }

    #[test]
    fn the_kanban_screen_draws_every_column_with_its_count_and_its_cards() {
        let area = Rect::new(0, 0, 120, 16);
        let mut buf = Buffer::empty(area);
        render_kanban(
            area,
            &mut buf,
            &Projection::Available(sample_board()),
            BoardCursor::default(),
            true,
        );
        let header = text_row(&buf, Rect::new(0, 1, 120, 1));
        for expected in [
            "○ Triage 0",
            "○ Todo 1",
            "● Running 2",
            "● Done 1",
            "● Archive 0",
        ] {
            assert!(header.contains(expected), "{expected} in {header:?}");
        }
        let all = whole_screen(&buf, area);
        for id in ["card-1", "card-2", "card-3", "card-4", "Budget guard"] {
            assert!(all.contains(id), "{id} drawn");
        }
        assert!(all.contains("←→ column"), "the key hints are in the frame");
    }

    #[test]
    fn only_the_card_under_the_cursor_is_reversed_and_only_while_focused() {
        let area = Rect::new(0, 0, 120, 16);
        let todo = column_index(BoardColumn::Todo);
        let cursor = BoardCursor {
            column: todo,
            row: 0,
        };
        let reversed_cells = |buf: &Buffer| -> String {
            (0..area.height)
                .flat_map(|y| (0..area.width).map(move |x| (x, y)))
                .filter(|&(x, y)| buf[(x, y)].modifier.contains(Modifier::REVERSED))
                .map(|(x, y)| buf[(x, y)].symbol().to_string())
                .collect()
        };

        let mut focused = Buffer::empty(area);
        render_kanban(
            area,
            &mut focused,
            &Projection::Available(sample_board()),
            cursor,
            true,
        );
        let selected = reversed_cells(&focused);
        assert!(selected.contains("card-1"), "{selected:?}");
        assert!(!selected.contains("card-2"), "{selected:?}");

        let mut unfocused = Buffer::empty(area);
        render_kanban(
            area,
            &mut unfocused,
            &Projection::Available(sample_board()),
            cursor,
            false,
        );
        assert_eq!(
            reversed_cells(&unfocused),
            "",
            "a board without focus shows no selection"
        );
    }

    #[test]
    fn a_column_taller_than_the_screen_scrolls_to_the_cursor_and_counts_the_rest() {
        let cards: Vec<BoardCard> = (0..12)
            .map(|n| kcard(&format!("card-{n:02}"), "", BoardColumn::Ready))
            .collect();
        let board = Projection::Available(BoardView {
            cards,
            unmapped: Vec::new(),
        });
        let ready = column_index(BoardColumn::Ready);
        let area = Rect::new(0, 0, 120, 10);

        let mut top = Buffer::empty(area);
        let at_top = BoardCursor {
            column: ready,
            row: 0,
        };
        render_kanban(area, &mut top, &board, at_top, true);
        let top_text = whole_screen(&top, area);
        assert!(
            top_text.contains("card-00") && top_text.contains("… +"),
            "{top_text}"
        );

        let mut bottom = Buffer::empty(area);
        let at_bottom = BoardCursor {
            column: ready,
            row: 11,
        };
        render_kanban(area, &mut bottom, &board, at_bottom, true);
        let bottom_text = whole_screen(&bottom, area);
        assert!(
            bottom_text.contains("card-11"),
            "the cursor's card is on screen"
        );
        assert!(
            !bottom_text.contains("card-00"),
            "and the top has scrolled away"
        );
    }

    #[test]
    fn an_unavailable_board_says_why_and_cards_in_unknown_states_are_named() {
        let area = Rect::new(0, 0, 120, 10);
        let mut buf = Buffer::empty(area);
        render_kanban(
            area,
            &mut buf,
            &Projection::Unavailable {
                reason: "no store".to_string(),
            },
            BoardCursor::default(),
            true,
        );
        assert!(whole_screen(&buf, area).contains("unavailable: no store"));

        let mut buf = Buffer::empty(area);
        render_kanban(
            area,
            &mut buf,
            &Projection::Available(BoardView {
                cards: Vec::new(),
                unmapped: vec!["card-9".to_string()],
            }),
            BoardCursor::default(),
            true,
        );
        let all = whole_screen(&buf, area);
        assert!(
            all.contains("1 card(s) in an unrecognized state: card-9"),
            "{all}"
        );
    }

    fn session(id: &str, name: Option<&str>, turns: usize) -> SessionRow {
        SessionRow {
            id: id.to_string(),
            name: name.map(str::to_string),
            turns,
            entries: turns * 2,
            problem: None,
        }
    }

    fn conversation(id: &str, messages: &[(&str, &str)]) -> Projection<TranscriptView> {
        Projection::Available(TranscriptView {
            id: id.to_string(),
            messages: messages
                .iter()
                .map(|(role, text)| (role.to_string(), text.to_string()))
                .collect(),
            omitted: 0,
        })
    }

    fn sessions_frame(
        list: &Projection<SessionList>,
        cursor: usize,
        transcript: Option<&Projection<TranscriptView>>,
        focused: bool,
    ) -> (Buffer, Rect) {
        let area = Rect::new(0, 0, 100, 14);
        let mut buf = Buffer::empty(area);
        render_sessions_screen(area, &mut buf, list, cursor, transcript, focused);
        (buf, area)
    }

    #[test]
    fn the_sessions_screen_lists_sessions_and_shows_the_selected_conversation() {
        let list = Projection::Available(SessionList {
            rows: vec![
                session("s-2", Some("budget guard"), 3),
                session("s-1", None, 1),
            ],
        });
        let transcript = conversation("s-2", &[("user", "hello"), ("assistant", "hi there")]);
        let (buf, area) = sessions_frame(&list, 0, Some(&transcript), true);
        let all = whole_screen(&buf, area);
        for expected in [
            "Sessions · 2",
            "▶ s-2",
            "budget guard · 3 turns",
            "(unnamed) · 1 turns",
            "s-2 · budget guard",
            "── user",
            "hello",
            "── assistant",
            "hi there",
        ] {
            assert!(all.contains(expected), "{expected} in {all}");
        }
    }

    #[test]
    fn the_conversation_is_anchored_to_its_newest_message() {
        let list = Projection::Available(SessionList {
            rows: vec![session("s-1", None, 20)],
        });
        let messages: Vec<(String, String)> = (0..30)
            .map(|n| ("user".to_string(), format!("message number {n}")))
            .collect();
        let refs: Vec<(&str, &str)> = messages
            .iter()
            .map(|(r, t)| (r.as_str(), t.as_str()))
            .collect();
        let transcript = conversation("s-1", &refs);
        let (buf, area) = sessions_frame(&list, 0, Some(&transcript), true);
        let all = whole_screen(&buf, area);
        assert!(all.contains("message number 29"), "the newest is shown");
        assert!(
            !all.contains("message number 0 "),
            "and the oldest has scrolled off"
        );
    }

    #[test]
    fn no_sessions_and_an_unreadable_store_are_told_apart() {
        let (empty, area) = sessions_frame(
            &Projection::Available(SessionList::default()),
            0,
            None,
            true,
        );
        assert!(whole_screen(&empty, area).contains("No sessions yet."));

        let (broken, area) = sessions_frame(
            &Projection::Unavailable {
                reason: "cannot read sessions".to_string(),
            },
            0,
            None,
            true,
        );
        let text = whole_screen(&broken, area);
        assert!(text.contains("unavailable: cannot read sessions"));
        assert!(!text.contains("No sessions"));
    }

    #[test]
    fn a_session_with_a_problem_shows_it_and_a_missing_transcript_says_why() {
        let mut row = session("s-9", None, 0);
        row.problem = Some("2 damaged line(s)".to_string());
        let list = Projection::Available(SessionList { rows: vec![row] });
        let broken = Projection::Unavailable {
            reason: "written by a newer format (version 99)".to_string(),
        };
        let (buf, area) = sessions_frame(&list, 0, Some(&broken), true);
        let all = whole_screen(&buf, area);
        assert!(all.contains("! 2 damaged line(s)"), "{all}");
        assert!(
            all.contains("unavailable: written by a newer format"),
            "{all}"
        );
    }

    #[test]
    fn only_the_session_under_the_cursor_is_reversed_and_only_while_focused() {
        let list = Projection::Available(SessionList {
            rows: vec![session("s-1", None, 1), session("s-2", None, 1)],
        });
        let reversed = |buf: &Buffer, area: Rect| -> String {
            (0..area.height)
                .flat_map(|y| (0..area.width).map(move |x| (x, y)))
                .filter(|&(x, y)| buf[(x, y)].modifier.contains(Modifier::REVERSED))
                .map(|(x, y)| buf[(x, y)].symbol().to_string())
                .collect()
        };
        let (focused, area) = sessions_frame(&list, 1, None, true);
        let selected = reversed(&focused, area);
        assert!(
            selected.contains("s-2") && !selected.contains("s-1"),
            "{selected:?}"
        );
        let (unfocused, area) = sessions_frame(&list, 1, None, false);
        assert_eq!(reversed(&unfocused, area), "");
    }

    #[test]
    fn the_chat_screen_shows_the_conversation_the_pending_message_and_the_reply_marker() {
        let area = Rect::new(0, 0, 80, 14);
        let render = |chat: &ChatView, focused: bool| {
            let mut buf = Buffer::empty(area);
            render_chat_screen(area, &mut buf, chat, focused);
            whole_screen(&buf, area)
        };

        let empty = render(&ChatView::default(), true);
        assert!(
            empty.contains("Chat · new conversation · no model chosen"),
            "{empty}"
        );
        assert!(empty.contains("Say something to start a conversation."));
        assert!(empty.contains("/chat model <name>"));

        let chat = ChatView {
            session: Some("s-1".to_string()),
            model: Some("llama3".to_string()),
            pending: Some("and now?".to_string()),
            partial: String::new(),
            transcript: Some(conversation(
                "s-1",
                &[("user", "hi"), ("assistant", "hello")],
            )),
        };
        let busy = render(&chat, true);
        for expected in [
            "Chat · s-1 · llama3",
            "── user",
            "hi",
            "hello",
            "and now?",
            "▌ writing…",
            "Esc stop the reply",
        ] {
            assert!(busy.contains(expected), "{expected} in {busy}");
        }
        let idle = render(
            &ChatView {
                pending: None,
                ..chat
            },
            true,
        );
        assert!(!idle.contains("▌ writing…"));
        assert!(idle.contains("⏎ send"));
    }

    #[test]
    fn the_part_of_a_reply_that_has_arrived_is_shown_with_a_cursor_where_the_rest_will_land() {
        let area = Rect::new(0, 0, 60, 10);
        let mut buf = Buffer::empty(area);
        let chat = ChatView {
            session: Some("s-1".to_string()),
            model: Some("m".to_string()),
            pending: Some("tell me".to_string()),
            partial: "the gate is".to_string(),
            transcript: None,
        };
        render_chat_screen(area, &mut buf, &chat, true);
        let shown = whole_screen(&buf, area);
        assert!(shown.contains("the gate is▌"), "{shown}");
        assert!(
            !shown.contains("writing…"),
            "the marker gives way to the text: {shown}"
        );
    }

    #[test]
    fn an_unreadable_conversation_says_so_instead_of_showing_a_blank() {
        let area = Rect::new(0, 0, 80, 8);
        let mut buf = Buffer::empty(area);
        let chat = ChatView {
            session: Some("s-1".to_string()),
            transcript: Some(Projection::Unavailable {
                reason: "cannot read sessions".to_string(),
            }),
            ..Default::default()
        };
        render_chat_screen(area, &mut buf, &chat, true);
        assert!(whole_screen(&buf, area).contains("unavailable: cannot read sessions"));
    }

    #[test]
    fn the_function_key_bar_labels_only_what_it_can_do_and_brightens_the_active_number() {
        let area = Rect::new(0, 0, 100, 1);
        let mut buf = Buffer::empty(area);
        render_fkeys(area, &mut buf, Some(6));
        let row = text_row(&buf, area);
        for label in ["1Help", "3Chat", "4Sess", "6Office", "7Board", "10Quit"] {
            assert!(row.contains(label), "{label} in {row:?}");
        }
        for unbound in ["2Menu", "5Inbox", "9Pause"] {
            assert!(
                !row.contains(unbound),
                "{unbound} must not be advertised: {row:?}"
            );
        }
        let white = |x: u16| buf[(x, 0)].fg == Color::White;
        let six = row.find("6Office").expect("slot 6") as u16;
        assert!(white(six), "the active slot's number is white");
        assert!(!white(0), "an inactive slot's number is not");
    }

    #[test]
    fn the_input_block_names_who_the_line_is_addressed_to() {
        let area = Rect::new(0, 0, 60, 3);
        for (addressee, label) in [
            (Addressee::Command, "Command"),
            (Addressee::Installation, "Installation"),
        ] {
            let mut buf = Buffer::empty(area);
            render_input(area, &mut buf, "doctor", addressee, true, true);
            assert!(text_row(&buf, Rect::new(0, 0, 60, 1)).contains(label));
            assert!(text_row(&buf, Rect::new(0, 1, 60, 1)).starts_with("❯ /doctor"));
        }
    }

    #[test]
    fn a_rule_fits_its_width_and_degrades_to_a_plain_line() {
        assert_eq!(rule(30, "L", "R").chars().count(), 30);
        assert_eq!(rule(4, "a long label", "and another"), "────");
    }

    #[test]
    fn the_status_line_shows_the_core_status_and_version_or_why_it_is_unavailable() {
        let area = Rect::new(0, 0, 50, 1);
        let mut buf = Buffer::empty(area);
        render_status_line(
            area,
            &mut buf,
            &Projection::Available(StatusView {
                version: "1.2.3".to_string(),
                status: "running".to_string(),
            }),
        );
        let row = text_row(&buf, area);
        assert!(row.contains("running") && row.contains("v1.2.3"), "{row:?}");

        let mut buf = Buffer::empty(area);
        render_status_line(
            area,
            &mut buf,
            &Projection::Unavailable {
                reason: "no core".to_string(),
            },
        );
        assert!(text_row(&buf, area).contains("unavailable: no core"));
    }

    #[test]
    fn a_long_line_wraps_instead_of_being_clipped() {
        assert_eq!(wrap_row("abcdefgh", 3), vec!["abc", "def", "gh"]);
        assert_eq!(wrap_row("abc", 3), vec!["abc"]);
        assert_eq!(wrap_row("", 3), vec![""]);
    }

    #[test]
    fn a_block_taller_than_the_space_is_cut_with_a_count_of_what_is_left_out() {
        let rows = block_rows(&block(&["1", "2", "3", "4", "5"]), 10, 3);
        assert_eq!(rows, vec!["1", "2", "… 3 more row(s)"]);
        assert_eq!(
            block_rows(&block(&["1", "2"]), 10, 3),
            vec!["1", "2"],
            "a block that fits is shown whole"
        );
    }

    #[test]
    fn the_block_sits_directly_above_the_command_bar_and_carries_its_title() {
        let area = Rect::new(0, 0, 40, 12);
        let bar = Rect::new(0, 11, 40, 1);
        let mut buf = Buffer::empty(area);
        render_result_block(
            area,
            &mut buf,
            bar,
            &block(&["workspace: main", "phase: ready"]),
        );

        // Two content rows plus two border rows end on the row above the bar.
        assert!(full_row(&buf, 7, 40).contains("/probe"), "title row");
        assert!(full_row(&buf, 8, 40).contains("workspace: main"));
        assert!(full_row(&buf, 9, 40).contains("phase: ready"));
        assert!(
            full_row(&buf, 11, 40).trim().is_empty(),
            "the bar's own row is left to the bar"
        );
    }

    #[test]
    fn a_frame_too_small_to_hold_a_block_draws_nothing() {
        let area = Rect::new(0, 0, 40, 3);
        let bar = Rect::new(0, 2, 40, 1);
        let mut buf = Buffer::empty(area);
        render_result_block(area, &mut buf, bar, &block(&["x"]));
        assert_eq!(buf, Buffer::empty(area));
    }

    #[test]
    fn layout_focus_splits_into_four_panels_plus_command_bar() {
        let area = Rect::new(0, 0, 80, 24);
        let panels = layout(area);

        // From the bottom up: status line, function keys, a three-row input
        // block (the command bar is its prompt row), ticker; the tab bar is
        // the first row.
        assert_eq!(panels.command_bar.height, 1);
        assert_eq!(
            panels.status_line.bottom(),
            area.height,
            "status on the last row"
        );
        assert_eq!(panels.fkeys.bottom(), panels.status_line.y);
        assert_eq!(panels.input.bottom(), panels.fkeys.y);
        assert_eq!(panels.input.height, 3);
        assert_eq!(
            panels.command_bar.y,
            panels.input.y + 1,
            "prompt is the middle row"
        );
        assert_eq!(panels.ticker.bottom(), panels.input.y);
        assert_eq!(panels.tabs.y, 0);
        assert_eq!(panels.board.y, panels.tabs.bottom());
        assert!(panels.sessions.bottom() <= panels.ticker.y);

        // All four view panels are non-empty at a representative size.
        for rect in [panels.board, panels.office, panels.status, panels.sessions] {
            assert!(rect.width > 0 && rect.height > 0, "panel {rect:?} is empty");
        }

        // Top row panels are side by side; bottom row sits below the top row.
        assert_eq!(panels.board.y, panels.office.y, "board/office share a row");
        assert!(panels.board.x < panels.office.x, "board is left of office");
        assert!(
            panels.status.y > panels.board.y,
            "status is below the board"
        );
    }

    #[test]
    fn layout_focus_does_not_panic_on_tiny_sizes() {
        for area in [
            Rect::new(0, 0, 0, 0),
            Rect::new(0, 0, 1, 1),
            Rect::new(0, 0, 3, 2),
        ] {
            let panels = layout(area);
            // Command bar never exceeds the available height.
            assert!(panels.command_bar.height <= area.height);
        }
    }

    #[test]
    fn layout_focus_tab_cycles_in_order_and_wraps() {
        let order: Vec<Focus> = std::iter::successors(Some(Focus::Board), |f| Some(f.next()))
            .take(6)
            .collect();
        assert_eq!(
            order,
            vec![
                Focus::Board,
                Focus::Office,
                Focus::Status,
                Focus::Sessions,
                Focus::CommandBar,
                Focus::Board, // wrapped
            ]
        );
    }

    #[test]
    fn layout_focus_shift_tab_cycles_backward_and_wraps() {
        assert_eq!(
            Focus::Board.prev(),
            Focus::CommandBar,
            "prev wraps to the end"
        );
        assert_eq!(Focus::Office.prev(), Focus::Board);
        // next then prev is the identity for every variant.
        for &f in &Focus::ORDER {
            assert_eq!(f.next().prev(), f);
        }
    }

    // ── Panel-render tests (off-screen Buffer) ──────────────────────────────

    /// Concatenate the cell symbols of row `y` within `rect` into a string.
    fn row_text(buf: &Buffer, rect: Rect, y: u16) -> String {
        (rect.x..rect.right())
            .filter_map(|x| buf.cell((x, y)).map(|c| c.symbol().to_string()))
            .collect()
    }

    /// Whether `needle` appears on any row inside `rect`.
    fn area_contains(buf: &Buffer, rect: Rect, needle: &str) -> bool {
        (rect.y..rect.bottom()).any(|y| row_text(buf, rect, y).contains(needle))
    }

    /// Every row inside `rect`, joined by newlines.
    fn area_text(buf: &Buffer, rect: Rect) -> String {
        (rect.y..rect.bottom())
            .map(|y| row_text(buf, rect, y))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn card(id: &str, column: BoardColumn) -> BoardCard {
        BoardCard {
            id: id.to_string(),
            column,
            ..Default::default()
        }
    }

    fn render_board_buffer(board: &BoardView, area: Rect) -> Buffer {
        let mut buf = Buffer::empty(area);
        render_board(area, &mut buf, &Projection::Available(board.clone()), false);
        buf
    }

    #[test]
    fn board_office_render_places_each_card_under_its_column() {
        let board = BoardView {
            cards: BoardColumn::ALL
                .iter()
                .enumerate()
                .map(|(i, &col)| card(&format!("k{i}"), col))
                .collect(),
            ..Default::default()
        };
        let area = Rect::new(0, 0, 70, 10);
        let buf = render_board_buffer(&board, area);

        let inner = Block::bordered().inner(area);
        let columns = board_columns(inner);
        for (i, column_area) in columns.iter().enumerate() {
            let id = format!("k{i}");
            assert!(
                area_contains(&buf, *column_area, &id),
                "{id} should render under column {i} ({:?})",
                BoardColumn::ALL[i]
            );
        }
    }

    #[test]
    fn board_office_render_moves_card_to_new_column_on_next_snapshot() {
        let area = Rect::new(0, 0, 70, 10);
        let inner = Block::bordered().inner(area);
        let columns = board_columns(inner);
        let todo_area = columns[1]; // BoardColumn::Todo
        let running_area = columns[3]; // BoardColumn::Running

        let before = BoardView {
            cards: vec![card("m1", BoardColumn::Todo)],
            ..Default::default()
        };
        let buf_before = render_board_buffer(&before, area);
        assert!(area_contains(&buf_before, todo_area, "m1"));
        assert!(!area_contains(&buf_before, running_area, "m1"));

        // Same card, moved one column over in the next snapshot.
        let after = BoardView {
            cards: vec![card("m1", BoardColumn::Running)],
            ..Default::default()
        };
        let buf_after = render_board_buffer(&after, area);
        assert!(
            !area_contains(&buf_after, todo_area, "m1"),
            "card left the Todo column"
        );
        assert!(
            area_contains(&buf_after, running_area, "m1"),
            "card re-rendered under the Running column"
        );
    }

    /// A card the board has no column for is named in a note under the
    /// columns, and the cards it could place still render where they belong.
    #[test]
    fn board_render_names_cards_it_could_not_place_and_still_places_the_rest() {
        let board = BoardView {
            cards: vec![card("ok1", BoardColumn::Todo)],
            unmapped: vec!["odd1".to_string(), "odd2".to_string()],
        };
        let area = Rect::new(0, 0, 70, 10);
        let buf = render_board_buffer(&board, area);

        let inner = Block::bordered().inner(area);
        let text = area_text(&buf, inner);
        assert!(
            text.contains("2 card(s) in an unrecognized state"),
            "{text}"
        );
        assert!(text.contains("odd1") && text.contains("odd2"), "{text}");
        assert!(
            text.contains("ok1"),
            "the placed card still renders: {text}"
        );
    }

    #[test]
    fn board_office_render_shows_agent_to_task_lines() {
        let office = OfficeView {
            agents: vec![
                AgentActivity {
                    agent: "orchestrator".to_string(),
                    task: "planning".to_string(),
                },
                AgentActivity {
                    agent: "coder".to_string(),
                    task: String::new(),
                },
            ],
        };
        let area = Rect::new(0, 0, 40, 6);
        let mut buf = Buffer::empty(area);
        render_office(area, &mut buf, &Projection::Available(office), false);

        let inner = Block::bordered().inner(area);
        assert!(
            area_contains(&buf, inner, "orchestrator → planning"),
            "active agent renders its current task"
        );
        assert!(
            area_contains(&buf, inner, "coder → idle"),
            "an empty task renders as idle"
        );
    }

    /// The first row within `rect` containing `needle`, if any.
    fn row_of(buf: &Buffer, rect: Rect, needle: &str) -> Option<u16> {
        (rect.y..rect.bottom()).find(|&y| row_text(buf, rect, y).contains(needle))
    }

    #[test]
    fn status_sessions_render_status_mirrors_snapshot() {
        let area = Rect::new(0, 0, 50, 6);
        let mut buf = Buffer::empty(area);
        let status = Projection::Available(StatusView {
            version: "0.1.0".to_string(),
            status: "running | progress 60%".to_string(),
        });
        render_status(area, &mut buf, &status, false);

        let inner = Block::bordered().inner(area);
        assert!(
            area_contains(&buf, inner, "version 0.1.0"),
            "version is mirrored"
        );
        assert!(
            area_contains(&buf, inner, "running | progress 60%"),
            "status line is mirrored"
        );
    }

    #[test]
    fn status_sessions_render_appends_entries_in_order() {
        let mut sessions = SessionsView::default();
        sessions.push("first");
        sessions.push("second");
        sessions.push("third");

        let area = Rect::new(0, 0, 30, 8);
        let mut buf = Buffer::empty(area);
        render_sessions(area, &mut buf, &Projection::Available(sessions), false);

        let inner = Block::bordered().inner(area);
        let first = row_of(&buf, inner, "first").expect("first present");
        let second = row_of(&buf, inner, "second").expect("second present");
        let third = row_of(&buf, inner, "third").expect("third present");
        assert!(
            first < second && second < third,
            "entries render in push order"
        );
    }

    #[test]
    fn status_sessions_render_bounds_scrollback() {
        let mut sessions = SessionsView::default();
        for i in 0..(MAX_SESSION_LINES + 50) {
            sessions.push(format!("e{i}"));
        }

        // Growth is bounded to the cap…
        assert_eq!(sessions.entries().len(), MAX_SESSION_LINES);
        // …dropping the oldest while keeping the newest, in order.
        assert_eq!(sessions.entries().first().unwrap(), "e50");
        assert_eq!(
            sessions.entries().last().unwrap(),
            &format!("e{}", MAX_SESSION_LINES + 49)
        );
    }

    // ── Unavailable vs. empty ───────────────────────────────────────
    //
    // Each panel gets its own positive proof — an `Unavailable` projection
    // renders its reason — plus one shared negative proof that a genuinely
    // *empty* `Available` projection never renders that same word, so the
    // two states are checked as distinguishable, not merely rendered
    // separately from each other by coincidence.

    #[test]
    fn render_board_shows_the_reason_when_unavailable() {
        let area = Rect::new(0, 0, 40, 6);
        let mut buf = Buffer::empty(area);
        let board = Projection::Unavailable {
            reason: "kanban store unreadable".to_string(),
        };
        render_board(area, &mut buf, &board, false);

        let inner = Block::bordered().inner(area);
        assert!(area_contains(
            &buf,
            inner,
            "unavailable: kanban store unreadable"
        ));
    }

    #[test]
    fn render_office_shows_the_reason_when_unavailable() {
        let area = Rect::new(0, 0, 40, 6);
        let mut buf = Buffer::empty(area);
        let office = Projection::Unavailable {
            reason: "role roster unreadable".to_string(),
        };
        render_office(area, &mut buf, &office, false);

        let inner = Block::bordered().inner(area);
        assert!(area_contains(
            &buf,
            inner,
            "unavailable: role roster unreadable"
        ));
    }

    #[test]
    fn render_status_shows_the_reason_when_unavailable() {
        let area = Rect::new(0, 0, 40, 6);
        let mut buf = Buffer::empty(area);
        let status = Projection::Unavailable {
            reason: "core status unreachable".to_string(),
        };
        render_status(area, &mut buf, &status, false);

        let inner = Block::bordered().inner(area);
        assert!(area_contains(
            &buf,
            inner,
            "unavailable: core status unreachable"
        ));
    }

    #[test]
    fn render_sessions_shows_the_reason_when_unavailable() {
        let area = Rect::new(0, 0, 40, 6);
        let mut buf = Buffer::empty(area);
        let sessions = Projection::Unavailable {
            reason: "activity log unreachable".to_string(),
        };
        render_sessions(area, &mut buf, &sessions, false);

        let inner = Block::bordered().inner(area);
        assert!(area_contains(
            &buf,
            inner,
            "unavailable: activity log unreachable"
        ));
    }

    /// The distinguishing half: a genuinely empty `Available` projection —
    /// the state every panel starts in before any real data has arrived —
    /// never renders the word "unavailable" on any of the four panels. An
    /// `Unavailable` reason and an empty `Available` value must be visibly
    /// different states in the view model, never the same rendering by
    /// coincidence.
    #[test]
    fn an_empty_available_projection_never_renders_as_unavailable() {
        let area = Rect::new(0, 0, 80, 24);
        let panels = layout(area);
        let mut buf = Buffer::empty(area);

        render_board(panels.board, &mut buf, &Projection::default(), false);
        render_office(panels.office, &mut buf, &Projection::default(), false);
        render_status(panels.status, &mut buf, &Projection::default(), false);
        render_sessions(panels.sessions, &mut buf, &Projection::default(), false);

        for panel in [panels.board, panels.office, panels.status, panels.sessions] {
            assert!(
                !area_contains(&buf, panel, "unavailable"),
                "an empty Available projection must never render as unavailable: {panel:?}"
            );
        }
    }
}
