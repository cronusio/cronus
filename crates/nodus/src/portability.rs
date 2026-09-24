//! Portability and extension-point traits for host integration.
//!
//! Provides the vocabulary-extension seam ([`SchemaProvider`]), a built-in
//! in-memory implementation of the storage seam ([`StorageProvider`] /
//! [`InMemoryStorageProvider`] — its executor wiring stays pending),
//! and the per-effect authorization gate ([`PolicyProvider`] / [`EffectClass`]
//! / [`effect_class_of`], wired into `Executor::execute_command`). Each
//! trait ships with a built-in implementation sufficient for in-process use
//! without I/O, matching the pattern established by
//! [`crate::executor::StubProvider`] and [`crate::observability::NoopAuditProvider`].
//!
//! It also defines the capability manifest ([`CapabilityManifest`]) and the
//! pre-run satisfiability gate ([`validate_manifest`]): a workflow declares the
//! extension roles, host commands, and named capabilities it needs, and the
//! runtime rejects fail-fast — before any step runs — when the active host
//! cannot satisfy them. The same manifest is the machine-checkable two-host
//! portability contract.

use crate::ast::{CommandCall, Conditional, Stmt, WorkflowFile};
use crate::executor::Value;
use crate::vocab;
use std::collections::BTreeSet;
use std::sync::Mutex;

// ─── SchemaProvider ───────────────────────────────────────────────────────────

/// Vocabulary-extension seam for host-supplied command and variable names.
///
/// Return non-empty slices to extend the builtin vocabulary; return `&[]`
/// to leave it unchanged. The extensions are merged with the builtin baseline
/// by [`crate::vocab::Schema::with_provider`] — collisions with builtin names
/// are silently deduplicated.
pub trait SchemaProvider {
    /// Host-declared command names that extend the builtin vocabulary.
    fn host_commands(&self) -> &[&str];

    /// Additional reserved variable names beyond the builtin set.
    fn host_reserved_variables(&self) -> &[&str];
}

/// Built-in provider: no extensions; pure builtin vocabulary.
pub struct BuiltinSchemaProvider;

impl SchemaProvider for BuiltinSchemaProvider {
    fn host_commands(&self) -> &[&str] {
        &[]
    }

    fn host_reserved_variables(&self) -> &[&str] {
        &[]
    }
}

// ─── StorageProvider (wiring pending) ───────────────────────────────────

/// Durable key/value store for cross-invocation state.
///
/// This interface is specified but executor integration is deferred until the graduation condition
/// is satisfied (two independent hosts require durable cross-invocation state).
pub trait StorageProvider {
    /// Persist a named value. `key` is host-defined; the runtime treats it as
    /// opaque.
    fn store(&self, key: &str, value: &Value);

    /// Retrieve a named value. Returns `None` if the key is absent.
    fn load(&self, key: &str) -> Option<Value>;
}

/// Built-in in-memory storage: round-trips within one process, holds nothing
/// across separate instances. `std::sync::Mutex`-guarded; a poisoned lock
/// degrades to an empty store rather than panicking.
#[derive(Default)]
pub struct InMemoryStorageProvider {
    values: Mutex<Vec<(String, Value)>>,
}

impl InMemoryStorageProvider {
    /// A fresh, empty in-memory store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl StorageProvider for InMemoryStorageProvider {
    fn store(&self, key: &str, value: &Value) {
        let mut values = self
            .values
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match values.iter_mut().find(|(k, _)| k == key) {
            Some((_, existing)) => *existing = value.clone(),
            None => values.push((key.to_string(), value.clone())),
        }
    }

    fn load(&self, key: &str) -> Option<Value> {
        let values = self
            .values
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        values
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    }
}

// ─── PolicyProvider ───────────────────────────────────────────────────

