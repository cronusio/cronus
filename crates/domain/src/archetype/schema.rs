//! The archetype definition and its closed-schema validator (OA-2, OA-4, OA-10).
//!
//! The schema is *closed*: the only recognized keys are the archetype's
//! identity (`id`, `domain`) and its four content fields (`pool`, `shape`,
//! `seed`, `norms`). An unrecognized key — `permissions`, `budget`,
//! `autonomy`, anything — is a validation failure, not an ignored extra. That
//! is how OA-4 (an archetype carries no authority) is made *unrepresentable*
//! rather than merely told-not-to: there is no key in which authority could be
//! smuggled.

use crate::roles::catalog::PRESET_CATALOG;

/// The complete set of keys a well-formed archetype definition may carry:
/// identity (`id`, `domain`) + the four content fields (OA-1). Any other key
/// fails validation (OA-4).
pub const ALLOWED_KEYS: &[&str] = &["id", "domain", "pool", "shape", "seed", "norms"];

/// Cap on `seed` entries (OA-2): a seed seats specialists before the first
/// sentence is spoken, so it is bounded and each entry must justify itself.
pub const SEED_CAP: usize = 2;

/// The org shape an archetype expects — named layers and the condition under
/// which the manager introduces one. `grow_when` is a condition, not a
/// structure to build: nothing here instantiates a department (OA-1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shape {
    pub departments: Vec<String>,
    pub grow_when: String,
}

/// One seeded role — a specialist seated at office instantiation — with the
/// first-contact work it performs (OA-2: the justification is mandatory).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeedEntry {
    pub role: String,
    pub justification: String,
}

/// A complete archetype definition. Constructed either from an embedded
/// program-tier constant (the shipped archetypes) or by a loader that has
/// already passed the raw key set through `validate_definition_keys` (OA-4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchetypeDefinition {
    pub id: String,
    pub domain: String,
    pub pool: Vec<String>,
    pub shape: Shape,
    pub seed: Vec<SeedEntry>,
    pub norms_ref: String,
}

/// Why a definition was rejected. Every variant names the offending element so
/// a caller can report precisely what to fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchetypeError {
    /// A key outside the closed schema was present (OA-4).
    UnknownKey(String),
    /// A `pool` or `seed` role identifier does not resolve against the role
    /// catalog (OA-10) — the reason the two non-technical archetypes are
    /// blocked rather than shipped.
    UnknownRole(String),
    /// An archetype *preset* id (`archetype create --from <id>`) does not
    /// resolve against the shipped catalog — distinct from
    /// [`ArchetypeError::UnknownRole`]: naming a role catalog for a
    /// failure that was actually about the archetype-preset catalog
    /// mis-described which lookup failed.
    UnknownPreset(String),
    /// More than `SEED_CAP` seed entries (OA-2).
    SeedTooLarge(usize),
    /// A seed entry carries an empty justification (OA-2).
    MissingJustification(String),
    /// A state-tier read/write failed (e.g. persisting a custom archetype).
    Io(String),
}

impl std::fmt::Display for ArchetypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArchetypeError::UnknownKey(k) => {
                write!(f, "unrecognized archetype key '{k}': the schema is closed")
            }
            ArchetypeError::UnknownRole(id) => {
                write!(f, "role '{id}' does not resolve against the role catalog")
            }
            ArchetypeError::UnknownPreset(id) => {
                write!(f, "archetype preset not found: {id}")
            }
            ArchetypeError::SeedTooLarge(n) => {
                write!(f, "seed has {n} entries; the cap is {SEED_CAP}")
            }
            ArchetypeError::MissingJustification(role) => {
                write!(f, "seed entry for '{role}' has no justification")
            }
            ArchetypeError::Io(e) => write!(f, "archetype state I/O error: {e}"),
        }
    }
}

impl std::error::Error for ArchetypeError {}

/// OA-4: the closed-schema gate applied at the parse boundary. A loader that
/// reads a raw archetype definition (e.g. a state-tier `archetype.json`) passes
/// its key set here *before* building an `ArchetypeDefinition`; any key outside
/// `ALLOWED_KEYS` is rejected. Reads no values — it cannot be subverted by
/// persuasive content, only by structure, and structure is all it checks.
pub fn validate_definition_keys(keys: &[&str]) -> Result<(), ArchetypeError> {
    for key in keys {
        if !ALLOWED_KEYS.contains(key) {
            return Err(ArchetypeError::UnknownKey((*key).to_string()));
        }
    }
    Ok(())
}

/// Whether a role identifier resolves against the preset role catalog.
fn role_exists(id: &str) -> bool {
    PRESET_CATALOG.iter().any(|r| r.id == id)
}

