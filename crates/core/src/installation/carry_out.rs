//! The installation verbs a composed surface can carry out itself.
//!
//! A verb that changes something is not carried out just because a surface
//! dispatched it: the surface must first have asked the person, naming the
//! exact resolved invocation, and said so by running only after the answer. A
//! verb for which [`is_carried_out`] is true has a handler here that does the
//! work; a surface may therefore ask and then dispatch it. Every other verb
//! above `Inspect` still answers with the command line that carries it out.
//!
//! Two kinds of fact belong to a verb and not to the surface that asks:
//! what the person must be shown before agreeing ([`disclosure`]) and the
//! acknowledgement the verb's non-interactive form requires
//! ([`acknowledgement`]). A surface collects consent its own way — a prompt
//! on a terminal, a confirmation block in a terminal UI — and then supplies
//! the acknowledgement, so the words shown and the grant given are the
//! core's, once.
//!
//! Like [`super::inspect`], these functions return an `Outcome` rather than
//! printed text, so each frontend renders the one answer its own way.

use std::path::Path;

pub mod catalogs;
pub mod recompose;

use cronus_contract::{
    ActivationMode, ActivationRegistry, ActivationState, ArgValue, ArgValues, InvocableId,
    LiveEffect, Outcome, OutcomeValue, Rejection, RejectionMode,
};
use cronus_domain::backup::{self, BackupOptions};

use crate::activation::{self, TransitionError, TransitionOutcome};
use crate::paths::display_clean;
use crate::workspace::{WorkspaceId, WorkspaceManager, WorkspaceTemplate};

/// The acknowledgement flag of the verb that grants background autonomy.
pub const UNATTENDED_FLAG: &str = "acknowledge-unattended-execution";

/// Whether this composition carries `id` out itself, with these arguments,
/// once a surface has asked. The arguments matter: enabling background
/// activation for the whole system may need an elevation prompt, which a
/// composed surface never attempts, so only the login mode is carried out
/// here and the system mode stays with the command line.
pub fn is_carried_out(id: &InvocableId, args: &ArgValues) -> bool {
    match id.tail() {
        "init" | "backup.create" | "workspace.create" | "workspace.switch" | "workspace.delete"
        | "registry.enable" | "registry.disable" | "archetype.set" | "ext.activate"
        | "ext.deactivate" | "restore" | "dev.admit" | "dev.revoke" | "activation.disable"
        | "archetype.create" | "registry.create" | "ext.add" | "ext.skill.import"
        | "ext.skill.create" => true,
        "activation.enable" => {
            matches!(args.get("mode"), Some(ArgValue::Text(mode)) if mode == "login")
        }
        // Read-only without its repair flag; with it, a change.
        "doctor" => matches!(args.get("fix"), Some(ArgValue::Flag)),
        _ => false,
    }
}

/// [`is_carried_out`] for an invocation whose class has been resolved.
/// Deleting the workspace the office runs around is a recompose that nothing
/// carries out — the installation would be left naming a floor that is gone —
/// so a class raised above `Install` for that verb is not carried out.
pub fn is_carried_out_as(id: &InvocableId, args: &ArgValues, effect: LiveEffect) -> bool {
    is_carried_out(id, args) && !(id.tail() == "workspace.delete" && effect != LiveEffect::Install)
}

/// Whether the answer to a change applied by relaunch asks for the composition
/// to be rebuilt: the change was made, and it changed something. A change that
/// was not applied changed nothing, and a repair that found nothing to repair
/// is the one applied change that also changed nothing.
pub fn needs_relaunch(id: &InvocableId, outcome: &Outcome) -> bool {
    match outcome {
        Outcome::Value(OutcomeValue::Text(said)) if id.tail() == "doctor" => {
            said != super::inspect::DOCTOR_ALL_CLEAR
        }
        Outcome::Value(_) => true,
        _ => false,
    }
}

/// Whether carrying `id` out right now would need an elevation prompt, which
/// a composed surface never attempts. Removing a registration that was made
/// for the whole system is such a case, and it depends on what is registered
/// now, so the operating system is asked.
pub fn needs_elevation_now(id: &InvocableId) -> bool {
    match id.tail() {
        "activation.disable" => matches!(
            crate::activation_bootstrap::default_activation_registry().observe(),
            ActivationState::Active(ActivationMode::System)
                | ActivationState::RequiresApproval(ActivationMode::System)
        ),
        _ => false,
    }
}

