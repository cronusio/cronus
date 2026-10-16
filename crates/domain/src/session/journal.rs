//! Reading session journals: the append-only JSONL files under a workspace's
//! `sessions/` directory, one file per session.
//!
//! A journal's first line is its header (`type: "session"`, a format
//! `version`, an optional `createdAt` in seconds); every later line is one
//! entry with a `type` (the taxonomy in the session loop's entry list). This
//! module only reads: it summarizes a journal for a listing and extracts its
//! conversation for a transcript. Appending is [`super::writer`]; deciding what
//! to record belongs to the session runner.
//!
//! Three kinds of "nothing" are kept apart, because a screen that blurs them
//! lies: a workspace with no `sessions/` directory has no sessions (an empty
//! list); a directory that cannot be read is a failure (an error); a journal
//! that is damaged or newer than this build is listed with the problem named
//! rather than dropped from the list.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde_json::Value;

/// The journal format this build reads: the current session version. Older
/// journals are read as they are (migration forward is the runner's load step);
/// newer ones are named and not interpreted.
pub const FORMAT_VERSION: u32 = 3;

const EXTENSION: &str = "jsonl";

/// Where a workspace keeps its session journals.
pub fn sessions_dir(workspace_root: &Path) -> PathBuf {
    workspace_root.join("sessions")
}

/// What a listing shows about one session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    /// The journal's file stem — the session's identity.
    pub id: String,
    /// The name the session was last given (`session_info`), if any.
    pub name: Option<String>,
    /// When the journal says it was created, in seconds since the epoch.
    pub created_at: Option<u64>,
    /// When the file was last written, in seconds since the epoch.
    pub updated_at: Option<u64>,
    /// Entries after the header.
    pub entries: usize,
    /// Assistant messages — the session's turns.
    pub turns: usize,
    /// What is wrong with the journal, when something is.
    pub problem: Option<String>,
}

/// One conversational message of a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptMessage {
    /// `user`, `assistant`, `toolResult`, …
    pub role: String,
    /// The message's text, blocks joined; empty when it carries none.
    pub text: String,
    /// How an assistant message ended (`stop`, `error`, `aborted`), when the
    /// journal says.
    pub stop: Option<String>,
    /// What went wrong, when the message records a failure.
    pub error: Option<String>,
}

/// A session's summary and its conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcript {
    pub summary: SessionSummary,
    pub messages: Vec<TranscriptMessage>,
}

/// Why a journal could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalError {
    /// No such session.
    NotFound(String),
    /// The journal or its directory could not be read, or is not readable by
    /// this build.
    Unreadable(String),
}

impl std::fmt::Display for JournalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JournalError::NotFound(id) => write!(f, "session not found: {id}"),
            JournalError::Unreadable(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for JournalError {}

/// A fresh session id: the time in milliseconds and the process id, so two
/// sessions started by different processes in the same instant still differ.
/// Always a valid id.
pub fn new_session_id() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("s-{millis}-{}", std::process::id())
}

/// A session id is a file stem: letters, digits, `-`, `_` and `.` inside it,
/// never empty, never a path. Anything else cannot name a journal, and must
/// not be allowed to reach outside the directory.
pub fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && !id.starts_with('.')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Every session in `dir`, most recently written first. A missing directory
/// is no sessions; an unreadable one is an error.
pub fn list(dir: &Path) -> Result<Vec<SessionSummary>, JournalError> {
    let read = match std::fs::read_dir(dir) {
        Ok(read) => read,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(JournalError::Unreadable(format!(
                "cannot read {}: {e}",
                dir.display()
            )));
        }
    };
    let mut sessions = Vec::new();
    for entry in read.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some(EXTENSION) {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if !is_valid_id(id) {
            continue;
        }
        sessions.push(match read_journal(&path, id) {
            Ok(parsed) => parsed.summary,
            Err(e) => SessionSummary {
                id: id.to_string(),
                name: None,
                created_at: None,
                updated_at: None,
                entries: 0,
                turns: 0,
                problem: Some(e.to_string()),
            },
        });
    }
    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.id.cmp(&b.id)));
    Ok(sessions)
}

