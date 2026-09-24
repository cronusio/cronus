//! Declarative workflow DSL runtime — lexer, parser/AST, validator (+ lint),
//! executor, and transpiler. The crate is `std`-only with zero external
//! dependencies: embed it anywhere Rust runs.
//!
//! # Quick start
//!
//! ```rust
//! use nodus::{workflows, executor::{Status, Value}};
//!
//! let source = r#"
//! §wf:greet v1.0
//! §runtime: { core: schema.nodus }
//! @in:  { name: text }
//! @out: $out
//! @err: ESCALATE(human)
//! @steps:
//!   1. GEN($in.name) → $out
//!   2. LOG($out)
//! "#;
//!
//! // The input must satisfy the declared `@in:` contract before anything runs.
//! let input = Value::Map(vec![("name".to_string(), Value::Text("Ada".to_string()))]);
//! let result = workflows::run(source, "greet.nodus", Some(input)).unwrap();
//! assert_eq!(result.status, Status::Ok);
//!
//! // A required field left out is a validation error, not a silent empty prompt.
//! assert!(workflows::run(source, "greet.nodus", None).is_err());
//! ```
//!
//! # Workflow lifecycle
//!
//! | Step | Function | Purpose |
//! | --- | --- | --- |
//! | Scaffold | [`workflows::scaffold`] | Minimal valid AST |
//! | Validate | [`workflows::validate`] | All lint diagnostics |
//! | Run | [`workflows::run`] | Execute with built-in stub |
//! | Transpile | [`workflows::transpile`] | Compact or human form |
//!
//! # Design
//!
//! The vocabulary lives in [`vocab`] as data separate from the logic that
//! consults it — updating the language is a data change, not a code change.
//! The [`executor::ModelProvider`] trait is the sole extension point for real
//! AI model integration; the built-in [`executor::StubProvider`] satisfies the
//! interface without I/O, enabling fully in-process tests.

mod error;

pub mod ast;
pub mod environment;
pub mod lexer;
pub mod observability;
pub mod parser;
pub mod portability;
pub mod vocab;

// Pipeline modules.
pub mod executor;
pub mod transpiler;
pub mod validator;
pub mod workflows;

pub use environment::{
    Action, Budget, CandidateResult, EnvRunResult, EnvironmentProfile, EnvironmentProvider,
    GradingMode, Instance, Observation, Reward, Seed, StubEnvironment, TaskId, grade,
};
pub use error::{Error, Result, Span};
pub use executor::{
    DefaultDialogProvider, DialogOutcome, DialogProvider, Executor, ExecutorBuilder, ModelError,
    ModelProvider, ResumeDescriptor, RunResult, Status, StubProvider, Value,
};
pub use lexer::{Lexer, Token, TokenType};
pub use observability::{
    Anomaly, AuditProvider, Determinism, DialogProvenance, Durability, EnvInteraction,
    EnvInteractionKind, EventAnnotations, ExecutionEvent, ExecutionMode, FaultIdentity,
    FieldDescriptor, LoopType, Measurement, NoopAuditProvider, ReproRecipe, RunManifest, RunStatus,
    SimFidelity, SourceRef, TraceCompleteness, classify_trace, step_identity,
};
pub use parser::Parser;
pub use portability::{
    BuiltinSchemaProvider, CapabilityManifest, ConfigOutcome, ConfigProvider,
    DefaultConfigProvider, ExtensionRole, HostCapabilities, InMemoryStorageProvider, Missing,
    NoopPolicyProvider, NoopSettlementRail, PolicyProvider, SchemaProvider, SettlementRail,
    StorageProvider, validate_manifest,
};
pub use transpiler::Transpiler;
pub use validator::{
    AcceptedConfig, ConfigReason, ConfigViolation, Diagnostic, Severity, Validator,
    check_config_values,
};
pub use vocab::Schema;
pub use workflows::{
    RunOptions, TestReport, TestResult, TranspileMode, ValidationReport, run, run_with_audit,
    run_with_config, run_with_config_and_audit, run_with_dialog, run_with_dialog_and_audit,
    run_with_environment, run_with_environment_and_audit, run_with_manifest,
    run_with_manifest_and_audit, run_with_options, run_with_policy, run_with_policy_and_audit,
    run_with_provider, run_with_provider_and_audit, run_with_schema, run_with_schema_and_audit,
    run_with_settlement, run_with_settlement_and_audit, scaffold, test, test_with_tags, transpile,
    validate,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_surface_assembles() {
        // The re-exported lexer + schema compose through the crate root.
        let schema = Schema::builtin();
        let tokens = Lexer::tokenize_str("LOG($out)").expect("tokenizes");
        assert_eq!(tokens.first().map(|t| t.ty), Some(TokenType::CommandName));
        assert!(schema.is_command(&tokens[0].value));
    }

    #[test]
    fn errors_render_with_position() {
        let err = Error::Parse {
            span: Span::new(3, 7),
            message: "unexpected token".to_string(),
        };
        assert_eq!(err.to_string(), "parse error at 3:7: unexpected token");
    }
}
