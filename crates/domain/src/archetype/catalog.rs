//! The archetype catalog: the shipped program-tier archetypes, the
//! declared-blocked ones, and the preset/custom split.
//!
//! Every examined domain seeds **zero** roles — the workspace manager already
//! performs all first-contact work — so the shipped `software-engineering`
//! archetype has an empty seed. The two non-technical archetypes are
//! *declared and blocked*, not shipped against invented specialties:
//! the role catalog lacks the specialties they name, and each missing role
//! must clear the gate in a separate role-catalog amendment first.

use std::path::Path;

use super::schema::{ArchetypeDefinition, ArchetypeError, Shape};

/// Whether an archetype ships or is declared-blocked pending role-catalog work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchetypeStatus {
    /// Ships: every `pool`/`seed` identifier resolves against the role catalog.
    Ships,
    /// Declared but blocked: the named roles are absent from the role
    /// catalog and must each clear the role-admission gate in a separate amendment first.
    Blocked { missing_roles: Vec<String> },
}

/// A declared-but-blocked archetype: named, with the exact
/// roles it still requires, rather than shipped against invented specialties.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedArchetype {
    pub id: String,
    pub domain: String,
    pub missing_roles: Vec<String>,
}

/// A custom archetype derived from a preset: a state-tier copy that
/// records where it came from and never mutates the read-only source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomArchetype {
    pub definition: ArchetypeDefinition,
    pub derived_from: String,
}

fn owned(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// The shipped `software-engineering` archetype: an 18-role pool that resolves
/// fully against the current role catalog, an **empty seed** (the workspace manager
/// performs all first-contact work), and a three-department shape (named,
/// never instantiated).
pub fn software_engineering() -> ArchetypeDefinition {
    ArchetypeDefinition {
        id: "software-engineering".to_string(),
        domain: "Building and maintaining software".to_string(),
        pool: owned(&[
            "architect",
            "backend-engineer",
            "frontend-engineer",
            "api-designer",
            "sql-expert",
            "code-reviewer",
            "test-writer",
            "debugger",
            "refactorer",
            "performance-optimizer",
            "security-auditor",
            "accessibility-auditor",
            "devops-engineer",
            "incident-responder",
            "doc-writer",
            "data-analyst",
            "prompt-engineer",
            "archivist",
        ]),
        shape: Shape {
            departments: owned(&["engineering", "quality", "operations"]),
            grow_when: "sustained parallel width in a department exceeds the manager's span"
                .to_string(),
        },
        seed: Vec::new(),
        norms_ref: "NORMS.md".to_string(),
    }
}

/// The two declared-blocked archetypes and the roles each still needs.
fn blocked_archetypes() -> Vec<BlockedArchetype> {
    vec![
        BlockedArchetype {
            id: "advertising-agency".to_string(),
            domain: "Campaigns, creative, media".to_string(),
            missing_roles: owned(&[
                "account-manager",
                "strategist",
                "copywriter",
                "art-director",
                "media-planner",
            ]),
        },
        BlockedArchetype {
            id: "finance-department".to_string(),
            domain: "Accounting, controlling, analysis".to_string(),
            missing_roles: owned(&[
                "accountant",
                "controller",
                "financial-analyst",
                "tax-specialist",
            ]),
        },
    ]
}

/// The program-tier archetype catalog: the shipped definitions + the
/// declared-blocked entries. Read-only; custom archetypes live in the state
/// tier and are not part of this constant.
#[derive(Debug, Clone)]
pub struct ArchetypeCatalog {
    shipped: Vec<ArchetypeDefinition>,
    blocked: Vec<BlockedArchetype>,
}

impl Default for ArchetypeCatalog {
    fn default() -> Self {
        ArchetypeCatalog::program()
    }
}

impl ArchetypeCatalog {
    /// The shipped program-tier catalog.
    pub fn program() -> Self {
        ArchetypeCatalog {
            shipped: vec![software_engineering()],
            blocked: blocked_archetypes(),
        }
    }

    /// The archetypes that ship (their pools resolve fully today).
    pub fn shipped(&self) -> &[ArchetypeDefinition] {
        &self.shipped
    }

    /// The declared-blocked archetypes and the roles each still needs.
    pub fn blocked(&self) -> &[BlockedArchetype] {
        &self.blocked
    }

    /// Look up a shipped archetype by id.
    pub fn get(&self, id: &str) -> Option<&ArchetypeDefinition> {
        self.shipped.iter().find(|d| d.id == id)
    }

    /// Whether an id names a declared-blocked archetype.
    pub fn blocked_status(&self, id: &str) -> Option<&BlockedArchetype> {
        self.blocked.iter().find(|b| b.id == id)
    }

    /// Copy a shipped preset into the state tier as a custom archetype,
    /// recording `derived_from` and **never mutating the source** (the source
    /// is an embedded program-tier constant, so it is immutable by
    /// construction). Writes a marker file under
    /// `<state_dir>/archetypes/<name>/` so the copy persists across runs.
    pub fn create_from_preset(
        &self,
        state_dir: &Path,
        name: &str,
        preset_id: &str,
    ) -> Result<CustomArchetype, ArchetypeError> {
        let preset = self
            .get(preset_id)
            .ok_or_else(|| ArchetypeError::UnknownPreset(preset_id.to_string()))?;
        let mut definition = preset.clone();
        definition.id = name.to_string();

        let dir = state_dir.join("archetypes").join(name);
        std::fs::create_dir_all(&dir).map_err(|e| ArchetypeError::Io(e.to_string()))?;
        std::fs::write(dir.join("derived_from"), preset_id)
            .map_err(|e| ArchetypeError::Io(e.to_string()))?;

        Ok(CustomArchetype {
            definition,
            derived_from: preset_id.to_string(),
        })
    }
}

/// An office's archetype selection. The archetype-free state —
/// `active = None` — is a valid, fully-functional default, the fallback when
/// inference is inconclusive.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActiveArchetype {
    pub active: Option<String>,
}

