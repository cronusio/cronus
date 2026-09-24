//! Hybrid semantic + keyword retrieval.
//!
//! Composes the already-KB-1-scoped store primitives
//! ([`KnowledgeStore::ann_search`], [`KnowledgeStore::fts_search`],
//! [`KnowledgeStore::hydrate_chunks`] — all built in the schema/store-
//! scaffolding task) with Reciprocal Rank Fusion. This module owns none of
//! the collection-isolation or curation-floor logic itself — it composes
//! store primitives that already enforce them, then ranks the union.

use std::collections::HashMap;

use cronus_contract::{KnowledgeStore, RetrievalRequest, RetrievedChunk};

use crate::knowledge_ingest::EmbeddingBackend;

/// A failure during retrieval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetrievalError {
    /// The embedding backend refused or failed on the query.
    Embed(String),
    /// The knowledge store rejected a search or hydration call.
    Store(String),
}

impl std::fmt::Display for RetrievalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RetrievalError::Embed(m) => write!(f, "query embedding failed: {m}"),
            RetrievalError::Store(m) => write!(f, "store search failed: {m}"),
        }
    }
}

impl std::error::Error for RetrievalError {}

/// Reciprocal Rank Fusion constant ("RRF, k=60").
const RRF_K: f64 = 60.0;

/// An optional pre-retrieval query transformation — keyword
/// extraction/expansion and/or compound-query decomposition. Implementations
/// MAY return the input unchanged; [`resolve_query`] enforces the
/// fallback-floor guarantee regardless (never an empty search), so a
/// preparer does not need to re-implement it defensively.
pub trait QueryPreparer {
    fn prepare(&self, raw: &str) -> PreparedQuery;
}

/// The result of query preparation, always carrying the raw query
/// alongside whatever was derived — the transparency requirement ("a reader
/// sees exactly what was searched").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedQuery {
    pub retrieval_query: String,
    /// Independently-retrieved, RRF-merged alongside `retrieval_query`.
    /// Empty for an atomic (non-compound) query.
    pub subqueries: Vec<String>,
    pub raw: String,
}

impl PreparedQuery {
    /// The identity preparation: `retrieval_query == raw`, no sub-queries.
    /// What an unwired (`preparer: None`) retrieval uses.
    fn identity(raw: &str) -> Self {
        PreparedQuery {
            retrieval_query: raw.to_string(),
            subqueries: Vec::new(),
            raw: raw.to_string(),
        }
    }
}

/// Resolve the query (or queries) to actually search: run `preparer` if
/// given, then enforce the fallback floor — an empty `retrieval_query`
/// with no sub-queries degrades to `raw`, so a buggy or overzealous
/// preparer can never turn a real query into an empty search. `preparer:
/// None` is the no-op path (identity, embedded directly — matching pre-KB-11
/// `retrieve` behavior exactly).
fn resolve_query(preparer: Option<&dyn QueryPreparer>, raw: &str) -> PreparedQuery {
    let mut prepared = match preparer {
        Some(p) => p.prepare(raw),
        None => return PreparedQuery::identity(raw),
    };
    if prepared.retrieval_query.trim().is_empty() && prepared.subqueries.is_empty() {
        prepared.retrieval_query = raw.to_string();
    }
    prepared
}

