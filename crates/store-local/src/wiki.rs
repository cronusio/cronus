//! SQLite-backed project-wiki store — a derived projection CACHE.
//! Rows are written only by the office regeneration
//! pipeline (later phase tasks); this module owns the schema and the
//! persistence round-trip. Per the store is a rebuildable cache, never a
//! source of truth — dropping `wiki.db` loses nothing durable.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use cronus_contract::{
    WikiCache, WikiChangelogEntry, WikiCitation, WikiPage, WikiPageKind, WikiReadSurface,
};

#[derive(Debug)]
pub enum WikiError {
    Database(rusqlite::Error),
    /// A stored row held data the type system rejects (an unknown page kind or
    /// malformed citations JSON) — a corrupt/incompatible cache row. Because
    /// the wiki is a rebuildable projection, the recovery for this is a
    /// rebuild, not a restore.
    Corrupt(String),
    /// The cache file's schema version could not be reconciled with the one
    /// this build understands. The cache is droppable, so a file this
    /// build refuses can be deleted and rebuilt rather than migrated.
    Schema(crate::versioning::SchemaError),
}

impl std::fmt::Display for WikiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WikiError::Database(e) => write!(f, "wiki database error: {e}"),
            WikiError::Corrupt(m) => write!(f, "corrupt wiki row: {m}"),
            WikiError::Schema(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for WikiError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            WikiError::Database(e) => Some(e),
            WikiError::Corrupt(_) => None,
            WikiError::Schema(e) => Some(e),
        }
    }
}

impl From<rusqlite::Error> for WikiError {
    fn from(e: rusqlite::Error) -> Self {
        WikiError::Database(e)
    }
}

impl From<crate::versioning::SchemaError> for WikiError {
    fn from(e: crate::versioning::SchemaError) -> Self {
        WikiError::Schema(e)
    }
}

pub type Result<T> = std::result::Result<T, WikiError>;

/// The per-office wiki cache (`<state>/workspaces/<ws>/wiki/wiki.db`).
pub struct WikiStore {
    conn: Connection,
}

