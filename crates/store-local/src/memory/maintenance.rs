//! Corpus-maintenance pass: confidence-gated actions against
//! accumulation pathologies, run off the hot path. Each action's blast
//! radius sets its own gate — additive/reversible actions (`archive`) apply
//! automatically; a lossy action (`merge`) requires an unambiguous signal.
//!
//! `summarize` (edge-graph community detection) needs the edge
//! graph, which the consolidation-write module owns; this module only
//! covers the three actions buildable against the signal/lifecycle schema
//! already in place (no edges needed).

use rusqlite::{Connection, params};

use super::Result;
use super::signal::{self, SignalKind};
use cronus_contract::{LifecycleState, MemoryId};

// ── schema ────────────────────────────────────────────────────────────────────

pub(crate) fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS maintenance_audit (
            item_id    TEXT    NOT NULL,
            action     TEXT    NOT NULL,
            target_id  TEXT,
            actor      TEXT    NOT NULL,
            instant    INTEGER NOT NULL
        )",
    )?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS maintenance_cooldown (
            item_id TEXT PRIMARY KEY NOT NULL,
            action  TEXT NOT NULL,
            until   INTEGER NOT NULL
        )",
    )?;
    Ok(())
}

// ── anti-cycle cooldown ──────────────────────────────────────────────────────

/// Idle window (seconds) an item must wait after one maintenance action
/// before an *opposing* action may target it again — prevents a
/// split→merge→split oscillation.
pub const ANTI_CYCLE_COOLDOWN_SECS: u64 = 3_600;

fn under_cooldown(conn: &Connection, item_id: &MemoryId, now: u64) -> Result<bool> {
    let until: Option<i64> = conn
        .query_row(
            "SELECT until FROM maintenance_cooldown WHERE item_id = ?1",
            params![item_id.as_str()],
            |row| row.get(0),
        )
        .optional_or_none()?;
    Ok(until.is_some_and(|u| (u as u64) > now))
}

fn set_cooldown(conn: &Connection, item_id: &MemoryId, action: &str, now: u64) -> Result<()> {
    conn.execute(
        "INSERT INTO maintenance_cooldown (item_id, action, until)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(item_id) DO UPDATE SET action = excluded.action, until = excluded.until",
        params![
            item_id.as_str(),
            action,
            (now + ANTI_CYCLE_COOLDOWN_SECS) as i64
        ],
    )?;
    Ok(())
}

fn record_audit(
    conn: &Connection,
    item_id: &MemoryId,
    action: &str,
    target: Option<&MemoryId>,
    actor: &str,
    now: u64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO maintenance_audit (item_id, action, target_id, actor, instant)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            item_id.as_str(),
            action,
            target.map(MemoryId::as_str),
            actor,
            now as i64,
        ],
    )?;
    Ok(())
}

// ── recency (feeds both ranking and this module's archive gate) ───────

/// Recompute the `Recency` derived signal for every active item: an
/// exponential decay of age-since-`created_at`, half-life `HALFLIFE_SECS`.
/// This is "step 1: recompute derived signals" for recency only — centrality
/// and cluster recomputation need the edge graph, owned by the
/// consolidation-write module.
const RECENCY_HALFLIFE_SECS: f64 = 30.0 * 24.0 * 3600.0; // 30 days, stub default

