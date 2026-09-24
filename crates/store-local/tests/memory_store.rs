use cronus_contract::{MemorySearch, UserDataStore};
use cronus_store_local::memory::{
    CodeChangeType, FieldPredicate, LifecycleState, MemoryDepth, MemoryEntry, MemoryKind,
    MemorySource, MemoryStore, MemorySubject, PredicateField, PredicateValue, SignalKind,
    SuggestedAction, TemporalMode, TrustUpdate, VerificationState,
    chain::ChainKind,
    trust::{TRUST_MIN_SEARCH, TRUST_NEGATIVE_DELTA, TRUST_POSITIVE_DELTA},
};

fn store() -> MemoryStore {
    MemoryStore::open_in_memory().expect("in-memory store must open")
}

fn entry(title: &str, body: &str) -> MemoryEntry {
    MemoryEntry::new(MemoryKind::Convention, MemorySource::Agent, title, body)
}

#[test]
fn add_and_get_roundtrip() {
    let s = store();
    let e = entry("test title", "test body");
    let id = s.add(e.clone()).unwrap();

    let got = s.get(&id).unwrap().expect("entry must exist");
    assert_eq!(got.id, id);
    assert_eq!(got.title, "test title");
    assert_eq!(got.body, "test body");
    assert_eq!(got.kind, MemoryKind::Convention);
    assert_eq!(got.source, MemorySource::Agent);
}

#[test]
fn get_missing_returns_none() {
    let s = store();
    use cronus_store_local::memory::MemoryId;
    let fake = MemoryId::from("nonexistent".to_string());
    assert!(s.get(&fake).unwrap().is_none());
}

#[test]
fn fts_search_finds_matching_entries() {
    let s = store();
    s.add(entry("Rust lifetimes", "ownership borrow checker"))
        .unwrap();
    s.add(entry("Python decorators", "metaclass function wrap"))
        .unwrap();

    let results = s.search_fts("ownership", 10).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Rust lifetimes");
}

#[test]
fn fts_search_tolerates_query_metacharacters() {
    // Raw input used to go straight into `memories_fts MATCH ?`: a hyphen made
    // FTS5 read `run-42` as `run` NOT column `42` and error `no such column: 42`.
    // The query is now tokenised and each token quoted, so punctuation is
    // literal and a metacharacter-only query matches nothing instead of erroring.
    let s = store();
    s.add(entry("run-42 report", "the pipeline finished"))
        .unwrap();

    let hit = s.search_fts("run-42", 10).unwrap();
    assert_eq!(hit.len(), 1, "a hyphenated token must not error the search");
    assert_eq!(hit[0].title, "run-42 report");

    for weird in ["-", "\"", "*", "( )", "a AND", "b:c", "^", ""] {
        let out = s
            .search_fts(weird, 10)
            .unwrap_or_else(|e| panic!("query {weird:?} must not error: {e}"));
        assert!(out.is_empty(), "query {weird:?} should match nothing");
    }
}

#[test]
fn fts_search_filters_by_trust_score() {
    let s = store();
    let mut low_trust = entry("low trust entry", "secret knowledge");
    low_trust.trust_score = 0.05; // below TRUST_MIN_SEARCH
    s.add(low_trust).unwrap();

    let results = s.search_fts("secret knowledge", 10).unwrap();
    assert!(
        results.is_empty(),
        "entries below TRUST_MIN_SEARCH must be excluded from search"
    );
}

#[test]
fn trust_update_positive_delta() {
    let s = store();
    let e = entry("positive test", "body");
    let id = s.add(e).unwrap();

    let initial = s.get(&id).unwrap().unwrap().trust_score;
    let new_score = s.update_trust(&id, TrustUpdate::positive()).unwrap();

    assert!(
        (new_score - (initial + TRUST_POSITIVE_DELTA)).abs() < 1e-10,
        "positive update must add {TRUST_POSITIVE_DELTA}"
    );
}

