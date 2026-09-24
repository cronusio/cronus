//! Knowledge-store ingestion pipeline.
//!
//! Chunk a source document, embed each chunk, and write the whole set
//! transactionally through the [`KnowledgeStore`] seam. Domain-logic-first:
//! chunking is a pure function; embedding is abstracted behind
//! [`EmbeddingBackend`] so the pipeline is testable against a deterministic
//! fake without a live model (the generator-optional precedent from
//! `wiki_regen`). File/Record source extraction lives here too; the
//! URL adapter is a later task (the third source type).

use std::sync::atomic::{AtomicU64, Ordering};

use cronus_contract::{Chunk, Document, DocumentStatus, KnowledgeStore, WriteOverride, now_secs};

use crate::file_store::FileStore;

/// A seam over embedding generation — a thin wrapper over
/// `contract::InferenceBackend::embed` bound to one model, so ingestion is
/// testable against a deterministic fake with no live model.
pub trait EmbeddingBackend {
    fn embed(&self, text: &str) -> Result<Vec<f32>, String>;
}

/// Adapts a `contract::InferenceBackend` (bound to one model name) into an
/// [`EmbeddingBackend`]. The facade wires this at the adapter boundary; this
/// crate never depends on `cronus-model-local` directly (the tier model has
/// no such edge). Holds the backend behind an `Arc` (the `NodusModelBridge`
/// convention) so a facade-owned service can hold both the concrete backend
/// and this adapter without a self-referential borrow.
pub struct InferenceEmbeddingBackend {
    pub backend: std::sync::Arc<dyn cronus_contract::InferenceBackend>,
    pub model: String,
}

impl EmbeddingBackend for InferenceEmbeddingBackend {
    fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
        self.backend
            .embed(&self.model, text)
            .map_err(|e| format!("{e:?}"))
    }
}

/// A failure at any ingestion stage. The document is left in `Error` status
/// with a diagnostic message rather than losing its prior chunks:
/// "the collection remains queryable from its prior
/// state".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngestError {
    /// Source text could not be extracted.
    Extract(String),
    /// The embedding backend refused or failed on a chunk.
    Embed(String),
    /// The knowledge store rejected the write (e.g. a read-only zone).
    Store(String),
}

impl std::fmt::Display for IngestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IngestError::Extract(m) => write!(f, "extraction failed: {m}"),
            IngestError::Embed(m) => write!(f, "embedding failed: {m}"),
            IngestError::Store(m) => write!(f, "store write failed: {m}"),
        }
    }
}

impl std::error::Error for IngestError {}

/// Chunking parameters (defaults: 512 tokens / 64
/// overlap, approximated by whitespace-word count — see [`chunk_text`]).
#[derive(Debug, Clone, Copy)]
pub struct ChunkParams {
    pub chunk_size: usize,
    pub overlap: usize,
}

impl Default for ChunkParams {
    fn default() -> Self {
        ChunkParams {
            chunk_size: 512,
            overlap: 64,
        }
    }
}

/// Split `text` into overlapping chunks, never cutting mid-sentence.
///
/// **Disclosed simplification:** sentences are approximated by scanning for
/// `.`/`!`/`?` followed by whitespace-or-end-of-text over Unicode scalar
/// values — this is a word-boundary heuristic, not full Unicode UAX#29
/// sentence segmentation. `chunk_size`/`overlap` are approximated by
/// whitespace-delimited word count, not a real tokenizer count. A single
/// sentence longer than `chunk_size` is kept whole rather than split
/// mid-sentence, so it may exceed the target size — the sentence-boundary
/// invariant always wins over the exact size target.
pub fn chunk_text(text: &str, params: ChunkParams) -> Vec<String> {
    let sentences = split_into_sentences(text);
    if sentences.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let mut current_words = 0usize;

    for sentence in &sentences {
        let words: Vec<&str> = sentence.split_whitespace().collect();
        if !current.is_empty() && current_words + words.len() > params.chunk_size {
            chunks.push(current.join(" "));
            let keep = current
                .len()
                .saturating_sub(params.overlap.min(current.len()));
            current = current[keep..].to_vec();
            current_words = current.len();
        }
        current.extend(words.iter());
        current_words += words.len();
    }
    if !current.is_empty() {
        chunks.push(current.join(" "));
    }
    chunks
}

