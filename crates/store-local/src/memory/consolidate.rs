//! Consolidation write path: the
//! synchronous authoring stage that turns a candidate unit of memory into
//! consolidated content, plus the periodic passes that operate on the
//! resulting edge graph.
//!
//! Deliberately a **new, separate table** from `memory_chains`/`ChainKind`:
//! the chain machinery serves session-continuation and explicit user chains
//! with a closed 3-variant vocabulary, walked by Bellman trust propagation.
//! The relationship graph wants an **open** typed-predicate vocabulary over consolidated
//! content specifically — a different concept with a different table, not a
//! retrofit onto code that already has established, tested semantics.

use rusqlite::Connection;
use rusqlite::params;

use super::Result;
use super::signal::{self, SignalKind};
use super::trust::apply_delta;
use cronus_contract::{LifecycleState, MemoryDepth, MemoryEntry, MemoryId};

// ── schema ────────────────────────────────────────────────────────────────────

pub(crate) fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS memory_edge (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            source_id  TEXT    NOT NULL,
            target_id  TEXT    NOT NULL,
            predicate  TEXT    NOT NULL,
            created_at INTEGER NOT NULL,
            UNIQUE (source_id, target_id, predicate)
        )",
    )?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS consolidation_checkpoint (
            id         INTEGER PRIMARY KEY CHECK (id = 1),
            watermark  INTEGER NOT NULL DEFAULT 0
        )",
    )?;
    Ok(())
}

// ── Write-time additive-only relationship binding ────────────────────

/// The mandatory edge every consolidated item carries back to the
/// working/raw material that grounds it.
pub const PROVENANCE_PREDICATE: &str = "derived-from";

/// A cluster-membership edge from an summary node to a member it rests
/// on — grounding for the summary, in the same open vocabulary as any other
/// relationship edge.
pub const SUMMARIZES_PREDICATE: &str = "summarizes";

/// A correction's forward pointer to the item it superseded (`correct`).
pub const SUPERSEDES_PREDICATE: &str = "supersedes";

/// A capture-time forward pointer to a related item the caller named
/// ("cheap forward edges") — distinct from `derived-from`
/// (provenance) and `supersedes` (correction): a cross-reference asserts no
/// lineage or replacement, only relatedness.
pub const CROSS_REF_PREDICATE: &str = "cross-ref";

/// Add a relationship edge. **Additive-only by construction**: this is an
/// insert-or-ignore — there is no `delete_edge`/`update_edge` in this module,
/// so the only way the edge set changes is by growing. A duplicate
/// `(source, target, predicate)` is a no-op, not an accreting duplicate.
pub(crate) fn add_edge(
    conn: &Connection,
    source: &MemoryId,
    target: &MemoryId,
    predicate: &str,
    now: u64,
) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO memory_edge (source_id, target_id, predicate, created_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![source.as_str(), target.as_str(), predicate, now as i64],
    )?;
    Ok(())
}

/// Every edge whose source is `id` — `(target, predicate)` pairs.
pub(crate) fn edges_from(conn: &Connection, id: &MemoryId) -> Result<Vec<(MemoryId, String)>> {
    let mut stmt =
        conn.prepare("SELECT target_id, predicate FROM memory_edge WHERE source_id = ?1")?;
    let rows = stmt
        .query_map(params![id.as_str()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .map(|r| r.map(|(t, p)| (MemoryId::from(t), p)))
        .collect::<std::result::Result<_, _>>()?;
    Ok(rows)
}

/// In-degree (count of distinct incoming edges) for every node that has at
/// least one — the graph in-degree the `centrality` factor is defined
/// over. Nodes with zero in-edges are absent (degrade to the signal store's
/// own neutral default).
fn in_degrees(conn: &Connection) -> Result<std::collections::HashMap<String, usize>> {
    let mut stmt =
        conn.prepare("SELECT target_id, COUNT(*) FROM memory_edge GROUP BY target_id")?;
    let rows: Vec<(String, i64)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    Ok(rows.into_iter().map(|(id, n)| (id, n as usize)).collect())
}

/// Recompute the `Centrality` derived signal for every node with at least
/// one incoming edge (step 1 of the maintenance pass, the half `recompute_recency`
/// left for this task). Normalized by the largest in-degree seen so the
/// factor stays in the same (0, 1] band as the other ranking factors.
pub(crate) fn recompute_centrality(conn: &Connection, now: u64) -> Result<usize> {
    let degrees = in_degrees(conn)?;
    let max_degree = degrees.values().copied().max().unwrap_or(0);
    if max_degree == 0 {
        return Ok(0);
    }
    let mut updated = 0;
    for (id_str, degree) in degrees {
        let factor = degree as f64 / max_degree as f64;
        signal::write(
            conn,
            &MemoryId::from(id_str),
            SignalKind::Centrality,
            factor,
            now,
        )?;
        updated += 1;
    }
    Ok(updated)
}

// ── Consolidation action algebra ──────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsolidationAction {
    Create,
    Corroborate,
    Refine,
    Correct,
}

impl ConsolidationAction {
    fn as_str(self) -> &'static str {
        match self {
            ConsolidationAction::Create => "create",
            ConsolidationAction::Corroborate => "corroborate",
            ConsolidationAction::Refine => "refine",
            ConsolidationAction::Correct => "correct",
        }
    }
}