pub(crate) fn recompute_recency(conn: &Connection, now: u64) -> Result<usize> {
    let mut stmt =
        conn.prepare("SELECT id, created_at FROM memories WHERE lifecycle_state = ?1")?;
    let rows: Vec<(String, i64)> = stmt
        .query_map(params![LifecycleState::Active.as_str()], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .collect::<std::result::Result<_, _>>()?;

    let mut updated = 0;
    for (id_str, created_at) in rows {
        let id = MemoryId::from(id_str);
        let age_secs = now.saturating_sub(created_at.max(0) as u64) as f64;
        let recency = (-age_secs * std::f64::consts::LN_2 / RECENCY_HALFLIFE_SECS).exp();
        signal::write(conn, &id, SignalKind::Recency, recency, now)?;
        updated += 1;
    }
    Ok(updated)
}

// ── Archive (additive/reversible — auto-applies) ──────────────────────

/// Archive threshold: an item whose recency factor falls below this,
/// cushioned by centrality (a well-connected hub tolerates more staleness
/// before archiving), becomes a candidate.
pub const ARCHIVE_RECENCY_THRESHOLD: f64 = 0.1;

/// Sweep active items and archive those whose cushioned recency has decayed
/// past the threshold. Auto-applies (no elevated gate — reversible).
/// Skips an item under an opposing cooldown. Returns the archived ids.
pub(crate) fn sweep_archive(conn: &Connection, actor: &str, now: u64) -> Result<Vec<MemoryId>> {
    let mut stmt = conn.prepare("SELECT id FROM memories WHERE lifecycle_state = ?1")?;
    let ids: Vec<String> = stmt
        .query_map(params![LifecycleState::Active.as_str()], |row| row.get(0))?
        .collect::<std::result::Result<_, _>>()?;

    let mut archived = Vec::new();
    for id_str in ids {
        let id = MemoryId::from(id_str);
        if under_cooldown(conn, &id, now)? {
            continue;
        }

        let recency = signal::factor(conn, &id, SignalKind::Recency)?;
        let centrality = signal::factor(conn, &id, SignalKind::Centrality)?;
        // A hub (higher centrality) needs a lower effective threshold to
        // trigger archiving — i.e. it is harder to archive, not easier.
        let effective_threshold = ARCHIVE_RECENCY_THRESHOLD / (1.0 + centrality);

        if recency < effective_threshold {
            conn.execute(
                "UPDATE memories SET lifecycle_state = ?1 WHERE id = ?2",
                params![LifecycleState::Archived.as_str(), id.as_str()],
            )?;
            conn.execute(
                "INSERT INTO lifecycle_audit (item_id, actor, instant, old_state, new_state)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    id.as_str(),
                    actor,
                    now as i64,
                    LifecycleState::Active.as_str(),
                    LifecycleState::Archived.as_str(),
                ],
            )?;
            record_audit(conn, &id, "archive", None, actor, now)?;
            set_cooldown(conn, &id, "archive", now)?;
            archived.push(id);
        }
    }
    Ok(archived)
}

/// Auto-thaw: touching an archived item (a recall hit, a consolidation
/// update) reverses the archive — the design's "auto-thawed the instant anything
/// touches the node." No-op (returns `false`) for any other state.
pub(crate) fn touch(conn: &Connection, id: &MemoryId, actor: &str, now: u64) -> Result<bool> {
    let current: Option<String> = conn
        .query_row(
            "SELECT lifecycle_state FROM memories WHERE id = ?1",
            params![id.as_str()],
            |row| row.get(0),
        )
        .optional_or_none()?;
    if current.as_deref() != Some(LifecycleState::Archived.as_str()) {
        return Ok(false);
    }

    conn.execute(
        "UPDATE memories SET lifecycle_state = ?1 WHERE id = ?2",
        params![LifecycleState::Active.as_str(), id.as_str()],
    )?;
    conn.execute(
        "INSERT INTO lifecycle_audit (item_id, actor, instant, old_state, new_state)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            id.as_str(),
            actor,
            now as i64,
            LifecycleState::Archived.as_str(),
            LifecycleState::Active.as_str(),
        ],
    )?;
    Ok(true)
}

// ── Split (dispersion heuristic; no-generator = successful no-op) ────

/// A body longer than this is *flagged* as an overload candidate. Real
/// splitting (topic segmentation into an overview + children) needs a
/// generator; with none bound, flagging is the complete, honest behavior —
/// a no-generator no-op, consistent with the other no-generator paths.
pub const SPLIT_LENGTH_THRESHOLD: usize = 4_000;

