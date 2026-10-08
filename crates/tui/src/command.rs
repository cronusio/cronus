//! Slash-command parsing and the command catalog.
//!
//! The command bar accepts `/verb arg…` lines. This module turns a line into a
//! structured [`SlashCommand`], builds the [`CommandSpec`] catalog from the
//! core's invocable registry — never a hand-maintained list — and classifies a
//! submitted line into a [`CommandOutcome`]. Dispatch to the core is a separate
//! concern — this module never calls the core; [`build_catalog`] only
//! ever consumes an already-fetched `&[&Invocable]` slice, the same shape data
//! the sibling CLI frontend's own `generated.rs` consumes.

use cronus_contract::{Invocable, InvocableId, LocusKind};

/// A parsed slash command: the verb plus its whitespace-separated arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashCommand {
    /// The command verb (without the leading `/`).
    pub verb: String,
    /// Positional arguments following the verb.
    pub args: Vec<String>,
}

/// Why a command-bar line is not a runnable slash command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    /// The line does not start with `/`.
    NotACommand,
    /// The line is `/` with no verb.
    Empty,
}

/// Split a command-bar line's body into whitespace-separated tokens,
/// honoring double- and single-quoted segments as one token each.
///
/// A CLI invocation gets this for free from the shell that splits its
/// argv; the command bar has no shell in front of it, so without this a
/// verb taking a prose argument — `board block A1 "waiting on dep"` — has
/// no TUI spelling at all: a bare whitespace split breaks the quoted
/// phrase into five separate tokens instead of one. Quote characters are
/// stripped from the token they delimit; an unterminated quote runs to the
/// end of the line rather than erroring, so a trailing typo degrades
/// gracefully instead of losing the whole command.
fn tokenize(body: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut chars = body.chars().peekable();
    while chars.peek().is_some() {
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        if chars.peek().is_none() {
            break;
        }
        let mut token = String::new();
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() {
                break;
            }
            if c == '"' || c == '\'' {
                chars.next(); // consume the opening quote
                for inner in chars.by_ref() {
                    if inner == c {
                        break;
                    }
                    token.push(inner);
                }
            } else {
                token.push(c);
                chars.next();
            }
        }
        tokens.push(token);
    }
    tokens
}

/// Parse a command-bar line into a [`SlashCommand`].
///
/// Syntactic only — it does not check the verb against a catalog; use
/// [`classify`] for that. Leading/trailing whitespace is ignored.
pub fn parse(input: &str) -> Result<SlashCommand, ParseError> {
    let body = input
        .trim()
        .strip_prefix('/')
        .ok_or(ParseError::NotACommand)?;
    let mut tokens = tokenize(body).into_iter();
    let verb = tokens.next().ok_or(ParseError::Empty)?;
    Ok(SlashCommand {
        verb,
        args: tokens.collect(),
    })
}

/// One entry in the slash-command catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    /// Verb name (without the leading `/`) — a registry `group`, or `help`.
    pub name: &'static str,
    /// One-line summary shown by `/help`.
    pub summary: String,
    /// Whether this is an installation group — a verb that configures or
    /// inspects the product rather than acting on the user's work. Such
    /// groups are listed under their own heading, so they are never mistaken
    /// for work commands.
    pub installation: bool,
}

/// The loci this surface projects: `Semantic` (the shared vocabulary
/// cross-surface parity binds), `ClientLocal` (this surface's own pane/panel
/// actions) and `Installation` (the verbs that configure or inspect the
/// product, run here beside the work). `HostOnly` belongs to the host.
pub const PROJECTED_LOCI: [LocusKind; 3] = [
    LocusKind::Semantic,
    LocusKind::ClientLocal,
    LocusKind::Installation,
];

/// The installation verbs this surface declares it does not offer, each with
/// its reason. A verb left out without a stated reason is indistinguishable
/// from one not yet built, so the exclusion is data that a test can check
/// against the catalog, never a silence.
pub const INSTALLATION_EXCLUSIONS: [(&str, &str); 3] = [
    (
        "core:tui",
        "it brings up this composition; inside it there is nothing to bring up",
    ),
    (
        "core:completion",
        "the script is generated from the command line's own grammar, which this \
         surface does not hold; run `cronus completion <shell>`",
    ),
    (
        "core:version",
        "the command line answers it as the --version flag, not as a verb",
    ),
];

