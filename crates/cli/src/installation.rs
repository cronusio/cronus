//! The installation half of the command surface: workspace
//! initialization, diagnostics, and developer-office admission — verbs that
//! configure or inspect the product itself rather than act on the user's
//! work.
//!
//! Unlike the semantic half ([`crate::generated`]), this grammar is not
//! generated from the registry — it is declared **once**, right here, and
//! that one declaration has two consumers: [`build_installation_tree`]
//! builds the parser from it before composition even runs, and
//! [`declared_invocables`] feeds the same list into the catalog at
//! composition, so another surface can see these verbs exist and declare
//! that it deliberately does not offer them, instead of looking merely
//! unfinished. A verb declared in a hand-written parser and again as a
//! hand-written descriptor is two statements of one fact — the exact fork
//! this whole command-surface redesign exists to close, one level up from
//! where the semantic half closes it.
//!
//! This is also why an installation verb answers directly, through the
//! functions already proven in [`crate::commands`], rather than through the
//! shared [`cronus_core::invocable::Dispatcher`]: that shared pipeline
//! exists so one handler can serve every surface, and no other surface ever
//! projects the `Installation` locus (only the command line does), so the
//! indirection would buy nothing here. `tui` is the one exception to
//! *which* function it answers through — it launches the sibling terminal
//! frontend's own composition (`cronus_tui::run`) rather than a
//! `crate::commands` handler, since that frontend's own registry/dispatcher
//! pair is what actually runs a session, not this launcher's.
//!
//! All twelve installation groups live here — `init`, `status`, `doctor`,
//! `restore`, `dev`, `workspace`, `backup`, `activation`, `archetype`,
//! `registry`, `ext`, and `tui` — covering four tree shapes: **flat** (a
//! group's one verb's id-tail equals its group name, so it renders as a
//! single top-level command with no nested verb — `tui` is this shape),
//! **nested** (a group command containing verb subcommands), a **named
//! value flag** (`--actor cli`, `--mode login`), which `BinderKind::NamedText`
//! exists to express — `Binder` had no positional-vs-named-value distinction
//! before this module needed one for `workspace create --name/--path`,
//! `backup create --to`, `activation enable --mode`, and `archetype create
//! --from` — and one extra level of **sub-nesting** (`ext skill
//! import|create|status`), signalled the same way `verb_of` already
//! separates a group from its verbs: a dot in the verb tail (`skill.import`)
//! names one sub-group, not a fourth top-level command.

use std::collections::HashSet;

use clap::{Arg, ArgMatches, Command};
use cronus_contract::{Binder, BinderKind, Invocable, InvocableId, Locus, Stability};

use crate::generated::verb_of;
use crate::output::Context;

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