/// Runtime policy evaluation for host-defined gates.
///
/// Gates every `ModelCall`/`Deferred` effect ([`EffectClass`]) in
/// `Executor::execute_command` before the effect happens — see
/// [`effect_class_of`]. The `evaluate` contract is boolean permit/deny only;
/// spend tracking and approval workflows are host-side concerns.
pub trait PolicyProvider {
    /// Evaluate a named policy gate.
    ///
    /// `gate` is the effect-class string ([`EffectClass::as_gate_str`]).
    /// `context` is a pre-effect `Value::Map` snapshot of the command name and
    /// its unresolved argument strings. Returns `true` to permit the effect,
    /// `false` to deny it — a denial never halts the run.
    fn evaluate(&self, gate: &str, context: &Value) -> bool;
}

/// No-op policy: permits every action unconditionally.
pub struct NoopPolicyProvider;

impl PolicyProvider for NoopPolicyProvider {
    fn evaluate(&self, _gate: &str, _context: &Value) -> bool {
        true
    }
}

// ─── SettlementRail ──────────────────────────────────────────────────

/// The act half of the settlement seam: attempt to settle a
/// gate-permitted `SETTLE` and return proof.
///
/// The decide half needs no new trait — it is an ordinary [`PolicyProvider`]
/// gate over [`EffectClass::Settlement`]. `PolicyProvider::evaluate` returns
/// only `bool`, with no channel for a receipt, so settling and proving are a
/// separate capability.
pub trait SettlementRail {
    /// Attempt to settle a permitted payment. `cmd.args` carries
    /// `[payee, amount, purpose]` raw, exactly as declared — nodus parses
    /// none of them. `None` means unaccounted: the rail
    /// could not produce a verifiable receipt, and the payment MUST NOT be
    /// treated as settled.
    fn settle(&self, cmd: &CommandCall) -> Option<Value>;
}

/// No-op settlement rail: no rail is wired, so every settlement is
/// unaccounted (cannot pay without a real rail).
pub struct NoopSettlementRail;

impl SettlementRail for NoopSettlementRail {
    fn settle(&self, _cmd: &CommandCall) -> Option<Value> {
        None
    }
}

// ─── ConfigProvider ──────────────────────────────────────────────────

/// The outcome of a host reviewing a shape-checked `§config` candidate set.
#[derive(Debug, Clone)]
pub enum ConfigOutcome {
    /// The host approved the candidate set.
    Accepted(crate::validator::AcceptedConfig),
    /// The host declined; the caller's previously accepted set (if any) stays
    /// in force — a rejection never partially configures a run.
    Rejected(Vec<crate::validator::ConfigViolation>),
}

/// Host acceptance authority over a shape-checked `§config` candidate:
/// a workflow reads its configuration but can never author,
/// widen, or self-grant it — acceptance is the host's decision alone.
pub trait ConfigProvider {
    /// Decide whether to accept a candidate that already passed the pure
    /// shape check ([`crate::validator::check_config_values`]).
    fn accept(
        &self,
        decl: &crate::ast::ConfigDecl,
        candidate: crate::validator::AcceptedConfig,
    ) -> ConfigOutcome;
}

/// Built-in host acceptance: accepts the shape-checked candidate as-is. No
/// I/O, no store, no UI — matching the [`NoopPolicyProvider`] built-in
/// precedent.
pub struct DefaultConfigProvider;

impl ConfigProvider for DefaultConfigProvider {
    fn accept(
        &self,
        _decl: &crate::ast::ConfigDecl,
        candidate: crate::validator::AcceptedConfig,
    ) -> ConfigOutcome {
        ConfigOutcome::Accepted(candidate)
    }
}

// ─── Capability Manifest ──────────────────────────────────────────────

/// Model-backed commands — those the executor dispatches to its
/// [`crate::executor::ModelProvider`]. A workflow invoking any of them requires
/// the [`ExtensionRole::Model`] role from its host.
///
/// `pub(crate)`: read directly by `validator.rs`'s dialog-authoring advisories
/// — a produced-artifact reference (`W017`) needs
/// the same model-command classification this module already owns.
pub(crate) const MODEL_COMMANDS: &[&str] = &["GEN", "ANALYZE"];

