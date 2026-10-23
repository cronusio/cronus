//! `diagnostics::install_with` end to end: the process-wide subscriber, the
//! `session.start` header and the per-kind file family, against a real
//! directory. This lives in its own test binary because installing a
//! subscriber is a once-per-process act.

use std::fs;
use std::path::PathBuf;

use cronus_core::diagnostics::{ProcessKind, Settings, install_with};
use serde_json::Value;

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cronus-diag-install-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

#[test]
fn install_makes_the_sink_the_process_subscriber_and_writes_the_kind_file() {
    let logs = scratch().join("logs");
    let settings = Settings::default();

    let guard = install_with(ProcessKind::Tui, &logs, &settings);
    assert!(guard.is_active());
    assert_eq!(guard.kind(), ProcessKind::Tui);

    tracing::info!(target: "cronus_core::install_test", bytes = 3u64, "hello from the test");
    tracing::info!(target: "some_dependency::client", "below the dependency floor");

    // A second install in the same process cannot replace the first and says so.
    let second = install_with(ProcessKind::Cli, &logs, &settings);
    assert!(!second.is_active());
    drop(second);

    let session = guard.session().to_owned();
    drop(guard);

    let text = fs::read_to_string(logs.join("tui.jsonl")).expect("the tui file family exists");
    let lines: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).expect("every line is JSON"))
        .collect();
    assert_eq!(lines.len(), 2, "header and one event: {text}");
    assert_eq!(lines[0]["msg"], "session.start");
    assert_eq!(lines[0]["kind"], "tui");
    assert_eq!(lines[0]["session"], session.as_str());
    assert!(
        lines[0]["state_root"]
            .as_str()
            .is_some_and(|root| !root.is_empty())
    );
    assert_eq!(lines[1]["msg"], "hello from the test");
    assert_eq!(lines[1]["bytes"], 3);
    assert!(
        !logs.join("cli.jsonl").exists(),
        "the inert second install must not create its own file"
    );

    let _ = fs::remove_dir_all(logs.parent().expect("scratch parent"));
}
