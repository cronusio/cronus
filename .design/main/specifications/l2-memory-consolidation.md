# Memory Consolidation (Local Realization)

**Version:** 1.0.3
**Status:** Stable
**Layer:** implementation
**Implements:** l1-memory-consolidation.md

## Overview

The concrete realization of the consolidation & corpus-maintenance layer (MC-1…MC-10) on the local-first SQLite substrate that [l2-memory-store.md](l2-memory-store.md) already provides. This spec adds **no storage engine of its own**: it composes the store's write path (§4.3), recall fusion (§4.2), entity links (§4.7), two-phase pipeline (§4.12), and bi-temporal record (§4.14), and layers onto them the three things the substrate does not yet guarantee — a **processing-depth axis**, a **fact-vs-derived signal separation**, and a **scheduled maintenance loop** with confidence-gated actions and audit trails.

The design stance follows the crate topology established in `l2-crate-topology`: consolidation is **off-hot-path host labor** that reaches persistence only through the `UserDataStore` seam, never by opening its own connection. The synchronous read/write paths stay in the store; this layer runs periodically, behind the seam, and degrades to a no-op when no generator is available (local-first).

## Related Specifications

- [l1-memory-consolidation.md](l1-memory-consolidation.md) - The concept realized here; MC-1…MC-10.
- [l2-memory-store.md](l2-memory-store.md) - The substrate this composes: recall fusion (§4.2), write path (§4.3), entity links (§4.7), two-phase consolidation pipeline (§4.12), bi-temporal record (§4.14), typed kinds (§4.15). Every "already owned" cell in §3 cites a section here.
- [l1-memory-intelligence.md](l1-memory-intelligence.md) - The active query surface above this layer; it consumes the derived ranking signals (MC-8) and lifecycle inputs (archive) this layer maintains. Its L2 realization sits atop this one.
- [l1-scheduler-model.md](l1-scheduler-model.md) - Fires the periodic consolidation and maintenance passes (MC-2, MC-6, MC-7).
- [l2-crate-topology.md](l2-crate-topology.md) - Why the maintenance labor lives behind the `UserDataStore` seam in an adapter tier, not in domain code (§4.4 minting rule).
- [l1-doctor.md](l1-doctor.md) - System-level self-healing; the corpus-health sweep here is its corpus-level analogue.
- [l1-inner-monologue.md](l1-inner-monologue.md) - Consumes the advisory interest topics (MC-10); decides whether/when to surface them.

## 1. Motivation

`l2-memory-store` accepts writes and returns ranked reads correctly, but it has no layer that keeps the corpus *durable-quality* over time. Its §4.12 pipeline already converts transcripts into consolidated `MEMORY.md`, and its §4.2 fuse already ranks — but the ranking is weighted-additive over a fixed signal set, there is no separation between authored fact and computed signal, and there is no maintenance discipline against redundancy/overload/staleness/abstraction-gap. This spec supplies exactly those gaps, and only those, on top of what the store already owns.

## 2. Constraints & Assumptions

