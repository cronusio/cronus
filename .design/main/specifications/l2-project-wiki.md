# Project Wiki Store (Implementation)

**Version:** 1.0.2
**Status:** Stable
**Layer:** implementation
**Implements:** l1-project-wiki.md

## Overview

The concrete realization of the project wiki for v0.1.0: the client-facing living documentation is **materialized as a derived projection cache in a per-office SQLite database** (`<state>/workspaces/<ws>/wiki/wiki.db`), not as loose Markdown files. Pages are rows written **only** by an event-driven, office-owned regeneration pipeline; each row carries its source citations and a source fingerprint; FTS5 powers search; the client reads through a read-only query API and can never write. The database is a **cache of a projection** — every row is reconstructable from the office's ground truth (board, decisions, operational ledger, deliverables), so dropping `wiki.db` loses nothing durable. This is deliberately distinct from the memory store's stance: for learned memory the store *is* the source of truth; for the wiki the store is only a rebuildable cache of upstream truth.

## Related Specifications

- [l1-project-wiki.md](l1-project-wiki.md) - The concept this spec implements (PW-1…PW-8).
- [l2-filesystem-layout.md](l2-filesystem-layout.md) - On-disk home of the per-office `wiki/wiki.db` (STO placement).
- [l1-operational-ledger.md](l1-operational-ledger.md) - Authoritative facts the wiki cites; grounding source for PW-4.
- [l2-kanban-board.md](l2-kanban-board.md) - Board items (work done / in flight) the wiki projects from (PW-3/PW-4).
- [l2-resource-sharing.md](l2-resource-sharing.md) - `access-grants` enforcement for PW-7 when the office is shared.
- [l2-knowledge-store.md](l2-knowledge-store.md) - Reused FTS5 hybrid-search pattern; the knowledge base the wiki is deliberately distinct from (PW-8).
- [l1-navigation-model.md](l1-navigation-model.md) - The Wiki sidebar tab (NV-1) that surfaces this store read-only.

## 1. Motivation

The concept requires a client-facing surface that maintains itself, stays grounded, and is a view rather than a second truth. Loose Markdown files fight all three: regenerating a page set is not transactional, "which page is stale" is not queryable, and a file tree invites hand-editing that would turn the projection into a divergent second truth. A per-office SQLite table set gives the projection the properties it needs for free: transactional whole-or-nothing regeneration of affected pages, a queryable freshness/citation model, FTS5 search (PW-6) on the same substrate memory and the knowledge base already use, and a store with **no client write path** — so "read-only projection" is enforced by the absence of an API, not by discipline. Because the rows are a cache, a rebuild reconstructs them from ground truth, which is exactly what makes PW-3 mechanically true rather than aspirational.

## 2. Constraints & Assumptions

