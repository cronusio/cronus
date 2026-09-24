//! Cross-process state for one `cronus-sim` run.
//!
//! `world`, `run`, `note`, `verdict`, and `finish` are separate CLI
//! invocations — that is the entire point of the wrapper:
//! an agent issues them one at a time, the way it
//! issues any other shell command. Everything they need to share therefore
//! lives in one JSON file inside the world's own root, so it disappears
//! with the world on teardown rather than lingering.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::findings::{Discovery, DiscoveryClass, FinishReport, RunOutcome};
use crate::scenario::Scenario;
use crate::world::World;

const STATE_FILE_NAME: &str = "run-state.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryState {
    pub argv: Vec<String>,
    /// Lossy UTF-8 — the cross-process persisted form trades exact-byte
    /// fidelity (which [`crate::transcript::Transcript`] keeps, in-process)
    /// for a value that survives a plain JSON round trip. Every scenario
    /// this harness drives emits well-formed text; a future binary output
    /// class would need a different field, not a silent truncation here.
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub state_digest_before: u64,
    pub state_digest_after: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerdictState {
    pub passed: bool,
    pub cites: Vec<usize>,
}

/// Persisted run state — one JSON file per world.
#[derive(Debug, Serialize, Deserialize)]
pub struct RunState {
    pub world_id: String,
    pub root: PathBuf,
    pub resolved_binary: PathBuf,
    pub product_version: String,
    /// Captured once, at `create()` time. The baseline `finish` compares a
    /// fresh recomputation against to detect contamination.
    pub repo_dirty_digest_at_build: Option<String>,
    pub bound_steps: u32,
    pub bound_wall_secs: u32,
    /// The scenario's declared dollar ceiling on the *driving agent's own*
    /// cost while it explores — the product itself has no notion of a
    /// dollar, so only [`Self::record_spend`] can move this, never
    /// [`Self::record_run`] on its own.
    pub bound_spend_usd: f64,
    pub created_at_unix_ms: u128,
    pub obligation_ids: Vec<String>,
    #[serde(default)]
    pub entries: Vec<EntryState>,
    #[serde(default)]
    pub verdicts: BTreeMap<String, VerdictState>,
    #[serde(default)]
    pub notes: Vec<Discovery>,
    /// Cumulative spend reported so far via [`Self::record_spend`].
    #[serde(default)]
    pub spend_usd_used: f64,
}

#[derive(Debug)]
pub enum RunStateError {
    World(crate::world::WorldError),
    Io(std::io::Error),
    Serde(String),
    UnknownWorld(String),
    UnknownObligation(String),
    UnknownEntry(usize),
    BoundExhausted {
        entries_used: u32,
        bound_steps: u32,
        wall_elapsed_secs: u64,
        bound_wall_secs: u32,
        spend_usd_used: f64,
        bound_spend_usd: f64,
    },
}

impl fmt::Display for RunStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RunStateError::World(err) => write!(f, "{err}"),
            RunStateError::Io(err) => write!(f, "run-state I/O error: {err}"),
            RunStateError::Serde(msg) => write!(f, "run-state serialization error: {msg}"),
            RunStateError::UnknownWorld(id) => {
                write!(f, "no such world `{id}` — was it built with `world new`?")
            }
            RunStateError::UnknownObligation(id) => {
                write!(
                    f,
                    "obligation `{id}` was not declared by this world's scenario"
                )
            }
            RunStateError::UnknownEntry(idx) => {
                write!(f, "cited transcript entry {idx} does not exist")
            }
            RunStateError::BoundExhausted {
                entries_used,
                bound_steps,
                wall_elapsed_secs,
                bound_wall_secs,
                spend_usd_used,
                bound_spend_usd,
            } => write!(
                f,
                "this world's bound is exhausted ({entries_used}/{bound_steps} steps, \
                 {wall_elapsed_secs}/{bound_wall_secs}s elapsed, \
                 ${spend_usd_used:.2}/${bound_spend_usd:.2} spent) — call finish"
            ),
        }
    }
}

