//! Panel layout and focus navigation — the read-only view-surface skeleton.
//!
//! This module is render-only: it computes screen regions from the terminal area
//! and tracks which region has focus. It holds no domain state and performs no
//! core calls; panels are fed by the [`crate::app::ViewModel`] snapshot.

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph, Widget};

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

/// Screen regions for the four panels plus the command bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelAreas {
    /// Board panel (top-left).
    pub board: Rect,
    /// Office panel (top-right).
    pub office: Rect,
    /// Status panel (bottom-left).
    pub status: Rect,
    /// Sessions / log panel (bottom-right).
    pub sessions: Rect,
    /// Single-row command bar pinned to the very bottom.
    pub command_bar: Rect,
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

/// Split `area` into a 2×2 grid of view panels above a one-row command bar.
///
/// ratatui's constraint solver clamps to the available space, so this degrades
/// to zero-sized regions at tiny terminal sizes rather than panicking.
pub fn layout(area: Rect) -> PanelAreas {
    let [body, command_bar] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);

    let [top, bottom] =
        Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(body);

    let [board, office] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(top);

    let [status, sessions] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(bottom);

    PanelAreas {
        board,
        office,
        status,
        sessions,
        command_bar,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_focus_splits_into_four_panels_plus_command_bar() {
        let area = Rect::new(0, 0, 80, 24);
        let panels = layout(area);

        // The command bar is a single row pinned to the bottom.
        assert_eq!(panels.command_bar.height, 1);
        assert_eq!(
            panels.command_bar.y + panels.command_bar.height,
            area.height,
            "command bar sits on the last row"
        );

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
            title: String::new(),
            column,
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
