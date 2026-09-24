//! Execution model: activation loads the instruction body; if the
//! package declares a workflow and is not degraded, invocation validates
//! then bounded-executes it, dispatching a built-in command per
//! operation step with a per-call grant check. A `degraded: instruction-only`
//! package never reaches the runtime, regardless of what it carries on disk.
//!
//! The nodus workflow runtime is a seam — [`WorkflowRuntime`] is the
//! interface this module drives; wiring it to the real `nodus` crate is a
//! separate cross-crate concern (out of this phase's scope.3 Notes).

use crate::extensions::ExtensionPermissions;
use crate::skills::commands::{CommandRegistry, DispatchError, ParamValue};
use crate::skills::package::SkillPackage;
use std::collections::HashMap;

/// Whether a package's workflow reached full canonical form, or was
/// downgraded to instruction-only by the conversion pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Degradation {
    Full,
    InstructionOnly,
}

/// One command dispatch a workflow's execution makes.
#[derive(Debug, Clone)]
pub struct OperationStep {
    pub command_id: String,
    pub args: HashMap<String, ParamValue>,
}

/// The workflow runtime seam. A real implementation validates/executes
/// `workflow.nd` against the nodus vocabulary; here it is anything that can
/// validate a package and report which operation steps a bounded execution
/// would dispatch.
pub trait WorkflowRuntime {
    /// Validate the package's workflow. `Err` stops execution before any
    /// operation dispatches (the runtime's own contract).
    fn validate(&self, package: &SkillPackage) -> Result<(), String>;

    /// Execute the validated workflow, bounded, returning the
    /// ordered operation steps it dispatches.
    fn execute(&self, package: &SkillPackage) -> Result<Vec<OperationStep>, String>;
}

/// The structured result of an activation.
#[derive(Debug, PartialEq)]
pub enum ActivationResult {
    /// Only the instruction body reached the agent's context; the runtime
    /// was never invoked — either the package carries no workflow, or it is
    /// degraded instruction-only.
    InstructionOnly,
    /// The workflow executed; each step's dispatch outcome, in step order.
    /// A per-call grant check is invoked for every step.
    WorkflowExecuted(Vec<Result<(), DispatchError>>),
}

#[derive(Debug, PartialEq, Eq)]
pub enum ActivationError {
    Validation(String),
    Execution(String),
}