/// One session's summary and conversation.
pub fn read(dir: &Path, id: &str) -> Result<Transcript, JournalError> {
    if !is_valid_id(id) {
        return Err(JournalError::NotFound(id.to_string()));
    }
    let path = dir.join(format!("{id}.{EXTENSION}"));
    match read_journal(&path, id) {
        Ok(transcript) => Ok(transcript),
        Err(JournalError::Unreadable(_)) if !path.exists() => {
            Err(JournalError::NotFound(id.to_string()))
        }
        Err(e) => Err(e),
    }
}

fn read_journal(path: &Path, id: &str) -> Result<Transcript, JournalError> {
    let bytes = std::fs::read(path)
        .map_err(|e| JournalError::Unreadable(format!("cannot read {}: {e}", path.display())))?;
    let updated_at = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    let text = String::from_utf8_lossy(&bytes);

    let mut header: Option<Value> = None;
    let mut name = None;
    let mut entries = 0usize;
    let mut turns = 0usize;
    let mut damaged = 0usize;
    let mut messages = Vec::new();

    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(Value::Object(object)) = serde_json::from_str::<Value>(line) else {
            damaged += 1;
            continue;
        };
        let kind = object.get("type").and_then(Value::as_str).unwrap_or("");
        if kind == "session" && header.is_none() && entries == 0 {
            header = Some(Value::Object(object));
            continue;
        }
        entries += 1;
        match kind {
            "session_info" | "sessionInfo" => {
                if let Some(n) = object.get("name").and_then(Value::as_str) {
                    name = Some(n.to_string());
                }
            }
            "message" => {
                let body = object.get("message").unwrap_or(&Value::Null);
                let role = body
                    .get("role")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if role == "assistant" {
                    turns += 1;
                }
                let text_of = |key: &str| body.get(key).and_then(Value::as_str).map(str::to_string);
                messages.push(TranscriptMessage {
                    role,
                    text: message_text(body.get("content")),
                    stop: text_of("stopReason"),
                    error: text_of("error"),
                });
            }
            _ => {}
        }
    }

    let version = header
        .as_ref()
        .and_then(|h| h.get("version"))
        .and_then(Value::as_u64);
    let mut problems = Vec::new();
    match (&header, version) {
        (None, _) => problems.push("no session header".to_string()),
        (Some(_), None) => problems.push("header carries no format version".to_string()),
        (Some(_), Some(v)) if v > u64::from(FORMAT_VERSION) => {
            problems.push(format!("written by a newer format (version {v})"));
        }
        _ => {}
    }
    if damaged > 0 {
        problems.push(format!("{damaged} damaged line(s)"));
    }
    let problem = (!problems.is_empty()).then(|| problems.join("; "));

    if version.is_some_and(|v| v > u64::from(FORMAT_VERSION)) {
        // Entries of a format this build does not know cannot be interpreted;
        // the listing still names the session and says why.
        return Err(JournalError::Unreadable(
            problem.unwrap_or_else(|| "unsupported format version".to_string()),
        ));
    }

    Ok(Transcript {
        summary: SessionSummary {
            id: id.to_string(),
            name,
            created_at: header
                .as_ref()
                .and_then(|h| h.get("createdAt"))
                .and_then(Value::as_u64),
            updated_at,
            entries,
            turns,
            problem,
        },
        messages,
    })
}

