//! `cronus-contract` — the ports tier of the crate topology:
//! shared types plus the seam traits domain code depends on and
//! adapter crates implement. Zero external dependencies, by construction —
//! nothing here may ever depend on I/O, a platform service, or a C library.
//!
//! This crate holds no logic of its own beyond what a data type's own
//! invariants require (id generation, display formatting, weight lookup). It
//! exists so `cronus-domain` and the adapter crates (`cronus-store-local`,
//! `cronus-auth-local`) can share a vocabulary without either depending on
//! the other.

// ── Memory types ─────────────────────────────────────────────────────────────
//
// Moved from `crates/core/src/memory/mod.rs`. `MemoryEntry` is the payload the
// `MemorySearch` / `UserDataStore` seam traits below carry across the
// domain/adapter boundary; its field types travel with it.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

fn generate_id() -> String {
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64;
    let c = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("mem_{t:016x}_{c:08x}")
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MemoryId(String);

impl MemoryId {
    pub fn new() -> Self {
        MemoryId(generate_id())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for MemoryId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<String> for MemoryId {
    fn from(s: String) -> Self {
        MemoryId(s)
    }
}

impl std::fmt::Display for MemoryId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryKind {
    ArchitecturalDecision,
    DebugContext,
    KnownIssue,
    Convention,
    ProjectContext,
}

impl MemoryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MemoryKind::ArchitecturalDecision => "ArchitecturalDecision",
            MemoryKind::DebugContext => "DebugContext",
            MemoryKind::KnownIssue => "KnownIssue",
            MemoryKind::Convention => "Convention",
            MemoryKind::ProjectContext => "ProjectContext",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "ArchitecturalDecision" => Some(MemoryKind::ArchitecturalDecision),
            "DebugContext" => Some(MemoryKind::DebugContext),
            "KnownIssue" => Some(MemoryKind::KnownIssue),
            "Convention" => Some(MemoryKind::Convention),
            "ProjectContext" => Some(MemoryKind::ProjectContext),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemorySource {
    Agent,
    User,
    Git,
    System,
    Import,
}

impl MemorySource {
    pub fn as_str(self) -> &'static str {
        match self {
            MemorySource::Agent => "Agent",
            MemorySource::User => "User",
            MemorySource::Git => "Git",
            MemorySource::System => "System",
            MemorySource::Import => "Import",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "Agent" => Some(MemorySource::Agent),
            "User" => Some(MemorySource::User),
            "Git" => Some(MemorySource::Git),
            "System" => Some(MemorySource::System),
            "Import" => Some(MemorySource::Import),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationState {
    Untested,
    Claimed,
    TestedInProject,
    ValidatedCrossProject,
}

impl VerificationState {
    pub fn weight(self) -> f64 {
        match self {
            VerificationState::Untested => 0.30,
            VerificationState::Claimed => 0.50,
            VerificationState::TestedInProject => 0.70,
            VerificationState::ValidatedCrossProject => 1.00,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            VerificationState::Untested => "Untested",
            VerificationState::Claimed => "Claimed",
            VerificationState::TestedInProject => "TestedInProject",
            VerificationState::ValidatedCrossProject => "ValidatedCrossProject",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "Untested" => Some(VerificationState::Untested),
            "Claimed" => Some(VerificationState::Claimed),
            "TestedInProject" => Some(VerificationState::TestedInProject),
            "ValidatedCrossProject" => Some(VerificationState::ValidatedCrossProject),
            _ => None,
        }
    }
}

/// Where a memory sits on the processing-depth axis, orthogonal to
/// scope. Refinement flows one way, `raw -> working -> consolidated`;
/// consolidation never rewrites raw evidence, so any consolidated claim can
/// be checked against what actually happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryDepth {
    /// Verbatim captured evidence (transcripts, source documents) — immutable.
    Raw,
    /// Recent, lightly-processed notes organized by occurrence.
    Working,
    /// Durable, reusable abstraction — the recallable long-term corpus.
    Consolidated,
}

impl MemoryDepth {
    pub fn as_str(self) -> &'static str {
        match self {
            MemoryDepth::Raw => "Raw",
            MemoryDepth::Working => "Working",
            MemoryDepth::Consolidated => "Consolidated",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "Raw" => Some(MemoryDepth::Raw),
            "Working" => Some(MemoryDepth::Working),
            "Consolidated" => Some(MemoryDepth::Consolidated),
            _ => None,
        }
    }
}

/// A memory's reversible lifecycle state, orthogonal to decay.
/// Decay may lower an item's ranking in any state, but MUST NOT delete an
/// item whose state is `Paused` or `Archived` — a deliberate shelving is a
/// value signal that overrides automatic pruning. `Deleted` is not a stored
/// variant: it is realized by the existing targeted forget (row removal),
/// per its own table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleState {
    /// In the default recall set.
    Active,
    /// Temporarily and reversibly excluded from recall, no data loss.
    Paused,
    /// Retained but out of the default recall set; opt-in to include.
    Archived,
}

impl LifecycleState {
    pub fn as_str(self) -> &'static str {
        match self {
            LifecycleState::Active => "Active",
            LifecycleState::Paused => "Paused",
            LifecycleState::Archived => "Archived",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "Active" => Some(LifecycleState::Active),
            "Paused" => Some(LifecycleState::Paused),
            "Archived" => Some(LifecycleState::Archived),
            _ => None,
        }
    }
}

/// The typed outcome of a captured experience (read back by the reuse gate, written
/// at capture) — `None` on every ordinary memory (the honest default for the
/// entire pre-existing corpus), `Some(_)` only for an item `distill_run`
/// produced. Orthogonal to `kind`: a distilled run is still classified by
/// `kind` for what it's *about* (`ProjectContext`, typically), while this
/// says how the attempt *went* — the axis `act_with_experience`'s reuse gate
/// actually needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExperienceOutcome {
    /// Reusable directly when the gate passes.
    Success,
    /// Never reused as a solution — injected as an avoid signal.
    Failure,
    /// Injected as guidance, never reused as a solution.
    Insight,
}

impl ExperienceOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            ExperienceOutcome::Success => "Success",
            ExperienceOutcome::Failure => "Failure",
            ExperienceOutcome::Insight => "Insight",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "Success" => Some(ExperienceOutcome::Success),
            "Failure" => Some(ExperienceOutcome::Failure),
            "Insight" => Some(ExperienceOutcome::Insight),
            _ => None,
        }
    }
}

/// The subject-of-memory lens: who the memory is *about*,
/// orthogonal to `actor` (who said it) and to `source` (how it entered the
/// system). A closed 2-variant vocabulary — no third-party subject exists
/// in this single-tenant model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemorySubject {
    User,
    AgentSelf,
}

impl MemorySubject {
    pub fn as_str(self) -> &'static str {
        match self {
            MemorySubject::User => "User",
            MemorySubject::AgentSelf => "AgentSelf",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "User" => Some(MemorySubject::User),
            "AgentSelf" => Some(MemorySubject::AgentSelf),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MemoryEntry {
    pub id: MemoryId,
    pub kind: MemoryKind,
    pub source: MemorySource,
    pub title: String,
    pub body: String,
    pub confidence: f64,
    pub trust_score: f64,
    pub valid_at: u64,
    pub created_at: u64,
    pub superseded_at: Option<u64>,
    pub workspace_id: Option<String>,
    pub verification_state: VerificationState,
    pub depth: MemoryDepth,
    pub lifecycle_state: LifecycleState,
    pub experience_outcome: Option<ExperienceOutcome>,
    /// Who said this — distinct from `workspace_id` (scope
    /// ownership) and `source` (how it entered the system).
    pub actor: Option<String>,
    /// A hard void-after instant — complements decay rather
    /// than replacing it; decay lowers ranking, `expiry` removes the item
    /// from default recall outright once passed.
    pub expiry: Option<u64>,
    /// The subject-of-memory lens.
    pub subject: Option<MemorySubject>,
}

impl MemoryEntry {
    pub fn new(
        kind: MemoryKind,
        source: MemorySource,
        title: impl Into<String>,
        body: impl Into<String>,
    ) -> Self {
        let now = now_secs();
        MemoryEntry {
            id: MemoryId::new(),
            kind,
            source,
            title: title.into(),
            body: body.into(),
            confidence: 1.0,
            trust_score: 0.5,
            valid_at: now,
            created_at: now,
            superseded_at: None,
            workspace_id: None,
            verification_state: VerificationState::Untested,
            // A single-shot `new()` call already represents a discrete,
            // finished fact — every pre-existing call site (auth, CLI `cronus
            // memory store`, session capture) writes exactly this shape, so
            // defaulting to `Consolidated` preserves that behavior exactly.
            // `Raw`/`Working` are for the future ingestion pipeline
            // and are opted into via `with_depth`.
            depth: MemoryDepth::Consolidated,
            lifecycle_state: LifecycleState::Active,
            experience_outcome: None,
            actor: None,
            expiry: None,
            subject: None,
        }
    }

    pub fn with_workspace(mut self, id: impl Into<String>) -> Self {
        self.workspace_id = Some(id.into());
        self
    }

    pub fn with_depth(mut self, depth: MemoryDepth) -> Self {
        self.depth = depth;
        self
    }

    /// Marks this entry as a captured experience so
    /// the reuse gate can later recall it typed. Ordinary memories never
    /// call this — `experience_outcome` stays `None`.
    pub fn with_experience_outcome(mut self, outcome: ExperienceOutcome) -> Self {
        self.experience_outcome = Some(outcome);
        self
    }

    /// Attribute this capture to who said it.
    pub fn with_actor(mut self, actor: impl Into<String>) -> Self {
        self.actor = Some(actor.into());
        self
    }

    /// A hard void-after instant, distinct from decay.
    pub fn with_expiry(mut self, expiry: u64) -> Self {
        self.expiry = Some(expiry);
        self
    }

    /// The subject-of-memory lens.
    pub fn with_subject(mut self, subject: MemorySubject) -> Self {
        self.subject = Some(subject);
        self
    }

    /// Effective trust score after applying the verification state weight.
    pub fn effective_trust(&self) -> f64 {
        self.trust_score * self.verification_state.weight()
    }
}

// ── StateStore seam ──────────────────────────────────────────────────────────
//
// Moved from `crates/core/src/store.rs`.

/// A durable key-value store the engine resumes from after a restart.
pub trait StateStore {
    /// Persist a value; durable once this returns `Ok`.
    fn put(&mut self, key: &str, value: &str) -> std::io::Result<()>;
    /// Read a value previously stored, if present.
    fn get(&self, key: &str) -> Option<String>;
}

// ── ModelProvider seam ───────────────────────────────────────────────────────
//
// Moved from `crates/core/src/router/provider.rs`. Only the types the trait
// signature itself references travel with it — `ProviderError`,
// `RoutingRequest`, and `RouteDecision` are router-internal and stay in
// `cronus-domain`.

/// Health state reported by a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderHealth {
    Healthy,
    Degraded,
    Unavailable,
}

/// Provider tier used for routing preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProviderTier {
    /// Locally-hosted model (lowest cost, highest privacy).
    Local = 0,
    /// Small, fast cloud model.
    Economy = 1,
    /// Standard cloud model.
    Standard = 2,
    /// Large, high-capability cloud model.
    Premium = 3,
}

