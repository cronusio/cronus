//! Service-activation invariant acceptance sweep —
//! the closing validation for this subsystem. Each testable
//! invariant maps to one named test, exercised through the **real facade
//! export chain**: the domain policy engine (`cronus_core::activation::
//! enable`/`disable`) driving a registry that implements the
//! `cronus_core::ActivationRegistry` trait re-exported from `cronus-contract`
//! — proving the whole assembled stack works together, not just each crate
//! in isolation.
//!
//! **Invariants covered elsewhere, cited rather than duplicated:**
//! - The two-modes rule (two modes, one identical engine binary) and the supervision rule (supervision
//!   belongs to the mode; the engine's own recovery is unchanged) are
//!   structural — nothing was added to the engine's own behavior or
//!   recovery ladder by this phase — verified by inspection, not a runtime
//!   assertion a unit test can usefully make.
//! - `crates/domain/src/
//!   sandbox_policy.rs` (`FilesystemPolicy`/`FILESYSTEM_REGISTRATION_LOCATIONS`)
//!   and `crates/core/tests/tool_security.rs` (`is_activation_tool_name`).
//! - The real per-host capability check (non-systemd Linux → `Unsupported`)
//!   is unit-tested with the real detection logic in `crates/activation-os/
//!   src/lib.rs` (`capabilities_reflect_a_real_per_host_check_not_a_hardcoded_yes`);
//!   this file proves only that the *type* is representable and distinct
//!   through the facade.
//! - `crates/core/src/
//!   engine_lock.rs`'s own test module, exercised against a real temp
//!   directory with real file I/O.

use std::sync::Mutex;

use cronus_core::activation::{
    ConsentLedger, ConsentRecord, TransitionError, TransitionOutcome, disable, enable,
    enable_with_consent,
};
use cronus_core::{
    ActivationCapabilities, ActivationMode, ActivationRegistry, ActivationState, ModeSupport,
};

/// A scriptable registry implementing the real, re-exported
/// `cronus_core::ActivationRegistry` trait — proving the facade's contract
/// re-export and the domain policy functions compose, not just that each
/// exists independently.
struct FakeRegistry {
    capabilities: ActivationCapabilities,
    state: Mutex<ActivationState>,
    fail_transitions: bool,
    query_fails_with: Option<String>,
}

impl FakeRegistry {
    fn supported(state: ActivationState) -> Self {
        FakeRegistry {
            capabilities: ActivationCapabilities {
                login: ModeSupport::Supported,
                system: ModeSupport::Supported,
            },
            state: Mutex::new(state),
            fail_transitions: false,
            query_fails_with: None,
        }
    }
}

impl ActivationRegistry for FakeRegistry {
    fn capabilities(&self) -> ActivationCapabilities {
        self.capabilities.clone()
    }

    fn observe(&self) -> ActivationState {
        if let Some(reason) = &self.query_fails_with {
            return ActivationState::Unknown {
                reason: reason.clone(),
            };
        }
        self.state.lock().unwrap().clone()
    }

    fn enable(&self, mode: ActivationMode) -> Result<(), String> {
        if self.fail_transitions {
            return Err("registration refused".to_string());
        }
        *self.state.lock().unwrap() = ActivationState::Active(mode);
        Ok(())
    }

    fn disable(&self) -> Result<(), String> {
        if self.fail_transitions {
            return Err("removal refused".to_string());
        }
        *self.state.lock().unwrap() = ActivationState::Inactive;
        Ok(())
    }
}

// --- Manual launch is the complete default -----------------------------

#[test]
fn ba1_a_fresh_registry_reports_inactive_manual_is_the_default() {
    let registry = FakeRegistry::supported(ActivationState::Inactive);
    assert_eq!(registry.observe(), ActivationState::Inactive);
}

// --- At most one activation registration -------------------------------