fn normalize(s: &str) -> String {
    s.trim().to_lowercase()
}

/// Find an existing **active, consolidated** item whose body matches
/// `candidate_body` after normalization — the same-abstraction detection
/// this domain-logic-first pass uses in place of semantic recall-for-linking
/// (a model-dependent operation with no safe non-model substitute; the same
/// normalized-body heuristic the maintenance pass's merge-candidate
/// detection also uses).
fn find_same_abstraction(conn: &Connection, candidate_body: &str) -> Result<Option<MemoryId>> {
    let mut stmt = conn.prepare(
        "SELECT id, body FROM memories
         WHERE lifecycle_state = ?1 AND depth = ?2",
    )?;
    let rows: Vec<(String, String)> = stmt
        .query_map(
            params![
                LifecycleState::Active.as_str(),
                MemoryDepth::Consolidated.as_str()
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?
        .collect::<std::result::Result<_, _>>()?;

    let target = normalize(candidate_body);
    Ok(rows
        .into_iter()
        .find(|(_, body)| normalize(body) == target)
        .map(|(id, _)| MemoryId::from(id)))
}

/// The routine consolidation write: a new unit meets the existing
/// corpus and resolves to exactly one action.
///
/// - No same-abstraction match → **create**: write `candidate` as a new
///   consolidated item, with a provenance edge to `provenance` if given.
/// - A match → **corroborate**: the new source attaches (a provenance edge
///   to `provenance`) and strengthens the existing item (a positive trust
///   bump, reusing the store's existing trust-update path) — no new row.
///
/// `refine`/`correct` are **not** auto-detected here (they require a
/// judgment — "does this extend scope" or "does this fix an error" — that a
/// normalization match cannot make safely); they are explicit operations
/// ([`refine`], [`correct`]) the caller invokes when it has that judgment
/// (from a generator, or from its own certain knowledge).
pub(crate) fn consolidate(
    conn: &Connection,
    candidate: MemoryEntry,
    provenance: Option<&MemoryId>,
    actor: &str,
    now: u64,
) -> Result<(MemoryId, ConsolidationAction)> {
    if let Some(existing) = find_same_abstraction(conn, &candidate.body)? {
        if let Some(src) = provenance {
            add_edge(conn, &existing, src, PROVENANCE_PREDICATE, now)?;
        }
        let current: f64 = conn.query_row(
            "SELECT trust_score FROM memories WHERE id = ?1",
            params![existing.as_str()],
            |row| row.get(0),
        )?;
        let bumped = apply_delta(current, true);
        conn.execute(
            "UPDATE memories SET trust_score = ?1 WHERE id = ?2",
            params![bumped, existing.as_str()],
        )?;
        record_action(
            conn,
            &existing,
            ConsolidationAction::Corroborate,
            provenance,
            actor,
            now,
        )?;
        return Ok((existing, ConsolidationAction::Corroborate));
    }

    let mut item = candidate;
    // A newly-consolidated node is a distinct identity from whatever `id`
    // the caller's `candidate` happened to carry — critically, from the raw
    // source's own id when driven by `run_incremental_pass`, which reads
    // `candidate` straight off the raw row it is about to cite as
    // provenance. Reusing that id would collide on `memories`' PRIMARY KEY.
    item.id = MemoryId::new();
    item.depth = MemoryDepth::Consolidated;
    let id = super::store::insert(conn, item)?;
    if let Some(src) = provenance {
        add_edge(conn, &id, src, PROVENANCE_PREDICATE, now)?;
    }
    record_action(
        conn,
        &id,
        ConsolidationAction::Create,
        provenance,
        actor,
        now,
    )?;
    Ok((id, ConsolidationAction::Create))
}

/// Explicit refine: the caller (a generator, or its own certain knowledge)
/// asserts that `addition` extends `target`'s scope, steps, or boundary
/// conditions. Additive-only: appends, never truncates or replaces
/// the existing body. Adds a provenance edge if `provenance` is given.
///
/// `expected_body` is what the caller read earlier and is refining against
/// — a real optimistic-concurrency token, not a same-call read that
/// could never observe a concurrent change: if the stored body no longer
/// matches (someone else's write landed in between), this refuses rather
/// than clobbering it, and the caller re-reads and retries.
pub(crate) fn refine(
    conn: &Connection,
    target: &MemoryId,
    expected_body: &str,
    addition: &str,
    provenance: Option<&MemoryId>,
    actor: &str,
    now: u64,
) -> Result<()> {
    let new_body = format!("{expected_body}\n{addition}");
    let affected = conn.execute(
        "UPDATE memories SET body = ?1 WHERE id = ?2 AND body = ?3",
        params![new_body, target.as_str(), expected_body],
    )?;
    if affected == 0 {
        return Err(super::MemoryError::Database(
            rusqlite::Error::QueryReturnedNoRows,
        ));
    }
    if let Some(src) = provenance {
        add_edge(conn, target, src, PROVENANCE_PREDICATE, now)?;
    }
    record_action(
        conn,
        target,
        ConsolidationAction::Refine,
        provenance,
        actor,
        now,
    )?;
    Ok(())
}

/// Explicit correct: the caller asserts `target` contains an error;
/// `corrected` replaces it non-destructively — `target` is
/// **superseded** (`superseded_at` set, never deleted, never rewritten) and
/// `corrected` is inserted as a new item carrying a `supersedes` edge back
/// to it. Transactional: supersede + insert + edge commit together.
pub(crate) fn correct(
    conn: &Connection,
    target: &MemoryId,
    mut corrected: MemoryEntry,
    actor: &str,
    now: u64,
) -> Result<MemoryId> {
    conn.execute_batch("BEGIN")?;
    let result = (|| -> Result<MemoryId> {
        let affected = conn.execute(
            "UPDATE memories SET superseded_at = ?1 WHERE id = ?2 AND superseded_at IS NULL",
            params![now as i64, target.as_str()],
        )?;
        if affected == 0 {
            // Already superseded (or missing) — refuse rather than double-supersede.
            return Err(super::MemoryError::Database(
                rusqlite::Error::QueryReturnedNoRows,
            ));
        }
        corrected.depth = MemoryDepth::Consolidated;
        let new_id = super::store::insert(conn, corrected)?;
        add_edge(conn, &new_id, target, SUPERSEDES_PREDICATE, now)?;
        record_action(
            conn,
            &new_id,
            ConsolidationAction::Correct,
            Some(target),
            actor,
            now,
        )?;
        Ok(new_id)
    })();

    match result {
        Ok(id) => {
            conn.execute_batch("COMMIT")?;
            Ok(id)
        }
        Err(e) => {
            conn.execute_batch("ROLLBACK")?;
            Err(e)
        }
    }
}

fn record_action(
    conn: &Connection,
    item_id: &MemoryId,
    action: ConsolidationAction,
    target: Option<&MemoryId>,
    actor: &str,
    now: u64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO maintenance_audit (item_id, action, target_id, actor, instant)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            item_id.as_str(),
            action.as_str(),
            target.map(MemoryId::as_str),
            actor,
            now as i64,
        ],
    )?;
    Ok(())
}