/// Category of task being routed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskType {
    CodeGeneration,
    Analysis,
    Summarization,
    QA,
    Chat,
}

/// The `ModelProvider` trait — implemented by each backend.
///
/// All methods take `&self` — providers are stateless from the router's
/// perspective; mutable circuit state lives in `RouterPool`.
pub trait ModelProvider: Send + Sync {
    /// Unique stable identifier (e.g. "openai-gpt4o", "local-llama3").
    fn id(&self) -> &str;

    /// Current health as reported by the provider's own health check.
    fn health(&self) -> ProviderHealth;

    /// Maximum tokens this provider accepts in context.
    fn context_window(&self) -> u32;

    /// Approximate cost per 1k tokens (output) in USD.
    fn cost_per_1k_tokens(&self) -> f64;

    /// Median observed latency in milliseconds.
    fn latency_p50_ms(&self) -> u64;

    /// Provider tier for routing priority.
    fn tier(&self) -> ProviderTier;

    /// Returns how well this provider handles the given task type (0.0–1.0).
    fn task_fit(&self, task: TaskType) -> f64;
}

// ── CheckpointWriter seam ────────────────────────────────────────────────────
//
// Moved from `crates/core/src/checkpoint.rs`.

/// The three canonical checkpoint files for a session.
#[derive(Debug, Clone)]
pub struct CheckpointPaths {
    /// Full session context JSON.
    pub context: std::path::PathBuf,
    /// Extracted memory facts (plain text).
    pub memory: std::path::PathBuf,
    /// Human-readable session notes.
    pub notes: std::path::PathBuf,
}

impl CheckpointPaths {
    pub fn new(state_dir: &std::path::Path) -> Self {
        let base = state_dir.join("checkpoint");
        CheckpointPaths {
            context: base.clone(),
            memory: base.join("memory"),
            notes: base.join("notes.md"),
        }
    }

    pub fn fork(state_dir: &std::path::Path, fork_id: &str) -> Self {
        let base = state_dir.join(format!("checkpoint-fork-{fork_id}"));
        CheckpointPaths {
            context: base.clone(),
            memory: base.join("memory"),
            notes: base.join("notes.md"),
        }
    }
}

#[derive(Debug)]
pub enum CheckpointError {
    Io(std::io::Error),
}

impl std::fmt::Display for CheckpointError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckpointError::Io(e) => write!(f, "checkpoint I/O error: {e}"),
        }
    }
}

impl std::error::Error for CheckpointError {}

impl From<std::io::Error> for CheckpointError {
    fn from(e: std::io::Error) -> Self {
        CheckpointError::Io(e)
    }
}

/// Seam trait for writing checkpoints (wired by agent-registry later).
pub trait CheckpointWriter: Send + Sync {
    fn write(&self, paths: &CheckpointPaths, body: &str) -> Result<(), CheckpointError>;
}

// ── Compactor seam ───────────────────────────────────────────────────────────
//
// Moved from `crates/core/src/context_mgmt.rs`.

/// Where a context entry falls in the eviction order (least-important last).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrimPriority {
    OrphanedToolResult,
    ToolUsePair,
    NonProtectedThinking,
    NonProtectedAssistant,
    NonProtectedUser,
    CompactionMarker,
    ModelChangeMarker,
    Protected, // never trimmed — invariant
}

/// One turn's worth of context, with its trim priority and token cost.
#[derive(Debug, Clone)]
pub struct ContextEntry {
    pub role: String,
    pub body: String,
    pub token_count: u64,
    pub protected: bool,
    pub priority: TrimPriority,
}

impl ContextEntry {
    pub fn new(role: impl Into<String>, body: impl Into<String>, token_count: u64) -> Self {
        ContextEntry {
            role: role.into(),
            body: body.into(),
            token_count,
            protected: false,
            priority: TrimPriority::NonProtectedUser,
        }
    }

    pub fn with_priority(mut self, p: TrimPriority) -> Self {
        self.priority = p;
        self
    }

    pub fn protect(mut self) -> Self {
        self.protected = true;
        self.priority = TrimPriority::Protected;
        self
    }
}

/// Seam trait for LLM-driven compaction (wiring deferred).
pub trait Compactor: Send + Sync {
    fn compact(&self, context: &[ContextEntry], keep_recent_tokens: u64) -> Result<String, String>;
}

// ── BusSender seam ───────────────────────────────────────────────────────────
//
// Moved from `crates/core/src/inbox.rs`.

/// Bus events emitted by the inbox module.
#[derive(Debug, Clone, PartialEq)]
pub enum BusEvent {
    InboxArrived { recipient_id: String, count: u32 },
}

/// Seam trait for bus event delivery (real bus wiring deferred).
pub trait BusSender: Send + Sync {
    fn send(&self, event: BusEvent);
}

/// No-op bus sender (stub). Lives beside the trait it implements — both
/// `cronus-domain` and `cronus-store-local` need a bus sender, and neither
/// may depend on the other, so a null-object default belongs in the ports
/// tier both already depend on.
pub struct NoOpBusSender;

impl BusSender for NoOpBusSender {
    fn send(&self, _: BusEvent) {}
}

/// Capturing bus sender for tests.
pub struct CaptureBusSender {
    pub events: std::sync::Mutex<Vec<BusEvent>>,
}

impl Default for CaptureBusSender {
    fn default() -> Self {
        Self::new()
    }
}

impl CaptureBusSender {
    pub fn new() -> Self {
        CaptureBusSender {
            events: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub fn captured(&self) -> Vec<BusEvent> {
        self.events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl BusSender for CaptureBusSender {
    fn send(&self, event: BusEvent) {
        // A sender that panicked elsewhere must not stop later events being
        // recorded: the list is only ever appended to, so it stays coherent.
        self.events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(event);
    }
}

// ── provider-plane seams ────────────────────────────────────────────────
//
// New trait declarations. Illustrative shape, not
// a final signature — no implementation exists yet; `cronus-store-local`
// (`UserDataStore`) and `cronus-auth-local` (`AuthProvider`/`IdentityProvider`)
// implement these in a later phase task. The plain `String` error is
// deliberately minimal for the same reason: these are net-new APIs, not an
// existing one being redesigned, so committing to a richer error type now
// would be speculative.

/// First-class temporal recall modes over the bi-temporal record —
/// resolved against valid-time (`AsOf`) or transaction-time (`ChangedSince`,
/// `Recent`), never conflating the two.
#[derive(Debug, Clone, Copy)]
pub enum TemporalMode {
    /// "What did we hold true at instant T?" — valid-time window containing T.
    AsOf(u64),
    /// "What is new or changed since checkpoint C?" — transaction-time > C.
    ChangedSince(u64),
    /// "What are the newest N, regardless of age?" — transaction-time desc.
    Recent,
}

/// The field a structured predicate compares — closed to the columns
/// `MemoryEntry` actually has. The vocabulary is backend-agnostic; this enum
/// names *which* field, the backend decides how to index or compare it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PredicateField {
    Kind,
    Source,
    WorkspaceId,
    TrustScore,
    Confidence,
    CreatedAt,
    ValidAt,
    ExperienceOutcome,
}

/// A comparison operand.
#[derive(Debug, Clone)]
pub enum PredicateValue {
    Text(String),
    Number(f64),
}

/// A small, closed structured-comparison vocabulary — equals/
/// not-equals, ordering, set membership, text containment — combinable via
/// AND/OR/NOT, composable with (never replacing) the fuzzy multi-signal
/// fusion and the temporal modes. Fixed at this layer precisely so it stays
/// backend-agnostic: a backend that cannot express a combinator natively
/// falls back to post-fetch evaluation rather than silently dropping the
/// constraint (SQLite, the only backend today, expresses every variant here
/// natively — the fallback contract is honored vacuously, not exercised).
#[derive(Debug, Clone)]
pub enum FieldPredicate {
    Eq(PredicateField, PredicateValue),
    Ne(PredicateField, PredicateValue),
    Gt(PredicateField, PredicateValue),
    Ge(PredicateField, PredicateValue),
    Lt(PredicateField, PredicateValue),
    Le(PredicateField, PredicateValue),
    In(PredicateField, Vec<PredicateValue>),
    NotIn(PredicateField, Vec<PredicateValue>),
    Contains(PredicateField, String),
    ContainsCi(PredicateField, String),
    And(Vec<FieldPredicate>),
    Or(Vec<FieldPredicate>),
    Not(Box<FieldPredicate>),
}

/// The read half of the user-data plane: full-text search over stored
/// memories, independent of where they are persisted.
pub trait MemorySearch {
    fn search_fts(&self, query: &str, limit: usize) -> Result<Vec<MemoryEntry>, String>;

    /// Temporal recall modes over the bi-temporal record, composing
    /// with the same trust/lifecycle defaults as `search_fts`.
    fn recall_temporal(&self, mode: TemporalMode, limit: usize)
    -> Result<Vec<MemoryEntry>, String>;

    /// The closed structured-comparison vocabulary.
    fn recall_structured(
        &self,
        predicate: &FieldPredicate,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>, String>;
}

/// The user-data plane. `MemorySearch` is one facet; a full
/// implementation also covers write, prune, and export (portability).
///
/// No `Send + Sync` bound: the on-device default wraps a `rusqlite::Connection`,
/// which is not `Sync` (SQLite connections are not shared across threads
/// without external synchronization). The illustrative sketch this trait
/// started from assumed it; the concrete implementation proved it wrong.
pub trait UserDataStore: MemorySearch {
    fn put(&self, entry: &MemoryEntry) -> Result<(), String>;
    fn export(&self) -> Result<Vec<MemoryEntry>, String>; // Always able to come home
}

/// The authentication plane.
pub trait AuthProvider: Send + Sync {
    fn authenticate(&self, principal: &str, credential: &str) -> Result<bool, String>;
}

/// The principal-identity plane.
pub trait IdentityProvider: Send + Sync {
    fn current_principal(&self) -> Option<String>;
}

// ── InferenceBackend seam ────────────────────────────────────────────────────
//
// The streaming call surface. Distinct from `ModelProvider`
// above: that trait is routing metadata (id/health/cost/latency/tier/
// task_fit — what the router scores) and has no method that performs a
// call. A concrete provider in `cronus-model-local` implements both traits —
// two facets of one object, score vs. call — never one subsuming the other.

/// A request to generate against a named model.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GenerateRequest {
    /// The model name/tag this request targets (resolves via the router's
    /// `api_base`, not looked up here).
    pub model: String,
    pub prompt: String,
    /// Backend-specific generation parameters (temperature, top_p, ...) as
    /// opaque key/value pairs — mirrors the modifier convention `nodus`
    /// already uses for its own provider seam.
    pub parameters: Vec<(String, String)>,
}

/// One event in a generation stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    Token(String),
    ToolCall {
        name: String,
        arguments: String,
    },
    Usage {
        prompt_tokens: u64,
        completion_tokens: u64,
    },
    /// Terminal: the stream completed normally.
    Done,
    /// Terminal: the stream ended abnormally, including on cancellation.
    Error(InferenceError),
}

/// The wire-failure taxonomy a transport maps onto. Deliberately flat
/// and small — retry/rotate/fallback policy over these variants lives in
/// the model-error-recovery layer, not here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InferenceError {
    ConnectRefused,
    Timeout,
    ClientError(u16),
    ServerError(u16),
    MalformedStream(String),
    /// The caller's `CancelHandle` was set mid-call.
    Cancelled,
    /// The backend has no support for the attempted operation (
    /// reported honestly, never silently emulated).
    Unsupported,
}

/// Static facts about a model as reported by its serving backend.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModelDescriptor {
    pub name: String,
    pub digest: Option<String>,
    pub size_bytes: Option<u64>,
    pub parameters: Option<String>,
}

