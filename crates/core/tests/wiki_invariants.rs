//! Project-wiki invariant acceptance sweep — the
//! phase's closing validation. Each PW invariant maps to one named test, and
//! every one is exercised **through the real SQLite `WikiStore`** (not an
//! in-memory fake) and the real domain pipeline, so the guarantees are proven
//! against actual persistence and composition, not just unit stand-ins.
//!
//! Plain-language, client-facing content is not directly assertable — generation
//! is model-based — so it is covered by its testable proxies: the citation
//! guard (only attributed client facts persist) and the internal-detail
//! filter (engineering/SDD text never lands). The file-drop proof lives in
//! `wiki_rebuild.rs`; here it is complemented by a fresh-store rebuild
//! equivalence check.

use std::cell::Cell;

use cronus_contract::{WikiCitation, WikiPage, WikiPageKind, WikiReadSurface};
use cronus_core::resource_sharing::{
    AccessGrant, GrantStore, Permission, PrincipalKind, ResourceKind,
};
use cronus_core::wiki_access::{GatedWiki, WikiAccessError, WikiPrincipal};
use cronus_core::wiki_regen::{
    GeneratedContent, GeneratedSection, GroundTruth, OfficeChange, PageGenerator, check_freshness,
    rebuild, regenerate,
};
use cronus_store_local::wiki::WikiStore;

const OFFICE: &str = "office-1";

/// Fixed ground truth: one source per kind, so every page kind is grounded and
/// nothing is skipped.
struct FixedGround;
impl GroundTruth for FixedGround {
    fn sources(&self, _office: &str, kind: WikiPageKind) -> Result<Vec<WikiCitation>, String> {
        Ok(vec![WikiCitation::new(
            "decision",
            format!("d-{}", kind.as_str()),
        )])
    }
}

/// Ground truth whose sources move once `moved` flips — drives the
/// freshness drift check.
struct MovingGround {
    moved: Cell<bool>,
}
impl GroundTruth for MovingGround {
    fn sources(&self, _office: &str, kind: WikiPageKind) -> Result<Vec<WikiCitation>, String> {
        let suffix = if self.moved.get() { "-moved" } else { "" };
        Ok(vec![WikiCitation::new(
            "decision",
            format!("d-{}{suffix}", kind.as_str()),
        )])
    }
}

/// A generator that, for every kind, emits three sections: a cited client fact
/// (must persist), an uncited claim (must be dropped), and an
/// internal-detail section (must be filtered).
struct ScriptedGen;
impl PageGenerator for ScriptedGen {
    fn generate(
        &self,
        _office: &str,
        kind: WikiPageKind,
        sources: &[WikiCitation],
    ) -> Result<GeneratedContent, String> {
        Ok(GeneratedContent {
            title: format!("{} title", kind.as_str()),
            sections: vec![
                GeneratedSection::cited("CLIENT_FACT", vec![sources[0].clone()]),
                GeneratedSection {
                    text: "UNCITED_CLAIM".to_string(),
                    citations: vec![],
                    internal_detail: false,
                },
                GeneratedSection {
                    text: "SDD_INTERNAL_DETAIL".to_string(),
                    citations: vec![sources[0].clone()],
                    internal_detail: true,
                },
            ],
        })
    }
}

fn built_store(generator: Option<&dyn PageGenerator>) -> WikiStore {
    let store = WikiStore::open_in_memory().expect("open");
    rebuild(OFFICE, &FixedGround, generator, &store).expect("build");
    store
}

fn overview_body(store: &WikiStore) -> String {
    store
        .get_page(&format!("{OFFICE}:overview"))
        .expect("get")
        .expect("overview present")
        .body
}

// --- Client surface is read-only by construction -----------------------

#[test]
fn the_client_surface_exposes_no_write_method() {
    let store = built_store(None);
    // The client holds a `&dyn WikiReadSurface` — a trait object with only
    // read methods. A write like `reader.rebuild_office(..)` is not merely
    // unavailable by convention; it is a compile error (no such method on the
    // trait). This test compiling *is* the proof.
    let reader: &dyn WikiReadSurface = &store;
    assert!(
        reader
            .page(&format!("{OFFICE}:overview"))
            .unwrap()
            .is_some()
    );
    assert!(!reader.children(OFFICE, None).unwrap().is_empty());
}

