//! Long-term memory subsystem — the SQLite-backed `UserDataStore` default:
//! storage, at-rest encryption, session chaining, and Bellman
//! trust propagation. `MemoryEntry` and its field types are defined in
//! `cronus-contract` (the ports tier `MemorySearch`/`UserDataStore` share
//! with domain code) and re-exported here for call-site convenience.
//!
//! `chain` and `trust` are pure computation with no I/O of their own, but
//! their only consumer is `store`'s Bellman-propagation logic, so they travel
//! with it rather than sitting stranded in the domain tier with no caller.

pub mod capture;
pub mod chain;
pub mod consolidate;
pub mod encryption;
pub mod maintenance;
pub(crate) mod predicate;
pub mod signal;
pub mod store;
pub mod trust;

pub use capture::CaptureOutcome;
pub use consolidate::{ConsolidationAction, InterestTopic};
pub use signal::SignalKind;
pub use store::MemoryStore;

pub(crate) use cronus_contract::now_secs;
pub use cronus_contract::{
    ExperienceOutcome, FieldPredicate, LifecycleState, MemoryDepth, MemoryEntry, MemoryId,
    MemoryKind, MemorySource, MemorySubject, PredicateField, PredicateValue, TemporalMode,
    VerificationState,
};

// ── Error ─────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum MemoryError {
    Database(rusqlite::Error),
    /// The database file's schema version could not be reconciled with the
    /// one this build understands (a newer file, or a failed migration).
    Schema(crate::versioning::SchemaError),
}

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryError::Database(e) => write!(f, "memory database error: {e}"),
            MemoryError::Schema(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for MemoryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            MemoryError::Database(e) => Some(e),
            MemoryError::Schema(e) => Some(e),
        }
    }
}

impl From<rusqlite::Error> for MemoryError {
    fn from(e: rusqlite::Error) -> Self {
        MemoryError::Database(e)
    }
}

impl From<crate::versioning::SchemaError> for MemoryError {
    fn from(e: crate::versioning::SchemaError) -> Self {
        MemoryError::Schema(e)
    }
}

pub type Result<T> = std::result::Result<T, MemoryError>;

// ── CodeChangeType / SuggestedAction ─────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeChangeType {
    Deleted,
    SignatureChanged,
    MajorRefactor,
    MinorEdit,
    Renamed,
    Moved,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SuggestedAction {
    Invalidate(f64),
    Review(f64),
    Update(f64),
    None,
}

impl CodeChangeType {
    pub fn suggested_action(self) -> SuggestedAction {
        match self {
            CodeChangeType::Deleted => SuggestedAction::Invalidate(1.0),
            CodeChangeType::SignatureChanged => SuggestedAction::Review(0.7),
            CodeChangeType::MajorRefactor => SuggestedAction::Review(0.5),
            CodeChangeType::MinorEdit => SuggestedAction::Update(0.2),
            CodeChangeType::Renamed => SuggestedAction::Update(0.3),
            CodeChangeType::Moved => SuggestedAction::Update(0.2),
        }
    }
}

// ── TrustUpdate ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct TrustUpdate {
    pub positive: bool,
    pub new_verification_state: Option<VerificationState>,
}

impl TrustUpdate {
    pub fn positive() -> Self {
        TrustUpdate {
            positive: true,
            new_verification_state: None,
        }
    }

    pub fn negative() -> Self {
        TrustUpdate {
            positive: false,
            new_verification_state: None,
        }
    }

    pub fn with_verification(state: VerificationState) -> Self {
        TrustUpdate {
            positive: true,
            new_verification_state: Some(state),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_id_uniqueness() {
        let a = MemoryId::new();
        let b = MemoryId::new();
        assert_ne!(a, b, "sequential IDs must be unique");
    }

    #[test]
    fn verification_state_weights_ordered() {
        assert!(VerificationState::Untested.weight() < VerificationState::Claimed.weight());
        assert!(VerificationState::Claimed.weight() < VerificationState::TestedInProject.weight());
        assert!(
            VerificationState::TestedInProject.weight()
                < VerificationState::ValidatedCrossProject.weight()
        );
        assert_eq!(VerificationState::ValidatedCrossProject.weight(), 1.0);
    }

    #[test]
    fn code_change_type_mapping() {
        assert_eq!(
            CodeChangeType::Deleted.suggested_action(),
            SuggestedAction::Invalidate(1.0)
        );
        assert_eq!(
            CodeChangeType::SignatureChanged.suggested_action(),
            SuggestedAction::Review(0.7)
        );
        assert_eq!(
            CodeChangeType::MinorEdit.suggested_action(),
            SuggestedAction::Update(0.2)
        );
    }

    #[test]
    fn memory_entry_effective_trust() {
        let mut entry =
            MemoryEntry::new(MemoryKind::Convention, MemorySource::Agent, "test", "body");
        entry.trust_score = 1.0;
        entry.verification_state = VerificationState::Untested;
        assert!((entry.effective_trust() - 0.30).abs() < f64::EPSILON);

        entry.verification_state = VerificationState::ValidatedCrossProject;
        assert!((entry.effective_trust() - 1.0).abs() < f64::EPSILON);
    }
}
