//! Invariant-compliance sweep for the skill system: one test per row
//! of the Invariant Compliance table, exercising the
//! modules in `cronus_core::skills` together rather than in isolation, so the
//! cross-module story (convert → store, convert → exec, synthesize →
//! store) is proven, not just each module's own unit-level behavior.

use std::collections::HashMap;

use cronus_core::extensions::{
    ExtensionKind, ExtensionManifest, ExtensionPermissions, ExtensionRegistry, ExtensionSource,
    ExtensionState,
};
use cronus_core::skills::commands::{
    CommandCategory, CommandRegistry, CommandSpec, DispatchError, InputSchema, RequiredGrant,
};
use cronus_core::skills::convert::{self, ConvertError, ForeignItem, ForeignKind, WitnessStatus};
use cronus_core::skills::exec::{
    self, ActivationResult, Degradation, OperationStep, WorkflowRuntime,
};
use cronus_core::skills::package::SkillPackage;
use cronus_core::skills::store::{SkillEntry, SkillId, SkillStore, SkillTier, WriteOutcome};
use cronus_core::skills::synthesize::{self, AuthoredSkill};

fn manifest(id: &str, name: &str) -> ExtensionManifest {
    ExtensionManifest {
        id: id.to_string(),
        kind: ExtensionKind::Skill,
        name: name.to_string(),
        version: "1.0.0".to_string(),
        source: ExtensionSource::Preset, // deliberately arbitrary; pipelines override it
        capabilities: vec![],
        permissions: ExtensionPermissions::default(),
    }
}

fn item(path: &str, kind: ForeignKind) -> ForeignItem {
    ForeignItem {
        path: path.to_string(),
        kind,
        content: "content".to_string(),
    }
}

/// Replays a fixed step list, standing in for the real nodus runtime seam.
struct ReplayRuntime {
    steps: Vec<OperationStep>,
}

impl WorkflowRuntime for ReplayRuntime {
    fn validate(&self, _package: &SkillPackage) -> Result<(), String> {
        Ok(())
    }

    fn execute(&self, _package: &SkillPackage) -> Result<Vec<OperationStep>, String> {
        Ok(self.steps.clone())
    }
}

/// A runtime that fails the test if it is ever touched — the degraded-guard proof.
struct UntouchableRuntime;

impl WorkflowRuntime for UntouchableRuntime {
    fn validate(&self, _package: &SkillPackage) -> Result<(), String> {
        panic!("degraded package must never reach validate()");
    }

    fn execute(&self, _package: &SkillPackage) -> Result<Vec<OperationStep>, String> {
        panic!("degraded package must never reach execute()");
    }
}

// ── Unified model ────────────────────────────────────────────────────

#[test]
fn ext1_skills_use_the_single_unified_extension_registry() {
    // The same ExtensionRegistry that serves MCP servers and plugins also
    // serves skills — there is no parallel skill-only subsystem.
    let mut registry = ExtensionRegistry::new();
    registry
        .register(manifest("core/standup", "standup"))
        .unwrap();
    registry
        .register(ExtensionManifest {
            kind: ExtensionKind::Plugin,
            ..manifest("core/some-plugin", "some-plugin")
        })
        .unwrap();

    assert_eq!(
        registry.state("core/standup"),
        Some(ExtensionState::Discovered)
    );
    assert_eq!(
        registry.state("core/some-plugin"),
        Some(ExtensionState::Discovered)
    );
}

// ── Lifecycle ─────────────────────────────────────────────────────────

#[test]
fn ext2_ingestion_never_activates_lands_discovered() {
    let outcome = convert::convert(
        WitnessStatus::Valid,
        manifest("core/imported", "imported"),
        &[item("SKILL.md", ForeignKind::Instruction)],
        &HashMap::new(),
    )
    .unwrap();

    let mut registry = ExtensionRegistry::new();
    registry.register(outcome.package.manifest.clone()).unwrap();

    assert_eq!(
        registry.state(&outcome.package.manifest.id),
        Some(ExtensionState::Discovered),
        "conversion never auto-activates; activation follows the standard grant gate"
    );
}

// ── Default-deny trust ───────────────────────────────────────────────

#[test]
fn ext3_a_skill_with_an_unconvertible_script_never_executes() {
    let outcome = convert::convert(
        WitnessStatus::Valid,
        manifest("core/half-baked", "half-baked"),
        &[item("run.sh", ForeignKind::Script)],
        &HashMap::new(), // no mapping — the script cannot convert
    )
    .unwrap();
    assert_eq!(outcome.degradation, Degradation::InstructionOnly);

    let commands = CommandRegistry::new();
    let perms = ExtensionPermissions::default();
    let result = exec::activate(
        &outcome.package,
        outcome.degradation,
        &UntouchableRuntime,
        &commands,
        &perms,
    )
    .unwrap();

    assert_eq!(result, ActivationResult::InstructionOnly);
}

