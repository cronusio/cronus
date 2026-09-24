//! Cronus core engine library — the facade and composition root.
//!
//! Domain logic lives in `cronus-domain` (no I/O); the SQLite-backed default
//! for the user-data plane lives in `cronus-store-local`; the default for
//! the auth/identity planes lives in `cronus-auth-local`. This crate wires
//! them together and re-exports every module under its historical path, so
//! `cronus::memory::MemoryEntry`, `cronus::orchestration::…`, and every other
//! existing call site are unaffected by the split. Frontends (CLI, TUI, app)
//! depend on this crate; it has no presentation dependencies of its own.

pub use cronus_domain::{
    Capabilities, Engine, acp, activation, agent_migration, agent_registry, archetype, automation,
    autonomy, backup, budget, checkpoint, config_hotreload, constitution, context_mgmt,
    context_router, deliberation, dev_office, development_workflow, doctor, egress,
    error_reporting, exec_workspace, extensions, file_store, global_orch, hooks, inner_monologue,
    invocable, kanban, knowledge_access, knowledge_ingest, knowledge_retrieval, learning,
    lookahead, loop_runner, memory_capture, memory_intelligence, mission, notes, office_control,
    orchestration, paths, quality, redact, research, resource_sharing, roles, router,
    sandbox_policy, scheduler, secrets, self_improvement, session, skills, state, store, telemetry,
    tool_receipts, tool_security, trigger_triage, version_control, voice, wiki_access, wiki_regen,
};

// The facade-wiring modules whose default implementation reaches into an
// adapter crate (`cronus-store-local` / `cronus-auth-local` /
// `cronus-model-local` / `cronus-activation-os`) stay defined here — the
// tier model has no edge from `cronus-domain` to any adapter, so these shims
// cannot live there.
pub mod activation_bootstrap;
pub mod auth;
pub mod context_compaction;
pub mod dev_office_gate;
pub mod dev_office_workspace;
pub mod engine_lock;
pub mod inbox;
pub mod invocable_bootstrap;
pub mod knowledge_bootstrap;
pub mod loop_bootstrap;
pub mod memory;
pub mod model_bridge;
pub mod receipts_bootstrap;
pub mod workspace;

pub use activation_bootstrap::{
    ActivationCapabilities, ActivationMode, ActivationRegistry, ActivationState, ModeSupport,
    default_activation_registry,
};
pub use context_compaction::TransportCompactor;
/// The per-OS activation adapter crate, re-exported so a host can construct
/// (or a frontend can name the types of) a `contract::ActivationRegistry`
/// without depending on the adapter crate directly — `activation` (the
/// domain policy engine) stays the re-export at that name.
pub use cronus_activation_os as activation_os;
/// The local model-transport adapter, re-exported so a host can construct a
/// `contract::InferenceBackend` (an `EndpointProfile`) and hand it to the
/// engine — the wired transport surface.
pub use cronus_model_local as model;
pub use loop_bootstrap::{FileExistsBackend, file_exists_spec};
pub use model_bridge::NodusModelBridge;
