//! The as-it-happens record of a run.
//!
//! Every claim a run makes cites what
//! the product actually emitted, and observed output is the only evidence.
//! An [`Entry`] is written the moment its invocation completes — never
//! reconstructed afterward from the actor's own account of what it did —
//! and is addressable by index, so a verdict cites `entries[i]` rather than
//! restating output.

use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::world::World;

/// One recorded product invocation.
#[derive(Debug, Clone)]
pub struct Entry {
    pub argv: Vec<String>,
    pub stdin: Vec<u8>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// `None` when the process was terminated by a signal rather than
    /// exiting normally (Unix) — a real outcome, not an error, and never
    /// coerced into a fabricated code.
    pub exit_code: Option<i32>,
    pub duration: Duration,
    /// [`World::state_digest`] taken immediately before this invocation
    /// was spawned.
    pub state_digest_before: u64,
    /// [`World::state_digest`] taken immediately after it exited.
    pub state_digest_after: u64,
}

impl Entry {
    /// Whether this invocation changed anything inside the world.
    pub fn changed_world(&self) -> bool {
        self.state_digest_before != self.state_digest_after
    }
}

/// The append-only record for one world's run.
#[derive(Debug)]
pub struct Transcript {
    pub world_id: String,
    pub product_version: &'static str,
    pub resolved_binary: PathBuf,
    entries: Vec<Entry>,
}

impl Transcript {
    pub fn new(world: &World, resolved_binary: PathBuf) -> Self {
        Transcript {
            world_id: world.id().to_string(),
            product_version: world.product_version,
            resolved_binary,
            entries: Vec::new(),
        }
    }

    /// Spawn `self.resolved_binary` with `argv` inside `world`, feeding it
    /// `stdin`, and append the resulting [`Entry`] the moment the process
    /// exits. Returns the new entry's index.
    pub fn record(&mut self, world: &World, argv: &[&str], stdin: &[u8]) -> io::Result<usize> {
        let digest_before = world.state_digest()?;
        let started = Instant::now();

        let mut child = Command::new(&self.resolved_binary)
            .args(argv)
            .current_dir(world.cwd())
            .env_clear()
            .envs(world.child_env(std::env::vars()))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        {
            use std::io::Write;
            let mut child_stdin = child.stdin.take().expect("stdin was piped");
            // A child that never reads stdin (the common case) must not
            // hang this call: a broken-pipe write is expected, not an error
            // worth propagating.
            let _ = child_stdin.write_all(stdin);
        }

        let output = child.wait_with_output()?;
        let duration = started.elapsed();
        let digest_after = world.state_digest()?;

        let entry = Entry {
            argv: argv.iter().map(|s| s.to_string()).collect(),
            stdin: stdin.to_vec(),
            stdout: output.stdout,
            stderr: output.stderr,
            exit_code: output.status.code(),
            duration,
            state_digest_before: digest_before,
            state_digest_after: digest_after,
        };
        self.entries.push(entry);
        Ok(self.entries.len() - 1)
    }

    pub fn entry(&self, index: usize) -> &Entry {
        &self.entries[index]
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::product::test_cronus_binary as cronus_binary;

    #[test]
    fn a_help_invocation_is_recorded_byte_for_byte_with_positive_duration() {
        let world = World::build("transcript-help").expect("build world");
        let mut transcript = Transcript::new(&world, cronus_binary());

        let expected = Command::new(&transcript.resolved_binary)
            .arg("--help")
            .current_dir(world.cwd())
            .output()
            .expect("direct spawn for comparison");

        let idx = transcript
            .record(&world, &["--help"], b"")
            .expect("record --help");
        let entry = transcript.entry(idx);

        assert_eq!(
            entry.stdout, expected.stdout,
            "stdout must match byte-for-byte"
        );
        assert!(entry.duration.as_nanos() > 0, "duration must be positive");
        assert_eq!(entry.exit_code, Some(0));

        world.teardown().expect("teardown");
    }

    #[test]
    fn a_failing_invocation_is_still_recorded_with_its_stderr() {
        let world = World::build("transcript-fail").expect("build world");
        let mut transcript = Transcript::new(&world, cronus_binary());

        let idx = transcript
            .record(&world, &["totally-unknown-cmd"], b"")
            .expect("record an invocation that will exit non-zero");
        let entry = transcript.entry(idx);

        assert_ne!(
            entry.exit_code,
            Some(0),
            "an unknown command must not exit 0"
        );
        assert!(
            !entry.stderr.is_empty(),
            "a failing invocation must still capture stderr"
        );

        world.teardown().expect("teardown");
    }

    #[test]
    fn state_digest_differs_only_for_an_invocation_that_writes() {
        let world = World::build("transcript-digest").expect("build world");
        let mut transcript = Transcript::new(&world, cronus_binary());

        let help_idx = transcript
            .record(&world, &["--help"], b"")
            .expect("record --help");
        assert!(
            !transcript.entry(help_idx).changed_world(),
            "a read-only invocation must leave the world's digest unchanged"
        );

        let init_idx = transcript
            .record(&world, &["init"], b"")
            .expect("record init");
        assert!(
            transcript.entry(init_idx).changed_world(),
            "`cronus init` writes `.cronus/app.json` and must change the world's digest"
        );

        world.teardown().expect("teardown");
    }
}
