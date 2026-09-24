//! Real Windows activation registration.
//!
//! **Login-scoped:** the `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
//! value, plus the `StartupApproved\Run` veto read (the raw `Run`
//! value alone is not the state — Windows records a user's Task-Manager
//! disablement separately, leaving `Run` intact).
//!
//! **System-scoped:** an S4U scheduled task (`schtasks.exe /create /sc
//! onstart /ru <user> /rl limited`, no `/rp` — the documented way to obtain
//! S4U logon type without a stored password) via `std::process::Command` —
//! Task Scheduler's real API is a large COM surface (`ITaskService` et al.);
//! shelling out to Microsoft's own first-party CLI is the established,
//! supported way most tools manage scheduled tasks, and needs no FFI beyond
//! what login-scoped already uses. Elevation is a real OS-mediated
//! ceremony: `ShellExecuteExW` with the `"runas"` verb, which pops the
//! actual UAC consent dialog — registration only, never applied to running
//! the engine itself.
//!
//! Raw `windows-sys` FFI, not a wrapper crate — matches this project's
//! std-first / hand-rolled-primitive precedent (e.g. `crates/model-local`'s
//! hand-rolled HTTP/1.1 rather than pulling in `ureq`).
//!
//! **Disclosed limitations:** (1) the `StartupApproved\Run` binary value's
//! format is undocumented by Microsoft; the disabled-marker byte
//! (`0x02`/`0x03` in the first byte) is based on widely-cited community
//! reverse-engineering, not an official contract. An absent value, an
//! unexpected length, or any read failure reports `None` (no veto signal) —
//! never guessed. (2) the live elevation path (`run_elevated`) is compiled
//! but never invoked in this session — it pops a real interactive UAC
//! dialog, unsuitable for an automated run; only the adapter's decision
//! logic is exercised, via the fake in `lib.rs`'s test module.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::process::Command;

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, HANDLE};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteValueW,
    RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows_sys::Win32::System::Threading::{GetExitCodeProcess, INFINITE, WaitForSingleObject};
use windows_sys::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

use cronus_contract::ModeSupport;

use crate::SystemCalls;

const RUN_KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const STARTUP_APPROVED_KEY_PATH: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
const TASK_NAME: &str = "Cronus";
const VALUE_NAME: &str = "Cronus";

fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// An open registry key handle, closed on drop.
struct OpenKey(HKEY);

impl OpenKey {
    /// Open `subkey` under `HKEY_CURRENT_USER`. `Ok(None)` means the key
    /// itself does not exist (distinct from a real failure) — `CurrentVersion
    /// \Run` always exists on a real Windows install, but `StartupApproved
    /// \Run` may not if the user has never touched Task Manager's Startup
    /// tab, which is the expected common case, not an error.
    fn open(subkey: &str, access: u32) -> Result<Option<Self>, String> {
        let wide = to_wide(subkey);
        let mut handle: HKEY = std::ptr::null_mut();
        // SAFETY: `wide` is a valid null-terminated UTF-16 string alive for
        // the duration of the call; `handle` is a valid out-pointer sized
        // for a single HKEY.
        let status =
            unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, wide.as_ptr(), 0, access, &mut handle) };
        match status as u32 {
            ERROR_SUCCESS => Ok(Some(OpenKey(handle))),
            ERROR_FILE_NOT_FOUND => Ok(None),
            other => Err(format!("RegOpenKeyExW({subkey}) failed: error {other}")),
        }
    }
}

