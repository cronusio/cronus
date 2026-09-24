# Code Graph

**Version:** 1.4.1
**Status:** Stable
**Layer:** implementation
**Implements:** l1-code-intelligence.md

## Overview

An in-workspace code intelligence index: language-aware entity extraction builds a graph of symbols and their relationships, stored in SQLite with FTS5 full-text search and dense vector embeddings. Queries fuse keyword and semantic signals via Reciprocal Rank Fusion. The index builds automatically on first use and updates incrementally on file change.

## Related Specifications

- [l1-code-intelligence.md](l1-code-intelligence.md) - **L1 parent.** This spec realizes the code-intelligence concept (CI-1…CI-13): the graph index, extraction pipeline, hybrid retrieval, and graph analysis.
- [l1-storage-model.md](l1-storage-model.md) - STO-1 (state tier over the read-only program tier) and STO-8 (a machine index derived from source, never the source of truth); `codegraph.db` follows the same placement rules.
- [l1-office-model.md](l1-office-model.md) - OFF-1: the global graph (§4.15) joins an office's graph only on an explicit add.
- [l2-filesystem-layout.md](l2-filesystem-layout.md) - `codegraph.db` lives in the workspace state tier.
- [l2-memory-store.md](l2-memory-store.md) - Memory store uses the same `sqlite-vec` extension for dense embeddings; shared pattern.
- [l2-execution-workspace.md](l2-execution-workspace.md) - Codegraph indexes an execution workspace's source tree during active tasks.
- [l2-quality-pipeline.md](l2-quality-pipeline.md) - Quality pipeline can seed codegraph queries to scope check commands.

## 1. Motivation

Agents working on code tasks need to find where a function is defined, which modules depend on it, and what semantically related symbols exist. Grepping is brittle on large codebases; full-file context floods the context window. A pre-built graph with FTS5 + embedding search lets agents ask precise questions with sub-second answers, without consuming token budget on whole-file reads.

## 2. Constraints & Assumptions

- The index lives entirely on-device in `<ws>/codegraph.db`. Code extraction (Pass 1, §4.8), media transcription (Pass 2) and search make no remote calls. Pass 3 sends documents and images to a model, so it runs only through the office's authorized routing — a local model by default, a cloud model only where egress is authorized (`l1-security` SEC-3) — and is skipped otherwise; the skip is reported, not silent.
- Embeddings are computed lazily on first search (not at index time) to keep initial indexing fast.
- The index is best-effort: it may be stale by up to one file-save cycle. Agents treat it as a fast cache, not an authoritative source, and must validate symbol locations before acting on them.
- Only files under the workspace's source root are indexed; generated files and vendored dependencies are excluded by default via `.codegraphignore` (same syntax as `.gitignore`).

## 3. Invariant Compliance (Layer 2 only)

Mapping of every `l1-code-intelligence.md` invariant to this implementation. Capabilities not yet built are marked **Roadmap** with the section that would host them — honest gaps, not regressions; the index's existing core (extraction, graph, fusion, incremental, multi-project, analysis) is fully realized.

