mod cli;
mod commands;
#[cfg(test)]
mod conformance_registration;
mod generated;
mod installation;
mod output;

use clap::{Command, CommandFactory};
use cronus_contract::{Dispatched, Invocable, Outcome, OutcomeValue};
use cronus_core::invocable::Registrant;
use output::{OutputFormat, json_escape};

/// Which of the launcher's top-level modes a raw `argv` resolves to,
/// decided before any composition or I/O runs — a pure function of the
/// argument list, so the routing decision itself is testable without
/// spawning a process or a terminal (LH-4's default composition made this
/// split worth making explicit rather than leaving it as an inline
/// boolean).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LaunchMode {
    /// No arguments at all — the default composition (LH-4).
    DefaultComposition,
    /// A request addressed to the whole surface (`--help`, `--version`, a
    /// bare `help`, …) — answered from the full composition, never from
    /// this frontend's own installation grammar (LH-3).
    FullSurfaceRequest,
    /// Everything else — tried against the installation half first, with
    /// no composition at all (LH-1/LH-5).
    Verb,
}

fn launch_mode(args: &[String]) -> LaunchMode {
    let Some(first) = args.get(1) else {
        return LaunchMode::DefaultComposition;
    };
    if matches!(
        first.as_str(),
        "--help" | "-h" | "help" | "--version" | "-V" | "completion"
    ) {
        // `completion` joins the full-surface requests: it renders a script
        // from the whole composed command tree, so it cannot be answered from
        // the installation half's pre-composition grammar alone.
        return LaunchMode::FullSurfaceRequest;
    }
    LaunchMode::Verb
}

fn main() -> std::process::ExitCode {
    let installation_invocables = installation::declared_invocables();
    let installation_refs: Vec<&Invocable> = installation_invocables.iter().collect();

    let args: Vec<String> = std::env::args().collect();

    match launch_mode(&args) {
        // A bare invocation is the default composition (LH-4): meeting the
        // product by typing its name brings up the terminal UI, exactly as
        // `cronus tui` names the same composition
        // explicitly. Answered with the same zero-composition property
        // every installation verb already has (LH-1/LH-5) —
        // `cronus_tui::run()` composes its own registry/dispatcher
        // internally the moment it starts, so this launcher never builds
        // one first just to hand off to it.
        LaunchMode::DefaultComposition => return exit_code(installation::launch_tui()),
        // A request addressed to the whole surface falls through to the
        // full composition below — this frontend's own installation
        // grammar only owns its *own* help (LH-3), not the top-level
        // listing.
        LaunchMode::FullSurfaceRequest => {}
        // Everything else is tried against the installation half first,
        // with no composition at all (LH-1/LH-5): an installation verb
        // must stay answerable even when the composition it would
        // configure is exactly what failed to come up.
        LaunchMode::Verb => {
            let (installation_groups, installation_group_names) =
                installation::build_installation_tree(&installation_refs);
            let pre = pre_composition_command(installation_groups);

            match pre.try_get_matches_from(&args) {
                Ok(pre_matches) => {
                    if let Some((name, sub_matches)) = pre_matches.subcommand()
                        && installation_group_names.contains(name)
                    {
                        let format = pre_matches
                            .get_one::<OutputFormat>("format")
                            .copied()
                            .unwrap_or(OutputFormat::Text);
                        let ctx = output::Context::new(format);
                        return exit_code(installation::dispatch(name, sub_matches, &ctx));
                    }
                    // Matched nothing this half owns (an external subcommand,
                    // under `allow_external_subcommands` below) — fall through
                    // to the full composition to resolve it.
                }
                Err(e) => {
                    // `allow_external_subcommands` means an error here can only
                    // come from a *recognized* installation verb's own args (or
                    // its own `--help`) — never from an unrecognized top-level
                    // name, which is swallowed as an external subcommand
                    // instead. This is therefore always a genuine usage failure
                    // scoped to this half's own grammar (LH-7): nothing below
                    // this line has run, no session opened, nothing journaled.
                    e.exit();
                }
            }
        }
    }

    // Compose: the shared registry and dispatcher every surface projects,
    // plus this frontend's own installation descriptors registered into the
    // same catalog (for cross-surface honesty, SP-11 — this is the *other*
    // consumer of the one declaration above). Structured as a real `Result`
    // rather than an early `expect()`: composition cannot actually fail
    // today (no I/O, no extension loading yet), but the distinction LH-7
    // asks for must be representable now rather than retrofitted the day it
    // can.
    let (registry, dispatcher) = match compose(&installation_refs) {
        Ok(pair) => pair,
        Err(reason) => {
            eprintln!("error: composition failed: {reason}");
            return std::process::ExitCode::from(3);
        }
    };

    let semantic = generated::semantic_shipped(&registry);
    let (semantic_groups, generated_group_names) = generated::build_semantic_tree(&semantic);
    let (installation_groups, installation_group_names) =
        installation::build_installation_tree(&installation_refs);

    // The installation half's hand-declared grammar, augmented with
    // whatever the registry currently ships on the semantic half. Built
    // fresh every run — cheap, and the only way a plugin's newly
    // registered verb becomes visible in `--help` without a rebuild.
    // `subcommand_required` is set explicitly here rather than left to
    // derive-macro inference (INV-9: `Cli` itself declares no subcommand
    // field for a compile-time enum to imply it from). `[MODIFIED]` A truly
    // bare invocation no longer reaches this line at all — it is now the
    // default composition (LH-4), answered above before composition even
    // runs. What this still refuses is the narrower case a bare invocation
    // used to stand in for: a global flag with no verb at all (`cronus
    // --format json`), which stays a genuine usage failure, not a spelling
    // of "launch the default."
    let mut command = cli::Cli::command().subcommand_required(true);
    // One alphabetical list, not two: the installation/semantic split is an
    // internal build-order distinction (§4.1.1), not something a user reading
    // `--help` should see as `workspace` … `agent` … `board`.
    let mut all_groups: Vec<Command> = installation_groups
        .into_iter()
        .chain(semantic_groups)
        .collect();
    all_groups.sort_by(|a, b| a.get_name().cmp(b.get_name()));
    for group in all_groups {
        command = command.subcommand(group);
    }
    // `completion` is served from this composed tree directly: it needs the
    // whole `clap::Command` (every group, every verb, including contributed
    // ones), which `installation::dispatch`'s per-verb handlers do not have.
    let matches = command.clone().get_matches();

    let format = matches
        .get_one::<OutputFormat>("format")
        .copied()
        .unwrap_or(OutputFormat::Text);
    let ctx = output::Context::new(format);

    if let Some(("completion", sub_matches)) = matches.subcommand() {
        return exit_code(installation::emit_completion(sub_matches, &mut command));
    }

    if let Some((name, sub_matches)) = matches.subcommand()
        && installation_group_names.contains(name)
    {
        return exit_code(installation::dispatch(name, sub_matches, &ctx));
    }

    if let Some((group, group_matches)) =
        generated::matched_generated_group(&matches, &generated_group_names)
        && let Some((invocable, args)) =
            generated::invocation_from_matches(group, group_matches, &semantic)
    {
        let invocation = cronus_contract::Invocation {
            id: invocable.id.clone(),
            args,
            caller: cronus_contract::Surface::Cli,
        };
        let rendered = render(dispatcher.dispatch(&registry, &invocation), &ctx);
        rendered.print();
        return exit_code(rendered.exit_code);
    }

    // Every verb this launcher can reach is either an installation verb or a
    // registry-generated semantic one (INV-9: nothing else has a
    // descriptor, so nothing else can be shipped) — `subcommand_required`
    // above means `command.get_matches()` itself already refused anything
    // that matched neither before this point could ever be reached. A
    // defensive internal error, not a panic, if that invariant is ever
    // violated.
    eprintln!("error: internal: matched a subcommand neither half of the launcher owns");
    std::process::ExitCode::from(1)
}

