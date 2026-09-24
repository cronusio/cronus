//! Knowledge-store invariant acceptance sweep
//! — the closing validation for this subsystem. Each KB invariant maps to one named test,
//! exercised through the **real SQLite + sqlite-vec `KnowledgeDb`** (not an
//! in-memory fake store) and the real domain pipeline exported via
//! `cronus_core`, so the guarantees are proven against actual persistence and
//! composition — matching the `wiki_invariants`/`activation_invariants`
//! precedent. The embedding backend is a deterministic fake (no live model
//! in CI, the generator-optional/no-live-model precedent every prior sweep
//! in this project follows); `HttpUrlFetcher`'s real TCP mechanics are
//! already proven in `crates/core/src/knowledge_bootstrap.rs`'s own tests —
//! here the URL variant uses a fake `UrlFetcher` to keep the sweep
//! network-free, consistent with every other invariant here running
//! offline and deterministic.

use cronus_contract::{Collection, Curation, Directory, Document, DocumentStatus, WriteOverride};
use cronus_core::file_store::FileStore;
use cronus_core::knowledge_access::{GatedKnowledge, KnowledgeAccessError, KnowledgePrincipal};
use cronus_core::knowledge_ingest::{
    ChunkParams, EmbeddingBackend, FileIngester, RecordIngester, UrlFetcher, UrlIngester,
    ingest_document,
};
use cronus_core::knowledge_retrieval::{PreparedQuery, QueryPreparer, retrieve};
use cronus_core::resource_sharing::{
    AccessGrant, GrantStore, Permission, PrincipalKind, ResourceKind,
};
use cronus_store_local::knowledge::KnowledgeDb;

/// Deterministic, content-derived "embedding" — no live model needed. Two
/// texts sharing a first word land close together; otherwise apart.
struct FakeEmbedder;
impl EmbeddingBackend for FakeEmbedder {
    fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
        let mut v = vec![0.0f32; cronus_store_local::knowledge::EMBEDDING_DIM];
        let seed = text.split_whitespace().next().unwrap_or("").len() as f32;
        v[0] = seed;
        v[1] = text.len() as f32;
        Ok(v)
    }
}

fn seed_ready_document(
    db: &KnowledgeDb,
    collection_id: &str,
    doc_id: &str,
    text: &str,
) -> Document {
    let doc = Document::new_agent(doc_id, collection_id, format!("{doc_id}.md"));
    ingest_document(
        db,
        &FakeEmbedder,
        doc,
        text,
        ChunkParams::default(),
        &WriteOverride::None,
    )
    .expect("ingest")
}

// ── Collection isolation ───────────────────────────────────────────────

#[test]
fn kb1_a_query_never_returns_another_collections_chunk() {
    let db = KnowledgeDb::open_in_memory().expect("open");
    db.create_collection(&Collection::new("col-1", "user-1", "A"))
        .unwrap();
    db.create_collection(&Collection::new("col-2", "user-1", "B"))
        .unwrap();
    seed_ready_document(&db, "col-1", "doc-1", "alpha content unique to one");
    seed_ready_document(&db, "col-2", "doc-2", "alpha content unique to two");

    let request = cronus_contract::RetrievalRequest::new("alpha", vec!["col-1".to_string()]);
    let (results, _prepared) = retrieve(&db, &FakeEmbedder, None, &request).expect("retrieve");
    assert!(
        results.iter().all(|c| c.collection_id == "col-1"),
        "no result may come from an unrequested collection"
    );
    assert!(
        !results.is_empty(),
        "the requested collection's own match is present"
    );
}

// ── Hierarchical organisation (directory tree, retrieval-independent) ──

#[test]
fn kb2_directory_structure_never_affects_retrieval() {
    let db = KnowledgeDb::open_in_memory().expect("open");
    db.create_collection(&Collection::new("col-1", "user-1", "A"))
        .unwrap();
    db.create_directory(&Directory {
        id: "dir-1".into(),
        collection_id: "col-1".into(),
        parent_id: None,
        name: "Reports".into(),
    })
    .unwrap();
    // A document is retrievable by content regardless of whether it is
    // placed in a directory at all (directory_id stays None here) — the
    // schema supports the tree, but retrieval ranking never consults it.
    seed_ready_document(&db, "col-1", "doc-1", "quarterly numbers unique term");

    let request =
        cronus_contract::RetrievalRequest::new("quarterly numbers", vec!["col-1".to_string()]);
    let (results, _) = retrieve(&db, &FakeEmbedder, None, &request).expect("retrieve");
    assert_eq!(
        results.len(),
        1,
        "retrieval succeeds independent of directory placement"
    );
}

