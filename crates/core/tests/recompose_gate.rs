// The gate in front of an installation change that recomposes the office,
// against a real state root, the operating system's real locks and, where an
// engine is wanted, the real seat and endpoint — the engine being this test
// process, as it is for a terminal UI.
//
// "Another process" is represented where it matters by holding the lock the
// way another process would (a lock is a lock to the system whoever holds it).
// The ledger is process-wide, so these tests take turns.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use cronus_core::endpoint::ledger::WorkKind;
use cronus_core::endpoint::worklock;
use cronus_core::endpoint::{BeginError, Role, begin_work, join};
use cronus_core::installation::gate::{self, Refused};
use cronus_domain::invocable::{Dispatcher, InvocableRegistry};

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    // The machine's own state root is where work also registers; it is kept off
    // the real one for the length of this file.
    static MACHINE: std::sync::Once = std::sync::Once::new();
    MACHINE.call_once(|| {
        let machine =
            std::env::temp_dir().join(format!("cronus-gate-machine-{}", std::process::id()));
        // SAFETY: set once, before any test in this file reads it, and the
        // tests take turns.
        unsafe { std::env::set_var("CRONUS_PORTABLE_DIR", machine) };
    });
    ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner)
}

fn state_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cronus-gate-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("creatable");
    dir
}

/// This process as the engine of `root`, serving an empty composition: the
/// gate needs the endpoint and the ledger, not any invocable.
fn become_the_engine(root: &Path) -> Role {
    let Role::Engine(mut host) = join(root, Some("test-release"), Duration::from_millis(50))
        .expect("the first process joins as the engine")
    else {
        panic!("expected to be the engine");
    };
    host.serve(
        Arc::new(InvocableRegistry::new()),
        Arc::new(Dispatcher::new()),
    )
    .expect("serves");
    Role::Engine(host)
}

#[test]
fn nothing_running_lets_a_change_apply_and_nothing_can_begin_while_it_does() {
    let _turn = serial();
    let root = state_root("alone");

    let applying = gate::begin(&root).expect("nothing is running anywhere");
    assert!(
        matches!(
            begin_work(&root, WorkKind::AgentTurn, "x"),
            Err(BeginError::ChangeInProgress)
        ),
        "no work begins while the change is applied"
    );

    drop(applying);
    assert!(begin_work(&root, WorkKind::AgentTurn, "x").is_ok());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn work_running_here_refuses_the_change_and_is_named() {
    let _turn = serial();
    let root = state_root("busy");
    let _in_flight = begin_work(&root, WorkKind::AgentTurn, "s-42").expect("begins");

    match gate::begin(&root) {
        Err(Refused::Busy(reason)) => {
            assert!(reason.contains("an agent turn (s-42)"), "{reason}");
        }
        Err(other) => panic!("expected the running work to be named, got {other:?}"),
        Ok(_) => panic!("a change must not apply over running work"),
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn work_in_another_process_refuses_the_change_even_though_nothing_here_knows_its_name() {
    let _turn = serial();
    let root = state_root("elsewhere");
    // What another process running work holds: the lock, shared, and no name
    // anywhere this process can read.
    let _their_work = worklock::share(&root).expect("their work begins");

    match gate::begin(&root) {
        Err(Refused::Busy(reason)) => {
            assert!(reason.contains("another process"), "{reason}");
        }
        Err(other) => panic!("expected the refusal to say so, got {other:?}"),
        Ok(_) => panic!("a change must not apply over work nobody here can name"),
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_engine_that_is_reachable_names_the_work_when_the_change_is_refused() {
    let _turn = serial();
    let root = state_root("named");
    let _engine = become_the_engine(&root);
    let _in_flight = begin_work(&root, WorkKind::WorkflowRun, "nightly").expect("begins");

    match gate::begin(&root) {
        Err(Refused::Busy(reason)) => {
            assert_eq!(
                reason, "work is running that this change would alter: a workflow run (nightly)",
                "named once, though both this process and the engine know of it"
            );
        }
        other => panic!("expected the work to be named, got {:?}", other.err()),
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn two_changes_do_not_apply_at_once() {
    let _turn = serial();
    let root = state_root("twice");

    let first = gate::begin(&root).expect("nothing is running");
    match gate::begin(&root) {
        Err(Refused::Busy(_)) => {}
        other => panic!("expected the second change to wait, got {:?}", other.err()),
    }
    drop(first);
    assert!(gate::begin(&root).is_ok());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_lock_that_cannot_be_asked_is_not_taken_for_an_idle_office() {
    let _turn = serial();
    let blocker = state_root("unaskable").join("a-file");
    std::fs::write(&blocker, "x").expect("writable");
    // A "state root" whose parent is a file: no directory can be made there.
    let root = blocker.join("root");

    match gate::begin(&root) {
        Err(Refused::Unknown(reason)) => {
            assert!(reason.contains("not applied"), "{reason}");
        }
        other => panic!("expected the unknown to be refused, got {:?}", other.err()),
    }
    let _ = std::fs::remove_dir_all(blocker.parent().expect("a parent"));
}

#[test]
fn asking_whether_a_change_could_apply_holds_nothing_and_names_what_would_stop_it() {
    let _turn = serial();
    // A machine-scope verb is asked over the machine's own state root, which
    // `serial` keeps off the real one.
    gate::check_for("registry.enable").expect("nothing is running");
    gate::check_for("registry.enable").expect("asking twice is asking once: nothing was held");

    let machine = cronus_core::endpoint::machine_root();
    let running = begin_work(&machine, WorkKind::AgentTurn, "s-3").expect("work can begin");
    match gate::check_for("registry.enable") {
        Err(Refused::Busy(reason)) => {
            assert!(reason.contains("an agent turn (s-3)"), "{reason}");
        }
        other => panic!("running work must refuse the change: {other:?}"),
    }
    drop(running);
    gate::check_for("registry.enable").expect("the work ended");
}
