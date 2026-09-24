//! Office archetypes — a domain-scoped prior on staffing, never a roster.
//!
//! An archetype changes what the manager *expects* (a candidate pool, an org
//! shape adopted only if the work grows, a bounded seed, domain norms), never
//! how it *decides*. Two structural properties are load-bearing: the closed
//! four-key schema makes "no authority" unrepresentable rather than
//! merely forbidden, and the candidate pool is read by the deviation recorder,
//! never by the hire gate.

pub mod catalog;
pub mod deviation;
pub mod schema;
pub mod selection;

pub use catalog::{
    ActiveArchetype, ArchetypeCatalog, ArchetypeStatus, BlockedArchetype, CustomArchetype,
    software_engineering,
};
pub use deviation::{ArchetypeDeviations, OfficeDeviations, ValidationStatus};
pub use schema::{
    ALLOWED_KEYS, ArchetypeDefinition, ArchetypeError, SeedEntry, Shape, validate_definition_keys,
};
pub use selection::{InferenceResult, infer};
