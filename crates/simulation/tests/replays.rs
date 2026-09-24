//! The always-on replay lane: every pinned route under `tests/replays/`
//! runs here, with no agent in the loop. This is the guarding half of
//! replay — discovery is expensive and free-route;
//! this is cheap, deterministic, and belongs in the ordinary `cargo test`
//! gate the whole workspace already runs on every change.

use std::path::Path;

#[test]
fn every_pinned_replay_case_holds() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/replays");
    let mut case_count = 0usize;

    for entry in std::fs::read_dir(&dir).expect("tests/replays/ must exist") {
        let entry = entry.expect("readable dir entry");
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        case_count += 1;

        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("{}: failed to read: {err}", path.display()));
        let case = cronus_simulation::replay::parse(&text)
            .unwrap_or_else(|err| panic!("{}: failed to parse: {err}", path.display()));

        cronus_simulation::replay::run(&case)
            .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
    }

    assert!(
        case_count > 0,
        "tests/replays/ must hold at least one pinned case — an empty replay lane guards nothing"
    );
}