/// A residency instruction for an explicit load/unload lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResidencyHint {
    KeepAliveSecs(u64),
    UnloadNow,
}

/// One event in a model-acquisition stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullProgress {
    Downloading {
        bytes_done: u64,
        bytes_total: Option<u64>,
    },
    Done {
        digest: Option<String>,
    },
    Error(InferenceError),
}

/// A cooperative cancellation flag shared between a caller and the worker
/// driving a `generate_stream` call. Cloning shares the same underlying flag
/// — the caller keeps one clone to call `cancel()` while another is moved
/// into the streaming call.
#[derive(Debug, Clone)]
pub struct CancelHandle(Arc<AtomicBool>);

impl CancelHandle {
    pub fn new() -> Self {
        CancelHandle(Arc::new(AtomicBool::new(false)))
    }

    /// Request cancellation. The next event the stream yields MUST be
    /// `StreamEvent::Error(InferenceError::Cancelled)`, followed by
    /// termination (no further events).
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

impl Default for CancelHandle {
    fn default() -> Self {
        Self::new()
    }
}

/// The streaming inference call surface. Implemented by a
/// concrete endpoint-profile provider in `cronus-model-local`; the nodus
/// `ModelProvider` (`generate`/`analyze` → `String`) is satisfied by a
/// host-side bridge that collapses this stream, never by implementing this
/// trait a second time.
pub trait InferenceBackend: Send + Sync {
    /// Blocking pull-iterator: the caller advances it to drive the call.
    /// Cancelling `cancel` mid-stream yields exactly one
    /// `Error(Cancelled)` and then `None`.
    fn generate_stream(
        &self,
        request: GenerateRequest,
        cancel: CancelHandle,
    ) -> Box<dyn Iterator<Item = StreamEvent> + Send>;

    fn embed(&self, model: &str, input: &str) -> Result<Vec<f32>, InferenceError>;

    fn describe(&self, model: &str) -> Result<ModelDescriptor, InferenceError>;

    /// Acquire a model by name; progress-streamed, never a hidden stall.
    fn pull(&self, model: &str) -> Box<dyn Iterator<Item = PullProgress> + Send>;

    fn set_residency(&self, model: &str, hint: ResidencyHint) -> Result<(), InferenceError>;
}

#[cfg(test)]
mod inference_tests {
    use super::*;

    /// A scripted backend: yields three tokens then `Done`, unless
    /// cancelled mid-stream, in which case it yields exactly one
    /// `Error(Cancelled)` and stops.
    struct ScriptedBackend;

    impl InferenceBackend for ScriptedBackend {
        fn generate_stream(
            &self,
            _request: GenerateRequest,
            cancel: CancelHandle,
        ) -> Box<dyn Iterator<Item = StreamEvent> + Send> {
            let tokens = ["Hello", ", ", "world"];
            let mut idx = 0usize;
            let mut finished = false;
            Box::new(std::iter::from_fn(move || {
                if finished {
                    return None;
                }
                if cancel.is_cancelled() {
                    finished = true;
                    return Some(StreamEvent::Error(InferenceError::Cancelled));
                }
                if idx < tokens.len() {
                    let tok = tokens[idx].to_string();
                    idx += 1;
                    Some(StreamEvent::Token(tok))
                } else {
                    finished = true;
                    Some(StreamEvent::Done)
                }
            }))
        }

        fn embed(&self, _model: &str, _input: &str) -> Result<Vec<f32>, InferenceError> {
            Err(InferenceError::Unsupported)
        }

        fn describe(&self, model: &str) -> Result<ModelDescriptor, InferenceError> {
            Ok(ModelDescriptor {
                name: model.to_string(),
                ..Default::default()
            })
        }

        fn pull(&self, _model: &str) -> Box<dyn Iterator<Item = PullProgress> + Send> {
            Box::new(std::iter::once(PullProgress::Done { digest: None }))
        }

        fn set_residency(&self, _model: &str, _hint: ResidencyHint) -> Result<(), InferenceError> {
            Ok(())
        }
    }

    #[test]
    fn generate_stream_runs_to_done_uncancelled() {
        let backend = ScriptedBackend;
        let cancel = CancelHandle::new();
        let events: Vec<StreamEvent> = backend
            .generate_stream(GenerateRequest::default(), cancel)
            .collect();

        assert_eq!(
            events,
            vec![
                StreamEvent::Token("Hello".to_string()),
                StreamEvent::Token(", ".to_string()),
                StreamEvent::Token("world".to_string()),
                StreamEvent::Done,
            ]
        );
    }

    #[test]
    fn cancel_mid_stream_yields_single_cancelled_error_then_stops() {
        let backend = ScriptedBackend;
        let cancel = CancelHandle::new();
        let mut stream = backend.generate_stream(GenerateRequest::default(), cancel.clone());

        // Pull two tokens, then cancel mid-stream.
        assert_eq!(stream.next(), Some(StreamEvent::Token("Hello".to_string())));
        assert_eq!(stream.next(), Some(StreamEvent::Token(", ".to_string())));
        cancel.cancel();

        assert_eq!(
            stream.next(),
            Some(StreamEvent::Error(InferenceError::Cancelled))
        );
        assert_eq!(
            stream.next(),
            None,
            "no events after the terminal Cancelled"
        );
    }

    #[test]
    fn unsupported_capability_reported_honestly_not_emulated() {
        let backend = ScriptedBackend;
        assert_eq!(
            backend.embed("any-model", "text"),
            Err(InferenceError::Unsupported)
        );
    }

    #[test]
    fn cancel_handle_clone_shares_the_same_flag() {
        let cancel = CancelHandle::new();
        let clone = cancel.clone();
        assert!(!clone.is_cancelled());
        cancel.cancel();
        assert!(
            clone.is_cancelled(),
            "clone must observe cancellation through the shared flag"
        );
    }
}

// ── Project wiki types ────────────────────────────────────────────────────────
//
// The client-facing project wiki is a derived projection
// CACHE: pages are rows written only by the office regeneration pipeline and
// reconstructable from ground truth. These are the payload types the
// store persists; the SQLite store itself lives in `cronus-store-local`.

/// The fixed page-kind hierarchy. The client wiki is
/// navigable overview → area → detail via `WikiPage::parent_id` + `ord`; the
/// kinds are closed so no page sits far from the overview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WikiPageKind {
    Overview,
    Area,
    Decisions,
    Howto,
    Glossary,
    Changelog,
}

impl WikiPageKind {
    /// Every page kind, in overview → detail order. The single source of truth
    /// for "regenerate the whole wiki" (`rebuild`): a new kind added to
    /// the enum is regenerated by a rebuild the moment it appears here.
    pub const fn all() -> [WikiPageKind; 6] {
        [
            WikiPageKind::Overview,
            WikiPageKind::Area,
            WikiPageKind::Decisions,
            WikiPageKind::Howto,
            WikiPageKind::Glossary,
            WikiPageKind::Changelog,
        ]
    }

    pub fn as_str(self) -> &'static str {
        match self {
            WikiPageKind::Overview => "overview",
            WikiPageKind::Area => "area",
            WikiPageKind::Decisions => "decisions",
            WikiPageKind::Howto => "howto",
            WikiPageKind::Glossary => "glossary",
            WikiPageKind::Changelog => "changelog",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "overview" => Some(WikiPageKind::Overview),
            "area" => Some(WikiPageKind::Area),
            "decisions" => Some(WikiPageKind::Decisions),
            "howto" => Some(WikiPageKind::Howto),
            "glossary" => Some(WikiPageKind::Glossary),
            "changelog" => Some(WikiPageKind::Changelog),
            _ => None,
        }
    }
}

/// One citation backing a wiki page's claims. Every substantive claim
/// must resolve to a citation; the regeneration pipeline drops an uncited
/// section rather than persisting it. `source_kind` names what is cited (e.g.
/// `decision`, `work_product`, `board_item`, `ledger_fact`); `source_id`
/// references the specific ground-truth record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikiCitation {
    pub source_kind: String,
    pub source_id: String,
}

impl WikiCitation {
    pub fn new(source_kind: impl Into<String>, source_id: impl Into<String>) -> Self {
        WikiCitation {
            source_kind: source_kind.into(),
            source_id: source_id.into(),
        }
    }
}

/// A client-facing wiki page — a derived projection row, never a
/// source of truth: reconstructable from ground truth by `rebuild`.
///
/// Optional structure is absent by default: a freshly-built page is a root
/// (`parent_id = None`), first in order (`ord = 0`), fresh (`stale = false`),
/// and uncited (`citations` empty) until the regeneration pipeline attributes
/// and places it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikiPage {
    pub id: String,
    pub office_id: String,
    /// `None` for the overview root; otherwise the parent in the nav tree.
    pub parent_id: Option<String>,
    /// Sibling ordering under `parent_id`.
    pub ord: i64,
    pub kind: WikiPageKind,
    pub title: String,
    /// Generated plain-language content.
    pub body: String,
    /// Sources backing the page; non-empty once the pipeline attributes it.
    pub citations: Vec<WikiCitation>,
    /// Hash of the inputs this page was generated from.
    pub source_fingerprint: String,
    pub generated_at: u64,
    /// `true` when the current source fingerprint differs from the stored one.
    pub stale: bool,
}

impl WikiPage {
    /// A minimal page with structure absent by default (root, ord 0, uncited,
    /// fresh). Callers set `parent_id`/`ord`/`citations`/`source_fingerprint`
    /// as the regeneration pipeline places and attributes it.
    pub fn new(
        id: impl Into<String>,
        office_id: impl Into<String>,
        kind: WikiPageKind,
        title: impl Into<String>,
        body: impl Into<String>,
    ) -> Self {
        WikiPage {
            id: id.into(),
            office_id: office_id.into(),
            parent_id: None,
            ord: 0,
            kind,
            title: title.into(),
            body: body.into(),
            citations: Vec::new(),
            source_fingerprint: String::new(),
            generated_at: now_secs(),
            stale: false,
        }
    }
}