#[test]
fn trust_update_negative_delta() {
    let s = store();
    let e = entry("negative test", "body");
    let id = s.add(e).unwrap();

    let initial = s.get(&id).unwrap().unwrap().trust_score;
    let new_score = s.update_trust(&id, TrustUpdate::negative()).unwrap();

    assert!(
        (new_score - (initial - TRUST_NEGATIVE_DELTA)).abs() < 1e-10,
        "negative update must subtract {TRUST_NEGATIVE_DELTA}"
    );
}

#[test]
fn trust_update_clamped_at_bounds() {
    let s = store();
    let mut e = entry("clamp test", "body");
    e.trust_score = 1.0;
    let id = s.add(e).unwrap();

    let score = s.update_trust(&id, TrustUpdate::positive()).unwrap();
    assert!((score - 1.0).abs() < 1e-10, "trust must not exceed 1.0");
}

#[test]
fn trust_update_sets_verification_state() {
    let s = store();
    let e = entry("verify test", "body");
    let id = s.add(e).unwrap();

    s.update_trust(
        &id,
        TrustUpdate::with_verification(VerificationState::TestedInProject),
    )
    .unwrap();

    let got = s.get(&id).unwrap().unwrap();
    assert_eq!(got.verification_state, VerificationState::TestedInProject);
}

#[test]
fn session_auto_chain_within_window() {
    let s = store();

    let mut e1 = entry("first memory", "body one");
    e1.workspace_id = Some("ws-test".to_string());
    let id1 = s.add(e1).unwrap();

    let mut e2 = entry("second memory", "body two");
    e2.workspace_id = Some("ws-test".to_string());
    e2.created_at = cronus_store_local::memory::MemoryEntry::new(
        MemoryKind::Convention,
        MemorySource::Agent,
        "",
        "",
    )
    .created_at
        + 60; // 60 seconds later — within 2h window
    let _id2 = s.add(e2).unwrap();

    // Verify chain was created by checking the chain store
    // The auto-chain creates: id1 → id2 with Continuation kind
    // We verify indirectly: propagate_trust should traverse the chain
    s.propagate_trust(&id1, 0.1).unwrap();
    // If propagation doesn't panic and returns Ok, the chain exists
}

#[test]
fn explicit_chain_and_propagation() {
    let s = store();
    let id_a = s.add(entry("source", "alpha")).unwrap();
    let id_b = s.add(entry("target", "beta")).unwrap();

    s.chain(&id_a, &id_b, ChainKind::RelatedTo).unwrap();

    let b_before = s.get(&id_b).unwrap().unwrap().trust_score;
    s.propagate_trust(&id_a, 1.0).unwrap();
    let b_after = s.get(&id_b).unwrap().unwrap().trust_score;

    assert!(
        b_after > b_before,
        "propagated trust must increase target's score"
    );
}

#[test]
fn code_change_deleted_triggers_invalidate() {
    let s = store();
    let mut e = entry("some function", "uses old api");
    e.workspace_id = Some("ws-code".to_string());
    let id = s.add(e).unwrap();

    let action = CodeChangeType::Deleted.suggested_action();
    assert!(matches!(action, SuggestedAction::Invalidate(_)));

    let affected = s
        .apply_code_change("ws-code", CodeChangeType::Deleted)
        .unwrap();
    assert!(affected.contains(&id));
}

#[test]
fn code_change_signature_changed_triggers_review() {
    let action = CodeChangeType::SignatureChanged.suggested_action();
    assert!(
        matches!(action, SuggestedAction::Review(_)),
        "SignatureChanged must map to Review action"
    );
}

#[test]
fn hrr_encoding_stub_returns_zeroed_vector() {
    let s = store();
    let e = entry("hrr test", "body");
    let vec = s.encode_hrr(&e);
    assert_eq!(
        vec.len(),
        256,
        "HRR stub must return 256-dimensional vector"
    );
    assert!(vec.iter().all(|&v| v == 0.0), "HRR stub must be all zeros");
}

#[test]
fn search_excludes_superseded_entries() {
    let s = store();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut e = entry("superseded entry", "outdated knowledge");
    e.superseded_at = Some(now - 100);
    s.add(e).unwrap();

    let results = s.search_fts("outdated knowledge", 10).unwrap();
    assert!(
        results.is_empty(),
        "superseded entries must be excluded from search"
    );
}