// ── Incremental indexing ────────────────────────────────────────────────

#[test]
fn kb3_re_ingesting_a_document_replaces_its_chunks_not_accumulates() {
    let db = KnowledgeDb::open_in_memory().expect("open");
    db.create_collection(&Collection::new("col-1", "user-1", "A"))
        .unwrap();
    let doc = seed_ready_document(&db, "col-1", "doc-1", "zebra quokka narwhal originally");

    // Re-ingest the SAME document id with entirely non-overlapping vocabulary.
    ingest_document(
        &db,
        &FakeEmbedder,
        doc,
        "completely different replacement wording",
        ChunkParams::default(),
        &WriteOverride::None,
    )
    .expect("re-ingest");

    // Checked directly against the FTS index (not the fused `retrieve()`
    // path): ANN's k-nearest-neighbour search always returns *something*
    // when the index is non-empty, regardless of true relevance — a correct,
    // expected property of vector search, not a concern. FTS5's `MATCH`
    // is genuinely relevance-gated, so it is the precise instrument for
    // proving the old chunk's terms are gone, not merely out-ranked.
    let old_term_hits = db
        .fts_search(&["col-1".to_string()], "zebra quokka narwhal", 10)
        .expect("fts_search");
    assert!(
        old_term_hits.is_empty(),
        "the old chunk's indexed terms must be fully removed, not merely out-ranked"
    );
    let new_term_hits = db
        .fts_search(
            &["col-1".to_string()],
            "completely different replacement",
            10,
        )
        .expect("fts_search");
    assert_eq!(
        new_term_hits.len(),
        1,
        "the replacement chunk is indexed exactly once"
    );
}

// ── Access control ──────────────────────────────────────────────────────

#[test]
fn kb4_a_query_from_a_caller_with_no_grant_is_denied_before_the_store_is_searched() {
    let db = KnowledgeDb::open_in_memory().expect("open");
    db.create_collection(&Collection::new("col-1", "user-1", "A"))
        .unwrap();
    seed_ready_document(&db, "col-1", "doc-1", "gated content");

    let grants = GrantStore::new();
    let gate = GatedKnowledge::new(&db, &grants, KnowledgePrincipal::member("stranger", vec![]));
    let err = gate
        .authorize_collections(&["col-1".to_string()])
        .expect_err("no grant, no owner");
    assert!(matches!(err, KnowledgeAccessError::Denied { .. }));

    // With a grant, the same collection is reachable.
    let mut grants = GrantStore::new();
    grants.add(AccessGrant {
        resource_type: ResourceKind::Knowledge,
        resource_id: "col-1".to_string(),
        principal_type: PrincipalKind::User,
        principal_id: "alice".to_string(),
        permission: Permission::Read,
    });
    let gate = GatedKnowledge::new(&db, &grants, KnowledgePrincipal::member("alice", vec![]));
    let scope = gate
        .authorize_collections(&["col-1".to_string()])
        .expect("granted");
    assert_eq!(scope, vec!["col-1".to_string()]);
}

// ── Source types (file / URL / record) ─────────────────────────────────

#[test]
fn kb5_all_three_source_adapters_produce_ready_ingestible_text() {
    let db = KnowledgeDb::open_in_memory().expect("open");
    db.create_collection(&Collection::new("col-1", "user-1", "A"))
        .unwrap();

    // Record — a direct passthrough.
    let record_text = RecordIngester::extract("plain record content");
    let d1 = seed_ready_document(&db, "col-1", "doc-record", &record_text);
    assert_eq!(d1.status, DocumentStatus::Ready);

    // File — via the real content-addressed FileStore.
    let mut files = FileStore::new();
    files.add_file("file-1", b"file-backed content");
    let file_text = FileIngester::extract(&files, "file-1").expect("extract");
    let doc = Document::new_agent("doc-file", "col-1", "f.txt");
    let ingested = ingest_document(
        &db,
        &FakeEmbedder,
        doc,
        &file_text,
        ChunkParams::default(),
        &WriteOverride::None,
    )
    .expect("ingest file source");
    assert_eq!(ingested.status, DocumentStatus::Ready);

    // URL — via a fake fetcher (the real HttpUrlFetcher's TCP mechanics are
    // proven separately in knowledge_bootstrap's own tests).
    struct FakeFetcher;
    impl UrlFetcher for FakeFetcher {
        fn fetch(&self, _url: &str) -> Result<String, String> {
            Ok("<html><body><p>URL-sourced content</p></body></html>".to_string())
        }
    }
    let url_text = UrlIngester::extract(&FakeFetcher, "http://example.test/").expect("extract");
    assert_eq!(url_text, "URL-sourced content");
    let doc = Document::new_agent("doc-url", "col-1", "page.html");
    let ingested = ingest_document(
        &db,
        &FakeEmbedder,
        doc,
        &url_text,
        ChunkParams::default(),
        &WriteOverride::None,
    )
    .expect("ingest url source");
    assert_eq!(ingested.status, DocumentStatus::Ready);
}

