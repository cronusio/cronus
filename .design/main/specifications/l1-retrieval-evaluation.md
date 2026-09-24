# Retrieval Evaluation

**Version:** 1.0.0
**Status:** Stable
**Layer:** concept

## Overview

Retrieval evaluation is the measurement subsystem for **ranked-recall quality**. Cronus has many surfaces that answer a query with an *ordered list of results* — memory recall fusion, knowledge-base RAG, code-index symbol search, the model-router's semantic cache. Every one of them ranks; none of them is measured. This spec defines a technology-neutral way to score any such surface against a human-labeled fixture using standard information-retrieval (IR) metrics, persist baselines, and gate regressions — so a change to ranking is *A/B-measured, not vibe-checked*.

The core loop: a version-controlled fixture pairs each representative query with the items a human labeled relevant (optionally graded); each query is run through the **real production pipeline**; the ordered results are scored with precision@K, recall@K, MRR, and nDCG@K; the run is persisted with provenance and reported as deltas against the previous baseline. A regression beyond a configured threshold blocks the change.

This is a distinct member of the evaluation family: [l1-evaluation-suites.md](l1-evaluation-suites.md) measures *agent-customization behavior* (skills/roles/workflows), [l1-practice-analytics.md](l1-practice-analytics.md) coaches *session conduct*, and this spec measures *ranking quality of a recall component*. None of the three substitutes for the others.

## Related Specifications

- [l2-memory-store.md](l2-memory-store.md) — primary subject under test: the recall fusion pipeline (§4.2) is the first surface to acquire a fixture + baseline; the whole utility-learning loop assumes recall is good but never proves it.
- [l1-memory-model.md](l1-memory-model.md) — MEM-3 hybrid recall is the contract this evaluation scores.
- [l1-knowledge-base.md](l1-knowledge-base.md) / [l2-knowledge-store.md](l2-knowledge-store.md) — RAG recall is a second ranked-recall surface evaluable by the same harness.
- [l1-code-intelligence.md](l1-code-intelligence.md) — code-index symbol search (CI-8 hybrid retrieval) is a third subject.
- [l2-model-router.md](l2-model-router.md) — the semantic-cache pool is a ranked-recall surface; eval can validate cache hit quality.
- [l1-evaluation-suites.md](l1-evaluation-suites.md) — sibling eval concept (customization behavior); this spec reuses its baseline-comparison + regression-gate discipline (ES-9) for retrieval metrics.
- [l1-practice-analytics.md](l1-practice-analytics.md) — sibling diagnostic; honest data-gap accounting (PA-7) and minimum-sample gating (PA-8) inform RE-8/RE-9.
- [l1-quality-standards.md](l1-quality-standards.md) — the regression gate (RE-6) is a quality gate consumable as a definition-of-done check.

## 1. Motivation

A ranked-recall surface can silently degrade in ways no test catches: a tweak to fusion weights, a new embedding model, an added re-ranker, or organic data growth can each lower the rank of the right answer without any error. Because the memory subsystem's *entire* value-learning machinery (utility scoring, Bellman propagation, decay) is premised on recall surfacing the right episodes, an unmeasured recall pipeline is an unfalsifiable claim of usefulness.

IR has a settled answer: label a fixture of `(query → relevant items)`, run the real retriever, and compute rank-aware metrics. P@K and R@K capture "are the right items in the top K"; MRR captures "how high is the first right item"; nDCG@K captures "is the *ordering* good, weighted by graded relevance." Persisting baselines turns ranking work from guesswork into a tracked delta, and a regression gate stops a recall drop from shipping.

Today no Cronus spec defines this. The result is a sophisticated recall stack with no quality instrument. This concept supplies one, defined once over the abstract ranked-recall contract so every surface reuses it.

## 2. Constraints & Assumptions

- **On-device.** Evaluation runs locally; no query, fixture, or result leaves the device.
- **Fixtures are owned artifacts.** They live in version control beside the code, are human-curated, and must be free of sensitive content (or kept out of VCS). The fixture is ground truth; the system never grades against its own output.
- **Real pipeline, not a stub.** Metrics must reflect the production ranker and its active configuration/mode, or they measure nothing useful.
- **Small fixtures are noisy.** 20–50 queries with 2–3 labels each is the working range; metrics on tiny fixtures are indicative, not authoritative (RE-9 minimum-sample caution).
- **Labels drift.** As stored items churn, labeled IDs can disappear; the harness tolerates prefix matching and assumes periodic relabeling.

## 3. Core Invariants

Layer 2 implementations MUST NOT violate these. They are technology-neutral.

