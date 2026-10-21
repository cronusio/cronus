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
//! - `Install` is carried out from here for the verbs the core carries out
//!   itself ([`cronus_core::installation::is_carried_out`]), and only after
//!   the person has been asked: the question names the exact resolved
//!   invocation, and the invocation that runs is the very one the question
//!   named, not a line parsed again. Any other `Install` verb is refused
//!   with the command-line invocation that does carry it out.
//! - `Recompose` is carried out from here for the verbs the core carries out
//!   itself, asked about in the same way, and only when the work it would alter
//!   is not running: that is asked of the operating system's work lock before
//!   the person is asked anything, never assumed, and a change that cannot be
//!   applied now is refused with what is running and is not queued. A change
//!   applied in place takes effect at once. One applied by relaunch is applied
//!   to the state on disk and then this frontend is rebuilt onto the same view
//!   — which only the process that owns the composition can do, so a frontend
//!   attached to another process's engine leaves those to the session that owns
//!   it. Any other `Recompose` verb is refused with the command-line
//!   invocation that does carry it out.

use cronus_contract::{
    Apply, ArgValue, Dispatched, Invocable, InvocableId, Invocation, LiveEffect, LocusKind,
    Outcome, OutcomeValue, Resolved, Surface,
};
use cronus_core::installation::carry_out::{acknowledgement, disclosure, needs_elevation_now};
use cronus_core::installation::gate::{self, Refused};
use cronus_core::installation::inspect::running_workspace;
use cronus_core::installation::{
    blocked_reason, command_line_invocation, is_carried_out_as, needs_relaunch, resolved_effect,
};
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

/// What an installation line asks of the loop.
#[derive(Debug)]
pub enum Step {
    /// Answered: shown as it is.
    Done(ResultBlock),
    /// Carrying it out changes the installation and this surface can: the
    /// person is asked first.
    Ask(Question),
}

/// Where this frontend stands in relation to the engine of its state root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// This process is the engine: it owns the composition, so a change that
    /// is applied by relaunch is one it can relaunch onto.
    Engine,
    /// Another process is the engine and this frontend is attached to it. A
    /// change applied by relaunch needs the engine's process to relaunch,
    /// which only that process can do.
    Spoke,
}

/// What `prepare` needs to know about the place it runs in: the placement, and
/// how to ask whether a change could be applied now.
#[derive(Clone, Copy)]
pub struct Setting {
    placement: Placement,
    may_apply: fn(&str) -> Result<(), Refused>,
}

impl Setting {
    /// The real setting: changes are asked of the operating system's work lock.
    pub fn live(placement: Placement) -> Self {
        Self {
            placement,
            may_apply: gate::check_for,
        }
    }

    /// Where this frontend stands.
    pub fn placement(&self) -> Placement {
        self.placement
    }

    /// A setting whose gate answers as the test says, so the tests of how a
    /// line is taken do not depend on the machine's work lock.
    #[cfg(test)]
    pub(crate) fn answering(
        placement: Placement,
        may_apply: fn(&str) -> Result<(), Refused>,
    ) -> Self {
        Self {
            placement,
            may_apply,
        }
    }
}

/// A change waiting for the person's answer: the block that asks, and the
/// exact invocation that runs if the answer is yes.
#[derive(Debug, Clone)]
pub struct Question {
    pub(crate) title: String,
    pub(crate) invocation: Invocation,
    pub(crate) block: ResultBlock,
    /// Applying it rebuilds this frontend once it has been applied.
    pub(crate) relaunches: bool,
}

impl Question {
    /// The block that puts the question to the person.
    pub fn block(&self) -> &ResultBlock {
        &self.block
    }
}

/// What carrying out an agreed change left.
#[derive(Debug)]
pub struct Carried {
    /// The answer, shown under the line that asked.
    pub block: ResultBlock,
    /// The change was applied and asks this frontend to be rebuilt onto the
    /// same view. False for every change applied in place, and for one that
    /// was not applied.
    pub relaunch: bool,
}

