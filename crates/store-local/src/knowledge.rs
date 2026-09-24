//! SQLite-backed knowledge store: named,
//! access-controlled document collections with hybrid semantic (sqlite-vec
//! ANN) + keyword (FTS5) retrieval. Rows are written only through this
//! module's write seam, which is where the authorship-zone and curation
//! lifecycle rules are enforced — never by caller convention.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Once;

use rusqlite::{Connection, OptionalExtension, params, params_from_iter};

use cronus_contract::{
    Chunk, Collection, Curation, Directory, Document, DocumentStatus, KnowledgeStore, Origin,
    RetrievedChunk, SourceRef, WriteOverride, now_secs,
};

/// The embedding vector dimension every `knowledge_chunk_vec` row must match.
/// One dimension system-wide: a model change
/// requires full re-indexing of the collection, never a mixed-dimension table.
pub const EMBEDDING_DIM: usize = 768;

#[derive(Debug)]
pub enum KnowledgeError {
    Database(rusqlite::Error),
    /// A stored row held data the type system rejects (unknown origin/
    /// curation/status, or malformed source_ref JSON) — a corrupt row.
    Corrupt(String),
    /// A write into an `Origin::Human` row with no override.
    ReadOnlyZone {
        document_id: String,
    },
    /// A curation advance to `Reviewed`/`Stable` with no human auth.
    HumanApprovalRequired {
        document_id: String,
        target: Curation,
    },
    /// An embedding vector whose length does not match [`EMBEDDING_DIM`].
    DimensionMismatch {
        expected: usize,
        actual: usize,
    },
    /// The database file's schema version could not be reconciled with the
    /// one this build understands (a newer file, or a failed migration).
    Schema(crate::versioning::SchemaError),
}

impl std::fmt::Display for KnowledgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KnowledgeError::Database(e) => write!(f, "knowledge database error: {e}"),
            KnowledgeError::Schema(e) => write!(f, "{e}"),
            KnowledgeError::Corrupt(m) => write!(f, "corrupt knowledge row: {m}"),
            KnowledgeError::ReadOnlyZone { document_id } => {
                write!(f, "read-only zone: human-authored document {document_id}")
            }
            KnowledgeError::HumanApprovalRequired {
                document_id,
                target,
            } => write!(
                f,
                "human approval required to advance document {document_id} to {}",
                target.as_str()
            ),
            KnowledgeError::DimensionMismatch { expected, actual } => write!(
                f,
                "embedding dimension mismatch: expected {expected}, got {actual}"
            ),
        }
    }
}

impl std::error::Error for KnowledgeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            KnowledgeError::Database(e) => Some(e),
            KnowledgeError::Schema(e) => Some(e),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for KnowledgeError {
    fn from(e: rusqlite::Error) -> Self {
        KnowledgeError::Database(e)
    }
}

impl From<crate::versioning::SchemaError> for KnowledgeError {
    fn from(e: crate::versioning::SchemaError) -> Self {
        KnowledgeError::Schema(e)
    }
}

pub type Result<T> = std::result::Result<T, KnowledgeError>;

static VEC_EXTENSION: Once = Once::new();

/// Register the sqlite-vec `vec0` module for every future `Connection` in
/// this process. `sqlite3_auto_extension` de-duplicates identical entry
/// points internally, but `Once` avoids the repeat FFI call. Must run before
/// the first `Connection::open` that needs `vec0`.
fn ensure_vec_extension() {
    VEC_EXTENSION.call_once(|| unsafe {
        #[allow(clippy::missing_transmute_annotations)]
        let rc = rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
            sqlite_vec::sqlite3_vec_init as *const (),
        )));
        debug_assert_eq!(rc, 0, "sqlite3_auto_extension registration failed");
    });
}

/// The knowledge store (`<state>/workspaces/<ws>/knowledge/knowledge.db`).
pub struct KnowledgeDb {
    conn: Connection,
}

