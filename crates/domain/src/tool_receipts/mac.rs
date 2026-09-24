//! Minting and verifying receipts: `blake3::keyed_hash` over the canonical
//! binding, with a constant-time tag comparison
//! so `verify` never leaks tag bytes positionally under repeated probing.

use super::binding::{ActionBinding, encode};
use super::key::ReceiptKey;

/// A minted receipt: a human/model-visible [`token`](Receipt::token) plus
/// the full 32-byte tag `verify` compares against. Carries no secret
/// material — safe to log, to echo in a tool result, or to pass through
/// `redact::redact` unredacted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    pub token: String,
    tag: [u8; 32],
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Fold-based constant-time comparison: the loop always runs to full
/// length regardless of where the first mismatch occurs, so repeated
/// probing cannot recover tag bytes positionally via timing.
fn constant_time_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Compute the receipt for `binding` under `key`. BLAKE3's keyed mode is a
/// MAC by design — this is not a hash-then-truncate improvisation. The
/// 128-bit token truncation is deliberate: the adversary is an in-process
/// model with no key access and no verification oracle to grind against.
pub fn mint(key: &ReceiptKey, binding: &ActionBinding) -> Receipt {
    let buf = encode(binding);
    let hash = blake3::keyed_hash(key.as_bytes(), &buf);
    let tag = *hash.as_bytes();
    let token = format!(
        "cronus-rcpt-{:x}-{}",
        binding.timestamp_ms,
        hex(&tag[0..16])
    );
    Receipt { token, tag }
}

/// Recompute the tag over `binding` under `key` and compare in constant
/// time against `receipt`'s stored tag.
pub fn verify(key: &ReceiptKey, binding: &ActionBinding, receipt: &Receipt) -> bool {
    let buf = encode(binding);
    let hash = blake3::keyed_hash(key.as_bytes(), &buf);
    constant_time_eq(hash.as_bytes(), &receipt.tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXED_KEY: [u8; 32] = [42u8; 32];
    const PINNED_TS: u64 = 1_700_000_000_000;

    fn binding(action_id: u64) -> ActionBinding {
        ActionBinding {
            action_id,
            action_kind: "deploy".to_string(),
            inputs_digest: b"prod".to_vec(),
            outcome_tag: "ok".to_string(),
            result_digest: b"ok-digest".to_vec(),
            timestamp_ms: PINNED_TS,
        }
    }

    #[test]
    fn a_receipt_round_trips_through_verify() {
        let key = ReceiptKey::from_bytes(FIXED_KEY);
        let b = binding(1);
        let receipt = mint(&key, &b);
        assert!(verify(&key, &b, &receipt));
    }

    #[test]
    fn replay_two_calls_identical_except_action_id_produce_different_tags() {
        // Clock pinned deliberately: a test that lets the real millisecond
        // advance would pass for the wrong reason and stay green even if
        // `action_id` were dropped from the binding entirely.
        let key = ReceiptKey::from_bytes(FIXED_KEY);
        let first = binding(1);
        let second = binding(2);

        let receipt_one = mint(&key, &first);
        let receipt_two = mint(&key, &second);

        assert_ne!(receipt_one.token, receipt_two.token);
        assert!(
            !verify(&key, &second, &receipt_one),
            "an echoed prior receipt must not validate as a new invocation"
        );
    }

    #[test]
    fn altering_the_result_digest_alone_flips_verify_to_mismatch() {
        let key = ReceiptKey::from_bytes(FIXED_KEY);
        let original = binding(1);
        let receipt = mint(&key, &original);

        let mut tampered = original;
        tampered.result_digest = b"different-outcome".to_vec();
        assert!(!verify(&key, &tampered, &receipt));
    }

    #[test]
    fn a_receipt_minted_under_one_key_fails_verify_under_another() {
        let key_one = ReceiptKey::from_bytes([1u8; 32]);
        let key_two = ReceiptKey::from_bytes([2u8; 32]);
        let b = binding(1);
        let receipt = mint(&key_one, &b);
        assert!(!verify(&key_two, &b, &receipt));
    }

    #[test]
    fn the_token_matches_the_documented_shape() {
        let key = ReceiptKey::from_bytes(FIXED_KEY);
        let receipt = mint(&key, &binding(1));
        let re = regex_lite(&receipt.token);
        assert!(
            re,
            "token {:?} does not match ^cronus-rcpt-[0-9a-f]+-[0-9a-f]{{32}}$",
            receipt.token
        );
    }

    // No regex crate in this dependency-minimal domain module — a small
    // hand-written matcher over the one documented token shape is enough.
    fn regex_lite(token: &str) -> bool {
        let Some(rest) = token.strip_prefix("cronus-rcpt-") else {
            return false;
        };
        let Some((ts_part, tag_part)) = rest.split_once('-') else {
            return false;
        };
        !ts_part.is_empty()
            && ts_part.chars().all(|c| c.is_ascii_hexdigit())
            && tag_part.len() == 32
            && tag_part.chars().all(|c| c.is_ascii_hexdigit())
    }

    #[test]
    fn constant_time_eq_rejects_any_single_byte_difference() {
        let a = [5u8; 32];
        let mut b = a;
        b[31] = 6;
        assert!(!constant_time_eq(&a, &b));
        assert!(constant_time_eq(&a, &a));
    }
}
