//! Reading the installation, once, for every frontend that shows it.
//!
//! Two kinds of function live here. The *acquisition* helpers — where the
//! backups directory is, which file records the active archetype, how a
//! workspace name is derived from a state root — are the facts a frontend
//! used to rediscover on its own; a second frontend repeating them would be a
//! second statement of each. The *outcome* functions run one read-only
//! installation verb against the real installation and shape the answer as a
//! structured [`Outcome`], so a frontend renders it (text, JSON, a block in a
//! terminal view) without recomputing what it says.
//!
//! Everything here is read-only. Two details of that are worth stating:
//! a verb whose subject is absent answers [`Outcome::Unavailable`] naming
//! what is missing — never an empty value, because "there is nothing" and
//! "this could not be read" are different facts — and the workspace registry
//! is opened the way the command line has always opened it, which creates an
//! empty registry file when none exists yet; no other store is created or
//! written by any function in this module.

use std::path::{Path, PathBuf};

use cronus_contract::{Outcome, OutcomeValue, Rejection, RejectionMode};

use crate::activation_bootstrap::{
    ActivationRegistry, ActivationState, default_activation_registry,
};
use crate::agent_registry::AgentRegistry;
use crate::archetype::ArchetypeCatalog;
use crate::dev_office::{AdmissionReader, AdmissionTier, DevOfficeGate, GateInputs};
use crate::dev_office_gate::{AuthLocalAdmissionReader, repo_authenticity};
use crate::doctor::{self, Disposition, DoctorInputs};
use crate::extensions::ExtensionRegistry;
use crate::paths::{Paths, Root};
use crate::tool_security::SkillScanner;
use crate::workspace::{WorkspaceId, WorkspaceManager};
use crate::{backup, paths};

/// What a verb that needs an initialized workspace answers when the resolved
/// state root has none.
pub const NO_WORKSPACE_MESSAGE: &str = "No workspace initialized. Run 'cronus init' first.";

/// The line a clean diagnostic run reports.
pub const DOCTOR_ALL_CLEAR: &str = "doctor: all checks passed";

/// The shipped default for the developer office's feedback tier: off. A
/// build or deploy opt-in, never a runtime flag.
const FEEDBACK_TIER_ENABLED: bool = false;

// ─── acquisition ─────────────────────────────────────────────────────────────

/// Whether `state_root` holds an initialized workspace.
pub fn is_initialized(state_root: &Path) -> bool {
    state_root.join("app.json").exists()
}

/// The workspace's name as shown to a person. For a project-local `.cronus`
/// state directory it is the project directory's name, not the literal
/// `.cronus`.
pub fn workspace_name(state_root: &Path) -> String {
    let name_source = if state_root.file_name().and_then(|n| n.to_str()) == Some(".cronus") {
        state_root.parent().unwrap_or(state_root)
    } else {
        state_root
    };
    name_source
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("default")
        .to_owned()
}

/// Where backups of `state_root` are kept: nested under it, so the backup
/// destination guard keeps a backup from ever copying itself into itself.
pub fn backups_dir(state_root: &Path) -> PathBuf {
    state_root.join("backups")
}

fn workspaces_db_path() -> PathBuf {
    Paths::os_native()
        .resolve(Root::State)
        .join("workspaces.db")
}

/// Open the machine-wide workspace registry, creating its directory and an
/// empty registry when none exists yet.
pub fn open_workspace_manager() -> Result<WorkspaceManager, String> {
    let path = workspaces_db_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    WorkspaceManager::open(&path).map_err(|e| e.to_string())
}

/// The single-file marker recording which archetype the office is staffed
/// against. Absent or empty means the archetype-free default.
pub fn active_archetype_marker(state_root: &Path) -> PathBuf {
    state_root.join("archetype").join("active")
}

/// The active archetype's id, when one is set.
pub fn read_active_archetype(state_root: &Path) -> Option<String> {
    std::fs::read_to_string(active_archetype_marker(state_root))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// The file that records developer-office admission.
pub fn dev_admission_path() -> PathBuf {
    Paths::os_native()
        .resolve(Root::State)
        .join("dev_office")
        .join("admission.txt")
}

/// The developer-office admission tier resolved for `cwd`.
pub fn resolve_dev_tier(cwd: &Path) -> AdmissionTier {
    let reader = AuthLocalAdmissionReader::open(dev_admission_path());
    DevOfficeGate::resolve(&GateInputs {
        repo: repo_authenticity(cwd),
        admitted: reader.is_admitted(),
        feedback_tier_enabled: FEEDBACK_TIER_ENABLED,
    })
}

/// The tier's name as shown to a person.
pub fn dev_tier_label(tier: AdmissionTier) -> &'static str {
    match tier {
        AdmissionTier::Absent => "absent",
        AdmissionTier::Feedback => "feedback",
        AdmissionTier::Elevated => "elevated",
    }
}

