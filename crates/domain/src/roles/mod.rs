//! Role catalog: preset blueprints and hired agent instances.
//!
//! Preset roles live in the program tier (read-only). Hiring copies a blueprint
//! into the state tier, creating a mutable instance with its own memory and
//! skills directories. Custom roles use the same on-disk format as presets.

use std::fs;
use std::path::PathBuf;

pub use catalog::*;
pub use instance::*;
pub use revision::*;

pub mod catalog;
pub mod instance;
pub mod revision;

// ── Error type ────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum RoleError {
    /// A `preset_id` that does not resolve against [`PRESET_CATALOG`] —
    /// distinct from [`RoleError::InstanceNotFound`]: "no such
    /// preset to hire from" and "no such hired instance" are different
    /// facts about different collections, and collapsing them into one
    /// generic "role not found" message named the wrong thing on whichever
    /// one it wasn't.
    PresetNotFound(String),
    /// A hired-instance `id` that has no directory under `employees/`.
    InstanceNotFound(String),
    AlreadyHired(String),
    PresetReadOnly,
    Io(std::io::Error),
    InvalidId(String),
}

impl std::fmt::Display for RoleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RoleError::PresetNotFound(id) => write!(f, "preset not found: {id}"),
            RoleError::InstanceNotFound(id) => write!(f, "hired instance not found: {id}"),
            RoleError::AlreadyHired(id) => write!(f, "role already hired: {id}"),
            RoleError::PresetReadOnly => write!(f, "preset roles are read-only"),
            RoleError::Io(e) => write!(f, "I/O error: {e}"),
            RoleError::InvalidId(s) => write!(f, "invalid role ID: {s}"),
        }
    }
}

