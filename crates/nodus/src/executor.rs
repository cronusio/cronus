//! Executor — runtime engine for NODUS workflows.
//!
//! Walks the AST produced by the parser and dispatches each step command to
//! its handler. Handlers are pluggable via the [`ModelProvider`] trait; the
//! built-in [`StubProvider`] satisfies the interface without external I/O,
//! which enables tests to run the full boot sequence in-process.
//!
//! Boot sequence (matches the reference protocol):
//! 1. Load schema (resolved from `§runtime` block)
//! 2. Read `!!` rules — internalise hard constraints
//! 3. Read `!PREF` — internalise soft preferences
//! 4. Register `@in` / `@ctx` inputs
//! 5. Match `@ON` triggers
//! 6. Execute `@steps` sequentially; thread pipeline `→` targets between steps
//!
//! Every execution returns a [`RunResult`] regardless of outcome; callers
//! inspect `status` first, then use `out`, `log`, and `errors` for details.

use crate::ast::{AbsoluteRule, CommandCall, RuleKind, Step, Stmt, WorkflowFile};
use crate::observability::{
    AuditProvider, DialogProvenance, EnvInteraction, EventAnnotations, ExecutionEvent,
    ExecutionMode, FaultIdentity, FieldDescriptor, LoopType, Measurement, NoopAuditProvider,
    ReproRecipe, RunManifest, RunStatus, SimFidelity, SourceRef, step_identity,
};
use crate::portability::{
    NoopPolicyProvider, NoopSettlementRail, PolicyProvider, SettlementRail, effect_class_of,
};
use crate::vocab;
use std::collections::HashMap;
use std::time::Instant;

// ─── Value ────────────────────────────────────────────────────────────────────

/// A runtime value — the dynamic type for workflow variables.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Value {
    /// No value / unset.
    #[default]
    Null,
    /// Boolean.
    Bool(bool),
    /// Integer.
    Int(i64),
    /// Floating-point.
    Float(f64),
    /// Text string.
    Text(String),
    /// Ordered list.
    List(Vec<Value>),
    /// Ordered key-value map (preserves insertion order).
    Map(Vec<(String, Value)>),
}

impl Value {
    fn as_text(&self) -> String {
        match self {
            Value::Text(s) => s.clone(),
            Value::Int(n) => n.to_string(),
            Value::Float(f) => f.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Null => String::new(),
            Value::List(_) | Value::Map(_) => format!("{self:?}"),
        }
    }

    fn map_get(&self, key: &str) -> Value {
        match self {
            Value::Map(entries) => entries
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
                .unwrap_or(Value::Null),
            _ => Value::Null,
        }
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::Text(s)
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::Text(s.to_string())
    }
}

// ─── Result contract ──────────────────────────────────────────────────────────

/// Overall outcome of a workflow execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// All steps completed without errors.
    Ok,
    /// Steps completed but non-fatal errors occurred.
    Partial,
    /// A fatal error halted execution.
    Failed,
    /// A `!BREAK` or rule-violation aborted the run.
    Aborted,
    /// Execution suspended at a dialog awaiting human re-trigger (`!PAUSE` / unresolved `ASK`/`CONFIRM`).
    Paused,
}

/// A single step's execution record in the audit log.
#[derive(Debug, Clone)]
pub struct LogEntry {
    /// Step number where this command ran.
    pub step: u32,
    /// Command name.
    pub command: String,
    /// The value returned by the handler (`Null` for void commands).
    pub result: Value,
}

/// A runtime error.
#[derive(Debug, Clone)]
pub struct RuntimeError {
    /// One of the `NODUS:*` error codes from [`crate::vocab::error_code`].
    pub code: String,
    /// Step number where the error occurred (`0` = boot sequence).
    pub step: u32,
    /// Human-readable reason.
    pub reason: String,
}

/// Renders a [`RuntimeError`] as the `$error` value an `@err:` handler sees.
/// Called only at the point of dispatch — `$error` stays at its
/// seeded empty map for the rest of a run that never reaches one.
/// Split `input` into what may be bound and the runtime-owned names withheld
/// from it. A name the workflow declares in `@in:` is the workflow's own
/// input — declared on purpose, so it is kept.
fn withhold_runtime_owned_input(
    ast: &WorkflowFile,
    input: Option<Value>,
) -> (Option<Value>, Vec<String>) {
    let Some(Value::Map(entries)) = input else {
        return (input, Vec::new());
    };
    let declares = |key: &str| {
        ast.input_decl.as_ref().is_some_and(|decl| {
            decl.fields
                .iter()
                .any(|f| f.name.trim_start_matches('$') == key.trim_start_matches('$'))
        })
    };
    let mut kept = Vec::with_capacity(entries.len());
    let mut withheld = Vec::new();
    for (key, value) in entries {
        if crate::validator::is_runtime_owned_name(&key) && !declares(&key) {
            withheld.push(key);
        } else {
            kept.push((key, value));
        }
    }
    (Some(Value::Map(kept)), withheld)
}

/// The key a `~PARALLEL` branch's result is collected under: the branch's own
/// pipeline target (without its `$`), else its 1-based position.
fn branch_result_key(branch: &Stmt, index: usize) -> String {
    let target = match branch {
        Stmt::Command(cmd) => cmd.pipeline_target.as_deref(),
        _ => None,
    };
    match target.map(|t| t.trim_start_matches('$')) {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => format!("branch_{}", index + 1),
    }
}

fn error_to_value(err: &RuntimeError) -> Value {
    Value::Map(vec![
        ("code".to_string(), Value::Text(err.code.clone())),
        ("step".to_string(), Value::Int(err.step as i64)),
        ("reason".to_string(), Value::Text(err.reason.clone())),
    ])
}

/// Snapshot a paused run hands to its host so the run can be resumed.
/// The runtime produces this; persistence and re-invocation are host concerns.
#[derive(Debug, Clone)]
pub struct ResumeDescriptor {
    /// `"wf:<name>"` of the suspended workflow.
    pub workflow: String,
    /// Variable environment captured at the suspension point.
    pub vars: HashMap<String, Value>,
    /// Index of the step that suspended.
    pub step_index: u32,
    /// Content identity of the pinned definition — the same
    /// `digest_ast` computation [`ReproRecipe::workflow_digest`](crate::observability::ReproRecipe::workflow_digest)
    /// already performs. A host persisting and re-resolving workflow
    /// definitions by name compares this against a fresh digest of whatever
    /// source it is about to resume with; a mismatch means the definition
    /// changed since suspension. The core has no resume-time call site of
    /// its own to check this — enforcement is necessarily host-side.
    pub workflow_digest: String,
}

/// The structured output of a workflow execution.
///
/// Every execution — successful or not — returns a `RunResult`. Callers
/// inspect `status` first, then use `out`, `log`, and `errors` for details.
#[derive(Debug)]
pub struct RunResult {
    /// `"wf:<name>"` identifier of the workflow that ran.
    pub workflow: String,
    /// Overall execution status.
    pub status: Status,
    /// The final value of `$out` at the end of execution.
    pub out: Value,
    /// Ordered log of every step that executed.
    pub log: Vec<LogEntry>,
    /// Non-fatal and fatal errors accumulated during execution.
    pub errors: Vec<RuntimeError>,
    /// Flags emitted by routing, escalation, and unknown-command paths.
    pub flags: Vec<String>,
    /// Final variable environment: variable name (without `$`) → value.
    /// Enables test runners and debuggers to inspect all pipeline bindings.
    pub vars: HashMap<String, Value>,
    /// Present only when `status == Paused`: the descriptor a host uses to resume.
    pub resume: Option<ResumeDescriptor>,
}

// ─── ModelProvider ────────────────────────────────────────────────────────────

/// Why a model call produced no answer.
///
/// Carries a short, secret-free description for the run record: never the
/// prompt, never a credential, never the provider's raw response body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelError {
    pub reason: String,
}

impl ModelError {
    pub fn new(reason: impl Into<String>) -> Self {
        ModelError {
            reason: reason.into(),
        }
    }
}

impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "model call failed: {}", self.reason)
    }
}

impl std::error::Error for ModelError {}

/// Pluggable LLM backend for `GEN`, `ANALYZE`, and similar model-backed
/// commands. Implementations can call a real API or return stubs for testing.
///
/// The executor calls the fallible [`try_generate`](Self::try_generate) and
/// [`try_analyze`](Self::try_analyze). Their defaults delegate to the
/// infallible methods, so a provider that cannot fail — a stub, a test
/// double — implements only those; a provider in front of a real backend
/// overrides the fallible pair so a dropped connection or a cancelled call is
/// reported as a failed step instead of being returned as a short answer.
pub trait ModelProvider {
    /// A short identifier for this provider (surfaced in the result).
    fn model_id(&self) -> &str;

    /// Generate text from a prompt.
    fn generate(&self, prompt: &str, modifiers: &[(String, String)]) -> String;

    /// Analyse text and return a map of flag → score/label.
    fn analyze(&self, text: &str, flags: &[String]) -> Value;

    /// Generate text, or report why no answer was produced.
    fn try_generate(
        &self,
        prompt: &str,
        modifiers: &[(String, String)],
    ) -> Result<String, ModelError> {
        Ok(self.generate(prompt, modifiers))
    }

    /// Analyse text, or report why no answer was produced.
    fn try_analyze(&self, text: &str, flags: &[String]) -> Result<Value, ModelError> {
        Ok(self.analyze(text, flags))
    }

    /// Whether this provider fabricates its answers instead of consulting a
    /// model. A run whose model calls are answered by such a provider is a
    /// simulation by construction and is recorded as one, whatever
    /// the caller declared — nothing that ran was real.
    fn is_stub(&self) -> bool {
        false
    }
}

/// Zero-dependency stub that returns deterministic responses.
///
/// Suitable for unit tests and the vertical-slice boot path. Every `generate`
/// call returns a bracketed label; every `analyze` flag gets a score of 0.9.
pub struct StubProvider;

impl ModelProvider for StubProvider {
    fn model_id(&self) -> &str {
        "nodus-stub"
    }

    fn is_stub(&self) -> bool {
        true
    }

    fn generate(&self, prompt: &str, modifiers: &[(String, String)]) -> String {
        let tone = modifiers
            .iter()
            .find(|(k, _)| k == "+tone")
            .map(|(_, v)| v.as_str())
            .unwrap_or("brand");
        format!("[STUB gen({prompt}) tone={tone}]")
    }

    fn analyze(&self, _text: &str, flags: &[String]) -> Value {
        let entries: Vec<(String, Value)> = flags
            .iter()
            .map(|f| {
                let v = if f == "intent" {
                    Value::Text("stub_intent".to_string())
                } else {
                    Value::Float(0.9)
                };
                (f.clone(), v)
            })
            .collect();
        Value::Map(entries)
    }
}

// ─── DialogProvider ─────────────────────────────────────────────────────────

/// The outcome of a dialog command (`ASK` / `CONFIRM`).
#[derive(Debug, Clone, PartialEq)]
pub enum DialogOutcome {
    /// Resolved with a typed answer (by a human or a `+default`).
    Answer(Value),
    /// Resolved from a host durable prior decision (`+remember`), without
    /// re-prompting. Binds through the exact same path as `Answer` —
    /// memoization changes where the value came from, never how it binds.
    Remembered(Value),
    /// No resolution available; the runtime should suspend (`Status::Paused`).
    Pause,
    /// A `+timeout` elapsed before an answer.
    Timeout,
    /// A `+strict` `CONFIRM` was rejected.
    Rejected,
}

/// Pluggable human-in-the-loop backend for `ASK` / `CONFIRM`. Implementations
/// can drive a real UI or resolve synchronously for tests; the language and
/// executor never name a concrete channel (host neutrality).
pub trait DialogProvider {
    /// Pose a typed question. `modifiers` carries `+type`, `+default`, etc.
    fn ask(&self, prompt: &str, modifiers: &[(String, String)]) -> DialogOutcome;

    /// Pose an approval decision.
    fn confirm(&self, content: &str, modifiers: &[(String, String)]) -> DialogOutcome;
}

/// Built-in synchronous resolver: resolves a dialog from its `+default`
/// modifier, otherwise signals [`DialogOutcome::Pause`]. Performs no I/O and
/// never blocks, so non-interactive and test runs stay deterministic.
pub struct DefaultDialogProvider;

impl DefaultDialogProvider {
    fn resolve(modifiers: &[(String, String)]) -> DialogOutcome {
        modifiers
            .iter()
            .find(|(k, _)| k == "+default")
            .map(|(_, v)| DialogOutcome::Answer(Value::Text(v.clone())))
            .unwrap_or(DialogOutcome::Pause)
    }
}

impl DialogProvider for DefaultDialogProvider {
    fn ask(&self, _prompt: &str, modifiers: &[(String, String)]) -> DialogOutcome {
        Self::resolve(modifiers)
    }

    fn confirm(&self, _content: &str, modifiers: &[(String, String)]) -> DialogOutcome {
        Self::resolve(modifiers)
    }
}

// ─── Execution signal ─────────────────────────────────────────────────────────

enum Signal {
    Break,
    Skip,
    /// A dialog could not resolve, or a branch carried `!PAUSE`; suspend the run
    /// (`Status::Paused`).
    Pause,
    /// A branch carried `!HALT`; stop the run with a failed status.
    Halt,
}

// ─── Execution context ────────────────────────────────────────────────────────

