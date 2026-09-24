//! SQLite-backed memory store with FTS5 and session chaining.

use rusqlite::{Connection, OptionalExtension, params};

use super::{
    CodeChangeType, MemoryEntry, MemoryError, MemoryId, MemoryKind, MemorySource, Result,
    SuggestedAction, TrustUpdate, VerificationState,
    capture::{self, CaptureOutcome},
    chain::{BELLMAN_MAX_DEPTH, ChainKind, SESSION_CHAIN_WINDOW_SECS, propagated_delta},
    consolidate::{self, ConsolidationAction, InterestTopic},
    maintenance, now_secs,
    signal::{self, SignalKind},
    trust::{TRUST_MIN_SEARCH, apply_delta},
};
use cronus_contract::{ExperienceOutcome, LifecycleState, MemoryDepth, MemorySubject};

pub struct MemoryStore {
    conn: Connection,
}

/// Turn a caller-supplied search string into a safe FTS5 `MATCH` expression.
///
/// Raw input went straight into `memories_fts MATCH ?` before, so any FTS5
/// query metacharacter broke it: a hyphen made `foo-123` parse as `foo` NOT
/// column `123` (`no such column: 123`), and `"`, `*`, `:`, `(`, `)`, `^`,
/// `AND`/`OR`/`NOT` all changed or errored the query. Each whitespace-separated
/// token is now wrapped as a quoted FTS5 string (embedded `"` doubled) and the
/// tokens are ANDed — `memory search` is a keyword search, so "every word
/// appears, punctuation is literal" is the right contract. An all-punctuation
/// or empty query yields `""`, which matches nothing rather than erroring.
fn fts_match_expr(raw: &str) -> String {
    let quoted: Vec<String> = raw
        .split_whitespace()
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect();
    if quoted.is_empty() {
        "\"\"".to_string()
    } else {
        quoted.join(" ")
    }
}