// ── Incremental, failed-not-checkpointed consolidation pass ──────────

/// Run `consolidate` over every `raw`/`working` item created after the
/// checkpoint watermark. The watermark advances only over inputs that
/// commit successfully — a failed input is **not** checkpointed, so it is
/// retried on the next pass. No changed input is a successful no-op.
pub(crate) fn run_incremental_pass(
    conn: &Connection,
    actor: &str,
    now: u64,
) -> Result<Vec<(MemoryId, ConsolidationAction)>> {
    let watermark: i64 = conn
        .query_row(
            "SELECT watermark FROM consolidation_checkpoint WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let mut stmt = conn.prepare(
        "SELECT id, kind, source, title, body, confidence, trust_score,
                valid_at, created_at, superseded_at, workspace_id, verification_state,
                depth, lifecycle_state, experience_outcome, actor, expiry, subject
         FROM memories
         WHERE depth != ?1 AND created_at > ?2
         ORDER BY created_at ASC",
    )?;
    let candidates: Vec<MemoryEntry> = stmt
        .query_map(
            params![MemoryDepth::Consolidated.as_str(), watermark],
            super::store::map_row,
        )?
        .collect::<std::result::Result<_, _>>()?;

    let mut results = Vec::new();
    let mut max_processed = watermark;
    for candidate in candidates {
        let created_at = candidate.created_at as i64;
        let provenance_id = candidate.id.clone();
        // The raw/working row stays exactly as-is (never rewritten) —
        // consolidate() writes a *new* consolidated item citing it as
        // provenance; nothing here mutates `candidate`'s own row.
        match consolidate(conn, candidate, Some(&provenance_id), actor, now) {
            Ok(outcome) => {
                results.push(outcome);
                max_processed = max_processed.max(created_at);
            }
            Err(_) => {
                // Not checkpointed — retried next pass. Continue with the
                // rest of the batch rather than aborting it wholesale.
                continue;
            }
        }
    }

    conn.execute(
        "INSERT INTO consolidation_checkpoint (id, watermark) VALUES (1, ?1)
         ON CONFLICT(id) DO UPDATE SET watermark = excluded.watermark",
        params![max_processed],
    )?;
    Ok(results)
}

