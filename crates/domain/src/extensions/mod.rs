//! Extension registry — skills, MCP servers, and plugins.
//!
//! Foundation: manifest validation, lifecycle state machine, auto-discovery.
//! MCP transport and sandbox enforcement wiring is deferred.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExtensionKind {
    Skill,
    McpServer,
    Plugin,
}

impl ExtensionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExtensionKind::Skill => "skill",
            ExtensionKind::McpServer => "mcp-server",
            ExtensionKind::Plugin => "plugin",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExtensionSource {
    Preset,
    Custom,
    Generated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExtensionState {
    Discovered,
    Permitted,
    Active,
    Inactive,
}

impl ExtensionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExtensionState::Discovered => "discovered",
            ExtensionState::Permitted => "permitted",
            ExtensionState::Active => "active",
            ExtensionState::Inactive => "inactive",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ExtensionPermissions {
    pub fs: Vec<String>,
    pub network: Vec<String>,
    pub secrets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionManifest {
    pub id: String,
    pub kind: ExtensionKind,
    pub name: String,
    pub version: String,
    pub source: ExtensionSource,
    pub capabilities: Vec<String>,
    pub permissions: ExtensionPermissions,
}

#[derive(Debug)]
pub enum RegistryError {
    InvalidManifest(String),
    NotFound(String),
    InvalidTransition {
        from: ExtensionState,
        to: ExtensionState,
    },
    InactiveExtension(String),
    Storage(String),
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegistryError::Storage(s) => write!(f, "extension store: {s}"),
            RegistryError::InvalidManifest(s) => write!(f, "invalid manifest: {s}"),
            RegistryError::NotFound(id) => write!(f, "extension not found: {id}"),
            RegistryError::InvalidTransition { from, to } => {
                write!(f, "invalid transition: {:?} → {:?}", from, to)
            }
            RegistryError::InactiveExtension(id) => write!(f, "extension not active: {id}"),
        }
    }
}

impl std::error::Error for RegistryError {}
pub type Result<T> = std::result::Result<T, RegistryError>;

pub fn validate_manifest(manifest: &ExtensionManifest) -> Result<()> {
    if manifest.id.is_empty() {
        return Err(RegistryError::InvalidManifest("id is required".to_string()));
    }
    if manifest.name.is_empty() {
        return Err(RegistryError::InvalidManifest(
            "name is required".to_string(),
        ));
    }
    if manifest.version.is_empty() {
        return Err(RegistryError::InvalidManifest(
            "version is required".to_string(),
        ));
    }
    Ok(())
}

#[derive(Debug)]
pub struct ExtensionRegistry {
    entries: HashMap<String, (ExtensionManifest, ExtensionState)>,
}

impl ExtensionRegistry {
    pub fn new() -> Self {
        ExtensionRegistry {
            entries: HashMap::new(),
        }
    }

    /// Where the registry persists between process runs: one index file under
    /// the state tier's `extensions/` directory. Honors the portable-mode
    /// override, so a test or QA run never touches the real user directory.
    pub fn persist_path() -> PathBuf {
        crate::paths::Paths::os_native()
            .resolve(crate::paths::Root::State)
            .join("extensions")
            .join("registry.json")
    }

    /// Load the persisted registry. A missing store is a fresh, empty
    /// registry; a store that exists but cannot be read or parsed is an
    /// error — never silently an empty registry, since an "empty" answer
    /// would hide every registered and activated extension.
    pub fn load() -> Result<Self> {
        Self::load_from(&Self::persist_path())
    }

    /// [`ExtensionRegistry::load`] against an explicit path.
    pub fn load_from(path: &Path) -> Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::new()),
            Err(e) => {
                return Err(RegistryError::Storage(format!(
                    "cannot read {}: {e}",
                    path.display()
                )));
            }
        };
        let stored: Vec<StoredEntry> = serde_json::from_str(&text).map_err(|e| {
            RegistryError::Storage(format!("{} is not a valid store: {e}", path.display()))
        })?;
        let mut registry = Self::new();
        for entry in stored {
            validate_manifest(&entry.manifest)?;
            let id = entry.manifest.id.clone();
            if registry
                .entries
                .insert(id.clone(), (entry.manifest, entry.state))
                .is_some()
            {
                return Err(RegistryError::Storage(format!(
                    "{} lists extension '{id}' more than once",
                    path.display()
                )));
            }
        }
        Ok(registry)
    }

    /// Persist the registry to its default location.
    pub fn save(&self) -> Result<()> {
        self.save_to(&Self::persist_path())
    }

    /// [`ExtensionRegistry::save`] against an explicit path. Written to a
    /// sibling file and renamed into place, so an interrupted write can never
    /// leave the store half-written — an unreadable store is a load error.
    pub fn save_to(&self, path: &Path) -> Result<()> {
        let stored: Vec<StoredEntry> = self
            .list()
            .into_iter()
            .map(|(manifest, state)| StoredEntry {
                manifest: manifest.clone(),
                state,
            })
            .collect();
        let json = serde_json::to_string_pretty(&stored)
            .map_err(|e| RegistryError::Storage(e.to_string()))?;
        let io = |e: std::io::Error| RegistryError::Storage(format!("{}: {e}", path.display()));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        let staged = path.with_extension("json.tmp");
        std::fs::write(&staged, json).map_err(io)?;
        std::fs::rename(&staged, path).map_err(io)
    }

    /// Registering an id that already exists replaces it and resets it to
    /// `Discovered`, so a changed manifest can never inherit an earlier grant.
    pub fn register(&mut self, manifest: ExtensionManifest) -> Result<()> {
        validate_manifest(&manifest)?;
        self.entries
            .insert(manifest.id.clone(), (manifest, ExtensionState::Discovered));
        Ok(())
    }

    pub fn state(&self, id: &str) -> Option<ExtensionState> {
        self.entries.get(id).map(|(_, s)| *s)
    }

    pub fn manifest(&self, id: &str) -> Option<&ExtensionManifest> {
        self.entries.get(id).map(|(m, _)| m)
    }

    pub fn transition(&mut self, id: &str, to: ExtensionState) -> Result<()> {
        let entry = self
            .entries
            .get_mut(id)
            .ok_or_else(|| RegistryError::NotFound(id.to_string()))?;
        let from = entry.1;
        let valid = matches!(
            (from, to),
            (ExtensionState::Discovered, ExtensionState::Permitted)
                | (ExtensionState::Permitted, ExtensionState::Active)
                | (ExtensionState::Active, ExtensionState::Inactive)
                | (ExtensionState::Inactive, ExtensionState::Active)
        );
        if !valid {
            return Err(RegistryError::InvalidTransition { from, to });
        }
        entry.1 = to;
        Ok(())
    }

    pub fn require_active(&self, id: &str) -> Result<&ExtensionManifest> {
        let (manifest, state) = self
            .entries
            .get(id)
            .ok_or_else(|| RegistryError::NotFound(id.to_string()))?;
        if *state != ExtensionState::Active {
            return Err(RegistryError::InactiveExtension(id.to_string()));
        }
        Ok(manifest)
    }

    pub fn list(&self) -> Vec<(&ExtensionManifest, ExtensionState)> {
        let mut all: Vec<_> = self.entries.values().map(|(m, s)| (m, *s)).collect();
        all.sort_by(|a, b| a.0.id.cmp(&b.0.id));
        all
    }
}