/// Run a hybrid retrieval request: resolve the query via `preparer` (optional;
/// `None` skips preparation), embed and search every resulting query
/// (`retrieval_query` plus each sub-query, each independently) against both
/// modalities (over-fetched at `top_k * 2` beyond the final cut, giving RRF
/// enough candidates per list to rank cross-modal matches well), fuse *all*
/// lists by Reciprocal Rank Fusion, then hydrate the fused top `top_k` ids
/// into fully attributed [`RetrievedChunk`]s — `hydrate_chunks` also
/// applies the `min_curation` floor, so a chunk RRF ranked highly can
/// still be absent from the final set if it falls below the requested
/// curation level. Returns the results alongside the [`PreparedQuery`] used,
/// so the caller can inspect/log exactly what was searched (query-preparation
/// transparency).
///
/// `request.min_score` filters on the **fused RRF score** — a relative
/// ranking signal (small positive reciprocals), not a normalized probability;
/// the default `None` applies no floor. Collection scoping (never implicit "search
/// everything") is enforced by delegating to `ann_search`/`fts_search`
/// unconditionally on `request.collection_ids` — an empty list short-circuits
/// to an empty result without touching the store at all. Recall (non-
/// authoritative recall) holds by construction: [`RetrievedChunk`] carries
/// only `(text, source_ref, score)`, never an assertion of correctness.
/// Preparation never alters `source_ref` nor widens `collection_ids`:
/// every query variant searches the exact same `collection_ids`, and
/// attribution comes from the store unchanged.
pub fn retrieve(
    store: &dyn KnowledgeStore,
    embedder: &dyn EmbeddingBackend,
    preparer: Option<&dyn QueryPreparer>,
    request: &RetrievalRequest,
) -> Result<(Vec<RetrievedChunk>, PreparedQuery), RetrievalError> {
    if request.collection_ids.is_empty() || request.top_k == 0 {
        return Ok((Vec::new(), PreparedQuery::identity(&request.query)));
    }

    let prepared = resolve_query(preparer, &request.query);
    let mut queries = vec![prepared.retrieval_query.clone()];
    queries.extend(prepared.subqueries.iter().cloned());

    let over_fetch = request.top_k.saturating_mul(2);
    let mut lists: Vec<Vec<(String, f32)>> = Vec::with_capacity(queries.len() * 2);
    for q in &queries {
        let query_vector = embedder.embed(q).map_err(RetrievalError::Embed)?;
        lists.push(
            store
                .ann_search(&request.collection_ids, &query_vector, over_fetch)
                .map_err(RetrievalError::Store)?,
        );
        lists.push(
            store
                .fts_search(&request.collection_ids, q, over_fetch)
                .map_err(RetrievalError::Store)?,
        );
    }

    let mut fused = rrf_fuse(&lists);
    fused.truncate(request.top_k);
    let score_by_id: HashMap<String, f64> = fused.iter().cloned().collect();

    let chunk_ids: Vec<String> = fused.into_iter().map(|(id, _)| id).collect();
    let hydrated = store
        .hydrate_chunks(&chunk_ids, request.min_curation)
        .map_err(RetrievalError::Store)?;

    let mut results: Vec<RetrievedChunk> = hydrated
        .into_iter()
        .map(|mut c| {
            c.score = score_by_id.get(&c.chunk_id).copied().unwrap_or(0.0) as f32;
            c
        })
        .filter(|c| request.min_score.is_none_or(|floor| c.score >= floor))
        .collect();
    // `hydrate_chunks` may return rows in an arbitrary (e.g. SQL) order and
    // may drop some ids (the curation floor) — re-sort by the fused
    // rank so the caller sees best-match-first regardless.
    results.sort_by(|a, b| b.score.total_cmp(&a.score));
    Ok((results, prepared))
}

/// Fuse any number of ranked (best-first) candidate lists by Reciprocal Rank
/// Fusion: each list contributes `1 / (k + rank)` per appearance (rank
/// 1-indexed), summed per chunk id across every list. Returns pairs sorted
/// by descending fused score.
fn rrf_fuse(lists: &[Vec<(String, f32)>]) -> Vec<(String, f64)> {
    let mut scores: HashMap<String, f64> = HashMap::new();
    for list in lists {
        for (rank, (chunk_id, _)) in list.iter().enumerate() {
            *scores.entry(chunk_id.clone()).or_insert(0.0) += 1.0 / (RRF_K + (rank + 1) as f64);
        }
    }
    let mut fused: Vec<(String, f64)> = scores.into_iter().collect();
    fused.sort_by(|a, b| b.1.total_cmp(&a.1));
    fused
}

#[cfg(test)]
mod tests {
    use super::*;
    use cronus_contract::{
        Chunk, Collection, Curation, Directory, Document, SourceRef, WriteOverride,
    };
    use std::cell::RefCell;

    #[derive(Default)]
    struct ScriptedStore {
        ann: Vec<(String, f32)>,
        fts: Vec<(String, f32)>,
        chunks: Vec<RetrievedChunk>,
        last_ann_collections: RefCell<Vec<String>>,
        last_fts_collections: RefCell<Vec<String>>,
    }

