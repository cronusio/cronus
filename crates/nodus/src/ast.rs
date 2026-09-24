//! AST node model — the shared shape both the compact and human forms parse to.
//!
//! The single contract consumed by the parser, transpiler, executor, and
//! validator. Conditions, triggers, and rule bodies are kept as raw strings
//! (matching the reference parser, which does not build expression trees on the
//! workflow path), so the tree stays a faithful, equality-testable data model.
//!
//! Nodes intentionally carry no source span: positions live on the tokens and
//! parse errors are reported against the offending token, so the AST is pure
//! data (`Debug` + `PartialEq` round-trip without span brittleness). Lint
//! diagnostics that need line numbers will attach them when the validator lands.

/// The three `.nodus` file kinds (`§wf:` / `§schema:` / `§config:`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    /// A `§wf:` workflow file.
    Workflow,
    /// A `§schema:` schema file.
    Schema,
    /// A `§config:` config file.
    Config,
}

/// File type, name, and version (`§wf:name v1.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHeader {
    /// The declared file type.
    pub file_type: FileType,
    /// The workflow / schema / config name.
    pub name: String,
    /// The version string (e.g. `v1.0`), empty if absent.
    pub version: String,
}

/// Runtime configuration block (`§runtime: { core: …, mode: … }`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuntimeBlock {
    /// Path to the core schema resource.
    pub core: String,
    /// Extension schema paths.
    pub extends: Vec<String>,
    /// Named agent bindings (ordered for deterministic round-trip).
    pub agents: Vec<(String, String)>,
    /// Execution mode (defaults to `production`).
    pub mode: String,
    /// Bounded whole-run self-restart ceiling. `None` disables the
    /// feature entirely — a workflow with no `restart_max` behaves exactly as
    /// before this construct existed.
    pub restart_max: Option<u32>,
}

/// Reactive trigger declaration (`@ON: condition → action`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trigger {
    /// The activation condition (raw).
    pub condition: String,
    /// The action to run when the trigger fires (raw).
    pub action: String,
}

/// The two hard-constraint kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleKind {
    /// `!!NEVER`
    Never,
    /// `!!ALWAYS`
    Always,
}

/// Absolute constraint (`!!NEVER:` / `!!ALWAYS:`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbsoluteRule {
    /// The rule kind; `None` if `!!` was not followed by `NEVER`/`ALWAYS`.
    pub kind: Option<RuleKind>,
    /// The rule body (raw).
    pub content: String,
}

/// Soft preference (`!PREF: preferred OVER other IF condition`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preference {
    /// The preferred option.
    pub preferred: String,
    /// The option it is preferred over.
    pub over: String,
    /// An optional guarding condition.
    pub condition: Option<String>,
}

/// A single field in an `@in` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputField {
    /// Field name.
    pub name: String,
    /// Declared type (defaults to `any`).
    pub type_name: String,
    /// Whether the field is optional (`?` suffix or a default).
    pub optional: bool,
    /// The default value, if any.
    pub default: Option<String>,
    /// An inline comment.
    pub comment: String,
}

impl Default for InputField {
    fn default() -> Self {
        InputField {
            name: String::new(),
            type_name: "any".to_string(),
            optional: false,
            default: None,
            comment: String::new(),
        }
    }
}

/// Workflow input declaration (`@in`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InputDecl {
    /// The declared fields.
    pub fields: Vec<InputField>,
}

/// Workflow output target (`@out: $var`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OutputDecl {
    /// The output variable.
    pub variable: String,
}

/// Required context keys (`@ctx: [a, b]`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContextDecl {
    /// The required context names.
    pub contexts: Vec<String>,
}

/// Error handler declaration (`@err: ESCALATE(human)`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ErrorDecl {
    /// The parsed handler command, if recognized.
    pub handler: Option<CommandCall>,
    /// The raw handler text.
    pub raw: String,
}

/// A command invocation (`NAME(args) +mods ^validators ~flags → $target`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommandCall {
    /// The command name.
    pub name: String,
    /// Positional arguments (raw strings).
    pub args: Vec<String>,
    /// `+name=value` modifiers (ordered).
    pub modifiers: Vec<(String, String)>,
    /// `^validator` rules.
    pub validators: Vec<String>,
    /// `~flag` extractors.
    pub flags: Vec<String>,
    /// The `→ $target` pipeline destination, if any.
    pub pipeline_target: Option<String>,
}

