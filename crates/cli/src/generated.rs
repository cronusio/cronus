//! The semantic half of the command surface (the split between what acts on
//! the user's work and what configures the product itself): a
//! `clap::Command` tree built at startup from the registry's `Semantic`,
//! `Shipped` descriptors — never a compile-time enum. Help, completion, and
//! grouping for this half all derive from the same descriptors; there is
//! nothing here to restate by hand, and a verb an extension contributes
//! becomes reachable the moment it registers, without a rebuild.
//!
//! The installation half (verbs that configure the product itself rather
//! than act on the user's work) stays a hand-declared, compile-time `clap`
//! grammar — a launcher must be answerable before this generated half can
//! even exist, since it is built from a registry that only comes alive
//! after composition.
//!
//! # Positional-only, except `Flag`
//!
//! Every `Text`/`Integer`/`Boolean` binder becomes a positional argument,
//! bound in declared order. The project's own CLI grammar also uses named
//! `--property` flags (`set <id> --actor <name>`), but `Binder` has no
//! positional-vs-named-flag distinction yet — every migrated verb so far
//! (the `memory` group) only ever needed positionals, so this gap is real
//! but unforced. A verb whose old shape used a named *value* flag
//! (`--actor cli`) is not migrated by this pass; resolving the distinction
//! is follow-up work, not invented here under the pressure of one group
//! that does not need it.
//!
//! `BinderKind::Flag` is the one exception, bound as `--name` rather than
//! positionally — a flag typed as a bare positional word ("type `presets`
//! to enable it") is not a shape any real CLI flag takes, so this one kind
//! does not wait on the general positional-vs-flag design question.

use std::collections::{HashMap, HashSet};

use clap::{Arg, ArgAction, ArgMatches, Command};
use cronus_contract::{ArgValue, ArgValues, Binder, BinderKind, Invocable, Locus, Stability};
use cronus_core::invocable::InvocableRegistry;

/// Every descriptor this generated half is responsible for: reachable from
/// every surface (`Semantic`) and currently on the default listing
/// (`Shipped`) — a `Retired` descriptor keeps resolving but does
/// not appear here, and neither does anything `ClientLocal`/`HostOnly`/
/// `Installation`, which this frontend does not project at all or projects
/// through its own hand-declared grammar instead.
pub fn semantic_shipped(registry: &InvocableRegistry) -> Vec<&Invocable> {
    let mut invocables: Vec<&Invocable> = registry
        .all()
        .filter(|invocable| {
            matches!(invocable.locus, Locus::Semantic)
                && matches!(invocable.stability, Stability::Shipped)
        })
        .collect();
    // Deterministic order: grouped, then alphabetical within a
    // group, so `--help` output and iteration order never depend on
    // registration order.
    invocables.sort_by(|a, b| (a.group, a.id.as_str()).cmp(&(b.group, b.id.as_str())));
    invocables
}

/// The verb a descriptor exposes within its own group: its id's tail with
/// the `"<group>."` prefix stripped. `invocable.group` is the declared
/// field, not re-derived from the id — the tail is only ever split to
/// recover the part after it.
///
/// `pub(crate)`: the installation half's own tree builder shares this exact
/// rule (a flat, single-verb group's one verb has a tail equal to its own
/// group name) rather than restating it.
pub(crate) fn verb_of(invocable: &Invocable) -> &str {
    let tail = invocable.id.tail();
    tail.strip_prefix(invocable.group)
        .and_then(|rest| rest.strip_prefix('.'))
        .unwrap_or(tail)
}

