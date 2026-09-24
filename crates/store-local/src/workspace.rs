//! Workspace management — create, list, switch, delete, and check workspaces.
//!
//! Workspace IDs follow kebab-case: `^[a-z0-9][a-z0-9-]*[a-z0-9]$`, max 64 chars.
//! The active workspace is tracked in a metadata table alongside the workspace
//! registry — both live in the same SQLite database.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, params};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

// ── WorkspaceId ───────────────────────────────────────────────────────────────

/// A validated workspace identifier.
///
/// Format: `^[a-z0-9][a-z0-9-]*[a-z0-9]$`, min 2 chars, max 64 chars.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorkspaceId(String);

impl WorkspaceId {
    pub fn new(s: impl Into<String>) -> Result<Self, WorkspaceError> {
        let s = s.into();
        if !is_valid_id(&s) {
            return Err(WorkspaceError::InvalidId(s));
        }
        Ok(WorkspaceId(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for WorkspaceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

fn is_valid_id(s: &str) -> bool {
    if s.len() < 2 || s.len() > 64 {
        return false;
    }
    let bytes = s.as_bytes();
    let first = bytes[0];
    let last = bytes[bytes.len() - 1];
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return false;
    }
    if !last.is_ascii_lowercase() && !last.is_ascii_digit() {
        return false;
    }
    bytes[1..bytes.len() - 1]
        .iter()
        .all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

// ── WorkspaceTemplate ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum WorkspaceTemplate {
    Default,
    Empty,
    Custom(PathBuf),
}

impl WorkspaceTemplate {
    pub fn as_str(&self) -> &str {
        match self {
            WorkspaceTemplate::Default => "Default",
            WorkspaceTemplate::Empty => "Empty",
            WorkspaceTemplate::Custom(_) => "Custom",
        }
    }
}

// ── Workspace ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    pub path: PathBuf,
    pub created_at: u64,
    pub template: String,
}

// ── WorkspaceStatus ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct WorkspaceStatus {
    pub exists: bool,
    pub is_active: bool,
    pub path_exists: bool,
}

// ── WorkspaceError ────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum WorkspaceError {
    Database(rusqlite::Error),
    /// The registry file's schema version could not be reconciled with the
    /// one this build understands (a newer file, or a failed migration).
    Schema(crate::versioning::SchemaError),
    /// The ID does not match the kebab-case format.
    InvalidId(String),
    /// No workspace with the given ID exists.
    NotFound(String),
    /// A workspace with the given ID already exists.
    AlreadyExists(String),
}

impl std::fmt::Display for WorkspaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkspaceError::Database(e) => write!(f, "workspace database error: {e}"),
            WorkspaceError::Schema(e) => write!(f, "{e}"),
            WorkspaceError::InvalidId(id) => {
                write!(
                    f,
                    "invalid workspace ID '{id}': must be lowercase kebab-case, 2–64 chars"
                )
            }
            WorkspaceError::NotFound(id) => write!(f, "workspace '{id}' not found"),
            WorkspaceError::AlreadyExists(id) => write!(f, "workspace '{id}' already exists"),
        }
    }
}

impl std::error::Error for WorkspaceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            WorkspaceError::Database(e) => Some(e),
            WorkspaceError::Schema(e) => Some(e),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for WorkspaceError {
    fn from(e: rusqlite::Error) -> Self {
        WorkspaceError::Database(e)
    }
}

impl From<crate::versioning::SchemaError> for WorkspaceError {
    fn from(e: crate::versioning::SchemaError) -> Self {
        WorkspaceError::Schema(e)
    }
}

// ── WorkspaceManager ──────────────────────────────────────────────────────────

pub struct WorkspaceManager {
    conn: Connection,
}

impl WorkspaceManager {
    pub fn open<P: AsRef<Path>>(db_path: P) -> Result<Self, WorkspaceError> {
        let conn = Connection::open(db_path)?;
        setup(&conn)?;
        Ok(WorkspaceManager { conn })
    }

    pub fn open_in_memory() -> Result<Self, WorkspaceError> {
        let conn = Connection::open_in_memory()?;
        setup(&conn)?;
        Ok(WorkspaceManager { conn })
    }

