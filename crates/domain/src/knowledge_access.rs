//! Access-gated knowledge-collection reads.
//!
//! The client never touches [`cronus_contract::KnowledgeStore`] retrieval
//! directly on a shared collection — it goes through [`GatedKnowledge`],
//! which runs the uniform `access-grants` check
//! (`has_access(Knowledge, collection_id, Read)`) **before every query** and
//! only then delegates. This is the `GatedWiki` precedent
//! applied to the same uniform grant model.

use cronus_contract::{KnowledgeStore, RetrievedChunk};

use crate::resource_sharing::{GrantStore, Permission, ResourceKind};

/// The caller's identity for a knowledge query. `is_owner`
/// short-circuits the grant lookup (the owner always reads their own
/// collection); `groups` are the caller's pre-resolved group memberships.
#[derive(Debug, Clone)]
pub struct KnowledgePrincipal {
    pub user_id: String,
    pub is_owner: bool,
    pub groups: Vec<String>,
}

impl KnowledgePrincipal {
    pub fn member(user_id: impl Into<String>, groups: Vec<String>) -> Self {
        KnowledgePrincipal {
            user_id: user_id.into(),
            is_owner: false,
            groups,
        }
    }

    pub fn owner(user_id: impl Into<String>) -> Self {
        KnowledgePrincipal {
            user_id: user_id.into(),
            is_owner: true,
            groups: Vec::new(),
        }
    }
}

/// A query that was refused or failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KnowledgeAccessError {
    /// The caller holds no `Read` grant on this collection. Distinct
    /// from "empty result" so a denial is never silently indistinguishable
    /// from a genuine no-match.
    Denied {
        collection_id: String,
    },
    Store(String),
}

impl std::fmt::Display for KnowledgeAccessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KnowledgeAccessError::Denied { collection_id } => {
                write!(f, "knowledge read denied for collection {collection_id}")
            }
            KnowledgeAccessError::Store(m) => write!(f, "knowledge query failed: {m}"),
        }
    }
}

impl std::error::Error for KnowledgeAccessError {}

/// An access-gated view over a [`KnowledgeStore`]. Checks the `Read`
/// grant for **every** requested `collection_id` before delegating; a
/// collection the caller cannot read is dropped from the query rather than
/// failing the whole request — so a multi-collection query partially
/// authorized still returns results from the collections the caller *can*
/// read, never leaking rows from ones they cannot. A query naming ONLY
/// unauthorized collections is denied outright.
pub struct GatedKnowledge<'a> {
    inner: &'a dyn KnowledgeStore,
    grants: &'a GrantStore,
    principal: KnowledgePrincipal,
}

impl<'a> GatedKnowledge<'a> {
    pub fn new(
        inner: &'a dyn KnowledgeStore,
        grants: &'a GrantStore,
        principal: KnowledgePrincipal,
    ) -> Self {
        GatedKnowledge {
            inner,
            grants,
            principal,
        }
    }

    fn authorized(&self, collection_id: &str) -> bool {
        self.grants.has_access(
            &self.principal.user_id,
            self.principal.is_owner,
            ResourceKind::Knowledge,
            collection_id,
            Permission::Read,
            &self.principal.groups,
        )
    }

    /// Filter `collection_ids` down to the ones this principal may read.
    fn authorized_scope(&self, collection_ids: &[String]) -> Vec<String> {
        collection_ids
            .iter()
            .filter(|id| self.authorized(id))
            .cloned()
            .collect()
    }

