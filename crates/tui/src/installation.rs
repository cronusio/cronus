//! Installation verbs in the command bar: the verbs that configure or inspect
//! the product rather than act on the user's work, run here beside the work —
//! never inside it, never as a turn.
//!
//! A line that resolves to one of these verbs is not addressed to any agent.
//! It is resolved against the shared registry under the command line's own
//! names (`/doctor`, `/backup list`, `/ext skill status`), classified by what
//! it would do to the composition running beside it, and its result is shown
//! as a result block — visibly distinct from the one-line feedback an
//! ordinary command leaves.
//!
//! What a verb may do from here depends on its live-effect class, decided for
//! the resolved invocation (`/doctor` inspects, `/doctor --fix` recomposes):
//!
//! - `Inspect` runs at once through the shared dispatcher.
//! - `Install` and `Recompose` are not carried out from here yet. Doing so
//!   needs this surface to ask for consent naming the exact resolved
//!   invocation and to know whether work it would disturb is running, and it
//!   has neither. The result says so and names the command-line invocation
//!   that does carry the verb out — the command line is the guaranteed path
//!   to every installation verb, and this surface never attempts what it
//!   cannot yet do safely.

use cronus_contract::{
    Apply, Dispatched, Invocable, InvocableId, Invocation, LiveEffect, LocusKind, Outcome,
    OutcomeValue, Resolved, Surface,
};
use cronus_core::installation::{command_line_invocation, resolved_effect};
use cronus_core::invocable::{Dispatcher, InvocableRegistry};

use crate::command::{SlashCommand, installation_exclusion};
use crate::dispatch::bind_args;
use crate::view::ResultBlock;

/// An installation verb a slash line resolved to, with how many of the
/// line's arguments named the verb rather than being its values.
struct Resolution<'a> {
    invocable: &'a Invocable,
    /// How many leading arguments spelled the verb (0 for a flat verb such as
    /// `/doctor`, 1 for `/backup list`, 2 for `/ext skill status`).
    verb_words: usize,
}

/// Resolve `command` to an offered installation verb, trying the longest
/// spelling first so `/ext skill status` is never read as `/ext skill` with a
/// stray value. `None` means the line names no installation verb this surface
/// offers — it is then an ordinary command, or ordinary input.
fn resolve<'a>(registry: &'a InvocableRegistry, command: &SlashCommand) -> Option<Resolution<'a>> {
    for verb_words in (0..=2usize.min(command.args.len())).rev() {
        let mut tail = command.verb.clone();
        for word in &command.args[..verb_words] {
            tail.push('.');
            tail.push_str(word);
        }
        let Ok(id) = InvocableId::new(format!("core:{tail}")) else {
            continue;
        };
        if let Resolved::Found(invocable) = registry.resolve(&id)
            && invocable.locus.kind() == LocusKind::Installation
            && installation_exclusion(&invocable.id).is_none()
        {
            return Some(Resolution {
                invocable,
                verb_words,
            });
        }
    }
    None
}

/// Whether `command` names an installation verb this surface offers.
pub fn is_installation_command(registry: &InvocableRegistry, command: &SlashCommand) -> bool {
    resolve(registry, command).is_some()
}

/// The effect class as a person reads it.
fn class_label(effect: LiveEffect) -> &'static str {
    match effect {
        LiveEffect::Inspect => "inspect",
        LiveEffect::Install => "install",
        LiveEffect::Recompose {
            apply: Apply::InPlace,
        } => "recompose, applied in place",
        LiveEffect::Recompose {
            apply: Apply::Relaunch,
        } => "recompose, applied by relaunch",
    }
}

fn spelled(command: &SlashCommand) -> String {
    let mut line = format!("/{}", command.verb);
    for word in &command.args {
        line.push(' ');
        line.push_str(word);
    }
    line
}