fn split_into_sentences(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut sentences = Vec::new();
    let mut start = 0usize;
    for i in 0..chars.len() {
        if matches!(chars[i], '.' | '!' | '?') {
            let boundary = i + 1 >= chars.len() || chars[i + 1].is_whitespace();
            if boundary {
                let sentence: String = chars[start..=i].iter().collect();
                let trimmed = sentence.trim();
                if !trimmed.is_empty() {
                    sentences.push(trimmed.to_string());
                }
                start = i + 1;
            }
        }
    }
    if start < chars.len() {
        let rest: String = chars[start..].iter().collect();
        let trimmed = rest.trim();
        if !trimmed.is_empty() {
            sentences.push(trimmed.to_string());
        }
    }
    sentences
}

/// Extracts plain text from an uploaded file.
pub struct FileIngester;

impl FileIngester {
    /// **Disclosed simplification:** reads the blob as UTF-8 text directly.
    /// Real binary-format extraction (PDF/DOCX/HTML→text) is out of this
    /// task's scope, matching the pipeline's own three-adapter split — the
    /// URL adapter's HTML→text conversion is a later, separately-scoped task.
    pub fn extract(file_store: &FileStore, file_id: &str) -> Result<String, IngestError> {
        let hash = file_store
            .hash_of(file_id)
            .ok_or_else(|| IngestError::Extract(format!("no such file: {file_id}")))?;
        let bytes = file_store
            .read(hash)
            .ok_or_else(|| IngestError::Extract(format!("blob missing for file: {file_id}")))?;
        String::from_utf8(bytes.to_vec())
            .map_err(|e| IngestError::Extract(format!("not valid UTF-8 text: {e}")))
    }
}

/// Extracts plain text from a plain-text or JSON record. A record is
/// already textual, so this is a passthrough; flattening structured JSON
/// into prose (if ever needed) is the caller's concern, not ingestion's.
pub struct RecordIngester;

impl RecordIngester {
    pub fn extract(record: &str) -> String {
        record.to_string()
    }
}

/// A seam over a one-shot HTTP fetch (I/O). The domain tier stays I/O-free —
/// this trait is implemented by a concrete adapter in the facade
/// (`cronus-core`); domain code only composes over it.
pub trait UrlFetcher {
    /// Fetch `url`, returning the raw response body (e.g. HTML) on success.
    fn fetch(&self, url: &str) -> Result<String, String>;
}

/// Extracts plain text from a fetched web page.
pub struct UrlIngester;

impl UrlIngester {
    /// Fetch `url` via `fetcher`, then strip HTML down to plain text.
    ///
    /// **Disclosed scope:** the production `fetcher` performs a plain
    /// HTTP/1.1 GET — `https://` (TLS), `robots.txt` compliance, and
    /// rate-limiting are deferred, separately-
    /// scoped follow-ups. This proves the fetch→extract mechanics are real
    /// against a hermetic local server, not simulated.
    pub fn extract(fetcher: &dyn UrlFetcher, url: &str) -> Result<String, IngestError> {
        let html = fetcher.fetch(url).map_err(IngestError::Extract)?;
        Ok(html_to_text(&html))
    }
}

/// A minimal HTML→text conversion: drops `<script>`/`<style>` block
/// contents, strips remaining tags, decodes the handful of named/numeric
/// entities that show up in ordinary web prose, and collapses whitespace.
///
/// **Disclosed simplification:** not a full HTML5 parser — malformed markup
/// is not corrected, and only the entities listed in [`decode_entities`] are
/// decoded.
pub fn html_to_text(html: &str) -> String {
    let without_scripts = strip_tag_block(html, "script");
    let without_styles = strip_tag_block(&without_scripts, "style");
    let stripped = strip_tags(&without_styles);
    let decoded = decode_entities(&stripped);
    collapse_whitespace(&decoded)
}

