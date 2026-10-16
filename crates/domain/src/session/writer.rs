//! Writing session journals: appending to the JSONL files that
//! [`super::journal`] reads.
//!
//! A journal is append-only. Every entry carries a stable `id` and a
//! `parentId` naming the entry before it, which is what makes the file a tree
//! rather than a list. Nothing is ever rewritten, so a crash can lose at most
//! the line being written — and that line is then a damaged one the reader
//! names, not a corrupted neighbour.
//!
//! **A journal comes into being with its first reply.** [`JournalWriter::create`]
//! touches nothing on disk: entries wait in memory, and the file — header and
//! all — is written when the first assistant entry arrives. A session that was
//! started and never answered therefore leaves no empty file in the list, and
//! a turn that dies before any reply costs nothing but the message that was
//! never answered.
//!
//! **A turn always ends in an assistant entry.** A reply that failed, or was
//! cut short, is recorded as the assistant entry it would have been, with a
//! stop reason — so the log says what happened instead of ending mid-air. A
//! turn that was interrupted before any entry could say so (the process died)
//! is *closed*, never truncated: [`JournalWriter::close_interrupted_turn`]
//! appends the missing aborted entry when the journal is reopened.
//!
//! The writer knows nothing about agents, models or turns: it records the
//! messages and metadata it is handed. What decides *when* a message is
//! recorded belongs to the session runner.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::journal::{FORMAT_VERSION, JournalError, is_valid_id};

/// Why an assistant entry ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// The reply completed.
    Stop,
    /// The reply failed; the entry's `error` says how.
    Error,
    /// The reply was cut short on purpose or by a lost process.
    Aborted,
}

impl StopReason {
    /// The word the journal stores.
    pub fn as_str(self) -> &'static str {
        match self {
            StopReason::Stop => "stop",
            StopReason::Error => "error",
            StopReason::Aborted => "aborted",
        }
    }
}

/// A journal entries can be appended to.
#[derive(Debug)]
pub struct JournalWriter {
    path: PathBuf,
    /// `None` until the journal has been written for the first time.
    file: Option<File>,
    created_at: u64,
    /// Lines accepted but not yet on disk (only before the first reply).
    pending: Vec<String>,
    /// Entries after the header, so far — pending ones included.
    entries: u64,
    /// The id of the last entry, the next entry's parent.
    last_id: Option<String>,
    /// Whether the last message entry is not an assistant's — a turn that
    /// has been asked and not answered.
    awaiting_reply: bool,
}

fn unreadable(path: &Path, why: impl std::fmt::Display) -> JournalError {
    JournalError::Unreadable(format!("{}: {why}", path.display()))
}

fn journal_path(dir: &Path, id: &str) -> Result<PathBuf, JournalError> {
    if !is_valid_id(id) {
        return Err(JournalError::NotFound(id.to_string()));
    }
    Ok(dir.join(format!("{id}.jsonl")))
}

impl JournalWriter {
    /// Begin a new session `id` in `dir`, stamped `created_at` seconds since
    /// the epoch. Nothing is written until the first assistant entry. Refuses
    /// a session that already exists: starting over would silently replace a
    /// conversation.
    pub fn create(dir: &Path, id: &str, created_at: u64) -> Result<Self, JournalError> {
        let path = journal_path(dir, id)?;
        if path.exists() {
            return Err(JournalError::Unreadable(format!(
                "session already exists: {id}"
            )));
        }
        Ok(JournalWriter {
            path,
            file: None,
            created_at,
            pending: Vec::new(),
            entries: 0,
            last_id: None,
            awaiting_reply: false,
        })
    }

