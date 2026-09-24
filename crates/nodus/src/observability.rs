//! Execution observability — the `AuditProvider` trait and closed event taxonomy.
//!
//! Every workflow run can be observed by plugging in an [`AuditProvider`]
//! implementation. The built-in [`NoopAuditProvider`] discards all events and
//! satisfies the interface without any I/O, keeping the no-audit path at the
//! cost of a single indirect call per event.
//!
//! # Observer neutrality
//!
//! The presence or absence of an `AuditProvider` must never alter a run's
//! `@out` values, execution branch outcomes, or error codes. All `record_event`
//! calls are write-only; no return value flows back into the executor.

// ─── Extension point ──────────────────────────────────────────────────────────

/// Stateful observer registered once per workflow run.
///
/// The executor calls [`AuditProvider::record_event`] synchronously for each
/// observable event (before returning control to the next step) and calls
/// [`AuditProvider::run_complete`] exactly once when the run terminates.
///
/// # Contract
///
/// - `record_event` is on the hot path; implementations should avoid blocking I/O.
/// - `run_complete` is always called — even when the run errors or halts.
/// - The same instance must not be shared across concurrent runs.
pub trait AuditProvider {
    /// Record a single execution event. Called synchronously in emission order.
    fn record_event(&self, event: ExecutionEvent);

    /// Called once when the run terminates; carries the run-level summary.
    fn run_complete(&self, manifest: RunManifest);
}

/// Built-in no-op implementation — discards all events with zero allocation.
pub struct NoopAuditProvider;

impl AuditProvider for NoopAuditProvider {
    #[inline]
    fn record_event(&self, _event: ExecutionEvent) {}

    #[inline]
    fn run_complete(&self, _manifest: RunManifest) {}
}

// ─── Aggregation-safe measurement ─────────────────────────────────────

/// A numeric that was either genuinely measured, or explicitly could not be.
///
/// `Unavailable` is **never** rendered as `0`, omitted, or carried forward from
/// a previous observation — each of those corrupts a downstream aggregate
/// (mean, minimum, coverage) while looking exactly like data. *Not
/// applicable* is a different thing entirely: a field the taxonomy does not
/// define for a given event type is simply absent from that variant, and
/// never carries this marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Measurement {
    Taken(u64),
    Unavailable,
}

// ─── Event annotations ────────────────────────────

/// Host-supplied verdict on an event's measurement, relative to that step's
/// own history. nodus computes no verdict, holds no history, and
/// names no model, threshold, or window — it reserves only the
/// carrier. `Unscored` is **never** `Normal`: an absent verdict is emitted as
/// absence, matching [`Measurement::Unavailable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anomaly {
    Anomalous,
    Normal,
    Unscored,
}

/// Durable (part of the record) vs. transient (a live affordance, never
/// persisted or positioned in the sequence). A transient event must
/// **never** consume a `seq` (see [`crate::executor::Executor::emit`]'s doc
/// comment for the load-bearing consequence). Default `Durable` — nodus
/// emits no transients today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Durability {
    #[default]
    Durable,
    Transient,
}

/// Annotations that may ride any [`ExecutionEvent`] — **one carrier field per
/// variant** rather than a separate field per concern, so the all-variant
/// churn happens once and a future annotation is a struct field, not a
/// tenth-variant edit. Most fields are host-supplied (receipt,
/// message, anomaly, durability); [`Self::dialog_provenance`] is
/// the first the crate's own dispatch logic populates directly, since only
/// the executor — not a host observing the stream after the fact — knows
/// which `DialogOutcome` variant resolved a step.
///
/// `Default` is all-`None` + `Durable`: a host declaring nothing produces a
/// stream identical to one with no annotations at all (observer neutrality preserved).
#[derive(Debug, Clone, Default)]
pub struct EventAnnotations {
    /// Host-rendered one-line human projection of *this event's own*
    /// structured fields. Adds no fact the fields lack; contradicts none —
    /// a faithful projection, not a second record. Within the data-safety boundary (descriptors
    /// and counts, never raw content). No renderer/locale vocabulary in core.
    pub message: Option<String>,
    /// Host-supplied verdict. `None` = not annotated at all
    /// (distinct from `Some(Anomaly::Unscored)` = annotated as "no verdict").
    pub anomaly: Option<Anomaly>,
    /// Opaque, secret-free, host-supplied authenticity token binding
    /// step identity to observed result. No crypto in core (the
    /// attestation precedent); the signing secret never enters a trace,
    /// prompt, or context.
    pub receipt: Option<String>,
    /// Durable vs. transient.
    pub durability: Durability,
    /// Whether a dialog step's `StepEnd` resolved fresh or from a host
    /// durable prior decision. `None` for every non-dialog event and for a
    /// dialog step that did not resolve (`Pause`/`Timeout`/`Rejected` decide
    /// nothing, so they carry no provenance).
    pub dialog_provenance: Option<DialogProvenance>,
}