| L1 Invariant | Implementation |
| --- | --- |
| CI-1 Parser-agnostic core | §4.1 — extraction is grammar-driven via the `CodeGraphGrammar` plugin interface; the SQLite graph store and query layer are language-neutral. |
| CI-2 Typed entity + edge taxonomy | §4.1 — fixed entity kinds and edge types; per-symbol attributes (`signature`, `docstring`, line span, `lang`) and per-edge `file`/`line`. |
| CI-3 Cache-not-authority | §2 — best-effort, ≤ one file-save cycle stale; agents validate symbol locations before acting. |
| CI-4 Persistent / incremental / namespaced | §4.2 (WAL SQLite, durable), §4.5 (mtime-keyed incremental upsert), §4.10 (stat-fastpath + content-hash cache), §4.15 (global graph, per-repo node prefixing = namespace isolation, source-hash skip). |
| CI-5 Structural analysis family | **Partial.** Implemented: §4.14 affected subgraph (impact/blast-radius), §4.13 import-cycle detection + god nodes. **Roadmap (§4.13):** hot paths, dead/unused detection, entry-point discovery, complexity metrics, module summary; change-kind classification (rename / signature-vs-body) for impact. |
| CI-6 Intent-aware budgeted context + compression accounting | **Roadmap.** New `codegraph.context(focus, intent, budget)` surface returning payload + `{entities_in_graph, traversed, kept}` accounting. |
| CI-7 Composite edit / PR bundles | **Roadmap.** New composite calls joining graph + version-control + tests + memory (edit bundle; PR blast-radius/test-gap/reviewer/commit-hint bundle). |
| CI-8 Hybrid + lazy + graph-only | **Partial.** §4.4 RRF lexical+semantic fusion and §4.6 lazy embeddings implemented. **Roadmap:** explicit model-free `--graph-only` mode serving structural tools with no embedding load. |
| CI-9 Docs↔design verification | **Partial.** §4.8 Pass-3 ingests docs into the graph. **Roadmap:** forward/reverse verification, design-gap detection, architecture-doc generation. |
| CI-10 Memory anchoring | Satisfied externally by [l2-memory-store.md](l2-memory-store.md) (CodeLink anchors, graph-proximity recall boost, `CodeChangeType`→`SuggestedAction` invalidation); this index supplies the entity identity + proximity signal. |
| CI-11 Capability-surface profiling | Satisfied externally by `ToolSurfaceProfile` in [l2-agent-session.md](l2-agent-session.md) and TC-7 deferred tools in `l1-tool-composition.md`; the §4.7 command surface is itself the structural profile. |
| CI-12 Interoperable export | **Roadmap (§4.7).** Add `codegraph export --format {dot,json,csv,triples}` over the live graph. |
| CI-13 Secret-exclusion at ingestion | **Partial.** §2/§4.1 exclude generated/vendored files via `.codegraphignore`. **Roadmap:** unconditional exclusion of credential dirs and secret-bearing file classes regardless of ignore config. |
| CI-14 Node summaries for navigation | **Roadmap.** Deterministic-first bounded per-node summary (from docstring/exported symbols/dominant relations/community) surfaced in `codegraph show`/MCP node lookup, optional model upgrade, budget-aware. |
| CI-15 Vocabulary-grounded query | **Roadmap.** Expand a natural-language query against the index's own symbol/label vocabulary before retrieval; navigable query surface (breadth/depth/shortest-path/explain) under a token budget on top of §4.14 traversal. |
| CI-16 Resolution & indirect-edge synthesis | **Partial.** §4.1 extracts direct edges; framework/SQL relations present. **Roadmap:** a distinct resolution pass with synthesized dynamic-dispatch edges (callback/observer/event/framework-render/cross-language) carrying `provenance` + wiring site, surfaced inline; close-flow-end-to-end discipline. |
| CI-17 Measured resolution coverage | **Roadmap.** Per-language/framework cross-file dependent coverage on benchmark repos with disclosed static-analysis frontier; never denominator-gamed. |
| CI-18 Disposable local cache / shared wiring | **Partial.** `codegraph.db` is already state-tier, per-workspace, and rebuildable (§4.5/§4.10), and `CRONUS_CODEGRAPH_OUT` redirects it (§4.10). **Roadmap:** an explicit statement that the DB is git-ignored and never a committed artifact, and — if any host wiring is ever emitted for an external agent — that *that* wiring, not the DB, is the shared unit; plus a `.ignore`-style re-admit if the DB path is ever inside a search-tool's ignore scope. |
| CI-19 Read-triggered freshness / working-tree bytes / read-only drift report | **Partial.** §4.5 already refreshes incrementally on first use per search call. **Roadmap:** define freshness against working-tree bytes explicitly (uncommitted/unstaged/staged alike; VCS selects the file set, not the key), offer a content-hash fingerprint mode alongside the mtime fast path (§4.5 currently mtime-only), and split a strictly read-only `cronus codegraph check` drift report from the mutating `index` path. |
| CI-20 Context delivery mode (push vs. pull) | **Roadmap.** The §4.7 command surface is the pull surface; an eager `context(...)` bundle (CI-6) is the push surface. Expose both, document the tradeoff (push = lower latency, pull = higher correctness), and keep any eager bundle in the session live zone (`l1-cache-stable-context`), never the frozen prefix. |

