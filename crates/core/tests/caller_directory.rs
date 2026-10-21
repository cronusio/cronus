// An invocation that runs in an engine for a caller standing elsewhere means
// the *caller's* directory by "here" and by every relative path. Handlers get
// that directory from `paths::caller_dir` / `paths::from_caller` and never
// from the process's own, so the rule has to hold for every handler — and a
// handler added next month is one nobody remembered to check. This reads the
// handler sources and fails on the call that would break it.

use std::fs;
use std::path::{Path, PathBuf};

fn sources_under(dir: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            sources_under(&path, into);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            into.push(path);
        }
    }
}

#[test]
fn no_handler_reads_the_process_directory_directly() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    for dir in ["invocable_bootstrap", "installation"] {
        sources_under(&src.join(dir), &mut files);
    }
    assert!(!files.is_empty(), "the handler sources were not found");

    let mut offenders = Vec::new();
    for file in files {
        let text = fs::read_to_string(&file).expect("readable source");
        for (number, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            if code.contains("env::current_dir") {
                offenders.push(format!("{}:{}", file.display(), number + 1));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "a handler must use cronus_core::paths::caller_dir / from_caller, not the process's own \
         directory (an engine runs handlers for callers standing elsewhere):\n{}",
        offenders.join("\n")
    );
}

#[test]
fn a_handler_run_for_a_caller_sees_the_callers_directory() {
    use cronus_core::paths::{caller_dir, from_caller, with_caller_dir};

    let elsewhere = std::env::temp_dir().join("cronus-caller-dir-elsewhere");
    with_caller_dir(&elsewhere, || {
        assert_eq!(caller_dir(), elsewhere);
        assert_eq!(
            from_caller(Path::new("notes/a.md")),
            elsewhere.join("notes/a.md")
        );
        let absolute = std::env::temp_dir().join("abs.md");
        assert_eq!(
            from_caller(&absolute),
            absolute,
            "an absolute path is left alone"
        );
    });
    // And it is the process's own again afterwards.
    assert_eq!(
        caller_dir(),
        std::env::current_dir().expect("a current directory")
    );
}

#[test]
fn the_callers_directory_does_not_leak_into_other_threads() {
    use cronus_core::paths::{caller_dir, with_caller_dir};

    let elsewhere = std::env::temp_dir().join("cronus-caller-dir-thread");
    with_caller_dir(&elsewhere, || {
        let other = std::thread::spawn(caller_dir).join().expect("joins");
        assert_ne!(
            other, elsewhere,
            "a spawned thread starts from the process's directory"
        );
    });
}
