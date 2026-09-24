//! Real macOS activation registration.
//!
//! **Login-scoped:** a `LaunchAgent` plist in `~/Library/LaunchAgents/`,
//! loaded via `launchctl`.
//!
//! **System-scoped:** a `LaunchDaemon` plist in `/Library/LaunchDaemons/`
//! (system-wide) with `UserName` set to the installing user — the same
//! account, unelevated at run time, but the daemon survives logout because
//! `launchd` (not a user session) supervises it. Writing into
//! `/Library/LaunchDaemons/` needs root; elevated via `sudo`, which prompts
//! for the user's own password — a real, human-refusable authorization
//! ceremony, though a rougher one than the native macOS
//! authorization dialog `SMAppService.daemon` would show.
//!
//! **Disclosed gap vs. the intended design (both modes):** it describes
//! `SMAppService`, whose `status` distinguishes `requiresApproval` — a
//! signal this LaunchAgent/LaunchDaemon+launchctl implementation cannot
//! produce (that approval gate, and a native authorization prompt in place
//! of `sudo`, are specific to the modern `ServiceManagement` framework API,
//! callable only through Objective-C/Swift interop — e.g. `objc2` — a
//! meaningfully larger dependency undertaking deferred to its own follow-up,
//! and not evaluable at all on the Windows host this was written on, which
//! has no macOS SDK). This implementation still delivers genuine
//! login-scoped and system-scoped activation; `*_vetoed` always
//! reports `None` (no veto signal available), which is honest —
//! never a fabricated "not vetoed". Pure `std` — no crate: a plist is XML
//! text (`std::fs`), `launchctl`/`sudo` are ordinary subprocesses
//! (`std::process::Command`).
//!
//! **Verification scope, disclosed:** type-checked via cross-compilation
//! (`cargo check --target aarch64-apple-darwin`) from a Windows host; no
//! macOS host was available to run it. Real-world validation is deferred.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use cronus_contract::ModeSupport;

use crate::SystemCalls;

const LABEL: &str = "com.cronus.engine";

fn home_dir() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is not set".to_string())
}

fn plist_path() -> Result<PathBuf, String> {
    Ok(home_dir()?
        .join("Library/LaunchAgents")
        .join(format!("{LABEL}.plist")))
}

fn plist_content(exe_path: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
         \x20\x20\x20\x20<key>Label</key>\n\
         \x20\x20\x20\x20<string>{LABEL}</string>\n\
         \x20\x20\x20\x20<key>ProgramArguments</key>\n\
         \x20\x20\x20\x20<array>\n\
         \x20\x20\x20\x20\x20\x20\x20\x20<string>{exe_path}</string>\n\
         \x20\x20\x20\x20</array>\n\
         \x20\x20\x20\x20<key>RunAtLoad</key>\n\
         \x20\x20\x20\x20<true/>\n\
         </dict>\n\
         </plist>\n"
    )
}

pub struct MacosSystemCalls;

impl SystemCalls for MacosSystemCalls {
    fn login_entry_present(&self) -> Result<bool, String> {
        Ok(plist_path()?.exists())
    }

    fn login_entry_vetoed(&self) -> Result<Option<bool>, String> {
        // No veto signal available without SMAppService (see module doc) —
        // honestly reported as "no signal", never fabricated as "not vetoed".
        Ok(None)
    }

    fn write_login_entry(&self) -> Result<(), String> {
        let exe_path = std::env::current_exe()
            .map_err(|e| format!("could not resolve the current executable path: {e}"))?
            .display()
            .to_string();
        let path = plist_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
        }
        fs::write(&path, plist_content(&exe_path))
            .map_err(|e| format!("could not write {}: {e}", path.display()))?;