    /// Create a new workspace. Fails if a workspace with the same ID exists.
    pub fn create(
        &self,
        id: &WorkspaceId,
        name: &str,
        path: &Path,
        template: WorkspaceTemplate,
    ) -> Result<Workspace, WorkspaceError> {
        if self.get(id)?.is_some() {
            return Err(WorkspaceError::AlreadyExists(id.as_str().to_owned()));
        }
        let now = now_secs();
        let path_str = path.to_string_lossy();
        let template_str = template.as_str();
        self.conn.execute(
            "INSERT INTO workspaces (id, name, path, created_at, template)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                id.as_str(),
                name,
                path_str.as_ref(),
                now as i64,
                template_str
            ],
        )?;
        Ok(Workspace {
            id: id.clone(),
            name: name.to_owned(),
            path: path.to_path_buf(),
            created_at: now,
            template: template_str.to_owned(),
        })
    }

    pub fn get(&self, id: &WorkspaceId) -> Result<Option<Workspace>, WorkspaceError> {
        let result = self.conn.query_row(
            "SELECT id, name, path, created_at, template FROM workspaces WHERE id = ?1",
            params![id.as_str()],
            map_row,
        );
        match result {
            Ok(ws) => Ok(Some(ws)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(WorkspaceError::Database(e)),
        }
    }

    pub fn list(&self) -> Result<Vec<Workspace>, WorkspaceError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, path, created_at, template FROM workspaces ORDER BY created_at",
        )?;
        let workspaces = stmt
            .query_map([], map_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(workspaces)
    }

    /// Delete a workspace. Returns `true` if deleted, `false` if it did not exist.
    pub fn delete(&self, id: &WorkspaceId) -> Result<bool, WorkspaceError> {
        let n = self
            .conn
            .execute("DELETE FROM workspaces WHERE id = ?1", params![id.as_str()])?;
        if n > 0 {
            // Clear active if it was this workspace
            self.conn.execute(
                "DELETE FROM metadata WHERE key = 'active_workspace' AND value = ?1",
                params![id.as_str()],
            )?;
        }
        Ok(n > 0)
    }

    /// Set the active workspace. The workspace must exist.
    pub fn set_active(&self, id: &WorkspaceId) -> Result<(), WorkspaceError> {
        // Verify existence
        if self.get(id)?.is_none() {
            return Err(WorkspaceError::NotFound(id.as_str().to_owned()));
        }
        self.conn.execute(
            "INSERT INTO metadata (key, value) VALUES ('active_workspace', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![id.as_str()],
        )?;
        Ok(())
    }

    pub fn get_active(&self) -> Result<Option<WorkspaceId>, WorkspaceError> {
        let result = self.conn.query_row(
            "SELECT value FROM metadata WHERE key = 'active_workspace'",
            [],
            |row| row.get::<_, String>(0),
        );
        match result {
            Ok(s) => Ok(Some(WorkspaceId::new(s)?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(WorkspaceError::Database(e)),
        }
    }

    pub fn check(&self, id: &WorkspaceId) -> Result<WorkspaceStatus, WorkspaceError> {
        let ws = self.get(id)?;
        let active = self
            .get_active()?
            .as_ref()
            .map(|a| a == id)
            .unwrap_or(false);
        Ok(WorkspaceStatus {
            exists: ws.is_some(),
            is_active: active,
            path_exists: ws.map(|w| w.path.exists()).unwrap_or(false),
        })
    }
}

// ── schema ────────────────────────────────────────────────────────────────────

/// The schema version this build reads and writes (`crate::versioning`).
const SCHEMA_VERSION: i64 = 1;

fn setup(conn: &Connection) -> Result<(), WorkspaceError> {
    conn.execute_batch("PRAGMA journal_mode = WAL")?;
    crate::versioning::open_schema(
        conn,
        "workspace registry",
        SCHEMA_VERSION,
        create_schema,
        &[],
    )
}

fn create_schema(conn: &Connection) -> Result<(), WorkspaceError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS workspaces (
            id         TEXT PRIMARY KEY NOT NULL,
            name       TEXT NOT NULL,
            path       TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            template   TEXT NOT NULL DEFAULT 'Default'
        )",
    )?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS metadata (
            key   TEXT PRIMARY KEY NOT NULL,
            value TEXT NOT NULL
        )",
    )?;
    Ok(())
}