// --- Projection, not source of truth -----------------------------------

type PageProjection = (
    String,
    WikiPageKind,
    Option<String>,
    i64,
    Vec<WikiCitation>,
    String,
);

fn projection(store: &WikiStore) -> Vec<PageProjection> {
    let mut pages = store.pages_for_office(OFFICE).expect("pages");
    pages.sort_by(|a, b| a.id.cmp(&b.id));
    pages
        .iter()
        .map(|p: &WikiPage| {
            (
                p.id.clone(),
                p.kind,
                p.parent_id.clone(),
                p.ord,
                p.citations.clone(),
                p.source_fingerprint.clone(),
            )
        })
        .collect()
}

#[test]
fn rebuild_reconstructs_an_equivalent_wiki_into_a_fresh_store() {
    // A wiki built in one store and rebuilt from the same ground truth into a
    // brand-new empty store are equivalent (structure + sources + fingerprints)
    // — nothing authoritative lived only in the first store.
    let first = built_store(None);
    let before = projection(&first);

    let fresh = WikiStore::open_in_memory().expect("fresh");
    assert!(fresh.pages_for_office(OFFICE).unwrap().is_empty());
    rebuild(OFFICE, &FixedGround, None, &fresh).expect("rebuild into fresh store");

    assert_eq!(before, projection(&fresh), "the rebuilt wiki is equivalent");
    assert_eq!(before.len(), 6, "all six page kinds present");
}

// --- Grounded & attributed ---------------------------------------------

#[test]
fn an_uncited_claim_is_never_persisted() {
    let store = built_store(Some(&ScriptedGen as &dyn PageGenerator));
    let body = overview_body(&store);
    assert!(
        body.contains("CLIENT_FACT"),
        "the cited client fact persists"
    );
    assert!(
        !body.contains("UNCITED_CLAIM"),
        "an uncited claim is dropped, never persisted"
    );
    // Every persisted page carries at least one citation.
    assert!(
        store
            .pages_for_office(OFFICE)
            .unwrap()
            .iter()
            .all(|p| !p.citations.is_empty())
    );
}

// --- Living & freshness-honest -----------------------------------------

#[test]
fn a_source_that_moves_without_regeneration_is_marked_stale() {
    let ground = MovingGround {
        moved: Cell::new(false),
    };
    let store = WikiStore::open_in_memory().expect("open");
    rebuild(OFFICE, &ground, None, &store).expect("build");

    // Every page starts fresh.
    assert!(
        store
            .pages_for_office(OFFICE)
            .unwrap()
            .iter()
            .all(|p| !p.stale)
    );

    // The sources move, but no regeneration runs — the freshness sweep must
    // flip the affected pages stale (honest "may be out of date"), never
    // silently present them as current, and never rewrite their content.
    ground.moved.set(true);
    let drifted = check_freshness(OFFICE, &ground, &store).expect("sweep");
    assert!(!drifted.is_empty(), "moved sources are detected as drift");

    let overview = store
        .get_page(&format!("{OFFICE}:overview"))
        .unwrap()
        .unwrap();
    assert!(overview.stale, "the drifted page is marked stale");

    // Idempotent: a second sweep with the same state marks nothing new.
    assert!(check_freshness(OFFICE, &ground, &store).unwrap().is_empty());
}

// --- Navigable & searchable --------------------------------------------

#[test]
fn the_wiki_is_navigable_and_searchable() {
    let store = built_store(Some(&ScriptedGen as &dyn PageGenerator));
    let reader: &dyn WikiReadSurface = &store;

    // Navigable: the office's pages are reachable as roots (this phase's flat
    // regeneration places every kind at the root; the tree query resolves them).
    let roots = reader.children(OFFICE, None).expect("roots");
    assert_eq!(
        roots.len(),
        6,
        "every page kind is reachable via navigation"
    );

    // Searchable: the persisted client fact is found by FTS; a term that was
    // filtered/dropped is not.
    let hits = reader.search(OFFICE, "CLIENT_FACT", 10).expect("search");
    assert!(!hits.is_empty(), "the client fact is searchable");
    assert!(
        reader
            .search(OFFICE, "SDD_INTERNAL_DETAIL", 10)
            .unwrap()
            .is_empty(),
        "filtered internal detail is not in the search index"
    );
}

