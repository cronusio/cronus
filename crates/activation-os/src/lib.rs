//! Per-OS activation-registration adapter,
//! implementing `cronus_contract::ActivationRegistry`. Minted per
//! OS registration opens registries, writes
//! plists/unit files, and shells out to service managers — infrastructure by
//! every definition, never domain logic.
//!
//! Testability discipline: raw OS primitives sit behind the [`SystemCalls`]
//! seam, so the adapter's LOGIC — weaker-wins `observe()` derivation, both
//! mode's registration sequences — is unit-tested against a scriptable fake
//! on any host, independent of which platform module backs it. Covers
//! login-scoped registration (Track D01) and system-scoped elevation
//! (Track D02) over the same seam.
//!
//! **Verification scope, disclosed:** the Windows implementation
//! (`windows_calls`) is compiled and its adapter logic tested on the host
//! this was written on; its real elevation ceremony (`ShellExecuteExW` with
//! the `runas` verb) is compiled but never invoked in this session — that
//! would pop a real interactive UAC dialog, unsuitable for an automated
//! run. The Linux (`linux_calls`) and macOS (`macos_calls`) implementations
//! are cross-compile type-checked (`cargo check --target
//! x86_64-unknown-linux-gnu` / `--target aarch64-apple-darwin`) but not run —
//! no Linux or macOS host was available. Real-world validation on those
//! platforms, and of Windows's live elevation path, is deferred.

use cronus_contract::{
    ActivationCapabilities, ActivationMode, ActivationRegistry, ActivationState, ModeSupport,
};

#[cfg(target_os = "linux")]
mod linux_calls;
#[cfg(target_os = "macos")]
mod macos_calls;
#[cfg(windows)]
mod windows_calls;

#[cfg(target_os = "linux")]
pub use linux_calls::LinuxSystemCalls;
#[cfg(target_os = "macos")]
pub use macos_calls::MacosSystemCalls;
#[cfg(windows)]
pub use windows_calls::WindowsSystemCalls;

/// The raw OS primitives a login-scoped adapter needs, factored out so the
/// adapter's decision logic — what to read, in what order, how to reconcile
/// a raw entry against the platform's own veto — is testable without a real
/// OS call (the weaker-wins rule: where the OS distinguishes *registered*
/// from *effective*, the weaker one wins).
pub trait SystemCalls: Send + Sync {
    /// Whether the login-scoped facility's raw entry exists.
    fn login_entry_present(&self) -> Result<bool, String>;
    /// The platform's own veto over that raw entry, if the platform has one
    /// (Windows `StartupApproved`, the `.desktop` `Hidden`/
    /// `X-GNOME-Autostart-enabled` keys). `None` when there is no separate
    /// veto signal to check — never fabricated as "not vetoed".
    fn login_entry_vetoed(&self) -> Result<Option<bool>, String>;
    /// Write the login-scoped entry (unelevated).
    fn write_login_entry(&self) -> Result<(), String>;
    /// Remove the login-scoped entry. A no-op `Ok(())` if already absent.
    fn remove_login_entry(&self) -> Result<(), String>;

    /// Whether the system-scoped facility's raw entry exists.
    fn system_entry_present(&self) -> Result<bool, String>;
    /// The platform's own veto over the system-scoped entry, if any.
    fn system_entry_vetoed(&self) -> Result<Option<bool>, String>;
    /// Register the system-scoped entry. Requires an OS-mediated elevation
    /// ceremony the human performs and can refuse — a refused or
    /// failed elevation returns `Err` and must leave no partial
    /// registration behind.
    fn write_system_entry(&self) -> Result<(), String>;
    /// Remove the system-scoped entry. A no-op `Ok(())` if already absent.
    fn remove_system_entry(&self) -> Result<(), String>;

    /// Remove every artifact — both modes, every sub-variant a given
    /// platform may have used ("all four locations") — regardless of
    /// which mode `observe()` currently reports active, so an orphan left by
    /// a previously-failed mode switch is still found. Never touches
    /// anything but this adapter's own well-known name.
    fn uninstall_all(&self) -> Result<(), String>;