/// Run one installation line, if `command` is one. `None` leaves the line to
/// the ordinary command path.
pub fn run(
    registry: &InvocableRegistry,
    dispatcher: &Dispatcher,
    command: &SlashCommand,
) -> Option<ResultBlock> {
    let Resolution {
        invocable,
        verb_words,
    } = resolve(registry, command)?;
    let title = spelled(command);
    let block = |lines: Vec<String>| ResultBlock {
        title: title.clone(),
        lines,
    };

    let args = match bind_args(&invocable.binders, &command.args[verb_words..]) {
        Ok(args) => args,
        Err(rejection) => return Some(block(vec![render_rejection(&rejection)])),
    };

    // The workspace this composition runs around is not known to this surface
    // yet, so an invocation is never raised for targeting it; both classes
    // above `Inspect` are refused below, so the omission cannot let a
    // disturbing verb run.
    let effect = resolved_effect(invocable, &args, None).unwrap_or(LiveEffect::Inspect);
    if effect != LiveEffect::Inspect {
        return Some(block(vec![
            format!("not run from here — class: {}", class_label(effect)),
            "this surface cannot yet ask for consent to change the installation".to_string(),
            format!(
                "run `{}` from a shell",
                command_line_invocation(invocable, &args)
            ),
        ]));
    }

    let invocation = Invocation {
        id: invocable.id.clone(),
        args,
        caller: Surface::Tui,
    };
    Some(block(match dispatcher.dispatch(registry, &invocation) {
        Dispatched::Unknown => vec!["no longer in the catalog".to_string()],
        Dispatched::Ran(outcome) => outcome_lines(outcome),
    }))
}

fn render_rejection(rejection: &cronus_contract::Rejection) -> String {
    format!(
        "rejected: {} ({:?}) — {}",
        rejection.binder, rejection.mode, rejection.detail
    )
}

fn outcome_lines(outcome: Outcome) -> Vec<String> {
    match outcome {
        Outcome::Value(value) => value_lines(&value),
        Outcome::Rejected(rejection) => vec![render_rejection(&rejection)],
        Outcome::Unavailable { reason } => vec![format!("unavailable: {reason}")],
        Outcome::Stream(_) => {
            vec![
                "error: internal: a stream outcome has no renderer on this surface yet".to_string(),
            ]
        }
    }
}

/// A scalar value as one line of text; `None` for a compound value.
fn scalar(value: &OutcomeValue) -> Option<String> {
    match value {
        OutcomeValue::Empty => Some("(none)".to_string()),
        OutcomeValue::Text(text) => Some(text.replace('\n', " ")),
        OutcomeValue::Integer(n) => Some(n.to_string()),
        OutcomeValue::Float(n) => Some(n.to_string()),
        OutcomeValue::Boolean(b) => Some(b.to_string()),
        OutcomeValue::List(_) | OutcomeValue::Record(_) => None,
    }
}

/// A record whose every field is a scalar, as one `name: value  name: value`
/// line; `None` when any field is compound.
fn flat_record(fields: &[(String, OutcomeValue)]) -> Option<String> {
    fields
        .iter()
        .map(|(name, value)| scalar(value).map(|text| format!("{name}: {text}")))
        .collect::<Option<Vec<_>>>()
        .map(|parts| parts.join("  "))
}

/// An outcome value as the lines of an installation block: a record is one
/// `name: value` line per field, a list one line per element, a nested value
/// indented under its name. Text keeps its own line breaks.
pub fn value_lines(value: &OutcomeValue) -> Vec<String> {
    let mut lines = Vec::new();
    push_lines(&mut lines, 0, value);
    lines
}