/// Dialog commands — those the executor dispatches to its [`crate::executor::DialogProvider`].
/// A workflow invoking one without a `+default` requires the [`ExtensionRole::Dialog`] role.
///
/// `pub(crate)`: read directly by `validator.rs`'s dialog-authoring advisories
/// — placement (`W016`) and payload (`W017`) both
/// need the same dialog-command classification this module already owns.
pub(crate) const DIALOG_COMMANDS: &[&str] = &["ASK", "CONFIRM"];

/// Settlement commands — those the executor dispatches to its
/// [`crate::executor::SettlementRail`]. A workflow invoking one always
/// requires the [`ExtensionRole::Settlement`] role.
const SETTLEMENT_COMMANDS: &[&str] = &["SETTLE"];

// ─── Effect Classification ───────────────────────────────────────────

/// The class of an effectful step, for the [`PolicyProvider`] gate.
///
/// Realized narrower than the two-role taxonomy above might suggest: a fourth
/// `ToolUse` class is deliberately absent. Every `tool`-shaped builtin command
/// (`FETCH`, `WRITE`, `GIT`, `NOTIFY`, …) is a fixed, zero-dependency stub with
/// no host-swappable seam behind it — gating one would authorize nothing real.
/// `ToolUse` activates only once a generic host-tool extension point exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectClass {
    /// A model-backed command (`GEN`, `ANALYZE`) — see [`MODEL_COMMANDS`].
    ModelCall,
    /// A deferred/external completion (`ASK`, `CONFIRM`) — see [`DIALOG_COMMANDS`].
    Deferred,
    /// An outbound value transfer (`SETTLE`) — see [`SETTLEMENT_COMMANDS`].
    Settlement,
}

impl EffectClass {
    /// The `gate` string passed to [`PolicyProvider::evaluate`]. The DSL has no
    /// syntax for a step to declare an arbitrary named gate, so the effect
    /// class itself is the only host-matchable vocabulary that exists today.
    pub fn as_gate_str(self) -> &'static str {
        match self {
            EffectClass::ModelCall => "model_call",
            EffectClass::Deferred => "deferred",
            EffectClass::Settlement => "settlement",
        }
    }
}

/// Classify a command by its effect class, if it has one.
///
/// Returns `None` for every builtin command outside `MODEL_COMMANDS` /
/// `DIALOG_COMMANDS` / `SETTLEMENT_COMMANDS` — `LOG`, `COUNTER`, `DATE`, and
/// the rest are purely in-memory and touch no host-swappable seam, so they
/// are never gated.
pub fn effect_class_of(command: &str) -> Option<EffectClass> {
    if MODEL_COMMANDS.contains(&command) {
        Some(EffectClass::ModelCall)
    } else if DIALOG_COMMANDS.contains(&command) {
        Some(EffectClass::Deferred)
    } else if SETTLEMENT_COMMANDS.contains(&command) {
        Some(EffectClass::Settlement)
    } else {
        None
    }
}

/// An extension-point role a workflow may require from its host.
///
/// Roles name *capabilities*, never concrete host types. They mirror the
/// extension-point taxonomy: model inference, audit tracing, durable storage,
/// policy evaluation, and host vocabulary extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ExtensionRole {
    /// Model inference backend ([`crate::executor::ModelProvider`]).
    Model,
    /// Execution-event audit sink ([`crate::observability::AuditProvider`]).
    Audit,
    /// Durable cross-invocation storage ([`StorageProvider`]).
    Storage,
    /// Runtime policy evaluation ([`PolicyProvider`]).
    Policy,
    /// Host vocabulary extension ([`SchemaProvider`]).
    Vocabulary,
    /// Human-in-the-loop dialog backend ([`crate::executor::DialogProvider`]).
    Dialog,
    /// Graded-run task world ([`crate::environment::EnvironmentProvider`]).
    Environment,
    /// `§config` host-acceptance authority ([`ConfigProvider`]).
    Config,
    /// Outbound value settlement backend ([`SettlementRail`]).
    Settlement,
}

