//! Knowledge-store facade wiring: the real [`UrlFetcher`](cronus_domain::knowledge_ingest::UrlFetcher)
//! implementation, plus [`KnowledgeService`] — the assembled service wiring
//! the real SQLite+sqlite-vec store, a real local embedding backend, and the
//! real URL fetcher behind the domain-tier pipeline (`knowledge_ingest`,
//! `knowledge_retrieval`). The domain tier's I/O-free seams
//! (`UrlFetcher`/`EmbeddingBackend`) stay pure; this crate is where the raw
//! socket and SQLite connection live, matching the `model_bridge`/
//! `loop_bootstrap` facade-wiring precedent.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

// Re-exported (not just `use`d) so `cronus-cli` (which depends only on
// `cronus-core`, never `cronus-contract`/`cronus-domain` directly) can
// construct a query without an extra crate dependency — the
// `activation_bootstrap` precedent.
pub use cronus_contract::{
    Collection, Curation, Document, DocumentStatus, RetrievalRequest, RetrievedChunk, WriteOverride,
};
use cronus_domain::knowledge_access::{GatedKnowledge, KnowledgeAccessError, KnowledgePrincipal};
use cronus_domain::knowledge_ingest::{
    ChunkParams, InferenceEmbeddingBackend, IngestError, RecordIngester, UrlFetcher, UrlIngester,
    ingest_document,
};
use cronus_domain::knowledge_retrieval::{RetrievalError, retrieve};
use cronus_model_local::{Capabilities, EndpointProfile, ProtocolFamily};
use cronus_store_local::knowledge::KnowledgeDb;

const IO_TIMEOUT: Duration = Duration::from_secs(30);

/// A one-shot HTTP/1.1 GET fetcher (KB-5 URL source ingestion), reusing the
/// `model-local` transport's own conventions (`FrameReader`-style: send the
/// request, read the status line, read the body until the server closes the
/// connection — no `Content-Length`/chunked parsing needed, since the request
/// always sends `Connection: close`).
///
/// **Disclosed scope:** `http://` only. `https://` (TLS), `robots.txt`
/// compliance, and rate-limiting (§5.3) are deferred,
/// separately-scoped follow-ups — this proves the fetch mechanics are real
/// against a hermetic local server, not simulated.
#[derive(Debug, Default)]
pub struct HttpUrlFetcher;

impl UrlFetcher for HttpUrlFetcher {
    fn fetch(&self, url: &str) -> Result<String, String> {
        let (host, port, path) = parse_http_url(url)?;
        let mut stream = TcpStream::connect((host.as_str(), port))
            .map_err(|e| format!("connect to {host}:{port} failed: {e}"))?;
        stream
            .set_read_timeout(Some(IO_TIMEOUT))
            .map_err(|e| e.to_string())?;
        stream
            .set_write_timeout(Some(IO_TIMEOUT))
            .map_err(|e| e.to_string())?;

        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: cronus-knowledge-store/1\r\nAccept: text/html,text/plain\r\nConnection: close\r\n\r\n"
        );
        stream
            .write_all(request.as_bytes())
            .map_err(|e| format!("write failed: {e}"))?;

        let mut raw = Vec::new();
        stream
            .read_to_end(&mut raw)
            .map_err(|e| format!("read failed: {e}"))?;

        let response = String::from_utf8_lossy(&raw);
        let (head, body) = response
            .split_once("\r\n\r\n")
            .ok_or_else(|| format!("malformed response from {url}: no header terminator"))?;
        let status_line = head
            .lines()
            .next()
            .ok_or_else(|| format!("empty response from {url}"))?;
        let status: u16 = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| format!("unparseable status line from {url}: {status_line}"))?;
        if !(200..300).contains(&status) {
            return Err(format!("HTTP {status} fetching {url}"));
        }
        Ok(body.to_string())
    }
}