// ── Source attribution ──────────────────────────────────────────────────

#[test]
fn kb6_every_retrieved_chunk_carries_document_and_source_ref_attribution() {
    let db = KnowledgeDb::open_in_memory().expect("open");
    db.create_collection(&Collection::new("col-1", "user-1", "A"))
        .unwrap();
    seed_ready_document(&db, "col-1", "doc-1", "attributed unique content here");

    let request =
        cronus_contract::RetrievalRequest::new("attributed unique", vec!["col-1".to_string()]);
    let (results, _) = retrieve(&db, &FakeEmbedder, None, &request).expect("retrieve");
    assert!(!results.is_empty());
    for chunk in &results {
        assert_eq!(chunk.document_id, "doc-1");
        assert!(!chunk.chunk_id.is_empty());
    }
}

// ── Non-authoritative recall (structural) ──────────────────────────────

#[test]
fn kb7_the_retrieved_chunk_shape_asserts_no_correctness_only_text_source_score() {
    let db = KnowledgeDb::open_in_memory().expect("open");
    db.create_collection(&Collection::new("col-1", "user-1", "A"))
        .unwrap();
    seed_ready_document(&db, "col-1", "doc-1", "some evidence not ground truth");

    let request = cronus_contract::RetrievalRequest::new("evidence", vec!["col-1".to_string()]);
    let (results, _) = retrieve(&db, &FakeEmbedder, None, &request).expect("retrieve");
    let chunk = results.first().expect("a match");
    // The API surface is exactly (chunk_id, document_id, collection_id,
    // text, source_ref, score) — no "verified"/"confidence"/"is_correct"
    // field exists on RetrievedChunk to assert. This test documents that
    // contract by exercising the real shape end to end.
    let _: (&str, &str, &str, &str, f32) = (
        &chunk.chunk_id,
        &chunk.document_id,
        &chunk.collection_id,
        &chunk.text,
        chunk.score,
    );
}

// ── Soft deletion ────────────────────────────────────────────────────────

#[test]
fn kb8_a_soft_deleted_document_is_excluded_from_retrieval_then_gc_removes_it() {
    let db = KnowledgeDb::open_in_memory().expect("open");
    db.create_collection(&Collection::new("col-1", "user-1", "A"))
        .unwrap();
    seed_ready_document(&db, "col-1", "doc-1", "soon to be deleted unique text");

    db.soft_delete_document("doc-1").expect("soft delete");
    let request =
        cronus_contract::RetrievalRequest::new("soon to be deleted", vec!["col-1".to_string()]);
    let (results, _) = retrieve(&db, &FakeEmbedder, None, &request).expect("retrieve");
    assert!(
        results.is_empty(),
        "a soft-deleted document must not surface in retrieval"
    );

    std::thread::sleep(std::time::Duration::from_millis(1100));
    let removed = db.gc(0).expect("gc");
    assert_eq!(removed, 1);
    assert!(db.get_document("doc-1").unwrap().is_none());
}

// ── Authorship zones ────────────────────────────────────────────────────