/// Carry out what the person agreed to: dispatch the invocation the question
/// named, and show its answer under the line that asked.
pub fn confirmed(
    registry: &InvocableRegistry,
    dispatcher: &Dispatcher,
    question: &Question,
) -> Carried {
    let (mut lines, changed) = match dispatcher.dispatch(registry, &question.invocation) {
        Dispatched::Unknown => (vec!["no longer in the catalog".to_string()], false),
        Dispatched::Ran(outcome) => {
            let changed = needs_relaunch(&question.invocation.id, &outcome);
            (outcome_lines(outcome), changed)
        }
    };
    let relaunch = question.relaunches && changed;
    if relaunch {
        lines.push("restarting onto the changed installation".to_string());
    }
    Carried {
        block: ResultBlock {
            title: question.title.clone(),
            lines,
        },
        relaunch,
    }
}

/// Take one installation line a step, if `command` is one: an inspection is
/// run and answered, a change this surface carries out becomes a question, and
/// anything else is refused with the command line that does it. `None` leaves
/// the line to the ordinary command path.
pub fn prepare(
    registry: &InvocableRegistry,
    dispatcher: &Dispatcher,
    command: &SlashCommand,
    setting: &Setting,
) -> Option<Step> {
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
        Err(rejection) => return Some(Step::Done(block(vec![render_rejection(&rejection)]))),
    };

    // Deleting the workspace the office runs around changes what is running,
    // so it is raised above `Install`. This surface is not built around a
    // registry workspace of its own; it treats the registry's active one as
    // the running one. A registry it cannot read is "unknown", which it treats
    // as "this one": the mistake that can be made is to refuse too much, never
    // too little.
    let running = if invocable.id.tail() == "workspace.delete" {
        match running_workspace() {
            Ok(active) => active,
            Err(_) => match args.get("id") {
                Some(ArgValue::Text(id)) => Some(id.clone()),
                _ => None,
            },
        }
    } else {
        None
    };
    let effect =
        resolved_effect(invocable, &args, running.as_deref()).unwrap_or(LiveEffect::Inspect);
    let mut invocation = Invocation {
        id: invocable.id.clone(),
        args,
        caller: Surface::Tui,
    };

    // A change nobody can carry out in this build is not asked about: the
    // person is told why instead of being invited to agree to a refusal.
    if let Some(reason) = blocked_reason(&invocable.id) {
        return Some(Step::Done(block(vec![
            "not run — nothing can carry this out in this build".to_string(),
            reason.to_string(),
        ])));
    }
    if effect == LiveEffect::Install
        && is_carried_out_as(&invocable.id, &invocation.args, effect)
        && !needs_elevation_now(&invocable.id)
    {
        // What the person must be shown comes from the verb, not from here.
        let shown = disclosure(&invocable.id, &invocation.args);
        // Agreeing is what supplies the acknowledgement the verb's
        // non-interactive form requires, so it is part of the invocation the
        // question names — the person is never asked about one thing and made
        // to grant another.
        if let Some(flag) = acknowledgement(&invocable.id) {
            invocation.args.insert(flag, ArgValue::Flag);
        }
        let line = command_line_invocation(invocable, &invocation.args);
        let mut lines = vec![format!(
            "class: {} — this changes the installation",
            class_label(effect)
        )];
        lines.extend(shown);
        lines.push(format!("will run: {line}"));
        lines.push("y or Enter to run it · n or Esc to leave it undone".to_string());
        return Some(Step::Ask(Question {
            title: title.clone(),
            block: ResultBlock {
                title: format!("confirm {title}"),
                lines,
            },
            invocation,
            relaunches: false,
        }));
    }
    if let LiveEffect::Recompose { apply } = effect
        && is_carried_out_as(&invocable.id, &invocation.args, effect)
    {
        return Some(recompose_step(
            invocable, command, invocation, apply, setting, &title,
        ));
    }
    let line = command_line_invocation(invocable, &invocation.args);
    if effect != LiveEffect::Inspect {
        let why = match invocable.id.tail() {
            "activation.enable" | "activation.disable" => {
                "system-wide activation may need an elevation prompt, which this surface never attempts"
            }
            "workspace.delete" if effect != LiveEffect::Install => {
                "this is the workspace the office runs around, so deleting it changes what is running"
            }
            _ => "this surface does not carry this change out yet",
        };
        return Some(Step::Done(block(vec![
            format!("not run from here — class: {}", class_label(effect)),
            why.to_string(),
            format!("run `{line}` from a shell"),
        ])));
    }

    Some(Step::Done(block(
        match dispatcher.dispatch(registry, &invocation) {
            Dispatched::Unknown => vec!["no longer in the catalog".to_string()],
            Dispatched::Ran(outcome) => outcome_lines(outcome),
        },
    )))
}

