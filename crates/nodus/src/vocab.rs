//! The language vocabulary as data, kept separate from the logic that consults
//! it. Updating the command set, tones, reserved variables, or transpiler verbs
//! is a data edit here — no logic recompile of the lexer / parser / validator /
//! transpiler, which only call the query methods on [`Schema`]. This realizes
//! the "schema is data, not code" property the runtime relies on so a workflow
//! is always interpreted against a loaded vocabulary contract.
//!
//! This module is the in-tree *builtin* schema; it mirrors the reference
//! vocabulary verbatim so ported workflows lex, validate, and transpile
//! identically. Loading an *external* schema file at run time is a later
//! refinement — [`Schema`]'s query surface is the seam it would back.

/// Version of the builtin vocabulary (tracks the reference schema it mirrors).
pub const BUILTIN_SCHEMA_VERSION: &str = "0.4.7";

/// Known command names (ALL_CAPS). The lexer tags an ALL_CAPS word as a
/// `CommandName` only when it appears here; anything else lexes as a generic
/// identifier. 54 commands total; `RUN` is the macro meta-command that
/// bypasses schema vocabulary checks and is recognized before the validation pass.
/// `ASK` / `CONFIRM` are the human-in-the-loop dialog commands; `SETTLE` is the
/// outbound-value-settlement command (LP-17).
pub const KNOWN_COMMANDS: &[&str] = &[
    "FETCH",
    "STORE",
    "LOAD",
    "APPEND",
    "MERGE",
    "ANALYZE",
    "SCORE",
    "COMPARE",
    "GEN",
    "REFINE",
    "TRANSLATE",
    "SUMMARIZE",
    "VALIDATE",
    "ROUTE",
    "ESCALATE",
    "NOTIFY",
    "PUBLISH",
    "QUERY_KB",
    "REMEMBER",
    "RECALL",
    "FORGET",
    "LOG",
    "WAIT",
    "TONE",
    "DEBUG",
    "WRITE",
    "MKDIR",
    "FILE_EXISTS",
    "FILL",
    "PARSE",
    "EXECUTE",
    "SIMULATE",
    "EXTRACT",
    "FILTER",
    "EXECUTE_TEST",
    "READ_FILE",
    "SCAN_DIR",
    "ENV",
    "DATE",
    "COUNTER",
    "GIT",
    "QUERY_GIT",
    "MOVE",
    "COPY",
    "TRANSPILE",
    "HASH",
    "PARSE_MD_HEADER",
    "PARSE_INDEX",
    "VERSION_BUMP",
    "GENERATE_DOC",
    "RUN",
    "ASK",
    "CONFIRM",
    "SETTLE",
];

/// Tone values accepted by the `+tone=` modifier and the `TONE` command.
pub const VALID_TONES: &[&str] = &[
    "warm",
    "neutral",
    "formal",
    "casual",
    "urgent",
    "empathetic",
    "brand",
];

/// Closed registry of analysis-flag extractors (`~flag`). The validator checks
/// `~flag` tokens against this set (advisory). Host extensions are a future
/// `SchemaProvider` follow-on; the builtin set is never mutated (LP-4).
pub const KNOWN_FLAGS: &[&str] = &[
    "sentiment",
    "intent",
    "entities",
    "topics",
    "lang",
    "toxicity",
    "urgency",
    "formality",
    "clarity",
    "relevance",
    "pii",
    "keywords",
];

/// Closed registry of validator names (`^validator`). A validator token may
/// carry a colon-delimited argument (`len:32`, `lang:en`); this set holds the
/// name (the segment before the first `:`).
pub const KNOWN_VALIDATORS: &[&str] = &[
    "len",
    "min_len",
    "no_pii",
    "no_toxic",
    "lang",
    "format",
    "required",
    "sentiment",
    "confidence",
    "no_links",
    "brand_voice",
    "approved",
];

/// Closed registry of primitive `@in` field types (NL-7 closed value space).
pub const PRIMITIVE_TYPES: &[&str] = &[
    "str", "int", "float", "bool", "list", "obj", "url", "ts", "null", "any",
];

