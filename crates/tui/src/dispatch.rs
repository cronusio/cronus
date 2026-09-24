//! Resolves a slash command against the shared invocable registry, binds its
//! raw arguments, dispatches through the real, shared `Dispatcher`, and
//! renders the result into the one feedback string the command bar shows.
//!
//! This is where INV-7 masking, the dispatch journal, and rejection handling
//! all happen exactly once, at the one boundary every surface shares — this
//! module never re-implements any of them; it only ever constructs the
//! typed `ArgValues` a raw slash line does not yet carry, and turns whatever
//! comes back into text.

use std::collections::HashMap;

use cronus_contract::{
    ArgValue, ArgValues, Binder, BinderKind, Dispatched, InvocableId, Invocation, Outcome,
    OutcomeValue, Rejection, RejectionMode, Resolved, Surface,
};
use cronus_core::invocable::{Dispatcher, InvocableRegistry};

use crate::command::SlashCommand;

/// Resolve and dispatch one recognized slash command, returning the text the
/// command bar shows afterward — or `None` when the line is ordinary input,
/// not a command to report anything about.
///
/// `command.verb` is a catalog **group** (a discovery-level noun, derived
/// from the registry rather than hand-maintained); `command.args[0]`, if
/// present, is the CLI-style verb within that group — together they name a
/// candidate `core:{group}.{verb}` identity, never asserted to exist ahead
/// of dispatch. Every remaining raw argument binds against that candidate's
/// own declared binders (`bind_args`) before the real, shared `Dispatcher`
/// is ever called.
///
/// A slash-shaped line naming no invocable is not an error (l2-tui's own
/// v1.2.0 clause): resolution answers `Dispatched::Unknown` separately from
/// any outcome (SP-13), and this surface's response is to treat the line as
/// ordinary input — `None` here, rendering nothing at all — rather than
/// fabricate a failure. Folding that answer into a rendered error would make
/// every message beginning with a slash-shaped token an error, which is
/// wrong for a surface whose primary input is free text; the sibling CLI
/// frontend renders the identical resolution answer as a usage error
/// instead, correctly, because a one-shot invocation has nothing else to
/// fall through to — one resolution result, two correct and opposite
/// renderings. This is distinct from the two early returns above it in this
/// function (an incomplete line with no verb at all, or one whose verb
/// cannot even form a well-formed identity): those never reach resolution,
/// so they are not the case this clause is about, and keep their usage-hint
/// text.
pub fn dispatch_command(
    registry: &InvocableRegistry,
    dispatcher: &Dispatcher,
    command: &SlashCommand,
) -> Option<String> {
    let Some(sub_verb) = command.args.first() else {
        return Some(format!("usage: /{} <verb> [args…]", command.verb));
    };
    let Ok(id) = InvocableId::new(format!("core:{}.{sub_verb}", command.verb)) else {
        return Some(format!(
            "unknown command: /{} {sub_verb} (try /help)",
            command.verb
        ));
    };

    let empty: &[Binder] = &[];
    let binders = match registry.resolve(&id) {
        Resolved::Found(descriptor) => descriptor.binders.as_slice(),
        Resolved::Unknown => empty,
    };
    let args = match bind_args(binders, &command.args[1..]) {
        Ok(args) => args,
        Err(rejection) => return render_outcome(Outcome::Rejected(rejection)),
    };

    let invocation = Invocation {
        id,
        args,
        caller: Surface::Tui,
    };
    match dispatcher.dispatch(registry, &invocation) {
        Dispatched::Unknown => None,
        Dispatched::Ran(outcome) => render_outcome(outcome),
    }
}