        let status = Command::new("launchctl")
            .args(["load", "-w"])
            .arg(&path)
            .status()
            .map_err(|e| format!("launchctl load failed to run: {e}"))?;
        if !status.success() {
            return Err(format!("launchctl load exited with {status}"));
        }
        Ok(())
    }

    fn remove_login_entry(&self) -> Result<(), String> {
        let path = plist_path()?;
        if !path.exists() {
            return Ok(());
        }
        let _ = Command::new("launchctl")
            .args(["unload", "-w"])
            .arg(&path)
            .status();
        fs::remove_file(&path).map_err(|e| format!("could not remove {}: {e}", path.display()))
    }

    fn system_entry_present(&self) -> Result<bool, String> {
        Ok(daemon_plist_path().exists())
    }

    fn system_entry_vetoed(&self) -> Result<Option<bool>, String> {
        // No veto signal available without SMAppService (see module doc).
        Ok(None)
    }

    fn write_system_entry(&self) -> Result<(), String> {
        let exe_path = std::env::current_exe()
            .map_err(|e| format!("could not resolve the current executable path: {e}"))?
            .display()
            .to_string();
        let user = current_username()?;
        let content = daemon_plist_content(&exe_path, &user);
        let path = daemon_plist_path();

        // `sudo` prompts for the user's own password — a real, refusable
        // authorization ceremony, rougher than the native
        // SMAppService authorization dialog (see module doc).
        let mut child = Command::new("sudo")
            .arg("tee")
            .arg(&path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("sudo tee failed to run: {e}"))?;
        {
            use std::io::Write;
            let stdin = child
                .stdin
                .as_mut()
                .ok_or_else(|| "sudo tee stdin unavailable".to_string())?;
            stdin
                .write_all(content.as_bytes())
                .map_err(|e| format!("could not write plist content to sudo tee: {e}"))?;
        }
        let status = child
            .wait()
            .map_err(|e| format!("sudo tee did not exit cleanly: {e}"))?;
        if !status.success() {
            return Err(format!(
                "sudo tee exited with {status} (authorization refused, or the plist could not be written)"
            ));
        }

        let bootstrap = Command::new("sudo")
            .args(["launchctl", "bootstrap", "system"])
            .arg(&path)
            .status()
            .map_err(|e| format!("sudo launchctl bootstrap failed to run: {e}"))?;
        if bootstrap.success() {
            Ok(())
        } else {
            Err(format!("launchctl bootstrap exited with {bootstrap}"))
        }
    }

    fn remove_system_entry(&self) -> Result<(), String> {
        let path = daemon_plist_path();
        if !path.exists() {
            return Ok(());
        }
        let _ = Command::new("sudo")
            .args(["launchctl", "bootout", "system"])
            .arg(&path)
            .status();
        let status = Command::new("sudo")
            .arg("rm")
            .arg("-f")
            .arg(&path)
            .status()
            .map_err(|e| format!("sudo rm failed to run: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("sudo rm exited with {status}"))
        }
    }

    fn uninstall_all(&self) -> Result<(), String> {
        // Every artifact this adapter's own label could occupy on macOS: the
        // LaunchAgent and the LaunchDaemon plists. Nothing else is ever
        // inspected, so no foreign registration is ever in scope.
        self.remove_login_entry()?;
        self.remove_system_entry()
    }

    fn login_capability(&self) -> ModeSupport {
        // `~/Library/LaunchAgents` is a core per-user directory on every
        // real macOS install — no plausible absence to detect.
        ModeSupport::Supported
    }

    fn system_capability(&self) -> ModeSupport {
        // `/Library/LaunchDaemons` + `launchd` are core OS components on
        // every real macOS install.
        ModeSupport::Supported
    }
}

fn current_username() -> Result<String, String> {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .map_err(|_| "USER/LOGNAME environment variable not set".to_string())
}

fn daemon_plist_path() -> PathBuf {
    PathBuf::from("/Library/LaunchDaemons").join(format!("{LABEL}.daemon.plist"))
}

fn daemon_plist_content(exe_path: &str, user: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
         \x20\x20\x20\x20<key>Label</key>\n\
         \x20\x20\x20\x20<string>{LABEL}.daemon</string>\n\
         \x20\x20\x20\x20<key>UserName</key>\n\
         \x20\x20\x20\x20<string>{user}</string>\n\
         \x20\x20\x20\x20<key>ProgramArguments</key>\n\
         \x20\x20\x20\x20<array>\n\
         \x20\x20\x20\x20\x20\x20\x20\x20<string>{exe_path}</string>\n\
         \x20\x20\x20\x20</array>\n\
         \x20\x20\x20\x20<key>RunAtLoad</key>\n\
         \x20\x20\x20\x20<true/>\n\
         \x20\x20\x20\x20<key>KeepAlive</key>\n\
         \x20\x20\x20\x20<true/>\n\
         </dict>\n\
         </plist>\n"
    )
}
