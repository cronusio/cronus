//! End-to-end coverage of the `cronus-sim` wrapper: every subcommand is
//! driven as a real, separate subprocess (exactly how an agent uses it —
//! `world new`, then `run`/`note`/`verdict` one call at a time, then
//! `finish`), never through the library API directly. This is the only
//! place that proves the *wrapper's* cross-process persistence actually
//! works, as opposed to the in-process library it wraps.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use cronus_simulation::run_state::RunState;

fn cronus_sim_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cronus-sim"))
}

/// Contamination detection reads the *whole repository's* `git status`,
/// which is process-wide state — `cargo test` runs the functions in this
/// file concurrently by default, so without this lock the contamination
/// test's deliberate probe file would corrupt every other test's digest
/// comparison mid-flight (and did, the first time this file ran). Every
/// test acquires this for its full body; it serializes only the tests in
/// *this* file, not the rest of the suite.
fn serialize_repo_access() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    match LOCK.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Duplicates `cronus_simulation::product::test_cronus_binary` (which is
/// `#[cfg(test)]`-gated inside the lib and therefore invisible to an
/// integration test, which links only the lib's non-test build): a bare
/// workspace-level `cargo test` gives no build-order guarantee between
/// `cronus-simulation` and the unrelated `cronus-cli` package, so this
/// forces the build rather than racing it.
fn ensure_cronus_binary() -> PathBuf {
    static RESOLVED: OnceLock<PathBuf> = OnceLock::new();
    RESOLVED
        .get_or_init(|| {
            let status = Command::new(env!("CARGO"))
                .args(["build", "-p", "cronus-cli", "--bin", "cronus"])
                .status()
                .expect("failed to invoke cargo build");
            assert!(
                status.success(),
                "cargo build -p cronus-cli --bin cronus must succeed"
            );
            cronus_simulation::product::resolve_binary("cronus")
                .expect("cronus must resolve immediately after building it")
        })
        .clone()
}

struct SimOutput {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run_sim(args: &[&str]) -> SimOutput {
    let output = Command::new(cronus_sim_bin())
        .args(args)
        .output()
        .expect("failed to spawn cronus-sim");
    SimOutput {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn world_new(scenario_path: &Path) -> String {
    let cronus = ensure_cronus_binary();
    let out = run_sim(&[
        "world",
        "new",
        "--bin",
        cronus.to_str().expect("path must be valid UTF-8"),
        scenario_path.to_str().expect("path must be valid UTF-8"),
    ]);
    assert_eq!(
        out.code, 0,
        "world new must succeed: stdout={} stderr={}",
        out.stdout, out.stderr
    );
    out.stdout.trim().to_string()
}

/// A minimal, valid scenario with `obligation_ids.len()` obligations, no
/// perturbations (perturbation count is irrelevant to this test file —
/// only the smoke-labelling reads it, exercised
/// in `scenario.rs`'s own tests), and the given bound.
fn write_scenario(
    file_stem: &str,
    bound_steps: u32,
    bound_wall_secs: u32,
    bound_spend_usd: f64,
    obligation_ids: &[&str],
) -> PathBuf {
    let mut obligations_toml = String::new();
    for id in obligation_ids {
        obligations_toml += &format!(
            "[[obligation]]\n\
             id = \"{id}\"\n\
             statement = \"the {id} condition holds at the end of the run\"\n\
             decided_by = \"observation-of-output\"\n\
             evidence = \"an invocation whose output supports this specific determination\"\n\n"
        );
    }
    let text = format!(
        "---\n\
         id = \"{file_stem}\"\n\
         tier = \"short\"\n\
         surfaces = [\"cli\"]\n\
         covers = []\n\
         roles = [\"owner\"]\n\
         vantage = \"builtin-help\"\n\
         world = \"fresh\"\n\
         bound = {{ steps = {bound_steps}, wall_secs = {bound_wall_secs}, spend_usd = {bound_spend_usd} }}\n\n\
         {obligations_toml}\
         ---\n\n\
         ## Persona\n\nA test persona.\n\n\
         ## Goal\n\nA test goal.\n"
    );
    let path = std::env::temp_dir().join(format!(
        "cronus-sim-e2e-{file_stem}-{}.md",
        std::process::id()
    ));
    std::fs::write(&path, text).expect("write scenario fixture");
    path
}

#[test]
fn a_run_whose_obligations_all_pass_exits_zero() {
    let _lock = serialize_repo_access();
    let scenario = write_scenario("all-pass", 10, 60, 100.0, &["goal-reachable"]);
    let world_id = world_new(&scenario);

    let ran = run_sim(&["run", &world_id, "--", "--help"]);
    assert_eq!(ran.code, 0, "the real `cronus --help` must exit 0");

    // Exercises --cite along the way: the pass verdict cites the entry the
    // `run` step above just produced.
    let verdicted = run_sim(&[
        "verdict",
        &world_id,
        "goal-reachable",
        "pass",
        "--cite",
        "0",
    ]);
    assert_eq!(
        verdicted.code, 0,
        "verdict must be accepted: {}",
        verdicted.stderr
    );

    let finished = run_sim(&["finish", &world_id]);
    assert_eq!(
        finished.code, 0,
        "an all-pass run must report exit 0: {}",
        finished.stdout
    );
    assert!(finished.stdout.contains("outcome: pass"));

    let _ = std::fs::remove_file(&scenario);
}

#[test]
fn a_run_with_one_failing_obligation_exits_non_zero() {
    let _lock = serialize_repo_access();
    let scenario = write_scenario("one-fail", 10, 60, 100.0, &["goal-reachable"]);
    let world_id = world_new(&scenario);

    let ran = run_sim(&["run", &world_id, "--", "--help"]);
    assert_eq!(ran.code, 0);

    let verdicted = run_sim(&["verdict", &world_id, "goal-reachable", "fail"]);
    assert_eq!(
        verdicted.code, 0,
        "verdict must be accepted: {}",
        verdicted.stderr
    );

    let finished = run_sim(&["finish", &world_id]);
    assert_ne!(finished.code, 0, "a failing obligation must not exit 0");
    assert!(finished.stdout.contains("outcome: fail"));

    let _ = std::fs::remove_file(&scenario);
}

#[test]
fn a_note_only_discovery_does_not_affect_a_passing_outcome() {
    let _lock = serialize_repo_access();
    let scenario = write_scenario("note-only", 10, 60, 100.0, &["goal-reachable"]);
    let world_id = world_new(&scenario);

    let ran = run_sim(&["run", &world_id, "--", "--help"]);
    assert_eq!(ran.code, 0);

    let noted = run_sim(&[
        "note",
        &world_id,
        "the help text is longer than a newcomer would want to read",
    ]);
    assert_eq!(noted.code, 0, "note must be accepted: {}", noted.stderr);

    let verdicted = run_sim(&["verdict", &world_id, "goal-reachable", "pass"]);
    assert_eq!(verdicted.code, 0);

    let finished = run_sim(&["finish", &world_id]);
    assert_eq!(
        finished.code, 0,
        "a discovery must never turn a passing run into a failing one: {}",
        finished.stdout
    );
    assert!(finished.stdout.contains("outcome: pass"));
    assert!(
        finished.stdout.contains("longer than a newcomer"),
        "the note must still be reported, just not scored: {}",
        finished.stdout
    );

    let _ = std::fs::remove_file(&scenario);
}

#[test]
fn a_bounded_out_run_exits_incomplete_and_names_the_undecided_obligation() {
    let _lock = serialize_repo_access();
    // bound.steps = 1: exactly one `run` is allowed before the wrapper
    // refuses further invocations.
    let scenario = write_scenario("bounded-out", 1, 600, 100.0, &["goal-reachable"]);
    let world_id = world_new(&scenario);

    let first = run_sim(&["run", &world_id, "--", "--help"]);
    assert_eq!(first.code, 0, "the first, allowed run must succeed");

    let second = run_sim(&["run", &world_id, "--", "status"]);
    assert_ne!(
        second.code, 0,
        "a second run past the declared bound must be refused"
    );
    assert!(
        second.stderr.contains("bound")
            || second.stderr.contains("exhausted")
            || second.stderr.to_lowercase().contains("finish"),
        "the refusal must explain itself: {}",
        second.stderr
    );

    // The obligation is deliberately left undecided — this is what a real
    // actor cut off by its own bound looks like.
    let finished = run_sim(&["finish", &world_id]);
    assert_eq!(
        finished.code, 2,
        "a bounded-out run with an undecided obligation must report `incomplete`: {}",
        finished.stdout
    );
    assert!(finished.stdout.contains("outcome: incomplete"));
    assert!(
        finished.stdout.contains("goal-reachable"),
        "the report must name the undecided obligation: {}",
        finished.stdout
    );

    let _ = std::fs::remove_file(&scenario);
}

#[test]
fn a_spend_exhausted_run_is_refused_and_reports_incomplete() {
    let _lock = serialize_repo_access();
    // A generous steps/wall bound, but a dollar ceiling low enough that a
    // single `spend` report exceeds it — proves the wrapper enforces the
    // agent's own reported cost, not just steps and wall clock.
    let scenario = write_scenario("spend-exhausted", 100, 600, 0.05, &["goal-reachable"]);
    let world_id = world_new(&scenario);

    let first = run_sim(&["run", &world_id, "--", "--help"]);
    assert_eq!(first.code, 0, "the first, allowed run must succeed");

    let spent = run_sim(&["spend", &world_id, "0.10"]);
    assert_eq!(
        spent.code, 0,
        "reporting spend must itself succeed even though it crosses the bound: {}",
        spent.stderr
    );

    let second = run_sim(&["run", &world_id, "--", "status"]);
    assert_ne!(
        second.code, 0,
        "a run attempted after the dollar bound is crossed must be refused"
    );
    assert!(
        second.stderr.contains("spent") || second.stderr.contains("exhausted"),
        "the refusal must explain itself in terms of spend: {}",
        second.stderr
    );

    let finished = run_sim(&["finish", &world_id]);
    assert_eq!(
        finished.code, 2,
        "a spend-exhausted run with an undecided obligation must report `incomplete`: {}",
        finished.stdout
    );
    assert!(finished.stdout.contains("outcome: incomplete"));

    let _ = std::fs::remove_file(&scenario);
}

#[test]
fn a_negative_spend_amount_is_refused() {
    let _lock = serialize_repo_access();
    let scenario = write_scenario("negative-spend", 10, 60, 100.0, &["goal-reachable"]);
    let world_id = world_new(&scenario);

    let rejected = run_sim(&["spend", &world_id, "-1.00"]);
    assert_ne!(
        rejected.code, 0,
        "a negative spend amount must be refused, not silently accepted"
    );

    let _ = std::fs::remove_file(&scenario);
}

/// Cleans up a replay file this test itself pinned into the real,
/// checked-in `tests/replays/` directory — a test artifact, never meant to
/// join the always-on replay lane it exists to prove doesn't wrongly flag
/// this exact pattern.
struct PinnedReplayGuard(PathBuf);

impl Drop for PinnedReplayGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn pinning_a_route_before_finish_does_not_report_the_run_contaminated() {
    let _lock = serialize_repo_access();
    // `pin` must run before `finish` (finish deletes the world `pin` reads
    // from), and `pin` itself writes a new file into the real, tracked
    // `tests/replays/` directory — exactly the kind of tracked-file change
    // the contamination check exists to catch when the *product* does it.
    // This proves that specific, sanctioned write is excluded rather than
    // producing a false `contaminated` on every single pinned discovery.
    let scenario = write_scenario("pin-then-finish", 10, 60, 100.0, &["goal-reachable"]);
    let world_id = world_new(&scenario);

    let ran = run_sim(&["run", &world_id, "--", "--help"]);
    assert_eq!(ran.code, 0);

    let replay_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/replays")
        .join("pin-then-finish-contamination-guard-test.toml");
    let _guard = PinnedReplayGuard(replay_path.clone());

    let pinned = run_sim(&["pin", &world_id, "pin-then-finish-contamination-guard-test"]);
    assert_eq!(pinned.code, 0, "pin must succeed: {}", pinned.stderr);
    assert!(
        replay_path.exists(),
        "pin must have written the replay file at {replay_path:?}"
    );

    let verdicted = run_sim(&["verdict", &world_id, "goal-reachable", "pass"]);
    assert_eq!(verdicted.code, 0);

    let finished = run_sim(&["finish", &world_id]);
    assert_eq!(
        finished.code, 0,
        "pinning a route must never itself cause `finish` to report contamination: {}",
        finished.stdout
    );
    assert!(finished.stdout.contains("outcome: pass"));

    let _ = std::fs::remove_file(&scenario);
}

#[test]
fn with_no_override_the_resolved_binary_sits_under_the_workspace_target_directory() {
    let _lock = serialize_repo_access();
    // Ensure `cronus` exists before `cronus-sim` tries to discover it on
    // its own — same build-order concern `ensure_cronus_binary` exists for.
    ensure_cronus_binary();

    let scenario = write_scenario("no-override-resolution", 10, 60, 100.0, &["goal-reachable"]);
    let out = run_sim(&["world", "new", scenario.to_str().unwrap()]);
    assert_eq!(
        out.code, 0,
        "world new without --bin must still resolve `cronus` on its own: {}",
        out.stderr
    );
    let world_id = out.stdout.trim().to_string();

    let state = RunState::load(&world_id).expect("run state must be readable");
    let target_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    let target_dir = target_dir.canonicalize().unwrap_or(target_dir);
    let resolved = state
        .resolved_binary
        .canonicalize()
        .unwrap_or_else(|_| state.resolved_binary.clone());
    assert!(
        resolved.starts_with(&target_dir),
        "resolved binary {resolved:?} must sit under the workspace target directory {target_dir:?}"
    );

    let verdicted = run_sim(&["verdict", &world_id, "goal-reachable", "pass"]);
    assert_eq!(verdicted.code, 0);
    let finished = run_sim(&["finish", &world_id]);
    assert_eq!(finished.code, 0);

    let _ = std::fs::remove_file(&scenario);
}

/// Cleans up the deliberately-created untracked probe file even if an
/// assertion above panics — a test that leaves stray files in the real
/// checkout to prove a point about contamination would be exactly the
/// contamination it is testing for.
struct ContaminationProbe(PathBuf);

impl Drop for ContaminationProbe {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root must resolve")
}

#[test]
fn a_repository_dirtied_during_the_run_is_reported_contaminated() {
    let _lock = serialize_repo_access();
    let scenario = write_scenario("contaminated", 10, 60, 100.0, &["goal-reachable"]);
    let world_id = world_new(&scenario);

    let ran = run_sim(&["run", &world_id, "--", "--help"]);
    assert_eq!(ran.code, 0);
    let verdicted = run_sim(&["verdict", &world_id, "goal-reachable", "pass"]);
    assert_eq!(verdicted.code, 0);

    // Dirty the *real* repository's tracked-file view: `git status
    // --porcelain` lists a new untracked file, which is exactly the
    // ambient signal a run writing outside its world would also produce.
    // `.md` — not `.tmp`: the repository's own `.gitignore` hides `*.tmp`
    // from `git status` entirely, which would make this probe invisible to
    // the very check it exists to exercise.
    let probe_path = repo_root().join("cronus-sim-contamination-probe.md");
    std::fs::write(&probe_path, b"probe").expect("write contamination probe");
    let _guard = ContaminationProbe(probe_path);

    let finished = run_sim(&["finish", &world_id]);
    assert_eq!(
        finished.code, 3,
        "a repository dirtied mid-run must report `contaminated`, discarding the otherwise-passing verdict: {}",
        finished.stdout
    );
    assert!(finished.stdout.contains("outcome: contaminated"));

    let _ = std::fs::remove_file(&scenario);
}

#[cfg(windows)]
#[test]
fn a_world_that_cannot_be_torn_down_is_attributed_environment_not_a_product_finding() {
    let _lock = serialize_repo_access();
    // Windows-only: a held-open file handle inside the world prevents its
    // removal, which is not reliably true on POSIX (a file may be deleted
    // while still open there). Cross-platform coverage of this exact path
    // is future work.
    let scenario = write_scenario("undeleteable", 10, 60, 100.0, &["goal-reachable"]);
    let world_id = world_new(&scenario);

    let ran = run_sim(&["run", &world_id, "--", "--help"]);
    assert_eq!(ran.code, 0);
    let verdicted = run_sim(&["verdict", &world_id, "goal-reachable", "pass"]);
    assert_eq!(verdicted.code, 0);

    let state = RunState::load(&world_id).expect("run state must be readable");
    let held_path = state.root.join("cwd").join("held-open.txt");
    // Rust's default Windows share mode includes `FILE_SHARE_DELETE`, so a
    // plain `File::create` would NOT actually block another process's
    // `remove_dir_all` — deletion-while-open is permitted by default,
    // specifically so ordinary temp-file patterns work smoothly. Denying
    // every share flag explicitly is what reproduces a genuine
    // sharing-violation teardown failure.
    let held_file = {
        use std::os::windows::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .share_mode(0)
            .open(&held_path)
            .expect("create held-open file with no sharing")
    };

    let finished = run_sim(&["finish", &world_id]);
    assert_eq!(
        finished.code, 4,
        "a world that cannot be torn down must be attributed `void`, never a product finding: {}",
        finished.stdout
    );
    assert!(finished.stdout.contains("outcome: void"));

    drop(held_file);
    let _ = std::fs::remove_dir_all(&state.root);
    let _ = std::fs::remove_file(&scenario);
}

#[test]
fn coverage_against_the_real_product_runs_clean_with_a_non_empty_complement() {
    ensure_cronus_binary();
    let out = run_sim(&["coverage"]);
    assert_eq!(
        out.code, 0,
        "coverage against the real product must exit 0: {}",
        out.stderr
    );
    assert!(
        out.stdout.contains("uncovered:"),
        "the report must carry an uncovered count: {}",
        out.stdout
    );
    // The corpus holds exactly one scenario today; a complement of zero
    // here would mean the catalog was silently never read, not that
    // coverage is complete.
    assert!(
        !out.stdout.contains("uncovered: 0 action(s)"),
        "the complement must be non-empty at this stage of the corpus: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains('%'),
        "must never report a percentage: {}",
        out.stdout
    );
}

#[test]
fn a_classified_discovery_with_a_proposed_remedy_survives_into_the_finish_report() {
    let _lock = serialize_repo_access();
    let scenario = write_scenario("classified-note", 10, 60, 100.0, &["goal-reachable"]);
    let world_id = world_new(&scenario);

    let ran = run_sim(&["run", &world_id, "--", "--help"]);
    assert_eq!(ran.code, 0);

    let noted = run_sim(&[
        "note",
        &world_id,
        "--class",
        "improvement-idea",
        "--remedy",
        "surface the flag in --help too",
        "the",
        "flag",
        "works",
        "but",
        "is",
        "undocumented",
    ]);
    assert_eq!(
        noted.code, 0,
        "a classified note with a remedy must be accepted: {}",
        noted.stderr
    );

    let verdicted = run_sim(&["verdict", &world_id, "goal-reachable", "pass"]);
    assert_eq!(verdicted.code, 0);

    let finished = run_sim(&["finish", &world_id]);
    assert_eq!(
        finished.code, 0,
        "a defect-free run stays pass regardless of an unrelated classified discovery: {}",
        finished.stdout
    );
    assert!(finished.stdout.contains("outcome: pass"));
    assert!(
        finished
            .stdout
            .contains("the flag works but is undocumented"),
        "the discovery text must appear: {}",
        finished.stdout
    );
    assert!(
        finished.stdout.contains("class: improvement-idea"),
        "the class must be rendered: {}",
        finished.stdout
    );
    assert!(
        finished.stdout.contains("proposed remedy")
            && finished.stdout.contains("surface the flag in --help too"),
        "the remedy must be rendered as a proposal, not an applied change: {}",
        finished.stdout
    );

    let _ = std::fs::remove_file(&scenario);
}

#[test]
fn an_unrecognized_discovery_class_is_a_usage_error_naming_all_five_valid_classes() {
    let _lock = serialize_repo_access();
    let scenario = write_scenario("bad-class", 10, 60, 100.0, &["goal-reachable"]);
    let world_id = world_new(&scenario);

    let ran = run_sim(&["run", &world_id, "--", "--help"]);
    assert_eq!(ran.code, 0);

    let noted = run_sim(&[
        "note",
        &world_id,
        "--class",
        "bug",
        "something odd happened",
    ]);
    assert_eq!(
        noted.code, 5,
        "an unrecognized class must be a usage error: stdout={} stderr={}",
        noted.stdout, noted.stderr
    );
    for name in [
        "defect",
        "friction",
        "inefficiency",
        "optimization-opportunity",
        "improvement-idea",
    ] {
        assert!(
            noted.stderr.contains(name),
            "the error must name `{name}` as a valid class: {}",
            noted.stderr
        );
    }

    let _ = std::fs::remove_file(&scenario);
}

#[test]
fn an_unclassified_note_renders_exactly_as_it_did_before_usm_13() {
    let _lock = serialize_repo_access();
    let scenario = write_scenario("unclassified-note", 10, 60, 100.0, &["goal-reachable"]);
    let world_id = world_new(&scenario);

    let ran = run_sim(&["run", &world_id, "--", "--help"]);
    assert_eq!(ran.code, 0);

    let noted = run_sim(&["note", &world_id, "no class or remedy attached to this one"]);
    assert_eq!(noted.code, 0);

    let verdicted = run_sim(&["verdict", &world_id, "goal-reachable", "pass"]);
    assert_eq!(verdicted.code, 0);

    let finished = run_sim(&["finish", &world_id]);
    assert_eq!(finished.code, 0);
    assert!(
        finished
            .stdout
            .contains("no class or remedy attached to this one")
    );
    assert!(
        !finished.stdout.contains("class:"),
        "an unclassified discovery must carry no class label: {}",
        finished.stdout
    );
    assert!(
        !finished.stdout.contains("proposed remedy"),
        "a remedy-free discovery must carry no remedy label: {}",
        finished.stdout
    );

    let _ = std::fs::remove_file(&scenario);
}
