//! Naming an installation verb the way a shell would be asked for it.
//!
//! A composed surface that will not carry an installation verb out itself —
//! because it needs a privilege the surface's process does not hold, or a
//! consent the surface cannot collect yet — answers with the command line
//! that can. The command line is the guaranteed path to every installation
//! verb, so the answer is always an invocation a person can paste into a
//! shell, spelled exactly as that frontend's grammar spells it.

use cronus_contract::{ArgValue, ArgValues, BinderKind, Invocable};

/// The words after the group that name the verb: nothing for a flat verb
/// (`cronus doctor`), one word for a nested one (`cronus backup create`),
/// two for a sub-nested one (`cronus ext skill import`).
fn verb_words(invocable: &Invocable) -> Vec<&str> {
    let tail = invocable.id.tail();
    let verb = tail
        .strip_prefix(invocable.group)
        .and_then(|rest| rest.strip_prefix('.'))
        .unwrap_or(tail);
    if verb == invocable.group {
        Vec::new()
    } else {
        verb.split('.').collect()
    }
}

fn quoted(value: &str) -> String {
    if !value.is_empty() && !value.contains(char::is_whitespace) && !value.contains('"') {
        return value.to_string();
    }
    format!("\"{}\"", value.replace('"', "\\\""))
}

fn positional(value: &ArgValue) -> Option<String> {
    match value {
        ArgValue::Text(text) => Some(quoted(text)),
        ArgValue::Integer(n) => Some(n.to_string()),
        ArgValue::Boolean(b) => Some(b.to_string()),
        ArgValue::Float(f) => Some(f.to_string()),
        ArgValue::Flag | ArgValue::List(_) => None,
    }
}

/// The command-line invocation that runs `invocable` with `args`, e.g.
/// `cronus backup create --to "D:\my backups"`. Arguments are placed the way
/// the verb's declared binders place them: positional values in order, then
/// each named flag or value by its own name.
pub fn command_line_invocation(invocable: &Invocable, args: &ArgValues) -> String {
    let mut words = vec!["cronus".to_string(), invocable.group.to_string()];
    words.extend(verb_words(invocable).into_iter().map(str::to_string));
    for binder in &invocable.binders {
        let Some(value) = args.get(binder.name) else {
            continue;
        };
        match (binder.kind, value) {
            (BinderKind::Flag, ArgValue::Flag) => words.push(format!("--{}", binder.name)),
            (BinderKind::NamedText, ArgValue::Text(text)) => {
                words.push(format!("--{}", binder.name));
                words.push(quoted(text));
            }
            (BinderKind::RepeatableNamedText, ArgValue::List(items)) => {
                for item in items {
                    words.push(format!("--{}", binder.name));
                    words.push(quoted(item));
                }
            }
            (_, value) => words.extend(positional(value)),
        }
    }
    words.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installation::declared_invocables;

    fn declared(tail: &str) -> Invocable {
        declared_invocables()
            .into_iter()
            .find(|invocable| invocable.id.tail() == tail)
            .unwrap_or_else(|| panic!("no declared installation verb {tail:?}"))
    }

    fn args(entries: &[(&str, ArgValue)]) -> ArgValues {
        let mut values = ArgValues::new();
        for (name, value) in entries {
            values.insert(*name, value.clone());
        }
        values
    }

    #[test]
    fn a_flat_verb_is_named_by_its_group_alone() {
        assert_eq!(
            command_line_invocation(&declared("doctor"), &ArgValues::new()),
            "cronus doctor"
        );
        assert_eq!(
            command_line_invocation(&declared("doctor"), &args(&[("fix", ArgValue::Flag)])),
            "cronus doctor --fix"
        );
    }

    #[test]
    fn a_nested_verb_carries_its_verb_and_its_named_values() {
        let invocation = command_line_invocation(
            &declared("backup.create"),
            &args(&[
                ("to", ArgValue::Text("D:\\my backups".to_string())),
                ("include-logs", ArgValue::Flag),
            ]),
        );
        assert_eq!(
            invocation,
            "cronus backup create --to \"D:\\my backups\" --include-logs"
        );
    }

    #[test]
    fn a_sub_nested_verb_carries_both_words() {
        assert_eq!(
            command_line_invocation(
                &declared("ext.skill.import"),
                &args(&[("path", ArgValue::Text("skills/standup.md".to_string()))])
            ),
            "cronus ext skill import skills/standup.md"
        );
    }

    #[test]
    fn a_positional_value_follows_the_verb_in_declared_order() {
        assert_eq!(
            command_line_invocation(
                &declared("registry.create"),
                &args(&[
                    ("name", ArgValue::Text("scribe".to_string())),
                    (
                        "description",
                        ArgValue::Text("writes the notes".to_string())
                    ),
                ])
            ),
            "cronus registry create scribe \"writes the notes\""
        );
    }

    #[test]
    fn an_absent_argument_is_left_out_not_invented() {
        assert_eq!(
            command_line_invocation(&declared("restore"), &ArgValues::new()),
            "cronus restore"
        );
    }
}
