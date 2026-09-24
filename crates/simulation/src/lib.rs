//! `cronus-simulation` — the usage-simulation harness
//! in this project's stack: a **disposable world** every simulated run
//! happens inside, an **as-it-happens transcript** of what the product
//! actually did, a **replay** lane that re-runs a pinned route with no
//! agent, and a **scenario** format whose parser refuses an obligation that
//! cannot fail.
//!
//! Kept on the ports tier (`cronus-contract` only, nothing from the domain
//! tier) for the same reason `cronus-conformance` is: a surface that cannot
//! yet link the domain tier must still be able to register later.

pub mod coverage;
pub mod findings;
pub mod product;
pub mod replay;
pub mod run_state;
pub mod scenario;
pub mod transcript;
pub mod world;

pub use transcript::{Entry, Transcript};
pub use world::{World, WorldError};
