use std::sync::atomic::{AtomicU64, Ordering};

use cronus_core::kanban::{Board, CardState, KanbanError};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_board(label: &str) -> Board {
    let pid = std::process::id();
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir()
        .join("kanban")
        .join(format!("{pid}-{id}-{label}"));
    Board::new(dir)
}

const T0: u64 = 1_000_000_000;

// ── Board initialisation ──────────────────────────────────────────────────────

#[test]
fn board_init_creates_required_directories() {
    let board = temp_board("init");
    board.init().unwrap();
    assert!(
        board.board_json_exists(),
        "board.json should exist after init"
    );
}

// ── Card CRUD ─────────────────────────────────────────────────────────────────

#[test]
fn add_card_starts_in_triage() {
    let board = temp_board("add");
    board.init().unwrap();
    let card = board.add_card("c1", "task-ref-1", T0).unwrap();
    assert_eq!(card.state, CardState::Triage);
    assert_eq!(card.id, "c1");
    assert_eq!(card.task_ref, "task-ref-1");
}

#[test]
fn adding_an_existing_card_id_is_rejected_not_upserted() {
    let board = temp_board("add-dup");
    board.init().unwrap();
    board.add_card("c1", "first-ref", T0).unwrap();

    let err = board.add_card("c1", "second-ref", T0 + 1).unwrap_err();
    assert!(matches!(err, KanbanError::CardAlreadyExists(id) if id == "c1"));

    // Confirmed against the actual on-disk state, not just the error type:
    // the first card's task_ref must survive a second `add` for the same id.
    let card = board.get_card("c1").unwrap().unwrap();
    assert_eq!(card.task_ref, "first-ref");
}

#[test]
fn get_card_returns_none_when_not_found() {
    let board = temp_board("get-none");
    board.init().unwrap();
    let result = board.get_card("no-such-card").unwrap();
    assert!(result.is_none());
}

#[test]
fn add_then_get_card_round_trip() {
    let board = temp_board("round-trip");
    board.init().unwrap();
    board.add_card("c2", "task-ref-2", T0).unwrap();
    let loaded = board.get_card("c2").unwrap().expect("card should exist");
    assert_eq!(loaded.id, "c2");
    assert_eq!(loaded.state, CardState::Triage);
}

// ── State machine transitions ─────────────────────────────────────────────────

#[test]
fn transition_triage_to_todo_succeeds() {
    let board = temp_board("triage-todo");
    board.init().unwrap();
    board.add_card("c3", "t", T0).unwrap();
    let card = board
        .move_card("c3", CardState::Todo, "agent", None, T0 + 1)
        .unwrap();
    assert_eq!(card.state, CardState::Todo);
    assert_eq!(card.history.len(), 1);
    assert_eq!(card.history[0].from, CardState::Triage);
    assert_eq!(card.history[0].to, CardState::Todo);
}

#[test]
fn transition_records_history_entry_per_move() {
    let board = temp_board("history");
    board.init().unwrap();
    board.add_card("c4", "t", T0).unwrap();
    board
        .move_card("c4", CardState::Todo, "agent", None, T0 + 1)
        .unwrap();
    board
        .move_card("c4", CardState::Ready, "agent", None, T0 + 2)
        .unwrap();
    let card = board
        .move_card("c4", CardState::Running, "agent", None, T0 + 3)
        .unwrap();
    assert_eq!(
        card.history.len(),
        3,
        "three transitions should produce three history entries"
    );
}

#[test]
fn transition_blocked_requires_reason() {
    let board = temp_board("blocked-reason");
    board.init().unwrap();
    board.add_card("c5", "t", T0).unwrap();
    board
        .move_card("c5", CardState::Todo, "agent", None, T0 + 1)
        .unwrap();
    board
        .move_card("c5", CardState::Ready, "agent", None, T0 + 2)
        .unwrap();
    board
        .move_card("c5", CardState::Running, "agent", None, T0 + 3)
        .unwrap();
    let err = board
        .move_card("c5", CardState::Blocked, "agent", None, T0 + 4)
        .unwrap_err();
    assert!(matches!(err, KanbanError::BlockedRequiresReason));
}

#[test]
fn transition_blocked_with_reason_succeeds() {
    let board = temp_board("blocked-with-reason");
    board.init().unwrap();
    board.add_card("c6", "t", T0).unwrap();
    board
        .move_card("c6", CardState::Todo, "agent", None, T0 + 1)
        .unwrap();
    board
        .move_card("c6", CardState::Ready, "agent", None, T0 + 2)
        .unwrap();
    board
        .move_card("c6", CardState::Running, "agent", None, T0 + 3)
        .unwrap();
    let card = board
        .move_card(
            "c6",
            CardState::Blocked,
            "agent",
            Some("awaiting approval".to_string()),
            T0 + 4,
        )
        .unwrap();
    assert_eq!(card.state, CardState::Blocked);
    assert_eq!(card.reason.as_deref(), Some("awaiting approval"));
}