impl MemoryStore {
    pub fn open<P: AsRef<std::path::Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        setup(&conn)?;
        Ok(MemoryStore { conn })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        setup(&conn)?;
        Ok(MemoryStore { conn })
    }

    // ── write ─────────────────────────────────────────────────────────────────

    /// Insert a new memory entry. Auto-chains to the most recent memory in
    /// the same workspace if one exists within the session window.
    pub fn add(&self, entry: MemoryEntry) -> Result<MemoryId> {
        let workspace_id = entry.workspace_id.clone();
        let created_at = entry.created_at;
        let id = insert(&self.conn, entry)?;

        // Auto-chain within session window
        if let Some(ws) = &workspace_id
            && let Some(prev) = self.latest_in_workspace(ws, created_at)?
            && prev.as_str() != id.as_str()
        {
            self.chain_internal(&prev, &id, ChainKind::Continuation)?;
        }

        Ok(id)
    }

    /// Update the trust score for a memory. Returns the new trust score.
    pub fn update_trust(&self, id: &MemoryId, update: TrustUpdate) -> Result<f64> {
        let current: f64 = self.conn.query_row(
            "SELECT trust_score FROM memories WHERE id = ?1",
            params![id.as_str()],
            |row| row.get(0),
        )?;

        let new_score = apply_delta(current, update.positive);

        if let Some(vs) = &update.new_verification_state {
            self.conn.execute(
                "UPDATE memories SET trust_score = ?1, verification_state = ?2 WHERE id = ?3",
                params![new_score, vs.as_str(), id.as_str()],
            )?;
        } else {
            self.conn.execute(
                "UPDATE memories SET trust_score = ?1 WHERE id = ?2",
                params![new_score, id.as_str()],
            )?;
        }

        Ok(new_score)
    }

    /// Create an explicit chain link between two memories.
    pub fn chain(&self, source: &MemoryId, target: &MemoryId, kind: ChainKind) -> Result<()> {
        self.chain_internal(source, target, kind)
    }

    /// Propagate a trust delta through the chain graph (Bellman, depth ≤ 2).
    pub fn propagate_trust(&self, from: &MemoryId, base_delta: f64) -> Result<()> {
        self.propagate_recursive(from, base_delta, 0)
    }

    /// Lower trust of memories in a workspace based on a code change event.
    ///
    /// Returns the IDs of memories whose trust was adjusted.
    pub fn apply_code_change(
        &self,
        workspace_id: &str,
        change: CodeChangeType,
    ) -> Result<Vec<MemoryId>> {
        let action = change.suggested_action();
        let weight = match &action {
            SuggestedAction::Invalidate(w) => *w,
            SuggestedAction::Review(w) => *w,
            SuggestedAction::Update(w) => *w,
            SuggestedAction::None => return Ok(vec![]),
        };

        let ids = self.ids_in_workspace(workspace_id)?;
        for id in &ids {
            let current: f64 = self.conn.query_row(
                "SELECT trust_score FROM memories WHERE id = ?1",
                params![id.as_str()],
                |row| row.get(0),
            )?;
            let new_score = (current - weight * 0.1).clamp(0.0, 1.0);
            self.conn.execute(
                "UPDATE memories SET trust_score = ?1 WHERE id = ?2",
                params![new_score, id.as_str()],
            )?;
        }
        Ok(ids)
    }

    // ── read ──────────────────────────────────────────────────────────────────

    pub fn get(&self, id: &MemoryId) -> Result<Option<MemoryEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, source, title, body, confidence, trust_score,
                    valid_at, created_at, superseded_at, workspace_id, verification_state,
                    depth, lifecycle_state, experience_outcome, actor, expiry, subject
             FROM memories WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id.as_str()], map_row)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    /// Full-text search over title+body. Filters entries below TRUST_MIN_SEARCH.
    pub fn search_fts(&self, query: &str, limit: usize) -> Result<Vec<MemoryEntry>> {
        // Step 1: collect matching memory IDs from the FTS index.
        let match_expr = fts_match_expr(query);
        let mut fts_stmt = self
            .conn
            .prepare("SELECT memory_id FROM memories_fts WHERE memories_fts MATCH ?1 LIMIT ?2")?;
        let ids: Vec<String> = fts_stmt
            .query_map(params![match_expr, limit as i64], |row| row.get(0))?
            .collect::<std::result::Result<_, _>>()?;

        if ids.is_empty() {
            return Ok(vec![]);
        }

        // Step 2: fetch full entries for those IDs.
        // The SQL trust_score filter is the gate — effective_trust is not applied
        // here so that newly added entries (Untested verification state) still
        // surface in search results at their raw trust_score.
        // MI-9: recall defaults to the `active` lifecycle state only; a
        // caller wanting paused/archived items uses a dedicated lookup
        // (e.g. `get`), not the default search path.
        let mut out = Vec::with_capacity(ids.len());
        let mut mem_stmt = self.conn.prepare(
            "SELECT id, kind, source, title, body, confidence, trust_score,
                    valid_at, created_at, superseded_at, workspace_id, verification_state,
                    depth, lifecycle_state, experience_outcome, actor, expiry, subject
             FROM memories
             WHERE id = ?1 AND trust_score >= ?2 AND superseded_at IS NULL
               AND lifecycle_state = 'Active'
               AND (expiry IS NULL OR expiry > ?3)",
        )?;
        let now = now_secs() as i64;
        for id in &ids {
            let mut rows = mem_stmt.query_map(params![id, TRUST_MIN_SEARCH, now], map_row)?;
            if let Some(entry) = rows.next() {
                out.push(entry?);
            }
        }
        Ok(out)
    }

    /// Ranked recall for MC-8 consumers: fuses `base_text_relevance` (this
    /// method's own FTS5/BM25-derived score) with the derived signals from
    /// `memory_signal` (MC-5) **multiplicatively** — a near-zero derived
    /// factor vetoes rather than being averaged away. Every factor is either
    /// the FTS5 engine's own score or a precomputed table lookup: no model
    /// call, no graph walk (MC-8/MEM-2).
    ///
    /// Distinct from [`MemoryStore::search_fts`] (the unranked `MemorySearch`
    /// seam implementation, unchanged) — this is the richer surface MC-8
    /// consumers (the intelligence layer's `answer`/temporal recall) call.
    /// Multi-script lexical robustness (a MATCH-miss substring fallback) is
    /// a separate, unrealized concern — this method inherits whatever
    /// `memories_fts` finds, nothing more.
    pub fn search_ranked(&self, query: &str, limit: usize) -> Result<Vec<(MemoryEntry, f64)>> {
        // FTS5's own BM25 score (negative; a larger magnitude is a stronger
        // match). Map to a bounded, monotonically increasing (0, 1) score.
        let match_expr = fts_match_expr(query);
        let mut fts_stmt = self.conn.prepare(
            "SELECT memory_id, bm25(memories_fts) FROM memories_fts
             WHERE memories_fts MATCH ?1 ORDER BY rank LIMIT ?2",
        )?;
        let hits: Vec<(String, f64)> = fts_stmt
            .query_map(params![match_expr, limit as i64], |row| {
                let id: String = row.get(0)?;
                let bm25_raw: f64 = row.get(1)?;
                let strength = -bm25_raw;
                Ok((id, strength / (1.0 + strength)))
            })?
            .collect::<std::result::Result<_, _>>()?;

        let now = now_secs();
        let mut out = Vec::with_capacity(hits.len());
        for (id_str, base_text_relevance) in hits {
            let id = MemoryId::from(id_str);
            let Some(entry) = self.get(&id)? else {
                continue;
            };
            if entry.trust_score < TRUST_MIN_SEARCH
                || entry.superseded_at.is_some()
                || entry.lifecycle_state != LifecycleState::Active
                || entry.expiry.is_some_and(|e| e <= now)
            {
                continue;
            }

            let centrality = self.signal_factor(&id, SignalKind::Centrality)?;
            let cluster = self.signal_factor(&id, SignalKind::Cluster)?;
            let recency = self.signal_factor(&id, SignalKind::Recency)?;
            let score = base_text_relevance * centrality * cluster * recency;

            out.push((entry, score));
        }

        out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        Ok(out)
    }

    /// MI-2: temporal recall modes over the bi-temporal record. Composes
    /// with the same active/trusted defaults as `search_fts`, except
    /// `as-of` deliberately does **not** exclude since-superseded records —
    /// the question is "what was true then," not "what is true now."
    pub fn recall_temporal(
        &self,
        mode: cronus_contract::TemporalMode,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        use cronus_contract::TemporalMode;
        let (where_clause, order, bound): (&str, &str, i64) = match mode {
            TemporalMode::AsOf(t) => (
                "valid_at <= ?3 AND (superseded_at IS NULL OR superseded_at > ?3)",
                "valid_at DESC",
                t as i64,
            ),
            TemporalMode::ChangedSince(c) => (
                "(created_at > ?3 OR superseded_at > ?3)",
                "created_at DESC",
                c as i64,
            ),
            TemporalMode::Recent => ("1 = 1", "created_at DESC", 0),
        };
        let sql = format!(
            "SELECT id, kind, source, title, body, confidence, trust_score,
                    valid_at, created_at, superseded_at, workspace_id, verification_state,
                    depth, lifecycle_state, experience_outcome, actor, expiry, subject
             FROM memories
             WHERE lifecycle_state = ?1 AND trust_score >= ?2 AND {where_clause}
               AND (expiry IS NULL OR expiry > ?5)
             ORDER BY {order} LIMIT ?4"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(
                params![
                    LifecycleState::Active.as_str(),
                    TRUST_MIN_SEARCH,
                    bound,
                    limit as i64,
                    now_secs() as i64
                ],
                map_row,
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// MI-8: the closed structured-predicate vocabulary, compiled to a
    /// parameterized SQL `WHERE` fragment (the `predicate` module).
    pub fn recall_structured(
        &self,
        predicate: &cronus_contract::FieldPredicate,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        // One owned-value bind list carries everything — the two leading
        // defaults, then whatever the predicate compiler appends, then the
        // limit — so the bind order always matches the `?` order in `sql`
        // textually, with no separate reference-lifetime bookkeeping.
        let mut binds: Vec<Box<dyn rusqlite::types::ToSql>> = vec![
            Box::new(LifecycleState::Active.as_str().to_string()),
            Box::new(TRUST_MIN_SEARCH),
            Box::new(now_secs() as i64),
        ];
        let clause = super::predicate::compile(predicate, &mut binds);
        binds.push(Box::new(limit as i64));

        let sql = format!(
            "SELECT id, kind, source, title, body, confidence, trust_score,
                    valid_at, created_at, superseded_at, workspace_id, verification_state,
                    depth, lifecycle_state, experience_outcome, actor, expiry, subject
             FROM memories
             WHERE lifecycle_state = ? AND trust_score >= ?
               AND (expiry IS NULL OR expiry > ?) AND ({clause})
             ORDER BY created_at DESC LIMIT ?"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(rusqlite::params_from_iter(param_refs), map_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Hard-delete a memory entry by raw ID string.
    ///
    /// Returns `true` when a row was removed, `false` when not found.
    pub fn delete(&self, id: &str) -> Result<bool> {
        let affected = self
            .conn
            .execute("DELETE FROM memories WHERE id = ?1", params![id])?;
        if affected > 0 {
            let _ = self
                .conn
                .execute("DELETE FROM memories_fts WHERE memory_id = ?1", params![id]);
        }
        Ok(affected > 0)
    }

    /// HRR encoding seam — returns a zeroed vector (stub).
    /// Real implementation ships with sqlite-vec integration.
    pub fn encode_hrr(&self, _entry: &MemoryEntry) -> Vec<f32> {
        vec![0.0f32; 256]
    }

    // ── derived signals (MC-5) ───────────────────────────────────────────────

    /// Write (overwrite) a derived ranking signal for `id`, stamped with the
    /// current build's algorithm version and the current time (MC-5).
    pub fn set_signal(&self, id: &MemoryId, kind: SignalKind, value: f64) -> Result<()> {
        signal::write(&self.conn, id, kind, value, now_secs())
    }

    /// Read a derived signal's ranking factor for `id` — [`signal::NEUTRAL_FACTOR`]
    /// when absent or version-stale (MC-5/MC-8), never an error.
    pub fn signal_factor(&self, id: &MemoryId, kind: SignalKind) -> Result<f64> {
        signal::factor(&self.conn, id, kind)
    }

    /// Remove every derived signal for `id` (e.g. before a MC-6 merge discards it).
    pub fn clear_signals(&self, id: &MemoryId) -> Result<()> {
        signal::clear(&self.conn, id)
    }

    // ── consolidation write path (MC-2/3/4/7/9/10) ───────────────────────────

    /// Add a relationship/provenance edge (MC-3, additive-only — no way to
    /// remove or update an edge through this API by design).
    pub fn add_edge(&self, source: &MemoryId, target: &MemoryId, predicate: &str) -> Result<()> {
        consolidate::add_edge(&self.conn, source, target, predicate, now_secs())
    }

    /// Every `(target, predicate)` edge whose source is `id`.
    pub fn edges_from(&self, id: &MemoryId) -> Result<Vec<(MemoryId, String)>> {
        consolidate::edges_from(&self.conn, id)
    }

    /// Recompute the `Centrality` derived signal from the MC-3 edge graph's
    /// in-degree (the half of "recompute derived signals" `recompute_recency`
    /// does not cover).
    pub fn recompute_centrality(&self) -> Result<usize> {
        consolidate::recompute_centrality(&self.conn, now_secs())
    }

    /// The routine consolidation write (MC-4): create or corroborate,
    /// depending on whether an active consolidated item already matches
    /// `candidate` after normalization.
    pub fn consolidate(
        &self,
        candidate: MemoryEntry,
        provenance: Option<&MemoryId>,
        actor: &str,
    ) -> Result<(MemoryId, ConsolidationAction)> {
        consolidate::consolidate(&self.conn, candidate, provenance, actor, now_secs())
    }

    /// MI-6: the salience-gated capture policy — a confidence-honest gate in
    /// front of the same create/corroborate decision `consolidate` makes,
    /// plus MI-6 cross-reference edges to `related`. `entry.actor`/
    /// `.expiry`/`.subject` (set via the existing builders) flow through
    /// untouched; `audit_actor` is the MC-4 action-algebra's own audit-trail
    /// actor, independent of `entry.actor`.
    pub fn capture(
        &self,
        entry: MemoryEntry,
        related: &[MemoryId],
        audit_actor: &str,
    ) -> Result<CaptureOutcome> {
        capture::capture(&self.conn, entry, related, audit_actor, now_secs())
    }

    /// Explicit refine (MC-4): append `addition` to `target`'s body
    /// (additive-only, MC-3), optimistic-concurrency guarded (MC-9) against
    /// `expected_body` — what the caller read before deciding to refine.
    pub fn refine(
        &self,
        target: &MemoryId,
        expected_body: &str,
        addition: &str,
        provenance: Option<&MemoryId>,
        actor: &str,
    ) -> Result<()> {
        consolidate::refine(
            &self.conn,
            target,
            expected_body,
            addition,
            provenance,
            actor,
            now_secs(),
        )
    }

    /// Explicit correct (MC-4): non-destructively supersede `target` with
    /// `corrected`, transactionally (MC-9). Returns the new item's id.
    pub fn correct(
        &self,
        target: &MemoryId,
        corrected: MemoryEntry,
        actor: &str,
    ) -> Result<MemoryId> {
        consolidate::correct(&self.conn, target, corrected, actor, now_secs())
    }

    /// Run the incremental consolidation pass (MC-2) over every `raw`/
    /// `working` item past the checkpoint watermark. Advances the watermark
    /// only over inputs that committed — a failed input retries next pass.
    pub fn run_incremental_consolidation(
        &self,
        actor: &str,
    ) -> Result<Vec<(MemoryId, ConsolidationAction)>> {
        consolidate::run_incremental_pass(&self.conn, actor, now_secs())
    }

    /// Synthesize emergent topic summaries (MC-7) over the current MC-3 edge
    /// graph. Returns the new summary items' ids.
    pub fn synthesize_summaries(&self, actor: &str) -> Result<Vec<MemoryId>> {
        consolidate::synthesize_summaries(&self.conn, actor, now_secs())
    }

    /// Advisory interest topics (MC-10) — bounded, deduplicated, read-only.
    pub fn extract_interest_topics(&self, limit: usize) -> Result<Vec<InterestTopic>> {
        consolidate::extract_interest_topics(&self.conn, limit)
    }

    // ── lifecycle transitions (MI-9) ─────────────────────────────────────────

    /// Read `id`'s current lifecycle state. `None` when the item does not exist.
    pub fn lifecycle_state(&self, id: &MemoryId) -> Result<Option<LifecycleState>> {
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT lifecycle_state FROM memories WHERE id = ?1",
                params![id.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        Ok(raw.and_then(|s| LifecycleState::from_db_str(&s)))
    }

    // ── corpus maintenance (MC-6) ────────────────────────────────────────────

    /// Recompute the `Recency` derived signal for every active item
    /// (step 1 of the maintenance pass; centrality/cluster need the MC-3
    /// edge graph, computed separately). Returns the count updated.
    pub fn recompute_recency(&self) -> Result<usize> {
        maintenance::recompute_recency(&self.conn, now_secs())
    }

    /// Archive active items whose cushioned recency has decayed past the
    /// threshold (auto-applies — reversible, MC-6). Returns the archived ids.
    pub fn sweep_archive(&self, actor: &str) -> Result<Vec<MemoryId>> {
        maintenance::sweep_archive(&self.conn, actor, now_secs())
    }

    /// Auto-thaw an archived item on touch (MC-6). No-op for any other state.
    pub fn touch(&self, id: &MemoryId, actor: &str) -> Result<bool> {
        maintenance::touch(&self.conn, id, actor, now_secs())
    }

    /// Flag active items whose content crosses the overload threshold
    /// (MC-6 split candidates). Does not split them — that needs a
    /// generator; flagging is the complete no-generator behavior.
    pub fn flag_split_candidates(&self) -> Result<Vec<MemoryId>> {
        maintenance::flag_split_candidates(&self.conn)
    }

    /// Find pairs of active items that are exact duplicates after
    /// normalization (MC-6 merge candidates, elevated-gate stand-in).
    pub fn find_merge_candidates(&self) -> Result<Vec<(MemoryId, MemoryId)>> {
        maintenance::find_merge_candidates(&self.conn)
    }

    /// Merge `discard` into `keep`: re-point chain edges, drop derived
    /// signals, hard-delete `discard`, all transactionally (MC-6/MC-9).
    pub fn merge_pair(&self, keep: &MemoryId, discard: &MemoryId, actor: &str) -> Result<()> {
        maintenance::merge_pair(&self.conn, keep, discard, actor, now_secs())
    }

    /// Transition `id` to `new_state`, recording an append-only audit row
    /// (actor, instant, old→new) so "who shelved this and when" is
    /// answerable (MI-9). Returns the prior state, or `Ok(None)` if `id`
    /// does not exist (no transition, no audit row).
    pub fn set_lifecycle_state(
        &self,
        id: &MemoryId,
        new_state: LifecycleState,
        actor: &str,
    ) -> Result<Option<LifecycleState>> {
        let Some(old_state) = self.lifecycle_state(id)? else {
            return Ok(None);
        };

        self.conn.execute(
            "UPDATE memories SET lifecycle_state = ?1 WHERE id = ?2",
            params![new_state.as_str(), id.as_str()],
        )?;
        self.conn.execute(
            "INSERT INTO lifecycle_audit (item_id, actor, instant, old_state, new_state)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                id.as_str(),
                actor,
                now_secs() as i64,
                old_state.as_str(),
                new_state.as_str(),
            ],
        )?;
        Ok(Some(old_state))
    }

    /// Every stored entry, unfiltered. Backs `UserDataStore::export` (DN-7:
    /// always able to come home) — unlike `search_fts`, this applies no
    /// trust-score gate, since export is an operator/backup action, not a
    /// ranked retrieval.
    pub fn export_all(&self) -> Result<Vec<MemoryEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, source, title, body, confidence, trust_score,
                    valid_at, created_at, superseded_at, workspace_id, verification_state,
                    depth, lifecycle_state, experience_outcome, actor, expiry, subject
             FROM memories",
        )?;
        let entries = stmt
            .query_map([], map_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(entries)
    }

    // ── private helpers ───────────────────────────────────────────────────────

    fn latest_in_workspace(
        &self,
        workspace_id: &str,
        before_secs: u64,
    ) -> Result<Option<MemoryId>> {
        let cutoff = before_secs.saturating_sub(SESSION_CHAIN_WINDOW_SECS) as i64;
        let result = self.conn.query_row(
            "SELECT id FROM memories
             WHERE workspace_id = ?1 AND created_at >= ?2
             ORDER BY created_at DESC LIMIT 1",
            params![workspace_id, cutoff],
            |row| row.get::<_, String>(0),
        );
        match result {
            Ok(id) => Ok(Some(MemoryId::from(id))),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(MemoryError::Database(e)),
        }
    }

    fn chain_internal(&self, source: &MemoryId, target: &MemoryId, kind: ChainKind) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO memory_chains (source_id, target_id, kind, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                source.as_str(),
                target.as_str(),
                kind.as_str(),
                now_secs() as i64
            ],
        )?;
        Ok(())
    }

    fn propagate_recursive(&self, from: &MemoryId, base_delta: f64, depth: usize) -> Result<()> {
        if depth >= BELLMAN_MAX_DEPTH {
            return Ok(());
        }
        let delta = propagated_delta(base_delta, depth);

        let mut stmt = self
            .conn
            .prepare("SELECT target_id FROM memory_chains WHERE source_id = ?1")?;
        let targets: Vec<String> = stmt
            .query_map(params![from.as_str()], |row| row.get(0))?
            .collect::<std::result::Result<_, _>>()?;

        for target_str in targets {
            let target = MemoryId::from(target_str);
            let current: f64 = match self.conn.query_row(
                "SELECT trust_score FROM memories WHERE id = ?1",
                params![target.as_str()],
                |row| row.get(0),
            ) {
                Ok(v) => v,
                Err(rusqlite::Error::QueryReturnedNoRows) => continue,
                Err(e) => return Err(MemoryError::Database(e)),
            };
            let new_score = (current + delta).clamp(0.0, 1.0);
            self.conn.execute(
                "UPDATE memories SET trust_score = ?1 WHERE id = ?2",
                params![new_score, target.as_str()],
            )?;
            self.propagate_recursive(&target, base_delta, depth + 1)?;
        }
        Ok(())
    }

    fn ids_in_workspace(&self, workspace_id: &str) -> Result<Vec<MemoryId>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM memories WHERE workspace_id = ?1")?;
        let ids = stmt
            .query_map(params![workspace_id], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .map(MemoryId::from)
            .collect();
        Ok(ids)
    }
}