/// The provenance of a resolved dialog answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogProvenance {
    /// Resolved fresh — by a human, or a `+default`.
    Answered,
    /// Resolved from a host durable prior decision (`+remember`), without
    /// re-prompting.
    Remembered,
}

/// A reference to the source element a produced element derived from
///  — an index within the mapping domain, **never** a copy of element
/// content (staying inside the data-safety boundary).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceRef {
    pub producing_step: u32,
    pub source_index: u32,
}

// ─── Trace completeness ────────────────────────────────────────────────

/// Whether a persisted trace is trustworthy as a whole run, decidable from
/// the trace alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceCompleteness {
    /// A terminal manifest is present and the durable sequence has no gap.
    Complete,
    /// A terminal manifest is present but `event_count` disagrees with the
    /// highest durable `seq` + 1 — events were lost inside the recorded range.
    GapDamaged,
    /// Durable events exist but no terminal manifest was ever written — a
    /// killed, panicked, or OOM-killed host never reached `run_complete`.
    Truncated,
    /// Neither events nor a manifest.
    Empty,
}

/// Classify a trace from its durable events and (if present) terminal
/// manifest — pure, read-side, no new emission (observer neutrality untouched). Reuses
/// the `event_count == highest seq + 1` identity as the gap test.
///
/// `durable_events` must already exclude any transient events — this
/// function does not filter for you; passing a stream that still contains
/// transients would misclassify a healthy trace as gap-damaged, since
/// transients never consume a `seq` and would appear as holes.
///
/// nodus does not capture the fault that truncated a trace — that is the
/// host's forensic diagnostic-log plane. This classifies only; it does not
/// diagnose.
pub fn classify_trace(
    durable_events: &[ExecutionEvent],
    manifest: Option<&RunManifest>,
) -> TraceCompleteness {
    let highest_seq = durable_events.iter().map(event_seq).max();
    match (manifest, highest_seq) {
        (Some(m), Some(hs)) if m.event_count as u64 == hs + 1 => TraceCompleteness::Complete,
        (Some(m), None) if m.event_count == 0 => TraceCompleteness::Complete,
        (Some(_), _) => TraceCompleteness::GapDamaged,
        (None, Some(_)) => TraceCompleteness::Truncated,
        (None, None) => TraceCompleteness::Empty,
    }
}

/// Extract `seq` from any `ExecutionEvent` variant — every one carries it.
fn event_seq(event: &ExecutionEvent) -> u64 {
    match event {
        ExecutionEvent::StepStart { seq, .. }
        | ExecutionEvent::StepEnd { seq, .. }
        | ExecutionEvent::StepError { seq, .. }
        | ExecutionEvent::ConstraintHit { seq, .. }
        | ExecutionEvent::BranchTaken { seq, .. }
        | ExecutionEvent::LoopIteration { seq, .. }
        | ExecutionEvent::MacroEnter { seq, .. }
        | ExecutionEvent::MacroExit { seq, .. }
        | ExecutionEvent::ModelCall { seq, .. }
        | ExecutionEvent::ModelResponse { seq, .. } => *seq,
    }
}

// ─── Event taxonomy ───────────────────────────────────────────────────────────

/// Discriminant for loop types (used in [`ExecutionEvent::LoopIteration`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopType {
    /// A `~FOR` bounded-collection loop.
    For,
    /// A `~UNTIL MAX:n` conditional loop.
    Until,
    /// A `~MAP` collection transform (each iteration is a true
    /// element-to-element mapping, distinct from plain iteration).
    Map,
}

/// Structural descriptor for model I/O fields.
///
/// Never contains raw user text — only shape metadata. This enforces the
/// data-safety boundary: model call traces can be shared without leaking
/// workflow content.
#[derive(Debug, Clone)]
pub struct FieldDescriptor {
    /// Number of top-level fields.
    pub field_count: u32,
    /// Type hints for each field (e.g. `["Text"]`, `["Map"]`).
    pub type_hints: Vec<String>,
    /// Approximate total byte count (content length, not content).
    pub total_bytes: u32,
}

impl FieldDescriptor {
    /// Single text field of `byte_len` bytes.
    pub fn text(byte_len: u32) -> Self {
        FieldDescriptor {
            field_count: 1,
            type_hints: vec!["Text".to_string()],
            total_bytes: byte_len,
        }
    }

    /// Map-like multi-field descriptor.
    pub fn map_like(field_count: u32, byte_len: u32) -> Self {
        FieldDescriptor {
            field_count,
            type_hints: vec!["Map".to_string()],
            total_bytes: byte_len,
        }
    }
}