/// One entry in a page's change history, appended newest-first by the
/// regeneration pipeline. `page_id` is `None` for an office-level change not
/// tied to a single page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikiChangelogEntry {
    pub id: String,
    pub office_id: String,
    pub page_id: Option<String>,
    /// Human-readable "what changed".
    pub change: String,
    pub at: u64,
}

/// The wiki store seam the office regeneration pipeline writes through.
/// Lives in the ports tier so `cronus-domain` (the
/// pipeline) never depends on `cronus-store-local` (the SQLite realization);
/// the client has no handle to this — only the curator-owned pipeline does.
/// The plain `String` error mirrors the other provider-plane seams.
pub trait WikiCache {
    fn get_page(&self, id: &str) -> Result<Option<WikiPage>, String>;

    /// Apply one regeneration **transactionally**: upsert every page
    /// and append every changelog entry as a single all-or-nothing unit. On
    /// any failure, nothing is written — the prior rows stay intact and
    /// correctly marked (a failed regeneration never leaves a half-written
    /// projection).
    fn apply_regeneration(
        &self,
        pages: &[WikiPage],
        changelog: &[WikiChangelogEntry],
    ) -> Result<(), String>;

    /// Rebuild an office's wiki from scratch: **transactionally** drop
    /// every existing `wiki_page` / `wiki_changelog` row for `office_id`, then
    /// write the freshly re-derived `pages` + `changelog` — all in one
    /// all-or-nothing unit. This is the operational proof the store is a
    /// rebuildable projection cache: the new set is derived entirely from
    /// ground truth, so a dropped or corrupt `wiki.db` is regenerated, never
    /// restored-as-truth. Passing an empty `pages` slice legitimately clears an
    /// office whose ground truth no longer grounds any page. The whole
    /// operation rolls back on any failure, so a failed rebuild leaves the
    /// prior rows intact rather than a half-cleared projection.
    fn rebuild_office(
        &self,
        office_id: &str,
        pages: &[WikiPage],
        changelog: &[WikiChangelogEntry],
    ) -> Result<(), String>;

    /// Every page belonging to an office — the input to the freshness sweep.
    fn pages_for_office(&self, office_id: &str) -> Result<Vec<WikiPage>, String>;

    /// Mark pages stale: their sources drifted since generation and no
    /// regeneration has caught up, so the UI must show a stale marker rather
    /// than silently presenting them as current.
    fn mark_stale(&self, page_ids: &[String]) -> Result<(), String>;

    /// Change history newest-first, at most `limit` entries.
    fn changelog(&self, office_id: &str, limit: usize) -> Result<Vec<WikiChangelogEntry>, String>;
}

/// The **read-only** client-facing wiki surface. The client is
/// handed a `&dyn WikiReadSurface`, which — by having no write method at all —
/// makes "the client can never curate the wiki" a compile-time property, not a
/// convention: there is simply no API to mutate a row through this trait. The
/// curator pipeline uses [`WikiCache`]; the client uses only this.
pub trait WikiReadSurface {
    /// One page by id, or `None`.
    fn page(&self, id: &str) -> Result<Option<WikiPage>, String>;

    /// The direct children of `parent_id` (or the roots when `None`), ordered
    /// for navigation — the overview → area → detail tree.
    fn children(&self, office_id: &str, parent_id: Option<&str>) -> Result<Vec<WikiPage>, String>;

    /// Full-text search over page title + body, best matches first.
    fn search(&self, office_id: &str, query: &str, limit: usize) -> Result<Vec<WikiPage>, String>;

    /// Change history newest-first, at most `limit` entries.
    fn changelog(&self, office_id: &str, limit: usize) -> Result<Vec<WikiChangelogEntry>, String>;
}

// ── Service Activation seam ────────

/// One of the two background modes beyond manual launch. Manual
/// launch itself is not a variant — it is the absence of any registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationMode {
    Login,
    System,
}

/// Whether a single mode can be offered on this host. Per-mode, not
/// whole-host: a non-systemd Linux host offers `Login` (via XDG autostart)
/// while reporting `System` unsupported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModeSupport {
    Supported,
    /// The mode cannot be offered here, and why — rendered to the user in
    /// place of the toggle, never an inert or silently-failing control.
    Unsupported {
        reason: String,
    },
}

/// What this host can offer, per mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationCapabilities {
    pub login: ModeSupport,
    pub system: ModeSupport,
}

/// The observed activation state — always read from the OS at the
/// moment it is asked, never a stored or remembered value. `RequiresApproval`
/// is a registered-but-vetoed state distinct from `Active`: the engine
/// genuinely will not run until the user approves it, so it must never be
/// folded into success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivationState {
    Inactive,
    Active(ActivationMode),
    RequiresApproval(ActivationMode),
    /// The facility could not be queried — never reported as `Active`.
    Unknown {
        reason: String,
    },
}

/// The per-OS activation seam. No OS type (registry key, plist, unit
/// file, service handle) crosses this trait. The domain tier holds the
/// mutual-exclusion and consent-bookkeeping policy and calls this
/// seam without ever naming a registry key itself; the adapter crate
/// (`cronus-activation-os`) holds the
/// OS calls.
///
/// There is no `set_state` and no persisted mirror: the only way to learn the
/// activation state is `observe()`, and the only way to change it is
/// `enable`/`disable` — both called only from an interactive frontend, never
/// from agent-run code.
pub trait ActivationRegistry: Send + Sync {
    /// What this host can offer, per mode. `Unsupported` on a spoke.
    fn capabilities(&self) -> ActivationCapabilities;

    /// Read the OS. Never a cached value.
    fn observe(&self) -> ActivationState;

    /// Register exactly `mode`. An adapter-level primitive — it does not
    /// decide whether some other mode should first be removed; that
    /// mutual-exclusion ordering is domain-tier policy, which calls
    /// `disable()` and verifies via `observe()` before calling this. May
    /// prompt for elevation for `System`.
    fn enable(&self, mode: ActivationMode) -> Result<(), String>;

    /// Remove whatever is currently registered. An adapter-level primitive;
    /// the caller verifies absence via `observe()`.
    fn disable(&self) -> Result<(), String>;
}

/// The honest do-nothing default ("seam and probe
/// first ... gives the settings surface something honest to render"). Before
/// the real per-OS adapter crate is wired in, this reports both modes
/// `Unsupported` rather than exposing a toggle that silently does nothing
///  — the settings surface renders the true "not available yet" state
/// instead of a fabricated one. `enable`/`disable` refuse rather than pretend
/// to register anything, so a caller can never observe a mode this adapter
/// claims to support.
pub struct NoOpActivationRegistry {
    reason: String,
}

impl NoOpActivationRegistry {
    pub fn new(reason: impl Into<String>) -> Self {
        NoOpActivationRegistry {
            reason: reason.into(),
        }
    }
}

impl Default for NoOpActivationRegistry {
    fn default() -> Self {
        Self::new("background activation is not wired on this build")
    }
}

impl ActivationRegistry for NoOpActivationRegistry {
    fn capabilities(&self) -> ActivationCapabilities {
        ActivationCapabilities {
            login: ModeSupport::Unsupported {
                reason: self.reason.clone(),
            },
            system: ModeSupport::Unsupported {
                reason: self.reason.clone(),
            },
        }
    }

    fn observe(&self) -> ActivationState {
        ActivationState::Inactive
    }

    fn enable(&self, _mode: ActivationMode) -> Result<(), String> {
        Err(self.reason.clone())
    }

    fn disable(&self) -> Result<(), String> {
        Err(self.reason.clone())
    }
}

#[cfg(test)]
mod activation_tests {
    use super::*;

    /// A scriptable registry for tests: capabilities and the current state
    /// are set at construction; `enable`/`disable` mutate the state, or fail
    /// without mutating it when scripted to — enough to exercise the honest-
    /// representation properties and a transition round-trip
    /// with no real OS calls.
    struct ScriptedRegistry {
        capabilities: ActivationCapabilities,
        state: std::sync::Mutex<ActivationState>,
        fail_transitions: bool,
    }

    impl ScriptedRegistry {
        fn new(capabilities: ActivationCapabilities, state: ActivationState) -> Self {
            ScriptedRegistry {
                capabilities,
                state: std::sync::Mutex::new(state),
                fail_transitions: false,
            }
        }

        /// Both modes supported, starting at `state`.
        fn supported(state: ActivationState) -> Self {
            Self::new(
                ActivationCapabilities {
                    login: ModeSupport::Supported,
                    system: ModeSupport::Supported,
                },
                state,
            )
        }
    }

    impl ActivationRegistry for ScriptedRegistry {
        fn capabilities(&self) -> ActivationCapabilities {
            self.capabilities.clone()
        }

        fn observe(&self) -> ActivationState {
            self.state.lock().unwrap().clone()
        }

        fn enable(&self, mode: ActivationMode) -> Result<(), String> {
            if self.fail_transitions {
                return Err("registration refused".to_string());
            }
            *self.state.lock().unwrap() = ActivationState::Active(mode);
            Ok(())
        }

        fn disable(&self) -> Result<(), String> {
            if self.fail_transitions {
                return Err("removal refused".to_string());
            }
            *self.state.lock().unwrap() = ActivationState::Inactive;
            Ok(())
        }
    }

    #[test]
    fn the_four_activation_states_round_trip_through_a_registry() {
        let registry = ScriptedRegistry::supported(ActivationState::Inactive);
        assert_eq!(registry.observe(), ActivationState::Inactive);

        registry
            .enable(ActivationMode::Login)
            .expect("enable succeeds");
        assert_eq!(
            registry.observe(),
            ActivationState::Active(ActivationMode::Login)
        );

        registry.disable().expect("disable succeeds");
        assert_eq!(registry.observe(), ActivationState::Inactive);

        let approval_pending =
            ScriptedRegistry::supported(ActivationState::RequiresApproval(ActivationMode::System));
        assert_eq!(
            approval_pending.observe(),
            ActivationState::RequiresApproval(ActivationMode::System)
        );

        let unknown = ScriptedRegistry::supported(ActivationState::Unknown {
            reason: "facility unreadable".to_string(),
        });
        assert!(matches!(unknown.observe(), ActivationState::Unknown { .. }));
    }

    #[test]
    fn a_spoke_host_reports_unsupported_for_both_modes() {
        // A host with no usable supervisor reports Unsupported with a
        // reason, never an inert toggle.
        let spoke = ScriptedRegistry::new(
            ActivationCapabilities {
                login: ModeSupport::Unsupported {
                    reason: "mobile target".to_string(),
                },
                system: ModeSupport::Unsupported {
                    reason: "mobile target".to_string(),
                },
            },
            ActivationState::Inactive,
        );
        let caps = spoke.capabilities();
        assert!(matches!(caps.login, ModeSupport::Unsupported { .. }));
        assert!(matches!(caps.system, ModeSupport::Unsupported { .. }));
    }