/// One-line `--help` text for a declared argument, keyed on the invocable's
/// id and the binder's name. Descriptors carry a `summary` per verb but no
/// per-argument text; a hand-kept map, mirroring `semantic_group_about` and
/// the installation half's `group_about`. An unlisted `(id, name)` renders
/// with just the argument name, as before.
pub(crate) fn binder_help(invocable_id: &str, binder_name: &str) -> Option<&'static str> {
    let tail = invocable_id.strip_prefix("core:").unwrap_or(invocable_id);
    Some(match (tail, binder_name) {
        ("board.show" | "board.move" | "board.block" | "board.done", "id") => "card id",
        ("board.list", "archived") => "list archived cards instead of live ones",
        ("board.add", "id") => "id to give the new card",
        ("board.add", "task_ref") => "task reference the card tracks",
        ("board.move", "state") => "target state",
        ("board.move" | "board.done", "actor") => "who is making the move (default: cli)",
        ("board.block", "reason") => "why the card is blocked",
        ("budget.set", "limit") => "monthly limit in USD",
        ("check.run" | "check.show" | "check.history", "card_id") => "card id",
        ("check.run", "path") => "path to check instead of the card's own tree",
        ("codegraph.index", "path") => "directory or file to index",
        ("codegraph.search" | "memory.search", "query") => "keyword query",
        ("exec.create", "ws_id") => "workspace id",
        ("exec.create", "card_id") => "card the execution workspace is for",
        ("exec.finalize" | "exec.discard", "id") => "execution workspace id",
        ("knowledge.collection-create", "id") => "collection id",
        ("knowledge.collection-create" | "knowledge.add" | "knowledge.add-url", "name") => {
            "human-readable name"
        }
        ("knowledge.add" | "knowledge.add-url" | "knowledge.query", "collection") => {
            "collection id (repeatable for query)"
        }
        ("knowledge.add", "id") => "id for the ingested record",
        ("knowledge.add", "text") => "record body",
        ("knowledge.add-url", "id") => "id for the ingested page",
        ("knowledge.add-url", "url") => "http:// URL to ingest",
        ("knowledge.query", "text") => "the question to retrieve for",
        ("knowledge.query", "top-k") => "max results to return",
        ("learn.approve" | "learn.reject", "id") => "skill-proposal id",
        ("loop.run", "file") => "unit file the oracle watches for",
        ("loop.run", "max-iter") => "iteration ceiling",
        ("loop.evolve", "harness_id") => "harness to evolve",
        ("loop.log" | "loop.show", "run_id") => "loop run id",
        ("memory.store", "key") => "entry key",
        ("memory.store", "value") => "entry value",
        ("memory.forget", "id") => "memory entry id",
        ("role.list", "presets") => "list the preset catalog instead of hired instances",
        ("role.hire", "preset") => "preset role id to hire",
        ("role.hire", "name") => "custom name for the instance (default: <preset>-<n>)",
        ("role.show" | "role.fire", "id") => "hired instance id",
        ("role.create", "id") => "id for the custom role",
        ("role.create", "display_name") => "display name for the custom role",
        ("schedule.add", "id") => "schedule id",
        ("schedule.add" | "schedule.run", "name") => "schedule name",
        ("schedule.add", "preset") => "schedule preset",
        ("schedule.delete" | "schedule.run", "id") => "schedule id",
        ("schedule.run", "prompt") => "prompt to run the schedule with",
        ("workflow.scaffold", "name") => "workflow name (or a path ending in .nodus)",
        ("workflow.scaffold", "out") => "output path (defaults to <name>.nodus)",
        ("workflow.validate" | "workflow.run" | "workflow.transpile", "file") => "workflow file",
        ("workflow.run", "input") => "input JSON for the workflow",
        ("workflow.transpile", "human") => "emit the human-readable form",
        ("workflow.transpile", "compact") => "emit the compact form",
        // Installation half.
        ("init", "path") => "directory to initialise (default: current)",
        ("doctor", "fix") => "apply safe repairs, not just report",
        ("restore", "backup") => "backup id to restore",
        (
            "workspace.create" | "workspace.switch" | "workspace.delete" | "workspace.check",
            "id",
        ) => "workspace id",
        ("workspace.create", "name") => "display name",
        ("workspace.create", "path") => "workspace directory (default: derived from id)",
        ("backup.create", "to") => "destination path for the archive",
        ("backup.create", "include-logs") => "include the logs tier in the backup",
        ("activation.enable", "mode") => "login or system",
        ("activation.enable", "acknowledge-unattended-execution") => {
            "confirm unattended execution (required non-interactively)"
        }
        ("archetype.list", "catalog") => "show the shipped catalog",
        ("archetype.list", "active") => "show the office's active archetype",
        ("archetype.info" | "archetype.set", "id") => "archetype id",
        ("archetype.info", "deviations") => "also report prior-vs-observed deviations",
        ("archetype.set", "clear") => "return to the archetype-free default",
        ("archetype.create", "name") => "name for the custom archetype",
        ("archetype.create", "from") => "preset to copy",
        ("registry.show" | "registry.disable" | "registry.enable", "name") => "agent name",
        ("registry.create", "name") => "name for the custom agent",
        ("registry.create", "description") => "what the agent does",
        ("ext.add" | "ext.scan" | "ext.skill.import", "path") => "manifest / package path",
        ("ext.remove" | "ext.activate" | "ext.deactivate", "id") => "extension id",
        ("ext.activate", "yes") => "confirm the grant (required non-interactively)",
        ("ext.skill.create", "prompt") => "natural-language description of the skill",
        ("ext.skill.status", "id") => "skill id (omit for all)",
        ("completion", "shell") => "bash, zsh, fish, powershell, or elvish",
        _ => return None,
    })
}