/// Conditional (`?IF` / `?ELIF` / `?ELSE`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Conditional {
    /// The branch condition (empty for `?ELSE`).
    pub condition: String,
    /// The inline action (`→ action`), if any.
    pub action: Option<CommandCall>,
    /// A nested block body (block-form `?IF …:`), usually empty.
    pub body: Vec<Stmt>,
    /// `!BREAK` flag present on the action.
    pub break_flag: bool,
    /// `!SKIP` flag present on the action.
    pub skip_flag: bool,
    /// `!OVERRIDE` flag present on the action.
    pub override_flag: bool,
    /// `!HALT` flag present on the action (fatal stop ⇒ failed status).
    pub halt_flag: bool,
    /// `!PAUSE` flag present on the action (suspension ⇒ paused status).
    pub pause_flag: bool,
    /// Chained `?ELIF` branches.
    pub elif_branches: Vec<Conditional>,
    /// A trailing `?ELSE` branch.
    pub else_branch: Option<Box<Conditional>>,
}

/// Iterator loop (`~FOR $var IN $collection … ~END`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ForLoop {
    /// The loop variable.
    pub variable: String,
    /// The collection iterated over.
    pub collection: String,
    /// The loop body.
    pub body: Vec<Stmt>,
}

/// Conditional loop (`~UNTIL condition | MAX:n … ~END`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UntilLoop {
    /// The continue-until condition (raw).
    pub condition: String,
    /// The iteration cap, if declared.
    pub max_iterations: Option<u32>,
    /// The loop body.
    pub body: Vec<Stmt>,
}

/// Concurrent block (`~PARALLEL … ~JOIN → $target`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParallelBlock {
    /// The concurrent branches.
    pub branches: Vec<Stmt>,
    /// The `~JOIN → $target` collection variable, if any.
    pub join_target: Option<String>,
}

/// Multi-branch dispatch (`?SWITCH $v: value → action … * → action ~END`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SwitchBlock {
    /// The scrutinee variable compared against each arm value.
    pub scrutinee: String,
    /// `value → action` arms, in declaration order (first match wins).
    pub arms: Vec<(String, CommandCall)>,
    /// The optional `* → action` default arm.
    pub default: Option<CommandCall>,
}

/// Collection transform (`~MAP $coll: CMD($it) → $out`). The per-element value
/// is bound to the implicit `$it`; results collect into `target`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MapBlock {
    /// The collection variable iterated over.
    pub collection: String,
    /// The per-element command, using the implicit `$it` binding.
    pub command: CommandCall,
    /// The `→ $out` destination that receives the collected list, if any.
    pub target: Option<String>,
}

/// A variable reference (`$name.field`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Variable {
    /// The variable name (including the leading `$`).
    pub name: String,
}

/// A `;;` comment line.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Comment {
    /// The comment text.
    pub text: String,
}

/// An inline `@test:` block with structured fields parsed from the block body.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TestBlock {
    /// The test name (from `@test: name`).
    pub name: String,
    /// `input:` key-value pairs that override `@in:` fields for this test run.
    pub input: Vec<(String, String)>,
    /// `expected:` assertions: variable name → expected literal value.
    pub expected: Vec<(String, String)>,
    /// `tags:` labels for filtering; no semantic effect on pass/fail.
    pub tags: Vec<String>,
    /// Raw body token values preserved for backward-compat (transpiler fallback).
    pub raw_lines: Vec<String>,
}

/// A `@macro:` definition (raw body lines preserved).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MacroBlock {
    /// The macro name.
    pub name: String,
    /// The raw body token values.
    pub raw_lines: Vec<String>,
}

// ─── Config AST (`§config:`) ────────────────────────────────────────────────

/// A field's value constraint: a bounded range or a closed enumeration.
///
/// Bounds and members are kept as raw literal strings — matching
/// [`InputField::default`]'s raw-string precedent — and are coerced to a typed
/// `Value` against the field's declared type at shape-check time, keeping this
/// module free of a dependency on `crate::executor`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldConstraint {
    /// `range: lo, hi` — inclusive bounds.
    Range {
        /// The lower bound (raw literal).
        lo: String,
        /// The upper bound (raw literal).
        hi: String,
    },
    /// `one_of: a | b | c` — a closed set of accepted literals.
    OneOf(Vec<String>),
}