/// Why `id` cannot be carried out by anyone in this build, if that is so —
/// asked before the person is, so nobody is invited to agree to something that
/// can only be refused. Importing a skill needs a witness that verifies, and a
/// build with no verifier can verify nothing.
pub fn blocked_reason(id: &InvocableId) -> Option<&'static str> {
    match id.tail() {
        "ext.remove" => Some("removing an extension is not supported yet, by any surface"),
        "ext.skill.import" if !catalogs::default_witness_verifier().can_verify() => Some(
            "an imported skill must present a signed witness that verifies, and this build has \
             no attestation verifier, so every import is refused",
        ),
        _ => None,
    }
}

/// The flag by which `id`'s non-interactive form is told the person has
/// agreed, if it has one. A surface that asked adds it to the invocation it
/// runs; one that did not ask must not.
pub fn acknowledgement(id: &InvocableId) -> Option<&'static str> {
    match id.tail() {
        "activation.enable" => Some(UNATTENDED_FLAG),
        "ext.activate" => Some("yes"),
        _ => None,
    }
}

/// What the person must be shown before agreeing to `id` with these
/// arguments: the consequence in plain words. Empty for a verb whose
/// consequence is evident from the command alone.
pub fn disclosure(id: &InvocableId, args: &ArgValues) -> Vec<String> {
    match id.tail() {
        "activation.enable" => {
            let mode = match args.get("mode") {
                Some(ArgValue::Text(mode)) => mode.as_str(),
                _ => "",
            };
            vec![
                format!(
                    "Enabling {mode} background activation lets Cronus run while you are not present."
                ),
                "It will call models, spend allowance, and act on your behalf under the \
                 currently configured autonomy level and spend ceiling."
                    .to_string(),
            ]
        }
        "ext.activate" => {
            let id = match args.get("id") {
                Some(ArgValue::Text(id)) => id.as_str(),
                _ => "",
            };
            // What it grants is read from the extension's own manifest.
            match crate::extensions::ExtensionRegistry::load() {
                Ok(registry) => registry
                    .manifest(id)
                    .map(recompose::grant_lines)
                    .unwrap_or_default(),
                Err(_) => Vec::new(),
            }
        }
        "restore" => vec![
            "Restoring writes the backup's files over the state root's; files the backup \
             does not contain are left where they are."
                .to_string(),
        ],
        "archetype.set" => vec![
            "Expectations are re-scoped to the archetype; the staff who are hired stay."
                .to_string(),
        ],
        "dev.admit" => vec![
            "Admission lets the self-hosting developer office run in this installation, on \
             the canonical repository only."
                .to_string(),
        ],
        "dev.revoke" => vec!["Revoking withdraws that admission.".to_string()],
        "doctor" => vec![
            "Safe repairs are applied; whatever needs a person is reported and left.".to_string(),
        ],
        "workspace.delete" => vec![
            "Deleting a workspace removes it from the registry; the files at its path are \
             left where they are."
                .to_string(),
        ],
        "activation.disable" => vec![
            "Disabling background activation stops Cronus from running while you are not \
             present."
                .to_string(),
        ],
        "ext.add" => vec![
            "The extension is registered as discovered only; nothing is granted to it until \
             it is activated."
                .to_string(),
        ],
        _ => Vec::new(),
    }
}

/// Take a backup of `state_root` into a fresh directory under `backups_dir`,
/// or into `to` when one is named. Answers the new backup's id and where it
/// is; a failure is `Unavailable` naming it, never a success-shaped empty
/// answer, and leaves no half-written backup behind.
pub fn backup_create(
    state_root: &Path,
    backups_dir: &Path,
    to: Option<&Path>,
    include_logs: bool,
) -> Outcome {
    match backup::create(state_root, backups_dir, to, BackupOptions { include_logs }) {
        Ok(made) => Outcome::Value(OutcomeValue::Record(vec![
            ("id".to_string(), OutcomeValue::Text(made.id)),
            (
                "path".to_string(),
                OutcomeValue::Text(display_clean(&made.path)),
            ),
        ])),
        Err(err) => Outcome::Unavailable {
            reason: format!("backup failed: {err}"),
        },
    }
}