/// Closed set of observable execution events. Adding a new variant is a
/// spec-level minor-version amendment.
///
/// Every variant carries `seq` (run-monotonic, dense, gap-free),
/// `correlation_id` (shared by every event of one run, equal to
/// `RunManifest.run_id`), and `annotations` (host-supplied receipt /
/// message / anomaly / durability). All three are assigned by
/// [`crate::executor::Executor`]'s single emission choke point, never by a
/// caller — there is no public constructor that lets a caller pick any of them.
#[derive(Debug, Clone)]
pub enum ExecutionEvent {
    /// Before a step's command is dispatched.
    StepStart {
        step_index: u32,
        step_command: String,
        /// Snapshot of variable names bound at this point (no values).
        input_vars: Vec<String>,
        /// Definition-derived, stable cross-run identity.
        step_identity: String,
        seq: u64,
        correlation_id: String,
        annotations: EventAnnotations,
    },
    /// After a step's command returns successfully.
    StepEnd {
        step_index: u32,
        step_command: String,
        /// Pipeline target names written by this step (empty if no `→`).
        output_vars: Vec<String>,
        elapsed_ms: Measurement,
        /// Definition-derived, stable cross-run identity.
        step_identity: String,
        seq: u64,
        correlation_id: String,
        annotations: EventAnnotations,
    },
    /// When a step produces a NODUS error code.
    StepError {
        step_index: u32,
        step_command: String,
        /// NODUS:* error code from the vocabulary taxonomy.
        error_code: String,
        error_detail: String,
        /// Definition-derived, stable cross-run identity.
        step_identity: String,
        /// Stable, message-independent grouping input.
        fault_identity: FaultIdentity,
        seq: u64,
        correlation_id: String,
        annotations: EventAnnotations,
    },
    /// When a hard (`!!NEVER`/`!!ALWAYS`) constraint fires.
    ConstraintHit {
        rule_name: String,
        triggering_step_index: u32,
        /// `true` when the executor halted execution as a result.
        halt: bool,
        seq: u64,
        correlation_id: String,
        annotations: EventAnnotations,
    },
    /// When a conditional (`?IF`/`?ELIF`/`?ELSE`) resolves.
    BranchTaken {
        step_index: u32,
        /// `"if"`, `"elif"`, or `"else"`.
        branch_label: String,
        condition_result: bool,
        seq: u64,
        correlation_id: String,
        annotations: EventAnnotations,
    },
    /// At the start of each `~FOR`/`~UNTIL`/`~MAP` loop body.
    LoopIteration {
        step_index: u32,
        loop_type: LoopType,
        iteration_number: Measurement,
        /// Variables bound for this iteration (e.g. the `~FOR` loop variable).
        bound_vars: Vec<String>,
        /// For a collection-mapping construct (`~MAP`), the source
        /// element(s) this iteration's produced element derived from —
        /// indices only, never content. `None` for plain iteration
        /// (`~FOR`/`~UNTIL`), which produces no mapped output to derive.
        derivation: Option<Vec<SourceRef>>,
        seq: u64,
        correlation_id: String,
        annotations: EventAnnotations,
    },
    /// When `RUN(@macro_name)` begins execution.
    MacroEnter {
        macro_name: String,
        call_step_index: u32,
        seq: u64,
        correlation_id: String,
        annotations: EventAnnotations,
    },
    /// When a macro body completes.
    MacroExit {
        macro_name: String,
        call_step_index: u32,
        elapsed_ms: Measurement,
        seq: u64,
        correlation_id: String,
        annotations: EventAnnotations,
    },
    /// Immediately before dispatching to the `ModelProvider` (GEN, ANALYZE, …).
    ModelCall {
        step_index: u32,
        /// Command that triggered the model call.
        command: String,
        /// Structural descriptor — no raw user text (data-safety boundary).
        input_summary: FieldDescriptor,
        seq: u64,
        correlation_id: String,
        annotations: EventAnnotations,
    },
    /// Immediately after the `ModelProvider` returns.
    ModelResponse {
        step_index: u32,
        command: String,
        /// Structural descriptor of the response — no raw content.
        output_summary: FieldDescriptor,
        elapsed_ms: Measurement,
        /// Cost-attribution token classes. All `Unavailable` in core
        /// today — `ModelProvider` exposes no token-accounting seam, so
        /// there is nothing to report; never fabricated as `0`. Extending
        /// `ModelProvider` with a token-reporting method is a separate,
        /// out-of-scope change to the extension-point contract.
        input_tokens: Measurement,
        output_tokens: Measurement,
        cache_read_tokens: Measurement,
        cache_creation_tokens: Measurement,
        seq: u64,
        correlation_id: String,
        annotations: EventAnnotations,
    },
}

// ─── Environment trajectory side-band ──────────────────────────────────

/// Discriminant for an environment-lifecycle interaction recorded in the
/// trajectory. `Step` is reserved for a host that drives
/// [`crate::environment::EnvironmentProvider::step`] directly; the built-in
/// `run_with_environment` combinator emits `Reset` only (v1 scope — see
/// `crate::environment` module docs). `evaluate`'s outcome is not represented
/// here: it occurs after the run is frozen — after `run_complete` has already
/// fired — and is delivered directly as the environment run's `Reward` instead,
/// not duplicated into the trajectory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvInteractionKind {
    Reset,
    Step,
}