impl std::error::Error for RoleError {}
impl From<std::io::Error> for RoleError {
    fn from(e: std::io::Error) -> Self {
        RoleError::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, RoleError>;

// ── Adapter protocol ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterLevel {
    Callable,
    StatusReporting,
    FullyInstrumented,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextDelivery {
    FatPayload,
    ThinPing,
}

// ── Role manager ──────────────────────────────────────────────────────────────

/// Manages the preset catalog and hired instances.
pub struct RoleManager {
    _program_dir: PathBuf,
    state_dir: PathBuf,
}

impl RoleManager {
    pub fn new(program_dir: PathBuf, state_dir: PathBuf) -> Self {
        RoleManager {
            _program_dir: program_dir,
            state_dir,
        }
    }

    /// List all preset roles from the embedded catalog.
    pub fn list_presets(&self) -> Vec<PresetRole> {
        PRESET_CATALOG.to_vec()
    }

    /// List all currently hired instances.
    pub fn list_hired(&self) -> Result<Vec<HiredInstance>> {
        let employees_dir = self.state_dir.join("employees");
        if !employees_dir.exists() {
            return Ok(Vec::new());
        }
        let mut instances = Vec::new();
        for entry in fs::read_dir(&employees_dir)?.flatten() {
            let path = entry.path();
            if path.is_dir()
                && let Some(inst) = HiredInstance::load_from(&path)?
            {
                instances.push(inst);
            }
        }
        Ok(instances)
    }

    /// Hire a preset role, creating a new instance in the state tier.
    pub fn hire(&self, preset_id: &str, custom_name: Option<&str>) -> Result<HiredInstance> {
        let preset = PRESET_CATALOG
            .iter()
            .find(|r| r.id == preset_id)
            .ok_or_else(|| RoleError::PresetNotFound(preset_id.to_string()))?;

        // Without a custom name, the id is `<preset>-<n>` for the lowest free
        // `n` — deterministic given the state tier and readable, rather than a
        // wall-clock millisecond stamp (which changed every run and could
        // collide on a fast double-hire).
        let (instance_id, instance_dir) = match custom_name {
            Some(n) => {
                let dir = self.state_dir.join("employees").join(n);
                if dir.exists() {
                    return Err(RoleError::AlreadyHired(n.to_string()));
                }
                (n.to_string(), dir)
            }
            None => {
                let employees = self.state_dir.join("employees");
                let mut n = 1u32;
                loop {
                    let id = format!("{preset_id}-{n}");
                    let dir = employees.join(&id);
                    if !dir.exists() {
                        break (id, dir);
                    }
                    n += 1;
                }
            }
        };

        fs::create_dir_all(&instance_dir)?;
        fs::create_dir_all(instance_dir.join("memory"))?;
        fs::create_dir_all(instance_dir.join("skills"))?;
        fs::create_dir_all(instance_dir.join("skins"))?;

        let instance = HiredInstance {
            id: instance_id,
            preset_id: preset_id.to_string(),
            hired_from: HiredFrom::Preset(preset_id.to_string()),
            reports_to: None,
            display_name: custom_name.unwrap_or(preset.name).to_string(),
            category: preset.category,
            adapter_level: AdapterLevel::Callable,
            context_delivery: ContextDelivery::ThinPing,
        };

        instance.save_to(&instance_dir)?;
        Ok(instance)
    }

    /// Create a custom role (not based on a preset).
    pub fn create_custom(&self, id: &str, display_name: &str) -> Result<HiredInstance> {
        validate_role_id(id)?;
        let instance_dir = self.state_dir.join("employees").join(id);
        if instance_dir.exists() {
            return Err(RoleError::AlreadyHired(id.to_string()));
        }

        fs::create_dir_all(&instance_dir)?;
        fs::create_dir_all(instance_dir.join("memory"))?;
        fs::create_dir_all(instance_dir.join("skills"))?;
        fs::create_dir_all(instance_dir.join("skins"))?;

        let instance = HiredInstance {
            id: id.to_string(),
            preset_id: id.to_string(),
            hired_from: HiredFrom::Custom,
            reports_to: None,
            display_name: display_name.to_string(),
            category: RoleCategory::Custom,
            adapter_level: AdapterLevel::Callable,
            context_delivery: ContextDelivery::ThinPing,
        };

        instance.save_to(&instance_dir)?;
        Ok(instance)
    }

    /// Create a custom role derived from a preset.
    pub fn create_from_preset(
        &self,
        id: &str,
        display_name: &str,
        preset_id: &str,
    ) -> Result<HiredInstance> {
        validate_role_id(id)?;
        let preset = PRESET_CATALOG
            .iter()
            .find(|r| r.id == preset_id)
            .ok_or_else(|| RoleError::PresetNotFound(preset_id.to_string()))?;

        let instance_dir = self.state_dir.join("employees").join(id);
        if instance_dir.exists() {
            return Err(RoleError::AlreadyHired(id.to_string()));
        }

        fs::create_dir_all(&instance_dir)?;
        fs::create_dir_all(instance_dir.join("memory"))?;
        fs::create_dir_all(instance_dir.join("skills"))?;
        fs::create_dir_all(instance_dir.join("skins"))?;

        let instance = HiredInstance {
            id: id.to_string(),
            preset_id: preset_id.to_string(),
            hired_from: HiredFrom::DerivedFrom(preset_id.to_string()),
            reports_to: None,
            display_name: display_name.to_string(),
            category: preset.category,
            adapter_level: AdapterLevel::Callable,
            context_delivery: ContextDelivery::ThinPing,
        };

        instance.save_to(&instance_dir)?;
        Ok(instance)
    }

    /// Get a hired instance by ID.
    pub fn get(&self, id: &str) -> Result<Option<HiredInstance>> {
        let instance_dir = self.state_dir.join("employees").join(id);
        if !instance_dir.exists() {
            return Ok(None);
        }
        HiredInstance::load_from(&instance_dir)
    }

    /// Fire (release) a hired instance — archives memory, removes from roster.
    pub fn fire(&self, id: &str) -> Result<()> {
        let instance_dir = self.state_dir.join("employees").join(id);
        if !instance_dir.exists() {
            return Err(RoleError::InstanceNotFound(id.to_string()));
        }

        let memory_dir = instance_dir.join("memory");
        if memory_dir.exists() {
            let archive_dir = instance_dir.join("archive-memory");
            fs::rename(&memory_dir, &archive_dir)?;
        }

        let fired_dir = self.state_dir.join("employees").join(format!("{id}-fired"));
        fs::rename(&instance_dir, &fired_dir)?;
        Ok(())
    }

    /// Mutate a preset — always returns an error (presets are read-only).
    pub fn mutate_preset(&self, _preset_id: &str) -> Result<()> {
        Err(RoleError::PresetReadOnly)
    }
}

fn validate_role_id(id: &str) -> Result<()> {
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(RoleError::InvalidId(id.to_string()));
    }
    Ok(())
}
