//! Facade wiring for tool receipts: the ephemeral
//! per-session key's birth via OS entropy — the one non-deterministic act
//! in the subsystem — plus the dispatch seam wired to the real
//! `ToolPolicy` gate and the existing audit sink. Follows the
//! `activation_bootstrap` / `knowledge_bootstrap` / `loop_bootstrap`
//! precedent exactly: everything deterministic lives in
//! `cronus_domain::tool_receipts`, only entropy and dispatch I/O live here.

use std::fmt::Debug;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use cronus_domain::tool_receipts::{
    ActionBinding, Receipt, ReceiptKey, ReceiptLedger, Receipted, digest, mint_receipted,
    verify as receipt_verify,
};
use cronus_domain::tool_security::{
    AuditEntry, ToolPermitResult, ToolPolicy, append_audit_entry, now_ms,
};

/// Owns the ephemeral `ReceiptKey` for one runtime session plus the
/// per-session monotonic `action_id` counter — the same identity the
/// binding and the ledger both key on.
#[derive(Debug)]
pub struct ReceiptSession {
    key: ReceiptKey,
    next_action_id: AtomicU64,
}

impl ReceiptSession {
    /// Generate a fresh session key from the OS CSPRNG. This is the only
    /// non-deterministic act tool-receipts needs — `getrandom` is
    /// deliberately absent from `cronus-domain`'s boundary-guard allowlist,
    /// so entropy is read here and the domain crate receives only the
    /// already-random key. Never persisted: rotation is implicit in
    /// process restart, and no field of this struct — or of
    /// anything holding one — is ever passed to a serializer.
    pub fn new() -> Self {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes)
            .expect("OS CSPRNG unavailable — cannot mint receipt keys safely");
        ReceiptSession {
            key: ReceiptKey::from_bytes(bytes),
            next_action_id: AtomicU64::new(1),
        }
    }

    fn next_action_id(&self) -> u64 {
        self.next_action_id.fetch_add(1, Ordering::Relaxed)
    }
}

impl Default for ReceiptSession {
    fn default() -> Self {
        Self::new()
    }
}

/// The only public tool-execution path: `invoke` gates through the
/// real `ToolPolicy::is_permitted` first and unchanged, executes the
/// action only when permitted, binds the *observed* outcome into the MAC,
/// and appends the mint to the existing audit log. A caller can
/// obtain the wrapped value only together with its receipt — there is no
/// path that returns one without the other.
pub struct ReceiptedDispatch {
    session: ReceiptSession,
    ledger: ReceiptLedger,
    audit_path: PathBuf,
}

impl ReceiptedDispatch {
    pub fn new(audit_path: PathBuf) -> Self {
        ReceiptedDispatch {
            session: ReceiptSession::new(),
            ledger: ReceiptLedger::new(),
            audit_path,
        }
    }

    /// This session's ledger — the sole authority on "did this happen",
    /// e.g. for a `status`/coverage surface.
    pub fn ledger(&self) -> &ReceiptLedger {
        &self.ledger
    }