/// One environment-lifecycle interaction, carried through [`RunManifest`]
/// rather than a parallel store — no new [`ExecutionEvent`] variant is
/// added, so the closed taxonomy is preserved (the additive-field
/// discipline). `observation`/`action` are structural descriptors only — never
/// raw content, matching the [`FieldDescriptor`] data-safety boundary used
/// elsewhere.
#[derive(Debug, Clone)]
pub struct EnvInteraction {
    pub kind: EnvInteractionKind,
    pub observation: Option<FieldDescriptor>,
    pub action: Option<FieldDescriptor>,
}

// ─── Run-manifest identity & reproducibility ─────────────

/// Definition-derived, stable step identity — NOT per-run allocated.
/// The same value across repeated runs, retries/resumes, and recursive
/// children; it changes only when the step's own definition changes
/// (its number or its command name). Deterministic.
///
/// Distinct from the within-run `(correlation_id, seq)` ordering: this is
/// the cross-run comparison key, not a within-run position.
///
/// Takes `(step_number, command_name)` rather than a `&Step` reference — the
/// executor's emission sites carry these two pieces of the definition, not a
/// `Step` value, so deriving identity from them avoids threading an AST
/// reference through the dispatch path for no semantic gain.
pub fn step_identity(step_number: u32, command_name: &str) -> String {
    format!("{step_number}:{command_name}")
}

/// A stable, message-independent grouping input for a failing step.
///
/// Composed only from stable inputs — [`step_identity`], the emitted
/// `NODUS:*` code, and an optional workflow-declared discriminator that
/// outranks the code when present. **Never** derived from `error_detail`
/// rendered text, which routinely carries per-occurrence interpolated values
/// (an id, a path, a count) that would shatter one recurring failure into
/// thousands of singletons. nodus performs no grouping itself — it guarantees
/// only that the inputs a host groups on are stable and content-independent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaultIdentity {
    pub step_identity: String,
    /// The canonical `NODUS:*` code (see [`crate::vocab::error_code`]).
    pub code: String,
    /// A workflow-declared discriminator, if any. No `.nodus` grammar
    /// declares one yet, so this is always `None` today — the carrier exists
    /// ahead of the declaring syntax (an honest, not a silent, gap).
    pub discriminator: Option<String>,
}

/// The execution mode a run declared: real, or simulated at a stated
/// fidelity. nodus substitutes no providers itself — a host that wires
/// modeled providers for a simulation declares the mode; nodus only records
/// what was declared. Absent/default is `Real` (today's behaviour), so a
/// caller declaring nothing is unaffected.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ExecutionMode {
    #[default]
    Real,
    Simulated {
        fidelity: SimFidelity,
    },
}

/// Declared fidelity of a simulated run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimFidelity {
    Structural,
    Modeled,
    Shadow,
}

/// Whether re-running the recorded workflow reproduces the same outcome.
/// **Stated, never inferred** from the mere presence of a
/// [`ReproRecipe`] — nodus's own evaluation is deterministic, but a run
/// that made any model call is not, and the recipe must say which of the two
/// it is rather than let a reader assume exactness.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Determinism {
    #[default]
    Deterministic,
    ContainsModelCalls,
}

/// What re-executing this run requires, recorded from the manifest alone
///  — the host reconstructs nothing. nodus embeds nothing into
/// produced artifacts, names no file format, and performs no replay; it only
/// records what it itself resolved.
///
/// An uncapturable field is `None` — **never silently omitted** — since a
/// short recipe must not read as a complete one (the honesty rule
/// applied at the manifest grain). `needs_vocabulary` is `None` today because
/// `@needs` selective vocabulary loading is not yet implemented; this is a
/// declared omission, not a defect.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReproRecipe {
    /// Content identity of the workflow definition (a `std`-only digest —
    /// the [`crate::environment::CandidateResult`] precedent; zero-dep).
    pub workflow_digest: String,
    /// The capability-manifest roles/commands that satisfied this run.
    pub capability_set: Vec<String>,
    /// Mirrors [`RunManifest::exposure_switches`] into the recipe.
    pub exposure_switches: Vec<(String, String)>,
    /// Mirrors [`RunManifest::execution_mode`] into the recipe.
    pub execution_mode: ExecutionMode,
    /// The producing crate version (`env!("CARGO_PKG_VERSION")`).
    pub nodus_version: String,
    /// Resolved `@needs` vocabulary units. `None` until `@needs` selective
    /// loading is implemented — an honest declared omission.
    pub needs_vocabulary: Option<Vec<String>>,
    /// Stated, never inferred (see the type's own documentation).
    pub determinism: Determinism,
}

// ─── Run manifest ─────────────────────────────────────────────────────────────