impl Drop for OpenKey {
    fn drop(&mut self) {
        // SAFETY: `self.0` is a valid HKEY opened by `RegOpenKeyExW` above,
        // not yet closed.
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

pub struct WindowsSystemCalls;

impl SystemCalls for WindowsSystemCalls {
    fn login_entry_present(&self) -> Result<bool, String> {
        let Some(key) = OpenKey::open(RUN_KEY_PATH, KEY_READ)? else {
            return Ok(false);
        };
        let wide_name = to_wide(VALUE_NAME);
        let mut value_type: u32 = 0;
        let mut data_len: u32 = 0;
        // A null data buffer just measures/checks presence; absence of the
        // named value (not the key) also reports ERROR_FILE_NOT_FOUND.
        // SAFETY: `key.0` is a valid open handle; `wide_name` is a valid
        // null-terminated UTF-16 string; the data pointer is null (measuring
        // only), and `value_type`/`data_len` are valid out-pointers.
        let status = unsafe {
            RegQueryValueExW(
                key.0,
                wide_name.as_ptr(),
                std::ptr::null_mut(),
                &mut value_type,
                std::ptr::null_mut(),
                &mut data_len,
            )
        };
        match status as u32 {
            ERROR_SUCCESS => Ok(true),
            ERROR_FILE_NOT_FOUND => Ok(false),
            other => Err(format!(
                "RegQueryValueExW({VALUE_NAME}) failed: error {other}"
            )),
        }
    }

    fn login_entry_vetoed(&self) -> Result<Option<bool>, String> {
        let Some(key) = OpenKey::open(STARTUP_APPROVED_KEY_PATH, KEY_READ)? else {
            return Ok(None); // no StartupApproved key at all — no veto signal
        };
        let wide_name = to_wide(VALUE_NAME);
        let mut value_type: u32 = 0;
        let mut buf = [0u8; 12]; // the community-documented value is 12 bytes
        let mut data_len: u32 = buf.len() as u32;
        // SAFETY: `key.0` is a valid open handle; `wide_name` is a valid
        // null-terminated UTF-16 string; `buf` is a 12-byte buffer and
        // `data_len` correctly reports its capacity as the in/out length.
        let status = unsafe {
            RegQueryValueExW(
                key.0,
                wide_name.as_ptr(),
                std::ptr::null_mut(),
                &mut value_type,
                buf.as_mut_ptr(),
                &mut data_len,
            )
        };
        match status as u32 {
            ERROR_SUCCESS if data_len >= 1 => {
                // Disclosed heuristic (see module doc): 0x02/0x03 in the
                // first byte denotes a user- or policy-disabled entry.
                Ok(Some(buf[0] == 0x02 || buf[0] == 0x03))
            }
            // Unexpected zero-length value, absence, or any other read
            // failure: no veto signal, never guessed as vetoed.
            _ => Ok(None),
        }
    }

    fn write_login_entry(&self) -> Result<(), String> {
        // `CurrentVersion\Run` always exists on a real Windows install.
        let key = OpenKey::open(RUN_KEY_PATH, KEY_SET_VALUE)?
            .ok_or_else(|| "Run key not found".to_string())?;
        let exe_path = std::env::current_exe()
            .map_err(|e| format!("could not resolve the current executable path: {e}"))?;
        let wide_name = to_wide(VALUE_NAME);
        let wide_value = to_wide(&exe_path.display().to_string());
        // SAFETY: reinterpreting a `Vec<u16>` (UTF-16 code units) as its raw
        // little-endian byte representation for the REG_SZ write — valid for
        // the buffer's own lifetime, which outlives this call.
        let data_bytes: &[u8] = unsafe {
            std::slice::from_raw_parts(wide_value.as_ptr() as *const u8, wide_value.len() * 2)
        };
        // SAFETY: `key.0` is a valid open handle with `KEY_SET_VALUE` access;
        // `wide_name` is a valid null-terminated UTF-16 string; `data_bytes`
        // is a valid, correctly-sized REG_SZ payload (UTF-16, null-terminated).
        let status = unsafe {
            RegSetValueExW(
                key.0,
                wide_name.as_ptr(),
                0,
                REG_SZ,
                data_bytes.as_ptr(),
                data_bytes.len() as u32,
            )
        };
        match status as u32 {
            ERROR_SUCCESS => Ok(()),
            other => Err(format!("RegSetValueExW failed: error {other}")),
        }
    }

    fn remove_login_entry(&self) -> Result<(), String> {
        let Some(key) = OpenKey::open(RUN_KEY_PATH, KEY_SET_VALUE)? else {
            return Ok(()); // key absent — nothing to remove, already inactive
        };
        let wide_name = to_wide(VALUE_NAME);
        // SAFETY: `key.0` is a valid open handle with `KEY_SET_VALUE` access;
        // `wide_name` is a valid null-terminated UTF-16 string.
        let status = unsafe { RegDeleteValueW(key.0, wide_name.as_ptr()) };
        match status as u32 {
            ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
            other => Err(format!("RegDeleteValueW failed: error {other}")),
        }
    }

    fn system_entry_present(&self) -> Result<bool, String> {
        let output = Command::new("schtasks")
            .args(["/query", "/tn", TASK_NAME])
            .output()
            .map_err(|e| format!("schtasks /query failed to run: {e}"))?;
        Ok(output.status.success())
    }

    fn system_entry_vetoed(&self) -> Result<Option<bool>, String> {
        let output = Command::new("schtasks")
            .args(["/query", "/tn", TASK_NAME, "/v", "/fo", "list"])
            .output()
            .map_err(|e| format!("schtasks /query /v failed to run: {e}"))?;
        if !output.status.success() {
            return Ok(None); // task absent — no veto signal
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let disabled = stdout.lines().any(|line| {
            line.trim_start()
                .to_lowercase()
                .starts_with("scheduled task state:")
                && line.to_lowercase().contains("disabled")
        });
        Ok(Some(disabled))
    }

    fn write_system_entry(&self) -> Result<(), String> {
        let exe_path = std::env::current_exe()
            .map_err(|e| format!("could not resolve the current executable path: {e}"))?;
        let run_as = current_username()?;
        // `/f` forces overwrite of a same-named task (idempotent re-register);
        // omitting `/rp` (no stored password) is the documented way schtasks
        // yields S4U logon type for a non-well-known account.
        let args = format!(
            "/create /tn \"{TASK_NAME}\" /tr \"{}\" /sc onstart /ru \"{run_as}\" /rl limited /f",
            exe_path.display()
        );
        let exit_code = run_elevated("schtasks.exe", &args)?;
        if exit_code == 0 {
            Ok(())
        } else {
            Err(format!(
                "schtasks /create exited with code {exit_code} (elevation refused, or the task could not be registered)"
            ))
        }
    }

    fn remove_system_entry(&self) -> Result<(), String> {
        if !self.system_entry_present()? {
            return Ok(()); // absent — nothing to remove, already inactive
        }
        let args = format!("/delete /tn \"{TASK_NAME}\" /f");
        let exit_code = run_elevated("schtasks.exe", &args)?;
        if exit_code == 0 {
            Ok(())
        } else {
            Err(format!("schtasks /delete exited with code {exit_code}"))
        }
    }

    fn uninstall_all(&self) -> Result<(), String> {
        // Every artifact this adapter's own name (`Cronus` / `TASK_NAME`)
        // could occupy on Windows: the Run value and the scheduled task.
        // Nothing else is ever inspected, so no foreign registration is
        // ever in scope.
        self.remove_login_entry()?;
        self.remove_system_entry()
    }

    fn login_capability(&self) -> ModeSupport {
        // `HKCU\...\Run` is a core shell component on every real Windows
        // install — no plausible absence to detect.
        ModeSupport::Supported
    }

    fn system_capability(&self) -> ModeSupport {
        // Task Scheduler is a core OS service on every real Windows install.
        ModeSupport::Supported
    }
}

/// The `DOMAIN\username` (or bare `username`) to register the scheduled
/// task under — the currently logged-in user, never a stored credential.
fn current_username() -> Result<String, String> {
    let username = std::env::var("USERNAME")
        .map_err(|_| "USERNAME environment variable not set".to_string())?;
    match std::env::var("USERDOMAIN") {
        Ok(domain) if !domain.is_empty() => Ok(format!("{domain}\\{username}")),
        _ => Ok(username),
    }
}

/// Run `exe args` elevated via the real Windows UAC ceremony: a human
/// performs and can refuse this. Blocks until the elevated process exits and
/// returns its exit code; `ShellExecuteExW` itself failing (including a
/// refused elevation) surfaces as `Err` before any process runs.
fn run_elevated(exe: &str, args: &str) -> Result<u32, String> {
    let wide_exe = to_wide(exe);
    let wide_verb = to_wide("runas");
    let wide_args = to_wide(args);

    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb = wide_verb.as_ptr();
    info.lpFile = wide_exe.as_ptr();
    info.lpParameters = wide_args.as_ptr();
    info.nShow = SW_HIDE;

    // SAFETY: `info` is a zero-initialized `SHELLEXECUTEINFOW` with `cbSize`
    // set and every pointer field pointing at a valid null-terminated UTF-16
    // buffer alive for the duration of the call.
    let ok = unsafe { ShellExecuteExW(&mut info) };
    if ok == 0 {
        return Err("elevation was refused or failed (ShellExecuteExW)".to_string());
    }
    let process: HANDLE = info.hProcess;
    if process.is_null() {
        return Err("elevated process handle unavailable".to_string());
    }
    // SAFETY: `process` is a valid process handle returned by
    // `ShellExecuteExW` with `SEE_MASK_NOCLOSEPROCESS` set, not yet closed.
    unsafe {
        WaitForSingleObject(process, INFINITE);
    }
    let mut exit_code: u32 = 0;
    // SAFETY: `process` is still valid; `exit_code` is a valid out-pointer.
    let got_exit_code = unsafe { GetExitCodeProcess(process, &mut exit_code) };
    // SAFETY: `process` is a valid, still-open handle being closed exactly
    // once here.
    unsafe {
        CloseHandle(process);
    }
    if got_exit_code == 0 {
        return Err("could not read the elevated process's exit code".to_string());
    }
    Ok(exit_code)
}