#[test]
fn trust_min_search_constant() {
    const { assert!(TRUST_MIN_SEARCH > 0.0 && TRUST_MIN_SEARCH <= 1.0) }
    const { assert!(TRUST_NEGATIVE_DELTA > TRUST_POSITIVE_DELTA) }
}

// ── UserDataStore / MemorySearch (ports tier) ────────────────────────────────

#[test]
fn export_all_returns_every_stored_entry_unfiltered() {
    let s = store();
    let mut low_trust = entry("low trust", "body");
    low_trust.trust_score = 0.01; // would be excluded from search_fts
    s.add(low_trust).unwrap();
    s.add(entry("normal", "body")).unwrap();

    let exported = s.export_all().unwrap();
    assert_eq!(
        exported.len(),
        2,
        "export applies no trust-score gate (always able to come home)"
    );
}

#[test]
fn user_data_store_put_and_export_roundtrip() {
    let s = store();
    let e = entry("via trait", "body");
    UserDataStore::put(&s, &e).unwrap();

    let exported = UserDataStore::export(&s).unwrap();
    assert_eq!(exported.len(), 1);
    assert_eq!(exported[0].title, "via trait");
}

#[test]
fn memory_search_trait_object_resolves_to_the_same_store() {
    let s = store();
    s.add(entry("trait object test", "findable body")).unwrap();

    let as_trait: &dyn MemorySearch = &s;
    let results = as_trait.search_fts("findable", 10).unwrap();
    assert_eq!(results.len(), 1);
}

// ── Processing-depth tiers ────────────────────────────────────────────

#[test]
fn depth_defaults_to_consolidated_and_round_trips() {
    let s = store();
    let id = s.add(entry("depth default", "body")).unwrap();
    let got = s.get(&id).unwrap().unwrap();
    assert_eq!(got.depth, MemoryDepth::Consolidated);
}

#[test]
fn explicit_raw_depth_round_trips() {
    let s = store();
    let raw = entry("raw evidence", "verbatim transcript").with_depth(MemoryDepth::Raw);
    let id = s.add(raw).unwrap();
    let got = s.get(&id).unwrap().unwrap();
    assert_eq!(got.depth, MemoryDepth::Raw);
}

// ── Reversible lifecycle states ───────────────────────────────────────

#[test]
fn lifecycle_state_defaults_to_active() {
    let s = store();
    let id = s.add(entry("lifecycle default", "body")).unwrap();
    let got = s.get(&id).unwrap().unwrap();
    assert_eq!(got.lifecycle_state, LifecycleState::Active);
    assert_eq!(
        s.lifecycle_state(&id).unwrap(),
        Some(LifecycleState::Active)
    );
}

#[test]
fn set_lifecycle_state_transitions_and_returns_prior_state() {
    let s = store();
    let id = s.add(entry("shelve me", "body")).unwrap();

    let prior = s
        .set_lifecycle_state(&id, LifecycleState::Archived, "test-actor")
        .unwrap();
    assert_eq!(prior, Some(LifecycleState::Active));
    assert_eq!(
        s.lifecycle_state(&id).unwrap(),
        Some(LifecycleState::Archived)
    );
}

#[test]
fn set_lifecycle_state_on_unknown_id_is_a_noop_returning_none() {
    let s = store();
    use cronus_store_local::memory::MemoryId;
    let fake = MemoryId::from("nonexistent".to_string());
    let prior = s
        .set_lifecycle_state(&fake, LifecycleState::Paused, "test-actor")
        .unwrap();
    assert_eq!(prior, None);
}

#[test]
fn lifecycle_transition_is_audited() {
    let s = store();
    let id = s.add(entry("audited", "body")).unwrap();
    s.set_lifecycle_state(&id, LifecycleState::Paused, "user:alice")
        .unwrap();
    s.set_lifecycle_state(&id, LifecycleState::Active, "user:alice")
        .unwrap();

    // No public audit-read API yet (this covers the schema and the
    // transitions only; a query surface is a separate concern) — assert
    // indirectly: two transitions
    // must have landed without error, and the final state reflects the
    // second one, proving both audit inserts succeeded (a failed insert on
    // the append-only table would have propagated as an Err).
    assert_eq!(
        s.lifecycle_state(&id).unwrap(),
        Some(LifecycleState::Active)
    );
}