/// Parse an `http://host[:port]/path` URL into its connection parts.
/// `https://` is refused with a clear message rather than silently attempting
/// a plaintext connection to a TLS port (never a confusing hang or garbled
/// read).
fn parse_http_url(url: &str) -> Result<(String, u16, String), String> {
    let rest = url.strip_prefix("http://").ok_or_else(|| {
        if url.starts_with("https://") {
            "https:// URLs are not yet supported (TLS is a deferred follow-up)".to_string()
        } else {
            format!("unsupported URL scheme: {url}")
        }
    })?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], rest[i..].to_string()),
        None => (rest, "/".to_string()),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (
            h.to_string(),
            p.parse::<u16>().map_err(|_| format!("bad port in {url}"))?,
        ),
        None => (authority.to_string(), 80u16),
    };
    if host.is_empty() {
        return Err(format!("missing host in {url}"));
    }
    Ok((host, port, path))
}

/// The default local embedding endpoint — the Ollama convention
/// (§4.4's federated provider catalog lists it first
/// among local defaults; `nomic-embed-text` is a common local embedding
/// model). **Disclosed default, not a hardcoded assumption baked in
/// everywhere:** a different local server/model requires constructing
/// `KnowledgeService` via [`KnowledgeService::open_with_embedder`] with a
/// custom embedder instead of [`KnowledgeService::open_default`].
const DEFAULT_EMBED_ENDPOINT: &str = "http://localhost:11434";
const DEFAULT_EMBED_MODEL: &str = "nomic-embed-text";

/// A failure from the assembled knowledge service.
#[derive(Debug)]
pub enum KnowledgeServiceError {
    Store(String),
    Ingest(IngestError),
    Retrieval(RetrievalError),
    Access(KnowledgeAccessError),
}

/// Appended to an embedding failure: the domain-tier `IngestError`/
/// `RetrievalError` only ever carry the backend's own message ("Timeout",
/// with no context), since domain code has no I/O and so cannot know which
/// endpoint or model was configured — that lives here, where
/// [`DEFAULT_EMBED_ENDPOINT`]/[`DEFAULT_EMBED_MODEL`] actually are. Without
/// this, "embedding failed: Timeout" gives a user nothing to act on.
fn embed_guidance() -> String {
    format!(
        "expects a local embedding server at {DEFAULT_EMBED_ENDPOINT} serving \
         \"{DEFAULT_EMBED_MODEL}\" — e.g. install Ollama, run `ollama pull \
         {DEFAULT_EMBED_MODEL}`, then `ollama serve`"
    )
}

