//! Activation policy — the domain-tier transition state machine over the
//! `ActivationRegistry` seam.
//! The seam's `enable`/`disable` are adapter-level primitives
//! (register exactly this mode / remove whatever is registered); this module
//! owns the *policy* — the mutual-exclusion ordering, the read-back
//! verification that makes a failure converge on `Inactive` rather than a
//! doubly-registered or silently-wrong state, and per-mode consent
//! bookkeeping. The adapter never decides any of this; it only executes the
//! OS calls this module directs it to.

use cronus_contract::{ActivationMode, ActivationRegistry, ActivationState};

/// What a transition accomplished. `RequiresApproval` is its own variant,
/// distinct from `Activated` — the registry registered the mode, but the OS
/// (or the user) has not yet approved it, so a caller must never treat it as
/// a full success.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionOutcome {
    Activated(ActivationMode),
    Deactivated,
    RequiresApproval(ActivationMode),
}

/// A transition that did not converge on the requested state. Every path
/// that returns this leaves the registry at `Inactive` (its default) or
/// unchanged from before the call — never a half-completed, doubly-
/// registered state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionError {
    /// Removing the prior mode did not verify absent; the target was never
    /// registered.
    PriorModeNotRemoved { observed: ActivationState },
    /// The adapter's `enable`/`disable` call itself returned an error
    /// (elevation refused, an OS call failed, ...).
    RegistrationFailed { detail: String },
    /// The adapter reported success, but the read-back does not show the
    /// target state — a claimed success is never trusted over the
    /// observation.
    VerificationFailed { observed: ActivationState },
    /// The user has not consented to this specific mode; consent
    /// never carries over from another mode.
    ConsentMissing { mode: ActivationMode },
}

/// Enable `target` on `registry`: if a *different*
/// mode is currently active or pending approval, remove it first and verify
/// its absence before registering the target; then verify the target is
/// observably active. `RequiresApproval` surfaces as its own outcome, never
/// folded into `Activated`.
pub fn enable(
    registry: &dyn ActivationRegistry,
    target: ActivationMode,
) -> Result<TransitionOutcome, TransitionError> {
    // Mutual exclusion: remove whatever else is registered first.
    match registry.observe() {
        ActivationState::Active(current) | ActivationState::RequiresApproval(current)
            if current != target =>
        {
            registry
                .disable()
                .map_err(|detail| TransitionError::RegistrationFailed { detail })?;
            match registry.observe() {
                ActivationState::Inactive => {}
                observed => return Err(TransitionError::PriorModeNotRemoved { observed }),
            }
        }
        _ => {} // Already Inactive, or already the target mode — nothing to remove.
    }

    // Register the target (may prompt for elevation on System).
    registry
        .enable(target)
        .map_err(|detail| TransitionError::RegistrationFailed { detail })?;

    // Read-back verification: a claimed success is not trusted
    // until observed.
    match registry.observe() {
        ActivationState::Active(mode) if mode == target => Ok(TransitionOutcome::Activated(target)),
        ActivationState::RequiresApproval(mode) if mode == target => {
            Ok(TransitionOutcome::RequiresApproval(target))
        }
        observed => Err(TransitionError::VerificationFailed { observed }),
    }
}

/// Disable whatever is registered: remove, then verify absence.
pub fn disable(registry: &dyn ActivationRegistry) -> Result<TransitionOutcome, TransitionError> {
    registry
        .disable()
        .map_err(|detail| TransitionError::RegistrationFailed { detail })?;

    match registry.observe() {
        ActivationState::Inactive => Ok(TransitionOutcome::Deactivated),
        observed => Err(TransitionError::VerificationFailed { observed }),
    }
}

/// What the user was shown and confirmed when granting a mode: the
/// consent moment names the autonomy level and spend ceiling in force.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsentRecord {
    pub mode: ActivationMode,
    pub autonomy_level: String,
    pub spend_ceiling: String,
}

/// Tracks which modes the user has actually consented to. A fresh
/// ledger authorizes nothing — consent is explicit and per-mode, never
/// assumed or inherited: a record for `Login` does not authorize `System`.
#[derive(Debug, Default)]
pub struct ConsentLedger {
    granted: Vec<ConsentRecord>,
}

impl ConsentLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that the user was shown and confirmed the disclosure for the
    /// mode named in `record`.
    pub fn record(&mut self, record: ConsentRecord) {
        self.granted.push(record);
    }

    /// Whether the user has consented to exactly this mode.
    pub fn has_consented(&self, mode: ActivationMode) -> bool {
        self.granted.iter().any(|r| r.mode == mode)
    }
}