// ── MemorySearch (ports tier) ────────────────────────────────────────────────
//
// The pivot (§4.6): `MemoryStore` is the concrete, SQLite-backed
// implementation of the `MemorySearch` seam declared in `cronus-contract`.
// `ContextRouter` depends on the trait, never on this type — this `impl`
// block is the only place that still names both sides.

impl cronus_contract::MemorySearch for MemoryStore {
    fn search_fts(
        &self,
        query: &str,
        limit: usize,
    ) -> std::result::Result<Vec<MemoryEntry>, String> {
        MemoryStore::search_fts(self, query, limit).map_err(|e| e.to_string())
    }

    fn recall_temporal(
        &self,
        mode: cronus_contract::TemporalMode,
        limit: usize,
    ) -> std::result::Result<Vec<MemoryEntry>, String> {
        MemoryStore::recall_temporal(self, mode, limit).map_err(|e| e.to_string())
    }

    fn recall_structured(
        &self,
        predicate: &cronus_contract::FieldPredicate,
        limit: usize,
    ) -> std::result::Result<Vec<MemoryEntry>, String> {
        MemoryStore::recall_structured(self, predicate, limit).map_err(|e| e.to_string())
    }
}

impl cronus_contract::UserDataStore for MemoryStore {
    fn put(&self, entry: &MemoryEntry) -> std::result::Result<(), String> {
        MemoryStore::add(self, entry.clone())
            .map(|_id| ())
            .map_err(|e| e.to_string())
    }