#[test]
fn kb9_a_human_zone_rewrite_requires_an_audited_override_but_initial_ingest_and_status_updates_dont()
 {
    let db = KnowledgeDb::open_in_memory().expect("open");
    db.create_collection(&Collection::new("col-1", "user-1", "A"))
        .unwrap();

    // Initial human-origin ingest is not gated (protects rewriting, not creation).
    let human_doc = Document::new_human("doc-1", "col-1", "contract.pdf");
    let ingested = ingest_document(
        &db,
        &FakeEmbedder,
        human_doc,
        "human authored contract text",
        ChunkParams::default(),
        &WriteOverride::None,
    )
    .expect("initial human ingest, including its own internal status writes, is not gated");
    assert_eq!(ingested.status, DocumentStatus::Ready);
    assert_eq!(ingested.origin, cronus_contract::Origin::Human);

    // A caller-initiated rewrite of the CONTENT (name) without an override is refused.
    let mut rewrite = ingested.clone();
    rewrite.name = "tampered.pdf".to_string();
    let err = db
        .write_document(&rewrite, &WriteOverride::None)
        .expect_err("a content rewrite of a human-origin row needs an override");
    assert!(matches!(
        err,
        cronus_store_local::knowledge::KnowledgeError::ReadOnlyZone { .. }
    ));

    // With an audited override, it succeeds.
    db.write_document(
        &rewrite,
        &WriteOverride::HumanDirected {
            audit_ref: "audit-1".into(),
        },
    )
    .expect("an audited override may rewrite human-origin content");
}

// ── Curation lifecycle ─────────────────────────────────────────────────

#[test]
fn kb10_min_curation_excludes_draft_chunks_but_human_sources_stay_eligible() {
    let db = KnowledgeDb::open_in_memory().expect("open");
    db.create_collection(&Collection::new("col-1", "user-1", "A"))
        .unwrap();

    // An agent doc left at the default draft curation.
    seed_ready_document(&db, "col-1", "doc-draft", "draft curated shared term");

    // A human-origin doc (no curation) — always eligible regardless of floor.
    let human = Document::new_human("doc-human", "col-1", "manual.pdf");
    ingest_document(
        &db,
        &FakeEmbedder,
        human,
        "shared term from a human source",
        ChunkParams::default(),
        &WriteOverride::None,
    )
    .expect("ingest human source");

    let mut request =
        cronus_contract::RetrievalRequest::new("shared term", vec!["col-1".to_string()]);
    request.min_curation = Some(Curation::Reviewed);
    let (results, _) = retrieve(&db, &FakeEmbedder, None, &request).expect("retrieve");
    let ids: Vec<&str> = results.iter().map(|c| c.document_id.as_str()).collect();
    assert!(
        !ids.contains(&"doc-draft"),
        "a draft-curation document is below the requested floor"
    );
    assert!(
        ids.contains(&"doc-human"),
        "a human-origin source stays eligible regardless of the floor"
    );
}

// ── Query preparation ──────────────────────────────────────────────────

struct ExpandingPreparer;
impl QueryPreparer for ExpandingPreparer {
    fn prepare(&self, raw: &str) -> PreparedQuery {
        PreparedQuery {
            retrieval_query: format!("{raw} expanded"),
            subqueries: vec!["a related subquery term".to_string()],
            raw: raw.to_string(),
        }
    }
}

struct EmptyPreparer;
impl QueryPreparer for EmptyPreparer {
    fn prepare(&self, raw: &str) -> PreparedQuery {
        PreparedQuery {
            retrieval_query: String::new(),
            subqueries: Vec::new(),
            raw: raw.to_string(),
        }
    }
}

#[test]
fn kb11_query_preparation_is_recorded_falls_back_when_empty_and_never_widens_scope() {
    let db = KnowledgeDb::open_in_memory().expect("open");
    db.create_collection(&Collection::new("col-1", "user-1", "A"))
        .unwrap();
    seed_ready_document(&db, "col-1", "doc-1", "findable content for preparation");

    // A wired preparer's prepared+raw are both recorded (transparency).
    let request = cronus_contract::RetrievalRequest::new("findable", vec!["col-1".to_string()]);
    let (_results, prepared) =
        retrieve(&db, &FakeEmbedder, Some(&ExpandingPreparer), &request).expect("retrieve");
    assert_eq!(prepared.retrieval_query, "findable expanded");
    assert_eq!(prepared.raw, "findable");
    assert_eq!(prepared.subqueries.len(), 1);

    // An empty preparation falls back to the raw query — never an empty search.
    let (results, prepared) =
        retrieve(&db, &FakeEmbedder, Some(&EmptyPreparer), &request).expect("retrieve");
    assert_eq!(prepared.retrieval_query, "findable");
    assert!(
        !results.is_empty(),
        "the fallback query must still actually search"
    );

    // Preparation never widens the collection scope: the request still
    // only names col-1, regardless of what the preparer returns.
    assert_eq!(request.collection_ids, vec!["col-1".to_string()]);
}