/// Identify active items whose content crosses the overload threshold.
/// Does **not** split them — no generator is wired in this phase; the
/// candidate list is the queued work for whenever one is.
pub(crate) fn flag_split_candidates(conn: &Connection) -> Result<Vec<MemoryId>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM memories
         WHERE lifecycle_state = ?1 AND length(body) > ?2",
    )?;
    let ids = stmt
        .query_map(
            params![
                LifecycleState::Active.as_str(),
                SPLIT_LENGTH_THRESHOLD as i64
            ],
            |row| row.get::<_, String>(0),
        )?
        .map(|r| r.map(MemoryId::from))
        .collect::<std::result::Result<_, _>>()?;
    Ok(ids)
}

// ── Merge (lossy — elevated gate, transactional) ──────────────────────

/// Merge-candidate detection: two **active** items whose bodies are
/// identical after case/whitespace normalization. Exact-after-normalization
/// is the domain-logic-first stand-in for "multi-sample agreement" (the
/// elevated gate) — unambiguous, not a similarity heuristic that could
/// misfire; a fuzzier detector is future work, not a regression risk today.
pub(crate) fn find_merge_candidates(conn: &Connection) -> Result<Vec<(MemoryId, MemoryId)>> {
    let mut stmt =
        conn.prepare("SELECT id, body, created_at FROM memories WHERE lifecycle_state = ?1")?;
    let rows: Vec<(String, String, i64)> = stmt
        .query_map(params![LifecycleState::Active.as_str()], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .collect::<std::result::Result<_, _>>()?;

    let normalize = |s: &str| s.trim().to_lowercase();
    let mut pairs = Vec::new();
    for i in 0..rows.len() {
        for j in (i + 1)..rows.len() {
            if normalize(&rows[i].1) == normalize(&rows[j].1) {
                // Keep the newer of the pair; discard the older.
                let (keep, discard) = if rows[i].2 >= rows[j].2 {
                    (&rows[i].0, &rows[j].0)
                } else {
                    (&rows[j].0, &rows[i].0)
                };
                pairs.push((
                    MemoryId::from(keep.clone()),
                    MemoryId::from(discard.clone()),
                ));
            }
        }
    }
    Ok(pairs)
}

/// Merge `discard` into `keep`: re-point `discard`'s chain edges *and* the
/// relationship/provenance edges onto `keep`, drop `discard`'s derived
/// signals, hard-delete `discard`, all inside one transaction (
/// multi-item actions commit whole-or-rollback).
pub(crate) fn merge_pair(
    conn: &Connection,
    keep: &MemoryId,
    discard: &MemoryId,
    actor: &str,
    now: u64,
) -> Result<()> {
    conn.execute_batch("BEGIN")?;
    let result = (|| -> Result<()> {
        conn.execute(
            "UPDATE memory_chains SET source_id = ?1 WHERE source_id = ?2",
            params![keep.as_str(), discard.as_str()],
        )?;
        conn.execute(
            "UPDATE memory_chains SET target_id = ?1 WHERE target_id = ?2",
            params![keep.as_str(), discard.as_str()],
        )?;
        // A self-chain can result from re-pointing both ends onto `keep`.
        conn.execute("DELETE FROM memory_chains WHERE source_id = target_id", [])?;
        // Same re-pointing for the edge table. `OR IGNORE`:
        // re-pointing could collide with an edge `keep` already has (the
        // table's UNIQUE constraint), which is fine — the edge already
        // exists on `keep`, so the discard's copy is redundant, not lost.
        conn.execute(
            "UPDATE OR IGNORE memory_edge SET source_id = ?1 WHERE source_id = ?2",
            params![keep.as_str(), discard.as_str()],
        )?;
        conn.execute(
            "UPDATE OR IGNORE memory_edge SET target_id = ?1 WHERE target_id = ?2",
            params![keep.as_str(), discard.as_str()],
        )?;
        conn.execute("DELETE FROM memory_edge WHERE source_id = target_id", [])?;
        conn.execute(
            "DELETE FROM memory_edge WHERE source_id = ?1 OR target_id = ?1",
            params![discard.as_str()],
        )?;
        signal::clear(conn, discard)?;
        conn.execute(
            "DELETE FROM memories_fts WHERE memory_id = ?1",
            params![discard.as_str()],
        )?;
        conn.execute(
            "DELETE FROM memories WHERE id = ?1",
            params![discard.as_str()],
        )?;
        record_audit(conn, keep, "merge", Some(discard), actor, now)?;
        set_cooldown(conn, keep, "merge", now)?;
        Ok(())
    })();

    match result {
        Ok(()) => {
            conn.execute_batch("COMMIT")?;
            Ok(())
        }
        Err(e) => {
            conn.execute_batch("ROLLBACK")?;
            Err(e)
        }
    }
}

// ── small helper: rusqlite's `.optional()` under our `Result` alias ────────

trait OptionalOrNone<T> {
    fn optional_or_none(self) -> Result<Option<T>>;
}

impl<T> OptionalOrNone<T> for rusqlite::Result<T> {
    fn optional_or_none(self) -> Result<Option<T>> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::memory::store::setup(&c).unwrap();
        c
    }

    #[test]
    fn recompute_recency_writes_a_neutral_ish_score_for_a_fresh_item() {
        let c = conn();
        c.execute(
            "INSERT INTO memories (id, kind, source, title, body, valid_at, created_at)
             VALUES ('a', 'Convention', 'Agent', 't', 'b', 1000, 1000)",
            [],
        )
        .unwrap();
        recompute_recency(&c, 1000).unwrap();
        let f = signal::factor(&c, &MemoryId::from("a".to_string()), SignalKind::Recency).unwrap();
        assert!(
            (f - 1.0).abs() < 1e-9,
            "zero age must decay to ~1.0, got {f}"
        );
    }

    #[test]
    fn recompute_recency_decays_with_age() {
        let c = conn();
        c.execute(
            "INSERT INTO memories (id, kind, source, title, body, valid_at, created_at)
             VALUES ('old', 'Convention', 'Agent', 't', 'b', 0, 0)",
            [],
        )
        .unwrap();
        let now = (RECENCY_HALFLIFE_SECS * 3.0) as u64;
        recompute_recency(&c, now).unwrap();
        let f =
            signal::factor(&c, &MemoryId::from("old".to_string()), SignalKind::Recency).unwrap();
        assert!(
            f < 0.2,
            "three half-lives out must have decayed well below 0.2, got {f}"
        );
    }

    #[test]
    fn sweep_archive_shelves_a_stale_item_and_leaves_a_fresh_one() {
        let c = conn();
        let now = (RECENCY_HALFLIFE_SECS * 10.0) as u64;
        c.execute(
            "INSERT INTO memories (id, kind, source, title, body, valid_at, created_at)
             VALUES ('stale', 'Convention', 'Agent', 't', 'b', 0, 0)",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO memories (id, kind, source, title, body, valid_at, created_at)
             VALUES ('fresh', 'Convention', 'Agent', 't', 'b', ?1, ?1)",
            params![now as i64],
        )
        .unwrap();
        recompute_recency(&c, now).unwrap();

        let archived = sweep_archive(&c, "test", now).unwrap();
        assert_eq!(archived, vec![MemoryId::from("stale".to_string())]);

        let stale_state: String = c
            .query_row(
                "SELECT lifecycle_state FROM memories WHERE id = 'stale'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let fresh_state: String = c
            .query_row(
                "SELECT lifecycle_state FROM memories WHERE id = 'fresh'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stale_state, "Archived");
        assert_eq!(fresh_state, "Active");
    }

    #[test]
    fn touch_thaws_an_archived_item_but_not_an_active_one() {
        let c = conn();
        c.execute(
            "INSERT INTO memories (id, kind, source, title, body, valid_at, created_at, lifecycle_state)
             VALUES ('arch', 'Convention', 'Agent', 't', 'b', 0, 0, 'Archived')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO memories (id, kind, source, title, body, valid_at, created_at)
             VALUES ('act', 'Convention', 'Agent', 't', 'b', 0, 0)",
            [],
        )
        .unwrap();

        assert!(touch(&c, &MemoryId::from("arch".to_string()), "test", 100).unwrap());
        assert!(!touch(&c, &MemoryId::from("act".to_string()), "test", 100).unwrap());

        let state: String = c
            .query_row(
                "SELECT lifecycle_state FROM memories WHERE id = 'arch'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(state, "Active");
    }

    #[test]
    fn flag_split_candidates_finds_only_long_bodies() {
        let c = conn();
        let long_body = "x".repeat(SPLIT_LENGTH_THRESHOLD + 1);
        c.execute(
            "INSERT INTO memories (id, kind, source, title, body, valid_at, created_at)
             VALUES ('long', 'Convention', 'Agent', 't', ?1, 0, 0)",
            params![long_body],
        )
        .unwrap();
        c.execute(
            "INSERT INTO memories (id, kind, source, title, body, valid_at, created_at)
             VALUES ('short', 'Convention', 'Agent', 't', 'tiny', 0, 0)",
            [],
        )
        .unwrap();

        let candidates = flag_split_candidates(&c).unwrap();
        assert_eq!(candidates, vec![MemoryId::from("long".to_string())]);
    }

    #[test]
    fn find_merge_candidates_pairs_normalized_duplicates_only() {
        let c = conn();
        c.execute(
            "INSERT INTO memories (id, kind, source, title, body, valid_at, created_at)
             VALUES ('a', 'Convention', 'Agent', 't', '  Same Body  ', 0, 100)",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO memories (id, kind, source, title, body, valid_at, created_at)
             VALUES ('b', 'Convention', 'Agent', 't', 'same body', 0, 200)",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO memories (id, kind, source, title, body, valid_at, created_at)
             VALUES ('c', 'Convention', 'Agent', 't', 'different', 0, 300)",
            [],
        )
        .unwrap();

        let pairs = find_merge_candidates(&c).unwrap();
        assert_eq!(pairs.len(), 1);
        // 'b' is newer (created_at=200 > 100) so it is kept, 'a' discarded.
        assert_eq!(pairs[0].0, MemoryId::from("b".to_string()));
        assert_eq!(pairs[0].1, MemoryId::from("a".to_string()));
    }

    #[test]
    fn merge_pair_repoints_chains_and_hard_deletes_the_discard() {
        let c = conn();
        let keep = MemoryId::from("keep".to_string());
        let discard = MemoryId::from("discard".to_string());
        let other = MemoryId::from("other".to_string());
        for (id, body) in [
            (&keep, "keep body"),
            (&discard, "discard body"),
            (&other, "other body"),
        ] {
            c.execute(
                "INSERT INTO memories (id, kind, source, title, body, valid_at, created_at)
                 VALUES (?1, 'Convention', 'Agent', 't', ?2, 0, 0)",
                params![id.as_str(), body],
            )
            .unwrap();
        }
        // other -> discard chain edge; must be re-pointed to `keep`.
        c.execute(
            "INSERT INTO memory_chains (source_id, target_id, kind, created_at)
             VALUES (?1, ?2, 'RelatedTo', 0)",
            params![other.as_str(), discard.as_str()],
        )
        .unwrap();

        merge_pair(&c, &keep, &discard, "test", 100).unwrap();

        let discard_exists: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM memories WHERE id = ?1",
                params![discard.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(discard_exists, 0, "discard must be hard-deleted");

        let repointed: String = c
            .query_row(
                "SELECT target_id FROM memory_chains WHERE source_id = ?1",
                params![other.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            repointed, "keep",
            "chain edge must be re-pointed onto `keep`"
        );
    }
}
