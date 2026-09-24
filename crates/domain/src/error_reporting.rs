//! GitHub issue reporting: on an unrepairable error, with consent,
//! sanitize diagnostics, fingerprint them for cross-episode dedup, and
//! prepare a previewable report — filing/updating the actual issue is a
//! GitHub CLI/API integration deferred behind the [`FilingDecision`] this
//! module produces.
//!
//! Sanitization reuses [`crate::redact`] rather than re-implementing secret
//! scrubbing; consent is checked before any preview is built for egress; the
//! error is fingerprinted (normalize → BLAKE3) so the same failure across
//! machines and episodes converges on one issue instead of spamming new ones.

use std::collections::BTreeMap;

/// The three consent modes (`report.consent`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentMode {
    Always,
    Ask,
    Never,
}

/// Whether the pipeline may proceed to sanitize/fingerprint/preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentDecision {
    Blocked,
    Proceed,
}

/// Resolve consent: `never` always blocks; `always` always proceeds;
/// `ask` proceeds only on an explicit `Some(true)` — an absent or negative
/// answer blocks, fail-closed (consent-gated, never assumed).
pub fn consent_decision(mode: ConsentMode, user_confirmed: Option<bool>) -> ConsentDecision {
    match mode {
        ConsentMode::Never => ConsentDecision::Blocked,
        ConsentMode::Always => ConsentDecision::Proceed,
        ConsentMode::Ask if user_confirmed == Some(true) => ConsentDecision::Proceed,
        ConsentMode::Ask => ConsentDecision::Blocked,
    }
}

// --- Error fingerprinting ---

/// Replace every `0x`-prefixed run of hex digits with a sentinel, so two
/// otherwise-identical panics at different ASLR addresses fingerprint the
/// same. Hand-rolled (no `regex` dependency) — the pattern is simple enough
/// that a manual scanner is both cheaper and easier to audit.
fn strip_hex_addresses(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'0' && i + 1 < bytes.len() && bytes[i + 1] == b'x' {
            let start = i;
            let mut j = i + 2;
            while j < bytes.len() && bytes[j].is_ascii_hexdigit() {
                j += 1;
            }
            if j > i + 2 {
                out.push_str("0xADDR");
                i = j;
                continue;
            }
            let _ = start;
        }
        // Safe: we only ever advance by whole-char boundaries below.
        let ch = text[i..].chars().next().unwrap_or('\u{0}');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Replace every occurrence of `home_dir` with `/USER` so cross-machine
/// panics carrying different home paths still match.
fn strip_home_dir(text: &str, home_dir: &str) -> String {
    if home_dir.is_empty() {
        return text.to_string();
    }
    text.replace(home_dir, "/USER")
}

/// Normalize a message before fingerprinting.
pub fn normalize_message(message: &str, home_dir: &str) -> String {
    strip_home_dir(&strip_hex_addresses(message), home_dir)
}

/// `BLAKE3(error_type|normalized_message)` as 64 lowercase hex chars.
pub fn fingerprint_error(error_type: &str, message: &str, home_dir: &str) -> String {
    let normalized = normalize_message(message, home_dir);
    let canonical = format!("{error_type}|{normalized}");
    blake3::hash(canonical.as_bytes()).to_hex().to_string()
}

// --- Dedup table ---

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FingerprintMatch {
    pub episode_id: String,
    pub occurrence_count: i64,
    pub first_seen: u64,
    pub last_seen: u64,
}

#[derive(Debug, Clone, Default)]
struct FingerprintRow {
    occurrence_count: i64,
    first_seen: u64,
    last_seen: u64,
}

/// In-memory dedup table keyed by `(hash, episode_id)`; a durable SQLite
/// backing (illustrative DDL) is a persistence concern layered on top
/// of this same API, not implemented here.
#[derive(Debug, Default)]
pub struct FingerprintTable {
    rows: BTreeMap<(String, String), FingerprintRow>,
}

impl FingerprintTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Upsert one occurrence at `now`, returning the row's new occurrence count.
    pub fn record_at(&mut self, hash: &str, episode_id: &str, now: u64) -> i64 {
        let row = self
            .rows
            .entry((hash.to_string(), episode_id.to_string()))
            .or_insert_with(|| FingerprintRow {
                occurrence_count: 0,
                first_seen: now,
                last_seen: now,
            });
        row.occurrence_count += 1;
        row.last_seen = now;
        row.occurrence_count
    }

    /// Prior episodes (excluding `exclude_episode_id`) where `hash` appeared,
    /// most recently seen first, capped at 10.
    pub fn matches(&self, hash: &str, exclude_episode_id: &str) -> Vec<FingerprintMatch> {
        let mut found: Vec<FingerprintMatch> = self
            .rows
            .iter()
            .filter(|((h, episode_id), _)| h == hash && episode_id != exclude_episode_id)
            .map(|((_, episode_id), row)| FingerprintMatch {
                episode_id: episode_id.clone(),
                occurrence_count: row.occurrence_count,
                first_seen: row.first_seen,
                last_seen: row.last_seen,
            })
            .collect();
        found.sort_by_key(|m| std::cmp::Reverse(m.last_seen));
        found.truncate(10);
        found
    }

    /// Sum of occurrences for `hash` across every episode.
    pub fn total_occurrences(&self, hash: &str) -> i64 {
        self.rows
            .iter()
            .filter(|((h, _), _)| h == hash)
            .map(|(_, row)| row.occurrence_count)
            .sum()
    }
}

