//! Prompt synthesis: the office authors SKILL.md
//! and, for a procedural skill, a `workflow.nd` directly against the loaded
//! nodus schema; the result is linted then validated before landing as
//! `source: generated`, `status: discovered`.
//!
//! The authoring model call itself is a seam — this module
//! takes already-authored content as input and owns linting + landing, not
//! generation. Workflow *content* validation against the nodus schema
//!  is the runtime's own contract, exactly like the transpile stage
//! of the conversion pipeline ([`crate::skills::convert`]); this module's
//! lint only catches an authoring-shape defect a package-shape check would
//! not: an incomplete workflow pair.

use crate::extensions::{ExtensionManifest, ExtensionSource};
use crate::skills::package::{PackageError, PackageListing, SkillPackage, validate_package};

/// What the authoring model produced for one synthesis request.
#[derive(Debug, Clone)]
pub struct AuthoredSkill {
    pub manifest: ExtensionManifest,
    /// The office authored a `workflow.nd` procedure for this skill.
    pub workflow_nd: bool,
    /// The human-rendered counterpart ("generated, lossless") exists.
    pub workflow_md: bool,
    /// Support material, as canonical-relative paths landing under `assets/`.
    pub assets: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LintError {
    /// `workflow.nd` and `workflow.md` must land together or not at all
    /// (the `.md` is a *generated* rendering of the `.nd`) — freshly
    /// authored content has no excuse for carrying only half the pair.
    IncompleteWorkflowPair,
}

/// Whether a synthesized skill may activate immediately for the requesting
/// user, or must wait for the standard review gate. The spec leaves this an
/// open TBD; resolved conservatively as always-review until the spec
/// amends — there is no "auto-activate" outcome to select.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationPolicy {
    RequiresReview,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SynthesizeError {
    Lint(LintError),
    InvalidResult(PackageError),
}

#[derive(Debug, Clone)]
pub struct SynthesisOutcome {
    pub package: SkillPackage,
    pub activation_policy: ActivationPolicy,
}

/// Land authored content as a canonical package. `source` is always
/// overridden to [`ExtensionSource::Generated`] — reaching this function at
/// all means a synthesis request happened, regardless of what the caller's
/// manifest otherwise declares.
pub fn synthesize(authored: AuthoredSkill) -> Result<SynthesisOutcome, SynthesizeError> {
    if authored.workflow_nd != authored.workflow_md {
        return Err(SynthesizeError::Lint(LintError::IncompleteWorkflowPair));
    }

    let mut entries = vec!["SKILL.md".to_string(), "extension.json".to_string()];
    for asset in &authored.assets {
        entries.push(format!("assets/{asset}"));
    }
    if authored.workflow_nd {
        entries.push("workflow.nd".to_string());
        entries.push("workflow.md".to_string());
    }

    let listing = PackageListing::new(entries);
    let manifest = ExtensionManifest {
        source: ExtensionSource::Generated,
        ..authored.manifest
    };
    let package = validate_package(listing, manifest).map_err(SynthesizeError::InvalidResult)?;

    Ok(SynthesisOutcome {
        package,
        activation_policy: ActivationPolicy::RequiresReview,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::{ExtensionKind, ExtensionPermissions};

    fn manifest() -> ExtensionManifest {
        ExtensionManifest {
            id: "core/synth-standup".into(),
            kind: ExtensionKind::Skill,
            name: "synth-standup".into(),
            version: "1.0.0".into(),
            source: ExtensionSource::Preset, // deliberately wrong; synthesize() must override it
            capabilities: vec![],
            permissions: ExtensionPermissions::default(),
        }
    }

    fn authored(workflow_nd: bool, workflow_md: bool) -> AuthoredSkill {
        AuthoredSkill {
            manifest: manifest(),
            workflow_nd,
            workflow_md,
            assets: vec![],
        }
    }

    #[test]
    fn synthesized_skill_with_full_workflow_pair_lands() {
        let outcome = synthesize(authored(true, true)).unwrap();
        assert!(outcome.package.has_workflow);
        assert_eq!(outcome.package.manifest.source, ExtensionSource::Generated);
        assert_eq!(outcome.activation_policy, ActivationPolicy::RequiresReview);
    }

    #[test]
    fn instruction_only_synthesis_lands_without_workflow() {
        let outcome = synthesize(authored(false, false)).unwrap();
        assert!(!outcome.package.has_workflow);
        assert_eq!(outcome.package.manifest.source, ExtensionSource::Generated);
    }

    #[test]
    fn workflow_nd_without_its_rendering_is_a_lint_failure() {
        let err = synthesize(authored(true, false)).unwrap_err();
        assert_eq!(
            err,
            SynthesizeError::Lint(LintError::IncompleteWorkflowPair)
        );
    }

    #[test]
    fn workflow_md_without_its_source_is_a_lint_failure() {
        let err = synthesize(authored(false, true)).unwrap_err();
        assert_eq!(
            err,
            SynthesizeError::Lint(LintError::IncompleteWorkflowPair)
        );
    }

    #[test]
    fn invalid_manifest_lands_nothing_even_with_a_clean_workflow_pair() {
        let mut a = authored(true, true);
        a.manifest.name = String::new();
        let err = synthesize(a).unwrap_err();
        assert!(matches!(err, SynthesizeError::InvalidResult(_)));
    }
}
