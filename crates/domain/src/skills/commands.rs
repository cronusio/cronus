//! Built-in command surface — the bridge between skill workflows and core
//! operations. A closed, versioned registry of `CommandSpec`s; every
//! dispatch validates its input against the command's schema, then the
//! caller's manifest grants, before the call is allowed through.
//!
//! Registering these commands into the nodus vocabulary
//! (`SchemaProvider::host_commands`) is the runtime wiring seam this module
//! feeds, not something it performs itself — no changes to the runtime
//! crate are made here (a genuine runtime gap routes through
//! that crate's own workspace, not this phase).

use crate::extensions::ExtensionPermissions;
use std::collections::HashMap;

/// WFL vocabulary categories a built-in command can belong to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandCategory {
    DataIo,
    Memory,
    Effects,
    Validation,
}

/// A typed input parameter ("typed parameters, validated before dispatch").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamType {
    String,
    Number,
    Bool,
    List,
    Object,
}

/// One parameter in a command's [`InputSchema`].
#[derive(Debug, Clone)]
pub struct ParamSpec {
    pub name: String,
    pub param_type: ParamType,
    pub required: bool,
}

impl ParamSpec {
    pub fn new(name: impl Into<String>, param_type: ParamType, required: bool) -> Self {
        ParamSpec {
            name: name.into(),
            param_type,
            required,
        }
    }
}

/// A dispatch-time argument value.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamValue {
    String(String),
    Number(f64),
    Bool(bool),
    List(Vec<ParamValue>),
    Object(HashMap<String, ParamValue>),
}

impl ParamValue {
    fn matches(&self, t: ParamType) -> bool {
        matches!(
            (self, t),
            (ParamValue::String(_), ParamType::String)
                | (ParamValue::Number(_), ParamType::Number)
                | (ParamValue::Bool(_), ParamType::Bool)
                | (ParamValue::List(_), ParamType::List)
                | (ParamValue::Object(_), ParamType::Object)
        )
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum SchemaError {
    MissingParam(String),
    TypeMismatch(String),
    UnknownParam(String),
}

impl std::fmt::Display for SchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SchemaError::MissingParam(name) => write!(f, "missing required param: {name}"),
            SchemaError::TypeMismatch(name) => write!(f, "type mismatch for param: {name}"),
            SchemaError::UnknownParam(name) => write!(f, "unknown param: {name}"),
        }
    }
}

impl std::error::Error for SchemaError {}

/// A command's typed parameter contract.
#[derive(Debug, Clone, Default)]
pub struct InputSchema {
    pub params: Vec<ParamSpec>,
}

impl InputSchema {
    pub fn new(params: impl IntoIterator<Item = ParamSpec>) -> Self {
        InputSchema {
            params: params.into_iter().collect(),
        }
    }

    /// Validate `args`: every required param present and type-matched, no
    /// unrecognized params ("validated before dispatch").
    pub fn validate(&self, args: &HashMap<String, ParamValue>) -> Result<(), SchemaError> {
        for spec in &self.params {
            match args.get(&spec.name) {
                Some(value) if value.matches(spec.param_type) => {}
                Some(_) => return Err(SchemaError::TypeMismatch(spec.name.clone())),
                None if spec.required => {
                    return Err(SchemaError::MissingParam(spec.name.clone()));
                }
                None => {}
            }
        }
        for key in args.keys() {
            if !self.params.iter().any(|p| &p.name == key) {
                return Err(SchemaError::UnknownParam(key.clone()));
            }
        }
        Ok(())
    }
}

/// A scope required from the calling skill's manifest before a command may
/// dispatch ("fs / network / secrets scopes checked against the skill
/// manifest").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequiredGrant {
    Fs(String),
    Network(String),
    Secrets(String),
}

impl RequiredGrant {
    fn satisfied_by(&self, perms: &ExtensionPermissions) -> bool {
        match self {
            RequiredGrant::Fs(scope) => perms.fs.iter().any(|s| s == scope),
            RequiredGrant::Network(scope) => perms.network.iter().any(|s| s == scope),
            RequiredGrant::Secrets(scope) => perms.secrets.iter().any(|s| s == scope),
        }
    }
}

/// The built-in command surface version. Bumped only by core releases, never
/// by skill installation — the lockstep test in this module's
/// `tests` pins the current value so an accidental bump fails loudly.
pub const SURFACE_VERSION: &str = "1.0.0";

/// One entry in the built-in command surface.
#[derive(Debug, Clone)]
pub struct CommandSpec {
    pub id: String,
    pub category: CommandCategory,
    pub input_schema: InputSchema,
    pub required_grants: Vec<RequiredGrant>,
    pub surface_version: &'static str,
}

