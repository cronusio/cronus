//! Two-tier skill stores and shadowing precedence.
//!
//! The preset store (`<program>/skills/`) ships with the product and is
//! read-only at runtime; the state store (`<state>/skills/`) holds
//! user-added and office-generated packages; a workspace store
//! (`<ws>/skills/`) scopes overrides to one workspace. A reference resolves
//! by shadowing precedence — workspace, then state, then preset, first match
//! wins — and an override whose content is identical to the preset it
//! shadows is a warning, never an error.
//!
//! In-memory here; the on-disk `<pack>/<name>/` layout is the canonical
//! package model (a separate task's contract). This module owns only
//! the tier index and the resolution/write algebra.

use std::collections::HashMap;

/// Identity of a skill package: its pack and name, per `<pack>/<name>/`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SkillId {
    pub pack: String,
    pub name: String,
}

impl SkillId {
    pub fn new(pack: impl Into<String>, name: impl Into<String>) -> Self {
        SkillId {
            pack: pack.into(),
            name: name.into(),
        }
    }
}

/// A stored package's content, in whatever form makes two packages comparable
/// for the override-identity check. The full canonical package shape
/// (SKILL.md/extension.json/workflow.nd) is defined elsewhere; this module only
/// needs equality over it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillEntry {
    pub content: String,
    /// Set by the conversion pipeline when the package landed
    /// instruction-only. Not part of the tier-resolution algebra — carried
    /// so `skill status` can report it without a second lookup.
    pub degraded: bool,
    /// Set for packages awaiting the standard review gate — every
    /// converted or synthesized package, until reviewed.
    pub pending_review: bool,
}

impl SkillEntry {
    pub fn new(content: impl Into<String>) -> Self {
        SkillEntry {
            content: content.into(),
            degraded: false,
            pending_review: false,
        }
    }

    /// Attach conversion/review metadata (status fields) to an entry.
    pub fn with_status(mut self, degraded: bool, pending_review: bool) -> Self {
        self.degraded = degraded;
        self.pending_review = pending_review;
        self
    }
}

/// The three tiers a skill package can live in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillTier {
    /// Shipped, immutable program tier. Read-only at runtime.
    Preset,
    /// User-added and office-generated. Mutable.
    State,
    /// Scoped to one workspace. Highest shadowing precedence.
    Workspace,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SkillStoreError {
    /// The program tier is read-only at runtime; nothing in this spec
    /// ever writes under `<program>/`. The preset tier is seeded once via
    /// [`SkillStore::with_presets`], never through `write`.
    PresetIsReadOnly,
}

impl std::fmt::Display for SkillStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkillStoreError::PresetIsReadOnly => {
                write!(f, "the preset tier is read-only at runtime")
            }
        }
    }
}

impl std::error::Error for SkillStoreError {}

/// The outcome of a successful [`SkillStore::write`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOutcome {
    Written,
    /// The written content is identical to the preset entry it shadows — a
    /// warning surfaced to the caller, never an error.
    IdenticalToPreset,
}

/// The two-tier skill store plus workspace overrides, and the shadowing
/// precedence that resolves a [`SkillId`] across them.
#[derive(Debug, Default)]
pub struct SkillStore {
    preset: HashMap<SkillId, SkillEntry>,
    state: HashMap<SkillId, SkillEntry>,
    workspace: HashMap<SkillId, SkillEntry>,
}

impl SkillStore {
    /// An empty store (no presets shipped).
    pub fn new() -> Self {
        SkillStore::default()
    }

    /// Seed the preset tier. Represents the build-time act of shipping
    /// packages into `<program>/skills/`; distinct from [`SkillStore::write`],
    /// which never accepts `SkillTier::Preset`.
    pub fn with_presets(presets: impl IntoIterator<Item = (SkillId, SkillEntry)>) -> Self {
        SkillStore {
            preset: presets.into_iter().collect(),
            state: HashMap::new(),
            workspace: HashMap::new(),
        }
    }

