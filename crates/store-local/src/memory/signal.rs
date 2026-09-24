//! Derived-signal store: the fact-vs-derived boundary. A row here is
//! computed, versioned, and disposable — never authored fact, never written
//! into `memories`. Absent or version-stale signals degrade to a neutral
//! multiplier rather than blocking or erroring, so a cold corpus with
//! no signals computed yet is a fully supported state, not a failure mode.

use rusqlite::{Connection, OptionalExtension, params};

use super::Result;
use cronus_contract::MemoryId;

/// A derived ranking signal kind. Closed set — adding a kind is a code
/// change, not configuration, so `current_version()` always reflects the
/// algorithm that produced rows of that kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalKind {
    /// Graph in-degree over the fact-layer edges (edges → ranking factor).
    Centrality,
    /// Topic-cluster membership from community detection (community detection → ranking factor).
    Cluster,
    /// Recency, decayed with age and cushioned by centrality (archive → ranking factor).
    Recency,
}

impl SignalKind {
    fn as_str(self) -> &'static str {
        match self {
            SignalKind::Centrality => "centrality",
            SignalKind::Cluster => "cluster",
            SignalKind::Recency => "recency",
        }
    }

    /// The algorithm version this build computes for this kind. A stored row
    /// whose `version` differs was computed by a since-changed algorithm and
    /// is treated as absent rather than trusted.
    fn current_version(self) -> i64 {
        match self {
            SignalKind::Centrality => 1,
            SignalKind::Cluster => 1,
            SignalKind::Recency => 1,
        }
    }
}

/// The neutral multiplier substituted for an absent or version-stale signal
///  — ranking degrades gracefully, it never blocks or errors.
pub const NEUTRAL_FACTOR: f64 = 1.0;

pub(crate) fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS memory_signal (
            item_id     TEXT    NOT NULL,
            signal_kind TEXT    NOT NULL,
            value       REAL    NOT NULL,
            version     INTEGER NOT NULL,
            computed_at INTEGER NOT NULL,
            PRIMARY KEY (item_id, signal_kind)
        )",
    )?;
    Ok(())
}

/// Write (or overwrite) a derived signal, always stamped with this build's
/// `current_version()` — a caller cannot accidentally persist a stale one.
pub(crate) fn write(
    conn: &Connection,
    item_id: &MemoryId,
    kind: SignalKind,
    value: f64,
    computed_at: u64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO memory_signal (item_id, signal_kind, value, version, computed_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(item_id, signal_kind) DO UPDATE SET
            value = excluded.value,
            version = excluded.version,
            computed_at = excluded.computed_at",
        params![
            item_id.as_str(),
            kind.as_str(),
            value,
            kind.current_version(),
            computed_at as i64,
        ],
    )?;
    Ok(())
}

/// Read a derived signal's ranking factor. Returns [`NEUTRAL_FACTOR`] when the
/// row is absent OR its stored `version` no longer matches this build's
/// `current_version()` — never an error, never a hot-path block. No
/// warning is logged here: absence is the expected, fully-supported
/// cold-start state, not an anomaly, and this stays on the recall hot path,
/// so it does no more than one indexed point lookup.
pub(crate) fn factor(conn: &Connection, item_id: &MemoryId, kind: SignalKind) -> Result<f64> {
    let row: Option<(f64, i64)> = conn
        .query_row(
            "SELECT value, version FROM memory_signal WHERE item_id = ?1 AND signal_kind = ?2",
            params![item_id.as_str(), kind.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;

    Ok(match row {
        Some((value, version)) if version == kind.current_version() => value,
        _ => NEUTRAL_FACTOR,
    })
}

/// Remove every signal for an item (e.g. before a merge discards it).
pub(crate) fn clear(conn: &Connection, item_id: &MemoryId) -> Result<()> {
    conn.execute(
        "DELETE FROM memory_signal WHERE item_id = ?1",
        params![item_id.as_str()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        migrate(&c).unwrap();
        c
    }

    #[test]
    fn absent_signal_degrades_to_neutral_factor() {
        let c = conn();
        let id = MemoryId::new();
        assert_eq!(
            factor(&c, &id, SignalKind::Centrality).unwrap(),
            NEUTRAL_FACTOR
        );
    }

    #[test]
    fn written_signal_is_read_back_exactly() {
        let c = conn();
        let id = MemoryId::new();
        write(&c, &id, SignalKind::Centrality, 0.75, 1_000).unwrap();
        assert_eq!(factor(&c, &id, SignalKind::Centrality).unwrap(), 0.75);
    }

    #[test]
    fn writing_twice_overwrites_not_duplicates() {
        let c = conn();
        let id = MemoryId::new();
        write(&c, &id, SignalKind::Recency, 0.2, 1_000).unwrap();
        write(&c, &id, SignalKind::Recency, 0.9, 2_000).unwrap();
        assert_eq!(factor(&c, &id, SignalKind::Recency).unwrap(), 0.9);

        let count: i64 = c
            .query_row("SELECT COUNT(*) FROM memory_signal", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "overwrite must not accrete duplicate rows");
    }

    #[test]
    fn different_kinds_for_the_same_item_are_independent() {
        let c = conn();
        let id = MemoryId::new();
        write(&c, &id, SignalKind::Centrality, 0.4, 1_000).unwrap();
        write(&c, &id, SignalKind::Cluster, 0.6, 1_000).unwrap();
        assert_eq!(factor(&c, &id, SignalKind::Centrality).unwrap(), 0.4);
        assert_eq!(factor(&c, &id, SignalKind::Cluster).unwrap(), 0.6);
        assert_eq!(
            factor(&c, &id, SignalKind::Recency).unwrap(),
            NEUTRAL_FACTOR
        );
    }

    #[test]
    fn version_mismatch_degrades_to_neutral_not_the_stale_value() {
        let c = conn();
        let id = MemoryId::new();
        // Simulate a row computed by an old algorithm version.
        c.execute(
            "INSERT INTO memory_signal (item_id, signal_kind, value, version, computed_at)
             VALUES (?1, 'centrality', 0.99, 0, 1000)",
            params![id.as_str()],
        )
        .unwrap();
        assert_eq!(
            factor(&c, &id, SignalKind::Centrality).unwrap(),
            NEUTRAL_FACTOR
        );
    }

    #[test]
    fn clear_removes_every_kind_for_the_item_only() {
        let c = conn();
        let id = MemoryId::new();
        let other = MemoryId::new();
        write(&c, &id, SignalKind::Centrality, 0.5, 1_000).unwrap();
        write(&c, &id, SignalKind::Cluster, 0.5, 1_000).unwrap();
        write(&c, &other, SignalKind::Centrality, 0.5, 1_000).unwrap();

        clear(&c, &id).unwrap();

        assert_eq!(
            factor(&c, &id, SignalKind::Centrality).unwrap(),
            NEUTRAL_FACTOR
        );
        assert_eq!(
            factor(&c, &id, SignalKind::Cluster).unwrap(),
            NEUTRAL_FACTOR
        );
        assert_eq!(factor(&c, &other, SignalKind::Centrality).unwrap(), 0.5);
    }
}