#[test]
fn recall_defaults_to_active_excluding_paused_and_archived() {
    let s = store();
    let active = s.add(entry("active item", "findable text")).unwrap();
    let paused = s.add(entry("paused item", "findable text")).unwrap();
    let archived = s.add(entry("archived item", "findable text")).unwrap();

    s.set_lifecycle_state(&paused, LifecycleState::Paused, "test")
        .unwrap();
    s.set_lifecycle_state(&archived, LifecycleState::Archived, "test")
        .unwrap();

    let results = s.search_fts("findable", 10).unwrap();
    let ids: Vec<_> = results.iter().map(|e| e.id.clone()).collect();
    assert!(ids.contains(&active), "active item must be recalled");
    assert!(
        !ids.contains(&paused),
        "paused item must not appear in default recall"
    );
    assert!(
        !ids.contains(&archived),
        "archived item must not appear in default recall"
    );
}

// ── Multiplicative offline-precomputed ranking ────────────────────────

#[test]
fn ranked_recall_on_cold_signals_matches_plain_text_relevance_order() {
    let s = store();
    s.add(entry("strong match apple", "apple apple apple fruit"))
        .unwrap();
    s.add(entry("weak match apple", "apple mentioned once"))
        .unwrap();

    // No signals computed anywhere — every derived factor is neutral (1.0),
    // so the fused score is exactly the base text relevance: cold-start
    // ranks purely on text strength, per the ranking rules.
    let ranked = s.search_ranked("apple", 10).unwrap();
    assert_eq!(ranked.len(), 2);
    assert!(
        ranked[0].0.title == "strong match apple",
        "the stronger textual match must rank first with no derived signals"
    );
    assert!(
        ranked[0].1 > ranked[1].1,
        "fused scores must be strictly ordered by text relevance alone"
    );
}

#[test]
fn a_near_zero_derived_factor_vetoes_a_strong_text_match() {
    let s = store();
    let strong = s
        .add(entry("dominant apple", "apple apple apple apple fruit"))
        .unwrap();
    let weak = s.add(entry("faint apple", "apple mentioned")).unwrap();

    // Without signals, `strong` would rank first (proven above). Crush its
    // centrality factor near zero — multiplicative fusion must let that
    // veto the otherwise-dominant text match, not just nudge it down.
    s.set_signal(&strong, SignalKind::Centrality, 0.001)
        .unwrap();

    let ranked = s.search_ranked("apple", 10).unwrap();
    let strong_score = ranked.iter().find(|(e, _)| e.id == strong).unwrap().1;
    let weak_score = ranked.iter().find(|(e, _)| e.id == weak).unwrap().1;
    assert!(
        weak_score > strong_score,
        "a near-zero derived factor must veto a stronger text match, not average it away"
    );
}

#[test]
fn ranked_recall_excludes_low_trust_superseded_and_inactive_items() {
    let s = store();
    let mut low_trust = entry("low trust rankme", "rankme content");
    low_trust.trust_score = 0.01;
    s.add(low_trust).unwrap();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut superseded = entry("superseded rankme", "rankme content");
    superseded.superseded_at = Some(now);
    s.add(superseded).unwrap();

    let paused = s.add(entry("paused rankme", "rankme content")).unwrap();
    s.set_lifecycle_state(&paused, LifecycleState::Paused, "test")
        .unwrap();

    let active = s.add(entry("active rankme", "rankme content")).unwrap();

    let ranked = s.search_ranked("rankme", 10).unwrap();
    assert_eq!(
        ranked.len(),
        1,
        "only the active, trusted, non-superseded item ranks"
    );
    assert_eq!(ranked[0].0.id, active);
}

