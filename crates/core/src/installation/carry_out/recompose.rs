//! The installation changes that recompose the office: they alter what running
//! work is built from, so a surface applies them only through the gate
//! (`installation::gate`) — which is also what each handler registered for
//! them does, so no surface reaches them any other way.
//!
//! Like the rest of `carry_out`, each function does the work and answers a
//! structured `Outcome`. Where a store lives at a default place, a `_default`
//! wrapper opens it and the function underneath takes the store, so a test
//! never touches the real one.

use std::path::Path;

use cronus_contract::{Outcome, OutcomeValue, Rejection, RejectionMode};
use cronus_domain::backup;
use cronus_domain::doctor::{self, DoctorInputs};

use crate::agent_registry::AgentRegistry;
use crate::archetype::ArchetypeCatalog;
use crate::auth::{DeveloperAdmissionStore, HumanPrincipal};
use crate::extensions::{ExtensionManifest, ExtensionRegistry, ExtensionState};
use crate::installation::inspect;

use super::catalogs::describe_io_error;

fn text(value: impl Into<String>) -> OutcomeValue {
    OutcomeValue::Text(value.into())
}

fn record(fields: Vec<(&str, OutcomeValue)>) -> Outcome {
    Outcome::Value(OutcomeValue::Record(
        fields
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect(),
    ))
}

fn unavailable(reason: impl Into<String>) -> Outcome {
    Outcome::Unavailable {
        reason: reason.into(),
    }
}

fn rejected(binder: &'static str, mode: RejectionMode, detail: impl Into<String>) -> Outcome {
    Outcome::Rejected(Rejection {
        binder,
        mode,
        detail: detail.into(),
    })
}

// ─── agent definitions ───────────────────────────────────────────────────────

/// Enable or disable the agent definition `name` in `registry` and persist it
/// with `save`. A definition that does not exist is `Unavailable` naming it:
/// it is not created by being disabled.
pub fn registry_set_enabled(
    registry: &mut AgentRegistry,
    save: impl FnOnce(&AgentRegistry) -> std::io::Result<()>,
    name: &str,
    enabled: bool,
) -> Outcome {
    if !registry.set_disabled(name, !enabled) {
        return unavailable(format!("agent definition not found: {name}"));
    }
    match save(registry) {
        Ok(()) => record(vec![
            ("name", text(name)),
            ("enabled", OutcomeValue::Boolean(enabled)),
        ]),
        Err(err) => unavailable(format!("could not persist the agent registry: {err}")),
    }
}

/// [`registry_set_enabled`] against the registry at its default place.
pub fn registry_set_enabled_default(name: &str, enabled: bool) -> Outcome {
    let mut registry = AgentRegistry::load();
    registry_set_enabled(&mut registry, |r| r.save(), name, enabled)
}

// ─── archetypes ──────────────────────────────────────────────────────────────

/// Make the shipped archetype `id` the active one for the workspace at
/// `state_dir`, or with `clear` make the office archetype-free. A blocked or
/// unknown archetype is refused, never silently accepted.
pub fn archetype_set(state_dir: &Path, id: Option<&str>, clear: bool) -> Outcome {
    let marker = inspect::active_archetype_marker(state_dir);
    if let Some(parent) = marker.parent()
        && let Err(err) = std::fs::create_dir_all(parent)
    {
        return unavailable(describe_io_error(&err, parent));
    }
    if clear {
        let _ = std::fs::remove_file(&marker);
        return record(vec![("active", OutcomeValue::Empty)]);
    }
    let Some(id) = id else {
        return rejected(
            "id",
            RejectionMode::Absent,
            "provide an archetype id or --clear",
        );
    };
    let catalog = ArchetypeCatalog::program();
    if catalog.get(id).is_none() {
        return rejected(
            "id",
            RejectionMode::Malformed,
            if catalog.blocked_status(id).is_some() {
                format!("'{id}' is declared-blocked and cannot be applied")
            } else {
                format!("archetype not found: {id}")
            },
        );
    }
    match std::fs::write(&marker, id) {
        Ok(()) => record(vec![("active", text(id))]),
        Err(err) => unavailable(describe_io_error(&err, &marker)),
    }
}

// ─── extensions ──────────────────────────────────────────────────────────────

fn declared(items: &[String]) -> String {
    if items.is_empty() {
        "(none declared)".to_string()
    } else {
        items.join(", ")
    }
}

/// What activating an extension grants it, in the person's words: exactly what
/// its manifest declares, and nothing it does not.
pub fn grant_lines(manifest: &ExtensionManifest) -> Vec<String> {
    vec![
        format!(
            "Activating '{}' ({} v{}) grants it what its manifest declares:",
            manifest.id,
            manifest.kind.as_str(),
            manifest.version
        ),
        format!("  filesystem: {}", declared(&manifest.permissions.fs)),
        format!("  network:    {}", declared(&manifest.permissions.network)),
        format!("  secrets:    {}", declared(&manifest.permissions.secrets)),
    ]
}