/// Activate a package. Degraded or workflow-less packages short-circuit to
/// [`ActivationResult::InstructionOnly`] before `runtime` is touched at all
/// (the required guard). Otherwise: validate, execute, then check the
/// caller's grants against every dispatched operation step.
pub fn activate(
    package: &SkillPackage,
    degradation: Degradation,
    runtime: &dyn WorkflowRuntime,
    commands: &CommandRegistry,
    caller_permissions: &ExtensionPermissions,
) -> Result<ActivationResult, ActivationError> {
    if degradation == Degradation::InstructionOnly || !package.has_workflow {
        return Ok(ActivationResult::InstructionOnly);
    }
    runtime
        .validate(package)
        .map_err(ActivationError::Validation)?;
    let steps = runtime
        .execute(package)
        .map_err(ActivationError::Execution)?;
    let results = steps
        .into_iter()
        .map(|step| {
            commands
                .check_dispatch(&step.command_id, &step.args, caller_permissions)
                .map(|_| ())
        })
        .collect();
    Ok(ActivationResult::WorkflowExecuted(results))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::{ExtensionKind, ExtensionManifest, ExtensionSource};
    use crate::skills::commands::{CommandCategory, CommandSpec, InputSchema, RequiredGrant};
    use crate::skills::package::{PackageListing, validate_package};
    use std::cell::Cell;

    fn manifest() -> ExtensionManifest {
        ExtensionManifest {
            id: "core/review".into(),
            kind: ExtensionKind::Skill,
            name: "review".into(),
            version: "1.0.0".into(),
            source: ExtensionSource::Preset,
            capabilities: vec![],
            permissions: ExtensionPermissions::default(),
        }
    }

    fn package_without_workflow() -> SkillPackage {
        validate_package(
            PackageListing::new(["SKILL.md", "extension.json"]),
            manifest(),
        )
        .unwrap()
    }

    fn package_with_workflow() -> SkillPackage {
        validate_package(
            PackageListing::new(["SKILL.md", "extension.json", "workflow.nd", "workflow.md"]),
            manifest(),
        )
        .unwrap()
    }

    struct MockRuntime {
        validate_called: Cell<bool>,
        execute_called: Cell<bool>,
        fail_validate: bool,
        steps: Vec<OperationStep>,
    }

    impl MockRuntime {
        fn new(steps: Vec<OperationStep>) -> Self {
            MockRuntime {
                validate_called: Cell::new(false),
                execute_called: Cell::new(false),
                fail_validate: false,
                steps,
            }
        }

        fn failing_validate() -> Self {
            MockRuntime {
                validate_called: Cell::new(false),
                execute_called: Cell::new(false),
                fail_validate: true,
                steps: vec![],
            }
        }
    }

    impl WorkflowRuntime for MockRuntime {
        fn validate(&self, _package: &SkillPackage) -> Result<(), String> {
            self.validate_called.set(true);
            if self.fail_validate {
                Err("invalid workflow".to_string())
            } else {
                Ok(())
            }
        }

        fn execute(&self, _package: &SkillPackage) -> Result<Vec<OperationStep>, String> {
            self.execute_called.set(true);
            Ok(self.steps.clone())
        }
    }

    fn step(command_id: &str) -> OperationStep {
        OperationStep {
            command_id: command_id.to_string(),
            args: HashMap::new(),
        }
    }

    fn no_arg_command(id: &str, grant: Option<RequiredGrant>) -> CommandSpec {
        CommandSpec::new(id, CommandCategory::Effects, InputSchema::new([]), grant)
    }

    #[test]
    fn instruction_only_package_never_touches_runtime() {
        let package = package_without_workflow();
        let runtime = MockRuntime::new(vec![]);
        let commands = CommandRegistry::new();
        let perms = ExtensionPermissions::default();

        let result = activate(&package, Degradation::Full, &runtime, &commands, &perms).unwrap();

        assert_eq!(result, ActivationResult::InstructionOnly);
        assert!(!runtime.validate_called.get());
        assert!(!runtime.execute_called.get());
    }

    #[test]
    fn degraded_package_never_reaches_runtime_even_with_workflow() {
        // Guard test: a workflow.nd is present on disk, but degradation must
        // still short-circuit before the runtime is touched.
        let package = package_with_workflow();
        let runtime = MockRuntime::new(vec![step("noop")]);
        let commands = CommandRegistry::new();
        let perms = ExtensionPermissions::default();

        let result = activate(
            &package,
            Degradation::InstructionOnly,
            &runtime,
            &commands,
            &perms,
        )
        .unwrap();

        assert_eq!(result, ActivationResult::InstructionOnly);
        assert!(!runtime.validate_called.get());
        assert!(!runtime.execute_called.get());
    }

    #[test]
    fn validation_failure_stops_before_execute() {
        let package = package_with_workflow();
        let runtime = MockRuntime::failing_validate();
        let commands = CommandRegistry::new();
        let perms = ExtensionPermissions::default();

        let err = activate(&package, Degradation::Full, &runtime, &commands, &perms).unwrap_err();

        assert_eq!(
            err,
            ActivationError::Validation("invalid workflow".to_string())
        );
        assert!(runtime.validate_called.get());
        assert!(!runtime.execute_called.get());
    }

    #[test]
    fn workflow_execution_checks_grants_per_operation_step() {
        let package = package_with_workflow();
        let runtime = MockRuntime::new(vec![step("allowed"), step("denied"), step("allowed")]);
        let mut commands = CommandRegistry::new();
        commands.register(no_arg_command("allowed", None));
        commands.register(no_arg_command(
            "denied",
            Some(RequiredGrant::Network("fetch".to_string())),
        ));
        let perms = ExtensionPermissions::default(); // no grants at all

        let result = activate(&package, Degradation::Full, &runtime, &commands, &perms).unwrap();

        assert!(runtime.validate_called.get());
        assert!(runtime.execute_called.get());
        match result {
            ActivationResult::WorkflowExecuted(results) => {
                // Every one of the three steps was checked ("per call").
                assert_eq!(results.len(), 3);
                assert!(results[0].is_ok());
                assert_eq!(
                    results[1],
                    Err(DispatchError::MissingGrant(RequiredGrant::Network(
                        "fetch".to_string()
                    )))
                );
                assert!(results[2].is_ok());
            }
            other => panic!("expected WorkflowExecuted, got {other:?}"),
        }
    }
}
