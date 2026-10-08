//! The installation half of the command surface, declared once: the verbs
//! that configure or inspect the product itself rather than act on the
//! user's work — workspace initialization, diagnostics, backup and restore,
//! extension and agent-definition management, activation.
//!
//! This is static data — identities, summaries, binders, and the effect each
//! verb has on a composition running beside it — so reading it composes
//! nothing, and a launcher can build its parser from it before any registry
//! exists. It lives here, below every frontend, because more than one
//! frontend renders these verbs: a second frontend that had to declare them
//! again, or reach into the first, would be holding two statements of one
//! fact. A frontend owns only how it parses, renders and asks for consent;
//! it owns no name in this list.
//!
//! Every verb carries a [`LiveEffect`] in its locus, so no verb can be
//! declared without saying what it does to a running composition. The effect
//! of one particular invocation can be higher than the verb's base (a repair
//! flag, a target that is the running workspace) and never lower:
//! [`resolved_effect`] decides it.
//!
//! The declaration has three consumers besides the launcher's parser. The
//! composition registers it ([`register`]), each verb with a handler: a
//! read-only verb is carried out through [`inspect`], which also holds what
//! the command line reads for the same verbs, and every other verb answers
//! with the shell invocation that carries it out ([`command_line_invocation`])
//! until a surface can collect consent and tell whether work is in flight.

use cronus_contract::{
    Apply, ArgValue, ArgValues, Binder, BinderKind, Invocable, InvocableId, LiveEffect, Locus,
    Stability,
};

pub mod inspect;
mod invocation;
mod registration;

pub use invocation::command_line_invocation;
pub use registration::register;

/// Reads installation state and changes nothing.
const fn inspect() -> Locus {
    Locus::Installation {
        effect: LiveEffect::Inspect,
    }
}

/// Changes installation state the running composition does not hold open.
const fn install() -> Locus {
    Locus::Installation {
        effect: LiveEffect::Install,
    }
}

/// Changes what the running composition is built from.
const fn recompose(apply: Apply) -> Locus {
    Locus::Installation {
        effect: LiveEffect::Recompose { apply },
    }
}

fn id(tail: &str) -> InvocableId {
    InvocableId::new(format!("core:{tail}"))
        .expect("literal installation-verb identity must be well-formed — a bug if it isn't")
}

fn text(name: &'static str, optional: bool) -> Binder {
    Binder {
        name,
        kind: BinderKind::Text,
        optional,
    }
}

fn flag(name: &'static str) -> Binder {
    Binder {
        name,
        kind: BinderKind::Flag,
        optional: true,
    }
}

/// A value bound as a named `--name <value>` flag rather than positionally.
/// A missing optional one reads back as absent from `ArgValues`, and the
/// handler applies its own default the same way an absent positional `Text`
/// binder's caller already would — `Binder` carries no default-value slot
/// of its own, matching the minimalism the rest of this mechanism already
/// holds to.
fn named_text(name: &'static str, optional: bool) -> Binder {
    Binder {
        name,
        kind: BinderKind::NamedText,
        optional,
    }
}