> Storage placement (formerly the parent contract): `codegraph.db` is state-tier mutable data over program-tier read-only source (STO-2) and uses durable WAL SQLite with transactional writes (STO-8) — see [l1-storage-model.md](l1-storage-model.md).

## 4. Detailed Design

### 4.1 Extraction

Language-aware extraction parses each source file and emits a list of symbols and edges.

**Supported languages** (via grammar packages): Rust, TypeScript/JavaScript, Python, Go, C/C++. Additional grammars can be loaded as extension plugins (interface: `CodeGraphGrammar`).

**Extracted entity kinds**:

| Kind | Examples |
| --- | --- |
| `function` | Free functions, methods, closures assigned to `const` |
| `class` | Classes, structs, records |
| `trait` | Traits, interfaces, protocols, abstract classes |
| `enum` | Enums, union types |
| `constant` | `const`, `static`, top-level `let`/`var` with literal initializers |
| `macro` | Rust macros, TypeScript decorators |
| `module` | Files, `mod` declarations, `namespace`s |
| `import` | Import/use statements (edge source only; not emitted as symbol rows) |

**Extracted edge types**:

| Edge | Meaning |
| --- | --- |
| `calls` | Function A calls function B |
| `imports` | Module A imports symbol B |
| `implements` | Type A implements trait/interface B |
| `extends` | Class A extends class B |
| `references` | Symbol A references type/const B (non-call, non-import) |

### 4.2 Storage schema

```text
[REFERENCE]
-- codegraph.db (WAL mode)

CREATE TABLE symbols (
  id          TEXT PRIMARY KEY,   -- SHA-256("{file}:{kind}:{qualified}")[0..16]
  kind        TEXT NOT NULL,      -- entity kind (§4.1)
  name        TEXT NOT NULL,
  qualified   TEXT NOT NULL,      -- fully qualified name ("crate::mod::fn_name")
  file        TEXT NOT NULL,
  start_line  INTEGER NOT NULL,
  end_line    INTEGER NOT NULL,
  signature   TEXT,               -- abbreviated signature / type annotation
  docstring   TEXT,               -- first doc comment block
  lang        TEXT NOT NULL,
  indexed_at  INTEGER NOT NULL    -- Unix ms
);

CREATE VIRTUAL TABLE symbols_fts USING fts5(
  name, qualified, signature, docstring,
  content='symbols', content_rowid='rowid'
);

CREATE TABLE edges (
  from_id   TEXT NOT NULL REFERENCES symbols(id),
  to_id     TEXT NOT NULL REFERENCES symbols(id),
  edge_type TEXT NOT NULL,
  file      TEXT,
  line      INTEGER
);

CREATE TABLE codegraph_meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
-- Keys used: last_full_index_at (ISO-8601), file_mtimes (JSON map path→mtime)

-- Dense embeddings via the sqlite-vec extension (same DB):
CREATE VIRTUAL TABLE symbol_vecs USING vec0(
  id        TEXT PRIMARY KEY,
  embedding FLOAT[768]    -- dimension matches the configured embedding model
);
```

### 4.3 Query interface

```text
[REFERENCE]
CodeGraphQuery {
  text:        Option<String>,       // keyword or natural-language query
  kinds:       Option<Vec<String>>,  // filter to specific entity kinds
  lang:        Option<String>,       // filter to a language
  file_prefix: Option<String>,       // filter to a file subtree
  limit:       u32,                  // default 20
  include_edges: bool,               // include outbound edges in results
}

CodeSymbol {
  id:         String,
  kind:       String,
  name:       String,
  qualified:  String,
  file:       String,
  start_line: u32,
  end_line:   u32,
  signature:  Option<String>,
  docstring:  Option<String>,
  lang:       String,
  score:      f32,                   // RRF-fused relevance score
  edges:      Option<Vec<CodeEdge>>,
}

CodeEdge {
  to_id:     String,
  to_name:   String,
  edge_type: String,
  file:      Option<String>,
  line:      Option<u32>,
}
```

### 4.4 RRF fusion

When `text` is provided, the query runs two parallel retrieval passes and fuses their ranked lists:

1. **FTS5 pass**: `SELECT … FROM symbols_fts WHERE symbols_fts MATCH ?` — BM25-ranked.
2. **Embedding pass**: compute query embedding (same model as symbol embeddings), then `SELECT … FROM symbol_vecs ORDER BY vec_distance_cosine(embedding, ?) LIMIT K`.

Scores are fused via Reciprocal Rank Fusion:

```text
[REFERENCE]
rrf_score(doc) = Σ  1 / (k + rank_i(doc))
  where k = 60 (constant), rank_i is the document's 1-based rank in list i, summed over lists
```

Results are sorted descending by `rrf_score`, then trimmed to `limit`. Filters (kind, lang, file_prefix) are applied before ranking.

### 4.5 Auto-index and incremental updates

**First-use trigger**: on the first call to `codegraph.search()` or `codegraph.index()`, the index is built if `codegraph.db` does not exist or its `symbols` table is empty.

**Incremental update**: after each successful build, file mtimes are stored in `codegraph_meta["file_mtimes"]`. On subsequent searches, files with changed mtimes are re-extracted and their symbols upserted; deleted files' symbols are removed. Incremental updates run synchronously but are bounded to ≤ 50 changed files per search call; larger batches are deferred to a background rebuild.

**Background full rebuild**: triggered by the scheduler (configurable cadence; default: daily at idle) or manually via `cronus codegraph index --full`.

### 4.6 Embedding computation

Embeddings are computed on first search, not at index time:

1. `codegraph.search()` detects that `symbol_vecs` is empty or stale (fewer rows than `symbols`).
2. A background task computes embeddings for all symbols in batches (default: 64 symbols per batch).
3. The current query proceeds with FTS5-only ranking until the embedding batch for the query's top results completes; subsequent calls use RRF fusion.

The embedding model is configurable and can be swapped via the extension registry (interface: `EmbeddingModel`). The default is a quantized sentence-embedding model bundled with the application. Embedding dimension must match the `symbol_vecs` schema; changing the model requires a `codegraph purge` and full rebuild.

### 4.7 Command surface

| Action | CLI | TUI | Library (no code) |
| --- | --- | --- | --- |
| build / rebuild index | `cronus codegraph index [--path <root>] [--full]` | `/codegraph index` | `codegraph.index(path, full) -> IndexStats` |
| search symbols | `cronus codegraph search <query> [--kind fn] [--lang rust] [--limit N]` | `/codegraph search …` | `codegraph.search(query) -> CodeSymbol[]` |
| show symbol + edges | `cronus codegraph show <id>` | `/codegraph show <id>` | `codegraph.get(id) -> CodeSymbol` |
| purge index | `cronus codegraph purge` | `/codegraph purge` | `codegraph.purge() -> void` |
| index stats | `cronus codegraph status` | `/codegraph status` | `codegraph.stats() -> IndexStats` |

### 4.8 Three-pass extraction pipeline

The extraction pipeline runs in three sequential passes, each processing a different media type.
Code files never go to the LLM semantic extractor; only documents, papers, and images do.

#### Pass 1 — Code structure (local, no API calls)

Grammar-based parsers extract AST nodes (functions, classes, imports, call-graph edges) locally.
Files are parsed in parallel across worker threads. SQL files receive deterministic extraction: tables, views, foreign keys, and JOIN
relationships are extracted without heuristic inference.

#### Pass 2 — Audio and video (local, no API calls)

Audio and video files are transcribed locally with a bundled speech-to-text model. The transcription
prompt is seeded with the top god-nodes from Pass 1 output (most-connected symbols) to focus the
transcript on the codebase domain rather than generic speech. Transcripts are cached by content
hash; re-runs skip already-processed media files entirely.

#### Pass 3 — Documents, papers, images (LLM subagents, costs tokens)

Markdown files, PDFs, and images are dispatched to parallel LLM subagents. Each subagent reads a
batch of files and returns a structured JSON fragment: nodes, edges, and hyperedges. Fragments are
merged into the unified graph after all subagents complete. Before dispatch, the semantic cache is
consulted; only files without a valid cache entry are sent to the LLM.

If the corpus contains only code files, Pass 3 is skipped entirely.