fn malformed(binder: &'static str, detail: String) -> Outcome {
    Outcome::Rejected(Rejection {
        binder,
        mode: RejectionMode::Malformed,
        detail,
    })
}

/// Create a workspace named `id` in `manager`. The name defaults to the id and
/// the path to the current directory. An id that is reserved for the system or
/// is not a well-formed id is rejected naming it; a registry that cannot take
/// the workspace is `Unavailable` naming why.
pub fn workspace_create(
    manager: &WorkspaceManager,
    id: &str,
    name: Option<&str>,
    path: Option<&Path>,
) -> Outcome {
    if crate::dev_office_workspace::is_reserved_dev_workspace_id(id) {
        return malformed(
            "id",
            format!("'{id}' is a reserved system workspace id and cannot be created here"),
        );
    }
    let workspace_id = match WorkspaceId::new(id) {
        Ok(workspace_id) => workspace_id,
        Err(err) => return malformed("id", err.to_string()),
    };
    let name = name.unwrap_or(id);
    let path = path
        .map(Path::to_path_buf)
        .unwrap_or_else(crate::paths::caller_dir);
    match manager.create(&workspace_id, name, &path, WorkspaceTemplate::Default) {
        Ok(made) => Outcome::Value(OutcomeValue::Record(vec![
            ("id".to_string(), OutcomeValue::Text(made.id.to_string())),
            ("name".to_string(), OutcomeValue::Text(made.name)),
        ])),
        Err(err) => Outcome::Unavailable {
            reason: err.to_string(),
        },
    }
}

/// Initialize a workspace in `target` (a project's `.cronus` directory).
/// Idempotent: initializing one that is already initialized changes nothing and
/// says so.
pub fn init(target: &Path) -> Outcome {
    let already = target.join("app.json").exists();
    // A project-local root sits next to the project's own files, so it also
    // gets the version-control exclusion that keeps office state out of a
    // commit.
    match crate::state::bootstrap_project_local(target) {
        Ok(()) => {
            let (result, shown) = if already {
                ("already-initialized", target.display().to_string())
            } else {
                let absolute = target
                    .canonicalize()
                    .unwrap_or_else(|_| target.to_path_buf());
                ("initialized", display_clean(&absolute))
            };
            Outcome::Value(OutcomeValue::Record(vec![
                ("result".to_string(), OutcomeValue::Text(result.to_string())),
                ("path".to_string(), OutcomeValue::Text(shown)),
            ]))
        }
        Err(err) => Outcome::Unavailable {
            reason: catalogs::describe_io_error(&err, target),
        },
    }
}

fn workspace_id_or_rejection(id: &str) -> Result<WorkspaceId, Outcome> {
    WorkspaceId::new(id).map_err(|err| malformed("id", err.to_string()))
}

/// Make `id` the active workspace in `manager`. It must exist.
pub fn workspace_switch(manager: &WorkspaceManager, id: &str) -> Outcome {
    let workspace_id = match workspace_id_or_rejection(id) {
        Ok(workspace_id) => workspace_id,
        Err(rejection) => return rejection,
    };
    match manager.set_active(&workspace_id) {
        Ok(()) => Outcome::Value(OutcomeValue::Record(vec![(
            "id".to_string(),
            OutcomeValue::Text(workspace_id.to_string()),
        )])),
        Err(err) => Outcome::Unavailable {
            reason: err.to_string(),
        },
    }
}

/// Delete workspace `id` from `manager`'s registry. The reserved system
/// workspace cannot be deleted here, and one that does not exist is
/// `Unavailable` naming it, never a silent success.
pub fn workspace_delete(manager: &WorkspaceManager, id: &str) -> Outcome {
    if crate::dev_office_workspace::is_reserved_dev_workspace_id(id) {
        return malformed(
            "id",
            format!("'{id}' is a reserved system workspace and cannot be deleted here"),
        );
    }
    let workspace_id = match workspace_id_or_rejection(id) {
        Ok(workspace_id) => workspace_id,
        Err(rejection) => return rejection,
    };
    match manager.delete(&workspace_id) {
        Ok(true) => Outcome::Value(OutcomeValue::Record(vec![(
            "id".to_string(),
            OutcomeValue::Text(workspace_id.to_string()),
        )])),
        Ok(false) => Outcome::Unavailable {
            reason: format!("workspace not found: {workspace_id}"),
        },
        Err(err) => Outcome::Unavailable {
            reason: err.to_string(),
        },
    }
}

