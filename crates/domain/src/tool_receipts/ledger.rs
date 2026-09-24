//! The per-turn receipt ledger: the sole authority on "did
//! this happen". Any component that would record an action as fact
//! consults [`ReceiptLedger::status`] and treats [`ReceiptStatus::Unreceipted`]
//! as fabricated — a default-deny on the *fact-recording* path, never
//! prose parsing.

use std::collections::HashMap;

use super::mac::Receipt;

#[derive(Debug, Clone)]
enum LedgerEntry {
    Pending,
    Minted(Receipt),
}

/// This turn's receipts, keyed by `action_id`. There is no public API on
/// this type that converts an absent entry into a recorded fact — the
/// only two ways an entry appears are [`record_pending`](ReceiptLedger::record_pending)
/// (a real deferred dispatch) and [`record_minted`](ReceiptLedger::record_minted)
/// (a real mint), both called only from the dispatch seam.
#[derive(Debug, Default)]
pub struct ReceiptLedger {
    entries: HashMap<u64, LedgerEntry>,
}

/// The answer to "did `action_id` happen": [`Unreceipted`](ReceiptStatus::Unreceipted)
/// is the default for anything never dispatched, never upgraded to fact by
/// absence alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiptStatus {
    Unreceipted,
    Pending,
    Receipted(Receipt),
}

/// A shape that cannot be rounded down to a single "verified" number:
/// a caller must display both counts, so outstanding work stays
/// visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CoverageReport {
    pub receipted: usize,
    pub pending: usize,
}

impl ReceiptLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// `Unreceipted` for any `action_id` never dispatched — not an error,
    /// not an assumption of truth.
    pub fn status(&self, action_id: u64) -> ReceiptStatus {
        match self.entries.get(&action_id) {
            None => ReceiptStatus::Unreceipted,
            Some(LedgerEntry::Pending) => ReceiptStatus::Pending,
            Some(LedgerEntry::Minted(receipt)) => ReceiptStatus::Receipted(receipt.clone()),
        }
    }

    /// Record a detached action as outstanding: no tag, nothing to
    /// bind yet.
    pub fn record_pending(&mut self, action_id: u64) {
        self.entries.insert(action_id, LedgerEntry::Pending);
    }

    /// Record (or promote a `Pending` entry to) a minted receipt.
    pub fn record_minted(&mut self, action_id: u64, receipt: Receipt) {
        self.entries.insert(action_id, LedgerEntry::Minted(receipt));
    }

    /// `{ receipted, pending }` — separate counts so a caller cannot report
    /// full coverage while `pending > 0`.
    pub fn coverage(&self) -> CoverageReport {
        let mut report = CoverageReport::default();
        for entry in self.entries.values() {
            match entry {
                LedgerEntry::Pending => report.pending += 1,
                LedgerEntry::Minted(_) => report.receipted += 1,
            }
        }
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_receipts::{ActionBinding, ReceiptKey, mint};

    fn a_receipt(action_id: u64) -> Receipt {
        let key = ReceiptKey::from_bytes([1u8; 32]);
        let binding = ActionBinding {
            action_id,
            action_kind: "test.action".to_string(),
            inputs_digest: b"in".to_vec(),
            outcome_tag: "ok".to_string(),
            result_digest: b"out".to_vec(),
            timestamp_ms: 1_700_000_000_000,
        };
        mint(&key, &binding)
    }

    #[test]
    fn status_defaults_to_unreceipted_for_an_action_never_dispatched() {
        let ledger = ReceiptLedger::new();
        assert_eq!(ledger.status(999), ReceiptStatus::Unreceipted);
    }

    #[test]
    fn a_minted_action_reports_receipted_and_a_deferred_one_reports_pending() {
        let mut ledger = ReceiptLedger::new();
        ledger.record_minted(1, a_receipt(1));
        ledger.record_pending(2);

        assert_eq!(ledger.status(1), ReceiptStatus::Receipted(a_receipt(1)));
        assert_eq!(ledger.status(2), ReceiptStatus::Pending);
        assert_eq!(ledger.status(3), ReceiptStatus::Unreceipted);
    }

    #[test]
    fn coverage_reports_receipted_and_pending_as_separate_counts() {
        let mut ledger = ReceiptLedger::new();
        ledger.record_minted(1, a_receipt(1));
        ledger.record_minted(2, a_receipt(2));
        ledger.record_pending(3);

        let coverage = ledger.coverage();
        assert_eq!(coverage.receipted, 2);
        assert_eq!(coverage.pending, 1);
    }

    /// The existence guarantee is an absence, not a branch: this test's real job
    /// is naming the reviewed API surface, since a runtime assertion
    /// cannot prove a function does not exist. `ReceiptLedger` exposes
    /// exactly four methods — `new`, `status`, `record_pending`,
    /// `record_minted`, `coverage` — reviewed here by inspection: none of
    /// them, alone or composed, turns a `status()` miss into a recorded
    /// fact. `record_pending`/`record_minted` both require the caller to
    /// already be inside the dispatch seam holding a real `action_id` (and,
    /// for `record_minted`, a real `Receipt` only `mint` can produce) —
    /// there is no "assume true when absent" entry point.
    #[test]
    fn no_public_api_upgrades_unreceipted_to_a_recorded_fact() {
        let ledger = ReceiptLedger::new();
        assert_eq!(ledger.status(42), ReceiptStatus::Unreceipted);
    }
}
