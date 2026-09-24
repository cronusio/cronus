//! Archetype inference and application.
//!
//! Inference runs at office instantiation and on demand thereafter, and is
//! **never a blocking prompt on the ordinary path**: an inconclusive result
//! leaves the office archetype-free (a complete state); a confident
//! result is applied silently. The office is asked only when two or more
//! archetypes score within an ambiguity band *and* the choice changes the
//! pool materially — and then in kinds-of-work terms, never roles.

use super::catalog::{ActiveArchetype, ArchetypeCatalog};

/// The domain keywords each shipped archetype is inferred from. Only
/// `software-engineering` ships today; the map degrades to "recognize
/// software work, else stay archetype-free".
fn domain_keywords(id: &str) -> &'static [&'static str] {
    match id {
        "software-engineering" => &[
            "software",
            "code",
            "build",
            "app",
            "api",
            "backend",
            "frontend",
            "engineering",
            "deploy",
            "bug",
            "feature",
            "test",
            "refactor",
            "database",
        ],
        _ => &[],
    }
}

/// The number of an archetype's domain keywords that appear in the intent
/// (case-insensitive substring match).
fn score(id: &str, intent_lower: &str) -> usize {
    domain_keywords(id)
        .iter()
        .filter(|kw| intent_lower.contains(*kw))
        .count()
}

/// The result of inferring an archetype from captured intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InferenceResult {
    /// One archetype clears the confidence bar — applied silently.
    Confident(String),
    /// No archetype is recognized — the office stays archetype-free.
    Inconclusive,
    /// Two or more score within the ambiguity band — ask the office, in
    /// kinds-of-work terms. Carries the tied ids.
    Ambiguous(Vec<String>),
}

/// The margin within which two archetypes are "tied" for ambiguity.
const AMBIGUITY_BAND: usize = 1;

/// Infer the archetype from captured intent. Scores every shipped
/// archetype, returns the confident leader, `Inconclusive` when nothing is
/// recognized, or `Ambiguous` when the top two are within the band (the only
/// case that may ask the office).
pub fn infer(catalog: &ArchetypeCatalog, captured_intent: &str) -> InferenceResult {
    let intent_lower = captured_intent.to_lowercase();
    let mut scored: Vec<(String, usize)> = catalog
        .shipped()
        .iter()
        .map(|def| (def.id.clone(), score(&def.id, &intent_lower)))
        .filter(|(_, s)| *s > 0)
        .collect();
    scored.sort_by_key(|(_, s)| std::cmp::Reverse(*s));

    match scored.as_slice() {
        [] => InferenceResult::Inconclusive,
        [(id, _)] => InferenceResult::Confident(id.clone()),
        [(top_id, top), (second_id, second), ..] => {
            if top - second <= AMBIGUITY_BAND {
                InferenceResult::Ambiguous(vec![top_id.clone(), second_id.clone()])
            } else {
                InferenceResult::Confident(top_id.clone())
            }
        }
    }
}

impl ActiveArchetype {
    /// Record exactly one active archetype, re-scoping future decisions.
    /// Touches no staff — this method has access only to the archetype record,
    /// so it *structurally cannot* release a role, discard memory, or
    /// invalidate work.
    pub fn set(&mut self, id: &str) {
        self.active = Some(id.to_string());
    }

    /// Return the office to the archetype-free state. Like `set`,
    /// it touches only the archetype record.
    pub fn clear(&mut self) {
        self.active = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Silent inference, inconclusive → archetype-free --------------

    #[test]
    fn a_software_shaped_intent_infers_software_engineering() {
        let catalog = ArchetypeCatalog::program();
        let result = infer(
            &catalog,
            "We need to build a backend API and deploy the app",
        );
        assert_eq!(
            result,
            InferenceResult::Confident("software-engineering".to_string())
        );
    }

    #[test]
    fn an_intent_with_no_recognized_domain_is_inconclusive() {
        let catalog = ArchetypeCatalog::program();
        let result = infer(&catalog, "Plan a birthday party for the office");
        assert_eq!(result, InferenceResult::Inconclusive);
    }

    #[test]
    fn an_inconclusive_inference_leaves_the_office_archetype_free_and_complete() {
        let catalog = ArchetypeCatalog::program();
        let mut office = ActiveArchetype::default();
        if let InferenceResult::Confident(id) = infer(&catalog, "something unrelated") {
            office.set(&id);
        }
        assert!(office.is_archetype_free()); // Still fully functional
    }

    // --- Apply / set / clear touch no staff ---------------------------

    #[test]
    fn set_then_clear_round_trips_the_active_field_without_touching_staff() {
        // A fixture office pairing the archetype record with a roster. `set`
        // and `clear` borrow only the archetype record, so the staff list is
        // structurally out of their reach.
        struct FixtureOffice {
            archetype: ActiveArchetype,
            staff: Vec<String>,
        }
        let mut office = FixtureOffice {
            archetype: ActiveArchetype::default(),
            staff: vec!["architect".to_string(), "backend-engineer".to_string()],
        };
        let staff_before = office.staff.clone();

        office.archetype.set("software-engineering");
        assert_eq!(
            office.archetype.active.as_deref(),
            Some("software-engineering")
        );
        assert_eq!(office.staff, staff_before, "set must not touch staff");

        office.archetype.clear();
        assert!(office.archetype.is_archetype_free());
        assert_eq!(office.staff, staff_before, "clear must not touch staff");
    }
}