#[derive(Serialize, Deserialize)]
struct StoredEntry {
    manifest: ExtensionManifest,
    state: ExtensionState,
}

impl Default for ExtensionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_path(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir()
            .join(format!(
                "cronus-ext-store-{tag}-{}-{nanos}",
                std::process::id()
            ))
            .join("registry.json")
    }

    fn manifest(id: &str) -> ExtensionManifest {
        ExtensionManifest {
            id: id.to_string(),
            kind: ExtensionKind::McpServer,
            name: format!("{id} name"),
            version: "1.2.3".to_string(),
            source: ExtensionSource::Generated,
            capabilities: vec!["read".to_string()],
            permissions: ExtensionPermissions {
                fs: vec!["scoped/path".to_string()],
                ..ExtensionPermissions::default()
            },
        }
    }

    #[test]
    fn a_saved_registry_loads_back_with_every_manifest_and_state() {
        let path = store_path("round-trip");
        let mut registry = ExtensionRegistry::new();
        registry.register(manifest("b/two")).unwrap();
        registry.register(manifest("a/one")).unwrap();
        registry
            .transition("a/one", ExtensionState::Permitted)
            .unwrap();
        registry
            .transition("a/one", ExtensionState::Active)
            .unwrap();
        registry.save_to(&path).unwrap();

        let loaded = ExtensionRegistry::load_from(&path).unwrap();
        assert_eq!(loaded.state("a/one"), Some(ExtensionState::Active));
        assert_eq!(loaded.state("b/two"), Some(ExtensionState::Discovered));
        let listed: Vec<&str> = loaded.list().iter().map(|(m, _)| m.id.as_str()).collect();
        assert_eq!(listed, ["a/one", "b/two"], "list is ordered by id");
        let restored = loaded.require_active("a/one").unwrap();
        assert_eq!(restored.kind, ExtensionKind::McpServer);
        assert_eq!(restored.source, ExtensionSource::Generated);
        assert_eq!(restored.permissions.fs, ["scoped/path"]);

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_missing_store_is_an_empty_registry_not_an_error() {
        let path = store_path("missing");
        let loaded = ExtensionRegistry::load_from(&path).unwrap();
        assert!(loaded.list().is_empty());
    }

    #[test]
    fn a_corrupt_store_is_an_error_never_an_empty_registry() {
        let path = store_path("corrupt");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ not json").unwrap();

        let err = ExtensionRegistry::load_from(&path).unwrap_err();
        assert!(matches!(err, RegistryError::Storage(_)), "got {err:?}");

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_store_listing_one_id_twice_is_rejected() {
        let path = store_path("duplicate");
        let mut registry = ExtensionRegistry::new();
        registry.register(manifest("dup/one")).unwrap();
        registry.save_to(&path).unwrap();
        let single = std::fs::read_to_string(&path).unwrap();
        let entry = single.trim().trim_start_matches('[').trim_end_matches(']');
        std::fs::write(&path, format!("[{entry},{entry}]")).unwrap();

        let err = ExtensionRegistry::load_from(&path).unwrap_err();
        assert!(matches!(err, RegistryError::Storage(_)), "got {err:?}");

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_store_holding_an_invalid_manifest_is_rejected() {
        let path = store_path("invalid");
        let mut registry = ExtensionRegistry::new();
        registry.register(manifest("ok/one")).unwrap();
        registry.save_to(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, text.replace("\"ok/one\"", "\"\"")).unwrap();

        let err = ExtensionRegistry::load_from(&path).unwrap_err();
        assert!(
            matches!(err, RegistryError::InvalidManifest(_)),
            "got {err:?}"
        );

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn re_registering_an_active_extension_resets_it_to_discovered() {
        let mut registry = ExtensionRegistry::new();
        registry.register(manifest("re/reg")).unwrap();
        registry
            .transition("re/reg", ExtensionState::Permitted)
            .unwrap();
        registry
            .transition("re/reg", ExtensionState::Active)
            .unwrap();

        registry.register(manifest("re/reg")).unwrap();
        assert_eq!(registry.state("re/reg"), Some(ExtensionState::Discovered));
    }
}
