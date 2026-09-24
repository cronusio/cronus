//! Deferred-action lifecycle: an action detached from the current
//! turn cannot be receipted at dispatch,
//! because its result — the thing the MAC must cover — does not
//! exist yet. `Pending` is a distinct state, never a receipt over a
//! placeholder result: a tag computed over a fabricated result would be a
//! *valid* receipt for a false claim, which inverts the entire subsystem.

use super::binding::ActionBinding;
use super::key::ReceiptKey;
use super::ledger::ReceiptLedger;
use super::receipted::{Receipted, mint_receipted};

/// Register a detached action at dispatch time: nothing to bind yet, so no
/// tag is produced — the ledger simply remembers that `action_id` is
/// outstanding.
pub fn defer(ledger: &mut ReceiptLedger, action_id: u64) {
    ledger.record_pending(action_id);
}

/// Resolve a previously deferred action once its real result is observed:
/// mint a full receipt over the actual binding and value, and transition
/// the ledger entry from `Pending` to `Minted`. There is no constructor
/// for a receipt over a result that was never observed — `value` and
/// `binding` (which itself carries the real `outcome_tag`/`result_digest`)
/// must both be supplied by the caller that actually observed completion.
pub fn resolve_deferred<T>(
    ledger: &mut ReceiptLedger,
    key: &ReceiptKey,
    binding: ActionBinding,
    value: T,
) -> Receipted<T> {
    let action_id = binding.action_id;
    let receipted = mint_receipted(key, binding, value);
    ledger.record_minted(action_id, receipted.receipt.clone());
    receipted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_receipts::ReceiptStatus;

    fn binding(action_id: u64) -> ActionBinding {
        ActionBinding {
            action_id,
            action_kind: "deferred.action".to_string(),
            inputs_digest: b"in".to_vec(),
            outcome_tag: "ok".to_string(),
            result_digest: b"real-result".to_vec(),
            timestamp_ms: 1_700_000_000_000,
        }
    }

    #[test]
    fn a_detached_action_registers_pending_with_no_tag_at_dispatch() {
        let mut ledger = ReceiptLedger::new();
        defer(&mut ledger, 7);
        assert_eq!(ledger.status(7), ReceiptStatus::Pending);
    }

    #[test]
    fn correlated_completion_mints_a_full_receipt_binding_the_real_result() {
        let mut ledger = ReceiptLedger::new();
        let key = ReceiptKey::from_bytes([4u8; 32]);
        defer(&mut ledger, 7);

        let receipted = resolve_deferred(&mut ledger, &key, binding(7), "the real value");
        assert_eq!(*receipted.value(), "the real value");
        assert!(matches!(ledger.status(7), ReceiptStatus::Receipted(_)));
    }

    #[test]
    fn coverage_moves_from_pending_to_receipted_on_resolution() {
        let mut ledger = ReceiptLedger::new();
        let key = ReceiptKey::from_bytes([4u8; 32]);
        defer(&mut ledger, 7);
        assert_eq!(ledger.coverage().pending, 1);
        assert_eq!(ledger.coverage().receipted, 0);

        resolve_deferred(&mut ledger, &key, binding(7), ());
        assert_eq!(ledger.coverage().pending, 0);
        assert_eq!(ledger.coverage().receipted, 1);
    }

    /// There is no constructor for a receipt over an unobserved result:
    /// `resolve_deferred` requires an actual `value` and a `binding` whose
    /// `result_digest` the caller can only have filled after observing the
    /// real outcome — reviewed here by inspection of the function
    /// signature, the same default-deny-style absence argument `ledger.rs` makes
    /// for its own API surface.
    #[test]
    fn resolving_requires_supplying_both_the_binding_and_the_value() {
        let mut ledger = ReceiptLedger::new();
        let key = ReceiptKey::from_bytes([4u8; 32]);
        let receipted = resolve_deferred(&mut ledger, &key, binding(1), 100u32);
        assert_eq!(*receipted.value(), 100u32);
    }
}
