//! Facade wiring for the activation adapter: picks
//! the right per-OS `SystemCalls` implementation and wraps it as a full
//! `ActivationRegistry` — the platform-dispatch decision frontends should
//! never need to make themselves (no domain/platform logic in
//! frontends). Lives here, not in `cronus-domain`, because it reaches into
//! the adapter crate `cronus-activation-os` (the tier model has no edge
//! from domain to an adapter).

use cronus_activation_os::OsActivationAdapter;
pub use cronus_contract::{
    ActivationCapabilities, ActivationMode, ActivationRegistry, ActivationState, ModeSupport,
};

#[cfg(windows)]
pub fn default_activation_registry() -> OsActivationAdapter<cronus_activation_os::WindowsSystemCalls>
{
    OsActivationAdapter::new(cronus_activation_os::WindowsSystemCalls)
}

#[cfg(target_os = "linux")]
pub fn default_activation_registry() -> OsActivationAdapter<cronus_activation_os::LinuxSystemCalls>
{
    OsActivationAdapter::new(cronus_activation_os::LinuxSystemCalls)
}

#[cfg(target_os = "macos")]
pub fn default_activation_registry() -> OsActivationAdapter<cronus_activation_os::MacosSystemCalls>
{
    OsActivationAdapter::new(cronus_activation_os::MacosSystemCalls)
}
