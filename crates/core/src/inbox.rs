//! Inter-actor inbox — SQLite-backed send/drain pipeline with GC and bus events.
//!
//! `BusEvent`, the `BusSender` trait, and its stub implementations moved to
//! `cronus-contract`; the SQLite-backed pipeline moved to
//! `cronus-store-local`. Re-exported here so every existing call site
//! is unaffected.

pub use cronus_contract::{BusEvent, BusSender, CaptureBusSender, NoOpBusSender};
pub use cronus_store_local::inbox::{
    DrainedMessage, GC_TTL_MS, InboxError, InboxMessage, InboxResult, MAX_DRAIN_PER_TURN,
    actor_exists, drain, gc, migrate, register_actor, send, undrained_count,
};