impl WikiStore {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        setup(&conn)?;
        Ok(WikiStore { conn })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        setup(&conn)?;
        Ok(WikiStore { conn })
    }

    /// Write a page (insert or replace by id) and keep the FTS index in sync.
    /// The client never calls this — only the office regeneration pipeline;
    /// it lives here as the store's write half for that pipeline.
    pub fn upsert_page(&self, page: &WikiPage) -> Result<()> {
        upsert_page_on(&self.conn, page)
    }

    /// Read a page by id, or `None` if absent.
    pub fn get_page(&self, id: &str) -> Result<Option<WikiPage>> {
        self.conn
            .query_row(
                "SELECT id, office_id, parent_id, ord, kind, title, body,
                        citations, source_fingerprint, generated_at, stale
                 FROM wiki_page WHERE id = ?1",
                params![id],
                row_to_page,
            )
            .optional()?
            .transpose()
    }

    /// Append a change-history entry (newest-first by `at`).
    pub fn append_changelog(&self, entry: &WikiChangelogEntry) -> Result<()> {
        append_changelog_on(&self.conn, entry)
    }

    /// Apply a regeneration transactionally: upsert every page and
    /// append every changelog entry inside one SQLite transaction, so a
    /// failure part-way rolls the whole batch back and the prior rows stay
    /// intact. `unchecked_transaction` is used because the store holds the
    /// connection behind `&self` (no `&mut` handle) — the batch is the only
    /// writer, so there is no reentrancy to guard against.
    pub fn apply_regeneration(
        &self,
        pages: &[WikiPage],
        changelog: &[WikiChangelogEntry],
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for page in pages {
            upsert_page_on(&tx, page)?;
        }
        for entry in changelog {
            append_changelog_on(&tx, entry)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Rebuild an office's wiki: transactionally drop every existing
    /// page + changelog + FTS row for `office_id`, then write the freshly
    /// re-derived set. All-or-nothing — a failure part-way rolls back and the
    /// prior rows stay intact, so a failed rebuild never leaves a half-cleared
    /// projection. This is the store half of the "drop and re-derive from
    /// ground truth" proof: nothing authoritative ever lived only here, so
    /// dropping `wiki.db` and rebuilding loses nothing durable.
    pub fn rebuild_office(
        &self,
        office_id: &str,
        pages: &[WikiPage],
        changelog: &[WikiChangelogEntry],
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        // Drop the FTS rows first (keyed by page_id) while the parent rows still
        // resolve which pages belong to the office, then the parent rows.
        tx.execute(
            "DELETE FROM wiki_page_fts
             WHERE page_id IN (SELECT id FROM wiki_page WHERE office_id = ?1)",
            params![office_id],
        )?;
        tx.execute(
            "DELETE FROM wiki_page WHERE office_id = ?1",
            params![office_id],
        )?;
        tx.execute(
            "DELETE FROM wiki_changelog WHERE office_id = ?1",
            params![office_id],
        )?;
        for page in pages {
            upsert_page_on(&tx, page)?;
        }
        for entry in changelog {
            append_changelog_on(&tx, entry)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Every page for an office (the freshness-sweep input).
    pub fn pages_for_office(&self, office_id: &str) -> Result<Vec<WikiPage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, office_id, parent_id, ord, kind, title, body,
                    citations, source_fingerprint, generated_at, stale
             FROM wiki_page WHERE office_id = ?1 ORDER BY parent_id, ord",
        )?;
        let rows = stmt.query_map(params![office_id], row_to_page)?;
        let mut pages = Vec::new();
        for row in rows {
            pages.push(row??);
        }
        Ok(pages)
    }

    /// Flip the `stale` flag on the given pages, transactionally.
    pub fn mark_stale(&self, page_ids: &[String]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for id in page_ids {
            tx.execute("UPDATE wiki_page SET stale = 1 WHERE id = ?1", params![id])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Change history newest-first, at most `limit` entries.
    pub fn changelog(&self, office_id: &str, limit: usize) -> Result<Vec<WikiChangelogEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, office_id, page_id, change, at
             FROM wiki_changelog WHERE office_id = ?1 ORDER BY at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![office_id, limit as i64], |row| {
            let at: i64 = row.get(4)?;
            Ok(WikiChangelogEntry {
                id: row.get(0)?,
                office_id: row.get(1)?,
                page_id: row.get(2)?,
                change: row.get(3)?,
                at: at as u64,
            })
        })?;
        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }
        Ok(entries)
    }

    /// Direct children of `parent_id` (roots when `None`), ordered by `ord`
    /// (navigation tree).
    pub fn children(&self, office_id: &str, parent_id: Option<&str>) -> Result<Vec<WikiPage>> {
        // `IS` is null-safe: binds NULL → matches root rows, binds a value →
        // matches that parent.
        let mut stmt = self.conn.prepare(
            "SELECT id, office_id, parent_id, ord, kind, title, body,
                    citations, source_fingerprint, generated_at, stale
             FROM wiki_page
             WHERE office_id = ?1 AND parent_id IS ?2
             ORDER BY ord",
        )?;
        let rows = stmt.query_map(params![office_id, parent_id], row_to_page)?;
        let mut pages = Vec::new();
        for row in rows {
            pages.push(row??);
        }
        Ok(pages)
    }

    /// Full-text search over title + body, best matches first.
    pub fn search(&self, office_id: &str, query: &str, limit: usize) -> Result<Vec<WikiPage>> {
        let mut stmt = self.conn.prepare(
            "SELECT p.id, p.office_id, p.parent_id, p.ord, p.kind, p.title, p.body,
                    p.citations, p.source_fingerprint, p.generated_at, p.stale
             FROM wiki_page_fts f
             JOIN wiki_page p ON p.id = f.page_id
             WHERE f.wiki_page_fts MATCH ?1 AND p.office_id = ?2
             ORDER BY f.rank
             LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![query, office_id, limit as i64], row_to_page)?;
        let mut pages = Vec::new();
        for row in rows {
            pages.push(row??);
        }
        Ok(pages)
    }
}