/// Overall status reported in the run manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunStatus {
    /// All steps completed without errors.
    Ok,
    /// Steps completed but non-fatal errors occurred.
    Error,
    /// A hard-constraint rule halted execution.
    ConstraintHalt,
    /// Validation errors prevented execution from starting.
    ValidationError,
    /// The run suspended awaiting a human answer (`!PAUSE` or an unresolved
    /// dialog). It has not completed, so it must not read as `Ok`.
    Paused,
}

/// Run-level summary delivered to [`AuditProvider::run_complete`] once per run.
#[derive(Debug, Clone)]
pub struct RunManifest {
    /// `wf:` identifier (without the `wf:` prefix).
    pub workflow_name: String,
    /// `BUILTIN_SCHEMA_VERSION` at run time.
    pub schema_version: String,
    /// Caller-supplied unique identifier (UUID or equivalent).
    pub run_id: String,
    /// ISO-8601 timestamp supplied by the caller at run construction.
    pub started_at: String,
    /// Wall-clock duration of the run.
    pub elapsed_ms: Measurement,
    pub status: RunStatus,
    /// First NODUS:* error code encountered, if any.
    pub error_code: Option<String>,
    /// Number of steps that reached execution (excludes unentered branches).
    pub total_steps: u32,
    /// Total events emitted to [`AuditProvider::record_event`] during this run.
    /// Doubles as the gap-check: for an undamaged trace this equals the
    /// highest emitted `seq` + 1 (both derive from the same counter through
    /// the executor's single emission choke point).
    pub event_count: u32,
    /// Environment-lifecycle interactions for this run. Empty for every
    /// run that did not go through [`crate::environment`]'s combinators —
    /// additive field, observer neutrality preserved for the common case.
    pub env_trajectory: Vec<EnvInteraction>,
    /// Real vs. simulated, and at what fidelity. Default `Real`.
    pub execution_mode: ExecutionMode,
    /// Resolved `(switch_name, value)` pairs the host froze once at run start
    /// (non-straddling). Empty = prevailing defaults.
    pub exposure_switches: Vec<(String, String)>,
    /// What re-executing this run requires, from the manifest alone.
    pub repro: ReproRecipe,
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    // ── RecordingProvider test helper ────────────────────────────────────────

    pub(crate) struct RecordingProvider {
        pub events: Arc<Mutex<Vec<ExecutionEvent>>>,
        pub manifests: Arc<Mutex<Vec<RunManifest>>>,
    }