- One `wiki.db` per office under `<state>/workspaces/<ws>/wiki/` (PW-7 scoping); it holds **no authoritative data of its own** — it is a projection cache (PW-3).
- Rows are written **only** by the office regeneration pipeline (curator-owned). There is no client or agent write API into `wiki_page`; the client UI issues read-only queries (PW-2).
- Ground truth lives elsewhere — the board, the graph (`decisions`/`artifacts`), the operational ledger, the file store. The wiki never holds the only copy of anything.
- Regeneration is **event-driven and incremental** (only affected pages), never per-turn (matches the concept's cost mitigation).
- Every stored section must carry ≥1 citation; an uncited claim is rejected at generation time, never persisted (PW-4).
- Internal engineering and specification-process detail is filtered out at generation; it must never reach a `wiki_page` row (PW-8).

## 3. Invariant Compliance (Layer 2)

| L1 Invariant | Implementation |
| --- | --- |
| PW-1 Client-facing, plain-language | The generation step composes plain-language, client-term `body` text per page; internal jargon and engineering detail are excluded by the PW-8 filter. Pages answer "what is my project / where does it stand," rendered read-only in the Wiki tab. |
| PW-2 Office-maintained, client never curates | `wiki_page` / `wiki_changelog` rows are written **only** by the curator-owned regeneration pipeline. The client-facing surface exposes read/search queries only — there is no write/update endpoint into wiki rows, so the client structurally cannot curate. |
| PW-3 Projection, not source-of-truth | Rows are a derived cache. `rebuild(office)` drops and re-derives every row from ground truth (board / graph decisions / ledger / deliverables); dropping `wiki.db` loses nothing durable. No write flows wiki → project; editing is impossible (PW-2). |
| PW-4 Grounded & attributed | Each page row stores `citations` (JSON array of `{source_kind, source_id}` referencing a decision, work product, board item, or ledger fact). Generation asserts every substantive claim resolves to a citation; a section that cannot be attributed is dropped, not stored. |
| PW-5 Living & freshness-honest | Each row stores a `source_fingerprint` (hash of the inputs it was generated from) + `generated_at`. A background check recomputes the current fingerprint of a page's sources; drift with no regeneration sets `stale=1`, surfaced in the UI as a stale marker (never shown silently as current). `wiki_changelog` appends entries newest-first. |
| PW-6 Navigable & searchable | `wiki_page.parent_id` + `ord` form the overview → area → detail tree; `wiki_page_fts` (FTS5 over `title`,`body`) backs client search. No page sits more than a few links from the overview (enforced by the fixed page-kind hierarchy). |
| PW-7 Scoped & access-controlled | Exactly one `wiki.db` per office directory. When the office is shared, read access passes the `access-grants` gate (`has_access(Wiki, office_id, Read)`); when private, the on-device no-egress posture applies — the file never leaves the device. |
| PW-8 Distinct from KB & internal artifacts | `wiki.db` is a separate store from `knowledge_*` (agent-facing input) and from the office's internal engineering and specification artifacts (developer-facing). A generation-time content filter strips that internal detail so it never lands in a row; the wiki is never queried as an agent retrieval source. |

## 4. Detailed Design

### 4.1 Schema (per-office SQLite, conceptual)

```sql
-- [REFERENCE] illustrative, not final DDL
-- <state>/workspaces/<ws>/wiki/wiki.db — a derived projection CACHE (PW-3), never source of truth
CREATE TABLE wiki_page (
  id                 TEXT PRIMARY KEY,
  office_id          TEXT NOT NULL,
  parent_id          TEXT,                    -- overview → area → detail (PW-6); NULL for root
  ord                INTEGER NOT NULL DEFAULT 0, -- sibling ordering
  kind               TEXT NOT NULL,           -- overview|area|decisions|howto|glossary|changelog
  title              TEXT NOT NULL,
  body               TEXT NOT NULL,           -- generated plain-language content (PW-1)
  citations          TEXT NOT NULL,           -- JSON [{source_kind, source_id}]; non-empty (PW-4)
  source_fingerprint TEXT NOT NULL,           -- hash of the inputs this page was generated from (PW-5)
  generated_at       INTEGER NOT NULL,
  stale              INTEGER NOT NULL DEFAULT 0 -- 1 when current source fingerprint ≠ stored (PW-5)
);
CREATE TABLE wiki_changelog (                 -- PW-5 change history, newest-first
  id        TEXT PRIMARY KEY,
  office_id TEXT NOT NULL,
  page_id   TEXT,
  change    TEXT NOT NULL,                     -- human-readable "what changed"
  at        INTEGER NOT NULL
);
CREATE VIRTUAL TABLE wiki_page_fts USING fts5(title, body, content=wiki_page); -- PW-6 search
CREATE INDEX idx_wiki_parent ON wiki_page(parent_id, ord);
CREATE INDEX idx_wiki_stale  ON wiki_page(office_id, stale);
```

The page kinds mirror the concept's page model (§4.2 of the parent): `overview`, `area`, `decisions`, `howto`, `glossary`, `changelog`.

### 4.2 Regeneration pipeline (event-driven, PW-3/PW-4/PW-5)

```text
[REFERENCE]
on significant office change (board item → done, decision recorded,
                             deliverable produced, milestone reached):
    affected := map(change → affected page kinds)
    for page in affected:
        inputs  := gather ground truth (board / graph decisions / ledger / deliverables)  // PW-3
        content := generate_client_language(inputs, exclude_internal_detail)              // PW-1, PW-8
        for claim in content:
            require citation(claim) ∈ inputs else drop claim                              // PW-4
        upsert wiki_page(content, citations, fingerprint(inputs), now(), stale := 0)      // transactional
        append wiki_changelog(page, summary)                                              // PW-5
    for page whose fingerprint(current_sources) ≠ stored source_fingerprint:
        set stale := 1                                                                    // PW-5
```

Regeneration is incremental (only affected pages) and transactional per page set — a failed regeneration rolls back, leaving the prior rows intact and correctly marked. It is off the hot path, triggered by office events, not per agent turn.

### 4.3 Rebuild = proof of PW-3

```text
[REFERENCE]
rebuild(office):
    drop all wiki_page / wiki_changelog rows for office
    re-derive every page from current ground truth (the §4.2 loop over all page kinds)
    re-derive wiki_changelog from the operational ledger's append-only history — current
      state alone cannot say what changed when; a period the ledger does not cover starts
      the rebuilt changelog at the rebuild, and the first entry says so
```

`rebuild` is the operational proof that the store is a cache: a full rebuild reconstructs an **equivalent** wiki — the same page structure, grounded in the same sources, asserting the same attributed facts — because nothing authoritative ever lived only in `wiki.db`. The regenerated prose need **not** be byte-identical (generation is model-based and not deterministic); what is guaranteed is that no *authoritative* content is lost, since every page is derived from ground truth that still exists. It is also the recovery path — a corrupted or deleted `wiki.db` is regenerated, never restored-as-truth.

### 4.4 Access & placement (PW-7)

- Home: `<state>/workspaces/<ws>/wiki/wiki.db` — one per office, alongside `memory/workspace.db` and `graph/graph.db`.
- The client reaches it through the Wiki sidebar tab (l1-navigation-model NV-1), read-only.
- Private office → on-device, no egress. Shared office → each read passes `access-grants` `has_access(Wiki, office_id, Permission::Read)` (l2-resource-sharing), so wiki visibility follows the office's sharing posture.

## 5. Drawbacks & Alternatives

- **A new per-office DB file.** One more `.db` under each office. Justified: isolating the cache makes `rebuild`/drop trivial and keeps the projection physically separate from authoritative stores (memory, graph), reinforcing PW-3.
- **Regeneration cost.** Generating client-language pages has a token cost. Mitigated exactly as the concept prescribes — event-driven and incremental, only affected pages, off the hot path.
- **Alternative — loose Markdown files (the original mental model).** Rejected: non-transactional regeneration, no queryable freshness/citation model, no shared FTS search, and a file tree invites hand-editing that would turn the projection into a divergent second truth (violating PW-2/PW-3). A read-only DB cache removes the write path entirely.
- **Alternative — embed wiki tables in `workspace.db`.** Viable, but couples a disposable projection cache to the authoritative workspace store; a dedicated `wiki.db` keeps "safe to nuke and rebuild" physically obvious.
- **Alternative — reuse the knowledge-base tables.** Rejected per PW-8: the knowledge base is agent-facing input; the wiki is client-facing output. Sharing a store would blur that boundary and risk leaking one into the other.

## nodus-relevance mapping

Largely a main-workspace product surface; the portable runtime contributes as a source and, optionally, as the regeneration trigger.

| Element | nodus seam | Note |
| --- | --- | --- |
| Grounded generation (PW-4) | workflow run artifacts / audit stream as cited sources | A workflow's produced deliverables and decisions are wiki sources, attributed by `run_id`. |
| Projection (PW-3) | read-only view over `StorageProvider` state | Regeneration writes only through the curator pipeline; the client path never writes. |
| Regeneration trigger (§4.2) | office event → `RUN` of a regeneration workflow | Whether regeneration runs as a nodus workflow is host-side; no nodus invariant is added. |

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[WIKI]` | `.design/main/specifications/l1-project-wiki.md` | The concept (PW-1…PW-8) this store realizes |
| `[LAYOUT]` | `.design/main/specifications/l2-filesystem-layout.md` | On-disk home of `<ws>/wiki/wiki.db` |
| `[LEDGER]` | `.design/main/specifications/l1-operational-ledger.md` | Grounding/attribution source for PW-4 |
| `[SHARING]` | `.design/main/specifications/l2-resource-sharing.md` | `access-grants` enforcement for PW-7 |

## Document History

| Version | Date | Notes |
| --- | --- | --- |
| 1.0.2 | 2026-09-23 | Consistency pass (2026-09-23): PW-8 named the development scaffolding's directory and process — restated as the office's internal engineering and specification artifacts (a product spec does not reference build scaffolding). `rebuild` dropped `wiki_changelog`, which current state cannot reconstruct — it is re-derived from the operational ledger's history, and an uncovered period is stated. |
| 1.0.1 | 2026-07-15 | Promoted RFC→Stable via Post-Update Review (`@role:spec-critic` + `@role:prompt-engineer` PASS). One review finding fixed: §4.3 overclaimed that `rebuild` yields a **byte-equivalent** wiki — corrected to *informational* equivalence (same structure/sources/attributed facts), since generation is model-based and non-deterministic; the PW-3 guarantee is no-loss-of-authoritative-content, not byte-identical prose. No other change. Stable = design agreed; concrete schema/DDL validated during implementation. |
| 1.0.0 | 2026-07-15 | Initial RFC — realizes l1-project-wiki as a per-office SQLite projection **cache** (`<ws>/wiki/wiki.db`), not loose Markdown files: `wiki_page` (parent/ord tree, citations, source_fingerprint, stale) + `wiki_changelog` + `wiki_page_fts`; event-driven incremental transactional regeneration (PW-3/4/5); `rebuild` as the operational proof the store is a rebuildable cache; read-only client access with the write path structurally absent (PW-2); per-office scoping + access-grants gate (PW-7); distinct from knowledge-base/SDD with a content filter (PW-8). Contrast recorded: wiki store = cache of upstream truth, unlike the memory learned-tier where the store IS the truth (MEM-4). |