// ── Emergent topic-cluster abstraction ────────────────────────────────

/// A cluster's minimum size to be summary-eligible — below this, a cluster
/// is too small to be worth an overview node.
pub const MC7_MIN_CLUSTER_SIZE: usize = 3;
/// A member's body is truncated to this many characters when folded into a
/// summary — keeps the summary size-bounded (must not re-trigger split).
const MC7_MEMBER_EXCERPT_CHARS: usize = 200;

/// Union-find over the `memory_edge` graph — the algorithmic *shape* named
/// in this spec's own Implementation Notes ("reuse the clustering primitive
/// shape from the code graph: union-find stub"), reimplemented locally
/// against `MemoryId` nodes rather than depending on the `codegraph` crate:
/// that crate's types are `i64` code-symbol ids, an unrelated domain, and
/// pulling in a code-intelligence crate from a memory-persistence one for a
/// ~15-line algorithm is not a justified dependency (project policy: add a
/// dependency only when strictly necessary).
fn connected_components(conn: &Connection) -> Result<Vec<Vec<MemoryId>>> {
    let mut stmt = conn.prepare("SELECT source_id, target_id FROM memory_edge")?;
    let edges: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;

    let mut parent: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for (a, b) in &edges {
        parent.entry(a.clone()).or_insert_with(|| a.clone());
        parent.entry(b.clone()).or_insert_with(|| b.clone());
    }

    fn find(parent: &mut std::collections::HashMap<String, String>, x: &str) -> String {
        let p = parent.get(x).cloned().unwrap_or_else(|| x.to_string());
        if p == x {
            x.to_string()
        } else {
            let root = find(parent, &p);
            parent.insert(x.to_string(), root.clone());
            root
        }
    }

    for (a, b) in &edges {
        let ra = find(&mut parent, a);
        let rb = find(&mut parent, b);
        if ra != rb {
            parent.insert(ra, rb);
        }
    }

    let mut clusters: std::collections::HashMap<String, Vec<MemoryId>> =
        std::collections::HashMap::new();
    let nodes: Vec<String> = parent.keys().cloned().collect();
    for node in nodes {
        let root = find(&mut parent, &node);
        clusters.entry(root).or_default().push(MemoryId::from(node));
    }
    Ok(clusters.into_values().collect())
}