- **No new storage engine, no new scheduler.** Persistence is the store's SQLite files reached through the `UserDataStore` seam; cadence is the scheduler's existing job shapes. This layer contributes tables and passes, not engines.
- **Off the hot path.** Nothing here runs on the synchronous recall or write path except the two cheap reads recall already makes (a derived-signal lookup, §4.7's edge join). All heavy work (clustering, multi-sample merge votes) is scheduled/idle.
- **Local-first, generator-optional.** Every pass that needs an LLM (consolidation abstraction, cluster summarization, merge adjudication) degrades to a deferred no-op when no generator is bound — never to corruption (MC-2).
- **Additive-edge substrate.** The fact layer is the store's authored content plus its edges; edges only grow (MC-3). This layer never introduces a side edge-store that could drift from the text.
- **Co-editable corpus.** Human and agent may edit the same items out of band; every fact-layer write is optimistic-concurrency-guarded (MC-9).

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Realization | Already owned vs net-new |
| --- | --- | --- |
| **MC-1** Processing-depth tiers | A `depth` enum (`raw`/`working`/`consolidated`), orthogonal to the scope column (§4.1). `raw` = the immutable rollout transcripts and `raw_memories.md` accumulator (§4.12 Phase 1) — evidence that exists as files, never as rows; `working` = recent lightly-processed `memory_item` rows; `consolidated` = `memory_item` rows written by a consolidation action (MC-4), whose `content` carries their edges (MC-3). The quick-memory `MEMORY.md` (§4.11) is a separate always-injected file, not a depth tier. Refinement is one-way; the raw rollout files are append-only evidence and are never rewritten by a consolidation pass. | §4.12 owns raw/consolidated files; **net-new**: the explicit `depth` column + the intermediate `working` tier + the never-rewrite-raw guard |
| **MC-2** Scheduled incremental idempotent consolidation | Extends §4.12. A per-role **checkpoint watermark** (max `created_at` consumed) is persisted; each pass reads only inputs with `created_at >` watermark (incremental). The existing Phase 1 lease/claim and Phase 2 global lock give concurrency-idempotency; this layer adds **failed-not-checkpointed**: the watermark advances only over inputs whose consolidation committed, so a failed input is reprocessed next pass. No changed input, or no generator → successful no-op. | §4.12 owns the two-phase pipeline + locks; **net-new**: the incremental watermark and the failed-not-checkpointed contract |
| **MC-3** Write-time additive-only relationship binding | Extends §4.7. Edges are authored **in the consolidated item's own content** (that `content` is the item's source of truth per MEM-4); the `memory_fact_entity` / typed-predicate rows are a rebuildable projection of the in-text edges, never an independent truth. Two edge classes: a mandatory **provenance edge** (`derived-from`) from each consolidated node to its raw/working source, and open typed **relationship edges** (`relates-to`/`depends-on`/…). Binding happens at the consolidation write via a recall-for-linking step (candidate retrieval → classify same/related/unrelated → write). **Additive-only** is mechanically enforced by the MC-9 conservation check: a prose edit may not drop an existing edge. | §4.7 owns shallow entity links; **net-new**: typed predicates, the mandatory provenance edge, write-time binding, and additive-only enforcement |
| **MC-4** Consolidation action algebra | The Phase 2 diff-driven sub-agent (§4.12) already "eliminates duplicates, resolves contradictions"; this layer formalizes that into the closed set **create / corroborate / refine / correct** and records the chosen action on the item as capture provenance. Distinct from and upstream of MI-4 adjudication: a clean `correct` stays here; a genuinely contested one escalates to the intelligence layer's conflict report. | §4.12 owns dedup/contradiction resolution; **net-new**: the closed action taxonomy recorded per write |
| **MC-5** Fact vs derived layer | **Net-new**: a separate `memory_signal` table (per scope), rows keyed by `memory_item.id`, each carrying `signal_kind` (`centrality`/`cluster_id`/`recency`/`archived`), a `value`, a **`version`**, and a `computed_at`. Recall (§4.2) reads it; on absent, stale, or version-incompatible signal it substitutes a **neutral multiplier of 1** (MC-8) and logs a warning. A cold corpus with an empty `memory_signal` table is a fully supported state yielding base text-relevance ranking. Signals are never written into an item's `content` (the authored fact layer, MEM-4). | **net-new** — the hard boundary the substrate lacks |
| **MC-6** Corpus-maintenance action set | **Net-new** scheduled maintenance pass with four actions, each its own cadence and gate (§4.6): **merge** (elevated gate — multi-sample agreement; miss → surface to MI-4; transactional reference rewrite per MC-9), **split** (additive, model-based dispersion trigger), **archive** (recency threshold cushioned by centrality → sets the MI-9 `archived` state, reversible, auto-thaw on touch), **summarize** (MC-7). An `anti_cycle` cooldown row per node blocks oscillation. Every action appends to a `maintenance_audit` (action/targets/actor/instant). | **net-new**, composing §4.4 archivist cadence and the MI-9 lifecycle state |
| **MC-7** Emergent topic-cluster abstraction | **Net-new**: a periodic community-detection pass over the fact-layer edge graph (the MC-3 edges). A cluster that is stable, active, large, diverse, and lacks an edge-coverage hub gains a synthesized **summary node** grounded strictly in its members (an edge to each member it rests on), size-bounded so it does not re-trigger split, and thereafter an ordinary node (no privileged type — hubs are recognized structurally by edge coverage, not a marker). | **net-new** |
| **MC-8** Multiplicative offline-precomputed ranking | **Composes `l2-memory-store` §4.2 — no edit to it.** §4.2's additive fuse remains the `base_text_relevance` factor; MC-8 multiplies it by the NEW derived factors (`centrality_factor`, `cluster_factor`) plus a derived `recency_factor`, each read from the `memory_signal` table (MC-5), defaulting to 1 when absent, so a near-zero factor vetoes. The recall path performs **no model call and no graph walk** — all factors are precomputed by the maintenance pass. §4.2's MMR diversity remains a post-fusion option; §4.2.2's opt-in recency is the no-consolidation fallback (§4.4). | **net-new** multiplicative wrap over the untouched §4.2 base relevance |
| **MC-9** Co-edit write safety | Every fact-layer write stamps the version it read (the §4.14 `created_at`/`superseded_at` pair plus a row `rowid` check) and re-checks on commit, re-planning against the latest content if the target changed underneath it. A **conservation check** (post-write edge set satisfies the MC-3 additive rule / the MC-6 action's edge contract) runs before commit. Multi-item actions (merge, split) run inside a single SQLite transaction — commit whole or roll back whole. A failed check or exhausted retry refuses the write and surfaces it. | **net-new**, grounded in SQLite transactions + the §4.14 timestamps |
| **MC-10** Interest extraction advisory | The consolidation pass, having just processed the recent window, emits at most a few interest topics (title, one-line rationale, supporting item ids), deduplicated against a recent diversity window, into a read-only `interest_topics` projection. It is generator-free to read; `l1-inner-monologue` applies its own thresholds to decide whether/when to surface. This layer never interrupts the user. | **net-new** read-only hand-off surface |

## 4. Detailed Design

### 4.1 Where this sits (behind the seam)

```text
[REFERENCE] tier placement (l2-crate-topology)
  domain tier      : consolidation policy (which action, which gate) — pure, no I/O
  adapter tier     : this layer's SQLite reads/writes, via the store, behind UserDataStore
  scheduler        : fires passes (l1-scheduler-model) — never the hot path
```

The synchronous `remember`/`recall`/`answer` paths are unchanged and untouched. This layer runs on the scheduler's cadence and, when it must mutate the fact layer (merge/split/summarize), does so through the store's write path under the MC-9 protocol.

### 4.2 The derived-signal store (MC-5, the load-bearing new table)

```sql
-- [REFERENCE] per-scope; disposable, rebuildable, never authored fact
CREATE TABLE memory_signal (
    item_id     INTEGER NOT NULL,        -- → memory_item.id
    signal_kind TEXT    NOT NULL,        -- centrality | cluster_id | recency | archived
    value       REAL    NOT NULL,        -- numeric; cluster_id stored as a stable REAL key
    version     INTEGER NOT NULL,        -- signal-algorithm version
    computed_at INTEGER NOT NULL,        -- unix secs
    PRIMARY KEY (item_id, signal_kind)
);
```

Recall loads a hit's signals in one indexed lookup; a row whose `version` does not match the recall path's expected version, or that is missing, contributes a factor of `1.0` (MC-8) — so a schema/algorithm bump degrades ranking gracefully rather than breaking recall, and a never-yet-maintained corpus ranks on base text relevance alone. Dropping the whole table is a valid reset; it costs ranking quality, never data.

### 4.3 The maintenance pass (MC-6/MC-7/MC-8-signals)

One scheduled entry point runs the actions in a fixed order so each sees the previous one's output, and each is independently skippable when its gate or generator is unavailable:

```text
[REFERENCE] maintain(scope):
  1. recompute derived signals   -> memory_signal   (centrality over MC-3 edges; recency decay)
  2. cluster + summarize (MC-7)  -> new summary nodes, cluster_id signals
  3. split overloaded nodes      -> additive (MC-9), edges conserved
  4. merge redundant nodes       -> elevated gate; miss -> surface to MI-4; transactional (MC-9)
  5. archive stale nodes         -> set MI-9 archived; centrality-cushioned; auto-thaw on touch
  6. emit interest topics (MC-10)
  each step: append to maintenance_audit; respect anti_cycle cooldown
```

Confidence is proportional to blast radius (§L1 MC-6): `summarize`/`archive` auto-apply (additive/reversible); `merge` (lossy, rewrites references) requires multi-sample agreement and otherwise defers to a surfaced report rather than acting. Archive runs on a faster cadence than the graph-wide steps because it needs no clustering.

### 4.4 Ranking reconciliation with §4.2 (MC-8)

This spec **requires no change to `l2-memory-store` §4.2** and does not replace its recall entry point. The two operate at different levels and compose cleanly: `l2-memory-store` realizes `l1-memory-model` (MEM-3) and its additive fuse (vec KNN + FTS5 BM25 + utility + verification weight, cross-scope merge) remains authoritative for the **text factor**; this spec realizes `l1-memory-consolidation` (MC-8) and multiplies that text factor by the **new** derived signals it introduces — `centrality` and `cluster`, which §4.2 does not have — plus a `recency` factor, each read from the §4.2-*of-this-spec* `memory_signal` table (MC-5) and defaulting to 1 when absent:

```text
[REFERENCE]
final_score = base_text_relevance         // = l2-memory-store §4.2 additive fuse (unchanged)
            × centrality_factor            // NEW derived signal (memory_signal)
            × cluster_factor               // NEW derived signal (memory_signal)
            × recency_factor               // derived, centrality-cushioned (MC-6 archive)
```

Recency is the one signal that overlaps: `l2-memory-store` §4.2.2 already offers an **opt-in, off-by-default** additive `recency_weight`. The two are complementary rather than conflicting — when this consolidation layer is active its centrality-cushioned `recency_factor` is the operative recency signal and the store's additive recency term is held at 0, so recency is counted once; when the layer is not active, §4.2.2's knob is the untouched fallback. Because §4.2.2's recency is off by default, nothing about the Stable store's default behavior changes. The store's `mmr_lambda` diversity re-order remains available as a post-fusion option. The net effect is a stricter composition (a near-zero derived factor vetoes) with zero added hot-path cost, because every factor is precomputed.

### 4.5 Co-edit write safety (MC-9)

```text
[REFERENCE] fact-layer write (single item):
  read v := (rowid, created_at, superseded_at) of target
  plan the edit (MC-4 action or MC-6 maintenance)
  begin transaction
    re-read v'; if v' != v -> abort, re-plan against latest (bounded retries)
    conservation_check(post_edge_set)          // additive rule / action contract
      fail -> rollback, surface, never partial
    write the item's content (MEM-4 truth) + rebuild the edge projection
  commit
merge/split: same shape, all touched items in ONE transaction, whole-or-nothing
```

Optimistic concurrency (not locking) because human edits are rare and the cost of a lost update — a dropped edge, a clobbered correction — is high while the cost of a re-plan is cheap. The conservation check is mechanical (edge-set arithmetic), not a model call, so it is always affordable.

## 5. Implementation Notes

1. **The `memory_signal` table and the MC-8 multiplicative wrap ship first** — they are the smallest change that makes ranking honest about missing signals, and they are testable with an empty table (cold-start = base relevance).
2. **The `depth` column and the never-rewrite-raw guard** land next; they are a schema addition plus a write-path assertion, independent of the maintenance pass.
3. **Maintenance actions land in blast-radius order**: archive (reversible, no clustering) → split/summarize (additive) → merge (lossy, gated) last, so the riskiest action ships only once the audit and cooldown machinery is proven.
4. **Community detection** may reuse the clustering primitive shape from the code graph (union-find stub → a real community algorithm behind a seam), applied to the memory edge graph rather than code symbols — the algorithm is a seam, the stub is a valid first cut.
5. **Every generator-dependent step checks generator availability first** and defers (re-queues for the next pass) rather than partially writing when none is bound.

## 6. Drawbacks & Alternatives

- **A second signal store to keep fresh.** Mitigated by making it disposable and version-guarded (MC-5): staleness only softens ranking, and a full rebuild is always safe.
- **Multiplicative ranking can over-veto.** A genuinely isolated but relevant node is punished by a near-zero centrality factor. Mitigated by flooring each factor into a bounded band (an isolated node's centrality factor is small, not zero) and by keeping `base_text_relevance` the dominant term.
- **Alternative — extend `l2-memory-store` in place.** Rejected for flexibility and blast radius: the store is Stable at 950 lines and realizes `l1-memory-model`; folding a second and third L1 into it would balloon it past reviewable size and revert the substrate spec to RFC on every consolidation change. A separate L2 keeps the substrate stable and this layer independently evolvable, and — decisively — keeps the maintenance loop reachable only through the `UserDataStore` seam, so an alternative backend inherits the substrate contract without inheriting this SQLite realization.
- **Alternative — compute ranking signals at read time.** Rejected by MC-8/MEM-2: read-time centrality/clustering is a graph walk on the hot path. Precomputing into `memory_signal` is the whole point.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[CONCEPT]` | `.design/main/specifications/l1-memory-consolidation.md` | MC-1…MC-10, the invariants realized here |
| `[STORE]` | `.design/main/specifications/l2-memory-store.md` | The SQLite substrate composed: §4.2 fuse, §4.7 edges, §4.12 pipeline, §4.14 bi-temporal |
| `[TOPOLOGY]` | `.design/main/specifications/l2-crate-topology.md` | Why the maintenance loop lives behind the UserDataStore seam |
| `[SCHED]` | `.design/main/specifications/l1-scheduler-model.md` | Fires the periodic passes |
| `[INTEL]` | `.design/main/specifications/l1-memory-intelligence.md` | The query surface consuming these signals (its L2 sits above this) |

## Document History

| Version | Date | Notes |
| --- | --- | --- |
| 1.0.3 | 2026-09-23 | Consistency pass (2026-09-23): Remaining `notes/*.md` references removed (MC-5 row, §4.5 write step) — v1.0.2 fixed only MC-3. MC-1 said `consolidated` = the MEMORY.md file while MC-3 edges live in the consolidated item's content: consolidated now means memory_item rows written by a consolidation action, raw exists only as files, MEMORY.md is not a depth tier. With the layer active the store's additive recency term is held at 0 so recency is not counted twice. |
| 1.0.2 | 2026-07-15 | Patch: corrected the MC-3 parenthetical that referenced `notes/*.md` as the source of truth — after l1-memory-model MEM-4 v1.1 the item's own `content` is authoritative (the learned corpus is store-authoritative, not note-authoritative). MC-3 logic unchanged: edges live in `content`, the relational rows remain a rebuildable projection. Reference correction only, no logic change. |
| 1.0.1 | 2026-07-11 | Promoted RFC→Stable via Post-Update Review (spec-critic + prompt-engineer PASS). One review finding fixed: §4.4/MC-8 had overclaimed that it "subsumes" `l2-memory-store` §4.2.2's recency knob — reworded to make the relationship explicitly compositional (§4.2's additive fuse is the untouched `base_text_relevance` factor; MC-8 multiplies the NEW derived signals over it, realizing a *different* L1 than §4.2 does), so no Stable-spec contradiction exists and `l2-memory-store` needs no edit. No other change. Stable = design agreed; concrete schemas/algorithms validated during implementation. |
| 1.0.0 | 2026-07-11 | Initial RFC — local SQLite realization of MC-1…MC-10 as a maintenance tier composing `l2-memory-store` (no new engine). Adds the processing-depth column (MC-1), incremental/failed-not-checkpointed watermark over the §4.12 pipeline (MC-2), typed additive-only edges with mandatory provenance over §4.7 (MC-3), the closed consolidation action algebra (MC-4), the load-bearing `memory_signal` fact-vs-derived table with version-guarded neutral-default degradation (MC-5), the four-action maintenance pass with blast-radius-proportional gates + anti-cycle cooldown + audit (MC-6), edge-coverage-structural emergent summary nodes (MC-7), the multiplicative offline-precomputed ranking wrapping §4.2's base relevance (MC-8), optimistic-concurrency + conservation-check + transactional co-edit safety (MC-9), and the read-only advisory interest-topic surface (MC-10). Sits behind the `UserDataStore` seam per `l2-crate-topology`, keeping the substrate backend-swappable. |