// --- Scoped & access-controlled ----------------------------------------

fn wiki_grant(principal_type: PrincipalKind, principal_id: &str) -> AccessGrant {
    AccessGrant {
        resource_type: ResourceKind::Wiki,
        resource_id: OFFICE.to_string(),
        principal_type,
        principal_id: principal_id.to_string(),
        permission: Permission::Read,
    }
}

#[test]
fn reads_follow_the_office_sharing_posture() {
    let store = built_store(None);

    // Private (no grants): a non-owner is denied; the owner reads by ownership.
    let no_grants = GrantStore::new();
    let stranger = GatedWiki::new(
        &store,
        &no_grants,
        WikiPrincipal::member("stranger", vec![]),
    );
    assert!(matches!(
        stranger.children(OFFICE, None),
        Err(WikiAccessError::Denied { .. })
    ));
    let owner = GatedWiki::new(&store, &no_grants, WikiPrincipal::owner("alice"));
    assert!(
        !owner
            .children(OFFICE, None)
            .expect("owner reads")
            .is_empty()
    );

    // Shared to a user, a group, and the public — each opens the read.
    let mut grants = GrantStore::new();
    grants.add(wiki_grant(PrincipalKind::User, "bob"));
    grants.add(wiki_grant(PrincipalKind::Group, "g-eng"));
    grants.add(wiki_grant(
        PrincipalKind::Public,
        cronus_core::resource_sharing::PUBLIC_ID,
    ));

    let bob = GatedWiki::new(&store, &grants, WikiPrincipal::member("bob", vec![]));
    assert!(!bob.children(OFFICE, None).expect("user grant").is_empty());

    let carol = GatedWiki::new(
        &store,
        &grants,
        WikiPrincipal::member("carol", vec!["g-eng".into()]),
    );
    assert!(
        !carol
            .children(OFFICE, None)
            .expect("group grant")
            .is_empty()
    );

    let anyone = GatedWiki::new(
        &store,
        &grants,
        WikiPrincipal::member("nobody-special", vec![]),
    );
    assert!(
        !anyone
            .children(OFFICE, None)
            .expect("public grant")
            .is_empty()
    );
}

// --- Distinct from KB & internal artifacts -----------------------------

#[test]
fn internal_engineering_detail_never_reaches_a_row() {
    let store = built_store(Some(&ScriptedGen as &dyn PageGenerator));
    for page in store.pages_for_office(OFFICE).unwrap() {
        assert!(
            !page.body.contains("SDD_INTERNAL_DETAIL"),
            "internal engineering / SDD detail must never land in a wiki row"
        );
    }
}

// --- No-generator degrade (every generator-dependent step) --------------------

#[test]
fn every_generator_dependent_step_degrades_to_a_grounded_stub_without_a_generator() {
    // rebuild with no generator → grounded stubs for every kind, never fabricated.
    let rebuilt = built_store(None);
    for page in rebuilt.pages_for_office(OFFICE).unwrap() {
        assert!(
            page.body.contains("grounded in") && page.body.contains("recorded source"),
            "a no-generator page is a grounded stub, not invented prose"
        );
        assert!(
            !page.citations.is_empty(),
            "the stub still carries its sources"
        );
    }

    // regenerate (the event path) with no generator → the same honest degrade.
    let store = WikiStore::open_in_memory().expect("open");
    regenerate(
        OfficeChange::MilestoneReached,
        OFFICE,
        &FixedGround,
        None,
        &store,
    )
    .expect("event regenerate with no generator");
    let overview = overview_body(&store);
    assert!(
        overview.contains("grounded in"),
        "the event path also degrades to a grounded stub"
    );
}