#[test]
fn invalid_transition_returns_error() {
    let board = temp_board("invalid-trans");
    board.init().unwrap();
    board.add_card("c7", "t", T0).unwrap();
    // triage → running is invalid
    let err = board
        .move_card("c7", CardState::Running, "agent", None, T0 + 1)
        .unwrap_err();
    assert!(matches!(err, KanbanError::InvalidTransition { .. }));
}

#[test]
fn move_card_nonexistent_returns_not_found() {
    let board = temp_board("not-found");
    board.init().unwrap();
    let err = board
        .move_card("ghost", CardState::Todo, "agent", None, T0)
        .unwrap_err();
    assert!(matches!(err, KanbanError::CardNotFound(_)));
}

// ── Done → archive ────────────────────────────────────────────────────────────

#[test]
fn archive_done_cards_moves_done_to_archive() {
    let board = temp_board("archive");
    board.init().unwrap();
    board.add_card("c8", "t", T0).unwrap();
    board
        .move_card("c8", CardState::Todo, "a", None, T0 + 1)
        .unwrap();
    board
        .move_card("c8", CardState::Ready, "a", None, T0 + 2)
        .unwrap();
    board
        .move_card("c8", CardState::Running, "a", None, T0 + 3)
        .unwrap();
    board
        .move_card("c8", CardState::Done, "a", None, T0 + 4)
        .unwrap();

    let archived = board.archive_done_cards().unwrap();
    assert_eq!(archived, 1, "one done card should be archived");

    // Card should no longer be in active list
    let active = board.list_cards().unwrap();
    assert!(
        active.iter().all(|c| c.id != "c8"),
        "archived card should not be in active list"
    );
}

#[test]
fn archived_card_is_still_readable_after_archive() {
    let board = temp_board("archive-read");
    board.init().unwrap();
    board.add_card("c9", "t", T0).unwrap();
    board
        .move_card("c9", CardState::Todo, "a", None, T0 + 1)
        .unwrap();
    board
        .move_card("c9", CardState::Ready, "a", None, T0 + 2)
        .unwrap();
    board
        .move_card("c9", CardState::Running, "a", None, T0 + 3)
        .unwrap();
    board
        .move_card("c9", CardState::Done, "a", None, T0 + 4)
        .unwrap();
    board.archive_done_cards().unwrap();

    // Archived card is still readable
    let card = board.get_archived_card("c9").unwrap();
    assert!(card.is_some(), "archived card should be readable");
    assert_eq!(card.unwrap().id, "c9");
}

#[test]
fn archive_skips_non_done_cards() {
    let board = temp_board("archive-skip");
    board.init().unwrap();
    board.add_card("c10", "t", T0).unwrap();
    board
        .move_card("c10", CardState::Todo, "a", None, T0 + 1)
        .unwrap();
    // Card is in Todo, not Done
    let archived = board.archive_done_cards().unwrap();
    assert_eq!(archived, 0);
    // Card still in active list
    let active = board.list_cards().unwrap();
    assert!(active.iter().any(|c| c.id == "c10"));
}

// ── list_cards ────────────────────────────────────────────────────────────────

#[test]
fn list_cards_returns_all_active_cards() {
    let board = temp_board("list");
    board.init().unwrap();
    board.add_card("ca", "t", T0).unwrap();
    board.add_card("cb", "t", T0).unwrap();
    let cards = board.list_cards().unwrap();
    assert_eq!(cards.len(), 2);
}

#[test]
fn list_cards_empty_board_returns_empty_vec() {
    let board = temp_board("list-empty");
    board.init().unwrap();
    let cards = board.list_cards().unwrap();
    assert!(cards.is_empty());
}

// ── list_archived_cards ───────────────────────────────────────────────────────

#[test]
fn list_archived_cards_holds_only_archived_cards_and_is_empty_before_any_archival() {
    let board = temp_board("archive-list");
    board.init().unwrap();
    assert!(
        board.list_archived_cards().unwrap().is_empty(),
        "no archive directory yet means no archived cards"
    );

    board.add_card("c11", "t", T0).unwrap();
    for (i, state) in [
        CardState::Todo,
        CardState::Ready,
        CardState::Running,
        CardState::Done,
    ]
    .into_iter()
    .enumerate()
    {
        board
            .move_card("c11", state, "a", None, T0 + 1 + i as u64)
            .unwrap();
    }
    board.add_card("c12", "t", T0 + 10).unwrap();
    board.archive_done_cards().unwrap();

    let archived: Vec<String> = board
        .list_archived_cards()
        .unwrap()
        .into_iter()
        .map(|c| c.id)
        .collect();
    assert_eq!(archived, ["c11"], "only the archived card is listed");
    let active: Vec<String> = board
        .list_cards()
        .unwrap()
        .into_iter()
        .map(|c| c.id)
        .collect();
    assert_eq!(active, ["c12"], "the archived card left the live list");
}