/// A single field declared in a `§config:` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigField {
    /// Field name.
    pub name: String,
    /// Declared primitive type (defaults to `any`).
    pub type_name: String,
    /// The default value (raw literal), if declared.
    pub default: Option<String>,
    /// An optional range or enumeration constraint.
    pub constraint: Option<FieldConstraint>,
    /// Whether the field is `required` (present unless a default covers it).
    pub required: bool,
    /// Whether the field is `secret` — write-only, never rendered or traced.
    pub secret: bool,
    /// The `describe:` human-readable purpose, if declared.
    pub describe: Option<String>,
}

impl Default for ConfigField {
    fn default() -> Self {
        ConfigField {
            name: String::new(),
            type_name: "any".to_string(),
            default: None,
            constraint: None,
            required: false,
            secret: false,
            describe: None,
        }
    }
}

/// The parsed body of a `§config:` file: an ordered set of field declarations.
/// Field order is preserved for deterministic rendering and stable
/// diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConfigDecl {
    /// The `§config:` header.
    pub header: Option<FileHeader>,
    /// The declared fields, in declaration order.
    pub fields: Vec<ConfigField>,
}

/// A statement: any node that can appear as a step body, sub-step, branch, or
/// loop-body element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    /// A command invocation.
    Command(CommandCall),
    /// A conditional chain.
    Conditional(Conditional),
    /// A `~FOR` loop.
    ForLoop(ForLoop),
    /// A `~UNTIL` loop.
    UntilLoop(UntilLoop),
    /// A `~PARALLEL` block.
    Parallel(ParallelBlock),
    /// A `?SWITCH` multi-branch dispatch.
    Switch(SwitchBlock),
    /// A `~MAP` collection transform.
    Map(MapBlock),
    /// A bare variable reference.
    VarRef(Variable),
    /// A raw/comment line the parser could not classify.
    Comment(Comment),
}

/// A numbered step in the `@steps` block.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Step {
    /// The step number (`0` for an unnumbered inline construct).
    pub number: u32,
    /// The main action on the step line.
    pub body: Option<Stmt>,
    /// An inline comment on the step line.
    pub comment: String,
    /// Nested indented sub-steps.
    pub sub_steps: Vec<Stmt>,
    /// `~RETRY:n` bound: re-run the step up to `n` times on a runtime error.
    /// `None` means no retry; `Some(0)` marks a present-but-invalid bound that
    /// the validator rejects.
    pub retry: Option<u32>,
    /// `~COMPENSATE: CMD(args)` — the host-supplied undo for this step's
    /// business effect. `None` means the step is honestly
    /// un-compensable, not that it has no effect.
    pub compensation: Option<CommandCall>,
}

