//! Conversion pipeline: verify → classify → retain → transpile →
//! degrade → report. Turns a foreign skill package into a canonical one.
//! Deterministic command-surface mapping is in scope; the LLM-assisted
//! transpile assist is a seam (deferred model wiring.4 Notes).
//!
//! Atomicity is structural, not a rollback mechanism: [`convert`] is a pure
//! function with no side effects of its own. On `Err`, nothing has been
//! written anywhere, because nothing is written until a caller lands the
//! returned `Ok` value in the mutable store — a separate step this module
//! does not perform ("a package that fails... lands nothing").

use crate::extensions::{ExtensionManifest, ExtensionSource};
use crate::skills::exec::Degradation;
use crate::skills::package::{PackageError, PackageListing, SkillPackage, validate_package};
use std::collections::HashMap;

/// An imported package's signed witness, verified before conversion
/// begins. The signature mechanism itself is a separate attestation
/// concept's concern (no implementation yet); this pipeline only branches
/// on the outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WitnessStatus {
    Valid,
    Missing,
    Invalid,
}

/// The four content classes a foreign package is partitioned into. Assigned
/// by the source adapter at ingestion — the same boundary `agent_migration`
/// draws for its own `ItemKind` — so this module classifies by the tag it is
/// given; it does not sniff foreign file formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForeignKind {
    Instruction,
    ProceduralStep,
    Script,
    Asset,
}

/// One file from the foreign package. `content` is kept opaque — matching it
/// against the command surface happens by path lookup in `transpile_map`,
/// not by interpreting this payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignItem {
    pub path: String,
    pub kind: ForeignKind,
    pub content: String,
}

/// A foreign package partitioned by content class.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Classified {
    pub instructions: Vec<ForeignItem>,
    pub procedural: Vec<ForeignItem>,
    pub scripts: Vec<ForeignItem>,
    pub assets: Vec<ForeignItem>,
}

/// Partition `items` by their pre-assigned kind.
pub fn classify(items: &[ForeignItem]) -> Classified {
    let mut c = Classified::default();
    for item in items {
        match item.kind {
            ForeignKind::Instruction => c.instructions.push(item.clone()),
            ForeignKind::ProceduralStep => c.procedural.push(item.clone()),
            ForeignKind::Script => c.scripts.push(item.clone()),
            ForeignKind::Asset => c.assets.push(item.clone()),
        }
    }
    c
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConvertError {
    /// A missing or failed witness stops the pipeline before
    /// anything else is inspected (default-deny).
    WitnessDenied(WitnessStatus),
    /// The resulting canonical shape failed validation — nothing lands
    /// (atomicity: "a package that fails ... lands nothing").
    InvalidResult(PackageError),
}

/// One foreign item successfully mapped onto the command surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappedStep {
    pub source_path: String,
    pub command_id: String,
}

/// The conversion report persisted with the landed package.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConversionReport {
    pub mapped: Vec<MappedStep>,
    /// Paths degraded to instruction-only — no command-surface equivalent found.
    pub degraded: Vec<String>,
}

/// A successfully converted package: its canonical shape, whether any item
/// degraded (which demotes the *whole* package to instruction-only —
/// the last stage speaks of "the skill", not the item), and the audit report.
#[derive(Debug, Clone)]
pub struct ConversionOutcome {
    pub package: SkillPackage,
    pub degradation: Degradation,
    pub report: ConversionReport,
}

