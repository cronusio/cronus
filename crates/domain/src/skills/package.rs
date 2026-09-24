//! Canonical skill package model and manifest validation.
//!
//! A package in canonical form has no `scripts/` directory and no material
//! outside the closed set defined for packages, except under `origin/` — imported
//! originals preserved verbatim for audit, whose contents are never
//! classified or scanned (they are never executed either way). This module
//! validates a package's file listing and its `extension.json` manifest
//! against that shape; it does not walk a real directory (a filesystem seam,
//! like every other domain module here).

use crate::extensions::{self, ExtensionKind, ExtensionManifest};

/// The canonical top-level entries permitted. Anything else outside
/// `origin/` — most notably a `scripts/` directory — is unknown material.
const CANONICAL_TOP_LEVEL: &[&str] = &[
    "SKILL.md",
    "DESCRIPTION.md",
    "extension.json",
    "workflow.nd",
    "workflow.md",
    "references",
    "templates",
    "assets",
    "origin",
];

/// Present in every canonical package, imported or authored.
const REQUIRED_TOP_LEVEL: &[&str] = &["SKILL.md", "extension.json"];

/// `origin/` contents are audit-only and are never classified.
const EXEMPT_TOP_LEVEL: &str = "origin";

/// A package's file listing, relative to `<pack>/<name>/` — what a directory
/// scan would report. Each entry is a relative path using `/` separators;
/// walking a real directory into this shape is a filesystem seam.
#[derive(Debug, Clone, Default)]
pub struct PackageListing {
    pub entries: Vec<String>,
}

impl PackageListing {
    pub fn new(entries: impl IntoIterator<Item = impl Into<String>>) -> Self {
        PackageListing {
            entries: entries.into_iter().map(Into::into).collect(),
        }
    }

    fn has_top_level(&self, name: &str) -> bool {
        self.entries.iter().any(|e| top_level(e) == name)
    }
}

fn top_level(path: &str) -> &str {
    path.split('/').next().unwrap_or(path)
}

#[derive(Debug, PartialEq, Eq)]
pub enum PackageError {
    MissingRequiredEntry(&'static str),
    /// A `scripts/` directory, or any other entry outside the canonical
    /// allow-list and outside `origin/`.
    UnknownMaterial(String),
    InvalidManifest(String),
}

impl std::fmt::Display for PackageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackageError::MissingRequiredEntry(name) => {
                write!(f, "missing required entry: {name}")
            }
            PackageError::UnknownMaterial(path) => {
                write!(f, "unknown material outside canonical form: {path}")
            }
            PackageError::InvalidManifest(reason) => write!(f, "invalid manifest: {reason}"),
        }
    }
}

impl std::error::Error for PackageError {}

/// A validated canonical skill package.
#[derive(Debug, Clone)]
pub struct SkillPackage {
    pub listing: PackageListing,
    pub manifest: ExtensionManifest,
    /// Whether the package carries a `workflow.nd` procedure (the optional
    /// workflow pair; `workflow.md` is its generated human rendering).
    pub has_workflow: bool,
}