    /// The store's `KnowledgeStore` trait (for ingestion/write callers that
    /// already hold their own authorization discipline). Exposed narrowly so
    /// the pipeline in `knowledge_ingest`/`knowledge_retrieval` can still be
    /// composed directly by trusted callers (the office/agent runtime),
    /// while a less-trusted or multi-tenant caller uses the gated `query`
    /// entry point below.
    pub fn inner(&self) -> &'a dyn KnowledgeStore {
        self.inner
    }

    /// The read-scoped `collection_ids` this principal may query — the
    /// primitive `knowledge_retrieval::retrieve` should be called with.
    /// Empty when the principal is authorized for none of the requested ids
    /// (never search collections implicitly, never leak via a partial
    /// grant on an unrelated id).
    pub fn authorize_collections(
        &self,
        requested: &[String],
    ) -> Result<Vec<String>, KnowledgeAccessError> {
        let scope = self.authorized_scope(requested);
        if scope.is_empty() && !requested.is_empty() {
            return Err(KnowledgeAccessError::Denied {
                collection_id: requested.join(","),
            });
        }
        Ok(scope)
    }

    /// A single document by id, scoped to the given (already access-checked)
    /// collection — `None` if it belongs to a different collection than
    /// requested or the caller lacks `Read` on that collection.
    pub fn document(
        &self,
        collection_id: &str,
        document_id: &str,
    ) -> Result<Option<cronus_contract::Document>, KnowledgeAccessError> {
        if !self.authorized(collection_id) {
            return Err(KnowledgeAccessError::Denied {
                collection_id: collection_id.to_string(),
            });
        }
        let doc = self
            .inner
            .get_document(document_id)
            .map_err(KnowledgeAccessError::Store)?;
        Ok(doc.filter(|d| d.collection_id == collection_id))
    }

    /// Post-filter already-fused retrieval results (from
    /// `knowledge_retrieval::retrieve`, called with `authorize_collections`'s
    /// output as its `collection_ids`) — defense-in-depth: even if a caller
    /// mis-scoped the request, a result belonging to an unauthorized
    /// collection never reaches the caller through this gate.
    pub fn filter_results(&self, results: Vec<RetrievedChunk>) -> Vec<RetrievedChunk> {
        results
            .into_iter()
            .filter(|c| self.authorized(&c.collection_id))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource_sharing::{AccessGrant, PrincipalKind};
    use cronus_contract::SourceRef;

    fn read_grant(user: &str, collection_id: &str) -> AccessGrant {
        AccessGrant {
            resource_type: ResourceKind::Knowledge,
            resource_id: collection_id.to_string(),
            principal_type: PrincipalKind::User,
            principal_id: user.to_string(),
            permission: Permission::Read,
        }
    }

    struct NoopStore;
    impl KnowledgeStore for NoopStore {
        fn create_collection(&self, _c: &cronus_contract::Collection) -> Result<(), String> {
            Ok(())
        }
        fn get_collection(&self, _id: &str) -> Result<Option<cronus_contract::Collection>, String> {
            Ok(None)
        }
        fn create_directory(&self, _d: &cronus_contract::Directory) -> Result<(), String> {
            Ok(())
        }
        fn write_document(
            &self,
            _d: &cronus_contract::Document,
            _o: &cronus_contract::WriteOverride,
        ) -> Result<(), String> {
            Ok(())
        }
        fn get_document(&self, id: &str) -> Result<Option<cronus_contract::Document>, String> {
            if id == "doc-1" {
                Ok(Some(cronus_contract::Document::new_agent(
                    "doc-1", "col-1", "n",
                )))
            } else {
                Ok(None)
            }
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
            _next: cronus_contract::Curation,
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
        fn insert_chunk(
            &self,
            _chunk: &cronus_contract::Chunk,
            _embedding: &[f32],
        ) -> Result<(), String> {
            Ok(())
        }
        fn reindex_chunks(
            &self,
            _document_id: &str,
            _chunks: &[(cronus_contract::Chunk, Vec<f32>)],
        ) -> Result<(), String> {
            Ok(())
        }
        fn ann_search(
            &self,
            _c: &[String],
            _q: &[f32],
            _k: usize,
        ) -> Result<Vec<(String, f32)>, String> {
            Ok(Vec::new())
        }
        fn fts_search(
            &self,
            _c: &[String],
            _q: &str,
            _k: usize,
        ) -> Result<Vec<(String, f32)>, String> {
            Ok(Vec::new())
        }
        fn hydrate_chunks(
            &self,
            _ids: &[String],
            _min_curation: Option<cronus_contract::Curation>,
        ) -> Result<Vec<RetrievedChunk>, String> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn a_read_without_a_grant_is_denied() {
        let store = NoopStore;
        let grants = GrantStore::new();
        let gate = GatedKnowledge::new(
            &store,
            &grants,
            KnowledgePrincipal::member("stranger", vec![]),
        );

        assert_eq!(
            gate.authorize_collections(&["col-1".to_string()]),
            Err(KnowledgeAccessError::Denied {
                collection_id: "col-1".to_string()
            })
        );
    }

    #[test]
    fn a_direct_read_grant_opens_the_collection() {
        let store = NoopStore;
        let mut grants = GrantStore::new();
        grants.add(read_grant("alice", "col-1"));
        let gate =
            GatedKnowledge::new(&store, &grants, KnowledgePrincipal::member("alice", vec![]));

        assert_eq!(
            gate.authorize_collections(&["col-1".to_string()]),
            Ok(vec!["col-1".to_string()])
        );
    }

    #[test]
    fn the_owner_reads_without_an_explicit_grant() {
        let store = NoopStore;
        let grants = GrantStore::new();
        let gate = GatedKnowledge::new(&store, &grants, KnowledgePrincipal::owner("alice"));
        assert_eq!(
            gate.authorize_collections(&["col-1".to_string()]),
            Ok(vec!["col-1".to_string()])
        );
    }

    #[test]
    fn a_multi_collection_request_is_scoped_to_only_the_authorized_ones() {
        let store = NoopStore;
        let mut grants = GrantStore::new();
        grants.add(read_grant("alice", "col-1"));
        let gate =
            GatedKnowledge::new(&store, &grants, KnowledgePrincipal::member("alice", vec![]));

        // Authorized for col-1 only; col-2 is silently dropped, not leaked
        // and not a hard failure (the request still returns col-1's scope).
        let scope = gate
            .authorize_collections(&["col-1".to_string(), "col-2".to_string()])
            .expect("partially authorized");
        assert_eq!(scope, vec!["col-1".to_string()]);
    }

    #[test]
    fn a_cross_collection_document_id_is_reported_absent_not_leaked() {
        let store = NoopStore;
        let mut grants = GrantStore::new();
        grants.add(read_grant("alice", "col-2"));
        let gate =
            GatedKnowledge::new(&store, &grants, KnowledgePrincipal::member("alice", vec![]));

        // Authorized for col-2, but "doc-1" belongs to col-1 — reported
        // absent, never the other collection's document.
        assert_eq!(
            gate.document("col-2", "doc-1").expect("read"),
            None,
            "a document from another collection is not visible through this gate"
        );
    }

    fn chunk(id: &str, collection_id: &str) -> RetrievedChunk {
        RetrievedChunk {
            chunk_id: id.to_string(),
            document_id: format!("doc-{id}"),
            collection_id: collection_id.to_string(),
            text: "t".to_string(),
            source_ref: Some(SourceRef::default()),
            score: 0.0,
        }
    }

    #[test]
    fn filter_results_drops_rows_from_unauthorized_collections_as_defense_in_depth() {
        let store = NoopStore;
        let mut grants = GrantStore::new();
        grants.add(read_grant("alice", "col-1"));
        let gate =
            GatedKnowledge::new(&store, &grants, KnowledgePrincipal::member("alice", vec![]));

        let results = vec![chunk("a", "col-1"), chunk("b", "col-2")];
        let filtered = gate.filter_results(results);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].collection_id, "col-1");
    }
}