impl WikiReadSurface for WikiStore {
    fn page(&self, id: &str) -> std::result::Result<Option<WikiPage>, String> {
        WikiStore::get_page(self, id).map_err(|e| e.to_string())
    }

    fn children(
        &self,
        office_id: &str,
        parent_id: Option<&str>,
    ) -> std::result::Result<Vec<WikiPage>, String> {
        WikiStore::children(self, office_id, parent_id).map_err(|e| e.to_string())
    }

    fn search(
        &self,
        office_id: &str,
        query: &str,
        limit: usize,
    ) -> std::result::Result<Vec<WikiPage>, String> {
        WikiStore::search(self, office_id, query, limit).map_err(|e| e.to_string())
    }

    fn changelog(
        &self,
        office_id: &str,
        limit: usize,
    ) -> std::result::Result<Vec<WikiChangelogEntry>, String> {
        WikiStore::changelog(self, office_id, limit).map_err(|e| e.to_string())
    }
}

impl WikiCache for WikiStore {
    fn get_page(&self, id: &str) -> std::result::Result<Option<WikiPage>, String> {
        WikiStore::get_page(self, id).map_err(|e| e.to_string())
    }

    fn apply_regeneration(
        &self,
        pages: &[WikiPage],
        changelog: &[WikiChangelogEntry],
    ) -> std::result::Result<(), String> {
        WikiStore::apply_regeneration(self, pages, changelog).map_err(|e| e.to_string())
    }

    fn rebuild_office(
        &self,
        office_id: &str,
        pages: &[WikiPage],
        changelog: &[WikiChangelogEntry],
    ) -> std::result::Result<(), String> {
        WikiStore::rebuild_office(self, office_id, pages, changelog).map_err(|e| e.to_string())
    }

    fn pages_for_office(&self, office_id: &str) -> std::result::Result<Vec<WikiPage>, String> {
        WikiStore::pages_for_office(self, office_id).map_err(|e| e.to_string())
    }

    fn mark_stale(&self, page_ids: &[String]) -> std::result::Result<(), String> {
        WikiStore::mark_stale(self, page_ids).map_err(|e| e.to_string())
    }

    fn changelog(
        &self,
        office_id: &str,
        limit: usize,
    ) -> std::result::Result<Vec<WikiChangelogEntry>, String> {
        WikiStore::changelog(self, office_id, limit).map_err(|e| e.to_string())
    }
}

fn upsert_page_on(conn: &Connection, page: &WikiPage) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO wiki_page
         (id, office_id, parent_id, ord, kind, title, body,
          citations, source_fingerprint, generated_at, stale)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            page.id,
            page.office_id,
            page.parent_id,
            page.ord,
            page.kind.as_str(),
            page.title,
            page.body,
            citations_to_json(&page.citations),
            page.source_fingerprint,
            page.generated_at as i64,
            page.stale as i64,
        ],
    )?;
    // Standalone FTS index (like the memory store): re-sync this page's row.
    conn.execute(
        "DELETE FROM wiki_page_fts WHERE page_id = ?1",
        params![page.id],
    )?;
    conn.execute(
        "INSERT INTO wiki_page_fts (page_id, title, body) VALUES (?1, ?2, ?3)",
        params![page.id, page.title, page.body],
    )?;
    Ok(())
}

fn append_changelog_on(conn: &Connection, entry: &WikiChangelogEntry) -> Result<()> {
    conn.execute(
        "INSERT INTO wiki_changelog (id, office_id, page_id, change, at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            entry.id,
            entry.office_id,
            entry.page_id,
            entry.change,
            entry.at as i64,
        ],
    )?;
    Ok(())
}