/// Reserved variable names (with leading `$`) that carry runtime-defined meaning.
pub const RESERVED_VARIABLES: &[&str] = &[
    "$in",
    "$out",
    "$error",
    "$meta",
    "$raw",
    "$draft",
    "$ctx",
    "$user",
    "$session",
    "$log",
    "$flags",
    "$quality",
    "$sentiment",
    "$confidence",
    "$memory",
    "$kb_results",
    // NL-23: $restart is the self-restart request — an ordinary writable
    // pipeline target, like $out/$draft. $restart_count is its runtime-owned
    // counterpart below (unforgeable — see that list's doc comment).
    "$restart",
    "$restart_count",
];

/// Runtime-owned variable names that user pipeline targets must not shadow (NL-8).
/// These are set exclusively by the executor and are not writable by workflow steps.
/// Writable reserved variables (`$out`, `$raw`, `$draft`, `$log`, `$quality`,
/// `$sentiment`, `$confidence`, `$restart`) are intentionally excluded — commands
/// assign to them. `$restart_count` is the one exception among the NL-23 pair:
/// it must stay runtime-owned so flow logic cannot forge the chain position it
/// reads (a workflow may *request* a restart via `$restart` but may not fabricate
/// how many have already happened).
pub const RUNTIME_OWNED_VARIABLES: &[&str] = &[
    "$in",
    "$error",
    "$meta",
    "$ctx",
    "$user",
    "$session",
    "$flags",
    "$memory",
    "$kb_results",
    "$restart_count",
];

/// Compact command → human-form verb (consumed by the transpiler). A command
/// not listed here falls back to title-casing its own name.
pub const TRANSPILER_VERB_MAP: &[(&str, &str)] = &[
    ("FETCH", "Fetch"),
    ("ANALYZE", "Analyze"),
    ("GEN", "Generate"),
    ("VALIDATE", "Validate"),
    ("ROUTE", "Route to"),
    ("ESCALATE", "Escalate to"),
    ("LOG", "Log"),
    ("PUBLISH", "Publish"),
    ("NOTIFY", "Notify"),
    ("REFINE", "Refine"),
    ("TONE", "Set tone to"),
    ("REMEMBER", "Store in memory:"),
    ("RECALL", "Load from memory:"),
    ("QUERY_KB", "Search knowledge base for"),
    ("RUN", "Run macro"),
];

/// Runtime error codes surfaced in the structured result's `errors` list.
pub mod error_code {
    /// A `!!` absolute rule was violated.
    pub const RULE_VIOLATION: &str = "NODUS:RULE_VIOLATION";
    /// A workflow file failed to parse.
    pub const PARSE_ERROR: &str = "NODUS:PARSE_ERROR";
    /// A `~UNTIL` loop hit its `MAX:n` limit.
    pub const MAX_REACHED: &str = "NODUS:MAX_REACHED";
    /// A step failed at run time.
    ///
    /// Non-canonical catch-all, superseded by the specific codes below.
    /// Retained for backward-compatible string matches; new emission sites
    /// must select a specific code instead of this generic one.
    #[deprecated(note = "non-canonical catch-all; use a specific NODUS:* code")]
    pub const EXECUTION_FAILED: &str = "NODUS:EXECUTION_FAILED";
    /// A variable was referenced before assignment.
    pub const UNDEFINED_VAR: &str = "NODUS:UNDEFINED_VAR";
    /// A `ROUTE(wf:name)` target does not exist.
    pub const ROUTE_NOT_FOUND: &str = "NODUS:ROUTE_NOT_FOUND";
    /// Two `!!` rules contradict each other.
    pub const RULE_CONFLICT: &str = "NODUS:RULE_CONFLICT";
    /// The schema version does not match the workflow.
    pub const SCHEMA_MISMATCH: &str = "NODUS:SCHEMA_MISMATCH";
    /// The workflow ran without a loaded schema.
    pub const NO_SCHEMA: &str = "NODUS:NO_SCHEMA";
    /// No `@ON:` trigger matched the current input.
    pub const NO_TRIGGER: &str = "NODUS:NO_TRIGGER";
    /// A step error reached no `@err:` handler.
    pub const UNHANDLED_ERROR: &str = "NODUS:UNHANDLED_ERROR";
    /// A workflow's capability manifest is unsatisfiable by the active host (LP-8).
    pub const CAPABILITY_UNMET: &str = "NODUS:CAPABILITY_UNMET";

    // ── Taxonomy expansion (schema v0.4.6 → v0.7). Severity/category metadata
    //    for each code lives in `super::error_meta`.