    fn export(&self) -> std::result::Result<Vec<MemoryEntry>, String> {
        MemoryStore::export_all(self).map_err(|e| e.to_string())
    }
}

// ── schema ────────────────────────────────────────────────────────────────────

/// The schema version this build reads and writes (`crate::versioning`).
/// Bump it together with a step appended to the migration list passed in
/// [`setup`] whenever the shape below changes.
pub(crate) const SCHEMA_VERSION: i64 = 1;

pub(crate) fn setup(conn: &Connection) -> Result<()> {
    conn.execute_batch("PRAGMA journal_mode = WAL")?;
    conn.execute_batch("PRAGMA foreign_keys = ON")?;
    crate::versioning::open_schema(conn, "memory", SCHEMA_VERSION, create_schema, &[])
}

/// Every table of the memory database at the current version, idempotently.
fn create_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS memories (
            id                 TEXT PRIMARY KEY NOT NULL,
            kind               TEXT NOT NULL,
            source             TEXT NOT NULL,
            title              TEXT NOT NULL,
            body               TEXT NOT NULL,
            confidence         REAL NOT NULL DEFAULT 1.0,
            trust_score        REAL NOT NULL DEFAULT 0.5,
            valid_at           INTEGER NOT NULL,
            created_at         INTEGER NOT NULL,
            superseded_at      INTEGER,
            workspace_id       TEXT,
            verification_state TEXT NOT NULL DEFAULT 'Untested',
            depth              TEXT NOT NULL DEFAULT 'Consolidated',
            lifecycle_state    TEXT NOT NULL DEFAULT 'Active',
            experience_outcome TEXT,
            actor              TEXT,
            expiry             INTEGER,
            subject            TEXT
        )",
    )?;
    // Standalone FTS5 table — synced manually in add().
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
            memory_id UNINDEXED,
            title,
            body
        )",
    )?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS memory_chains (
            source_id  TEXT NOT NULL,
            target_id  TEXT NOT NULL,
            kind       TEXT NOT NULL DEFAULT 'Continuation',
            created_at INTEGER NOT NULL,
            PRIMARY KEY (source_id, target_id)
        )",
    )?;
    // MI-9: append-only audit of every lifecycle-state transition.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS lifecycle_audit (
            item_id    TEXT    NOT NULL,
            actor      TEXT    NOT NULL,
            instant    INTEGER NOT NULL,
            old_state  TEXT    NOT NULL,
            new_state  TEXT    NOT NULL
        )",
    )?;
    signal::migrate(conn)?;
    super::maintenance::migrate(conn)?;
    super::consolidate::migrate(conn)?;
    Ok(())
}