/// Remove whatever background-activation registration is current, and verify
/// it is gone — never left partially removed.
pub fn activation_disable(registry: &dyn ActivationRegistry) -> Outcome {
    match activation::disable(registry) {
        Ok(_) => Outcome::Value(OutcomeValue::Record(vec![(
            "outcome".to_string(),
            OutcomeValue::Text("deactivated".to_string()),
        )])),
        Err(err) => Outcome::Unavailable {
            reason: format!("activation disable failed: {}", transition_error_text(&err)),
        },
    }
}

/// The mode a word names.
pub fn parse_mode(word: &str) -> Option<ActivationMode> {
    match word {
        "login" => Some(ActivationMode::Login),
        "system" => Some(ActivationMode::System),
        _ => None,
    }
}

/// The word for a mode.
pub fn mode_word(mode: ActivationMode) -> &'static str {
    match mode {
        ActivationMode::Login => "login",
        ActivationMode::System => "system",
    }
}

/// Why a transition did not converge, in a sentence.
pub fn transition_error_text(err: &TransitionError) -> String {
    match err {
        TransitionError::PriorModeNotRemoved { observed } => {
            format!("the prior mode could not be verified removed (observed {observed:?})")
        }
        TransitionError::RegistrationFailed { detail } => detail.clone(),
        TransitionError::VerificationFailed { observed } => {
            format!("registration did not verify (observed {observed:?})")
        }
        TransitionError::ConsentMissing { mode } => {
            format!("no consent recorded for {} mode", mode_word(*mode))
        }
    }
}