// ── row mapper ────────────────────────────────────────────────────────────────

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Workspace> {
    let id_str: String = row.get(0)?;
    let path_str: String = row.get(2)?;
    let created_at: i64 = row.get(3)?;
    Ok(Workspace {
        id: WorkspaceId(id_str),
        name: row.get(1)?,
        path: PathBuf::from(path_str),
        created_at: created_at as u64,
        template: row.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_workspace_ids() {
        assert!(WorkspaceId::new("my-project").is_ok());
        assert!(WorkspaceId::new("project123").is_ok());
        assert!(WorkspaceId::new("a0").is_ok());
    }

    #[test]
    fn invalid_workspace_ids() {
        assert!(WorkspaceId::new("a").is_err(), "too short");
        assert!(WorkspaceId::new("-start").is_err(), "starts with dash");
        assert!(WorkspaceId::new("end-").is_err(), "ends with dash");
        assert!(WorkspaceId::new("UPPER").is_err(), "uppercase");
        assert!(WorkspaceId::new("has space").is_err(), "space");
        let long = "a".repeat(65);
        assert!(WorkspaceId::new(long).is_err(), "too long");
    }

    #[test]
    fn create_and_get_workspace() {
        let mgr = WorkspaceManager::open_in_memory().unwrap();
        let id = WorkspaceId::new("test-ws").unwrap();
        let ws = mgr
            .create(
                &id,
                "Test Workspace",
                Path::new("/tmp/test"),
                WorkspaceTemplate::Default,
            )
            .unwrap();
        assert_eq!(ws.id, id);
        assert_eq!(ws.name, "Test Workspace");

        let got = mgr.get(&id).unwrap().expect("must exist");
        assert_eq!(got.id, id);
    }

    #[test]
    fn duplicate_create_fails() {
        let mgr = WorkspaceManager::open_in_memory().unwrap();
        let id = WorkspaceId::new("dup").unwrap();
        mgr.create(&id, "First", Path::new("/a"), WorkspaceTemplate::Default)
            .unwrap();
        let err = mgr.create(&id, "Second", Path::new("/b"), WorkspaceTemplate::Default);
        assert!(matches!(err, Err(WorkspaceError::AlreadyExists(_))));
    }

    #[test]
    fn list_returns_all_workspaces() {
        let mgr = WorkspaceManager::open_in_memory().unwrap();
        for i in 0..3u8 {
            let id = WorkspaceId::new(format!("ws-{i}")).unwrap();
            mgr.create(
                &id,
                &format!("WS {i}"),
                Path::new("/tmp"),
                WorkspaceTemplate::Empty,
            )
            .unwrap();
        }
        assert_eq!(mgr.list().unwrap().len(), 3);
    }

    #[test]
    fn delete_workspace() {
        let mgr = WorkspaceManager::open_in_memory().unwrap();
        let id = WorkspaceId::new("to-delete").unwrap();
        mgr.create(
            &id,
            "Delete me",
            Path::new("/tmp"),
            WorkspaceTemplate::Default,
        )
        .unwrap();
        assert!(mgr.delete(&id).unwrap());
        assert!(mgr.get(&id).unwrap().is_none());
        assert!(!mgr.delete(&id).unwrap(), "second delete must return false");
    }

    #[test]
    fn active_workspace_lifecycle() {
        let mgr = WorkspaceManager::open_in_memory().unwrap();
        let id = WorkspaceId::new("active-ws").unwrap();
        mgr.create(&id, "Active", Path::new("/tmp"), WorkspaceTemplate::Default)
            .unwrap();

        assert!(
            mgr.get_active().unwrap().is_none(),
            "no active workspace initially"
        );
        mgr.set_active(&id).unwrap();
        assert_eq!(
            mgr.get_active().unwrap().as_ref().map(|i| i.as_str()),
            Some("active-ws")
        );
    }

    #[test]
    fn set_active_nonexistent_workspace_fails() {
        let mgr = WorkspaceManager::open_in_memory().unwrap();
        let id = WorkspaceId::new("ghost").unwrap();
        assert!(matches!(
            mgr.set_active(&id),
            Err(WorkspaceError::NotFound(_))
        ));
    }

    #[test]
    fn check_workspace_status() {
        let mgr = WorkspaceManager::open_in_memory().unwrap();
        let id = WorkspaceId::new("chk").unwrap();
        mgr.create(&id, "Chk", Path::new("/tmp"), WorkspaceTemplate::Default)
            .unwrap();
        mgr.set_active(&id).unwrap();

        let status = mgr.check(&id).unwrap();
        assert!(status.exists);
        assert!(status.is_active);
    }
}