/// What a workflow declares it needs from its host to execute.
///
/// Expressed only in terms of the extension-point taxonomy ([`ExtensionRole`])
/// and named schema capabilities — never a concrete host type. An empty
/// manifest is satisfied by any host, so manifest-free and model-only workflows
/// stay runnable against the built-in in-process host without host wiring.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CapabilityManifest {
    roles: BTreeSet<ExtensionRole>,
    commands: BTreeSet<String>,
    capabilities: BTreeSet<String>,
}

impl CapabilityManifest {
    /// An empty manifest — satisfied by every host.
    pub fn new() -> Self {
        Self::default()
    }

    /// Require an extension-point role.
    pub fn require_role(mut self, role: ExtensionRole) -> Self {
        self.roles.insert(role);
        self
    }

    /// Require a host-schema command by name.
    pub fn require_command(mut self, command: impl Into<String>) -> Self {
        self.commands.insert(command.into());
        self
    }

    /// Require a named capability.
    pub fn require_capability(mut self, capability: impl Into<String>) -> Self {
        self.capabilities.insert(capability.into());
        self
    }

    /// The required extension roles.
    pub fn roles(&self) -> &BTreeSet<ExtensionRole> {
        &self.roles
    }

    /// The required host-schema commands.
    pub fn commands(&self) -> &BTreeSet<String> {
        &self.commands
    }

    /// The required named capabilities.
    pub fn capabilities(&self) -> &BTreeSet<String> {
        &self.capabilities
    }

    /// Whether the manifest requires nothing (satisfied by any host).
    pub fn is_empty(&self) -> bool {
        self.roles.is_empty() && self.commands.is_empty() && self.capabilities.is_empty()
    }

    /// Derive the manifest a workflow requires by walking its AST.
    ///
    /// A model-backed command (`GEN`/`ANALYZE`) requires [`ExtensionRole::Model`].
    /// A command outside the builtin vocabulary is a host-extension command: it
    /// requires [`ExtensionRole::Vocabulary`] and is recorded as a required
    /// command name. Builtin non-model commands need nothing — they are always
    /// available. Explicit DSL declaration (an `@needs` section) is a later
    /// refinement; this derives the manifest from invoked commands alone.
    ///
    /// [`ExtensionRole::Storage`], [`ExtensionRole::Policy`], and
    /// [`ExtensionRole::Environment`] have no corresponding command syntax, so
    /// they are never derived here — a caller requires them explicitly via
    /// [`CapabilityManifest::require_role`] (`run_with_environment` does this
    /// for `Environment` on every call, since calling it is itself the need
    /// declaration).
    pub fn from_workflow(ast: &WorkflowFile) -> Self {
        let mut calls: Vec<&CommandCall> = Vec::new();
        for step in &ast.steps {
            if let Some(body) = &step.body {
                collect_command_calls(body, &mut calls);
            }
            for sub in &step.sub_steps {
                collect_command_calls(sub, &mut calls);
            }
        }

        let mut manifest = Self::new();
        for cmd in calls {
            let name = cmd.name.as_str();
            if MODEL_COMMANDS.contains(&name) {
                manifest.roles.insert(ExtensionRole::Model);
            }
            // A dialog with a `+default` is resolved by the built-in synchronous
            // provider, so it needs no host dialog backend.
            if DIALOG_COMMANDS.contains(&name)
                && !cmd.modifiers.iter().any(|(k, _)| k == "+default")
            {
                manifest.roles.insert(ExtensionRole::Dialog);
            }
            if SETTLEMENT_COMMANDS.contains(&name) {
                manifest.roles.insert(ExtensionRole::Settlement);
            }
            if !vocab::is_known_command(name) {
                manifest.roles.insert(ExtensionRole::Vocabulary);
                manifest.commands.insert(cmd.name.clone());
            }
        }
        manifest
    }
}