    /// Whether THIS host can actually offer login-scoped activation:
    /// a real, checked capability — not "we would technically try" — so a
    /// host with no usable facility is reported `Unsupported` with a reason
    /// before the user ever presses enable, never discovered only when
    /// `enable` fails.
    fn login_capability(&self) -> ModeSupport;
    /// Whether THIS host can actually offer system-scoped activation.
    fn system_capability(&self) -> ModeSupport;
}

/// `observe()`'s weaker-wins derivation: an entry present but vetoed
/// is `RequiresApproval`, present and not (verifiably) vetoed is `Active`,
/// absent is `Inactive`. Pure function of the two facts plus which mode they
/// describe — the same rule every platform's real [`SystemCalls`] impl
/// feeds into.
fn derive_mode_state(present: bool, vetoed: Option<bool>, mode: ActivationMode) -> ActivationState {
    if !present {
        return ActivationState::Inactive;
    }
    match vetoed {
        Some(true) => ActivationState::RequiresApproval(mode),
        _ => ActivationState::Active(mode),
    }
}

/// The activation adapter (login-scoped + system-scoped):
/// wraps a [`SystemCalls`] implementation as a full `ActivationRegistry`
/// over both modes.
pub struct OsActivationAdapter<C: SystemCalls> {
    calls: C,
}

impl<C: SystemCalls> OsActivationAdapter<C> {
    pub fn new(calls: C) -> Self {
        OsActivationAdapter { calls }
    }

    /// Remove every artifact this adapter's label could occupy across
    /// both modes. A product lifecycle event (uninstall), not an activation
    /// transition — deliberately not part of `ActivationRegistry` itself.
    pub fn uninstall(&self) -> Result<(), String> {
        self.calls.uninstall_all()
    }
}

impl<C: SystemCalls> ActivationRegistry for OsActivationAdapter<C> {
    fn capabilities(&self) -> ActivationCapabilities {
        ActivationCapabilities {
            login: self.calls.login_capability(),
            system: self.calls.system_capability(),
        }
    }

    fn observe(&self) -> ActivationState {
        // Defensive tie-break: System takes priority if somehow both are
        // present. Domain-tier mutual exclusion (crate `cronus-domain`
        // `activation::enable`) should prevent this in normal operation;
        // reporting the stronger claim rather than silently picking one is
        // the honest choice if an out-of-band edit ever causes it.
        let system_present = match self.calls.system_entry_present() {
            Ok(p) => p,
            Err(reason) => return ActivationState::Unknown { reason },
        };
        if system_present {
            let vetoed = match self.calls.system_entry_vetoed() {
                Ok(v) => v,
                Err(reason) => return ActivationState::Unknown { reason },
            };
            return derive_mode_state(true, vetoed, ActivationMode::System);
        }

        let login_present = match self.calls.login_entry_present() {
            Ok(p) => p,
            Err(reason) => return ActivationState::Unknown { reason },
        };
        let login_vetoed = match self.calls.login_entry_vetoed() {
            Ok(v) => v,
            Err(reason) => return ActivationState::Unknown { reason },
        };
        derive_mode_state(login_present, login_vetoed, ActivationMode::Login)
    }

    fn enable(&self, mode: ActivationMode) -> Result<(), String> {
        match mode {
            ActivationMode::Login => self.calls.write_login_entry(),
            ActivationMode::System => self.calls.write_system_entry(),
        }
    }

