//! The work lock: the one fact the gate in front of a recomposing change rests
//! on — *is any work running, in any process, over this state root?*
//!
//! `work.lock` in the state root is a reader–writer lock the operating system
//! holds. Every producer of work holds it **shared** for as long as the work
//! lasts, in whatever process it runs; a change that recomposes the office holds
//! it **exclusive** for as long as the change takes. So a change cannot begin
//! while any work holds the lock, and no work can begin while the change does —
//! and there is no instant between "nothing is running" and "the change holds
//! the lock" at which work could slip in, because the system decides both.
//!
//! Nothing here is remembered: when a process ends, however it ends, the system
//! releases what it held. There are no marker files to go stale and no
//! process-liveness probe.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::Path;

const WORK_LOCK: &str = "work.lock";

/// Why a lock was not taken.
#[derive(Debug)]
pub enum Contended {
    /// Someone holds it in a way that conflicts: work is running (for the
    /// change) or a change is being applied (for work).
    Held,
    /// The lock could not be opened or asked for.
    Io(io::Error),
}

impl std::fmt::Display for Contended {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Contended::Held => f.write_str("the work lock is held"),
            Contended::Io(err) => write!(f, "the work lock could not be taken: {err}"),
        }
    }
}

fn open(root: &Path) -> Result<File, Contended> {
    fs::create_dir_all(root).map_err(Contended::Io)?;
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join(WORK_LOCK))
        .map_err(Contended::Io)
}

/// Work in progress, as far as the system is concerned: the shared lock, held
/// until this is dropped.
#[derive(Debug)]
pub struct Share {
    _file: File,
}

/// An installation change in progress: the exclusive lock, held until this is
/// dropped.
#[derive(Debug)]
pub struct Exclusive {
    _file: File,
}

/// Register work over `root`: take the lock shared. Refused while a change that
/// recomposes the office is being applied.
pub fn share(root: &Path) -> Result<Share, Contended> {
    let file = open(root)?;
    match file.try_lock_shared() {
        Ok(()) => Ok(Share { _file: file }),
        Err(fs::TryLockError::WouldBlock) => Err(Contended::Held),
        Err(fs::TryLockError::Error(err)) => Err(Contended::Io(err)),
    }
}

/// Begin a change over `root`: take the lock exclusive. Refused while any work
/// holds it shared, or while another change holds it.
pub fn exclusive(root: &Path) -> Result<Exclusive, Contended> {
    let file = open(root)?;
    match file.try_lock() {
        Ok(()) => Ok(Exclusive { _file: file }),
        Err(fs::TryLockError::WouldBlock) => Err(Contended::Held),
        Err(fs::TryLockError::Error(err)) => Err(Contended::Io(err)),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cronus-worklock-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("creatable");
        dir
    }

    #[test]
    fn many_pieces_of_work_share_the_lock() {
        let root = root("many");
        let _a = share(&root).expect("first");
        let _b = share(&root).expect("second");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_change_cannot_begin_while_any_work_runs_and_can_once_it_ends() {
        let root = root("change-waits");
        let work = share(&root).expect("work begins");
        assert!(matches!(exclusive(&root), Err(Contended::Held)));
        drop(work);
        assert!(exclusive(&root).is_ok());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn no_work_begins_while_a_change_is_applied_and_can_once_it_is_done() {
        let root = root("work-waits");
        let change = exclusive(&root).expect("the change begins");
        assert!(matches!(share(&root), Err(Contended::Held)));
        drop(change);
        assert!(share(&root).is_ok());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn two_changes_do_not_apply_at_once() {
        let root = root("two-changes");
        let _first = exclusive(&root).expect("first");
        assert!(matches!(exclusive(&root), Err(Contended::Held)));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn two_state_roots_are_two_locks() {
        let (a, b) = (root("root-a"), root("root-b"));
        let _work = share(&a).expect("work over a");
        assert!(
            exclusive(&b).is_ok(),
            "work over a does not stop a change over b"
        );
        let _ = fs::remove_dir_all(&a);
        let _ = fs::remove_dir_all(&b);
    }

    #[test]
    fn a_begin_and_a_change_racing_never_both_win() {
        // Whichever the system admits first decides; the other is refused.
        // Repeated, with real threads, to give a gap a chance to show.
        for round in 0..100 {
            let root = root(&format!("race-{round}"));
            let (a, b) = (root.clone(), root.clone());
            let began = std::thread::spawn(move || share(&a));
            let changed = std::thread::spawn(move || exclusive(&b));
            let (began, changed) = (began.join().expect("joins"), changed.join().expect("joins"));
            assert!(
                !(began.is_ok() && changed.is_ok()),
                "work began under a change, or a change began over work"
            );
            let _ = fs::remove_dir_all(&root);
        }
    }

    #[test]
    fn a_lock_dies_with_its_holder_even_on_a_panic() {
        let root = root("panic");
        let held = root.clone();
        let result = std::thread::spawn(move || {
            let _work = share(&held).expect("work begins");
            panic!("the work failed");
        })
        .join();
        assert!(result.is_err());
        assert!(
            exclusive(&root).is_ok(),
            "a panicked run is not still running"
        );
        let _ = fs::remove_dir_all(&root);
    }
}