/// The observed activation state as shown to a person.
pub fn activation_state_label(state: &ActivationState) -> String {
    let mode = |mode: &crate::ActivationMode| match mode {
        crate::ActivationMode::Login => "login",
        crate::ActivationMode::System => "system",
    };
    match state {
        ActivationState::Inactive => "inactive".to_string(),
        ActivationState::Active(m) => format!("active ({})", mode(m)),
        ActivationState::RequiresApproval(m) => format!("requires-approval ({})", mode(m)),
        ActivationState::Unknown { reason } => format!("unknown: {reason}"),
    }
}

/// One line per diagnostic finding, tagged with what became of it. `repaired`
/// says whether safe repairs were applied in this run.
pub fn doctor_lines(report: &doctor::Report, repaired: bool) -> Vec<String> {
    report
        .findings
        .iter()
        .map(|finding| {
            let tag = match finding.disposition {
                Disposition::SafeRepair if repaired => "repaired",
                Disposition::SafeRepair => "repairable (--fix to apply)",
                Disposition::Escalate => "escalate",
            };
            format!("[{tag}] {}: {}", finding.id, finding.description)
        })
        .collect()
}

// ─── outcome shaping ─────────────────────────────────────────────────────────

fn text(value: impl Into<String>) -> OutcomeValue {
    OutcomeValue::Text(value.into())
}

fn record<const N: usize>(fields: [(&str, OutcomeValue); N]) -> OutcomeValue {
    OutcomeValue::Record(
        fields
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect(),
    )
}

fn count(n: usize) -> OutcomeValue {
    OutcomeValue::Integer(i64::try_from(n).unwrap_or(i64::MAX))
}

fn strings(items: &[String]) -> OutcomeValue {
    OutcomeValue::List(items.iter().map(text).collect())
}

fn unavailable(reason: impl Into<String>) -> Outcome {
    Outcome::Unavailable {
        reason: reason.into(),
    }
}

fn value(value: OutcomeValue) -> Outcome {
    Outcome::Value(value)
}

/// `status`: the workspace the resolved state root holds.
pub fn status(state_root: &Path) -> Outcome {
    if !is_initialized(state_root) {
        return unavailable(NO_WORKSPACE_MESSAGE);
    }
    value(record([
        ("workspace", text(workspace_name(state_root))),
        ("phase", text("ready")),
    ]))
}

/// `doctor` without repair: the findings of one health check. A clean run is
/// a sentence, not an empty list, because "nothing found" is the answer the
/// person asked for.
pub fn doctor_check(state_root: &Path) -> Outcome {
    if !is_initialized(state_root) {
        return unavailable(NO_WORKSPACE_MESSAGE);
    }
    let report = doctor::check(&DoctorInputs::default());
    if report.findings.is_empty() {
        return value(text(DOCTOR_ALL_CLEAR));
    }
    value(OutcomeValue::List(
        doctor_lines(&report, false).into_iter().map(text).collect(),
    ))
}

/// `backup list`: the backups under `backups_dir`, each with its identity,
/// location and creation time. No backups is an empty list.
pub fn backup_list(backups_dir: &Path) -> Outcome {
    match backup::list(backups_dir) {
        Ok(backups) => value(OutcomeValue::List(
            backups
                .iter()
                .map(|b| {
                    record([
                        ("id", text(&b.id)),
                        ("path", text(paths::display_clean(&b.path))),
                        (
                            "created_at",
                            OutcomeValue::Integer(i64::try_from(b.created_at_unix).unwrap_or(0)),
                        ),
                    ])
                })
                .collect(),
        )),
        Err(err) => unavailable(format!("listing backups failed: {err}")),
    }
}