    /// A dispatched command is absent from the schema vocabulary.
    pub const UNDEFINED_CMD: &str = "NODUS:UNDEFINED_CMD";
    /// `RUN(@x)` referenced a macro that is not defined.
    pub const UNDEFINED_MACRO: &str = "NODUS:UNDEFINED_MACRO";
    /// A `^validator` rule failed against a step result.
    pub const VALIDATION_FAILED: &str = "NODUS:VALIDATION_FAILED";
    /// An `ESCALATE` target could not be reached.
    pub const ESCALATION_FAILED: &str = "NODUS:ESCALATION_FAILED";
    /// A model result fell below the required confidence threshold.
    pub const CONFIDENCE_LOW: &str = "NODUS:CONFIDENCE_LOW";
    /// A `QUERY_KB` knowledge-base backend was unavailable.
    pub const KB_UNAVAILABLE: &str = "NODUS:KB_UNAVAILABLE";
    /// A `REMEMBER` / `RECALL` memory operation failed.
    pub const MEMORY_FAILED: &str = "NODUS:MEMORY_FAILED";
    /// A `@test:` assertion failed.
    pub const TEST_FAILED: &str = "NODUS:TEST_FAILED";
    /// A `?SWITCH` matched no arm and declared no `*` default.
    pub const SWITCH_NO_MATCH: &str = "NODUS:SWITCH_NO_MATCH";
    /// The run is suspended awaiting human re-trigger (`!PAUSE` / dialog).
    pub const PAUSED: &str = "NODUS:PAUSED";
    /// A `COUNTER` exceeded its declared bound.
    pub const COUNTER_OVERFLOW: &str = "NODUS:COUNTER_OVERFLOW";
    /// A `GIT` / `QUERY_GIT` backend was unavailable.
    pub const GIT_UNAVAILABLE: &str = "NODUS:GIT_UNAVAILABLE";
    /// A dialog `+timeout` elapsed before an answer was supplied.
    pub const DIALOG_TIMEOUT: &str = "NODUS:DIALOG_TIMEOUT";
    /// A `CONFIRM` was rejected under `+strict`.
    pub const DIALOG_REJECTED: &str = "NODUS:DIALOG_REJECTED";
    /// A proposed `§config` value set failed the pre-run shape check or was
    /// rejected by the host `ConfigProvider` (NL-20).
    pub const CONFIG_INVALID: &str = "NODUS:CONFIG_INVALID";
    /// A `$restart` request was refused: either `restart_max` is undeclared
    /// (self-restart disabled) or the declared ceiling was reached (NL-23).
    /// A bounded construct reaching its bound, mirroring `MAX_REACHED` — a
    /// normal reported outcome, not a fault.
    pub const RESTART_LIMIT: &str = "NODUS:RESTART_LIMIT";
    /// A `~COMPENSATE` action itself failed while unwinding a completed
    /// effect (NL-22). The original effect stays recorded live — this marks
    /// the compensation attempt as failed, never as if the effect were undone.
    pub const COMPENSATION_FAILED: &str = "NODUS:COMPENSATION_FAILED";
    /// A host `PolicyProvider` denied a per-effect authorization check before
    /// the effect ran (LP-11). Non-halting: the effect never occurred, its
    /// pipeline target stays unbound, and execution continues.
    pub const POLICY_DENIED: &str = "NODUS:POLICY_DENIED";
    /// A gate-permitted `SETTLE` produced no verifiable receipt from the host
    /// `SettlementRail` (LP-17, VS-7). Non-halting: the payment is not treated
    /// as settled, and the pipeline target stays unbound.
    pub const SETTLEMENT_UNACCOUNTED: &str = "NODUS:SETTLEMENT_UNACCOUNTED";
    /// An `EnvironmentProfile` declares a `max_tokens` budget with no
    /// identified `token_measure` (NE-14). Fail-fast pre-run: rejected before
    /// `execute_for_environment` runs, never a silent default-encoder
    /// substitution.
    pub const ENV_MEASURE_UNKNOWN: &str = "NODUS:ENV_MEASURE_UNKNOWN";
    /// A host `ModelProvider` reported that a `GEN` / `ANALYZE` call failed
    /// (transport error, cancellation, refusal). The step produced no answer:
    /// its pipeline target stays unbound and the error reaches the workflow's
    /// `@err:` handler — a failed call is never folded into a short answer.
    pub const MODEL_CALL_FAILED: &str = "NODUS:MODEL_CALL_FAILED";
}

/// Severity of a runtime error code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorSeverity {
    /// Fatal — the code marks a failure.
    Error,
    /// Advisory — execution may continue.
    Warn,
    /// Informational — not a failure.
    Info,
}

