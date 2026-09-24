//! Real Linux activation registration.
//!
//! **Login-scoped:** a systemd **user** unit
//! (`~/.config/systemd/user/cronus.service`) enabled via `systemctl --user`,
//! with an **XDG autostart** fallback (`~/.config/autostart/cronus.desktop`)
//! on non-systemd hosts (the settled resolution
//! of the non-systemd case).
//!
//! **System-scoped:** a systemd **system** unit with `User=<user>`
//! (`/etc/systemd/system/cronus.service`), preferred when the user can
//! authorize it — elevated via `pkexec` (PolicyKit), the real graphical
//! authentication ceremony a human performs and can refuse — falling
//! back to the **lingering user unit** (`loginctl enable-linger`) when they
//! cannot, exactly as designed ("preferring the system unit when the
//! user can authorize it and falling back to the lingering user unit when
//! they cannot"). Both are system-scoped in the boot-time sense (boot-started,
//! survive logout) — "two realizations, one mode".
//!
//! Pure `std` — no crate beyond `cronus-contract`: unit/desktop-entry
//! content is a plain text file (`std::fs`), and `systemctl`/`pkexec`/
//! `loginctl` are invoked as ordinary subprocesses
//! (`std::process::Command`), matching the project's dependency policy
//! (prefer std; justify every third-party crate) — there is nothing here a
//! crate would meaningfully abstract.
//!
//! **Verification scope, disclosed:** type-checked via cross-compilation
//! (`cargo check --target x86_64-unknown-linux-gnu`) from a Windows host; no
//! Linux host was available to run it. Real-world validation is deferred.
//! The exact polkit authorization rules for self-service `enable-linger`
//! vary by distribution; both the unelevated and `pkexec`-elevated paths are
//! attempted, in that order.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use cronus_contract::ModeSupport;

use crate::SystemCalls;

const UNIT_NAME: &str = "cronus.service";
const DESKTOP_NAME: &str = "cronus.desktop";

fn home_dir() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is not set".to_string())
}

fn systemd_user_unit_path() -> Result<PathBuf, String> {
    Ok(home_dir()?.join(".config/systemd/user").join(UNIT_NAME))
}

fn xdg_autostart_path() -> Result<PathBuf, String> {
    Ok(home_dir()?.join(".config/autostart").join(DESKTOP_NAME))
}

/// The conventional presence check: `systemctl --user` succeeds (or reports
/// the well-defined "degraded" exit code) only under an active user systemd
/// instance — absent on a non-systemd host (an open question since resolved).
fn has_systemd_user() -> bool {
    Command::new("systemctl")
        .args(["--user", "is-system-running"])
        .output()
        .map(|o| o.status.success() || o.status.code() == Some(1))
        .unwrap_or(false)
}

fn unit_file_content(exe_path: &str) -> String {
    format!(
        "[Unit]\nDescription=Cronus background engine\n\n\
         [Service]\nExecStart={exe_path}\nRestart=on-failure\n\n\
         [Install]\nWantedBy=default.target\n"
    )
}

fn desktop_entry_content(exe_path: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Cronus\nExec={exe_path}\n\
         X-GNOME-Autostart-enabled=true\nHidden=false\n"
    )
}

pub struct LinuxSystemCalls;

impl SystemCalls for LinuxSystemCalls {
    fn login_entry_present(&self) -> Result<bool, String> {
        if has_systemd_user() {
            Ok(systemd_user_unit_path()?.exists())
        } else {
            Ok(xdg_autostart_path()?.exists())
        }
    }

    fn login_entry_vetoed(&self) -> Result<Option<bool>, String> {
        if has_systemd_user() {
            let path = systemd_user_unit_path()?;
            if !path.exists() {
                return Ok(None);
            }
            // `systemctl --user is-enabled` distinguishes "enabled" from a
            // unit file that exists but was disabled — the weaker-wins
            // signal the registry needs.
            let output = Command::new("systemctl")
                .args(["--user", "is-enabled", UNIT_NAME])
                .output()
                .map_err(|e| format!("systemctl --user is-enabled failed to run: {e}"))?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            Ok(Some(stdout.trim() == "disabled"))
        } else {
            let path = xdg_autostart_path()?;
            if !path.exists() {
                return Ok(None);
            }
            let content = fs::read_to_string(&path)
                .map_err(|e| format!("could not read {}: {e}", path.display()))?;
            let vetoed = content.lines().any(|line| {
                let line = line.trim();
                line.eq_ignore_ascii_case("Hidden=true")
                    || line.eq_ignore_ascii_case("X-GNOME-Autostart-enabled=false")
            });
            Ok(Some(vetoed))
        }
    }

