//! `CRONUS_PORTABLE_DIR` overrides `Paths::os_native()` for every root at
//! once — the isolation seam a test run, a CI job, or a
//! QA pass needs so the product never touches the real per-OS user
//! directories. This lives in its own integration-test binary, a separate
//! OS process from the crate's `--lib` unit tests (and every other
//! integration test file) — `std::env::set_var` here cannot leak into, or
//! race with, anything outside this one process.

use std::path::PathBuf;

use cronus_domain::paths::{PORTABLE_DIR_ENV, Paths, Root};

// `CRONUS_PORTABLE_DIR` is process-global, and `cargo test` runs one
// binary's tests multi-threaded — so the tests in *this* file that set/
// remove it must not interleave with each other. Same pattern as
// `crates/core/tests/mission_mode.rs`'s `CRONUS_MISSION_MODE` lock.
static PORTABLE_DIR_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock_portable_dir_env() -> std::sync::MutexGuard<'static, ()> {
    PORTABLE_DIR_ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn temp_base(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "cronus-portable-dir-env-{tag}-{}",
        std::process::id()
    ))
}

#[test]
fn os_native_honors_the_override_for_every_root() {
    let _env = lock_portable_dir_env();
    let base = temp_base("override");
    // SAFETY: serialized by the env lock (see above) — no concurrent
    // reader/writer of `CRONUS_PORTABLE_DIR` within this process.
    unsafe { std::env::set_var(PORTABLE_DIR_ENV, &base) };

    let paths = Paths::os_native();
    for root in [Root::Program, Root::State, Root::Cache, Root::Logs] {
        assert!(
            paths.resolve(root).starts_with(&base),
            "{root:?} must resolve under the override base"
        );
    }

    unsafe { std::env::remove_var(PORTABLE_DIR_ENV) };
}

#[test]
fn os_native_ignores_an_empty_override() {
    let _env = lock_portable_dir_env();
    // SAFETY: serialized by the env lock (see above).
    unsafe { std::env::set_var(PORTABLE_DIR_ENV, "") };

    let paths = Paths::os_native();
    let state = paths.resolve(Root::State);
    assert!(
        !state.as_os_str().is_empty(),
        "an empty override must fall back to OS-native resolution, not an empty base"
    );

    unsafe { std::env::remove_var(PORTABLE_DIR_ENV) };
}

#[test]
fn os_native_resolves_normally_when_unset() {
    let _env = lock_portable_dir_env();
    // SAFETY: serialized by the env lock (see above).
    unsafe { std::env::remove_var(PORTABLE_DIR_ENV) };

    let native = Paths::os_native().resolve(Root::State);
    let portable = Paths::portable(temp_base("control")).resolve(Root::State);
    assert_ne!(
        native, portable,
        "with the override unset, os_native() must not resolve like portable mode"
    );
}

#[test]
fn an_explicit_portable_base_still_wins_over_the_environment() {
    let _env = lock_portable_dir_env();
    let env_base = temp_base("env-base");
    let explicit_base = temp_base("explicit-base");
    // SAFETY: serialized by the env lock (see above).
    unsafe { std::env::set_var(PORTABLE_DIR_ENV, &env_base) };

    let resolved = Paths::portable(&explicit_base).resolve(Root::State);
    assert!(
        resolved.starts_with(&explicit_base),
        "Paths::portable(base) must use its own base, not CRONUS_PORTABLE_DIR"
    );

    unsafe { std::env::remove_var(PORTABLE_DIR_ENV) };
}