// ── shared insert (also used by the consolidation write path) ────────────────

/// Raw insert: writes `memories` + syncs the FTS index. No auto-chain — that
/// is a session-continuation concept `MemoryStore::add` layers on top;
/// consolidation-authored items are not session activity.
pub(crate) fn insert(conn: &Connection, entry: MemoryEntry) -> Result<MemoryId> {
    let id = entry.id.clone();
    conn.execute(
        "INSERT INTO memories
         (id, kind, source, title, body, confidence, trust_score,
          valid_at, created_at, superseded_at, workspace_id, verification_state,
          depth, lifecycle_state, experience_outcome, actor, expiry, subject)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
        params![
            id.as_str(),
            entry.kind.as_str(),
            entry.source.as_str(),
            &entry.title,
            &entry.body,
            entry.confidence,
            entry.trust_score,
            entry.valid_at as i64,
            entry.created_at as i64,
            entry.superseded_at.map(|v| v as i64),
            entry.workspace_id.as_deref(),
            entry.verification_state.as_str(),
            entry.depth.as_str(),
            entry.lifecycle_state.as_str(),
            entry.experience_outcome.map(|o| o.as_str()),
            entry.actor.as_deref(),
            entry.expiry.map(|v| v as i64),
            entry.subject.map(|s| s.as_str()),
        ],
    )?;
    conn.execute(
        "INSERT INTO memories_fts(memory_id, title, body) VALUES (?1, ?2, ?3)",
        params![id.as_str(), &entry.title, &entry.body],
    )?;
    Ok(id)
}

