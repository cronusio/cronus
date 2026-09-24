//! Kanban board — single per-workspace board with file-backed cards.
//!
//! Fixed canonical state set: triage → todo → ready → running → blocked → done.
//! Each transition appends a history entry (KAN-7). Done cards are auto-archived
//! to `<ws>/kanban/archive/` without deletion (KAN-4). Custom columns and saved
//! views (KAN-8) extend the canonical backbone as mapped extensions — see
//! [`custom_boards`].

pub mod custom_boards;

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

// ── Error type ────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum KanbanError {
    InvalidTransition {
        from: CardState,
        to: CardState,
    },
    BlockedRequiresReason,
    CardNotFound(String),
    /// `add_card` was given an id an existing card already holds. Adding
    /// never upserts (a silent overwrite would discard the existing card's
    /// `task_ref`, `created_at`, and transition history with no way to tell
    /// "created" from "clobbered" apart) — callers who want an explicit
    /// replace go through `move_card`/`archive` first.
    CardAlreadyExists(String),
    /// The card id is not a safe single path segment (empty, or contains a
    /// separator, a `..` component, a drive/root prefix, or a control byte).
    /// Card ids are turned into `cards/<id>.json` filenames, so an id that
    /// escapes that directory would let a caller write an arbitrary `.json`
    /// file anywhere the process can reach.
    InvalidCardId(String),
    Io(std::io::Error),
}

impl fmt::Display for KanbanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KanbanError::InvalidTransition { from, to } => {
                let legal = from.legal_targets();
                if legal.is_empty() {
                    write!(
                        f,
                        "invalid transition: {from:?} → {to:?} ({from:?} is terminal)"
                    )
                } else {
                    let names = legal
                        .iter()
                        .map(|s| format!("{s:?}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    write!(
                        f,
                        "invalid transition: {from:?} → {to:?} (from {from:?}, only {names} {} allowed)",
                        if legal.len() == 1 { "is" } else { "are" }
                    )
                }
            }
            KanbanError::BlockedRequiresReason => write!(f, "blocked state requires a reason"),
            KanbanError::CardNotFound(id) => write!(f, "card not found: {id}"),
            KanbanError::CardAlreadyExists(id) => write!(f, "card already exists: {id}"),
            KanbanError::InvalidCardId(id) => {
                write!(
                    f,
                    "invalid card id {id:?}: must be a single path segment of at most {MAX_CARD_ID_LEN} characters"
                )
            }
            KanbanError::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

/// Comfortably under NTFS's 255-UTF-16-code-unit filename component limit
/// even with the longest suffix a card id ever grows (`.jsonl`, in
/// `events_dir`) — an id past this reaches the OS's own filename-length
/// error instead of a clear product one.
const MAX_CARD_ID_LEN: usize = 200;

/// Reject any card id that would not stay inside `cards/` when turned into a
/// `<id>.json` filename. Accepts a plain single segment; rejects empty ids,
/// `.`/`..`, ids containing `/`, `\`, or a NUL/control byte, anything the OS
/// would treat as absolute or drive-qualified, and anything over
/// [`MAX_CARD_ID_LEN`] characters.
fn validate_card_id(id: &str) -> Result<()> {
    let bad = id.is_empty()
        || id.chars().count() > MAX_CARD_ID_LEN
        || id == "."
        || id == ".."
        || id.contains('/')
        || id.contains('\\')
        || id.contains('\0')
        || id.contains("..")
        || id.chars().any(|c| c.is_control())
        || std::path::Path::new(id).components().count() != 1
        || std::path::Path::new(id).is_absolute();
    if bad {
        return Err(KanbanError::InvalidCardId(id.to_string()));
    }
    Ok(())
}

impl std::error::Error for KanbanError {}
impl From<std::io::Error> for KanbanError {
    fn from(e: std::io::Error) -> Self {
        KanbanError::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, KanbanError>;

// ── Card state ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardState {
    Triage,
    Todo,
    Ready,
    Running,
    Blocked,
    Done,
}

impl CardState {
    pub fn as_str(&self) -> &'static str {
        match self {
            CardState::Triage => "triage",
            CardState::Todo => "todo",
            CardState::Ready => "ready",
            CardState::Running => "running",
            CardState::Blocked => "blocked",
            CardState::Done => "done",
        }
    }