    /// Continue an existing journal. Refuses one with no readable header or
    /// one written by a newer format — appending to what this build cannot
    /// interpret would mix two formats in one file.
    pub fn open(dir: &Path, id: &str) -> Result<Self, JournalError> {
        let path = journal_path(dir, id)?;
        let mut file = OpenOptions::new()
            .read(true)
            .append(true)
            .open(&path)
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    JournalError::NotFound(id.to_string())
                } else {
                    unreadable(&path, e)
                }
            })?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|e| unreadable(&path, e))?;
        let text = String::from_utf8_lossy(&bytes);

        let mut lines = text.lines().filter(|l| !l.trim().is_empty());
        let header = lines
            .next()
            .and_then(|l| serde_json::from_str::<Value>(l).ok())
            .filter(|h| h.get("type").and_then(Value::as_str) == Some("session"))
            .ok_or_else(|| unreadable(&path, "no session header"))?;
        match header.get("version").and_then(Value::as_u64) {
            Some(v) if v > u64::from(FORMAT_VERSION) => {
                return Err(unreadable(
                    &path,
                    format!("written by a newer format (version {v})"),
                ));
            }
            None => return Err(unreadable(&path, "header carries no format version")),
            Some(_) => {}
        }
        let created_at = header.get("createdAt").and_then(Value::as_u64).unwrap_or(0);

        let mut entries = 0u64;
        let mut last_id = None;
        let mut awaiting_reply = false;
        for line in lines {
            entries += 1;
            let Ok(entry) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if let Some(entry_id) = entry.get("id").and_then(Value::as_str) {
                last_id = Some(entry_id.to_string());
            }
            if entry.get("type").and_then(Value::as_str) == Some("message") {
                let role = entry
                    .get("message")
                    .and_then(|m| m.get("role"))
                    .and_then(Value::as_str);
                awaiting_reply = role != Some("assistant");
            }
        }
        // A line torn by a crash has no trailing newline; the next entry must
        // not be glued onto it.
        if bytes.last().is_some_and(|b| *b != b'\n') {
            file.seek(SeekFrom::End(0))
                .and_then(|_| file.write_all(b"\n"))
                .map_err(|e| unreadable(&path, e))?;
        }
        Ok(JournalWriter {
            path,
            file: Some(file),
            created_at,
            pending: Vec::new(),
            entries,
            last_id,
            awaiting_reply,
        })
    }

    /// Entries after the header, those not yet on disk included.
    pub fn entries(&self) -> u64 {
        self.entries
    }

    /// Whether the journal is on disk yet.
    pub fn is_materialized(&self) -> bool {
        self.file.is_some()
    }

    /// Whether the last message was asked and never answered.
    pub fn awaiting_reply(&self) -> bool {
        self.awaiting_reply
    }

    /// Append a message from `role` (`user`, `tool`, …); returns the new
    /// entry's id. An `assistant` role is recorded as a completed reply — use
    /// [`Self::append_reply`] to say how a reply ended.
    pub fn append_message(&mut self, role: &str, text: &str) -> Result<String, JournalError> {
        if role == "assistant" {
            return self.append_reply(text, StopReason::Stop, None);
        }
        self.awaiting_reply = true;
        self.append(
            "message",
            json!({"message": {"role": role, "content": text}}),
            false,
        )
    }

    /// Append the assistant's reply and how it ended. `text` is whatever was
    /// produced (possibly nothing, possibly partial); `error` says what went
    /// wrong when `stop` is not [`StopReason::Stop`]. The first reply brings
    /// the journal onto disk.
    pub fn append_reply(
        &mut self,
        text: &str,
        stop: StopReason,
        error: Option<&str>,
    ) -> Result<String, JournalError> {
        let mut message = json!({
            "role": "assistant",
            "content": text,
            "stopReason": stop.as_str(),
        });
        if let (Some(error), Some(fields)) = (error, message.as_object_mut()) {
            fields.insert("error".to_string(), Value::String(error.to_string()));
        }
        let id = self.append("message", json!({ "message": message }), true)?;
        self.awaiting_reply = false;
        Ok(id)
    }

    /// If the last message was asked and never answered, close the turn with
    /// an aborted reply that says so, and report that it did. Never removes
    /// anything: an interrupted turn may hold real work, and the log keeps it.
    pub fn close_interrupted_turn(&mut self) -> Result<bool, JournalError> {
        if !self.awaiting_reply {
            return Ok(false);
        }
        self.append_reply(
            "",
            StopReason::Aborted,
            Some("interrupted before a reply was recorded"),
        )?;
        Ok(true)
    }

    /// Record the session's name; the last one recorded is the one shown.
    pub fn append_session_info(&mut self, name: &str) -> Result<String, JournalError> {
        self.append("session_info", json!({"name": name}), false)
    }

    fn append(
        &mut self,
        kind: &str,
        body: Value,
        brings_to_disk: bool,
    ) -> Result<String, JournalError> {
        let id = format!("{:08x}", self.entries + 1);
        let mut entry = json!({"type": kind, "id": id, "parentId": self.last_id});
        if let (Some(target), Value::Object(fields)) = (entry.as_object_mut(), body) {
            target.extend(fields);
        }
        let line = format!("{entry}\n");

        match self.file.as_mut() {
            Some(file) => write_all(file, &self.path, line.as_bytes())?,
            None if brings_to_disk => self.materialize(&line)?,
            None => self.pending.push(line),
        }
        self.entries += 1;
        self.last_id = Some(id.clone());
        Ok(id)
    }

    /// Write the header, every waiting entry and `last` to a file that must
    /// not exist, in one durable write.
    fn materialize(&mut self, last: &str) -> Result<(), JournalError> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| unreadable(dir, e))?;
        }
        let mut file = OpenOptions::new()
            .append(true)
            .create_new(true)
            .open(&self.path)
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    JournalError::Unreadable(format!(
                        "session already exists: {}",
                        self.path.display()
                    ))
                } else {
                    unreadable(&self.path, e)
                }
            })?;
        let header =
            json!({"type": "session", "version": FORMAT_VERSION, "createdAt": self.created_at});
        let mut text = format!("{header}\n");
        for line in &self.pending {
            text.push_str(line);
        }
        text.push_str(last);
        write_all(&mut file, &self.path, text.as_bytes())?;
        self.pending.clear();
        self.file = Some(file);
        Ok(())
    }
}