/// The installation half's own pre-composition parser: just enough grammar
/// to recognize and fully parse its own verbs, with everything it does not
/// own passed through untouched rather than refused — ownership of an
/// unrecognized name is not this half's question to answer (LH-5).
fn pre_composition_command(installation_groups: Vec<Command>) -> Command {
    let mut pre = Command::new("cronus")
        .about("Cronus — workflow automation toolkit")
        .disable_version_flag(true)
        .subcommand_required(false)
        .arg_required_else_help(false)
        .allow_external_subcommands(true)
        .arg(installation::format_arg());
    for group in installation_groups {
        pre = pre.subcommand(group);
    }
    pre
}

/// Bring up the registry and dispatcher, then register this frontend's own
/// installation descriptors into the same catalog through the public
/// registration door — no privileged path, the same one a contribution
/// uses. A real `Result`: a registration refusal (a malformed literal
/// descriptor, a duplicate id) is a genuine composition failure, not a
/// panic.
fn compose(
    installation_invocables: &[&Invocable],
) -> Result<
    (
        cronus_core::invocable::InvocableRegistry,
        cronus_core::invocable::Dispatcher,
    ),
    String,
> {
    let (mut registry, dispatcher) =
        cronus_core::invocable_bootstrap::bootstrap(cronus_core::Engine::new());
    for invocable in installation_invocables {
        registry
            .register(&Registrant::core(), (*invocable).clone())
            .map_err(|e| format!("{e:?}"))?;
    }
    Ok((registry, dispatcher))
}