    fn write_login_entry(&self) -> Result<(), String> {
        let exe_path = std::env::current_exe()
            .map_err(|e| format!("could not resolve the current executable path: {e}"))?
            .display()
            .to_string();

        if has_systemd_user() {
            let path = systemd_user_unit_path()?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
            }
            fs::write(&path, unit_file_content(&exe_path))
                .map_err(|e| format!("could not write {}: {e}", path.display()))?;
            let status = Command::new("systemctl")
                .args(["--user", "enable", UNIT_NAME])
                .status()
                .map_err(|e| format!("systemctl --user enable failed to run: {e}"))?;
            if !status.success() {
                return Err(format!("systemctl --user enable exited with {status}"));
            }
        } else {
            let path = xdg_autostart_path()?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
            }
            fs::write(&path, desktop_entry_content(&exe_path))
                .map_err(|e| format!("could not write {}: {e}", path.display()))?;
        }
        Ok(())
    }

    fn remove_login_entry(&self) -> Result<(), String> {
        if has_systemd_user() {
            let path = systemd_user_unit_path()?;
            if path.exists() {
                let _ = Command::new("systemctl")
                    .args(["--user", "disable", UNIT_NAME])
                    .status();
                fs::remove_file(&path)
                    .map_err(|e| format!("could not remove {}: {e}", path.display()))?;
            }
        } else {
            let path = xdg_autostart_path()?;
            if path.exists() {
                fs::remove_file(&path)
                    .map_err(|e| format!("could not remove {}: {e}", path.display()))?;
            }
        }
        Ok(())
    }

    fn system_entry_present(&self) -> Result<bool, String> {
        if system_unit_path().exists() {
            return Ok(true);
        }
        // Lingering-user-unit realization: present only if BOTH the login
        // unit/autostart exists AND linger is enabled ("two
        // realizations, one mode").
        Ok(linger_enabled()? && self.login_entry_present()?)
    }

    fn system_entry_vetoed(&self) -> Result<Option<bool>, String> {
        if system_unit_path().exists() {
            let output = Command::new("systemctl")
                .args(["is-enabled", UNIT_NAME])
                .output()
                .map_err(|e| format!("systemctl is-enabled failed to run: {e}"))?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Ok(Some(stdout.trim() == "disabled"));
        }
        // The lingering-user-unit realization has no separate veto concept
        // beyond presence — no signal, never fabricated.
        Ok(None)
    }

    fn write_system_entry(&self) -> Result<(), String> {
        let exe_path = std::env::current_exe()
            .map_err(|e| format!("could not resolve the current executable path: {e}"))?
            .display()
            .to_string();

        match try_write_system_unit(&exe_path) {
            Ok(()) => Ok(()),
            Err(system_unit_err) => {
                // Fall back to the login-scoped realization + linger — still
                // system-scoped.
                self.write_login_entry()?;
                let user = current_username()?;
                try_enable_linger(&user).map_err(|linger_err| {
                    format!(
                        "system unit registration failed ({system_unit_err}); \
                         linger fallback also failed ({linger_err})"
                    )
                })
            }
        }
    }

    fn remove_system_entry(&self) -> Result<(), String> {
        let path = system_unit_path();
        if path.exists() {
            let _ = Command::new("pkexec")
                .args(["systemctl", "disable", UNIT_NAME])
                .status();
            let status = Command::new("pkexec")
                .arg("rm")
                .arg("-f")
                .arg(&path)
                .status()
                .map_err(|e| format!("pkexec rm failed to run: {e}"))?;
            if !status.success() {
                return Err(format!("pkexec rm exited with {status}"));
            }
            let _ = Command::new("pkexec")
                .args(["systemctl", "daemon-reload"])
                .status();
            return Ok(());
        }
        // Lingering-user-unit realization: disable linger, then remove the
        // login-scoped entry it rode on.
        if let Ok(user) = current_username() {
            let unelevated = Command::new("loginctl")
                .args(["disable-linger", &user])
                .status();
            if unelevated.map(|s| !s.success()).unwrap_or(true) {
                let _ = Command::new("pkexec")
                    .args(["loginctl", "disable-linger", &user])
                    .status();
            }
        }
        self.remove_login_entry()
    }

    fn uninstall_all(&self) -> Result<(), String> {
        // Every artifact this adapter's own name could occupy on Linux: the
        // user unit, the XDG autostart entry, the system unit, and the
        // linger flag. Nothing else is ever inspected, so no foreign
        // registration is ever in scope.
        self.remove_login_entry()?;
        self.remove_system_entry()
    }

    fn login_capability(&self) -> ModeSupport {
        // The systemd user-unit path works headless or graphical. Absent
        // systemd, XDG autostart needs a graphical session to ever be read
        // — a headless non-systemd host genuinely cannot offer login-scoped
        // activation (an open question since resolved), so this is checked, not assumed.
        if has_systemd_user() || has_graphical_session() {
            ModeSupport::Supported
        } else {
            ModeSupport::Unsupported {
                reason: "no systemd user instance and no graphical session to read XDG autostart"
                    .to_string(),
            }
        }
    }

    fn system_capability(&self) -> ModeSupport {
        // Both realizations (system unit, lingering user unit) are
        // systemd/logind features — absent systemd, neither exists.
        if has_systemd_user() {
            ModeSupport::Supported
        } else {
            ModeSupport::Unsupported {
                reason: "system-scoped activation needs systemd (system unit or logind linger); \
                         none detected"
                    .to_string(),
            }
        }
    }
}