/// Activate the extension `id` in `registry` and persist it with `save`.
///
/// Activation grants an extension what its manifest declares, so it runs only
/// when the caller says the person has seen that and agreed (`acknowledged`):
/// without it, it is rejected naming the missing acknowledgement and nothing
/// changes.
pub fn ext_activate(
    registry: &mut ExtensionRegistry,
    save: impl FnOnce(&ExtensionRegistry) -> Result<(), String>,
    id: &str,
    acknowledged: bool,
) -> Outcome {
    if registry.manifest(id).is_none() {
        return unavailable(format!("extension not found: {id}"));
    }
    if !acknowledged {
        return rejected(
            "yes",
            RejectionMode::Absent,
            "activation grants an extension what its manifest declares and refuses to run \
             without the person's acknowledgement",
        );
    }
    // Discovered → Permitted → Active: the intermediate step first, which is
    // simply not valid from a state that is already past it.
    let _ = registry.transition(id, ExtensionState::Permitted);
    if let Err(err) = registry.transition(id, ExtensionState::Active) {
        return unavailable(err.to_string());
    }
    match save(registry) {
        Ok(()) => record(vec![("id", text(id))]),
        Err(err) => unavailable(format!("could not persist the extension registry: {err}")),
    }
}

/// Deactivate the extension `id` in `registry` and persist it with `save`.
pub fn ext_deactivate(
    registry: &mut ExtensionRegistry,
    save: impl FnOnce(&ExtensionRegistry) -> Result<(), String>,
    id: &str,
) -> Outcome {
    if let Err(err) = registry.transition(id, ExtensionState::Inactive) {
        return unavailable(err.to_string());
    }
    match save(registry) {
        Ok(()) => record(vec![("id", text(id))]),
        Err(err) => unavailable(format!("could not persist the extension registry: {err}")),
    }
}

fn with_extension_registry(
    change: impl FnOnce(
        &mut ExtensionRegistry,
        &dyn Fn(&ExtensionRegistry) -> Result<(), String>,
    ) -> Outcome,
) -> Outcome {
    match ExtensionRegistry::load() {
        Ok(mut registry) => change(&mut registry, &|r| r.save().map_err(|e| e.to_string())),
        Err(err) => unavailable(err.to_string()),
    }
}

/// [`ext_activate`] against the extension registry at its default place.
pub fn ext_activate_default(id: &str, acknowledged: bool) -> Outcome {
    with_extension_registry(|registry, save| ext_activate(registry, save, id, acknowledged))
}

/// [`ext_deactivate`] against the extension registry at its default place.
pub fn ext_deactivate_default(id: &str) -> Outcome {
    with_extension_registry(|registry, save| ext_deactivate(registry, save, id))
}

// ─── the state itself ────────────────────────────────────────────────────────

/// Restore the backup `backup_id` found under `backups_dir` into `state_root`.
pub fn restore(state_root: &Path, backups_dir: &Path, backup_id: &str) -> Outcome {
    let Ok(backups) = backup::list(backups_dir) else {
        return unavailable("could not read backups directory");
    };
    let Some(found) = backups.into_iter().find(|b| b.id == backup_id) else {
        return unavailable(format!("backup not found: {backup_id}"));
    };
    match backup::restore(&found, state_root) {
        Ok(()) => record(vec![
            ("id", text(backup_id)),
            ("into", text(crate::paths::display_clean(state_root))),
        ]),
        Err(err) => unavailable(format!("restore failed: {err}")),
    }
}

/// Run the diagnostics and apply the repairs that are safe. Answers the
/// findings, each tagged with what became of it, or that all checks passed; a
/// workspace that was never initialized is `Unavailable`, as for the read-only
/// check.
pub fn doctor_repair(state_root: &Path) -> Outcome {
    if !inspect::is_initialized(state_root) {
        return unavailable(inspect::NO_WORKSPACE_MESSAGE);
    }
    let report = doctor::repair(&DoctorInputs::default());
    if report.findings.is_empty() {
        return Outcome::Value(text(inspect::DOCTOR_ALL_CLEAR));
    }
    Outcome::Value(OutcomeValue::List(
        inspect::doctor_lines(&report, true)
            .into_iter()
            .map(text)
            .collect(),
    ))
}

// ─── developer-office admission ──────────────────────────────────────────────