/// The top-level node for a parsed workflow file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkflowFile {
    /// The `§wf:` header.
    pub header: Option<FileHeader>,
    /// The `§runtime` block.
    pub runtime: Option<RuntimeBlock>,
    /// `@ON` triggers.
    pub triggers: Vec<Trigger>,
    /// `!!` hard rules.
    pub rules: Vec<AbsoluteRule>,
    /// `!PREF` preferences.
    pub preferences: Vec<Preference>,
    /// The `@in` declaration.
    pub input_decl: Option<InputDecl>,
    /// The `@out` declaration.
    pub output_decl: Option<OutputDecl>,
    /// The `@ctx` declaration.
    pub context_decl: Option<ContextDecl>,
    /// The `@err` declaration.
    pub error_decl: Option<ErrorDecl>,
    /// The ordered `@steps`.
    pub steps: Vec<Step>,
    /// `@test` blocks.
    pub tests: Vec<TestBlock>,
    /// `@macro` blocks.
    pub macros: Vec<MacroBlock>,
    /// The `;; HUMAN MODE` block text, if present.
    pub human_mode: Option<String>,
    /// Free-standing comments.
    pub comments: Vec<Comment>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nodes_construct_and_compare() {
        let cmd = CommandCall {
            name: "GEN".to_string(),
            args: vec!["reply".to_string()],
            modifiers: vec![("+tone".to_string(), "warm".to_string())],
            pipeline_target: Some("$draft".to_string()),
            ..Default::default()
        };
        let same = cmd.clone();
        assert_eq!(cmd, same);
        assert_eq!(cmd.modifiers[0].1, "warm");
        assert_eq!(cmd.pipeline_target.as_deref(), Some("$draft"));
    }

    #[test]
    fn input_field_defaults_to_any() {
        let f = InputField::default();
        assert_eq!(f.type_name, "any");
        assert!(!f.optional);
        assert!(f.default.is_none());
    }

    #[test]
    fn statements_are_distinct_variants() {
        let a = Stmt::Command(CommandCall {
            name: "LOG".to_string(),
            ..Default::default()
        });
        let b = Stmt::VarRef(Variable {
            name: "$out".to_string(),
        });
        assert_ne!(a, b);
        assert!(matches!(a, Stmt::Command(_)));
        assert!(matches!(b, Stmt::VarRef(_)));
    }

    #[test]
    fn conditional_chains_nest() {
        let cond = Conditional {
            condition: "$x < 0.2".to_string(),
            break_flag: true,
            else_branch: Some(Box::new(Conditional {
                condition: String::new(),
                ..Default::default()
            })),
            ..Default::default()
        };
        assert!(cond.break_flag);
        assert!(cond.else_branch.is_some());
        // Round-trips through clone unchanged.
        assert_eq!(cond, cond.clone());
    }

    #[test]
    fn config_field_defaults_to_any_and_unrestricted() {
        let f = ConfigField::default();
        assert_eq!(f.type_name, "any");
        assert!(!f.required);
        assert!(!f.secret);
        assert!(f.default.is_none());
        assert!(f.constraint.is_none());
        assert!(f.describe.is_none());
    }

    #[test]
    fn config_decl_preserves_field_order() {
        let decl = ConfigDecl {
            header: Some(FileHeader {
                file_type: FileType::Config,
                name: "settings".to_string(),
                version: "v1.0".to_string(),
            }),
            fields: vec![
                ConfigField {
                    name: "max_retries".to_string(),
                    type_name: "int".to_string(),
                    default: Some("3".to_string()),
                    constraint: Some(FieldConstraint::Range {
                        lo: "1".to_string(),
                        hi: "10".to_string(),
                    }),
                    ..Default::default()
                },
                ConfigField {
                    name: "api_key".to_string(),
                    type_name: "str".to_string(),
                    required: true,
                    secret: true,
                    ..Default::default()
                },
            ],
        };
        assert_eq!(decl.fields[0].name, "max_retries");
        assert_eq!(decl.fields[1].name, "api_key");
        assert!(decl.fields[1].secret);
        assert_eq!(decl, decl.clone());
    }

    #[test]
    fn config_field_one_of_constraint() {
        let f = ConfigField {
            name: "level".to_string(),
            constraint: Some(FieldConstraint::OneOf(vec![
                "low".to_string(),
                "medium".to_string(),
                "high".to_string(),
            ])),
            ..Default::default()
        };
        match &f.constraint {
            Some(FieldConstraint::OneOf(vals)) => assert_eq!(vals.len(), 3),
            _ => panic!("expected OneOf constraint"),
        }
    }

    #[test]
    fn workflow_file_assembles() {
        let wf = WorkflowFile {
            header: Some(FileHeader {
                file_type: FileType::Workflow,
                name: "beautiful_mention".to_string(),
                version: "v1.0".to_string(),
            }),
            rules: vec![AbsoluteRule {
                kind: Some(RuleKind::Never),
                content: "publish WITHOUT validate".to_string(),
            }],
            steps: vec![Step {
                number: 1,
                body: Some(Stmt::Command(CommandCall {
                    name: "FETCH".to_string(),
                    args: vec!["$post_url".to_string()],
                    pipeline_target: Some("$raw".to_string()),
                    ..Default::default()
                })),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(wf.header.as_ref().unwrap().name, "beautiful_mention");
        assert_eq!(wf.rules[0].kind, Some(RuleKind::Never));
        assert_eq!(wf.steps.len(), 1);
        assert_eq!(wf, wf.clone());
        // Debug is non-empty (round-trip-friendly representation).
        assert!(!format!("{wf:?}").is_empty());
    }
}
