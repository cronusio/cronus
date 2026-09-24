//! Agent registry — runtime catalog of agent definitions.
//!
//! Built-in agents loaded first; user config applies on top (highest precedence).
//! `model_ref` resolution falls back to run-default for unknown group names.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentMode {
    Primary,
    SubAgent,
    All,
}

impl AgentMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentMode::Primary => "primary",
            AgentMode::SubAgent => "subagent",
            AgentMode::All => "all",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Ruleset {
    pub entries: HashMap<String, String>,
}

impl Ruleset {
    /// Last-key-wins merge: override entries take precedence.
    pub fn merge(base: &Ruleset, override_: &Ruleset) -> Ruleset {
        let mut merged = base.entries.clone();
        for (k, v) in &override_.entries {
            merged.insert(k.clone(), v.clone());
        }
        Ruleset { entries: merged }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefinition {
    pub name: String,
    pub description: Option<String>,
    pub mode: AgentMode,
    pub native: bool,
    pub hidden: bool,
    pub temperature: Option<f32>,
    pub permission: Ruleset,
    pub model_ref: Option<String>,
    pub steps: Option<u32>,
    pub tool_allowlist: Option<Vec<String>>,
    pub disabled: bool,
}

impl AgentDefinition {
    fn builtin(name: &str, description: &str, mode: AgentMode) -> Self {
        AgentDefinition {
            name: name.to_string(),
            description: Some(description.to_string()),
            mode,
            native: true,
            hidden: false,
            temperature: None,
            permission: Ruleset::default(),
            model_ref: Some("default".to_string()),
            steps: None,
            tool_allowlist: None,
            disabled: false,
        }
    }
}

static BUILTIN_NAMES: &[(&str, &str, AgentMode)] = &[
    (
        "work",
        "Default interactive agent; full permission profile",
        AgentMode::Primary,
    ),
    ("code", "Code generation and modification", AgentMode::All),
    (
        "plan",
        "Planning and task decomposition",
        AgentMode::SubAgent,
    ),
    ("edit", "Targeted file editing", AgentMode::SubAgent),
    ("search", "Codebase and web search", AgentMode::SubAgent),
    ("test", "Test authoring and execution", AgentMode::SubAgent),
    (
        "refactor",
        "Code refactoring and cleanup",
        AgentMode::SubAgent,
    ),
];

#[derive(Debug)]
pub enum RegistryError {
    NotFound(String),
    Disabled(String),
    UnknownBuiltin(String),
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegistryError::NotFound(n) => write!(f, "agent not found: {n}"),
            RegistryError::Disabled(n) => write!(f, "agent is disabled: {n}"),
            RegistryError::UnknownBuiltin(n) => write!(f, "unknown built-in agent: {n}"),
        }
    }
}

impl std::error::Error for RegistryError {}
pub type Result<T> = std::result::Result<T, RegistryError>;