    #[test]
    fn an_unqueryable_facility_yields_unknown_never_active() {
        // Where the OS cannot be queried, the state is Unknown, never
        // silently reported as Active.
        let registry = ScriptedRegistry::supported(ActivationState::Unknown {
            reason: "registry key unreadable".to_string(),
        });
        let state = registry.observe();
        assert!(matches!(state, ActivationState::Unknown { .. }));
        assert_ne!(state, ActivationState::Active(ActivationMode::Login));
        assert_ne!(state, ActivationState::Active(ActivationMode::System));
    }

    #[test]
    fn requires_approval_is_representable_and_distinct_from_active() {
        let active = ActivationState::Active(ActivationMode::System);
        let pending = ActivationState::RequiresApproval(ActivationMode::System);
        assert_ne!(
            active, pending,
            "a registered-but-vetoed state must never be folded into Active"
        );
    }

    #[test]
    fn a_failed_transition_leaves_the_prior_state_unchanged() {
        let mut registry = ScriptedRegistry::supported(ActivationState::Inactive);
        registry.fail_transitions = true;

        let result = registry.enable(ActivationMode::System);
        assert!(result.is_err());
        assert_eq!(
            registry.observe(),
            ActivationState::Inactive,
            "a failed enable must not mutate the observed state"
        );
    }

    #[test]
    fn the_noop_registry_reports_unsupported_and_refuses_every_transition() {
        let registry = NoOpActivationRegistry::default();

        let caps = registry.capabilities();
        assert!(matches!(caps.login, ModeSupport::Unsupported { .. }));
        assert!(matches!(caps.system, ModeSupport::Unsupported { .. }));
        assert_eq!(registry.observe(), ActivationState::Inactive);
        assert!(registry.enable(ActivationMode::Login).is_err());
        assert!(registry.disable().is_err());
    }
}

// ── Knowledge Store seam ────────────────
//
// Named, access-controlled document collections with hybrid semantic+keyword
// retrieval. The SQLite/sqlite-vec/FTS5 realization lives in
// `cronus-store-local`; `cronus-domain` (ingestion, hybrid retrieval fusion,
// query preparation) depends only on the `KnowledgeStore` port below, never on
// the store crate directly (the `WikiCache` precedent).

/// Who authored a document — the write-zone boundary. Assigned from the
/// ingestion source at document creation, never chosen by a later agent
/// write, so the agent cannot mint a `Human` row to smuggle authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Human,
    Agent,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Human => "human",
            Origin::Agent => "agent",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "human" => Some(Origin::Human),
            "agent" => Some(Origin::Agent),
            _ => None,
        }
    }
}

/// Editorial-trust state for an agent-synthesized document, ordered
/// `Draft < Reviewed < Stable` so a `min_curation` filter is a simple
/// comparison. Absent (`None` on [`Document::curation`]) for `Origin::Human`
/// rows — curation is an agent-document concept, distinct from the indexing
/// `DocumentStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Curation {
    Draft,
    Reviewed,
    Stable,
}

impl Curation {
    pub fn as_str(self) -> &'static str {
        match self {
            Curation::Draft => "draft",
            Curation::Reviewed => "reviewed",
            Curation::Stable => "stable",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "draft" => Some(Curation::Draft),
            "reviewed" => Some(Curation::Reviewed),
            "stable" => Some(Curation::Stable),
            _ => None,
        }
    }
}

/// Index-pipeline state, distinct from the editorial-trust
/// `Curation` above — this tracks ingestion progress, not trust.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentStatus {
    Pending,
    Indexing,
    Ready,
    Error,
    Deleted,
}

impl DocumentStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            DocumentStatus::Pending => "pending",
            DocumentStatus::Indexing => "indexing",
            DocumentStatus::Ready => "ready",
            DocumentStatus::Error => "error",
            DocumentStatus::Deleted => "deleted",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(DocumentStatus::Pending),
            "indexing" => Some(DocumentStatus::Indexing),
            "ready" => Some(DocumentStatus::Ready),
            "error" => Some(DocumentStatus::Error),
            "deleted" => Some(DocumentStatus::Deleted),
            _ => None,
        }
    }
}

/// A named, access-controlled document collection. Every retrieval
/// query targets an explicit set of collection ids; there is no
/// implicit "search everything".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collection {
    pub id: String,
    pub owner_id: String,
    pub name: String,
    pub description: String,
    /// Arbitrary JSON metadata (tags, language hints, chunking overrides).
    pub meta: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl Collection {
    pub fn new(
        id: impl Into<String>,
        owner_id: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        let now = now_secs();
        Collection {
            id: id.into(),
            owner_id: owner_id.into(),
            name: name.into(),
            description: String::new(),
            meta: None,
            created_at: now,
            updated_at: now,
        }
    }
}

/// An optional per-collection directory tree — human navigation only;
/// directory structure never affects retrieval ranking or chunking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directory {
    pub id: String,
    pub collection_id: String,
    pub parent_id: Option<String>,
    pub name: String,
}

/// A source document within a collection. Structure absent
/// by default: a freshly-created document is `Origin::Agent`, `draft`
/// curation, `pending` status, with no directory placement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub id: String,
    pub collection_id: String,
    pub directory_id: Option<String>,
    /// `FileId` when the source is an uploaded file (references the file subsystem).
    pub source_file_id: Option<String>,
    /// The URL when the source is a scraped web page.
    pub source_url: Option<String>,
    pub name: String,
    pub status: DocumentStatus,
    pub origin: Origin,
    /// `None` for `Origin::Human` — editorial trust is an agent-document concept.
    pub curation: Option<Curation>,
    pub error_msg: Option<String>,
    pub meta: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl Document {
    /// A new agent-synthesized document: `pending`, `draft`, no placement.
    pub fn new_agent(
        id: impl Into<String>,
        collection_id: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        let now = now_secs();
        Document {
            id: id.into(),
            collection_id: collection_id.into(),
            directory_id: None,
            source_file_id: None,
            source_url: None,
            name: name.into(),
            status: DocumentStatus::Pending,
            origin: Origin::Agent,
            curation: Some(Curation::Draft),
            error_msg: None,
            meta: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// A new human-authored document (an upload or human-owned record):
    /// `pending`, no curation (an agent-document concept).
    pub fn new_human(
        id: impl Into<String>,
        collection_id: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        let now = now_secs();
        Document {
            id: id.into(),
            collection_id: collection_id.into(),
            directory_id: None,
            source_file_id: None,
            source_url: None,
            name: name.into(),
            status: DocumentStatus::Pending,
            origin: Origin::Human,
            curation: None,
            error_msg: None,
            meta: None,
            created_at: now,
            updated_at: now,
        }
    }
}

/// A locator for where in a source document a chunk came from (source
/// attribution). All fields optional — precision varies by source type (a
/// PDF has pages, an HTML page has sections, a plain-text record has neither).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceRef {
    pub page: Option<u32>,
    pub section: Option<String>,
    pub byte_start: Option<u64>,
    pub byte_end: Option<u64>,
}

/// One retrievable, embedded slice of a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub id: String,
    pub document_id: String,
    pub text: String,
    /// Ordinal position within the document.
    pub position: i64,
    pub source_ref: Option<SourceRef>,
    pub created_at: u64,
}

/// A semantic+keyword retrieval request.
#[derive(Debug, Clone)]
pub struct RetrievalRequest {
    pub query: String,
    /// Explicit target collections — retrieval never implicitly searches
    /// every collection.
    pub collection_ids: Vec<String>,
    pub top_k: usize,
    pub min_score: Option<f32>,
    /// Trust floor: chunks whose document `curation` is below this are
    /// excluded. `Origin::Human` documents (no curation) are always eligible.
    pub min_curation: Option<Curation>,
}

impl RetrievalRequest {
    pub fn new(query: impl Into<String>, collection_ids: Vec<String>) -> Self {
        RetrievalRequest {
            query: query.into(),
            collection_ids,
            top_k: 5,
            min_score: None,
            min_curation: None,
        }
    }
}

/// A retrieved, attributed chunk — the API asserts no
/// correctness, only "this text, from this source, at this score".
#[derive(Debug, Clone, PartialEq)]
pub struct RetrievedChunk {
    pub chunk_id: String,
    pub document_id: String,
    pub collection_id: String,
    pub text: String,
    pub source_ref: Option<SourceRef>,
    /// Fusion rank score (higher is better) — see [`KnowledgeStore::ann_search`]
    /// / [`KnowledgeStore::fts_search`] for the pre-fusion per-source scores.
    pub score: f32,
}

/// The explicit, audited override required to write into an
/// `Origin::Human` zone. `HumanDirected` carries an `audit_ref` so every
/// override write is attributable on the durable audit path — never a silent
/// default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteOverride {
    None,
    HumanDirected { audit_ref: String },
}

/// The knowledge-store seam. `cronus-domain`
/// composes ingestion and hybrid retrieval over this port; the SQLite +
/// sqlite-vec + FTS5 realization lives in `cronus-store-local`. No `Send +
/// Sync` bound — `rusqlite::Connection` is not `Sync` (the `WikiCache` /
/// `WikiReadSurface` precedent).
pub trait KnowledgeStore {
    // -- Collections / directories ---------------------------------------
    fn create_collection(&self, collection: &Collection) -> Result<(), String>;
    fn get_collection(&self, id: &str) -> Result<Option<Collection>, String>;
    fn create_directory(&self, directory: &Directory) -> Result<(), String>;

    // -- Documents (write-gated) -------------------------------
    /// Insert or replace a document. Refused when the *existing* row (if
    /// any) is `Origin::Human` and `override_` is not `HumanDirected`
    /// — enforced here, the single write seam, never by caller convention.
    /// A brand-new `Origin::Human` document (no existing row) is not gated —
    /// the gate protects *rewriting* human material, not its initial ingest.
    fn write_document(&self, document: &Document, override_: &WriteOverride) -> Result<(), String>;
    fn get_document(&self, id: &str) -> Result<Option<Document>, String>;
    /// Update only the index-state fields (`status`, `error_msg`) of an
    /// existing document — **never write-gated**, unlike [`write_document`].
    /// Index state is system bookkeeping the ingestion pipeline maintains,
    /// not authored content; an `origin = human` document's pipeline
    /// (pending → indexing → ready/error) must be able to complete without
    /// requiring a `WriteOverride` for every internal status transition, so
    /// long as the *content* write that created/updated the row was itself
    /// properly authorized.
    fn update_document_status(
        &self,
        document_id: &str,
        status: DocumentStatus,
        error_msg: Option<&str>,
    ) -> Result<(), String>;
    /// Advance curation. `Draft` is agent-free; `Reviewed`/`Stable`
    /// require `human_auth` (an opaque authorization reference) or the
    /// transition is refused.
    fn set_curation(
        &self,
        id: &str,
        next: Curation,
        human_auth: Option<&str>,
    ) -> Result<(), String>;
    /// Mark a document deleted — excluded from retrieval immediately;
    /// physical cleanup is [`KnowledgeStore::gc`].
    fn soft_delete_document(&self, id: &str) -> Result<(), String>;
    /// Physically remove documents soft-deleted more than
    /// `older_than_secs` ago, plus their chunk/FTS/vector rows. Returns the
    /// number of documents removed.
    fn gc(&self, older_than_secs: u64) -> Result<u64, String>;

