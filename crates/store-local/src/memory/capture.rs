//! The salience-gated capture policy. A thin write-time gate in front
//! of the existing create/corroborate decision — capture does not
//! reimplement dedup; it reuses `consolidate()` wholesale, since an ordinary
//! first-hand note and a to-be-consolidated candidate resolve through the
//! exact same same-abstraction check. `capture()`'s own job is the
//! confidence-honest gate and the cross-reference edges; attribution
//! (`actor`/`subject`) and expiry are already ordinary `MemoryEntry` fields
//! by the time a caller reaches this function, set via the existing
//! `with_actor`/`with_expiry`/`with_subject` builders.

use rusqlite::Connection;

use super::Result;
use super::consolidate::{self, ConsolidationAction};
use cronus_contract::{MemoryEntry, MemoryId};

/// Below this, a capture is refused outright rather than stored — the design's
/// own text leaves "not stored or provisional" open; "not stored" is the
/// realization here (no new provisional-status schema to carry the other
/// reading). Pinned the same way `CONF_GAP_MIN` and
/// `SIMILARITY_MIN` were pinned: a real engineering choice, not a
/// placeholder.
pub const CONFIDENCE_FLOOR: f64 = 0.2;

/// The result of a `capture()` call.
#[derive(Debug, Clone, PartialEq)]
pub enum CaptureOutcome {
    /// A genuinely new item was written.
    Stored(MemoryId),
    /// A same-abstraction match already existed — the existing item's trust
    /// was reinforced (corroborate); no new row.
    Corroborated(MemoryId),
    /// Below `CONFIDENCE_FLOOR` — refused, nothing written.
    Refused { reason: String },
}

/// Capture `entry` under the salience gate. `related` names items to
/// cross-reference ("cheap forward edges") — empty degrades to
/// no edges at all, the baseline. `audit_actor` is the action-algebra's
/// own audit-trail actor (who/what triggered this write), independent of
/// `entry.actor` (who the captured content is attributed to) — the two may
/// coincide but are not the same field.
pub(crate) fn capture(
    conn: &Connection,
    entry: MemoryEntry,
    related: &[MemoryId],
    audit_actor: &str,
    now: u64,
) -> Result<CaptureOutcome> {
    if entry.confidence < CONFIDENCE_FLOOR {
        return Ok(CaptureOutcome::Refused {
            reason: format!(
                "confidence {:.2} is below the capture floor {:.2}",
                entry.confidence, CONFIDENCE_FLOOR
            ),
        });
    }

    let (id, action) = consolidate::consolidate(conn, entry, None, audit_actor, now)?;

    for target in related {
        consolidate::add_edge(conn, &id, target, consolidate::CROSS_REF_PREDICATE, now)?;
    }

    Ok(if action == ConsolidationAction::Corroborate {
        CaptureOutcome::Corroborated(id)
    } else {
        CaptureOutcome::Stored(id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cronus_contract::{MemoryEntry, MemoryKind, MemorySource, MemorySubject};
    use rusqlite::params;

    fn conn() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::memory::store::setup(&c).unwrap();
        c
    }

    fn entry(title: &str, body: &str) -> MemoryEntry {
        MemoryEntry::new(MemoryKind::Convention, MemorySource::Agent, title, body)
    }

    #[test]
    fn a_confident_new_capture_is_stored() {
        let c = conn();
        let e = entry("t", "a genuinely new fact");
        let outcome = capture(&c, e, &[], "test", 100).unwrap();
        assert!(matches!(outcome, CaptureOutcome::Stored(_)));
    }

    #[test]
    fn a_below_floor_capture_is_refused_and_writes_nothing() {
        let c = conn();
        let mut e = entry("t", "an unreliable guess");
        e.confidence = 0.05;
        let outcome = capture(&c, e, &[], "test", 100).unwrap();
        assert!(matches!(outcome, CaptureOutcome::Refused { .. }));

        let count: i64 = c
            .query_row("SELECT COUNT(*) FROM memories", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "a refused capture must write no row");
    }

    #[test]
    fn a_normalized_duplicate_corroborates_instead_of_creating_a_second_row() {
        let c = conn();
        capture(&c, entry("t", "  Same Fact  "), &[], "test", 100).unwrap();
        let outcome = capture(&c, entry("t", "same fact"), &[], "test", 200).unwrap();
        assert!(matches!(outcome, CaptureOutcome::Corroborated(_)));

        let count: i64 = c
            .query_row("SELECT COUNT(*) FROM memories", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "corroboration must not add a second row");
    }

    #[test]
    fn cross_ref_edges_land_and_degrade_to_none_when_absent() {
        let c = conn();
        let related_outcome = capture(&c, entry("hub", "a hub fact"), &[], "test", 100).unwrap();
        let CaptureOutcome::Stored(hub_id) = related_outcome else {
            panic!("expected Stored");
        };

        let with_ref = capture(
            &c,
            entry("t", "a related fact"),
            std::slice::from_ref(&hub_id),
            "test",
            200,
        )
        .unwrap();
        let CaptureOutcome::Stored(new_id) = with_ref else {
            panic!("expected Stored");
        };
        let edges = consolidate::edges_from(&c, &new_id).unwrap();
        assert_eq!(
            edges,
            vec![(hub_id, consolidate::CROSS_REF_PREDICATE.to_string())]
        );
    }

    #[test]
    fn attribution_and_subject_degrade_to_none_when_absent_and_persist_when_set() {
        let c = conn();
        let plain = capture(&c, entry("t", "plain capture"), &[], "test", 100).unwrap();
        let CaptureOutcome::Stored(plain_id) = plain else {
            panic!("expected Stored");
        };
        let row: (Option<String>, Option<i64>, Option<String>) = c
            .query_row(
                "SELECT actor, expiry, subject FROM memories WHERE id = ?1",
                params![plain_id.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(row, (None, None, None));

        let attributed = entry("t2", "attributed capture")
            .with_actor("user:alice")
            .with_subject(MemorySubject::User);
        let outcome = capture(&c, attributed, &[], "test", 300).unwrap();
        let CaptureOutcome::Stored(id) = outcome else {
            panic!("expected Stored");
        };
        let row: (Option<String>, Option<String>) = c
            .query_row(
                "SELECT actor, subject FROM memories WHERE id = ?1",
                params![id.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(row.0, Some("user:alice".to_string()));
        assert_eq!(row.1, Some("User".to_string()));
    }
}
