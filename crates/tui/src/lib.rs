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
pub mod command;
#[cfg(test)]
mod conformance_registration;
pub mod dispatch;
pub mod installation;
pub mod pane_actions;
pub mod terminal;
pub mod view;

pub use app::{
    App, CapabilitySource, CoreSnapshot, RatatuiRenderer, Renderer, SnapshotSource, TickResult,
    ViewModel, render_view, run, run_with,
};
pub use command::{
    CommandOutcome, CommandSpec, ParseError, SlashCommand, build_catalog, classify, parse,
};
pub use dispatch::{bind_args, dispatch_command, render_outcome};
pub use pane_actions::PaneAction;
pub use terminal::{CrosstermBackend, Key, TermEvent, TerminalBackend, Tui};
pub use view::{
    AgentActivity, BoardCard, BoardColumn, BoardView, Focus, OfficeView, PanelAreas, ResultBlock,
    SessionsView, layout, render_board, render_office, render_result_block, render_sessions,
    render_status,
};