impl CommandSpec {
    /// `surface_version` is always stamped from [`SURFACE_VERSION`] — no
    /// command can declare its own, which is what keeps the surface version
    /// a single core-release-controlled value rather than a per-command one.
    pub fn new(
        id: impl Into<String>,
        category: CommandCategory,
        input_schema: InputSchema,
        required_grants: impl IntoIterator<Item = RequiredGrant>,
    ) -> Self {
        CommandSpec {
            id: id.into(),
            category,
            input_schema,
            required_grants: required_grants.into_iter().collect(),
            surface_version: SURFACE_VERSION,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum DispatchError {
    UnknownCommand(String),
    Schema(SchemaError),
    MissingGrant(RequiredGrant),
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DispatchError::UnknownCommand(id) => write!(f, "unknown command: {id}"),
            DispatchError::Schema(e) => write!(f, "schema validation failed: {e}"),
            DispatchError::MissingGrant(g) => write!(f, "missing required grant: {g:?}"),
        }
    }
}

impl std::error::Error for DispatchError {}

/// The closed, versioned registry of built-in commands.
#[derive(Debug, Default)]
pub struct CommandRegistry {
    commands: HashMap<String, CommandSpec>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        CommandRegistry::default()
    }

    pub fn register(&mut self, spec: CommandSpec) {
        self.commands.insert(spec.id.clone(), spec);
    }

    pub fn get(&self, id: &str) -> Option<&CommandSpec> {
        self.commands.get(id)
    }

    /// Check that a dispatch is allowed: the command exists, `args` validate
    /// against its schema, and every required grant is present in the
    /// caller's manifest permissions. Schema is checked
    /// before grants — input shape is invalid regardless of who is calling.
    /// Invocation itself belongs to the execution model (a separate task).
    pub fn check_dispatch(
        &self,
        command_id: &str,
        args: &HashMap<String, ParamValue>,
        caller_permissions: &ExtensionPermissions,
    ) -> Result<&CommandSpec, DispatchError> {
        let spec = self
            .commands
            .get(command_id)
            .ok_or_else(|| DispatchError::UnknownCommand(command_id.to_string()))?;
        spec.input_schema
            .validate(args)
            .map_err(DispatchError::Schema)?;
        for grant in &spec.required_grants {
            if !grant.satisfied_by(caller_permissions) {
                return Err(DispatchError::MissingGrant(grant.clone()));
            }
        }
        Ok(spec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_grants_perms() -> ExtensionPermissions {
        ExtensionPermissions::default()
    }

    fn read_file_command() -> CommandSpec {
        CommandSpec::new(
            "fs.read_file",
            CommandCategory::DataIo,
            InputSchema::new([ParamSpec::new("path", ParamType::String, true)]),
            [RequiredGrant::Fs("read".to_string())],
        )
    }

    #[test]
    fn command_spec_carries_all_fields() {
        let spec = read_file_command();
        assert_eq!(spec.id, "fs.read_file");
        assert_eq!(spec.category, CommandCategory::DataIo);
        assert_eq!(spec.input_schema.params.len(), 1);
        assert_eq!(spec.required_grants, vec![RequiredGrant::Fs("read".into())]);
        assert_eq!(spec.surface_version, SURFACE_VERSION);
    }

    #[test]
    fn surface_version_is_pinned() {
        // Lockstep test: SURFACE_VERSION changes only with a core release.
        // An accidental bump must fail this test and force a deliberate ack.
        assert_eq!(SURFACE_VERSION, "1.0.0");
    }

    #[test]
    fn dispatch_rejects_unknown_command() {
        let registry = CommandRegistry::new();
        let perms = no_grants_perms();
        let err = registry
            .check_dispatch("missing", &HashMap::new(), &perms)
            .unwrap_err();
        assert_eq!(err, DispatchError::UnknownCommand("missing".to_string()));
    }

    #[test]
    fn dispatch_validates_input_before_checking_grants() {
        let mut registry = CommandRegistry::new();
        registry.register(read_file_command());
        // No `path` param and no grant — schema failure must surface, not the
        // grant failure, proving schema is checked first.
        let err = registry
            .check_dispatch("fs.read_file", &HashMap::new(), &no_grants_perms())
            .unwrap_err();
        assert_eq!(
            err,
            DispatchError::Schema(SchemaError::MissingParam("path".to_string()))
        );
    }

    #[test]
    fn dispatch_rejects_call_missing_required_grant() {
        let mut registry = CommandRegistry::new();
        registry.register(read_file_command());
        let mut args = HashMap::new();
        args.insert("path".to_string(), ParamValue::String("a.txt".into()));
        let err = registry
            .check_dispatch("fs.read_file", &args, &no_grants_perms())
            .unwrap_err();
        assert_eq!(
            err,
            DispatchError::MissingGrant(RequiredGrant::Fs("read".to_string()))
        );
    }

    #[test]
    fn dispatch_succeeds_with_valid_input_and_grant() {
        let mut registry = CommandRegistry::new();
        registry.register(read_file_command());
        let mut args = HashMap::new();
        args.insert("path".to_string(), ParamValue::String("a.txt".into()));
        let perms = ExtensionPermissions {
            fs: vec!["read".to_string()],
            network: vec![],
            secrets: vec![],
        };
        let spec = registry
            .check_dispatch("fs.read_file", &args, &perms)
            .unwrap();
        assert_eq!(spec.id, "fs.read_file");
    }
}