/// Build one `clap::Arg` for `binder`, bound positionally in declared order
/// (see the module doc on why every binder is positional for now), with an
/// optional one-line `--help` string for the argument.
///
/// `pub(crate)`: the installation half's own tree builder binds the same
/// `Binder` kinds the same way — one mapping, not two.
pub(crate) fn arg_for_with_help(binder: &Binder, help: Option<&'static str>) -> Arg {
    let arg = Arg::new(binder.name);
    let arg = match help {
        Some(h) => arg.help(h),
        None => arg,
    };
    match binder.kind {
        BinderKind::Text => arg.required(!binder.optional),
        // A closed set of positional values: clap lists them in `--help` and
        // rejects anything else as a usage error (exit 2) before dispatch.
        BinderKind::EnumText(values) => arg
            .required(!binder.optional)
            .value_parser(clap::builder::PossibleValuesParser::new(values)),
        BinderKind::Integer => arg
            .required(!binder.optional)
            .value_parser(clap::value_parser!(i64)),
        BinderKind::Boolean => arg
            .required(!binder.optional)
            .value_parser(clap::value_parser!(bool)),
        // A flag is never positional — "type the word `presets` to enable
        // it" is not a shape any real CLI flag takes — so this is the one
        // binder kind bound as `--name` rather than by position, and it is
        // never required (its absence simply means false).
        BinderKind::Flag => arg
            .long(binder.name)
            .required(false)
            .action(ArgAction::SetTrue),
        // A named text value — `--actor cli`, `--mode login` — bound the
        // same way `Flag` already is (by its own name, not by position),
        // but carrying a value rather than a presence bit.
        BinderKind::NamedText => arg.long(binder.name).required(!binder.optional),
        BinderKind::Float => arg
            .required(!binder.optional)
            .value_parser(clap::value_parser!(f64)),
        // Repeatable: `--collection a --collection b` accumulates, rather
        // than each occurrence overwriting the last — the one binder kind
        // whose action is `Append` instead of `Set`.
        BinderKind::RepeatableNamedText => arg
            .long(binder.name)
            .required(!binder.optional)
            .action(ArgAction::Append),
    }
}

