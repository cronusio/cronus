# Knowledge Base

**Version:** 1.6.0
**Status:** Stable
**Layer:** concept

## Overview

Named collections of documents — files, web pages, structured records — organized for semantic retrieval. A knowledge base is an agent-queryable source that enriches generation with grounded, attributed context. It is distinct from the memory model (short-lived conversational context) and from the agent's training (static model weights). Knowledge bases are user-managed, access-controlled, and incrementally indexed.

## Related Specifications

- [l1-memory-model.md](l1-memory-model.md) - Memory is ephemeral conversational context; knowledge base is persistent, user-managed reference material.
- [l1-resource-sharing.md](l1-resource-sharing.md) - Knowledge collections are shareable resources governed by the access grant model.
- [l1-file-management.md](l1-file-management.md) - Files are the source documents for a knowledge collection.
- [l1-extensions.md](l1-extensions.md) - A retrieval skill may query the knowledge base as a tool.
- [l1-document-understanding.md](l1-document-understanding.md) - The deep, layout-aware front of ingestion (KB-12); turns raw sources into structured documents.
- [l1-content-segmentation.md](l1-content-segmentation.md) - Structure-aware chunking (KB-13); how understood documents become retrieval units.
- [l1-knowledge-graph.md](l1-knowledge-graph.md) - Optional graph representation of a collection (KB-14) for relational/global questions.
- [l1-hierarchical-summarization.md](l1-hierarchical-summarization.md) - Optional summary-tree representation (KB-14) for multi-resolution retrieval.
- [l2-knowledge-store.md](l2-knowledge-store.md) - Concrete implementation: schema, indexing pipeline, retrieval.
- [l1-candidate-windows.md](l1-candidate-windows.md) - [ADDED v1.6.0] KB-19 applies CW-1…CW-5 to scoped collection retrieval: scope before the cut, sized and widened windows, fill or disclose, fusion fed deeper than `top_k`.
- [l1-state-authority.md](l1-state-authority.md) - [ADDED v1.6.0] The vector index is a projection of the chunk text; KB-18 is SAU-8 for an embedding-model change.
- [l1-retrieval-evaluation.md](l1-retrieval-evaluation.md) - [ADDED v1.6.0] The fixture a new embedding generation is compared on before the switch (RE-6, RE-7), and the curve CW-5 sets window parameters from.

## 1. Motivation

Agents reason only from what they can see in their context window. Without a structured retrieval layer, agents must load entire documents into context (expensive, often too large) or rely on model training (stale, hallucination-prone). A knowledge base provides a standard path: ingest documents once, retrieve relevant chunks on demand, inject them as cited context. This keeps generation grounded and the context window lean.

## 2. Constraints & Assumptions

- A knowledge base is a retrieval aid; it does not replace authoritative data sources or transactional systems.
- Retrieval does not guarantee factual accuracy — agents must cite sources and note uncertainty when relying on retrieved content.
- Large files are split into chunks; the original file is preserved and retrievable in full.
- Cross-collection retrieval (querying multiple collections simultaneously) is supported but the caller must explicitly select collections; there is no implicit "search everything."

## 3. Core Invariants

Rules every Layer 2 implementation MUST NOT violate:

