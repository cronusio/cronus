//! Workspace management — create, list, switch, delete, and check workspaces.
//!
//! Moved wholesale to `cronus-store-local`: every operation is
//! SQLite-backed and nothing else in the domain tier references these types
//! directly (only frontends, through this facade re-export). Workspace IDs
//! follow kebab-case: `^[a-z0-9][a-z0-9-]*[a-z0-9]$`, max 64 chars.

pub use cronus_store_local::workspace::{
    Workspace, WorkspaceError, WorkspaceId, WorkspaceManager, WorkspaceStatus, WorkspaceTemplate,
};