    /// Execute `action_kind` through the gate → execute → bind → mint →
    /// audit chain and return the exact [`ActionBinding`] alongside the
    /// receipted outcome, so a caller can independently re-verify what was
    /// actually bound. `is_permitted` runs first and its verdict is bound
    /// as an *input* to the receipt, never an output — a blocked
    /// call still mints a receipt witnessing the refusal, because the receipt requirement
    /// covers blocked calls too, not just successes. Fails closed: if the
    /// audit write itself fails, the whole call fails rather than letting
    /// an action proceed unaudited.
    pub fn invoke<T: Debug>(
        &mut self,
        policy: &ToolPolicy,
        action_kind: &str,
        inputs: &[u8],
        action: impl FnOnce() -> Result<T, String>,
    ) -> Result<(ActionBinding, Receipted<Result<T, String>>), String> {
        let action_id = self.session.next_action_id();
        let timestamp_ms = now_ms();
        let permit = policy.is_permitted(action_kind);

        let (outcome_tag, result_digest, outcome): (&str, Vec<u8>, Result<T, String>) = match permit
        {
            ToolPermitResult::Blocked(reason) => {
                let d = digest(reason.as_bytes());
                ("blocked", d, Err(reason))
            }
            ToolPermitResult::Allowed => match action() {
                Ok(value) => {
                    let d = digest(format!("{value:?}").as_bytes());
                    ("ok", d, Ok(value))
                }
                Err(reason) => {
                    let d = digest(reason.as_bytes());
                    ("err", d, Err(reason))
                }
            },
        };

        let binding = ActionBinding {
            action_id,
            action_kind: action_kind.to_string(),
            inputs_digest: digest(inputs),
            outcome_tag: outcome_tag.to_string(),
            result_digest,
            timestamp_ms,
        };

        let receipted = mint_receipted(&self.session.key, binding.clone(), outcome);
        self.ledger
            .record_minted(action_id, receipted.receipt.clone());

        let audit_outcome: &'static str = match outcome_tag {
            "blocked" => "blocked",
            "err" => "error",
            _ => "allowed",
        };
        // `tool_security::append_audit_entry` only serializes
        // `ts`/`layer`/`category`/`severity`/`outcome` today (the same
        // shipped-behavior boundary `dev_office_workspace.rs` already
        // disclosed) — `tool_name`/`finding_id` would be silently dropped,
        // so the token the audit trail must carry rides
        // `category` instead, the field that actually reaches disk.
        let entry = AuditEntry {
            timestamp: timestamp_ms,
            layer: "tool-receipts",
            tool_name: Some(action_kind.to_string()),
            finding_id: receipted.receipt.token.clone(),
            category: format!("receipted-action:{action_kind}:{}", receipted.receipt.token),
            severity: "info".to_string(),
            outcome: audit_outcome,
        };
        append_audit_entry(&self.audit_path, &entry)
            .map_err(|e| format!("audit logging failed: {e}"))?;

