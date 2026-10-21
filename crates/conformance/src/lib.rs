//! The conformance corpus: a shared fixture library plus a harness every
//! surface's own test target drives against its **real** projection,
//! proving that once every surface renders from one invocable registry, the
//! surfaces still agree in behavior — not merely in which verbs exist.
//!
//! Construction removes one class of divergence (three hand-written
//! mappings of one fact); it does not remove the class this crate exists
//! for: two projections of the identical descriptor can still disagree
//! about what a rejection means, whether a result is empty or unavailable,
//! or which argument schema a binder actually enforces. No import gate,
//! type check, or per-surface unit test can see that class, because each
//! side is internally consistent and the defect lives only in the
//! comparison.
//!
//! # Why this crate depends on nothing but the ports tier
//!
//! The desktop shell builds in a workspace detached from the engine
//! workspace specifically so WebView dependencies never reach it, and it
//! links no invocable registry or dispatcher at all — only a catalog and an
//! `invoke()` call delivered across its IPC seam. A harness that named
//! either of those domain-tier types would be a harness the desktop could
//! never call, which is exactly the corpus one surface could never join.
//! Every type this crate names is therefore a `cronus-contract` shape, and
//! every surface answers through [`SurfaceProjection`] — a trait a live
//! registry and an IPC bridge can implement identically.
//!
//! # Registration
//!
//! A surface registers by calling [`run`] from its own test target,
//! supplying an implementation of [`SurfaceProjection`] built from its real
//! command surface and any [`DeclaredExclusion`]s it has named. A surface
//! that cannot register is not yet a surface — this is a gate on shipping,
//! not a recommendation.

mod findings;
mod fixtures;
mod harness;
mod projection;
mod renderings;
mod report;

pub use findings::{
    AcceptedDebt, Finding, FindingClass, Repayment, Tombstone, accepted_debt, seed_inventory,
    tombstones,
};
pub use fixtures::{
    Corpus, FIXTURE_IDENTITY, OutcomeFixture, SECRET_VALUE, corpus, retired_pair, shadow_pair,
};
pub use harness::{check_outcomes, check_schema, check_surface_set, run};
pub use projection::SurfaceProjection;
pub use renderings::{
    InstallationFixture, InstallationRendering, InstallationState, Shown, Signal,
    check_installation_renderings, installation_fixtures,
};
pub use report::{ConformanceReport, DeclaredExclusion, RenderingProblem};