/// A change that recomposes the office, taken a step: refused where it cannot
/// be applied from here or now, otherwise put to the person.
///
/// The refusals come first, so nobody is asked to agree to something that can
/// only be refused: a relaunch from a frontend that is not the engine, and work
/// that is running. Neither is assumed away; the second is asked of the
/// operating system's work lock.
fn recompose_step(
    invocable: &Invocable,
    command: &SlashCommand,
    mut invocation: Invocation,
    apply: Apply,
    setting: &Setting,
    title: &str,
) -> Step {
    let effect = LiveEffect::Recompose { apply };
    let block = |lines: Vec<String>| ResultBlock {
        title: title.to_string(),
        lines,
    };

    if apply == Apply::Relaunch && setting.placement == Placement::Spoke {
        return Step::Done(block(vec![
            format!("not run from here — class: {}", class_label(effect)),
            "this terminal UI is attached to the engine of another session, and a change \
             applied by relaunch needs the session that owns the engine to relaunch"
                .to_string(),
            format!("run `{}` in that session", spelled(command)),
        ]));
    }
    if let Err(refused) = (setting.may_apply)(invocable.id.tail()) {
        let nothing = match refused {
            Refused::Busy(_) => "nothing was changed; run it again once that has finished",
            Refused::Unknown(_) => "nothing was changed, and nothing was assumed",
        };
        return Step::Done(block(vec![
            format!("not applied — {refused}"),
            nothing.to_string(),
        ]));
    }

    let shown = disclosure(&invocable.id, &invocation.args);
    if let Some(flag) = acknowledgement(&invocable.id) {
        invocation.args.insert(flag, ArgValue::Flag);
    }
    let line = command_line_invocation(invocable, &invocation.args);
    let consequence = match apply {
        Apply::InPlace => "this changes what the office is built from, and takes effect at once",
        Apply::Relaunch => {
            "this changes what the office is built from, and this screen restarts onto \
             the same view once it is applied"
        }
    };
    let mut lines = vec![format!("class: {} — {consequence}", class_label(effect))];
    lines.extend(shown);
    lines.push(format!("will run: {line}"));
    lines.push("y or Enter to run it · n or Esc to leave it undone".to_string());
    Step::Ask(Question {
        title: title.to_string(),
        block: ResultBlock {
            title: format!("confirm {title}"),
            lines,
        },
        invocation,
        relaunches: apply == Apply::Relaunch,
    })
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

    /// The engine of an office with nothing running.
    fn engine() -> Setting {
        Setting::answering(Placement::Engine, |_| Ok(()))
    }

    /// The block an installation line is answered with — failing the test if
    /// the line asked a question instead.
    fn answered(registry: &InvocableRegistry, dispatcher: &Dispatcher, line: &str) -> ResultBlock {
        answered_in(registry, dispatcher, line, &engine())
    }

    fn answered_in(
        registry: &InvocableRegistry,
        dispatcher: &Dispatcher,
        line: &str,
        setting: &Setting,
    ) -> ResultBlock {
        match prepare(registry, dispatcher, &command(line), setting).expect("is a command") {
            Step::Done(block) => block,
            Step::Ask(question) => panic!("{line} asked: {:?}", question.block().lines),
        }
    }

    /// The question an installation line puts to the person — failing the test
    /// if the line was answered instead.
    fn asked(registry: &InvocableRegistry, dispatcher: &Dispatcher, line: &str) -> Question {
        asked_in(registry, dispatcher, line, &engine())
    }

    fn asked_in(
        registry: &InvocableRegistry,
        dispatcher: &Dispatcher,
        line: &str,
        setting: &Setting,
    ) -> Question {
        match prepare(registry, dispatcher, &command(line), setting).expect("is a command") {
            Step::Ask(question) => question,
            Step::Done(block) => panic!("{line} was answered: {:?}", block.lines),
        }
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
        let block = answered(&registry, &dispatcher, "/backup list");
        assert_eq!(block.title, "/backup list");
        assert_eq!(
            block.lines,
            vec!["ran core:backup.list fix=false".to_string()]
        );
    }

    #[test]
    fn a_change_this_surface_carries_out_is_asked_about_and_not_yet_run() {
        let (registry, dispatcher) = composed();
        let question = asked(&registry, &dispatcher, "/backup create --to D:/x");
        let lines = &question.block().lines;
        assert_eq!(question.block().title, "confirm /backup create --to D:/x");
        assert!(lines[0].contains("class: install"), "{lines:?}");
        assert!(
            lines
                .iter()
                .any(|line| line == "will run: cronus backup create --to D:/x"),
            "the question names the exact resolved invocation: {lines:?}"
        );
        assert!(
            !lines.iter().any(|line| line.starts_with("ran ")),
            "nothing is dispatched before the answer: {lines:?}"
        );
    }

    #[test]
    fn the_autonomy_grant_discloses_what_it_does_and_the_question_carries_the_acknowledgement() {
        let (registry, dispatcher) = composed();
        let question = asked(&registry, &dispatcher, "/activation enable --mode login");
        let lines = &question.block().lines;
        assert!(
            lines.iter().any(|l| l
                .contains("login background activation lets Cronus run while you are not present")),
            "the consequence is put to the person in plain words: {lines:?}"
        );
        assert!(
            lines.iter().any(|l| l.contains("spend allowance")),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|l| l
                == "will run: cronus activation enable --mode login --acknowledge-unattended-execution"),
            "the question names the acknowledgement it will give: {lines:?}"
        );
        assert!(
            matches!(
                question
                    .invocation
                    .args
                    .get("acknowledge-unattended-execution"),
                Some(ArgValue::Flag)
            ),
            "the invocation that runs is the one that was named"
        );
    }

    #[test]
    fn system_wide_activation_is_left_to_the_command_line_because_it_may_need_elevation() {
        let (registry, dispatcher) = composed();
        let block = answered(&registry, &dispatcher, "/activation enable --mode system");
        assert!(
            block.lines.iter().any(|l| l.contains("elevation prompt")),
            "{:?}",
            block.lines
        );
        assert!(
            block
                .lines
                .iter()
                .any(|l| l.contains("`cronus activation enable --mode system`")),
            "{:?}",
            block.lines
        );
    }

    #[test]
    fn a_workspace_is_asked_about_naming_its_id_name_and_place() {
        let (registry, dispatcher) = composed();
        let question = asked(
            &registry,
            &dispatcher,
            "/workspace create acme --name Acme --path D:/work",
        );
        assert!(
            question
                .block()
                .lines
                .iter()
                .any(|l| l.starts_with("will run: cronus workspace create acme")),
            "{:?}",
            question.block().lines
        );
    }

    /// Every change this surface carries out is asked about before anything
    /// is dispatched, and the question names what will run.
    #[test]
    fn each_change_this_surface_carries_out_is_asked_about_by_name() {
        let (registry, dispatcher) = composed();
        for (line, runs) in [
            ("/init", "cronus init"),
            ("/workspace switch bravo", "cronus workspace switch bravo"),
            (
                "/archetype create mine --from software-engineering",
                "cronus archetype create mine --from software-engineering",
            ),
            (
                "/registry create reviewer reviews",
                "cronus registry create reviewer reviews",
            ),
            ("/ext add m.json", "cronus ext add m.json"),
            (
                "/ext skill create --prompt a-skill",
                "cronus ext skill create --prompt a-skill",
            ),
        ] {
            let question = asked(&registry, &dispatcher, line);
            assert!(
                question
                    .block()
                    .lines
                    .iter()
                    .any(|l| l == &format!("will run: {runs}")),
                "{line}: {:?}",
                question.block().lines
            );
        }
    }

    #[test]
    fn a_skill_import_is_answered_with_why_it_cannot_run_and_the_person_is_not_asked() {
        let (registry, dispatcher) = composed();
        let block = answered(&registry, &dispatcher, "/ext skill import notes.md");
        assert!(
            block
                .lines
                .iter()
                .any(|l| l.contains("no attestation verifier")),
            "{:?}",
            block.lines
        );
        assert!(
            !block.lines.iter().any(|l| l.starts_with("ran ")),
            "nothing was dispatched: {:?}",
            block.lines
        );
    }

    #[test]
    fn deleting_a_workspace_and_adding_an_extension_say_what_they_do_and_do_not_do() {
        let (registry, dispatcher) = composed();
        // An id no registry would hold, so it is not the running workspace.
        let delete = asked(&registry, &dispatcher, "/workspace delete zz-not-running");
        assert!(
            delete
                .block()
                .lines
                .iter()
                .any(|l| l.contains("left where they are")),
            "{:?}",
            delete.block().lines
        );
        let add = asked(&registry, &dispatcher, "/ext add m.json");
        assert!(
            add.block()
                .lines
                .iter()
                .any(|l| l.contains("nothing is granted")),
            "{:?}",
            add.block().lines
        );
    }

    #[test]
    fn agreeing_runs_the_invocation_the_question_named() {
        let (registry, dispatcher) = composed();
        let question = asked(&registry, &dispatcher, "/backup create --to D:/x");
        let carried = confirmed(&registry, &dispatcher, &question);
        let block = carried.block;
        assert_eq!(block.title, "/backup create --to D:/x");
        assert_eq!(block.lines.len(), 1, "{:?}", block.lines);
        assert!(
            block.lines[0].starts_with("ran core:backup.create"),
            "{:?}",
            block.lines
        );
        assert!(!carried.relaunch, "an install change relaunches nothing");
    }

    /// The recomposing verbs a change can be made with from here, with what
    /// the question must say it will run.
    const RECOMPOSING: [(&str, &str); 9] = [
        ("/registry enable plan", "cronus registry enable plan"),
        ("/registry disable plan", "cronus registry disable plan"),
        (
            "/archetype set software-engineering",
            "cronus archetype set software-engineering",
        ),
        ("/archetype set --clear", "cronus archetype set --clear"),
        ("/ext activate demo/x", "cronus ext activate demo/x --yes"),
        ("/ext deactivate demo/x", "cronus ext deactivate demo/x"),
        ("/restore backup-1", "cronus restore backup-1"),
        ("/dev admit", "cronus dev admit"),
        ("/doctor --fix", "cronus doctor --fix"),
    ];

    #[test]
    fn a_change_applied_in_place_is_asked_about_and_says_it_takes_effect_at_once() {
        let (registry, dispatcher) = composed();
        let question = asked(&registry, &dispatcher, "/registry enable plan");
        let lines = &question.block().lines;
        assert_eq!(question.block().title, "confirm /registry enable plan");
        assert!(
            lines[0].contains("recompose, applied in place") && lines[0].contains("at once"),
            "{lines:?}"
        );
        assert!(!question.relaunches);
        assert!(
            !lines.iter().any(|line| line.starts_with("ran ")),
            "nothing is dispatched before the answer: {lines:?}"
        );
    }

    #[test]
    fn a_change_applied_by_relaunch_is_asked_about_and_says_the_screen_restarts() {
        let (registry, dispatcher) = composed();
        for line in [
            "/restore backup-1",
            "/doctor --fix",
            "/dev admit",
            "/dev revoke",
        ] {
            let question = asked(&registry, &dispatcher, line);
            let lines = &question.block().lines;
            assert!(
                lines[0].contains("recompose, applied by relaunch")
                    && lines[0].contains("restarts onto the same view"),
                "{line}: {lines:?}"
            );
            assert!(question.relaunches, "{line}");
        }
    }

    /// Every recomposing change this surface carries out is asked about
    /// before anything is dispatched, and the question names what will run —
    /// the grant included, since agreeing is what gives it.
    #[test]
    fn each_recomposing_change_is_asked_about_by_name() {
        let (registry, dispatcher) = composed();
        for (line, runs) in RECOMPOSING {
            let question = asked(&registry, &dispatcher, line);
            assert!(
                question
                    .block()
                    .lines
                    .iter()
                    .any(|l| l == &format!("will run: {runs}")),
                "{line}: {:?}",
                question.block().lines
            );
            assert!(
                matches!(question.invocation.args.get("yes"), Some(ArgValue::Flag))
                    == line.starts_with("/ext activate"),
                "only an activation carries an acknowledgement: {line}"
            );
        }
    }

    #[test]
    fn a_restore_and_an_archetype_say_what_they_do_before_they_are_agreed_to() {
        let (registry, dispatcher) = composed();
        let restore = asked(&registry, &dispatcher, "/restore backup-1");
        assert!(
            restore
                .block()
                .lines
                .iter()
                .any(|l| l.contains("over the state root")),
            "{:?}",
            restore.block().lines
        );
        let archetype = asked(
            &registry,
            &dispatcher,
            "/archetype set software-engineering",
        );
        assert!(
            archetype
                .block()
                .lines
                .iter()
                .any(|l| l.contains("the staff who are hired stay")),
            "{:?}",
            archetype.block().lines
        );
    }

    /// A relaunch is the engine's to make. A frontend attached to another
    /// process's engine would rebuild itself and leave the engine as it was,
    /// so it leaves those to the session that owns the engine — and does not
    /// ask, so nobody agrees to a refusal.
    #[test]
    fn a_frontend_attached_to_another_engine_leaves_a_relaunch_to_that_session() {
        let (registry, dispatcher) = composed();
        let spoke = Setting::answering(Placement::Spoke, |_| Ok(()));
        for line in ["/restore backup-1", "/doctor --fix", "/dev admit"] {
            let block = answered_in(&registry, &dispatcher, line, &spoke);
            assert!(
                block.lines[0].contains("class: recompose, applied by relaunch"),
                "{line}: {:?}",
                block.lines
            );
            assert!(
                block.lines.iter().any(|l| l.contains("another session")),
                "{line}: {:?}",
                block.lines
            );
            assert!(
                !block.lines.iter().any(|l| l.starts_with("ran ")),
                "a refused verb must not have been dispatched: {:?}",
                block.lines
            );
        }
        // A change that takes effect in place does not need the engine to
        // relaunch, so it is asked about as it would be anywhere.
        let question = asked_in(&registry, &dispatcher, "/registry enable plan", &spoke);
        assert!(!question.relaunches);
    }

    /// Running work refuses the change before the person is asked, says what
    /// is running, and leaves nothing queued.
    #[test]
    fn running_work_refuses_the_change_before_anyone_is_asked() {
        let (registry, dispatcher) = composed();
        let busy = Setting::answering(Placement::Engine, |_| {
            Err(Refused::Busy(
                "work is running that this change would alter: an agent turn (s-7)".to_string(),
            ))
        });
        for (line, _) in RECOMPOSING {
            let block = answered_in(&registry, &dispatcher, line, &busy);
            assert!(
                block.lines[0].starts_with("not applied —"),
                "{line}: {:?}",
                block.lines
            );
            assert!(
                block.lines[0].contains("an agent turn (s-7)"),
                "{line}: {:?}",
                block.lines
            );
            assert!(
                block
                    .lines
                    .iter()
                    .any(|l| l.contains("nothing was changed")),
                "{line}: {:?}",
                block.lines
            );
            assert!(
                !block.lines.iter().any(|l| l.starts_with("ran ")),
                "nothing was dispatched: {:?}",
                block.lines
            );
        }
    }

    #[test]
    fn a_work_lock_that_cannot_be_asked_refuses_without_assuming_idleness() {
        let (registry, dispatcher) = composed();
        let unknown = Setting::answering(Placement::Engine, |_| {
            Err(Refused::Unknown(
                "this cannot be told whether work is running, so the change is not applied: denied"
                    .to_string(),
            ))
        });
        let block = answered_in(&registry, &dispatcher, "/restore backup-1", &unknown);
        assert!(
            block.lines[0].starts_with("not applied —"),
            "{:?}",
            block.lines
        );
        assert!(
            block
                .lines
                .iter()
                .any(|l| l.contains("nothing was assumed")),
            "{:?}",
            block.lines
        );
    }

    /// The gate is asked about the verb the line resolved to, so the machine's
    /// verbs and the workspace's are each held to their own work.
    #[test]
    fn the_gate_is_asked_about_the_verb_that_was_typed() {
        use std::sync::Mutex;
        static ASKED: Mutex<Vec<String>> = Mutex::new(Vec::new());
        let (registry, dispatcher) = composed();
        let recording = Setting::answering(Placement::Engine, |tail| {
            ASKED
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(tail.to_string());
            Ok(())
        });
        asked_in(&registry, &dispatcher, "/registry enable plan", &recording);
        asked_in(&registry, &dispatcher, "/restore backup-1", &recording);
        let seen = ASKED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        assert_eq!(seen, vec!["registry.enable", "restore"]);
    }

    /// Agreeing to a change applied by relaunch asks this frontend to be
    /// rebuilt — but only when the change was applied.
    #[test]
    fn only_an_applied_relaunch_change_asks_for_the_relaunch() {
        let (registry, dispatcher) = composed();
        let restore = asked(&registry, &dispatcher, "/restore backup-1");
        let carried = confirmed(&registry, &dispatcher, &restore);
        assert!(carried.relaunch);
        assert_eq!(
            carried.block.lines.last().map(String::as_str),
            Some("restarting onto the changed installation")
        );

        let in_place = asked(&registry, &dispatcher, "/registry enable plan");
        assert!(
            !confirmed(&registry, &dispatcher, &in_place).relaunch,
            "a change applied in place needs no relaunch"
        );

        // The same verb, answered as not applied: nothing changed, so nothing
        // restarts.
        let mut refusing = InvocableRegistry::new();
        let mut dispatcher = Dispatcher::new();
        for invocable in cronus_core::installation::declared_invocables() {
            let id = invocable.id.clone();
            refusing
                .register(&Registrant::core(), invocable)
                .expect("declared verbs register cleanly");
            dispatcher.attach(
                id,
                Arc::new(|_: &ArgValues| Outcome::Unavailable {
                    reason: "not applied — work is running".to_string(),
                }),
            );
        }
        let carried = confirmed(&refusing, &dispatcher, &restore);
        assert!(!carried.relaunch);
        assert_eq!(
            carried.block.lines,
            vec!["unavailable: not applied — work is running".to_string()]
        );
    }

    /// A repair that found nothing to repair changed nothing, so it asks for
    /// no relaunch even though it is a change applied by relaunch.
    #[test]
    fn a_repair_with_nothing_to_repair_does_not_restart_the_screen() {
        let (registry, _) = composed();
        let mut clear = Dispatcher::new();
        for invocable in cronus_core::installation::declared_invocables() {
            clear.attach(
                invocable.id,
                Arc::new(|_: &ArgValues| {
                    Outcome::Value(OutcomeValue::Text(
                        cronus_core::installation::inspect::DOCTOR_ALL_CLEAR.to_string(),
                    ))
                }),
            );
        }
        let question = asked(&registry, &clear, "/doctor --fix");
        let carried = confirmed(&registry, &clear, &question);
        assert!(!carried.relaunch);
        assert_eq!(
            carried.block.lines,
            vec![cronus_core::installation::inspect::DOCTOR_ALL_CLEAR.to_string()]
        );
    }

    /// The class is that of the resolved invocation: the same verb inspects
    /// without its repair flag and recomposes with it.
    #[test]
    fn the_repair_flag_raises_diagnostics_above_inspect() {
        let (registry, dispatcher) = composed();
        let plain = answered(&registry, &dispatcher, "/doctor");
        assert_eq!(plain.lines, vec!["ran core:doctor fix=false".to_string()]);

        let repair = asked(&registry, &dispatcher, "/doctor --fix");
        assert!(
            repair.block().lines[0].contains("recompose, applied by relaunch"),
            "{:?}",
            repair.block().lines
        );
    }

    #[test]
    fn a_verb_nobody_can_carry_out_is_not_asked_about_even_when_it_would_recompose() {
        let (registry, dispatcher) = composed();
        let block = answered(&registry, &dispatcher, "/ext remove demo/x");
        assert!(
            block.lines.iter().any(|l| l.contains("not supported yet")),
            "{:?}",
            block.lines
        );
    }

    #[test]
    fn a_binding_failure_is_a_rejection_naming_the_binder() {
        let (registry, dispatcher) = composed();
        let block = answered(&registry, &dispatcher, "/status unexpected");
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