/// The single declaration every consumer reads from: a launcher's parser
/// before composition, the catalog registration at composition, and any
/// frontend that projects these verbs.
pub fn declared_invocables() -> Vec<Invocable> {
    vec![
        Invocable {
            id: id("init"),
            name: "Init",
            summary: "Initialize a Cronus workspace in the target directory",
            group: "init",
            locus: install(),
            binders: vec![text("path", true)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("status"),
            name: "Status",
            summary: "Show the current workspace status",
            group: "status",
            locus: inspect(),
            binders: Vec::new(),
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("doctor"),
            name: "Doctor",
            summary: "Self-healing: run health checks, optionally applying safe repairs",
            group: "doctor",
            // Reports by default; `--fix` raises one invocation to a
            // recompose (see `resolved_effect`).
            locus: inspect(),
            binders: vec![flag("fix")],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("restore"),
            name: "Restore",
            summary: "Restore a backup into the current state tier",
            group: "restore",
            locus: recompose(Apply::Relaunch),
            binders: vec![text("backup", false)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("dev.status"),
            name: "Dev Status",
            summary: "Print the resolved developer-office admission tier for the current directory",
            group: "dev",
            locus: inspect(),
            binders: Vec::new(),
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("dev.admit"),
            name: "Dev Admit",
            summary: "Grant developer-office admission — a human-operator act; run this \
                       yourself, never through an agent-invoked path",
            group: "dev",
            locus: recompose(Apply::Relaunch),
            binders: Vec::new(),
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("dev.revoke"),
            name: "Dev Revoke",
            summary: "Revoke developer-office admission",
            group: "dev",
            locus: recompose(Apply::Relaunch),
            binders: Vec::new(),
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("workspace.create"),
            name: "Workspace Create",
            summary: "Create a new workspace",
            group: "workspace",
            locus: install(),
            binders: vec![
                text("id", false),
                named_text("name", true),
                named_text("path", true),
            ],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("workspace.list"),
            name: "Workspace List",
            summary: "List all workspaces",
            group: "workspace",
            locus: inspect(),
            binders: Vec::new(),
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("workspace.switch"),
            name: "Workspace Switch",
            summary: "Switch the active workspace",
            group: "workspace",
            locus: install(),
            binders: vec![text("id", false)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("workspace.delete"),
            name: "Workspace Delete",
            summary: "Delete a workspace",
            group: "workspace",
            // Deleting one that is not running changes only stored state;
            // deleting the running one raises the invocation to a recompose
            // (see `resolved_effect`).
            locus: install(),
            binders: vec![text("id", false)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("workspace.check"),
            name: "Workspace Check",
            summary: "Check the status of a workspace",
            group: "workspace",
            locus: inspect(),
            binders: vec![text("id", false)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("backup.create"),
            name: "Backup Create",
            summary: "Create a backup",
            group: "backup",
            locus: install(),
            binders: vec![named_text("to", true), flag("include-logs")],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("backup.list"),
            name: "Backup List",
            summary: "List backups under the state tier's backups/ directory",
            group: "backup",
            locus: inspect(),
            binders: Vec::new(),
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("activation.status"),
            name: "Activation Status",
            summary: "Print the observed activation state — read from the OS, never a \
                       remembered value",
            group: "activation",
            locus: inspect(),
            binders: Vec::new(),
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("activation.enable"),
            name: "Activation Enable",
            summary: "Register background activation for a mode — an autonomy grant, not a \
                       preference, disclosed and confirmed before it takes effect",
            group: "activation",
            locus: install(),
            binders: vec![
                named_text("mode", false),
                flag("acknowledge-unattended-execution"),
            ],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("activation.disable"),
            name: "Activation Disable",
            summary: "Remove whatever activation registration is currently active — removed \
                       and verified, never left partially registered",
            group: "activation",
            locus: install(),
            binders: Vec::new(),
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("archetype.list"),
            name: "Archetype List",
            summary: "List archetypes — the shipped catalog, or the office's active one",
            group: "archetype",
            locus: inspect(),
            binders: vec![flag("catalog"), flag("active")],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("archetype.info"),
            name: "Archetype Info",
            summary: "Show one archetype's pool, shape, and seed (or its blocked reason)",
            group: "archetype",
            locus: inspect(),
            binders: vec![text("id", false), flag("deviations")],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("archetype.set"),
            name: "Archetype Set",
            summary: "Apply an archetype, or return to the archetype-free default. Changes \
                       what the manager expects, never staff — non-destructive by construction",
            group: "archetype",
            locus: recompose(Apply::InPlace),
            binders: vec![text("id", true), flag("clear")],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("archetype.create"),
            name: "Archetype Create",
            summary: "Create a custom archetype by copying a preset into the state tier",
            group: "archetype",
            locus: install(),
            binders: vec![text("name", false), named_text("from", false)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("registry.list"),
            name: "Registry List",
            summary: "List all agent definitions",
            group: "registry",
            locus: inspect(),
            binders: Vec::new(),
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("registry.show"),
            name: "Registry Show",
            summary: "Show an agent definition",
            group: "registry",
            locus: inspect(),
            binders: vec![text("name", false)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("registry.create"),
            name: "Registry Create",
            summary: "Create a custom agent entry",
            group: "registry",
            locus: install(),
            binders: vec![text("name", false), text("description", false)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("registry.disable"),
            name: "Registry Disable",
            summary: "Disable an agent",
            group: "registry",
            locus: recompose(Apply::InPlace),
            binders: vec![text("name", false)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("registry.enable"),
            name: "Registry Enable",
            summary: "Enable a previously disabled agent",
            group: "registry",
            locus: recompose(Apply::InPlace),
            binders: vec![text("name", false)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("ext.list"),
            name: "Ext List",
            summary: "List registered extensions",
            group: "ext",
            locus: inspect(),
            binders: Vec::new(),
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("ext.add"),
            name: "Ext Add",
            summary: "Add an extension by manifest path",
            group: "ext",
            locus: install(),
            binders: vec![text("path", false)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("ext.remove"),
            name: "Ext Remove",
            summary: "Remove an extension",
            group: "ext",
            locus: recompose(Apply::InPlace),
            binders: vec![text("id", false)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("ext.scan"),
            name: "Ext Scan",
            summary: "Scan an extension for security issues",
            group: "ext",
            locus: inspect(),
            binders: vec![text("path", false)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("ext.activate"),
            name: "Ext Activate",
            summary: "Activate an extension — an explicit grant of what its manifest declares, \
                       confirmed before it takes effect",
            group: "ext",
            locus: recompose(Apply::InPlace),
            binders: vec![text("id", false), flag("yes")],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("ext.deactivate"),
            name: "Ext Deactivate",
            summary: "Deactivate an extension",
            group: "ext",
            locus: recompose(Apply::InPlace),
            binders: vec![text("id", false)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        // `ext skill …` — one level deeper than every other `ext` verb
        // (§ the flat-vs-nested-vs-sub-nested note on `build_installation_tree`):
        // the dot in the verb tail (`skill.import`) is what signals it.
        Invocable {
            id: id("ext.skill.import"),
            name: "Ext Skill Import",
            summary: "Import and convert a foreign skill package",
            group: "ext",
            locus: install(),
            binders: vec![text("path", false)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("ext.skill.create"),
            name: "Ext Skill Create",
            summary: "Author a new skill from a natural-language prompt",
            group: "ext",
            locus: install(),
            binders: vec![named_text("prompt", false)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("ext.skill.status"),
            name: "Ext Skill Status",
            summary: "Show conversion and review status for one or all tracked skills",
            group: "ext",
            locus: inspect(),
            binders: vec![text("id", true)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        // `Installation`, not `Semantic`/`ClientLocal`: launching the
        // terminal UI has no meaning inside an already-running session, the
        // same reasoning that places every other verb here.
        // Answerable with zero composition, matching every other
        // installation verb — the terminal UI composes its own registry and
        // dispatcher internally the moment it starts, so this launcher never
        // needs to build one first.
        Invocable {
            id: id("tui"),
            name: "Tui",
            summary: "Launch the terminal UI — also the default composition when no verb is given",
            group: "tui",
            locus: inspect(),
            binders: Vec::new(),
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        // `Installation`: shell completion is asked far more often than the
        // product runs and must be answerable from this frontend's own
        // grammar. The generated script is emitted from the composed command
        // tree once, at install time — the script itself does the
        // per-keystroke work without calling back (a pre-composition
        // artifact refinement is future work; this verb still composes once).
        Invocable {
            id: id("completion"),
            name: "Completion",
            summary: "Print a shell completion script (bash, zsh, fish, powershell, elvish)",
            group: "completion",
            locus: inspect(),
            binders: vec![text("shell", false)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
    ]
}

/// The live effect of one resolved invocation of an installation verb: the
/// verb's declared base, raised where this invocation's arguments make it
/// disturb the running composition more than the verb usually does — never
/// lowered. `None` for an invocable that is not an installation verb.
///
/// `running_workspace` is the workspace the live composition is built
/// around, when there is one; deleting that one changes what is running,
/// deleting any other does not.
pub fn resolved_effect(
    invocable: &Invocable,
    args: &ArgValues,
    running_workspace: Option<&str>,
) -> Option<LiveEffect> {
    let base = invocable.locus.live_effect()?;
    let raised = match invocable.id.tail() {
        "doctor" if matches!(args.get("fix"), Some(ArgValue::Flag)) => LiveEffect::Recompose {
            apply: Apply::Relaunch,
        },
        "workspace.delete"
            if running_workspace.is_some()
                && matches!(args.get("id"), Some(ArgValue::Text(id)) if Some(id.as_str()) == running_workspace) =>
        {
            LiveEffect::Recompose {
                apply: Apply::Relaunch,
            }
        }
        _ => base,
    };
    Some(base.raised_to(raised))
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashSet};

    use super::*;

    fn declared(tail: &str) -> Invocable {
        declared_invocables()
            .into_iter()
            .find(|invocable| invocable.id.tail() == tail)
            .unwrap_or_else(|| panic!("no declared installation verb {tail:?}"))
    }

    fn effect_of(tail: &str) -> LiveEffect {
        declared(tail)
            .locus
            .live_effect()
            .unwrap_or_else(|| panic!("{tail} is not an installation verb"))
    }

    const RELAUNCH: LiveEffect = LiveEffect::Recompose {
        apply: Apply::Relaunch,
    };
    const IN_PLACE: LiveEffect = LiveEffect::Recompose {
        apply: Apply::InPlace,
    };

    const INSPECT_VERBS: [&str; 15] = [
        "status",
        "doctor",
        "dev.status",
        "workspace.list",
        "workspace.check",
        "backup.list",
        "activation.status",
        "archetype.list",
        "archetype.info",
        "registry.list",
        "registry.show",
        "ext.list",
        "ext.scan",
        "ext.skill.status",
        "completion",
    ];

    const INSTALL_VERBS: [&str; 12] = [
        "init",
        "workspace.create",
        "workspace.switch",
        "workspace.delete",
        "backup.create",
        "activation.enable",
        "activation.disable",
        "archetype.create",
        "registry.create",
        "ext.add",
        "ext.skill.import",
        "ext.skill.create",
    ];

    const RELAUNCH_VERBS: [&str; 3] = ["restore", "dev.admit", "dev.revoke"];

    const IN_PLACE_VERBS: [&str; 6] = [
        "archetype.set",
        "registry.enable",
        "registry.disable",
        "ext.activate",
        "ext.deactivate",
        "ext.remove",
    ];

    #[test]
    fn every_declared_verb_is_an_installation_verb_with_a_live_effect() {
        for invocable in declared_invocables() {
            assert!(
                invocable.locus.live_effect().is_some(),
                "{} is declared here but is not an installation verb",
                invocable.id
            );
            assert_eq!(invocable.stability, Stability::Shipped, "{}", invocable.id);
        }
    }

    #[test]
    fn declared_identities_are_unique_and_core_qualified() {
        let declared = declared_invocables();
        let mut seen = HashSet::new();
        for invocable in &declared {
            assert_eq!(invocable.id.qualifier(), "core", "{}", invocable.id);
            assert!(
                seen.insert(invocable.id.as_str().to_string()),
                "{} is declared twice",
                invocable.id
            );
        }
    }

    /// The grammar is closed: a new group is a deliberate decision, so adding
    /// one must change this list.
    #[test]
    fn the_declaration_covers_exactly_the_closed_set_of_groups() {
        let groups: BTreeSet<&str> = declared_invocables()
            .iter()
            .map(|invocable| invocable.group)
            .collect();
        let expected: BTreeSet<&str> = [
            "activation",
            "archetype",
            "backup",
            "completion",
            "dev",
            "doctor",
            "ext",
            "init",
            "registry",
            "restore",
            "status",
            "tui",
            "workspace",
        ]
        .into_iter()
        .collect();
        assert_eq!(groups, expected);
    }

    #[test]
    fn verbs_that_only_read_are_declared_inspect() {
        for tail in INSPECT_VERBS {
            assert_eq!(effect_of(tail), LiveEffect::Inspect, "{tail}");
        }
    }

    #[test]
    fn verbs_that_change_stored_state_the_composition_does_not_hold_are_declared_install() {
        for tail in INSTALL_VERBS {
            assert_eq!(effect_of(tail), LiveEffect::Install, "{tail}");
        }
    }

    #[test]
    fn verbs_that_change_what_the_composition_is_built_from_are_declared_recompose() {
        for tail in RELAUNCH_VERBS {
            assert_eq!(effect_of(tail), RELAUNCH, "{tail}");
        }
        for tail in IN_PLACE_VERBS {
            assert_eq!(effect_of(tail), IN_PLACE, "{tail}");
        }
    }

    /// Every declared verb is classified by one of the tables above or is the
    /// launcher's own `tui`; a verb added without a classification decision
    /// fails here instead of defaulting to something quiet.
    #[test]
    fn no_declared_verb_escapes_the_classification_tables() {
        let classified: HashSet<&str> = INSPECT_VERBS
            .into_iter()
            .chain(INSTALL_VERBS)
            .chain(RELAUNCH_VERBS)
            .chain(IN_PLACE_VERBS)
            .chain(["tui"])
            .collect();
        for invocable in declared_invocables() {
            assert!(
                classified.contains(invocable.id.tail()),
                "{} has no entry in the classification tables",
                invocable.id
            );
        }
        assert_eq!(
            classified.len(),
            declared_invocables().len(),
            "a table names a verb that is not declared"
        );
    }

    fn args_with(entries: &[(&str, ArgValue)]) -> ArgValues {
        let mut args = ArgValues::new();
        for (name, value) in entries {
            args.insert(*name, value.clone());
        }
        args
    }

    #[test]
    fn diagnostics_reports_by_default_and_recomposes_when_asked_to_repair() {
        let doctor = declared("doctor");
        assert_eq!(
            resolved_effect(&doctor, &ArgValues::new(), None),
            Some(LiveEffect::Inspect)
        );
        assert_eq!(
            resolved_effect(&doctor, &args_with(&[("fix", ArgValue::Flag)]), None),
            Some(RELAUNCH)
        );
    }

    #[test]
    fn deleting_the_running_workspace_recomposes_and_any_other_does_not() {
        let delete = declared("workspace.delete");
        let other = args_with(&[("id", ArgValue::Text("scratch".to_string()))]);
        let running = args_with(&[("id", ArgValue::Text("main".to_string()))]);

        assert_eq!(
            resolved_effect(&delete, &other, Some("main")),
            Some(LiveEffect::Install)
        );
        assert_eq!(
            resolved_effect(&delete, &running, Some("main")),
            Some(RELAUNCH)
        );
        assert_eq!(
            resolved_effect(&delete, &running, None),
            Some(LiveEffect::Install),
            "with nothing running there is nothing for the delete to disturb"
        );
    }

    #[test]
    fn an_argument_can_raise_a_verbs_effect_but_never_lower_it() {
        let restore = declared("restore");
        assert_eq!(
            resolved_effect(
                &restore,
                &args_with(&[("backup", ArgValue::Text("b-1".to_string()))]),
                Some("main")
            ),
            Some(RELAUNCH)
        );
        let activate = declared("ext.activate");
        assert_eq!(
            resolved_effect(&activate, &ArgValues::new(), None),
            Some(IN_PLACE),
            "a verb already at recompose keeps its declared way of applying"
        );
        let delete = declared("workspace.delete");
        assert_eq!(
            resolved_effect(&delete, &ArgValues::new(), Some("main")),
            Some(LiveEffect::Install),
            "an absent target raises nothing"
        );
    }

    #[test]
    fn an_invocable_that_is_not_an_installation_verb_has_no_effect_to_resolve() {
        let semantic = Invocable {
            id: InvocableId::new("core:board.list").expect("well-formed id"),
            name: "List cards",
            summary: "List cards on the board",
            group: "board",
            locus: Locus::Semantic,
            binders: Vec::new(),
            stability: Stability::Shipped,
            journal_raw_input: true,
        };
        assert_eq!(resolved_effect(&semantic, &ArgValues::new(), None), None);
    }

    /// The raising rules name binders by string; renaming one in the
    /// declaration must fail here rather than silently disable the rule.
    #[test]
    fn the_binders_the_raising_rules_read_exist_on_their_verbs() {
        let doctor = declared("doctor");
        assert!(
            doctor
                .binders
                .iter()
                .any(|b| b.name == "fix" && b.kind == BinderKind::Flag)
        );
        let delete = declared("workspace.delete");
        assert!(
            delete
                .binders
                .iter()
                .any(|b| b.name == "id" && b.kind == BinderKind::Text)
        );
    }
}