    /// Every state token `parse` accepts, in canonical order — for help text
    /// and "valid: …" error messages so the vocabulary is discoverable rather
    /// than found by trial.
    pub const NAMES: &'static [&'static str] =
        &["triage", "todo", "ready", "running", "blocked", "done"];

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "triage" => Some(CardState::Triage),
            "todo" => Some(CardState::Todo),
            "ready" => Some(CardState::Ready),
            "running" => Some(CardState::Running),
            "blocked" => Some(CardState::Blocked),
            "done" => Some(CardState::Done),
            _ => None,
        }
    }

    /// Every state `self` may transition to directly — derived from
    /// [`Self::can_transition_to`] rather than a second, hand-maintained
    /// list, so the two can never drift apart. Used to name what *is*
    /// allowed in an [`KanbanError::InvalidTransition`] message:
    /// naming only what was rejected leaves a caller to discover the legal
    /// set by trial.
    pub fn legal_targets(self) -> Vec<CardState> {
        Self::NAMES
            .iter()
            .filter_map(|s| Self::parse(s))
            .filter(|&to| self.can_transition_to(to))
            .collect()
    }

    /// Returns true when the transition from self → to is valid.
    pub fn can_transition_to(self, to: CardState) -> bool {
        matches!(
            (self, to),
            (CardState::Triage, CardState::Todo)
                | (CardState::Todo, CardState::Ready)
                | (CardState::Ready, CardState::Running)
                | (CardState::Running, CardState::Done)
                | (CardState::Running, CardState::Blocked)
                | (CardState::Running, CardState::Todo) // allow running-back
                | (CardState::Blocked, CardState::Ready)
                | (CardState::Blocked, CardState::Todo)
                | (CardState::Todo, CardState::Triage) // allow triage-back
        )
    }
}

// ── Card types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    Low,
    Medium,
    High,
    Critical,
}