/// `workspace list`: every registered workspace, marking the active one.
pub fn workspace_list() -> Outcome {
    let manager = match open_workspace_manager() {
        Ok(manager) => manager,
        Err(reason) => return unavailable(reason),
    };
    match manager.list() {
        Ok(workspaces) => {
            let active = manager.get_active().ok().flatten();
            value(OutcomeValue::List(
                workspaces
                    .iter()
                    .map(|w| {
                        record([
                            ("id", text(w.id.to_string())),
                            ("name", text(&w.name)),
                            (
                                "active",
                                OutcomeValue::Boolean(active.as_ref() == Some(&w.id)),
                            ),
                        ])
                    })
                    .collect(),
            ))
        }
        Err(e) => unavailable(e.to_string()),
    }
}

/// `workspace check`: whether a workspace exists, is the active one, and
/// whose directory is still there.
pub fn workspace_check(id: &str) -> Outcome {
    let workspace_id = match WorkspaceId::new(id) {
        Ok(workspace_id) => workspace_id,
        Err(e) => {
            return Outcome::Rejected(Rejection {
                binder: "id",
                mode: RejectionMode::Malformed,
                detail: e.to_string(),
            });
        }
    };
    let manager = match open_workspace_manager() {
        Ok(manager) => manager,
        Err(reason) => return unavailable(reason),
    };
    match manager.check(&workspace_id) {
        Ok(status) => value(record([
            ("exists", OutcomeValue::Boolean(status.exists)),
            ("active", OutcomeValue::Boolean(status.is_active)),
            ("path_ok", OutcomeValue::Boolean(status.path_exists)),
        ])),
        Err(e) => unavailable(e.to_string()),
    }
}

/// `registry list`: the active agent definitions.
pub fn registry_list() -> Outcome {
    let registry = AgentRegistry::load();
    value(OutcomeValue::List(
        registry
            .list_active()
            .iter()
            .map(|agent| {
                record([
                    ("name", text(&agent.name)),
                    (
                        "description",
                        text(agent.description.as_deref().unwrap_or("")),
                    ),
                ])
            })
            .collect(),
    ))
}

/// `registry show`: one agent definition by name.
pub fn registry_show(name: &str) -> Outcome {
    let registry = AgentRegistry::load();
    match registry.resolve(name) {
        Ok(agent) => value(record([
            ("name", text(&agent.name)),
            (
                "description",
                text(agent.description.as_deref().unwrap_or("")),
            ),
            ("mode", text(agent.mode.as_str())),
        ])),
        Err(e) => unavailable(e.to_string()),
    }
}

/// `ext list`: the registered extensions and where each stands.
pub fn ext_list() -> Outcome {
    let registry = match ExtensionRegistry::load() {
        Ok(registry) => registry,
        Err(e) => return unavailable(e.to_string()),
    };
    value(OutcomeValue::List(
        registry
            .list()
            .iter()
            .map(|(manifest, state)| {
                record([
                    ("id", text(&manifest.id)),
                    ("name", text(&manifest.name)),
                    ("version", text(&manifest.version)),
                    ("state", text(state.as_str())),
                ])
            })
            .collect(),
    ))
}

/// `ext scan`: the security scan of one file, read and scored, nothing
/// installed.
pub fn ext_scan(path: &Path) -> Outcome {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) => {
            return Outcome::Rejected(Rejection {
                binder: "path",
                mode: RejectionMode::Unreadable,
                detail: format!("{}: {e}", path.display()),
            });
        }
    };
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("ext");
    let scan = SkillScanner::scan_content(&content, file_name);
    value(record([
        ("safe", OutcomeValue::Boolean(scan.is_safe)),
        (
            "risk_score",
            OutcomeValue::Integer(i64::from(scan.risk_score)),
        ),
        ("risk_band", text(scan.risk_band.as_str())),
        ("findings", count(scan.findings.len())),
    ]))
}

/// `activation status`: the state observed from the operating system, never a
/// remembered value.
pub fn activation_status() -> Outcome {
    let state = default_activation_registry().observe();
    value(record([("state", text(activation_state_label(&state)))]))
}