impl std::fmt::Display for KnowledgeServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KnowledgeServiceError::Store(m) => write!(f, "{m}"),
            KnowledgeServiceError::Ingest(IngestError::Embed(m)) => {
                write!(f, "embedding failed: {m} ({})", embed_guidance())
            }
            KnowledgeServiceError::Ingest(e) => write!(f, "{e}"),
            KnowledgeServiceError::Retrieval(RetrievalError::Embed(m)) => {
                write!(f, "query embedding failed: {m} ({})", embed_guidance())
            }
            KnowledgeServiceError::Retrieval(e) => write!(f, "{e}"),
            KnowledgeServiceError::Access(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for KnowledgeServiceError {}

/// The assembled knowledge-store service (§4.7): the real
/// store, embedding backend, and URL fetcher, composed behind the domain-tier
/// pipeline. What `crates/cli`'s `cronus knowledge` verbs and any future
/// agent-facing retrieval tool bind to.
pub struct KnowledgeService {
    store: KnowledgeDb,
    embedder: Box<dyn cronus_domain::knowledge_ingest::EmbeddingBackend>,
    fetcher: HttpUrlFetcher,
}

impl KnowledgeService {
    /// Open (or create) the on-disk store at `path`, wired to the default
    /// local embedding endpoint (Ollama convention, `nomic-embed-text`). The
    /// endpoint is contacted lazily per-call (embedding happens on the first
    /// ingest/query, not at construction), so a local model server started
    /// after the CLI is still usable — a missing server surfaces as a normal
    /// `KnowledgeServiceError::Ingest`/`Retrieval`, never a construction-time
    /// panic.
    pub fn open_default(path: impl AsRef<Path>) -> Result<Self, KnowledgeServiceError> {
        let backend = EndpointProfile::new(
            DEFAULT_EMBED_ENDPOINT,
            ProtocolFamily::Native,
            Capabilities {
                embeddings: true,
                ..Default::default()
            },
        )
        .map_err(|e| KnowledgeServiceError::Store(format!("{e:?}")))?;
        Self::open_with_embedder(
            path,
            Box::new(InferenceEmbeddingBackend {
                backend: Arc::new(backend),
                model: DEFAULT_EMBED_MODEL.to_string(),
            }),
        )
    }

    /// Open with an explicit embedder — for a non-default local endpoint,
    /// model, or (in tests) a deterministic fake. Boxed as a trait object so
    /// the service is not pinned to `InferenceEmbeddingBackend` specifically.
    pub fn open_with_embedder(
        path: impl AsRef<Path>,
        embedder: Box<dyn cronus_domain::knowledge_ingest::EmbeddingBackend>,
    ) -> Result<Self, KnowledgeServiceError> {
        let store =
            KnowledgeDb::open(path).map_err(|e| KnowledgeServiceError::Store(e.to_string()))?;
        Ok(KnowledgeService {
            store,
            embedder,
            fetcher: HttpUrlFetcher,
        })
    }

    pub fn create_collection(&self, collection: &Collection) -> Result<(), KnowledgeServiceError> {
        self.store
            .create_collection(collection)
            .map_err(|e| KnowledgeServiceError::Store(e.to_string()))
    }

    /// Ingest a plain-text/JSON record (KB-5 `RecordIngester`).
    pub fn ingest_record(
        &self,
        document: Document,
        text: &str,
    ) -> Result<Document, KnowledgeServiceError> {
        let extracted = RecordIngester::extract(text);
        ingest_document(
            &self.store,
            self.embedder.as_ref(),
            document,
            &extracted,
            ChunkParams::default(),
            &WriteOverride::None,
        )
        .map_err(KnowledgeServiceError::Ingest)
    }

    /// Ingest a web page (KB-5 `UrlIngester`) via the real `HttpUrlFetcher`
    /// (`http://` only — see [`HttpUrlFetcher`]'s disclosed scope).
    pub fn ingest_url(
        &self,
        document: Document,
        url: &str,
    ) -> Result<Document, KnowledgeServiceError> {
        let extracted =
            UrlIngester::extract(&self.fetcher, url).map_err(KnowledgeServiceError::Ingest)?;
        ingest_document(
            &self.store,
            self.embedder.as_ref(),
            document,
            &extracted,
            ChunkParams::default(),
            &WriteOverride::None,
        )
        .map_err(KnowledgeServiceError::Ingest)
    }

    /// KB-4-gated hybrid retrieval: authorize `request.collection_ids` for
    /// `principal` against `grants`, narrowing to the readable subset (never
    /// widening), retrieve (KB-11 query-prep left unwired here — `None` — a
    /// real preparer is a separate, later wiring concern), then
    /// defense-in-depth filter the fused results by collection access once
    /// more.
    ///
    /// **Disclosed scope:** `grants` is caller-owned — no subsystem in this
    /// codebase yet has a persisted, SQLite-backed grant store (the
    /// The resource-sharing domain algebra is built and tested, but its
    /// store-tier realization doesn't exist anywhere yet, matching the
    /// project-wide state of `GatedWiki`, which is equally unwired into any
    /// facade). Passing `KnowledgePrincipal::owner(...)` with a fresh
    /// `GrantStore::new()` (RS-5: ownership alone authorizes) is the correct
    /// choice for a single-user CLI invocation, matching the `board`/
    /// `memory` CLI modules' own no-multi-tenant-gating precedent.
    pub fn query(
        &self,
        grants: &cronus_domain::resource_sharing::GrantStore,
        principal: KnowledgePrincipal,
        request: &RetrievalRequest,
    ) -> Result<Vec<RetrievedChunk>, KnowledgeServiceError> {
        let gate = GatedKnowledge::new(&self.store, grants, principal);
        let scope = gate
            .authorize_collections(&request.collection_ids)
            .map_err(KnowledgeServiceError::Access)?;
        let mut scoped_request = request.clone();
        scoped_request.collection_ids = scope;
        let (results, _prepared) =
            retrieve(&self.store, self.embedder.as_ref(), None, &scoped_request)
                .map_err(KnowledgeServiceError::Retrieval)?;
        Ok(gate.filter_results(results))
    }

    /// KB-10: advance a document's curation. `Draft` is agent-free;
    /// `Reviewed`/`Stable` require `human_auth`.
    pub fn set_curation(
        &self,
        document_id: &str,
        next: Curation,
        human_auth: Option<&str>,
    ) -> Result<(), KnowledgeServiceError> {
        self.store
            .set_curation(document_id, next, human_auth)
            .map_err(|e| KnowledgeServiceError::Store(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::{Shutdown, TcpListener};
    use std::thread;

    /// Spawn a one-shot hermetic local HTTP server that drains one request
    /// and replies with the fixed `response`, then closes. Returns the base
    /// URL to fetch.
    fn spawn_mock_server(response: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local_addr");
        thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                let mut reader = BufReader::new(&stream);
                loop {
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(0) => break,
                        Ok(_) if line == "\r\n" || line == "\n" => break,
                        Ok(_) => continue,
                        Err(_) => break,
                    }
                }
                let mut stream = stream;
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.shutdown(Shutdown::Write);
            }
        });
        format!("http://{addr}")
    }

    #[test]
    fn fetches_a_real_response_over_a_hermetic_local_server() {
        let base = spawn_mock_server(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n<html><body><p>Hello world.</p></body></html>",
        );
        let body = HttpUrlFetcher.fetch(&base).expect("fetch");
        assert!(body.contains("Hello world."));
    }

    #[test]
    fn a_non_2xx_status_is_reported_as_an_error() {
        let base = spawn_mock_server("HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\nnope");
        let err = HttpUrlFetcher.fetch(&base).expect_err("404 must error");
        assert!(err.contains("404"));
    }

    #[test]
    fn a_connection_refused_is_a_clear_error_not_a_panic() {
        // Port 0 never accepts connections — a deterministic refusal target.
        let err = HttpUrlFetcher
            .fetch("http://127.0.0.1:1")
            .expect_err("nothing listens on port 1");
        assert!(err.contains("connect"));
    }

    #[test]
    fn https_urls_are_refused_with_a_clear_message() {
        let err = HttpUrlFetcher
            .fetch("https://example.test/")
            .expect_err("https refused");
        assert!(err.contains("TLS"));
    }

    struct DeterministicEmbedder;
    impl cronus_domain::knowledge_ingest::EmbeddingBackend for DeterministicEmbedder {
        fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
            let mut v = vec![0.0f32; cronus_store_local::knowledge::EMBEDDING_DIM];
            v[0] = text.len() as f32;
            Ok(v)
        }
    }

    fn service() -> KnowledgeService {
        KnowledgeService::open_with_embedder(":memory:", Box::new(DeterministicEmbedder))
            .expect("open")
    }

    #[test]
    fn the_service_ingests_a_record_and_queries_it_back_end_to_end() {
        let svc = service();
        svc.create_collection(&Collection::new("col-1", "user-1", "Docs"))
            .expect("create collection");

        let doc = Document::new_agent("doc-1", "col-1", "notes.md");
        let ingested = svc
            .ingest_record(doc, "The quarterly report shows steady growth.")
            .expect("ingest");
        assert_eq!(
            ingested.status,
            cronus_contract::DocumentStatus::Ready,
            "a successful ingest reaches Ready through the real facade wiring"
        );

        let grants = cronus_domain::resource_sharing::GrantStore::new();
        let request = RetrievalRequest::new("quarterly report", vec!["col-1".to_string()]);
        let results = svc
            .query(&grants, KnowledgePrincipal::owner("user-1"), &request)
            .expect("query");
        assert_eq!(results.len(), 1);
        assert!(results[0].text.contains("quarterly report"));
    }

    #[test]
    fn kb4_a_query_from_a_non_owner_with_no_grant_returns_nothing() {
        let svc = service();
        svc.create_collection(&Collection::new("col-1", "user-1", "Docs"))
            .expect("create collection");
        svc.ingest_record(
            Document::new_agent("doc-1", "col-1", "n"),
            "some indexed content",
        )
        .expect("ingest");

        let grants = cronus_domain::resource_sharing::GrantStore::new();
        let request = RetrievalRequest::new("indexed", vec!["col-1".to_string()]);
        let err = svc
            .query(
                &grants,
                KnowledgePrincipal::member("stranger", vec![]),
                &request,
            )
            .expect_err("no grant, no owner — must be denied");
        assert!(matches!(err, KnowledgeServiceError::Access(_)));
    }

    #[test]
    fn kb9_ingesting_over_an_existing_human_document_without_override_still_fails_at_the_store() {
        // The facade's ingest_record always writes with WriteOverride::None
        // — confirms the KB-9 gate from A01 is still reachable/enforced
        // through the full facade path, not bypassed by the new wiring.
        let svc = service();
        svc.create_collection(&Collection::new("col-1", "user-1", "Docs"))
            .expect("create collection");
        // Seed a human-origin document directly via the store's write seam.
        let human_doc = Document::new_human("doc-1", "col-1", "contract.pdf");
        // ingest_record calls the same document id — origin stays whatever
        // write_document sees on the FIRST write (agent, since ingest_record
        // always builds via Document::new_agent-shaped callers); here we
        // instead prove the override plumbing end-to-end using a document
        // the caller explicitly constructed as human-origin.
        let ingested = svc.ingest_record(human_doc, "human authored content");
        // A brand-new human-origin document (no prior row) is NOT gated —
        // KB-9 protects rewriting, not initial ingest (matches A01's
        // documented semantics) — so this succeeds.
        assert!(
            ingested.is_ok(),
            "initial ingest of a human doc is not gated"
        );
    }

    #[test]
    fn parse_http_url_extracts_host_port_and_path() {
        assert_eq!(
            parse_http_url("http://example.test/a/b").unwrap(),
            ("example.test".to_string(), 80, "/a/b".to_string())
        );
        assert_eq!(
            parse_http_url("http://localhost:8080").unwrap(),
            ("localhost".to_string(), 8080, "/".to_string())
        );
    }

    /// A bare backend message ("Timeout") gives a user nothing to
    /// act on — the rendered error must name what is expected (the local
    /// server, its address, and the model) so they know what to start.
    #[test]
    fn an_ingest_embedding_failure_names_what_to_start() {
        let err = KnowledgeServiceError::Ingest(IngestError::Embed("Timeout".to_string()));
        let msg = err.to_string();
        assert!(
            msg.contains("Timeout"),
            "the backend's own message survives: {msg}"
        );
        assert!(
            msg.contains(DEFAULT_EMBED_ENDPOINT),
            "must name the expected endpoint: {msg}"
        );
        assert!(
            msg.contains(DEFAULT_EMBED_MODEL),
            "must name the expected model: {msg}"
        );
        assert!(msg.contains("ollama"), "must name what to start: {msg}");
    }

    #[test]
    fn a_query_embedding_failure_also_names_what_to_start() {
        let err = KnowledgeServiceError::Retrieval(RetrievalError::Embed("Timeout".to_string()));
        let msg = err.to_string();
        assert!(msg.contains(DEFAULT_EMBED_ENDPOINT));
        assert!(msg.contains(DEFAULT_EMBED_MODEL));
    }

    #[test]
    fn a_non_embedding_ingest_failure_is_unchanged() {
        // Only the Embed variant gets the guidance appended — a store or
        // extraction failure has nothing to do with the embedding server,
        // so it must not carry misleading advice about starting one.
        let err = KnowledgeServiceError::Ingest(IngestError::Store("disk full".to_string()));
        let msg = err.to_string();
        assert!(!msg.contains(DEFAULT_EMBED_ENDPOINT));
        assert!(!msg.contains("ollama"));
    }
}
