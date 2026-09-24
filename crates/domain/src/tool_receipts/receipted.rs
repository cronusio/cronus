//! `Receipted<T>` — an outcome that cannot exist without a receipt.
//! There is no public constructor and no `From<T>`: the only way to obtain
//! one is [`mint_receipted`], which requires the caller to already hold a
//! [`ReceiptKey`] and a filled [`ActionBinding`] carrying the real observed
//! outcome. The value and its proof are produced together in one step, so
//! a caller can never hold the value without the receipt travelling
//! beside it.

use super::binding::ActionBinding;
use super::key::ReceiptKey;
use super::mac::{self, Receipt};

/// `Receipted<T>` cannot be constructed directly — there is no public
/// constructor and no `From<T>` impl. Attempting to build one from
/// outside this module fails to compile:
///
/// ```compile_fail
/// use cronus_domain::tool_receipts::Receipted;
///
/// fn build<T>(value: T, receipt: cronus_domain::tool_receipts::Receipt) -> Receipted<T> {
///     Receipted::new(value, receipt)
/// }
/// ```
#[derive(Debug, Clone)]
pub struct Receipted<T> {
    value: T,
    pub receipt: Receipt,
}

impl<T> Receipted<T> {
    fn new(value: T, receipt: Receipt) -> Self {
        Receipted { value, receipt }
    }

    /// The observed value this receipt witnesses.
    pub fn value(&self) -> &T {
        &self.value
    }

    /// Unwrap into `(value, receipt)` — e.g. to return the value to a
    /// caller that separately logs or displays the receipt.
    pub fn into_parts(self) -> (T, Receipt) {
        (self.value, self.receipt)
    }
}

/// The only way to obtain a `Receipted<T>`: mint the receipt over
/// `binding` and wrap `value` with it in one step. `binding` must already
/// carry the real observed `outcome_tag`/`result_digest` — there is no
/// path that wraps a value before its result is known.
pub fn mint_receipted<T>(key: &ReceiptKey, binding: ActionBinding, value: T) -> Receipted<T> {
    let receipt = mac::mint(key, &binding);
    Receipted::new(value, receipt)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_binding(action_id: u64) -> ActionBinding {
        ActionBinding {
            action_id,
            action_kind: "test.action".to_string(),
            inputs_digest: b"in".to_vec(),
            outcome_tag: "ok".to_string(),
            result_digest: b"out".to_vec(),
            timestamp_ms: 1_700_000_000_000,
        }
    }

    #[test]
    fn mint_receipted_carries_the_value_and_a_matching_receipt() {
        let key = ReceiptKey::from_bytes([7u8; 32]);
        let binding = fixed_binding(1);
        let receipted = mint_receipted(&key, binding.clone(), "the value");
        assert_eq!(*receipted.value(), "the value");
        assert!(mac::verify(&key, &binding, &receipted.receipt));
    }

    #[test]
    fn into_parts_round_trips_the_value_and_receipt() {
        let key = ReceiptKey::from_bytes([9u8; 32]);
        let binding = fixed_binding(2);
        let receipted = mint_receipted(&key, binding, 42u32);
        let (value, receipt) = receipted.into_parts();
        assert_eq!(value, 42);
        assert!(!receipt.token.is_empty());
    }
}
