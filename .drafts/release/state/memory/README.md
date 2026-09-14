# Global Memory (installation level)

Cross-project, long-lived memory about the human client and globally learned facts.

- `global.db` — SQLite + vector index (facts, preferences). Created at runtime.
- `graph.db` — global knowledge graph. Created at runtime.
- `notes/` — human-readable Markdown memory (editable).

Read order at retrieval: employee → workspace → global (most specific wins).

## Item shape

Each memory item: id, scope, type, content, tags[], validity_scope
(Forever|Domain|Project|Workaround), verification (Untested→Tested→Confirmed→Stable),
utility, created_at/valid_at/invalid_at (supersede, not delete), provenance.

## Recall (v0.1.0)

Hybrid: sqlite-vec (semantic) + FTS5 (lexical) + tags, fused and resolved
most-specific-first (employee → workspace → global), injected under a token budget.
Relationship graph deferred (added incrementally).

## Ownership

Core service: synchronous read/write/recall (hot path).
`archivist` role: asynchronous consolidation (verify→decay→promote→distill→reconcile→prune).