/// Bind a slash command's raw, whitespace-split arguments against `binders`,
/// producing the [`ArgValues`] a dispatch needs.
///
/// This is the raw-string→typed step IB-4 leaves to whichever surface
/// constructs `ArgValues` from raw input — the domain tier's own
/// bind-before-invoke `bind()` only ever validates already-typed values, so
/// it can never itself produce [`RejectionMode::Malformed`] (a value present
/// but not parseable as anything); this function is where that mode
/// legitimately originates for this surface.
///
/// Positional-kind binders (`Text`/`Integer`/`Boolean`/`Float`) consume raw
/// tokens in declared order, skipping over any `--name` token and its value
/// (those belong to named-kind binders). Named-kind binders
/// (`Flag`/`NamedText`/`RepeatableNamedText`) are recognized by a
/// `--{binder.name}` token: a `Flag` never consumes a following token, a
/// `NamedText` consumes exactly one, a `RepeatableNamedText` collects one
/// per occurrence — the same three shapes the CLI's own generated grammar
/// binds, expressed here without a clap parser. A binder with no candidate
/// raw value is simply left absent (whether required or optional): the
/// domain tier's own `bind()` reports `RejectionMode::Absent` with the
/// correct location once dispatch runs, and duplicating that check here
/// would only produce a second, potentially inconsistent copy of it.
pub fn bind_args(binders: &[Binder], args: &[String]) -> Result<ArgValues, Rejection> {
    let named_kind = |name: &str| binders.iter().find(|b| b.name == name).map(|b| b.kind);

    let mut positionals: Vec<&str> = Vec::new();
    let mut named: HashMap<&str, Vec<Option<&str>>> = HashMap::new();
    let mut i = 0;
    while i < args.len() {
        if let Some(name) = args[i].strip_prefix("--") {
            let takes_value = matches!(
                named_kind(name),
                Some(BinderKind::NamedText) | Some(BinderKind::RepeatableNamedText)
            );
            let value = if takes_value {
                args.get(i + 1)
                    .filter(|v| !v.starts_with("--"))
                    .map(String::as_str)
            } else {
                None
            };
            named.entry(name).or_default().push(value);
            i += if value.is_some() { 2 } else { 1 };
        } else {
            positionals.push(args[i].as_str());
            i += 1;
        }
    }

    let mut values = ArgValues::new();
    let mut positionals = positionals.into_iter();
    for binder in binders {
        match binder.kind {
            BinderKind::Text => {
                if let Some(raw) = positionals.next() {
                    values.insert(binder.name, ArgValue::Text(raw.to_string()));
                }
            }
            BinderKind::EnumText(allowed) => {
                if let Some(raw) = positionals.next() {
                    if allowed.contains(&raw) {
                        values.insert(binder.name, ArgValue::Text(raw.to_string()));
                    } else {
                        return Err(malformed(binder.name, raw));
                    }
                }
            }
            BinderKind::Integer => {
                if let Some(raw) = positionals.next() {
                    match raw.parse::<i64>() {
                        Ok(n) => values.insert(binder.name, ArgValue::Integer(n)),
                        Err(_) => return Err(malformed(binder.name, raw)),
                    }
                }
            }
            BinderKind::Boolean => {
                if let Some(raw) = positionals.next() {
                    match raw.parse::<bool>() {
                        Ok(b) => values.insert(binder.name, ArgValue::Boolean(b)),
                        Err(_) => return Err(malformed(binder.name, raw)),
                    }
                }
            }
            BinderKind::Float => {
                if let Some(raw) = positionals.next() {
                    match raw.parse::<f64>() {
                        Ok(f) => values.insert(binder.name, ArgValue::Float(f)),
                        Err(_) => return Err(malformed(binder.name, raw)),
                    }
                }
            }
            BinderKind::Flag => {
                if named.get(binder.name).is_some_and(|occ| !occ.is_empty()) {
                    values.insert(binder.name, ArgValue::Flag);
                }
            }
            BinderKind::NamedText => {
                if let Some(Some(raw)) = named.get(binder.name).and_then(|occ| occ.first()) {
                    values.insert(binder.name, ArgValue::Text((*raw).to_string()));
                }
            }
            BinderKind::RepeatableNamedText => {
                let collected: Vec<String> = named
                    .get(binder.name)
                    .map(|occ| occ.iter().filter_map(|v| v.map(str::to_string)).collect())
                    .unwrap_or_default();
                if !collected.is_empty() {
                    values.insert(binder.name, ArgValue::List(collected));
                }
            }
        }
    }
    // A positional token past what any declared binder consumes: the
    // CLI's own clap-composed grammar already rejects this ("unexpected
    // argument"), so a surplus positional the TUI silently drops is an
    // invocation the two surfaces disagree on, not one this surface is
    // extending the sibling CLI frontend's own vocabulary to accept.
    let extra: Vec<&str> = positionals.collect();
    if !extra.is_empty() {
        return Err(too_many_positionals(&extra));
    }
    Ok(values)
}

fn malformed(binder: &'static str, raw: &str) -> Rejection {
    Rejection {
        binder,
        mode: RejectionMode::Malformed,
        detail: format!("{raw:?} is not a valid value for this argument"),
    }
}