### 4.9 Confidence taxonomy and hyperedges

Every edge carries a confidence label. The label is stored on the edge and drives graph analysis
(god-node filtering, surprising-connection scoring, question generation).

```text
[REFERENCE]
EXTRACTED   (score 1.0)   — relationship explicitly present in source: import statement,
                            direct call, explicit inheritance declaration.

INFERRED    (score ∈ {0.95, 0.85, 0.75, 0.65, 0.55}) — model-reasoned relationship:
              0.95 — near-certain (explicit cross-file reference, one plausible target)
              0.85 — strong evidence (naming + context align)
              0.75 — reasonable (contextual but not explicit)
              0.65 — weak (naming similarity only)
              0.55 — speculative

AMBIGUOUS   — uncertain; flagged in analysis reports for manual review.
              AMBIGUOUS edges generate "What is the exact relationship between A and B?"
              questions and are prioritised in surprising-connection ranking.
```

**Hyperedges** represent group relationships connecting three or more nodes simultaneously (e.g.,
a design pattern that couples several components). They are stored in a separate table from binary
edges and cannot be reduced to a set of pairwise relationships.

```text
[REFERENCE]
hyperedge { id, relation, source_file, participant_ids: [id, ...] }
-- Stored in: codegraph_hyperedges table
-- Never flattened to binary edges; queried separately.
```

### 4.10 Dual-layer extraction cache

Two cache layers with different invalidation semantics prevent unnecessary re-extraction.

**AST cache** (versioned by extractor version)

Stores the output of the local grammar-based extractor. Cache entries are namespaced by extractor
version (`cache/ast/v{version}/`) so that bug fixes in the extractor correctly invalidate stale
results from the previous release. Stale version directories are swept lazily on first use.

**Semantic cache** (unversioned)

Stores the output of LLM-subagent extraction. Not version-namespaced: re-extracting on every
release would rebill tokens for unchanged files. Stored under `cache/semantic/`.

**Shared properties of both layers:**

```text
[REFERENCE]
Cache key:     SHA256(body_content + "\x00" + relative_path_lowercase)
               For .md files: body_content = file_content with YAML frontmatter stripped.
               Frontmatter changes (review dates, tags) do not invalidate semantic cache.

Stat fastpath: before full SHA256, consult (abs_path → {size, mtime_ns, hash}) index.
               Skip SHA256 when size AND mtime_ns match the recorded entry.
               Index is loaded once per process, flushed atomically at exit.

Atomic write:  write to tmp file in same directory → os.replace(tmp, target).
               Prevents torn reads when two processes update the same cache entry.

Portable storage: absolute source_file paths are relativized to the workspace root before
               write; re-anchored to the absolute path when loaded. Cache entries are
               therefore portable across machines and checkout directories.

Cache structure:
  cache/ast/v{version}/{hash}.json    — AST entries (version-scoped)
  cache/semantic/{hash}.json          — semantic entries (content-keyed only)
  cache/stat-index.json               — stat fastpath index

Override: CRONUS_CODEGRAPH_OUT env var to redirect output/cache directory
          (useful for worktrees or shared-output setups).
```

### 4.11 Community detection

Graph nodes are clustered into communities to reveal subsystem boundaries and improve god-node
ranking, deduplication scoring, and analysis surfacing. No embeddings are needed — the semantic
similarity edges already in the graph provide the similarity signal directly.

```text
[REFERENCE]
Algorithm:  Leiden (preferred) → Louvain fallback if Leiden unavailable.
            A fixed random seed is pinned so the partition is deterministic across runs.

resolution: float, default 1.0
            > 1.0 → more, smaller communities
            < 1.0 → fewer, larger communities

Hub exclusion:
  exclude_hubs_percentile (optional) — nodes whose degree exceeds this percentile
  are excluded from partitioning. After partition, hubs are re-attached to their
  majority-vote neighbour community. Prevents super-hubs from pulling unrelated
  subsystems into the same community.

Oversized community splitting:
  MAX_COMMUNITY_FRACTION = 0.25   — communities > 25% of graph nodes are split
  MIN_SPLIT_SIZE         = 10     — only split if community has at least this many nodes

  Second pass:
  COHESION_SPLIT_THRESHOLD = 0.05 — re-split communities with cohesion below this
  COHESION_SPLIT_MIN_SIZE  = 50   — only cohesion-split if community has at least this many nodes
  (handles doc-hub nodes that bridge otherwise-unrelated subsystems)

cohesion_score(community) = actual_intra_edges / (n * (n-1) / 2)

Community ID stability:
  IDs are remapped after each run to maximize overlap with the previous assignment
  (greedy one-to-one matching by intersection size, then fresh IDs for unmatched,
  deterministic tie-break by size desc + lexical node order).
  Without remapping, identical groupings appear as massive ID churn in diff tools.
```