/// Drop every `<tag ...>...</tag>` block, contents included (case-insensitive
/// tag matching). An unterminated opening tag drops the remainder of the
/// input rather than looping — malformed markup degrades safely.
fn strip_tag_block(input: &str, tag: &str) -> String {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    loop {
        let lower = rest.to_ascii_lowercase();
        match lower.find(&open) {
            Some(start) => {
                out.push_str(&rest[..start]);
                match lower[start..].find(&close) {
                    Some(end_rel) => rest = &rest[start + end_rel + close.len()..],
                    None => return out, // unterminated block: drop the remainder
                }
            }
            None => {
                out.push_str(rest);
                return out;
            }
        }
    }
}

fn strip_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

fn decode_entities(input: &str) -> String {
    input
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
}

fn collapse_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

static CHUNK_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A globally-unique chunk id (the `contract::MemoryId` counter idiom, local
/// to this module since one document's ingestion mints many chunk ids at
/// once and the caller cannot know the count in advance).
fn next_chunk_id(document_id: &str) -> String {
    let n = CHUNK_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("chk_{document_id}_{:016x}_{n:08x}", now_secs())
}

/// The outcome of a completed ingestion run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestReport {
    pub document_id: String,
    pub chunks_written: usize,
}

/// Run the incremental-reindex ingestion pipeline for one document:
/// mark `indexing`, chunk + embed the source text, atomically replace the
/// document's chunk set (`KnowledgeStore::reindex_chunks` — a safe no-op
/// clear-and-reinsert on a first-time ingest with no prior chunks), then mark
/// `ready`. Every embedding is computed *before* any chunk write, so an
/// embedding failure never touches the store at all; a store-level failure
/// leaves the prior chunk set intact (the store's own transactional
/// guarantee) — either way the document lands in `Error` with a diagnostic
/// message, never a half-updated state.
pub fn ingest_document(
    store: &dyn KnowledgeStore,
    embedder: &dyn EmbeddingBackend,
    mut document: Document,
    text: &str,
    params: ChunkParams,
    override_: &WriteOverride,
) -> Result<Document, IngestError> {
    document.status = DocumentStatus::Indexing;
    document.updated_at = now_secs();
    store
        .write_document(&document, override_)
        .map_err(IngestError::Store)?;

    let pieces = chunk_text(text, params);
    let mut embedded = Vec::with_capacity(pieces.len());
    for (i, piece) in pieces.iter().enumerate() {
        match embedder.embed(piece) {
            Ok(vector) => {
                let chunk = Chunk {
                    id: next_chunk_id(&document.id),
                    document_id: document.id.clone(),
                    text: piece.clone(),
                    position: i as i64,
                    source_ref: None,
                    created_at: now_secs(),
                };
                embedded.push((chunk, vector));
            }
            Err(e) => return Err(fail(store, &mut document, IngestError::Embed(e))),
        }
    }

    if let Err(e) = store.reindex_chunks(&document.id, &embedded) {
        return Err(fail(store, &mut document, IngestError::Store(e)));
    }

    document.status = DocumentStatus::Ready;
    document.error_msg = None;
    document.updated_at = now_secs();
    // Index-state-only transition (never write-gated): the content row was
    // already authorized by the write above (or a pre-existing row's own
    // prior authorization); flipping `status` is system bookkeeping, not a
    // fresh authorship act, so it must not re-trigger the human-zone gate
    // for the very row this same call just created or was permitted to touch.
    store
        .update_document_status(&document.id, document.status, None)
        .map_err(IngestError::Store)?;

    Ok(document)
}