    impl KnowledgeStore for ScriptedStore {
        fn create_collection(&self, _c: &Collection) -> Result<(), String> {
            Ok(())
        }
        fn get_collection(&self, _id: &str) -> Result<Option<Collection>, String> {
            Ok(None)
        }
        fn create_directory(&self, _d: &Directory) -> Result<(), String> {
            Ok(())
        }
        fn write_document(&self, _d: &Document, _o: &WriteOverride) -> Result<(), String> {
            Ok(())
        }
        fn get_document(&self, _id: &str) -> Result<Option<Document>, String> {
            Ok(None)
        }
        fn update_document_status(
            &self,
            _document_id: &str,
            _status: cronus_contract::DocumentStatus,
            _error_msg: Option<&str>,
        ) -> Result<(), String> {
            Ok(())
        }
        fn set_curation(
            &self,
            _id: &str,
            _next: Curation,
            _human_auth: Option<&str>,
        ) -> Result<(), String> {
            Ok(())
        }
        fn soft_delete_document(&self, _id: &str) -> Result<(), String> {
            Ok(())
        }
        fn gc(&self, _older_than_secs: u64) -> Result<u64, String> {
            Ok(0)
        }
        fn delete_chunks(&self, _document_id: &str) -> Result<(), String> {
            Ok(())
        }
        fn insert_chunk(&self, _chunk: &Chunk, _embedding: &[f32]) -> Result<(), String> {
            Ok(())
        }
        fn reindex_chunks(
            &self,
            _document_id: &str,
            _chunks: &[(Chunk, Vec<f32>)],
        ) -> Result<(), String> {
            Ok(())
        }
        fn ann_search(
            &self,
            collection_ids: &[String],
            _query_vector: &[f32],
            _top_k: usize,
        ) -> Result<Vec<(String, f32)>, String> {
            *self.last_ann_collections.borrow_mut() = collection_ids.to_vec();
            Ok(self.ann.clone())
        }
        fn fts_search(
            &self,
            collection_ids: &[String],
            _query_text: &str,
            _top_k: usize,
        ) -> Result<Vec<(String, f32)>, String> {
            *self.last_fts_collections.borrow_mut() = collection_ids.to_vec();
            Ok(self.fts.clone())
        }
        fn hydrate_chunks(
            &self,
            chunk_ids: &[String],
            _min_curation: Option<Curation>,
        ) -> Result<Vec<RetrievedChunk>, String> {
            Ok(self
                .chunks
                .iter()
                .filter(|c| chunk_ids.contains(&c.chunk_id))
                .cloned()
                .collect())
        }
    }

    fn chunk(id: &str, collection_id: &str) -> RetrievedChunk {
        RetrievedChunk {
            chunk_id: id.to_string(),
            document_id: format!("doc-{id}"),
            collection_id: collection_id.to_string(),
            text: format!("text for {id}"),
            source_ref: Some(SourceRef {
                page: Some(1),
                ..Default::default()
            }),
            score: 0.0,
        }
    }

    struct FakeEmbedder;
    impl EmbeddingBackend for FakeEmbedder {
        fn embed(&self, _text: &str) -> Result<Vec<f32>, String> {
            Ok(vec![0.1, 0.2])
        }
    }

    #[test]
    fn retrieve_fuses_ann_and_fts_by_reciprocal_rank() {
        let store = ScriptedStore {
            ann: vec![("a".into(), 0.1), ("b".into(), 0.2)],
            fts: vec![("b".into(), 1.0), ("c".into(), 2.0)],
            chunks: vec![
                chunk("a", "col-1"),
                chunk("b", "col-1"),
                chunk("c", "col-1"),
            ],
            ..Default::default()
        };
        let request = RetrievalRequest::new("query", vec!["col-1".to_string()]);
        let (results, _prepared) =
            retrieve(&store, &FakeEmbedder, None, &request).expect("retrieve");

        // "b" appears in BOTH lists (rank 2 in ann, rank 1 in fts) — its
        // summed reciprocal-rank score beats either single-list top hit.
        assert_eq!(results[0].chunk_id, "b");
        let ids: Vec<&str> = results.iter().map(|c| c.chunk_id.as_str()).collect();
        assert_eq!(ids.len(), 3);
        assert!(ids.contains(&"a"));
        assert!(ids.contains(&"c"));
    }