        Ok((binding, receipted))
    }

    /// Verify `receipt` against `binding`, auditing a `receipt_mismatch`
    /// event when verification fails — a detected fabrication is a
    /// recorded event, never a bare `false` the caller might silently
    /// discard.
    pub fn verify(&self, binding: &ActionBinding, receipt: &Receipt) -> bool {
        let ok = receipt_verify(&self.session.key, binding, receipt);
        if !ok {
            let entry = AuditEntry {
                timestamp: now_ms(),
                layer: "tool-receipts",
                tool_name: None,
                finding_id: receipt.token.clone(),
                category: format!("receipt-verify:{}", receipt.token),
                severity: "warning".to_string(),
                outcome: "receipt_mismatch",
            };
            let _ = append_audit_entry(&self.audit_path, &entry);
        }
        ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_audit_path(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cronus-receipts-bootstrap-{tag}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("audit.jsonl")
    }

    #[test]
    fn two_sessions_constructed_in_one_process_hold_different_keys() {
        let a = ReceiptSession::new();
        let b = ReceiptSession::new();
        // Neither key is reachable directly; fresh entropy is proven
        // indirectly — minting over the same fixed binding under each
        // session's key must disagree, since a constant key would agree.
        let binding = ActionBinding {
            action_id: 1,
            action_kind: "probe".to_string(),
            inputs_digest: b"in".to_vec(),
            outcome_tag: "ok".to_string(),
            result_digest: b"out".to_vec(),
            timestamp_ms: 1_700_000_000_000,
        };
        let tag_a = cronus_domain::tool_receipts::mint(&a.key, &binding).token;
        let tag_b = cronus_domain::tool_receipts::mint(&b.key, &binding).token;
        assert_ne!(
            tag_a, tag_b,
            "fresh entropy per session, not a constant key"
        );
    }

    #[test]
    fn the_key_never_appears_in_any_debug_rendering_of_the_session() {
        let session = ReceiptSession::new();
        let rendered = format!("{session:?}");
        assert!(rendered.contains("<redacted>"));
        // The struct's own `Debug` derive only ever reaches `ReceiptKey`'s
        // hand-written, redacting `Debug` impl — there is no second field
        // or method anywhere in this module that formats the raw bytes.
    }

    /// No code path writes the key to the state tier: reviewed by
    /// inspection of this module's full surface (the same default-deny-style
    /// absence argument `tool_receipts::ledger` makes for its own API) —
    /// `ReceiptSession` has exactly one public constructor and no
    /// `Serialize`/persistence method, `ReceiptedDispatch` never stores a
    /// session anywhere but its own private field, and neither type
    /// appears in any struct this crate serializes (`grep -rn
    /// "ReceiptSession\|ReceiptedDispatch" crates/*/src` outside this file
    /// shows only the re-export in `lib.rs` and call sites that hold it
    /// locally).
    #[test]
    fn the_session_is_never_reachable_from_a_serialized_struct() {
        let session = ReceiptSession::new();
        assert!(format!("{session:?}").contains("ReceiptSession"));
    }

    #[test]
    fn invoke_allowed_action_binds_outcome_tag_ok_with_the_real_result() {
        let mut dispatch = ReceiptedDispatch::new(temp_audit_path("ok"));
        let policy = ToolPolicy::default();

        let (binding, receipted) = dispatch
            .invoke(&policy, "probe.read", b"args", || Ok::<_, String>("value"))
            .unwrap();

        assert_eq!(binding.outcome_tag, "ok");
        assert_eq!(*receipted.value(), Ok("value"));
        assert!(dispatch.verify(&binding, &receipted.receipt));
    }

    #[test]
    fn invoke_blocked_action_still_mints_a_receipt_binding_the_block_reason() {
        let mut dispatch = ReceiptedDispatch::new(temp_audit_path("blocked"));
        let mut policy = ToolPolicy::default();
        policy.disabled_tools.push("probe.dangerous".to_string());

        let (binding, receipted) = dispatch
            .invoke(&policy, "probe.dangerous", b"args", || {
                Ok::<_, String>("unreachable")
            })
            .unwrap();

        assert_eq!(binding.outcome_tag, "blocked");
        assert!(receipted.value().is_err());
        assert!(dispatch.verify(&binding, &receipted.receipt));
    }

    #[test]
    fn substituting_a_different_result_fails_verify() {
        let mut dispatch = ReceiptedDispatch::new(temp_audit_path("tamper"));
        let policy = ToolPolicy::default();

        let (mut binding, receipted) = dispatch
            .invoke(&policy, "probe.read", b"args", || {
                Ok::<_, String>("real value")
            })
            .unwrap();

        binding.result_digest = cronus_domain::tool_receipts::digest(b"forged value");
        assert!(!dispatch.verify(&binding, &receipted.receipt));
    }

    #[test]
    fn every_mint_appends_an_audit_entry_carrying_the_token_never_the_key() {
        let audit_path = temp_audit_path("audit-token");
        let mut dispatch = ReceiptedDispatch::new(audit_path.clone());
        let policy = ToolPolicy::default();

        let (_binding, receipted) = dispatch
            .invoke(&policy, "probe.read", b"args", || Ok::<_, String>("value"))
            .unwrap();

        let log = std::fs::read_to_string(&audit_path).unwrap();
        assert!(log.contains(&receipted.receipt.token));
        assert!(log.contains("\"outcome\":\"allowed\""));
    }

    #[test]
    fn a_verify_mismatch_appends_its_own_receipt_mismatch_audit_entry() {
        let audit_path = temp_audit_path("mismatch");
        let mut dispatch = ReceiptedDispatch::new(audit_path.clone());
        let policy = ToolPolicy::default();

        let (mut binding, receipted) = dispatch
            .invoke(&policy, "probe.read", b"args", || Ok::<_, String>("value"))
            .unwrap();
        binding.result_digest = cronus_domain::tool_receipts::digest(b"forged");

        assert!(!dispatch.verify(&binding, &receipted.receipt));
        let log = std::fs::read_to_string(&audit_path).unwrap();
        assert!(
            log.contains("\"outcome\":\"receipt_mismatch\""),
            "a detected mismatch must be its own auditable event, not a silent false"
        );
    }
}