- **KB-1 (Collection isolation):** each collection is an independently indexed retrieval unit; queries always target an explicit set of one or more collections, never all collections implicitly.
- **KB-2 (Hierarchical organization):** documents within a collection may be arranged in a directory tree for human navigation; directory structure does NOT affect retrieval ranking or chunking.
- **KB-3 (Incremental indexing):** adding, replacing, or removing a document triggers partial re-indexing of only the affected document; full-collection re-index is an explicit admin action.
- **KB-4 (Access control):** a collection obeys the resource-sharing model (RS-1…RS-8); a worker may only query collections to which it holds at least `read` access.
- **KB-5 (Source types):** the system ingests at minimum: uploaded files (text, PDF, Markdown, HTML), web URLs (scraped and cached), and plain-text records. Ingestion normalizes all sources to retrievable, attributed chunks.
- **KB-6 (Source attribution):** every retrieved chunk carries a reference to its source document and, where applicable, its position (page, section, byte offset) within that document.
- **KB-7 (Non-authoritative recall):** the knowledge base stores and retrieves; it does not assert correctness. Agents must treat retrieved content as evidence, not ground truth.
- **KB-8 (Soft deletion):** removing a document from a collection marks it deleted and removes it from retrieval immediately; physical cleanup (storage + index cleanup) is deferred.
- **KB-9 (Authorship zones):** within a collection, documents carry an authorship origin that places them in a zone with a declared agent write-boundary — *human-authored* documents (uploaded sources, human-owned records) are agent-read-only, while *agent-synthesized* documents are agent-read-write. The boundary is enforced mechanically at the storage layer: a write to a read-only zone is refused unless an explicit override is supplied, never merely discouraged by guidance. This authorship boundary is orthogonal to KB-4 — KB-4 governs which *workers* may access a collection (resource-sharing), KB-9 governs whether the *agent* may rewrite human-authored material within a collection it can already access.
- **KB-10 (Curation lifecycle):** every agent-synthesized document carries a curation status advancing `draft → reviewed → stable`. The agent may freely create and revise `draft` documents; advancing to `reviewed` or `stable` requires explicit human action. Downstream consumers MUST treat `draft` content as provisional, and retrieval MAY filter or down-weight by curation status. This editorial-trust lifecycle is distinct from the per-document indexing `status` (pending/indexing/ready/error/deleted), which tracks index state, not trust.
- **KB-11 (Optional query preparation):** before retrieval, a raw query MAY be transformed into an optimized retrieval query — extracting or expanding search keywords (including multi-language terms where the collection is multilingual) and/or decomposing a compound query into sub-queries whose results are merged. Preparation is optional and, when applied, MUST be transparent (the prepared query is recorded alongside the raw one), MUST preserve the caller's original query as a fallback (a preparation yielding nothing usable degrades to the raw query, never to an empty search), and MUST NOT alter chunk attribution (KB-6) or bypass access control (KB-4). Query preparation improves recall; it never fabricates sources and never widens the accessible set.
- **KB-12 (Deep document-understanding stage):** before segmentation, a source document passes through a document-understanding stage ([l1-document-understanding.md](l1-document-understanding.md)) that reconstructs structure, reading order, tables, and figures with positional provenance. Plain "parse & extract text" is the explicitly-degraded fallback when deep understanding is unavailable, and reduced fidelity is recorded, never masked.
- **KB-13 (Structure-aware segmentation):** chunking is structure-aware ([l1-content-segmentation.md](l1-content-segmentation.md)) — boundaries follow document structure, each chunk carries its heading breadcrumb, and tables/figures stay atomic. Fixed-window overlap is the fallback for unstructured runs, not the default; chunk attribution (KB-6) is preserved through segmentation.
- **KB-14 (Multi-representation index):** a collection MAY be indexed into more than the flat chunk/vector representation — additionally a corpus knowledge graph ([l1-knowledge-graph.md](l1-knowledge-graph.md)) and/or a hierarchical summary tree ([l1-hierarchical-summarization.md](l1-hierarchical-summarization.md)). These are opt-in enrichments; the flat chunk/vector index is mandatory and a collection is fully usable with it alone. Every representation grounds back to the same source chunks.
- **KB-15 (Multi-channel fused retrieval + rerank):** retrieval MAY fan out across the available representations — vector, lexical, graph-local/global, summary-tree — fuse their results by rank fusion, then optionally rerank the fused top set with a stronger relevance model before returning. Fusion and reranking improve precision only; they never widen the accessible set (KB-4) and never fabricate attribution (KB-6). Every returned item still carries its source reference.
- **KB-16 (Structured-constraint separation — self-query & metadata filter):** a natural-language query often conflates **semantic intent** with **structured constraints** ("recent security papers by Smith" carries the topic *security* plus author=*Smith* and date≈*recent*). Retrieval MAY separate the two: a query-preparation step (composing KB-11) extracts the structured constraints into a **metadata-filter predicate** over the collection's declared document metadata (§4.1 `meta`), leaving a residual **semantic query** for the vector/fused search (KB-15). The predicate **filters** the candidate set — it restricts, never ranks — so a document whose metadata fails it is excluded regardless of semantic similarity. Three guards keep it safe: **transparent** — the parsed filter is recorded beside the raw query (the KB-11 discipline); **fallback-floored** — an unparseable or empty filter degrades to plain semantic search, never an empty result; and **narrow-only** — the filter can only restrict the already access-bounded set (KB-4), never reaching a document the caller could not otherwise retrieve, with attribution preserved (KB-6). Filtering improves precision on constraint-bearing queries; it never fabricates a source and never widens the accessible set.
- **KB-17 (Index-side query-bridging representations):** a chunk MAY be indexed under **more than its own text** — additional generated match representations that bridge the gap between how users *ask* and how documents *state*: the **questions** a chunk answers (a query then matches a generated question, question↔question, instead of question↔prose), a **summary** (matching gist over surface wording), extracted **keywords**, or a modality bridge (a figure's caption/description for a text query). Each is an alternative **match surface for the same source chunk** — a hit on any representation returns the chunk with the chunk's own attribution (KB-6), and the generated proxy is **never returned as if it were the source**. This is the index-side **dual** of query preparation (KB-11 transforms the query toward the documents; this transforms the documents toward likely queries), so the two close the ask-state gap from both ends; it composes fused retrieval (the extra representations are additional recall surfaces, KB-15) and never changes the access model (KB-4). Generation is a bounded, host-supplied, local-first enrichment (like the rest of the multi-representation index, KB-14) — absent it, a chunk is matched by its own text alone.
- **KB-18 (Embedding generations):** `[ADDED v1.6.0]` the vector index records the embedding model, its dimension and its distance metric, and a query embedded under any other identity is refused rather than scored — not comparable, never merely different (`l1-state-authority` SAU-8). A dimension check alone does not satisfy this: two models of the same dimension embed into unrelated spaces. A model change is a new index **generation** built beside the live one: retrieval keeps answering from the live generation until the new one holds a vector for every ready chunk in scope; ingestion meanwhile embeds each new or replaced chunk under both; the new generation is then compared with the live one on the retrieval-evaluation fixture (RE-7) and becomes active in one atomic switch when it shows no regression beyond the RE-6 threshold, or when the user directs it — a regression holds the switch and is reported. The previous generation is kept until the user releases it, so reverting is a switch, not a re-embedding. Without a fixture the switch is recorded as unmeasured, never as passed. This is the full-collection re-index KB-3 reserves for an explicit admin action, made non-destructive.
- **KB-19 (Scoped retrieval fills or discloses):** `[ADDED v1.6.0]` each retrieval channel treats the caller's collection scope (KB-1), document readiness and soft deletion (KB-8), the curation floor (KB-10) and any metadata filter (KB-16) as eligibility under `l1-candidate-windows`: applied inside the channel's search where its engine can evaluate it (CW-1), otherwise through a window sized from the scope's share of the index and widened (CW-2). Fusion and reranking (KB-15) receive each channel at a depth greater than `top_k` (CW-4). A result shorter than `top_k` while eligible matches exist is a defect; a retrieval stopped by a hard bound returns its results marked truncated (CW-3). A short result caused by the caller's own `min_score` is not a shortfall.

> L2 specs cannot reach RFC status until all invariants here are addressed in their "Invariant Compliance" section.

## 4. Detailed Design

### 4.1 Collection

A collection is a named, access-controlled group of documents:

```text
Collection {
  id          : CollectionId
  owner_id    : UserId
  name        : string
  description : string
  meta        : dict          // arbitrary metadata (tags, language hints, etc.)
}
```

### 4.2 Document and Directory

Documents live in a flat list or an optional directory tree within a collection:

```text
Document {
  id            : DocumentId
  collection_id : CollectionId
  directory_id  : DirectoryId?
  source_file_id: FileId?       // reference to the file subsystem
  source_url    : string?       // web URL when source is remote
  name          : string
  status        : "pending" | "indexing" | "ready" | "error" | "deleted"  // index state
  origin        : "human" | "agent"                                       // authorship zone (KB-9)
  curation      : "draft" | "reviewed" | "stable"?                        // editorial trust (KB-10); agent docs only
}

Directory {
  id            : DirectoryId
  collection_id : CollectionId
  parent_id     : DirectoryId?
  name          : string
}
```

### 4.3 Chunk

Each document is split into chunks for retrieval:

```text
Chunk {
  id          : ChunkId
  document_id : DocumentId
  text        : string
  embedding   : vector<f32>   // dense semantic embedding
  position    : int           // ordinal position within the document
  source_ref  : SourceRef     // page/section/byte reference for attribution
}
```

### 4.4 Ingestion Pipeline

```mermaid
graph LR
    IN[File / URL / Record] --> UNDERSTAND[Understand: layout-aware structured document]
    UNDERSTAND --> SEGMENT[Segment: structure-aware chunks + breadcrumb]
    SEGMENT --> EMBED[Embed each chunk]
    EMBED --> STORE[Store chunks + embeddings]
    STORE --> IDX[Update flat index]
    IDX --> ENRICH[Optional enrichment: knowledge graph and/or summary tree]
    ENRICH --> READY[Document status: ready]
```

The understand and segment stages are the deep front of ingestion (KB-12/KB-13); flat "parse & extract text → chunk with overlap" is their explicitly-degraded fallback. The optional enrichment representations (KB-14) are built beside — never in place of — the mandatory flat index. Errors at any stage leave the document in `error` status with a diagnostic message; the collection remains queryable from its prior state, and a failed optional enrichment never blocks the flat index from becoming `ready`.

### 4.5 Retrieval

Query flow for a semantic retrieval request:

1. Caller provides: `{query_text, collection_ids[], top_k, min_score?}`.
2. Optional query preparation (KB-11): transform `query_text` into a prepared retrieval query — keyword extraction/expansion and/or compound-query decomposition — recording both and falling back to the raw query if preparation yields nothing usable.
3. Embed the prepared (or raw) query using the same embedding model used at ingestion.
4. Fan out across the collection's available representations (KB-15): ANN (Approximate Nearest Neighbour) over `ready` chunks; a keyword channel (BM25 or FTS); and, where built, the knowledge-graph channels (local/global) and the hierarchical-summary channel. Each channel applies scope and readiness before its cut where its engine can, and returns a window deeper than `top_k` (KB-19).
5. Fuse the per-channel results by rank fusion (e.g. RRF) into one candidate set.
6. Optionally rerank the fused top candidates with a stronger relevance model, then keep the `top_k` above `min_score`.
7. Return `top_k` results — fewer only when fewer eligible matches exist, otherwise marked truncated (KB-19) — each carrying `{text, source_ref, score, document_id, collection_id}` and its originating channel.
8. The caller injects the results into the model context with attribution.

### 4.6 Lifecycle Summary

| Action | Effect |
| --- | --- |
| Create collection | Empty collection created; index initialised. |
| Add document | Document queued for ingestion; status `pending → indexing → ready`. |
| Replace document | Old chunks removed from index; new document ingested. |
| Remove document | Document marked `deleted`; chunks excluded from retrieval immediately. |
| Delete collection | All documents and chunks removed; index dropped. |

### 4.7 Authorship Zones & Curation Lifecycle

Two orthogonal dimensions govern how the agent may touch a document, layered on top of the indexing pipeline:

**Authorship zone (KB-9).** Each document's `origin` declares who authored it. The storage layer treats `origin = human` documents as a read-only zone for the agent:

```text
[REFERENCE]
write(document):
    if document.origin == "human" and not override_supplied:
        reject("read-only zone: human-authored material")
    else:
        persist(document)
```

The override exists for explicit, audited operations (e.g. a user-directed correction routed through the agent), never as a silent default. This makes the human/agent boundary an enforced invariant of the store rather than a behavioural request in a prompt — robust against agent drift.

**Curation lifecycle (KB-10).** Agent-synthesized documents advance through editorial-trust states:

```mermaid
graph LR
    draft -- "human review" --> reviewed -- "human approval" --> stable
    reviewed -- "needs work" --> draft
```

The agent owns `draft`; only human action advances `reviewed`/`stable`. Retrieval may expose a `min_curation` filter so high-trust queries exclude provisional `draft` content. A new agent-synthesized document defaults to `draft`.

### 4.8 Query Preparation (KB-11)

Raw user queries are often poorly shaped for retrieval: conversational phrasing dilutes the signal, a single query bundles several distinct questions, and a multilingual corpus is under-matched by a monolingual query. An optional preparation step sits between the caller's request and the embedding/search, transforming the query without ever narrowing the caller's reach:

```text
[REFERENCE]
prepare(query_text, collection_meta):
    prepared := extract_or_expand_keywords(query_text, collection_meta.languages?)
    subqueries := decompose_if_compound(query_text)        // optional; [] when atomic
    if prepared is empty and subqueries is empty:
        return { retrieval_query: query_text, raw: query_text }   // KB-11 fallback: never empty
    return { retrieval_query: prepared, subqueries, raw: query_text }
```

Two properties keep it safe. First, **transparent and reversible**: the prepared query is recorded next to the raw one, so a reader can see exactly what was searched and a poor preparation is diagnosable rather than invisible. Second, **fallback-floored**: preparation that yields nothing usable degrades to the raw query — it can improve recall but never turn a real query into an empty search. When `decompose_if_compound` returns sub-queries, each is retrieved independently and the results are merged (the same RRF fusion of §4.5), with every chunk still carrying its own attribution (KB-6) and access already bounded by KB-4 — preparation reshapes the query, never the accessible set.

### 4.9 Multi-Representation Index & Fused Retrieval (KB-14, KB-15)

The flat chunk/vector index answers local, lookup-style questions, but two classes of question defeat it: relational/corpus-global questions (no single chunk holds the connective evidence) and gestalt questions (no chunk holds the overview). A collection MAY therefore be indexed into additional, opt-in representations, each grounded back to the same source chunks:

| Representation | Answers | Contract |
| --- | --- | --- |
| Flat chunk/vector (mandatory) | Local, lookup — "what does it say about X" | this spec |
| Knowledge graph (optional) | Relational, corpus-global — "how do A and B connect" | [l1-knowledge-graph.md](l1-knowledge-graph.md) |
| Hierarchical summary tree (optional) | Gestalt / multi-resolution — "what is this collection about" | [l1-hierarchical-summarization.md](l1-hierarchical-summarization.md) |

Retrieval (KB-15) fans a query out across whichever representations exist, fuses their results by rank fusion, and optionally reranks the fused top set with a stronger cross-encoder before returning. The enrichments never change the access model: fusion and reranking reshape *ranking*, never the accessible set (KB-4), and every returned item still carries its source attribution (KB-6). A collection with only the flat index behaves exactly as before — the enrichments are additive, and their construction (l1-document-understanding and l1-content-segmentation feeding l1-knowledge-graph and l1-hierarchical-summarization) is a bounded, observable, host-supplied, local-first pipeline.

### 4.10 Structured-Constraint Separation (KB-16)

Semantic search is powerful for topical intent but blind to structured constraints. Ask for "papers on retrieval from 2024" and a pure vector search happily returns a 2019 paper that is topically perfect — the "2024" was embedded as just more topic text, not enforced as a constraint. Self-query separation refuses that failure by splitting the query along its two natures:

```text
[REFERENCE]
prepare_with_filter(query_text, collection_meta_schema):
    filter, semantic := separate(query_text, collection_meta_schema)     // structured vs semantic
    if filter is unparseable or empty:
        return { semantic: query_text, filter: none }                    // KB-16 fallback-floor
    return { semantic, filter, raw: query_text }                         // transparent: both recorded

retrieve(prepared, collection_ids):
    candidates := access_bounded(collection_ids)                         // KB-4 already narrowed
    candidates := candidates where doc.meta satisfies prepared.filter    // KB-16 narrow-only
    return fused_rank(candidates, prepared.semantic)                     // KB-15 over the filtered set
```

Two properties keep it honest. First, **narrow-only**: the filter can only restrict the set access control already bounded (KB-4) — it never reaches a document the caller could not otherwise retrieve, so a metadata filter is never a privilege-escalation path. Second, **fallback-floored**: an extraction that yields no usable filter degrades to plain semantic search (the KB-11 discipline), so a mis-parsed constraint never turns a real query into an empty result. The extracted filter is recorded beside the raw query, so a reader sees exactly what was enforced — and a claim grounded on a filtered result still traces to its source (KB-6).

### 4.11 Index-Side Query-Bridging Representations (KB-17)

A query and the document that answers it often live in different spaces. A user asks "how do I reset my password"; the manual states "the credential-recovery flow is initiated from the account panel" — topically identical, embedding-distant. A text query cannot match an image at all. Query preparation (KB-11) attacks this from the query side; KB-17 attacks it from the **index** side, giving each chunk extra match surfaces generated to look like what a user would send:

| Representation | Bridges | Match direction |
| --- | --- | --- |
| Generated questions the chunk answers | ask ↔ state (question vs prose) | a question query ↔ a question |
| Summary | gist ↔ surface wording | query ↔ condensed meaning |
| Extracted keywords | lexical intent ↔ verbose prose | keyword query ↔ keyword index |
| Caption / description of a figure | text ↔ image modality | text query ↔ image proxy |

All representations of a chunk point back to the **one** source chunk: a hit on a generated question returns the chunk (with the chunk's attribution, KB-6), never the question as if it were a document. The generated surfaces are additional recall channels the fused retrieval (KB-15) ranks over, and they are opt-in, bounded, and host-supplied like the rest of the enrichment layer (KB-14) — a collection with only default text indexing behaves exactly as before. Query preparation and index-side bridging are **duals**: together they close the gap between how questions are asked and how answers are written, from both directions.

## 5. Implementation Notes

1. Embedding model selection: ingestion and query use the model the active index generation records; a model change builds a new generation beside the live one and switches only when it is complete and compared (KB-18), never purge-then-rebuild.
2. Chunk size and overlap are configurable per collection; sensible defaults (512 tokens / 64-token overlap) are applied when unspecified.
3. Web URL sources are scraped at ingest time and cached; subsequent refreshes are a manual or scheduled action.

## 7. Drawbacks & Alternatives

- **Single global index:** simpler to query but breaks collection isolation (KB-1) and makes access control harder.
- **External vector database:** offloads ANN but adds operational complexity and network dependency. The in-process, file-local approach (sqlite-vec) keeps the system embeddable.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[IMPL]` | `.design/main/specifications/l2-knowledge-store.md` | Concrete schema, indexing pipeline, and Rust implementation. |
| `[MEMORY]` | `.design/main/specifications/l2-memory-store.md` | Memory store also uses sqlite-vec; share the embedding engine pattern. |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.6.0 | 2026-10-21 | Core Team | Added KB-18 (embedding generations) and KB-19 (scoped retrieval fills or discloses). KB-18: the vector index records model, dimension and distance metric; a query under another identity is refused as not comparable (`l1-state-authority` SAU-8) — a dimension check is not enough, since same-dimension models embed into unrelated spaces; a model change builds a new generation beside the live one, dual-embeds new chunks, is compared on the retrieval-evaluation fixture (RE-6/RE-7) and switches atomically, the predecessor kept so reverting is a switch. KB-19: scope, readiness, soft deletion, curation floor and metadata filters are eligibility under `l1-candidate-windows` — applied before the cut where the engine can (CW-1), otherwise a window sized from the scope's share and widened (CW-2), fusion fed deeper than `top_k` (CW-4), a short result disclosed (CW-3). §4.5 steps 4 and 7 and §5 note 1 (formerly "a model change requires full re-indexing") updated. Motivated by the realization taking a fixed `max(4·k, k + 20)` window before filtering by collection, and recording the vector dimension but not the model. |
| 1.5.0 | 2026-07-23 | Core Team | Added KB-17 (index-side query-bridging representations): a chunk MAY be indexed under additional generated match representations beyond its own text — the questions it answers (query↔question instead of query↔prose), a summary, extracted keywords, a figure caption/description (text↔image bridge) — each an alternative match surface for the SAME source chunk (a hit returns the chunk with its own attribution KB-6, never the proxy as the source); the index-side dual of KB-11 query preparation (query→docs vs docs→likely-queries), composing fused retrieval (KB-15) without changing access (KB-4), opt-in/bounded/host-supplied like the multi-representation enrichment (KB-14); §4.11 added. Mined from a studied knowledge-base/workflow platform's multi-index-per-datum (default/question/summary/custom/image) dataset model. |
| 1.4.0 | 2026-07-22 | Core Team | Added KB-16 (structured-constraint separation — self-query & metadata filter): a NL query MAY be split into a structured metadata-filter predicate over declared document metadata (§4.1) plus a residual semantic query (composing KB-11 query preparation), the predicate applied as a narrow-only filter on the already-access-bounded candidate set (KB-4) with attribution preserved (KB-6), transparent (parsed filter recorded) and fallback-floored (unparseable/empty filter → plain semantic search, never an empty result); resolves the tension where a semantic blob under-enforces structured constraints embedded in NL ("papers from 2024" returning a 2019 hit). §4.10 added. Mined from a studied LLM-orchestration framework's self-query / metadata-filter retriever. |
| 1.3.0 | 2026-07-22 | Core Team | Opened the ingestion black box and added optional multi-representation retrieval. New invariants: KB-12 (deep document-understanding stage before chunking — l1-document-understanding, with plain text extraction as the honest, fidelity-recording fallback), KB-13 (structure-aware segmentation — l1-content-segmentation: breadcrumb-carrying chunks, atomic tables/figures), KB-14 (multi-representation index — optional corpus knowledge graph l1-knowledge-graph and/or hierarchical summary tree l1-hierarchical-summarization beside the mandatory flat index), KB-15 (multi-channel fused retrieval + rerank). §4.4 ingestion deepened to understand→segment→embed→index→optional-enrich; §4.5 retrieval generalized to multi-channel fuse+rerank; new §4.9. Mined from a studied retrieval/document-intelligence engine; enrichments are opt-in, source-faithful, access-preserving, and local-first. |
| 1.2.0 | 2026-07-09 | Core Team | Added KB-11 (optional pre-retrieval query preparation) — a raw query MAY be transformed into an optimized retrieval query via keyword extraction/expansion (multi-language where the collection is multilingual) and/or compound-query decomposition with merged sub-query results; transparent (prepared query recorded), fallback-floored (degrades to the raw query, never an empty search), and attribution/access-preserving (never alters KB-6 attribution nor bypasses KB-4 access); §4.5 retrieval flow gains the prep step, new §4.8. Mined from a studied agent framework's pre-retrieval keyword-generation strategy; recall-improving, source-faithful. |
| 1.1.0 | 2026-06-26 | Core Team | Added KB-9 (authorship zones — storage-enforced human/agent write boundary) and KB-10 (curation lifecycle draft→reviewed→stable); Document model gains `origin` + `curation` fields; new §4.7. |
| 1.0.0 | 2026-06-25 | Core Team | Initial spec — collections, ingestion pipeline, retrieval, KB-1…KB-8. |