/// `archetype list`: the shipped and blocked archetypes, or — with `active` —
/// the one the office is staffed against. Asking for both at once is refused:
/// they are two different displays, and letting one silently win would
/// discard the other request.
pub fn archetype_list(state_root: &Path, catalog_only: bool, active: bool) -> Outcome {
    if catalog_only && active {
        return Outcome::Rejected(Rejection {
            binder: "active",
            mode: RejectionMode::IllShaped,
            detail: "--catalog and --active cannot both be given".to_string(),
        });
    }
    if active {
        return value(record([(
            "active",
            read_active_archetype(state_root).map_or(OutcomeValue::Empty, text),
        )]));
    }
    let catalog = ArchetypeCatalog::program();
    let shipped = catalog
        .shipped()
        .iter()
        .map(|def| {
            record([
                ("id", text(&def.id)),
                ("domain", text(&def.domain)),
                ("roles", count(def.pool.len())),
            ])
        })
        .collect();
    let blocked = catalog
        .blocked()
        .iter()
        .map(|b| {
            record([
                ("id", text(&b.id)),
                ("domain", text(&b.domain)),
                ("missing_roles", strings(&b.missing_roles)),
            ])
        })
        .collect();
    value(record([
        ("shipped", OutcomeValue::List(shipped)),
        ("blocked", OutcomeValue::List(blocked)),
    ]))
}

/// `archetype info`: one archetype's pool, shape and seed, or why it is
/// blocked. With `deviations`, whether its prior has been validated — which
/// it has not, because no office has run under it here.
pub fn archetype_info(id: &str, deviations: bool) -> Outcome {
    let catalog = ArchetypeCatalog::program();
    if let Some(def) = catalog.get(id) {
        let mut fields = vec![
            ("id".to_string(), text(&def.id)),
            ("domain".to_string(), text(&def.domain)),
            ("pool".to_string(), strings(&def.pool)),
            ("departments".to_string(), strings(&def.shape.departments)),
            ("grow_when".to_string(), text(&def.shape.grow_when)),
            ("seed".to_string(), count(def.seed.len())),
        ];
        if deviations {
            fields.push((
                "validation".to_string(),
                text(format!(
                    "{:?}",
                    crate::archetype::ValidationStatus::Unvalidated
                )),
            ));
        }
        return value(OutcomeValue::Record(fields));
    }
    if let Some(b) = catalog.blocked_status(id) {
        return value(record([
            ("id", text(&b.id)),
            ("blocked", OutcomeValue::Boolean(true)),
            ("domain", text(&b.domain)),
            ("missing_roles", strings(&b.missing_roles)),
        ]));
    }
    unavailable(format!("archetype not found: {id}"))
}