    impl RecordingProvider {
        pub fn new() -> Self {
            RecordingProvider {
                events: Arc::new(Mutex::new(Vec::new())),
                manifests: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    impl AuditProvider for RecordingProvider {
        fn record_event(&self, event: ExecutionEvent) {
            self.events.lock().unwrap().push(event);
        }
        fn run_complete(&self, manifest: RunManifest) {
            self.manifests.lock().unwrap().push(manifest);
        }
    }

    // ── NoopAuditProvider ────────────────────────────────────────────────────

    /// Test-only helper: a fixed seq/correlation pair, since these tests
    /// exercise event *shape*, not the executor's real assignment discipline
    /// (that lives in `executor.rs`'s integration tests).
    fn sc(seq: u64) -> (u64, String) {
        (seq, "corr-test".to_string())
    }

    #[test]
    fn noop_provider_discards_all() {
        let noop = NoopAuditProvider;
        let (seq, cid) = sc(0);
        noop.record_event(ExecutionEvent::StepStart {
            step_index: 1,
            step_command: "GEN".to_string(),
            input_vars: vec!["query".to_string()],
            step_identity: step_identity(1, "GEN"),
            seq,
            correlation_id: cid,
            annotations: EventAnnotations::default(),
        });
        let (seq, cid) = sc(1);
        noop.record_event(ExecutionEvent::StepEnd {
            step_index: 1,
            step_command: "GEN".to_string(),
            output_vars: vec!["$out".to_string()],
            elapsed_ms: Measurement::Taken(5),
            step_identity: step_identity(1, "GEN"),
            seq,
            correlation_id: cid,
            annotations: EventAnnotations::default(),
        });
        let (seq, cid) = sc(2);
        noop.record_event(ExecutionEvent::StepError {
            step_index: 1,
            step_command: "PUBLISH".to_string(),
            error_code: "NODUS:RULE_VIOLATION".to_string(),
            error_detail: "!!NEVER: publish".to_string(),
            step_identity: step_identity(1, "PUBLISH"),
            fault_identity: FaultIdentity {
                step_identity: step_identity(1, "PUBLISH"),
                code: "NODUS:RULE_VIOLATION".to_string(),
                discriminator: None,
            },
            seq,
            correlation_id: cid,
            annotations: EventAnnotations::default(),
        });
        let (seq, cid) = sc(3);
        noop.record_event(ExecutionEvent::ConstraintHit {
            rule_name: "!!NEVER: publish".to_string(),
            triggering_step_index: 1,
            halt: true,
            seq,
            correlation_id: cid,
            annotations: EventAnnotations::default(),
        });
        let (seq, cid) = sc(4);
        noop.record_event(ExecutionEvent::BranchTaken {
            step_index: 2,
            branch_label: "if".to_string(),
            condition_result: true,
            seq,
            correlation_id: cid,
            annotations: EventAnnotations::default(),
        });
        let (seq, cid) = sc(5);
        noop.record_event(ExecutionEvent::LoopIteration {
            step_index: 3,
            loop_type: LoopType::For,
            iteration_number: Measurement::Taken(0),
            bound_vars: vec!["$item".to_string()],
            derivation: None,
            seq,
            correlation_id: cid,
            annotations: EventAnnotations::default(),
        });
        let (seq, cid) = sc(6);
        noop.record_event(ExecutionEvent::LoopIteration {
            step_index: 4,
            loop_type: LoopType::Until,
            iteration_number: Measurement::Taken(1),
            bound_vars: vec![],
            derivation: None,
            seq,
            correlation_id: cid,
            annotations: EventAnnotations::default(),
        });
        let (seq, cid) = sc(7);
        noop.record_event(ExecutionEvent::MacroEnter {
            macro_name: "refine".to_string(),
            call_step_index: 5,
            seq,
            correlation_id: cid,
            annotations: EventAnnotations::default(),
        });
        let (seq, cid) = sc(8);
        noop.record_event(ExecutionEvent::MacroExit {
            macro_name: "refine".to_string(),
            call_step_index: 5,
            elapsed_ms: Measurement::Taken(2),
            seq,
            correlation_id: cid,
            annotations: EventAnnotations::default(),
        });
        let (seq, cid) = sc(9);
        noop.record_event(ExecutionEvent::ModelCall {
            step_index: 1,
            command: "GEN".to_string(),
            input_summary: FieldDescriptor::text(42),
            seq,
            correlation_id: cid,
            annotations: EventAnnotations::default(),
        });
        let (seq, cid) = sc(10);
        noop.record_event(ExecutionEvent::ModelResponse {
            step_index: 1,
            command: "GEN".to_string(),
            output_summary: FieldDescriptor::text(100),
            elapsed_ms: Measurement::Taken(10),
            input_tokens: Measurement::Unavailable,
            output_tokens: Measurement::Unavailable,
            cache_read_tokens: Measurement::Unavailable,
            cache_creation_tokens: Measurement::Unavailable,
            seq,
            correlation_id: cid,
            annotations: EventAnnotations::default(),
        });
        noop.run_complete(RunManifest {
            workflow_name: "test".to_string(),
            schema_version: "0.4.6".to_string(),
            run_id: "run-0".to_string(),
            started_at: "2026-06-24T00:00:00Z".to_string(),
            elapsed_ms: Measurement::Taken(20),
            status: RunStatus::Ok,
            error_code: None,
            total_steps: 5,
            event_count: 10,
            env_trajectory: Vec::new(),
            execution_mode: ExecutionMode::default(),
            exposure_switches: Vec::new(),
            repro: ReproRecipe::default(),
        });
        // All 10 event variants + run_complete accepted without panic.
    }

    // ── RecordingProvider captures events ────────────────────────────────────

    #[test]
    fn step_start_end_emitted_in_order() {
        let recording = RecordingProvider::new();
        let (seq, cid) = sc(0);
        recording.record_event(ExecutionEvent::StepStart {
            step_index: 1,
            step_command: "GEN".to_string(),
            input_vars: vec![],
            step_identity: step_identity(1, "GEN"),
            seq,
            correlation_id: cid,
            annotations: EventAnnotations::default(),
        });
        let (seq, cid) = sc(1);
        recording.record_event(ExecutionEvent::StepEnd {
            step_index: 1,
            step_command: "GEN".to_string(),
            output_vars: vec!["$out".to_string()],
            elapsed_ms: Measurement::Taken(3),
            step_identity: step_identity(1, "GEN"),
            seq,
            correlation_id: cid,
            annotations: EventAnnotations::default(),
        });

        let events = recording.events.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0],
            ExecutionEvent::StepStart { step_index: 1, .. }
        ));
        assert!(matches!(
            &events[1],
            ExecutionEvent::StepEnd { step_index: 1, .. }
        ));
    }

    #[test]
    fn constraint_hit_halt_true() {
        let recording = RecordingProvider::new();
        let (seq, cid) = sc(0);
        recording.record_event(ExecutionEvent::ConstraintHit {
            rule_name: "!!NEVER: DELETE".to_string(),
            triggering_step_index: 2,
            halt: true,
            seq,
            correlation_id: cid,
            annotations: EventAnnotations::default(),
        });
        let events = recording.events.lock().unwrap();
        match &events[0] {
            ExecutionEvent::ConstraintHit { halt, .. } => assert!(*halt),
            other => panic!("expected ConstraintHit, got {other:?}"),
        }
    }