/// Map a `wiki_page` row to a `WikiPage`. Returns `Result` inside the row
/// closure's `Ok` so a corrupt row (unknown kind / bad JSON) surfaces as
/// `WikiError::Corrupt` rather than a panic.
fn row_to_page(row: &rusqlite::Row<'_>) -> rusqlite::Result<Result<WikiPage>> {
    let kind_str: String = row.get(4)?;
    let citations_json: String = row.get(7)?;
    let stale: i64 = row.get(10)?;
    let generated_at: i64 = row.get(9)?;

    Ok((|| {
        let kind = WikiPageKind::from_db_str(&kind_str)
            .ok_or_else(|| WikiError::Corrupt(format!("unknown page kind: {kind_str}")))?;
        let citations = citations_from_json(&citations_json)?;
        Ok(WikiPage {
            id: row.get(0)?,
            office_id: row.get(1)?,
            parent_id: row.get(2)?,
            ord: row.get(3)?,
            kind,
            title: row.get(5)?,
            body: row.get(6)?,
            citations,
            source_fingerprint: row.get(8)?,
            generated_at: generated_at as u64,
            stale: stale != 0,
        })
    })())
}

fn citations_to_json(citations: &[WikiCitation]) -> String {
    let arr: Vec<serde_json::Value> = citations
        .iter()
        .map(|c| serde_json::json!({ "source_kind": c.source_kind, "source_id": c.source_id }))
        .collect();
    serde_json::Value::Array(arr).to_string()
}

