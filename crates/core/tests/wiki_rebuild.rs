//! End-to-end proof that the project wiki is a rebuildable projection cache.
//!
//! Two properties, both through the whole seam (domain pipeline → the real
//! SQLite `WikiStore`, no fakes):
//!
//! 1. **Drop-loses-nothing:** build a wiki into a `wiki.db`, capture it,
//!    physically delete the file, then rebuild from the SAME ground truth into
//!    a brand-new empty store — the result is an *equivalent* wiki (same page
//!    structure, same sources, same fingerprints). Nothing authoritative ever
//!    lived only in `wiki.db`, so losing the file loses nothing durable.
//! 2. **Access gate:** reading the real store through `GatedWiki` is
//!    denied without a `Read` grant and allowed with one.

use std::path::PathBuf;

use cronus_contract::{WikiCitation, WikiPage, WikiPageKind};
use cronus_core::resource_sharing::{
    AccessGrant, GrantStore, Permission, PrincipalKind, ResourceKind,
};
use cronus_core::wiki_access::{GatedWiki, WikiAccessError, WikiPrincipal};
use cronus_core::wiki_regen::{GroundTruth, rebuild};
use cronus_store_local::wiki::WikiStore;

/// Ground truth for the office — the authoritative sources that outlive any
/// `wiki.db`. Fixed per kind so a rebuild is deterministic here (no model):
/// each kind cites two records, so every page is grounded and non-empty.
struct FixtureGround;

impl GroundTruth for FixtureGround {
    fn sources(&self, _office_id: &str, kind: WikiPageKind) -> Result<Vec<WikiCitation>, String> {
        Ok(vec![
            WikiCitation::new("decision", format!("dec-{}", kind.as_str())),
            WikiCitation::new("board_item", format!("card-{}", kind.as_str())),
        ])
    }
}

fn unique_db_path(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "cronus-wiki-{tag}-{}-{nanos}.db",
        std::process::id()
    ))
}

/// The structural + attribution projection of a page — everything the
/// equivalence guarantee covers, excluding wall-clock `generated_at` (which
/// legitimately differs between two builds) and the model-generated `body`
/// (equivalence is informational, not byte-identical prose): the id, kind,
/// tree placement, citations, and source fingerprint.
type PageProjection = (
    String,
    WikiPageKind,
    Option<String>,
    i64,
    Vec<WikiCitation>,
    String,
);

fn projection(p: &WikiPage) -> PageProjection {
    (
        p.id.clone(),
        p.kind,
        p.parent_id.clone(),
        p.ord,
        p.citations.clone(),
        p.source_fingerprint.clone(),
    )
}

fn equivalent_set(store: &WikiStore, office: &str) -> Vec<PageProjection> {
    let mut pages = store.pages_for_office(office).expect("pages");
    pages.sort_by(|a, b| a.id.cmp(&b.id));
    pages.iter().map(projection).collect()
}

#[test]
fn dropping_wiki_db_loses_nothing_a_rebuild_reconstructs_an_equivalent_wiki() {
    let office = "office-1";
    let ground = FixtureGround;

    // --- Build the wiki into a real on-disk wiki.db ---
    let path_a = unique_db_path("a");
    let before = {
        let store = WikiStore::open(&path_a).expect("open a");
        rebuild(office, &ground, None, &store).expect("initial build");
        let snapshot = equivalent_set(&store, office);
        assert_eq!(snapshot.len(), 6, "all six page kinds materialized");
        assert!(
            snapshot
                .iter()
                .all(|(_, _, _, _, cites, _)| !cites.is_empty()),
            "every page is grounded"
        );
        snapshot
        // `store` drops here, closing the connection.
    };

    // --- Drop wiki.db entirely: the file is gone ---
    std::fs::remove_file(&path_a).expect("delete wiki.db");
    assert!(!path_a.exists(), "wiki.db is gone");

    // --- Rebuild from the SAME ground truth into a fresh, empty store ---
    let path_b = unique_db_path("b");
    let after = {
        let fresh = WikiStore::open(&path_b).expect("open b");
        // The fresh store starts empty — nothing carried over from `wiki.db`.
        assert!(fresh.pages_for_office(office).unwrap().is_empty());
        rebuild(office, &ground, None, &fresh).expect("rebuild");
        equivalent_set(&fresh, office)
    };

    assert_eq!(
        before, after,
        "the rebuilt wiki is equivalent — same structure, sources, and fingerprints"
    );

    std::fs::remove_file(&path_b).ok();
}

#[test]
fn a_client_read_through_the_real_store_is_access_gated() {
    let office = "office-1";
    let path = unique_db_path("gate");
    let store = WikiStore::open(&path).expect("open");
    rebuild(office, &FixtureGround, None, &store).expect("build");

    // Without a grant, a non-owner is denied — the read never reaches
    // the store.
    let empty_grants = GrantStore::new();
    let denied = GatedWiki::new(
        &store,
        &empty_grants,
        WikiPrincipal::member("stranger", vec![]),
    );
    assert!(matches!(
        denied.children(office, None),
        Err(WikiAccessError::Denied { .. })
    ));

    // With a Read grant, the same read succeeds against the real store.
    let mut grants = GrantStore::new();
    grants.add(AccessGrant {
        resource_type: ResourceKind::Wiki,
        resource_id: office.to_string(),
        principal_type: PrincipalKind::User,
        principal_id: "alice".to_string(),
        permission: Permission::Read,
    });
    let allowed = GatedWiki::new(&store, &grants, WikiPrincipal::member("alice", vec![]));
    let roots = allowed.children(office, None).expect("granted read");
    assert!(!roots.is_empty(), "the granted client sees the wiki pages");

    drop(store);
    std::fs::remove_file(&path).ok();
}
