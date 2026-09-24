//! State-root lock: exactly one engine
//! owns a durable-state root at a time. A frontend that cannot take the lock
//! attaches to the running engine's endpoint instead of starting a second
//! one — the local instance of the architecture's own hub-and-spoke relation,
//! independent of which activation mode (if any) started the
//! engine holding it. This is facade-tier, not domain: taking a real
//! exclusive file lock is I/O, which the no-I/O domain tier may not hold.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// What a lock record says about the engine holding it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockRecord {
    pub pid: u32,
    pub endpoint: String,
    pub version: String,
}

impl LockRecord {
    fn serialize(&self) -> String {
        format!("{}\n{}\n{}\n", self.pid, self.endpoint, self.version)
    }

    fn parse(content: &str) -> Option<Self> {
        let mut lines = content.lines();
        let pid = lines.next()?.parse().ok()?;
        let endpoint = lines.next()?.to_string();
        let version = lines.next()?.to_string();
        Some(LockRecord {
            pid,
            endpoint,
            version,
        })
    }
}

/// The result of trying to become the engine for a state root.
#[derive(Debug)]
pub enum EngineLockOutcome {
    /// This process now holds the exclusive lock — it IS the engine.
    Acquired(EngineLock),
    /// Another live, version-compatible engine already holds the lock —
    /// attach to it as a frontend/spoke instead of starting a second one.
    Attach(LockRecord),
    /// The lock is held by an incompatible version, or the lock file
    /// could not be created/read/removed — fail visibly. Never starts a
    /// second engine over the same state root.
    Failed { reason: String },
}

/// A held state-root lock. Dropping it releases the lock (removes the file)
/// — an engine that exits normally (even via a panic that unwinds) does not
/// leave a stale lock behind. A hard crash (process killed, power loss)
/// leaves the file in place; that is the stale-lock case that is handed to the
/// *existing* liveness/crash-recovery reclamation (stranded-work
/// reconciliation, unclean-shutdown detection) via the `is_alive` check
/// passed to [`acquire`] — this module introduces no second reclamation
/// mechanism of its own.
#[derive(Debug)]
pub struct EngineLock {
    path: PathBuf,
    pub record: LockRecord,
}

impl Drop for EngineLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn try_create(path: &Path, record: &LockRecord) -> io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true) // atomic exclusive create — the mutual-exclusion primitive
        .open(path)?;
    file.write_all(record.serialize().as_bytes())?;
    Ok(())
}

fn resolve_existing(
    path: &Path,
    my_record: &LockRecord,
    is_alive: &dyn Fn(u32) -> bool,
) -> EngineLockOutcome {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            return EngineLockOutcome::Failed {
                reason: format!("lock file exists but could not be read: {e}"),
            };
        }
    };

    let Some(existing) = LockRecord::parse(&content) else {
        // Corrupt/unreadable content: cannot confirm the holder is dead, and
        // The activation fail-closed posture (Unknown, never Active) means we
        // never guess — refuse rather than silently reclaim or attach.
        return EngineLockOutcome::Failed {
            reason: "lock file exists but is unreadable/corrupt — refusing to guess".to_string(),
        };
    };

    if is_alive(existing.pid) {
        if existing.version == my_record.version {
            EngineLockOutcome::Attach(existing)
        } else {
            EngineLockOutcome::Failed {
                reason: format!(
                    "an incompatible engine (v{}) holds the lock; this is v{}",
                    existing.version, my_record.version
                ),
            }
        }
    } else {
        // Stale lock (reclamation): the holder is confirmed dead —
        // remove and retry the exclusive create once.
        if let Err(e) = fs::remove_file(path) {
            return EngineLockOutcome::Failed {
                reason: format!("stale lock detected but could not be removed: {e}"),
            };
        }
        match try_create(path, my_record) {
            Ok(()) => EngineLockOutcome::Acquired(EngineLock {
                path: path.to_path_buf(),
                record: my_record.clone(),
            }),
            Err(e) => EngineLockOutcome::Failed {
                reason: format!("reclaimed a stale lock but could not re-create it: {e}"),
            },
        }
    }
}

/// Try to become the engine for `state_root`. `is_alive` checks
/// whether a PID still denotes a live process — injected so the state
/// machine (create / attach / fail / reclaim) is fully testable without a
/// real OS process table.
///
/// **Disclosed scope note:** [`conservative_is_alive`] is the only
/// `is_alive` implementation this module ships — it always reports "alive"
/// (never wrongly reclaims a live engine's lock), deferring a real per-OS
/// process check to the platform-adapter work (Track D). A real crash's
/// stale lock is still recoverable through the existing liveness/crash-
/// recovery path this spec explicitly defers to — that path is
/// what supplies a truthful `is_alive` once it exists.
pub fn acquire(
    state_root: &Path,
    endpoint: &str,
    version: &str,
    is_alive: &dyn Fn(u32) -> bool,
) -> EngineLockOutcome {
    let path = state_root.join("engine.lock");
    let record = LockRecord {
        pid: std::process::id(),
        endpoint: endpoint.to_string(),
        version: version.to_string(),
    };

    match try_create(&path, &record) {
        Ok(()) => EngineLockOutcome::Acquired(EngineLock { path, record }),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            resolve_existing(&path, &record, is_alive)
        }
        Err(e) => EngineLockOutcome::Failed {
            reason: format!("could not create lock file: {e}"),
        },
    }
}