/// Validate a package's file listing and manifest against the canonical
/// shape. Rejects a `scripts/` directory or any other entry outside
/// the allow-list and outside `origin/`; requires an `extension.json`
/// manifest declaring `kind: skill`.
pub fn validate_package(
    listing: PackageListing,
    manifest: ExtensionManifest,
) -> Result<SkillPackage, PackageError> {
    for required in REQUIRED_TOP_LEVEL {
        if !listing.has_top_level(required) {
            return Err(PackageError::MissingRequiredEntry(required));
        }
    }
    for entry in &listing.entries {
        let top = top_level(entry);
        if top == EXEMPT_TOP_LEVEL {
            continue;
        }
        if !CANONICAL_TOP_LEVEL.contains(&top) {
            return Err(PackageError::UnknownMaterial(entry.clone()));
        }
    }
    if manifest.kind != ExtensionKind::Skill {
        return Err(PackageError::InvalidManifest(format!(
            "expected kind {}, got {}",
            ExtensionKind::Skill.as_str(),
            manifest.kind.as_str()
        )));
    }
    extensions::validate_manifest(&manifest)
        .map_err(|e| PackageError::InvalidManifest(e.to_string()))?;
    let has_workflow = listing.has_top_level("workflow.nd");
    Ok(SkillPackage {
        listing,
        manifest,
        has_workflow,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::{ExtensionPermissions, ExtensionSource};

    fn manifest(kind: ExtensionKind) -> ExtensionManifest {
        ExtensionManifest {
            id: "core/review".into(),
            kind,
            name: "review".into(),
            version: "1.0.0".into(),
            source: ExtensionSource::Preset,
            capabilities: vec![],
            permissions: ExtensionPermissions::default(),
        }
    }

    #[test]
    fn minimal_package_validates() {
        let listing = PackageListing::new(["SKILL.md", "extension.json"]);
        let pkg = validate_package(listing, manifest(ExtensionKind::Skill)).unwrap();
        assert!(!pkg.has_workflow);
    }

    #[test]
    fn full_package_with_workflow_pair_validates() {
        let listing = PackageListing::new([
            "SKILL.md",
            "DESCRIPTION.md",
            "extension.json",
            "workflow.nd",
            "workflow.md",
            "references/api.md",
            "templates/report.md",
            "assets/logo.png",
        ]);
        let pkg = validate_package(listing, manifest(ExtensionKind::Skill)).unwrap();
        assert!(pkg.has_workflow);
    }

    #[test]
    fn missing_skill_md_fails() {
        let listing = PackageListing::new(["extension.json"]);
        let err = validate_package(listing, manifest(ExtensionKind::Skill)).unwrap_err();
        assert_eq!(err, PackageError::MissingRequiredEntry("SKILL.md"));
    }

    #[test]
    fn missing_manifest_fails() {
        let listing = PackageListing::new(["SKILL.md"]);
        let err = validate_package(listing, manifest(ExtensionKind::Skill)).unwrap_err();
        assert_eq!(err, PackageError::MissingRequiredEntry("extension.json"));
    }

    #[test]
    fn scripts_directory_fails_canonical_validation() {
        let listing = PackageListing::new(["SKILL.md", "extension.json", "scripts/run.sh"]);
        let err = validate_package(listing, manifest(ExtensionKind::Skill)).unwrap_err();
        assert_eq!(
            err,
            PackageError::UnknownMaterial("scripts/run.sh".to_string())
        );
    }

    #[test]
    fn unknown_top_level_material_fails() {
        let listing = PackageListing::new(["SKILL.md", "extension.json", "run.py"]);
        let err = validate_package(listing, manifest(ExtensionKind::Skill)).unwrap_err();
        assert_eq!(err, PackageError::UnknownMaterial("run.py".to_string()));
    }

    #[test]
    fn origin_contents_are_never_classified() {
        // Imported originals can contain anything — a script, a binary — and
        // are still valid, because origin/ is audit-only and never executed.
        let listing = PackageListing::new([
            "SKILL.md",
            "extension.json",
            "origin/legacy/run.sh",
            "origin/legacy/tool.exe",
        ]);
        assert!(validate_package(listing, manifest(ExtensionKind::Skill)).is_ok());
    }

    #[test]
    fn non_skill_kind_manifest_fails() {
        let listing = PackageListing::new(["SKILL.md", "extension.json"]);
        let err = validate_package(listing, manifest(ExtensionKind::McpServer)).unwrap_err();
        assert!(matches!(err, PackageError::InvalidManifest(_)));
    }

    #[test]
    fn invalid_manifest_fields_fail() {
        let listing = PackageListing::new(["SKILL.md", "extension.json"]);
        let mut m = manifest(ExtensionKind::Skill);
        m.id = String::new();
        let err = validate_package(listing, m).unwrap_err();
        assert!(matches!(err, PackageError::InvalidManifest(_)));
    }
}