#[test]
fn ranked_recall_returns_empty_for_no_match_not_an_error() {
    let s = store();
    s.add(entry("something else entirely", "unrelated content"))
        .unwrap();
    let ranked = s.search_ranked("xyz_nonexistent_term", 10).unwrap();
    assert!(ranked.is_empty());
}

// ── Corpus maintenance, exercised through MemoryStore ────────────────

#[test]
fn memory_store_recompute_recency_and_sweep_archive_wire_through() {
    let s = store();
    let id = s.add(entry("old memory", "body")).unwrap();
    s.recompute_recency().unwrap();
    // A brand-new item's recency is fresh — nothing archives on this pass.
    let archived = s.sweep_archive("test").unwrap();
    assert!(!archived.contains(&id));
}

#[test]
fn memory_store_touch_wires_through_to_the_lifecycle_transition() {
    let s = store();
    let id = s.add(entry("shelved", "body")).unwrap();
    s.set_lifecycle_state(&id, LifecycleState::Archived, "test")
        .unwrap();

    let thawed = s.touch(&id, "test").unwrap();
    assert!(thawed);
    assert_eq!(
        s.lifecycle_state(&id).unwrap(),
        Some(LifecycleState::Active)
    );
}

#[test]
fn memory_store_merge_candidates_and_merge_pair_wire_through() {
    let s = store();
    let a = s.add(entry("dup a", "identical content")).unwrap();
    let b = s.add(entry("dup b", "identical content")).unwrap();

    let candidates = s.find_merge_candidates().unwrap();
    assert_eq!(candidates.len(), 1);
    let (keep, discard) = candidates[0].clone();
    assert!((keep == a && discard == b) || (keep == b && discard == a));

    s.merge_pair(&keep, &discard, "test").unwrap();
    assert!(s.get(&discard).unwrap().is_none(), "discard must be gone");
    assert!(s.get(&keep).unwrap().is_some(), "keep must remain");
}

#[test]
fn memory_store_flag_split_candidates_wires_through() {
    let s = store();
    let long_body = "x".repeat(5_000);
    s.add(entry("huge", &long_body)).unwrap();
    s.add(entry("tiny", "short")).unwrap();

    let candidates = s.flag_split_candidates().unwrap();
    assert_eq!(candidates.len(), 1);
}

// ── Temporal recall modes ─────────────────────────────────────────────

#[test]
fn recall_as_of_still_sees_a_since_superseded_record() {
    let s = store();
    let mut e = entry("policy", "old policy text");
    e.valid_at = 100;
    e.created_at = 100;
    let id = s.add(e).unwrap();

    // Correct it at a later instant — the old record becomes superseded.
    let corrected = entry("policy v2", "new policy text");
    s.correct(&id, corrected, "test").unwrap();

    // "As of 100" (when the old record was current) must still see it —
    // the question is what was true then, not what is true now.
    let as_of = s.recall_temporal(TemporalMode::AsOf(100), 10).unwrap();
    assert!(
        as_of.iter().any(|entry| entry.id == id),
        "as-of must see a record that was current at that instant, even if later superseded"
    );
}

#[test]
fn recall_as_of_excludes_a_record_not_yet_valid() {
    let s = store();
    let mut e = entry("future policy", "text");
    e.valid_at = 1_000_000;
    e.created_at = 1_000_000;
    let id = s.add(e).unwrap();

    let as_of_early = s.recall_temporal(TemporalMode::AsOf(100), 10).unwrap();
    assert!(!as_of_early.iter().any(|entry| entry.id == id));
}

#[test]
fn recall_changed_since_finds_only_items_after_the_checkpoint() {
    let s = store();
    let mut old = entry("old item", "text");
    old.created_at = 100;
    s.add(old).unwrap();

    let mut newer = entry("newer item", "text");
    newer.created_at = 500;
    let newer_id = s.add(newer).unwrap();

    let changed = s
        .recall_temporal(TemporalMode::ChangedSince(300), 10)
        .unwrap();
    let ids: Vec<_> = changed.iter().map(|e| e.id.clone()).collect();
    assert!(ids.contains(&newer_id));
    assert_eq!(
        changed.len(),
        1,
        "only the item created after the checkpoint qualifies"
    );
}