/// Collect every command invocation reachable from a statement, descending into
/// conditionals, loops, and parallel branches.
fn collect_command_calls<'a>(stmt: &'a Stmt, out: &mut Vec<&'a CommandCall>) {
    match stmt {
        Stmt::Command(cmd) => out.push(cmd),
        Stmt::Conditional(cond) => collect_from_conditional(cond, out),
        Stmt::ForLoop(fl) => {
            for child in &fl.body {
                collect_command_calls(child, out);
            }
        }
        Stmt::UntilLoop(ul) => {
            for child in &ul.body {
                collect_command_calls(child, out);
            }
        }
        Stmt::Parallel(pb) => {
            for branch in &pb.branches {
                collect_command_calls(branch, out);
            }
        }
        Stmt::Switch(sw) => {
            for (_, action) in &sw.arms {
                out.push(action);
            }
            if let Some(default) = &sw.default {
                out.push(default);
            }
        }
        Stmt::Map(mb) => out.push(&mb.command),
        Stmt::VarRef(_) | Stmt::Comment(_) => {}
    }
}

/// Collect command invocations from a conditional chain: inline action, nested
/// body, every `?ELIF` branch, and the trailing `?ELSE`.
fn collect_from_conditional<'a>(cond: &'a Conditional, out: &mut Vec<&'a CommandCall>) {
    if let Some(action) = &cond.action {
        out.push(action);
    }
    for child in &cond.body {
        collect_command_calls(child, out);
    }
    for elif in &cond.elif_branches {
        collect_from_conditional(elif, out);
    }
    if let Some(else_branch) = &cond.else_branch {
        collect_from_conditional(else_branch, out);
    }
}

/// What a host actually provides — the resolution surface a manifest is checked
/// against. Hosts are built explicitly so the same struct serves both the
/// built-in in-process configuration and host-substitution tests.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostCapabilities {
    roles: BTreeSet<ExtensionRole>,
    commands: BTreeSet<String>,
    capabilities: BTreeSet<String>,
}

impl HostCapabilities {
    /// A host that provides nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// The built-in in-process host: it provides [`ExtensionRole::Model`] (the
    /// [`crate::executor::StubProvider`]), [`ExtensionRole::Audit`] (a sink is
    /// always wired), [`ExtensionRole::Vocabulary`] (the builtin schema),
    /// [`ExtensionRole::Environment`] (the [`crate::environment::StubEnvironment`]
    /// — a complete, if trivial, graded world, so a manifest-declaring workflow
    /// stays runnable in-process; this is a deliberate contrast with
    /// [`ExtensionRole::Dialog`], which `builtin()` does **not** provide, since
    /// the default dialog resolver only handles `+default`-marked dialogs), and
    /// [`ExtensionRole::Config`] (the [`DefaultConfigProvider`] — like
    /// `Environment`, a complete trivial acceptor, not merely absent like
    /// `Dialog`). [`ExtensionRole::Settlement`] is likewise **not** provided —
    /// [`NoopSettlementRail`] never settles anything, so a manifest-declaring
    /// workflow is rejected pre-run rather than discovering unaccounted
    /// settlements one step at a time. It declares no host-extension commands
    /// and no named capabilities.
    pub fn builtin() -> Self {
        let mut host = Self::new();
        host.roles.insert(ExtensionRole::Model);
        host.roles.insert(ExtensionRole::Audit);
        host.roles.insert(ExtensionRole::Vocabulary);
        host.roles.insert(ExtensionRole::Environment);
        host.roles.insert(ExtensionRole::Config);
        host
    }

    /// Declare that this host provides a role.
    pub fn with_role(mut self, role: ExtensionRole) -> Self {
        self.roles.insert(role);
        self
    }

    /// Declare that this host provides a host-schema command.
    pub fn with_command(mut self, command: impl Into<String>) -> Self {
        self.commands.insert(command.into());
        self
    }