/// Enable `target`, gated on the user having separately consented to it.
/// `enable` alone enforces the mutual-exclusion, elevation and verified-removal rules; this wraps it so a caller
/// can never activate a mode the disclosure was never shown for.
pub fn enable_with_consent(
    registry: &dyn ActivationRegistry,
    ledger: &ConsentLedger,
    target: ActivationMode,
) -> Result<TransitionOutcome, TransitionError> {
    if !ledger.has_consented(target) {
        return Err(TransitionError::ConsentMissing { mode: target });
    }
    enable(registry, target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cronus_contract::{ActivationCapabilities, ModeSupport};

    /// Records every seam call in order and lets a test script `enable`/
    /// `disable` to fail, to lie about success (report `Ok` without moving
    /// the observed state), or to land on `RequiresApproval` instead of
    /// `Active` — enough to drive every branch of the policy engine with no
    /// real OS call.
    struct FakeRegistry {
        state: std::sync::Mutex<ActivationState>,
        calls: std::sync::Mutex<Vec<&'static str>>,
        fail_enable: bool,
        fail_disable: bool,
        lie_on_enable: bool,
        require_approval_on_enable: bool,
    }

    impl FakeRegistry {
        fn new(state: ActivationState) -> Self {
            FakeRegistry {
                state: std::sync::Mutex::new(state),
                calls: std::sync::Mutex::new(Vec::new()),
                fail_enable: false,
                fail_disable: false,
                lie_on_enable: false,
                require_approval_on_enable: false,
            }
        }

        fn calls(&self) -> Vec<&'static str> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl ActivationRegistry for FakeRegistry {
        fn capabilities(&self) -> ActivationCapabilities {
            ActivationCapabilities {
                login: ModeSupport::Supported,
                system: ModeSupport::Supported,
            }
        }

        fn observe(&self) -> ActivationState {
            self.calls.lock().unwrap().push("observe");
            self.state.lock().unwrap().clone()
        }

        fn enable(&self, mode: ActivationMode) -> Result<(), String> {
            self.calls.lock().unwrap().push("enable");
            if self.fail_enable {
                return Err("enable refused".to_string());
            }
            if !self.lie_on_enable {
                let landed = if self.require_approval_on_enable {
                    ActivationState::RequiresApproval(mode)
                } else {
                    ActivationState::Active(mode)
                };
                *self.state.lock().unwrap() = landed;
            }
            Ok(())
        }

        fn disable(&self) -> Result<(), String> {
            self.calls.lock().unwrap().push("disable");
            if self.fail_disable {
                return Err("disable refused".to_string());
            }
            *self.state.lock().unwrap() = ActivationState::Inactive;
            Ok(())
        }
    }

    #[test]
    fn enabling_a_different_mode_removes_and_verifies_the_prior_mode_first() {
        let fake = FakeRegistry::new(ActivationState::Active(ActivationMode::Login));

        let outcome = enable(&fake, ActivationMode::System).expect("enable succeeds");
        assert_eq!(
            outcome,
            TransitionOutcome::Activated(ActivationMode::System)
        );

        // `disable` (removing Login) must precede `enable` (registering System).
        let calls = fake.calls();
        let disable_idx = calls
            .iter()
            .position(|c| *c == "disable")
            .expect("disable was called");
        let enable_idx = calls
            .iter()
            .position(|c| *c == "enable")
            .expect("enable was called");
        assert!(
            disable_idx < enable_idx,
            "the prior mode must be removed before the target is registered"
        );
    }

    #[test]
    fn a_forced_registration_failure_leaves_the_host_inactive() {
        // Removing System succeeds, but registering Login then fails — the
        // host must land at Inactive (the default), never back at System
        // and never a half-registered Login.
        let mut fake = FakeRegistry::new(ActivationState::Active(ActivationMode::System));
        fake.fail_enable = true;

        let result = enable(&fake, ActivationMode::Login);
        assert!(matches!(
            result,
            Err(TransitionError::RegistrationFailed { .. })
        ));
        assert_eq!(
            fake.observe(),
            ActivationState::Inactive,
            "a failed registration must not leave the prior mode active nor a half-registered target"
        );
    }

    #[test]
    fn requires_approval_is_a_distinct_outcome_never_folded_into_activated() {
        let mut fake = FakeRegistry::new(ActivationState::Inactive);
        fake.require_approval_on_enable = true;

        let outcome =
            enable(&fake, ActivationMode::System).expect("the enable call itself succeeds");
        assert_eq!(
            outcome,
            TransitionOutcome::RequiresApproval(ActivationMode::System)
        );
        assert_ne!(
            outcome,
            TransitionOutcome::Activated(ActivationMode::System),
            "RequiresApproval must never be reported as a full Activated success"
        );
    }

    #[test]
    fn a_lying_registration_success_is_caught_by_readback_verification() {
        // The adapter's enable() call returns Ok, but the observed state
        // never actually moved — this must surface as VerificationFailed,
        // never as a trusted Activated (observation, not a
        // claimed success, is the truth).
        let mut fake = FakeRegistry::new(ActivationState::Inactive);
        fake.lie_on_enable = true;

        let result = enable(&fake, ActivationMode::Login);
        assert!(matches!(
            result,
            Err(TransitionError::VerificationFailed { .. })
        ));
    }

    #[test]
    fn disable_removes_and_verifies_absence() {
        let fake = FakeRegistry::new(ActivationState::Active(ActivationMode::Login));
        let outcome = disable(&fake).expect("disable succeeds");
        assert_eq!(outcome, TransitionOutcome::Deactivated);
        assert_eq!(fake.observe(), ActivationState::Inactive);
    }

    #[test]
    fn consent_for_one_mode_does_not_authorize_the_other() {
        let mut ledger = ConsentLedger::new();
        ledger.record(ConsentRecord {
            mode: ActivationMode::Login,
            autonomy_level: "standard".to_string(),
            spend_ceiling: "$5/day".to_string(),
        });

        assert!(ledger.has_consented(ActivationMode::Login));
        assert!(
            !ledger.has_consented(ActivationMode::System),
            "Login consent must not imply System consent"
        );

        let fake = FakeRegistry::new(ActivationState::Inactive);
        let result = enable_with_consent(&fake, &ledger, ActivationMode::System);
        assert!(matches!(
            result,
            Err(TransitionError::ConsentMissing {
                mode: ActivationMode::System
            })
        ));
    }
}