impl KnowledgeDb {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        ensure_vec_extension();
        let conn = Connection::open(path)?;
        setup(&conn)?;
        Ok(KnowledgeDb { conn })
    }

    pub fn open_in_memory() -> Result<Self> {
        ensure_vec_extension();
        let conn = Connection::open_in_memory()?;
        setup(&conn)?;
        Ok(KnowledgeDb { conn })
    }

    // -- Collections / directories ----------------------------------------

    pub fn create_collection(&self, collection: &Collection) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO knowledge_collection
             (id, owner_id, name, description, meta, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                collection.id,
                collection.owner_id,
                collection.name,
                collection.description,
                collection.meta,
                collection.created_at as i64,
                collection.updated_at as i64,
            ],
        )?;
        Ok(())
    }

    pub fn get_collection(&self, id: &str) -> Result<Option<Collection>> {
        self.conn
            .query_row(
                "SELECT id, owner_id, name, description, meta, created_at, updated_at
                 FROM knowledge_collection WHERE id = ?1",
                params![id],
                |row| {
                    let created_at: i64 = row.get(5)?;
                    let updated_at: i64 = row.get(6)?;
                    Ok(Collection {
                        id: row.get(0)?,
                        owner_id: row.get(1)?,
                        name: row.get(2)?,
                        description: row.get(3)?,
                        meta: row.get(4)?,
                        created_at: created_at as u64,
                        updated_at: updated_at as u64,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn create_directory(&self, directory: &Directory) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO knowledge_directory
             (id, collection_id, parent_id, name) VALUES (?1, ?2, ?3, ?4)",
            params![
                directory.id,
                directory.collection_id,
                directory.parent_id,
                directory.name,
            ],
        )?;
        Ok(())
    }

    // -- Documents (write-gated) --------------------------------

    /// Authorship enforcement point: refuses to overwrite an existing
    /// `Origin::Human` row unless `override_` is `HumanDirected`. A brand-new
    /// document (no existing row by this id) is never gated here — the gate
    /// protects *rewriting* human material, not its initial ingest.
    pub fn write_document(&self, document: &Document, override_: &WriteOverride) -> Result<()> {
        if let Some(existing) = self.get_document(&document.id)? {
            let overridden = matches!(override_, WriteOverride::HumanDirected { .. });
            if existing.origin == Origin::Human && !overridden {
                return Err(KnowledgeError::ReadOnlyZone {
                    document_id: document.id.clone(),
                });
            }
        }
        self.conn.execute(
            "INSERT OR REPLACE INTO knowledge_document
             (id, collection_id, directory_id, source_file_id, source_url, name,
              status, origin, curation, error_msg, meta, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                document.id,
                document.collection_id,
                document.directory_id,
                document.source_file_id,
                document.source_url,
                document.name,
                document.status.as_str(),
                document.origin.as_str(),
                document.curation.map(Curation::as_str),
                document.error_msg,
                document.meta,
                document.created_at as i64,
                document.updated_at as i64,
            ],
        )?;
        Ok(())
    }

    /// Update only `status`/`error_msg` — never write-gated (index-state
    /// bookkeeping, not authored content). See the trait doc for why.
    pub fn update_document_status(
        &self,
        document_id: &str,
        status: DocumentStatus,
        error_msg: Option<&str>,
    ) -> Result<()> {
        let now = now_secs() as i64;
        let changed = self.conn.execute(
            "UPDATE knowledge_document SET status = ?1, error_msg = ?2, updated_at = ?3 WHERE id = ?4",
            params![status.as_str(), error_msg, now, document_id],
        )?;
        if changed == 0 {
            return Err(KnowledgeError::Corrupt(format!(
                "update_document_status: no such document {document_id}"
            )));
        }
        Ok(())
    }

    pub fn get_document(&self, id: &str) -> Result<Option<Document>> {
        self.conn
            .query_row(
                "SELECT id, collection_id, directory_id, source_file_id, source_url, name,
                        status, origin, curation, error_msg, meta, created_at, updated_at
                 FROM knowledge_document WHERE id = ?1",
                params![id],
                row_to_document,
            )
            .optional()?
            .transpose()
    }

    /// `Draft` is agent-free; `Reviewed`/`Stable` require `human_auth`.
    pub fn set_curation(&self, id: &str, next: Curation, human_auth: Option<&str>) -> Result<()> {
        if !matches!(next, Curation::Draft) && human_auth.is_none() {
            return Err(KnowledgeError::HumanApprovalRequired {
                document_id: id.to_string(),
                target: next,
            });
        }
        let now = now_secs() as i64;
        let changed = self.conn.execute(
            "UPDATE knowledge_document SET curation = ?1, updated_at = ?2 WHERE id = ?3",
            params![next.as_str(), now, id],
        )?;
        if changed == 0 {
            return Err(KnowledgeError::Corrupt(format!(
                "set_curation: no such document {id}"
            )));
        }
        Ok(())
    }

    /// Mark deleted — excluded from retrieval immediately.
    pub fn soft_delete_document(&self, id: &str) -> Result<()> {
        let now = now_secs() as i64;
        self.conn.execute(
            "UPDATE knowledge_document SET status = 'deleted', updated_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    }

    /// Physically remove documents soft-deleted more than
    /// `older_than_secs` ago, plus their chunk/FTS/vector rows.
    pub fn gc(&self, older_than_secs: u64) -> Result<u64> {
        let cutoff = now_secs().saturating_sub(older_than_secs) as i64;
        let tx = self.conn.unchecked_transaction()?;
        let doc_ids: Vec<String> = {
            let mut stmt = tx.prepare(
                "SELECT id FROM knowledge_document WHERE status = 'deleted' AND updated_at < ?1",
            )?;
            let rows = stmt.query_map(params![cutoff], |r| r.get::<_, String>(0))?;
            let mut ids = Vec::new();
            for row in rows {
                ids.push(row?);
            }
            ids
        };
        for id in &doc_ids {
            delete_chunks_on(&tx, id)?;
            tx.execute("DELETE FROM knowledge_document WHERE id = ?1", params![id])?;
        }
        tx.commit()?;
        Ok(doc_ids.len() as u64)
    }

    // -- Chunks (incremental re-index) --------------------------------

    /// Re-index precondition: delete every chunk (+ FTS + vector rows)
    /// for `document_id` before fresh ones are inserted.
    pub fn delete_chunks(&self, document_id: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        delete_chunks_on(&tx, document_id)?;
        tx.commit()?;
        Ok(())
    }

    /// Insert a chunk and its embedding, keeping the FTS and vector indices
    /// in sync with the chunk row, atomically.
    pub fn insert_chunk(&self, chunk: &Chunk, embedding: &[f32]) -> Result<()> {
        if embedding.len() != EMBEDDING_DIM {
            return Err(KnowledgeError::DimensionMismatch {
                expected: EMBEDDING_DIM,
                actual: embedding.len(),
            });
        }
        let tx = self.conn.unchecked_transaction()?;
        insert_chunk_on(&tx, chunk, embedding)?;
        tx.commit()?;
        Ok(())
    }

    /// Transactionally: replace every chunk for `document_id` with
    /// `chunks` as one all-or-nothing unit. Every embedding is validated
    /// *before* the transaction opens, so a malformed batch never touches the
    /// store at all — the prior chunks stay intact, never a half-deleted,
    /// half-inserted intermediate state.
    pub fn reindex_chunks(&self, document_id: &str, chunks: &[(Chunk, Vec<f32>)]) -> Result<()> {
        for (_, embedding) in chunks {
            if embedding.len() != EMBEDDING_DIM {
                return Err(KnowledgeError::DimensionMismatch {
                    expected: EMBEDDING_DIM,
                    actual: embedding.len(),
                });
            }
        }
        let tx = self.conn.unchecked_transaction()?;
        delete_chunks_on(&tx, document_id)?;
        for (chunk, embedding) in chunks {
            insert_chunk_on(&tx, chunk, embedding)?;
        }
        tx.commit()?;
        Ok(())
    }

    // -- Retrieval primitives (collection-scoped) --------------------------------

    /// Vector nearest-neighbour candidates among `ready`, non-deleted
    /// documents in `collection_ids`, ascending by distance (closest first).
    /// The `vec0` index is not collection-partitioned, so this over-fetches
    /// from `vec0` and filters by collection/status in a second step
    /// ("post-ANN filtering").
    pub fn ann_search(
        &self,
        collection_ids: &[String],
        query_vector: &[f32],
        top_k: usize,
    ) -> Result<Vec<(String, f32)>> {
        if collection_ids.is_empty() || top_k == 0 {
            return Ok(Vec::new());
        }
        if query_vector.len() != EMBEDDING_DIM {
            return Err(KnowledgeError::DimensionMismatch {
                expected: EMBEDDING_DIM,
                actual: query_vector.len(),
            });
        }
        let ready = self.ready_document_ids(collection_ids)?;
        if ready.is_empty() {
            return Ok(Vec::new());
        }
        let blob = serialize_f32_blob(query_vector);
        let over_fetch = (top_k.saturating_mul(4)).max(top_k + 20);
        let mut stmt = self.conn.prepare(
            "SELECT chunk_id, distance FROM knowledge_chunk_vec
             WHERE embedding MATCH ?1 AND k = ?2",
        )?;
        let candidates: Vec<(String, f32)> = stmt
            .query_map(params![blob, over_fetch as i64], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f32>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let owners = self.chunk_document_map(
            &candidates
                .iter()
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>(),
        )?;
        let mut out = Vec::with_capacity(top_k);
        for (chunk_id, distance) in candidates {
            if out.len() >= top_k {
                break;
            }
            if let Some(doc_id) = owners.get(&chunk_id)
                && ready.contains(doc_id)
            {
                out.push((chunk_id, distance));
            }
        }
        Ok(out)
    }

    /// FTS5 keyword candidates, same scope, best match first (ascending
    /// `rank` — SQLite FTS5's convention, more negative is a better match).
    pub fn fts_search(
        &self,
        collection_ids: &[String],
        query_text: &str,
        top_k: usize,
    ) -> Result<Vec<(String, f32)>> {
        if collection_ids.is_empty() || top_k == 0 {
            return Ok(Vec::new());
        }
        let ready = self.ready_document_ids(collection_ids)?;
        if ready.is_empty() {
            return Ok(Vec::new());
        }
        let over_fetch = (top_k.saturating_mul(4)).max(top_k + 20);
        let mut stmt = self.conn.prepare(
            "SELECT chunk_id, rank FROM knowledge_chunk_fts
             WHERE knowledge_chunk_fts MATCH ?1 ORDER BY rank LIMIT ?2",
        )?;
        let candidates: Vec<(String, f32)> = stmt
            .query_map(params![query_text, over_fetch as i64], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f32>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let owners = self.chunk_document_map(
            &candidates
                .iter()
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>(),
        )?;
        let mut out = Vec::with_capacity(top_k);
        for (chunk_id, rank) in candidates {
            if out.len() >= top_k {
                break;
            }
            if let Some(doc_id) = owners.get(&chunk_id)
                && ready.contains(doc_id)
            {
                out.push((chunk_id, rank));
            }
        }
        Ok(out)
    }

    /// Hydrate chunk ids into full [`RetrievedChunk`]s, applying the
    /// `min_curation` floor. `score` is left at `0.0` — the caller
    /// (domain-tier RRF fusion) sets it.
    pub fn hydrate_chunks(
        &self,
        chunk_ids: &[String],
        min_curation: Option<Curation>,
    ) -> Result<Vec<RetrievedChunk>> {
        if chunk_ids.is_empty() {
            return Ok(Vec::new());
        }
        let sql = format!(
            "SELECT c.id, c.document_id, c.text, c.source_ref, d.collection_id, d.origin, d.curation
             FROM knowledge_chunk c JOIN knowledge_document d ON d.id = c.document_id
             WHERE c.id IN ({}) AND d.status = 'ready'",
            placeholders(chunk_ids.len())
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(chunk_ids.iter()), |row| {
            let source_ref_json: Option<String> = row.get(3)?;
            let origin_str: String = row.get(5)?;
            let curation_str: Option<String> = row.get(6)?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                source_ref_json,
                row.get::<_, String>(4)?,
                origin_str,
                curation_str,
            ))
        })?;

        let mut out = Vec::new();
        for row in rows {
            let (
                chunk_id,
                document_id,
                text,
                source_ref_json,
                collection_id,
                origin_str,
                curation_str,
            ) = row?;
            let origin = Origin::from_db_str(&origin_str)
                .ok_or_else(|| KnowledgeError::Corrupt(format!("unknown origin: {origin_str}")))?;
            let curation = curation_str
                .map(|s| {
                    Curation::from_db_str(&s)
                        .ok_or_else(|| KnowledgeError::Corrupt(format!("unknown curation: {s}")))
                })
                .transpose()?;

            // Human-origin (no curation) is always eligible; otherwise
            // the stored curation must meet the requested floor.
            let eligible = match (origin, curation, min_curation) {
                (Origin::Human, _, _) => true,
                (_, None, _) => true,
                (_, Some(_), None) => true,
                (_, Some(have), Some(floor)) => have >= floor,
            };
            if !eligible {
                continue;
            }

            let source_ref = source_ref_json
                .map(|j| source_ref_from_json(&j))
                .transpose()?;
            out.push(RetrievedChunk {
                chunk_id,
                document_id,
                collection_id,
                text,
                source_ref,
                score: 0.0,
            });
        }
        Ok(out)
    }

    /// The set of `ready`, non-deleted document ids within `collection_ids`.
    fn ready_document_ids(&self, collection_ids: &[String]) -> Result<HashSet<String>> {
        let sql = format!(
            "SELECT id FROM knowledge_document WHERE status = 'ready' AND collection_id IN ({})",
            placeholders(collection_ids.len())
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(collection_ids.iter()), |r| {
            r.get::<_, String>(0)
        })?;
        let mut set = HashSet::new();
        for row in rows {
            set.insert(row?);
        }
        Ok(set)
    }

    /// chunk_id → document_id for the given chunk ids (membership lookup).
    fn chunk_document_map(&self, chunk_ids: &[String]) -> Result<HashMap<String, String>> {
        if chunk_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let sql = format!(
            "SELECT id, document_id FROM knowledge_chunk WHERE id IN ({})",
            placeholders(chunk_ids.len())
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(chunk_ids.iter()), |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut map = HashMap::new();
        for row in rows {
            let (chunk_id, document_id) = row?;
            map.insert(chunk_id, document_id);
        }
        Ok(map)
    }
}

impl KnowledgeStore for KnowledgeDb {
    fn create_collection(&self, collection: &Collection) -> std::result::Result<(), String> {
        KnowledgeDb::create_collection(self, collection).map_err(|e| e.to_string())
    }
    fn get_collection(&self, id: &str) -> std::result::Result<Option<Collection>, String> {
        KnowledgeDb::get_collection(self, id).map_err(|e| e.to_string())
    }
    fn create_directory(&self, directory: &Directory) -> std::result::Result<(), String> {
        KnowledgeDb::create_directory(self, directory).map_err(|e| e.to_string())
    }
    fn write_document(
        &self,
        document: &Document,
        override_: &WriteOverride,
    ) -> std::result::Result<(), String> {
        KnowledgeDb::write_document(self, document, override_).map_err(|e| e.to_string())
    }
    fn get_document(&self, id: &str) -> std::result::Result<Option<Document>, String> {
        KnowledgeDb::get_document(self, id).map_err(|e| e.to_string())
    }
    fn update_document_status(
        &self,
        document_id: &str,
        status: DocumentStatus,
        error_msg: Option<&str>,
    ) -> std::result::Result<(), String> {
        KnowledgeDb::update_document_status(self, document_id, status, error_msg)
            .map_err(|e| e.to_string())
    }
    fn set_curation(
        &self,
        id: &str,
        next: Curation,
        human_auth: Option<&str>,
    ) -> std::result::Result<(), String> {
        KnowledgeDb::set_curation(self, id, next, human_auth).map_err(|e| e.to_string())
    }
    fn soft_delete_document(&self, id: &str) -> std::result::Result<(), String> {
        KnowledgeDb::soft_delete_document(self, id).map_err(|e| e.to_string())
    }
    fn gc(&self, older_than_secs: u64) -> std::result::Result<u64, String> {
        KnowledgeDb::gc(self, older_than_secs).map_err(|e| e.to_string())
    }
    fn delete_chunks(&self, document_id: &str) -> std::result::Result<(), String> {
        KnowledgeDb::delete_chunks(self, document_id).map_err(|e| e.to_string())
    }
    fn insert_chunk(&self, chunk: &Chunk, embedding: &[f32]) -> std::result::Result<(), String> {
        KnowledgeDb::insert_chunk(self, chunk, embedding).map_err(|e| e.to_string())
    }
    fn reindex_chunks(
        &self,
        document_id: &str,
        chunks: &[(Chunk, Vec<f32>)],
    ) -> std::result::Result<(), String> {
        KnowledgeDb::reindex_chunks(self, document_id, chunks).map_err(|e| e.to_string())
    }
    fn ann_search(
        &self,
        collection_ids: &[String],
        query_vector: &[f32],
        top_k: usize,
    ) -> std::result::Result<Vec<(String, f32)>, String> {
        KnowledgeDb::ann_search(self, collection_ids, query_vector, top_k)
            .map_err(|e| e.to_string())
    }
    fn fts_search(
        &self,
        collection_ids: &[String],
        query_text: &str,
        top_k: usize,
    ) -> std::result::Result<Vec<(String, f32)>, String> {
        KnowledgeDb::fts_search(self, collection_ids, query_text, top_k).map_err(|e| e.to_string())
    }
    fn hydrate_chunks(
        &self,
        chunk_ids: &[String],
        min_curation: Option<Curation>,
    ) -> std::result::Result<Vec<RetrievedChunk>, String> {
        KnowledgeDb::hydrate_chunks(self, chunk_ids, min_curation).map_err(|e| e.to_string())
    }
}

fn placeholders(n: usize) -> String {
    (1..=n)
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn serialize_f32_blob(v: &[f32]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(v.len() * 4);
    for x in v {
        buf.extend_from_slice(&x.to_ne_bytes());
    }
    buf
}

fn delete_chunks_on(conn: &Connection, document_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM knowledge_chunk_fts WHERE chunk_id IN
         (SELECT id FROM knowledge_chunk WHERE document_id = ?1)",
        params![document_id],
    )?;
    conn.execute(
        "DELETE FROM knowledge_chunk_vec WHERE chunk_id IN
         (SELECT id FROM knowledge_chunk WHERE document_id = ?1)",
        params![document_id],
    )?;
    conn.execute(
        "DELETE FROM knowledge_chunk WHERE document_id = ?1",
        params![document_id],
    )?;
    Ok(())
}

fn insert_chunk_on(conn: &Connection, chunk: &Chunk, embedding: &[f32]) -> Result<()> {
    let source_ref_json = chunk.source_ref.as_ref().map(source_ref_to_json);
    conn.execute(
        "INSERT OR REPLACE INTO knowledge_chunk
         (id, document_id, text, position, source_ref, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            chunk.id,
            chunk.document_id,
            chunk.text,
            chunk.position,
            source_ref_json,
            chunk.created_at as i64,
        ],
    )?;
    conn.execute(
        "DELETE FROM knowledge_chunk_fts WHERE chunk_id = ?1",
        params![chunk.id],
    )?;
    conn.execute(
        "INSERT INTO knowledge_chunk_fts (chunk_id, text) VALUES (?1, ?2)",
        params![chunk.id, chunk.text],
    )?;
    conn.execute(
        "DELETE FROM knowledge_chunk_vec WHERE chunk_id = ?1",
        params![chunk.id],
    )?;
    conn.execute(
        "INSERT INTO knowledge_chunk_vec (chunk_id, embedding) VALUES (?1, ?2)",
        params![chunk.id, serialize_f32_blob(embedding)],
    )?;
    Ok(())
}

fn row_to_document(row: &rusqlite::Row<'_>) -> rusqlite::Result<Result<Document>> {
    let status_str: String = row.get(6)?;
    let origin_str: String = row.get(7)?;
    let curation_str: Option<String> = row.get(8)?;
    let created_at: i64 = row.get(11)?;
    let updated_at: i64 = row.get(12)?;

    Ok((|| {
        let status = DocumentStatus::from_db_str(&status_str)
            .ok_or_else(|| KnowledgeError::Corrupt(format!("unknown status: {status_str}")))?;
        let origin = Origin::from_db_str(&origin_str)
            .ok_or_else(|| KnowledgeError::Corrupt(format!("unknown origin: {origin_str}")))?;
        let curation = curation_str
            .map(|s| {
                Curation::from_db_str(&s)
                    .ok_or_else(|| KnowledgeError::Corrupt(format!("unknown curation: {s}")))
            })
            .transpose()?;
        Ok(Document {
            id: row.get(0)?,
            collection_id: row.get(1)?,
            directory_id: row.get(2)?,
            source_file_id: row.get(3)?,
            source_url: row.get(4)?,
            name: row.get(5)?,
            status,
            origin,
            curation,
            error_msg: row.get(9)?,
            meta: row.get(10)?,
            created_at: created_at as u64,
            updated_at: updated_at as u64,
        })
    })())
}

fn source_ref_to_json(s: &SourceRef) -> String {
    serde_json::json!({
        "page": s.page,
        "section": s.section,
        "byte_start": s.byte_start,
        "byte_end": s.byte_end,
    })
    .to_string()
}

fn source_ref_from_json(s: &str) -> Result<SourceRef> {
    let value: serde_json::Value =
        serde_json::from_str(s).map_err(|e| KnowledgeError::Corrupt(e.to_string()))?;
    Ok(SourceRef {
        page: value.get("page").and_then(|v| v.as_u64()).map(|n| n as u32),
        section: value
            .get("section")
            .and_then(|v| v.as_str())
            .map(String::from),
        byte_start: value.get("byte_start").and_then(|v| v.as_u64()),
        byte_end: value.get("byte_end").and_then(|v| v.as_u64()),
    })
}

/// The schema version this build reads and writes (`crate::versioning`).
pub(crate) const SCHEMA_VERSION: i64 = 1;

pub(crate) fn setup(conn: &Connection) -> Result<()> {
    conn.execute_batch("PRAGMA journal_mode = WAL")?;
    crate::versioning::open_schema(conn, "knowledge", SCHEMA_VERSION, create_schema, &[])
}

fn create_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS knowledge_collection (
            id          TEXT PRIMARY KEY NOT NULL,
            owner_id    TEXT NOT NULL,
            name        TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            meta        TEXT,
            created_at  INTEGER NOT NULL,
            updated_at  INTEGER NOT NULL
        )",
    )?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS knowledge_directory (
            id            TEXT PRIMARY KEY NOT NULL,
            collection_id TEXT NOT NULL REFERENCES knowledge_collection(id),
            parent_id     TEXT REFERENCES knowledge_directory(id),
            name          TEXT NOT NULL
        )",
    )?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS ix_kdir_collection ON knowledge_directory(collection_id)",
    )?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS knowledge_document (
            id             TEXT PRIMARY KEY NOT NULL,
            collection_id  TEXT NOT NULL REFERENCES knowledge_collection(id),
            directory_id   TEXT REFERENCES knowledge_directory(id),
            source_file_id TEXT,
            source_url     TEXT,
            name           TEXT NOT NULL,
            status         TEXT NOT NULL DEFAULT 'pending',
            origin         TEXT NOT NULL DEFAULT 'agent',
            curation       TEXT,
            error_msg      TEXT,
            meta           TEXT,
            created_at     INTEGER NOT NULL,
            updated_at     INTEGER NOT NULL
        )",
    )?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS ix_kdoc_collection ON knowledge_document(collection_id)",
    )?;
    conn.execute_batch("CREATE INDEX IF NOT EXISTS ix_kdoc_status ON knowledge_document(status)")?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS ix_kdoc_curation ON knowledge_document(collection_id, curation)",
    )?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS knowledge_chunk (
            id          TEXT PRIMARY KEY NOT NULL,
            document_id TEXT NOT NULL REFERENCES knowledge_document(id),
            text        TEXT NOT NULL,
            position    INTEGER NOT NULL,
            source_ref  TEXT,
            created_at  INTEGER NOT NULL
        )",
    )?;
    conn.execute_batch("CREATE INDEX IF NOT EXISTS ix_kchunk_doc ON knowledge_chunk(document_id)")?;
    // Standalone FTS5 (synced manually in insert_chunk_on), matching the
    // memory/wiki store's proven pattern rather than an external-content table.
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_chunk_fts USING fts5(
            chunk_id UNINDEXED,
            text
        )",
    )?;
    // sqlite-vec ANN index. TEXT primary key supported by this vec0 build
    // (pkIsText); the KNN query returns chunk_id directly, no rowid join.
    conn.execute_batch(&format!(
        "CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_chunk_vec USING vec0(
            chunk_id TEXT PRIMARY KEY,
            embedding FLOAT[{EMBEDDING_DIM}]
        )"
    ))?;
    Ok(())
}