/// The single declaration both the pre-composition parser and the
/// post-composition catalog registration read from.
pub fn declared_invocables() -> Vec<Invocable> {
    vec![
        Invocable {
            id: id("init"),
            name: "Init",
            summary: "Initialize a Cronus workspace in the target directory",
            group: "init",
            locus: Locus::Installation,
            binders: vec![text("path", true)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("status"),
            name: "Status",
            summary: "Show the current workspace status",
            group: "status",
            locus: Locus::Installation,
            binders: Vec::new(),
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("doctor"),
            name: "Doctor",
            summary: "Self-healing: run health checks, optionally applying safe repairs",
            group: "doctor",
            locus: Locus::Installation,
            binders: vec![flag("fix")],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("restore"),
            name: "Restore",
            summary: "Restore a backup into the current state tier",
            group: "restore",
            locus: Locus::Installation,
            binders: vec![text("backup", false)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("dev.status"),
            name: "Dev Status",
            summary: "Print the resolved developer-office admission tier for the current directory",
            group: "dev",
            locus: Locus::Installation,
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
            locus: Locus::Installation,
            binders: Vec::new(),
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("dev.revoke"),
            name: "Dev Revoke",
            summary: "Revoke developer-office admission",
            group: "dev",
            locus: Locus::Installation,
            binders: Vec::new(),
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("workspace.create"),
            name: "Workspace Create",
            summary: "Create a new workspace",
            group: "workspace",
            locus: Locus::Installation,
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
            locus: Locus::Installation,
            binders: Vec::new(),
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("workspace.switch"),
            name: "Workspace Switch",
            summary: "Switch the active workspace",
            group: "workspace",
            locus: Locus::Installation,
            binders: vec![text("id", false)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("workspace.delete"),
            name: "Workspace Delete",
            summary: "Delete a workspace",
            group: "workspace",
            locus: Locus::Installation,
            binders: vec![text("id", false)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("workspace.check"),
            name: "Workspace Check",
            summary: "Check the status of a workspace",
            group: "workspace",
            locus: Locus::Installation,
            binders: vec![text("id", false)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("backup.create"),
            name: "Backup Create",
            summary: "Create a backup",
            group: "backup",
            locus: Locus::Installation,
            binders: vec![named_text("to", true), flag("include-logs")],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("backup.list"),
            name: "Backup List",
            summary: "List backups under the state tier's backups/ directory",
            group: "backup",
            locus: Locus::Installation,
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
            locus: Locus::Installation,
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
            locus: Locus::Installation,
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
            locus: Locus::Installation,
            binders: Vec::new(),
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("archetype.list"),
            name: "Archetype List",
            summary: "List archetypes — the shipped catalog, or the office's active one",
            group: "archetype",
            locus: Locus::Installation,
            binders: vec![flag("catalog"), flag("active")],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("archetype.info"),
            name: "Archetype Info",
            summary: "Show one archetype's pool, shape, and seed (or its blocked reason)",
            group: "archetype",
            locus: Locus::Installation,
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
            locus: Locus::Installation,
            binders: vec![text("id", true), flag("clear")],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("archetype.create"),
            name: "Archetype Create",
            summary: "Create a custom archetype by copying a preset into the state tier",
            group: "archetype",
            locus: Locus::Installation,
            binders: vec![text("name", false), named_text("from", false)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("registry.list"),
            name: "Registry List",
            summary: "List all agent definitions",
            group: "registry",
            locus: Locus::Installation,
            binders: Vec::new(),
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("registry.show"),
            name: "Registry Show",
            summary: "Show an agent definition",
            group: "registry",
            locus: Locus::Installation,
            binders: vec![text("name", false)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("registry.create"),
            name: "Registry Create",
            summary: "Create a custom agent entry",
            group: "registry",
            locus: Locus::Installation,
            binders: vec![text("name", false), text("description", false)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("registry.disable"),
            name: "Registry Disable",
            summary: "Disable an agent",
            group: "registry",
            locus: Locus::Installation,
            binders: vec![text("name", false)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("registry.enable"),
            name: "Registry Enable",
            summary: "Enable a previously disabled agent",
            group: "registry",
            locus: Locus::Installation,
            binders: vec![text("name", false)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("ext.list"),
            name: "Ext List",
            summary: "List registered extensions",
            group: "ext",
            locus: Locus::Installation,
            binders: Vec::new(),
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("ext.add"),
            name: "Ext Add",
            summary: "Add an extension by manifest path",
            group: "ext",
            locus: Locus::Installation,
            binders: vec![text("path", false)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("ext.remove"),
            name: "Ext Remove",
            summary: "Remove an extension",
            group: "ext",
            locus: Locus::Installation,
            binders: vec![text("id", false)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("ext.scan"),
            name: "Ext Scan",
            summary: "Scan an extension for security issues",
            group: "ext",
            locus: Locus::Installation,
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
            locus: Locus::Installation,
            binders: vec![text("id", false), flag("yes")],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("ext.deactivate"),
            name: "Ext Deactivate",
            summary: "Deactivate an extension",
            group: "ext",
            locus: Locus::Installation,
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
            locus: Locus::Installation,
            binders: vec![text("path", false)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("ext.skill.create"),
            name: "Ext Skill Create",
            summary: "Author a new skill from a natural-language prompt",
            group: "ext",
            locus: Locus::Installation,
            binders: vec![named_text("prompt", false)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
        Invocable {
            id: id("ext.skill.status"),
            name: "Ext Skill Status",
            summary: "Show conversion and review status for one or all tracked skills",
            group: "ext",
            locus: Locus::Installation,
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
            locus: Locus::Installation,
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
            locus: Locus::Installation,
            binders: vec![text("shell", false)],
            stability: Stability::Shipped,
            journal_raw_input: true,
        },
    ]
}

/// A nested group's own about text — declared here rather than invented as
/// a generic `"<group> operations"` fallback, since a nested installation
/// group deserves the same care a flat one gets from its one verb's own
/// summary. Groups not yet listed here (the six not migrated this pass)
/// fall back to the generic text until they join this list.
fn group_about(group: &str) -> Option<&'static str> {
    match group {
        "dev" => {
            Some("Self-hosting developer office: conditional, human-admitted, canonical-repo-only")
        }
        "workspace" => Some("Manage named workspaces"),
        "backup" => Some("Back up the state tier (secrets and cache excluded)"),
        "activation" => Some("Manage background activation"),
        "archetype" => Some("Office archetypes: a prior on staffing, never a roster"),
        "registry" => Some("Agent registry: list and manage agent definitions"),
        "ext" => Some("Extensions: manage skills, MCP servers, and plugins"),
        "skill" => Some("Skill packages: import, create, and inspect status"),
        _ => None,
    }
}

/// Build the clap subcommand tree for the installation half. Unlike the
/// semantic tree (always nested, one subcommand level per group), a group
/// with exactly one verb whose id-tail equals its own group name — no
/// `"<group>."` prefix to strip — renders **flat**: the group's own
/// top-level `Command` carries that verb's args directly, matching the
/// existing `cronus doctor --fix` / `cronus restore <backup>` shape rather
/// than inventing `cronus doctor doctor`.
pub fn build_installation_tree(invocables: &[&Invocable]) -> (Vec<Command>, HashSet<String>) {
    let mut by_group: std::collections::HashMap<&str, Vec<&Invocable>> =
        std::collections::HashMap::new();
    for invocable in invocables {
        by_group.entry(invocable.group).or_default().push(invocable);
    }

    let mut group_order: Vec<&str> = by_group.keys().copied().collect();
    group_order.sort_unstable();

    let mut group_names = HashSet::new();
    let mut groups = Vec::new();

    for group in group_order {
        let members = &by_group[group];
        group_names.insert(group.to_string());

        if members.len() == 1 && verb_of(members[0]) == group {
            let mut flat = Command::new(group.to_string()).about(members[0].summary);
            for binder in &members[0].binders {
                flat = flat.arg(crate::generated::arg_for_with_help(
                    binder,
                    crate::generated::binder_help(members[0].id.as_str(), binder.name),
                ));
            }
            groups.push(flat);
            continue;
        }

        let about = group_about(group)
            .map(str::to_string)
            .unwrap_or_else(|| format!("{group} operations"));
        // A nested group with no verb is a usage failure (clap prints its help
        // and exits 2), never an application failure: `dispatch`'s
        // `dispatch_leaf(group, group, ...)` fallback — which produced the
        // "no installation handler wired for <g> <g>" internal error — is
        // reached only for a genuinely flat group now.
        let mut nested = Command::new(group.to_string())
            .about(about)
            .subcommand_required(true)
            .arg_required_else_help(true);

        // A verb tail itself containing a dot (`skill.import`) names one
        // sub-group, one level deeper than every other verb in this group
        // — `ext skill import`, not a fourth top-level `skill-import`
        // command. Not a general N-level scheme: exactly one extra level,
        // for the one group that has ever needed it, following the same
        // "the dot in the tail is the nesting signal" rule `verb_of`
        // already uses to separate a group from its verbs.
        let mut plain_verbs: Vec<&Invocable> = Vec::new();
        let mut subgroups: std::collections::HashMap<&str, Vec<(&str, &Invocable)>> =
            std::collections::HashMap::new();
        for invocable in members.iter().copied() {
            match verb_of(invocable).split_once('.') {
                Some((subgroup, leaf)) => subgroups
                    .entry(subgroup)
                    .or_default()
                    .push((leaf, invocable)),
                None => plain_verbs.push(invocable),
            }
        }

        for invocable in plain_verbs {
            let mut verb = Command::new(verb_of(invocable).to_string()).about(invocable.summary);
            for binder in &invocable.binders {
                verb = verb.arg(crate::generated::arg_for_with_help(
                    binder,
                    crate::generated::binder_help(invocable.id.as_str(), binder.name),
                ));
            }
            nested = nested.subcommand(verb);
        }

        let mut subgroup_names: Vec<&str> = subgroups.keys().copied().collect();
        subgroup_names.sort_unstable();
        for subgroup in subgroup_names {
            let about = group_about(subgroup)
                .map(str::to_string)
                .unwrap_or_else(|| format!("{subgroup} operations"));
            let mut sub_command = Command::new(subgroup.to_string())
                .about(about)
                .subcommand_required(true)
                .arg_required_else_help(true);
            for (leaf, invocable) in &subgroups[subgroup] {
                let mut verb = Command::new((*leaf).to_string()).about(invocable.summary);
                for binder in &invocable.binders {
                    verb = verb.arg(crate::generated::arg_for_with_help(
                        binder,
                        crate::generated::binder_help(invocable.id.as_str(), binder.name),
                    ));
                }
                sub_command = sub_command.subcommand(verb);
            }
            nested = nested.subcommand(sub_command);
        }

        groups.push(nested);
    }

    (groups, group_names)
}

/// The shared `--format`/`-f` global, duplicated here rather than shared
/// with `Cli`'s derive-generated definition: the two are genuinely two
/// consumers of the same **flag**, not the verb grammar this module's own
/// "declared once" property is about, and it is the one launcher-owned flag
/// that is called out explicitly ("which configuration overlays to apply").
pub fn format_arg() -> Arg {
    Arg::new("format")
        .long("format")
        .short('f')
        .global(true)
        .value_parser(clap::value_parser!(crate::output::OutputFormat))
        .default_value("text")
        // `Cli`'s derive-generated `--format` gets its `<FORMAT>` value
        // name and "Output format" help text from the field name and its
        // doc comment automatically; this hand-built duplicate needs the
        // same two calls to match, or the two halves render the same flag
        // differently in --help.
        .value_name("FORMAT")
        .help("Output format")
}

/// Launch the terminal UI and map its result to a process exit code — the
/// one function both spellings of "start the terminal UI" call: `cronus tui`
/// through [`dispatch_leaf`]'s own `"tui"` arm, and a bare invocation (the
/// default composition) directly from `main`, before any composition of
/// this launcher's own tree runs. One function, two callers, rather than the
/// same `cronus_tui::run()` call and error mapping written out twice.
pub fn launch_tui() -> i32 {
    match cronus_tui::run() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: tui: {e}");
            1
        }
    }
}

/// Recursively disables clap's auto-generated `help <verb>` pseudo-subcommand
/// on `command` and every subcommand it owns, at every nesting level.
///
/// `-h`/`--help` stays fully available on every command — `disable_help_
/// subcommand` is documented as leaving the flag untouched, it only removes
/// the redundant `help` subcommand alias for it. Recurses via
/// `mut_subcommands`, which maps this same function over each immediate
/// subcommand — since that in turn calls `mut_subcommands` on itself, the
/// whole tree is walked regardless of depth, not just one level.
///
/// A simulation-qa pass found the generated bash completion script at
/// ~240KB: clap injects a `help` subcommand at every level that has
/// children, so a two-level `<group> <verb>` grammar gets one real
/// subcommand entry plus one `help`-alias entry per node, roughly doubling
/// the case-arm count every completion generator has to emit. This trims
/// the tree handed to `clap_complete::generate` specifically — the tree
/// used for real argument parsing (`main.rs`'s `command`) is untouched, so
/// `cronus <group> help <verb>` keeps working exactly as before; only the
/// generated completion scripts get smaller.
fn strip_help_subcommand(command: Command) -> Command {
    command
        .disable_help_subcommand(true)
        .mut_subcommands(strip_help_subcommand)
}

/// Write a shell completion script for the composed command tree to stdout.
///
/// `command` is the *whole* tree — every installation and semantic group,
/// including anything an extension contributed — so the emitted script is a
/// faithful, static snapshot of the current surface. It is generated once (at
/// install time); the script itself does the per-keystroke work without
/// re-invoking `cronus`.
///
/// Generates from a [`strip_help_subcommand`]-trimmed *clone* of `command`,
/// never `command` itself: the caller's tree still serves real argument
/// parsing (`--help`, usage errors) after this call returns, so trimming it
/// in place would leak into every other consumer of the same `Command`.
pub fn emit_completion(matches: &ArgMatches, command: &mut Command) -> i32 {
    let shell_arg = matches
        .get_one::<String>("shell")
        .map(String::as_str)
        .unwrap_or_default();
    let shell = match shell_arg {
        "bash" => clap_complete::Shell::Bash,
        "zsh" => clap_complete::Shell::Zsh,
        "fish" => clap_complete::Shell::Fish,
        "powershell" | "pwsh" => clap_complete::Shell::PowerShell,
        "elvish" => clap_complete::Shell::Elvish,
        other => {
            eprintln!(
                "error: unknown shell {other:?} (expected: bash, zsh, fish, powershell, elvish)"
            );
            return 2;
        }
    };
    let mut trimmed = strip_help_subcommand(command.clone());
    clap_complete::generate(shell, &mut trimmed, "cronus", &mut std::io::stdout());
    0
}

/// Resolve `group`'s own matched subcommand and dispatch it directly to its
/// already-proven handler in [`crate::commands`] — no shared
/// `Dispatcher`/`Outcome` indirection, for the reason the module doc gives.
/// Returns the same exit code the handler itself already returned before
/// this migration.
///
/// `group_matches` is the group's own top-level `ArgMatches` (what its
/// `Command` produced), not pre-split into verb/args by the caller: a group
/// with `ext`'s one extra nesting level needs a second `.subcommand()` call
/// to reach the leaf verb, and resolving both levels here — rather than
/// asking every caller to know which groups nest how deep — keeps that
/// knowledge in the one place [`build_installation_tree`] already has it.
///
/// `[FIXED]` A **flat** group (its one verb's id-tail equals its own group
/// name — `build_installation_tree`'s own flat-shape rule) carries that
/// verb's args directly on the group's own `Command`, with nothing nested
/// under it at all: `group_matches.subcommand()` is genuinely `None` for
/// one, not a caller error. This was a real, latent bug — every flat verb
/// (`init`/`status`/`doctor`/`restore`/`tui`) fell straight into the
/// internal-error branch below the moment it was actually run (not merely
/// `--help`'d), undiscovered because no end-to-end test had ever invoked
/// one without `--help` until this task's own manual verification did.
pub fn dispatch(group: &str, group_matches: &ArgMatches, ctx: &Context) -> i32 {
    let Some((first, first_matches)) = group_matches.subcommand() else {
        // No nested subcommand at all means this is a flat group: its own
        // top-level matches ARE the one verb's matches, and that verb's
        // name is the group's own name.
        return dispatch_leaf(group, group, group_matches, ctx);
    };
    // A verb never has a subcommand of its own (every verb's own arguments
    // are positional/named `Arg`s, never further subcommands) — so a
    // *further* subcommand here means `first` named a sub-group (`skill`),
    // not a leaf verb, and the leaf verb is one level deeper.
    let (verb, matches): (String, &ArgMatches) = match first_matches.subcommand() {
        Some((leaf, leaf_matches)) => (format!("{first}.{leaf}"), leaf_matches),
        None => (first.to_string(), first_matches),
    };
    dispatch_leaf(group, &verb, matches, ctx)
}

fn dispatch_leaf(group: &str, verb: &str, matches: &ArgMatches, ctx: &Context) -> i32 {
    match (group, verb) {
        ("init", _) => {
            let path = matches
                .get_one::<String>("path")
                .map(std::path::PathBuf::from);
            crate::commands::init::run(path, ctx)
        }
        ("status", _) => crate::commands::status::run(ctx),
        ("doctor", _) => {
            let fix = matches.get_flag("fix");
            crate::commands::doctor::run(fix, ctx)
        }
        ("restore", _) => {
            let backup = matches
                .get_one::<String>("backup")
                .map(String::as_str)
                .unwrap_or_default();
            crate::commands::backup_cmd::restore(backup, ctx)
        }
        ("dev", "status") => crate::commands::dev_office_cmd::status(ctx),
        ("dev", "admit") => crate::commands::dev_office_cmd::admit(ctx),
        ("dev", "revoke") => crate::commands::dev_office_cmd::revoke(ctx),
        ("workspace", "create") => {
            let id = matches.get_one::<String>("id").cloned().unwrap_or_default();
            let name = matches.get_one::<String>("name").cloned();
            let path = matches
                .get_one::<String>("path")
                .map(std::path::PathBuf::from);
            crate::commands::workspace::create(id, name, path, ctx)
        }
        ("workspace", "list") => crate::commands::workspace::list(ctx),
        ("workspace", "switch") => {
            let id = matches.get_one::<String>("id").cloned().unwrap_or_default();
            crate::commands::workspace::switch(id, ctx)
        }
        ("workspace", "delete") => {
            let id = matches.get_one::<String>("id").cloned().unwrap_or_default();
            crate::commands::workspace::delete(id, ctx)
        }
        ("workspace", "check") => {
            let id = matches.get_one::<String>("id").cloned().unwrap_or_default();
            crate::commands::workspace::check(id, ctx)
        }
        ("backup", "create") => {
            let to = matches
                .get_one::<String>("to")
                .map(std::path::PathBuf::from);
            let include_logs = matches.get_flag("include-logs");
            crate::commands::backup_cmd::create(to, include_logs, ctx)
        }
        ("backup", "list") => crate::commands::backup_cmd::list(ctx),
        ("activation", "status") => crate::commands::activation_cmd::status(ctx),
        ("activation", "enable") => {
            let mode = match matches.get_one::<String>("mode").map(String::as_str) {
                Some("login") => crate::cli::ActivationModeArg::Login,
                Some("system") => crate::cli::ActivationModeArg::System,
                other => {
                    // An application-level refusal, not a clap-level usage
                    // failure: the flag parsed fine as text, dispatch ran,
                    // and the value it ran with was one it does not accept
                    // — the same "ran and refused" shape every other
                    // handler in this module reports with exit 1.
                    eprintln!(
                        "error: --mode must be 'login' or 'system', got {:?}",
                        other.unwrap_or("<none>")
                    );
                    return 1;
                }
            };
            let acknowledged = matches.get_flag("acknowledge-unattended-execution");
            crate::commands::activation_cmd::enable(mode, acknowledged, ctx)
        }
        ("activation", "disable") => crate::commands::activation_cmd::disable(ctx),
        ("archetype", "list") => {
            let catalog = matches.get_flag("catalog");
            let active = matches.get_flag("active");
            crate::commands::archetype_cmd::list(catalog, active, ctx)
        }
        ("archetype", "info") => {
            let id = matches.get_one::<String>("id").cloned().unwrap_or_default();
            let deviations = matches.get_flag("deviations");
            crate::commands::archetype_cmd::info(&id, deviations, ctx)
        }
        ("archetype", "set") => {
            let id = matches.get_one::<String>("id").cloned();
            let clear = matches.get_flag("clear");
            crate::commands::archetype_cmd::set(id, clear, ctx)
        }
        ("archetype", "create") => {
            let name = matches
                .get_one::<String>("name")
                .cloned()
                .unwrap_or_default();
            let from = matches
                .get_one::<String>("from")
                .cloned()
                .unwrap_or_default();
            crate::commands::archetype_cmd::create(&name, &from, ctx)
        }
        ("registry", "list") => crate::commands::registry::list(ctx),
        ("registry", "show") => {
            let name = matches
                .get_one::<String>("name")
                .cloned()
                .unwrap_or_default();
            crate::commands::registry::show(name, ctx)
        }
        ("registry", "create") => {
            let name = matches
                .get_one::<String>("name")
                .cloned()
                .unwrap_or_default();
            let description = matches
                .get_one::<String>("description")
                .cloned()
                .unwrap_or_default();
            crate::commands::registry::create(name, description, ctx)
        }
        ("registry", "disable") => {
            let name = matches
                .get_one::<String>("name")
                .cloned()
                .unwrap_or_default();
            crate::commands::registry::disable(name, ctx)
        }
        ("registry", "enable") => {
            let name = matches
                .get_one::<String>("name")
                .cloned()
                .unwrap_or_default();
            crate::commands::registry::enable(name, ctx)
        }
        ("ext", "list") => crate::commands::ext::list(ctx),
        ("ext", "add") => {
            let path = matches
                .get_one::<String>("path")
                .map(std::path::PathBuf::from)
                .unwrap_or_default();
            crate::commands::ext::add(path, ctx)
        }
        ("ext", "remove") => {
            let ext_id = matches.get_one::<String>("id").cloned().unwrap_or_default();
            crate::commands::ext::remove(ext_id, ctx)
        }
        ("ext", "scan") => {
            let path = matches
                .get_one::<String>("path")
                .map(std::path::PathBuf::from)
                .unwrap_or_default();
            crate::commands::ext::scan(path, ctx)
        }
        ("ext", "activate") => {
            let ext_id = matches.get_one::<String>("id").cloned().unwrap_or_default();
            let acknowledged = matches.get_flag("yes");
            crate::commands::ext::activate(ext_id, acknowledged, ctx)
        }
        ("ext", "deactivate") => {
            let ext_id = matches.get_one::<String>("id").cloned().unwrap_or_default();
            crate::commands::ext::deactivate(ext_id, ctx)
        }
        ("ext", "skill.import") => {
            let path = matches
                .get_one::<String>("path")
                .map(std::path::PathBuf::from)
                .unwrap_or_default();
            crate::commands::ext::skill::import(&path, ctx)
        }
        ("ext", "skill.create") => {
            let prompt = matches
                .get_one::<String>("prompt")
                .cloned()
                .unwrap_or_default();
            crate::commands::ext::skill::create(&prompt, ctx)
        }
        ("ext", "skill.status") => {
            let skill_id = matches.get_one::<String>("id").cloned();
            crate::commands::ext::skill::status(skill_id, ctx)
        }
        ("tui", _) => launch_tui(),
        _ => {
            eprintln!(
                "error: internal: no installation handler wired for {group} {verb} — a bug in this module's own dispatch table, not a caller condition"
            );
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet as Set;

    use cronus_core::invocable::{InvocableRegistry, Registrant};

    use super::*;

    #[test]
    fn a_flat_group_renders_as_one_top_level_command_with_no_nested_verb() {
        let invocables = declared_invocables();
        let refs: Vec<&Invocable> = invocables.iter().filter(|i| i.group == "doctor").collect();
        let (groups, names) = build_installation_tree(&refs);
        assert_eq!(names, Set::from(["doctor".to_string()]));
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].get_name(), "doctor");
        assert_eq!(
            groups[0].get_subcommands().count(),
            0,
            "a flat group must carry its one verb's args directly, not nest a same-named subcommand"
        );
        assert!(groups[0].get_arguments().any(|a| a.get_id() == "fix"));
    }

    /// The structural assumption `dispatch`'s flat-group fix depends on: a
    /// flat group's own matches carry no nested subcommand at all — proven
    /// against a real parse, not assumed from `build_installation_tree`'s
    /// own tree shape alone. This was the actual, latent bug: `dispatch`
    /// used to unconditionally require a subcommand here and fell into its
    /// internal-error branch for every flat verb the moment one was
    /// actually run, not merely `--help`'d.
    #[test]
    fn a_flat_groups_own_matches_have_no_subcommand_to_find() {
        let invocables = declared_invocables();
        let refs: Vec<&Invocable> = invocables.iter().filter(|i| i.group == "doctor").collect();
        let (groups, _) = build_installation_tree(&refs);
        let tree = Command::new("cronus").subcommand(groups.into_iter().next().unwrap());

        let matches = tree
            .try_get_matches_from(["cronus", "doctor", "--fix"])
            .expect("doctor --fix must parse");
        let (_, group_matches) = matches.subcommand().unwrap();
        assert!(
            group_matches.subcommand().is_none(),
            "a flat group's own matches must carry no nested subcommand"
        );
        assert!(group_matches.get_flag("fix"));
    }

    #[test]
    fn a_multi_verb_group_renders_nested_exactly_like_the_semantic_half() {
        let invocables = declared_invocables();
        let refs: Vec<&Invocable> = invocables.iter().filter(|i| i.group == "dev").collect();
        let (groups, _) = build_installation_tree(&refs);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].get_name(), "dev");
        let verbs: Vec<&str> = groups[0].get_subcommands().map(|c| c.get_name()).collect();
        assert_eq!(verbs.len(), 3);
        for expected in ["status", "admit", "revoke"] {
            assert!(verbs.contains(&expected), "missing dev verb: {expected}");
        }
    }

    #[test]
    fn a_required_text_binder_rejects_the_verb_with_no_value() {
        let invocables = declared_invocables();
        let refs: Vec<&Invocable> = invocables.iter().filter(|i| i.group == "restore").collect();
        let (groups, _) = build_installation_tree(&refs);
        let tree = Command::new("cronus").subcommand(groups.into_iter().next().unwrap());
        assert!(
            tree.clone()
                .try_get_matches_from(["cronus", "restore"])
                .is_err(),
            "restore's required backup id must not be optional"
        );
        let matches = tree
            .try_get_matches_from(["cronus", "restore", "b-1"])
            .expect("restore b-1 must parse");
        let (_, sub) = matches.subcommand().unwrap();
        assert_eq!(
            sub.get_one::<String>("backup").map(String::as_str),
            Some("b-1")
        );
    }

    /// `BinderKind::NamedText` — proves the extension end to end: bound as
    /// `--name <value>`, never positionally, and a required one refuses the
    /// verb entirely when absent, exactly like a required positional would.
    #[test]
    fn a_named_text_binder_is_bound_as_a_named_flag_never_a_positional_word() {
        let invocables = declared_invocables();
        let refs: Vec<&Invocable> = invocables
            .iter()
            .filter(|i| i.group == "archetype")
            .collect();
        let (groups, _) = build_installation_tree(&refs);
        let tree = Command::new("cronus").subcommand(groups.into_iter().next().unwrap());

        // `create` requires `name` (positional) and `--from` (named) — the
        // bare word form must not satisfy `--from`.
        assert!(
            tree.clone()
                .try_get_matches_from(["cronus", "archetype", "create", "custom", "preset-a"])
                .is_err(),
            "a NamedText binder must not accept its value as a second positional word"
        );

        let matches = tree
            .try_get_matches_from([
                "cronus",
                "archetype",
                "create",
                "custom",
                "--from",
                "preset-a",
            ])
            .expect("create custom --from preset-a must parse");
        let (_, group_matches) = matches.subcommand().unwrap();
        let (_, verb_matches) = group_matches.subcommand().unwrap();
        assert_eq!(
            verb_matches.get_one::<String>("from").map(String::as_str),
            Some("preset-a")
        );
    }

    /// An optional `NamedText` binder (`workspace create --name`) must
    /// parse fine when absent — the handler, not the parser, supplies a
    /// default, matching every other optional binder's contract.
    #[test]
    fn an_optional_named_text_binder_is_absent_when_not_supplied() {
        let invocables = declared_invocables();
        let refs: Vec<&Invocable> = invocables
            .iter()
            .filter(|i| i.group == "workspace")
            .collect();
        let (groups, _) = build_installation_tree(&refs);
        let tree = Command::new("cronus").subcommand(groups.into_iter().next().unwrap());

        let matches = tree
            .try_get_matches_from(["cronus", "workspace", "create", "my-ws"])
            .expect("create with no --name/--path must still parse");
        let (_, group_matches) = matches.subcommand().unwrap();
        let (_, verb_matches) = group_matches.subcommand().unwrap();
        assert_eq!(
            verb_matches.get_one::<String>("id").map(String::as_str),
            Some("my-ws")
        );
        assert_eq!(verb_matches.get_one::<String>("name"), None);
        assert_eq!(verb_matches.get_one::<String>("path"), None);
    }

    /// `ext skill create --prompt` — the one group needing a third tree
    /// level (`ext` → `skill` → its own verbs), proven end to end through
    /// real parsing: the sub-group renders, its own verb binds its own
    /// binder, and `dispatch` resolves the composite `"skill.create"` verb
    /// key rather than stopping at the sub-group's own name.
    #[test]
    fn a_sub_nested_groups_verb_parses_and_dispatch_resolves_the_composite_verb() {
        let invocables = declared_invocables();
        let refs: Vec<&Invocable> = invocables.iter().filter(|i| i.group == "ext").collect();
        let (groups, _) = build_installation_tree(&refs);
        let tree = Command::new("cronus").subcommand(groups.into_iter().next().unwrap());

        let matches = tree
            .try_get_matches_from(["cronus", "ext", "skill", "create", "--prompt", "a skill"])
            .expect("ext skill create --prompt must parse");
        let (_, ext_matches) = matches.subcommand().unwrap();
        let (subgroup, skill_matches) = ext_matches.subcommand().unwrap();
        assert_eq!(subgroup, "skill");
        let (verb, verb_matches) = skill_matches.subcommand().unwrap();
        assert_eq!(verb, "create");
        assert_eq!(
            verb_matches.get_one::<String>("prompt").map(String::as_str),
            Some("a skill")
        );

        // `dispatch` itself resolves the same matches down to the leaf —
        // proven directly rather than only proving the parse tree shape.
        assert_eq!(
            dispatch(
                "ext",
                ext_matches,
                &Context::new(crate::output::OutputFormat::Json)
            ),
            0
        );
    }

    /// The literal Verify criterion this task names: the pre-composition
    /// parser and the post-composition catalog registration are two
    /// **consumers** of the same declaration, not two declarations — proven
    /// by comparing the verb set each one actually produces.
    #[test]
    fn the_parser_and_the_registered_catalog_advertise_exactly_the_same_verbs() {
        let invocables = declared_invocables();
        let invocable_refs: Vec<&Invocable> = invocables.iter().collect();

        let (groups, _) = build_installation_tree(&invocable_refs);
        let mut parser_verbs: Set<(String, String)> = Set::new();
        for group in &groups {
            if group.get_subcommands().count() == 0 {
                parser_verbs.insert((group.get_name().to_string(), group.get_name().to_string()));
                continue;
            }
            for verb in group.get_subcommands() {
                if verb.get_subcommands().count() == 0 {
                    parser_verbs
                        .insert((group.get_name().to_string(), verb.get_name().to_string()));
                } else {
                    // `verb` is actually a sub-group (`ext skill`) — one
                    // level deeper, composed the same way `verb_of` does
                    // on the catalog side (`"skill.import"`).
                    for leaf in verb.get_subcommands() {
                        parser_verbs.insert((
                            group.get_name().to_string(),
                            format!("{}.{}", verb.get_name(), leaf.get_name()),
                        ));
                    }
                }
            }
        }

        let mut registry = InvocableRegistry::new();
        for invocable in invocables.iter().cloned() {
            registry
                .register(&Registrant::core(), invocable)
                .expect("every declared installation invocable must register cleanly");
        }
        let catalog_verbs: Set<(String, String)> = registry
            .all()
            .filter(|invocable| matches!(invocable.locus, Locus::Installation))
            .map(|invocable| (invocable.group.to_string(), verb_of(invocable).to_string()))
            .collect();

        assert_eq!(
            parser_verbs, catalog_verbs,
            "the launcher parser and the registered catalog must advertise exactly the same \
             verbs — any difference means the two are drifting from separately maintained lists \
             rather than deriving from the one declaration"
        );
    }

    /// SDD reference containment: a requirement-clause id
    /// (a capitalized prefix, a hyphen and a number) resolves to nothing once
    /// `.design/` is absent from a release — printing one in a `--help`
    /// summary a user reads leaves dead, unexplained text behind. Scans
    /// every `summary` across both grammar halves (installation's own
    /// `declared_invocables`, plus the semantic half a real bootstrap
    /// registers) for the shape a citation takes: a parenthesized run of
    /// 2-4 uppercase letters, a hyphen, and digits.
    #[test]
    fn no_summary_in_either_grammar_half_cites_a_spec_layer_requirement_id() {
        fn citation_in(s: &str) -> Option<&str> {
            let bytes = s.as_bytes();
            let mut i = 0;
            while let Some(open) = s[i..].find('(') {
                let start = i + open + 1;
                let rest = &s[start..];
                let letters_end = rest
                    .find(|c: char| !c.is_ascii_uppercase())
                    .unwrap_or(rest.len());
                if (2..=4).contains(&letters_end)
                    && rest[letters_end..].starts_with('-')
                    && rest[letters_end + 1..]
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_digit())
                {
                    return Some(&rest[..letters_end]);
                }
                i = start;
                if i >= bytes.len() {
                    break;
                }
            }
            None
        }

        let mut offenders = Vec::new();
        for invocable in declared_invocables() {
            if let Some(id) = citation_in(invocable.summary) {
                offenders.push(format!("{} (installation): cites {id}", invocable.id));
            }
        }
        let (registry, _dispatcher) =
            cronus_core::invocable_bootstrap::bootstrap(cronus_core::Engine::new());
        for invocable in registry.all() {
            if let Some(id) = citation_in(invocable.summary) {
                offenders.push(format!("{} (semantic): cites {id}", invocable.id));
            }
        }
        assert!(
            offenders.is_empty(),
            "a summary must restate spec-layer rationale in plain language, never cite the \
             requirement id directly: {offenders:?}"
        );
    }

    // --- completion-script trimming (`strip_help_subcommand`) --------------

    /// A small tree, three levels deep, matching the real grammar's own
    /// shape (`cronus <group> <subgroup> <verb>`, e.g. `ext skill install`).
    fn three_level_tree() -> Command {
        Command::new("root").subcommand(
            Command::new("group").subcommand(
                Command::new("subgroup")
                    .subcommand(Command::new("verb").arg(clap::Arg::new("flag").long("flag"))),
            ),
        )
    }

    /// Baseline: clap injects a `help` subcommand at every node with
    /// children by default — the exact behavior that makes an un-stripped
    /// tree's completion script roughly double the size it needs to be.
    /// `Command::build()` is what `clap_complete::generate` (and
    /// real parsing) triggers internally to synthesize that subcommand —
    /// a fresh, unbuilt tree does not show it yet, so this test forces the
    /// same build pass a completion generator relies on. If a future clap
    /// upgrade ever changes this default, this test fails loudly rather
    /// than `strip_help_subcommand` silently becoming a no-op nobody
    /// notices.
    #[test]
    fn an_unstripped_tree_carries_the_help_subcommand_by_default() {
        let mut tree = three_level_tree();
        assert!(!tree.is_disable_help_subcommand_set());
        tree.build();
        assert!(tree.get_subcommands().any(|s| s.get_name() == "help"));
    }

    /// The actual fix: after `strip_help_subcommand`, every node at every
    /// depth has the setting flipped — not just the root, and not just one
    /// level down.
    #[test]
    fn strip_help_subcommand_disables_it_at_every_depth() {
        let mut stripped = strip_help_subcommand(three_level_tree());

        fn assert_disabled_recursively(command: &Command) {
            assert!(
                command.is_disable_help_subcommand_set(),
                "{:?} still has the help subcommand enabled",
                command.get_name()
            );
            for sub in command.get_subcommands() {
                assert_disabled_recursively(sub);
            }
        }
        assert_disabled_recursively(&stripped);

        // The real subcommand at each level must still be there — only the
        // `help` alias was removed, not real structure.
        assert!(stripped.get_subcommands().any(|s| s.get_name() == "group"));
        let group = stripped
            .get_subcommands()
            .find(|s| s.get_name() == "group")
            .expect("the real 'group' subcommand must survive stripping");
        assert!(
            group.get_subcommands().any(|s| s.get_name() == "subgroup"),
            "nested real subcommands must survive stripping too"
        );

        // The functional mirror of the baseline test: after the same
        // `build()` pass that made the baseline tree grow a `help` node,
        // the stripped tree must grow none, at any depth.
        stripped.build();
        fn assert_no_help_subcommand_recursively(command: &Command) {
            assert!(
                !command.get_subcommands().any(|s| s.get_name() == "help"),
                "{:?} still synthesized a help subcommand after build()",
                command.get_name()
            );
            for sub in command.get_subcommands() {
                assert_no_help_subcommand_recursively(sub);
            }
        }
        assert_no_help_subcommand_recursively(&stripped);
    }

    /// `disable_help_subcommand` is documented as leaving `-h`/`--help`
    /// untouched — proven here against a real parse rather than trusted
    /// from the doc comment alone, since this whole fix depends on it.
    #[test]
    fn strip_help_subcommand_leaves_the_help_flag_working() {
        let mut stripped = strip_help_subcommand(three_level_tree());
        let err = stripped
            .try_get_matches_from_mut(["root", "--help"])
            .expect_err("--help always short-circuits into a DisplayHelp error");
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
    }

    /// `emit_completion` must never trim the caller's own tree — only a
    /// clone. A regression here would strip `help` out of the live
    /// `Command` `main.rs` also uses for `--help`/usage errors, which
    /// nothing in this crate's other tests would catch (none of them call
    /// `emit_completion` directly).
    #[test]
    fn emit_completion_never_mutates_the_callers_command_tree() {
        let mut tree = three_level_tree();
        assert!(!tree.is_disable_help_subcommand_set());

        let matches = Command::new("cronus")
            .arg(clap::Arg::new("shell").required(true))
            .try_get_matches_from(["cronus", "bash"])
            .expect("well-formed test args");
        // `emit_completion` writes to stdout as a side effect; the return
        // code and stdout content are not under test here, only whether
        // `tree` itself was left untouched.
        let _ = emit_completion(&matches, &mut tree);

        assert!(
            !tree.is_disable_help_subcommand_set(),
            "emit_completion must generate from a clone, never mutate the caller's tree"
        );
    }
}
