// Hide the extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // The diagnostic sink is installed before the shell starts, so a failure
    // while the window is being built leaves a record. The application's event
    // loop ends the process when it exits, so this guard is dropped only if
    // `run` ever returns; a long-lived host writes its events well before then.
    let _diagnostics = cronus_core::diagnostics::install(
        cronus_core::diagnostics::ProcessKind::Desktop,
        &cronus_core::diagnostics::logs_root(),
    );
    cronus_desktop_lib::run();
}
