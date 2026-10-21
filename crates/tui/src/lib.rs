//! Cronus TUI library — the interactive terminal frontend over the core.
//!
//! This crate is pure presentation: it renders core state and maps key/slash
//! input to core capability calls. It holds only view state, never domain state,
//! and links the core library directly (never the CLI frontend).
//!
//! Layer breakdown:
//! - [`terminal`] — raw-mode lifecycle + event polling (the RAII shell).
//! - [`app`] — the event-driven render loop and the view-model it drives.

pub mod app;
pub mod chat_actions;
pub mod command;
#[cfg(test)]
mod conformance_registration;
pub mod dispatch;
pub mod installation;
pub mod pane_actions;
pub mod terminal;
pub mod view;
mod work;

pub use app::{
    App, CapabilitySource, CoreSnapshot, Hosted, RatatuiRenderer, Renderer, SnapshotSource,
    TickResult, ViewModel, compose_live, render_view, run, run_with,
};
pub use chat_actions::ChatAction;
pub use command::{
    CommandOutcome, CommandSpec, ParseError, SlashCommand, build_catalog, classify, parse,
};
pub use dispatch::{bind_args, dispatch_command, render_outcome};
pub use pane_actions::PaneAction;
pub use terminal::{CrosstermBackend, Key, TermEvent, TerminalBackend, Tui};
pub use view::{
    AgentActivity, BoardCard, BoardColumn, BoardCursor, BoardView, ChatView, Focus, OfficeView,
    PanelAreas, Projection, ResultBlock, Screen, SessionList, SessionRow, SessionsView,
    TranscriptView, layout, render_board, render_chat_screen, render_kanban, render_office,
    render_result_block, render_sessions, render_sessions_screen, render_status,
};