// ── row mapper ────────────────────────────────────────────────────────────────

pub(crate) fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryEntry> {
    let id: String = row.get(0)?;
    let kind_str: String = row.get(1)?;
    let src_str: String = row.get(2)?;
    let valid_at: i64 = row.get(7)?;
    let created_at: i64 = row.get(8)?;
    let superseded_at: Option<i64> = row.get(9)?;
    let vs_str: String = row.get(11)?;
    let depth_str: String = row.get(12)?;
    let lifecycle_str: String = row.get(13)?;
    let experience_outcome_str: Option<String> = row.get(14)?;
    let actor: Option<String> = row.get(15)?;
    let expiry: Option<i64> = row.get(16)?;
    let subject_str: Option<String> = row.get(17)?;

    Ok(MemoryEntry {
        id: MemoryId::from(id),
        kind: MemoryKind::from_db_str(&kind_str).unwrap_or(MemoryKind::ProjectContext),
        source: MemorySource::from_db_str(&src_str).unwrap_or(MemorySource::System),
        title: row.get(3)?,
        body: row.get(4)?,
        confidence: row.get(5)?,
        trust_score: row.get(6)?,
        valid_at: valid_at as u64,
        created_at: created_at as u64,
        superseded_at: superseded_at.map(|v| v as u64),
        workspace_id: row.get(10)?,
        verification_state: VerificationState::from_db_str(&vs_str)
            .unwrap_or(VerificationState::Untested),
        depth: MemoryDepth::from_db_str(&depth_str).unwrap_or(MemoryDepth::Consolidated),
        lifecycle_state: LifecycleState::from_db_str(&lifecycle_str)
            .unwrap_or(LifecycleState::Active),
        experience_outcome: experience_outcome_str.and_then(|s| ExperienceOutcome::from_db_str(&s)),
        actor,
        expiry: expiry.map(|v| v as u64),
        subject: subject_str.and_then(|s| MemorySubject::from_db_str(&s)),
    })
}