    fn disable(&self) -> Result<(), String> {
        // Remove whichever is present; both calls are no-ops if absent, so
        // this also mops up an orphan in the "other" mode as a bonus.
        self.calls.remove_login_entry()?;
        self.calls.remove_system_entry()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A scriptable `SystemCalls` for tests: presence/veto for each mode are
    /// set at construction and mutated through `write`/`remove`;
    /// `query_fails_with` forces `Err` from every read; `fail_system_write`
    /// scripts a refused/failed elevation without mutating state —
    /// enough to drive the adapter's weaker-wins derivation and both modes'
    /// transitions with no real OS call.
    struct FakeSystemCalls {
        login_present: Mutex<bool>,
        login_vetoed: Mutex<Option<bool>>,
        system_present: Mutex<bool>,
        system_vetoed: Mutex<Option<bool>>,
        query_fails_with: Mutex<Option<String>>,
        fail_system_write: Mutex<bool>,
        login_capability: ModeSupport,
        system_capability: ModeSupport,
    }

    impl FakeSystemCalls {
        fn new(login_present: bool, login_vetoed: Option<bool>) -> Self {
            FakeSystemCalls {
                login_present: Mutex::new(login_present),
                login_vetoed: Mutex::new(login_vetoed),
                system_present: Mutex::new(false),
                system_vetoed: Mutex::new(None),
                query_fails_with: Mutex::new(None),
                fail_system_write: Mutex::new(false),
                login_capability: ModeSupport::Supported,
                system_capability: ModeSupport::Supported,
            }
        }
    }

    impl SystemCalls for FakeSystemCalls {
        fn login_entry_present(&self) -> Result<bool, String> {
            if let Some(reason) = self.query_fails_with.lock().unwrap().clone() {
                return Err(reason);
            }
            Ok(*self.login_present.lock().unwrap())
        }

        fn login_entry_vetoed(&self) -> Result<Option<bool>, String> {
            if let Some(reason) = self.query_fails_with.lock().unwrap().clone() {
                return Err(reason);
            }
            Ok(*self.login_vetoed.lock().unwrap())
        }

        fn write_login_entry(&self) -> Result<(), String> {
            *self.login_present.lock().unwrap() = true;
            *self.login_vetoed.lock().unwrap() = None;
            Ok(())
        }

        fn remove_login_entry(&self) -> Result<(), String> {
            *self.login_present.lock().unwrap() = false;
            Ok(())
        }

        fn system_entry_present(&self) -> Result<bool, String> {
            if let Some(reason) = self.query_fails_with.lock().unwrap().clone() {
                return Err(reason);
            }
            Ok(*self.system_present.lock().unwrap())
        }

        fn system_entry_vetoed(&self) -> Result<Option<bool>, String> {
            if let Some(reason) = self.query_fails_with.lock().unwrap().clone() {
                return Err(reason);
            }
            Ok(*self.system_vetoed.lock().unwrap())
        }

        fn write_system_entry(&self) -> Result<(), String> {
            if *self.fail_system_write.lock().unwrap() {
                return Err("elevation refused".to_string());
            }
            *self.system_present.lock().unwrap() = true;
            *self.system_vetoed.lock().unwrap() = None;
            Ok(())
        }

        fn remove_system_entry(&self) -> Result<(), String> {
            *self.system_present.lock().unwrap() = false;
            Ok(())
        }

        fn uninstall_all(&self) -> Result<(), String> {
            *self.login_present.lock().unwrap() = false;
            *self.system_present.lock().unwrap() = false;
            Ok(())
        }

        fn login_capability(&self) -> ModeSupport {
            self.login_capability.clone()
        }

        fn system_capability(&self) -> ModeSupport {
            self.system_capability.clone()
        }
    }

    #[test]
    fn observe_derives_inactive_when_no_entry_present() {
        let adapter = OsActivationAdapter::new(FakeSystemCalls::new(false, None));
        assert_eq!(adapter.observe(), ActivationState::Inactive);
    }

    #[test]
    fn observe_derives_active_when_present_and_not_vetoed() {
        let adapter = OsActivationAdapter::new(FakeSystemCalls::new(true, Some(false)));
        assert_eq!(
            adapter.observe(),
            ActivationState::Active(ActivationMode::Login)
        );
    }

    #[test]
    fn observe_derives_requires_approval_when_present_and_vetoed() {
        // Weaker-wins: the veto beats the raw "present" entry.
        let adapter = OsActivationAdapter::new(FakeSystemCalls::new(true, Some(true)));
        assert_eq!(
            adapter.observe(),
            ActivationState::RequiresApproval(ActivationMode::Login)
        );
    }

    #[test]
    fn observe_reports_unknown_when_the_facility_cannot_be_queried() {
        let fake = FakeSystemCalls::new(true, None);
        *fake.query_fails_with.lock().unwrap() = Some("registry unreadable".to_string());
        let adapter = OsActivationAdapter::new(fake);
        assert!(matches!(adapter.observe(), ActivationState::Unknown { .. }));
    }

    #[test]
    fn enable_login_writes_the_entry_and_disable_removes_it() {
        let adapter = OsActivationAdapter::new(FakeSystemCalls::new(false, None));
        assert_eq!(adapter.observe(), ActivationState::Inactive);

        adapter
            .enable(ActivationMode::Login)
            .expect("enable succeeds");
        assert_eq!(
            adapter.observe(),
            ActivationState::Active(ActivationMode::Login)
        );

        adapter.disable().expect("disable succeeds");
        assert_eq!(adapter.observe(), ActivationState::Inactive);
    }

    #[test]
    fn both_modes_are_supported_when_the_host_reports_so() {
        let adapter = OsActivationAdapter::new(FakeSystemCalls::new(false, None));
        let caps = adapter.capabilities();
        assert_eq!(caps.login, ModeSupport::Supported);
        assert_eq!(caps.system, ModeSupport::Supported);
    }

    #[test]
    fn capabilities_reflect_a_real_per_host_check_not_a_hardcoded_yes() {
        // `capabilities()` must be a checked property of the ACTUAL
        // host, reported up front — never a hardcoded "yes" a caller only
        // discovers is false when `enable` later fails.
        let mut fake = FakeSystemCalls::new(false, None);
        fake.system_capability = ModeSupport::Unsupported {
            reason: "no systemd or logind linger on this host".to_string(),
        };
        let adapter = OsActivationAdapter::new(fake);
        let caps = adapter.capabilities();
        assert_eq!(caps.login, ModeSupport::Supported);
        assert!(matches!(caps.system, ModeSupport::Unsupported { .. }));
    }

    #[test]
    fn enable_system_writes_the_entry_and_disable_removes_it() {
        let adapter = OsActivationAdapter::new(FakeSystemCalls::new(false, None));
        assert_eq!(adapter.observe(), ActivationState::Inactive);

        adapter
            .enable(ActivationMode::System)
            .expect("enable succeeds");
        assert_eq!(
            adapter.observe(),
            ActivationState::Active(ActivationMode::System)
        );

        adapter.disable().expect("disable succeeds");
        assert_eq!(adapter.observe(), ActivationState::Inactive);
    }

    #[test]
    fn observe_prefers_system_when_both_are_somehow_present() {
        // Defensive tie-break: domain-tier mutual exclusion should prevent
        // this in normal operation, but if an out-of-band edit causes it,
        // the stronger claim (System) is reported, never silently dropped.
        let fake = FakeSystemCalls::new(true, Some(false));
        *fake.system_present.lock().unwrap() = true;
        *fake.system_vetoed.lock().unwrap() = Some(false);
        let adapter = OsActivationAdapter::new(fake);
        assert_eq!(
            adapter.observe(),
            ActivationState::Active(ActivationMode::System)
        );
    }

    #[test]
    fn a_refused_system_elevation_leaves_prior_state_unchanged() {
        // A refused/failed elevation must not partially register.
        let fake = FakeSystemCalls::new(false, None);
        *fake.fail_system_write.lock().unwrap() = true;
        let adapter = OsActivationAdapter::new(fake);

        let result = adapter.enable(ActivationMode::System);
        assert!(result.is_err());
        assert_eq!(
            adapter.observe(),
            ActivationState::Inactive,
            "a refused elevation must leave the host exactly as it was"
        );
    }

    #[test]
    fn uninstall_delegates_to_the_seams_uninstall_all() {
        let adapter = OsActivationAdapter::new(FakeSystemCalls::new(true, Some(false)));
        assert_eq!(
            adapter.observe(),
            ActivationState::Active(ActivationMode::Login)
        );
        adapter.uninstall().expect("uninstall succeeds");
        assert_eq!(adapter.observe(), ActivationState::Inactive);
    }
}
