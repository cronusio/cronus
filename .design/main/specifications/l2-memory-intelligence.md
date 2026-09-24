# Memory Intelligence (Local Realization)

**Version:** 1.0.1
**Status:** Stable
**Layer:** implementation
**Implements:** l1-memory-intelligence.md

## Overview

The concrete realization of the active query & intelligence surface (MI-1…MI-13) over the local SQLite substrate of [l2-memory-store.md](l2-memory-store.md) and the maintenance layer of [l2-memory-consolidation.md](l2-memory-consolidation.md). This is the **caller-facing** tier of the memory subsystem: the operations a worker actually invokes — `answer`, temporal recall, conflict adjudication, the intelligence digest, experience reuse, lifecycle control — expressed as a bounded contract on top of the store's passive read/write.

Like its sibling, it adds **no storage engine**: it reaches persistence only through the `UserDataStore` seam (`l2-crate-topology`), reads the derived ranking signals the consolidation layer precomputes (MC-8), and composes existing engines — knowledge-base attribution (KB-6), claim verification (CV-3/CV-4), the scheduler, the dashboard — rather than forking them. Keeping the surface behind the seam is the load-bearing design choice: the intelligence API is substrate-agnostic, so an alternative `UserDataStore` backend inherits the whole query surface without inheriting this SQLite realization.

This spec also **fixes the two parameters `l1-memory-intelligence` deferred to L2**: the MI-4 ambiguity threshold (§4.3) and the MI-5 digest cadence (§4.4).

## Related Specifications

- [l1-memory-intelligence.md](l1-memory-intelligence.md) - The concept realized here; MI-1…MI-13.
- [l2-memory-store.md](l2-memory-store.md) - The substrate composed: recall fusion (§4.2), write path (§4.3), archivist reconcile (§4.4), trust scoring (§4.6), bi-temporal record + point-in-time query methods (§4.14), typed `MemoryKind`/`MemorySource` + `confidence` (§4.15).
- [l2-memory-consolidation.md](l2-memory-consolidation.md) - The maintenance layer below: supplies the `memory_signal` derived layer this surface's ranking reads (MC-8) and owns the MC-4 write algebra upstream of MI-4 adjudication.
- [l1-knowledge-base.md](l1-knowledge-base.md) - Source-attribution contract (KB-6) reused by the `answer` projection (MI-1).
- [l1-claim-verification.md](l1-claim-verification.md) - Ternary grounding gate (CV-3/CV-4) that keeps `answer` honest (MI-1).
- [l1-scheduler-model.md](l1-scheduler-model.md) - Fires the periodic intelligence digest (MI-5).
- [l1-dashboard.md](l1-dashboard.md) - Read-only projection host for the digest analytics (MI-5).
- [l1-security.md](l1-security.md) - SEC-9/SEC-10 authority gate a reused experience still passes (MI-13).
- [l1-harness-engineering.md](l1-harness-engineering.md) - Artifact-extraction discipline (HE-8) the procedural-distillation operation composes with (MI-7).
- [l2-crate-topology.md](l2-crate-topology.md) - Why the surface is realized behind the `UserDataStore` seam, not welded into the store.

## 1. Motivation

`l2-memory-store` returns ranked items; a caller still has to phrase queries, parse hits, reconcile stale-vs-current, notice contradictions, and reconstruct "what changed since last session." This spec lifts those recurring operations into a uniform contract so behavior is consistent across callers instead of re-implemented per agent. It is deliberately a thin composition layer: every operation binds an existing engine to the memory store as its data source.

## 2. Constraints & Assumptions