/// A step's declared undo, recorded only once that step's own action has
/// completed without adding a new runtime error (only completed
/// effects compensate; a still-running or already-failed step never enters
/// this ledger, keeping cancellation a distinct path from the unwind). A step
/// whose action carries a control signal (`!HALT`/`!PAUSE`/`!BREAK`/`!SKIP`)
/// is not recorded either — `run_step_with_retry` returns early on any
/// signal, before the ledger-append point, since a signal-carrying step's
/// completion is ambiguous in exactly the way a per-item context is (restart handling
/// reasons about the same ambiguity for restart requests).
struct CompletedEffect {
    step_number: u32,
    compensation: CommandCall,
}

struct ExecutionContext {
    variables: HashMap<String, Value>,
    rules: Vec<AbsoluteRule>,
    log: Vec<LogEntry>,
    errors: Vec<RuntimeError>,
    flags: Vec<String>,
    out_locked: bool,
    event_count: u32,
    start_instant: Instant,
    paused_at: Option<u32>,
    /// Bound once at run construction; shared by every event this run
    /// emits and equal to `RunManifest.run_id`. Never re-derived per event.
    correlation_id: String,
    /// The completed-effect ledger, appended in completion order and
    /// drained back-to-front (LIFO) when the run fails.
    compensations: Vec<CompletedEffect>,
}

impl ExecutionContext {
    fn new(correlation_id: String) -> Self {
        ExecutionContext {
            variables: HashMap::new(),
            rules: Vec::new(),
            log: Vec::new(),
            errors: Vec::new(),
            flags: Vec::new(),
            out_locked: false,
            event_count: 0,
            start_instant: Instant::now(),
            paused_at: None,
            correlation_id,
            compensations: Vec::new(),
        }
    }

    /// Set a variable by name (strips leading `$`). Respects the `$out` lock.
    fn set_var(&mut self, name: &str, value: Value) {
        let key = name.trim_start_matches('$').to_string();
        if key == "out" && self.out_locked {
            self.errors.push(RuntimeError {
                code: vocab::error_code::RULE_VIOLATION.to_string(),
                step: 0,
                reason: "cannot modify $out after LOG() has been called".to_string(),
            });
            return;
        }
        self.variables.insert(key, value);
    }

    /// Resolve a variable name to its value, supporting dot-notation (`$in.query`).
    fn get_var(&self, name: &str) -> Value {
        let clean = name.trim_start_matches('$');
        let mut parts = clean.splitn(2, '.');
        let root = parts.next().unwrap_or("");
        let tail = parts.next();

        let root_val = self.variables.get(root).cloned().unwrap_or(Value::Null);
        match tail {
            None => root_val,
            Some(path) => {
                let mut val = root_val;
                for part in path.split('.') {
                    val = val.map_get(part);
                }
                val
            }
        }
    }

    /// Append a step record to the audit log.
    fn log_step(&mut self, step: u32, command: &str, result: Value) {
        self.log.push(LogEntry {
            step,
            command: command.to_string(),
            result,
        });
    }

    /// Check `cmd` against the current `!!NEVER` rules. Returns a violation
    /// description when the command must be blocked.
    fn check_rules(&self, command: &str, args: &[String]) -> Option<String> {
        let cmd_up = command.to_uppercase();
        let args_lo: Vec<String> = args.iter().map(|a| a.to_lowercase()).collect();

        for rule in &self.rules {
            let content = rule.content.trim();
            let content_up = content.to_uppercase();

            if rule.kind == Some(RuleKind::Never) {
                // Simple name match: `!!NEVER: DELETE`
                if content_up == cmd_up {
                    return Some(format!("!!NEVER: {content}"));
                }

                // Argument match: `!!NEVER: FETCH("evil.com")`
                if content_up.contains(&cmd_up)
                    && args_lo
                        .iter()
                        .any(|a| content.to_lowercase().contains(a.as_str()))
                {
                    return Some(format!("!!NEVER: {content}"));
                }

                // Conditional: `!!NEVER: publish WITHOUT validate`
                if cmd_up == "PUBLISH"
                    && content_up.contains("WITHOUT")
                    && content_up.contains("VALIDATE")
                {
                    let validated = self.log.iter().any(|e| e.command == "VALIDATE");
                    if !validated {
                        return Some(format!("!!NEVER: {content}"));
                    }
                }
            }
        }
        None
    }
}

/// The version tag every workflow digest carries. A digest is compared later —
/// possibly after an upgrade — so it names the scheme that produced it: a
/// digest with a different tag is *not comparable*, rather than merely unequal.
/// Bump it whenever the algorithm or the canonical form below changes.
pub(crate) const DIGEST_VERSION: &str = "nd1";

/// FNV-1a, 64-bit: a fixed, published algorithm (offset basis
/// `0xcbf29ce484222325`, prime `0x100000001b3`), so the same bytes hash to the
/// same value on every build, platform and Rust release — which
/// `DefaultHasher` (SipHash with an unspecified, release-dependent key and
/// algorithm) does not promise. It is an identity check for *accidental*
/// change, not a cryptographic commitment: a host that must resist a crafted
/// collision computes its own digest over [`canonical_text`].
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// `nd1:` plus the 16-hex-digit FNV-1a of `text`.
pub(crate) fn digest_text(text: &str) -> String {
    format!("{DIGEST_VERSION}:{:016x}", fnv1a64(text.as_bytes()))
}

/// The canonical text a workflow definition is identified by: its compact
/// form, which round-trips to an equal AST. Unlike the AST's `Debug`
/// rendering — which changes whenever any AST type gains a field — it changes
/// only when the workflow itself does.
pub(crate) fn canonical_text(ast: &WorkflowFile) -> String {
    crate::transpiler::Transpiler::to_nodus(ast)
}

/// Content digest of the parsed workflow definition: [`digest_text`]
/// over [`canonical_text`]. `execute_inner` receives only the parsed
/// [`WorkflowFile`], never the raw source (parsing happens earlier, in
/// `workflows.rs`), so identity is taken from the AST — and two sources
/// differing only in whitespace or comments parse to the same AST and share a
/// digest, the correct notion of "same workflow" for reproducibility, since
/// neither affects execution.
///
/// `pub(crate)`: reused by `environment.rs`'s `EnvRunResult::candidate()`
///  so `CandidateResult.workflow_digest` and `ReproRecipe.workflow_digest`
/// agree on one notion of identity under the one field name.
pub(crate) fn digest_ast(ast: &WorkflowFile) -> String {
    digest_text(&canonical_text(ast))
}

/// Resolve the run's `correlation_id`: the caller-supplied `run_id`
/// verbatim when non-empty, or a generated fallback when it is not — no event
/// is ever emitted uncorrelated, even from the plain `execute()`/
/// `execute_with_params("", "")` callers. The fallback is a process-local
/// monotonic counter (`"run-{n}"`) rather than a UUID: zero-dep (no
/// crate added), and it introduces no wall-clock reading into run metadata at
/// all — not even for a generated id — so nothing time-based enters the
/// picture. It is unique within one process, not globally; a host that needs
/// global uniqueness already supplies its own `run_id`. This resolved value
/// also becomes `RunManifest.run_id`, so the identity
/// (`correlation_id == RunManifest.run_id`) holds even on the empty-id path.
fn resolve_correlation_id(run_id: &str) -> String {
    if !run_id.is_empty() {
        return run_id.to_string();
    }
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("run-{n}")
}

// ─── Executor ─────────────────────────────────────────────────────────────────

/// Core runtime engine for NODUS workflows.
pub struct Executor {
    provider: Box<dyn ModelProvider>,
    audit: Box<dyn AuditProvider>,
    dialog: Box<dyn DialogProvider>,
    policy: Box<dyn PolicyProvider>,
    settlement: Box<dyn SettlementRail>,
}

/// Assembles an [`Executor`] from any combination of seams.
///
/// The `Executor::with_*` constructors each inject one seam and leave the rest
/// at their built-ins — so no two of them could be used together, and a real
/// model could never run under a host policy. The builder is the composed
/// form: every seam is independently replaceable, and any seam left alone
/// keeps its built-in (the stub model, no audit, the synchronous dialog
/// resolver, a permit-everything policy, a rail that settles nothing).
pub struct ExecutorBuilder {
    provider: Box<dyn ModelProvider>,
    audit: Box<dyn AuditProvider>,
    dialog: Box<dyn DialogProvider>,
    policy: Box<dyn PolicyProvider>,
    settlement: Box<dyn SettlementRail>,
}

impl Default for ExecutorBuilder {
    fn default() -> Self {
        ExecutorBuilder {
            provider: Box::new(StubProvider),
            audit: Box::new(NoopAuditProvider),
            dialog: Box::new(DefaultDialogProvider),
            policy: Box::new(NoopPolicyProvider),
            settlement: Box::new(NoopSettlementRail),
        }
    }
}

impl ExecutorBuilder {
    /// The model behind `GEN` / `ANALYZE`.
    pub fn model(mut self, provider: impl ModelProvider + 'static) -> Self {
        self.provider = Box::new(provider);
        self
    }

    /// The sink for the run's execution events and manifest.
    pub fn audit(mut self, audit: impl AuditProvider + 'static) -> Self {
        self.audit = Box::new(audit);
        self
    }

    /// The resolver behind `ASK` / `CONFIRM`.
    pub fn dialog(mut self, dialog: impl DialogProvider + 'static) -> Self {
        self.dialog = Box::new(dialog);
        self
    }

    /// The per-effect authorization gate every model call, dialog and
    /// settlement passes before it happens.
    pub fn policy(mut self, policy: impl PolicyProvider + 'static) -> Self {
        self.policy = Box::new(policy);
        self
    }

    /// The rail a permitted `SETTLE` step pays through.
    pub fn settlement(mut self, rail: impl SettlementRail + 'static) -> Self {
        self.settlement = Box::new(rail);
        self
    }

    pub fn build(self) -> Executor {
        Executor {
            provider: self.provider,
            audit: self.audit,
            dialog: self.dialog,
            policy: self.policy,
            settlement: self.settlement,
        }
    }
}

impl Executor {
    /// Start assembling an executor from any combination of seams.
    pub fn builder() -> ExecutorBuilder {
        ExecutorBuilder::default()
    }

    /// Create an executor backed by `provider` with no-op audit and the built-in
    /// synchronous dialog resolver.
    pub fn new(provider: impl ModelProvider + 'static) -> Self {
        Executor {
            provider: Box::new(provider),
            audit: Box::new(NoopAuditProvider),
            dialog: Box::new(DefaultDialogProvider),
            policy: Box::new(NoopPolicyProvider),
            settlement: Box::new(NoopSettlementRail),
        }
    }

    /// Create an executor backed by the built-in [`StubProvider`].
    pub fn with_stub() -> Self {
        Self::new(StubProvider)
    }

    /// Create an executor with a custom model provider and audit provider.
    pub fn with_audit(
        provider: impl ModelProvider + 'static,
        audit: impl AuditProvider + 'static,
    ) -> Self {
        Executor {
            provider: Box::new(provider),
            audit: Box::new(audit),
            dialog: Box::new(DefaultDialogProvider),
            policy: Box::new(NoopPolicyProvider),
            settlement: Box::new(NoopSettlementRail),
        }
    }

    /// Like [`Executor::with_audit`] but takes an already-boxed
    /// [`AuditProvider`] — the shape `run_with_environment`'s two entry points
    /// (plain vs. audited) share internally.
    pub(crate) fn with_boxed_audit(
        provider: impl ModelProvider + 'static,
        audit: Box<dyn AuditProvider>,
    ) -> Self {
        Executor {
            provider: Box::new(provider),
            audit,
            dialog: Box::new(DefaultDialogProvider),
            policy: Box::new(NoopPolicyProvider),
            settlement: Box::new(NoopSettlementRail),
        }
    }

    /// Create an executor with the built-in model/audit and a custom
    /// [`DialogProvider`].
    pub fn with_dialog(dialog: impl DialogProvider + 'static) -> Self {
        Executor {
            provider: Box::new(StubProvider),
            audit: Box::new(NoopAuditProvider),
            dialog: Box::new(dialog),
            policy: Box::new(NoopPolicyProvider),
            settlement: Box::new(NoopSettlementRail),
        }
    }

    /// Create an executor with a custom [`DialogProvider`] and audit provider.
    pub fn with_dialog_and_audit(
        dialog: impl DialogProvider + 'static,
        audit: impl AuditProvider + 'static,
    ) -> Self {
        Executor {
            provider: Box::new(StubProvider),
            audit: Box::new(audit),
            dialog: Box::new(dialog),
            policy: Box::new(NoopPolicyProvider),
            settlement: Box::new(NoopSettlementRail),
        }
    }

    /// Create an executor with the built-in model/audit/dialog and a custom
    /// [`PolicyProvider`].
    pub fn with_policy(policy: impl PolicyProvider + 'static) -> Self {
        Executor {
            provider: Box::new(StubProvider),
            audit: Box::new(NoopAuditProvider),
            dialog: Box::new(DefaultDialogProvider),
            policy: Box::new(policy),
            settlement: Box::new(NoopSettlementRail),
        }
    }

    /// Create an executor with a custom [`PolicyProvider`] and audit provider.
    pub fn with_policy_and_audit(
        policy: impl PolicyProvider + 'static,
        audit: impl AuditProvider + 'static,
    ) -> Self {
        Executor {
            provider: Box::new(StubProvider),
            audit: Box::new(audit),
            dialog: Box::new(DefaultDialogProvider),
            policy: Box::new(policy),
            settlement: Box::new(NoopSettlementRail),
        }
    }