/// On any mid-pipeline failure: mark the document `Error` with a diagnostic
/// message and best-effort persist that via the ungated status-only update
/// (if this write also fails, the original error is still what the caller
/// sees — never silently swallowed).
fn fail(store: &dyn KnowledgeStore, document: &mut Document, err: IngestError) -> IngestError {
    document.status = DocumentStatus::Error;
    document.error_msg = Some(err.to_string());
    document.updated_at = now_secs();
    let _ =
        store.update_document_status(&document.id, document.status, document.error_msg.as_deref());
    err
}

#[cfg(test)]
mod tests {
    use super::*;
    use cronus_contract::{Collection, Curation, RetrievedChunk};
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[derive(Default)]
    struct FakeStore {
        documents: RefCell<HashMap<String, Document>>,
        chunks: RefCell<HashMap<String, Vec<Chunk>>>, // document_id -> chunks
        reindex_calls: RefCell<usize>,
    }

    impl KnowledgeStore for FakeStore {
        fn create_collection(&self, _c: &Collection) -> Result<(), String> {
            Ok(())
        }
        fn get_collection(&self, _id: &str) -> Result<Option<Collection>, String> {
            Ok(None)
        }
        fn create_directory(&self, _d: &cronus_contract::Directory) -> Result<(), String> {
            Ok(())
        }
        fn write_document(
            &self,
            document: &Document,
            _override_: &WriteOverride,
        ) -> Result<(), String> {
            self.documents
                .borrow_mut()
                .insert(document.id.clone(), document.clone());
            Ok(())
        }
        fn get_document(&self, id: &str) -> Result<Option<Document>, String> {
            Ok(self.documents.borrow().get(id).cloned())
        }
        fn update_document_status(
            &self,
            document_id: &str,
            status: DocumentStatus,
            error_msg: Option<&str>,
        ) -> Result<(), String> {
            let mut docs = self.documents.borrow_mut();
            let doc = docs
                .get_mut(document_id)
                .ok_or_else(|| format!("no such document {document_id}"))?;
            doc.status = status;
            doc.error_msg = error_msg.map(String::from);
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
        fn delete_chunks(&self, document_id: &str) -> Result<(), String> {
            self.chunks.borrow_mut().remove(document_id);
            Ok(())
        }
        fn insert_chunk(&self, chunk: &Chunk, _embedding: &[f32]) -> Result<(), String> {
            self.chunks
                .borrow_mut()
                .entry(chunk.document_id.clone())
                .or_default()
                .push(chunk.clone());
            Ok(())
        }
        fn reindex_chunks(
            &self,
            document_id: &str,
            chunks: &[(Chunk, Vec<f32>)],
        ) -> Result<(), String> {
            *self.reindex_calls.borrow_mut() += 1;
            self.chunks.borrow_mut().insert(
                document_id.to_string(),
                chunks.iter().map(|(c, _)| c.clone()).collect(),
            );
            Ok(())
        }
        fn ann_search(
            &self,
            _collection_ids: &[String],
            _query_vector: &[f32],
            _top_k: usize,
        ) -> Result<Vec<(String, f32)>, String> {
            Ok(Vec::new())
        }
        fn fts_search(
            &self,
            _collection_ids: &[String],
            _query_text: &str,
            _top_k: usize,
        ) -> Result<Vec<(String, f32)>, String> {
            Ok(Vec::new())
        }
        fn hydrate_chunks(
            &self,
            _chunk_ids: &[String],
            _min_curation: Option<Curation>,
        ) -> Result<Vec<RetrievedChunk>, String> {
            Ok(Vec::new())
        }
    }

    struct FakeEmbedder {
        fail_on_word: Option<&'static str>,
    }

    impl EmbeddingBackend for FakeEmbedder {
        fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
            if let Some(bad) = self.fail_on_word
                && text.contains(bad)
            {
                return Err(format!("refused chunk containing {bad}"));
            }
            // Deterministic, content-derived "embedding" — length stands in
            // for a real vector; ingestion doesn't inspect its contents.
            Ok(vec![text.len() as f32])
        }
    }