impl std::error::Error for RunStateError {}

impl RunState {
    fn state_path(root: &Path) -> PathBuf {
        root.join(STATE_FILE_NAME)
    }

    /// Build a fresh world for `scenario` and persist its initial state.
    pub fn create(resolved_binary: PathBuf, scenario: &Scenario) -> Result<Self, RunStateError> {
        let world = World::build(&scenario.id).map_err(RunStateError::World)?;
        let state = RunState {
            world_id: world.id().to_string(),
            root: world.root().to_path_buf(),
            resolved_binary,
            product_version: world.product_version.to_string(),
            repo_dirty_digest_at_build: world.repo_dirty_digest.clone(),
            bound_steps: scenario.bound.steps,
            bound_wall_secs: scenario.bound.wall_secs,
            bound_spend_usd: scenario.bound.spend_usd,
            created_at_unix_ms: now_unix_ms(),
            obligation_ids: scenario.obligations.iter().map(|o| o.id.clone()).collect(),
            entries: Vec::new(),
            verdicts: BTreeMap::new(),
            notes: Vec::new(),
            spend_usd_used: 0.0,
        };
        // `world` owns the directory it just created; letting it fall out
        // of scope here does NOT delete anything — teardown is an explicit
        // consuming method, never a Drop impl. The directory must survive
        // this call, since the whole point is a later, separate process
        // finding it again by id.
        state.save()?;
        Ok(state)
    }