/// What a render pass produced: the lines to print on each stream, in
/// order, and the process exit code — a value, not a side effect. Every
/// rendering function below is a pure function of `Outcome`/`OutcomeValue`
/// plus the requested [`output::Context`]: nothing in this module writes to
/// `stdout`/`stderr` directly except [`Rendered::print`] itself. This is
/// what makes "rendering is one function of `Outcome` and the requested
/// format, not a per-command decision" a checkable property rather than a
/// description — a test can call `render`, inspect the `Rendered` value for
/// both formats, and assert on it without spawning a subprocess or
/// capturing real `stdout`.
#[derive(Debug, Default, PartialEq)]
struct Rendered {
    stdout: Vec<String>,
    stderr: Vec<String>,
    exit_code: i32,
}

impl Rendered {
    fn stdout_only(line: String, exit_code: i32) -> Self {
        Rendered {
            stdout: vec![line],
            exit_code,
            ..Default::default()
        }
    }

    fn stderr_only(line: String, exit_code: i32) -> Self {
        Rendered {
            stderr: vec![line],
            exit_code,
            ..Default::default()
        }
    }

    /// The one place this module ever calls `println!`/`eprintln!` —
    /// everything above this point only ever builds a value.
    fn print(&self) {
        for line in &self.stdout {
            println!("{line}");
        }
        for line in &self.stderr {
            eprintln!("{line}");
        }
    }
}

/// Render a dispatched semantic invocation into what to print and its
/// process exit code. `Dispatched::Unknown` cannot occur on the call site
/// this module actually uses: it is only ever invoked after
/// `invocation_from_matches` already resolved a real descriptor from the
/// same registry `dispatch` immediately consults — handled here anyway
/// (rather than asserted away) because this function is also the one a
/// test drives directly, without going through that resolution step first.
fn render(dispatched: Dispatched, ctx: &output::Context) -> Rendered {
    let outcome = match dispatched {
        Dispatched::Ran(outcome) => outcome,
        Dispatched::Unknown => {
            return Rendered::stderr_only(
                "error: internal: dispatched an invocable the registry does not know".to_string(),
                1,
            );
        }
    };
    // Every rejection/unavailability path renders as plain diagnostic prose
    // to `stderr` regardless of the requested format — a deliberate,
    // project-wide convention (the installation half's own handlers follow
    // it identically): `--format` governs the success payload a
    // programmatic consumer parses off `stdout`, not the human-readable
    // diagnostic a failure writes to `stderr`.
    match outcome {
        Outcome::Rejected(rejection) => Rendered::stderr_only(
            format!(
                "error: {} ({:?}): {}",
                rejection.binder, rejection.mode, rejection.detail
            ),
            2,
        ),
        Outcome::Unavailable { reason } => Rendered::stderr_only(format!("error: {reason}"), 1),
        Outcome::Stream(_) => Rendered::stderr_only(
            "error: internal: a stream outcome has no renderer on this surface yet".to_string(),
            1,
        ),
        Outcome::Value(value) => render_value(value, ctx),
    }
}