/// Category of a runtime error code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    /// Source failed to tokenize/parse.
    Parse,
    /// A failure during step execution.
    Runtime,
    /// A pre-run validation failure.
    Validation,
    /// A routing / escalation failure.
    Routing,
    /// A knowledge-base / memory failure.
    Memory,
    /// A `@test:` failure.
    Test,
    /// A control-construct outcome (loops, switch, pause).
    Control,
    /// A human-in-the-loop dialog outcome.
    Dialog,
}

/// Return the `(severity, category)` metadata for a canonical `NODUS:*` code.
///
/// Returns `None` for a non-canonical code — including the deprecated
/// catch-all [`error_code::EXECUTION_FAILED`] — which lets callers detect
/// non-canonical usage.
pub fn error_meta(code: &str) -> Option<(ErrorSeverity, ErrorCategory)> {
    use ErrorCategory::*;
    use ErrorSeverity::*;
    use error_code as ec;

    let meta = match code {
        // Original codes (the catch-all EXECUTION_FAILED is excluded — superseded).
        ec::RULE_VIOLATION => (Error, Runtime),
        ec::PARSE_ERROR => (Error, Parse),
        ec::MAX_REACHED => (Warn, Control),
        ec::UNDEFINED_VAR => (Error, Runtime),
        ec::ROUTE_NOT_FOUND => (Error, Routing),
        ec::RULE_CONFLICT => (Error, Validation),
        ec::SCHEMA_MISMATCH => (Error, Validation),
        ec::NO_SCHEMA => (Error, Validation),
        ec::NO_TRIGGER => (Warn, Routing),
        ec::UNHANDLED_ERROR => (Error, Runtime),
        // Portability-layer code (LP-8).
        ec::CAPABILITY_UNMET => (Error, Control),
        // Taxonomy expansion (v0.4.6 → v0.7).
        ec::UNDEFINED_CMD => (Error, Validation),
        ec::UNDEFINED_MACRO => (Error, Validation),
        ec::VALIDATION_FAILED => (Error, Validation),
        ec::ESCALATION_FAILED => (Error, Routing),
        ec::CONFIDENCE_LOW => (Warn, Runtime),
        ec::KB_UNAVAILABLE => (Error, Memory),
        ec::MEMORY_FAILED => (Error, Memory),
        ec::TEST_FAILED => (Error, Test),
        ec::SWITCH_NO_MATCH => (Warn, Control),
        ec::PAUSED => (Info, Control),
        ec::COUNTER_OVERFLOW => (Error, Runtime),
        ec::GIT_UNAVAILABLE => (Error, Runtime),
        ec::DIALOG_TIMEOUT => (Error, Dialog),
        ec::DIALOG_REJECTED => (Error, Dialog),
        // Config-layer code (NL-20).
        ec::CONFIG_INVALID => (Error, Validation),
        // Self-restart code (NL-23).
        ec::RESTART_LIMIT => (Warn, Control),
        // Compensation-seam code (NL-22).
        ec::COMPENSATION_FAILED => (Error, Runtime),
        // Portability-layer code (LP-11).
        ec::POLICY_DENIED => (Error, Runtime),
        // Portability-layer code (LP-17).
        ec::SETTLEMENT_UNACCOUNTED => (Error, Runtime),
        // Environment-layer code (NE-14).
        ec::ENV_MEASURE_UNKNOWN => (Error, Control),
        // Model-call failure (a host provider reported the call failed).
        ec::MODEL_CALL_FAILED => (Error, Runtime),
        // Non-canonical (incl. deprecated EXECUTION_FAILED) → no metadata.
        _ => return None,
    };
    Some(meta)
}

/// Is `name` a known command?
pub fn is_known_command(name: &str) -> bool {
    KNOWN_COMMANDS.contains(&name)
}

/// The loaded vocabulary contract a workflow is interpreted against.
///
/// In this foundation it is the builtin schema; the query methods are the
/// stable seam the parser and validator depend on, independent of where the
/// vocabulary is sourced from.
#[derive(Debug, Clone)]
pub struct Schema {
    version: &'static str,
    host_commands: Vec<String>,
    host_reserved: Vec<String>,
}

impl Schema {
    /// The in-tree builtin vocabulary.
    pub fn builtin() -> Self {
        Schema {
            version: BUILTIN_SCHEMA_VERSION,
            host_commands: Vec::new(),
            host_reserved: Vec::new(),
        }
    }