pub struct AgentRegistry {
    agents: HashMap<String, AgentDefinition>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        let mut agents = HashMap::new();
        for (name, desc, mode) in BUILTIN_NAMES {
            agents.insert(
                name.to_string(),
                AgentDefinition::builtin(name, desc, *mode),
            );
        }
        AgentRegistry { agents }
    }

    pub fn resolve(&self, name: &str) -> Result<&AgentDefinition> {
        let def = self
            .agents
            .get(name)
            .ok_or_else(|| RegistryError::NotFound(name.to_string()))?;
        if def.disabled {
            return Err(RegistryError::Disabled(name.to_string()));
        }
        Ok(def)
    }

    /// Apply a user-config override. A `disable: true` entry disables the built-in.
    pub fn apply_user_config(&mut self, name: &str, disable: bool, model_ref: Option<String>) {
        if let Some(def) = self.agents.get_mut(name) {
            if disable {
                def.disabled = true;
            }
            if let Some(mr) = model_ref {
                def.model_ref = Some(mr);
            }
        } else {
            // Custom agent
            let mut def = AgentDefinition::builtin(name, "", AgentMode::All);
            def.native = false;
            def.disabled = disable;
            if let Some(mr) = model_ref {
                def.model_ref = Some(mr);
            }
            self.agents.insert(name.to_string(), def);
        }
    }

    /// Active agent definitions, sorted by name: `agents` is a
    /// `HashMap`, whose iteration order is randomized per process — without
    /// this, `registry list` printed a different first row on every run,
    /// unusable for anything that diffs or scripts over the output.
    pub fn list_active(&self) -> Vec<&AgentDefinition> {
        let mut agents: Vec<&AgentDefinition> =
            self.agents.values().filter(|d| !d.disabled).collect();
        agents.sort_by(|a, b| a.name.cmp(&b.name));
        agents
    }

    pub fn builtin_count(&self) -> usize {
        self.agents.values().filter(|d| d.native).count()
    }

    /// Insert a custom agent definition into the registry.
    pub fn register_custom(&mut self, def: AgentDefinition) {
        self.agents.insert(def.name.clone(), def);
    }

    /// State-tier file holding everything the built-in seed does not: custom
    /// agents in full, plus any built-in carrying a user override (disabled,
    /// `model_ref`, description).
    ///
    /// Deliberately kept at the OS-native state tier rather than scoped to
    /// the current workspace (unlike `board`/`memory`/`knowledge`/`role`/
    /// `schedule`): an agent *definition* is a reusable template a
    /// user builds once and expects to have available in every project, not
    /// data belonging to one of them — the same character the shipped
    /// preset catalog already has.
    pub fn persist_path() -> PathBuf {
        crate::paths::Paths::os_native()
            .resolve(crate::paths::Root::State)
            .join("agents")
            .join("registry.json")
    }

    /// Built-in seed with the persisted overlay applied on top. A missing or
    /// unreadable file yields just the built-ins — the registry is always
    /// usable, never blocked on its own store.
    pub fn load() -> Self {
        Self::load_from(&Self::persist_path())
    }

    /// [`AgentRegistry::load`] against an explicit path.
    pub fn load_from(path: &std::path::Path) -> Self {
        let mut reg = Self::new();
        let Ok(text) = std::fs::read_to_string(path) else {
            return reg;
        };
        let Ok(saved) = serde_json::from_str::<Vec<AgentDefinition>>(&text) else {
            return reg;
        };
        for def in saved {
            reg.agents.insert(def.name.clone(), def);
        }
        reg
    }

    /// Persist the overlay: every non-native agent, plus any native one that
    /// diverges from its seeded shape (disabled, or a changed `model_ref`).
    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&Self::persist_path())
    }

    /// [`AgentRegistry::save`] against an explicit path.
    pub fn save_to(&self, path: &std::path::Path) -> std::io::Result<()> {
        let seeded: std::collections::HashSet<&str> =
            BUILTIN_NAMES.iter().map(|(name, _, _)| *name).collect();
        let mut overlay: Vec<&AgentDefinition> = self
            .agents
            .values()
            .filter(|d| {
                !d.native
                    || d.disabled
                    || d.model_ref.as_deref() != Some("default")
                    || !seeded.contains(d.name.as_str())
            })
            .collect();
        overlay.sort_by(|a, b| a.name.cmp(&b.name));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&overlay)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, json)
    }

    /// Generate a stub AgentDefinition from a description (seam; real LLM wiring deferred).
    pub fn generate_from_description(name: &str, description: &str) -> AgentDefinition {
        AgentDefinition {
            name: name.to_string(),
            description: Some(description.to_string()),
            mode: AgentMode::All,
            native: false,
            hidden: false,
            temperature: None,
            permission: Ruleset::default(),
            model_ref: Some("default".to_string()),
            steps: Some(50),
            tool_allowlist: None,
            disabled: false,
        }
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_file(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "cronus-agentreg-{tag}-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ))
    }

    #[test]
    fn a_created_custom_agent_survives_a_reload() {
        let path = tmp_file("custom");
        let _ = std::fs::remove_file(&path);

        let mut reg = AgentRegistry::load_from(&path);
        reg.register_custom(AgentRegistry::generate_from_description(
            "reviewer",
            "reviews diffs",
        ));
        reg.save_to(&path).unwrap();

        let reloaded = AgentRegistry::load_from(&path);
        let names: Vec<&str> = reloaded
            .list_active()
            .iter()
            .map(|d| d.name.as_str())
            .collect();
        assert!(
            names.contains(&"reviewer"),
            "a custom agent must be visible after reload, got {names:?}"
        );
        // built-ins are still there too
        assert!(names.contains(&"work"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn disabling_a_builtin_persists_and_hides_it_from_the_active_list() {
        let path = tmp_file("disable");
        let _ = std::fs::remove_file(&path);

        let mut reg = AgentRegistry::load_from(&path);
        reg.apply_user_config("code", true, None);
        reg.save_to(&path).unwrap();

        let reloaded = AgentRegistry::load_from(&path);
        assert!(
            reloaded.resolve("code").is_err(),
            "a persisted-disabled built-in must not resolve"
        );
        assert!(
            !reloaded.list_active().iter().any(|d| d.name == "code"),
            "a persisted-disabled built-in must be absent from the active list"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_absent_store_yields_exactly_the_builtins() {
        let reg = AgentRegistry::load_from(std::path::Path::new(
            "definitely-not-a-real-registry-file.json",
        ));
        assert_eq!(reg.list_active().len(), BUILTIN_NAMES.len());
    }

    /// `agents` is a `HashMap`, whose iteration order is randomized
    /// per process — `list_active()` must sort rather than pass that
    /// randomness through, or `registry list` prints a different order on
    /// every run.
    #[test]
    fn list_active_is_sorted_by_name() {
        let reg = AgentRegistry::load_from(std::path::Path::new(
            "definitely-not-a-real-registry-file-either.json",
        ));
        let names: Vec<&str> = reg.list_active().iter().map(|d| d.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted, "list_active() must already be sorted");
    }
}
