//! `cronus-store-local` — the on-device default for the user-data plane:
//! SQLite persistence, at-rest encryption, session chaining,
//! and Bellman trust propagation for memory; plus the SQLite-backed inbox
//! and workspace-registry infrastructure. Depends only on `cronus-contract`
//! (the ports tier), never on `cronus-domain` — the tier model has no
//! edge in that direction.

pub mod inbox;
pub mod knowledge;
pub mod memory;
pub mod versioning;
pub mod wiki;
pub mod workspace;
