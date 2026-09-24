//! What one corpus run against one surface produces: a closed set of
//! report shapes, one per assertion family. An empty `Vec<ConformanceReport>`
//! is the only passing result — this type has no "mostly conformant" state,
//! because a divergence class either was seen or was not.

use cronus_contract::{Binder, Dispatched, InvocableId};

/// A surface deliberately does not expose an otherwise-shipped invocable
///  — a scope decision stated with its reason, so an unexposed id is
/// read as "declared here, on purpose" rather than a corpus finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredExclusion {
    pub id: InvocableId,
    pub reason: &'static str,
}

impl DeclaredExclusion {
    pub fn new(id: InvocableId, reason: &'static str) -> Self {
        DeclaredExclusion { id, reason }
    }
}

/// One divergence the corpus observed, naming the assertion family that
/// caught it and everything a finding record (owned by the
/// sibling finding-inventory task) needs to describe the site. This type
/// does not itself track, tombstone, or repay anything — it is the raw
/// comparison result a caller converts into a finding, or asserts empty.
#[derive(Debug, Clone, PartialEq)]
pub enum ConformanceReport {
    /// The surface's exposed set disagrees with the canonical shipped set
    /// minus declared exclusions.
    SurfaceSet {
        /// Canonically shipped, not declared-excluded, but absent from
        /// `SurfaceProjection::exposed` — a silent gap indistinguishable
        /// from a missing feature.
        missing: Vec<InvocableId>,
        /// Present in `SurfaceProjection::exposed` but not part of the
        /// canonical shipped set at all — a surface exposing something the
        /// registry never shipped.
        unexpected: Vec<InvocableId>,
    },
    /// This surface's advertised argument schema for `id` does not match
    /// its declared binders — the two are supposed to be the same
    /// fact rendered twice, and this is exactly the divergence that
    /// re-derivation produces.
    Schema {
        id: InvocableId,
        declared: Vec<Binder>,
        advertised: Option<Vec<Binder>>,
    },
    /// Driving `fixture` through this surface's real dispatch path produced
    /// a different `Dispatched` than the corpus's declared expectation —
    /// including a rejection mode/location mismatch and the
    /// empty-versus-unavailable distinction, since both are folded into the
    /// full value this variant carries.
    Outcome {
        fixture: &'static str,
        expected: Dispatched,
        actual: Dispatched,
    },
}
