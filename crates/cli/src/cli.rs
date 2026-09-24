use clap::Parser;

use crate::output::OutputFormat;

/// The exit-code taxonomy and the `--format`/diagnostics split, shown
/// after `--help` (the long form) rather than repeated on every verb's own
/// help text. The taxonomy itself was already this small and consistent —
/// what was missing was writing it down anywhere a user could find it.
const EXIT_CODE_HELP: &str = "\
Exit codes:
  0  success
  1  the command ran but did not succeed (not found, rejected input,
     a quality-gate escalation, ...) — the common \"no\" from a verb
     that otherwise understood the request
  2  usage error — unrecognized command, missing/malformed argument;
     printed by the argument parser before any verb runs
  3  internal error — composition itself failed (not a verb's own
     result)

Diagnostics and --format:
  --format governs the success payload on stdout only. Every error,
  rejection, and usage message is always English prose on stderr,
  regardless of --format — there is no JSON error shape to parse.
";

/// The launcher's own top-level grammar: just the global `--format` flag and
/// the `--help`/`--version` clap provides for free. Every verb this product
/// ships now comes from one of two projections built at runtime
/// (`crate::installation`'s hand-declared grammar, `crate::generated`'s
/// registry-derived one) rather than from a field on this struct — a
/// compile-time subcommand enum is exactly the shape forbidden as a
/// source of truth, since an unshipped action would need to be representable
/// in it.
#[derive(Parser)]
#[command(
    name = "cronus",
    about = "Cronus — workflow automation toolkit",
    version,
    after_long_help = EXIT_CODE_HELP
)]
pub struct Cli {
    /// Output format
    #[arg(long, short = 'f', global = true, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

/// Bound at runtime by the installation half's own generated grammar now
/// (`crate::installation`, `activation enable --mode`) — the `ArchetypeCommand`/
/// `BackupCommand`/`ActivationCommand`/`WorkspaceCommand` enums those verbs
/// used to derive from are tombstoned, but `enable`'s handler still takes
/// this small value type rather than a bare string.
#[derive(Clone, Copy, clap::ValueEnum)]
pub enum ActivationModeArg {
    /// Starts with the user's session; ends when it ends; no elevation
    Login,
    /// Boot-started, survives logout; registration requires elevation
    System,
}