fn render_value(value: OutcomeValue, ctx: &output::Context) -> Rendered {
    // `core:loop.run`: kept as its own bespoke arm — `cli_smoke.rs`'s own
    // end-to-end test extracts the run id straight out of this exact text
    // shape (`"loop {run_id}: done (...)"`.`strip_prefix("loop
    // ").split(':')`) to drive a real `log`/`show` round trip, so this is
    // real, tested, shipped output, not free to reshape the way the
    // general fallback below reshapes everything else.
    if let OutcomeValue::Record(fields) = &value {
        let get_text = |key: &str| {
            fields
                .iter()
                .find(|(name, _)| name == key)
                .and_then(|(_, v)| match v {
                    OutcomeValue::Text(s) => Some(s.as_str()),
                    _ => None,
                })
        };
        if let (Some(run_id), Some(status)) = (get_text("run_id"), get_text("status")) {
            return match status {
                "done" => {
                    let iterations = fields
                        .iter()
                        .find(|(name, _)| name == "iterations")
                        .and_then(|(_, v)| match v {
                            OutcomeValue::Integer(n) => Some(*n),
                            _ => None,
                        })
                        .unwrap_or(0);
                    let line = if ctx.is_json() {
                        format!(
                            "{{\"outcome\":\"done\",\"run_id\":\"{}\",\"iterations\":{iterations}}}",
                            json_escape(run_id)
                        )
                    } else {
                        format!("loop {run_id}: done ({iterations} iteration(s))")
                    };
                    Rendered::stdout_only(line, 0)
                }
                _ => {
                    let reason = get_text("reason").unwrap_or("");
                    let line = if ctx.is_json() {
                        format!(
                            "{{\"outcome\":\"{}\",\"reason\":\"{}\",\"run_id\":\"{}\"}}",
                            json_escape(status),
                            json_escape(reason),
                            json_escape(run_id)
                        )
                    } else {
                        format!("loop {run_id}: {status} ({reason})")
                    };
                    Rendered::stdout_only(line, 1)
                }
            };
        }
    }
    // `core:workflow.validate`'s own shape — a Record naming "status" plus
    // "errors"/"warnings"/"infos" (each a List of {code, message, line}
    // Records) — gets its own arm ahead of the failed/aborted/… check
    // below (F-16): the general fallback flattened every diagnostic and
    // the record's own "status"/"errors: "/"warnings: " labels onto one
    // comma-separated line, unreadable past a couple of findings. --format
    // json already produced well-structured, valid nesting through the
    // general recursive render_json below — only the text path needed
    // fixing, so JSON keeps using it unchanged.
    if let OutcomeValue::Record(fields) = &value
        && let [
            ("status", OutcomeValue::Text(status)),
            ("errors", OutcomeValue::List(errors)),
            ("warnings", OutcomeValue::List(warnings)),
            ("infos", OutcomeValue::List(infos)),
        ] = fields
            .iter()
            .map(|(name, v)| (name.as_str(), v))
            .collect::<Vec<_>>()
            .as_slice()
    {
        if ctx.is_json() {
            return Rendered::stdout_only(render_json(&value), if status == "ok" { 0 } else { 1 });
        }
        let mut lines = vec![format!("status: {status}")];
        for (label, diags) in [("error", errors), ("warning", warnings), ("info", infos)] {
            for diag in diags.iter() {
                lines.push(render_diagnostic_line(label, diag));
            }
        }
        return Rendered {
            stdout: lines,
            exit_code: if status == "ok" { 0 } else { 1 },
            ..Default::default()
        };
    }
    // `core:board.show`'s own shape — a multi-field Record naming "id" and
    // "history" (a List of transition Records) — gets a dedicated
    // multi-line renderer (F-21/F-24): once `show` grew beyond id+state to
    // the card's full detail, the general one-line-per-Record fallback
    // flattened every field, including the whole transition history, onto
    // one unreadable line. Recognized by field shape (an "id" field plus a
    // "history" List) rather than verb identity, so any future card-shaped
    // verb gets the same rendering for free. --format json already renders
    // this shape correctly through the general recursive render_json below
    // — only text needed a dedicated arm.
    if !ctx.is_json()
        && let OutcomeValue::Record(fields) = &value
        && fields.iter().any(|(n, _)| n == "id")
        && let Some(OutcomeValue::List(history)) =
            fields.iter().find(|(n, _)| n == "history").map(|(_, v)| v)
    {
        let get = |key: &str| fields.iter().find(|(n, _)| n == key).map(|(_, v)| v);
        let mut lines = Vec::new();
        for key in [
            "id",
            "state",
            "task_ref",
            "reason",
            "assignee",
            "priority",
            "created_at",
            "updated_at",
        ] {
            if let Some(v) = get(key) {
                lines.push(format!("{key}: {}", render_text_inline(v)));
            }
        }
        if history.is_empty() {
            lines.push("history: (none)".to_string());
        } else {
            lines.push("history:".to_string());
            for entry in history {
                lines.push(format!("  {}", render_text_line(entry)));
            }
        }
        return Rendered {
            stdout: lines,
            exit_code: 0,
            ..Default::default()
        };
    }
    // A Record naming its own "status" among "failed"/"aborted"/
    // "stopped"/"paused" (`core:workflow.run` today) is the one shape
    // among the shipped verbs whose success is not uniformly exit 0.
    // Recognized by field name rather than by verb identity, so any
    // future verb needing the same property gets it for free rather than
    // needing its own renderer arm. The pre-migration 3-way scheme
    // (`workflow run`'s Paused got its own exit 2) collapses to 2-way
    // here, disclosed rather than silently kept or silently dropped: no
    // test locks the finer distinction.
    if let OutcomeValue::Record(fields) = &value
        && let Some(OutcomeValue::Text(status)) = fields
            .iter()
            .find(|(name, _)| name == "status")
            .map(|(_, v)| v)
        && matches!(status.as_str(), "failed" | "aborted" | "stopped" | "paused")
    {
        let line = if ctx.is_json() {
            render_json(&value)
        } else {
            render_text_line(&value)
        };
        return Rendered::stdout_only(line, 1);
    }
    match &value {
        // `core:memory.store` / `core:memory.forget` / `core:role.fire` /
        // `core:exec.create|finalize|discard`: a one-field Record naming
        // the affected entry's id — kept as its own arm since "Ok: <id>"
        // reads better than the generic `id: <id>` the fallback below
        // would produce, and several shipped verbs share this exact
        // shape.
        OutcomeValue::Record(fields) if fields.len() == 1 && fields[0].0 == "id" => {
            let id = match &fields[0].1 {
                OutcomeValue::Text(id) => id.as_str(),
                _ => "",
            };
            let line = if ctx.is_json() {
                format!("{{\"result\":\"ok\",\"id\":\"{}\"}}", json_escape(id))
            } else {
                format!("Ok: {id}")
            };
            Rendered::stdout_only(line, 0)
        }
        OutcomeValue::List(items) if ctx.is_json() => {
            let rendered: Vec<String> = items.iter().map(render_json).collect();
            Rendered::stdout_only(format!("[{}]", rendered.join(",")), 0)
        }
        OutcomeValue::List(items) if items.is_empty() => {
            Rendered::stdout_only("No results.".to_string(), 0)
        }
        OutcomeValue::List(items) => Rendered {
            stdout: items.iter().map(render_text_line).collect(),
            exit_code: 0,
            ..Default::default()
        },
        OutcomeValue::Text(text) if ctx.is_json() => {
            Rendered::stdout_only(format!("\"{}\"", json_escape(text)), 0)
        }
        OutcomeValue::Text(text) => Rendered::stdout_only(text.clone(), 0),
        // Every other shape — an arbitrary `Record` (`core:codegraph.index`'s
        // `{path, symbols}`, `core:check.run`'s `{card, language}`, …), a
        // bare `Integer`/`Boolean`, or `Empty` — has no per-verb bespoke
        // format to preserve (none was ever shipped for these shapes), so
        // it renders through the general recursive mapping below rather
        // than through a hand-written arm per verb.
        OutcomeValue::Empty => Rendered {
            exit_code: 0,
            ..Default::default()
        },
        other if ctx.is_json() => Rendered::stdout_only(render_json(other), 0),
        other => Rendered::stdout_only(render_text_line(other), 0),
    }
}

