//! SQLite + FTS5 code symbol index.
//!
//! The storage engine is confined to this module: [`CodeIndex`] is the only
//! public entry point, and it never exposes the underlying `rusqlite`
//! connection — a caller has no way to open a `Connection` of its own,
//! which is what keeps persistence from leaking into a frontend (a frontend
//! that could name `rusqlite::Connection` could also open one directly).

use std::path::Path;

use rusqlite::{Connection, params};

use crate::extractor::Symbol;

// ── CodeIndex ─────────────────────────────────────────────────────────────────

/// A SQLite + FTS5-backed code symbol index. Owns its connection privately —
/// callers never see the storage engine, only this API.
pub struct CodeIndex {
    conn: Connection,
}

impl CodeIndex {
    /// Open a fresh in-memory index with the schema already migrated. Every
    /// caller gets a private, empty database — useful for tests, but never
    /// what a CLI/TUI invocation wants: an `index` in one process call and a
    /// `search` in the next would never see each other's data. Product
    /// code opens [`Self::open`] against a real path instead.
    pub fn open_in_memory() -> IndexResult<Self> {
        let conn = Connection::open_in_memory()?;
        migrate(&conn)?;
        Ok(Self { conn })
    }

    /// Open (creating if absent) a persistent, file-backed index at `path`
    /// with the schema migrated. The parent directory must already exist —
    /// callers create it themselves, the same convention the sibling memory
    /// store uses.
    pub fn open(path: &Path) -> IndexResult<Self> {
        let conn = Connection::open(path)?;
        migrate(&conn)?;
        Ok(Self { conn })
    }

    /// Insert a batch of symbols extracted from `file` into the index.
    pub fn index_symbols(&self, file: &str, symbols: &[Symbol]) -> IndexResult<usize> {
        store_symbols(&self.conn, file, symbols)
    }

    /// Retrieve a symbol by exact name.
    pub fn get_by_name(&self, name: &str) -> IndexResult<Option<IndexedSymbol>> {
        get_by_name(&self.conn, name)
    }

    /// FTS5 keyword search — returns up to `limit` symbols ranked by relevance.
    pub fn search(&self, query: &str, limit: usize) -> IndexResult<Vec<IndexedSymbol>> {
        fts_search(&self.conn, query, limit)
    }
}

// ── Schema ────────────────────────────────────────────────────────────────────

/// The schema version this build reads and writes, recorded in the database
/// header (`PRAGMA user_version`). A file with no version is a fresh index or
/// one from before versioning — both are the current schema, so it is stamped;
/// a higher version is refused rather than read optimistically.
const SCHEMA_VERSION: i64 = 1;

/// Create the `symbols` and `symbols_fts` tables and stamp the schema version.
fn migrate(conn: &Connection) -> IndexResult<()> {
    let found: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if found > SCHEMA_VERSION {
        return Err(IndexError::NewerSchema {
            found,
            supported: SCHEMA_VERSION,
        });
    }
    create_schema(conn)?;
    if found != SCHEMA_VERSION {
        conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION}"))?;
    }
    Ok(())
}

fn create_schema(conn: &Connection) -> IndexResult<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS symbols (
            id       INTEGER PRIMARY KEY AUTOINCREMENT,
            name     TEXT    NOT NULL,
            kind     TEXT    NOT NULL,
            file     TEXT    NOT NULL,
            line     INTEGER NOT NULL,
            doc      TEXT
        );
        CREATE VIRTUAL TABLE IF NOT EXISTS symbols_fts USING fts5(
            name,
            doc,
            content='symbols',
            content_rowid='id'
        );
        CREATE TRIGGER IF NOT EXISTS symbols_ai
            AFTER INSERT ON symbols BEGIN
                INSERT INTO symbols_fts(rowid, name, doc)
                VALUES (new.id, new.name, new.doc);
        END;
        CREATE TRIGGER IF NOT EXISTS symbols_ad
            AFTER DELETE ON symbols BEGIN
                INSERT INTO symbols_fts(symbols_fts, rowid, name, doc)
                VALUES ('delete', old.id, old.name, old.doc);
        END;
        ",
    )?;
    Ok(())
}

// ── IndexedSymbol ─────────────────────────────────────────────────────────────

/// A symbol retrieved from the index.
#[derive(Debug, Clone)]
pub struct IndexedSymbol {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub file: String,
    pub line: u32,
    pub doc: Option<String>,
}

// ── Store ─────────────────────────────────────────────────────────────────────

/// Insert a batch of symbols from `file` into the index.
fn store_symbols(conn: &Connection, file: &str, symbols: &[Symbol]) -> IndexResult<usize> {
    let mut count = 0;
    for sym in symbols {
        conn.execute(
            "INSERT INTO symbols (name, kind, file, line, doc) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![sym.name, format!("{:?}", sym.kind), file, sym.line, sym.doc],
        )?;
        count += 1;
    }
    Ok(count)
}

/// Retrieve a symbol by exact name.
fn get_by_name(conn: &Connection, name: &str) -> IndexResult<Option<IndexedSymbol>> {
    let mut stmt = conn
        .prepare("SELECT id, name, kind, file, line, doc FROM symbols WHERE name = ?1 LIMIT 1")?;
    let mut rows = stmt.query_map(params![name], map_row)?;
    Ok(rows.next().and_then(|r| r.ok()))
}

/// FTS5 keyword search — returns up to `limit` symbols ranked by relevance.
fn fts_search(conn: &Connection, query: &str, limit: usize) -> IndexResult<Vec<IndexedSymbol>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.name, s.kind, s.file, s.line, s.doc
         FROM symbols s
         JOIN symbols_fts f ON f.rowid = s.id
         WHERE symbols_fts MATCH ?1
         ORDER BY rank
         LIMIT ?2",
    )?;
    let rows: Vec<IndexedSymbol> = stmt
        .query_map(params![query, limit as i64], map_row)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

fn map_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<IndexedSymbol> {
    Ok(IndexedSymbol {
        id: r.get(0)?,
        name: r.get(1)?,
        kind: r.get(2)?,
        file: r.get(3)?,
        line: r.get::<_, u32>(4)?,
        doc: r.get(5)?,
    })
}

// ── Error ─────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum IndexError {
    Db(rusqlite::Error),
    /// The index file was written by a newer build. The index is a disposable
    /// cache, so the way out is to delete the file and re-index — never to
    /// read a shape this build does not know.
    NewerSchema {
        found: i64,
        supported: i64,
    },
}

impl std::fmt::Display for IndexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IndexError::Db(e) => write!(f, "codegraph index error: {e}"),
            IndexError::NewerSchema { found, supported } => write!(
                f,
                "the codegraph index has schema version {found}, newer than the {supported} \
                 this build understands — it was written by a newer version; update Cronus, \
                 or delete the index file and re-index"
            ),
        }
    }
}

impl std::error::Error for IndexError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            IndexError::Db(e) => Some(e),
            IndexError::NewerSchema { .. } => None,
        }
    }
}

impl From<rusqlite::Error> for IndexError {
    fn from(e: rusqlite::Error) -> Self {
        IndexError::Db(e)
    }
}

pub type IndexResult<T> = Result<T, IndexError>;