#[cfg(test)]
mod schema {
    use super::*;

    fn names(conn: &Connection, kind: &str) -> Vec<String> {
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = ?1 ORDER BY name")
            .unwrap();
        stmt.query_map(params![kind], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    fn fake_embedding(seed: f32) -> Vec<f32> {
        let mut v = vec![0.0f32; EMBEDDING_DIM];
        v[0] = seed;
        v[1] = 1.0 - seed;
        v
    }

    #[test]
    fn tables_and_indices_created() {
        let db = KnowledgeDb::open_in_memory().expect("open");
        let tables = names(&db.conn, "table");
        for t in [
            "knowledge_collection",
            "knowledge_directory",
            "knowledge_document",
            "knowledge_chunk",
            "knowledge_chunk_fts",
            "knowledge_chunk_vec",
        ] {
            assert!(tables.contains(&t.to_string()), "missing table {t}");
        }
        let indices = names(&db.conn, "index");
        for i in ["ix_kdoc_collection", "ix_kdoc_status", "ix_kdoc_curation"] {
            assert!(indices.contains(&i.to_string()), "missing index {i}");
        }
    }

    #[test]
    fn sqlite_vec_extension_is_registered() {
        let db = KnowledgeDb::open_in_memory().expect("open");
        let version: String = db
            .conn
            .query_row("SELECT vec_version()", [], |r| r.get(0))
            .expect("vec0 module must be registered");
        assert!(
            version.starts_with('v'),
            "unexpected vec_version(): {version}"
        );
    }

    #[test]
    fn a_collection_round_trips() {
        let db = KnowledgeDb::open_in_memory().expect("open");
        let c = Collection::new("col-1", "user-1", "Docs");
        db.create_collection(&c).expect("create");
        let got = db.get_collection("col-1").expect("get").expect("present");
        assert_eq!(got, c);
        assert!(db.get_collection("nope").expect("get missing").is_none());
    }

    #[test]
    fn a_document_round_trips_with_origin_and_curation() {
        let db = KnowledgeDb::open_in_memory().expect("open");
        db.create_collection(&Collection::new("col-1", "user-1", "Docs"))
            .unwrap();
        let mut doc = Document::new_agent("doc-1", "col-1", "notes.md");
        doc.status = DocumentStatus::Ready;
        db.write_document(&doc, &WriteOverride::None)
            .expect("write");

        let got = db.get_document("doc-1").expect("get").expect("present");
        assert_eq!(got, doc);
        assert_eq!(got.origin, Origin::Agent);
        assert_eq!(got.curation, Some(Curation::Draft));
    }

    #[test]
    fn a_human_zone_write_without_override_is_refused() {
        let db = KnowledgeDb::open_in_memory().expect("open");
        db.create_collection(&Collection::new("col-1", "user-1", "Docs"))
            .unwrap();
        let original = Document::new_human("doc-1", "col-1", "contract.pdf");
        db.write_document(&original, &WriteOverride::None)
            .expect("initial human ingest is not gated");

        let mut rewrite = original.clone();
        rewrite.name = "tampered.pdf".to_string();
        let err = db
            .write_document(&rewrite, &WriteOverride::None)
            .expect_err("a rewrite of a human-origin row must be refused without override");
        assert!(matches!(err, KnowledgeError::ReadOnlyZone { .. }));

        // The row is untouched.
        let got = db.get_document("doc-1").unwrap().unwrap();
        assert_eq!(got.name, "contract.pdf");
    }

    #[test]
    fn a_human_zone_write_with_override_succeeds_and_is_attributable() {
        let db = KnowledgeDb::open_in_memory().expect("open");
        db.create_collection(&Collection::new("col-1", "user-1", "Docs"))
            .unwrap();
        let original = Document::new_human("doc-1", "col-1", "contract.pdf");
        db.write_document(&original, &WriteOverride::None).unwrap();

        let mut rewrite = original.clone();
        rewrite.name = "corrected.pdf".to_string();
        db.write_document(
            &rewrite,
            &WriteOverride::HumanDirected {
                audit_ref: "audit-42".to_string(),
            },
        )
        .expect("an audited override may rewrite a human-origin row");

        let got = db.get_document("doc-1").unwrap().unwrap();
        assert_eq!(got.name, "corrected.pdf");
    }

    #[test]
    fn curation_advance_requires_human_auth_except_draft() {
        let db = KnowledgeDb::open_in_memory().expect("open");
        db.create_collection(&Collection::new("col-1", "user-1", "Docs"))
            .unwrap();
        db.write_document(
            &Document::new_agent("doc-1", "col-1", "notes.md"),
            &WriteOverride::None,
        )
        .unwrap();

        // Draft is agent-free — the document already starts there, but
        // re-asserting Draft with no auth must still succeed.
        db.set_curation("doc-1", Curation::Draft, None)
            .expect("draft needs no human auth");

        let err = db
            .set_curation("doc-1", Curation::Reviewed, None)
            .expect_err("advancing to Reviewed without auth must be refused");
        assert!(matches!(err, KnowledgeError::HumanApprovalRequired { .. }));
        assert_eq!(
            db.get_document("doc-1").unwrap().unwrap().curation,
            Some(Curation::Draft),
            "a refused transition must not have mutated the row"
        );

        db.set_curation("doc-1", Curation::Reviewed, Some("reviewer-1"))
            .expect("advancing with human auth succeeds");
        assert_eq!(
            db.get_document("doc-1").unwrap().unwrap().curation,
            Some(Curation::Reviewed)
        );
    }

    #[test]
    fn delete_chunks_removes_chunk_fts_and_vec_rows() {
        let db = KnowledgeDb::open_in_memory().expect("open");
        db.create_collection(&Collection::new("col-1", "user-1", "Docs"))
            .unwrap();
        let mut doc = Document::new_agent("doc-1", "col-1", "notes.md");
        doc.status = DocumentStatus::Ready;
        db.write_document(&doc, &WriteOverride::None).unwrap();

        let chunk = Chunk {
            id: "chk-1".into(),
            document_id: "doc-1".into(),
            text: "hello world".into(),
            position: 0,
            source_ref: None,
            created_at: now_secs(),
        };
        db.insert_chunk(&chunk, &fake_embedding(0.1)).unwrap();

        let fts_count: i64 = db
            .conn
            .query_row(
                "SELECT count(*) FROM knowledge_chunk_fts WHERE chunk_id = 'chk-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fts_count, 1);

        db.delete_chunks("doc-1").expect("delete");

        let chunk_count: i64 = db
            .conn
            .query_row("SELECT count(*) FROM knowledge_chunk", [], |r| r.get(0))
            .unwrap();
        assert_eq!(chunk_count, 0, "chunk row removed");
        let fts_count: i64 = db
            .conn
            .query_row("SELECT count(*) FROM knowledge_chunk_fts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fts_count, 0, "fts row removed");
        let vec_count: i64 = db
            .conn
            .query_row("SELECT count(*) FROM knowledge_chunk_vec", [], |r| r.get(0))
            .unwrap();
        assert_eq!(vec_count, 0, "vec row removed");
    }

    fn seed_ready_doc_with_chunk(
        db: &KnowledgeDb,
        collection_id: &str,
        doc_id: &str,
        chunk_id: &str,
        text: &str,
        seed: f32,
    ) {
        let mut doc = Document::new_agent(doc_id, collection_id, format!("{doc_id}.md"));
        doc.status = DocumentStatus::Ready;
        db.write_document(&doc, &WriteOverride::None).unwrap();
        let chunk = Chunk {
            id: chunk_id.to_string(),
            document_id: doc_id.to_string(),
            text: text.to_string(),
            position: 0,
            source_ref: Some(SourceRef {
                page: Some(1),
                ..Default::default()
            }),
            created_at: now_secs(),
        };
        db.insert_chunk(&chunk, &fake_embedding(seed)).unwrap();
    }

    #[test]
    fn update_document_status_is_never_kb9_gated_even_for_a_human_origin_row() {
        let db = KnowledgeDb::open_in_memory().expect("open");
        db.create_collection(&Collection::new("col-1", "user-1", "A"))
            .unwrap();
        let human_doc = Document::new_human("doc-1", "col-1", "contract.pdf");
        db.write_document(&human_doc, &WriteOverride::None)
            .expect("initial human ingest is not gated");

        // A pure index-state transition on a human-origin row, with NO
        // override supplied — must succeed, unlike a `write_document` rewrite
        // of the same row (which IS gated). Proves the write-gate/index-state
        // separation directly at the store tier.
        db.update_document_status("doc-1", DocumentStatus::Ready, None)
            .expect("status-only updates are never write-gated");

        let got = db.get_document("doc-1").unwrap().unwrap();
        assert_eq!(got.status, DocumentStatus::Ready);
        assert_eq!(got.origin, Origin::Human, "content fields are untouched");
        assert_eq!(got.name, "contract.pdf", "content fields are untouched");
    }

    #[test]
    fn reindex_chunks_is_all_or_nothing_on_a_malformed_batch() {
        let db = KnowledgeDb::open_in_memory().expect("open");
        db.create_collection(&Collection::new("col-1", "user-1", "A"))
            .unwrap();
        let mut doc = Document::new_agent("doc-1", "col-1", "notes.md");
        doc.status = DocumentStatus::Ready;
        db.write_document(&doc, &WriteOverride::None).unwrap();

        // Seed one prior chunk via the atomic path itself.
        let seed = Chunk {
            id: "chk-old".into(),
            document_id: "doc-1".into(),
            text: "original".into(),
            position: 0,
            source_ref: None,
            created_at: now_secs(),
        };
        db.reindex_chunks("doc-1", &[(seed.clone(), fake_embedding(0.2))])
            .expect("seed reindex");

        // A batch where one embedding has the wrong dimension must fail
        // WITHOUT touching the store at all — the prior chunk survives.
        let good = Chunk {
            id: "chk-new-1".into(),
            document_id: "doc-1".into(),
            text: "new one".into(),
            position: 0,
            source_ref: None,
            created_at: now_secs(),
        };
        let bad_embedding = vec![0.0f32; 3]; // wrong dimension
        let err = db
            .reindex_chunks(
                "doc-1",
                &[(good, fake_embedding(0.3)), (seed.clone(), bad_embedding)],
            )
            .expect_err("a malformed batch must be refused");
        assert!(matches!(err, KnowledgeError::DimensionMismatch { .. }));

        // The prior chunk is untouched; no partial new chunk was inserted.
        let chunks: Vec<String> = db
            .conn
            .prepare("SELECT id FROM knowledge_chunk ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(
            chunks,
            vec!["chk-old".to_string()],
            "a rejected batch must leave the prior chunk set exactly as it was"
        );
    }

    #[test]
    fn reindex_chunks_replaces_the_full_prior_set() {
        let db = KnowledgeDb::open_in_memory().expect("open");
        db.create_collection(&Collection::new("col-1", "user-1", "A"))
            .unwrap();
        let mut doc = Document::new_agent("doc-1", "col-1", "notes.md");
        doc.status = DocumentStatus::Ready;
        db.write_document(&doc, &WriteOverride::None).unwrap();

        let old = Chunk {
            id: "chk-old".into(),
            document_id: "doc-1".into(),
            text: "stale content".into(),
            position: 0,
            source_ref: None,
            created_at: now_secs(),
        };
        db.reindex_chunks("doc-1", &[(old, fake_embedding(0.1))])
            .unwrap();

        let fresh = Chunk {
            id: "chk-fresh".into(),
            document_id: "doc-1".into(),
            text: "fresh content".into(),
            position: 0,
            source_ref: None,
            created_at: now_secs(),
        };
        db.reindex_chunks("doc-1", &[(fresh, fake_embedding(0.4))])
            .expect("second reindex");

        let chunks: Vec<String> = db
            .conn
            .prepare("SELECT id FROM knowledge_chunk ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(
            chunks,
            vec!["chk-fresh".to_string()],
            "the second reindex must fully replace the first, no duplication"
        );
    }

    #[test]
    fn ann_search_never_returns_another_collections_chunk() {
        let db = KnowledgeDb::open_in_memory().expect("open");
        db.create_collection(&Collection::new("col-1", "user-1", "A"))
            .unwrap();
        db.create_collection(&Collection::new("col-2", "user-1", "B"))
            .unwrap();
        seed_ready_doc_with_chunk(&db, "col-1", "doc-1", "chk-1", "alpha", 0.9);
        seed_ready_doc_with_chunk(&db, "col-2", "doc-2", "chk-2", "beta", 0.9);

        let query = fake_embedding(0.9);
        let hits = db
            .ann_search(&["col-1".to_string()], &query, 10)
            .expect("ann_search");
        let ids: Vec<&str> = hits.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, vec!["chk-1"], "col-2's chunk must never appear");
    }

    #[test]
    fn ann_search_finds_the_nearest_neighbour() {
        let db = KnowledgeDb::open_in_memory().expect("open");
        db.create_collection(&Collection::new("col-1", "user-1", "A"))
            .unwrap();
        seed_ready_doc_with_chunk(&db, "col-1", "doc-near", "chk-near", "close match", 0.5);
        seed_ready_doc_with_chunk(&db, "col-1", "doc-far", "chk-far", "distant match", 0.99);

        let query = fake_embedding(0.5);
        let hits = db
            .ann_search(&["col-1".to_string()], &query, 1)
            .expect("ann_search");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, "chk-near", "the closer embedding wins");
    }

    #[test]
    fn fts_search_never_returns_another_collections_chunk() {
        let db = KnowledgeDb::open_in_memory().expect("open");
        db.create_collection(&Collection::new("col-1", "user-1", "A"))
            .unwrap();
        db.create_collection(&Collection::new("col-2", "user-1", "B"))
            .unwrap();
        seed_ready_doc_with_chunk(&db, "col-1", "doc-1", "chk-1", "refund policy details", 0.1);
        seed_ready_doc_with_chunk(
            &db,
            "col-2",
            "doc-2",
            "chk-2",
            "refund policy elsewhere",
            0.2,
        );

        let hits = db
            .fts_search(&["col-1".to_string()], "refund", 10)
            .expect("fts_search");
        let ids: Vec<&str> = hits.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, vec!["chk-1"]);
    }

    #[test]
    fn soft_deleted_documents_are_excluded_from_retrieval() {
        let db = KnowledgeDb::open_in_memory().expect("open");
        db.create_collection(&Collection::new("col-1", "user-1", "A"))
            .unwrap();
        seed_ready_doc_with_chunk(&db, "col-1", "doc-1", "chk-1", "term-unique-xyz", 0.7);
        db.soft_delete_document("doc-1").expect("soft delete");

        assert!(
            db.fts_search(&["col-1".to_string()], "unique", 10)
                .unwrap()
                .is_empty(),
            "a soft-deleted document's chunks must not surface in retrieval"
        );
        let query = fake_embedding(0.7);
        assert!(
            db.ann_search(&["col-1".to_string()], &query, 10)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn gc_removes_documents_past_the_retention_window() {
        let db = KnowledgeDb::open_in_memory().expect("open");
        db.create_collection(&Collection::new("col-1", "user-1", "A"))
            .unwrap();
        seed_ready_doc_with_chunk(&db, "col-1", "doc-1", "chk-1", "gc me", 0.3);
        db.soft_delete_document("doc-1").unwrap();

        // Not yet past the retention window.
        let removed = db.gc(3600).expect("gc");
        assert_eq!(removed, 0);
        assert!(db.get_document("doc-1").unwrap().is_some());

        // Past the window (older_than_secs = 0 → cutoff = now).
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let removed = db.gc(0).expect("gc");
        assert_eq!(removed, 1);
        assert!(db.get_document("doc-1").unwrap().is_none());
        let chunk_count: i64 = db
            .conn
            .query_row("SELECT count(*) FROM knowledge_chunk", [], |r| r.get(0))
            .unwrap();
        assert_eq!(chunk_count, 0, "the document's chunks were GC'd too");
    }

    #[test]
    fn min_curation_excludes_draft_but_keeps_human_sources_eligible() {
        let db = KnowledgeDb::open_in_memory().expect("open");
        db.create_collection(&Collection::new("col-1", "user-1", "A"))
            .unwrap();

        // An agent doc left at draft.
        seed_ready_doc_with_chunk(&db, "col-1", "doc-draft", "chk-draft", "draft text", 0.4);

        // A human-authored doc (no curation) — always eligible regardless of floor.
        let mut human = Document::new_human("doc-human", "col-1", "manual.pdf");
        human.status = DocumentStatus::Ready;
        db.write_document(&human, &WriteOverride::None).unwrap();
        db.insert_chunk(
            &Chunk {
                id: "chk-human".into(),
                document_id: "doc-human".into(),
                text: "human text".into(),
                position: 0,
                source_ref: None,
                created_at: now_secs(),
            },
            &fake_embedding(0.6),
        )
        .unwrap();

        let hydrated = db
            .hydrate_chunks(
                &["chk-draft".to_string(), "chk-human".to_string()],
                Some(Curation::Reviewed),
            )
            .expect("hydrate");
        let ids: Vec<&str> = hydrated.iter().map(|c| c.chunk_id.as_str()).collect();
        assert!(!ids.contains(&"chk-draft"), "draft is below the floor");
        assert!(
            ids.contains(&"chk-human"),
            "human-origin sources stay eligible regardless of the floor"
        );
    }

    #[test]
    fn hydrate_chunks_carries_source_ref_attribution() {
        let db = KnowledgeDb::open_in_memory().expect("open");
        db.create_collection(&Collection::new("col-1", "user-1", "A"))
            .unwrap();
        seed_ready_doc_with_chunk(&db, "col-1", "doc-1", "chk-1", "attributed text", 0.2);

        let hydrated = db
            .hydrate_chunks(&["chk-1".to_string()], None)
            .expect("hydrate");
        assert_eq!(hydrated.len(), 1);
        assert_eq!(hydrated[0].collection_id, "col-1");
        assert_eq!(hydrated[0].source_ref.as_ref().unwrap().page, Some(1));
    }
}
