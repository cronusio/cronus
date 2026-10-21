//! The seat: the exclusive claim on a state root.
//!
//! The claim is an advisory lock the operating system holds on `engine.lock`
//! for as long as the holding process lives. Because the system releases it
//! when the process ends — by any means — a lock that can be taken is a lock
//! nobody holds, and no process-liveness probe is needed or kept.
//!
//! What a would-be spoke needs to know (where the engine listens, the token
//! that proves it is welcome, which release it is) lives in a second file,
//! `engine.endpoint`, because a lock held on a file makes the file unreadable
//! to others on some platforms. The record is replaced by renaming a complete
//! file into place, so a reader never sees half of one.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

const LOCK_FILE: &str = "engine.lock";
const RECORD_FILE: &str = "engine.endpoint";

/// What the engine publishes for others to find it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub pid: u32,
    /// Where the engine listens, loopback only: `127.0.0.1:<port>`.
    pub addr: String,
    /// The secret every request must present. Never logged.
    pub token: String,
    pub version: String,
    /// When the engine started, in seconds since the Unix epoch.
    pub started: u64,
}

impl Record {
    fn to_json(&self) -> String {
        json!({
            "pid": self.pid,
            "addr": self.addr,
            "token": self.token,
            "version": self.version,
            "started": self.started,
        })
        .to_string()
    }

    fn from_json(text: &str) -> Option<Self> {
        let value: Value = serde_json::from_str(text).ok()?;
        Some(Record {
            pid: u32::try_from(value.get("pid")?.as_u64()?).ok()?,
            addr: value.get("addr")?.as_str()?.to_string(),
            token: value.get("token")?.as_str()?.to_string(),
            version: value.get("version")?.as_str()?.to_string(),
            started: value.get("started")?.as_u64()?,
        })
    }

    /// A record for the current process.
    pub fn for_this_process(addr: String, token: String, version: String) -> Self {
        Record {
            pid: std::process::id(),
            addr,
            token,
            version,
            started: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        }
    }
}

/// What reading the record found.
#[derive(Debug, PartialEq, Eq)]
pub enum Read {
    /// No record: the engine is between claiming the seat and serving, or
    /// there is no engine.
    Absent,
    /// A well-formed record.
    Found(Record),
    /// Something is there that is not a record. Never guessed at, never
    /// deleted by a reader.
    Corrupt,
}

/// Read the record of the engine over `root`, if there is one.
pub fn read_record(root: &Path) -> io::Result<Read> {
    match fs::read_to_string(root.join(RECORD_FILE)) {
        Ok(text) => Ok(match Record::from_json(text.trim()) {
            Some(record) => Read::Found(record),
            None => Read::Corrupt,
        }),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Read::Absent),
        Err(err) => Err(err),
    }
}

/// The result of trying to take the seat.
pub enum Claim {
    /// This process now holds the seat.
    Held(Seat),
    /// A live process holds it.
    Occupied,
}

/// The held seat. Dropping it removes the record (best effort) and closes the
/// lock file, which releases the claim.
pub struct Seat {
    // Held for its lifetime: closing it is what releases the claim.
    _lock: File,
    root: PathBuf,
}

/// Try to take the seat for `root`, creating the directory if it is missing.
pub fn claim(root: &Path) -> io::Result<Claim> {
    fs::create_dir_all(root)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join(LOCK_FILE))?;
    match lock.try_lock() {
        Ok(()) => {
            // Whatever record is here belongs to an engine that no longer
            // holds the claim; nothing may be trusted from it.
            match fs::remove_file(root.join(RECORD_FILE)) {
                Ok(()) => {}
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(err),
            }
            Ok(Claim::Held(Seat {
                _lock: lock,
                root: root.to_path_buf(),
            }))
        }
        Err(fs::TryLockError::WouldBlock) => Ok(Claim::Occupied),
        Err(fs::TryLockError::Error(err)) => Err(err),
    }
}

impl Seat {
    /// The state root this seat is for.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Publish the record, by renaming a complete file into place.
    pub fn publish(&self, record: &Record) -> io::Result<()> {
        let staged = self
            .root
            .join(format!("{RECORD_FILE}.{}.tmp", std::process::id()));
        {
            let mut options = OpenOptions::new();
            options.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&staged)?;
            file.write_all(record.to_json().as_bytes())?;
            file.sync_all()?;
        }
        fs::rename(&staged, self.root.join(RECORD_FILE)).inspect_err(|_| {
            let _ = fs::remove_file(&staged);
        })
    }
}

impl Drop for Seat {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.root.join(RECORD_FILE));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("cronus-seat-{tag}-{}-{nanos}", std::process::id()))
    }

    fn record() -> Record {
        Record::for_this_process(
            "127.0.0.1:4242".to_string(),
            "00ff".to_string(),
            "1.2.3".to_string(),
        )
    }

    fn held(root: &Path) -> Seat {
        match claim(root).expect("claims") {
            Claim::Held(seat) => seat,
            Claim::Occupied => panic!("expected to take the seat"),
        }
    }

    #[test]
    fn the_first_claim_holds_and_the_second_finds_it_occupied() {
        let root = temp_root("two");
        let first = held(&root);
        assert!(matches!(claim(&root).expect("claims"), Claim::Occupied));
        drop(first);
        // Released: a successor takes it with no probe and no timeout.
        let _second = held(&root);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn two_roots_are_two_seats() {
        let (a, b) = (temp_root("a"), temp_root("b"));
        let _first = held(&a);
        let _second = held(&b);
        let _ = fs::remove_dir_all(&a);
        let _ = fs::remove_dir_all(&b);
    }

    #[test]
    fn a_published_record_is_read_back_whole() {
        let root = temp_root("record");
        let seat = held(&root);
        assert_eq!(read_record(&root).expect("reads"), Read::Absent);

        seat.publish(&record()).expect("publishes");

        assert_eq!(read_record(&root).expect("reads"), Read::Found(record()));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_record_left_by_a_dead_engine_is_deleted_by_its_successor() {
        let root = temp_root("stale");
        fs::create_dir_all(&root).expect("creatable");
        // What a crashed engine leaves: a record, and a lock nobody holds.
        fs::write(root.join(RECORD_FILE), record().to_json()).expect("writable");
        assert!(matches!(read_record(&root).expect("reads"), Read::Found(_)));

        let _seat = held(&root);

        assert_eq!(
            read_record(&root).expect("reads"),
            Read::Absent,
            "nothing is trusted from an engine that no longer holds the claim"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn dropping_the_seat_withdraws_the_record() {
        let root = temp_root("withdraw");
        let seat = held(&root);
        seat.publish(&record()).expect("publishes");
        drop(seat);
        assert_eq!(read_record(&root).expect("reads"), Read::Absent);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn something_that_is_not_a_record_is_corrupt_and_left_alone() {
        let root = temp_root("corrupt");
        fs::create_dir_all(&root).expect("creatable");
        fs::write(root.join(RECORD_FILE), "not json").expect("writable");
        assert_eq!(read_record(&root).expect("reads"), Read::Corrupt);
        assert!(root.join(RECORD_FILE).exists(), "a reader never deletes");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn publishing_again_replaces_the_record_without_a_gap() {
        let root = temp_root("replace");
        let seat = held(&root);
        seat.publish(&record()).expect("publishes");
        let mut next = record();
        next.addr = "127.0.0.1:5000".to_string();
        seat.publish(&next).expect("publishes");
        assert_eq!(read_record(&root).expect("reads"), Read::Found(next));
        let _ = fs::remove_dir_all(&root);
    }
}
