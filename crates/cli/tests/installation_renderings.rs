// One installation outcome, two renderings: the command line answers with an
// exit signal and printed text, the terminal UI with an inline block. Both
// are held to the shared installation fixtures of the conformance corpus, so
// they agree with each other without either knowing how the other prints.
//
// This lives in the command line's test target because that crate links the
// terminal UI (its `tui` verb) and builds the program under test. It is one
// test function in its own test binary on purpose: the terminal UI half
// resolves the workspace from the process working directory, so the two
// states are visited in turn rather than in parallel.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;

use cronus_conformance::{
    InstallationFixture, InstallationRendering, InstallationState, Shown, Signal,
    check_installation_renderings, installation_fixtures,
};
use cronus_tui::command;
use cronus_tui::installation::Step;

/// The command line, run as the program it is: the exit code is the signal
/// (0 succeeded, 1 unavailable, 2 rejected) and everything printed is the text.
struct CommandLine<'a> {
    home: &'a Path,
}

impl InstallationRendering for CommandLine<'_> {
    fn show(&self, fixture: &InstallationFixture) -> Shown {
        let out = Command::new(env!("CARGO_BIN_EXE_cronus"))
            .args(fixture.words)
            .current_dir(self.home)
            .env("CRONUS_PORTABLE_DIR", self.home.join("portable"))
            .output()
            .expect("the program starts");
        let signal = match out.status.code() {
            Some(0) => Signal::Success,
            Some(2) => Signal::Rejected,
            _ => Signal::Unavailable,
        };
        Shown {
            signal,
            text: format!(
                "{}\n{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
        }
    }
}

/// The terminal UI: the block its `installation::run` produces is the text,
/// and its first line says how the verb went.
struct Block {
    registry: cronus_core::invocable::InvocableRegistry,
    dispatcher: cronus_core::invocable::Dispatcher,
}

impl Block {
    fn composed() -> Self {
        let (registry, dispatcher) =
            cronus_core::invocable_bootstrap::bootstrap(cronus_core::Engine::new());
        Self {
            registry,
            dispatcher,
        }
    }
}

impl InstallationRendering for Block {
    fn show(&self, fixture: &InstallationFixture) -> Shown {
        let line = format!("/{}", fixture.words.join(" "));
        let slash = command::parse(&line).expect("a well-formed slash line");
        let setting =
            cronus_tui::installation::Setting::live(cronus_tui::installation::Placement::Engine);
        let Some(Step::Done(block)): Option<Step> =
            cronus_tui::installation::prepare(&self.registry, &self.dispatcher, &slash, &setting)
        else {
            panic!("an inspection is answered, not asked about: {line}");
        };
        let first = block.lines.first().map(String::as_str).unwrap_or_default();
        let signal = if first.starts_with("unavailable:") {
            Signal::Unavailable
        } else if first.starts_with("rejected:") {
            Signal::Rejected
        } else {
            Signal::Success
        };
        Shown {
            signal,
            text: block.lines.join("\n"),
        }
    }
}

/// What is on disk under a root: every path, whether it is a directory, and its
/// size. An inspection that creates, removes or rewrites anything shows up as
/// a difference between two of these.
fn tree(root: &Path) -> Vec<(String, bool, u64)> {
    fn walk(root: &Path, dir: &Path, into: &mut Vec<(String, bool, u64)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .display()
                .to_string();
            into.push((
                relative,
                meta.is_dir(),
                if meta.is_dir() { 0 } else { meta.len() },
            ));
            if meta.is_dir() {
                walk(root, &path, into);
            }
        }
    }
    let mut found = Vec::new();
    walk(root, root, &mut found);
    found.sort();
    found
}

/// A rendering that also watches the disk: the live-effect class of an
/// inspection is that it leaves the installation and every store it reads
/// exactly as they were, and a verb that quietly creates a directory is
/// misclassified.
struct Watched<'a, R> {
    inner: R,
    root: &'a Path,
    disturbed: RefCell<Vec<String>>,
}

impl<'a, R> Watched<'a, R> {
    fn new(inner: R, root: &'a Path) -> Self {
        Self {
            inner,
            root,
            disturbed: RefCell::default(),
        }
    }
}

impl<R: InstallationRendering> InstallationRendering for Watched<'_, R> {
    fn show(&self, fixture: &InstallationFixture) -> Shown {
        let before = tree(self.root);
        let shown = self.inner.show(fixture);
        let after = tree(self.root);
        if before != after {
            let added: Vec<_> = after.iter().filter(|e| !before.contains(e)).collect();
            let removed: Vec<_> = before.iter().filter(|e| !after.contains(e)).collect();
            self.disturbed.borrow_mut().push(format!(
                "{}: added {added:?}, removed {removed:?}",
                fixture.name
            ));
        }
        shown
    }
}

fn fresh_home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cronus-renderings-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("creatable");
    dir
}

#[test]
fn the_command_line_and_the_terminal_ui_give_one_installation_outcome() {
    let home = fresh_home("both");
    let previous = std::env::current_dir().expect("a working directory");
    // SAFETY: this binary holds exactly one test, so nothing else reads or
    // writes the environment while it runs.
    unsafe { std::env::set_var("CRONUS_PORTABLE_DIR", home.join("portable")) };
    std::env::set_current_dir(&home).expect("enterable");

    let fixtures = installation_fixtures();
    let in_state = |state: InstallationState| -> Vec<InstallationFixture> {
        fixtures
            .iter()
            .filter(|f| f.state == state)
            .cloned()
            .collect()
    };
    let block = Watched::new(Block::composed(), &home);
    let line = Watched::new(CommandLine { home: &home }, &home);
    let mut reports = Vec::new();
    let mut run = |label: &str, state: InstallationState| {
        let wanted = in_state(state);
        for (surface, found) in [
            (
                "command line",
                check_installation_renderings(&line, &wanted),
            ),
            (
                "terminal UI",
                check_installation_renderings(&block, &wanted),
            ),
        ] {
            reports.extend(
                found
                    .into_iter()
                    .map(|r| format!("{label} · {surface}: {r:?}")),
            );
        }
    };

    run("fresh", InstallationState::Fresh);
    let before_init = tree(&home);
    let init = Command::new(env!("CARGO_BIN_EXE_cronus"))
        .arg("init")
        .current_dir(&home)
        .env("CRONUS_PORTABLE_DIR", home.join("portable"))
        .output()
        .expect("the program starts");
    assert!(init.status.success(), "init must succeed: {init:?}");
    assert_ne!(
        before_init,
        tree(&home),
        "the watch must see what an installing verb writes, or it proves nothing"
    );
    run("initialized", InstallationState::Initialized);

    let disturbed: Vec<String> = [
        ("command line", &line.disturbed),
        ("terminal UI", &block.disturbed),
    ]
    .into_iter()
    .flat_map(|(surface, seen)| {
        seen.take()
            .into_iter()
            .map(move |what| format!("{surface} · {what}"))
    })
    .collect();

    std::env::set_current_dir(previous).expect("restorable");
    unsafe { std::env::remove_var("CRONUS_PORTABLE_DIR") };
    let _ = std::fs::remove_dir_all(&home);

    assert!(
        reports.is_empty(),
        "the two renderings diverge from the shared fixtures:\n{}",
        reports.join("\n")
    );
    assert!(
        disturbed.is_empty(),
        "an inspection must leave the installation as it found it:\n{}",
        disturbed.join("\n")
    );
}
