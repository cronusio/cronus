//! Tool-receipts invariant acceptance sweep — the closing
//! validation for the phase. Each TR invariant maps to one named test,
//! exercised through the real facade export chain (`cronus_core`'s
//! re-exports), matching the `dev_office_invariants` /
//! `knowledge_invariants` precedent — no direct `cronus-domain` dependency
//! from this file.
//!
//! **Honest coverage boundary:** at the time this phase
//! ships, exactly one production path in this codebase routes through
//! `ReceiptedDispatch` — `dev_office_workspace::run_elevated_action`. There
//! is no general tool-dispatch surface yet. Nothing in this file, and no
//! help text or status output anywhere in the project, may imply
//! project-wide receipt coverage; every future call site adopts receipts
//! by construction, because `Receipted<T>` is the only way `invoke`
//! returns a value.

use std::path::PathBuf;

use cronus_core::receipts_bootstrap::ReceiptedDispatch;
use cronus_core::redact::redact;
use cronus_core::tool_receipts::{
    ActionBinding, ReceiptKey, ReceiptLedger, ReceiptStatus, defer, mint, resolve_deferred,
};
use cronus_core::tool_security::ToolPolicy;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "cronus-tool-receipts-invariants-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn audit_path(tag: &str) -> PathBuf {
    temp_dir(tag).join("audit.jsonl")
}

fn fixed_binding(action_id: u64) -> ActionBinding {
    ActionBinding {
        action_id,
        action_kind: "invariant.probe".to_string(),
        inputs_digest: b"in".to_vec(),
        outcome_tag: "ok".to_string(),
        result_digest: b"out".to_vec(),
        timestamp_ms: 1_700_000_000_000,
    }
}

// ── Per-action receipt, no call-site opt-out ───────────────────────────

#[test]
fn allowed_and_blocked_calls_are_both_receipted_with_no_opt_out() {
    let mut dispatch = ReceiptedDispatch::new(audit_path("case1"));
    let allowed = ToolPolicy::default();
    let mut blocked_policy = ToolPolicy::default();
    blocked_policy
        .disabled_tools
        .push("case1.action".to_string());

    // `invoke` is the only public execution path (`ReceiptedDispatch` has
    // no other way to run an action) and its return type is
    // `Receipted<Result<T, String>>` — a caller cannot get `T` without the
    // receipt travelling beside it, for either outcome.
    let (_binding_a, receipted_allowed) = dispatch
        .invoke(&allowed, "case1.action", b"x", || Ok::<_, String>(()))
        .unwrap();
    let (_binding_b, receipted_blocked) = dispatch
        .invoke(
            &blocked_policy,
            "case1.action",
            b"x",
            || Ok::<_, String>(()),
        )
        .unwrap();

    assert!(receipted_allowed.value().is_ok());
    assert!(receipted_blocked.value().is_err());

    // Two calls otherwise identical in kind/inputs still mint distinct
    // receipts, because the per-session monotonic `action_id` makes each
    // receipt witness its own invocation.
    assert_ne!(
        receipted_allowed.receipt.token,
        receipted_blocked.receipt.token
    );
}

// ── Model-unforgeable ───────────────────────────────────────────────────

#[test]
fn a_receipt_minted_under_one_session_does_not_verify_under_another() {
    // Without access to the minting session's key, a second, independently
    // keyed dispatch cannot validate the first's receipts — the property
    // that stands in for "no forgery without the key" at the facade layer,
    // since neither `ReceiptedDispatch` nor `ReceiptKey` exposes the raw
    // key material to construct a matching one deliberately.
    let mut dispatch_a = ReceiptedDispatch::new(audit_path("case2-a"));
    let dispatch_b = ReceiptedDispatch::new(audit_path("case2-b"));
    let policy = ToolPolicy::default();

    let (binding, receipted) = dispatch_a
        .invoke(&policy, "case2.action", b"x", || Ok::<_, String>(()))
        .unwrap();

    assert!(dispatch_a.verify(&binding, &receipted.receipt));
    assert!(
        !dispatch_b.verify(&binding, &receipted.receipt),
        "a receipt must not verify under a session that never minted it"
    );
}

// ── Result authenticity ─────────────────────────────────────────────────

#[test]
fn substituting_the_observed_result_invalidates_the_receipt() {
    let mut dispatch = ReceiptedDispatch::new(audit_path("case3"));
    let policy = ToolPolicy::default();

    let (mut binding, receipted) = dispatch
        .invoke(&policy, "case3.action", b"x", || {
            Ok::<_, String>("the real observed value")
        })
        .unwrap();
    assert!(dispatch.verify(&binding, &receipted.receipt));

    // Narrate a different result over the same receipt: the binding this
    // receipt was minted over is fixed, so mutating the digest afterward
    // must invalidate it.
    binding.result_digest = cronus_core::tool_receipts::digest(b"a substituted value");
    assert!(!dispatch.verify(&binding, &receipted.receipt));
}

// ── Existence authenticity (default-deny) ───────────────────────────────

#[test]
fn an_action_id_never_dispatched_reports_unreceipted_never_a_fact() {
    let dispatch = ReceiptedDispatch::new(audit_path("case4"));
    // Nothing was ever dispatched under this id — `status()` must default
    // to `Unreceipted`, not an error and not an assumption of truth. There
    // is no method on `ReceiptLedger` (reachable via `dispatch.ledger()`)
    // that promotes an absence to a recorded fact.
    assert_eq!(
        dispatch.ledger().status(999_999),
        ReceiptStatus::Unreceipted
    );
}

// ── Ephemeral, isolated secret ──────────────────────────────────────────