/// Run the conversion pipeline.
///
/// `manifest` is the caller-supplied manifest for the incoming package (id,
/// name, version, kind, permissions — parsed from the foreign package's own
/// metadata upstream of this module, the same boundary `agent_migration`
/// draws around its opaque item `content`); `source` is always overridden to
/// [`ExtensionSource::Custom`] here, because "arrived via import" is a fact
/// only this pipeline can assert, never something a foreign manifest declares
/// about itself.
///
/// `transpile_map` supplies the deterministic mapping from a procedural-step
/// or script item's path to a built-in command id (stage 4); a path absent
/// from the map degrades (stage 5). Every foreign item — mapped, degraded,
/// instruction, or asset — is preserved verbatim under `origin/`,
/// regardless of outcome.
pub fn convert(
    witness: WitnessStatus,
    manifest: ExtensionManifest,
    items: &[ForeignItem],
    transpile_map: &HashMap<String, String>,
) -> Result<ConversionOutcome, ConvertError> {
    if witness != WitnessStatus::Valid {
        return Err(ConvertError::WitnessDenied(witness));
    }

    let classified = classify(items);

    let mut report = ConversionReport::default();
    let mut any_degraded = false;
    for item in classified
        .procedural
        .iter()
        .chain(classified.scripts.iter())
    {
        match transpile_map.get(&item.path) {
            Some(command_id) => report.mapped.push(MappedStep {
                source_path: item.path.clone(),
                command_id: command_id.clone(),
            }),
            None => {
                report.degraded.push(item.path.clone());
                any_degraded = true;
            }
        }
    }
    let degradation = if any_degraded {
        Degradation::InstructionOnly
    } else {
        Degradation::Full
    };

    // Retain (stage 3): instructions collapse onto the single canonical
    // SKILL.md; assets copy into a support directory; every original lands
    // under origin/ regardless of how it was classified or transpiled.
    let mut entries = vec!["SKILL.md".to_string(), "extension.json".to_string()];
    for item in &classified.assets {
        entries.push(format!("assets/{}", item.path));
    }
    // A degraded package produces no canonical workflow — only origin/ copies.
    if !any_degraded && !report.mapped.is_empty() {
        entries.push("workflow.nd".to_string());
        entries.push("workflow.md".to_string());
    }
    for item in items {
        entries.push(format!("origin/{}", item.path));
    }

    let listing = PackageListing::new(entries);
    let manifest = ExtensionManifest {
        source: ExtensionSource::Custom,
        ..manifest
    };
    let package = validate_package(listing, manifest).map_err(ConvertError::InvalidResult)?;

    Ok(ConversionOutcome {
        package,
        degradation,
        report,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::{ExtensionKind, ExtensionPermissions};

    fn manifest() -> ExtensionManifest {
        ExtensionManifest {
            id: "core/imported-review".into(),
            kind: ExtensionKind::Skill,
            name: "imported-review".into(),
            version: "1.0.0".into(),
            source: ExtensionSource::Preset, // deliberately wrong; convert() must override it
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

    #[test]
    fn missing_witness_denies_before_anything_else() {
        let err = convert(
            WitnessStatus::Missing,
            manifest(),
            &[item("run.sh", ForeignKind::Script)],
            &HashMap::new(),
        )
        .unwrap_err();
        assert_eq!(err, ConvertError::WitnessDenied(WitnessStatus::Missing));
    }

    #[test]
    fn invalid_witness_denies_before_anything_else() {
        let err = convert(
            WitnessStatus::Invalid,
            manifest(),
            &[item("run.sh", ForeignKind::Script)],
            &HashMap::new(),
        )
        .unwrap_err();
        assert_eq!(err, ConvertError::WitnessDenied(WitnessStatus::Invalid));
    }

    #[test]
    fn classification_partitions_all_four_kinds() {
        let items = vec![
            item("SKILL.md", ForeignKind::Instruction),
            item("steps.yaml", ForeignKind::ProceduralStep),
            item("run.sh", ForeignKind::Script),
            item("logo.png", ForeignKind::Asset),
        ];
        let c = classify(&items);
        assert_eq!(c.instructions.len(), 1);
        assert_eq!(c.procedural.len(), 1);
        assert_eq!(c.scripts.len(), 1);
        assert_eq!(c.assets.len(), 1);
    }

    #[test]
    fn unmapped_script_degrades_with_original_preserved_under_origin() {
        let outcome = convert(
            WitnessStatus::Valid,
            manifest(),
            &[item("run.sh", ForeignKind::Script)],
            &HashMap::new(), // no mapping for run.sh
        )
        .unwrap();

        assert_eq!(outcome.degradation, Degradation::InstructionOnly);
        assert_eq!(outcome.report.degraded, vec!["run.sh".to_string()]);
        assert!(outcome.report.mapped.is_empty());
        assert!(
            !outcome.package.has_workflow,
            "degraded package has no workflow.nd"
        );
        assert!(
            outcome
                .package
                .listing
                .entries
                .contains(&"origin/run.sh".to_string()),
            "the original is preserved verbatim under origin/"
        );
    }

    #[test]
    fn mapped_scripts_and_procedures_produce_a_full_workflow() {
        let mut map = HashMap::new();
        map.insert(
            "steps.yaml".to_string(),
            "workflow.run_pipeline".to_string(),
        );
        map.insert("run.sh".to_string(), "fs.read_file".to_string());

        let outcome = convert(
            WitnessStatus::Valid,
            manifest(),
            &[
                item("steps.yaml", ForeignKind::ProceduralStep),
                item("run.sh", ForeignKind::Script),
            ],
            &map,
        )
        .unwrap();

        assert_eq!(outcome.degradation, Degradation::Full);
        assert_eq!(outcome.report.mapped.len(), 2);
        assert!(outcome.report.degraded.is_empty());
        assert!(outcome.package.has_workflow);
    }

    #[test]
    fn source_is_always_overridden_to_custom() {
        let outcome = convert(WitnessStatus::Valid, manifest(), &[], &HashMap::new()).unwrap();
        assert_eq!(outcome.package.manifest.source, ExtensionSource::Custom);
    }

    #[test]
    fn a_failing_manifest_lands_nothing_even_after_a_clean_pipeline_run() {
        // classify/retain/transpile all "succeed" structurally (no items,
        // nothing to map or degrade) — only the manifest is bad. The whole
        // conversion must still fail, proving there is no partial landing.
        let mut bad = manifest();
        bad.name = String::new();
        let err = convert(WitnessStatus::Valid, bad, &[], &HashMap::new()).unwrap_err();
        assert!(matches!(err, ConvertError::InvalidResult(_)));
    }
}