- **RE-1 — Labeled fixture as ground truth.** Evaluation is driven by a version-controlled fixture of `(query → relevant items)` pairs. Each query has a stable id; each relevant item may carry a graded relevance; queries may carry an optional filter scope and free-form slicing tags. The fixture is human-authored ground truth — the system never scores against its own ranking.
- **RE-2 — Production pipeline under test.** Each fixture query runs through the *exact* production retrieval pipeline with its active configuration, not a reimplementation or a frozen copy. The score reflects what a user would actually receive.
- **RE-3 — Standard IR metric set, no opaque single score.** Each run reports a fixed set of rank-aware metrics at a cutoff K: precision@K, recall@K, MRR, and nDCG@K. They are reported separately; the system never collapses them into one unexplained number.
- **RE-4 — Graded relevance, binary-compatible.** Relevance labels are graded (a small ordinal scale from irrelevant to highly-relevant) for nDCG, with a default that degrades cleanly to binary relevance when grades are omitted.
- **RE-5 — Baseline persistence + delta reporting.** Every run is persisted with full provenance — timestamp, code version, fixture identity, K, retrieval mode, and both per-query and aggregate results. A run is reported as signed deltas against the most recent baseline, so improvement and regression are explicit.
- **RE-6 — Regression gate.** A configurable threshold on a primary metric (e.g., a P@K drop beyond X) gates retrieval changes; the gate is overridable only with an explicit, recorded justification. A silent recall regression must not ship.
- **RE-7 — Mode/variant comparability.** Each run records the retrieval mode/variant under test so competing strategies (lexical / semantic / hybrid / re-ranked) are A/B-comparable on the same fixture under the same metrics and cutoff.
- **RE-8 — Fault isolation.** A failure on a single query is recorded as a zero score for that query and never aborts the run. Partial measurement is preferred over no measurement; the count of evaluated queries is always reported.
- **RE-9 — Sliceable, sample-honest aggregates.** Per-query tags let aggregates be sliced (e.g., literal-token vs conceptual vs multi-aspect queries) to localize weakness. Aggregates disclose the query count; results over fixtures below a minimum sample size are flagged as low-confidence, never presented as authoritative.
- **RE-10 — Surface-agnostic applicability.** The harness is defined over the abstract ranked-recall contract — `query → ordered list of item ids` — so it applies uniformly to any recall surface (memory recall, RAG, code search, semantic cache), not to one store.
- **RE-11 — Privacy-aware, on-device.** Evaluation, fixtures, and baselines stay on-device; fixtures must exclude sensitive content. No metric, query, or label is egressed.

## 4. Concept Detail

### 4.1 Fixture model

A fixture is a line-oriented set of labeled queries (comments and blank lines ignored):

```text
[REFERENCE] one labeled query per line
{
  id:       "q001",                       // stable id, used in per-query diffs
  query:    "fix login redirect bug",     // sent verbatim to the retriever
  relevant: [ {id: "abc123", grade: 3},   // graded 0-3; default 3 (binary-compatible)
              {id: "def456", grade: 2} ],
  scope:    "myproject",                  // optional filter mirroring the live query
  tags:     ["security","auth"],          // optional, for sliced aggregates (RE-9)
  notes:    "open-redirect class"         // optional labeler rationale
}
```

Authoring guidance (non-normative): pick 20–50 queries representative of real questions, mixing **literal-token** queries (names, error codes — stress lexical ranking), **conceptual** queries ("how do I X" — stress semantic ranking), and **multi-aspect** queries (stress re-ranking). Two or three labels per query suffice for P@5/R@5; exhaustive labeling is not required.

### 4.2 Metrics

Computed at cutoff K (default 5), per query then averaged:

```text
[REFERENCE]
P@K   = |relevant ∩ retrieved[..K]| / K           // divides by K → penalizes under-retrieval
R@K   = |relevant ∩ retrieved[..K]| / |relevant|  // coverage of the relevant set
RR    = 1 / rank_of_first_relevant (1-indexed), else 0 ;  MRR = mean(RR) over queries
nDCG@K = DCG@K / IDCG@K,  gain = 2^grade − 1,  discount = log2(position + 2)
```

Each metric isolates a failure mode: P@K (precision of the top slice), R@K (did we find the relevant set at all), MRR (is the first hit near the top), nDCG@K (is the *ordering* right under graded relevance). They are reported together (RE-3).

### 4.3 Run record + baseline lifecycle

A run produces a record `{timestamp, code_version, fixture, K, mode, aggregate, per_query[]}` persisted to a baselines store. The lifecycle:

```text
[REFERENCE]
eval baseline --fixture F   → run, persist as the reference point
eval run      --fixture F   → run, diff aggregate vs latest baseline, print signed deltas (↑green / ↓red)
eval run      --fixture F --save  → run, diff, and promote to new baseline
```

Baselines are provenance-stamped (code version + timestamp) and ordered chronologically so "latest" is well-defined. Deltas make a ranking change's effect legible at a glance.

### 4.4 Regression gate (RE-6)