    /// Build an extended schema merging the builtin baseline with host additions.
    ///
    /// Host commands that collide with `KNOWN_COMMANDS` are silently deduplicated.
    /// Host reserved variables that collide with `RESERVED_VARIABLES` are silently
    /// deduplicated. This preserves LP-4: builtin constants are never mutated.
    pub fn with_provider(provider: &dyn crate::portability::SchemaProvider) -> Self {
        let host_commands = provider
            .host_commands()
            .iter()
            .filter(|&&cmd| !is_known_command(cmd))
            .map(|&cmd| cmd.to_string())
            .collect();

        let host_reserved = provider
            .host_reserved_variables()
            .iter()
            .filter(|&&var| !RESERVED_VARIABLES.contains(&var))
            .map(|&var| var.to_string())
            .collect();

        Schema {
            version: BUILTIN_SCHEMA_VERSION,
            host_commands,
            host_reserved,
        }
    }

    /// The schema version this vocabulary declares.
    pub fn version(&self) -> &str {
        self.version
    }

    /// All known command names.
    pub fn commands(&self) -> &'static [&'static str] {
        KNOWN_COMMANDS
    }

    /// All host-added command names (those not in the builtin `KNOWN_COMMANDS`).
    pub fn host_commands(&self) -> &[String] {
        &self.host_commands
    }

    /// Is `name` a known command (builtin or host-added)?
    pub fn is_command(&self, name: &str) -> bool {
        is_known_command(name) || self.host_commands.iter().any(|c| c == name)
    }

    /// Is `name` a host-added command (i.e., not in the builtin `KNOWN_COMMANDS`)?
    pub fn is_host_command(&self, name: &str) -> bool {
        self.host_commands.iter().any(|c| c == name)
    }

    /// Is `name` (with leading `$`) a reserved variable (builtin or host-added)?
    pub fn is_reserved_var(&self, name: &str) -> bool {
        RESERVED_VARIABLES.contains(&name) || self.host_reserved.iter().any(|v| v == name)
    }

    /// Is `name` (with leading `$`) a runtime-owned variable that user steps must not shadow?
    pub fn is_runtime_owned(&self, name: &str) -> bool {
        RUNTIME_OWNED_VARIABLES.contains(&name)
    }

    /// Is `tone` a valid tone value?
    pub fn is_valid_tone(&self, tone: &str) -> bool {
        VALID_TONES.contains(&tone)
    }

    /// Is `name` a known analysis-flag extractor (`~flag`)?
    pub fn is_known_flag(&self, name: &str) -> bool {
        KNOWN_FLAGS.contains(&name)
    }

    /// Is `name` a known validator (`^validator`)? Matches the validator name —
    /// the segment before the first `:` — so `len:32` and `len` both resolve.
    pub fn is_known_validator(&self, name: &str) -> bool {
        let head = name.split(':').next().unwrap_or(name);
        KNOWN_VALIDATORS.contains(&head)
    }

    /// Is `name` a known primitive `@in` field type?
    pub fn is_known_type(&self, name: &str) -> bool {
        PRIMITIVE_TYPES.contains(&name)
    }

    /// Human-form verb for a command, if one is mapped.
    pub fn transpiler_verb(&self, command: &str) -> Option<&'static str> {
        TRANSPILER_VERB_MAP
            .iter()
            .find(|(k, _)| *k == command)
            .map(|(_, v)| *v)
    }
}