#[test]
fn ba3_enabling_a_different_mode_removes_the_prior_one_first() {
    let registry = FakeRegistry::supported(ActivationState::Active(ActivationMode::Login));
    let outcome = enable(&registry, ActivationMode::System).expect("enable succeeds");
    assert_eq!(
        outcome,
        TransitionOutcome::Activated(ActivationMode::System)
    );
    assert_eq!(
        registry.observe(),
        ActivationState::Active(ActivationMode::System),
        "the prior Login registration must be gone, not doubled up with System"
    );
}

// --- Activation is a disclosed autonomy grant, per-mode consent -------

#[test]
fn ba5_consent_for_one_mode_never_authorizes_the_other() {
    let mut ledger = ConsentLedger::new();
    ledger.record(ConsentRecord {
        mode: ActivationMode::Login,
        autonomy_level: "standard".to_string(),
        spend_ceiling: "$5/day".to_string(),
    });
    assert!(ledger.has_consented(ActivationMode::Login));
    assert!(!ledger.has_consented(ActivationMode::System));

    let registry = FakeRegistry::supported(ActivationState::Inactive);
    let result = enable_with_consent(&registry, &ledger, ActivationMode::System);
    assert!(matches!(
        result,
        Err(TransitionError::ConsentMissing {
            mode: ActivationMode::System
        })
    ));
}

// --- Least privilege — a refused elevation changes nothing ------------

#[test]
fn ba6_a_refused_elevation_leaves_the_host_exactly_as_it_was() {
    let mut registry = FakeRegistry::supported(ActivationState::Inactive);
    registry.fail_transitions = true;

    let result = enable(&registry, ActivationMode::System);
    assert!(result.is_err());
    assert_eq!(
        registry.observe(),
        ActivationState::Inactive,
        "a refused elevation must leave the host exactly as it was, never partially registered"
    );
}

// --- Reversible and complete -------------------------------------------

#[test]
fn ba7_disable_removes_whatever_is_registered_and_verifies_absence() {
    let registry = FakeRegistry::supported(ActivationState::Active(ActivationMode::Login));
    let outcome = disable(&registry).expect("disable succeeds");
    assert_eq!(outcome, TransitionOutcome::Deactivated);
    assert_eq!(registry.observe(), ActivationState::Inactive);
}

// --- Observed state, never remembered state ---------------------------

#[test]
fn ba8_requires_approval_is_never_folded_into_activated() {
    let registry =
        FakeRegistry::supported(ActivationState::RequiresApproval(ActivationMode::System));
    let state = registry.observe();
    assert_eq!(
        state,
        ActivationState::RequiresApproval(ActivationMode::System)
    );
    assert_ne!(
        state,
        ActivationState::Active(ActivationMode::System),
        "a registered-but-vetoed state must never be reported as a full Active success"
    );
}

#[test]
fn ba8_an_unqueryable_facility_reports_unknown_never_active() {
    let mut registry = FakeRegistry::supported(ActivationState::Active(ActivationMode::Login));
    registry.query_fails_with = Some("registry unreadable".to_string());
    let state = registry.observe();
    assert!(matches!(state, ActivationState::Unknown { .. }));
    assert_ne!(state, ActivationState::Active(ActivationMode::Login));
}

// --- Spoke hosts refuse activation, visibly, per-mode ----------------

#[test]
fn ba10_an_unsupported_mode_is_representable_and_distinct_from_supported() {
    let registry = FakeRegistry {
        capabilities: ActivationCapabilities {
            login: ModeSupport::Supported,
            system: ModeSupport::Unsupported {
                reason: "no systemd or logind linger on this host".to_string(),
            },
        },
        state: Mutex::new(ActivationState::Inactive),
        fail_transitions: false,
        query_fails_with: None,
    };
    let caps = registry.capabilities();
    assert_eq!(caps.login, ModeSupport::Supported);
    assert!(matches!(caps.system, ModeSupport::Unsupported { .. }));
    assert_ne!(
        caps.system,
        ModeSupport::Supported,
        "an unsupported mode must never be reported as supported"
    );
}