/// Why this surface does not offer the installation verb `id`, when it
/// declares that it does not.
pub fn installation_exclusion(id: &InvocableId) -> Option<&'static str> {
    INSTALLATION_EXCLUSIONS
        .iter()
        .find(|(excluded, _)| *excluded == id.as_str())
        .map(|(_, reason)| *reason)
}

/// Whether this surface projects `invocable` at all: one of the loci in
/// [`PROJECTED_LOCI`], only `Shipped` stability, and not a declared
/// installation exclusion — a retired or unshipped invocable is
/// unrepresentable here, not merely undiscoverable.
///
/// Delegates to [`Invocable::is_projected_for`], the predicate every
/// frontend's catalog is decided by, so "what does a frontend expose"
/// stays decided in exactly one place (the contract crate) rather than each
/// surface hand-rolling the rule again; what a surface contributes is its own
/// set of loci and its own declared exclusions.
pub fn is_projected(invocable: &Invocable) -> bool {
    invocable.is_projected_for(&PROJECTED_LOCI) && installation_exclusion(&invocable.id).is_none()
}

/// Build the slash-command catalog from the core's invocable registry.
///
/// One entry per **group**, not per invocable: this surface's slash form
/// mirrors the CLI's own two-level `<noun> <verb>` grammar (`/<noun> <verb>
/// …`) — the noun is the slash verb here, and the CLI-side
/// verb travels as the command's first argument. A slash verb and its shell
/// counterpart are still two renderings of one descriptor once a group+args
/// pair resolves to a specific invocable; this catalog is the discovery
/// layer above that, exactly as `/board` (not `/board.list`) is what a user
/// discovers before typing `list`.
///
/// `help` is prepended as this surface's own discovery affordance — it has
/// no registry counterpart (the CLI uses `--help` instead), matching the
/// same carve-out the deleted hand-copied mirror already documented.
///
/// No per-group summary exists in the registry (`Invocable.summary` is
/// per-verb, not per-group), so every derived entry reads `"{group}
/// operations"` — the exact fallback text the CLI's own `generated.rs`
/// already uses for a semantic group's top-level `--help` line. A curated,
/// hand-written per-group description was deliberately not built instead:
/// that would be a second list to keep in sync, the same defect class this
/// task deletes, just relocated from verb *names* (which broke parity) to
/// verb *prose* (which would only ever degrade quietly).
pub fn build_catalog(invocables: &[&Invocable]) -> Vec<CommandSpec> {
    let mut groups: Vec<(&'static str, bool)> = invocables
        .iter()
        .filter(|invocable| is_projected(invocable))
        .map(|invocable| {
            (
                invocable.group,
                invocable.locus.kind() == LocusKind::Installation,
            )
        })
        .collect();
    groups.sort_unstable();
    groups.dedup();

    // An installation group of exactly one verb (`doctor`, `restore`, …) is
    // described by that verb's own summary; every other group keeps the
    // generic text.
    let summary_of = |group: &'static str| {
        let mut members = invocables
            .iter()
            .filter(|invocable| invocable.group == group && is_projected(invocable));
        match (members.next(), members.next()) {
            (Some(only), None) if only.locus.kind() == LocusKind::Installation => {
                only.summary.to_string()
            }
            _ => format!("{group} operations"),
        }
    };

    let mut catalog = Vec::with_capacity(groups.len() + 1);
    catalog.push(CommandSpec {
        name: "help",
        summary: "List available slash commands".to_string(),
        installation: false,
    });
    catalog.extend(groups.into_iter().map(|(group, installation)| CommandSpec {
        name: group,
        summary: summary_of(group),
        installation,
    }));
    catalog
}

/// The catalog entry for `verb`, if it is a known command.
pub fn lookup<'a>(verb: &str, catalog: &'a [CommandSpec]) -> Option<&'a CommandSpec> {
    catalog.iter().find(|c| c.name == verb)
}

/// Whether `verb` is a known slash command.
pub fn is_known(verb: &str, catalog: &[CommandSpec]) -> bool {
    lookup(verb, catalog).is_some()
}

/// Every known verb name, in catalog order.
pub fn names(catalog: &[CommandSpec]) -> impl Iterator<Item = &str> {
    catalog.iter().map(|c| c.name)
}