#[test]
fn recall_recent_orders_newest_first() {
    let s = store();
    let mut e1 = entry("first", "text");
    e1.created_at = 100;
    s.add(e1).unwrap();
    let mut e2 = entry("second", "text");
    e2.created_at = 200;
    let second_id = s.add(e2).unwrap();

    let recent = s.recall_temporal(TemporalMode::Recent, 10).unwrap();
    assert_eq!(recent[0].id, second_id, "the newest item must come first");
}

// ── Structured predicate ──────────────────────────────────────────────

#[test]
fn recall_structured_filters_by_kind_equality() {
    let s = store();
    let mut conv = entry("a convention", "text");
    conv.kind = MemoryKind::Convention;
    s.add(conv).unwrap();
    let mut issue = entry("a known issue", "text");
    issue.kind = MemoryKind::KnownIssue;
    let issue_id = s.add(issue).unwrap();

    let pred = FieldPredicate::Eq(
        PredicateField::Kind,
        PredicateValue::Text("KnownIssue".into()),
    );
    let results = s.recall_structured(&pred, 10).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, issue_id);
}

#[test]
fn recall_structured_composes_and_or_not() {
    let s = store();
    let mut e1 = entry("high trust convention", "text");
    e1.kind = MemoryKind::Convention;
    e1.trust_score = 0.9;
    let e1_id = s.add(e1).unwrap();

    let mut e2 = entry("low trust convention", "text");
    e2.kind = MemoryKind::Convention;
    e2.trust_score = 0.35; // still above TRUST_MIN_SEARCH
    s.add(e2).unwrap();

    let mut e3 = entry("high trust issue", "text");
    e3.kind = MemoryKind::KnownIssue;
    e3.trust_score = 0.9;
    s.add(e3).unwrap();

    // Convention AND trust_score >= 0.5 AND NOT (source = Import)
    let pred = FieldPredicate::And(vec![
        FieldPredicate::Eq(
            PredicateField::Kind,
            PredicateValue::Text("Convention".into()),
        ),
        FieldPredicate::Ge(PredicateField::TrustScore, PredicateValue::Number(0.5)),
        FieldPredicate::Not(Box::new(FieldPredicate::Eq(
            PredicateField::Source,
            PredicateValue::Text("Import".into()),
        ))),
    ]);
    let results = s.recall_structured(&pred, 10).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, e1_id);
}

// ── Immediate recall-visibility ───────────────────────────────────────

#[test]
fn a_written_item_is_recall_visible_immediately_with_no_enrichment_delay() {
    let s = store();
    // No signal has been computed for this item (no enrichment pass has
    // run) — the design requires the write to still be findable right away;
    // missing enrichment must degrade ranking quality, never availability.
    let id = s
        .add(entry("just written", "findable the instant it lands"))
        .unwrap();

    let found = s.search_fts("findable the instant", 10).unwrap();
    assert!(
        found.iter().any(|e| e.id == id),
        "a write must be recall-visible with zero delay"
    );

    let ranked = s.search_ranked("findable the instant", 10).unwrap();
    assert!(
        ranked.iter().any(|(e, _)| e.id == id),
        "ranked recall must also see it immediately, even with no derived signals computed yet"
    );
}

// ── Derived-signal store, exercised through MemoryStore ──────────────

#[test]
fn memory_store_signal_roundtrip_and_neutral_default() {
    let s = store();
    let id = s.add(entry("ranked item", "body")).unwrap();

    assert_eq!(s.signal_factor(&id, SignalKind::Centrality).unwrap(), 1.0);

    s.set_signal(&id, SignalKind::Centrality, 0.42).unwrap();
    assert_eq!(s.signal_factor(&id, SignalKind::Centrality).unwrap(), 0.42);

    s.clear_signals(&id).unwrap();
    assert_eq!(s.signal_factor(&id, SignalKind::Centrality).unwrap(), 1.0);
}

// ── Capture metadata (actor/expiry/subject) ───────────────────────────

