//! Putting the installation verbs into a composition: every declared verb
//! registered through the one public door, each with a handler attached.
//!
//! A verb that only reads is carried out here, against the real installation,
//! through [`super::inspect`]. A verb that changes something is not carried
//! out by a handler on a composed surface yet: doing so needs the surface to
//! collect consent for the exact resolved invocation and to know whether work
//! it would disturb is running, and neither exists. Its handler answers with
//! the command-line invocation that does carry it out — an outcome a person
//! can act on, never an attempt and never a silent success. Registering every
//! verb with a handler, rather than only the ones that run, is what keeps a
//! projected verb from being one that answers "no handler attached".

use std::path::PathBuf;
use std::sync::Arc;

use cronus_contract::{ArgValue, ArgValues, Invocable, Outcome};
use cronus_domain::invocable::{Dispatcher, Handler, InvocableRegistry, Registrant};

use super::invocation::command_line_invocation;
use super::{declared_invocables, inspect};
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

fn text_arg(args: &ArgValues, name: &str) -> String {
    match args.get(name) {
        Some(ArgValue::Text(value)) => value.clone(),
        _ => String::new(),
    }
}

fn flag(args: &ArgValues, name: &str) -> bool {
    matches!(args.get(name), Some(ArgValue::Flag))
}

fn current_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn handler_for(invocable: &Invocable) -> Handler {
    match invocable.id.tail() {
        "status" => Arc::new(|_| inspect::status(&resolve_workspace_root())),
        "doctor" => {
            let repair = command_line_only(invocable);
            Arc::new(move |args| {
                if flag(args, "fix") {
                    repair(args)
                } else {
                    inspect::doctor_check(&resolve_workspace_root())
                }
            })
        }
        "backup.list" => {
            Arc::new(|_| inspect::backup_list(&inspect::backups_dir(&resolve_workspace_root())))
        }
        "workspace.list" => Arc::new(|_| inspect::workspace_list()),
        "workspace.check" => Arc::new(|args| inspect::workspace_check(&text_arg(args, "id"))),
        "registry.list" => Arc::new(|_| inspect::registry_list()),
        "registry.show" => Arc::new(|args| inspect::registry_show(&text_arg(args, "name"))),
        "ext.list" => Arc::new(|_| inspect::ext_list()),
        "ext.scan" => Arc::new(|args| inspect::ext_scan(&PathBuf::from(text_arg(args, "path")))),
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
        "dev.status" => Arc::new(|_| inspect::dev_status(&current_dir())),
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

    /// A verb that changes something answers with the command line that does
    /// it, and changes nothing here.
    #[test]
    fn a_verb_that_changes_state_answers_with_the_command_line_that_carries_it_out() {
        let (registry, dispatcher) = composed();
        let mut args = ArgValues::new();
        args.insert("to", ArgValue::Text("elsewhere".to_string()));
        match dispatch(&registry, &dispatcher, "backup.create", args) {
            Outcome::Unavailable { reason } => {
                assert!(
                    reason.contains("`cronus backup create --to elsewhere`"),
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
            if !above_inspect || invocable.binders.iter().any(|b| !b.optional) {
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

    #[test]
    fn diagnostics_with_repair_is_never_run_by_the_read_only_handler() {
        let (registry, dispatcher) = composed();
        let mut args = ArgValues::new();
        args.insert("fix", ArgValue::Flag);
        match dispatch(&registry, &dispatcher, "doctor", args) {
            Outcome::Unavailable { reason } => {
                assert!(reason.contains("`cronus doctor --fix`"), "{reason}");
            }
            other => panic!("expected the command line to be named, got {other:?}"),
        }
    }
}
