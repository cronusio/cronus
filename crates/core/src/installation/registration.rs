//! Putting the installation verbs into a composition: every declared verb
//! registered through the one public door, each with a handler attached.
//!
//! A verb that only reads is carried out here, against the real installation,
//! through [`super::inspect`]. A verb that changes something is carried out
//! only where a surface can first collect consent for the exact resolved
//! invocation (and, for a recompose, know whether work it would disturb is
//! running): the ones that have such a handler are named by
//! [`super::carry_out::is_carried_out`], and the surface asks before it
//! dispatches them. Every other handler answers with the command-line
//! invocation that does carry the verb out — an outcome a person can act on,
//! never an attempt and never a silent success. Registering every
//! verb with a handler, rather than only the ones that run, is what keeps a
//! projected verb from being one that answers "no handler attached".

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cronus_contract::{ActivationMode, ArgValue, ArgValues, Invocable, Outcome};
use cronus_domain::invocable::{Dispatcher, Handler, InvocableRegistry, Registrant};

use super::invocation::command_line_invocation;
use super::{carry_out, declared_invocables, inspect};
use crate::activation_bootstrap::default_activation_registry;
use crate::paths::resolve_workspace_root;

/// Register every declared installation verb, and attach its handler.
///
/// An `expect` is correct here for the reason the rest of the bootstrap's
/// registrations make: a duplicate or malformed literal declaration is a bug
/// in this crate, never a condition of the caller.
pub fn register(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    for invocable in declared_invocables() {
        let id = invocable.id.clone();
        let handler = handler_for(&invocable);
        registry
            .register(&Registrant::core(), invocable)
            .expect("declared installation verbs register cleanly — a duplicate id is a bug");
        dispatcher.attach(id, handler);
    }
}

/// The answer for a verb this composition does not carry out itself: the
/// command line that does.
fn command_line_only(invocable: &Invocable) -> Handler {
    let invocable = invocable.clone();
    Arc::new(move |args: &ArgValues| Outcome::Unavailable {
        reason: format!(
            "not carried out from this surface — run `{}` from a shell",
            command_line_invocation(&invocable, args)
        ),
    })
}

/// A handler for a change that recomposes the office: it takes the work gate
/// for the verb's scope before it changes anything and holds it until the
/// change is done, so no surface can apply one over running work by dispatching
/// it without asking — the gate is below every surface's own check, which exists
/// to refuse early and to say why.
fn gated(
    tail: &'static str,
    change: impl Fn(&ArgValues) -> Outcome + Send + Sync + 'static,
) -> Handler {
    Arc::new(move |args| match super::gate::begin_for(tail) {
        Ok(_applying) => change(args),
        Err(refused) => Outcome::Unavailable {
            reason: format!("not applied — {refused}"),
        },
    })
}

fn text_arg(args: &ArgValues, name: &str) -> String {
    match args.get(name) {
        Some(ArgValue::Text(value)) => value.clone(),
        _ => String::new(),
    }
}

/// A named text value that was given and is not empty.
fn optional_text(args: &ArgValues, name: &str) -> Option<String> {
    match args.get(name) {
        Some(ArgValue::Text(value)) if !value.is_empty() => Some(value.clone()),
        _ => None,
    }
}

fn flag(args: &ArgValues, name: &str) -> bool {
    matches!(args.get(name), Some(ArgValue::Flag))
}

/// A path argument as the caller meant it: relative to where the caller
/// stands, which in an engine is not where the engine does.
fn path_arg(args: &ArgValues, name: &str) -> PathBuf {
    crate::paths::from_caller(Path::new(&text_arg(args, name)))
}