A primary metric and a tolerance are configured (e.g., "fail if P@5 drops more than 2 points vs baseline"). On a gated run that breaches tolerance, the change is blocked; an override requires an explicit recorded reason (mirroring the quality-pipeline override convention). This is the mechanism that lets retrieval changes flow through CI without fear of silent recall loss.

### 4.5 Applicability across surfaces (RE-10)

The harness only needs the ranked-recall contract `query → [item_id…]`. Each Cronus recall surface supplies an adapter and its own fixture:

| Surface | Ranked output | Fixture labels |
| --- | --- | --- |
| Memory recall | fused episode list | query → relevant memory ids |
| Knowledge-base RAG | chunk list | question → relevant chunk ids |
| Code-index search | symbol list | intent → relevant symbol ids |
| Router semantic cache | cache-hit candidates | prompt → acceptable cache ids |

One metric definition, one baseline/gate discipline, many subjects.

## 5. Ideas to Adopt

| Mined mechanic | Adoption in Cronus |
| --- | --- |
| IR-metric retrieval eval (P@K/R@K/MRR/nDCG@K) against a labeled fixture | **[new]** this spec (RE-1…RE-4); first subject is `l2-memory-store` recall fusion (§4.2). |
| Baseline persistence + signed-delta reporting | **[new]** RE-5; reuses the baseline-comparison discipline already in `l1-evaluation-suites.md` (ES-9), specialized to ranking metrics. |
| Regression gate on a primary metric with override | **[new]** RE-6; a quality gate in the `l1-quality-standards.md` family; mirrors the quality-pipeline override-with-reason convention. |
| Retrieval-mode A/B comparability | **[new]** RE-7; lets `l2-memory-store` / `l2-model-router` compare lexical vs semantic vs hybrid objectively. |
| Per-query tag slicing + sample-honesty | RE-9; reuses honest data-gap accounting (PA-7) and minimum-sample gating (PA-8) from `l1-practice-analytics.md`. |
| Surface-agnostic ranked-recall contract | RE-10; one harness over `l2-memory-store`, `l2-knowledge-store`, `l2-codegraph`, `l2-model-router`. |
| MMR diversity + opt-in recency in recall ranking | **[applied separately]** added to `l2-memory-store.md` §4.2.2 — concrete ranking refinements whose effect this harness can now measure. |
| Trend charting of eval metrics over time | `l2-dashboard.md` runtime analytics can chart P@K/nDCG trends per surface as a health signal. |
| Dream-cycle refinements (Wilson-CI contradiction rate, agglomerative reflection clustering, day-level triage-verdict cache, triage-gated reflection authorship) | **Already covered conceptually** by the archivist phases in `l2-memory-store.md §4.4` (reconcile / distill) and `l2-self-improvement.md`; recorded as available implementation refinements, not new requirements (no duplication). |

## 6. Nodus Relevance

The IR metrics themselves do not apply to nodus — a workflow DSL is deterministic, not a ranked retriever. What transfers is the **methodology**: fixture-driven, baseline-persisted, regression-gated evaluation. `l1-nodus-testing.md` already defines `@test:` blocks with input/expected/graders and a route-coverage advisory; the missing pieces this reference suggests are (a) **persisted baselines** of a suite's pass-rate and route-coverage, and (b) a **regression gate** that fails a workflow change when coverage or pass-rate drops versus the last baseline — the same "measure against a stored baseline, block silent regressions" discipline as RE-5/RE-6, applied to deterministic suite outcomes rather than ranking metrics. The nodus workspace owns any realization.

## 7. Drawbacks & Alternatives

- **Labeling cost.** Fixtures require human judgment to build and maintain. Mitigation: small fixtures (2–3 labels/query) are enough for P@5/R@5; labeling can grow incrementally.
- **Fixture staleness.** As stored items churn, labeled ids can vanish, deflating metrics. Mitigation: prefix-id matching, periodic relabel, and treating absolute numbers as less important than deltas on a stable fixture.
- **Over-fitting to the fixture.** Optimizing ranking against one fixture can overfit. Mitigation: multiple fixtures, tag slices (RE-9), and rotating held-out queries.
- **Alternative — online/implicit signals only.** Click/feedback utility (already in the memory store) measures *in-situ* usefulness but is confounded and slow; offline fixture eval gives a fast, controlled, repeatable signal for ranking changes. The two are complements, not substitutes.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[MEMORY]` | `.design/main/specifications/l2-memory-store.md` | First subject under test (recall fusion §4.2) |
| `[EVAL-SUITES]` | `.design/main/specifications/l1-evaluation-suites.md` | Sibling eval concept; baseline/regression discipline (ES-9) |
| `[QUALITY]` | `.design/main/specifications/l1-quality-standards.md` | The regression gate as a definition-of-done check |

## Document History

| Version | Date | Change |
| --- | --- | --- |
| 1.0.0 | 2026-06-26 | Initial specification: the measurement subsystem for ranked-recall quality — labeled query fixtures, information-retrieval metrics, and a regression gate. |