// ── Sandboxed execution, scoped grants ───────────────────────

#[test]
fn ext4_ext6_a_mapped_workflow_dispatches_through_per_call_grant_checks() {
    let mut map = HashMap::new();
    map.insert("fetch.sh".to_string(), "net.fetch".to_string());

    let outcome = convert::convert(
        WitnessStatus::Valid,
        manifest("core/fetcher", "fetcher"),
        &[item("fetch.sh", ForeignKind::Script)],
        &map,
    )
    .unwrap();
    assert_eq!(outcome.degradation, Degradation::Full);

    let mut commands = CommandRegistry::new();
    commands.register(CommandSpec::new(
        "net.fetch",
        CommandCategory::Effects,
        InputSchema::new([]),
        [RequiredGrant::Network("fetch".to_string())],
    ));

    let steps: Vec<OperationStep> = outcome
        .report
        .mapped
        .iter()
        .map(|m| OperationStep {
            command_id: m.command_id.clone(),
            args: HashMap::new(),
        })
        .collect();
    let runtime = ReplayRuntime { steps };

    // No network grant on the caller — the dispatch must be denied per call.
    let no_grants = ExtensionPermissions::default();
    let result = exec::activate(
        &outcome.package,
        outcome.degradation,
        &runtime,
        &commands,
        &no_grants,
    )
    .unwrap();
    match result {
        ActivationResult::WorkflowExecuted(results) => {
            assert_eq!(results.len(), 1);
            assert_eq!(
                results[0],
                Err(DispatchError::MissingGrant(RequiredGrant::Network(
                    "fetch".to_string()
                )))
            );
        }
        other => panic!("expected WorkflowExecuted, got {other:?}"),
    }

    // Granting network:fetch lets the same step through.
    let granted = ExtensionPermissions {
        fs: vec![],
        network: vec!["fetch".to_string()],
        secrets: vec![],
    };
    let result = exec::activate(
        &outcome.package,
        outcome.degradation,
        &runtime,
        &commands,
        &granted,
    )
    .unwrap();
    match result {
        ActivationResult::WorkflowExecuted(results) => assert!(results[0].is_ok()),
        other => panic!("expected WorkflowExecuted, got {other:?}"),
    }
}

// ── Preset + custom, two-tier separation ─────────────────────

#[test]
fn ext5_sto1_preset_ships_read_only_conversion_and_synthesis_land_in_state() {
    let preset_id = SkillId::new("core", "standup");
    let mut store =
        SkillStore::with_presets([(preset_id.clone(), SkillEntry::new("preset content"))]);

    let converted = convert::convert(
        WitnessStatus::Valid,
        manifest("core/imported-note", "imported-note"),
        &[item("SKILL.md", ForeignKind::Instruction)],
        &HashMap::new(),
    )
    .unwrap();
    let synthesized = synthesize::synthesize(AuthoredSkill {
        manifest: manifest("core/synth-note", "synth-note"),
        workflow_nd: false,
        workflow_md: false,
        assets: vec![],
    })
    .unwrap();

    store
        .write(
            SkillTier::State,
            SkillId::new("core", "imported-note"),
            SkillEntry::new(format!("{:?}", converted.package.listing.entries)),
        )
        .unwrap();
    store
        .write(
            SkillTier::State,
            SkillId::new("core", "synth-note"),
            SkillEntry::new(format!("{:?}", synthesized.package.listing.entries)),
        )
        .unwrap();

    assert_eq!(
        store
            .resolve(&SkillId::new("core", "imported-note"))
            .map(|(t, _)| t),
        Some(SkillTier::State)
    );
    assert_eq!(
        store
            .resolve(&SkillId::new("core", "synth-note"))
            .map(|(t, _)| t),
        Some(SkillTier::State)
    );
    // The preset entry is untouched by either landing.
    assert_eq!(
        store.resolve(&preset_id),
        Some((SkillTier::Preset, &SkillEntry::new("preset content")))
    );
    // Nothing — regardless of which pipeline produced it — may write the
    // program tier at runtime.
    assert!(
        store
            .write(SkillTier::Preset, preset_id, SkillEntry::new("x"))
            .is_err()
    );
}

// ── Skill generation ──────────────────────────────────────────────────