fn too_many_positionals(extra: &[&str]) -> Rejection {
    Rejection {
        binder: "<unexpected>",
        mode: RejectionMode::IllShaped,
        detail: format!(
            "unexpected extra argument(s): {}",
            extra
                .iter()
                .map(|s| format!("{s:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Render a dispatched `Outcome` into the single feedback string the
/// command bar shows — `None` when a real success carries nothing to say.
/// A stateless acknowledgment (`core:pane.*`'s own `Outcome::Value(
/// Empty)`, "ran, no further detail") is not the same fact as "empty
/// result" or "empty string", so it renders as no feedback line at all
/// rather than the placeholder text `"(empty)"`. That placeholder still
/// appears wherever `Empty` shows up *inside* a larger structure (a
/// `Record` field genuinely absent, e.g. `board show`'s `reason: (empty)`)
/// — only this top-level, bare case changes, since only here does "nothing
/// to say" and "the value was empty" collapse into the same shape.
///
/// `Rejected`/`Unavailable` render their binder/mode/reason directly rather
/// than being flattened into an undifferentiated string, so the two stay
/// distinguishable in what the user actually sees — the same discipline the
/// sibling CLI frontend's own renderer already holds to. `Value` recurses
/// through the same shape `OutcomeValue` can take; reimplemented locally
/// (not imported from the CLI, which this crate must not depend on, INV-2).
pub fn render_outcome(outcome: Outcome) -> Option<String> {
    match outcome {
        Outcome::Value(OutcomeValue::Empty) => None,
        Outcome::Value(value) => Some(render_value(&value)),
        Outcome::Rejected(rejection) => Some(format!(
            "rejected: {} ({:?}) — {}",
            rejection.binder, rejection.mode, rejection.detail
        )),
        Outcome::Unavailable { reason } => Some(format!("unavailable: {reason}")),
        Outcome::Stream(_) => Some(
            "error: internal: a stream outcome has no renderer on this surface yet".to_string(),
        ),
    }
}

fn render_value(value: &OutcomeValue) -> String {
    match value {
        OutcomeValue::Empty => "(empty)".to_string(),
        OutcomeValue::Text(s) => s.clone(),
        OutcomeValue::Integer(n) => n.to_string(),
        OutcomeValue::Float(n) => format!("{n}"),
        OutcomeValue::Boolean(b) => b.to_string(),
        OutcomeValue::List(items) if items.is_empty() => "no results".to_string(),
        OutcomeValue::List(items) => items
            .iter()
            .map(render_list_item)
            .collect::<Vec<_>>()
            .join(", "),
        OutcomeValue::Record(fields) => fields
            .iter()
            .map(|(name, v)| format!("{name}: {}", render_value(v)))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// Render one element of a list, delimiting it when it is itself a compound
/// value (a record's own fields, or a nested list) — otherwise a list of
/// records renders as one undifferentiated, comma-joined string with no way
/// to tell where one record's fields end and the next record begins (a real
/// simulation-qa finding: `/board list` with two cards rendered as `id: k1,
/// state: running, id: k2, state: blocked`). A scalar element is rendered
/// exactly as `render_value` already would, so an existing all-scalar list
/// (e.g. `cards: a, b`) is unaffected.
fn render_list_item(value: &OutcomeValue) -> String {
    match value {
        OutcomeValue::Record(_) => format!("{{{}}}", render_value(value)),
        OutcomeValue::List(_) => format!("[{}]", render_value(value)),
        other => render_value(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binder(name: &'static str, kind: BinderKind, optional: bool) -> Binder {
        Binder {
            name,
            kind,
            optional,
        }
    }

    fn strings(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn bind_args_binds_positional_text_in_declared_order() {
        let binders = vec![
            binder("key", BinderKind::Text, false),
            binder("value", BinderKind::Text, false),
        ];
        let args = bind_args(&binders, &strings(&["fact", "the sky is blue"])).unwrap();
        assert_eq!(args.get("key"), Some(&ArgValue::Text("fact".to_string())));
        assert_eq!(
            args.get("value"),
            Some(&ArgValue::Text("the sky is blue".to_string()))
        );
    }

    #[test]
    fn bind_args_leaves_a_missing_binder_absent_rather_than_erroring() {
        let binders = vec![binder("id", BinderKind::Text, false)];
        let args = bind_args(&binders, &strings(&[])).unwrap();
        assert_eq!(
            args.get("id"),
            None,
            "absence is left to the domain tier's own bind() to report"
        );
    }

    #[test]
    fn bind_args_parses_typed_positionals() {
        let binders = vec![
            binder("limit", BinderKind::Float, false),
            binder("count", BinderKind::Integer, false),
            binder("active", BinderKind::Boolean, false),
        ];
        let args = bind_args(&binders, &strings(&["12.5", "3", "true"])).unwrap();
        assert_eq!(args.get("limit"), Some(&ArgValue::Float(12.5)));
        assert_eq!(args.get("count"), Some(&ArgValue::Integer(3)));
        assert_eq!(args.get("active"), Some(&ArgValue::Boolean(true)));
    }

    #[test]
    fn bind_args_rejects_a_malformed_typed_value() {
        let binders = vec![binder("count", BinderKind::Integer, false)];
        let err = bind_args(&binders, &strings(&["abc"])).unwrap_err();
        assert_eq!(err.binder, "count");
        assert_eq!(err.mode, RejectionMode::Malformed);
    }

    #[test]
    fn bind_args_binds_a_flag_by_its_own_name_never_a_positional_word() {
        let binders = vec![binder("presets", BinderKind::Flag, true)];
        let absent = bind_args(&binders, &strings(&[])).unwrap();
        assert_eq!(absent.get("presets"), None);

        let present = bind_args(&binders, &strings(&["--presets"])).unwrap();
        assert_eq!(present.get("presets"), Some(&ArgValue::Flag));
    }

    #[test]
    fn bind_args_binds_a_named_text_value_and_leaves_it_out_of_positionals() {
        let binders = vec![
            binder("id", BinderKind::Text, false),
            binder("actor", BinderKind::NamedText, true),
        ];
        let args = bind_args(&binders, &strings(&["--actor", "cli", "card-1"])).unwrap();
        assert_eq!(args.get("id"), Some(&ArgValue::Text("card-1".to_string())));
        assert_eq!(args.get("actor"), Some(&ArgValue::Text("cli".to_string())));
    }

    #[test]
    fn bind_args_collects_every_occurrence_of_a_repeatable_named_text() {
        let binders = vec![binder("collection", BinderKind::RepeatableNamedText, false)];
        let args = bind_args(
            &binders,
            &strings(&["--collection", "a", "--collection", "b"]),
        )
        .unwrap();
        assert_eq!(
            args.get("collection"),
            Some(&ArgValue::List(vec!["a".to_string(), "b".to_string()]))
        );
    }

    /// A positional token past what any declared binder consumes is
    /// rejected, not silently dropped — `board add T2 ref2 EXTRA` used to
    /// succeed identically to `board add T2 ref2`.
    #[test]
    fn bind_args_rejects_a_surplus_positional_rather_than_dropping_it() {
        let binders = vec![
            binder("id", BinderKind::Text, false),
            binder("task_ref", BinderKind::Text, false),
        ];
        let err = bind_args(&binders, &strings(&["T2", "ref2", "EXTRA"])).unwrap_err();
        assert_eq!(err.mode, RejectionMode::IllShaped);
        assert!(err.detail.contains("EXTRA"));
    }

    #[test]
    fn bind_args_reports_every_surplus_positional_not_just_the_first() {
        let binders = vec![binder("id", BinderKind::Text, false)];
        let err = bind_args(&binders, &strings(&["a", "b", "c"])).unwrap_err();
        assert!(err.detail.contains("b") && err.detail.contains("c"));
    }

    #[test]
    fn bind_args_exact_arg_count_still_succeeds() {
        let binders = vec![
            binder("id", BinderKind::Text, false),
            binder("task_ref", BinderKind::Text, false),
        ];
        let args = bind_args(&binders, &strings(&["T2", "ref2"])).unwrap();
        assert_eq!(args.get("id"), Some(&ArgValue::Text("T2".to_string())));
    }

    #[test]
    fn render_outcome_shows_the_rejections_binder_and_mode_not_a_bare_string() {
        let rendered = render_outcome(Outcome::Rejected(Rejection {
            binder: "id",
            mode: RejectionMode::Absent,
            detail: "no value supplied".to_string(),
        }))
        .expect("a rejection always carries feedback text");
        assert!(
            rendered.contains("id"),
            "the offending binder must be named"
        );
        assert!(
            rendered.contains("Absent"),
            "the rejection mode must be legible, not flattened away"
        );
    }

    #[test]
    fn render_outcome_recurses_through_a_nested_record_and_list() {
        let rendered = render_outcome(Outcome::Value(OutcomeValue::Record(vec![(
            "cards".to_string(),
            OutcomeValue::List(vec![
                OutcomeValue::Text("a".to_string()),
                OutcomeValue::Text("b".to_string()),
            ]),
        )])))
        .expect("a nonempty Record always carries feedback text");
        assert_eq!(rendered, "cards: a, b");
    }

    /// The simulation-qa finding this fixes: a list of records (the exact
    /// shape `core:board.list` returns for more than one card) must keep
    /// each record's own fields visually distinguishable from the next
    /// list element, never one undifferentiated comma-joined string.
    #[test]
    fn render_outcome_marks_record_boundaries_inside_a_list() {
        let rendered = render_outcome(Outcome::Value(OutcomeValue::List(vec![
            OutcomeValue::Record(vec![
                ("id".to_string(), OutcomeValue::Text("k1".to_string())),
                (
                    "state".to_string(),
                    OutcomeValue::Text("running".to_string()),
                ),
            ]),
            OutcomeValue::Record(vec![
                ("id".to_string(), OutcomeValue::Text("k2".to_string())),
                (
                    "state".to_string(),
                    OutcomeValue::Text("blocked".to_string()),
                ),
            ]),
        ])))
        .expect("a nonempty List always carries feedback text");
        assert_eq!(
            rendered, "{id: k1, state: running}, {id: k2, state: blocked}",
            "each record's braces must make the element boundary legible"
        );
    }

    /// A pane action's own stateless acknowledgment
    /// (`Outcome::Value(Empty)`, the exact shape every `core:pane.*`
    /// handler answers) used to render the literal placeholder text
    /// `"(empty)"` — a real success with nothing further to say is not the
    /// same fact as "the value was empty".
    #[test]
    fn render_outcome_shows_no_feedback_for_a_bare_empty_success() {
        let rendered = render_outcome(Outcome::Value(OutcomeValue::Empty));
        assert_eq!(
            rendered, None,
            "a stateless acknowledgment must render no feedback line at all"
        );
    }

    /// The same `Empty` value *inside* a larger structure is a different
    /// fact (a field the domain layer genuinely left absent) and keeps its
    /// placeholder text — only the bare top-level case changes.
    #[test]
    fn render_outcome_still_shows_empty_placeholder_for_a_nested_field() {
        let rendered = render_outcome(Outcome::Value(OutcomeValue::Record(vec![(
            "reason".to_string(),
            OutcomeValue::Empty,
        )])))
        .expect("a Record with a field always carries feedback text");
        assert_eq!(rendered, "reason: (empty)");
    }

    /// The literal Verify criterion this task names: a slash line whose
    /// candidate identity resolves to nothing (`Dispatched::Unknown`) is
    /// ordinary input, not a rendered failure — `dispatch_command` returns
    /// `None`, never an "unknown command" string. Proven against a real,
    /// empty registry/dispatcher pair — not a stub — so this is the actual
    /// resolution path, the same one a genuinely bound verb goes through.
    #[test]
    fn dispatch_command_treats_an_unresolved_identity_as_ordinary_input() {
        let registry = InvocableRegistry::new();
        let dispatcher = Dispatcher::new();
        let command = SlashCommand {
            verb: "board".to_string(),
            args: strings(&["frobnicate"]),
        };
        assert_eq!(
            dispatch_command(&registry, &dispatcher, &command),
            None,
            "an unresolved candidate identity must render nothing, not an error string"
        );
    }

    /// The other half of the same criterion: a genuinely `Rejected` outcome
    /// — the candidate identity *does* resolve, but binding fails — is still
    /// rendered as a refusal, proving the two are distinguishable rather
    /// than both silently swallowed.
    #[test]
    fn dispatch_command_still_renders_a_genuine_rejection() {
        let mut registry = InvocableRegistry::new();
        let dispatcher = Dispatcher::new();
        let id = InvocableId::new("core:test.probe").expect("well-formed test id");
        registry
            .register(
                &cronus_core::invocable::Registrant::core(),
                cronus_contract::Invocable {
                    id,
                    name: "Probe",
                    summary: "Test-only probe invocable.",
                    group: "test",
                    locus: cronus_contract::Locus::Semantic,
                    binders: vec![Binder {
                        name: "value",
                        kind: BinderKind::Text,
                        optional: false,
                    }],
                    stability: cronus_contract::Stability::Shipped,
                    journal_raw_input: true,
                },
            )
            .expect("test fixture registers cleanly");
        let command = SlashCommand {
            verb: "test".to_string(),
            args: strings(&["probe"]),
        };
        let rendered = dispatch_command(&registry, &dispatcher, &command)
            .expect("a resolved-but-rejected invocation must still render something");
        assert!(
            rendered.contains("value"),
            "the offending binder must be named: {rendered:?}"
        );
        assert!(
            rendered.contains("Absent"),
            "the rejection mode must reach the rendered text: {rendered:?}"
        );
    }
}
