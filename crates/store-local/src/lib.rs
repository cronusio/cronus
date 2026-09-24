//! `cronus-store-local` — the on-device default for the DN-2 user-data plane
//! (§4.2, §4.5): SQLite persistence, at-rest encryption, session chaining,
//! and Bellman trust propagation for memory; plus the SQLite-backed inbox
//! and workspace-registry infrastructure. Depends only on `cronus-contract`
//! (the ports tier), never on `cronus-domain` — the tier model (§4.1) has no
//! edge in that direction.

pub mod inbox;
pub mod knowledge;
pub mod memory;
pub mod versioning;
pub mod wiki;
pub mod workspace;