/// Whether a graphical session exists to read XDG autostart entries — the
/// conventional check (a display server is set).
fn has_graphical_session() -> bool {
    std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

fn system_unit_path() -> PathBuf {
    PathBuf::from("/etc/systemd/system").join(UNIT_NAME)
}

fn current_username() -> Result<String, String> {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .map_err(|_| "USER/LOGNAME environment variable not set".to_string())
}

fn linger_enabled() -> Result<bool, String> {
    let user = current_username()?;
    let output = Command::new("loginctl")
        .args(["show-user", &user, "--property=Linger"])
        .output()
        .map_err(|e| format!("loginctl show-user failed to run: {e}"))?;
    Ok(String::from_utf8_lossy(&output.stdout).trim() == "Linger=yes")
}

fn system_unit_file_content(exe_path: &str, user: &str) -> String {
    format!(
        "[Unit]\nDescription=Cronus background engine\n\n\
         [Service]\nUser={user}\nExecStart={exe_path}\nRestart=on-failure\n\n\
         [Install]\nWantedBy=multi-user.target\n"
    )
}

/// Write and enable the system-wide unit — every step needs root, so every
/// step goes through `pkexec` (the real polkit authentication ceremony,
/// which a human can refuse). `pkexec tee` writes the file since we (unprivileged) cannot open
/// `/etc/systemd/system/` for writing directly.
fn try_write_system_unit(exe_path: &str) -> Result<(), String> {
    let user = current_username()?;
    let content = system_unit_file_content(exe_path, &user);
    let path = system_unit_path();

    let mut child = Command::new("pkexec")
        .arg("tee")
        .arg(&path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("pkexec tee failed to run: {e}"))?;
    {
        use std::io::Write;
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| "pkexec tee stdin unavailable".to_string())?;
        stdin
            .write_all(content.as_bytes())
            .map_err(|e| format!("could not write unit content to pkexec tee: {e}"))?;
    }
    let status = child
        .wait()
        .map_err(|e| format!("pkexec tee did not exit cleanly: {e}"))?;
    if !status.success() {
        return Err(format!("pkexec tee exited with {status}"));
    }

    let reload = Command::new("pkexec")
        .args(["systemctl", "daemon-reload"])
        .status()
        .map_err(|e| format!("pkexec systemctl daemon-reload failed to run: {e}"))?;
    if !reload.success() {
        return Err(format!("systemctl daemon-reload exited with {reload}"));
    }

    let enable = Command::new("pkexec")
        .args(["systemctl", "enable", UNIT_NAME])
        .status()
        .map_err(|e| format!("pkexec systemctl enable failed to run: {e}"))?;
    if enable.success() {
        Ok(())
    } else {
        Err(format!("systemctl enable exited with {enable}"))
    }
}

/// Enable lingering for `user` — try unelevated first (some distributions'
/// default polkit rules permit a user to enable their own linger without
/// authentication), falling back to `pkexec` (the real elevation ceremony).
fn try_enable_linger(user: &str) -> Result<(), String> {
    if let Ok(status) = Command::new("loginctl")
        .args(["enable-linger", user])
        .status()
        && status.success()
    {
        return Ok(());
    }
    let elevated = Command::new("pkexec")
        .args(["loginctl", "enable-linger", user])
        .status()
        .map_err(|e| format!("pkexec loginctl enable-linger failed to run: {e}"))?;
    if elevated.success() {
        Ok(())
    } else {
        Err(format!(
            "loginctl enable-linger failed (elevated exit {elevated})"
        ))
    }
}