    // -- Chunks (incremental re-index) -------------------------------
    /// Delete every chunk for `document_id` — the re-index precondition
    /// (existing chunks removed before fresh ones are inserted).
    fn delete_chunks(&self, document_id: &str) -> Result<(), String>;
    /// Insert a chunk and its embedding, keeping the FTS and vector indices
    /// in sync with the chunk row, atomically.
    fn insert_chunk(&self, chunk: &Chunk, embedding: &[f32]) -> Result<(), String>;
    /// Transactionally: delete every existing chunk for `document_id`
    /// (if any) and insert the given fresh set, as one all-or-nothing unit.
    /// A failure anywhere in the batch leaves the document's *prior* chunk
    /// set intact — never a half-deleted, half-inserted state (the
    /// `WikiCache::apply_regeneration` precedent). This is the atomicity
    /// primitive the ingestion pipeline's re-index composes over;
    /// [`KnowledgeStore::delete_chunks`] alone remains for the standalone
    /// removal case (soft-delete follow-up).
    fn reindex_chunks(&self, document_id: &str, chunks: &[(Chunk, Vec<f32>)])
    -> Result<(), String>;

    // -- Retrieval primitives (collection-scoped; RRF fusion composed in domain) --
    /// Vector nearest-neighbour candidates among `ready`, non-deleted
    /// documents in `collection_ids`, ascending by distance (closest first).
    fn ann_search(
        &self,
        collection_ids: &[String],
        query_vector: &[f32],
        top_k: usize,
    ) -> Result<Vec<(String, f32)>, String>;
    /// FTS5 keyword candidates, same scope, best match first.
    fn fts_search(
        &self,
        collection_ids: &[String],
        query_text: &str,
        top_k: usize,
    ) -> Result<Vec<(String, f32)>, String>;
    /// Hydrate chunk ids into full [`RetrievedChunk`]s (with attribution),
    /// applying the `min_curation` floor — `Origin::Human` documents
    /// are always eligible regardless of the floor. `score` on each result is
    /// left at `0.0`; the caller (domain-tier RRF fusion) sets it.
    fn hydrate_chunks(
        &self,
        chunk_ids: &[String],
        min_curation: Option<Curation>,
    ) -> Result<Vec<RetrievedChunk>, String>;
}

// ── Invocable registry seam ──────────────────────
//
// The descriptor types below are the whole advertised contract for one
// action. A surface renders help, completion, and command grouping from an
// `Invocable` alone; nothing about an action may be known to a surface that
// is not reachable from its descriptor, because any fact held privately
// instead is the first step of two surfaces disagreeing about what the
// product can do. The registry that holds and looks these up, and the
// dispatch that runs them, live in the domain tier — this crate carries only
// the shapes every tier and every surface must name without depending on
// each other.

/// A qualified invocable identity: `"core:board.list"` for a core action,
/// `"<extension-id>:verb"` for a contributed one. The qualifier is
/// what lets a contribution and a core action share one namespace without
/// either ever shadowing the other, whatever the core adds later.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct InvocableId(String);

/// Why a candidate identity does not match the grammar: exactly one
/// `:` separator, with a non-empty qualifier before it and a non-empty,
/// separator-free tail after it. Returned by [`InvocableId::new`] —
/// everything downstream of construction can therefore rely on any
/// `InvocableId` it holds already having this shape, with nothing left to
/// re-check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvocableIdError {
    /// No `:` separator is present at all.
    MissingSeparator,
    /// The qualifier (before the first `:`) is empty.
    EmptyQualifier,
    /// The tail (after the first `:`) is empty.
    EmptyTail,
    /// The tail itself contains a `:` — more than one separator.
    ExtraSeparator,
}

impl std::fmt::Display for InvocableIdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            InvocableIdError::MissingSeparator => "missing ':' qualifier separator",
            InvocableIdError::EmptyQualifier => "qualifier before ':' is empty",
            InvocableIdError::EmptyTail => "tail after ':' is empty",
            InvocableIdError::ExtraSeparator => "tail contains a second ':' separator",
        })
    }
}

impl std::error::Error for InvocableIdError {}

impl InvocableId {
    /// Construct a qualified identity, refusing anything that does not
    /// match the grammar: `<qualifier>:<tail>`, both halves non-empty,
    /// neither containing the separator. Refusing here — rather than
    /// accepting any string and validating later at registration — is what
    /// keeps an unvalidated identity from existing at all: a multi-
    /// separator or empty-half string would otherwise parse silently and
    /// resolve to the wrong entry, or to none, far from where it was built.
    pub fn new(id: impl Into<String>) -> Result<Self, InvocableIdError> {
        let id = id.into();
        let Some((qualifier, tail)) = id.split_once(':') else {
            return Err(InvocableIdError::MissingSeparator);
        };
        if qualifier.is_empty() {
            return Err(InvocableIdError::EmptyQualifier);
        }
        if tail.is_empty() {
            return Err(InvocableIdError::EmptyTail);
        }
        if tail.contains(':') {
            return Err(InvocableIdError::ExtraSeparator);
        }
        Ok(InvocableId(id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The identity a qualified id is prefixed with (e.g. `"core"` in
    /// `"core:board.list"`). Every `InvocableId` in existence was refused
    /// by [`InvocableId::new`] unless it has exactly this shape, so this
    /// split never falls back to the whole string.
    pub fn qualifier(&self) -> &str {
        self.0.split_once(':').map_or(&self.0, |(q, _)| q)
    }

    /// The unqualified part after the identity (e.g. `"board.list"` in
    /// `"core:board.list"`).
    pub fn tail(&self) -> &str {
        self.0.split_once(':').map_or(&self.0, |(_, t)| t)
    }
}

impl TryFrom<String> for InvocableId {
    type Error = InvocableIdError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        InvocableId::new(s)
    }
}

impl TryFrom<&str> for InvocableId {
    type Error = InvocableIdError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        InvocableId::new(s)
    }
}

impl std::fmt::Display for InvocableId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Where an invocable's behavior is decided — the machine-
/// readable form of "legitimate difference, named with its reason". A
/// surface consults this to know whether an action is reachable through
/// generic dispatch at all, never by guessing from an omission.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum Locus {
    /// Belongs to the core; reachable from every surface and remotely
    /// invocable.
    Semantic,
    /// Belongs to a surface; the same identity may have a different
    /// implementation per surface (e.g. pane focus, panel toggle).
    ClientLocal,
    /// Never remotely invocable — a host-owned facility, kept outside
    /// generic dispatch and declared with the reason, never silently
    /// omitted.
    HostOnly { reason: &'static str },
    /// Acts on the product's own installation rather than on the user's
    /// work — runs to completion without a session and never appears on a
    /// session-scoped surface (workspace init, configuration, extension
    /// management, diagnostics). Declared once in the launcher's own closed
    /// grammar and registered here so a session-scoped surface can
    /// *declare* that it deliberately does not offer it, rather than
    /// merely omitting it.
    Installation,
}

/// Whether an invocable is on the shipped surface. An action the
/// core cannot yet perform simply has no descriptor at all — this type has
/// no "not implemented" state, because that state must be unrepresentable
/// rather than discouraged.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum Stability {
    /// Appears on every default surface, in help, and in completion.
    Shipped,
    /// Off the shipped surface and out of completion; an invocation still
    /// resolves — to a message naming the replacement — never a silent
    /// unknown-command failure, and never a permanent second name for one
    /// capability.
    Retired { superseded_by: InvocableId },
}

/// The closed set of shapes a [`Binder`] may declare. A kind names both a
/// value's shape and — for `Flag` and `NamedText` — how it is positioned on
/// a command line, the same way `Flag` (a boolean, always `--name`, never a
/// bare word) already does: a binding style is not a fourth, independent
/// axis bolted onto every kind, only the two kinds that actually need one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum BinderKind {
    /// A string value, bound positionally in declared order.
    Text,
    Integer,
    Boolean,
    /// A boolean presence flag: `--name`, never a bare positional word, and
    /// never required (its absence simply means false).
    Flag,
    /// A string value bound as a named `--name <value>` flag rather than
    /// positionally — for a verb whose grammar names its argument rather
    /// than ordering it (`--actor cli`, `--mode login`). `optional`
    /// distinguishes a required named flag from one that may be absent; a
    /// missing optional one is read back as `None` by whatever handler
    /// wants a default, the same way an absent `Text` binder already is.
    NamedText,
    /// A floating-point value, bound positionally — the one shape `Integer`
    /// cannot carry (`budget set <limit>`, a dollar amount).
    Float,
    /// `--name <value>`, repeatable (`--collection a --collection b`),
    /// collected in declared order. The one shape `NamedText` cannot carry
    /// — everything else this kind set expresses is single-valued.
    RepeatableNamedText,
    /// A positional string restricted to a fixed set of values (`board move
    /// <state>`). Advertised as `[possible values: …]` and enforced as a
    /// usage failure, so a bad value is rejected before dispatch rather than
    /// as an application error. Bound and read back exactly like `Text`.
    EnumText(&'static [&'static str]),
}

/// One argument an invocable declares, in order. This is the single
/// source an invocable's advertised schema is derived from — a caller is
/// told exactly this, and it is exactly this the runtime later enforces, so
/// the two cannot drift into two different answers to "what does this
/// take". `optional` distinguishes a value that may be genuinely absent
/// from one that, if present, must parse: a present-but-malformed
/// value is never silently treated as absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Binder {
    pub name: &'static str,
    pub kind: BinderKind,
    pub optional: bool,
}

/// Bounds enforced on a contributed descriptor's text at registration.
/// Declared here, in the dependency-free tier, so the registry
/// (domain tier) and any surface rendering a validation refusal read the
/// same numbers rather than each guessing its own. Enforcement lives at the
/// registration door, not here — this crate only names the limits.
///
/// `Binder` carries no `description` field today, so the bound below covers
/// `name` only; a spec passage describing a bounded binder *description*
/// predates the field and is a wording defect, not a shape this crate
/// implements.
pub const INVOCABLE_NAME_MAX_LEN: usize = 80;
pub const INVOCABLE_SUMMARY_MAX_LEN: usize = 240;
pub const INVOCABLE_GROUP_MAX_LEN: usize = 40;
pub const INVOCABLE_MAX_BINDERS: usize = 16;
pub const BINDER_NAME_MAX_LEN: usize = 40;

