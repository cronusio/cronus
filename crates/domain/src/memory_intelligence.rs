//! The active query & intelligence surface over the memory
//! substrate, reached only through the `MemorySearch` seam (never a
//! concrete store type) — this module has zero I/O of its own, matching the
//! domain tier's no-infra-dependency contract. `cronus-domain` cannot depend
//! on `cronus-store-local` (the tier model has no such edge); the facade
//! wires the concrete `MemoryStore` in as `&dyn MemorySearch` at the call site.

use cronus_contract::{MemoryEntry, MemoryId, MemorySearch, TemporalMode};

/// A memory item cited as grounding for an `answer` — the
/// source-attribution contract specialized to internal memory items.
/// Distinct from `research.rs`'s `Citation` (a web URL): a different concept
/// that happens to share the English word, not a type to reuse here.
#[derive(Debug, Clone, PartialEq)]
pub struct MemoryCitation {
    pub item_id: MemoryId,
    pub excerpt: String,
}

/// The ternary honesty gate `answer` requires — the domain-logic-first
/// realization of the verification rules: no claim-verification engine exists anywhere
/// in this codebase yet (grepped before writing this), so `answer` can prove
/// *insufficient grounding* deterministically (nothing matched) but cannot
/// prove semantic contradiction without a model. A future generator-backed
/// verifier replaces this function's internals, not its call shape or the
/// shape of this verdict.
#[derive(Debug, Clone, PartialEq)]
pub enum AnswerVerdict {
    Supported,
    Insufficient { reason: String },
}

/// The result of an `answer` call.
#[derive(Debug, Clone)]
pub struct Answer {
    pub text: String,
    pub citations: Vec<MemoryCitation>,
    pub verdict: AnswerVerdict,
}

const EXCERPT_CHARS: usize = 200;

