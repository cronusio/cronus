// One engine per state root, across real processes: this test process is the
// engine (as a terminal UI would be) and the command line is run as the
// program it is. What is proved here cannot be proved in one process — that a
// second launch attaches rather than duplicating, that the engine runs what a
// spoke sends for the spoke's directory, and that a change applied by one
// process is refused while another's work is running.
//
// One test function in its own test binary on purpose: the working directory
// and the portable-directory variable are process-global.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use cronus_contract::Surface;
use cronus_core::endpoint::ledger::WorkKind;
use cronus_core::endpoint::{DEFAULT_PATIENCE, Role, begin_work, join_composition};
use cronus_core::invocable::{DispatchJournal, JournalRecord};

/// Counts the dispatches that reach this engine.
struct Counting(Arc<AtomicUsize>);

impl DispatchJournal for Counting {
    fn record(&mut self, record: JournalRecord<'_>) -> Result<(), String> {
        if matches!(record, JournalRecord::Enter { .. }) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
        Ok(())
    }
}

fn cronus(home: &Path, from: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cronus"))
        .args(args)
        .current_dir(from)
        .env("CRONUS_PORTABLE_DIR", home.join("portable"))
        .output()
        .expect("the program starts")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn a_second_launch_attaches_to_the_engine_and_a_change_waits_for_its_work() {
    let home: PathBuf = std::env::temp_dir().join(format!("cronus-attach-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    let elsewhere = home.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).expect("creatable");
    // SAFETY: this binary holds exactly one test.
    unsafe { std::env::set_var("CRONUS_PORTABLE_DIR", home.join("portable")) };
    std::env::set_current_dir(&home).expect("enterable");
    let root = cronus_core::paths::resolve_workspace_root();

    // The engine: the real composition, journaling so the test can see what
    // reaches it.
    let seen = Arc::new(AtomicUsize::new(0));
    let (registry, mut dispatcher) =
        cronus_core::invocable_bootstrap::bootstrap(cronus_core::Engine::new());
    dispatcher.set_journal(Box::new(Counting(Arc::clone(&seen))));
    let engine = join_composition(&root, registry, dispatcher, Surface::Tui, DEFAULT_PATIENCE)
        .expect("the first process becomes the engine");
    assert!(matches!(engine.role, Role::Engine(_)));

    // A second launch is a spoke: its dispatch reaches the engine.
    let listed = cronus(&home, &elsewhere, &["board", "list", "--format", "json"]);
    assert!(listed.status.success(), "{}", text(&listed.stderr));
    assert!(
        seen.load(Ordering::SeqCst) >= 1,
        "the command ran at the engine, not beside it"
    );

    // …and what it sent means ITS directory, not the engine's.
    let scaffolded = cronus(&home, &elsewhere, &["workflow", "scaffold", "demo"]);
    assert!(scaffolded.status.success(), "{}", text(&scaffolded.stderr));
    assert!(
        elsewhere.join("demo.nodus").is_file(),
        "a relative path in a forwarded command is relative to the caller"
    );
    assert!(
        !home.join("demo.nodus").exists(),
        "and not to the engine's own directory"
    );

    // A change that recomposes the office is refused while work is running at
    // the engine, and the refusal names the work.
    let turn = begin_work(&root, WorkKind::AgentTurn, "s-42").expect("work begins");
    let refused = cronus(&home, &elsewhere, &["registry", "disable", "work"]);
    assert!(!refused.status.success(), "it must not apply over work");
    let said = text(&refused.stderr);
    assert!(said.contains("not applied"), "{said}");
    assert!(said.contains("an agent turn (s-42)"), "{said}");

    // Once the work has ended the same change goes through.
    drop(turn);
    let applied = cronus(&home, &elsewhere, &["registry", "disable", "work"]);
    assert!(applied.status.success(), "{}", text(&applied.stderr));

    // With the engine gone, the command line runs on its own.
    let before = seen.load(Ordering::SeqCst);
    drop(engine);
    let alone = cronus(&home, &elsewhere, &["board", "list", "--format", "json"]);
    assert!(alone.status.success(), "{}", text(&alone.stderr));
    assert_eq!(
        seen.load(Ordering::SeqCst),
        before,
        "nothing reaches a dead engine"
    );

    let _ = std::env::set_current_dir(std::env::temp_dir());
    unsafe { std::env::remove_var("CRONUS_PORTABLE_DIR") };
    let _ = std::fs::remove_dir_all(&home);
}