/// `dev status`: the developer-office admission tier resolved for `cwd`.
pub fn dev_status(cwd: &Path) -> Outcome {
    value(record([(
        "tier",
        text(dev_tier_label(resolve_dev_tier(cwd))),
    )]))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cronus-inspect-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("test directory is creatable");
        dir
    }

    fn field<'a>(outcome: &'a Outcome, name: &str) -> Option<&'a OutcomeValue> {
        match outcome {
            Outcome::Value(OutcomeValue::Record(fields)) => {
                fields.iter().find(|(k, _)| k == name).map(|(_, v)| v)
            }
            _ => None,
        }
    }

    #[test]
    fn a_workspace_directory_names_itself_and_a_dot_cronus_state_directory_names_its_project() {
        let project = temp_dir("name").join("my-project");
        let state = project.join(".cronus");
        assert_eq!(workspace_name(&state), "my-project");
        assert_eq!(workspace_name(&project), "my-project");
    }

    #[test]
    fn status_of_an_initialized_root_names_the_workspace_and_its_phase() {
        let root = temp_dir("status");
        fs::write(root.join("app.json"), "{}\n").expect("fixture is writable");
        let outcome = status(&root);
        assert_eq!(
            field(&outcome, "workspace"),
            Some(&OutcomeValue::Text(workspace_name(&root)))
        );
        assert_eq!(
            field(&outcome, "phase"),
            Some(&OutcomeValue::Text("ready".to_string()))
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn status_of_a_root_with_no_workspace_is_unavailable_not_empty() {
        let root = temp_dir("status-none");
        assert_eq!(
            status(&root),
            Outcome::Unavailable {
                reason: NO_WORKSPACE_MESSAGE.to_string()
            }
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_diagnostic_run_over_an_uninitialized_root_is_unavailable_and_a_clean_one_is_a_sentence() {
        let root = temp_dir("doctor");
        assert!(matches!(doctor_check(&root), Outcome::Unavailable { .. }));
        fs::write(root.join("app.json"), "{}\n").expect("fixture is writable");
        assert_eq!(
            doctor_check(&root),
            Outcome::Value(OutcomeValue::Text(DOCTOR_ALL_CLEAR.to_string()))
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn backup_list_distinguishes_no_backups_from_an_unreadable_directory() {
        let root = temp_dir("backups");
        let dir = backups_dir(&root);
        fs::create_dir_all(&dir).expect("fixture is creatable");
        assert_eq!(
            backup_list(&dir),
            Outcome::Value(OutcomeValue::List(Vec::new())),
            "a readable directory with no backups is an empty list"
        );
        let file = root.join("not-a-directory");
        fs::write(&file, "x").expect("fixture is writable");
        assert!(
            matches!(backup_list(&file), Outcome::Unavailable { .. }),
            "a path that cannot be listed is unavailable, never an empty list"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn workspace_check_rejects_a_malformed_id_before_touching_any_store() {
        match workspace_check("NOT VALID") {
            Outcome::Rejected(rejection) => {
                assert_eq!(rejection.binder, "id");
                assert_eq!(rejection.mode, RejectionMode::Malformed);
            }
            other => panic!("expected a rejection, got {other:?}"),
        }
    }

    #[test]
    fn ext_scan_of_a_missing_file_is_a_rejection_naming_the_path() {
        let missing = temp_dir("scan").join("absent.json");
        match ext_scan(&missing) {
            Outcome::Rejected(rejection) => {
                assert_eq!(rejection.binder, "path");
                assert_eq!(rejection.mode, RejectionMode::Unreadable);
                assert!(rejection.detail.contains("absent.json"));
            }
            other => panic!("expected a rejection, got {other:?}"),
        }
    }

    #[test]
    fn ext_scan_of_a_plain_manifest_reports_a_score_and_a_finding_count() {
        let dir = temp_dir("scan-ok");
        let file = dir.join("manifest.json");
        fs::write(&file, "{\"id\":\"x\",\"name\":\"X\",\"version\":\"1\"}").expect("writable");
        let outcome = ext_scan(&file);
        assert_eq!(
            field(&outcome, "safe"),
            Some(&OutcomeValue::Boolean(true)),
            "{outcome:?}"
        );
        assert!(matches!(
            field(&outcome, "risk_score"),
            Some(OutcomeValue::Integer(_))
        ));
        assert!(matches!(
            field(&outcome, "findings"),
            Some(OutcomeValue::Integer(0))
        ));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn archetype_list_refuses_the_two_display_modes_together() {
        let root = temp_dir("archetype");
        match archetype_list(&root, true, true) {
            Outcome::Rejected(rejection) => assert_eq!(rejection.mode, RejectionMode::IllShaped),
            other => panic!("expected a rejection, got {other:?}"),
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn archetype_list_active_is_empty_when_none_is_set_and_names_it_when_one_is() {
        let root = temp_dir("archetype-active");
        assert_eq!(
            field(&archetype_list(&root, false, true), "active"),
            Some(&OutcomeValue::Empty)
        );
        fs::create_dir_all(root.join("archetype")).expect("creatable");
        fs::write(active_archetype_marker(&root), "software\n").expect("writable");
        assert_eq!(
            field(&archetype_list(&root, false, true), "active"),
            Some(&OutcomeValue::Text("software".to_string()))
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn archetype_list_lists_what_ships_and_what_is_blocked() {
        let root = temp_dir("archetype-list");
        let outcome = archetype_list(&root, false, false);
        assert!(matches!(
            field(&outcome, "shipped"),
            Some(OutcomeValue::List(_))
        ));
        assert!(matches!(
            field(&outcome, "blocked"),
            Some(OutcomeValue::List(_))
        ));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn archetype_info_of_an_unknown_id_is_unavailable_naming_it() {
        match archetype_info("no-such-archetype", false) {
            Outcome::Unavailable { reason } => assert!(reason.contains("no-such-archetype")),
            other => panic!("expected unavailable, got {other:?}"),
        }
    }

    #[test]
    fn dev_status_names_a_tier() {
        let outcome = dev_status(&temp_dir("dev"));
        assert!(matches!(
            field(&outcome, "tier"),
            Some(OutcomeValue::Text(tier)) if ["absent", "feedback", "elevated"].contains(&tier.as_str())
        ));
    }

    #[test]
    fn activation_status_reports_an_observed_state_label() {
        let outcome = activation_status();
        assert!(matches!(
            field(&outcome, "state"),
            Some(OutcomeValue::Text(label)) if !label.is_empty()
        ));
    }
}