/// Build the clap subcommand tree for the semantic half: one top-level
/// subcommand per declared `group`, one verb-level subcommand per
/// descriptor within it, its positional args built from declared binders.
/// Returns the tree alongside the set of group names it owns, so a caller
/// can tell a registry-generated group apart from one still on the old
/// hand-declared enum without re-deriving the same set twice.
/// A one-line description for a semantic group's top-level `--help` entry.
/// A hand-kept map, mirroring the installation half's own `group_about` — the
/// registry carries per-verb summaries but no group-level text, and
/// `"<group> operations"` reads as a placeholder. An unlisted group (a new
/// core group, an extension's) falls back to that generic form.
fn semantic_group_about(group: &str) -> String {
    match group {
        "memory" => "Store, search, and forget key-value memory entries",
        "codegraph" => "Index and query the code graph",
        "agent" => "Inspect the running agent: identity files and session status",
        "role" => "Hire, fire, and inspect role instances from the preset catalog",
        "exec" => "Manage execution workspaces for cards",
        "check" => "Run and inspect quality gates for a card",
        "learn" => "Review pending skill proposals",
        "board" => "Kanban board: add, move, block, and archive cards",
        "schedule" => "Recurring schedules: add, list, run, delete",
        "budget" => "Workspace budget: show usage, set a limit, reset counters",
        "loop" => "Run execution/evolution loops and inspect their ledgers",
        "workflow" => "Author, validate, transpile, and run workflow files",
        "knowledge" => "Ingest documents and run hybrid retrieval over collections",
        _ => return format!("{group} operations"),
    }
    .to_string()
}

pub fn build_semantic_tree(invocables: &[&Invocable]) -> (Vec<Command>, HashSet<String>) {
    let mut by_group: HashMap<&str, Vec<&Invocable>> = HashMap::new();
    for invocable in invocables {
        by_group.entry(invocable.group).or_default().push(invocable);
    }

    let mut group_names = HashSet::new();
    let mut groups = Vec::new();
    let mut group_order: Vec<&str> = by_group.keys().copied().collect();
    group_order.sort_unstable();

    for group in group_order {
        let members = &by_group[group];
        // A group with no verb is a usage failure, not an application failure:
        // clap answers it with the group's own help and exit code 2, the same
        // as an unknown verb — never reaching dispatch, so the "matched a
        // subcommand neither half owns" internal-error path in `main` becomes
        // genuinely unreachable for this shape.
        let mut verb_command = Command::new(group.to_string())
            .about(semantic_group_about(group))
            .subcommand_required(true)
            .arg_required_else_help(true);
        for invocable in members.iter() {
            let mut verb = Command::new(verb_of(invocable).to_string()).about(invocable.summary);
            for binder in &invocable.binders {
                verb = verb.arg(arg_for_with_help(
                    binder,
                    binder_help(invocable.id.as_str(), binder.name),
                ));
            }
            verb_command = verb_command.subcommand(verb);
        }
        group_names.insert(group.to_string());
        groups.push(verb_command);
    }

    (groups, group_names)
}

/// Whether `matches`' matched top-level subcommand names a registry-
/// generated group — the caller's cue to route through the registry
/// instead of falling back to the hand-declared enum.
pub fn matched_generated_group<'a>(
    matches: &'a ArgMatches,
    group_names: &HashSet<String>,
) -> Option<(&'a str, &'a ArgMatches)> {
    let (name, sub_matches) = matches.subcommand()?;
    if group_names.contains(name) {
        Some((name, sub_matches))
    } else {
        None
    }
}

/// Resolve the matched group's verb subcommand against `invocables`,
/// returning the invocable it names and the bound arguments — `None` when
/// the group matched but no verb subcommand was actually given (clap's own
/// `--help`/usage handling covers that case before this is ever reached in
/// practice).
///
/// Scoped to `group` deliberately: verb names are unique only *within* a
/// group (the whole reason the tree nests verbs under their group in the
/// first place), not across the full descriptor set — searching the
/// unscoped set let the first alphabetically-sorted group holding a
/// same-named verb silently win over the one the user actually typed
/// (`role list` resolving to `core:exec.list` the moment a second group
/// also declared a `list` verb). A regression test below proves this with
/// two groups sharing a verb name.
pub fn invocation_from_matches<'a>(
    group: &str,
    group_matches: &ArgMatches,
    invocables: &'a [&'a Invocable],
) -> Option<(&'a Invocable, ArgValues)> {
    let (verb_name, verb_matches) = group_matches.subcommand()?;
    let invocable = invocables
        .iter()
        .find(|invocable| invocable.group == group && verb_of(invocable) == verb_name)?;

    let mut args = ArgValues::new();
    for binder in &invocable.binders {
        let value = match binder.kind {
            BinderKind::Text | BinderKind::EnumText(_) => verb_matches
                .get_one::<String>(binder.name)
                .map(|value| ArgValue::Text(value.clone())),
            BinderKind::Integer => verb_matches
                .get_one::<i64>(binder.name)
                .map(|value| ArgValue::Integer(*value)),
            BinderKind::Boolean => verb_matches
                .get_one::<bool>(binder.name)
                .map(|value| ArgValue::Boolean(*value)),
            BinderKind::Flag => verb_matches.get_flag(binder.name).then_some(ArgValue::Flag),
            BinderKind::NamedText => verb_matches
                .get_one::<String>(binder.name)
                .map(|value| ArgValue::Text(value.clone())),
            BinderKind::Float => verb_matches
                .get_one::<f64>(binder.name)
                .map(|value| ArgValue::Float(*value)),
            BinderKind::RepeatableNamedText => verb_matches
                .get_many::<String>(binder.name)
                .map(|values| ArgValue::List(values.cloned().collect())),
        };
        if let Some(value) = value {
            args.insert(binder.name, value);
        }
    }
    Some((invocable, args))
}

