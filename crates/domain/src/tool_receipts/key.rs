//! The ephemeral 32-byte MAC key: opaque by construction, with every
//! accidental leak path closed at the type level rather than left to
//! caller discipline.

use std::fmt;

/// A 32-byte keyed-BLAKE3 key. No `Display`, no `Serialize`, no `Clone`,
/// and no accessor returning the raw bytes outside this module — the only
/// way a caller reaches the key material is by holding a `&ReceiptKey` and
/// passing it to [`super::mint`] / [`super::verify`], which never move or
/// copy it out.
pub struct ReceiptKey([u8; 32]);

impl ReceiptKey {
    /// Construct from already-random bytes. Generating those bytes is the
    /// facade's job (`crates/core/src/receipts_bootstrap.rs`) — `getrandom`
    /// is deliberately absent from the domain boundary-guard allowlist, so
    /// this crate never reads entropy itself.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        ReceiptKey(bytes)
    }

    /// Module-scoped access to the raw bytes — reachable only from
    /// [`super::mac`], never re-exported past `tool_receipts`.
    pub(super) fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for ReceiptKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ReceiptKey(<redacted>)")
    }
}

impl Drop for ReceiptKey {
    fn drop(&mut self) {
        // Best-effort scrub: a volatile write the compiler may not elide.
        // This does not defeat a copy the optimizer already spilled into a
        // register or a `memcpy` temporary earlier in the key's life — that
        // residual is accepted because the threat model's adversary is the
        // in-context reasoning model, which reads prompts and tool results,
        // not process memory.
        for byte in self.0.iter_mut() {
            // SAFETY: `byte` is a valid `&mut u8` for the duration of the
            // call; `write_volatile` only prevents the store being elided.
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_is_the_fixed_redaction_placeholder_and_leaks_no_key_byte() {
        let key = ReceiptKey::from_bytes([0xABu8; 32]);
        let rendered = format!("{key:?}");
        assert_eq!(rendered, "ReceiptKey(<redacted>)");
        assert!(
            !rendered.contains("ab") && !rendered.contains("AB"),
            "no hex rendering of the key byte may leak into Debug output"
        );
    }

    #[test]
    fn as_bytes_returns_exactly_the_constructed_bytes() {
        let raw = [3u8; 32];
        let key = ReceiptKey::from_bytes(raw);
        assert_eq!(key.as_bytes(), &raw);
    }
}