// --- Report pipeline (actionable content) ---

/// The raw facts about one error occurrence, before sanitization.
#[derive(Debug, Clone)]
pub struct ReportRequest {
    pub error_type: String,
    pub message: String,
    pub app_version: String,
    pub environment_summary: String,
    pub repro_hints: Vec<String>,
    pub episode_id: String,
}

/// The exact content that would be sent — always shown before send (the
/// preview) and containing only sanitized/allowlisted fields, plus
/// prior-occurrence context surfaced from the dedup table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportPreview {
    pub fingerprint: String,
    pub sanitized_message: String,
    pub app_version: String,
    pub environment_summary: String,
    pub repro_hints: Vec<String>,
    pub prior_matches: Vec<FingerprintMatch>,
    pub total_occurrences: i64,
}

/// Sanitize (via [`crate::redact::redact`], never re-implemented), record
/// the fingerprint, and build the previewable payload. This performs no
/// egress by itself — it is the content the consent-gated
/// filing step (a separate, deferred GitHub API/CLI integration) would send.
pub fn prepare_preview(
    request: &ReportRequest,
    home_dir: &str,
    secrets: &[&str],
    fingerprints: &mut FingerprintTable,
    now: u64,
) -> ReportPreview {
    let sanitized_message = crate::redact::redact(&request.message, secrets);
    let fingerprint = fingerprint_error(&request.error_type, &sanitized_message, home_dir);
    fingerprints.record_at(&fingerprint, &request.episode_id, now);

    ReportPreview {
        prior_matches: fingerprints.matches(&fingerprint, &request.episode_id),
        total_occurrences: fingerprints.total_occurrences(&fingerprint),
        fingerprint,
        sanitized_message,
        app_version: request.app_version.clone(),
        environment_summary: request.environment_summary.clone(),
        repro_hints: request.repro_hints.clone(),
    }
}

/// What filing would do, given whether a matching open issue is already known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilingDecision {
    CreateNew,
    UpdateExisting { issue_ref: String },
}