/// The full advertised contract for one action. Every render of
/// help, completion, or a command grammar's grouping is a projection of this
/// struct; none restates it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Invocable {
    /// Qualified identity: `"core:…"` for the core, `"<ext-id>:…"`
    /// for a contribution.
    pub id: InvocableId,
    /// Human-readable, catalog-facing name.
    pub name: &'static str,
    /// One line; feeds help, slash discovery, and completion.
    pub summary: &'static str,
    /// Noun namespace this action groups under, per the project's command
    /// grammar.
    pub group: &'static str,
    pub locus: Locus,
    /// Ordered, typed; the single schema source.
    pub binders: Vec<Binder>,
    pub stability: Stability,
    /// Whether a resolved dispatch of this invocable records its raw
    /// arguments in the dispatch journal. An invocable whose
    /// argument may itself be a secret, or whose own domain event already
    /// owns the payload, declares `false` — this removes that argument
    /// class from the journal entirely rather than relying on redaction to
    /// catch it after the fact, which cannot help an argument that IS
    /// entirely a credential the store never learns.
    pub journal_raw_input: bool,
}

impl Invocable {
    /// Whether a frontend's own local discovery projection — a slash
    /// catalog, an action registry, an IPC catalog command — should offer
    /// this descriptor at all. The single predicate every surface's
    /// catalog filter is defined in terms of, so no surface hand-rolls this
    /// rule a second time: only a live, generically-dispatchable action
    /// belongs on a discovery surface — `ClientLocal` because it is
    /// per-surface by design, `Semantic` because it is the core's own
    /// reachable-everywhere case; `HostOnly`/`Installation` are each
    /// reachable by their own declared, non-generic path instead, and a
    /// `Stability` other than `Shipped` is not yet (or no longer) meant for
    /// a user to discover.
    pub fn is_projected(&self) -> bool {
        matches!(self.locus, Locus::Semantic | Locus::ClientLocal)
            && matches!(self.stability, Stability::Shipped)
    }
}

/// Whether an id names something the registry currently knows, asked and
/// answered **before** dispatch. Deliberately not `Option`: the two
/// outcomes read as domain facts here (`Found`/`Unknown`), not as a generic
/// absence, because three different surfaces act on `Unknown` in three
/// different — and mutually incompatible — ways, and folding it into
/// a failure-shaped `Outcome` would force one of them to behave incorrectly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved<'a> {
    Found(&'a Invocable),
    Unknown,
}

impl<'a> Resolved<'a> {
    pub fn is_found(&self) -> bool {
        matches!(self, Resolved::Found(_))
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self, Resolved::Unknown)
    }

    /// Convert to the generic `Option` shape, for a caller that only needs
    /// the descriptor and not the domain distinction.
    pub fn found(self) -> Option<&'a Invocable> {
        match self {
            Resolved::Found(invocable) => Some(invocable),
            Resolved::Unknown => None,
        }
    }
}

#[cfg(test)]
mod invocable_tests {
    use super::*;

    #[test]
    fn invocable_id_round_trips_through_display_and_as_str() {
        let id = InvocableId::new("core:board.list").expect("well-formed invocable id");
        assert_eq!(id.as_str(), "core:board.list");
        assert_eq!(id.to_string(), "core:board.list");
    }

    #[test]
    fn invocable_id_equality_is_by_qualified_string() {
        assert_eq!(
            InvocableId::new("core:board.list".to_string()).expect("well-formed invocable id"),
            InvocableId::new("core:board.list").expect("well-formed invocable id")
        );
        assert_ne!(
            InvocableId::new("core:board.list").expect("well-formed invocable id"),
            InvocableId::new("ext:board.list").expect("well-formed invocable id")
        );
    }

    #[test]
    fn invocable_id_refuses_a_string_with_no_separator() {
        assert_eq!(
            InvocableId::new("noqualifier").unwrap_err(),
            InvocableIdError::MissingSeparator
        );
    }

    #[test]
    fn invocable_id_refuses_an_empty_qualifier() {
        assert_eq!(
            InvocableId::new(":board.list").unwrap_err(),
            InvocableIdError::EmptyQualifier
        );
    }

    #[test]
    fn invocable_id_refuses_an_empty_tail() {
        assert_eq!(
            InvocableId::new("core:").unwrap_err(),
            InvocableIdError::EmptyTail
        );
    }

    #[test]
    fn invocable_id_refuses_a_second_separator_in_the_tail() {
        // "a:b:c" splits on the FIRST ':' into qualifier "a", tail "b:c" —
        // the tail itself still contains a ':', which is refused rather
        // than silently accepted as an ambiguous three-part id.
        assert_eq!(
            InvocableId::new("a:b:c").unwrap_err(),
            InvocableIdError::ExtraSeparator
        );
    }

    #[test]
    fn invocable_id_refuses_the_empty_string() {
        assert_eq!(
            InvocableId::new("").unwrap_err(),
            InvocableIdError::MissingSeparator
        );
    }

    #[test]
    fn invocable_id_try_from_agrees_with_new() {
        let via_new = InvocableId::new("core:board.list").expect("well-formed invocable id");
        let via_try_from_str: InvocableId = "core:board.list".try_into().expect("well-formed");
        let via_try_from_string: InvocableId = "core:board.list"
            .to_string()
            .try_into()
            .expect("well-formed");
        assert_eq!(via_new, via_try_from_str);
        assert_eq!(via_new, via_try_from_string);
        assert_eq!(
            InvocableId::try_from("bad").unwrap_err(),
            InvocableIdError::MissingSeparator
        );
    }

    #[test]
    fn installation_locus_is_distinct_from_the_other_three() {
        assert_ne!(Locus::Installation, Locus::Semantic);
        assert_ne!(Locus::Installation, Locus::ClientLocal);
        assert_ne!(
            Locus::Installation,
            Locus::HostOnly {
                reason: "unrelated"
            }
        );
    }

    #[test]
    fn host_only_locus_carries_a_stated_reason() {
        let locus = Locus::HostOnly {
            reason: "shell-owned marshalling, not core logic",
        };
        let Locus::HostOnly { reason } = locus else {
            panic!("expected HostOnly");
        };
        assert!(!reason.is_empty());
    }

    #[test]
    fn retired_stability_names_its_replacement() {
        let stability = Stability::Retired {
            superseded_by: InvocableId::new("core:board.list").expect("well-formed invocable id"),
        };
        assert_eq!(
            stability,
            Stability::Retired {
                superseded_by: InvocableId::new("core:board.list")
                    .expect("well-formed invocable id")
            }
        );
    }

    #[test]
    fn descriptor_carries_ordered_binders_and_their_optionality() {
        let invocable = Invocable {
            id: InvocableId::new("core:board.add").expect("well-formed invocable id"),
            name: "Add card",
            summary: "Add a card to the board",
            group: "board",
            locus: Locus::Semantic,
            binders: vec![
                Binder {
                    name: "id",
                    kind: BinderKind::Text,
                    optional: false,
                },
                Binder {
                    name: "task_ref",
                    kind: BinderKind::Text,
                    optional: true,
                },
            ],
            stability: Stability::Shipped,
            journal_raw_input: true,
        };
        assert_eq!(invocable.binders.len(), 2);
        assert!(!invocable.binders[0].optional);
        assert!(invocable.binders[1].optional);
    }

    #[test]
    fn resolved_found_and_unknown_are_distinguishable_and_convert_to_option() {
        let invocable = Invocable {
            id: InvocableId::new("core:board.list").expect("well-formed invocable id"),
            name: "List cards",
            summary: "List cards on the board",
            group: "board",
            locus: Locus::Semantic,
            binders: Vec::new(),
            stability: Stability::Shipped,
            journal_raw_input: true,
        };

        let found = Resolved::Found(&invocable);
        assert!(found.is_found());
        assert!(!found.is_unknown());

        let unknown: Resolved<'_> = Resolved::Unknown;
        assert!(unknown.is_unknown());
        assert!(!unknown.is_found());

        assert_eq!(
            Resolved::Found(&invocable).found().map(|i| i.name),
            Some("List cards")
        );
        assert_eq!(Resolved::<'_>::Unknown.found(), None);
    }
}

// ── Dispatch envelope ─────────────────────────────────────
//
// `Invocation` is the runtime call; `Outcome` is what dispatch returns —
// structured data, never rendered text, so one dispatch serves a text
// renderer, a structured renderer, terminal widgets, and an IPC payload
// without any of them re-deriving the others' content. `Rejection` is scoped
// strictly to binder failures (bind-before-invoke: the body never ran); a
// resource the body itself depends on being unreachable is a different kind
// of failure with no binder and no input location to name, so it is its own
// `Outcome` variant rather than a rejection with an invented location.

/// The known callers of dispatch. Closed by design: a new surface joining
/// the shared registry is a deliberate event, not something a caller
/// declares for itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Cli,
    Tui,
    Desktop,
}

/// One argument value, shaped like the [`BinderKind`] it binds.
///
/// `PartialEq` only, not `Eq`: `Float` carries `f64`, which has no total
/// equality (`NaN != NaN`) — the same reason `f64` itself is `PartialEq`
/// and not `Eq`. Nothing in this crate ever put an `ArgValue`/`ArgValues`
/// in a `HashSet`/`HashMap` key position (neither derives `Hash`), so
/// dropping `Eq` costs no real capability, only the derive.
///
/// This is the JS → Rust argument half of the desktop IPC seam
/// (`capability_invoke`'s own `args` parameter): every field is owned data
/// (no `&'static str`), so unlike [`Invocable`] there is no per-instance
/// leak to avoid deriving `Deserialize`. `Serialize` travels alongside it —
/// no production call site in this crate sends an `ArgValue` back out, but
/// deriving it costs nothing here and is what lets this module's own test
/// prove a real round trip rather than asserting against hand-built JSON.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ArgValue {
    Text(String),
    Integer(i64),
    Boolean(bool),
    Flag,
    Float(f64),
    /// Bound by a [`BinderKind::RepeatableNamedText`] binder, in the order
    /// supplied.
    List(Vec<String>),
}

/// The caller-supplied arguments for one invocation, keyed by binder name.
/// `PartialEq` only — see [`ArgValue`]'s own doc comment on why.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ArgValues(HashMap<String, ArgValue>);

impl ArgValues {
    pub fn new() -> Self {
        ArgValues(HashMap::new())
    }

    pub fn insert(&mut self, binder: impl Into<String>, value: ArgValue) {
        self.0.insert(binder.into(), value);
    }

    pub fn get(&self, binder: &str) -> Option<&ArgValue> {
        self.0.get(binder)
    }

    /// Every bound argument by name, in no particular order. No in-process
    /// caller ever needed this — `bind()` and every handler read by name —
    /// so it did not exist until a caller crossing a real wire boundary
    /// (a corpus registration driving a real IPC bridge's own `invoke()`,
    /// which takes the wire's flat map shape, not this type) needed to
    /// enumerate a full set rather than look up one name at a time.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &ArgValue)> {
        self.0.iter().map(|(name, value)| (name.as_str(), value))
    }
}

/// A runtime call against one invocable. `PartialEq` only — carries
/// `ArgValues`, which carries `ArgValue::Float(f64)`; see `ArgValue`'s own
/// doc comment.
#[derive(Debug, Clone, PartialEq)]
pub struct Invocation {
    pub id: InvocableId,
    pub args: ArgValues,
    pub caller: Surface,
}