/// One `workflow.validate` diagnostic (a `{code, message, line}` Record) as
/// its own line: `"{label}[{code}] line {line}: {message}"`. Missing/
/// mistyped fields degrade to an empty/zero placeholder rather than
/// panicking — this renders whatever the domain layer actually produced,
/// never asserts its shape.
fn render_diagnostic_line(label: &str, diag: &OutcomeValue) -> String {
    let OutcomeValue::Record(fields) = diag else {
        return format!("{label}: {}", render_text_inline(diag));
    };
    let get = |key: &str| fields.iter().find(|(name, _)| name == key).map(|(_, v)| v);
    let code = match get("code") {
        Some(OutcomeValue::Text(s)) => s.as_str(),
        _ => "",
    };
    let message = match get("message") {
        Some(OutcomeValue::Text(s)) => s.as_str(),
        _ => "",
    };
    let line = match get("line") {
        Some(OutcomeValue::Integer(n)) => *n,
        _ => 0,
    };
    format!("{label}[{code}] line {line}: {message}")
}

/// A general JSON projection of any `OutcomeValue` shape — recursive, so a
/// nested `List`/`Record` renders correctly without a dedicated arm.
fn render_json(value: &OutcomeValue) -> String {
    match value {
        OutcomeValue::Empty => "null".to_string(),
        OutcomeValue::Text(s) => format!("\"{}\"", json_escape(s)),
        OutcomeValue::Integer(n) => n.to_string(),
        OutcomeValue::Float(n) if n.is_finite() => format!("{n}"),
        OutcomeValue::Float(_) => "null".to_string(),
        OutcomeValue::Boolean(b) => b.to_string(),
        OutcomeValue::List(items) => {
            format!(
                "[{}]",
                items.iter().map(render_json).collect::<Vec<_>>().join(",")
            )
        }
        OutcomeValue::Record(fields) => {
            let rendered: Vec<String> = fields
                .iter()
                .map(|(name, v)| format!("\"{}\":{}", json_escape(name), render_json(v)))
                .collect();
            format!("{{{}}}", rendered.join(","))
        }
    }
}

/// A general one-line text projection: a `Record`'s fields as `key: value`
/// pairs, everything else inline. Used for a `List`'s items and for any
/// bare value that reaches the general fallback.
fn render_text_line(value: &OutcomeValue) -> String {
    match value {
        OutcomeValue::Record(fields) => fields
            .iter()
            .map(|(name, v)| format!("{name}: {}", render_text_inline(v)))
            .collect::<Vec<_>>()
            .join(", "),
        other => render_text_inline(other),
    }
}

fn render_text_inline(value: &OutcomeValue) -> String {
    match value {
        OutcomeValue::Empty => "(empty)".to_string(),
        OutcomeValue::Text(s) => s.clone(),
        OutcomeValue::Integer(n) => n.to_string(),
        OutcomeValue::Float(n) => format!("{n}"),
        OutcomeValue::Boolean(b) => b.to_string(),
        OutcomeValue::List(items) => items
            .iter()
            .map(render_text_inline)
            .collect::<Vec<_>>()
            .join(", "),
        record @ OutcomeValue::Record(_) => render_text_line(record),
    }
}