/// The `/help` discovery listing: one `"/verb — summary"` line per command,
/// with the installation verbs set apart under their own heading so they are
/// never mistaken for commands that act on the user's work.
pub fn help_lines(catalog: &[CommandSpec]) -> Vec<String> {
    let line = |c: &CommandSpec| format!("/{:<10} — {}", c.name, c.summary);
    let mut lines: Vec<String> = catalog
        .iter()
        .filter(|c| !c.installation)
        .map(line)
        .collect();
    let installation: Vec<String> = catalog
        .iter()
        .filter(|c| c.installation)
        .map(line)
        .collect();
    if !installation.is_empty() {
        lines.push("Installation — runs beside the work, never as a turn".to_string());
        lines.extend(installation);
    }
    lines
}

/// The reason a slash verb that names an excluded installation verb is not
/// offered here, so asking for it is answered rather than called unknown.
fn exclusion_for_verb(verb: &str) -> Option<&'static str> {
    INSTALLATION_EXCLUSIONS
        .iter()
        .find(|(id, _)| id.strip_prefix("core:") == Some(verb))
        .map(|(_, reason)| *reason)
}

/// What submitting a command-bar line resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandOutcome {
    /// Show the help listing (the discovery surface).
    Help,
    /// A recognized command ready to dispatch (executed by the dispatch step).
    Run(SlashCommand),
    /// An inline error to surface to the user.
    Error(String),
}