/// The safe placeholder default (see [`acquire`]'s scope note): always
/// reports a PID alive, so this module never wrongly reclaims a live
/// engine's lock in the absence of a real per-OS liveness check.
pub fn conservative_is_alive(_pid: u32) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_state_root(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "cronus-engine-lock-{tag}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("create temp state root");
        dir
    }

    #[test]
    fn two_starts_over_one_state_root_the_second_attaches() {
        let root = temp_state_root("attach");
        let always_alive: &dyn Fn(u32) -> bool = &|_| true;

        let first = acquire(&root, "http://127.0.0.1:9001", "1.0.0", always_alive);
        let first_lock = match first {
            EngineLockOutcome::Acquired(lock) => lock,
            other => panic!("expected the first start to acquire, got {other:?}"),
        };

        let second = acquire(&root, "http://127.0.0.1:9002", "1.0.0", always_alive);
        match second {
            EngineLockOutcome::Attach(record) => {
                assert_eq!(record.endpoint, "http://127.0.0.1:9001");
                assert_eq!(record.pid, first_lock.record.pid);
            }
            other => panic!("expected the second start to attach, got {other:?}"),
        }

        drop(first_lock);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_incompatible_version_fails_visibly_never_starting_a_second_engine() {
        let root = temp_state_root("version-mismatch");
        let always_alive: &dyn Fn(u32) -> bool = &|_| true;

        let first = acquire(&root, "http://127.0.0.1:9001", "1.0.0", always_alive);
        let _first_lock = match first {
            EngineLockOutcome::Acquired(lock) => lock,
            other => panic!("expected acquire, got {other:?}"),
        };

        let second = acquire(&root, "http://127.0.0.1:9002", "2.0.0", always_alive);
        assert!(
            matches!(second, EngineLockOutcome::Failed { .. }),
            "an incompatible version must fail visibly, not attach and not start a second engine"
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn two_distinct_state_roots_coexist() {
        let root_a = temp_state_root("root-a");
        let root_b = temp_state_root("root-b");
        let always_alive: &dyn Fn(u32) -> bool = &|_| true;

        let a = acquire(&root_a, "http://127.0.0.1:9001", "1.0.0", always_alive);
        let b = acquire(&root_b, "http://127.0.0.1:9002", "1.0.0", always_alive);

        assert!(matches!(a, EngineLockOutcome::Acquired(_)));
        assert!(matches!(b, EngineLockOutcome::Acquired(_)));

        fs::remove_dir_all(&root_a).ok();
        fs::remove_dir_all(&root_b).ok();
    }

    #[test]
    fn a_dead_holders_lock_is_reclaimed() {
        let root = temp_state_root("reclaim");
        let never_alive: &dyn Fn(u32) -> bool = &|_| false;

        // Seed a lock record as if a prior engine crashed while holding it.
        let stale = LockRecord {
            pid: 999_999, // scripted dead in this test via `never_alive`
            endpoint: "http://127.0.0.1:9001".to_string(),
            version: "1.0.0".to_string(),
        };
        try_create(&root.join("engine.lock"), &stale).expect("seed stale lock");

        let result = acquire(&root, "http://127.0.0.1:9002", "1.0.0", never_alive);
        match result {
            EngineLockOutcome::Acquired(lock) => {
                assert_eq!(lock.record.endpoint, "http://127.0.0.1:9002");
                assert_ne!(lock.record.pid, stale.pid);
            }
            other => panic!("expected the stale lock to be reclaimed, got {other:?}"),
        }

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn dropping_the_lock_releases_it_for_a_subsequent_acquire() {
        let root = temp_state_root("drop-release");
        let always_alive: &dyn Fn(u32) -> bool = &|_| true;

        let first = acquire(&root, "http://127.0.0.1:9001", "1.0.0", always_alive);
        let first_lock = match first {
            EngineLockOutcome::Acquired(lock) => lock,
            other => panic!("expected acquire, got {other:?}"),
        };
        drop(first_lock);

        let second = acquire(&root, "http://127.0.0.1:9002", "1.0.0", always_alive);
        assert!(
            matches!(second, EngineLockOutcome::Acquired(_)),
            "after the holder drops the lock, a fresh start must acquire it, not attach or fail"
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn conservative_is_alive_always_reports_alive() {
        assert!(conservative_is_alive(1));
        assert!(conservative_is_alive(999_999));
    }
}