    #[test]
    fn branch_taken_if_and_else() {
        let recording = RecordingProvider::new();
        let (seq, cid) = sc(0);
        recording.record_event(ExecutionEvent::BranchTaken {
            step_index: 1,
            branch_label: "if".to_string(),
            condition_result: true,
            seq,
            correlation_id: cid,
            annotations: EventAnnotations::default(),
        });
        let (seq, cid) = sc(1);
        recording.record_event(ExecutionEvent::BranchTaken {
            step_index: 2,
            branch_label: "else".to_string(),
            condition_result: false,
            seq,
            correlation_id: cid,
            annotations: EventAnnotations::default(),
        });
        let events = recording.events.lock().unwrap();
        assert!(matches!(
            &events[0],
            ExecutionEvent::BranchTaken {
                condition_result: true,
                ..
            }
        ));
        assert!(matches!(
            &events[1],
            ExecutionEvent::BranchTaken {
                condition_result: false,
                ..
            }
        ));
    }

    #[test]
    fn loop_iteration_for() {
        let recording = RecordingProvider::new();
        for i in 0u64..3 {
            let (seq, cid) = sc(i);
            recording.record_event(ExecutionEvent::LoopIteration {
                step_index: 1,
                loop_type: LoopType::For,
                iteration_number: Measurement::Taken(i),
                bound_vars: vec!["$item".to_string()],
                derivation: None,
                seq,
                correlation_id: cid,
                annotations: EventAnnotations::default(),
            });
        }
        let events = recording.events.lock().unwrap();
        assert_eq!(events.len(), 3);
        for (i, ev) in events.iter().enumerate() {
            match ev {
                ExecutionEvent::LoopIteration {
                    loop_type: LoopType::For,
                    iteration_number,
                    ..
                } => {
                    assert_eq!(*iteration_number, Measurement::Taken(i as u64));
                }
                other => panic!("expected LoopIteration(For), got {other:?}"),
            }
        }
    }

    #[test]
    fn loop_iteration_until() {
        let recording = RecordingProvider::new();
        let (seq, cid) = sc(0);
        recording.record_event(ExecutionEvent::LoopIteration {
            step_index: 2,
            loop_type: LoopType::Until,
            iteration_number: Measurement::Taken(0),
            bound_vars: vec![],
            derivation: None,
            seq,
            correlation_id: cid,
            annotations: EventAnnotations::default(),
        });
        let events = recording.events.lock().unwrap();
        assert!(matches!(
            &events[0],
            ExecutionEvent::LoopIteration {
                loop_type: LoopType::Until,
                ..
            }
        ));
    }

    #[test]
    fn macro_enter_exit_pair() {
        let recording = RecordingProvider::new();
        let (seq, cid) = sc(0);
        recording.record_event(ExecutionEvent::MacroEnter {
            macro_name: "summarize".to_string(),
            call_step_index: 3,
            seq,
            correlation_id: cid,
            annotations: EventAnnotations::default(),
        });
        let (seq, cid) = sc(1);
        recording.record_event(ExecutionEvent::MacroExit {
            macro_name: "summarize".to_string(),
            call_step_index: 3,
            elapsed_ms: Measurement::Taken(7),
            seq,
            correlation_id: cid,
            annotations: EventAnnotations::default(),
        });
        let events = recording.events.lock().unwrap();
        assert!(
            matches!(&events[0], ExecutionEvent::MacroEnter { macro_name, .. } if macro_name == "summarize")
        );
        assert!(
            matches!(&events[1], ExecutionEvent::MacroExit { macro_name, .. } if macro_name == "summarize")
        );
    }

    #[test]
    fn model_call_no_raw_content() {
        let desc = FieldDescriptor::text(256);
        // Confirm FieldDescriptor holds only structural metadata, never the actual text.
        assert_eq!(desc.field_count, 1);
        assert_eq!(desc.type_hints, vec!["Text"]);
        assert_eq!(desc.total_bytes, 256);

        let (seq, cid) = sc(0);
        let event = ExecutionEvent::ModelCall {
            step_index: 1,
            command: "GEN".to_string(),
            input_summary: desc,
            seq,
            correlation_id: cid,
            annotations: EventAnnotations::default(),
        };
        // Verify debug output contains no prose that could be mistaken for content.
        let debug = format!("{event:?}");
        assert!(debug.contains("total_bytes: 256"));
        assert!(!debug.contains("prompt"));
    }