    /// Declare that this host satisfies a named capability.
    pub fn with_capability(mut self, capability: impl Into<String>) -> Self {
        self.capabilities.insert(capability.into());
        self
    }

    /// Does the host provide `role`?
    pub fn provides(&self, role: ExtensionRole) -> bool {
        self.roles.contains(&role)
    }

    /// Does the host provide the host-schema command `command`?
    pub fn has_command(&self, command: &str) -> bool {
        self.commands.contains(command)
    }

    /// Does the host satisfy the named capability `capability`?
    pub fn satisfies(&self, capability: &str) -> bool {
        self.capabilities.contains(capability)
    }
}

/// A single capability a host failed to provide, named precisely for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Missing {
    /// An extension-point role the host does not provide.
    Role(ExtensionRole),
    /// A host-schema command the host does not provide.
    Command(String),
    /// A named capability the host does not satisfy.
    Capability(String),
}

/// Resolve a manifest against a host: return every capability the host fails to
/// provide. An empty result means the manifest is fully satisfiable and
/// the workflow may run; a non-empty result is the fail-fast rejection set. The
/// order is stable (roles, then commands, then capabilities, each sorted).
pub fn validate_manifest(manifest: &CapabilityManifest, host: &HostCapabilities) -> Vec<Missing> {
    let mut missing = Vec::new();
    for &role in &manifest.roles {
        if !host.provides(role) {
            missing.push(Missing::Role(role));
        }
    }
    for command in &manifest.commands {
        if !host.has_command(command) {
            missing.push(Missing::Command(command.clone()));
        }
    }
    for capability in &manifest.capabilities {
        if !host.satisfies(capability) {
            missing.push(Missing::Capability(capability.clone()));
        }
    }
    missing
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_schema_provider_empty_commands() {
        let p = BuiltinSchemaProvider;
        assert!(p.host_commands().is_empty());
    }

    #[test]
    fn builtin_schema_provider_empty_reserved() {
        let p = BuiltinSchemaProvider;
        assert!(p.host_reserved_variables().is_empty());
    }

    #[test]
    fn effect_class_of_model_commands() {
        assert_eq!(effect_class_of("GEN"), Some(EffectClass::ModelCall));
        assert_eq!(effect_class_of("ANALYZE"), Some(EffectClass::ModelCall));
    }

    #[test]
    fn effect_class_of_dialog_commands() {
        assert_eq!(effect_class_of("ASK"), Some(EffectClass::Deferred));
        assert_eq!(effect_class_of("CONFIRM"), Some(EffectClass::Deferred));
    }

    #[test]
    fn effect_class_of_settlement_command() {
        assert_eq!(effect_class_of("SETTLE"), Some(EffectClass::Settlement));
        assert_eq!(EffectClass::Settlement.as_gate_str(), "settlement");
    }

    #[test]
    fn effect_class_of_non_effectful_command_is_none() {
        assert_eq!(effect_class_of("LOG"), None);
        assert_eq!(effect_class_of("COUNTER"), None);
        assert_eq!(effect_class_of("FETCH"), None);
    }

    #[test]
    fn effect_class_gate_strings() {
        assert_eq!(EffectClass::ModelCall.as_gate_str(), "model_call");
        assert_eq!(EffectClass::Deferred.as_gate_str(), "deferred");
    }

    #[test]
    fn in_memory_storage_load_absent_returns_none() {
        let s = InMemoryStorageProvider::new();
        assert!(s.load("any_key").is_none());
    }

    #[test]
    fn in_memory_storage_round_trips() {
        let s = InMemoryStorageProvider::new();
        s.store("k", &Value::Text("v".to_string()));
        assert_eq!(s.load("k"), Some(Value::Text("v".to_string())));
    }

    #[test]
    fn in_memory_storage_overwrites_existing_key() {
        let s = InMemoryStorageProvider::new();
        s.store("k", &Value::Int(1));
        s.store("k", &Value::Int(2));
        assert_eq!(s.load("k"), Some(Value::Int(2)));
    }

    #[test]
    fn in_memory_storage_instances_do_not_share_state() {
        let a = InMemoryStorageProvider::new();
        let b = InMemoryStorageProvider::new();
        a.store("k", &Value::Bool(true));
        assert!(b.load("k").is_none());
    }

    #[test]
    fn noop_policy_permits_all() {
        let p = NoopPolicyProvider;
        assert!(p.evaluate("any_gate", &Value::Null));
    }

    // ── capability manifest ────────────────────────────────────────────

    #[test]
    fn manifest_default_is_empty() {
        let m = CapabilityManifest::new();
        assert!(m.is_empty());
        assert!(m.roles().is_empty());
        assert!(m.commands().is_empty());
        assert!(m.capabilities().is_empty());
    }

    #[test]
    fn host_caps_reports_wired_roles() {
        let host = HostCapabilities::new().with_role(ExtensionRole::Model);
        assert!(host.provides(ExtensionRole::Model));
        assert!(!host.provides(ExtensionRole::Storage));
    }

    #[test]
    fn builtin_host_provides_model_audit_vocabulary() {
        let host = HostCapabilities::builtin();
        assert!(host.provides(ExtensionRole::Model));
        assert!(host.provides(ExtensionRole::Audit));
        assert!(host.provides(ExtensionRole::Vocabulary));
        assert!(!host.provides(ExtensionRole::Storage));
        assert!(!host.provides(ExtensionRole::Policy));
    }

    #[test]
    fn builtin_host_provides_environment_but_not_dialog() {
        // Environment: StubEnvironment is a complete trivial world.
        // Dialog: the default resolver only handles `+default` dialogs, so
        // builtin deliberately does NOT provide it.
        let host = HostCapabilities::builtin();
        assert!(host.provides(ExtensionRole::Environment));
        assert!(!host.provides(ExtensionRole::Dialog));
    }

    #[test]
    fn builtin_host_does_not_provide_settlement() {
        // NoopSettlementRail never settles anything, so builtin deliberately
        // does NOT provide Settlement — same shape as Dialog.
        let host = HostCapabilities::builtin();
        assert!(!host.provides(ExtensionRole::Settlement));
    }

    #[test]
    fn noop_settlement_rail_never_settles() {
        let rail = NoopSettlementRail;
        let cmd = CommandCall {
            name: "SETTLE".to_string(),
            args: vec![
                "vendor".to_string(),
                "10.00".to_string(),
                "invoice".to_string(),
            ],
            ..Default::default()
        };
        assert_eq!(rail.settle(&cmd), None);
    }

    #[test]
    fn validate_manifest_satisfiable_empty() {
        let manifest = CapabilityManifest::new().require_role(ExtensionRole::Model);
        let host = HostCapabilities::builtin();
        assert!(validate_manifest(&manifest, &host).is_empty());
    }

    #[test]
    fn validate_manifest_reports_exact_missing() {
        let manifest = CapabilityManifest::new().require_role(ExtensionRole::Storage);
        let host = HostCapabilities::builtin(); // builtin host provides no Storage
        let missing = validate_manifest(&manifest, &host);
        assert_eq!(missing, vec![Missing::Role(ExtensionRole::Storage)]);
    }

    #[test]
    fn validate_manifest_reports_missing_command_and_capability() {
        let manifest = CapabilityManifest::new()
            .require_command("HOST_CMD")
            .require_capability("vision");
        let host = HostCapabilities::builtin();
        let missing = validate_manifest(&manifest, &host);
        assert!(missing.contains(&Missing::Command("HOST_CMD".to_string())));
        assert!(missing.contains(&Missing::Capability("vision".to_string())));
    }

    #[test]
    fn manifest_from_model_workflow_requires_model() {
        let src = "\
§wf:m v1.0
@in: { query }
@out: $out
@steps:
  1. GEN($in.query) → $out
";
        let ast = crate::parser::Parser::parse(src).expect("parse");
        let manifest = CapabilityManifest::from_workflow(&ast);
        assert!(
            manifest.roles().contains(&ExtensionRole::Model),
            "GEN workflow must require the Model role: {manifest:?}"
        );
    }

    #[test]
    fn manifest_from_settle_workflow_requires_settlement() {
        let src = "\
§wf:s v1.0
@out: $receipt
@steps:
  1. SETTLE(vendor, 10.00, invoice) → $receipt
";
        let ast = crate::parser::Parser::parse(src).expect("parse");
        let manifest = CapabilityManifest::from_workflow(&ast);
        assert!(
            manifest.roles().contains(&ExtensionRole::Settlement),
            "SETTLE workflow must require the Settlement role: {manifest:?}"
        );
    }

    #[test]
    fn manifest_from_pure_workflow_is_empty() {
        // LOG is a builtin, non-model command → no roles required.
        let src = "\
§wf:p v1.0
@out: $out
@steps:
  1. LOG($out)
";
        let ast = crate::parser::Parser::parse(src).expect("parse");
        let manifest = CapabilityManifest::from_workflow(&ast);
        assert!(
            manifest.is_empty(),
            "pure builtin workflow needs nothing: {manifest:?}"
        );
    }

    // ── ConfigProvider / ExtensionRole::Config ──────────────────────

    #[test]
    fn default_config_provider_accepts_candidate() {
        use crate::ast::ConfigDecl;
        use crate::validator::{AcceptedConfig, check_config_values};

        let decl = ConfigDecl::default();
        let candidate = check_config_values(&decl, &[]).expect("empty decl always passes");
        let provider = DefaultConfigProvider;
        match provider.accept(&decl, candidate) {
            ConfigOutcome::Accepted(_) => {}
            ConfigOutcome::Rejected(v) => panic!("expected Accepted, got Rejected({v:?})"),
        }
        // Type-level sanity: AcceptedConfig is constructible via the shape check.
        let _: AcceptedConfig = check_config_values(&ConfigDecl::default(), &[]).unwrap();
    }

    #[test]
    fn builtin_host_provides_config() {
        let host = HostCapabilities::builtin();
        assert!(host.provides(ExtensionRole::Config));
    }

    #[test]
    fn manifest_requiring_config_is_satisfied_by_builtin() {
        let manifest = CapabilityManifest::new().require_role(ExtensionRole::Config);
        let host = HostCapabilities::builtin();
        assert!(validate_manifest(&manifest, &host).is_empty());
    }

    #[test]
    fn manifest_requiring_config_rejected_by_stripped_host() {
        let manifest = CapabilityManifest::new().require_role(ExtensionRole::Config);
        let host = HostCapabilities::new(); // no roles at all
        let missing = validate_manifest(&manifest, &host);
        assert_eq!(missing, vec![Missing::Role(ExtensionRole::Config)]);
    }

    #[test]
    fn manifest_from_host_command_requires_vocabulary() {
        // A command outside the builtin vocabulary requires Vocabulary + the command.
        // The host command is recognized only through schema-aware parsing.
        struct HostSchema;
        impl SchemaProvider for HostSchema {
            fn host_commands(&self) -> &[&str] {
                &["CUSTOM_CMD"]
            }
            fn host_reserved_variables(&self) -> &[&str] {
                &[]
            }
        }
        let schema = crate::vocab::Schema::with_provider(&HostSchema);
        let src = "\
§wf:h v1.0
@out: $out
@steps:
  1. CUSTOM_CMD($out) → $out
";
        let ast = crate::parser::Parser::parse_with_schema(src, &schema).expect("parse");
        let manifest = CapabilityManifest::from_workflow(&ast);
        assert!(manifest.roles().contains(&ExtensionRole::Vocabulary));
        assert!(manifest.commands().contains("CUSTOM_CMD"));
    }
}