fn exit_code(code: i32) -> std::process::ExitCode {
    if code == 0 {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
}

#[cfg(test)]
mod launch_mode_tests {
    use super::*;

    fn argv(rest: &[&str]) -> Vec<String> {
        std::iter::once("cronus".to_string())
            .chain(rest.iter().map(|s| s.to_string()))
            .collect()
    }

    /// The literal Verify criterion this task names: running the binary
    /// with no arguments resolves to the default composition, never a
    /// usage error.
    #[test]
    fn no_arguments_resolves_to_the_default_composition() {
        assert_eq!(launch_mode(&argv(&[])), LaunchMode::DefaultComposition);
    }

    #[test]
    fn every_full_surface_request_falls_through_to_the_full_composition() {
        for spelling in ["--help", "-h", "help", "--version", "-V"] {
            assert_eq!(
                launch_mode(&argv(&[spelling])),
                LaunchMode::FullSurfaceRequest,
                "{spelling} must resolve to FullSurfaceRequest"
            );
        }
    }

    #[test]
    fn a_real_verb_is_tried_against_the_installation_half_first() {
        assert_eq!(launch_mode(&argv(&["tui"])), LaunchMode::Verb);
        assert_eq!(launch_mode(&argv(&["board", "list"])), LaunchMode::Verb);
    }

    /// A flag with no verb (`cronus --format json`) is deliberately **not**
    /// the default composition — it stays the narrower, pre-existing usage
    /// question `subcommand_required(true)` already answers, unchanged by
    /// this task's own scoping decision.
    #[test]
    fn a_bare_flag_with_no_verb_is_not_the_default_composition() {
        assert_eq!(
            launch_mode(&argv(&["--format", "json"])),
            LaunchMode::Verb,
            "a flag alone must not be treated as a bare invocation"
        );
    }
}

#[cfg(test)]
mod render_tests {
    use std::collections::BTreeSet;

    use cronus_contract::{Locus, Rejection, RejectionMode, Stability};

    use super::*;
    use output::{Context, OutputFormat};

    fn text() -> Context {
        Context::new(OutputFormat::Text)
    }

    fn json() -> Context {
        Context::new(OutputFormat::Json)
    }

    fn value(outcome_value: OutcomeValue) -> Dispatched {
        Dispatched::Ran(Outcome::Value(outcome_value))
    }

    /// A minimal, dependency-free JSON-shape probe — not a parser, just
    /// enough to distinguish "this is JSON-shaped" from "this is plain
    /// prose that ignored the format flag". Cheap enough to avoid pulling
    /// a JSON crate into this test alone.
    fn looks_like_json(s: &str) -> bool {
        let t = s.trim();
        t.starts_with('{')
            || t.starts_with('[')
            || t.starts_with('"')
            || t == "true"
            || t == "false"
            || t == "null"
            || t.parse::<f64>().is_ok()
    }

    /// The literal Verify criterion this task names: every `OutcomeValue`
    /// shape the shared renderer distinguishes — the closed set any shipped
    /// invocable's handler can produce — is exercised through both output
    /// formats, and the requested one is honoured every time. Exhaustive
    /// over the *type* rather than sampled from today's call sites, which
    /// is the stronger property: a future invocable returning any of these
    /// shapes inherits the guarantee for free, without a renderer change.
    #[test]
    fn every_outcome_value_shape_honors_the_requested_format() {
        let cases: Vec<(&str, OutcomeValue, i32)> = vec![
            ("empty", OutcomeValue::Empty, 0),
            (
                "bare text",
                OutcomeValue::Text("hello world".to_string()),
                0,
            ),
            ("bare integer", OutcomeValue::Integer(42), 0),
            ("bare float", OutcomeValue::Float(1.5), 0),
            (
                "record with a float field",
                OutcomeValue::Record(vec![("limit".to_string(), OutcomeValue::Float(100.0))]),
                0,
            ),
            ("bare boolean", OutcomeValue::Boolean(true), 0),
            ("empty list", OutcomeValue::List(Vec::new()), 0),
            (
                "nonempty list of records",
                OutcomeValue::List(vec![
                    OutcomeValue::Record(vec![(
                        "id".to_string(),
                        OutcomeValue::Text("a".to_string()),
                    )]),
                    OutcomeValue::Record(vec![(
                        "id".to_string(),
                        OutcomeValue::Text("b".to_string()),
                    )]),
                ]),
                0,
            ),
            (
                "single-id record",
                OutcomeValue::Record(vec![(
                    "id".to_string(),
                    OutcomeValue::Text("x-1".to_string()),
                )]),
                0,
            ),
            (
                "generic multi-field record",
                OutcomeValue::Record(vec![
                    (
                        "path".to_string(),
                        OutcomeValue::Text("a.nodus".to_string()),
                    ),
                    ("symbols".to_string(), OutcomeValue::Integer(7)),
                ]),
                0,
            ),
            (
                "nested record with a list field",
                OutcomeValue::Record(vec![(
                    "errors".to_string(),
                    OutcomeValue::List(vec![OutcomeValue::Text("boom".to_string())]),
                )]),
                0,
            ),
            (
                "loop run: done",
                OutcomeValue::Record(vec![
                    (
                        "run_id".to_string(),
                        OutcomeValue::Text("run-1".to_string()),
                    ),
                    ("status".to_string(), OutcomeValue::Text("done".to_string())),
                    ("iterations".to_string(), OutcomeValue::Integer(3)),
                ]),
                0,
            ),
            (
                "loop run: stopped",
                OutcomeValue::Record(vec![
                    (
                        "run_id".to_string(),
                        OutcomeValue::Text("run-2".to_string()),
                    ),
                    (
                        "status".to_string(),
                        OutcomeValue::Text("stopped".to_string()),
                    ),
                    (
                        "reason".to_string(),
                        OutcomeValue::Text("MaxIterations".to_string()),
                    ),
                ]),
                1,
            ),
            (
                "workflow run: failed (no run_id)",
                OutcomeValue::Record(vec![(
                    "status".to_string(),
                    OutcomeValue::Text("failed".to_string()),
                )]),
                1,
            ),
            (
                "board show: card detail with history",
                OutcomeValue::Record(vec![
                    ("id".to_string(), OutcomeValue::Text("A1".to_string())),
                    ("state".to_string(), OutcomeValue::Text("todo".to_string())),
                    (
                        "history".to_string(),
                        OutcomeValue::List(vec![OutcomeValue::Record(vec![(
                            "from".to_string(),
                            OutcomeValue::Text("triage".to_string()),
                        )])]),
                    ),
                ]),
                0,
            ),
            (
                "workflow validate: ok with warnings",
                OutcomeValue::Record(vec![
                    ("status".to_string(), OutcomeValue::Text("ok".to_string())),
                    ("errors".to_string(), OutcomeValue::List(Vec::new())),
                    (
                        "warnings".to_string(),
                        OutcomeValue::List(vec![OutcomeValue::Record(vec![
                            ("code".to_string(), OutcomeValue::Text("W001".to_string())),
                            (
                                "message".to_string(),
                                OutcomeValue::Text("no tests".to_string()),
                            ),
                            ("line".to_string(), OutcomeValue::Integer(0)),
                        ])]),
                    ),
                    ("infos".to_string(), OutcomeValue::List(Vec::new())),
                ]),
                0,
            ),
        ];

        for (label, shape, expected_exit) in cases {
            let text_rendered = render(value(shape.clone()), &text());
            let json_rendered = render(value(shape.clone()), &json());

            assert_eq!(
                text_rendered.exit_code, expected_exit,
                "{label}: text-format exit code"
            );
            assert_eq!(
                json_rendered.exit_code, expected_exit,
                "{label}: json-format exit code"
            );

            let text_out = text_rendered.stdout.join("\n");
            let json_out = json_rendered.stdout.join("\n");

            if shape == OutcomeValue::Empty {
                assert!(
                    text_out.is_empty() && json_out.is_empty(),
                    "{label}: an empty value prints nothing in either format"
                );
                continue;
            }

            assert!(
                !json_out.is_empty(),
                "{label}: json format must not discard the value"
            );
            assert!(
                looks_like_json(&json_out),
                "{label}: json format did not look like JSON: {json_out:?}"
            );
            // A bare `Integer`/`Boolean` legitimately renders as the same
            // literal in both formats (`42`, `true`) — Rust's `Display` and
            // JSON's number/bool syntax coincide there, so identical output
            // is the *correct* honouring of format, not a sign format was
            // ignored. Every shape with actual structure (quoting,
            // brackets, `key: value` vs. `"key":value`) must genuinely
            // differ, which is the real signal a site silently reused one
            // format's text for the other.
            if !matches!(
                shape,
                OutcomeValue::Integer(_) | OutcomeValue::Boolean(_) | OutcomeValue::Float(_)
            ) {
                assert_ne!(
                    text_out, json_out,
                    "{label}: text and json format must render differently — identical output \
                     means the format flag was consulted by neither, or discarded by one"
                );
            }
            // A float must reach JSON as a bare number, never a quoted string.
            if matches!(shape, OutcomeValue::Float(_)) {
                assert!(
                    json_out.parse::<f64>().is_ok(),
                    "{label}: a float must render as a bare JSON number, got {json_out:?}"
                );
            }
        }
    }

    /// F-21/F-24: `board show`'s text rendering is one field per line, with
    /// the transition history indented underneath rather than flattened
    /// onto the same comma-separated line every other field-name shape
    /// falls back to.
    #[test]
    fn board_show_renders_one_field_per_line_with_indented_history() {
        let shape = OutcomeValue::Record(vec![
            ("id".to_string(), OutcomeValue::Text("A1".to_string())),
            ("state".to_string(), OutcomeValue::Text("todo".to_string())),
            (
                "history".to_string(),
                OutcomeValue::List(vec![OutcomeValue::Record(vec![
                    ("from".to_string(), OutcomeValue::Text("triage".to_string())),
                    ("to".to_string(), OutcomeValue::Text("todo".to_string())),
                ])]),
            ),
        ]);
        let rendered = render(value(shape), &text());
        assert_eq!(rendered.stdout[0], "id: A1");
        assert_eq!(rendered.stdout[1], "state: todo");
        assert_eq!(rendered.stdout[2], "history:");
        assert!(
            rendered.stdout[3].starts_with("  ") && rendered.stdout[3].contains("triage"),
            "a history entry must be indented under its own line: {:?}",
            rendered.stdout[3]
        );
    }

    #[test]
    fn board_show_renders_no_history_explicitly_rather_than_an_empty_section() {
        let shape = OutcomeValue::Record(vec![
            ("id".to_string(), OutcomeValue::Text("A1".to_string())),
            ("history".to_string(), OutcomeValue::List(Vec::new())),
        ]);
        let rendered = render(value(shape), &text());
        assert!(rendered.stdout.iter().any(|l| l == "history: (none)"));
    }

    /// F-16: each `workflow validate` diagnostic renders on its own line,
    /// never flattened together with the record's own status/errors/
    /// warnings/infos labels.
    #[test]
    fn workflow_validate_renders_one_diagnostic_per_line() {
        let shape = OutcomeValue::Record(vec![
            ("status".to_string(), OutcomeValue::Text("ok".to_string())),
            ("errors".to_string(), OutcomeValue::List(Vec::new())),
            (
                "warnings".to_string(),
                OutcomeValue::List(vec![OutcomeValue::Record(vec![
                    ("code".to_string(), OutcomeValue::Text("W001".to_string())),
                    (
                        "message".to_string(),
                        OutcomeValue::Text("no tests".to_string()),
                    ),
                    ("line".to_string(), OutcomeValue::Integer(3)),
                ])]),
            ),
            ("infos".to_string(), OutcomeValue::List(Vec::new())),
        ]);
        let rendered = render(value(shape), &text());
        assert_eq!(rendered.stdout[0], "status: ok");
        assert_eq!(rendered.stdout[1], "warning[W001] line 3: no tests");
        assert_eq!(rendered.stdout.len(), 2, "no empty errors/infos lines");
    }

    /// A rejected/unavailable/unresolved dispatch renders as plain
    /// diagnostic prose on `stderr`, deliberately identical in both
    /// formats — `--format` governs the success payload on `stdout`, not a
    /// failure's human-readable diagnostic (the same convention every
    /// installation-half handler already follows). Locked in explicitly so
    /// this reads as a decision the shape test above does not otherwise
    /// exercise, not an oversight.
    #[test]
    fn failure_outcomes_render_identical_diagnostic_prose_regardless_of_format() {
        let cases: Vec<Dispatched> = vec![
            Dispatched::Unknown,
            Dispatched::Ran(Outcome::Unavailable {
                reason: "boom".to_string(),
            }),
            Dispatched::Ran(Outcome::Rejected(Rejection {
                binder: "id",
                mode: RejectionMode::Absent,
                detail: "missing".to_string(),
            })),
        ];
        for dispatched in cases {
            let a = render(dispatched.clone(), &text());
            let b = render(dispatched, &json());
            assert_eq!(
                a, b,
                "a failure outcome must render identically in both formats"
            );
            assert!(
                a.stdout.is_empty(),
                "a failure outcome must print nothing to stdout"
            );
            assert!(
                !a.stderr.is_empty(),
                "a failure outcome must print its diagnostic to stderr"
            );
        }
    }

    /// Ties the shape coverage above to the actual shipped surface: every
    /// `Semantic`+`Shipped` group this renderer is reached for still
    /// exists, by name — catching a group silently vanishing (or
    /// reclassifying) from the surface this renderer serves. Deliberately
    /// does not dispatch these invocables for real: several resolve a
    /// real, un-overridable OS state directory or a live network
    /// dependency (`knowledge query`'s embedding backend, absent in this
    /// environment — the same reason `cli_smoke.rs` never exercises that
    /// group), and the renderer under test is provably decoupled from
    /// invocable identity — its `match` operates on `OutcomeValue` shape
    /// alone, so the shape-exhaustive test above already proves
    /// format-honouring for anything any of these invocables could ever
    /// return.
    #[test]
    fn every_shipped_semantic_group_this_renderer_serves_still_exists() {
        let (registry, _dispatcher) =
            cronus_core::invocable_bootstrap::bootstrap(cronus_core::Engine::new());
        let groups: BTreeSet<&str> = registry
            .all()
            .filter(|i| {
                matches!(i.locus, Locus::Semantic) && matches!(i.stability, Stability::Shipped)
            })
            .map(|i| i.group)
            .collect();
        let expected: BTreeSet<&str> = [
            "memory",
            "codegraph",
            "agent",
            "role",
            "exec",
            "check",
            "learn",
            "board",
            "schedule",
            "budget",
            "loop",
            "workflow",
            "knowledge",
        ]
        .into_iter()
        .collect();
        assert_eq!(
            groups, expected,
            "the shipped semantic surface this renderer serves must be exactly the known set"
        );
    }
}