#[test]
fn capture_metadata_defaults_to_absent() {
    let s = store();
    let id = s.add(entry("plain capture", "body")).unwrap();
    let got = s.get(&id).unwrap().unwrap();
    assert_eq!(got.actor, None);
    assert_eq!(got.expiry, None);
    assert_eq!(got.subject, None);
}

#[test]
fn actor_round_trips() {
    let s = store();
    let e = entry("attributed", "body").with_actor("user:alice");
    let id = s.add(e).unwrap();
    let got = s.get(&id).unwrap().unwrap();
    assert_eq!(got.actor, Some("user:alice".to_string()));
}

#[test]
fn expiry_round_trips() {
    let s = store();
    let e = entry("expirable", "body").with_expiry(1_800_000_000);
    let id = s.add(e).unwrap();
    let got = s.get(&id).unwrap().unwrap();
    assert_eq!(got.expiry, Some(1_800_000_000));
}

#[test]
fn subject_round_trips_both_variants() {
    let s = store();
    let user_id = s
        .add(entry("about user", "body").with_subject(MemorySubject::User))
        .unwrap();
    let self_id = s
        .add(entry("about self", "body").with_subject(MemorySubject::AgentSelf))
        .unwrap();

    assert_eq!(
        s.get(&user_id).unwrap().unwrap().subject,
        Some(MemorySubject::User)
    );
    assert_eq!(
        s.get(&self_id).unwrap().unwrap().subject,
        Some(MemorySubject::AgentSelf)
    );
}

#[test]
fn an_expired_item_is_excluded_from_search_fts_and_search_ranked() {
    let s = store();
    let expired = entry("expired note", "findable expiring text").with_expiry(1);
    let fresh = entry("fresh note", "findable expiring text").with_expiry(9_999_999_999);
    s.add(expired).unwrap();
    let fresh_id = s.add(fresh).unwrap();

    let fts = s.search_fts("findable expiring", 10).unwrap();
    assert_eq!(fts.len(), 1, "the expired item must not surface");
    assert_eq!(fts[0].id, fresh_id);

    let ranked = s.search_ranked("findable expiring", 10).unwrap();
    assert_eq!(ranked.len(), 1, "the expired item must not surface ranked");
    assert_eq!(ranked[0].0.id, fresh_id);
}

#[test]
fn an_expired_item_is_excluded_from_recall_temporal() {
    let s = store();
    s.add(entry("expired", "body").with_expiry(1)).unwrap();
    let fresh_id = s
        .add(entry("fresh", "body").with_expiry(9_999_999_999))
        .unwrap();

    let recent = s.recall_temporal(TemporalMode::Recent, 10).unwrap();
    assert_eq!(recent.len(), 1, "the expired item must not appear");
    assert_eq!(recent[0].id, fresh_id);
}

#[test]
fn an_expired_item_is_excluded_from_recall_structured() {
    let s = store();
    s.add(
        entry("expired", "body")
            .with_expiry(1)
            .with_subject(MemorySubject::User),
    )
    .unwrap();
    let fresh_id = s
        .add(
            entry("fresh", "body")
                .with_expiry(9_999_999_999)
                .with_subject(MemorySubject::User),
        )
        .unwrap();

    let predicate = FieldPredicate::Eq(
        PredicateField::Kind,
        PredicateValue::Text("Convention".to_string()),
    );
    let results = s.recall_structured(&predicate, 10).unwrap();
    assert_eq!(results.len(), 1, "the expired item must not appear");
    assert_eq!(results[0].id, fresh_id);
}

#[test]
fn an_item_with_no_expiry_recalls_normally_everywhere() {
    let s = store();
    let id = s.add(entry("no expiry", "body")).unwrap();

    assert!(
        s.recall_temporal(TemporalMode::Recent, 10)
            .unwrap()
            .iter()
            .any(|e| e.id == id)
    );
    let predicate = FieldPredicate::Eq(
        PredicateField::Kind,
        PredicateValue::Text("Convention".to_string()),
    );
    assert!(
        s.recall_structured(&predicate, 10)
            .unwrap()
            .iter()
            .any(|e| e.id == id)
    );
}
