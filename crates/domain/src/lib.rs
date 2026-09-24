//! Cronus domain — the no-I/O tier of the crate topology: pure
//! domain logic with no C toolchain, platform service, or cryptographic
//! implementation of its own. Depends only on `cronus-contract`; the
//! `cronus` facade wires this together with the adapter crates.
//!
//! `memory`'s SQLite-backed store, `inbox`'s SQLite-backed pipeline,
//! `workspace`'s SQLite-backed registry, and `auth`'s password/TOTP/session
//! machinery moved to the adapter crates (`cronus-store-local`,
//! `cronus-auth-local`) — this crate cannot depend on either (the tier
//! model has no edge from domain to an adapter), so their facade re-export
//! shims stay in `crates/core`, not here.

pub mod acp;
pub mod activation;
pub mod agent_migration;
pub mod agent_registry;
pub mod archetype;
pub mod automation;
pub mod autonomy;
pub mod backup;
pub mod budget;
pub mod checkpoint;
pub mod config_hotreload;
pub mod constitution;
pub mod context_mgmt;
pub mod context_router;
pub mod deliberation;
pub mod dev_office;
pub mod development_workflow;
pub mod doctor;
pub mod egress;
pub mod error_reporting;
pub mod exec_workspace;
pub mod extensions;
pub mod file_store;
pub mod global_orch;
pub mod hooks;
pub mod inner_monologue;
pub mod invocable;
pub mod io_message;
pub mod kanban;
pub mod knowledge_access;
pub mod knowledge_ingest;
pub mod knowledge_retrieval;
pub mod learning;
pub mod lookahead;
pub mod loop_runner;
pub mod memory_capture;
pub mod memory_intelligence;
pub mod mission;
pub mod notes;
pub mod office_control;
pub mod orchestration;
pub mod paths;
pub mod quality;
pub mod redact;
pub mod research;
pub mod resource_sharing;
pub mod roles;
pub mod router;
pub mod sandbox_policy;
pub mod scheduler;
pub mod secrets;
pub mod self_improvement;
pub mod session;
pub mod skills;
pub mod state;
pub mod store;
pub mod telemetry;
pub mod tool_receipts;
pub mod tool_security;
pub mod trigger_triage;
pub mod version_control;
pub mod voice;
pub mod wiki_access;
pub mod wiki_regen;

/// The public capability contract that frontends invoke.
///
/// Frontends hold no domain logic — they map input to these calls and render
/// the results.
pub trait Capabilities {
    /// Engine/product version string.
    fn version(&self) -> &str;

    /// A human-readable status line (placeholder until subsystems land).
    fn status(&self) -> String;
}

/// The Cronus engine. Embeddable: links into a host without pulling any frontend.
#[derive(Debug, Default)]
pub struct Engine {
    _private: (),
}

impl Engine {
    /// Construct an engine instance.
    pub fn new() -> Self {
        Engine { _private: () }
    }
}

impl Capabilities for Engine {
    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    fn status(&self) -> String {
        format!("Cronus core {} — no subsystems loaded yet", self.version())
    }
}
