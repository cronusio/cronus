//! Cross-layer validation for the memory L2 pair: proves the consolidation
//! and intelligence — both query-surface
//! and capture-path — invariants hold
//! together on the built tier, through the real facade and the real SQLite
//! adapter, not each task's own isolated stub/unit tests.

use cronus_contract::{
    ExperienceOutcome, MemoryDepth, MemoryEntry, MemoryKind, MemorySource, MemorySubject,
};
use cronus_core::autonomy::AutonomyLevel;
use cronus_core::memory::MemoryStore;
use cronus_core::memory_capture::{
    CaptureDirectives, CaptureMode, NoGenerator, apply_directives, prepare_capture_body,
};
use cronus_core::memory_intelligence::{self, AnswerVerdict, ExperienceDecision, RunTrace};
use cronus_store_local::memory::{CaptureOutcome, ConsolidationAction, SignalKind};

// ── Capture consolidates and becomes answerable ──────

#[test]
fn raw_capture_consolidates_and_becomes_answerable_end_to_end() {
    let store = MemoryStore::open_in_memory().unwrap();
    let mut raw = MemoryEntry::new(
        MemoryKind::Convention,
        MemorySource::Agent,
        "raw note",
        "native Rust builds must run through PowerShell, not Git Bash",
    );
    raw.depth = MemoryDepth::Raw;
    store.add(raw).unwrap();

    let results = store.run_incremental_consolidation("test").unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].1, ConsolidationAction::Create);

    let answer = memory_intelligence::answer(&store, "PowerShell", 5);
    assert_eq!(answer.verdict, AnswerVerdict::Supported);
    assert!(
        answer.text.contains("PowerShell"),
        "the consolidated item must be answerable through the same seam intelligence reads"
    );
}

// ── cold start: no signal ever computed, ranking still works ──────

#[test]
fn cold_start_ranking_degrades_to_base_relevance_with_no_signals_computed() {
    let store = MemoryStore::open_in_memory().unwrap();
    store
        .add(MemoryEntry::new(
            MemoryKind::Convention,
            MemorySource::Agent,
            "a",
            "the deploy pipeline uses a staging gate",
        ))
        .unwrap();
    store
        .add(MemoryEntry::new(
            MemoryKind::Convention,
            MemorySource::Agent,
            "b",
            "the staging gate requires manual approval",
        ))
        .unwrap();

    // No recompute_centrality / recompute_recency / signal write of any kind.
    let ranked = store.search_ranked("staging gate", 10).unwrap();
    assert_eq!(
        ranked.len(),
        2,
        "cold-start recall must not error or drop hits"
    );
    for (_, score) in &ranked {
        assert!(
            *score > 0.0,
            "a neutral-factor score must still be positive"
        );
    }
}

// ── The ranking-time signal read never re-walks the live graph ──