/// Periodic pass: cluster the edge graph, and for every cluster at or above
/// [`MC7_MIN_CLUSTER_SIZE`] with no existing summary (checked structurally —
/// no node in the cluster already has a `summarizes` edge pointing into it
/// from outside the cluster), synthesize a grounded, size-bounded summary
/// node. Returns the new summary ids.
pub(crate) fn synthesize_summaries(
    conn: &Connection,
    actor: &str,
    now: u64,
) -> Result<Vec<MemoryId>> {
    let clusters = connected_components(conn)?;
    let mut created = Vec::new();

    for members in clusters {
        if members.len() < MC7_MIN_CLUSTER_SIZE {
            continue;
        }
        // Structural hub check: skip if any member already has an inbound
        // `summarizes` edge (an existing summary already covers this cluster).
        let already_summarized = members.iter().any(|m| {
            conn.query_row(
                "SELECT 1 FROM memory_edge WHERE target_id = ?1 AND predicate = ?2 LIMIT 1",
                params![m.as_str(), SUMMARIZES_PREDICATE],
                |_| Ok(()),
            )
            .is_ok()
        });
        if already_summarized {
            continue;
        }

        let mut excerpt = String::new();
        for member in &members {
            if let Ok(Some(body)) = conn
                .query_row(
                    "SELECT body FROM memories WHERE id = ?1 AND lifecycle_state = ?2",
                    params![member.as_str(), LifecycleState::Active.as_str()],
                    |row| row.get::<_, String>(0),
                )
                .map(Some)
            {
                let truncated: String = body.chars().take(MC7_MEMBER_EXCERPT_CHARS).collect();
                excerpt.push_str(&truncated);
                excerpt.push('\n');
            }
        }
        if excerpt.is_empty() {
            continue; // every member inactive/missing — nothing to ground a summary in
        }

        let summary = MemoryEntry::new(
            cronus_contract::MemoryKind::ProjectContext,
            cronus_contract::MemorySource::System,
            format!("Topic summary ({} related items)", members.len()),
            excerpt,
        )
        .with_depth(MemoryDepth::Consolidated);
        let summary_id = super::store::insert(conn, summary)?;
        for member in &members {
            add_edge(conn, &summary_id, member, SUMMARIZES_PREDICATE, now)?;
        }
        record_action(
            conn,
            &summary_id,
            ConsolidationAction::Create,
            None,
            actor,
            now,
        )?;
        created.push(summary_id);
    }
    Ok(created)
}

// ── Advisory interest extraction (read-only, generator-free) ────────

/// One advisory interest topic — memory decides *what*; the caller (e.g. an
/// inner-monologue-style background reviewer) decides *whether/when/how* to
/// surface it.
#[derive(Debug, Clone)]
pub struct InterestTopic {
    pub title: String,
    pub rationale: String,
    pub item_id: MemoryId,
}