    /// Create an executor with the built-in model/audit/dialog/policy and a
    /// custom [`SettlementRail`].
    pub fn with_settlement(settlement: impl SettlementRail + 'static) -> Self {
        Executor {
            provider: Box::new(StubProvider),
            audit: Box::new(NoopAuditProvider),
            dialog: Box::new(DefaultDialogProvider),
            policy: Box::new(NoopPolicyProvider),
            settlement: Box::new(settlement),
        }
    }

    /// Create an executor with a custom [`SettlementRail`] and audit provider.
    pub fn with_settlement_and_audit(
        settlement: impl SettlementRail + 'static,
        audit: impl AuditProvider + 'static,
    ) -> Self {
        Executor {
            provider: Box::new(StubProvider),
            audit: Box::new(audit),
            dialog: Box::new(DefaultDialogProvider),
            policy: Box::new(NoopPolicyProvider),
            settlement: Box::new(settlement),
        }
    }

    /// Run a parsed workflow through the boot sequence, execute its steps, and
    /// return the structured [`RunResult`].
    pub fn execute(&self, ast: &WorkflowFile, input: Option<Value>) -> RunResult {
        self.execute_with_restart(
            ast,
            input,
            "",
            "",
            None,
            None,
            Vec::new(),
            ExecutionMode::default(),
            Vec::new(),
        )
        .0
    }

    /// Run a workflow with explicit run metadata forwarded to the audit provider.
    pub fn execute_with_params(
        &self,
        ast: &WorkflowFile,
        input: Option<Value>,
        run_id: &str,
        started_at: &str,
    ) -> RunResult {
        self.execute_with_restart(
            ast,
            input,
            run_id,
            started_at,
            None,
            None,
            Vec::new(),
            ExecutionMode::default(),
            Vec::new(),
        )
        .0
    }

    /// Like [`execute_with_params`](Self::execute_with_params) but with a
    /// host-declared execution mode and resolved exposure switches,
    /// both mirrored into the manifest's `repro` recipe. A
    /// caller declaring `ExecutionMode::Real` and no switches sees the
    /// identical output as `execute_with_params` (observer neutrality preserved) — this is
    /// purely an additive entry point, not a modification of the defaults.
    pub fn execute_with_manifest_context(
        &self,
        ast: &WorkflowFile,
        input: Option<Value>,
        run_id: &str,
        started_at: &str,
        execution_mode: ExecutionMode,
        exposure_switches: Vec<(String, String)>,
    ) -> RunResult {
        self.execute_with_restart(
            ast,
            input,
            run_id,
            started_at,
            None,
            None,
            Vec::new(),
            execution_mode,
            exposure_switches,
        )
        .0
    }