- **No storage, no new engines.** Reads and annotates through the `UserDataStore` seam; composes knowledge-base, claim-verification, scheduler, dashboard.
- **Hot-path discipline.** `recall` stays cheap; temporal modes and the `answer` projection add no unbounded cost, and ranking reads the precomputed `memory_signal` layer (never a graph walk at query time).
- **Local-first, generator-optional.** `answer` degrades to attributed extractive recall, temporal normalization to verbatim, capture to `raw` mode — all functional with no model bound.
- **Budget- and consent-bounded generation.** Digests and answers carry no secret or raw-transcript egress beyond what the source items already permit.
- **Substrate-agnostic by construction.** The surface names no SQLite type across the seam; it is realized here against SQLite but the contract is backend-neutral.

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Realization | Already owned vs net-new |
| --- | --- | --- |
| **MI-1** Grounded answer | An `answer(query)` projection distinct from `recall`: retrieve a grounding set via §4.2 fusion → synthesize grounded **only** in the retrieved items → attach citations via the KB-6 source-attribution contract → pass the CV-3/CV-4 claim-verification gate; `contradicted`/`unverifiable` → return an honest insufficient outcome. No generator → return the top attributed items verbatim (extractive). Never asserts beyond retrieved memory; never falls back to model priors. | §4.2 recall owned; **net-new**: the projection binding KB-6 + CV-3/CV-4 to the store as grounding source |
| **MI-2** Temporal recall modes | Three query modes over §4.14's bi-temporal record, reusing its `was_valid_at`/`was_current_at`/`is_current` methods: `as-of T` (valid-time containing T), `changed-since C` (transaction-time `created_at`/`superseded_at` > C), `recent N` (transaction-time desc). Compose with the type/scope/tag filters and the MI-8 predicate; a checkpoint C is a caller-held opaque instant enabling `changed-since(last_session)` session restoration. | §4.14 owns the record + methods; **net-new**: the three first-class modes + checkpoint handling |
| **MI-3** Immediate recall-visibility | The §4.3 write path makes an item recall-visible on commit (the FTS5/tags rows are written synchronously); embedding, entity extraction, and consolidation (MC-2) are deferred/async and never gate visibility. Missing enrichment degrades ranking (a not-yet-embedded item still matches lexically), never availability. | §4.3 owns synchronous upsert; **net-new**: the explicit "enrichment never blocks the write" guarantee (embedding may be deferred to the async pass) |
| **MI-4** Conflict surfacing | The §4.4 archivist reconcile stage detects contradictions; this surface routes them. Classify into `contradiction`/`update`/`duplicate`/`conflict`; **unambiguous** → auto-supersede via §4.14 supersession (`superseded_at`), recorded non-destructively; **ambiguous** → a structured conflict report carrying the closed `keep-new`/`keep-old`/`merge`/`drop` set, `status = awaiting-adjudication`. The threshold that splits the two is **fixed here** (§4.3), discharging the L1 deferral. | §4.4 owns detection, §4.14 owns supersession; **net-new**: routing + report + the concrete ambiguity threshold |
| **MI-5** Periodic intelligence digest | A scheduler-fired (`l1-scheduler-model`), read-only job over a time window: narrative (the MI-1 `answer` projection over the window's items) + analytics (activity/type/confidence-trust distributions, rendered by `l1-dashboard`) + the MI-4 conflict report for the window. Budget-bounded, derived only from stored memory, itself storable as a `context`/`event` memory (§4.15 kinds); mutates nothing it reads. Cadence **fixed here** (§4.4). | scheduler + dashboard owned; **net-new**: the digest composition + the concrete cadence |
| **MI-6** Salience-gated capture policy | Policy over the store's write-time `confidence` + typed `MemorySource`/`MemoryKind` (§4.15) + semantic dedup (§4.3) + trust (§4.6): selective, confidence-honest (below-floor → not stored or provisional), de-duplicated, **actor-attributed** (who-said-it, distinct from scope ownership), optionally **explicitly-expirable** (a hard void-after-date complementing MEM-5 decay), **cross-referenced** at capture (naming related item ids, cheap forward MC-3 edges), and **subject-attributed** (about-user vs about-agent-self). Each optional field degrades to baseline when absent. | §4.3/§4.6/§4.15 own the fields; **net-new**: the actor/expiry/subject metadata + the explicit capture policy |
| **MI-7** Procedural distillation | An explicit end-of-run `distill_run(trace)` operation writing **one** procedure-kind memory (§4.15) — objective / actions / findings / end-state / next-steps — grounded only in the run's own trace (no invention), recallable like any item, tagged to distinguish "distilled procedure" from "captured fact". Opt-in, sibling to ordinary MI-6 capture (which continues throughout the run), composes HE-8. | §4.15 kinds owned; **net-new**: the end-of-run distillation operation |
| **MI-8** Structured filter predicate | A closed comparison vocabulary — `equals`/`not-equals`, ordering (`gt`/`ge`/`lt`/`le`), `in`/`not-in`, `contains`/`icontains`, combined with `AND`/`OR`/`NOT` — compiled to a parameterized SQL `WHERE` over `memory_item` columns; a combinator a backend cannot express natively (deep `NOT`/`OR` nesting) is evaluated post-fetch, never silently dropped. Composes with — never replaces — the §4.2 fuzzy fusion and the MI-2 temporal modes (narrow by predicate, then rank). | §4.1 schema owned; **net-new**: the closed predicate layer + its SQL translation |
| **MI-9** Reversible lifecycle states | A `lifecycle_state` column (`active`/`paused`/`archived`/`deleted`) on `memory_item`; transitions append to an audit (actor, instant, old→new). `archived` is the state the MC-6 maintenance pass sets and the consolidation layer auto-thaws on touch; `paused` is the user-driven mute. Recall (§4.2) defaults to `active`; temporal/structured modes may opt into paused/archived. **Prune-protective**: MEM-5 decay may lower ranking but MUST NOT delete a `paused`/`archived` item. | **net-new** column + state machine; composes MC-6 archive and the §4.14 supersession |
| **MI-10** Capture-time temporal normalization | At capture, relative expressions in the content ("last week") are rewritten to absolute dates against the **observation instant** (defaulting to write time), normalizing the MEM-4 source text; distinct from and complementary to the §4.14 bi-temporal metadata. No generator → store verbatim (never fabricate a date). | **net-new** capture step |
| **MI-11** Caller capture directives | Optional caller-scoped `include`/`exclude`/`custom-instruction` inputs steering extraction emphasis, recorded as capture provenance; they never lower the MI-6 honesty floor nor suppress a safety-relevant fact. Absent directives → baseline MI-6. | **net-new** directive inputs on the capture path |
| **MI-12** Raw vs inferred capture | A per-write mode flag: `inferred` (model extracts salient facts; default) or `raw` (verbatim, no model). Raw is the local-first / audit-exact / no-generator escape hatch and MUST function with no model bound. Both produce ordinary items — uniform recall/lifecycle/trust/decay. Complements MI-3: MI-3 makes enrichment async, MI-12 makes it optional. | **net-new** write-mode flag; raw path reuses §4.3 minus the embed/extract step |
| **MI-14** Anchored neighbourhood recall | A `neighbourhood(anchor, depth)` mode beside similarity (MI-1) and temporal-window (MI-2) recall: resolve the anchor (a record id, or the top hit of a query), then return the records immediately before and after it by transaction time up to the declared `depth` (bounded by a configured maximum), interleaving every record kind in one chronological sequence; each item carries its kind and its offset from the anchor, and the anchor is present and marked. Served by an index scan on `created_at` around the anchor — never by re-ranking a similarity result. Pending realization. | §4.14 transaction-time record owned; **net-new**: the anchored window mode |
| **MI-13** Experience reuse | A recall-before-acting projection: query typed (`success`/`failure`/`insight`), quality-scored prior experiences (MI-2/MI-8 filters over procedure-kind items from MI-7); a sufficiently-similar high-quality **prior success** is reused directly **only** when a reuse gate (similarity ≥ σ **and** score ≥ τ **and** freshness **and** not safety-sensitive) passes; a **failure** injects as an avoid-signal (never reused); an **insight** injects as guidance. Every outcome is captured back typed+scored (MI-6/MI-7). Four guards: gated reuse, independent read/write, reused-not-re-derived attribution (MI-1 honesty), retained authority gate (a reused plan still passes SEC-9/SEC-10). | **net-new** projection composing MI-1/MI-2/MI-6/MI-7/MI-8 + the security gate |

## 4. Detailed Design

### 4.1 The three operations and where they run

`remember` (write, §4.3) and `recall` (ranked read, §4.2) are the substrate's. This surface adds **`answer`** as their sibling and hosts the periodic/enrichment operations. All of it sits behind the `UserDataStore` seam; the synchronous ones (`answer`, temporal recall, structured filter, lifecycle transitions, `distill_run`, experience-reuse read) are caller-invoked, and the periodic one (digest) is scheduler-fired. None runs a graph walk at query time — ranking factors come from the consolidation layer's `memory_signal` table (MC-8).

```text
[REFERENCE] answer(query):
  set   := recall(query)                       // §4.2 fusion + MC-8 multiplicative signals
  if insufficient(set): return Unverifiable{ missing }        // MI-1 honest refusal
  draft := synthesize(set)                      // grounded ONLY in `set`
  cite  := attribute(draft, set)                // KB-6 source attribution
  gate  := verify(cite)                         // CV-3/CV-4 ternary
  match gate { Supported => Answer{cite}, _ => Unverifiable }
  // no generator -> return top attributed items of `set` verbatim (extractive)
```

### 4.2 Temporal modes over the bi-temporal record (MI-2)

The three modes are thin projections of §4.14's existing methods — no new temporal logic:

| Mode | Predicate | §4.14 method |
| --- | --- | --- |
| `as-of T` | current-at-T records whose valid window contains T | `was_valid_at(T)` + `was_current_at(T)` |
| `changed-since C` | `created_at > C OR superseded_at > C` | transaction-time scan |
| `recent N` | newest N by `created_at` | transaction-time desc |

A checkpoint `C` is an opaque caller-held instant; `changed-since(last_session_start)` yields exactly the session-restoration delta.

### 4.3 Conflict adjudication threshold (MI-4) — discharging the L1 deferral

`l1-memory-intelligence` §4.3 left the ambiguous/unambiguous cutoff to L2. Fixed here, using the store's existing `confidence` (§4.15) and `trust_score` (§4.6):

```text
[REFERENCE] classify(old, new):   // both concern the same subject/entity; first matching rule wins
  duplicate  : semantic_sim(old,new) >= DEDUP_SIM         (§4.3 dedup threshold)   -> auto
  update     : new.valid_at strictly after old.valid_at
               AND new.confidence >= old.confidence        -> auto-supersede old (recency-dominant)
  otherwise  : AMBIGUOUS -> surface, status = awaiting-adjudication
               recommendation = merge     when |Δconfidence| < CONF_GAP_MIN (default 0.15)
                                          AND |Δtrust_score| < TRUST_GAP_MIN (default 0.15)
                              = keep-new  otherwise (a lead exists, e.g. newer but less confident)
               // the gaps choose the recommendation shown to the adjudicator; they never auto-apply it
```

So the split is **objective, local and total**: every pair lands in exactly one branch, and a contradiction is never left standing unrecorded (MEM-6). A clear duplicate or a strictly-newer higher-or-equal-confidence statement auto-supersedes (recorded via §4.14 `superseded_at`); everything else — including a newer statement with lower confidence, where recency and confidence point in opposite directions — surfaces to the report with a recommendation, and a person or the adjudicating agent decides. `CONF_GAP_MIN`/`TRUST_GAP_MIN` are config-tunable; the defaults bias toward auto-supersede so only real disagreement surfaces (drawback: surfacing fatigue, §6).

### 4.4 Digest cadence (MI-5) — discharging the L1 deferral

Fixed here: **default cadence is per-session-close, plus a daily floor** (if no session closed in 24 h, run once), and the digest is **opt-in per office** (off by default; enabling it is an office-config choice, not a global default). Rationale: session-close is when a window of activity is genuinely complete and worth summarizing; the daily floor keeps a long-running office from never producing one; opt-in avoids spending generation budget for offices that never read digests. The scheduler owns firing; the digest job is read-only and budget-bounded.

### 4.5 Structured predicate compilation (MI-8)

The closed vocabulary compiles to a parameterized SQL `WHERE` fragment over indexed `memory_item` columns; unsupported nestings fall back to post-fetch evaluation over the fused candidate set. The predicate runs **before** ranking (narrow, then rank by §4.2 + MC-8), so it is a cheap index filter, not a re-rank.

### 4.6 Lifecycle states and prune protection (MI-9)

```text
[REFERENCE] lifecycle_state: active | paused | archived | deleted
  active   -> in default recall              (write default)
  paused   -> excluded, reversible           (user mute)
  archived -> excluded, opt-in include        (MC-6 sets; auto-thaw on touch)
  deleted  -> user-initiated targeted forget of the item (audited; the only state that removes content)
PRUNE GUARD: MEM-5 decay may lower ranking in any state,
             but MUST NOT delete an item whose state is paused|archived.
```

The prune guard is the single most important rule here: a deliberate shelving is a value signal that overrides automatic utility decay. Transitions append to a `lifecycle_audit` (actor, instant, old→new).

### 4.7 Experience reuse (MI-13)

The reuse projection wraps a costly action in a recall-before-acting gate; the gate, the typing, and the scoring live here (host-side), and — per the L1's nodus mapping — reuse of a promoted procedure is a `RUN(@macro)` invocation, so this surface contributes the *judgement*, not a new execution primitive.

```text
[REFERENCE] act_with_experience(action, req):
  exps := recall_experiences(req, filter = type in {success,failure,insight})  // MI-2/MI-8
  best := top(exps by score)
  if best.type == success and gate(best):        // sim>=σ AND score>=τ AND fresh AND not safety_sensitive
      result := best.result
      attribute(result, reused_from = best.id)   // MI-13(c) never passed as fresh
      require action.authority_gate(result)       // MI-13(d) SEC-9/SEC-10 still enforced
      return result
  inject_as_context(exps)                          // failures -> avoid; insights -> guidance
  result := execute(action, req)
  if write_enabled: capture_experience(req, result, type_of(result), score(result))  // MI-13(b)
  return result
```

## 5. Implementation Notes

1. **`answer` first, extractive-only** — bind recall + KB-6 citations with the verification gate stubbed to pass-through, so the honest-refusal and citation paths are testable before a generator is wired.
2. **Temporal modes are pure projections** of §4.14 — no schema change; land them second.
3. **The `lifecycle_state` column + prune guard** before archive is exercised by MC-6 — the guard must exist before the maintenance pass can set `archived`, or decay could delete a shelved item.
4. **MI-4 routing** consumes the §4.4 reconcile output; ship the auto-supersede path first, the surfaced-report path second (it needs the adjudication UI).
5. **Experience reuse last** — it composes the most other invariants (MI-1/2/6/7/8 + the security gate) and should build on their landed realizations.
6. **Every generator-dependent operation** (`answer` inferred, MI-10 normalization, MI-12 inferred capture, digest narrative) checks generator availability and takes its degraded path rather than failing.

## 6. Drawbacks & Alternatives

- **`answer` latency.** Grounded synthesis + verification costs more than raw recall. Mitigated by keeping `answer` a distinct sibling (callers needing hits call `recall`) and by the extractive-degrade path.
- **Surfacing fatigue (MI-4).** Every contradiction that is neither a duplicate nor a recency-dominant update surfaces for adjudication. Mitigated by the recency-dominant auto-supersede branch, which absorbs the common case — a newer statement at least as confident as the old one — and by the recommendation the gaps attach, which makes each surfaced report a one-step decision (§4.3).
- **Alternative — fold into `l2-memory-store`.** Rejected for the same flexibility/scalability reason as the consolidation layer: the caller-facing surface must be realizable over an alternative `UserDataStore` backend, which welding it into the SQLite store forecloses. A separate L2 behind the seam keeps it backend-swappable and keeps the substrate spec Stable and small.
- **Alternative — a separate memory-intelligence engine.** Rejected: every operation composes an existing engine (knowledge-base, claim-verification, scheduler, dashboard); a parallel engine would fork that logic.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[CONCEPT]` | `.design/main/specifications/l1-memory-intelligence.md` | MI-1…MI-13, the invariants realized here |
| `[STORE]` | `.design/main/specifications/l2-memory-store.md` | The SQLite substrate composed: §4.2/4.3/4.4/4.6/4.14/4.15 |
| `[CONSOLIDATION]` | `.design/main/specifications/l2-memory-consolidation.md` | The maintenance layer supplying MC-8 ranking signals |
| `[KB]` | `.design/main/specifications/l1-knowledge-base.md` | Source-attribution contract reused by MI-1 |
| `[VERIFY]` | `.design/main/specifications/l1-claim-verification.md` | Ternary grounding gate reused by MI-1 |
| `[TOPOLOGY]` | `.design/main/specifications/l2-crate-topology.md` | Why the surface is realized behind the UserDataStore seam |

## Document History

| Version | Date | Notes |
| --- | --- | --- |
| 1.0.1 | 2026-09-23 | Consistency pass (2026-09-23): MI-14 (anchored neighbourhood recall) compliance row added, pending realization. The §4.3 MI-4 classifier was not total — a newer-but-less-confident statement, or a lead on one signal without a recency order, matched no branch and a contradiction could stand unrecorded; it now surfaces every non-duplicate, non-update pair with a recommendation chosen by the gap thresholds, exactly as the shipped classifier does (the thresholds never auto-apply). `deleted` no longer points at the quick-memory file forget. §6 surfacing-fatigue note corrected. |
| 1.0.0 (Stable) | 2026-07-11 | Promoted RFC→Stable via Post-Update Review (spec-critic + prompt-engineer PASS), same session as authoring — the review found no blocker: it composes Stable specs (`l2-memory-store` §4.2/4.3/4.4/4.14/4.15, KB-6, CV-3/CV-4) without contradicting any, and discharges its two L1-deferred parameters concretely. Content unchanged from the initial cut (status-only promotion). Stable = design agreed; concrete SQL/algorithm choices validated during implementation. Depends on `l2-memory-consolidation` (Stable) for MC-8 signals. |
| 1.0.0 | 2026-07-11 | Initial RFC — local realization of MI-1…MI-13 as the caller-facing query surface behind the `UserDataStore` seam, composing `l2-memory-store` (§4.2/4.3/4.4/4.6/4.14/4.15) and `l2-memory-consolidation` (MC-8 signals) with no new engine. Realizes the `answer` grounded projection (KB-6 + CV-3/CV-4, extractive degrade), the three temporal modes over §4.14, immediate recall-visibility, conflict routing, the intelligence digest, the capture policy, procedural distillation, the closed structured predicate compiled to SQL, reversible lifecycle states with the prune guard, capture-time normalization, capture directives, raw/inferred modes, and gated experience reuse. **Discharges the two L1-deferred parameters**: the MI-4 ambiguity threshold (objective confidence/trust-gap + recency-dominance rule, §4.3) and the MI-5 digest cadence (per-session-close + daily floor, opt-in per office, §4.4). Kept a separate L2 for backend-swappability and small blast radius (parallels l2-memory-consolidation). |