/// Emit at most `limit` interest topics from the most recently created
/// active items, deduplicated by normalized title against the window
/// itself (never the same title twice in one call) — bounded, read-only,
/// generator-free.
pub(crate) fn extract_interest_topics(
    conn: &Connection,
    limit: usize,
) -> Result<Vec<InterestTopic>> {
    let mut stmt = conn.prepare(
        "SELECT id, title FROM memories
         WHERE lifecycle_state = ?1
         ORDER BY created_at DESC",
    )?;
    let rows: Vec<(String, String)> = stmt
        .query_map(params![LifecycleState::Active.as_str()], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .collect::<std::result::Result<_, _>>()?;

    let mut seen = std::collections::HashSet::new();
    let mut topics = Vec::new();
    for (id, title) in rows {
        if topics.len() >= limit {
            break;
        }
        let key = normalize(&title);
        if !seen.insert(key) {
            continue; // dedup within this window
        }
        topics.push(InterestTopic {
            title: title.clone(),
            rationale: "recently added to the corpus".to_string(),
            item_id: MemoryId::from(id),
        });
    }
    Ok(topics)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cronus_contract::{MemoryKind, MemorySource};

    fn conn() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::memory::store::setup(&c).unwrap();
        c
    }

    fn seed(conn: &Connection, id: &str, body: &str) -> MemoryId {
        let mut e = MemoryEntry::new(MemoryKind::Convention, MemorySource::Agent, "t", body);
        e.id = MemoryId::from(id.to_string());
        super::super::store::insert(conn, e).unwrap()
    }

    #[test]
    fn add_edge_is_additive_only_duplicate_insert_is_a_noop() {
        let c = conn();
        let a = MemoryId::from("a".to_string());
        let b = MemoryId::from("b".to_string());
        add_edge(&c, &a, &b, "relates-to", 100).unwrap();
        add_edge(&c, &a, &b, "relates-to", 200).unwrap(); // duplicate, ignored

        let count: i64 = c
            .query_row("SELECT COUNT(*) FROM memory_edge", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            count, 1,
            "a duplicate (source, target, predicate) must not accrete"
        );
    }

    #[test]
    fn edges_from_returns_target_and_predicate_pairs() {
        let c = conn();
        let a = MemoryId::from("a".to_string());
        let b = MemoryId::from("b".to_string());
        let d = MemoryId::from("d".to_string());
        add_edge(&c, &a, &b, "relates-to", 100).unwrap();
        add_edge(&c, &a, &d, "derived-from", 100).unwrap();

        let mut edges = edges_from(&c, &a).unwrap();
        edges.sort_by(|x, y| x.1.cmp(&y.1));
        assert_eq!(
            edges,
            vec![
                (d, "derived-from".to_string()),
                (b, "relates-to".to_string()),
            ]
        );
    }

    #[test]
    fn recompute_centrality_normalizes_by_max_in_degree() {
        let c = conn();
        let hub = MemoryId::from("hub".to_string());
        let spoke1 = MemoryId::from("s1".to_string());
        let spoke2 = MemoryId::from("s2".to_string());
        let lonely = MemoryId::from("lonely".to_string());
        add_edge(&c, &spoke1, &hub, "relates-to", 100).unwrap();
        add_edge(&c, &spoke2, &hub, "relates-to", 100).unwrap();

        recompute_centrality(&c, 100).unwrap();

        let hub_factor = signal::factor(&c, &hub, SignalKind::Centrality).unwrap();
        assert!(
            (hub_factor - 1.0).abs() < 1e-9,
            "the max in-degree node normalizes to 1.0"
        );
        // `lonely` has zero in-edges — no row written, degrades to the
        // signal store's own neutral default.
        let lonely_factor = signal::factor(&c, &lonely, SignalKind::Centrality).unwrap();
        assert_eq!(lonely_factor, signal::NEUTRAL_FACTOR);
    }

    #[test]
    fn consolidate_with_no_match_creates_a_new_consolidated_item() {
        let c = conn();
        let raw = seed(&c, "raw-1", "the user prefers dark mode");
        let candidate = MemoryEntry::new(
            MemoryKind::Convention,
            MemorySource::Agent,
            "preference",
            "the user prefers dark mode, extracted",
        );

        let (id, action) = consolidate(&c, candidate, Some(&raw), "test", 100).unwrap();
        assert_eq!(action, ConsolidationAction::Create);
        let stored = c
            .query_row(
                "SELECT depth FROM memories WHERE id = ?1",
                params![id.as_str()],
                |r| r.get::<_, String>(0),
            )
            .unwrap();
        assert_eq!(stored, "Consolidated");

        let edges = edges_from(&c, &id).unwrap();
        assert_eq!(edges, vec![(raw, PROVENANCE_PREDICATE.to_string())]);
    }

    #[test]
    fn consolidate_with_a_normalized_match_corroborates_instead_of_creating() {
        let c = conn();
        let existing = seed(&c, "existing", "  Dark Mode Preference  ");
        // Promote it to Consolidated + Active so find_same_abstraction sees it.
        c.execute(
            "UPDATE memories SET depth = 'Consolidated' WHERE id = 'existing'",
            [],
        )
        .unwrap();
        let before_trust: f64 = c
            .query_row(
                "SELECT trust_score FROM memories WHERE id = 'existing'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        let raw2 = seed(&c, "raw-2", "dark mode preference"); // same after normalize
        let candidate = MemoryEntry::new(
            MemoryKind::Convention,
            MemorySource::Agent,
            "t",
            "dark mode preference",
        );

        let (id, action) = consolidate(&c, candidate, Some(&raw2), "test", 200).unwrap();
        assert_eq!(action, ConsolidationAction::Corroborate);
        assert_eq!(
            id, existing,
            "corroborate must return the EXISTING id, not a new one"
        );

        let after_trust: f64 = c
            .query_row(
                "SELECT trust_score FROM memories WHERE id = 'existing'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            after_trust > before_trust,
            "corroboration must bump trust upward"
        );

        let count: i64 = c
            .query_row("SELECT COUNT(*) FROM memories", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            count, 2,
            "no new memories row — only `existing` and `raw-2`"
        );
    }

    #[test]
    fn refine_appends_additively_and_adds_provenance() {
        let c = conn();
        let target = seed(&c, "target", "original scope");
        let src = seed(&c, "src", "new boundary condition");

        refine(
            &c,
            &target,
            "original scope",
            "plus a new boundary condition",
            Some(&src),
            "test",
            100,
        )
        .unwrap();

        let body: String = c
            .query_row("SELECT body FROM memories WHERE id = 'target'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(
            body.contains("original scope"),
            "refine must never drop existing content"
        );
        assert!(body.contains("plus a new boundary condition"));

        assert_eq!(
            edges_from(&c, &target).unwrap(),
            vec![(src, PROVENANCE_PREDICATE.to_string())]
        );
    }

    #[test]
    fn refine_refuses_when_the_body_changed_underneath_the_caller() {
        let c = conn();
        let target = seed(&c, "target", "original scope");
        // Simulate a concurrent writer landing a change after the caller's read.
        c.execute(
            "UPDATE memories SET body = 'someone else edited this' WHERE id = 'target'",
            [],
        )
        .unwrap();

        let result = refine(
            &c,
            &target,
            "original scope",
            "my addition",
            None,
            "test",
            100,
        );
        assert!(
            result.is_err(),
            "a stale expected_body must be refused, never silently clobbered"
        );

        let body: String = c
            .query_row("SELECT body FROM memories WHERE id = 'target'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(
            body, "someone else edited this",
            "the concurrent write must survive untouched"
        );
    }

    #[test]
    fn correct_supersedes_the_target_and_links_the_new_item_back_to_it() {
        let c = conn();
        let target = seed(&c, "target", "the API returns null on error");
        let corrected = MemoryEntry::new(
            MemoryKind::KnownIssue,
            MemorySource::Agent,
            "correction",
            "the API throws on error, not null",
        );

        let new_id = correct(&c, &target, corrected, "test", 100).unwrap();

        let superseded_at: Option<i64> = c
            .query_row(
                "SELECT superseded_at FROM memories WHERE id = 'target'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            superseded_at,
            Some(100),
            "target must be superseded, never deleted or rewritten"
        );

        let still_there: String = c
            .query_row("SELECT body FROM memories WHERE id = 'target'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(
            still_there, "the API returns null on error",
            "the old body is untouched"
        );

        assert_eq!(
            edges_from(&c, &new_id).unwrap(),
            vec![(target, SUPERSEDES_PREDICATE.to_string())]
        );
    }

    #[test]
    fn correct_refuses_to_double_supersede() {
        let c = conn();
        let target = seed(&c, "target", "body");
        let first = MemoryEntry::new(MemoryKind::KnownIssue, MemorySource::Agent, "t", "fix 1");
        correct(&c, &target, first, "test", 100).unwrap();

        let second = MemoryEntry::new(MemoryKind::KnownIssue, MemorySource::Agent, "t", "fix 2");
        let result = correct(&c, &target, second, "test", 200);
        assert!(
            result.is_err(),
            "correcting an already-superseded target must refuse"
        );
    }

    #[test]
    fn run_incremental_pass_consolidates_raw_items_and_advances_the_watermark() {
        let c = conn();
        let mut raw = MemoryEntry::new(MemoryKind::Convention, MemorySource::Agent, "t", "a fact");
        raw.depth = MemoryDepth::Raw;
        raw.created_at = 500;
        super::super::store::insert(&c, raw).unwrap();

        let results = run_incremental_pass(&c, "test", 1000).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, ConsolidationAction::Create);

        // A second pass with no new input is a successful no-op.
        let second = run_incremental_pass(&c, "test", 1000).unwrap();
        assert!(second.is_empty());
    }

    #[test]
    fn run_incremental_pass_never_rewrites_the_raw_source_row() {
        let c = conn();
        let mut raw = MemoryEntry::new(
            MemoryKind::Convention,
            MemorySource::Agent,
            "t",
            "verbatim evidence",
        );
        raw.id = MemoryId::from("raw-source".to_string());
        raw.depth = MemoryDepth::Raw;
        raw.created_at = 500;
        super::super::store::insert(&c, raw).unwrap();

        run_incremental_pass(&c, "test", 1000).unwrap();

        let (depth, body): (String, String) = c
            .query_row(
                "SELECT depth, body FROM memories WHERE id = 'raw-source'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(depth, "Raw", "the raw row's depth must never change");
        assert_eq!(
            body, "verbatim evidence",
            "the raw row's content must never be rewritten"
        );
    }

    #[test]
    fn synthesize_summaries_grounds_a_summary_in_its_members_when_cluster_is_large_enough() {
        let c = conn();
        let a = seed(&c, "a", "topic content a");
        let b = seed(&c, "b", "topic content b");
        let d = seed(&c, "d", "topic content d");
        add_edge(&c, &a, &b, "relates-to", 100).unwrap();
        add_edge(&c, &b, &d, "relates-to", 100).unwrap();

        let summaries = synthesize_summaries(&c, "test", 200).unwrap();
        assert_eq!(summaries.len(), 1);

        let edges = edges_from(&c, &summaries[0]).unwrap();
        let members: std::collections::HashSet<_> = edges.into_iter().map(|(id, _)| id).collect();
        assert_eq!(members, [a, b, d].into_iter().collect());
    }

    #[test]
    fn synthesize_summaries_skips_clusters_below_the_minimum_size() {
        let c = conn();
        let a = seed(&c, "a", "content a");
        let b = seed(&c, "b", "content b");
        add_edge(&c, &a, &b, "relates-to", 100).unwrap(); // only 2 members

        let summaries = synthesize_summaries(&c, "test", 200).unwrap();
        assert!(summaries.is_empty());
    }

    #[test]
    fn synthesize_summaries_does_not_re_summarize_an_already_summarized_cluster() {
        let c = conn();
        let a = seed(&c, "a", "content a");
        let b = seed(&c, "b", "content b");
        let d = seed(&c, "d", "content d");
        add_edge(&c, &a, &b, "relates-to", 100).unwrap();
        add_edge(&c, &b, &d, "relates-to", 100).unwrap();

        let first = synthesize_summaries(&c, "test", 200).unwrap();
        assert_eq!(first.len(), 1);
        let second = synthesize_summaries(&c, "test", 300).unwrap();
        assert!(
            second.is_empty(),
            "a structurally-hubbed cluster must not gain a second summary"
        );
    }

    #[test]
    fn extract_interest_topics_is_bounded_and_deduplicated() {
        let c = conn();
        let mut e1 = MemoryEntry::new(
            MemoryKind::Convention,
            MemorySource::Agent,
            "topic one",
            "body 1",
        );
        e1.id = MemoryId::from("1".to_string());
        let mut e2 = MemoryEntry::new(
            MemoryKind::Convention,
            MemorySource::Agent,
            "topic two",
            "body 2",
        );
        e2.id = MemoryId::from("2".to_string());
        let mut e3 = MemoryEntry::new(
            MemoryKind::Convention,
            MemorySource::Agent,
            "topic three",
            "body 3",
        );
        e3.id = MemoryId::from("3".to_string());
        // A duplicate normalized title — the window-dedup must collapse it.
        let mut dup = MemoryEntry::new(
            MemoryKind::Convention,
            MemorySource::Agent,
            "Topic One",
            "dup body",
        );
        dup.id = MemoryId::from("4".to_string());
        for e in [e1, e2, e3, dup] {
            super::super::store::insert(&c, e).unwrap();
        }

        let topics = extract_interest_topics(&c, 2).unwrap();
        assert_eq!(topics.len(), 2, "bounded by `limit`");

        let all = extract_interest_topics(&c, 100).unwrap();
        assert_eq!(
            all.len(),
            3,
            "the duplicate-titled item must be deduplicated out of the window"
        );
        let titles: std::collections::HashSet<_> =
            all.iter().map(|t| t.title.to_lowercase()).collect();
        assert_eq!(
            titles.len(),
            all.len(),
            "no duplicate normalized title within the window"
        );
    }
}