    #[test]
    fn retrieve_scopes_both_searches_to_the_requested_collections() {
        let store = ScriptedStore::default();
        let request = RetrievalRequest::new("q", vec!["col-a".to_string(), "col-b".to_string()]);
        retrieve(&store, &FakeEmbedder, None, &request).expect("retrieve");
        assert_eq!(
            *store.last_ann_collections.borrow(),
            vec!["col-a".to_string(), "col-b".to_string()]
        );
        assert_eq!(
            *store.last_fts_collections.borrow(),
            vec!["col-a".to_string(), "col-b".to_string()]
        );
    }

    #[test]
    fn no_collection_ids_returns_empty_without_touching_the_store() {
        let store = ScriptedStore {
            ann: vec![("leak".into(), 0.0)],
            chunks: vec![chunk("leak", "col-x")],
            ..Default::default()
        };
        let request = RetrievalRequest::new("q", vec![]);
        let (results, _prepared) =
            retrieve(&store, &FakeEmbedder, None, &request).expect("retrieve");
        assert!(
            results.is_empty(),
            "no implicit search-everything on an empty collection scope"
        );
        assert!(
            store.last_ann_collections.borrow().is_empty(),
            "the store must not even be called"
        );
    }

    #[test]
    fn every_result_carries_source_ref_attribution() {
        let store = ScriptedStore {
            ann: vec![("a".into(), 0.0)],
            chunks: vec![chunk("a", "col-1")],
            ..Default::default()
        };
        let request = RetrievalRequest::new("q", vec!["col-1".to_string()]);
        let (results, _prepared) =
            retrieve(&store, &FakeEmbedder, None, &request).expect("retrieve");
        assert!(results[0].source_ref.is_some());
    }