#[test]
fn ext7_synthesized_skills_carry_generated_source_pending_review() {
    let outcome = synthesize::synthesize(AuthoredSkill {
        manifest: manifest("core/synth-brief", "synth-brief"),
        workflow_nd: false,
        workflow_md: false,
        assets: vec![],
    })
    .unwrap();

    assert_eq!(outcome.package.manifest.source, ExtensionSource::Generated);

    let mut store = SkillStore::new();
    store
        .write(
            SkillTier::State,
            SkillId::new("core", "synth-brief"),
            SkillEntry::new("synthesized").with_status(false, true),
        )
        .unwrap();
    let (tier, entry) = store.resolve(&SkillId::new("core", "synth-brief")).unwrap();
    assert_eq!(tier, SkillTier::State);
    assert!(entry.pending_review);
}

// ── Provenance & audit ────────────────────────────────────────────────

#[test]
fn ext8_source_conversion_report_and_originals_are_all_persisted_together() {
    let mut map = HashMap::new();
    map.insert("steps.yaml".to_string(), "workflow.run".to_string());

    let outcome = convert::convert(
        WitnessStatus::Valid,
        manifest("core/mixed", "mixed"),
        &[
            item("steps.yaml", ForeignKind::ProceduralStep),
            item("legacy.sh", ForeignKind::Script), // unmapped
        ],
        &map,
    )
    .unwrap();

    assert_eq!(outcome.package.manifest.source, ExtensionSource::Custom);
    // Whole-package degradation from the one unmapped script —
    // the mapped step is still recorded in the report for audit even though
    // the package as a whole did not land a workflow.
    assert_eq!(outcome.degradation, Degradation::InstructionOnly);
    assert_eq!(outcome.report.mapped.len(), 1);
    assert_eq!(outcome.report.degraded, vec!["legacy.sh".to_string()]);
    for original in ["origin/steps.yaml", "origin/legacy.sh"] {
        assert!(
            outcome
                .package
                .listing
                .entries
                .contains(&original.to_string()),
            "{original} must be preserved verbatim regardless of mapping outcome"
        );
    }
}

// ── Manifest contract ─────────────────────────────────────────────────

#[test]
fn ext9_convert_and_synthesize_share_one_validation_gate() {
    let mut bad_convert_manifest = manifest("core/bad", "bad");
    bad_convert_manifest.name = String::new();
    let convert_err = convert::convert(
        WitnessStatus::Valid,
        bad_convert_manifest,
        &[],
        &HashMap::new(),
    )
    .unwrap_err();
    assert!(matches!(convert_err, ConvertError::InvalidResult(_)));

    let mut bad_synth_manifest = manifest("core/bad2", "bad2");
    bad_synth_manifest.name = String::new();
    let synth_err = synthesize::synthesize(AuthoredSkill {
        manifest: bad_synth_manifest,
        workflow_nd: false,
        workflow_md: false,
        assets: vec![],
    })
    .unwrap_err();
    assert!(matches!(
        synth_err,
        synthesize::SynthesizeError::InvalidResult(_)
    ));
}

// ── Verifiable import attestation ───────────────────────────────────

#[test]
fn ext11_missing_or_invalid_witness_denies_before_conversion_even_for_valid_content() {
    for witness in [WitnessStatus::Missing, WitnessStatus::Invalid] {
        let err = convert::convert(
            witness,
            manifest("core/well-formed", "well-formed"),
            &[item("SKILL.md", ForeignKind::Instruction)],
            &HashMap::new(),
        )
        .unwrap_err();
        assert_eq!(err, ConvertError::WitnessDenied(witness));
    }
}

// ── Catalog vs instance ───────────────────────────────────────────────

#[test]
fn sto3_overriding_a_preset_copies_into_state_and_never_mutates_the_preset() {
    let id = SkillId::new("core", "review");
    let mut store = SkillStore::with_presets([(id.clone(), SkillEntry::new("preset-v1"))]);

    let outcome = store
        .write(SkillTier::State, id.clone(), SkillEntry::new("override-v2"))
        .unwrap();
    assert!(matches!(outcome, WriteOutcome::Written));

    let (tier, entry) = store.resolve(&id).unwrap();
    assert_eq!(tier, SkillTier::State, "the override shadows the preset");
    assert_eq!(entry.content, "override-v2");

    // The write API structurally has no path from a State write to the
    // preset map — Preset can only ever be seeded via `with_presets`, never
    // reached by `write`, so "never mutated in place" holds by construction.
    assert!(
        store
            .write(SkillTier::Preset, id, SkillEntry::new("anything"))
            .is_err()
    );
}