/// Decide create-vs-update (`SRCH`) from a caller-supplied existing-issue
/// lookup result — the actual GitHub search/API call is a network
/// integration deferred behind this seam.
pub fn decide_filing(existing_issue_ref: Option<&str>) -> FilingDecision {
    match existing_issue_ref {
        Some(issue_ref) => FilingDecision::UpdateExisting {
            issue_ref: issue_ref.to_string(),
        },
        None => FilingDecision::CreateNew,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consent_never_always_blocks_regardless_of_user_answer() {
        assert_eq!(
            consent_decision(ConsentMode::Never, Some(true)),
            ConsentDecision::Blocked
        );
        assert_eq!(
            consent_decision(ConsentMode::Never, None),
            ConsentDecision::Blocked
        );
    }

    #[test]
    fn consent_always_proceeds_without_asking() {
        assert_eq!(
            consent_decision(ConsentMode::Always, None),
            ConsentDecision::Proceed
        );
    }

    #[test]
    fn consent_ask_requires_an_explicit_yes_fail_closed() {
        assert_eq!(
            consent_decision(ConsentMode::Ask, Some(true)),
            ConsentDecision::Proceed
        );
        assert_eq!(
            consent_decision(ConsentMode::Ask, Some(false)),
            ConsentDecision::Blocked
        );
        assert_eq!(
            consent_decision(ConsentMode::Ask, None),
            ConsentDecision::Blocked
        );
    }

    #[test]
    fn hex_addresses_normalize_to_one_sentinel() {
        let a = normalize_message("panic at 0xdeadbeef in frame", "");
        let b = normalize_message("panic at 0xcafebabe in frame", "");
        assert_eq!(a, b);
        assert_eq!(a, "panic at 0xADDR in frame");
    }

    #[test]
    fn home_directory_normalizes_to_a_sentinel() {
        let normalized =
            normalize_message("failed to read /home/alice/project/file.rs", "/home/alice");
        assert_eq!(normalized, "failed to read /USER/project/file.rs");
    }

    #[test]
    fn same_panic_on_two_machines_fingerprints_identically() {
        let machine_a = fingerprint_error(
            "panic",
            "at 0xdeadbeef reading /home/alice/x.rs",
            "/home/alice",
        );
        let machine_b =
            fingerprint_error("panic", "at 0xfeedface reading /home/bob/x.rs", "/home/bob");
        assert_eq!(machine_a, machine_b);
        assert_eq!(machine_a.len(), 64, "BLAKE3 hex digest is 64 chars");
    }

    #[test]
    fn a_different_error_type_never_collides_with_the_same_message() {
        let a = fingerprint_error("panic", "same text", "");
        let b = fingerprint_error("io_error", "same text", "");
        assert_ne!(
            a, b,
            "error type is part of the signature (SS5 collision mitigation)"
        );
    }

    #[test]
    fn dedup_table_upserts_and_reports_occurrence_count() {
        let mut table = FingerprintTable::new();
        assert_eq!(table.record_at("h1", "ep-1", 100), 1);
        assert_eq!(
            table.record_at("h1", "ep-1", 200),
            2,
            "same episode increments, not duplicates"
        );
        assert_eq!(
            table.record_at("h1", "ep-2", 300),
            1,
            "a different episode starts its own count"
        );
    }

    #[test]
    fn matches_excludes_the_current_episode_and_orders_by_recency() {
        let mut table = FingerprintTable::new();
        table.record_at("h1", "ep-1", 100);
        table.record_at("h1", "ep-2", 300);
        table.record_at("h1", "ep-3", 200);

        let found = table.matches("h1", "ep-1");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].episode_id, "ep-2", "most recently seen first");
        assert_eq!(found[1].episode_id, "ep-3");
    }

    #[test]
    fn matches_caps_at_ten_prior_episodes() {
        let mut table = FingerprintTable::new();
        for n in 0..15 {
            table.record_at("h1", &format!("ep-{n}"), n as u64);
        }
        assert_eq!(table.matches("h1", "current").len(), 10);
    }

    #[test]
    fn total_occurrences_sums_across_every_episode() {
        let mut table = FingerprintTable::new();
        table.record_at("h1", "ep-1", 1);
        table.record_at("h1", "ep-1", 2);
        table.record_at("h1", "ep-2", 3);
        assert_eq!(table.total_occurrences("h1"), 3);
    }

    #[test]
    fn preview_scrubs_a_seeded_secret_via_core_redaction() {
        let request = ReportRequest {
            error_type: "panic".to_string(),
            message: "auth failed with token=sk-LIVE-abc123 while connecting".to_string(),
            app_version: "0.1.0".to_string(),
            environment_summary: "windows 10".to_string(),
            repro_hints: vec!["run cronus status".to_string()],
            episode_id: "ep-current".to_string(),
        };
        let mut table = FingerprintTable::new();
        let preview = prepare_preview(&request, "", &["sk-LIVE-abc123"], &mut table, 1000);

        assert!(!preview.sanitized_message.contains("sk-LIVE-abc123"));
        assert!(preview.sanitized_message.contains(crate::redact::MASK));
        assert!(
            preview.sanitized_message.contains("while connecting"),
            "non-secret content preserved"
        );
        assert_eq!(preview.fingerprint.len(), 64);
    }

    #[test]
    fn a_prior_occurrence_surfaces_in_the_next_episodes_preview() {
        let mut table = FingerprintTable::new();
        let first = ReportRequest {
            error_type: "panic".to_string(),
            message: "boom".to_string(),
            app_version: "0.1.0".to_string(),
            environment_summary: "linux".to_string(),
            repro_hints: vec![],
            episode_id: "ep-1".to_string(),
        };
        let first_preview = prepare_preview(&first, "", &[], &mut table, 100);
        assert!(
            first_preview.prior_matches.is_empty(),
            "no prior episodes yet"
        );

        let second = ReportRequest {
            episode_id: "ep-2".to_string(),
            ..first.clone()
        };
        let second_preview = prepare_preview(&second, "", &[], &mut table, 200);
        assert_eq!(second_preview.prior_matches.len(), 1);
        assert_eq!(second_preview.prior_matches[0].episode_id, "ep-1");
        assert_eq!(
            second_preview.total_occurrences, 2,
            "seen twice total across both episodes"
        );
    }

    #[test]
    fn filing_decision_creates_when_no_existing_issue_and_updates_when_found() {
        assert_eq!(decide_filing(None), FilingDecision::CreateNew);
        assert_eq!(
            decide_filing(Some("owner/repo#42")),
            FilingDecision::UpdateExisting {
                issue_ref: "owner/repo#42".to_string()
            }
        );
    }
}