#[test]
fn centrality_signal_is_precomputed_not_rewalked_on_every_read() {
    // Centrality normalizes by the graph's max in-degree, so proving
    // staleness needs two competing hubs: `rival` fixed at 2 in-edges,
    // `hub` starting at 1 (normalized 1/2) and later overtaking it at 3
    // (normalized 3/3) once recomputed — a single-hub graph is always its
    // own max and would trivially read 1.0 before and after, proving nothing.
    let store = MemoryStore::open_in_memory().unwrap();
    let hub_id = store
        .add(MemoryEntry::new(
            MemoryKind::Convention,
            MemorySource::Agent,
            "hub",
            "hub raw fact",
        ))
        .unwrap();
    let rival_id = store
        .add(MemoryEntry::new(
            MemoryKind::Convention,
            MemorySource::Agent,
            "rival",
            "rival raw fact",
        ))
        .unwrap();
    for body in ["rival edge one", "rival edge two"] {
        let c = MemoryEntry::new(MemoryKind::Convention, MemorySource::Agent, "r", body);
        store.consolidate(c, Some(&rival_id), "test").unwrap();
    }

    let c1 = MemoryEntry::new(
        MemoryKind::Convention,
        MemorySource::Agent,
        "c1",
        "alpha derived fact",
    );
    store.consolidate(c1, Some(&hub_id), "test").unwrap();
    store.recompute_centrality().unwrap();
    let factor_after_one_edge = store
        .signal_factor(&hub_id, SignalKind::Centrality)
        .unwrap();

    // Two more edges land on `hub`, overtaking `rival`'s in-degree, but
    // centrality is never recomputed again — the next read must not
    // silently re-derive it from the now-changed live graph.
    let c2 = MemoryEntry::new(
        MemoryKind::Convention,
        MemorySource::Agent,
        "c2",
        "beta derived fact",
    );
    store.consolidate(c2, Some(&hub_id), "test").unwrap();
    let c3 = MemoryEntry::new(
        MemoryKind::Convention,
        MemorySource::Agent,
        "c3",
        "gamma derived fact",
    );
    store.consolidate(c3, Some(&hub_id), "test").unwrap();

    let factor_still = store
        .signal_factor(&hub_id, SignalKind::Centrality)
        .unwrap();
    assert_eq!(
        factor_after_one_edge, factor_still,
        "a ranking-time signal read must not silently re-walk the live edge graph"
    );

    // Confirm the graph really did change underneath the stale signal —
    // otherwise this test would pass vacuously for the wrong reason.
    store.recompute_centrality().unwrap();
    let factor_after_recompute = store
        .signal_factor(&hub_id, SignalKind::Centrality)
        .unwrap();
    assert_ne!(
        factor_still, factor_after_recompute,
        "recompute_centrality must actually observe the new edges once explicitly re-run"
    );
}

// ── The fact/derived boundary holds through the public write path ────

#[test]
fn signal_writes_never_touch_the_authored_fact_columns() {
    let store = MemoryStore::open_in_memory().unwrap();
    let entry = MemoryEntry::new(
        MemoryKind::Convention,
        MemorySource::Agent,
        "authored title",
        "authored body text",
    );
    let id = store.add(entry).unwrap();

    store.set_signal(&id, SignalKind::Recency, 0.9).unwrap();
    store.set_signal(&id, SignalKind::Cluster, 0.4).unwrap();

    let after = store.get(&id).unwrap().expect("item must still exist");
    assert_eq!(after.title, "authored title");
    assert_eq!(after.body, "authored body text");
}

// ── A distilled experience round-trips through the real adapter ─

#[test]
fn distilled_experience_round_trips_through_the_real_store_and_is_reused() {
    let store = MemoryStore::open_in_memory().unwrap();
    let trace = RunTrace {
        objective: "restart the flaky worker after a timeout".to_string(),
        actions_taken: vec![],
        findings: vec![],
        end_state: "worker restarts cleanly".to_string(),
        next_steps: vec![],
    };
    let mut distilled = memory_intelligence::distill_run(&trace, ExperienceOutcome::Success);
    distilled.trust_score = 0.9;
    distilled.verification_state = cronus_contract::VerificationState::TestedInProject;
    let distilled_id = distilled.id.clone();
    store.add(distilled).unwrap();

    let decision = memory_intelligence::recall_for_reuse(
        &store,
        "restart the flaky worker after a timeout",
        cronus_contract::now_secs(),
        false,
        AutonomyLevel::Autonomous,
        10,
    );

    match decision {
        ExperienceDecision::Reuse(citation) => assert_eq!(citation.item_id, distilled_id),
        other => panic!(
            "expected the distilled success to be reused through the real adapter, got {other:?}"
        ),
    }
}

// ── Capture policy with full metadata, through the real adapter ──────