fn push_lines(lines: &mut Vec<String>, indent: usize, value: &OutcomeValue) {
    let pad = " ".repeat(indent);
    match value {
        OutcomeValue::Text(text) => {
            lines.extend(text.lines().map(|line| format!("{pad}{line}")));
        }
        OutcomeValue::Empty
        | OutcomeValue::Integer(_)
        | OutcomeValue::Float(_)
        | OutcomeValue::Boolean(_) => {
            lines.extend(scalar(value).map(|text| format!("{pad}{text}")))
        }
        OutcomeValue::List(items) if items.is_empty() => lines.push(format!("{pad}(none)")),
        OutcomeValue::List(items) => {
            for item in items {
                match item {
                    OutcomeValue::Record(fields) => match flat_record(fields) {
                        Some(line) => lines.push(format!("{pad}{line}")),
                        None => {
                            lines.push(format!("{pad}-"));
                            push_lines(lines, indent + 2, item);
                        }
                    },
                    other => push_lines(lines, indent, other),
                }
            }
        }
        OutcomeValue::Record(fields) => {
            for (name, field) in fields {
                match scalar(field) {
                    Some(text) => lines.push(format!("{pad}{name}: {text}")),
                    None => {
                        lines.push(format!("{pad}{name}:"));
                        push_lines(lines, indent + 2, field);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use cronus_contract::{ArgValue, ArgValues, Binder, BinderKind, Locus, Stability};
    use cronus_core::invocable::Registrant;

    use super::*;

    fn command(line: &str) -> SlashCommand {
        crate::command::parse(line).expect("a well-formed slash line")
    }

    /// A registry holding the real installation declaration, with each
    /// handler replaced by one that records nothing and answers a fixed
    /// value — so resolution and gating are exercised against the real
    /// verbs without touching the machine's installation.
    fn composed() -> (InvocableRegistry, Dispatcher) {
        let mut registry = InvocableRegistry::new();
        let mut dispatcher = Dispatcher::new();
        for invocable in cronus_core::installation::declared_invocables() {
            let id = invocable.id.clone();
            registry
                .register(&Registrant::core(), invocable)
                .expect("declared verbs register cleanly");
            let answer = id.as_str().to_string();
            dispatcher.attach(
                id,
                Arc::new(move |args: &ArgValues| {
                    let doctor_fix = matches!(args.get("fix"), Some(ArgValue::Flag));
                    Outcome::Value(OutcomeValue::Text(format!("ran {answer} fix={doctor_fix}")))
                }),
            );
        }
        (registry, dispatcher)
    }

    #[test]
    fn a_flat_verb_resolves_with_no_verb_words() {
        let (registry, _) = composed();
        let resolution = resolve(&registry, &command("/doctor --fix")).expect("resolves");
        assert_eq!(resolution.invocable.id.as_str(), "core:doctor");
        assert_eq!(resolution.verb_words, 0);
    }

    #[test]
    fn a_nested_verb_consumes_one_word_and_a_sub_nested_verb_two() {
        let (registry, _) = composed();
        let nested = resolve(&registry, &command("/backup list")).expect("resolves");
        assert_eq!(nested.invocable.id.as_str(), "core:backup.list");
        assert_eq!(nested.verb_words, 1);

        let deep = resolve(&registry, &command("/ext skill status standup")).expect("resolves");
        assert_eq!(deep.invocable.id.as_str(), "core:ext.skill.status");
        assert_eq!(deep.verb_words, 2);
    }

    #[test]
    fn an_excluded_or_unknown_verb_is_not_an_installation_command() {
        let (registry, _) = composed();
        assert!(!is_installation_command(&registry, &command("/tui")));
        assert!(!is_installation_command(
            &registry,
            &command("/completion bash")
        ));
        assert!(!is_installation_command(&registry, &command("/board list")));
        assert!(!is_installation_command(
            &registry,
            &command("/backup frobnicate")
        ));
    }

    #[test]
    fn an_inspect_verb_runs_through_the_dispatcher_and_shows_its_result() {
        let (registry, dispatcher) = composed();
        let block = run(&registry, &dispatcher, &command("/backup list")).expect("is a command");
        assert_eq!(block.title, "/backup list");
        assert_eq!(
            block.lines,
            vec!["ran core:backup.list fix=false".to_string()]
        );
    }

    #[test]
    fn a_verb_that_changes_the_installation_is_refused_and_names_the_command_line() {
        let (registry, dispatcher) = composed();
        let block = run(&registry, &dispatcher, &command("/backup create --to D:/x"))
            .expect("is a command");
        assert!(
            block.lines[0].contains("class: install"),
            "{:?}",
            block.lines
        );
        assert!(
            block
                .lines
                .iter()
                .any(|line| line.contains("`cronus backup create --to D:/x`")),
            "{:?}",
            block.lines
        );
        assert!(
            !block.lines.iter().any(|line| line.starts_with("ran ")),
            "a refused verb must not have been dispatched: {:?}",
            block.lines
        );
    }

    /// The class is that of the resolved invocation: the same verb inspects
    /// without its repair flag and recomposes with it.
    #[test]
    fn the_repair_flag_raises_diagnostics_above_inspect() {
        let (registry, dispatcher) = composed();
        let plain = run(&registry, &dispatcher, &command("/doctor")).expect("is a command");
        assert_eq!(plain.lines, vec!["ran core:doctor fix=false".to_string()]);

        let repair = run(&registry, &dispatcher, &command("/doctor --fix")).expect("is a command");
        assert!(
            repair.lines[0].contains("recompose, applied by relaunch"),
            "{:?}",
            repair.lines
        );
        assert!(
            repair
                .lines
                .iter()
                .any(|l| l.contains("`cronus doctor --fix`"))
        );
    }

    #[test]
    fn a_binding_failure_is_a_rejection_naming_the_binder() {
        let (registry, dispatcher) = composed();
        let block = run(&registry, &dispatcher, &command("/status unexpected")).expect("command");
        assert!(block.lines[0].starts_with("rejected:"), "{:?}", block.lines);
    }

    #[test]
    fn records_lists_and_nested_values_become_lines() {
        let record = |fields: &[(&str, OutcomeValue)]| {
            OutcomeValue::Record(
                fields
                    .iter()
                    .map(|(name, value)| (name.to_string(), value.clone()))
                    .collect(),
            )
        };
        let text = |s: &str| OutcomeValue::Text(s.to_string());

        assert_eq!(
            value_lines(&record(&[
                ("workspace", text("main")),
                ("phase", text("ready"))
            ])),
            vec!["workspace: main", "phase: ready"]
        );
        assert_eq!(
            value_lines(&OutcomeValue::List(vec![
                record(&[("id", text("a")), ("active", OutcomeValue::Boolean(true))]),
                record(&[("id", text("b")), ("active", OutcomeValue::Boolean(false))]),
            ])),
            vec!["id: a  active: true", "id: b  active: false"]
        );
        assert_eq!(value_lines(&OutcomeValue::List(Vec::new())), vec!["(none)"]);
        assert_eq!(
            value_lines(&record(&[(
                "shipped",
                OutcomeValue::List(vec![record(&[("id", text("software"))])])
            )])),
            vec!["shipped:", "  id: software"]
        );
        assert_eq!(
            value_lines(&record(&[("active", OutcomeValue::Empty)])),
            vec!["active: (none)"]
        );
        assert_eq!(value_lines(&text("one\ntwo")), vec!["one", "two"]);
    }

    #[test]
    fn unavailable_and_rejected_outcomes_stay_distinguishable_from_values() {
        assert_eq!(
            outcome_lines(Outcome::Unavailable {
                reason: "no workspace".to_string()
            }),
            vec!["unavailable: no workspace"]
        );
        let lines = outcome_lines(Outcome::Rejected(cronus_contract::Rejection {
            binder: "id",
            mode: cronus_contract::RejectionMode::Absent,
            detail: "nothing supplied".to_string(),
        }));
        assert!(lines[0].starts_with("rejected: id"), "{lines:?}");
    }

    #[test]
    fn a_semantic_verb_is_never_resolved_as_an_installation_verb() {
        let mut registry = InvocableRegistry::new();
        registry
            .register(
                &Registrant::core(),
                Invocable {
                    id: InvocableId::new("core:board.list").expect("well-formed"),
                    name: "List",
                    summary: "List cards",
                    group: "board",
                    locus: Locus::Semantic,
                    binders: vec![Binder {
                        name: "x",
                        kind: BinderKind::Text,
                        optional: true,
                    }],
                    stability: Stability::Shipped,
                    journal_raw_input: true,
                },
            )
            .expect("registers");
        assert!(!is_installation_command(&registry, &command("/board list")));
    }
}