    /// Add or override a package in `tier`. Rejects `SkillTier::Preset` — the
    /// program tier is read-only at runtime. When the written content
    /// is identical to the preset entry it would shadow, returns
    /// `IdenticalToPreset` instead of failing (a warning, not an error).
    pub fn write(
        &mut self,
        tier: SkillTier,
        id: SkillId,
        entry: SkillEntry,
    ) -> Result<WriteOutcome, SkillStoreError> {
        let target = match tier {
            SkillTier::Preset => return Err(SkillStoreError::PresetIsReadOnly),
            SkillTier::State => &mut self.state,
            SkillTier::Workspace => &mut self.workspace,
        };
        let outcome = match self.preset.get(&id) {
            Some(preset_entry) if preset_entry == &entry => WriteOutcome::IdenticalToPreset,
            _ => WriteOutcome::Written,
        };
        target.insert(id, entry);
        Ok(outcome)
    }

    /// Resolve `id` by shadowing precedence — workspace, then state, then
    /// preset; first match wins — returning the tier it resolved from
    /// alongside the entry (the provenance `skill status` reports).
    pub fn resolve(&self, id: &SkillId) -> Option<(SkillTier, &SkillEntry)> {
        self.workspace
            .get(id)
            .map(|e| (SkillTier::Workspace, e))
            .or_else(|| self.state.get(id).map(|e| (SkillTier::State, e)))
            .or_else(|| self.preset.get(id).map(|e| (SkillTier::Preset, e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(name: &str) -> SkillId {
        SkillId::new("core", name)
    }

    #[test]
    fn resolves_preset_when_no_override_exists() {
        let store = SkillStore::with_presets([(id("review"), SkillEntry::new("preset-v1"))]);
        let (tier, entry) = store.resolve(&id("review")).expect("preset should resolve");
        assert_eq!(tier, SkillTier::Preset);
        assert_eq!(entry.content, "preset-v1");
    }

    #[test]
    fn state_shadows_preset() {
        let mut store = SkillStore::with_presets([(id("review"), SkillEntry::new("preset-v1"))]);
        store
            .write(SkillTier::State, id("review"), SkillEntry::new("state-v2"))
            .unwrap();
        let (tier, entry) = store.resolve(&id("review")).unwrap();
        assert_eq!(tier, SkillTier::State);
        assert_eq!(entry.content, "state-v2");
    }

    #[test]
    fn workspace_shadows_state_and_preset() {
        let mut store = SkillStore::with_presets([(id("review"), SkillEntry::new("preset-v1"))]);
        store
            .write(SkillTier::State, id("review"), SkillEntry::new("state-v2"))
            .unwrap();
        store
            .write(
                SkillTier::Workspace,
                id("review"),
                SkillEntry::new("workspace-v3"),
            )
            .unwrap();
        let (tier, entry) = store.resolve(&id("review")).unwrap();
        assert_eq!(tier, SkillTier::Workspace);
        assert_eq!(entry.content, "workspace-v3");
    }

    #[test]
    fn preset_tier_write_is_rejected() {
        let mut store = SkillStore::new();
        let result = store.write(SkillTier::Preset, id("review"), SkillEntry::new("x"));
        assert_eq!(result, Err(SkillStoreError::PresetIsReadOnly));
    }

    #[test]
    fn override_identical_to_preset_warns_not_errors() {
        let mut store = SkillStore::with_presets([(id("review"), SkillEntry::new("preset-v1"))]);
        let outcome = store
            .write(SkillTier::State, id("review"), SkillEntry::new("preset-v1"))
            .expect("identical override must not error");
        assert_eq!(outcome, WriteOutcome::IdenticalToPreset);
    }

    #[test]
    fn override_with_different_content_is_a_plain_write() {
        let mut store = SkillStore::with_presets([(id("review"), SkillEntry::new("preset-v1"))]);
        let outcome = store
            .write(SkillTier::State, id("review"), SkillEntry::new("changed"))
            .unwrap();
        assert_eq!(outcome, WriteOutcome::Written);
    }

    #[test]
    fn unresolved_id_returns_none() {
        let store = SkillStore::new();
        assert!(store.resolve(&id("missing")).is_none());
    }
}