/// Register background activation for `mode_word` on `registry`.
///
/// Background activation is an autonomy grant, so it runs only when the
/// caller says the person has agreed (`acknowledged`): without that it is
/// rejected naming the missing acknowledgement, and nothing is registered.
/// `requires-approval` is its own answer — registered, but the operating
/// system will not run it until the person approves — never folded into
/// success.
pub fn activation_enable(
    registry: &dyn ActivationRegistry,
    mode_word_given: &str,
    acknowledged: bool,
) -> Outcome {
    let Some(mode) = parse_mode(mode_word_given) else {
        return malformed(
            "mode",
            format!("'{mode_word_given}' is not an activation mode (login or system)"),
        );
    };
    if !acknowledged {
        return Outcome::Rejected(Rejection {
            binder: UNATTENDED_FLAG,
            mode: RejectionMode::Absent,
            detail: "background activation is an autonomy grant and refuses to run without \
                     the person's acknowledgement"
                .to_string(),
        });
    }
    let record = |outcome: &str, note: Option<String>| {
        let mut fields = vec![
            (
                "outcome".to_string(),
                OutcomeValue::Text(outcome.to_string()),
            ),
            (
                "mode".to_string(),
                OutcomeValue::Text(mode_word(mode).to_string()),
            ),
        ];
        if let Some(note) = note {
            fields.push(("note".to_string(), OutcomeValue::Text(note)));
        }
        Outcome::Value(OutcomeValue::Record(fields))
    };
    match activation::enable(registry, mode) {
        Ok(TransitionOutcome::Activated(_)) => record("activated", None),
        Ok(TransitionOutcome::RequiresApproval(_)) => record(
            "requires-approval",
            Some(format!(
                "registered ({}), but the OS requires your approval before it will run — see \
                 your system's background-items settings",
                mode_word(mode)
            )),
        ),
        Ok(TransitionOutcome::Deactivated) => record("deactivated", None),
        Err(err) => Outcome::Unavailable {
            reason: format!("activation enable failed: {}", transition_error_text(&err)),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Mutex;

    use cronus_contract::{ActivationCapabilities, ActivationState, ModeSupport};

    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cronus-carry-out-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("creatable");
        dir
    }

    fn field<'a>(outcome: &'a Outcome, name: &str) -> &'a str {
        let Outcome::Value(OutcomeValue::Record(fields)) = outcome else {
            panic!("expected a record, got {outcome:?}");
        };
        match fields.iter().find(|(k, _)| k == name) {
            Some((_, OutcomeValue::Text(text))) => text,
            other => panic!("no text field {name}: {other:?}"),
        }
    }

    fn id(tail: &str) -> InvocableId {
        InvocableId::new(format!("core:{tail}")).expect("well-formed")
    }

    fn mode_args(mode: &str) -> ArgValues {
        let mut args = ArgValues::new();
        args.insert("mode", ArgValue::Text(mode.to_string()));
        args
    }

    #[test]
    fn the_backup_and_the_workspace_are_carried_out_and_the_rest_are_not() {
        let none = ArgValues::new();
        assert!(is_carried_out(&id("backup.create"), &none));
        assert!(is_carried_out(&id("workspace.create"), &none));
        assert!(!is_carried_out(&id("ext.remove"), &none));
        assert!(
            !is_carried_out(&id("doctor"), &none),
            "diagnostics without the repair flag only read"
        );
    }

    #[test]
    fn every_install_verb_but_the_system_mode_of_activation_is_carried_out() {
        let none = ArgValues::new();
        for tail in [
            "init",
            "workspace.create",
            "workspace.switch",
            "workspace.delete",
            "backup.create",
            "activation.disable",
            "archetype.create",
            "registry.create",
            "ext.add",
            "ext.skill.import",
            "ext.skill.create",
        ] {
            assert!(is_carried_out(&id(tail), &none), "{tail}");
        }
    }

    #[test]
    fn only_the_skill_import_is_blocked_and_it_says_why() {
        let reason =
            blocked_reason(&id("ext.skill.import")).expect("blocked while nothing can verify");
        assert!(reason.contains("no attestation verifier"), "{reason}");
        for tail in ["ext.add", "ext.skill.create", "backup.create", "init"] {
            assert_eq!(blocked_reason(&id(tail)), None, "{tail}");
        }
    }

    #[test]
    fn the_recomposing_verbs_are_carried_out_and_the_repair_flag_is_what_makes_a_diagnosis_one() {
        let none = ArgValues::new();
        for tail in [
            "registry.enable",
            "registry.disable",
            "archetype.set",
            "ext.activate",
            "ext.deactivate",
            "restore",
            "dev.admit",
            "dev.revoke",
        ] {
            assert!(is_carried_out(&id(tail), &none), "{tail}");
        }
        let mut fix = ArgValues::new();
        fix.insert("fix", ArgValue::Flag);
        assert!(is_carried_out(&id("doctor"), &fix));
    }

    #[test]
    fn only_an_applied_change_that_changed_something_asks_for_a_relaunch() {
        let applied = Outcome::Value(OutcomeValue::Text("done".to_string()));
        assert!(needs_relaunch(&id("restore"), &applied));
        assert!(!needs_relaunch(
            &id("restore"),
            &Outcome::Unavailable {
                reason: "not applied — work is running".to_string()
            }
        ));
        let clear = Outcome::Value(OutcomeValue::Text(
            crate::installation::inspect::DOCTOR_ALL_CLEAR.to_string(),
        ));
        assert!(
            !needs_relaunch(&id("doctor"), &clear),
            "a repair with nothing to repair changed nothing"
        );
        let repaired = Outcome::Value(OutcomeValue::List(vec![OutcomeValue::Text(
            "repaired: a stale lock".to_string(),
        )]));
        assert!(needs_relaunch(&id("doctor"), &repaired));
    }

    #[test]
    fn deleting_the_running_workspace_is_not_carried_out_but_any_other_is() {
        let mut args = ArgValues::new();
        args.insert("id", ArgValue::Text("acme".to_string()));
        let delete = id("workspace.delete");
        assert!(is_carried_out_as(&delete, &args, LiveEffect::Install));
        assert!(!is_carried_out_as(
            &delete,
            &args,
            LiveEffect::Recompose {
                apply: cronus_contract::Apply::Relaunch
            }
        ));
        // Other recomposing verbs are unaffected by the class they resolved to.
        assert!(is_carried_out_as(
            &id("restore"),
            &ArgValues::new(),
            LiveEffect::Recompose {
                apply: cronus_contract::Apply::Relaunch
            }
        ));
    }

    #[test]
    fn removing_an_extension_is_blocked_for_everyone_and_says_so() {
        let reason = blocked_reason(&id("ext.remove")).expect("not supported anywhere");
        assert!(reason.contains("not supported yet"), "{reason}");
    }

    #[test]
    fn activating_an_extension_has_an_acknowledgement_and_restoring_says_what_it_overwrites() {
        assert_eq!(acknowledgement(&id("ext.activate")), Some("yes"));
        let shown = disclosure(&id("restore"), &ArgValues::new());
        assert!(shown[0].contains("over the state root"), "{shown:?}");
    }

    #[test]
    fn deleting_a_workspace_says_it_leaves_the_files() {
        let shown = disclosure(&id("workspace.delete"), &ArgValues::new());
        assert!(shown[0].contains("left where they are"), "{shown:?}");
    }

    #[test]
    fn background_activation_is_carried_out_for_the_login_mode_and_never_for_the_system_one() {
        let enable = id("activation.enable");
        assert!(is_carried_out(&enable, &mode_args("login")));
        assert!(
            !is_carried_out(&enable, &mode_args("system")),
            "the system mode may need an elevation prompt, which this surface never attempts"
        );
        assert!(!is_carried_out(&enable, &ArgValues::new()));
    }

    #[test]
    fn only_the_autonomy_grant_has_an_acknowledgement_and_a_disclosure() {
        let enable = id("activation.enable");
        assert_eq!(acknowledgement(&enable), Some(UNATTENDED_FLAG));
        assert_eq!(acknowledgement(&id("backup.create")), None);

        let shown = disclosure(&enable, &mode_args("login"));
        assert_eq!(shown.len(), 2);
        assert!(
            shown[0].contains("login background activation"),
            "{shown:?}"
        );
        assert!(shown[1].contains("spend allowance"), "{shown:?}");
        assert!(disclosure(&id("backup.create"), &ArgValues::new()).is_empty());
    }

    #[test]
    fn a_backup_answers_its_id_and_where_it_is_and_the_files_are_there() {
        let root = temp("made");
        let state = root.join("state");
        std::fs::create_dir_all(&state).expect("creatable");
        std::fs::write(state.join("app.json"), "{}").expect("writable");
        let dest = root.join("elsewhere");

        let outcome = backup_create(&state, &state.join("backups"), Some(&dest), false);

        assert!(field(&outcome, "id").starts_with("backup-"));
        assert_eq!(field(&outcome, "path"), display_clean(&dest));
        assert!(dest.join("app.json").is_file(), "the state was copied");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_backup_that_cannot_be_made_says_so_and_leaves_nothing() {
        let root = temp("failed");
        let state = root.join("state");
        std::fs::create_dir_all(&state).expect("creatable");
        // The destination's parent is a file, so no directory can be made.
        let blocker = root.join("blocker");
        std::fs::write(&blocker, "x").expect("writable");
        let dest = blocker.join("inside");

        let outcome = backup_create(&state, &state.join("backups"), Some(&dest), false);

        match outcome {
            Outcome::Unavailable { reason } => assert!(reason.starts_with("backup failed:")),
            other => panic!("expected the failure to be named, got {other:?}"),
        }
        assert!(!dest.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    fn manager(root: &Path) -> WorkspaceManager {
        WorkspaceManager::open(root.join("workspaces.db")).expect("opens")
    }

    #[test]
    fn a_workspace_is_created_under_the_id_and_name_it_was_given() {
        let root = temp("ws-made");
        let manager = manager(&root);
        let place = root.join("project");
        std::fs::create_dir_all(&place).expect("creatable");

        let outcome = workspace_create(&manager, "acme", Some("Acme Corp"), Some(&place));

        assert_eq!(field(&outcome, "id"), "acme");
        assert_eq!(field(&outcome, "name"), "Acme Corp");
        let listed = manager.list().expect("lists");
        assert_eq!(listed.len(), 1, "{listed:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_workspace_name_defaults_to_its_id() {
        let root = temp("ws-default");
        let outcome = workspace_create(&manager(&root), "solo", None, Some(&root));
        assert_eq!(field(&outcome, "name"), "solo");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_reserved_or_malformed_workspace_id_is_rejected_naming_the_id_binder() {
        let root = temp("ws-bad");
        let manager = manager(&root);
        for bad in ["dev-office", "Not Valid!"] {
            match workspace_create(&manager, bad, None, Some(&root)) {
                Outcome::Rejected(rejection) => {
                    assert_eq!(rejection.binder, "id", "{bad}");
                    assert_eq!(rejection.mode, RejectionMode::Malformed, "{bad}");
                }
                other => panic!("{bad}: expected a rejection, got {other:?}"),
            }
        }
        assert!(manager.list().expect("lists").is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_second_workspace_under_the_same_id_is_unavailable_not_a_silent_success() {
        let root = temp("ws-twice");
        let manager = manager(&root);
        let _ = workspace_create(&manager, "acme", None, Some(&root));
        match workspace_create(&manager, "acme", None, Some(&root)) {
            Outcome::Unavailable { reason } => assert!(reason.contains("acme"), "{reason}"),
            other => panic!("expected the clash to be named, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    fn manager_with(root: &Path, ids: &[&str]) -> WorkspaceManager {
        let manager = manager(root);
        for id in ids {
            assert!(matches!(
                workspace_create(&manager, id, None, Some(root)),
                Outcome::Value(_)
            ));
        }
        manager
    }

    #[test]
    fn switching_makes_the_workspace_active_and_an_unknown_one_is_unavailable() {
        let root = temp("ws-switch");
        let manager = manager_with(&root, &["alpha", "bravo"]);

        let outcome = workspace_switch(&manager, "bravo");

        assert_eq!(field(&outcome, "id"), "bravo");
        assert_eq!(
            manager
                .get_active()
                .expect("reads")
                .map(|id| id.to_string()),
            Some("bravo".to_string())
        );
        match workspace_switch(&manager, "nope") {
            Outcome::Unavailable { reason } => assert!(reason.contains("nope"), "{reason}"),
            other => panic!("expected the workspace to be named, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn deleting_removes_it_from_the_registry_and_leaves_its_files() {
        let root = temp("ws-delete");
        let manager = manager_with(&root, &["alpha", "bravo"]);
        std::fs::write(root.join("keep.txt"), "mine").expect("writable");

        let outcome = workspace_delete(&manager, "alpha");

        assert_eq!(field(&outcome, "id"), "alpha");
        let left: Vec<_> = manager
            .list()
            .expect("lists")
            .into_iter()
            .map(|w| w.id.to_string())
            .collect();
        assert_eq!(left, vec!["bravo".to_string()]);
        assert!(root.join("keep.txt").is_file(), "the files are left");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn deleting_one_that_is_not_there_or_is_reserved_is_told_not_swallowed() {
        let root = temp("ws-delete-bad");
        let manager = manager_with(&root, &["alpha"]);
        match workspace_delete(&manager, "ghost") {
            Outcome::Unavailable { reason } => {
                assert_eq!(reason, "workspace not found: ghost");
            }
            other => panic!("expected the miss to be named, got {other:?}"),
        }
        match workspace_delete(&manager, "dev-office") {
            Outcome::Rejected(rejection) => assert_eq!(rejection.binder, "id"),
            other => panic!("expected the reserved id to be rejected, got {other:?}"),
        }
        assert_eq!(manager.list().expect("lists").len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn initializing_scaffolds_once_and_a_second_time_changes_nothing() {
        let root = temp("init");
        let target = root.join(".cronus");

        let first = init(&target);
        assert_eq!(field(&first, "result"), "initialized");
        assert!(target.join("app.json").is_file());

        let second = init(&target);
        assert_eq!(field(&second, "result"), "already-initialized");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn initializing_somewhere_that_cannot_be_made_is_unavailable_in_plain_words() {
        let root = temp("init-bad");
        let blocker = root.join("blocker");
        std::fs::write(&blocker, "x").expect("writable");
        match init(&blocker.join(".cronus")) {
            Outcome::Unavailable { reason } => {
                assert!(reason.contains("blocker"), "{reason}");
            }
            other => panic!("expected the path to be named, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn deactivating_removes_the_registration_and_verifies_it() {
        let registry = Fake::landing_as(ActivationState::Active(ActivationMode::Login));
        let _ = activation_enable(&registry, "login", true);
        assert!(matches!(registry.observe(), ActivationState::Active(_)));

        let outcome = activation_disable(&registry);

        assert_eq!(field(&outcome, "outcome"), "deactivated");
        assert_eq!(registry.observe(), ActivationState::Inactive);
    }

    /// A registry that remembers what it was told, so a test never touches the
    /// operating system's real background registrations.
    struct Fake {
        state: Mutex<ActivationState>,
        lands_as: ActivationState,
        refuses: Option<String>,
        calls: Mutex<Vec<ActivationMode>>,
    }

    impl Fake {
        fn landing_as(lands_as: ActivationState) -> Self {
            Self {
                state: Mutex::new(ActivationState::Inactive),
                lands_as,
                refuses: None,
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl ActivationRegistry for Fake {
        fn capabilities(&self) -> ActivationCapabilities {
            ActivationCapabilities {
                login: ModeSupport::Supported,
                system: ModeSupport::Supported,
            }
        }
        fn observe(&self) -> ActivationState {
            self.state.lock().expect("lock").clone()
        }
        fn enable(&self, mode: ActivationMode) -> Result<(), String> {
            self.calls.lock().expect("lock").push(mode);
            if let Some(reason) = &self.refuses {
                return Err(reason.clone());
            }
            *self.state.lock().expect("lock") = self.lands_as.clone();
            Ok(())
        }
        fn disable(&self) -> Result<(), String> {
            *self.state.lock().expect("lock") = ActivationState::Inactive;
            Ok(())
        }
    }

    #[test]
    fn activation_without_the_acknowledgement_is_rejected_and_registers_nothing() {
        let registry = Fake::landing_as(ActivationState::Active(ActivationMode::Login));
        match activation_enable(&registry, "login", false) {
            Outcome::Rejected(rejection) => {
                assert_eq!(rejection.binder, UNATTENDED_FLAG);
                assert_eq!(rejection.mode, RejectionMode::Absent);
            }
            other => panic!("expected the missing acknowledgement, got {other:?}"),
        }
        assert!(registry.calls.lock().expect("lock").is_empty());
    }

    #[test]
    fn an_unknown_activation_mode_is_rejected_naming_the_mode_binder() {
        let registry = Fake::landing_as(ActivationState::Inactive);
        match activation_enable(&registry, "weekly", true) {
            Outcome::Rejected(rejection) => assert_eq!(rejection.binder, "mode"),
            other => panic!("expected a rejection, got {other:?}"),
        }
        assert!(registry.calls.lock().expect("lock").is_empty());
    }

    #[test]
    fn an_acknowledged_activation_registers_the_mode_and_says_it_is_active() {
        let registry = Fake::landing_as(ActivationState::Active(ActivationMode::Login));
        let outcome = activation_enable(&registry, "login", true);
        assert_eq!(field(&outcome, "outcome"), "activated");
        assert_eq!(field(&outcome, "mode"), "login");
        assert_eq!(
            *registry.calls.lock().expect("lock"),
            vec![ActivationMode::Login]
        );
    }

    #[test]
    fn registered_but_awaiting_the_operating_system_is_not_reported_as_active() {
        let registry = Fake::landing_as(ActivationState::RequiresApproval(ActivationMode::Login));
        let outcome = activation_enable(&registry, "login", true);
        assert_eq!(field(&outcome, "outcome"), "requires-approval");
        assert!(field(&outcome, "note").contains("requires your approval"));
    }

    #[test]
    fn a_registration_the_system_refuses_is_unavailable_naming_why() {
        let mut registry = Fake::landing_as(ActivationState::Inactive);
        registry.refuses = Some("elevation refused".to_string());
        match activation_enable(&registry, "login", true) {
            Outcome::Unavailable { reason } => {
                assert_eq!(reason, "activation enable failed: elevation refused");
            }
            other => panic!("expected the refusal to be named, got {other:?}"),
        }
    }
}