/// Grant developer-office admission at `path`. The person running this *is*
/// the human principal: a legitimate admission, distinct from an agent
/// granting itself one, for which no tool exists.
pub fn dev_admit_at(path: &Path) -> Outcome {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match DeveloperAdmissionStore::open(path.to_path_buf())
        .mint(&HumanPrincipal::assert_human_operated())
    {
        Ok(()) => record(vec![("result", text("admitted"))]),
        Err(err) => unavailable(describe_io_error(&err, path)),
    }
}

/// Revoke developer-office admission at `path`.
pub fn dev_revoke_at(path: &Path) -> Outcome {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match DeveloperAdmissionStore::open(path.to_path_buf())
        .revoke(&HumanPrincipal::assert_human_operated())
    {
        Ok(()) => record(vec![("result", text("revoked"))]),
        Err(err) => unavailable(describe_io_error(&err, path)),
    }
}

/// [`dev_admit_at`] at the admission file's default place.
pub fn dev_admit() -> Outcome {
    dev_admit_at(&inspect::dev_admission_path())
}

/// [`dev_revoke_at`] at the admission file's default place.
pub fn dev_revoke() -> Outcome {
    dev_revoke_at(&inspect::dev_admission_path())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use cronus_contract::{Outcome, OutcomeValue};

    use super::*;
    use crate::extensions::{ExtensionKind, ExtensionPermissions, ExtensionSource};

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cronus-recompose-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("creatable");
        dir
    }

    fn field<'a>(outcome: &'a Outcome, name: &str) -> &'a OutcomeValue {
        let Outcome::Value(OutcomeValue::Record(fields)) = outcome else {
            panic!("expected a record, got {outcome:?}");
        };
        &fields
            .iter()
            .find(|(k, _)| k == name)
            .unwrap_or_else(|| panic!("no field {name} in {fields:?}"))
            .1
    }

    #[test]
    fn disabling_an_agent_definition_takes_it_out_of_the_active_list_and_enabling_brings_it_back() {
        let dir = temp("agents");
        let path = dir.join("registry.json");
        let mut registry = AgentRegistry::load_from(&path);
        assert!(registry.resolve("plan").is_ok());

        let off = registry_set_enabled(&mut registry, |r| r.save_to(&path), "plan", false);
        assert_eq!(field(&off, "enabled"), &OutcomeValue::Boolean(false));
        assert!(registry.resolve("plan").is_err());
        assert!(
            AgentRegistry::load_from(&path).resolve("plan").is_err(),
            "persisted"
        );

        let on = registry_set_enabled(&mut registry, |r| r.save_to(&path), "plan", true);
        assert_eq!(field(&on, "enabled"), &OutcomeValue::Boolean(true));
        assert!(
            AgentRegistry::load_from(&path).resolve("plan").is_ok(),
            "and back"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_agent_definition_that_does_not_exist_is_named_and_not_created_by_being_disabled() {
        let mut registry = AgentRegistry::new();
        let before = registry.list_active().len();
        match registry_set_enabled(&mut registry, |_| panic!("nothing to save"), "ghost", false) {
            Outcome::Unavailable { reason } => {
                assert_eq!(reason, "agent definition not found: ghost");
            }
            other => panic!("expected the miss to be named, got {other:?}"),
        }
        assert_eq!(registry.list_active().len(), before);
    }

    #[test]
    fn an_archetype_is_set_active_and_cleared() {
        let dir = temp("archetype");
        let set = archetype_set(&dir, Some("software-engineering"), false);
        assert_eq!(
            field(&set, "active"),
            &OutcomeValue::Text("software-engineering".to_string())
        );
        assert_eq!(
            inspect::read_active_archetype(&dir).as_deref(),
            Some("software-engineering")
        );

        let cleared = archetype_set(&dir, None, true);
        assert_eq!(field(&cleared, "active"), &OutcomeValue::Empty);
        assert_eq!(inspect::read_active_archetype(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_blocked_or_unknown_archetype_and_a_missing_id_are_refused_naming_the_id() {
        let dir = temp("archetype-bad");
        for (id, said) in [
            (Some("advertising-agency"), "declared-blocked"),
            (Some("no-such"), "archetype not found"),
        ] {
            match archetype_set(&dir, id, false) {
                Outcome::Rejected(r) => {
                    assert_eq!(r.binder, "id");
                    assert!(r.detail.contains(said), "{r:?}");
                }
                other => panic!("expected a rejection, got {other:?}"),
            }
        }
        match archetype_set(&dir, None, false) {
            Outcome::Rejected(r) => assert_eq!(r.mode, RejectionMode::Absent),
            other => panic!("expected a rejection, got {other:?}"),
        }
        assert_eq!(
            inspect::read_active_archetype(&dir),
            None,
            "nothing was set"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn registry_with(id: &str) -> ExtensionRegistry {
        let mut registry = ExtensionRegistry::default();
        registry
            .register(ExtensionManifest {
                id: id.to_string(),
                kind: ExtensionKind::Skill,
                name: id.to_string(),
                version: "1.0.0".to_string(),
                source: ExtensionSource::Custom,
                capabilities: Vec::new(),
                permissions: ExtensionPermissions {
                    fs: vec!["read:notes".to_string()],
                    network: Vec::new(),
                    secrets: Vec::new(),
                },
            })
            .expect("registers");
        registry
    }

    #[test]
    fn activating_needs_the_acknowledgement_and_then_makes_the_extension_active() {
        let mut registry = registry_with("demo/x");

        match ext_activate(
            &mut registry,
            |_| panic!("nothing to save"),
            "demo/x",
            false,
        ) {
            Outcome::Rejected(r) => assert_eq!(r.binder, "yes"),
            other => panic!("expected the missing acknowledgement, got {other:?}"),
        }
        assert_eq!(registry.state("demo/x"), Some(ExtensionState::Discovered));

        let done = ext_activate(&mut registry, |_| Ok(()), "demo/x", true);
        assert_eq!(
            field(&done, "id"),
            &OutcomeValue::Text("demo/x".to_string())
        );
        assert_eq!(registry.state("demo/x"), Some(ExtensionState::Active));
    }

    #[test]
    fn what_activation_grants_is_what_the_manifest_declares_in_words() {
        let registry = registry_with("demo/x");
        let lines = grant_lines(registry.manifest("demo/x").expect("registered"));
        assert!(lines[0].contains("'demo/x'"), "{lines:?}");
        assert_eq!(lines[1], "  filesystem: read:notes");
        assert_eq!(lines[2], "  network:    (none declared)");
    }

    #[test]
    fn an_unknown_extension_is_named_and_deactivating_what_is_active_works() {
        let mut registry = registry_with("demo/x");
        match ext_activate(&mut registry, |_| Ok(()), "demo/nope", true) {
            Outcome::Unavailable { reason } => assert_eq!(reason, "extension not found: demo/nope"),
            other => panic!("expected the miss to be named, got {other:?}"),
        }
        let _ = ext_activate(&mut registry, |_| Ok(()), "demo/x", true);
        let off = ext_deactivate(&mut registry, |_| Ok(()), "demo/x");
        assert_eq!(field(&off, "id"), &OutcomeValue::Text("demo/x".to_string()));
        assert_eq!(registry.state("demo/x"), Some(ExtensionState::Inactive));
    }

    #[test]
    fn a_backup_is_restored_into_the_state_root() {
        let dir = temp("restore");
        let (state, backups) = (dir.join("state"), dir.join("backups"));
        std::fs::create_dir_all(&state).expect("creatable");
        std::fs::write(state.join("config.json"), "{\"v\":1}").expect("writable");
        let made = backup::create(&state, &backups, None, backup::BackupOptions::default())
            .expect("a backup");
        std::fs::write(state.join("config.json"), "{\"v\":2}").expect("writable");

        let outcome = restore(&state, &backups, &made.id);

        assert_eq!(field(&outcome, "id"), &OutcomeValue::Text(made.id));
        assert_eq!(
            std::fs::read_to_string(state.join("config.json")).expect("reads"),
            "{\"v\":1}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restoring_a_backup_that_is_not_there_names_it() {
        let dir = temp("restore-missing");
        std::fs::create_dir_all(dir.join("backups")).expect("creatable");
        match restore(&dir.join("state"), &dir.join("backups"), "backup-1") {
            Outcome::Unavailable { reason } => assert_eq!(reason, "backup not found: backup-1"),
            other => panic!("expected the miss to be named, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn diagnostics_with_repair_say_there_is_no_workspace_rather_than_a_repairable_defect() {
        let dir = temp("doctor");
        match doctor_repair(&dir) {
            Outcome::Unavailable { reason } => assert_eq!(reason, inspect::NO_WORKSPACE_MESSAGE),
            other => panic!("expected the missing workspace to be named, got {other:?}"),
        }
        std::fs::write(dir.join("app.json"), "{}\n").expect("writable");
        assert!(matches!(doctor_repair(&dir), Outcome::Value(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn developer_admission_is_granted_and_revoked_at_the_path_it_was_given() {
        let dir = temp("admission");
        let path = dir.join("dev_office").join("admission.txt");

        let admitted = dev_admit_at(&path);
        assert_eq!(
            field(&admitted, "result"),
            &OutcomeValue::Text("admitted".to_string())
        );
        assert!(path.exists());

        let revoked = dev_revoke_at(&path);
        assert_eq!(
            field(&revoked, "result"),
            &OutcomeValue::Text("revoked".to_string())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