fn handler_for(invocable: &Invocable) -> Handler {
    match invocable.id.tail() {
        "status" => Arc::new(|_| inspect::status(&resolve_workspace_root())),
        "doctor" => {
            let repair = gated("doctor", |_| {
                carry_out::recompose::doctor_repair(&resolve_workspace_root())
            });
            Arc::new(move |args| {
                if flag(args, "fix") {
                    repair(args)
                } else {
                    inspect::doctor_check(&resolve_workspace_root())
                }
            })
        }
        "registry.enable" => gated("registry.enable", |args| {
            carry_out::recompose::registry_set_enabled_default(&text_arg(args, "name"), true)
        }),
        "registry.disable" => gated("registry.disable", |args| {
            carry_out::recompose::registry_set_enabled_default(&text_arg(args, "name"), false)
        }),
        "archetype.set" => gated("archetype.set", |args| {
            carry_out::recompose::archetype_set(
                &resolve_workspace_root(),
                optional_text(args, "id").as_deref(),
                flag(args, "clear"),
            )
        }),
        "ext.activate" => gated("ext.activate", |args| {
            carry_out::recompose::ext_activate_default(&text_arg(args, "id"), flag(args, "yes"))
        }),
        "ext.deactivate" => gated("ext.deactivate", |args| {
            carry_out::recompose::ext_deactivate_default(&text_arg(args, "id"))
        }),
        "restore" => gated("restore", |args| {
            let root = resolve_workspace_root();
            carry_out::recompose::restore(
                &root,
                &inspect::backups_dir(&root),
                &text_arg(args, "backup"),
            )
        }),
        "dev.admit" => gated("dev.admit", |_| carry_out::recompose::dev_admit()),
        "dev.revoke" => gated("dev.revoke", |_| carry_out::recompose::dev_revoke()),
        "backup.list" => {
            Arc::new(|_| inspect::backup_list(&inspect::backups_dir(&resolve_workspace_root())))
        }
        "workspace.list" => Arc::new(|_| inspect::workspace_list()),
        "workspace.check" => Arc::new(|args| inspect::workspace_check(&text_arg(args, "id"))),
        "registry.list" => Arc::new(|_| inspect::registry_list()),
        "registry.show" => Arc::new(|args| inspect::registry_show(&text_arg(args, "name"))),
        "ext.list" => Arc::new(|_| inspect::ext_list()),
        "ext.scan" => Arc::new(|args| inspect::ext_scan(&path_arg(args, "path"))),
        "activation.status" => Arc::new(|_| inspect::activation_status()),
        "archetype.list" => Arc::new(|args| {
            inspect::archetype_list(
                &resolve_workspace_root(),
                flag(args, "catalog"),
                flag(args, "active"),
            )
        }),
        "archetype.info" => Arc::new(|args| {
            inspect::archetype_info(&text_arg(args, "id"), flag(args, "deviations"))
        }),
        "dev.status" => Arc::new(|_| inspect::dev_status(&crate::paths::caller_dir())),
        "init" => Arc::new(|args| {
            let dir = optional_text(args, "path")
                .map(|path| crate::paths::from_caller(Path::new(&path)))
                .unwrap_or_else(crate::paths::caller_dir);
            // The workspace state lives in one `.cronus/` directory, never
            // scattered across the project's own files.
            carry_out::init(&dir.join(".cronus"))
        }),
        "workspace.switch" => Arc::new(|args| match inspect::open_workspace_manager() {
            Ok(manager) => carry_out::workspace_switch(&manager, &text_arg(args, "id")),
            Err(reason) => Outcome::Unavailable { reason },
        }),
        "workspace.delete" => {
            // Deleting the workspace the office runs around changes what is
            // running: that is a recompose, which a handler of the `Install`
            // class does not carry out. Whoever dispatches it unasked still
            // meets this.
            let recompose = command_line_only(invocable);
            let declared = invocable.clone();
            Arc::new(move |args| {
                let id = text_arg(args, "id");
                let running = match inspect::running_workspace() {
                    Ok(active) => active,
                    // Unknown counts as "this one": the mistake that can be
                    // made is to refuse too much, never too little.
                    Err(_) => Some(id.clone()),
                };
                if matches!(
                    super::resolved_effect(&declared, args, running.as_deref()),
                    Some(cronus_contract::LiveEffect::Recompose { .. })
                ) {
                    return recompose(args);
                }
                match inspect::open_workspace_manager() {
                    Ok(manager) => carry_out::workspace_delete(&manager, &id),
                    Err(reason) => Outcome::Unavailable { reason },
                }
            })
        }
        "activation.disable" => {
            let elevated = command_line_only(invocable);
            let id = invocable.id.clone();
            Arc::new(move |args| {
                if carry_out::needs_elevation_now(&id) {
                    return elevated(args);
                }
                carry_out::activation_disable(&default_activation_registry())
            })
        }
        "archetype.create" => Arc::new(|args| {
            carry_out::catalogs::archetype_create(
                &resolve_workspace_root(),
                &text_arg(args, "name"),
                &text_arg(args, "from"),
            )
        }),
        "registry.create" => Arc::new(|args| {
            carry_out::catalogs::registry_create_default(
                &text_arg(args, "name"),
                &text_arg(args, "description"),
            )
        }),
        "ext.add" => Arc::new(|args| carry_out::catalogs::ext_add_default(&path_arg(args, "path"))),
        "ext.skill.import" => {
            Arc::new(|args| carry_out::catalogs::skill_import_default(&path_arg(args, "path")))
        }
        "ext.skill.create" => {
            Arc::new(|args| carry_out::catalogs::skill_create(&text_arg(args, "prompt")))
        }
        "workspace.create" => Arc::new(|args| match inspect::open_workspace_manager() {
            Ok(manager) => carry_out::workspace_create(
                &manager,
                &text_arg(args, "id"),
                optional_text(args, "name").as_deref(),
                optional_text(args, "path")
                    .map(|path| crate::paths::from_caller(Path::new(&path)))
                    .as_deref(),
            ),
            Err(reason) => Outcome::Unavailable { reason },
        }),
        "activation.enable" => {
            // The system mode may need an elevation prompt, which a composed
            // surface never attempts: that one stays with the command line.
            let elevated = command_line_only(invocable);
            Arc::new(move |args| {
                let mode = text_arg(args, "mode");
                if carry_out::parse_mode(&mode) == Some(ActivationMode::System) {
                    return elevated(args);
                }
                carry_out::activation_enable(
                    &default_activation_registry(),
                    &mode,
                    flag(args, carry_out::UNATTENDED_FLAG),
                )
            })
        }
        "backup.create" => Arc::new(|args| {
            let root = resolve_workspace_root();
            let to = match args.get("to") {
                Some(ArgValue::Text(path)) if !path.is_empty() => {
                    Some(crate::paths::from_caller(Path::new(path)))
                }
                _ => None,
            };
            carry_out::backup_create(
                &root,
                &inspect::backups_dir(&root),
                to.as_deref(),
                flag(args, "include-logs"),
            )
        }),
        _ => command_line_only(invocable),
    }
}