    fn embedder() -> FakeEmbedder {
        FakeEmbedder { fail_on_word: None }
    }

    #[test]
    fn chunk_text_never_splits_mid_sentence() {
        let text = "First sentence here. Second sentence follows. Third one too.";
        let chunks = chunk_text(
            text,
            ChunkParams {
                chunk_size: 4,
                overlap: 1,
            },
        );
        assert!(!chunks.is_empty());
        for c in &chunks {
            // Each chunk boundary lands on sentence punctuation, never mid-word.
            assert!(
                c.ends_with('.') || c.ends_with('!') || c.ends_with('?'),
                "chunk did not end on a sentence boundary: {c:?}"
            );
        }
        // Every sentence's content is present somewhere across the chunks.
        let joined = chunks.join(" ");
        assert!(joined.contains("First sentence here."));
        assert!(joined.contains("Third one too."));
    }

    #[test]
    fn chunk_text_carries_overlap_words_into_the_next_chunk() {
        let text = "Alpha beta gamma delta. Epsilon zeta eta theta.";
        let chunks = chunk_text(
            text,
            ChunkParams {
                chunk_size: 4,
                overlap: 2,
            },
        );
        assert_eq!(
            chunks.len(),
            2,
            "two sentences over budget split into two chunks"
        );
        // The second chunk starts with the last 2 words carried from the first.
        assert!(chunks[1].starts_with("gamma delta"));
    }

    #[test]
    fn chunk_text_of_empty_input_is_empty() {
        assert_eq!(chunk_text("", ChunkParams::default()), Vec::<String>::new());
        assert_eq!(
            chunk_text("   ", ChunkParams::default()),
            Vec::<String>::new()
        );
    }

    fn agent_doc() -> Document {
        let mut d = Document::new_agent("doc-1", "col-1", "notes.md");
        d.status = DocumentStatus::Pending;
        d
    }

    #[test]
    fn ingest_document_reaches_ready_and_writes_chunks_via_the_atomic_path() {
        let store = FakeStore::default();
        let report_doc = ingest_document(
            &store,
            &embedder(),
            agent_doc(),
            "One sentence here. Another sentence follows.",
            ChunkParams::default(),
            &WriteOverride::None,
        )
        .expect("ingest succeeds");

        assert_eq!(report_doc.status, DocumentStatus::Ready);
        assert!(report_doc.error_msg.is_none());
        assert_eq!(*store.reindex_calls.borrow(), 1, "one atomic reindex call");
        assert_eq!(store.chunks.borrow().get("doc-1").unwrap().len(), 1);
    }

    #[test]
    fn kb3_a_second_ingest_replaces_the_first_set_with_no_duplication() {
        let store = FakeStore::default();
        ingest_document(
            &store,
            &embedder(),
            agent_doc(),
            "Short text only.",
            ChunkParams::default(),
            &WriteOverride::None,
        )
        .expect("first ingest");
        assert_eq!(store.chunks.borrow().get("doc-1").unwrap().len(), 1);

        let doc = store.get_document("doc-1").unwrap().unwrap();
        ingest_document(
            &store,
            &embedder(),
            doc,
            "A totally different, longer replacement source document body.",
            ChunkParams::default(),
            &WriteOverride::None,
        )
        .expect("re-ingest");

        // Exactly the second run's chunks remain — no accumulation.
        assert_eq!(store.chunks.borrow().get("doc-1").unwrap().len(), 1);
        assert!(
            store.chunks.borrow()["doc-1"][0]
                .text
                .contains("replacement")
        );
    }