impl Default for Schema {
    fn default() -> Self {
        Self::builtin()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_schema_lists_known_commands() {
        let schema = Schema::builtin();
        assert!(schema.is_command("GEN"));
        assert!(schema.is_command("VALIDATE"));
        assert!(!schema.is_command("NOT_A_COMMAND"));
        assert_eq!(schema.commands().len(), KNOWN_COMMANDS.len());
    }

    #[test]
    fn builtin_schema_knows_reserved_vars_and_tones() {
        let schema = Schema::builtin();
        assert!(schema.is_reserved_var("$draft"));
        assert!(!schema.is_reserved_var("$nope"));
        assert!(schema.is_valid_tone("warm"));
        assert!(!schema.is_valid_tone("sarcastic"));
    }

    #[test]
    fn transpiler_verb_maps_known_and_misses_unknown() {
        let schema = Schema::builtin();
        assert_eq!(schema.transpiler_verb("GEN"), Some("Generate"));
        assert_eq!(schema.transpiler_verb("ROUTE"), Some("Route to"));
        assert_eq!(schema.transpiler_verb("FETCH_ALL"), None);
    }

    #[test]
    fn version_is_reported() {
        assert_eq!(Schema::builtin().version(), BUILTIN_SCHEMA_VERSION);
    }

    #[test]
    fn run_is_known_command() {
        let schema = Schema::builtin();
        assert!(schema.is_command("RUN"), "RUN must be a known command");
        assert_eq!(
            BUILTIN_SCHEMA_VERSION, "0.4.7",
            "version bump must accompany a command addition (RUN, then SETTLE — LP-17)"
        );
    }

    #[test]
    fn runtime_owned_is_subset_of_reserved() {
        for var in RUNTIME_OWNED_VARIABLES {
            assert!(
                RESERVED_VARIABLES.contains(var),
                "{var} in RUNTIME_OWNED_VARIABLES but missing from RESERVED_VARIABLES"
            );
        }
    }

    #[test]
    fn runtime_owned_excludes_writable_reserved() {
        let writable = [
            "$out",
            "$raw",
            "$draft",
            "$log",
            "$quality",
            "$sentiment",
            "$confidence",
        ];
        for var in writable {
            assert!(
                !RUNTIME_OWNED_VARIABLES.contains(&var),
                "{var} must not be in RUNTIME_OWNED_VARIABLES (it is user-writable)"
            );
        }
    }

    #[test]
    fn with_provider_extends_schema() {
        use crate::portability::SchemaProvider;

        struct TestProvider;
        impl SchemaProvider for TestProvider {
            fn host_commands(&self) -> &[&str] {
                &["MY_CMD"]
            }
            fn host_reserved_variables(&self) -> &[&str] {
                &["$myvar"]
            }
        }

        let schema = Schema::with_provider(&TestProvider);
        assert!(
            schema.is_command("MY_CMD"),
            "host command must be recognized"
        );
        assert!(
            schema.is_host_command("MY_CMD"),
            "MY_CMD must be flagged as host command"
        );
        assert!(
            schema.is_command("GEN"),
            "builtin command must remain recognized"
        );
        assert!(
            !schema.is_host_command("GEN"),
            "builtin command must not be flagged as host"
        );
    }

    #[test]
    fn with_provider_deduplicates_builtins() {
        use crate::portability::SchemaProvider;

        struct CollidingProvider;
        impl SchemaProvider for CollidingProvider {
            fn host_commands(&self) -> &[&str] {
                &["GEN"]
            }
            fn host_reserved_variables(&self) -> &[&str] {
                &[]
            }
        }

        let schema = Schema::with_provider(&CollidingProvider);
        assert!(schema.is_command("GEN"), "GEN must remain recognized");
        assert!(
            !schema.is_host_command("GEN"),
            "GEN must not be treated as host-added"
        );
        assert!(
            schema.host_commands().is_empty(),
            "deduplicated GEN must not appear in host list"
        );
    }

    // ── Error taxonomy ──────────────────────────────────────────────────────

    #[test]
    fn error_meta_types_construct() {
        assert_ne!(ErrorSeverity::Error, ErrorSeverity::Warn);
        assert_ne!(ErrorSeverity::Warn, ErrorSeverity::Info);
        assert_ne!(ErrorCategory::Parse, ErrorCategory::Dialog);
        let _ = (ErrorSeverity::Info, ErrorCategory::Control);
    }

    #[test]
    fn new_error_codes_are_namespaced() {
        use error_code::*;
        let new = [
            UNDEFINED_CMD,
            UNDEFINED_MACRO,
            VALIDATION_FAILED,
            ESCALATION_FAILED,
            CONFIDENCE_LOW,
            KB_UNAVAILABLE,
            MEMORY_FAILED,
            TEST_FAILED,
            SWITCH_NO_MATCH,
            PAUSED,
            COUNTER_OVERFLOW,
            GIT_UNAVAILABLE,
            DIALOG_TIMEOUT,
            DIALOG_REJECTED,
        ];
        assert_eq!(new.len(), 14);
        for c in new {
            assert!(c.starts_with("NODUS:"), "{c} must be NODUS:-namespaced");
        }
        let mut sorted = new.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 14, "the 14 new codes must be distinct");
    }