#[cfg(test)]
mod tests {
    use cronus_contract::{Dispatched, Invocation, LiveEffect, Locus, Surface};

    use super::*;

    fn composed() -> (InvocableRegistry, Dispatcher) {
        let mut registry = InvocableRegistry::new();
        let mut dispatcher = Dispatcher::new();
        register(&mut registry, &mut dispatcher);
        (registry, dispatcher)
    }

    #[test]
    fn every_declared_verb_is_registered_and_has_a_handler() {
        let (registry, dispatcher) = composed();
        for invocable in declared_invocables() {
            assert!(
                registry.resolve(&invocable.id).is_found(),
                "{}",
                invocable.id
            );
            assert!(
                dispatcher.has_handler(&invocable.id),
                "{} is registered with no handler",
                invocable.id
            );
        }
    }

    fn dispatch(
        registry: &InvocableRegistry,
        dispatcher: &Dispatcher,
        tail: &str,
        args: ArgValues,
    ) -> Outcome {
        let invocation = Invocation {
            id: super::super::declared_invocables()
                .into_iter()
                .find(|i| i.id.tail() == tail)
                .map(|i| i.id)
                .unwrap_or_else(|| panic!("no declared verb {tail:?}")),
            args,
            caller: Surface::Tui,
        };
        match dispatcher.dispatch(registry, &invocation) {
            Dispatched::Ran(outcome) => outcome,
            Dispatched::Unknown => panic!("{tail} did not resolve"),
        }
    }

    /// A verb that changes something and is not carried out here answers
    /// with the command line that does it, and changes nothing.
    #[test]
    fn a_verb_that_changes_state_answers_with_the_command_line_that_carries_it_out() {
        let (registry, dispatcher) = composed();
        let mut args = ArgValues::new();
        // The system mode may need an elevation prompt, which a composed
        // surface never attempts.
        args.insert("mode", ArgValue::Text("system".to_string()));
        match dispatch(&registry, &dispatcher, "activation.enable", args) {
            Outcome::Unavailable { reason } => {
                assert!(
                    reason.contains("`cronus activation enable --mode system`"),
                    "{reason}"
                );
            }
            other => panic!("expected the command line to be named, got {other:?}"),
        }
    }

    #[test]
    fn every_verb_above_inspect_answers_with_a_command_line_and_not_a_missing_handler() {
        let (registry, dispatcher) = composed();
        for invocable in declared_invocables() {
            let above_inspect = matches!(
                invocable.locus,
                Locus::Installation { effect } if effect != LiveEffect::Inspect
            );
            if !above_inspect
                || carry_out::is_carried_out(&invocable.id, &ArgValues::new())
                || invocable.binders.iter().any(|b| !b.optional)
            {
                continue;
            }
            match dispatch(
                &registry,
                &dispatcher,
                invocable.id.tail(),
                ArgValues::new(),
            ) {
                Outcome::Unavailable { reason } => {
                    assert!(reason.contains("`cronus "), "{}: {reason}", invocable.id);
                    assert!(!reason.contains("no handler attached"), "{}", invocable.id);
                }
                other => panic!("{} unexpectedly ran: {other:?}", invocable.id),
            }
        }
    }
}