/// A message's content as text: a string as is, a list of blocks as the
/// `text` of each block that has one, anything else as nothing.
fn message_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cronus-journal-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("test directory is creatable");
        dir
    }

    const GOOD: &str = concat!(
        "{\"type\":\"session\",\"version\":1,\"createdAt\":1700000000}\n",
        "{\"type\":\"message\",\"id\":\"a\",\"message\":{\"role\":\"user\",\"content\":\"hello\"}}\n",
        "{\"type\":\"message\",\"id\":\"b\",\"parentId\":\"a\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"hi\"},{\"type\":\"text\",\"text\":\"there\"}]}}\n",
        "{\"type\":\"session_info\",\"name\":\"first try\"}\n",
    );

    #[test]
    fn a_missing_directory_is_no_sessions_and_an_unreadable_one_is_an_error() {
        let root = temp_dir("missing");
        assert_eq!(list(&root.join("nope")), Ok(Vec::new()));
        let file = root.join("a-file");
        fs::write(&file, "x").expect("writable");
        assert!(matches!(list(&file), Err(JournalError::Unreadable(_))));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_journal_is_summarized_with_its_name_turns_and_creation_time() {
        let dir = temp_dir("good");
        fs::write(dir.join("s-1.jsonl"), GOOD).expect("writable");
        fs::write(dir.join("notes.txt"), "not a journal").expect("writable");
        let listed = list(&dir).expect("readable");
        assert_eq!(listed.len(), 1, "only journals are sessions");
        let session = &listed[0];
        assert_eq!(session.id, "s-1");
        assert_eq!(session.name.as_deref(), Some("first try"));
        assert_eq!(session.created_at, Some(1_700_000_000));
        assert_eq!((session.entries, session.turns), (3, 1));
        assert_eq!(session.problem, None);
        assert!(session.updated_at.is_some());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_transcript_carries_the_conversation_with_blocks_joined() {
        let dir = temp_dir("transcript");
        fs::write(dir.join("s-1.jsonl"), GOOD).expect("writable");
        let transcript = read(&dir, "s-1").expect("readable");
        assert_eq!(
            transcript.messages,
            vec![
                TranscriptMessage {
                    role: "user".to_string(),
                    text: "hello".to_string(),
                    stop: None,
                    error: None,
                },
                TranscriptMessage {
                    role: "assistant".to_string(),
                    text: "hi\nthere".to_string(),
                    stop: None,
                    error: None,
                },
            ]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_damaged_journal_is_listed_with_the_problem_not_dropped() {
        let dir = temp_dir("damaged");
        let text = format!("{GOOD}this is not json\n");
        fs::write(dir.join("bad.jsonl"), text).expect("writable");
        fs::write(
            dir.join("headless.jsonl"),
            "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"x\"}}\n",
        )
        .expect("writable");
        let listed = list(&dir).expect("readable");
        let problem = |id: &str| {
            listed
                .iter()
                .find(|s| s.id == id)
                .and_then(|s| s.problem.clone())
        };
        assert_eq!(problem("bad").as_deref(), Some("1 damaged line(s)"));
        assert_eq!(problem("headless").as_deref(), Some("no session header"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_journal_from_a_newer_format_is_named_and_not_interpreted() {
        let dir = temp_dir("newer");
        fs::write(
            dir.join("future.jsonl"),
            "{\"type\":\"session\",\"version\":99}\n{\"type\":\"message\"}\n",
        )
        .expect("writable");
        let listed = list(&dir).expect("readable");
        assert_eq!(listed.len(), 1);
        assert!(
            listed[0]
                .problem
                .as_deref()
                .is_some_and(|p| p.contains("version 99")),
            "{:?}",
            listed[0]
        );
        assert!(matches!(
            read(&dir, "future"),
            Err(JournalError::Unreadable(_))
        ));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_id_that_could_name_a_path_never_reaches_the_file_system() {
        let dir = temp_dir("ids");
        for bad in ["", "..", "../x", "a/b", "a\\b", ".hidden", "x y"] {
            assert!(!is_valid_id(bad), "{bad:?}");
            assert_eq!(
                read(&dir, bad),
                Err(JournalError::NotFound(bad.to_string()))
            );
        }
        assert_eq!(
            read(&dir, "absent"),
            Err(JournalError::NotFound("absent".to_string()))
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_most_recently_written_session_comes_first() {
        let dir = temp_dir("order");
        fs::write(dir.join("old.jsonl"), GOOD).expect("writable");
        std::thread::sleep(std::time::Duration::from_millis(1100));
        fs::write(dir.join("new.jsonl"), GOOD).expect("writable");
        let ids: Vec<String> = list(&dir)
            .expect("readable")
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(ids, vec!["new", "old"]);
        let _ = fs::remove_dir_all(&dir);
    }
}