/// A handle to the existing push-channel transport (no second streaming
/// mechanism is introduced here) — a caller subscribes to `channel` on that
/// transport to receive the invocation's stream.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct StreamHandle {
    pub channel: String,
}

/// A bounded structured value tree — general enough for a text renderer, a
/// structured renderer, and an IPC payload to each project without any of
/// them recomputing the others' content, and no more general than the
/// shapes a command's output actually takes.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum OutcomeValue {
    /// A genuinely empty result — distinct from [`Outcome::Unavailable`],
    /// which means the result could not be obtained at all.
    Empty,
    Text(String),
    Integer(i64),
    /// A real number — rendered as a bare JSON number, never a quoted string.
    /// Handlers with a fractional value (a budget amount, a confidence score)
    /// use this rather than `Text(format!("{x:.4}"))`.
    Float(f64),
    Boolean(bool),
    List(Vec<OutcomeValue>),
    Record(Vec<(String, OutcomeValue)>),
}

/// The closed set of reasons a binder failed to produce a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum RejectionMode {
    /// Nothing was supplied for a binder that requires a value.
    Absent,
    /// The source named by the caller could not be obtained at all.
    Unreadable,
    /// Present, but not parseable as anything.
    Malformed,
    /// Parseable, but not conforming to the binder's declared shape.
    IllShaped,
}

/// A binding failure, naming its mode and the binder it concerns (its
/// location). Travels the normal result channel as a structured value
///  — never an unstructured fault.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Rejection {
    /// The declared binder this rejection concerns (its location; this
    /// crate's [`Binder`] is flat and named, so the binder name is exactly
    /// the location).
    pub binder: &'static str,
    pub mode: RejectionMode,
    pub detail: String,
}

/// What dispatch returns for a **resolved** invocation — an
/// invocable the registry does know exists. `Rejected` means a declared
/// binder failed, so the body never ran at all — it always names a
/// binder and a location. `Unavailable` covers everything else that keeps a
/// result from existing without that being the caller's input at fault: a
/// resource the body depends on could not be reached, or a contribution's
/// handler panicked or exceeded its time bound. Neither case has a binder
/// or a location to name, which is exactly why it is not `Rejected`.
/// Collapsing `Unavailable` into an empty [`OutcomeValue`] with a
/// success-shaped `Value` is exactly the quiet failure this split forbids.
///
/// An invocation naming nothing the registry knows produces no `Outcome`
/// at all — see [`Resolved`] and [`Dispatched`]. Folding that case in here
/// was this type's shape before the distinction was drawn; an
/// unresolved invocation is not an incident, and three different surfaces
/// react to it three different ways, none of which is "render an error".
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum Outcome {
    Value(OutcomeValue),
    Stream(StreamHandle),
    Rejected(Rejection),
    Unavailable { reason: String },
}

/// What `Dispatcher::dispatch` returns: resolution, asked first,
/// before whether a resolved invocation produced an ordinary [`Outcome`].
/// `Unknown` is not a failure — nothing ran, nothing was rejected, nothing
/// was journaled — and each surface acts on it differently (fall through to
/// ordinary input, a usage error, a catalog refresh); collapsing it into
/// `Outcome::Unavailable` would force one of the three to behave
/// incorrectly.
#[derive(Debug, Clone, PartialEq)]
pub enum Dispatched {
    /// The invocation named nothing the registry currently knows.
    Unknown,
    /// The invocable resolved; this is its ordinary dispatch outcome.
    Ran(Outcome),
}

impl Dispatched {
    pub fn is_unknown(&self) -> bool {
        matches!(self, Dispatched::Unknown)
    }

    /// Convert to the generic `Option` shape, for a caller that only needs
    /// the outcome and not the domain distinction.
    pub fn outcome(self) -> Option<Outcome> {
        match self {
            Dispatched::Ran(outcome) => Some(outcome),
            Dispatched::Unknown => None,
        }
    }
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;

    #[test]
    fn empty_result_and_unavailable_are_distinct_and_not_interconvertible() {
        let empty = Outcome::Value(OutcomeValue::Empty);
        let unavailable = Outcome::Unavailable {
            reason: "store connection failed".to_string(),
        };
        assert_ne!(empty, unavailable);
        assert!(matches!(empty, Outcome::Value(OutcomeValue::Empty)));
        assert!(matches!(unavailable, Outcome::Unavailable { .. }));
    }

    #[test]
    fn rejection_names_one_of_four_modes_with_its_binder_location() {
        for mode in [
            RejectionMode::Absent,
            RejectionMode::Unreadable,
            RejectionMode::Malformed,
            RejectionMode::IllShaped,
        ] {
            let rejection = Rejection {
                binder: "id",
                mode,
                detail: "test".to_string(),
            };
            assert_eq!(rejection.binder, "id");
            assert_eq!(rejection.mode, mode);
        }
    }

    #[test]
    fn arg_values_round_trips_by_binder_name() {
        let mut args = ArgValues::new();
        args.insert("id", ArgValue::Text("card-1".to_string()));
        assert_eq!(args.get("id"), Some(&ArgValue::Text("card-1".to_string())));
        assert_eq!(args.get("missing"), None);
    }

    #[test]
    fn arg_values_iter_visits_every_bound_argument_exactly_once() {
        let mut args = ArgValues::new();
        args.insert("id", ArgValue::Text("card-1".to_string()));
        args.insert("count", ArgValue::Integer(3));

        let mut seen: Vec<(String, ArgValue)> = args
            .iter()
            .map(|(name, value)| (name.to_string(), value.clone()))
            .collect();
        seen.sort_by(|a, b| a.0.cmp(&b.0));

        assert_eq!(
            seen,
            vec![
                ("count".to_string(), ArgValue::Integer(3)),
                ("id".to_string(), ArgValue::Text("card-1".to_string())),
            ]
        );
    }

    #[test]
    fn arg_values_iter_is_empty_for_a_fresh_instance() {
        assert_eq!(ArgValues::new().iter().count(), 0);
    }

    #[test]
    fn invocation_names_its_target_arguments_and_caller() {
        let mut args = ArgValues::new();
        args.insert("id", ArgValue::Text("card-1".to_string()));
        let invocation = Invocation {
            id: InvocableId::new("core:board.show").expect("well-formed invocable id"),
            args,
            caller: Surface::Cli,
        };
        assert_eq!(invocation.id.as_str(), "core:board.show");
        assert_eq!(invocation.caller, Surface::Cli);
    }

    #[test]
    fn dispatched_unknown_and_ran_are_distinguishable_and_convert_to_option() {
        let unknown = Dispatched::Unknown;
        assert!(unknown.is_unknown());
        assert_eq!(Dispatched::Unknown.outcome(), None);

        let ran = Dispatched::Ran(Outcome::Value(OutcomeValue::Empty));
        assert!(!ran.is_unknown());
        assert_eq!(
            Dispatched::Ran(Outcome::Value(OutcomeValue::Empty)).outcome(),
            Some(Outcome::Value(OutcomeValue::Empty))
        );
    }

    #[test]
    fn outcome_value_represents_nested_records_and_lists() {
        let value = OutcomeValue::Record(vec![
            ("id".to_string(), OutcomeValue::Text("card-1".to_string())),
            (
                "tags".to_string(),
                OutcomeValue::List(vec![OutcomeValue::Text("urgent".to_string())]),
            ),
        ]);
        match value {
            OutcomeValue::Record(fields) => assert_eq!(fields.len(), 2),
            _ => panic!("expected Record"),
        }
    }
}

#[cfg(all(test, feature = "serde"))]
mod serde_tests {
    //! `Invocable`/`Outcome` cross the desktop IPC seam Rust → JS only (the
    //! shell's `catalog()` and the return half of `invoke()`). So these types
    //! derive `Serialize` and deliberately not `Deserialize` — `&'static str`
    //! fields (declared literals, not owned data) cannot honestly be
    //! reconstructed from arbitrary wire input without leaking memory per
    //! instance, and the real data flow never asks them to. A true
    //! round-trip is not the property to test here; that a shipped
    //! descriptor and every `Outcome` variant serialize to the expected JSON
    //! shape is.
    //!
    //! The argument half (JS → Rust) is `ArgValue` alone, not the whole
    //! `Invocation` — it derives both directions (see its own doc comment)
    //! and is tested for a real round-trip below, since unlike `Invocable`
    //! it owns every field. `Invocation.caller: Surface` deliberately does NOT
    //! travel the wire at all: the bridge asserts `Surface::Desktop` itself
    //! rather than deserializing a caller identity a script could otherwise
    //! forge, so `Invocation`/`Surface` carry no serde derive here.

    use super::*;

    #[test]
    fn descriptor_serializes_every_field_including_nested_binders() {
        let invocable = Invocable {
            id: InvocableId::new("core:board.add").expect("well-formed invocable id"),
            name: "Add card",
            summary: "Add a card to the board",
            group: "board",
            locus: Locus::HostOnly {
                reason: "shell-owned marshalling, not core logic",
            },
            binders: vec![Binder {
                name: "id",
                kind: BinderKind::Text,
                optional: false,
            }],
            stability: Stability::Retired {
                superseded_by: InvocableId::new("core:board.show")
                    .expect("well-formed invocable id"),
            },
            journal_raw_input: true,
        };
        let json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&invocable).expect("serialize"))
                .expect("valid json");
        assert_eq!(json["name"], "Add card");
        assert_eq!(json["binders"][0]["name"], "id");
        assert_eq!(json["binders"][0]["optional"], false);
    }

    #[test]
    fn every_outcome_variant_serializes_to_distinct_json() {
        let variants: Vec<Outcome> = vec![
            Outcome::Value(OutcomeValue::Empty),
            Outcome::Value(OutcomeValue::Record(vec![(
                "id".to_string(),
                OutcomeValue::Text("card-1".to_string()),
            )])),
            Outcome::Stream(StreamHandle {
                channel: "board.events".to_string(),
            }),
            Outcome::Rejected(Rejection {
                binder: "id",
                mode: RejectionMode::Absent,
                detail: "no id supplied".to_string(),
            }),
            Outcome::Unavailable {
                reason: "store connection failed".to_string(),
            },
        ];
        let mut seen = std::collections::HashSet::new();
        for outcome in variants {
            let json = serde_json::to_string(&outcome).expect("serialize");
            assert!(seen.insert(json), "two variants serialized identically");
        }
    }

    /// `ArgValue` is the one type in this module tested for a real
    /// round-trip (JS → Rust is the direction it actually travels) — every
    /// variant, not only `Text`, since a wire payload can name any binder
    /// shape.
    #[test]
    fn every_arg_value_variant_round_trips_from_json() {
        let variants = vec![
            ArgValue::Text("card-1".to_string()),
            ArgValue::Integer(42),
            ArgValue::Boolean(true),
            ArgValue::Flag,
            ArgValue::Float(1.5),
            ArgValue::List(vec!["a".to_string(), "b".to_string()]),
        ];
        for value in variants {
            let json = serde_json::to_string(&value).expect("serialize");
            let round_tripped: ArgValue = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(value, round_tripped);
        }
    }
}
