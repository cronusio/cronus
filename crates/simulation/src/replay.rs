//! Replaying a pinned route with no agent in the loop.
//!
//! The guarding half of route replay: a route
//! discovered by the expensive free-route agent runner is distilled into a
//! [`ReplayCase`], and replaying it costs nothing more than an ordinary
//! `cargo test` — no vantage, no judgement, no inference. Everything
//! interesting happened when the route was discovered; a replay's only job
//! is to notice when the route stops holding.

use std::fmt;

use serde::Deserialize;

use crate::product::resolve_binary;
use crate::transcript::Transcript;
use crate::world::{World, WorldError};

/// One pinned route: a sequence of invocations and the exit code each one
/// must produce. Deliberately narrower than a full [`crate::transcript::Entry`]
/// — a replay case asserts the outcome a route is pinned *for*, not every
/// byte the discovery run happened to observe.
#[derive(Debug, Deserialize)]
pub struct ReplayCase {
    pub id: String,
    #[serde(rename = "step")]
    pub steps: Vec<ReplayStep>,
}

#[derive(Debug, Deserialize)]
pub struct ReplayStep {
    pub argv: Vec<String>,
    pub expect_exit: i32,
}

/// A replay failure.
#[derive(Debug)]
pub enum ReplayError {
    Parse(String),
    World(WorldError),
    Io(std::io::Error),
    BinaryNotFound,
    /// A step's actual exit code did not match what the case pinned.
    StepDiverged {
        case_id: String,
        step_index: usize,
        argv: Vec<String>,
        expected_exit: i32,
        actual_exit: Option<i32>,
        stderr: String,
    },
}

impl fmt::Display for ReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReplayError::Parse(msg) => write!(f, "failed to parse replay case: {msg}"),
            ReplayError::World(err) => write!(f, "{err}"),
            ReplayError::Io(err) => write!(f, "replay I/O error: {err}"),
            ReplayError::BinaryNotFound => {
                write!(f, "could not resolve the `cronus` binary to replay against")
            }
            ReplayError::StepDiverged {
                case_id,
                step_index,
                argv,
                expected_exit,
                actual_exit,
                stderr,
            } => write!(
                f,
                "replay case `{case_id}` diverged at step {step_index} ({argv:?}): \
                 expected exit {expected_exit}, got {actual_exit:?}. stderr: {stderr}"
            ),
        }
    }
}

impl std::error::Error for ReplayError {}

/// Parse a replay case from its TOML text.
pub fn parse(text: &str) -> Result<ReplayCase, ReplayError> {
    toml::from_str(text).map_err(|err| ReplayError::Parse(err.to_string()))
}

/// Build a fresh world, replay every step of `case` against the real
/// `cronus` binary in order, and assert each one's exit code against what
/// the case pinned. Tears the world down whether the replay passed or
/// diverged; a diverged run leaves nothing behind for the next one to
/// inherit.
pub fn run(case: &ReplayCase) -> Result<(), ReplayError> {
    let binary = resolve_binary("cronus").ok_or(ReplayError::BinaryNotFound)?;
    let world = World::build(&format!("replay-{}", case.id)).map_err(ReplayError::World)?;
    let mut transcript = Transcript::new(&world, binary);

    for (step_index, step) in case.steps.iter().enumerate() {
        let argv: Vec<&str> = step.argv.iter().map(String::as_str).collect();
        let idx = match transcript.record(&world, &argv, b"") {
            Ok(idx) => idx,
            Err(err) => {
                let _ = world.teardown();
                return Err(ReplayError::Io(err));
            }
        };
        let entry = transcript.entry(idx);

        if entry.exit_code != Some(step.expect_exit) {
            let diverged = ReplayError::StepDiverged {
                case_id: case.id.clone(),
                step_index,
                argv: step.argv.clone(),
                expected_exit: step.expect_exit,
                actual_exit: entry.exit_code,
                stderr: String::from_utf8_lossy(&entry.stderr).into_owned(),
            };
            let _ = world.teardown();
            return Err(diverged);
        }
    }

    world.teardown().map_err(ReplayError::World)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_then_status_case() -> ReplayCase {
        parse(
            r#"
            id = "init-then-status"

            [[step]]
            argv = ["init"]
            expect_exit = 0

            [[step]]
            argv = ["status"]
            expect_exit = 0
            "#,
        )
        .expect("valid inline case must parse")
    }

    #[test]
    fn parses_a_well_formed_case() {
        let case = init_then_status_case();
        assert_eq!(case.id, "init-then-status");
        assert_eq!(case.steps.len(), 2);
        assert_eq!(case.steps[0].argv, vec!["init"]);
        assert_eq!(case.steps[1].argv, vec!["status"]);
    }

    #[test]
    fn a_wrong_expected_exit_code_is_caught_by_the_positive_control() {
        crate::product::test_cronus_binary();
        let mut case = init_then_status_case();
        // Deliberately wrong on purpose: `init` cannot exit 99. This is the
        // positive control the discipline asks for — if this test ever
        // passes, `run()` has stopped checking anything.
        case.steps[0].expect_exit = 99;

        let err = run(&case).expect_err("a wrong expectation must fail the replay");
        let message = err.to_string();
        assert!(
            message.contains("init-then-status"),
            "message must name the case: {message}"
        );
        assert!(
            message.contains("99"),
            "message must name the expected exit code: {message}"
        );
    }
}