    #[test]
    fn error_meta_maps_known_codes() {
        assert_eq!(
            error_meta(error_code::PARSE_ERROR),
            Some((ErrorSeverity::Error, ErrorCategory::Parse))
        );
        assert_eq!(
            error_meta(error_code::SWITCH_NO_MATCH),
            Some((ErrorSeverity::Warn, ErrorCategory::Control))
        );
        assert_eq!(
            error_meta(error_code::PAUSED),
            Some((ErrorSeverity::Info, ErrorCategory::Control))
        );
        assert_eq!(
            error_meta(error_code::DIALOG_TIMEOUT),
            Some((ErrorSeverity::Error, ErrorCategory::Dialog))
        );
        assert_eq!(
            error_meta(error_code::CONFIG_INVALID),
            Some((ErrorSeverity::Error, ErrorCategory::Validation))
        );
    }

    #[test]
    #[allow(deprecated)]
    fn execution_failed_is_non_canonical() {
        assert!(error_meta(error_code::EXECUTION_FAILED).is_none());
        assert!(error_meta("NODUS:NOT_A_REAL_CODE").is_none());
    }

    #[test]
    fn error_registry_lockstep() {
        use error_code::*;
        // Every canonical code (24 language codes + CAPABILITY_UNMET; the
        // deprecated EXECUTION_FAILED excluded) must carry metadata.
        let canonical = [
            RULE_VIOLATION,
            PARSE_ERROR,
            MAX_REACHED,
            UNDEFINED_VAR,
            ROUTE_NOT_FOUND,
            RULE_CONFLICT,
            SCHEMA_MISMATCH,
            NO_SCHEMA,
            NO_TRIGGER,
            UNHANDLED_ERROR,
            CAPABILITY_UNMET,
            UNDEFINED_CMD,
            UNDEFINED_MACRO,
            VALIDATION_FAILED,
            ESCALATION_FAILED,
            CONFIDENCE_LOW,
            KB_UNAVAILABLE,
            MEMORY_FAILED,
            TEST_FAILED,
            SWITCH_NO_MATCH,
            PAUSED,
            COUNTER_OVERFLOW,
            GIT_UNAVAILABLE,
            DIALOG_TIMEOUT,
            DIALOG_REJECTED,
            CONFIG_INVALID,
            RESTART_LIMIT,
            COMPENSATION_FAILED,
            POLICY_DENIED,
            SETTLEMENT_UNACCOUNTED,
            ENV_MEASURE_UNKNOWN,
            MODEL_CALL_FAILED,
        ];
        assert_eq!(
            canonical.len(),
            32,
            "24 language codes + CAPABILITY_UNMET + CONFIG_INVALID + RESTART_LIMIT + COMPENSATION_FAILED + POLICY_DENIED + SETTLEMENT_UNACCOUNTED + ENV_MEASURE_UNKNOWN + MODEL_CALL_FAILED"
        );
        for code in canonical {
            assert!(
                error_meta(code).is_some(),
                "canonical code {code} is missing metadata"
            );
        }
    }

    // ── Closed vocabulary registries ────────────────────────────────────────

    #[test]
    fn registries_have_expected_size() {
        assert_eq!(KNOWN_FLAGS.len(), 12);
        assert_eq!(KNOWN_VALIDATORS.len(), 12);
        assert_eq!(PRIMITIVE_TYPES.len(), 10);
    }

    #[test]
    fn schema_knows_flags() {
        let s = Schema::builtin();
        assert!(s.is_known_flag("sentiment"));
        assert!(s.is_known_flag("keywords"));
        assert!(!s.is_known_flag("bogus"));
    }

    #[test]
    fn schema_knows_validators_by_prefix() {
        let s = Schema::builtin();
        assert!(s.is_known_validator("len"));
        assert!(s.is_known_validator("len:32"), "pre-colon name must match");
        assert!(s.is_known_validator("lang:en"));
        assert!(!s.is_known_validator("nope"));
        assert!(!s.is_known_validator("nope:1"));
    }

    #[test]
    fn schema_knows_types() {
        let s = Schema::builtin();
        assert!(s.is_known_type("str"));
        assert!(s.is_known_type("any"));
        assert!(!s.is_known_type("widget"));
    }

    #[test]
    fn ask_confirm_are_known_commands() {
        let s = Schema::builtin();
        assert!(s.is_command("ASK"), "ASK must be a known command");
        assert!(s.is_command("CONFIRM"), "CONFIRM must be a known command");
    }
}