    #[test]
    fn run_manifest_fields_populated() {
        let manifest = RunManifest {
            workflow_name: "hello".to_string(),
            schema_version: "0.4.6".to_string(),
            run_id: "r-001".to_string(),
            started_at: "2026-06-24T12:00:00Z".to_string(),
            elapsed_ms: Measurement::Taken(42),
            status: RunStatus::Ok,
            error_code: None,
            total_steps: 3,
            event_count: 6,
            env_trajectory: Vec::new(),
            execution_mode: ExecutionMode::default(),
            exposure_switches: Vec::new(),
            repro: ReproRecipe::default(),
        };
        let recording = RecordingProvider::new();
        recording.run_complete(manifest);
        let manifests = recording.manifests.lock().unwrap();
        assert_eq!(manifests[0].workflow_name, "hello");
        assert_eq!(manifests[0].run_id, "r-001");
        assert_eq!(manifests[0].event_count, 6);
        assert_eq!(manifests[0].status, RunStatus::Ok);
        assert_eq!(manifests[0].execution_mode, ExecutionMode::Real);
    }

    // ── Measurement ──────────────────────────────────────────────────

    #[test]
    fn measurement_unavailable_is_not_taken_zero() {
        assert_ne!(Measurement::Unavailable, Measurement::Taken(0));
    }

    #[test]
    fn measurement_round_trips_through_recording_provider() {
        let recording = RecordingProvider::new();
        let (seq, cid) = sc(0);
        recording.record_event(ExecutionEvent::StepEnd {
            step_index: 1,
            step_command: "ASK".to_string(),
            output_vars: vec![],
            elapsed_ms: Measurement::Unavailable,
            step_identity: step_identity(1, "ASK"),
            seq,
            correlation_id: cid,
            annotations: EventAnnotations::default(),
        });
        let events = recording.events.lock().unwrap();
        assert!(matches!(
            &events[0],
            ExecutionEvent::StepEnd {
                elapsed_ms: Measurement::Unavailable,
                ..
            }
        ));
    }

    // ── EventAnnotations carrier ───────────────────────

    #[test]
    fn event_annotations_default_is_all_none_and_durable() {
        let a = EventAnnotations::default();
        assert!(a.message.is_none());
        assert!(a.anomaly.is_none());
        assert!(a.receipt.is_none());
        assert_eq!(a.durability, Durability::Durable);
        assert!(a.dialog_provenance.is_none());
    }

    #[test]
    fn dialog_provenance_answered_is_never_remembered() {
        assert_ne!(DialogProvenance::Answered, DialogProvenance::Remembered);
    }

    #[test]
    fn anomaly_unscored_is_never_normal() {
        assert_ne!(Anomaly::Unscored, Anomaly::Normal);
    }

    // ── derivation lineage ─────────────────────────────────────────────

    #[test]
    fn source_ref_carries_indices_only() {
        let r = SourceRef {
            producing_step: 2,
            source_index: 5,
        };
        assert_eq!(r.producing_step, 2);
        assert_eq!(r.source_index, 5);
    }

    // ── classify_trace ─────────────────────────────────────────────────

    fn sample_manifest(event_count: u32) -> RunManifest {
        RunManifest {
            workflow_name: "t".to_string(),
            schema_version: "0.4.6".to_string(),
            run_id: "r".to_string(),
            started_at: "2026-07-25T00:00:00Z".to_string(),
            elapsed_ms: Measurement::Taken(1),
            status: RunStatus::Ok,
            error_code: None,
            total_steps: 1,
            event_count,
            env_trajectory: Vec::new(),
            execution_mode: ExecutionMode::default(),
            exposure_switches: Vec::new(),
            repro: ReproRecipe::default(),
        }
    }

    fn sample_events(n: u64) -> Vec<ExecutionEvent> {
        (0..n)
            .map(|i| ExecutionEvent::ConstraintHit {
                rule_name: "x".to_string(),
                triggering_step_index: 0,
                halt: false,
                seq: i,
                correlation_id: "r".to_string(),
                annotations: EventAnnotations::default(),
            })
            .collect()
    }

    #[test]
    fn classify_trace_complete_with_matching_manifest() {
        let events = sample_events(3);
        let manifest = sample_manifest(3);
        assert_eq!(
            classify_trace(&events, Some(&manifest)),
            TraceCompleteness::Complete
        );
    }

    #[test]
    fn classify_trace_gap_damaged_when_event_count_disagrees() {
        let events = sample_events(3); // seq 0,1,2 -> highest+1 == 3
        let manifest = sample_manifest(5); // claims 5, only 3 durable events present
        assert_eq!(
            classify_trace(&events, Some(&manifest)),
            TraceCompleteness::GapDamaged
        );
    }

    #[test]
    fn classify_trace_truncated_when_events_but_no_manifest() {
        let events = sample_events(2);
        assert_eq!(classify_trace(&events, None), TraceCompleteness::Truncated);
    }

    #[test]
    fn classify_trace_empty_when_neither() {
        assert_eq!(classify_trace(&[], None), TraceCompleteness::Empty);
    }

    #[test]
    fn classify_trace_complete_for_zero_event_manifest() {
        let manifest = sample_manifest(0);
        assert_eq!(
            classify_trace(&[], Some(&manifest)),
            TraceCompleteness::Complete
        );
    }
}
