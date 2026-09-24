//! Inter-actor inbox — SQLite-backed send/drain pipeline with GC and bus events.
//!
//! The `BusSender` stub implementations (`NoOpBusSender`, `CaptureBusSender`)
//! stay in the domain tier — they hold no infrastructure dependency;
//! only this SQLite-backed pipeline moved here.

use rusqlite::{Connection, params};

use cronus_contract::{BusEvent, BusSender};

// ── Constants ─────────────────────────────────────────────────────────────────

/// Milliseconds before an inbox row is garbage-collected (7 days).
pub const GC_TTL_MS: u64 = 7 * 24 * 60 * 60 * 1000;

/// Maximum messages returned per drain call.
pub const MAX_DRAIN_PER_TURN: usize = 100;

// ── Schema ────────────────────────────────────────────────────────────────────

/// The schema version this build reads and writes (`crate::versioning`).
pub const SCHEMA_VERSION: i64 = 1;

/// Bring the inbox database to the current schema: create the inbox and actors
/// tables when absent, stamp the schema version, and refuse a file written by
/// a newer build.
pub fn migrate(conn: &Connection) -> InboxResult<()> {
    crate::versioning::open_schema(conn, "inbox", SCHEMA_VERSION, create_schema, &[])
}

fn create_schema(conn: &Connection) -> InboxResult<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS actors (
            id TEXT PRIMARY KEY NOT NULL
        );
        CREATE TABLE IF NOT EXISTS inbox (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            sender_id   TEXT    NOT NULL,
            recipient_id TEXT   NOT NULL,
            body        TEXT    NOT NULL,
            created_at  INTEGER NOT NULL,
            expires_at  INTEGER NOT NULL,
            drained_at  INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_inbox_recipient
            ON inbox (recipient_id, drained_at, created_at);
        ",
    )?;
    Ok(())
}

// ── Actor registry ────────────────────────────────────────────────────────────

/// Register an actor so it can be a valid sender.
pub fn register_actor(conn: &Connection, id: &str) -> InboxResult<()> {
    conn.execute("INSERT OR IGNORE INTO actors (id) VALUES (?1)", params![id])?;
    Ok(())
}

/// Returns true when the actor is registered.
pub fn actor_exists(conn: &Connection, id: &str) -> InboxResult<bool> {
    let count: u32 = conn.query_row(
        "SELECT COUNT(1) FROM actors WHERE id = ?1",
        params![id],
        |r| r.get(0),
    )?;
    Ok(count > 0)
}

// ── Send ──────────────────────────────────────────────────────────────────────

/// An inbox message as received by the sender.
pub struct InboxMessage {
    pub sender_id: String,
    pub recipient_id: String,
    pub body: String,
}

/// Insert a message into the inbox.
///
/// Returns `Err(InboxError::SenderNotFound)` if `sender_id` is not registered.
pub fn send(
    conn: &Connection,
    msg: &InboxMessage,
    now_ms: u64,
    bus: &dyn BusSender,
) -> InboxResult<()> {
    if !actor_exists(conn, &msg.sender_id)? {
        return Err(InboxError::SenderNotFound(msg.sender_id.clone()));
    }
    let expires_at = now_ms + GC_TTL_MS;
    conn.execute(
        "INSERT INTO inbox (sender_id, recipient_id, body, created_at, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            msg.sender_id,
            msg.recipient_id,
            msg.body,
            now_ms as i64,
            expires_at as i64
        ],
    )?;

    // Count undrained messages for this recipient and fire the bus event.
    let count: u32 = conn.query_row(
        "SELECT COUNT(1) FROM inbox WHERE recipient_id = ?1 AND drained_at IS NULL",
        params![msg.recipient_id],
        |r| r.get(0),
    )?;
    bus.send(BusEvent::InboxArrived {
        recipient_id: msg.recipient_id.clone(),
        count,
    });
    Ok(())
}

// ── Drain ─────────────────────────────────────────────────────────────────────

/// A drained inbox row ready for injection as a session entry.
#[derive(Debug, Clone)]
pub struct DrainedMessage {
    pub id: i64,
    pub sender_id: String,
    pub body: String,
}

/// Pull up to `MAX_DRAIN_PER_TURN` undrained messages for `recipient_id`.
///
/// Marks each drained row with the current timestamp.
pub fn drain(
    conn: &Connection,
    recipient_id: &str,
    now_ms: u64,
) -> InboxResult<Vec<DrainedMessage>> {
    let mut stmt = conn.prepare(
        "SELECT id, sender_id, body
         FROM inbox
         WHERE recipient_id = ?1 AND drained_at IS NULL
         ORDER BY created_at
         LIMIT ?2",
    )?;

    let rows: Vec<DrainedMessage> = stmt
        .query_map(params![recipient_id, MAX_DRAIN_PER_TURN as i64], |r| {
            Ok(DrainedMessage {
                id: r.get(0)?,
                sender_id: r.get(1)?,
                body: r.get(2)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    if !rows.is_empty() {
        let ids: String = rows
            .iter()
            .map(|r| r.id.to_string())
            .collect::<Vec<_>>()
            .join(",");
        conn.execute_batch(&format!(
            "UPDATE inbox SET drained_at = {now_ms} WHERE id IN ({ids})"
        ))?;
    }

    Ok(rows)
}

/// Count undrained messages for `recipient_id` — used for the deferred-drain baseline.
pub fn undrained_count(conn: &Connection, recipient_id: &str) -> InboxResult<u32> {
    let count: u32 = conn.query_row(
        "SELECT COUNT(1) FROM inbox WHERE recipient_id = ?1 AND drained_at IS NULL",
        params![recipient_id],
        |r| r.get(0),
    )?;
    Ok(count)
}

// ── GC ────────────────────────────────────────────────────────────────────────

/// Delete rows where `expires_at < now_ms`. Returns the number removed.
pub fn gc(conn: &Connection, now_ms: u64) -> InboxResult<usize> {
    let n = conn.execute(
        "DELETE FROM inbox WHERE expires_at < ?1",
        params![now_ms as i64],
    )?;
    Ok(n)
}

// ── Error ─────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum InboxError {
    Db(rusqlite::Error),
    /// Sender actor ID is not in the actors registry (ESRCH).
    SenderNotFound(String),
    /// The database file's schema version could not be reconciled with the
    /// one this build understands (a newer file, or a failed migration).
    Schema(crate::versioning::SchemaError),
}

impl std::fmt::Display for InboxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InboxError::Db(e) => write!(f, "inbox DB error: {e}"),
            InboxError::Schema(e) => write!(f, "{e}"),
            InboxError::SenderNotFound(id) => {
                write!(f, "inbox send error: sender '{id}' not registered (ESRCH)")
            }
        }
    }
}

impl std::error::Error for InboxError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            InboxError::Db(e) => Some(e),
            InboxError::Schema(e) => Some(e),
            InboxError::SenderNotFound(_) => None,
        }
    }
}

impl From<rusqlite::Error> for InboxError {
    fn from(e: rusqlite::Error) -> Self {
        InboxError::Db(e)
    }
}

impl From<crate::versioning::SchemaError> for InboxError {
    fn from(e: crate::versioning::SchemaError) -> Self {
        InboxError::Schema(e)
    }
}

pub type InboxResult<T> = Result<T, InboxError>;