    pub fn load(world_id: &str) -> Result<Self, RunStateError> {
        let root = World::root_for_id(world_id);
        let path = Self::state_path(&root);
        let text = std::fs::read_to_string(&path).map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                RunStateError::UnknownWorld(world_id.to_string())
            } else {
                RunStateError::Io(err)
            }
        })?;
        serde_json::from_str(&text).map_err(|err| RunStateError::Serde(err.to_string()))
    }

    fn save(&self) -> Result<(), RunStateError> {
        let text = serde_json::to_string_pretty(self)
            .map_err(|err| RunStateError::Serde(err.to_string()))?;
        std::fs::write(Self::state_path(&self.root), text).map_err(RunStateError::Io)
    }

    fn attached(&self) -> World {
        World::attach(self.root.clone())
    }

    fn wall_elapsed_secs(&self) -> u64 {
        let now = now_unix_ms();
        (now.saturating_sub(self.created_at_unix_ms) / 1000) as u64
    }

    /// `Some(..)` once any one of the three declared bounds (steps, wall
    /// clock, dollar spend) has been reached — a pure snapshot, so it can
    /// be checked (and tested) without spawning anything. Three meters,
    /// one gate: `record_run` refuses to spawn once any of them trips.
    fn bound_status(&self) -> Option<RunStateError> {
        let steps_exhausted = self.entries.len() as u32 >= self.bound_steps;
        let wall_exhausted = self.wall_elapsed_secs() >= u64::from(self.bound_wall_secs);
        let spend_exhausted = self.spend_usd_used >= self.bound_spend_usd;

        if steps_exhausted || wall_exhausted || spend_exhausted {
            Some(RunStateError::BoundExhausted {
                entries_used: self.entries.len() as u32,
                bound_steps: self.bound_steps,
                wall_elapsed_secs: self.wall_elapsed_secs(),
                bound_wall_secs: self.bound_wall_secs,
                spend_usd_used: self.spend_usd_used,
                bound_spend_usd: self.bound_spend_usd,
            })
        } else {
            None
        }
    }

    /// Spawn `argv` inside this world and append the resulting entry.
    /// Refuses (without spawning anything) once the scenario's declared
    /// bound is exhausted — the mechanism that stops a stuck actor from
    /// spending without limit on a goal the product cannot satisfy.
    pub fn record_run(&mut self, argv: &[String]) -> Result<usize, RunStateError> {
        if let Some(err) = self.bound_status() {
            return Err(err);
        }

        let world = self.attached();
        let digest_before = world.state_digest().map_err(RunStateError::Io)?;
        let started = Instant::now();

        let child = Command::new(&self.resolved_binary)
            .args(argv)
            .current_dir(world.cwd())
            .env_clear()
            .envs(world.child_env(std::env::vars()))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(RunStateError::Io)?;
        let output = child.wait_with_output().map_err(RunStateError::Io)?;
        let duration_ms = started.elapsed().as_millis() as u64;
        let digest_after = world.state_digest().map_err(RunStateError::Io)?;

        let entry = EntryState {
            argv: argv.to_vec(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            exit_code: output.status.code(),
            duration_ms,
            state_digest_before: digest_before,
            state_digest_after: digest_after,
        };
        self.entries.push(entry);
        let idx = self.entries.len() - 1;
        self.save()?;
        Ok(idx)
    }

    pub fn entry(&self, index: usize) -> Option<&EntryState> {
        self.entries.get(index)
    }

    /// Append a discovery, optionally classified against the
    /// improvement-loop taxonomy and optionally carrying a proposed remedy.
    /// Neither addition is consulted when computing an outcome
    /// (unchanged) — a discovery is information, not an obligation,
    /// classified or not — and a remedy here is a claim recorded for a
    /// human to weigh, never an act this call performs.
    pub fn add_note(
        &mut self,
        text: String,
        class: Option<DiscoveryClass>,
        remedy: Option<String>,
    ) -> Result<(), RunStateError> {
        self.notes.push(Discovery {
            text,
            class,
            remedy,
        });
        self.save()
    }

    /// Record incremental dollar spend the driving agent itself incurred
    /// while producing the step(s) since its last report — its own token
    /// cost, not anything the product can measure, which is why this is a
    /// separate, explicit call rather than something `record_run` infers.
    /// Always succeeds (recording a cost is never itself refusable, the
    /// same way `add_note` never is); the bound it feeds is enforced the
    /// next time `record_run` is attempted, exactly like the wall-clock
    /// meter, which also accumulates passively between calls.
    pub fn record_spend(&mut self, usd: f64) -> Result<(), RunStateError> {
        self.spend_usd_used += usd;
        self.save()
    }

    pub fn record_verdict(
        &mut self,
        obligation_id: &str,
        passed: bool,
        cites: Vec<usize>,
    ) -> Result<(), RunStateError> {
        if !self.obligation_ids.iter().any(|id| id == obligation_id) {
            return Err(RunStateError::UnknownObligation(obligation_id.to_string()));
        }
        for &idx in &cites {
            if idx >= self.entries.len() {
                return Err(RunStateError::UnknownEntry(idx));
            }
        }
        self.verdicts
            .insert(obligation_id.to_string(), VerdictState { passed, cites });
        self.save()
    }

    /// Compute the outcome from obligation verdicts alone, tear the world
    /// down, and report. Order matters: contamination is checked first
    /// because it invalidates every verdict regardless of what they said;
    /// undecided obligations are checked next because an incomplete run
    /// has not actually produced a pass or a fail yet.
    pub fn finish(self) -> Result<FinishReport, RunStateError> {
        let world = self.attached();
        let current_digest = world.current_repo_dirty_digest();

        let contaminated = matches!(
            (&self.repo_dirty_digest_at_build, &current_digest),
            (Some(before), Some(after)) if before != after
        );

        let undecided: Vec<String> = self
            .obligation_ids
            .iter()
            .filter(|id| !self.verdicts.contains_key(id.as_str()))
            .cloned()
            .collect();

        let outcome = if contaminated {
            RunOutcome::Contaminated
        } else if !undecided.is_empty() {
            RunOutcome::Incomplete
        } else if self.verdicts.values().any(|v| !v.passed) {
            RunOutcome::Fail
        } else {
            RunOutcome::Pass
        };

        let mut report = FinishReport {
            world_id: self.world_id.clone(),
            outcome,
            undecided_obligations: undecided,
            notes: self.notes.clone(),
        };

        // A world that cannot be torn down is an environment failure, not
        // a product one — it must never override an already-computed
        // Contaminated verdict (both point at the environment, and
        // Contaminated is the more specific, already-established finding).
        if world.teardown().is_err() && report.outcome != RunOutcome::Contaminated {
            report.outcome = RunOutcome::Environment;
        }

        Ok(report)
    }
}

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal, in-memory-shaped `RunState` with generous steps/wall
    /// bounds and a tight, caller-supplied dollar bound — everything this
    /// module's bound logic needs, none of it backed by a real spawned
    /// process or a real world directory (this state is never `save()`d
    /// in these tests, so `root` never needs to exist).
    fn state_with_spend_bound(bound_spend_usd: f64) -> RunState {
        RunState {
            world_id: "spend-test-world".to_string(),
            root: std::env::temp_dir().join("cronus-sim-spend-bound-test-unused"),
            resolved_binary: PathBuf::from("cronus"),
            product_version: "0.0.0".to_string(),
            repo_dirty_digest_at_build: None,
            bound_steps: 1_000,
            bound_wall_secs: 1_000,
            bound_spend_usd,
            created_at_unix_ms: now_unix_ms(),
            obligation_ids: Vec::new(),
            entries: Vec::new(),
            verdicts: BTreeMap::new(),
            notes: Vec::new(),
            spend_usd_used: 0.0,
        }
    }

    #[test]
    fn bound_status_is_none_while_spend_stays_under_the_declared_ceiling() {
        let mut state = state_with_spend_bound(1.00);
        state.spend_usd_used = 0.99;
        assert!(state.bound_status().is_none());
    }

    #[test]
    fn bound_status_trips_once_accumulated_spend_reaches_the_declared_ceiling() {
        let mut state = state_with_spend_bound(1.00);
        state.spend_usd_used = 1.00;
        let err = state
            .bound_status()
            .expect("spend at exactly the bound must trip it");
        match err {
            RunStateError::BoundExhausted {
                spend_usd_used,
                bound_spend_usd,
                ..
            } => {
                assert_eq!(spend_usd_used, 1.00);
                assert_eq!(bound_spend_usd, 1.00);
            }
            other => panic!("expected BoundExhausted, got {other}"),
        }
    }

    #[test]
    fn record_spend_accumulates_across_calls_rather_than_replacing() {
        let mut state = state_with_spend_bound(10.0);
        // `save()` needs `root` to exist; these tests only assert on the
        // in-memory accumulator, not on the persisted file, so give it a
        // real (temporary) directory rather than special-casing save().
        std::fs::create_dir_all(&state.root).expect("create test world root");

        state.record_spend(0.30).expect("first report must succeed");
        state
            .record_spend(0.25)
            .expect("second report must succeed");
        assert_eq!(state.spend_usd_used, 0.55);

        let _ = std::fs::remove_dir_all(&state.root);
    }

    /// A `RunState` whose `root` actually agrees with what
    /// [`World::root_for_id`] computes for `id` — unlike
    /// [`state_with_spend_bound`], which deliberately never round-trips
    /// through [`RunState::load`]. These tests need a real reload, so the
    /// two must match.
    fn persistable_state(id: &str) -> RunState {
        RunState {
            world_id: id.to_string(),
            root: World::root_for_id(id),
            resolved_binary: PathBuf::from("cronus"),
            product_version: "0.0.0".to_string(),
            repo_dirty_digest_at_build: None,
            bound_steps: 1_000,
            bound_wall_secs: 1_000,
            bound_spend_usd: 100.0,
            created_at_unix_ms: now_unix_ms(),
            obligation_ids: Vec::new(),
            entries: Vec::new(),
            verdicts: BTreeMap::new(),
            notes: Vec::new(),
            spend_usd_used: 0.0,
        }
    }

    #[test]
    fn discoveries_of_every_shape_persist_and_reload_through_run_state_json() {
        let id = "discovery-persist-test-world";
        let mut state = persistable_state(id);
        std::fs::create_dir_all(&state.root).expect("create test world root");

        state
            .add_note(
                "the scaffold truncates hyphenated names".to_string(),
                Some(DiscoveryClass::Defect),
                Some("stop splitting on the first hyphen".to_string()),
            )
            .expect("fully-classified discovery must save");
        state
            .add_note("just a plain observation".to_string(), None, None)
            .expect("text-only discovery must save");
        state
            .add_note(
                "search is literal, not semantic".to_string(),
                Some(DiscoveryClass::Friction),
                None,
            )
            .expect("classified discovery with no remedy must save");

        let reloaded = RunState::load(id).expect("world must reload");
        assert_eq!(reloaded.notes.len(), 3);

        assert_eq!(
            reloaded.notes[0].text,
            "the scaffold truncates hyphenated names"
        );
        assert_eq!(reloaded.notes[0].class, Some(DiscoveryClass::Defect));
        assert_eq!(
            reloaded.notes[0].remedy.as_deref(),
            Some("stop splitting on the first hyphen")
        );

        assert_eq!(reloaded.notes[1].text, "just a plain observation");
        assert_eq!(reloaded.notes[1].class, None);
        assert_eq!(reloaded.notes[1].remedy, None);

        assert_eq!(reloaded.notes[2].text, "search is literal, not semantic");
        assert_eq!(reloaded.notes[2].class, Some(DiscoveryClass::Friction));
        assert_eq!(reloaded.notes[2].remedy, None);

        let _ = std::fs::remove_dir_all(&state.root);
    }

    #[test]
    fn a_run_state_json_with_no_notes_key_at_all_still_loads_with_an_empty_discovery_list() {
        // Simulates a schema older than the two new `Discovery` fields
        // ever existing at the `RunState` level — not merely a `Discovery`
        // missing `class`/`remedy` (covered in `findings.rs`), but a whole
        // persisted world whose `notes` key is absent entirely. The field's
        // `#[serde(default)]` must still produce an empty list rather than
        // a load failure.
        let id = "discovery-legacy-load-test-world";
        let root = World::root_for_id(id);
        std::fs::create_dir_all(&root).expect("create test world root");

        let raw = r#"{
            "world_id": "discovery-legacy-load-test-world",
            "root": "dummy-root",
            "resolved_binary": "cronus",
            "product_version": "0.0.0",
            "repo_dirty_digest_at_build": null,
            "bound_steps": 10,
            "bound_wall_secs": 60,
            "bound_spend_usd": 5.0,
            "created_at_unix_ms": 0,
            "obligation_ids": [],
            "entries": [],
            "verdicts": {}
        }"#;
        std::fs::write(root.join(STATE_FILE_NAME), raw).expect("write legacy-shaped state");

        let loaded =
            RunState::load(id).expect("must load despite missing notes/spend_usd_used keys");
        assert!(loaded.notes.is_empty());
        assert_eq!(loaded.spend_usd_used, 0.0);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn record_spend_never_itself_refuses_even_once_it_crosses_the_bound() {
        // Mirrors the wall-clock meter: crossing a bound is discovered the
        // next time `record_run` is attempted, not at the moment the meter
        // ticks past it — reporting a cost is information, like a note,
        // never an action that can itself be rejected.
        let mut state = state_with_spend_bound(1.00);
        std::fs::create_dir_all(&state.root).expect("create test world root");

        state
            .record_spend(5.00)
            .expect("reporting spend must succeed even when it exceeds the bound");
        assert_eq!(state.spend_usd_used, 5.00);
        assert!(
            state.bound_status().is_some(),
            "the next bound check must now see the crossed ceiling"
        );

        let _ = std::fs::remove_dir_all(&state.root);
    }
}