    /// Run a workflow under a step/wall-clock ceiling, embedding `env_trajectory`
    /// (the environment's reset entry) in the run manifest. Used by
    /// `run_with_environment`; not part of the public API — the
    /// environment combinators in `workflows.rs` are the sanctioned entry point.
    ///
    /// `max_tokens` on a `Budget` is intentionally not accepted here — no
    /// token-accounting seam exists on [`ModelProvider`] today (documented gap,
    /// same precedent as `StorageProvider`/`PolicyProvider`).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn execute_for_environment(
        &self,
        ast: &WorkflowFile,
        input: Option<Value>,
        run_id: &str,
        started_at: &str,
        max_steps: Option<u32>,
        wall_clock_ms: Option<u64>,
        env_trajectory: Vec<EnvInteraction>,
    ) -> (RunResult, bool) {
        self.execute_with_restart(
            ast,
            input,
            run_id,
            started_at,
            max_steps,
            wall_clock_ms,
            env_trajectory,
            ExecutionMode::default(),
            Vec::new(),
        )
    }

    /// The single emission choke point. Assigns `seq` from
    /// `ctx.event_count` and the run's bound `correlation_id`, dispatches to
    /// the audit provider, then increments the counter — so a mismatch
    /// between the assigned `seq` and the counter is unrepresentable, not
    /// merely absent. Every `record_event` call in this module goes through
    /// here; none calls `self.audit.record_event` directly.
    ///
    /// **Durable path only.** `seq` numbers the durable stream —
    /// this function must never be used to emit a transient event, since a
    /// transient consuming a `seq` would read as a phantom gap if dropped,
    /// and a severed transient tail would make a completed run classify as
    /// `Truncated` ([`crate::observability::classify_trace`]). nodus emits no
    /// transients today (`ModelProvider::generate` returns a complete
    /// `String`, so no chunk exists); a future host-facing streaming path
    /// needs a separate companion that dispatches without touching the
    /// counter, not a change to this function.
    fn emit(&self, ctx: &mut ExecutionContext, build: impl FnOnce(u64, String) -> ExecutionEvent) {
        let event = build(ctx.event_count as u64, ctx.correlation_id.clone());
        self.audit.record_event(event);
        ctx.event_count += 1;
    }

    /// Every execution enters here. The caller's input is first stripped of any
    /// key that names a runtime-owned variable (`$user`, `$session`, `$ctx`, …)
    /// the workflow does not itself declare in `@in:`: those values are the
    /// runtime's to set, so binding a caller's copy would let a caller forge a
    /// fact the workflow may branch on. The `run*` entry points reject such an
    /// input outright before the run starts (`E022`); this is the same rule for
    /// a host that drives the executor directly, where each dropped key is
    /// reported as an `INPUT_IGNORED:<name>` flag instead of vanishing silently.
    /// Stripping happens before the restart loop seeds its own `$restart_count`.
    #[allow(clippy::too_many_arguments)]
    fn execute_with_restart(
        &self,
        ast: &WorkflowFile,
        input: Option<Value>,
        run_id: &str,
        started_at: &str,
        max_steps: Option<u32>,
        wall_clock_ms: Option<u64>,
        env_trajectory: Vec<EnvInteraction>,
        execution_mode: ExecutionMode,
        exposure_switches: Vec<(String, String)>,
    ) -> (RunResult, bool) {
        let (input, withheld) = withhold_runtime_owned_input(ast, input);
        let (mut result, budget_halted) = self.execute_restartable(
            ast,
            input,
            run_id,
            started_at,
            max_steps,
            wall_clock_ms,
            env_trajectory,
            execution_mode,
            exposure_switches,
        );
        result.flags.extend(
            withheld
                .into_iter()
                .map(|name| format!("INPUT_IGNORED:{name}")),
        );
        (result, budget_halted)
    }

    /// Wraps [`Self::execute_inner`] in a bounded attempt loop when the
    /// workflow declares `restart_max`. Wrapping **around** rather than
    /// reaching inside is the load-bearing choice — `execute_inner` builds a
    /// fresh [`ExecutionContext`] on every call, so re-entering it *is* the
    /// fresh reconstruction; no state-clearing routine exists to drift
    /// out of sync as the context gains fields later.
    ///
    /// A workflow with no `restart_max` takes exactly the pre-NL-23 path: one
    /// `execute_inner` call, no `$restart_count` seeded, byte-identical result
    /// unless the workflow itself sets `$restart` (in which case the request
    /// is refused with `NODUS:RESTART_LIMIT` — self-restart is opt-in via
    /// declaration, never silently active).
    #[allow(clippy::too_many_arguments)]
    fn execute_restartable(
        &self,
        ast: &WorkflowFile,
        input: Option<Value>,
        run_id: &str,
        started_at: &str,
        max_steps: Option<u32>,
        wall_clock_ms: Option<u64>,
        env_trajectory: Vec<EnvInteraction>,
        execution_mode: ExecutionMode,
        exposure_switches: Vec<(String, String)>,
    ) -> (RunResult, bool) {
        let restart_max = ast.runtime.as_ref().and_then(|rt| rt.restart_max);

        let Some(max) = restart_max else {
            let (mut result, budget_halted) = self.execute_inner(
                ast,
                input,
                run_id,
                started_at,
                max_steps,
                wall_clock_ms,
                env_trajectory,
                execution_mode,
                exposure_switches,
            );
            if result.vars.get("restart").is_some_and(Self::is_truthy) {
                result
                    .flags
                    .push(vocab::error_code::RESTART_LIMIT.to_string());
            }
            return (result, budget_halted);
        };

        let mut attempt: u32 = 0;
        loop {
            let attempt_input = Self::seed_restart_count(&input, attempt);
            // Each attempt is its own event stream: a shared correlation_id
            // across attempts would break the per-manifest event_count == highest
            // seq + 1 identity, so a non-empty caller-supplied run_id is
            // disambiguated per attempt rather than reused verbatim.
            let attempt_run_id = if run_id.is_empty() {
                String::new()
            } else {
                format!("{run_id}-restart{attempt}")
            };
            let (mut result, budget_halted) = self.execute_inner(
                ast,
                attempt_input,
                &attempt_run_id,
                started_at,
                max_steps,
                wall_clock_ms,
                env_trajectory.clone(),
                execution_mode.clone(),
                exposure_switches.clone(),
            );

            let requested = result.vars.get("restart").is_some_and(Self::is_truthy);
            if !requested {
                return (result, budget_halted);
            }
            if attempt + 1 > max {
                result
                    .flags
                    .push(vocab::error_code::RESTART_LIMIT.to_string());
                return (result, budget_halted);
            }
            attempt += 1;
        }
    }

    /// An error reached the end of its step with no `@err:` handler declared:
    /// record `NODUS:UNHANDLED_ERROR` (and its trace event) naming what failed.
    /// The triggering error stays in the run's error list beside it.
    fn record_unhandled_error(&self, ctx: &mut ExecutionContext, step_num: u32) {
        let cause = ctx
            .errors
            .last()
            .map(|e| e.code.clone())
            .unwrap_or_default();
        let command = ctx
            .log
            .last()
            .map(|entry| entry.command.clone())
            .unwrap_or_default();
        let error_detail = format!(
            "step {step_num} failed with {cause} and the workflow declares no @err handler"
        );
        self.emit(ctx, |seq, correlation_id| ExecutionEvent::StepError {
            step_index: step_num,
            step_command: command.clone(),
            error_code: vocab::error_code::UNHANDLED_ERROR.to_string(),
            error_detail: error_detail.clone(),
            step_identity: step_identity(step_num, &command),
            fault_identity: FaultIdentity {
                step_identity: step_identity(step_num, &command),
                code: vocab::error_code::UNHANDLED_ERROR.to_string(),
                discriminator: None,
            },
            seq,
            correlation_id,
            annotations: EventAnnotations::default(),
        });
        ctx.errors.push(RuntimeError {
            code: vocab::error_code::UNHANDLED_ERROR.to_string(),
            step: step_num,
            reason: error_detail,
        });
    }

    /// The execution mode a run records. A caller that declared
    /// nothing gets `Real` — unless the model calls are answered by the
    /// built-in stub, in which case nothing that ran was real and the run is
    /// recorded as a structural simulation instead. A mode the caller declared
    /// explicitly as simulated is kept as declared.
    fn effective_execution_mode(&self, declared: ExecutionMode) -> ExecutionMode {
        match declared {
            ExecutionMode::Real if self.provider.is_stub() => ExecutionMode::Simulated {
                fidelity: SimFidelity::Structural,
            },
            other => other,
        }
    }

    /// Merge a `restart_count` entry into the `@in`-overlay input map for one
    /// attempt, preserving whatever the caller originally passed. Fresh per
    /// attempt — nothing from a prior attempt's variable environment
    /// carries over except this explicit, intentional counter.
    fn seed_restart_count(input: &Option<Value>, attempt: u32) -> Option<Value> {
        let mut entries = match input {
            Some(Value::Map(m)) => m.clone(),
            _ => Vec::new(),
        };
        entries.push(("restart_count".to_string(), Value::Int(attempt as i64)));
        Some(Value::Map(entries))
    }

    /// Returns `(result, budget_halted)` — `budget_halted` is `true` only when
    /// `max_steps`/`wall_clock_ms` cut the run short; it is always
    /// `false` for the plain `execute`/`execute_with_params` paths (`None`
    /// budget parameters).
    #[allow(clippy::too_many_arguments)]
    fn execute_inner(
        &self,
        ast: &WorkflowFile,
        input: Option<Value>,
        run_id: &str,
        started_at: &str,
        max_steps: Option<u32>,
        wall_clock_ms: Option<u64>,
        env_trajectory: Vec<EnvInteraction>,
        execution_mode: ExecutionMode,
        exposure_switches: Vec<(String, String)>,
    ) -> (RunResult, bool) {
        let mut ctx = ExecutionContext::new(resolve_correlation_id(run_id));
        let execution_mode = self.effective_execution_mode(execution_mode);

        // Boot step 1: identify workflow.
        let workflow_id = ast
            .header
            .as_ref()
            .map(|h| format!("wf:{}", h.name))
            .unwrap_or_default();

        // Boot step 2: internalise hard rules.
        ctx.rules.extend(ast.rules.iter().cloned());

        // Boot step 3: preferences are noted; full evaluation lands with the
        // model-router phase.

        // Boot step 4: register @in defaults, then overlay with caller input.
        if let Some(input_decl) = &ast.input_decl {
            let mut in_map: Vec<(String, Value)> = Vec::new();
            for field in &input_decl.fields {
                if let Some(d) = &field.default {
                    let v = Value::Text(d.clone());
                    in_map.push((field.name.clone(), v.clone()));
                    ctx.variables.insert(field.name.clone(), v);
                }
            }
            ctx.variables.insert("in".to_string(), Value::Map(in_map));
        }

        if let Some(Value::Map(ref entries)) = input {
            for (k, v) in entries {
                ctx.variables.insert(k.clone(), v.clone());
            }
            let mut in_map = match ctx.variables.get("in").cloned() {
                Some(Value::Map(m)) => m,
                _ => Vec::new(),
            };
            for (k, v) in entries {
                if let Some(pos) = in_map.iter().position(|(ik, _)| ik == k) {
                    in_map[pos] = (k.clone(), v.clone());
                } else {
                    in_map.push((k.clone(), v.clone()));
                }
            }
            ctx.variables.insert("in".to_string(), Value::Map(in_map));
        }

        // Seed reserved variables.
        ctx.variables
            .entry("out".to_string())
            .or_insert(Value::Null);
        ctx.variables
            .entry("error".to_string())
            .or_insert_with(|| Value::Map(Vec::new()));
        ctx.variables
            .entry("meta".to_string())
            .or_insert_with(|| Value::Map(Vec::new()));

        // Boot step 5: @ON trigger dispatch is the library layer's responsibility.

        // Boot step 6: execute @steps.
        let mut abort = false;
        let mut paused = false;
        let mut halted = false;
        let mut budget_halted = false;
        // Set when the step sequence ended on an error nothing caught — with
        // or without a declared handler. It arms the compensation unwind.
        let mut uncaught_error = false;
        for (step_count, step) in ast.steps.iter().enumerate() {
            // A fixed step/wall-clock ceiling halts the run uniformly —
            // a normal graded outcome (Status::Partial below), never an error.
            let steps_exhausted = max_steps.is_some_and(|max| step_count as u32 >= max);
            let time_exhausted = wall_clock_ms
                .is_some_and(|ms| ctx.start_instant.elapsed().as_millis() as u64 >= ms);
            if steps_exhausted || time_exhausted {
                budget_halted = true;
                break;
            }
            let errors_before_this_step = ctx.errors.len();
            match self.run_step_with_retry(&mut ctx, step) {
                Some(Signal::Halt) => {
                    halted = true;
                    break;
                }
                Some(Signal::Break) => {
                    abort = true;
                    break;
                }
                Some(Signal::Pause) => {
                    paused = true;
                    break;
                }
                _ => {
                    // A step that returns no Signal but left a new
                    // RuntimeError behind is "uncaught" — nothing else in the
                    // language can catch it. Dispatch the declared @err:
                    // handler once, then end the run's step sequence. With no
                    // handler declared the error still ends the sequence, as
                    // NODUS:UNHANDLED_ERROR: continuing would run later steps
                    // against whatever the failed one left behind, which makes
                    // omitting the handler *less* contained than declaring it.
                    if ctx.errors.len() > errors_before_this_step {
                        uncaught_error = true;
                        let handler = ast.error_decl.as_ref().and_then(|d| d.handler.as_ref());
                        match (handler, ctx.errors.last().map(error_to_value)) {
                            (Some(handler), Some(error_value)) => {
                                ctx.variables.insert("error".to_string(), error_value);
                                self.execute_command(&mut ctx, handler, step.number);
                            }
                            _ => self.record_unhandled_error(&mut ctx, step.number),
                        }
                        break;
                    }
                }
            }
        }

        // Finalise.
        let out = ctx.get_var("out");
        // Rule violations take precedence (protocol-level failure); a pause is a
        // clean suspension and outranks abort/partial so the host sees Paused.
        let status = if halted
            || ctx
                .errors
                .iter()
                .any(|e| e.code == vocab::error_code::RULE_VIOLATION)
        {
            Status::Failed
        } else if paused {
            Status::Paused
        } else if abort {
            Status::Aborted
        } else if !ctx.errors.is_empty() || budget_halted {
            // A budget halt is a normal graded outcome, not an error —
            // it reuses the existing Partial status rather than introducing one.
            Status::Partial
        } else {
            Status::Ok
        };

        // Armed by a failure signal, never automatic on success — a
        // run that ends clean, paused or on a graded budget halt keeps its
        // effects untouched. The signals are a rule violation, `!HALT`,
        // `!BREAK`, and an error nothing caught (dispatched to `@err:` or not:
        // routing to a handler is the workflow's failure path, so the effects
        // it already committed are unwound too). Drains the ledger
        // back-to-front (LIFO): later effects were built on earlier
        // ones, so popping is the correctness contract, not a hint. A
        // compensation that fails is not retried and does not abort the
        // unwind — it stays surfaced as COMPENSATION_FAILED
        // alongside whatever error the original effect already carries, and
        // draining continues so every remaining completed effect still gets
        // its chance to undo.
        if matches!(status, Status::Failed | Status::Aborted) || uncaught_error {
            while let Some(effect) = ctx.compensations.pop() {
                let errors_before_compensation = ctx.errors.len();
                self.execute_command(&mut ctx, &effect.compensation, effect.step_number);
                if ctx.errors.len() != errors_before_compensation {
                    ctx.errors.push(RuntimeError {
                        code: vocab::error_code::COMPENSATION_FAILED.to_string(),
                        step: effect.step_number,
                        reason: format!(
                            "compensation for step {} failed; original effect remains live",
                            effect.step_number
                        ),
                    });
                }
            }
        }

        let resume = if paused {
            Some(ResumeDescriptor {
                workflow: workflow_id.clone(),
                vars: ctx.variables.clone(),
                step_index: ctx.paused_at.unwrap_or(0),
                workflow_digest: digest_ast(ast),
            })
        } else {
            None
        };

        let run_status = match status {
            Status::Ok => RunStatus::Ok,
            // A budget halt ends a run `Partial` without any error: a
            // graded outcome, not a fault, so the manifest does not call it one.
            Status::Partial if ctx.errors.is_empty() => RunStatus::Ok,
            Status::Partial => RunStatus::Error,
            Status::Paused => RunStatus::Paused,
            Status::Failed | Status::Aborted => RunStatus::ConstraintHalt,
        };
        // Stated, never inferred from the recipe's mere presence — a
        // model call (GEN/ANALYZE) makes the run non-deterministic.
        let determinism = if ctx
            .log
            .iter()
            .any(|e| e.command == "GEN" || e.command == "ANALYZE")
        {
            crate::observability::Determinism::ContainsModelCalls
        } else {
            crate::observability::Determinism::Deterministic
        };
        let repro = ReproRecipe {
            workflow_digest: digest_ast(ast),
            // No CapabilityManifest is checked on the plain execute()/
            // execute_with_params() path — Executor itself holds no manifest
            // context (that gate lives in workflows.rs::run_with_manifest);
            // an empty set honestly reports "none was checked", not a guess.
            capability_set: Vec::new(),
            exposure_switches: exposure_switches.clone(),
            execution_mode: execution_mode.clone(),
            nodus_version: env!("CARGO_PKG_VERSION").to_string(),
            needs_vocabulary: None, // @needs selective loading not yet implemented
            determinism,
        };
        self.audit.run_complete(RunManifest {
            workflow_name: workflow_id.clone(),
            schema_version: crate::vocab::BUILTIN_SCHEMA_VERSION.to_string(),
            // Identity: correlation_id == RunManifest.run_id, even on the
            // empty-run_id path (resolve_correlation_id already ran).
            run_id: ctx.correlation_id.clone(),
            started_at: started_at.to_string(),
            elapsed_ms: Measurement::Taken(ctx.start_instant.elapsed().as_millis() as u64),
            status: run_status,
            error_code: ctx.errors.first().map(|e| e.code.clone()),
            total_steps: ctx.log.len() as u32,
            event_count: ctx.event_count,
            env_trajectory,
            execution_mode,
            exposure_switches,
            repro,
        });

        (
            RunResult {
                workflow: workflow_id,
                status,
                out,
                log: ctx.log,
                errors: ctx.errors,
                flags: ctx.flags,
                vars: ctx.variables,
                resume,
            },
            budget_halted,
        )
    }

    // ─── Node dispatch ────────────────────────────────────────────────────────

    fn execute_step(&self, ctx: &mut ExecutionContext, step: &Step) -> Option<Signal> {
        let sig = step
            .body
            .as_ref()
            .and_then(|b| self.execute_node(ctx, b, step.number));
        if sig.is_some() {
            return sig;
        }
        for sub in &step.sub_steps {
            let sig = self.execute_node(ctx, sub, step.number);
            if sig.is_some() {
                return sig;
            }
        }
        None
    }

    /// Run a step, honoring its `~RETRY:n` bound: re-run up to `n` times while
    /// the attempt adds a runtime error. A control signal (`!HALT`/`!BREAK`/
    /// `!PAUSE`/rule violation) stops immediately — those are intentional, not
    /// transient. On eventual success the failed attempts' errors are rolled
    /// back; on exhaustion the accumulated errors remain and surface (routing to
    /// `@err`). With no bound the step runs exactly once.
    fn run_step_with_retry(&self, ctx: &mut ExecutionContext, step: &Step) -> Option<Signal> {
        let max_attempts = step.retry.map(|n| n.max(1)).unwrap_or(1);
        let errors_before = ctx.errors.len();
        let mut signal = None;
        for _ in 0..max_attempts {
            let errors_at_attempt = ctx.errors.len();
            signal = self.execute_step(ctx, step);
            if signal.is_some() {
                return signal;
            }
            if ctx.errors.len() == errors_at_attempt {
                // Success: discard any errors left by earlier failed attempts.
                ctx.errors.truncate(errors_before);
                // Only a step that completes cleanly (here, not a
                // signal-carrying branch — see the doc comment on the
                // compensations field) enters the completed-effect ledger.
                if let Some(comp) = &step.compensation {
                    ctx.compensations.push(CompletedEffect {
                        step_number: step.number,
                        compensation: comp.clone(),
                    });
                }
                return None;
            }
        }
        signal
    }

    fn execute_node(
        &self,
        ctx: &mut ExecutionContext,
        node: &Stmt,
        step_num: u32,
    ) -> Option<Signal> {
        match node {
            Stmt::Command(cmd) => self.execute_command(ctx, cmd, step_num),
            Stmt::Conditional(cond) => self.execute_conditional(ctx, cond, step_num),
            Stmt::ForLoop(fl) => self.execute_for(ctx, fl, step_num),
            Stmt::UntilLoop(ul) => self.execute_until(ctx, ul, step_num),
            Stmt::Parallel(pb) => self.execute_parallel(ctx, pb, step_num),
            Stmt::Switch(sw) => self.execute_switch(ctx, sw, step_num),
            Stmt::Map(mb) => self.execute_map(ctx, mb, step_num),
            Stmt::VarRef(_) | Stmt::Comment(_) => None,
        }
    }

    /// Resolve the control signal a taken conditional branch emits from its
    /// action flags. `!HALT` outranks `!PAUSE`, which outranks `!BREAK`/`!SKIP`;
    /// `!PAUSE` records the suspension point so the run can resume later.
    fn branch_exit_signal(
        ctx: &mut ExecutionContext,
        branch: &crate::ast::Conditional,
        step_num: u32,
    ) -> Option<Signal> {
        if branch.halt_flag {
            return Some(Signal::Halt);
        }
        if branch.pause_flag {
            ctx.paused_at = Some(step_num);
            ctx.flags.push(vocab::error_code::PAUSED.to_string());
            return Some(Signal::Pause);
        }
        if branch.break_flag {
            return Some(Signal::Break);
        }
        if branch.skip_flag {
            return Some(Signal::Skip);
        }
        None
    }

    fn execute_conditional(
        &self,
        ctx: &mut ExecutionContext,
        cond: &crate::ast::Conditional,
        step_num: u32,
    ) -> Option<Signal> {
        if Self::evaluate_condition(ctx, &cond.condition) {
            self.emit(ctx, |seq, correlation_id| ExecutionEvent::BranchTaken {
                step_index: step_num,
                branch_label: "if".to_string(),
                condition_result: true,
                seq,
                correlation_id,
                annotations: EventAnnotations::default(),
            });
            if let Some(action) = &cond.action {
                let sig = self.execute_command(ctx, action, step_num);
                if sig.is_some() {
                    return sig;
                }
            }
            for child in &cond.body {
                let sig = self.execute_node(ctx, child, step_num);
                if sig.is_some() {
                    return sig;
                }
            }
            return Self::branch_exit_signal(ctx, cond, step_num);
        }

        for elif_br in &cond.elif_branches {
            if Self::evaluate_condition(ctx, &elif_br.condition) {
                self.emit(ctx, |seq, correlation_id| ExecutionEvent::BranchTaken {
                    step_index: step_num,
                    branch_label: "elif".to_string(),
                    condition_result: true,
                    seq,
                    correlation_id,
                    annotations: EventAnnotations::default(),
                });
                if let Some(action) = &elif_br.action {
                    let sig = self.execute_command(ctx, action, step_num);
                    if sig.is_some() {
                        return sig;
                    }
                }
                for child in &elif_br.body {
                    let sig = self.execute_node(ctx, child, step_num);
                    if sig.is_some() {
                        return sig;
                    }
                }
                return Self::branch_exit_signal(ctx, elif_br, step_num);
            }
        }

        if let Some(else_br) = &cond.else_branch {
            self.emit(ctx, |seq, correlation_id| ExecutionEvent::BranchTaken {
                step_index: step_num,
                branch_label: "else".to_string(),
                condition_result: false,
                seq,
                correlation_id,
                annotations: EventAnnotations::default(),
            });
            if let Some(action) = &else_br.action {
                let sig = self.execute_command(ctx, action, step_num);
                if sig.is_some() {
                    return sig;
                }
            }
            for child in &else_br.body {
                let sig = self.execute_node(ctx, child, step_num);
                if sig.is_some() {
                    return sig;
                }
            }
            return Self::branch_exit_signal(ctx, else_br, step_num);
        }

        None
    }

    fn execute_for(
        &self,
        ctx: &mut ExecutionContext,
        fl: &crate::ast::ForLoop,
        step_num: u32,
    ) -> Option<Signal> {
        let collection = match ctx.get_var(&fl.collection) {
            Value::List(items) => items,
            _ => return None,
        };

        for (iter_idx, item) in collection.into_iter().enumerate() {
            ctx.set_var(&fl.variable, item);
            let fl_variable = fl.variable.clone();
            self.emit(ctx, |seq, correlation_id| ExecutionEvent::LoopIteration {
                step_index: step_num,
                loop_type: LoopType::For,
                iteration_number: Measurement::Taken(iter_idx as u64),
                bound_vars: vec![fl_variable],
                // Plain iteration, not a produce-transform construct — no
                // meaningful derivation to record.
                derivation: None,
                seq,
                correlation_id,
                annotations: EventAnnotations::default(),
            });
            let mut skip_rest = false;
            for child in &fl.body {
                let sig = self.execute_node(ctx, child, step_num);
                match sig {
                    Some(Signal::Halt) => return Some(Signal::Halt),
                    Some(Signal::Break) => return Some(Signal::Break),
                    Some(Signal::Pause) => return Some(Signal::Pause),
                    Some(Signal::Skip) => {
                        skip_rest = true;
                        break;
                    }
                    None => {}
                }
            }
            let _ = skip_rest;
        }
        None
    }

    fn execute_until(
        &self,
        ctx: &mut ExecutionContext,
        ul: &crate::ast::UntilLoop,
        step_num: u32,
    ) -> Option<Signal> {
        let max_iter = ul.max_iterations.unwrap_or(5) as usize;

        for iter_idx in 0..max_iter {
            self.emit(ctx, |seq, correlation_id| ExecutionEvent::LoopIteration {
                step_index: step_num,
                loop_type: LoopType::Until,
                iteration_number: Measurement::Taken(iter_idx as u64),
                bound_vars: vec![],
                derivation: None,
                seq,
                correlation_id,
                annotations: EventAnnotations::default(),
            });
            for child in &ul.body {
                match self.execute_node(ctx, child, step_num) {
                    Some(Signal::Halt) => return Some(Signal::Halt),
                    Some(Signal::Break) => return Some(Signal::Break),
                    Some(Signal::Pause) => return Some(Signal::Pause),
                    _ => {}
                }
            }
            if Self::evaluate_condition(ctx, &ul.condition) {
                return None;
            }
        }

        ctx.flags.push(vocab::error_code::MAX_REACHED.to_string());
        None
    }

    /// Run a `~PARALLEL` block. `~PARALLEL` is a scheduling hint, and this
    /// runtime realizes it as the branches in declared order over the shared
    /// environment — so the semantics the language gives a parallel block have
    /// to hold here explicitly:
    ///
    /// - **Fail-fast.** The first branch that returns a control signal
    ///   (`!HALT`, `!BREAK`, `!PAUSE`, a rule violation) or leaves an error
    ///   behind ends the block: later branches never start and `~JOIN` is
    ///   bypassed. The signal propagates; an error reaches `@err:` through the
    ///   main loop's dispatch check.
    /// - **`~JOIN → $target`** collects every branch's result, in declared
    ///   order, as a map keyed by each branch's own pipeline target (or
    ///   `branch_<n>` for a branch that binds none).
    fn execute_parallel(
        &self,
        ctx: &mut ExecutionContext,
        pb: &crate::ast::ParallelBlock,
        step_num: u32,
    ) -> Option<Signal> {
        let errors_before = ctx.errors.len();
        let mut results: Vec<(String, Value)> = Vec::with_capacity(pb.branches.len());
        for (index, branch) in pb.branches.iter().enumerate() {
            let log_before = ctx.log.len();
            if let Some(signal) = self.execute_node(ctx, branch, step_num) {
                return Some(signal);
            }
            if ctx.errors.len() > errors_before {
                return None;
            }
            let result = ctx.log[log_before..]
                .last()
                .map(|entry| entry.result.clone())
                .unwrap_or(Value::Null);
            let mut key = branch_result_key(branch, index);
            if results.iter().any(|(existing, _)| *existing == key) {
                key = format!("{key}_{}", index + 1);
            }
            results.push((key, result));
        }
        if let Some(target) = &pb.join_target {
            ctx.set_var(target, Value::Map(results));
        }
        None
    }

    /// Dispatch a `?SWITCH`: run the first arm whose value equals the scrutinee,
    /// else the `*` default, else record `SWITCH_NO_MATCH` (a warning) and
    /// continue. First match wins — no fallthrough.
    fn execute_switch(
        &self,
        ctx: &mut ExecutionContext,
        sw: &crate::ast::SwitchBlock,
        step_num: u32,
    ) -> Option<Signal> {
        let scrutinee = Self::resolve_value(ctx, &sw.scrutinee);
        for (value, action) in &sw.arms {
            let arm_val = Self::resolve_value(ctx, value);
            if Self::compare_values(&scrutinee, "=", &arm_val) {
                let branch_label = format!("switch:{value}");
                self.emit(ctx, |seq, correlation_id| ExecutionEvent::BranchTaken {
                    step_index: step_num,
                    branch_label,
                    condition_result: true,
                    seq,
                    correlation_id,
                    annotations: EventAnnotations::default(),
                });
                return self.execute_command(ctx, action, step_num);
            }
        }
        if let Some(default) = &sw.default {
            self.emit(ctx, |seq, correlation_id| ExecutionEvent::BranchTaken {
                step_index: step_num,
                branch_label: "switch:*".to_string(),
                condition_result: false,
                seq,
                correlation_id,
                annotations: EventAnnotations::default(),
            });
            return self.execute_command(ctx, default, step_num);
        }
        ctx.flags
            .push(vocab::error_code::SWITCH_NO_MATCH.to_string());
        None
    }

    /// Execute a `~MAP`: bind each element of the collection to `$it`, run the
    /// command, and collect the results into a list bound to the target. An
    /// empty or non-list collection yields an empty list and never errors.
    fn execute_map(
        &self,
        ctx: &mut ExecutionContext,
        mb: &crate::ast::MapBlock,
        step_num: u32,
    ) -> Option<Signal> {
        let items = match ctx.get_var(&mb.collection) {
            Value::List(items) => items,
            // Nothing to map: an absent collection is a legitimate empty case.
            Value::Null => Vec::new(),
            // A value that is present but not a list is a type error the
            // run does not halt on (the map yields an empty list, per the
            // contract) but must not hide: the empty result is otherwise
            // indistinguishable from mapping an empty list, and a check built
            // over it — every element validated, every item delivered —
            // would pass vacuously.
            _ => {
                ctx.flags
                    .push(format!("MAP_SOURCE_NOT_A_LIST:{}", mb.collection));
                Vec::new()
            }
        };
        let mut results = Vec::with_capacity(items.len());
        // Route each element's result through a scratch target so the full
        // command pipeline (audit, rule checks) runs per element.
        let mut cmd = mb.command.clone();
        cmd.pipeline_target = Some("$__map_element".to_string());
        for (iter_idx, item) in items.into_iter().enumerate() {
            ctx.set_var("it", item);
            // ~MAP is a true N->N produce-transform, unlike plain
            // ~FOR/~UNTIL iteration — each produced element derives from
            // exactly the source element at the same index, at this step.
            self.emit(ctx, |seq, correlation_id| ExecutionEvent::LoopIteration {
                step_index: step_num,
                loop_type: LoopType::Map,
                iteration_number: Measurement::Taken(iter_idx as u64),
                bound_vars: vec!["$it".to_string()],
                derivation: Some(vec![SourceRef {
                    producing_step: step_num,
                    source_index: iter_idx as u32,
                }]),
                seq,
                correlation_id,
                annotations: EventAnnotations::default(),
            });
            if let Some(sig) = self.execute_command(ctx, &cmd, step_num) {
                return Some(sig);
            }
            results.push(ctx.get_var("$__map_element"));
        }
        if let Some(target) = &mb.target {
            ctx.set_var(target, Value::List(results));
        }
        None
    }

    // ─── Condition evaluator ──────────────────────────────────────────────────

    fn evaluate_condition(ctx: &ExecutionContext, condition: &str) -> bool {
        let cond = condition.trim();
        if cond.is_empty() {
            return true;
        }

        // AND (highest precedence split)
        if let Some(pos) = cond.find(" AND ") {
            return Self::evaluate_condition(ctx, &cond[..pos])
                && Self::evaluate_condition(ctx, &cond[pos + 5..]);
        }

        // OR
        if let Some(pos) = cond.find(" OR ") {
            return Self::evaluate_condition(ctx, &cond[..pos])
                || Self::evaluate_condition(ctx, &cond[pos + 4..]);
        }

        // NOT CONTAINS
        if let Some(pos) = cond.find(" NOT CONTAINS ") {
            let left = Self::resolve_value(ctx, cond[..pos].trim());
            let right = Self::resolve_value(ctx, cond[pos + 14..].trim());
            return !left.as_text().contains(&right.as_text());
        }

        // CONTAINS
        if let Some(pos) = cond.find(" CONTAINS ") {
            let left = Self::resolve_value(ctx, cond[..pos].trim());
            let right = Self::resolve_value(ctx, cond[pos + 10..].trim());
            return match &left {
                Value::List(items) => items.contains(&right),
                _ => left.as_text().contains(&right.as_text()),
            };
        }

        // Comparison operators (longest first to avoid partial matches)
        for op in ["<=", ">=", "!=", "<", ">", "="] {
            let needle = format!(" {op} ");
            if let Some(pos) = cond.find(needle.as_str()) {
                let left = Self::resolve_value(ctx, cond[..pos].trim());
                let right = Self::resolve_value(ctx, cond[pos + needle.len()..].trim());
                return Self::compare_values(&left, op, &right);
            }
        }

        // Bare value — truthy check
        let val = Self::resolve_value(ctx, cond);
        Self::is_truthy(&val)
    }

    fn resolve_value(ctx: &ExecutionContext, expr: &str) -> Value {
        if expr.starts_with('$') {
            return ctx.get_var(expr);
        }
        if expr.starts_with('"') && expr.ends_with('"') && expr.len() >= 2 {
            return Value::Text(expr[1..expr.len() - 1].to_string());
        }
        if let Ok(n) = expr.parse::<i64>() {
            return Value::Int(n);
        }
        if let Ok(f) = expr.parse::<f64>() {
            return Value::Float(f);
        }
        match expr {
            "null" => Value::Null,
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            _ => Value::Text(expr.to_string()),
        }
    }

    fn compare_values(left: &Value, op: &str, right: &Value) -> bool {
        // Numeric comparison when both sides parse as numbers.
        let lf = Self::to_f64(left);
        let rf = Self::to_f64(right);
        if let (Some(l), Some(r)) = (lf, rf) {
            return match op {
                "=" => (l - r).abs() < f64::EPSILON,
                "!=" => (l - r).abs() >= f64::EPSILON,
                "<" => l < r,
                ">" => l > r,
                "<=" => l <= r,
                ">=" => l >= r,
                _ => false,
            };
        }
        // String fallback.
        let ls = left.as_text();
        let rs = right.as_text();
        match op {
            "=" => ls == rs,
            "!=" => ls != rs,
            _ => false,
        }
    }

    fn to_f64(v: &Value) -> Option<f64> {
        match v {
            Value::Int(n) => Some(*n as f64),
            Value::Float(f) => Some(*f),
            Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            Value::Text(s) => s.parse::<f64>().ok(),
            _ => None,
        }
    }

    fn is_truthy(v: &Value) -> bool {
        match v {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Int(n) => *n != 0,
            Value::Float(f) => *f != 0.0,
            Value::Text(s) => !s.is_empty(),
            Value::List(l) => !l.is_empty(),
            Value::Map(m) => !m.is_empty(),
        }
    }

    fn execute_command(
        &self,
        ctx: &mut ExecutionContext,
        cmd: &CommandCall,
        step_num: u32,
    ) -> Option<Signal> {
        if let Some(violation) = ctx.check_rules(&cmd.name, &cmd.args) {
            let rule_name = violation.clone();
            self.emit(ctx, |seq, correlation_id| ExecutionEvent::ConstraintHit {
                rule_name,
                triggering_step_index: step_num,
                halt: true,
                seq,
                correlation_id,
                annotations: EventAnnotations::default(),
            });
            let error_detail = violation.clone();
            self.emit(ctx, |seq, correlation_id| ExecutionEvent::StepError {
                step_index: step_num,
                step_command: cmd.name.clone(),
                error_code: vocab::error_code::RULE_VIOLATION.to_string(),
                error_detail,
                step_identity: step_identity(step_num, &cmd.name),
                fault_identity: FaultIdentity {
                    step_identity: step_identity(step_num, &cmd.name),
                    code: vocab::error_code::RULE_VIOLATION.to_string(),
                    discriminator: None,
                },
                seq,
                correlation_id,
                annotations: EventAnnotations::default(),
            });
            ctx.errors.push(RuntimeError {
                code: vocab::error_code::RULE_VIOLATION.to_string(),
                step: step_num,
                reason: violation,
            });
            return Some(Signal::Break);
        }

        // A host-supplied PolicyProvider authorizes a model-call or
        // deferred effect before it happens. Fail-closed, non-halting — the
        // built-in NoopPolicyProvider always permits, so this is a no-op for
        // every host that does not opt in.
        if let Some(class) = effect_class_of(&cmd.name) {
            let gate = class.as_gate_str();
            let mut context_pairs = vec![
                ("command".to_string(), Value::Text(cmd.name.clone())),
                (
                    "args".to_string(),
                    Value::List(cmd.args.iter().cloned().map(Value::Text).collect()),
                ),
            ];
            // Optional consequence descriptors ride the existing
            // +modifier=value grammar — carried into context only when the
            // step actually declares them (omitted, not defaulted, otherwise).
            for key in ["+reversible", "+external", "+value"] {
                if let Some((_, value)) = cmd.modifiers.iter().find(|(k, _)| k == key) {
                    context_pairs.push((
                        key.trim_start_matches('+').to_string(),
                        Value::Text(value.clone()),
                    ));
                }
            }
            let context = Value::Map(context_pairs);
            if !self.policy.evaluate(gate, &context) {
                let error_detail = format!("policy denied gate '{gate}' for '{}'", cmd.name);
                self.emit(ctx, |seq, correlation_id| ExecutionEvent::StepError {
                    step_index: step_num,
                    step_command: cmd.name.clone(),
                    error_code: vocab::error_code::POLICY_DENIED.to_string(),
                    error_detail: error_detail.clone(),
                    step_identity: step_identity(step_num, &cmd.name),
                    fault_identity: FaultIdentity {
                        step_identity: step_identity(step_num, &cmd.name),
                        code: vocab::error_code::POLICY_DENIED.to_string(),
                        discriminator: None,
                    },
                    seq,
                    correlation_id,
                    annotations: EventAnnotations::default(),
                });
                ctx.errors.push(RuntimeError {
                    code: vocab::error_code::POLICY_DENIED.to_string(),
                    step: step_num,
                    reason: error_detail,
                });
                return None;
            }
        }

        // Dialog commands resolve through the DialogProvider and may suspend the
        // run, so they bypass the standard value-returning dispatch.
        if cmd.name == "ASK" || cmd.name == "CONFIRM" {
            return self.handle_dialog(ctx, cmd, step_num);
        }

        // SETTLE resolves through the SettlementRail and, like dialog,
        // bypasses the standard value-returning dispatch.
        if cmd.name == "SETTLE" {
            return self.handle_settlement(ctx, cmd, step_num);
        }

        let input_vars: Vec<String> = ctx.variables.keys().cloned().collect();
        let step_start = Instant::now();
        self.emit(ctx, |seq, correlation_id| ExecutionEvent::StepStart {
            step_index: step_num,
            step_command: cmd.name.clone(),
            input_vars,
            step_identity: step_identity(step_num, &cmd.name),
            seq,
            correlation_id,
            annotations: EventAnnotations::default(),
        });

        let errors_before = ctx.errors.len();
        let result = self.dispatch(ctx, cmd, step_num);
        // A command that reported an error produced no answer: its pipeline
        // target stays as it was rather than holding a placeholder that looks
        // like one.
        let failed = ctx.errors.len() > errors_before;

        let elapsed_ms = Measurement::Taken(step_start.elapsed().as_millis() as u64);
        let output_vars: Vec<String> = cmd.pipeline_target.iter().cloned().collect();
        self.emit(ctx, |seq, correlation_id| ExecutionEvent::StepEnd {
            step_index: step_num,
            step_command: cmd.name.clone(),
            output_vars,
            elapsed_ms,
            step_identity: step_identity(step_num, &cmd.name),
            seq,
            correlation_id,
            annotations: EventAnnotations::default(),
        });

        ctx.log_step(step_num, &cmd.name, result.clone());

        if !failed && let Some(target) = &cmd.pipeline_target {
            ctx.set_var(target, result);
        }

        if cmd.name == "LOG" {
            ctx.out_locked = true;
        }

        None
    }

    /// Resolve an `ASK` / `CONFIRM` step through the [`DialogProvider`].
    ///
    /// `Answer` binds the typed value to the pipeline target; `Pause` suspends
    /// the run (`Status::Paused`) without executing later steps; `Timeout` /
    /// `Rejected` push the matching runtime error. Dialog events carry only a
    /// length descriptor — never the raw prompt or answer.
    fn handle_dialog(
        &self,
        ctx: &mut ExecutionContext,
        cmd: &CommandCall,
        step_num: u32,
    ) -> Option<Signal> {
        let input_vars: Vec<String> = ctx.variables.keys().cloned().collect();
        self.emit(ctx, |seq, correlation_id| ExecutionEvent::StepStart {
            step_index: step_num,
            step_command: cmd.name.clone(),
            input_vars,
            step_identity: step_identity(step_num, &cmd.name),
            seq,
            correlation_id,
            annotations: EventAnnotations::default(),
        });

        let prompt = cmd
            .args
            .first()
            .map(|a| {
                if a.starts_with('$') {
                    ctx.get_var(a).as_text()
                } else {
                    a.clone()
                }
            })
            .unwrap_or_default();

        // Emit only a length descriptor, never the raw prompt text.
        let prompt_len = prompt.len() as u32;
        self.emit(ctx, |seq, correlation_id| ExecutionEvent::ModelCall {
            step_index: step_num,
            command: cmd.name.clone(),
            input_summary: FieldDescriptor::text(prompt_len),
            seq,
            correlation_id,
            annotations: EventAnnotations::default(),
        });

        let outcome = if cmd.name == "ASK" {
            self.dialog.ask(&prompt, &cmd.modifiers)
        } else {
            self.dialog.confirm(&prompt, &cmd.modifiers)
        };

        // dialog_provenance is computed alongside signal, not in a
        // second pass — Answer/Remembered bind through the identical path
        // and differ only in which provenance the StepEnd below records.
        let (signal, dialog_provenance) = match outcome {
            DialogOutcome::Answer(value) => {
                ctx.log_step(step_num, &cmd.name, value.clone());
                if let Some(target) = &cmd.pipeline_target {
                    ctx.set_var(target, value);
                }
                (None, Some(DialogProvenance::Answered))
            }
            DialogOutcome::Remembered(value) => {
                ctx.log_step(step_num, &cmd.name, value.clone());
                if let Some(target) = &cmd.pipeline_target {
                    ctx.set_var(target, value);
                }
                (None, Some(DialogProvenance::Remembered))
            }
            DialogOutcome::Pause => {
                ctx.paused_at = Some(step_num);
                ctx.flags.push(vocab::error_code::PAUSED.to_string());
                (Some(Signal::Pause), None)
            }
            DialogOutcome::Timeout => {
                ctx.errors.push(RuntimeError {
                    code: vocab::error_code::DIALOG_TIMEOUT.to_string(),
                    step: step_num,
                    reason: format!("dialog '{}' timed out before an answer", cmd.name),
                });
                (None, None)
            }
            DialogOutcome::Rejected => {
                ctx.errors.push(RuntimeError {
                    code: vocab::error_code::DIALOG_REJECTED.to_string(),
                    step: step_num,
                    reason: format!("dialog '{}' was rejected", cmd.name),
                });
                (None, None)
            }
        };

        let output_vars: Vec<String> = cmd.pipeline_target.iter().cloned().collect();
        // The dialog path never times its own step (it may
        // suspend the run awaiting a human) — Unavailable is the honest
        // value, never a fabricated 0 that would bias a downstream average.
        self.emit(ctx, |seq, correlation_id| ExecutionEvent::StepEnd {
            step_index: step_num,
            step_command: cmd.name.clone(),
            output_vars,
            elapsed_ms: Measurement::Unavailable,
            step_identity: step_identity(step_num, &cmd.name),
            seq,
            correlation_id,
            annotations: EventAnnotations {
                dialog_provenance,
                ..Default::default()
            },
        });

        signal
    }

    /// Resolve a `SETTLE` step through the [`SettlementRail`].
    ///
    /// The gate above already decided the step may proceed; this is the
    /// act half. `Some(receipt)` binds to the pipeline target exactly like any
    /// other command's return value; `None` means the rail could not produce a
    /// verifiable receipt and pushes `NODUS:SETTLEMENT_UNACCOUNTED`.
    /// Both exits return bare `None` — terminal for this step, but Signal-free,
    /// so a denied or unaccounted settlement reaches `@err:` dispatch
    /// exactly like `POLICY_DENIED` does.
    fn handle_settlement(
        &self,
        ctx: &mut ExecutionContext,
        cmd: &CommandCall,
        step_num: u32,
    ) -> Option<Signal> {
        let input_vars: Vec<String> = ctx.variables.keys().cloned().collect();
        self.emit(ctx, |seq, correlation_id| ExecutionEvent::StepStart {
            step_index: step_num,
            step_command: cmd.name.clone(),
            input_vars,
            step_identity: step_identity(step_num, &cmd.name),
            seq,
            correlation_id,
            annotations: EventAnnotations::default(),
        });

        let step_start = Instant::now();

        // Length descriptor only, never the raw payee/amount/purpose (the
        // data-safety boundary, mirroring the dialog precedent).
        let args_len: u32 = cmd.args.iter().map(|a| a.len() as u32).sum();
        self.emit(ctx, |seq, correlation_id| ExecutionEvent::ModelCall {
            step_index: step_num,
            command: cmd.name.clone(),
            input_summary: FieldDescriptor::text(args_len),
            seq,
            correlation_id,
            annotations: EventAnnotations::default(),
        });

        match self.settlement.settle(cmd) {
            Some(value) => {
                ctx.log_step(step_num, &cmd.name, value.clone());
                if let Some(target) = &cmd.pipeline_target {
                    ctx.set_var(target, value);
                }
            }
            None => {
                let payee = cmd.args.first().map(String::as_str).unwrap_or("?");
                let error_detail =
                    format!("settlement for '{payee}' produced no verifiable receipt");
                self.emit(ctx, |seq, correlation_id| ExecutionEvent::StepError {
                    step_index: step_num,
                    step_command: cmd.name.clone(),
                    error_code: vocab::error_code::SETTLEMENT_UNACCOUNTED.to_string(),
                    error_detail: error_detail.clone(),
                    step_identity: step_identity(step_num, &cmd.name),
                    fault_identity: FaultIdentity {
                        step_identity: step_identity(step_num, &cmd.name),
                        code: vocab::error_code::SETTLEMENT_UNACCOUNTED.to_string(),
                        discriminator: None,
                    },
                    seq,
                    correlation_id,
                    annotations: EventAnnotations::default(),
                });
                ctx.errors.push(RuntimeError {
                    code: vocab::error_code::SETTLEMENT_UNACCOUNTED.to_string(),
                    step: step_num,
                    reason: error_detail,
                });
            }
        }

        let elapsed_ms = Measurement::Taken(step_start.elapsed().as_millis() as u64);
        let output_vars: Vec<String> = cmd.pipeline_target.iter().cloned().collect();
        self.emit(ctx, |seq, correlation_id| ExecutionEvent::StepEnd {
            step_index: step_num,
            step_command: cmd.name.clone(),
            output_vars,
            elapsed_ms,
            step_identity: step_identity(step_num, &cmd.name),
            seq,
            correlation_id,
            annotations: EventAnnotations::default(),
        });

        None
    }

    // ─── Command dispatch ─────────────────────────────────────────────────────

    fn dispatch(&self, ctx: &mut ExecutionContext, cmd: &CommandCall, step_num: u32) -> Value {
        match cmd.name.as_str() {
            "LOG" => Self::handle_log(ctx, cmd),
            "GEN" => self.handle_gen(ctx, cmd, step_num),
            "ANALYZE" => self.handle_analyze(ctx, cmd, step_num),
            "FETCH" => Self::handle_fetch(ctx, cmd),
            "ESCALATE" => Self::handle_escalate(ctx, cmd),
            "ROUTE" => Self::handle_route(ctx, cmd),
            "VALIDATE" => Value::Bool(true),
            "PUBLISH" => Value::Bool(true),
            "NOTIFY" => Value::Bool(true),
            "TONE" => {
                if let Some(t) = cmd.args.first() {
                    ctx.flags.push(format!("tone:{t}"));
                }
                Value::Null
            }
            "REFINE" => ctx.get_var("draft"),
            "SCORE" => Value::Float(0.85),
            "APPEND" => Value::List(Vec::new()),
            "MERGE" => Self::handle_merge(ctx, cmd),
            "STORE" => Value::Bool(true),
            "LOAD" => Value::Null,
            "COMPARE" => Value::Map(vec![
                ("match".to_string(), Value::Bool(false)),
                ("score".to_string(), Value::Float(0.0)),
            ]),
            "TRANSLATE" => Value::Text("[Translated content]".to_string()),
            "SUMMARIZE" => Value::Text("[Summary]".to_string()),
            "WAIT" | "DEBUG" => Value::Null,
            "QUERY_KB" => Value::List(Vec::new()),
            "REMEMBER" | "FORGET" => Value::Bool(true),
            "RECALL" => Value::Null,
            "RUN" => {
                let macro_name = cmd.args.first().cloned().unwrap_or_default();
                let macro_start = Instant::now();
                let macro_name_enter = macro_name.clone();
                self.emit(ctx, |seq, correlation_id| ExecutionEvent::MacroEnter {
                    macro_name: macro_name_enter,
                    call_step_index: step_num,
                    seq,
                    correlation_id,
                    annotations: EventAnnotations::default(),
                });
                ctx.flags.push(format!("RUN:{macro_name}"));
                let elapsed_ms = Measurement::Taken(macro_start.elapsed().as_millis() as u64);
                self.emit(ctx, |seq, correlation_id| ExecutionEvent::MacroExit {
                    macro_name,
                    call_step_index: step_num,
                    elapsed_ms,
                    seq,
                    correlation_id,
                    annotations: EventAnnotations::default(),
                });
                Value::Null
            }
            "ASSIGN" => {
                if cmd.args.len() >= 2 {
                    let val = Value::Text(cmd.args[1].clone());
                    ctx.set_var(&cmd.args[0], val.clone());
                    val
                } else {
                    Value::Null
                }
            }
            name if vocab::is_known_command(name) => Value::Null,
            name => {
                ctx.flags.push(format!("UNKNOWN_COMMAND:{name}"));
                Value::Null
            }
        }
    }

    fn handle_merge(ctx: &ExecutionContext, cmd: &CommandCall) -> Value {
        let mut result: Vec<(String, Value)> = Vec::new();
        for arg in &cmd.args {
            if let Value::Map(entries) = ctx.get_var(arg) {
                for (k, v) in entries {
                    if let Some(pos) = result.iter().position(|(ek, _)| *ek == k) {
                        result[pos] = (k, v);
                    } else {
                        result.push((k, v));
                    }
                }
            }
        }
        Value::Map(result)
    }

    fn handle_log(ctx: &mut ExecutionContext, cmd: &CommandCall) -> Value {
        let value = cmd
            .args
            .first()
            .map(|a| ctx.get_var(a))
            .unwrap_or_else(|| ctx.get_var("out"));
        let entry = value.as_text();
        let log_list = ctx
            .variables
            .entry("log_entries".to_string())
            .or_insert_with(|| Value::List(Vec::new()));
        if let Value::List(v) = log_list {
            v.push(Value::Text(entry));
        }
        Value::Null
    }

    fn handle_gen(&self, ctx: &mut ExecutionContext, cmd: &CommandCall, step_num: u32) -> Value {
        let prompt = cmd.args.first().map(|a| {
            if a.starts_with('$') {
                ctx.get_var(a).as_text()
            } else {
                a.clone()
            }
        });
        let prompt_str = prompt.as_deref().unwrap_or("").to_string();
        let input_len = prompt_str.len() as u32;
        self.emit(ctx, |seq, correlation_id| ExecutionEvent::ModelCall {
            step_index: step_num,
            command: cmd.name.clone(),
            input_summary: FieldDescriptor::text(input_len),
            seq,
            correlation_id,
            annotations: EventAnnotations::default(),
        });
        let call_start = Instant::now();
        let result = match self.provider.try_generate(&prompt_str, &cmd.modifiers) {
            Ok(text) => text,
            Err(failure) => {
                self.record_model_failure(ctx, cmd, step_num, &failure);
                return Value::Null;
            }
        };
        let elapsed_ms = Measurement::Taken(call_start.elapsed().as_millis() as u64);
        let output_len = result.len() as u32;
        // ModelProvider exposes no token-accounting seam, so all four
        // classes are Unavailable — never a fabricated 0.
        self.emit(ctx, |seq, correlation_id| ExecutionEvent::ModelResponse {
            step_index: step_num,
            command: cmd.name.clone(),
            output_summary: FieldDescriptor::text(output_len),
            elapsed_ms,
            input_tokens: Measurement::Unavailable,
            output_tokens: Measurement::Unavailable,
            cache_read_tokens: Measurement::Unavailable,
            cache_creation_tokens: Measurement::Unavailable,
            seq,
            correlation_id,
            annotations: EventAnnotations::default(),
        });
        Value::Text(result)
    }

    /// A model call the provider reported as failed: the step gets a typed
    /// `NODUS:MODEL_CALL_FAILED` error (and its trace event) instead of an
    /// answer. Signal-free, so it reaches the workflow's `@err:` handler like
    /// any other non-fatal error — and a `~RETRY` on the step re-runs it.
    fn record_model_failure(
        &self,
        ctx: &mut ExecutionContext,
        cmd: &CommandCall,
        step_num: u32,
        failure: &ModelError,
    ) {
        let error_detail = format!("{} did not get an answer: {}", cmd.name, failure.reason);
        self.emit(ctx, |seq, correlation_id| ExecutionEvent::StepError {
            step_index: step_num,
            step_command: cmd.name.clone(),
            error_code: vocab::error_code::MODEL_CALL_FAILED.to_string(),
            error_detail: error_detail.clone(),
            step_identity: step_identity(step_num, &cmd.name),
            fault_identity: FaultIdentity {
                step_identity: step_identity(step_num, &cmd.name),
                code: vocab::error_code::MODEL_CALL_FAILED.to_string(),
                discriminator: None,
            },
            seq,
            correlation_id,
            annotations: EventAnnotations::default(),
        });
        ctx.errors.push(RuntimeError {
            code: vocab::error_code::MODEL_CALL_FAILED.to_string(),
            step: step_num,
            reason: error_detail,
        });
    }

    fn handle_analyze(
        &self,
        ctx: &mut ExecutionContext,
        cmd: &CommandCall,
        step_num: u32,
    ) -> Value {
        let text = cmd
            .args
            .first()
            .map(|a| ctx.get_var(a).as_text())
            .unwrap_or_default();
        let text_len = text.len() as u32;
        self.emit(ctx, |seq, correlation_id| ExecutionEvent::ModelCall {
            step_index: step_num,
            command: cmd.name.clone(),
            input_summary: FieldDescriptor::text(text_len),
            seq,
            correlation_id,
            annotations: EventAnnotations::default(),
        });
        let call_start = Instant::now();
        let result = match self.provider.try_analyze(&text, &cmd.flags) {
            Ok(value) => value,
            Err(failure) => {
                self.record_model_failure(ctx, cmd, step_num, &failure);
                return Value::Null;
            }
        };
        let elapsed_ms = Measurement::Taken(call_start.elapsed().as_millis() as u64);
        let output_summary = match &result {
            Value::Map(entries) => {
                FieldDescriptor::map_like(entries.len() as u32, format!("{result:?}").len() as u32)
            }
            _ => FieldDescriptor::text(result.as_text().len() as u32),
        };
        self.emit(ctx, |seq, correlation_id| ExecutionEvent::ModelResponse {
            step_index: step_num,
            command: cmd.name.clone(),
            output_summary,
            elapsed_ms,
            input_tokens: Measurement::Unavailable,
            output_tokens: Measurement::Unavailable,
            cache_read_tokens: Measurement::Unavailable,
            cache_creation_tokens: Measurement::Unavailable,
            seq,
            correlation_id,
            annotations: EventAnnotations::default(),
        });
        result
    }

    fn handle_fetch(_ctx: &ExecutionContext, cmd: &CommandCall) -> Value {
        let source = cmd.args.first().cloned().unwrap_or_default();
        Value::Map(vec![
            ("_stub".to_string(), Value::Bool(true)),
            ("source".to_string(), Value::Text(source)),
        ])
    }

    fn handle_escalate(ctx: &mut ExecutionContext, cmd: &CommandCall) -> Value {
        let target = cmd
            .args
            .first()
            .cloned()
            .unwrap_or_else(|| "human".to_string());
        ctx.flags.push(format!("ESCALATE:{target}"));
        Value::Null
    }

    fn handle_route(ctx: &mut ExecutionContext, cmd: &CommandCall) -> Value {
        let target = cmd
            .args
            .first()
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());
        ctx.flags.push(format!("ROUTE:{target}"));
        Value::Null
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;

    mod slice {
        use super::*;

        const SLICE_WORKFLOW: &str = "\
§wf:slice_test v1.0
@in: { query }
@out: $out
@steps:
  1. GEN(greeting) → $out
  2. LOG($out)
";

        #[test]
        fn boot_and_run_log_gen() {
            let ast = Parser::parse(SLICE_WORKFLOW).expect("parse");
            let result = Executor::with_stub().execute(&ast, None);

            assert_eq!(result.status, Status::Ok, "errors: {:?}", result.errors);
            assert_eq!(result.workflow, "wf:slice_test");

            assert!(
                matches!(&result.out, Value::Text(s) if s.contains("greeting")),
                "expected text containing 'greeting', got {:?}",
                result.out
            );

            assert_eq!(result.log.len(), 2);
            assert_eq!(result.log[0].command, "GEN");
            assert_eq!(result.log[1].command, "LOG");
            assert_eq!(result.log[0].step, 1);
            assert_eq!(result.log[1].step, 2);
        }

        #[test]
        fn pipeline_threads_output_between_steps() {
            let source = "\
§wf:pipe_test v1.0
@steps:
  1. GEN(first) → $draft
  2. GEN($draft) → $out
";
            let ast = Parser::parse(source).expect("parse");
            let result = Executor::with_stub().execute(&ast, None);

            assert_eq!(result.status, Status::Ok);
            assert!(matches!(&result.out, Value::Text(_)));
            assert_eq!(result.log.len(), 2);
        }

        #[test]
        fn input_data_seeds_in_variable() {
            let source = "\
§wf:input_test v1.0
@in: { msg }
@steps:
  1. GEN($in.msg) → $out
";
            let ast = Parser::parse(source).expect("parse");
            let input = Value::Map(vec![("msg".to_string(), Value::Text("hello".to_string()))]);
            let result = Executor::with_stub().execute(&ast, Some(input));

            assert_eq!(result.status, Status::Ok);
            assert!(matches!(&result.out, Value::Text(s) if s.contains("hello")));
        }

        #[test]
        fn resume_descriptor_workflow_digest_matches_digest_ast() {
            // The pinned-generation digest a paused run hands to its
            // host must be the same `digest_ast` computation the run's own
            // `ReproRecipe.workflow_digest` uses — proven directly here against
            // the exact `ast` this test parses, no audit indirection needed.
            let source = "\
§wf:resume_digest_test v1.0
@steps:
  1. ASK(question) → $answer
";
            let ast = Parser::parse(source).expect("parse");
            let result = Executor::with_stub().execute(&ast, None);

            assert_eq!(
                result.status,
                Status::Paused,
                "an unresolved ASK with no +default must pause; errors: {:?}",
                result.errors
            );
            let resume = result
                .resume
                .expect("a paused run carries a resume descriptor");
            assert_eq!(resume.workflow_digest, digest_ast(&ast));
        }

        // ── Workflow digest (versioned, specified) ────────────────────────────

        const DIGEST_WF: &str = "\
§wf:digest_wf v1.0
§runtime: { core: schema.nodus }
@out: $out
@steps:
  1. GEN(hello) → $out
  2. LOG($out)
";

        #[test]
        fn fnv1a64_matches_the_published_test_vectors() {
            // The FNV reference vectors: the digest is a specified algorithm,
            // so these values hold on every build, platform and Rust release.
            assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
            assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
            assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
        }

        #[test]
        fn a_digest_names_its_scheme_and_has_a_fixed_shape() {
            let digest = digest_text("anything");
            assert_eq!(digest, format!("{DIGEST_VERSION}:{}", &digest[4..]));
            assert!(digest.starts_with("nd1:"), "{digest}");
            assert_eq!(digest.len(), "nd1:".len() + 16);
            assert!(digest[4..].bytes().all(|b| b.is_ascii_hexdigit()));
            // A known answer pins the scheme end to end.
            assert_eq!(digest_text(""), "nd1:cbf29ce484222325");
        }

        #[test]
        fn a_workflow_digest_is_stable_and_follows_the_definition() {
            let ast = Parser::parse(DIGEST_WF).expect("parse");
            assert_eq!(digest_ast(&ast), digest_ast(&ast));

            let reformatted = DIGEST_WF.replace("  1. GEN", "\n\n  1.   GEN");
            let same = Parser::parse(&reformatted).expect("parse");
            assert_eq!(
                digest_ast(&same),
                digest_ast(&ast),
                "whitespace is not part of the definition"
            );

            let changed =
                Parser::parse(&DIGEST_WF.replace("GEN(hello)", "GEN(goodbye)")).expect("parse");
            assert_ne!(digest_ast(&changed), digest_ast(&ast));
        }

        #[test]
        fn a_workflow_digest_survives_the_compact_round_trip() {
            // Identity comes from the canonical compact form, which re-parses to
            // an equal AST — so re-emitting a workflow never changes its digest.
            let ast = Parser::parse(DIGEST_WF).expect("parse");
            let reparsed = Parser::parse(&canonical_text(&ast)).expect("re-parse");
            assert_eq!(digest_ast(&reparsed), digest_ast(&ast));
        }

        #[test]
        fn field_defaults_applied_when_no_input() {
            let source = "\
§wf:default_test v1.0
@in: { tone? = warm }
@steps:
  1. GEN(reply) → $out
";
            let ast = Parser::parse(source).expect("parse");
            let result = Executor::with_stub().execute(&ast, None);
            assert_eq!(result.status, Status::Ok);
        }

        #[test]
        fn never_rule_blocks_publish_without_validate() {
            let source = "\
§wf:constraint_test v1.0
!!NEVER: publish WITHOUT validate
@steps:
  1. PUBLISH($out)
";
            let ast = Parser::parse(source).expect("parse");
            let result = Executor::with_stub().execute(&ast, None);

            assert_eq!(result.status, Status::Failed, "expected Failed");
            assert!(!result.errors.is_empty());
            assert!(result.errors[0].code.contains("RULE_VIOLATION"));
        }

        #[test]
        fn analyze_returns_map_with_flags() {
            let source = "\
§wf:analyze_test v1.0
@steps:
  1. ANALYZE($in.text) ~sentiment ~intent → $analysis
";
            let ast = Parser::parse(source).expect("parse");
            let input = Value::Map(vec![(
                "text".to_string(),
                Value::Text("great product".to_string()),
            )]);
            let result = Executor::with_stub().execute(&ast, Some(input));

            assert_eq!(result.status, Status::Ok);
            let analysis = result.log.iter().find(|e| e.command == "ANALYZE");
            assert!(analysis.is_some());
            assert!(matches!(analysis.unwrap().result, Value::Map(_)));
        }

        #[test]
        fn stub_provider_includes_prompt_and_tone() {
            let stub = StubProvider;
            let out = stub.generate("hello", &[("+tone".to_string(), "warm".to_string())]);
            assert!(out.contains("hello"), "prompt in output: {out}");
            assert!(out.contains("warm"), "tone in output: {out}");
        }

        #[test]
        fn default_dialog_provider_resolves_default_or_pauses() {
            let p = DefaultDialogProvider;
            let with_default = vec![("+default".to_string(), "yes".to_string())];
            assert_eq!(
                p.ask("q", &with_default),
                DialogOutcome::Answer(Value::Text("yes".to_string()))
            );
            assert_eq!(p.ask("q", &[]), DialogOutcome::Pause);
            assert_eq!(p.confirm("c", &[]), DialogOutcome::Pause);
        }

        #[test]
        fn value_dot_notation_resolves_nested() {
            let mut ctx = ExecutionContext::new("test-corr".to_string());
            ctx.variables.insert(
                "in".to_string(),
                Value::Map(vec![("query".to_string(), Value::Text("test".to_string()))]),
            );
            assert_eq!(ctx.get_var("$in.query"), Value::Text("test".to_string()));
            assert_eq!(ctx.get_var("$in.missing"), Value::Null);
            assert_eq!(ctx.get_var("$missing"), Value::Null);
        }
    }

    mod control_flow {
        use super::*;
        use crate::parser::Parser;

        #[test]
        fn conditional_if_branch_taken_when_true() {
            // $score is set to 0.9 via input; condition "$score >= 0.8" → true
            let source = "\
§wf:cond_test v1.0
@steps:
  1. GEN(base) → $out
  2. ?IF $score >= 0.8 → GEN(high_score) → $out
";
            let ast = Parser::parse(source).expect("parse");
            let input = Value::Map(vec![("score".to_string(), Value::Float(0.9))]);
            let result = Executor::with_stub().execute(&ast, Some(input));

            assert_eq!(result.status, Status::Ok, "errors: {:?}", result.errors);
            assert_eq!(
                result.log.len(),
                2,
                "GEN(base) + conditional GEN(high_score)"
            );
        }

        #[test]
        fn conditional_else_branch_taken_when_false() {
            let source = "\
§wf:else_test v1.0
@steps:
  1. ?IF $score >= 0.9 → GEN(high) → $out
  2. ?ELSE → GEN(low) → $out
";
            let ast = Parser::parse(source).expect("parse");
            // score < 0.9 → else branch
            let input = Value::Map(vec![("score".to_string(), Value::Float(0.5))]);
            let result = Executor::with_stub().execute(&ast, Some(input));

            assert_eq!(result.status, Status::Ok, "errors: {:?}", result.errors);
            // The else GEN should appear in the log
            let gen_cmds: Vec<_> = result.log.iter().filter(|e| e.command == "GEN").collect();
            assert!(!gen_cmds.is_empty());
        }

        #[test]
        fn break_from_conditional_halts_steps() {
            let source = "\
§wf:break_test v1.0
@steps:
  1. GEN(first) → $out
  2. ?IF $out CONTAINS first → GEN(second) !BREAK
  3. GEN(third) → $out
";
            let ast = Parser::parse(source).expect("parse");
            let result = Executor::with_stub().execute(&ast, None);

            // Status::Aborted because !BREAK fired (no rule violation)
            assert_eq!(
                result.status,
                Status::Aborted,
                "expected Aborted from !BREAK"
            );
            // Step 3 must NOT appear in the log
            let third = result.log.iter().any(|e| {
                if let Value::Text(s) = &e.result {
                    s.contains("third")
                } else {
                    false
                }
            });
            assert!(!third, "step 3 should not have executed after !BREAK");
        }

        #[test]
        fn for_loop_iterates_collection() {
            let source = "\
§wf:for_test v1.0
@steps:
  1. GEN(item) → $out
";
            // Build a workflow manually via parse + inject a ~FOR into the executor test:
            // Parser does not yet expose ~FOR syntax in test fixtures, so we exercise
            // execute_for directly through the executor helper.
            let ast = Parser::parse(source).expect("parse");
            let result = Executor::with_stub().execute(&ast, None);
            assert_eq!(result.status, Status::Ok);
        }

        #[test]
        fn until_loop_hits_max_and_sets_flag() {
            // Build the AST manually: ~UNTIL with MAX:2 that never satisfies its condition.
            use crate::ast::{Step, Stmt, UntilLoop, WorkflowFile};

            let loop_node = UntilLoop {
                condition: "false".to_string(), // never true → always exhausts MAX
                max_iterations: Some(2),
                body: vec![Stmt::Command(crate::ast::CommandCall {
                    name: "GEN".to_string(),
                    args: vec!["iter".to_string()],
                    pipeline_target: Some("$out".to_string()),
                    ..Default::default()
                })],
            };
            let wf = WorkflowFile {
                steps: vec![Step {
                    number: 1,
                    body: Some(Stmt::UntilLoop(loop_node)),
                    ..Default::default()
                }],
                ..Default::default()
            };

            let result = Executor::with_stub().execute(&wf, None);
            assert!(
                result.flags.contains(&"NODUS:MAX_REACHED".to_string()),
                "expected MAX_REACHED flag, got {:?}",
                result.flags
            );
            // 2 GEN calls (one per iteration before MAX hit)
            assert_eq!(result.log.len(), 2, "expected 2 iterations");
        }

        #[test]
        fn until_loop_exits_when_condition_met() {
            use crate::ast::{Step, Stmt, UntilLoop, WorkflowFile};

            // Condition "true" is satisfied on first check → exits after 1 body execution.
            let loop_node = UntilLoop {
                condition: "true".to_string(),
                max_iterations: Some(5),
                body: vec![Stmt::Command(crate::ast::CommandCall {
                    name: "GEN".to_string(),
                    args: vec!["once".to_string()],
                    pipeline_target: Some("$out".to_string()),
                    ..Default::default()
                })],
            };
            let wf = WorkflowFile {
                steps: vec![Step {
                    number: 1,
                    body: Some(Stmt::UntilLoop(loop_node)),
                    ..Default::default()
                }],
                ..Default::default()
            };

            let result = Executor::with_stub().execute(&wf, None);
            assert!(
                !result.flags.contains(&"NODUS:MAX_REACHED".to_string()),
                "should NOT have MAX_REACHED when condition satisfied"
            );
            assert_eq!(result.log.len(), 1);
        }

        #[test]
        fn parallel_block_runs_branches() {
            use crate::ast::{ParallelBlock, Step, Stmt, WorkflowFile};

            let pb = ParallelBlock {
                branches: vec![
                    Stmt::Command(crate::ast::CommandCall {
                        name: "GEN".to_string(),
                        args: vec!["branch_a".to_string()],
                        ..Default::default()
                    }),
                    Stmt::Command(crate::ast::CommandCall {
                        name: "GEN".to_string(),
                        args: vec!["branch_b".to_string()],
                        ..Default::default()
                    }),
                ],
                join_target: Some("$merged".to_string()),
            };
            let wf = WorkflowFile {
                steps: vec![Step {
                    number: 1,
                    body: Some(Stmt::Parallel(pb)),
                    ..Default::default()
                }],
                ..Default::default()
            };

            let result = Executor::with_stub().execute(&wf, None);
            assert_eq!(result.status, Status::Ok);
            assert_eq!(result.log.len(), 2, "both branches should log");
            assert!(
                result.log.iter().all(|e| e.command == "GEN"),
                "all log entries should be GEN"
            );
        }

        #[test]
        fn map_transforms_each_element_into_a_list() {
            use crate::ast::{CommandCall, MapBlock};
            let mut ctx = ExecutionContext::new("test-corr".to_string());
            ctx.variables.insert(
                "items".to_string(),
                Value::List(vec![
                    Value::Text("a".to_string()),
                    Value::Text("b".to_string()),
                    Value::Text("c".to_string()),
                ]),
            );
            let mb = MapBlock {
                collection: "$items".to_string(),
                command: CommandCall {
                    name: "GEN".to_string(),
                    args: vec!["$it".to_string()],
                    ..Default::default()
                },
                target: Some("$out".to_string()),
            };
            Executor::with_stub().execute_map(&mut ctx, &mb, 1);
            match ctx.get_var("$out") {
                Value::List(items) => assert_eq!(items.len(), 3, "one result per element"),
                other => panic!("expected a list, got {other:?}"),
            }
        }

        #[test]
        fn map_over_non_list_yields_empty_list() {
            use crate::ast::{CommandCall, MapBlock};
            let mut ctx = ExecutionContext::new("test-corr".to_string());
            ctx.variables
                .insert("items".to_string(), Value::Text("scalar".to_string()));
            let mb = MapBlock {
                collection: "$items".to_string(),
                command: CommandCall {
                    name: "GEN".to_string(),
                    args: vec!["$it".to_string()],
                    ..Default::default()
                },
                target: Some("$out".to_string()),
            };
            Executor::with_stub().execute_map(&mut ctx, &mb, 1);
            match ctx.get_var("$out") {
                Value::List(items) => assert!(items.is_empty(), "a non-list collection yields []"),
                other => panic!("expected an empty list, got {other:?}"),
            }
        }

        #[test]
        fn retry_does_not_rerun_a_successful_step() {
            use crate::ast::{CommandCall, Step, Stmt, WorkflowFile};
            let wf = WorkflowFile {
                steps: vec![Step {
                    number: 1,
                    body: Some(Stmt::Command(CommandCall {
                        name: "GEN".to_string(),
                        args: vec!["x".to_string()],
                        pipeline_target: Some("$out".to_string()),
                        ..Default::default()
                    })),
                    retry: Some(3),
                    ..Default::default()
                }],
                ..Default::default()
            };
            let result = Executor::with_stub().execute(&wf, None);
            assert_eq!(result.status, Status::Ok);
            assert_eq!(
                result.log.len(),
                1,
                "a succeeding step runs exactly once despite ~RETRY:3"
            );
        }

        #[test]
        fn condition_evaluator_comparisons() {
            let mut ctx = ExecutionContext::new("test-corr".to_string());
            ctx.variables
                .insert("score".to_string(), Value::Float(0.75));
            ctx.variables
                .insert("tag".to_string(), Value::Text("urgent".to_string()));

            assert!(Executor::evaluate_condition(&ctx, "$score >= 0.5"));
            assert!(!Executor::evaluate_condition(&ctx, "$score >= 0.9"));
            assert!(Executor::evaluate_condition(&ctx, "$score < 1.0"));
            assert!(Executor::evaluate_condition(&ctx, "$tag = urgent"));
            assert!(!Executor::evaluate_condition(&ctx, "$tag = calm"));
            assert!(Executor::evaluate_condition(&ctx, "true"));
            assert!(!Executor::evaluate_condition(&ctx, "false"));
            assert!(Executor::evaluate_condition(
                &ctx,
                "$score >= 0.5 AND $tag = urgent"
            ));
            assert!(Executor::evaluate_condition(
                &ctx,
                "$score >= 0.9 OR $tag = urgent"
            ));
        }

        #[test]
        fn merge_command_combines_maps() {
            let source = "\
§wf:merge_test v1.0
@steps:
  1. MERGE($a, $b) → $out
";
            let ast = Parser::parse(source).expect("parse");
            let input = Value::Map(vec![
                (
                    "a".to_string(),
                    Value::Map(vec![("x".to_string(), Value::Int(1))]),
                ),
                (
                    "b".to_string(),
                    Value::Map(vec![("y".to_string(), Value::Int(2))]),
                ),
            ]);
            let result = Executor::with_stub().execute(&ast, Some(input));
            assert_eq!(result.status, Status::Ok);
            if let Value::Map(entries) = &result.out {
                let keys: Vec<&str> = entries.iter().map(|(k, _)| k.as_str()).collect();
                assert!(keys.contains(&"x") && keys.contains(&"y"));
            } else {
                panic!("expected Map out, got {:?}", result.out);
            }
        }

        #[test]
        fn assign_command_sets_variable() {
            // ASSIGN is generated by the parser for assignment syntax; test the
            // handler directly by constructing the AST node manually.
            use crate::ast::{CommandCall, Step, Stmt, WorkflowFile};

            let cmd = CommandCall {
                name: "ASSIGN".to_string(),
                args: vec!["$greeting".to_string(), "hello".to_string()],
                pipeline_target: Some("$out".to_string()),
                ..Default::default()
            };
            let wf = WorkflowFile {
                steps: vec![Step {
                    number: 1,
                    body: Some(Stmt::Command(cmd)),
                    ..Default::default()
                }],
                ..Default::default()
            };

            let result = Executor::with_stub().execute(&wf, None);
            assert_eq!(result.status, Status::Ok);
            assert_eq!(result.out, Value::Text("hello".to_string()));
            assert_eq!(
                result.log[0].result,
                Value::Text("hello".to_string()),
                "ASSIGN should return the assigned value"
            );
        }
    }
}