impl ArchetypeDefinition {
    /// Validate a built definition's content (OA-2, OA-10). The key-set check
    /// (OA-4) is `validate_definition_keys`, run earlier at the parse boundary;
    /// a definition built in code from the closed struct cannot express an
    /// unknown key, so this method covers the value-level invariants.
    pub fn validate(&self) -> Result<(), ArchetypeError> {
        // OA-10: every pool identifier resolves against the role catalog.
        for role in &self.pool {
            if !role_exists(role) {
                return Err(ArchetypeError::UnknownRole(role.clone()));
            }
        }
        // OA-2: seed is capped and every entry justifies itself; its roles
        // resolve too (a seeded role is also a hire, OA-10).
        if self.seed.len() > SEED_CAP {
            return Err(ArchetypeError::SeedTooLarge(self.seed.len()));
        }
        for entry in &self.seed {
            if entry.justification.trim().is_empty() {
                return Err(ArchetypeError::MissingJustification(entry.role.clone()));
            }
            if !role_exists(&entry.role) {
                return Err(ArchetypeError::UnknownRole(entry.role.clone()));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn se_pool() -> Vec<String> {
        [
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
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    fn valid_definition() -> ArchetypeDefinition {
        ArchetypeDefinition {
            id: "software-engineering".to_string(),
            domain: "Building and maintaining software".to_string(),
            pool: se_pool(),
            shape: Shape {
                departments: vec![
                    "engineering".to_string(),
                    "quality".to_string(),
                    "operations".to_string(),
                ],
                grow_when: "sustained parallel width exceeds the manager's span".to_string(),
            },
            seed: Vec::new(),
            norms_ref: "NORMS.md".to_string(),
        }
    }

    // --- OA-4: the closed schema rejects a fifth key ------------------------

    #[test]
    fn a_definition_carrying_an_unrecognized_key_fails_validation() {
        let keys = [
            "id",
            "domain",
            "pool",
            "shape",
            "seed",
            "norms",
            "permissions",
        ];
        assert_eq!(
            validate_definition_keys(&keys),
            Err(ArchetypeError::UnknownKey("permissions".to_string()))
        );
    }

    #[test]
    fn the_six_recognized_keys_all_pass_the_closed_schema_gate() {
        let keys = ["id", "domain", "pool", "shape", "seed", "norms"];
        assert_eq!(validate_definition_keys(&keys), Ok(()));
    }

    #[test]
    fn a_budget_or_autonomy_key_is_unrepresentable_not_ignored() {
        assert!(matches!(
            validate_definition_keys(&["pool", "budget"]),
            Err(ArchetypeError::UnknownKey(_))
        ));
        assert!(matches!(
            validate_definition_keys(&["pool", "autonomy"]),
            Err(ArchetypeError::UnknownKey(_))
        ));
    }

    // --- OA-10: every role id resolves against the catalog ------------------

    #[test]
    fn the_shipped_software_engineering_pool_resolves_fully_today() {
        let def = valid_definition();
        assert_eq!(def.validate(), Ok(()));
        assert_eq!(def.pool.len(), 18);
    }

    #[test]
    fn an_unknown_pool_role_is_rejected_with_the_offending_id_named() {
        let mut def = valid_definition();
        def.pool.push("copywriter".to_string()); // a role the catalog lacks
        assert_eq!(
            def.validate(),
            Err(ArchetypeError::UnknownRole("copywriter".to_string()))
        );
    }

    // --- OA-2: seed cap + mandatory justification ---------------------------

    #[test]
    fn a_seed_over_the_cap_of_two_fails() {
        let mut def = valid_definition();
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
    }

    #[test]
    fn a_seed_entry_with_an_empty_justification_fails() {
        let mut def = valid_definition();
        def.seed = vec![SeedEntry {
            role: "architect".to_string(),
            justification: "   ".to_string(),
        }];
        assert_eq!(
            def.validate(),
            Err(ArchetypeError::MissingJustification(
                "architect".to_string()
            ))
        );
    }

    #[test]
    fn a_seed_entry_naming_an_unknown_role_is_rejected() {
        let mut def = valid_definition();
        def.seed = vec![SeedEntry {
            role: "strategist".to_string(),
            justification: "runs campaign strategy".to_string(),
        }];
        assert_eq!(
            def.validate(),
            Err(ArchetypeError::UnknownRole("strategist".to_string()))
        );
    }

    #[test]
    fn a_within_cap_justified_seed_of_known_roles_passes() {
        let mut def = valid_definition();
        def.seed = vec![SeedEntry {
            role: "architect".to_string(),
            justification: "shapes the initial technical plan".to_string(),
        }];
        assert_eq!(def.validate(), Ok(()));
    }
}