#[cfg(test)]
mod tests {
    use cronus_contract::{Invocable, InvocableId, Stability};

    use super::*;

    fn descriptor(id: &str, group: &'static str, binders: Vec<Binder>) -> Invocable {
        Invocable {
            id: InvocableId::new(id).expect("well-formed test id"),
            name: "Test",
            summary: "A test descriptor.",
            group,
            locus: Locus::Semantic,
            binders,
            stability: Stability::Shipped,
            journal_raw_input: true,
        }
    }

    #[test]
    fn verb_of_strips_the_group_prefix() {
        let d = descriptor("core:memory.store", "memory", Vec::new());
        assert_eq!(verb_of(&d), "store");
    }

    #[test]
    fn build_semantic_tree_groups_by_declared_group_and_reports_names() {
        let a = descriptor(
            "core:memory.store",
            "memory",
            vec![Binder {
                name: "key",
                kind: BinderKind::Text,
                optional: false,
            }],
        );
        let b = descriptor("core:memory.search", "memory", Vec::new());
        let invocables = vec![&a, &b];

        let (groups, names) = build_semantic_tree(&invocables);
        assert_eq!(names, HashSet::from(["memory".to_string()]));
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].get_name(), "memory");
        let verb_names: Vec<&str> = groups[0].get_subcommands().map(|c| c.get_name()).collect();
        assert_eq!(verb_names.len(), 2);
        assert!(verb_names.contains(&"store"));
        assert!(verb_names.contains(&"search"));
    }

    #[test]
    fn invocation_from_matches_binds_positional_text_args_in_declared_order() {
        let d = descriptor(
            "core:memory.store",
            "memory",
            vec![
                Binder {
                    name: "key",
                    kind: BinderKind::Text,
                    optional: false,
                },
                Binder {
                    name: "value",
                    kind: BinderKind::Text,
                    optional: false,
                },
            ],
        );
        let invocables = vec![&d];
        let (groups, _) = build_semantic_tree(&invocables);
        let tree = Command::new("cronus").subcommand(groups.into_iter().next().unwrap());

        let matches = tree
            .try_get_matches_from(["cronus", "memory", "store", "fact", "the sky is blue"])
            .expect("parse must succeed");
        let (_, group_matches) = matches.subcommand().unwrap();

        let (resolved, args) = invocation_from_matches("memory", group_matches, &invocables)
            .expect("store must resolve");
        assert_eq!(resolved.id.as_str(), "core:memory.store");
        assert_eq!(args.get("key"), Some(&ArgValue::Text("fact".to_string())));
        assert_eq!(
            args.get("value"),
            Some(&ArgValue::Text("the sky is blue".to_string()))
        );
    }

    #[test]
    fn a_flag_binder_is_bound_as_a_named_flag_never_a_positional_word() {
        let d = descriptor(
            "core:role.list",
            "role",
            vec![Binder {
                name: "presets",
                kind: BinderKind::Flag,
                optional: true,
            }],
        );
        let invocables = vec![&d];
        let (groups, _) = build_semantic_tree(&invocables);
        let tree = Command::new("cronus").subcommand(groups.into_iter().next().unwrap());

        // The flag's own name typed as a bare word must NOT satisfy it —
        // if it did, `arg_for` had silently regressed to positional binding.
        let bare_word = tree
            .clone()
            .try_get_matches_from(["cronus", "role", "list", "presets"]);
        assert!(
            bare_word.is_err(),
            "a Flag binder must not accept its own name as a positional argument"
        );

        let matches = tree
            .try_get_matches_from(["cronus", "role", "list", "--presets"])
            .expect("--presets must parse");
        let (_, group_matches) = matches.subcommand().unwrap();
        let (_, args) =
            invocation_from_matches("role", group_matches, &invocables).expect("list must resolve");
        assert_eq!(args.get("presets"), Some(&ArgValue::Flag));
    }

    /// A real bug this exact shape produced: two different groups each
    /// declaring a `list` verb, and typing `role list` silently resolved
    /// to the *other* group's `list` (the first one alphabetically) once
    /// `invocation_from_matches` searched the whole descriptor set instead
    /// of the matched group alone. `exec.list` takes no binders at all, so
    /// the wrong resolution was invisible except by its own dropped
    /// `--presets` flag — caught by comparing which invocable actually
    /// resolved, not merely that dispatch succeeded.
    #[test]
    fn invocation_from_matches_resolves_the_matched_groups_own_verb_not_a_same_named_one_elsewhere()
    {
        let exec_list = descriptor("core:exec.list", "exec", Vec::new());
        let role_list = descriptor(
            "core:role.list",
            "role",
            vec![Binder {
                name: "presets",
                kind: BinderKind::Flag,
                optional: true,
            }],
        );
        // Sorted the same way `semantic_shipped` sorts its output — "exec"
        // alphabetically precedes "role", which is exactly the ordering
        // that let the bug hide.
        let invocables = vec![&exec_list, &role_list];

        let (groups, _) = build_semantic_tree(&invocables);
        let mut tree = Command::new("cronus");
        for group in groups {
            tree = tree.subcommand(group);
        }

        let matches = tree
            .try_get_matches_from(["cronus", "role", "list", "--presets"])
            .expect("role list --presets must parse");
        let (matched_group, group_matches) = matches.subcommand().unwrap();
        let (resolved, args) = invocation_from_matches(matched_group, group_matches, &invocables)
            .expect("role list must resolve");

        assert_eq!(
            resolved.id.as_str(),
            "core:role.list",
            "must resolve role's own `list`, not exec's same-named one"
        );
        assert_eq!(args.get("presets"), Some(&ArgValue::Flag));
    }

    #[test]
    fn semantic_shipped_excludes_client_local_host_only_installation_and_retired() {
        let mut registry = InvocableRegistry::new();
        let semantic = descriptor("core:memory.store", "memory", Vec::new());
        let client_local = Invocable {
            locus: Locus::ClientLocal,
            ..descriptor("core:pane.focus", "pane", Vec::new())
        };
        let installation = Invocable {
            locus: Locus::Installation,
            ..descriptor("core:init", "init", Vec::new())
        };
        let retired = Invocable {
            stability: Stability::Retired {
                superseded_by: InvocableId::new("core:memory.store").expect("well-formed test id"),
            },
            ..descriptor("core:memory.old-store", "memory", Vec::new())
        };

        registry
            .register(
                &cronus_core::invocable::Registrant::core(),
                semantic.clone(),
            )
            .unwrap();
        registry
            .register(&cronus_core::invocable::Registrant::core(), client_local)
            .unwrap();
        registry
            .register(&cronus_core::invocable::Registrant::core(), installation)
            .unwrap();
        registry
            .register(&cronus_core::invocable::Registrant::core(), retired)
            .unwrap();

        let shipped = semantic_shipped(&registry);
        assert_eq!(shipped.len(), 1);
        assert_eq!(shipped[0].id, semantic.id);
    }
}