fn citations_from_json(s: &str) -> Result<Vec<WikiCitation>> {
    let value: serde_json::Value =
        serde_json::from_str(s).map_err(|e| WikiError::Corrupt(e.to_string()))?;
    let array = value
        .as_array()
        .ok_or_else(|| WikiError::Corrupt("citations is not a JSON array".to_string()))?;
    array
        .iter()
        .map(|c| {
            let source_kind = c
                .get("source_kind")
                .and_then(|v| v.as_str())
                .ok_or_else(|| WikiError::Corrupt("citation missing source_kind".to_string()))?;
            let source_id = c
                .get("source_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| WikiError::Corrupt("citation missing source_id".to_string()))?;
            Ok(WikiCitation::new(source_kind, source_id))
        })
        .collect()
}

/// The schema version this build reads and writes (`crate::versioning`).
pub(crate) const SCHEMA_VERSION: i64 = 1;

pub(crate) fn setup(conn: &Connection) -> Result<()> {
    conn.execute_batch("PRAGMA journal_mode = WAL")?;
    crate::versioning::open_schema(conn, "wiki", SCHEMA_VERSION, create_schema, &[])
}

fn create_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS wiki_page (
            id                 TEXT PRIMARY KEY NOT NULL,
            office_id          TEXT NOT NULL,
            parent_id          TEXT,
            ord                INTEGER NOT NULL DEFAULT 0,
            kind               TEXT NOT NULL,
            title              TEXT NOT NULL,
            body               TEXT NOT NULL,
            citations          TEXT NOT NULL,
            source_fingerprint TEXT NOT NULL,
            generated_at       INTEGER NOT NULL,
            stale              INTEGER NOT NULL DEFAULT 0
        )",
    )?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS wiki_changelog (
            id        TEXT PRIMARY KEY NOT NULL,
            office_id TEXT NOT NULL,
            page_id   TEXT,
            change    TEXT NOT NULL,
            at        INTEGER NOT NULL
        )",
    )?;
    // Standalone FTS5 (synced manually in upsert_page), matching the memory
    // store's proven pattern rather than an external-content table — the spec
    // DDL's `content=wiki_page` is marked illustrative and would couple the
    // FTS rowid to wiki_page's implicit rowid despite its TEXT primary key.
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS wiki_page_fts USING fts5(
            page_id UNINDEXED,
            title,
            body
        )",
    )?;
    conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_wiki_parent ON wiki_page(parent_id, ord)")?;
    conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_wiki_stale ON wiki_page(office_id, stale)")?;
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

    #[test]
    fn tables_and_indices_created() {
        let store = WikiStore::open_in_memory().expect("open");
        let tables = names(&store.conn, "table");
        assert!(tables.contains(&"wiki_page".to_string()));
        assert!(tables.contains(&"wiki_changelog".to_string()));
        assert!(tables.contains(&"wiki_page_fts".to_string()));

        let indices = names(&store.conn, "index");
        assert!(indices.contains(&"idx_wiki_parent".to_string()));
        assert!(indices.contains(&"idx_wiki_stale".to_string()));
    }

    #[test]
    fn a_page_round_trips_with_its_citations_and_fingerprint() {
        let store = WikiStore::open_in_memory().expect("open");

        let mut page = WikiPage::new(
            "overview",
            "office-1",
            WikiPageKind::Overview,
            "Project Overview",
            "This project builds a local-first AI office.",
        );
        page.citations = vec![
            WikiCitation::new("decision", "dec-42"),
            WikiCitation::new("board_item", "card-7"),
        ];
        page.source_fingerprint = "fp-abc123".to_string();
        page.generated_at = 1_700_000_000;

        store.upsert_page(&page).expect("upsert");
        let got = store.get_page("overview").expect("get").expect("present");

        assert_eq!(got, page, "the page round-trips unchanged");
        assert_eq!(got.citations.len(), 2);
        assert_eq!(got.source_fingerprint, "fp-abc123");
        assert!(!got.stale);

        // A missing id is None, not an error.
        assert!(store.get_page("nope").expect("get missing").is_none());
    }

    #[test]
    fn upsert_replaces_by_id_and_resyncs_fts() {
        let store = WikiStore::open_in_memory().expect("open");
        let mut page = WikiPage::new("p", "office-1", WikiPageKind::Area, "T", "old body");
        page.citations = vec![WikiCitation::new("ledger_fact", "f1")];
        store.upsert_page(&page).expect("first upsert");

        page.body = "new body".to_string();
        store.upsert_page(&page).expect("replace");

        let got = store.get_page("p").expect("get").expect("present");
        assert_eq!(got.body, "new body");

        // FTS holds exactly one row for this page after the replace.
        let fts_count: i64 = store
            .conn
            .query_row(
                "SELECT count(*) FROM wiki_page_fts WHERE page_id = ?1",
                params!["p"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fts_count, 1);
    }

    fn changelog(id: &str) -> WikiChangelogEntry {
        WikiChangelogEntry {
            id: id.to_string(),
            office_id: "office-1".to_string(),
            page_id: Some("p".to_string()),
            change: "regenerated".to_string(),
            at: 1_700_000_000,
        }
    }

    #[test]
    fn apply_regeneration_is_atomic_and_rolls_back_on_failure() {
        let store = WikiStore::open_in_memory().expect("open");

        // Seed a committed page + changelog.
        let mut original = WikiPage::new("p", "office-1", WikiPageKind::Overview, "T", "original");
        original.citations = vec![WikiCitation::new("decision", "d1")];
        store
            .apply_regeneration(&[original.clone()], &[changelog("cl-1")])
            .expect("seed regeneration commits");

        // A second batch updates the page but its changelog has a DUPLICATE id
        // (`cl-1` already exists) — the second changelog INSERT violates the
        // PRIMARY KEY, failing the transaction part-way.
        let mut updated = original.clone();
        updated.body = "SHOULD NOT PERSIST".to_string();
        let result = store.apply_regeneration(&[updated], &[changelog("cl-1")]);
        assert!(
            result.is_err(),
            "a duplicate changelog id must fail the batch"
        );

        // The whole batch rolled back: the prior page row is intact...
        let got = store.get_page("p").expect("get").expect("present");
        assert_eq!(
            got.body, "original",
            "the failed regeneration must not have written"
        );
        // ...and no duplicate/partial changelog row was left behind.
        let cl_count: i64 = store
            .conn
            .query_row("SELECT count(*) FROM wiki_changelog", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cl_count, 1, "only the seed changelog row exists");
    }

    #[test]
    fn rebuild_office_replaces_every_row_and_resyncs_fts() {
        let store = WikiStore::open_in_memory().expect("open");

        // Seed a prior wiki for the office (+ a changelog + FTS content).
        let mut old = WikiPage::new(
            "office-1:overview",
            "office-1",
            WikiPageKind::Overview,
            "Old Overview",
            "obsolete stale body",
        );
        old.citations = vec![WikiCitation::new("decision", "d-old")];
        store
            .apply_regeneration(&[old], &[changelog("cl-old")])
            .expect("seed");
        // A row for a DIFFERENT office must survive the rebuild untouched.
        let other = WikiPage::new(
            "office-2:overview",
            "office-2",
            WikiPageKind::Overview,
            "Other",
            "keep me",
        );
        store.upsert_page(&other).expect("other office");

        // Rebuild office-1 from a fresh derived set.
        let mut fresh = WikiPage::new(
            "office-1:overview",
            "office-1",
            WikiPageKind::Overview,
            "Fresh Overview",
            "rebuilt current body",
        );
        fresh.citations = vec![WikiCitation::new("decision", "d-new")];
        store
            .rebuild_office("office-1", &[fresh.clone()], &[changelog("cl-new")])
            .expect("rebuild");

        // The office now holds exactly the rebuilt page — the old row is gone.
        let pages = store.pages_for_office("office-1").unwrap();
        assert_eq!(pages, vec![fresh], "office-1 holds only the rebuilt page");

        // The changelog was replaced too (old entry dropped, new one present).
        let cl = store.changelog("office-1", 10).unwrap();
        assert_eq!(cl.len(), 1);
        assert_eq!(cl[0].id, "cl-new");

        // FTS reflects only the fresh body — the stale term no longer matches,
        // the new term does.
        assert!(
            store.search("office-1", "obsolete", 10).unwrap().is_empty(),
            "the dropped body's terms no longer match"
        );
        assert_eq!(
            store
                .search("office-1", "rebuilt", 10)
                .unwrap()
                .into_iter()
                .map(|p| p.id)
                .collect::<Vec<_>>(),
            vec!["office-1:overview"]
        );

        // The other office is untouched.
        assert!(
            store.get_page("office-2:overview").unwrap().is_some(),
            "a rebuild is office-scoped — other offices survive"
        );
    }

    #[test]
    fn rebuild_office_rolls_back_on_failure_leaving_prior_rows_intact() {
        let store = WikiStore::open_in_memory().expect("open");
        let mut original = WikiPage::new("p", "office-1", WikiPageKind::Overview, "T", "original");
        original.citations = vec![WikiCitation::new("decision", "d1")];
        store
            .apply_regeneration(&[original.clone()], &[changelog("cl-1")])
            .expect("seed");

        // A rebuild whose changelog carries a DUPLICATE id within the batch
        // fails the second INSERT mid-transaction (PRIMARY KEY violation) —
        // after the DELETEs have already run, proving atomicity across the
        // clear + reinsert.
        let mut replacement = original.clone();
        replacement.body = "SHOULD NOT PERSIST".to_string();
        let dup = changelog("dup");
        let result = store.rebuild_office("office-1", &[replacement], &[dup.clone(), dup]);
        assert!(
            result.is_err(),
            "a duplicate changelog id must fail the batch"
        );

        // The clear was rolled back with the rest: the prior row is intact.
        let got = store.get_page("p").expect("get").expect("present");
        assert_eq!(
            got.body, "original",
            "a failed rebuild must not have cleared the prior rows"
        );
        let cl_count: i64 = store
            .conn
            .query_row("SELECT count(*) FROM wiki_changelog", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cl_count, 1, "only the seed changelog row survives");
    }

    #[test]
    fn mark_stale_flips_the_flag_and_pages_for_office_reads_it_back() {
        let store = WikiStore::open_in_memory().expect("open");
        let a = WikiPage::new(
            "office-1:overview",
            "office-1",
            WikiPageKind::Overview,
            "O",
            "b",
        );
        let b = WikiPage::new(
            "office-1:decisions",
            "office-1",
            WikiPageKind::Decisions,
            "D",
            "b",
        );
        store.upsert_page(&a).expect("a");
        store.upsert_page(&b).expect("b");

        // Both start fresh.
        assert!(
            store
                .pages_for_office("office-1")
                .unwrap()
                .iter()
                .all(|p| !p.stale)
        );

        store
            .mark_stale(&["office-1:overview".to_string()])
            .expect("mark");

        let pages = store.pages_for_office("office-1").unwrap();
        let overview = pages.iter().find(|p| p.id == "office-1:overview").unwrap();
        let decisions = pages.iter().find(|p| p.id == "office-1:decisions").unwrap();
        assert!(overview.stale, "the marked page is stale");
        assert!(!decisions.stale, "an unmarked page stays fresh");
    }

    #[test]
    fn changelog_is_returned_newest_first() {
        let store = WikiStore::open_in_memory().expect("open");
        for (id, at) in [("cl-a", 100u64), ("cl-b", 300), ("cl-c", 200)] {
            store
                .append_changelog(&WikiChangelogEntry {
                    id: id.to_string(),
                    office_id: "office-1".to_string(),
                    page_id: None,
                    change: id.to_string(),
                    at,
                })
                .expect("append");
        }

        let entries = store.changelog("office-1", 10).expect("changelog");
        let ats: Vec<u64> = entries.iter().map(|e| e.at).collect();
        assert_eq!(ats, vec![300, 200, 100], "newest-first by `at`");

        // The limit is honored.
        assert_eq!(store.changelog("office-1", 1).unwrap().len(), 1);
    }

    // Build an overview → area → detail tree for the read-surface tests.
    fn seed_tree(store: &WikiStore) {
        let mut overview = WikiPage::new(
            "o",
            "office-1",
            WikiPageKind::Overview,
            "Overview",
            "the project",
        );
        let mut area = WikiPage::new(
            "a",
            "office-1",
            WikiPageKind::Area,
            "Billing",
            "billing area",
        );
        area.parent_id = Some("o".to_string());
        area.ord = 0;
        let mut detail = WikiPage::new(
            "d",
            "office-1",
            WikiPageKind::Howto,
            "Refunds",
            "how to issue a refund",
        );
        detail.parent_id = Some("a".to_string());
        detail.ord = 0;
        overview.ord = 0;
        for p in [&overview, &area, &detail] {
            store.upsert_page(p).expect("seed");
        }
    }

    #[test]
    fn read_surface_navigates_the_parent_ord_tree() {
        let store = WikiStore::open_in_memory().expect("open");
        seed_tree(&store);
        // Exercise through the read-only trait object — the client's handle.
        let reader: &dyn WikiReadSurface = &store;

        let roots = reader.children("office-1", None).expect("roots");
        assert_eq!(
            roots.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            ["o"]
        );

        let under_overview = reader.children("office-1", Some("o")).expect("children");
        assert_eq!(
            under_overview
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>(),
            ["a"]
        );

        let under_area = reader.children("office-1", Some("a")).expect("children");
        assert_eq!(
            under_area.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            ["d"]
        );

        assert_eq!(reader.page("d").unwrap().unwrap().title, "Refunds");
    }

    #[test]
    fn read_surface_full_text_search_finds_the_matching_page() {
        let store = WikiStore::open_in_memory().expect("open");
        seed_tree(&store);
        let reader: &dyn WikiReadSurface = &store;

        let hits = reader.search("office-1", "refund", 10).expect("search");
        assert_eq!(
            hits.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            ["d"]
        );

        // A term only in the overview matches the overview.
        let hits = reader.search("office-1", "project", 10).expect("search");
        assert_eq!(
            hits.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            ["o"]
        );

        // No match → empty.
        assert!(
            reader
                .search("office-1", "nonexistentterm", 10)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn read_surface_is_read_only_by_construction() {
        // `&dyn WikiReadSurface` exposes ONLY read methods — there is no
        // upsert/mark_stale/apply_regeneration on it, so the client
        // structurally cannot mutate the wiki. This compiles; a write
        // call like `reader.upsert_page(..)` would not.
        let store = WikiStore::open_in_memory().expect("open");
        seed_tree(&store);
        let reader: &dyn WikiReadSurface = &store;
        let _ = reader.page("o").unwrap();
        let _ = reader.children("office-1", None).unwrap();
        let _ = reader.search("office-1", "project", 5).unwrap();
        let _ = reader.changelog("office-1", 5).unwrap();
    }
}