    #[test]
    fn a_failed_embed_leaves_the_document_in_error_status_and_the_collection_queryable() {
        let store = FakeStore::default();
        // Seed a prior successful ingest so there IS a "prior state" to preserve.
        ingest_document(
            &store,
            &embedder(),
            agent_doc(),
            "Original good content here.",
            ChunkParams::default(),
            &WriteOverride::None,
        )
        .expect("seed ingest");
        let prior_chunks = store.chunks.borrow().get("doc-1").unwrap().clone();
        assert_eq!(prior_chunks.len(), 1);

        let failing = FakeEmbedder {
            fail_on_word: Some("POISON"),
        };
        let doc = store.get_document("doc-1").unwrap().unwrap();
        let err = ingest_document(
            &store,
            &failing,
            doc,
            "This sentence is fine. This one has POISON in it.",
            ChunkParams {
                chunk_size: 3,
                overlap: 0,
            },
            &WriteOverride::None,
        )
        .expect_err("a mid-pipeline embed failure must surface as an error");
        assert!(matches!(err, IngestError::Embed(_)));

        let doc_after = store.get_document("doc-1").unwrap().unwrap();
        assert_eq!(doc_after.status, DocumentStatus::Error);
        assert!(doc_after.error_msg.is_some());

        // The collection remains queryable from its PRIOR state: the old
        // chunk set was never touched, because embedding failed before any
        // store write (reindex_chunks was never called for the failed run).
        assert_eq!(
            store.chunks.borrow().get("doc-1").unwrap(),
            &prior_chunks,
            "an embed failure must not have reached the store at all"
        );
    }

    #[test]
    fn record_ingester_is_a_passthrough() {
        assert_eq!(RecordIngester::extract("plain text"), "plain text");
    }

    #[test]
    fn file_ingester_reads_the_blob_as_utf8() {
        let mut fs = FileStore::new();
        fs.add_file("file-1", b"hello from a file");
        let text = FileIngester::extract(&fs, "file-1").expect("extract");
        assert_eq!(text, "hello from a file");
    }

    #[test]
    fn file_ingester_reports_a_missing_file() {
        let fs = FileStore::new();
        let err = FileIngester::extract(&fs, "nope").expect_err("missing file");
        assert!(matches!(err, IngestError::Extract(_)));
    }

    struct FakeFetcher {
        body: Result<String, String>,
    }

    impl UrlFetcher for FakeFetcher {
        fn fetch(&self, _url: &str) -> Result<String, String> {
            self.body.clone()
        }
    }

    #[test]
    fn url_ingester_extracts_plain_text_from_fetched_html() {
        let fetcher = FakeFetcher {
            body: Ok("<html><head><style>p{color:red}</style></head><body><p>Hello &amp; welcome.</p><script>track()</script></body></html>".to_string()),
        };
        let text = UrlIngester::extract(&fetcher, "http://example.test/page").expect("extract");
        assert_eq!(text, "Hello & welcome.");
    }

    #[test]
    fn url_ingester_propagates_a_fetch_failure() {
        let fetcher = FakeFetcher {
            body: Err("connection refused".to_string()),
        };
        let err = UrlIngester::extract(&fetcher, "http://example.test/page")
            .expect_err("fetch failure must surface");
        assert!(matches!(err, IngestError::Extract(_)));
    }

    #[test]
    fn html_to_text_strips_tags_and_decodes_entities() {
        let html = "<div class=\"x\">Price: &lt;$10&gt; &amp; free &nbsp;shipping.</div>";
        assert_eq!(html_to_text(html), "Price: <$10> & free shipping.");
    }

    #[test]
    fn html_to_text_drops_script_and_style_block_contents() {
        let html = "<style>.a{}</style><p>Visible</p><script>var x = 1 < 2;</script>";
        assert_eq!(html_to_text(html), "Visible");
    }

    #[test]
    fn html_to_text_of_malformed_unterminated_markup_degrades_safely() {
        // An unterminated <script> tag drops the remainder rather than looping.
        let html = "<p>Before</p><script>oops, never closed";
        assert_eq!(html_to_text(html), "Before");
    }
}