/// Retrieve a grounding set through the seam, then synthesize —
/// asserting **nothing beyond** what `store` actually returned. With no
/// generator bound (none is wired in this phase), "synthesize" is the
/// honest extractive degrade: the top attributed items, concatenated
/// verbatim, never a model-prior fabrication. An empty grounding set is an
/// honest `Insufficient` outcome, never a guess.
pub fn answer(store: &dyn MemorySearch, query: &str, limit: usize) -> Answer {
    let hits = store.search_fts(query, limit).unwrap_or_default();
    if hits.is_empty() {
        return Answer {
            text: String::new(),
            citations: Vec::new(),
            verdict: AnswerVerdict::Insufficient {
                reason: "no memory matched the query".to_string(),
            },
        };
    }

    let citations = hits
        .iter()
        .map(|e| MemoryCitation {
            item_id: e.id.clone(),
            excerpt: e.body.chars().take(EXCERPT_CHARS).collect(),
        })
        .collect();
    let text = hits
        .iter()
        .map(|e| e.body.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    Answer {
        text,
        citations,
        verdict: AnswerVerdict::Supported,
    }
}

// ── Conflict surfacing, never silent overwrite ────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    Duplicate,
    Update,
    Contradiction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictRecommendation {
    KeepNew,
    KeepOld,
    Merge,
    Drop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictStatus {
    AutoResolved,
    AwaitingAdjudication,
}

/// One conflict finding — the conceptual shape the design sketches,
/// realized as a concrete type.
#[derive(Debug, Clone, PartialEq)]
pub struct ConflictFinding {
    pub kind: ConflictKind,
    pub old_id: MemoryId,
    pub new_id: MemoryId,
    pub recommendation: ConflictRecommendation,
    pub status: ConflictStatus,
}

/// The ambiguity threshold the design leaves as an implementation-tuning decision
/// (pinned here): a confidence gap below this, combined with a comparable
/// trust gap and no recency winner, is genuine ambiguity — surfaced, never
/// silently resolved.
pub const CONF_GAP_MIN: f64 = 0.15;
pub const TRUST_GAP_MIN: f64 = 0.15;

fn normalized_eq(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// Classify a conflict between an existing item and a new candidate.
/// An exact match (after normalization) is an unambiguous duplicate; a
/// strictly-newer, equal-or-higher-confidence candidate is a recency-
/// dominant update — both auto-resolve. Anything else close enough in both
/// confidence and trust that recency does not settle it is genuine
/// disagreement — surfaced for adjudication, never guessed.
pub fn classify(old: &MemoryEntry, new: &MemoryEntry) -> ConflictFinding {
    if normalized_eq(&old.body, &new.body) {
        return ConflictFinding {
            kind: ConflictKind::Duplicate,
            old_id: old.id.clone(),
            new_id: new.id.clone(),
            recommendation: ConflictRecommendation::KeepOld,
            status: ConflictStatus::AutoResolved,
        };
    }

    let recency_dominant = new.valid_at > old.valid_at && new.confidence >= old.confidence;
    if recency_dominant {
        return ConflictFinding {
            kind: ConflictKind::Update,
            old_id: old.id.clone(),
            new_id: new.id.clone(),
            recommendation: ConflictRecommendation::KeepNew,
            status: ConflictStatus::AutoResolved,
        };
    }

    let confidence_gap = (new.confidence - old.confidence).abs();
    let trust_gap = (new.trust_score - old.trust_score).abs();
    let genuinely_balanced = confidence_gap < CONF_GAP_MIN && trust_gap < TRUST_GAP_MIN;

    ConflictFinding {
        kind: ConflictKind::Contradiction,
        old_id: old.id.clone(),
        new_id: new.id.clone(),
        // A balanced disagreement suggests neither side — merge is the
        // neutral recommendation; an unbalanced-but-not-recency-dominant
        // case (e.g. lower confidence but newer) still needs a human/agent
        // call, so it recommends the newer statement without auto-applying.
        recommendation: if genuinely_balanced {
            ConflictRecommendation::Merge
        } else {
            ConflictRecommendation::KeepNew
        },
        status: ConflictStatus::AwaitingAdjudication,
    }
}

// ── Periodic intelligence digest ──────────────────────────────────────

/// Per-kind and honesty-signal analytics for a digest window.
#[derive(Debug, Clone, Default)]
pub struct DigestAnalytics {
    pub total_items: usize,
    pub by_kind: std::collections::HashMap<String, usize>,
    pub avg_confidence: f64,
    pub avg_trust: f64,
}

/// A bounded, read-only digest of one time window.
#[derive(Debug, Clone)]
pub struct Digest {
    pub narrative: String,
    pub analytics: DigestAnalytics,
}

/// A scheduled job's *computation* — firing it on a cadence is a
/// separate scheduler's job, out of this function's scope; it takes one
/// window and returns one digest. The pinned cadence (decided when
/// this behavior was finalized) is per-session-close + a daily floor,
/// opt-in per office — a policy for whoever wires the scheduler call, not a
/// parameter here.
///
/// The narrative is the honest extractive degrade (a count, not a
/// generated summary) — reusing `answer`'s synthesis would need a query to
/// answer, and a digest has none; composing `answer` with no generator
/// bound would only return the same items already being counted.
pub fn build_digest(store: &dyn MemorySearch, window_start: u64, limit: usize) -> Digest {
    let items = store
        .recall_temporal(TemporalMode::ChangedSince(window_start), limit)
        .unwrap_or_default();

    if items.is_empty() {
        return Digest {
            narrative: "No memory activity in this window.".to_string(),
            analytics: DigestAnalytics::default(),
        };
    }

    let mut by_kind = std::collections::HashMap::new();
    let mut confidence_sum = 0.0;
    let mut trust_sum = 0.0;
    for item in &items {
        *by_kind.entry(item.kind.as_str().to_string()).or_insert(0) += 1;
        confidence_sum += item.confidence;
        trust_sum += item.trust_score;
    }
    let n = items.len() as f64;
    Digest {
        narrative: format!("{} item(s) recorded this window.", items.len()),
        analytics: DigestAnalytics {
            total_items: items.len(),
            by_kind,
            avg_confidence: confidence_sum / n,
            avg_trust: trust_sum / n,
        },
    }
}

// ── Procedural distillation ───────────────────────────────────────────

/// One structured record of a completed bounded run: objective,
/// action sequence, key findings, end state, open next steps. Written
/// **once**, at the caller's explicit request, grounded only in the run's
/// own trace — never invented beyond what happened.
#[derive(Debug, Clone)]
pub struct RunTrace {
    pub objective: String,
    pub actions_taken: Vec<String>,
    pub findings: Vec<String>,
    pub end_state: String,
    pub next_steps: Vec<String>,
}

/// Render a [`RunTrace`] into the single procedure memory's body,
/// typed by how the run went. A sibling to ordinary capture, not a
/// replacement — this composes with whatever per-turn capture already
/// happened during the run, it does not gate on or replace it. The caller
/// writes the returned `MemoryEntry` through the seam (`UserDataStore::put`)
/// at the point it decides to keep the distillation — this function only
/// shapes the content. `outcome` is the read side's only hook: it is
/// how a later `recall_for_reuse` call finds this item at all (ordinary
/// memories carry `experience_outcome: None` and are never candidates).
pub fn distill_run(trace: &RunTrace, outcome: cronus_contract::ExperienceOutcome) -> MemoryEntry {
    let mut body = format!("Objective: {}\n\n", trace.objective);
    if !trace.actions_taken.is_empty() {
        body.push_str("Actions taken:\n");
        for action in &trace.actions_taken {
            body.push_str(&format!("- {action}\n"));
        }
        body.push('\n');
    }
    if !trace.findings.is_empty() {
        body.push_str("Key findings:\n");
        for finding in &trace.findings {
            body.push_str(&format!("- {finding}\n"));
        }
        body.push('\n');
    }
    body.push_str(&format!("End state: {}\n", trace.end_state));
    if !trace.next_steps.is_empty() {
        body.push_str("\nOpen next steps:\n");
        for step in &trace.next_steps {
            body.push_str(&format!("- {step}\n"));
        }
    }

    MemoryEntry::new(
        cronus_contract::MemoryKind::ProjectContext,
        cronus_contract::MemorySource::Agent,
        format!("Distilled procedure: {}", trace.objective),
        body,
    )
    .with_experience_outcome(outcome)
}

// ── Gated experience reuse ───────────────────────────────────────────

use crate::autonomy::{AutonomyLevel, CommandRiskLevel, GateDecision, classify_command, evaluate};
use cronus_contract::ExperienceOutcome;
use std::collections::HashSet;

/// The reuse-gate thresholds the design leaves as an implementation-tuning
/// decision ("similarity ≥ σ AND score ≥ τ AND fresh"), pinned here exactly
/// as `CONF_GAP_MIN`/`TRUST_GAP_MIN` were pinned — a real engineering
/// choice recorded in code, not a placeholder waiting on a future spec pass.
pub const SIMILARITY_MIN: f64 = 0.5;
pub const SCORE_MIN: f64 = 0.6;
pub const FRESHNESS_MAX_SECS: u64 = 30 * 24 * 3600;

/// The "quality-scored" measure: the average of a candidate's authored
/// confidence and its verification-weighted trust — both already-persisted
/// fields, so scoring needs no new signal beyond what the answer and conflict paths already use.
pub fn experience_score(entry: &MemoryEntry) -> f64 {
    (entry.confidence + entry.effective_trust()) / 2.0
}

fn token_set(s: &str) -> HashSet<String> {
    s.to_lowercase()
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
        .filter(|w| !w.is_empty())
        .collect()
}

/// Deterministic lexical similarity (Jaccard over normalized token sets) —
/// the domain-logic-first stand-in for a semantic/embedding comparison no
/// model is bound to provide in this phase (this phase's recurring pattern:
/// the corroborate match, the duplicate match). A future
/// embedding-backed similarity replaces this function's internals only, not
/// the reuse gate's call shape or threshold semantics.
pub fn similarity(a: &str, b: &str) -> f64 {
    let sa = token_set(a);
    let sb = token_set(b);
    if sa.is_empty() || sb.is_empty() {
        return 0.0;
    }
    let intersection = sa.intersection(&sb).count();
    let union = sa.union(&sb).count();
    intersection as f64 / union as f64
}

fn is_fresh(entry: &MemoryEntry, now: u64) -> bool {
    now.saturating_sub(entry.valid_at) <= FRESHNESS_MAX_SECS
}

/// The reuse gate — similarity AND score AND freshness AND not
/// safety-sensitive. `safety_sensitive` describes the ACTION about to be
/// attempted (a caller judgment about what's happening now), not a property
/// of the stored candidate — this module has no way to know that on its own.
pub fn reuse_gate(
    candidate: &MemoryEntry,
    request: &str,
    now: u64,
    safety_sensitive: bool,
) -> bool {
    !safety_sensitive
        && similarity(request, &candidate.body) >= SIMILARITY_MIN
        && experience_score(candidate) >= SCORE_MIN
        && is_fresh(candidate, now)
}

/// The outcome of the recall-before-acting decision. `Reuse` is the only
/// variant meaning "skip re-derivation, use this result" — every other
/// variant means "still do the work," carrying whatever should be injected
/// while doing it (the L1's own table: failures → avoid, insights →
/// guidance).
#[derive(Debug, Clone, PartialEq)]
pub enum ExperienceDecision {
    /// A gated prior success, already past the retained authority
    /// gate — `citation.item_id` is `reused_from`, never passed off
    /// as fresh work.
    Reuse(MemoryCitation),
    /// A gated prior success exists, but reusing its body would need
    /// approval the caller's current autonomy level does not auto-grant.
    /// Reuse never silently bypasses the action's own gate, so
    /// this is surfaced rather than auto-applied.
    ReuseNeedsApproval {
        citation: MemoryCitation,
        risk: CommandRiskLevel,
    },
    /// No reusable success (none existed, or none passed the gate) — do the
    /// work, informed by whatever avoid/guidance signals were found.
    Execute {
        avoid: Vec<MemoryCitation>,
        guidance: Vec<MemoryCitation>,
    },
}

fn to_citation(entry: &MemoryEntry) -> MemoryCitation {
    MemoryCitation {
        item_id: entry.id.clone(),
        excerpt: entry.body.chars().take(EXCERPT_CHARS).collect(),
    }
}

/// Recall-before-acting. Retrieves typed experiences through the seam
/// (the structured-recall machinery — `recall_structured` over the
/// `ExperienceOutcome` predicate field added for this), picks the
/// highest-scored prior `Success`, and applies the full four-guard contract:
/// **(a)** gated reuse (`reuse_gate`); **(b)** read/write independence —
/// this function is read-only, capturing the fresh result back is a
/// separate `distill_run` call the caller makes only when it chooses to
/// write; **(c)** attribution via `MemoryCitation`, never a bare
/// `MemoryEntry` indistinguishable from fresh work; **(d)** the retained
/// authority gate, composing `crate::autonomy::{classify_command, evaluate}`
/// — the real authority-gate realization already built in this crate (grepped
/// first; no second gate invented) — so a reused result that would need
/// approval comes back `ReuseNeedsApproval`, never a silent `Reuse`.
pub fn recall_for_reuse(
    store: &dyn MemorySearch,
    request: &str,
    now: u64,
    safety_sensitive: bool,
    autonomy_level: AutonomyLevel,
    limit: usize,
) -> ExperienceDecision {
    use cronus_contract::{FieldPredicate, PredicateField, PredicateValue};

    let predicate = FieldPredicate::In(
        PredicateField::ExperienceOutcome,
        vec![
            PredicateValue::Text(ExperienceOutcome::Success.as_str().to_string()),
            PredicateValue::Text(ExperienceOutcome::Failure.as_str().to_string()),
            PredicateValue::Text(ExperienceOutcome::Insight.as_str().to_string()),
        ],
    );
    let candidates = store
        .recall_structured(&predicate, limit)
        .unwrap_or_default();

    let best_success = candidates
        .iter()
        .filter(|e| e.experience_outcome == Some(ExperienceOutcome::Success))
        .max_by(|a, b| {
            experience_score(a)
                .partial_cmp(&experience_score(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

    if let Some(best) = best_success
        && reuse_gate(best, request, now, safety_sensitive)
    {
        let citation = to_citation(best);
        let risk = classify_command(&best.body);
        return match evaluate(autonomy_level, risk) {
            GateDecision::Allow => ExperienceDecision::Reuse(citation),
            GateDecision::RequireApproval | GateDecision::Deny => {
                ExperienceDecision::ReuseNeedsApproval { citation, risk }
            }
        };
    }

    let avoid = candidates
        .iter()
        .filter(|e| e.experience_outcome == Some(ExperienceOutcome::Failure))
        .map(to_citation)
        .collect();
    let guidance = candidates
        .iter()
        .filter(|e| e.experience_outcome == Some(ExperienceOutcome::Insight))
        .map(to_citation)
        .collect();

    ExperienceDecision::Execute { avoid, guidance }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cronus_contract::{FieldPredicate, MemoryEntry, TemporalMode};

    /// A minimal in-memory `MemorySearch` stub — this module must compile
    /// and be testable against *any* implementor of the seam, not just the
    /// SQLite one, which is the whole point of reaching it only via `&dyn`.
    struct StubSearch {
        entries: Vec<MemoryEntry>,
    }

    impl MemorySearch for StubSearch {
        fn search_fts(
            &self,
            query: &str,
            limit: usize,
        ) -> std::result::Result<Vec<MemoryEntry>, String> {
            Ok(self
                .entries
                .iter()
                .filter(|e| e.body.contains(query) || e.title.contains(query))
                .take(limit)
                .cloned()
                .collect())
        }

        fn recall_temporal(
            &self,
            mode: TemporalMode,
            limit: usize,
        ) -> std::result::Result<Vec<MemoryEntry>, String> {
            let filtered: Vec<MemoryEntry> = match mode {
                TemporalMode::ChangedSince(checkpoint) => self
                    .entries
                    .iter()
                    .filter(|e| e.created_at > checkpoint)
                    .cloned()
                    .collect(),
                TemporalMode::AsOf(_) | TemporalMode::Recent => self.entries.clone(),
            };
            Ok(filtered.into_iter().take(limit).collect())
        }

        fn recall_structured(
            &self,
            predicate: &FieldPredicate,
            limit: usize,
        ) -> std::result::Result<Vec<MemoryEntry>, String> {
            // Only the shape `recall_for_reuse` actually constructs is
            // honored — extend this match when a future caller needs a
            // different predicate shape against the stub.
            let filtered: Vec<MemoryEntry> = match predicate {
                FieldPredicate::In(cronus_contract::PredicateField::ExperienceOutcome, values) => {
                    let wanted: Vec<&str> = values
                        .iter()
                        .filter_map(|v| match v {
                            cronus_contract::PredicateValue::Text(s) => Some(s.as_str()),
                            _ => None,
                        })
                        .collect();
                    self.entries
                        .iter()
                        .filter(|e| {
                            e.experience_outcome
                                .is_some_and(|o| wanted.contains(&o.as_str()))
                        })
                        .cloned()
                        .collect()
                }
                _ => Vec::new(),
            };
            Ok(filtered.into_iter().take(limit).collect())
        }
    }

    fn entry(title: &str, body: &str) -> MemoryEntry {
        MemoryEntry::new(
            cronus_contract::MemoryKind::Convention,
            cronus_contract::MemorySource::Agent,
            title,
            body,
        )
    }

    /// A high-quality, high-trust experience entry — passes the score half
    /// of the reuse gate by construction; tests adjust `valid_at` and
    /// `body` themselves for the similarity/freshness halves.
    fn experience(title: &str, body: &str, outcome: ExperienceOutcome) -> MemoryEntry {
        let mut e = entry(title, body);
        e.trust_score = 0.9;
        e.verification_state = cronus_contract::VerificationState::TestedInProject;
        e.experience_outcome = Some(outcome);
        e
    }

    #[test]
    fn answer_with_no_hits_is_an_honest_insufficient_outcome() {
        let store = StubSearch { entries: vec![] };
        let result = answer(&store, "anything", 5);
        assert_eq!(
            result.verdict,
            AnswerVerdict::Insufficient {
                reason: "no memory matched the query".to_string()
            }
        );
        assert!(result.text.is_empty());
        assert!(result.citations.is_empty());
    }

    #[test]
    fn answer_with_hits_cites_every_grounding_item() {
        let e1 = entry("dark mode", "the user prefers dark mode everywhere");
        let e2 = entry("dark mode again", "dark mode is the default theme");
        let ids = [e1.id.clone(), e2.id.clone()];
        let store = StubSearch {
            entries: vec![e1, e2],
        };

        let result = answer(&store, "dark mode", 10);
        assert_eq!(result.verdict, AnswerVerdict::Supported);
        assert_eq!(result.citations.len(), 2);
        let cited_ids: Vec<_> = result.citations.iter().map(|c| c.item_id.clone()).collect();
        for id in ids {
            assert!(
                cited_ids.contains(&id),
                "every grounding item must be cited"
            );
        }
    }

    #[test]
    fn answer_never_asserts_text_beyond_the_retrieved_items() {
        let e = entry("scoped fact", "only this sentence exists in memory");
        let store = StubSearch { entries: vec![e] };

        let result = answer(&store, "scoped", 10);
        assert!(
            result.text.contains("only this sentence exists in memory"),
            "the answer must be grounded in the retrieved body"
        );
        // The whole point of the extractive degrade: nothing appears in
        // `text` that didn't come from a retrieved item's own body.
        assert!(!result.text.contains("fabricated"));
    }

    #[test]
    fn answer_reaches_the_store_only_through_the_seam_trait() {
        // Compile-time proof, not a runtime assertion: `answer` takes
        // `&dyn MemorySearch`, so it type-checks against ANY implementor —
        // this stub, or the real `cronus-store-local::MemoryStore` — with
        // no dependency edge from this crate to the adapter.
        fn accepts_any_seam_impl(store: &dyn MemorySearch) -> Answer {
            answer(store, "x", 1)
        }
        let store = StubSearch { entries: vec![] };
        let _ = accepts_any_seam_impl(&store);
    }

    // ── Classify ───────────────────────────────────────────────────

    #[test]
    fn classify_exact_normalized_match_is_an_auto_resolved_duplicate() {
        let old = entry("t", "  Same Fact  ");
        let new = entry("t", "same fact");
        let finding = classify(&old, &new);
        assert_eq!(finding.kind, ConflictKind::Duplicate);
        assert_eq!(finding.status, ConflictStatus::AutoResolved);
        assert_eq!(finding.recommendation, ConflictRecommendation::KeepOld);
    }

    #[test]
    fn classify_recency_dominant_update_auto_resolves_to_keep_new() {
        let mut old = entry("t", "old statement");
        old.valid_at = 100;
        old.confidence = 0.6;
        let mut new = entry("t", "new statement");
        new.valid_at = 200;
        new.confidence = 0.7;

        let finding = classify(&old, &new);
        assert_eq!(finding.kind, ConflictKind::Update);
        assert_eq!(finding.status, ConflictStatus::AutoResolved);
        assert_eq!(finding.recommendation, ConflictRecommendation::KeepNew);
    }

    #[test]
    fn classify_balanced_disagreement_surfaces_for_adjudication() {
        let mut old = entry("t", "the server runs on port 8080");
        old.valid_at = 200; // NOT older than new — recency does not dominate
        old.confidence = 0.6;
        old.trust_score = 0.6;
        let mut new = entry("t", "the server runs on port 9090");
        new.valid_at = 100;
        new.confidence = 0.62; // within CONF_GAP_MIN of old
        new.trust_score = 0.65; // within TRUST_GAP_MIN of old

        let finding = classify(&old, &new);
        assert_eq!(finding.kind, ConflictKind::Contradiction);
        assert_eq!(finding.status, ConflictStatus::AwaitingAdjudication);
        assert_eq!(finding.recommendation, ConflictRecommendation::Merge);
    }

    #[test]
    fn classify_never_silently_resolves_a_genuine_disagreement() {
        // Whatever the recommendation, a genuine (non-duplicate,
        // non-recency-dominant) disagreement must never come back
        // AutoResolved — the core promise.
        let mut old = entry("t", "fact A");
        old.valid_at = 200;
        old.confidence = 0.9;
        let mut new = entry("t", "fact B");
        new.valid_at = 100; // older, so not recency-dominant
        new.confidence = 0.2; // far apart, not a "balanced" case either

        let finding = classify(&old, &new);
        assert_ne!(finding.kind, ConflictKind::Duplicate);
        assert_eq!(finding.status, ConflictStatus::AwaitingAdjudication);
    }

    // ── build_digest ───────────────────────────────────────────────

    #[test]
    fn build_digest_on_an_empty_window_is_an_honest_no_op() {
        let store = StubSearch { entries: vec![] };
        let digest = build_digest(&store, 0, 10);
        assert_eq!(digest.analytics.total_items, 0);
        assert_eq!(digest.narrative, "No memory activity in this window.");
    }

    #[test]
    fn build_digest_aggregates_kind_counts_and_averages() {
        let mut e1 = entry("a", "body a");
        e1.kind = cronus_contract::MemoryKind::Convention;
        e1.confidence = 1.0;
        e1.trust_score = 0.8;
        e1.created_at = 500;
        let mut e2 = entry("b", "body b");
        e2.kind = cronus_contract::MemoryKind::Convention;
        e2.confidence = 0.5;
        e2.trust_score = 0.4;
        e2.created_at = 600;

        let store = StubSearch {
            entries: vec![e1, e2],
        };
        let digest = build_digest(&store, 0, 10);

        assert_eq!(digest.analytics.total_items, 2);
        assert_eq!(digest.analytics.by_kind.get("Convention"), Some(&2));
        assert!((digest.analytics.avg_confidence - 0.75).abs() < 1e-9);
        assert!((digest.analytics.avg_trust - 0.6).abs() < 1e-9);
    }

    #[test]
    fn build_digest_only_reflects_the_window_since_the_checkpoint() {
        let mut old = entry("old", "old body");
        old.created_at = 100;
        let mut recent = entry("recent", "recent body");
        recent.created_at = 900;

        let store = StubSearch {
            entries: vec![old, recent],
        };
        let digest = build_digest(&store, 500, 10);
        assert_eq!(
            digest.analytics.total_items, 1,
            "only items after the checkpoint count"
        );
    }

    // ── distill_run ────────────────────────────────────────────────

    #[test]
    fn distill_run_grounds_the_body_strictly_in_the_trace() {
        let trace = RunTrace {
            objective: "fix the flaky test".to_string(),
            actions_taken: vec!["reran with --test-threads=1".to_string()],
            findings: vec!["a shared env var caused the race".to_string()],
            end_state: "test passes deterministically".to_string(),
            next_steps: vec!["add a regression test".to_string()],
        };
        let memory = distill_run(&trace, ExperienceOutcome::Success);
        assert!(memory.body.contains("fix the flaky test"));
        assert!(memory.body.contains("reran with --test-threads=1"));
        assert!(memory.body.contains("a shared env var caused the race"));
        assert!(memory.body.contains("test passes deterministically"));
        assert!(memory.body.contains("add a regression test"));
        assert!(memory.title.contains("fix the flaky test"));
    }

    #[test]
    fn distill_run_never_invents_content_beyond_the_trace() {
        let trace = RunTrace {
            objective: "minimal run".to_string(),
            actions_taken: vec![],
            findings: vec![],
            end_state: "done".to_string(),
            next_steps: vec![],
        };
        let memory = distill_run(&trace, ExperienceOutcome::Success);
        assert!(
            !memory.body.contains("Actions taken:"),
            "an empty section must not render its header"
        );
        assert!(!memory.body.contains("Key findings:"));
        assert!(!memory.body.contains("Open next steps:"));
    }

    // ── reuse_gate ───────────────────────────────────────────────

    #[test]
    fn reuse_gate_passes_for_a_similar_high_scoring_fresh_success() {
        let now = 10_000_000u64;
        let mut candidate = experience(
            "t",
            "restart the flaky worker after a timeout",
            ExperienceOutcome::Success,
        );
        candidate.valid_at = now - 1_000;
        assert!(reuse_gate(
            &candidate,
            "restart the flaky worker",
            now,
            false
        ));
    }

    #[test]
    fn reuse_gate_refuses_low_similarity() {
        let now = 10_000_000u64;
        let mut candidate = experience(
            "t",
            "restart the flaky worker after a timeout",
            ExperienceOutcome::Success,
        );
        candidate.valid_at = now - 1_000;
        assert!(!reuse_gate(
            &candidate,
            "deploy the new frontend build",
            now,
            false
        ));
    }

    #[test]
    fn reuse_gate_refuses_a_stale_experience() {
        let now = 10_000_000u64;
        let mut candidate = experience(
            "t",
            "restart the flaky worker after a timeout",
            ExperienceOutcome::Success,
        );
        candidate.valid_at = now - (FRESHNESS_MAX_SECS + 1);
        assert!(!reuse_gate(
            &candidate,
            "restart the flaky worker",
            now,
            false
        ));
    }

    #[test]
    fn reuse_gate_refuses_a_safety_sensitive_action_even_when_everything_else_passes() {
        let now = 10_000_000u64;
        let mut candidate = experience(
            "t",
            "restart the flaky worker after a timeout",
            ExperienceOutcome::Success,
        );
        candidate.valid_at = now - 1_000;
        assert!(!reuse_gate(
            &candidate,
            "restart the flaky worker",
            now,
            true // safety_sensitive
        ));
    }

    // ── recall_for_reuse ──────────────────────────────────────────

    #[test]
    fn recall_for_reuse_reuses_a_gated_low_risk_success() {
        let now = 10_000_000u64;
        let mut success = experience(
            "t",
            "search the logs for the timeout error",
            ExperienceOutcome::Success,
        );
        success.valid_at = now - 1_000;
        let success_id = success.id.clone();
        let store = StubSearch {
            entries: vec![success],
        };

        let decision = recall_for_reuse(
            &store,
            "search the logs for the timeout error",
            now,
            false,
            AutonomyLevel::Supervised,
            10,
        );
        assert_eq!(
            decision,
            ExperienceDecision::Reuse(MemoryCitation {
                item_id: success_id,
                excerpt: "search the logs for the timeout error".to_string(),
            })
        );
    }

    #[test]
    fn recall_for_reuse_surfaces_approval_for_a_gated_high_risk_success() {
        // Reuse never silently bypasses the action's own
        // authority gate — a gated, similar, fresh success whose body
        // classifies as High risk still needs approval under Supervised.
        let now = 10_000_000u64;
        let mut success = experience(
            "t",
            "execute the deploy script and overwrite the config",
            ExperienceOutcome::Success,
        );
        success.valid_at = now - 1_000;
        let store = StubSearch {
            entries: vec![success],
        };

        let decision = recall_for_reuse(
            &store,
            "execute the deploy script and overwrite the config",
            now,
            false,
            AutonomyLevel::Supervised,
            10,
        );
        match decision {
            ExperienceDecision::ReuseNeedsApproval { risk, .. } => {
                assert_eq!(risk, CommandRiskLevel::High);
            }
            other => panic!("expected ReuseNeedsApproval, got {other:?}"),
        }
    }

    #[test]
    fn recall_for_reuse_never_reuses_a_failure_injects_as_avoid_instead() {
        let now = 10_000_000u64;
        let mut failure = experience(
            "t",
            "restart the flaky worker after a timeout",
            ExperienceOutcome::Failure,
        );
        failure.valid_at = now - 1_000;
        let failure_id = failure.id.clone();
        let store = StubSearch {
            entries: vec![failure],
        };

        let decision = recall_for_reuse(
            &store,
            "restart the flaky worker",
            now,
            false,
            AutonomyLevel::Autonomous,
            10,
        );
        match decision {
            ExperienceDecision::Execute { avoid, guidance } => {
                assert_eq!(avoid.len(), 1);
                assert_eq!(avoid[0].item_id, failure_id);
                assert!(guidance.is_empty());
            }
            other => panic!("a failure must never be reused, got {other:?}"),
        }
    }

    #[test]
    fn recall_for_reuse_never_reuses_an_insight_injects_as_guidance_instead() {
        let now = 10_000_000u64;
        let mut insight = experience(
            "t",
            "restart the flaky worker after a timeout",
            ExperienceOutcome::Insight,
        );
        insight.valid_at = now - 1_000;
        let insight_id = insight.id.clone();
        let store = StubSearch {
            entries: vec![insight],
        };

        let decision = recall_for_reuse(
            &store,
            "restart the flaky worker",
            now,
            false,
            AutonomyLevel::Autonomous,
            10,
        );
        match decision {
            ExperienceDecision::Execute { avoid, guidance } => {
                assert!(avoid.is_empty());
                assert_eq!(guidance.len(), 1);
                assert_eq!(guidance[0].item_id, insight_id);
            }
            other => panic!("an insight must never be reused, got {other:?}"),
        }
    }

    #[test]
    fn recall_for_reuse_with_no_prior_experience_is_an_honest_execute_with_nothing_to_inject() {
        let store = StubSearch { entries: vec![] };
        let decision = recall_for_reuse(
            &store,
            "anything",
            10_000_000,
            false,
            AutonomyLevel::Autonomous,
            10,
        );
        assert_eq!(
            decision,
            ExperienceDecision::Execute {
                avoid: vec![],
                guidance: vec![]
            }
        );
    }
}