/// Classify a submitted command-bar line into an outcome.
pub fn classify(input: &str, catalog: &[CommandSpec]) -> CommandOutcome {
    match parse(input) {
        Err(ParseError::NotACommand) => CommandOutcome::Error("commands start with /".to_string()),
        Err(ParseError::Empty) => CommandOutcome::Error("type a command, e.g. /help".to_string()),
        Ok(cmd) if cmd.verb == "help" => CommandOutcome::Help,
        Ok(cmd) if is_known(&cmd.verb, catalog) => CommandOutcome::Run(cmd),
        Ok(cmd) => CommandOutcome::Error(match exclusion_for_verb(&cmd.verb) {
            Some(reason) => format!("/{} is not offered here: {reason}", cmd.verb),
            None => format!("unknown command: /{} (try /help)", cmd.verb),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cronus_contract::{InvocableId, LiveEffect, Locus, Stability};

    #[test]
    fn command_parse_extracts_verb_and_args() {
        let cmd = parse("/goal start my-objective").expect("parses");
        assert_eq!(cmd.verb, "goal");
        assert_eq!(cmd.args, vec!["start", "my-objective"]);
    }

    #[test]
    fn command_parse_ignores_surrounding_whitespace() {
        let cmd = parse("   /status   ").expect("parses");
        assert_eq!(cmd.verb, "status");
        assert!(cmd.args.is_empty());
    }

    #[test]
    fn command_parse_rejects_non_slash_input() {
        assert_eq!(parse("status"), Err(ParseError::NotACommand));
    }

    #[test]
    fn command_parse_rejects_bare_slash() {
        assert_eq!(parse("/"), Err(ParseError::Empty));
        assert_eq!(parse("/   "), Err(ParseError::Empty));
    }

    /// A double-quoted phrase is one argument, not several — without
    /// this, `board block A1 "waiting on dep"` has no TUI spelling at all.
    #[test]
    fn command_parse_keeps_a_double_quoted_phrase_as_one_argument() {
        let cmd = parse(r#"/board block A1 "waiting on dep""#).expect("parses");
        assert_eq!(cmd.verb, "board");
        assert_eq!(cmd.args, vec!["block", "A1", "waiting on dep"]);
    }

    #[test]
    fn command_parse_keeps_a_single_quoted_phrase_as_one_argument() {
        let cmd = parse("/board block A1 'waiting on dep'").expect("parses");
        assert_eq!(cmd.args, vec!["block", "A1", "waiting on dep"]);
    }

    #[test]
    fn command_parse_an_unterminated_quote_runs_to_end_of_line() {
        let cmd = parse(r#"/board block A1 "trailing"#).expect("parses");
        assert_eq!(cmd.args, vec!["block", "A1", "trailing"]);
    }

    #[test]
    fn command_parse_a_quote_mid_token_still_joins_into_one_argument() {
        let cmd = parse(r#"/memory store key pre"mid word"post"#).expect("parses");
        assert_eq!(cmd.args, vec!["store", "key", "premid wordpost"]);
    }

    /// A small, hand-built catalog — deliberately not `build_catalog`'s own
    /// output, so these tests exercise the catalog-*consuming* functions
    /// (`lookup`/`is_known`/`names`/`help_lines`/`classify`) independently of
    /// whether derivation from a registry is itself correct (that is
    /// `build_catalog`'s own test, below).
    fn sample_catalog() -> Vec<CommandSpec> {
        vec![
            CommandSpec {
                name: "help",
                summary: "List available slash commands".to_string(),
                installation: false,
            },
            CommandSpec {
                name: "board",
                summary: "board operations".to_string(),
                installation: false,
            },
            CommandSpec {
                name: "memory",
                summary: "memory operations".to_string(),
                installation: false,
            },
        ]
    }

    #[test]
    fn command_parse_help_lists_known_commands() {
        let catalog = sample_catalog();
        let listing = help_lines(&catalog);
        for verb in ["help", "board", "memory"] {
            assert!(
                listing.iter().any(|l| l.contains(&format!("/{verb} "))),
                "/help should list /{verb}"
            );
        }
        assert_eq!(listing.len(), catalog.len());
    }

    #[test]
    fn command_parse_classifies_help_run_and_unknown() {
        let catalog = sample_catalog();
        assert_eq!(classify("/help", &catalog), CommandOutcome::Help);
        assert!(matches!(
            classify("/board", &catalog),
            CommandOutcome::Run(c) if c.verb == "board"
        ));
        assert!(matches!(
            classify("/frobnicate", &catalog),
            CommandOutcome::Error(_)
        ));
        assert!(matches!(
            classify("hello", &catalog),
            CommandOutcome::Error(_)
        ));
    }

    #[test]
    fn command_parse_catalog_verbs_are_unique() {
        let catalog = sample_catalog();
        let mut seen = std::collections::HashSet::new();
        for name in names(&catalog) {
            assert!(seen.insert(name), "duplicate catalog verb: {name}");
        }
    }

    fn descriptor(
        tail: &str,
        group: &'static str,
        locus: Locus,
        stability: Stability,
    ) -> Invocable {
        Invocable {
            id: InvocableId::new(format!("core:{tail}")).expect("well-formed test id"),
            name: "Test",
            summary: "A test descriptor.",
            group,
            locus,
            binders: Vec::new(),
            stability,
            journal_raw_input: true,
        }
    }

    /// Proves the built catalog's verb set equals the registry's own
    /// `Semantic`+`ClientLocal`+`Installation`+`Shipped` set for a registry
    /// this test populates itself — never a restated literal list, which is
    /// the exact defect this task deletes.
    #[test]
    fn build_catalog_matches_the_registrys_own_projected_shipped_set() {
        let semantic = descriptor("board.list", "board", Locus::Semantic, Stability::Shipped);
        let client_local = descriptor("pane.focus", "pane", Locus::ClientLocal, Stability::Shipped);
        let installation = descriptor(
            "status",
            "status",
            Locus::Installation {
                effect: LiveEffect::Inspect,
            },
            Stability::Shipped,
        );
        let host_only = descriptor(
            "settings.write",
            "settings",
            Locus::HostOnly {
                reason: "host-owned",
            },
            Stability::Shipped,
        );
        let retired = Invocable {
            stability: Stability::Retired {
                superseded_by: InvocableId::new("core:board.list").expect("well-formed test id"),
            },
            ..descriptor(
                "board.old-list",
                "board",
                Locus::Semantic,
                Stability::Shipped,
            )
        };
        let all = [
            &semantic,
            &client_local,
            &installation,
            &host_only,
            &retired,
        ];

        let catalog = build_catalog(&all);
        let names: Vec<&str> = catalog.iter().map(|c| c.name).collect();

        assert!(
            names.contains(&"help"),
            "help is always the discovery entry"
        );
        assert!(
            names.contains(&"board"),
            "a Semantic Shipped group must appear"
        );
        assert!(
            names.contains(&"pane"),
            "a ClientLocal Shipped group must appear"
        );
        assert!(
            names.contains(&"status"),
            "a Shipped Installation group must appear"
        );
        assert!(
            !names.contains(&"settings"),
            "a HostOnly-locus group must not appear on any generic surface"
        );
        assert_eq!(
            names.len(),
            4,
            "exactly help + the three Shipped Semantic/ClientLocal/Installation groups — a \
             Retired descriptor is never a member of the shipped set by construction"
        );
        let status = catalog
            .iter()
            .find(|c| c.name == "status")
            .expect("status is in the catalog");
        assert!(
            status.installation,
            "the installation group is marked as such"
        );
        assert!(
            catalog
                .iter()
                .filter(|c| c.name != "status")
                .all(|c| !c.installation),
            "no other group is marked installation"
        );
    }

    #[test]
    fn a_declared_installation_exclusion_is_not_projected_and_stays_explained() {
        let tui = descriptor(
            "tui",
            "tui",
            Locus::Installation {
                effect: LiveEffect::Inspect,
            },
            Stability::Shipped,
        );
        assert!(!is_projected(&tui), "the declared exclusion is not offered");
        assert!(
            installation_exclusion(&tui.id).is_some_and(|reason| !reason.is_empty()),
            "and it carries its reason"
        );
        let catalog = build_catalog(&[&tui]);
        assert!(catalog.iter().all(|c| c.name != "tui"));

        let other = descriptor(
            "doctor",
            "doctor",
            Locus::Installation {
                effect: LiveEffect::Inspect,
            },
            Stability::Shipped,
        );
        assert!(is_projected(&other));
        assert!(installation_exclusion(&other.id).is_none());
    }

    #[test]
    fn asking_for_an_excluded_verb_is_answered_with_its_reason_not_called_unknown() {
        let catalog = sample_catalog();
        match classify("/tui", &catalog) {
            CommandOutcome::Error(message) => {
                assert!(message.contains("not offered here"), "{message}");
                assert!(!message.contains("unknown command"), "{message}");
            }
            other => panic!("expected an explained refusal, got {other:?}"),
        }
        match classify("/frobnicate", &catalog) {
            CommandOutcome::Error(message) => assert!(message.contains("unknown command")),
            other => panic!("expected the unknown-command error, got {other:?}"),
        }
    }

    #[test]
    fn help_sets_the_installation_verbs_apart_under_their_own_heading() {
        let mut catalog = sample_catalog();
        catalog.push(CommandSpec {
            name: "doctor",
            summary: "Run health checks".to_string(),
            installation: true,
        });
        let listing = help_lines(&catalog);
        let heading = listing
            .iter()
            .position(|line| line.starts_with("Installation"))
            .expect("a heading appears when an installation group exists");
        let doctor = listing
            .iter()
            .position(|line| line.starts_with("/doctor"))
            .expect("the installation verb is listed");
        let board = listing
            .iter()
            .position(|line| line.starts_with("/board"))
            .expect("work commands stay listed");
        assert!(board < heading && heading < doctor);
    }

    #[test]
    fn build_catalog_deduplicates_multiple_invocables_in_one_group() {
        let list = descriptor("board.list", "board", Locus::Semantic, Stability::Shipped);
        let add = descriptor("board.add", "board", Locus::Semantic, Stability::Shipped);
        let catalog = build_catalog(&[&list, &add]);
        let board_entries = catalog.iter().filter(|c| c.name == "board").count();
        assert_eq!(
            board_entries, 1,
            "a group with several invocables still yields exactly one catalog entry"
        );
    }

    #[test]
    fn parity_matrix_crate_depends_on_core_not_the_cli() {
        // Structural inward-dependency guard: the manifest links the engine tiers it
        // actually needs — `cronus-domain` (Capabilities/Engine/redact),
        // `cronus-contract` (the descriptor types this module consumes), and
        // `cronus-core` (the facade composing the registry) — never the CLI
        // frontend.
        let manifest = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"));
        assert!(
            manifest.contains("cronus-domain = { workspace = true }"),
            "the TUI must link the domain crate (cronus-domain)"
        );
        assert!(
            manifest.contains("cronus-contract = { workspace = true }"),
            "the TUI must link the ports crate (cronus-contract) for descriptor types"
        );
        assert!(
            manifest.contains("cronus-core = { workspace = true }"),
            "the TUI must link the facade crate (cronus-core) for the invocable registry"
        );
        assert!(
            !manifest.contains("cronus-cli"),
            "the TUI must not depend on cronus-cli (inward dependency)"
        );
    }
}