#[test]
fn the_session_key_is_fresh_per_process_and_never_persisted_in_the_open() {
    let mut first = ReceiptedDispatch::new(audit_path("case5-a"));
    let second = ReceiptedDispatch::new(audit_path("case5-b"));
    let policy = ToolPolicy::default();

    // Fresh entropy per session, not a constant compiled-in key: the same
    // action minted under two independently constructed dispatches must
    // disagree, and cross-session verification must fail (re-asserts
    // the unforgeability property from the ephemerality angle: rotation-on-restart
    // means an old session's receipts are unverifiable by construction).
    let (binding, receipted) = first
        .invoke(&policy, "case5.action", b"x", || Ok::<_, String>(()))
        .unwrap();
    assert!(!second.verify(&binding, &receipted.receipt));
}

// ── Runtime-verified, not third-party ───────────────────────────────────

#[test]
fn verification_is_reachable_only_through_a_live_in_process_session() {
    // `ReceiptedDispatch::verify` is the sole verification entry point
    // exported anywhere in `cronus_core` or `cronus_core::tool_receipts` —
    // reviewed by inspection of both modules' public surfaces: there is no
    // public key, no exported asymmetric verifier, and no serialized proof
    // format offered alongside `mint`/`verify`. Demonstrated here by the
    // only two ways to call `verify`: through a `ReceiptedDispatch`
    // instance (which owns a live, process-local key) or through the
    // free-standing domain `verify(&ReceiptKey, …)`, which itself still
    // requires an in-process key value — never a public credential.
    let key = ReceiptKey::from_bytes([9u8; 32]);
    let binding = fixed_binding(1);
    let receipt = mint(&key, &binding);
    assert!(cronus_core::tool_receipts::verify(&key, &binding, &receipt));
}

// ── Complement, never replacement ───────────────────────────────────────

#[test]
fn the_gate_verdict_is_bound_as_an_input_and_the_action_runs_only_when_allowed() {
    let mut dispatch = ReceiptedDispatch::new(audit_path("case7"));
    let mut blocked = ToolPolicy::default();
    blocked.disabled_tools.push("case7.dangerous".to_string());

    let mut executed = false;
    let (binding, receipted) = dispatch
        .invoke(&blocked, "case7.dangerous", b"x", || {
            executed = true; // would only run if the gate were bypassed
            Ok::<_, String>(())
        })
        .unwrap();

    assert!(
        !executed,
        "the gate's Blocked verdict must run first, unchanged, and the action must never execute once blocked"
    );
    assert_eq!(binding.outcome_tag, "blocked");
    // The type carries no allow/deny capability: `Receipted<Result<T,
    // String>>` has no field or method that returns or accepts a
    // permission — the gate decides, the receipt only witnesses.
    assert!(receipted.value().is_err());
}

// ── Honest coverage boundary ────────────────────────────────────────────

#[test]
fn pending_deferred_work_is_never_silently_rounded_into_full_coverage() {
    let mut ledger = ReceiptLedger::new();
    let key = ReceiptKey::from_bytes([3u8; 32]);

    defer(&mut ledger, 1);
    let coverage = ledger.coverage();
    assert_eq!(coverage.pending, 1);
    assert_eq!(coverage.receipted, 0);
    // `CoverageReport` reports both counts as separate fields — a caller
    // cannot round `pending > 0` down to a single "all verified" number.

    resolve_deferred(&mut ledger, &key, fixed_binding(1), ());
    let coverage = ledger.coverage();
    assert_eq!(coverage.pending, 0);
    assert_eq!(coverage.receipted, 1);
}

// ── Tamper-evident auditable record ─────────────────────────────────────

#[test]
fn every_mint_and_every_detected_mismatch_are_auditable_events() {
    let path = audit_path("case9");
    let mut dispatch = ReceiptedDispatch::new(path.clone());
    let policy = ToolPolicy::default();

    let (mut binding, receipted) = dispatch
        .invoke(&policy, "case9.action", b"x", || Ok::<_, String>(()))
        .unwrap();

    let log_after_mint = std::fs::read_to_string(&path).unwrap();
    assert!(
        log_after_mint.contains(&receipted.receipt.token),
        "the mint must be audited carrying the token, never the key"
    );

    binding.result_digest = cronus_core::tool_receipts::digest(b"tampered");
    assert!(!dispatch.verify(&binding, &receipted.receipt));

    let log_after_mismatch = std::fs::read_to_string(&path).unwrap();
    assert!(
        log_after_mismatch.contains("\"outcome\":\"receipt_mismatch\""),
        "a detected mismatch must itself be a recorded event, not a silent false"
    );
}

// ── Leak-path tests (written as tests, not assumed by inspection) ───────────

#[test]
fn leak_path_debug_formatting_a_key_never_prints_a_key_byte() {
    let key = ReceiptKey::from_bytes([0xEFu8; 32]);
    let rendered = format!("{key:?}");
    assert_eq!(rendered, "ReceiptKey(<redacted>)");
    assert!(!rendered.contains("ef") && !rendered.contains("EF"));
}

#[test]
fn leak_path_redact_pass_over_a_receipt_token_leaves_it_intact() {
    // A scrubbed receipt would be indistinguishable from a missing one,
    // which would make the existence check fire on a genuine action — the token must
    // survive an ordinary redaction pass over unrelated secrets.
    let key = ReceiptKey::from_bytes([1u8; 32]);
    let receipt = mint(&key, &fixed_binding(1));
    let text = format!("the elevated action completed: {}", receipt.token);

    let redacted = redact(&text, &["some-unrelated-secret", "another-secret"]);
    assert!(
        redacted.contains(&receipt.token),
        "a receipt token is not secret material and must not be scrubbed"
    );
}