/// Bytes written whole and made durable before returning.
fn write_all(file: &mut File, path: &Path, bytes: &[u8]) -> Result<(), JournalError> {
    file.write_all(bytes)
        .and_then(|()| file.sync_data())
        .map_err(|e| unreadable(path, e))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::super::journal::{list, read};
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cronus-writer-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn what_is_written_is_read_back_as_the_same_conversation() {
        let dir = temp_dir("round-trip");
        let mut writer = JournalWriter::create(&dir, "s-1", 1_700_000_000).expect("creates");
        writer
            .append_message("user", "what blocks the card?")
            .expect("appends");
        writer
            .append_message("assistant", "the gate is red\nand \"quoted\"")
            .expect("appends");
        writer
            .append_session_info("gate question")
            .expect("appends");

        let transcript = read(&dir, "s-1").expect("readable");
        assert_eq!(transcript.summary.name.as_deref(), Some("gate question"));
        assert_eq!(transcript.summary.created_at, Some(1_700_000_000));
        assert_eq!(
            (transcript.summary.entries, transcript.summary.turns),
            (3, 1)
        );
        assert_eq!(transcript.summary.problem, None);
        assert_eq!(
            transcript.messages[1].text,
            "the gate is red\nand \"quoted\""
        );
        assert_eq!(list(&dir).expect("readable").len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_session_that_was_never_answered_leaves_nothing_on_disk() {
        let dir = temp_dir("lazy");
        let mut writer = JournalWriter::create(&dir, "s-1", 0).expect("creates");
        writer
            .append_message("user", "anyone there?")
            .expect("accepts");
        writer.append_session_info("pending").expect("accepts");
        assert!(!writer.is_materialized());
        assert_eq!(writer.entries(), 2, "accepted, though not on disk");
        assert!(!dir.exists(), "not even the directory exists yet");
        drop(writer);
        assert_eq!(list(&dir), Ok(Vec::new()), "and nothing is listed");
    }

    #[test]
    fn the_first_reply_brings_the_waiting_entries_onto_disk_in_order() {
        let dir = temp_dir("first-reply");
        let mut writer = JournalWriter::create(&dir, "s-1", 5).expect("creates");
        let user = writer.append_message("user", "hello").expect("accepts");
        assert!(!writer.is_materialized());
        let reply = writer.append_message("assistant", "hi").expect("appends");
        assert!(writer.is_materialized());

        let text = fs::read_to_string(dir.join("s-1.jsonl")).expect("readable");
        let lines: Vec<Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).expect("valid json"))
            .collect();
        assert_eq!(lines[0]["type"], "session", "the header leads");
        assert_eq!(lines[1]["id"], Value::String(user.clone()));
        assert_eq!(lines[2]["id"], Value::String(reply));
        assert_eq!(lines[2]["parentId"], Value::String(user));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn each_entry_names_the_one_before_it_as_its_parent() {
        let dir = temp_dir("parents");
        let mut writer = JournalWriter::create(&dir, "s-1", 0).expect("creates");
        let first = writer.append_message("user", "a").expect("appends");
        let second = writer.append_message("assistant", "b").expect("appends");
        assert_ne!(first, second);

        let text = fs::read_to_string(dir.join("s-1.jsonl")).expect("readable");
        let entries: Vec<Value> = text
            .lines()
            .skip(1)
            .map(|l| serde_json::from_str(l).expect("valid json"))
            .collect();
        assert_eq!(
            entries[0]["parentId"],
            Value::Null,
            "the first has no parent"
        );
        assert_eq!(entries[1]["parentId"], Value::String(first));
        assert_eq!(entries[1]["id"], Value::String(second));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_reply_is_recorded_as_the_reply_it_would_have_been() {
        let dir = temp_dir("failed");
        let mut writer = JournalWriter::create(&dir, "s-1", 0).expect("creates");
        writer.append_message("user", "hello").expect("accepts");
        writer
            .append_reply("partial", StopReason::Error, Some("ConnectRefused"))
            .expect("appends");
        assert!(
            !writer.awaiting_reply(),
            "the turn is answered, however it ended"
        );

        let transcript = read(&dir, "s-1").expect("readable");
        assert_eq!(transcript.summary.turns, 1);
        let reply = &transcript.messages[1];
        assert_eq!(reply.text, "partial");
        assert_eq!(reply.stop.as_deref(), Some("error"));
        assert_eq!(reply.error.as_deref(), Some("ConnectRefused"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_session_that_exists_is_not_started_over() {
        let dir = temp_dir("exists");
        let mut first = JournalWriter::create(&dir, "s-1", 0).expect("creates");
        first
            .append_message("assistant", "on disk now")
            .expect("appends");
        let again = JournalWriter::create(&dir, "s-1", 0);
        assert!(
            matches!(&again, Err(JournalError::Unreadable(why)) if why.contains("already exists")),
            "{again:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_writers_for_one_new_session_cannot_both_materialize_it() {
        let dir = temp_dir("race");
        let mut a = JournalWriter::create(&dir, "s-1", 0).expect("creates");
        let mut b = JournalWriter::create(&dir, "s-1", 0).expect("nothing on disk yet");
        a.append_message("assistant", "first").expect("wins");
        let lost = b.append_message("assistant", "second");
        assert!(
            matches!(&lost, Err(JournalError::Unreadable(why)) if why.contains("already exists")),
            "{lost:?}"
        );
        assert_eq!(read(&dir, "s-1").expect("readable").messages.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn reopening_continues_the_chain_and_the_count() {
        let dir = temp_dir("reopen");
        let mut writer = JournalWriter::create(&dir, "s-1", 0).expect("creates");
        writer.append_message("user", "a").expect("accepts");
        let first = writer.append_message("assistant", "a2").expect("appends");
        drop(writer);

        let mut writer = JournalWriter::open(&dir, "s-1").expect("opens");
        assert_eq!(writer.entries(), 2);
        assert!(!writer.awaiting_reply());
        writer.append_message("user", "b").expect("appends");

        let text = fs::read_to_string(dir.join("s-1.jsonl")).expect("readable");
        let last: Value = serde_json::from_str(text.lines().last().expect("a line")).expect("json");
        assert_eq!(last["parentId"], Value::String(first));
        assert_eq!(read(&dir, "s-1").expect("readable").summary.entries, 3);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unanswered_turn_is_closed_when_the_journal_is_reopened_and_never_truncated() {
        let dir = temp_dir("closer");
        let mut writer = JournalWriter::create(&dir, "s-1", 0).expect("creates");
        writer
            .append_message("assistant", "earlier")
            .expect("appends");
        writer.append_message("user", "and then?").expect("appends");
        drop(writer); // the process died before the reply

        let mut writer = JournalWriter::open(&dir, "s-1").expect("opens");
        assert!(writer.awaiting_reply());
        assert_eq!(writer.close_interrupted_turn(), Ok(true));
        assert_eq!(writer.close_interrupted_turn(), Ok(false), "closed once");

        let transcript = read(&dir, "s-1").expect("readable");
        let roles: Vec<&str> = transcript
            .messages
            .iter()
            .map(|m| m.role.as_str())
            .collect();
        assert_eq!(
            roles,
            ["assistant", "user", "assistant"],
            "nothing was removed"
        );
        let closer = &transcript.messages[2];
        assert_eq!(closer.stop.as_deref(), Some("aborted"));
        assert!(
            closer
                .error
                .as_deref()
                .is_some_and(|e| e.contains("interrupted"))
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_line_torn_by_a_crash_is_kept_apart_from_the_next_entry() {
        let dir = temp_dir("torn");
        let mut writer = JournalWriter::create(&dir, "s-1", 0).expect("creates");
        writer.append_message("assistant", "a").expect("appends");
        drop(writer);
        let path = dir.join("s-1.jsonl");
        let mut text = fs::read_to_string(&path).expect("readable");
        text.push_str("{\"type\":\"message\",\"id\":\"0000");
        fs::write(&path, text).expect("writable");

        let mut writer = JournalWriter::open(&dir, "s-1").expect("opens");
        writer.append_message("assistant", "b").expect("appends");
        let summary = read(&dir, "s-1").expect("readable").summary;
        assert_eq!(summary.problem.as_deref(), Some("1 damaged line(s)"));
        assert_eq!(
            summary.turns, 2,
            "the new entry is intact, not glued to the torn one"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_journal_it_cannot_interpret_is_never_appended_to() {
        let dir = temp_dir("refuse");
        fs::create_dir_all(&dir).expect("creatable");
        fs::write(
            dir.join("future.jsonl"),
            "{\"type\":\"session\",\"version\":99}\n",
        )
        .expect("writable");
        fs::write(dir.join("headless.jsonl"), "{\"type\":\"message\"}\n").expect("writable");
        for id in ["future", "headless"] {
            assert!(
                matches!(
                    JournalWriter::open(&dir, id),
                    Err(JournalError::Unreadable(_))
                ),
                "{id}"
            );
        }
        assert_eq!(
            JournalWriter::open(&dir, "absent").map(|_| ()),
            Err(JournalError::NotFound("absent".to_string()))
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_id_that_could_name_a_path_creates_nothing() {
        let dir = temp_dir("ids");
        for bad in ["", "..", "../x", "a/b", ".hidden"] {
            assert!(JournalWriter::create(&dir, bad, 0).is_err(), "{bad:?}");
        }
        assert!(
            !dir.exists(),
            "a refused id must not even create the directory"
        );
    }
}