### 4.12 Entity deduplication pipeline

LLM extraction can assign different labels to the same real-world concept across batches
(e.g., `AuthManager`, `AuthenticationManager`, `auth_mgr`). This pipeline merges such
near-duplicate entities into a single canonical node before community detection.

The pipeline runs after graph construction and before clustering. Order matters: a cleaner
graph produces better community assignments.

```text
[REFERENCE]
ENTROPY_THRESHOLD  = 2.5    // bits/char; labels below this are too ambiguous to auto-merge
LSH_THRESHOLD      = 0.7    // MinHash candidate-pair threshold
MERGE_THRESHOLD    = 92.0   // Jaro-Winkler normalized_similarity × 100
COMMUNITY_BOOST    = 5.0    // score bonus for pairs sharing a Leiden community ID
```

**Step 1 — Exact normalization**
Lowercase + collapse non-alphanumeric runs to a space (Unicode NFKC). Identical normalized
labels within the same source file merge immediately. Cross-file matches fall through to fuzzy.

**Step 2 — Entropy gate**
Labels with < ENTROPY_THRESHOLD bits/char Shannon entropy are skipped. Short ambiguous names
(`"AI"`, `"DB"`, `"x"`) are too risky to auto-merge.

**Step 3 — MinHash/LSH blocking**
3-gram character shingles, 128 permutations, LSH threshold 0.7. Generates candidate pairs
in O(n) rather than O(n²). Operates in sub-second at 10 k nodes.

**Step 4 — Jaro-Winkler verification**
Each candidate pair is verified at ≥ MERGE_THRESHOLD. Guards (any guard hit → skip merge):

- Cross-file long labels (≥ 12 chars): plain Jaro (no prefix bonus) to prevent false positives
  from names that share a prefix but differ in a distinguishing token.
- Prefix-extension pairs (`parseConfig` / `parseConfigFile`): never merged regardless of score.
- Labels with differing embedded digit runs: numbered siblings, not duplicates.
- `rationale` and `document` node types: not label-merged across source files (boilerplate risk).
- Short label pairs: blocked unless same-length and single-char substitution.

**Step 5 — Community boost**
Pairs where both nodes share a Leiden community ID receive +COMMUNITY_BOOST to score.
Same-community membership is a strong signal that the nodes belong to the same subsystem.

**Step 6 — Union-Find merge**
Confirmed pairs feed a Union-Find structure. Connected components → each component merged
into one canonical node. Winner: shortest ID without chunk suffix. Edges rewired to survivor;
self-loops dropped.

**Step 7 — Optional LLM tiebreaker**
Ambiguous pairs (score 75–92) are batched in groups of 30 for LLM yes/no judgment.
Off by default; enables more aggressive deduplication at small token cost.

**Exclusions:**

- Code symbols (AST-extracted): identity is the fully-qualified path; same-named symbols in
  different files are always distinct.
- Cross-project deduplication is not supported; labels coincide across projects by chance.

### 4.13 Graph analysis

#### God nodes

Top-N most-connected entities in the graph. Excluded from ranking:

- File-level hub nodes (filename == label; accumulate import/contains edges mechanically)
- AST method stubs (label starts with `.` and ends with `()`)
- Concept nodes (empty source_file or no file extension)
- Built-in/stdlib type names (`str`, `int`, `Path`, `Optional`, `Counter`, etc.)
- Common JSON schema key names (`name`, `id`, `type`, `version`, `dependencies`, etc.)

#### Surprising connections

Cross-file edges ranked by composite surprise score:

```text
[REFERENCE]
score components (additive):
  confidence weight:    AMBIGUOUS=3, INFERRED=2, EXTRACTED=1
  cross file-type:      +2 (code↔paper, code↔image — non-obvious coupling)
  cross-repo:           +2 (different top-level directory)
  cross-community:      +1 (Leiden says structurally distant)
  peripheral→hub:       +1 (min-degree ≤ 2 node reaching max-degree ≥ 5 node)
  semantically_similar_to: score × 1.5 (conceptual links with no structural edge)

Suppressed: INFERRED "calls"/"uses" edges that cross language families or connect
code to a doc file — these are resolver artefacts, not real architecture.
```

#### Graph diff

```text
[REFERENCE]
graph_diff(G_old, G_new) -> {
  new_nodes:      [{id, label}],
  removed_nodes:  [{id, label}],
  new_edges:      [{source, target, relation, confidence}],
  removed_edges:  [{source, target, relation, confidence}],
  summary:        "3 new nodes, 5 new edges, 1 node removed",
}
```

#### Import cycle detection

```text
[REFERENCE]
1. Collapse symbol-level nodes to file via source_file attribute.
2. Build directed file-level graph from "imports_from" and "re_exports" edges only.
3. Find simple cycles bounded by max_cycle_length (default 5) to prevent combinatorial explosion.
4. Deduplicate rotations: normalize by starting from the lexicographically smallest file.
5. Sort by length (shortest = tightest coupling) then return top-N.
```

**Suggested questions** (generated automatically from graph signals)

| Signal | Question template |
| --- | --- |
| AMBIGUOUS edge | "What is the exact relationship between `A` and `B`?" |
| Bridge node (high betweenness) | "Why does `X` connect `Community A` to `Community B`?" |
| God node with ≥ 2 INFERRED edges | "Are the N inferred relationships involving `X` actually correct?" |
| Weakly-connected nodes (degree ≤ 1) | "What connects `X`, `Y`, `Z` to the rest of the system?" |
| Low-cohesion community (score < 0.15, ≥ 5 nodes) | "Should `Module` be split into smaller, more focused modules?" |

### 4.14 Affected subgraph analysis

Given a seed entity, traverse incoming dependency edges to find all entities that would be
affected by a change to the seed — useful for impact analysis before refactoring.

```text
[REFERENCE]
DEFAULT_AFFECTED_RELATIONS = {
  "calls", "references", "imports", "imports_from", "re_exports",
  "inherits", "extends", "implements", "uses", "mixes_in", "embeds",
}

AffectedHit { node_id: String, depth: u32, via_relation: String }

affected_nodes(graph, seed_id, relations, depth=2) -> Vec<AffectedHit>
  // BFS over incoming edges of types in `relations`, depth-limited.

Seed resolution (first match wins, ambiguous multi-match → null):
  1. Exact node ID
  2. Exact label (Unicode NFC, case-folded)
  3. Bare name — strip trailing "()" from callable labels before match
  4. Exact source_file path
  5. Substring of label (contains match)
```

### 4.15 Global multi-project graph

For cross-project queries, individual project graphs may be merged into a global graph in the
user's state tier. A project graph joins it only through an explicit `global_add` — never
implicitly because several workspaces are open — since reading one office's graph from another
is exactly the cross-office access OFF-1 requires to be explicit.

```text
[REFERENCE]
Locations (state tier, l2-filesystem-layout):
  <state>/global-graph/graph.json      — merged graph (node-link JSON: a nodes array and a links array)
  <state>/global-graph/manifest.json   — per-repo metadata

Node prefixing: all nodes from project P receive ID "{repo_tag}:{original_id}"
to prevent cross-project ID collisions.

External library nodes (source_file is empty): deduplicated by label across projects —
the same stdlib/framework concept becomes a single node in the global graph, with edges
from all projects that reference it.

Skip-if-unchanged: manifest stores source_hash (first 16 hex chars of SHA256 of the
source graph.json). global_add() is a no-op if the hash matches the stored value.

Manifest repo entry: { added_at, source_path, node_count, edge_count, source_hash }

Corrupt manifest recovery: on JSON parse error, rename to
manifest.corrupt.{unix_timestamp} and start fresh — prevents a corrupt file from
blocking all global queries. Error is surfaced to stderr, not silently swallowed.

Operations:
  global_add(source_path, repo_tag)  -> { nodes_added, nodes_removed, skipped }
  global_remove(repo_tag)            -> nodes_removed
  global_list()                      -> manifest repos dict
```