    #[test]
    fn min_score_filters_out_low_ranked_fused_results() {
        let store = ScriptedStore {
            ann: vec![("a".into(), 0.0), ("b".into(), 0.0), ("c".into(), 0.0)],
            chunks: vec![
                chunk("a", "col-1"),
                chunk("b", "col-1"),
                chunk("c", "col-1"),
            ],
            ..Default::default()
        };
        let mut request = RetrievalRequest::new("q", vec!["col-1".to_string()]);
        request.top_k = 3;
        // Fused scores (ann-only, ranks 1/2/3): 1/61≈0.016393, 1/62≈0.016129,
        // 1/63≈0.015873. A floor of 0.0162 admits only rank 1 ("a").
        request.min_score = Some(0.0162);
        let (results, _prepared) =
            retrieve(&store, &FakeEmbedder, None, &request).expect("retrieve");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].chunk_id, "a");
    }

    #[test]
    fn embed_failure_surfaces_as_a_retrieval_error() {
        struct FailingEmbedder;
        impl EmbeddingBackend for FailingEmbedder {
            fn embed(&self, _text: &str) -> Result<Vec<f32>, String> {
                Err("no model".to_string())
            }
        }
        let store = ScriptedStore::default();
        let request = RetrievalRequest::new("q", vec!["col-1".to_string()]);
        let err = retrieve(&store, &FailingEmbedder, None, &request).expect_err("embed failure");
        assert!(matches!(err, RetrievalError::Embed(_)));
    }

    #[test]
    fn a_chunk_ranked_by_rrf_but_dropped_at_hydration_is_simply_absent() {
        // Simulates hydrate_chunks applying the curation floor: "b" is
        // RRF-ranked but never returned by hydrate_chunks (as if filtered).
        let store = ScriptedStore {
            ann: vec![("a".into(), 0.0), ("b".into(), 0.0)],
            chunks: vec![chunk("a", "col-1")], // "b" deliberately absent
            ..Default::default()
        };
        let request = RetrievalRequest::new("q", vec!["col-1".to_string()]);
        let (results, _prepared) =
            retrieve(&store, &FakeEmbedder, None, &request).expect("retrieve");
        let ids: Vec<&str> = results.iter().map(|c| c.chunk_id.as_str()).collect();
        assert_eq!(ids, vec!["a"]);
    }

    #[test]
    fn unwired_preparer_uses_the_raw_query_unchanged() {
        let store = ScriptedStore {
            ann: vec![("a".into(), 0.0)],
            chunks: vec![chunk("a", "col-1")],
            ..Default::default()
        };
        let request = RetrievalRequest::new("raw query text", vec!["col-1".to_string()]);
        let (_results, prepared) =
            retrieve(&store, &FakeEmbedder, None, &request).expect("retrieve");
        assert_eq!(prepared.retrieval_query, "raw query text");
        assert_eq!(prepared.raw, "raw query text");
        assert!(prepared.subqueries.is_empty());
    }

    struct ExpandingPreparer;
    impl QueryPreparer for ExpandingPreparer {
        fn prepare(&self, raw: &str) -> PreparedQuery {
            PreparedQuery {
                retrieval_query: format!("{raw} expanded"),
                subqueries: vec!["sub one".to_string(), "sub two".to_string()],
                raw: raw.to_string(),
            }
        }
    }

    #[test]
    fn a_wired_preparer_records_both_prepared_and_raw_transparently() {
        let store = ScriptedStore::default();
        let request = RetrievalRequest::new("original", vec!["col-1".to_string()]);
        let (_results, prepared) =
            retrieve(&store, &FakeEmbedder, Some(&ExpandingPreparer), &request).expect("retrieve");
        assert_eq!(prepared.retrieval_query, "original expanded");
        assert_eq!(prepared.raw, "original");
        assert_eq!(prepared.subqueries, vec!["sub one", "sub two"]);
    }

    #[test]
    fn subqueries_are_searched_independently_and_rrf_merged() {
        // Each of the 3 queries (main + 2 subqueries) contributes an
        // independent ann_search call in this fake — but ScriptedStore
        // always returns the SAME configured `ann`/`fts` list regardless of
        // query text, so a chunk appearing in ann's list accumulates RRF
        // score once per query issued. With 3 queries × (ann+fts) = 6 lists
        // all containing "x" at rank 1, "x" must still be a single ranked
        // result, not duplicated.
        let store = ScriptedStore {
            ann: vec![("x".into(), 0.0)],
            fts: vec![("x".into(), 0.0)],
            chunks: vec![chunk("x", "col-1")],
            ..Default::default()
        };
        let request = RetrievalRequest::new("original", vec!["col-1".to_string()]);
        let (results, prepared) =
            retrieve(&store, &FakeEmbedder, Some(&ExpandingPreparer), &request).expect("retrieve");
        assert_eq!(prepared.subqueries.len(), 2);
        assert_eq!(
            results.len(),
            1,
            "the same chunk id across multiple queries is fused, not duplicated"
        );
        assert_eq!(results[0].chunk_id, "x");
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
    fn an_empty_preparation_falls_back_to_the_raw_query_never_an_empty_search() {
        let store = ScriptedStore {
            ann: vec![("a".into(), 0.0)],
            chunks: vec![chunk("a", "col-1")],
            ..Default::default()
        };
        let request = RetrievalRequest::new("fallback me", vec!["col-1".to_string()]);
        let (results, prepared) =
            retrieve(&store, &FakeEmbedder, Some(&EmptyPreparer), &request).expect("retrieve");
        assert_eq!(
            prepared.retrieval_query, "fallback me",
            "an empty preparation must fall back to the raw query"
        );
        assert_eq!(
            results.len(),
            1,
            "the fallback query must still actually search, never an empty result"
        );
    }

    #[test]
    fn preparation_never_widens_the_collection_scope() {
        struct WideningAttemptPreparer;
        impl QueryPreparer for WideningAttemptPreparer {
            fn prepare(&self, raw: &str) -> PreparedQuery {
                // A preparer has no field to name collections at all — its
                // output type structurally cannot widen the collection scope. This
                // test documents that guarantee via the type shape, plus
                // confirms the searched scope matches the request exactly.
                PreparedQuery::identity(raw)
            }
        }
        let store = ScriptedStore::default();
        let request = RetrievalRequest::new("q", vec!["col-1".to_string()]);
        retrieve(
            &store,
            &FakeEmbedder,
            Some(&WideningAttemptPreparer),
            &request,
        )
        .expect("retrieve");
        assert_eq!(
            *store.last_ann_collections.borrow(),
            vec!["col-1".to_string()]
        );
    }
}