impl ActiveArchetype {
    /// The archetype-free office is complete.
    pub fn is_archetype_free(&self) -> bool {
        self.active.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- the shipped software-engineering archetype -------------------------

    #[test]
    fn the_shipped_software_engineering_archetype_validates_with_an_empty_seed() {
        let def = software_engineering();
        assert_eq!(def.validate(), Ok(()));
        assert!(def.seed.is_empty());
        assert_eq!(def.pool.len(), 18);
    }

    #[test]
    fn the_catalog_lists_software_engineering_as_shipped() {
        let catalog = ArchetypeCatalog::program();
        assert_eq!(catalog.shipped().len(), 1);
        assert!(catalog.get("software-engineering").is_some());
    }

    // --- the two declared-blocked archetypes -------------------------

    #[test]
    fn the_two_blocked_archetypes_are_present_with_their_missing_role_lists() {
        let catalog = ArchetypeCatalog::program();
        let adv = catalog.blocked_status("advertising-agency").unwrap();
        assert_eq!(adv.missing_roles.len(), 5);
        assert!(adv.missing_roles.contains(&"copywriter".to_string()));
        let fin = catalog.blocked_status("finance-department").unwrap();
        assert_eq!(fin.missing_roles.len(), 4);
        assert!(fin.missing_roles.contains(&"controller".to_string()));
        // A blocked archetype is not shipped.
        assert!(catalog.get("advertising-agency").is_none());
    }

    // --- The archetype-free office is complete -----------------------

    #[test]
    fn the_default_office_is_archetype_free_and_that_is_a_complete_state() {
        let state = ActiveArchetype::default();
        assert!(state.is_archetype_free());
        assert_eq!(state.active, None);
    }

    // --- Create-from-preset records derived_from, source untouched ----

    #[test]
    fn create_from_preset_writes_a_state_tier_copy_recording_derived_from() {
        let dir = std::env::temp_dir().join(format!("cronus-archetype-b01-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let catalog = ArchetypeCatalog::program();
        let custom = catalog
            .create_from_preset(&dir, "my-eng", "software-engineering")
            .expect("create_from_preset");

        assert_eq!(custom.derived_from, "software-engineering");
        assert_eq!(custom.definition.id, "my-eng");
        // The copy carries the preset's pool, and the copy still validates.
        assert_eq!(custom.definition.pool.len(), 18);
        assert_eq!(custom.definition.validate(), Ok(()));
        // The program-tier source is unchanged (an embedded constant).
        assert_eq!(
            catalog.get("software-engineering").unwrap().id,
            "software-engineering"
        );
        // The state-tier marker persisted.
        let marker = dir.join("archetypes").join("my-eng").join("derived_from");
        assert_eq!(
            std::fs::read_to_string(marker).unwrap(),
            "software-engineering"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn create_from_an_unknown_preset_is_rejected() {
        let dir =
            std::env::temp_dir().join(format!("cronus-archetype-b01b-{}", std::process::id()));
        let catalog = ArchetypeCatalog::program();
        let err = catalog
            .create_from_preset(&dir, "x", "no-such-preset")
            .unwrap_err();
        // Naming the *preset* catalog, not the role catalog — a
        // caller reading the message must not have to guess which
        // "no-such-preset" failed to resolve.
        assert!(
            matches!(&err, ArchetypeError::UnknownPreset(id) if id == "no-such-preset"),
            "expected UnknownPreset(\"no-such-preset\"), got {err:?}"
        );
        assert_eq!(
            err.to_string(),
            "archetype preset not found: no-such-preset"
        );
    }
}