## Document History

| Version | Change |
| --- | --- |
| 1.4.1 | Consistency pass (2026-09-23): §2 claimed no remote calls for indexing while Pass 3 sends documents and images to LLM subagents — Pass 3 now runs only under authorized routing (SEC-3) and reports when skipped. Implementation details of a Python reference removed (hash-seed variable, process workaround, a graph library's file format); the global graph moves from a home-directory path to the state tier and joins an office's graph only on explicit `global_add` (OFF-1). Storage citations corrected (STO-1/STO-8); canonical range CI-1…CI-20. |
| 1.4.0 | Mapped new parent invariants CI-18 (disposable local cache / shared wiring — partial), CI-19 (read-triggered freshness against working-tree bytes; read-only drift report — partial), CI-20 (context delivery mode push vs. pull — roadmap) in §3; added the "graph as a folder of linked plain files, no store" alternative to §5 with the reason Cronus keeps the SQLite index |
| 1.3.0 | Mapped new parent invariants CI-16 (resolution & indirect-edge synthesis w/ provenance — partial) and CI-17 (measured resolution coverage — roadmap) in §3 |
| 1.2.0 | Mapped new parent invariants CI-14 (node summaries) and CI-15 (vocabulary-grounded query) as roadmap rows in §3 |
| 1.1.0 | Re-parented from `l1-storage-model.md` to the new `l1-code-intelligence.md` concept; rewrote §3 to map CI-1…CI-13 (implemented vs roadmap); storage-model retained as a Related placement contract |
| 1.0.1 | Added §4.8–§4.15: three-pass pipeline, confidence taxonomy, dual-layer cache, community detection, entity deduplication, graph analysis, affected subgraph, global multi-project graph |
| 1.0.0 | Initial spec |

## 5. Drawbacks & Alternatives

- **Lazy embedding computation**: the first search after a cold index returns FTS5-only results. A warm-up step (`codegraph index --embed`) pre-computes all embeddings if full-quality results from the first query are required.
- **Grammar coverage**: less-common languages fall back to regex-based extraction (function/class name patterns only). Grammar plugins extend coverage without modifying core.
- **Stale entries for deleted files**: if incremental update does not detect a deletion (e.g. file renamed outside tracked paths), stale symbols remain until full rebuild. Agents must validate that referenced files still exist before acting on symbol locations.
- **Alternative — external language servers (LSP)**: richer analysis (type inference, cross-file resolution) but requires per-language server processes, complex lifecycle management, and IPC. The self-contained SQLite approach is more portable and embeddable in the Tauri context.
- **Alternative — graph as a folder of linked plain files, no store**: a fourth code-graph engine surveyed for this concept keeps its concept-node graph as linked Markdown files the agent greps and opens with its ordinary file tools — no database, no query API required to *read* the graph, and the retrieval tool and the browsable artifact are the same bytes (so an agent that cannot reach the query surface degrades to `grep`, not to nothing). Cronus keeps the SQLite store because FTS5 + `sqlite-vec` fusion (§4.4) and incremental upsert (§4.5) need an index, and the memory store already depends on the same extension (shared pattern). The plain-files idea is still partially honoured: CI-12's vault/wiki export is a browsable projection, and CI-18's "the cache is just a file, git-ignored" framing is the same instinct one layer down.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[CONCEPT]` | `.design/main/specifications/l1-code-intelligence.md` | L1 parent — CI-1…CI-20 invariant contract |
| `[STORAGE]` | `.design/main/specifications/l1-storage-model.md` | STO-1 placement, STO-8 derived machine index |
| `[LAYOUT]` | `.design/main/specifications/l2-filesystem-layout.md` | `codegraph.db` placement |
| `[MEM]` | `.design/main/specifications/l2-memory-store.md` | sqlite-vec pattern (shared) |