impl Priority {
    pub fn as_str(&self) -> &'static str {
        match self {
            Priority::Low => "low",
            Priority::Medium => "medium",
            Priority::High => "high",
            Priority::Critical => "critical",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TransitionRecord {
    pub from: CardState,
    pub to: CardState,
    pub actor: String,
    pub at: u64,
    pub reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Card {
    pub id: String,
    pub task_ref: String,
    pub state: CardState,
    pub reason: Option<String>,
    pub assignee: Option<String>,
    pub priority: Option<Priority>,
    pub skills: Vec<String>,
    pub max_retries: u8,
    pub history: Vec<TransitionRecord>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl Card {
    pub fn new(id: &str, task_ref: &str, created_at: u64) -> Self {
        Card {
            id: id.to_string(),
            task_ref: task_ref.to_string(),
            state: CardState::Triage,
            reason: None,
            assignee: None,
            priority: None,
            skills: Vec::new(),
            max_retries: 0,
            history: Vec::new(),
            created_at,
            updated_at: created_at,
        }
    }

    /// Serialize the card to a simple JSON string (including full history).
    fn to_json(&self) -> String {
        let history_items: Vec<String> = self
            .history
            .iter()
            .map(|r| {
                format!(
                    "{{\"from\":\"{}\",\"to\":\"{}\",\"actor\":\"{}\",\"at\":{}}}",
                    r.from.as_str(),
                    r.to.as_str(),
                    r.actor,
                    r.at
                )
            })
            .collect();
        format!(
            "{{\"id\":\"{}\",\"task_ref\":\"{}\",\"state\":\"{}\",\"created_at\":{},\"updated_at\":{},\"history\":[{}]}}",
            self.id,
            self.task_ref,
            self.state.as_str(),
            self.created_at,
            self.updated_at,
            history_items.join(",")
        )
    }

    /// Minimal deserialize from JSON — extracts only the fields we need.
    fn from_json(json: &str) -> Option<Self> {
        let id = extract_json_str(json, "id")?;
        let task_ref = extract_json_str(json, "task_ref").unwrap_or_default();
        let state_str = extract_json_str(json, "state").unwrap_or_else(|| "triage".to_string());
        let state = CardState::parse(&state_str).unwrap_or(CardState::Triage);
        let created_at = extract_json_u64(json, "created_at").unwrap_or(0);
        let updated_at = extract_json_u64(json, "updated_at").unwrap_or(created_at);
        let history = extract_json_history(json);
        Some(Card {
            id,
            task_ref,
            state,
            reason: None,
            assignee: None,
            priority: None,
            skills: Vec::new(),
            max_retries: 0,
            history,
            created_at,
            updated_at,
        })
    }
}

// ── Board ─────────────────────────────────────────────────────────────────────

/// Single per-workspace kanban board.
pub struct Board {
    kanban_dir: PathBuf,
}

impl Board {
    pub fn new(kanban_dir: PathBuf) -> Self {
        Board { kanban_dir }
    }

    pub fn open(ws_path: &Path) -> Self {
        Board {
            kanban_dir: ws_path.join("kanban"),
        }
    }

    fn cards_dir(&self) -> PathBuf {
        self.kanban_dir.join("cards")
    }

    fn archive_dir(&self) -> PathBuf {
        self.kanban_dir.join("archive")
    }

    fn events_dir(&self) -> PathBuf {
        self.kanban_dir.join("events")
    }

    fn card_path(&self, card_id: &str) -> PathBuf {
        self.cards_dir().join(format!("{card_id}.json"))
    }

    fn archive_path(&self, card_id: &str) -> PathBuf {
        self.archive_dir().join(format!("{card_id}.json"))
    }

    /// Ensure board directories exist.
    pub fn init(&self) -> Result<()> {
        fs::create_dir_all(self.cards_dir())?;
        fs::create_dir_all(self.archive_dir())?;
        fs::create_dir_all(self.events_dir())?;
        fs::create_dir_all(self.kanban_dir.join("runs"))?;
        fs::create_dir_all(self.kanban_dir.join("comments"))?;
        // Write board.json meta
        let board_json = r#"{"state_set":["triage","todo","ready","running","blocked","done"]}"#;
        fs::write(self.kanban_dir.join("board.json"), board_json)?;
        Ok(())
    }

    /// Add a new card (starts in Triage). Rejects an id an existing card
    /// already holds — see [`KanbanError::CardAlreadyExists`].
    pub fn add_card(&self, id: &str, task_ref: &str, now: u64) -> Result<Card> {
        validate_card_id(id)?;
        if self.get_card(id)?.is_some() {
            return Err(KanbanError::CardAlreadyExists(id.to_string()));
        }
        fs::create_dir_all(self.cards_dir())?;
        let card = Card::new(id, task_ref, now);
        self.save_card(&card)?;
        Ok(card)
    }

    fn save_card(&self, card: &Card) -> Result<()> {
        fs::create_dir_all(self.cards_dir())?;
        let json = card.to_json();
        fs::write(self.card_path(&card.id), json)?;
        Ok(())
    }

    /// Load a card by ID.
    pub fn get_card(&self, id: &str) -> Result<Option<Card>> {
        validate_card_id(id)?;
        let path = self.card_path(id);
        if !path.exists() {
            return Ok(None);
        }
        let json = fs::read_to_string(&path)?;
        Ok(Card::from_json(&json))
    }

    /// Move a card to a new state, recording the transition in its history.
    pub fn move_card(
        &self,
        card_id: &str,
        to: CardState,
        actor: &str,
        reason: Option<String>,
        now: u64,
    ) -> Result<Card> {
        let mut card = self
            .get_card(card_id)?
            .ok_or_else(|| KanbanError::CardNotFound(card_id.to_string()))?;

        if to == CardState::Blocked && reason.is_none() {
            return Err(KanbanError::BlockedRequiresReason);
        }
        if !card.state.can_transition_to(to) {
            return Err(KanbanError::InvalidTransition {
                from: card.state,
                to,
            });
        }

        let record = TransitionRecord {
            from: card.state,
            to,
            actor: actor.to_string(),
            at: now,
            reason: reason.clone(),
        };
        card.history.push(record);
        card.state = to;
        card.reason = reason;
        card.updated_at = now;

        self.save_card(&card)?;
        self.append_event(
            card_id,
            &format!(
                "{{\"type\":\"transition\",\"to\":\"{}\",\"at\":{now}}}",
                to.as_str()
            ),
        )?;
        Ok(card)
    }

    /// Append a line to the card's event log.
    ///
    /// Creates `events/` first — the same way [`Board::save_card`] creates
    /// `cards/`. Without this, a board whose cards were added via
    /// [`Board::add_card`] (which only creates `cards/`) has no `events/`
    /// directory, so this open fails *after* [`Board::move_card`] has already
    /// written the card in its new state: the transition takes effect but the
    /// command still reports an I/O error.
    fn append_event(&self, card_id: &str, event: &str) -> Result<()> {
        use std::io::Write;
        fs::create_dir_all(self.events_dir())?;
        let path = self.events_dir().join(format!("{card_id}.jsonl"));
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        writeln!(file, "{event}")?;
        Ok(())
    }

    /// List all active cards (not archived).
    pub fn list_cards(&self) -> Result<Vec<Card>> {
        Self::read_cards(&self.cards_dir())
    }

    /// List archived cards — still readable after archival (KAN-4). An
    /// archive directory that does not exist yet holds no cards.
    pub fn list_archived_cards(&self) -> Result<Vec<Card>> {
        Self::read_cards(&self.archive_dir())
    }

    fn read_cards(dir: &Path) -> Result<Vec<Card>> {
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut cards = Vec::new();
        for entry in fs::read_dir(dir)?.flatten() {
            if entry.path().extension().is_some_and(|e| e == "json")
                && let Ok(json) = fs::read_to_string(entry.path())
                && let Some(card) = Card::from_json(&json)
            {
                cards.push(card);
            }
        }
        Ok(cards)
    }

    /// Archive all done cards by moving them to archive/ (non-destructive).
    pub fn archive_done_cards(&self) -> Result<usize> {
        fs::create_dir_all(self.archive_dir())?;
        let cards = self.list_cards()?;
        let mut count = 0;
        for card in cards {
            if card.state == CardState::Done {
                let src = self.card_path(&card.id);
                let dst = self.archive_path(&card.id);
                fs::rename(&src, &dst)?;
                count += 1;
            }
        }
        Ok(count)
    }

    /// Load an archived card (still readable after archival, KAN-4).
    pub fn get_archived_card(&self, id: &str) -> Result<Option<Card>> {
        let path = self.archive_path(id);
        if !path.exists() {
            return Ok(None);
        }
        let json = fs::read_to_string(&path)?;
        Ok(Card::from_json(&json))
    }

    pub fn board_json_exists(&self) -> bool {
        self.kanban_dir.join("board.json").exists()
    }
}

// ── Minimal JSON helpers ─────────────────────────────────────────────────────

fn extract_json_str(json: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{key}\":\"");
    let start = json.find(&pattern)? + pattern.len();
    let rest = &json[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn extract_json_u64(json: &str, key: &str) -> Option<u64> {
    let pattern = format!("\"{key}\":");
    let start = json.find(&pattern)? + pattern.len();
    let rest = &json[start..];
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// Extract the `"history"` array from a card JSON, rebuilding TransitionRecords.
fn extract_json_history(json: &str) -> Vec<TransitionRecord> {
    let marker = "\"history\":[";
    let Some(array_start) = json.find(marker) else {
        return Vec::new();
    };
    let inner_start = array_start + marker.len();
    let rest = &json[inner_start..];
    // find the matching closing bracket
    let Some(array_end) = rest.find(']') else {
        return Vec::new();
    };
    let array = &rest[..array_end];

    let mut records = Vec::new();
    // each entry is delimited by `{...}`
    let mut pos = 0;
    while let Some(obj_start) = array[pos..].find('{') {
        let abs_start = pos + obj_start;
        let Some(obj_end) = array[abs_start..].find('}') else {
            break;
        };
        let obj = &array[abs_start..abs_start + obj_end + 1];

        let from_str = extract_json_str(obj, "from").unwrap_or_default();
        let to_str = extract_json_str(obj, "to").unwrap_or_default();
        let actor = extract_json_str(obj, "actor").unwrap_or_default();
        let at = extract_json_u64(obj, "at").unwrap_or(0);

        if let (Some(from), Some(to)) = (CardState::parse(&from_str), CardState::parse(&to_str)) {
            records.push(TransitionRecord {
                from,
                to,
                actor,
                at,
                reason: None,
            });
        }
        pos = abs_start + obj_end + 1;
    }
    records
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_board() -> (Board, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "cronus-kanban-test-{}-{}",
            std::process::id(),
            now_nanos()
        ));
        let board = Board::new(dir.join("kanban"));
        (board, dir)
    }

    fn now_nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    }

    #[test]
    fn add_card_rejects_path_traversal_and_separator_ids() {
        let (board, root) = tmp_board();
        for bad in [
            "../../../../pwned",
            "..\\..\\pwned",
            "a/b",
            "a\\b",
            "..",
            ".",
            "",
            "/abs",
        ] {
            let err = board.add_card(bad, "TASK", 1).unwrap_err();
            assert!(
                matches!(err, KanbanError::InvalidCardId(_)),
                "id {bad:?} should be rejected as InvalidCardId, got {err:?}"
            );
        }
        // Nothing escaped the board directory.
        assert!(
            !root.parent().unwrap().join("pwned.json").exists(),
            "a traversal id must not have written a file outside the board"
        );
        // A plain id still works.
        assert!(board.add_card("card-1", "TASK", 1).is_ok());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn invalid_transition_message_names_the_legal_targets() {
        // `board block` from Todo (only legal from Running) used to
        // report the rejected pair with no hint of what *was* allowed.
        let err = KanbanError::InvalidTransition {
            from: CardState::Todo,
            to: CardState::Blocked,
        };
        let msg = err.to_string();
        assert!(msg.contains("Ready"), "must name a legal target: {msg}");
        assert!(
            msg.contains("Triage"),
            "must name every legal target: {msg}"
        );
    }

    #[test]
    fn invalid_transition_message_names_a_terminal_state_as_terminal() {
        let err = KanbanError::InvalidTransition {
            from: CardState::Done,
            to: CardState::Todo,
        };
        assert!(err.to_string().contains("terminal"));
    }

    #[test]
    fn legal_targets_matches_can_transition_to_for_every_state() {
        // The derivation this fix relies on: legal_targets() must never
        // drift from can_transition_to()'s own decisions.
        for &from in &[
            CardState::Triage,
            CardState::Todo,
            CardState::Ready,
            CardState::Running,
            CardState::Blocked,
            CardState::Done,
        ] {
            for &to in &[
                CardState::Triage,
                CardState::Todo,
                CardState::Ready,
                CardState::Running,
                CardState::Blocked,
                CardState::Done,
            ] {
                assert_eq!(
                    from.legal_targets().contains(&to),
                    from.can_transition_to(to),
                    "legal_targets()/can_transition_to() disagree for {from:?} -> {to:?}"
                );
            }
        }
    }

    #[test]
    fn add_card_rejects_an_overlong_id_as_a_product_error_not_an_os_one() {
        let (board, root) = tmp_board();
        let long_id = "x".repeat(MAX_CARD_ID_LEN + 1);
        let err = board.add_card(&long_id, "TASK", 1).unwrap_err();
        assert!(
            matches!(err, KanbanError::InvalidCardId(_)),
            "an id past the cap must be InvalidCardId, got {err:?}"
        );

        // At the cap is still fine.
        let ok_id = "x".repeat(MAX_CARD_ID_LEN);
        assert!(board.add_card(&ok_id, "TASK", 1).is_ok());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn add_card_rejects_a_duplicate_id_rather_than_silently_overwriting() {
        let (board, root) = tmp_board();
        let first = board.add_card("c1", "first-ref", 1).unwrap();

        let err = board.add_card("c1", "second-ref", 2).unwrap_err();
        assert!(
            matches!(&err, KanbanError::CardAlreadyExists(id) if id == "c1"),
            "expected CardAlreadyExists(\"c1\"), got {err:?}"
        );

        // The original card survives untouched — task_ref, created_at, and
        // the (empty) history are exactly what the first add produced.
        let reloaded = board.get_card("c1").unwrap().expect("card must survive");
        assert_eq!(reloaded.task_ref, "first-ref");
        assert_eq!(reloaded.created_at, first.created_at);
        assert!(reloaded.history.is_empty());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn get_card_rejects_traversal_ids_too() {
        let (board, root) = tmp_board();
        let err = board.get_card("../../secret").unwrap_err();
        assert!(matches!(err, KanbanError::InvalidCardId(_)));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn move_card_does_not_partially_write_when_events_dir_is_absent() {
        // A board whose card was added via `add_card` — which creates only
        // `cards/`, never `events/`. `move_card` must still succeed end to end
        // (create `events/`, append, return Ok) rather than save the card in
        // its new state and then fail on the event append.
        let (board, root) = tmp_board();
        board.add_card("c1", "TASK", 1).unwrap();
        assert!(!board.events_dir().exists(), "precondition: events/ absent");

        let card = board
            .move_card("c1", CardState::Todo, "tester", None, 2)
            .expect("triage -> todo must succeed even with events/ absent");
        assert_eq!(card.state, CardState::Todo);

        // The transition and its event log are both persisted.
        assert_eq!(
            board.get_card("c1").unwrap().unwrap().state,
            CardState::Todo
        );
        assert!(board.events_dir().join("c1.jsonl").exists());
        let _ = fs::remove_dir_all(&root);
    }

    /// `running → todo` is an allowed backward move, the same kind as
    /// `blocked → todo` — a simulation-qa pass found `can_transition_to`
    /// missing this exact pair, rejecting it as an `InvalidTransition`
    /// despite being a legitimate transition.
    #[test]
    fn running_card_can_move_back_to_todo() {
        let (board, root) = tmp_board();
        board.add_card("c1", "TASK", 1).unwrap();
        board
            .move_card("c1", CardState::Todo, "t", None, 2)
            .unwrap();
        board
            .move_card("c1", CardState::Ready, "t", None, 3)
            .unwrap();
        board
            .move_card("c1", CardState::Running, "t", None, 4)
            .unwrap();

        let card = board
            .move_card("c1", CardState::Todo, "t", None, 5)
            .expect("running -> todo must be a valid backward move");
        assert_eq!(card.state, CardState::Todo);
        let _ = fs::remove_dir_all(&root);
    }
}
