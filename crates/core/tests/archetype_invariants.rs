//! Office-archetype invariant acceptance sweep —
//! the closing validation for this subsystem. Each testable
//! invariant maps to one named test, exercised through the **real facade
//! export chain** (`cronus_core::archetype::{...}`) — proving the assembled
//! re-export composes, not just that each domain module works in isolation.
//!
//! The deep unit-level proof for every invariant lives in
//! `crates/domain/src/archetype/{schema,catalog,selection,deviation}.rs`'s own
//! 24 tests, and the real CLI wiring in `crates/cli/tests/cli_smoke.rs`'s 4
//! binary-spawning tests. **Imported-archetype content vetting** is
//! deferred this phase — it depends on the admission-vetting gate — and is
//! cited here rather than tested.

use cronus_core::archetype::{
    ActiveArchetype, ArchetypeCatalog, ArchetypeDeviations, ArchetypeError, InferenceResult,
    OfficeDeviations, SeedEntry, ValidationStatus, infer, software_engineering,
    validate_definition_keys,
};

// --- A prior, not a roster — no "hire these" field, empty seed --------

#[test]
fn oa1_the_shipped_archetype_carries_no_hire_field_and_seeds_no_one() {
    let def = software_engineering();
    // The definition's only fields are pool/shape/seed/norms (+ identity):
    // there is no field expressing "hire these". Application hires only the
    // seed, which is empty — nobody is seated before the first sentence.
    assert!(def.seed.is_empty());
    assert!(!def.pool.is_empty());
}

// --- Bounded, justified seed ------------------------------------------

#[test]
fn oa2_a_seed_over_the_cap_or_without_justification_fails() {
    let mut def = software_engineering();
    def.seed = vec![
        SeedEntry {
            role: "architect".to_string(),
            justification: "a".to_string(),
        },
        SeedEntry {
            role: "backend-engineer".to_string(),
            justification: "b".to_string(),
        },
        SeedEntry {
            role: "doc-writer".to_string(),
            justification: "c".to_string(),
        },
    ];
    assert_eq!(def.validate(), Err(ArchetypeError::SeedTooLarge(3)));

    def.seed = vec![SeedEntry {
        role: "architect".to_string(),
        justification: " ".to_string(),
    }];
    assert_eq!(
        def.validate(),
        Err(ArchetypeError::MissingJustification(
            "architect".to_string()
        ))
    );
}

// --- A hire outside the pool succeeds and is recorded, never refused ---

#[test]
fn oa3_a_hire_outside_the_pool_is_recorded_not_refused() {
    let pool = software_engineering().pool;
    let mut dev = OfficeDeviations::default();
    // record_hire returns () — no channel to refuse; it classifies a hire the
    // gate already allowed. "marketing" is a real role but outside the SE pool.
    dev.record_hire(&pool, "marketing");
    assert_eq!(dev.hired_outside_pool, 1);
}

// --- A fifth schema key is unrepresentable ----------------------------

#[test]
fn oa4_an_authority_key_fails_the_closed_schema_gate() {
    assert_eq!(
        validate_definition_keys(&[
            "id",
            "domain",
            "pool",
            "shape",
            "seed",
            "norms",
            "permissions"
        ]),
        Err(ArchetypeError::UnknownKey("permissions".to_string()))
    );
}

// --- preset/custom split records derived_from -------------------------

#[test]
fn oa6_create_from_preset_records_derived_from_and_leaves_the_source_untouched() {
    let dir = std::env::temp_dir().join(format!("cronus-archetype-sweep-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let catalog = ArchetypeCatalog::program();
    let custom = catalog
        .create_from_preset(&dir, "my-eng", "software-engineering")
        .expect("create_from_preset");
    assert_eq!(custom.derived_from, "software-engineering");
    assert_eq!(
        catalog.get("software-engineering").unwrap().id,
        "software-engineering"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// --- Inference is silent; inconclusive → archetype-free ---------------

#[test]
fn oa7_inference_is_confident_on_software_intent_and_inconclusive_otherwise() {
    let catalog = ArchetypeCatalog::program();
    assert_eq!(
        infer(&catalog, "build the backend API and deploy the app"),
        InferenceResult::Confident("software-engineering".to_string())
    );
    assert_eq!(
        infer(&catalog, "throw a party"),
        InferenceResult::Inconclusive
    );
}

// --- One active; set touches no staff ---------------------------------

#[test]
fn oa8_set_and_clear_touch_only_the_archetype_record() {
    let mut office = ActiveArchetype::default();
    office.set("software-engineering");
    assert_eq!(office.active.as_deref(), Some("software-engineering"));
    office.clear();
    assert!(office.is_archetype_free());
}

// --- Three counters + the unvalidated state ---------------------------

#[test]
fn oa9_an_unobserved_archetype_is_unvalidated_never_correct() {
    let agg = ArchetypeDeviations::default();
    assert_eq!(agg.status(), ValidationStatus::Unvalidated);

    let mut agg2 = ArchetypeDeviations::default();
    agg2.absorb(&OfficeDeviations::default()); // observed, zero deviations
    assert!(matches!(agg2.status(), ValidationStatus::Validated { .. }));
}

// --- An unknown role id rejects the archetype (why two are blocked) --

#[test]
fn oa10_an_unknown_role_id_rejects_the_archetype() {
    let mut def = software_engineering();
    def.pool.push("copywriter".to_string()); // a role the catalog lacks
    assert_eq!(
        def.validate(),
        Err(ArchetypeError::UnknownRole("copywriter".to_string()))
    );

    // This is exactly why the two non-technical archetypes are blocked, not
    // shipped: their pools name roles the catalog does not hold.
    let catalog = ArchetypeCatalog::program();
    assert!(catalog.get("advertising-agency").is_none());
    assert!(catalog.blocked_status("advertising-agency").is_some());
}

// --- The archetype-free office is a complete state --------------------

#[test]
fn oa11_the_archetype_free_office_is_a_complete_default() {
    let office = ActiveArchetype::default();
    assert!(office.is_archetype_free());
    assert_eq!(office.active, None);
}