#[test]
fn a_fully_attributed_capture_persists_metadata_and_cross_ref_edges_for_real() {
    let store = MemoryStore::open_in_memory().unwrap();
    let hub_id = store
        .add(MemoryEntry::new(
            MemoryKind::Convention,
            MemorySource::Agent,
            "hub",
            "a hub fact",
        ))
        .unwrap();

    let entry = MemoryEntry::new(
        MemoryKind::Convention,
        MemorySource::Agent,
        "attributed",
        "a fully attributed capture",
    )
    .with_actor("user:alice")
    .with_subject(MemorySubject::User)
    .with_expiry(9_999_999_999);

    let outcome = store
        .capture(entry, std::slice::from_ref(&hub_id), "test")
        .unwrap();
    let CaptureOutcome::Stored(id) = outcome else {
        panic!("expected Stored, got {outcome:?}");
    };

    let got = store.get(&id).unwrap().unwrap();
    assert_eq!(got.actor, Some("user:alice".to_string()));
    assert_eq!(got.subject, Some(MemorySubject::User));
    assert_eq!(got.expiry, Some(9_999_999_999));

    let edges = store.edges_from(&id).unwrap();
    assert!(
        edges.iter().any(|(target, _)| *target == hub_id),
        "the cross-ref edge must be readable through the real graph"
    );
}

#[test]
fn a_below_floor_capture_is_refused_through_the_real_adapter_and_writes_nothing() {
    let store = MemoryStore::open_in_memory().unwrap();
    let mut entry = MemoryEntry::new(
        MemoryKind::Convention,
        MemorySource::Agent,
        "unreliable",
        "an unreliable guess",
    );
    entry.confidence = 0.05;

    let outcome = store.capture(entry, &[], "test").unwrap();
    assert!(matches!(outcome, CaptureOutcome::Refused { .. }));

    let hits = store.search_fts("unreliable guess", 10).unwrap();
    assert!(hits.is_empty(), "a refused capture must not be recallable");
}

// ── A raw capture with no generator is immediately recallable ──

#[test]
fn a_raw_capture_with_no_generator_bound_flows_through_to_real_recall() {
    let store = MemoryStore::open_in_memory().unwrap();
    let body = prepare_capture_body(
        &NoGenerator,
        "meet next Tuesday about the launch",
        CaptureMode::Raw,
        cronus_contract::now_secs(),
    );
    // Raw never touches the generator, so the relative date is untouched —
    // proving the no-generator degrade holds all the way to a real,
    // immediately-recallable stored item, not just the pure function.
    assert!(body.contains("next Tuesday"));

    let entry = MemoryEntry::new(
        MemoryKind::Convention,
        MemorySource::Agent,
        "raw note",
        &body,
    );
    let outcome = store.capture(entry, &[], "test").unwrap();
    assert!(matches!(outcome, CaptureOutcome::Stored(_)));

    let hits = store.search_fts("launch", 10).unwrap();
    assert_eq!(
        hits.len(),
        1,
        "a raw capture must be immediately recallable"
    );
}

// ── Directives shape what actually lands in the real store ──────────

#[test]
fn directive_shaped_content_lands_in_the_real_store_with_the_safety_guard_intact() {
    let store = MemoryStore::open_in_memory().unwrap();
    let directives = CaptureDirectives {
        exclude: vec!["irrelevant".to_string()],
        ..Default::default()
    };
    let out = apply_directives(
        "the useful fact. warning: an irrelevant but unsafe aside.",
        &directives,
    );
    // The safety guard must survive even though "irrelevant" matched the
    // exclude term — real end-to-end proof, not just the pure-function test.
    assert!(out.content.contains("unsafe aside"));

    let entry = MemoryEntry::new(
        MemoryKind::Convention,
        MemorySource::Agent,
        "directed capture",
        &out.content,
    );
    let outcome = store.capture(entry, &[], "test").unwrap();
    let CaptureOutcome::Stored(id) = outcome else {
        panic!("expected Stored, got {outcome:?}");
    };
    let got = store.get(&id).unwrap().unwrap();
    assert!(
        got.body.contains("unsafe aside"),
        "the safety-relevant sentence directives retained must actually reach storage"
    );
}
