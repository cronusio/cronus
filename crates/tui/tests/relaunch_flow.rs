//! A change applied by relaunch rebuilds the terminal UI onto the same view —
//! and only the engine can do it.
//!
//! These drive the real loop ([`cronus_tui::run_with`]) over the real
//! composition ([`cronus_tui::compose_live`], seat and all), with a scripted
//! terminal that watches what is drawn and answers it: it types the next line
//! only when the screen shows what the person would have waited for. The state
//! root is an isolated directory, so the change is really made (a backup is
//! really restored) without touching the machine's own.
//!
//! The portable-directory variable is process-global, so the tests take turns.

use std::collections::VecDeque;
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use cronus_core::endpoint::{DEFAULT_PATIENCE, Role, join_composition};
use cronus_core::invocable::{Dispatcher, InvocableRegistry};
use cronus_tui::installation::Placement;
use cronus_tui::{
    CoreSnapshot, Focus, Key, Renderer, Screen, SnapshotSource, TermEvent, TerminalBackend,
    ViewModel,
};

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// Take the process-global environment for the length of one test, pointing the
/// state root at a directory of its own.
fn isolated(tag: &str) -> (MutexGuard<'static, ()>, std::path::PathBuf) {
    let turn = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
    let base = std::env::temp_dir().join(format!("cronus-tui-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("creatable");
    // SAFETY: the tests in this file take turns.
    unsafe { std::env::set_var("CRONUS_PORTABLE_DIR", &base) };
    let root = cronus_core::paths::resolve_workspace_root();
    std::fs::create_dir_all(&root).expect("creatable");
    (turn, base)
}

fn finish(base: &std::path::Path) {
    unsafe { std::env::remove_var("CRONUS_PORTABLE_DIR") };
    let _ = std::fs::remove_dir_all(base);
}

/// What the script can see: the last frame drawn, and how the loop has been
/// composed so far.
#[derive(Default)]
struct Observed {
    frame: Mutex<Option<ViewModel>>,
    placements: Mutex<Vec<Placement>>,
    released: AtomicUsize,
}

impl Observed {
    fn frame(&self) -> Option<ViewModel> {
        self.frame
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn composed(&self) -> usize {
        self.placements().len()
    }

    fn placements(&self) -> Vec<Placement> {
        self.placements
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The lines of the result block on screen, when its title is `title`.
    fn block_titled(&self, title: &str) -> Option<Vec<String>> {
        let block = self.frame()?.result_block?;
        (block.title == title).then_some(block.lines)
    }
}

struct Mirror(Arc<Observed>);

impl Renderer for Mirror {
    fn draw(&mut self, view: &ViewModel) -> io::Result<()> {
        *self.0.frame.lock().unwrap_or_else(PoisonError::into_inner) = Some(view.clone());
        Ok(())
    }
}

type Until = Box<dyn Fn(&Observed) -> bool>;
type Types = Box<dyn Fn(&Observed) -> Vec<Key>>;

struct Step {
    name: &'static str,
    until: Until,
    types: Types,
}

/// A terminal that types the next step once the screen shows what that step
/// waits for. A script stuck at a step fails the test, naming the step.
struct Scripted {
    steps: VecDeque<Step>,
    keys: VecDeque<Key>,
    seen: Arc<Observed>,
    started: Instant,
}

impl TerminalBackend for Scripted {
    fn enter(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn leave(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn size(&self) -> io::Result<(u16, u16)> {
        Ok((120, 40))
    }

    fn poll_event(&mut self, _timeout: Duration) -> io::Result<Option<TermEvent>> {
        if let Some(key) = self.keys.pop_front() {
            return Ok(Some(TermEvent::Key(key)));
        }
        let seen = &self.seen;
        if let Some(step) = self.steps.pop_front_if(|step| (step.until)(seen)) {
            self.keys.extend((step.types)(seen));
            return Ok(None);
        }
        if self.started.elapsed() > Duration::from_secs(60) {
            let waiting = self.steps.front().map(|step| step.name);
            panic!("the script was still waiting at {waiting:?}");
        }
        std::thread::sleep(Duration::from_millis(2));
        Ok(None)
    }
}

/// Counts how many compositions have been let go.
struct Released(Arc<Observed>);

impl Drop for Released {
    fn drop(&mut self) {
        self.0.released.fetch_add(1, Ordering::SeqCst);
    }
}

/// What the loop reads its snapshots through: none, so the screen is only what
/// the commands show.
struct NoSnapshots;

impl SnapshotSource for NoSnapshots {
    fn poll_snapshot(
        &mut self,
        _registry: &Arc<InvocableRegistry>,
        _dispatcher: &Arc<Dispatcher>,
    ) -> Option<CoreSnapshot> {
        None
    }
}

fn line(text: &str) -> Vec<Key> {
    text.chars()
        .map(Key::Char)
        .chain(std::iter::once(Key::Enter))
        .collect()
}

/// Run the real loop over the real composition until the script quits it,
/// noting each composition's placement and when it is let go.
fn run_script(steps: VecDeque<Step>, seen: &Arc<Observed>) {
    let backend = Scripted {
        steps,
        keys: VecDeque::new(),
        seen: Arc::clone(seen),
        started: Instant::now(),
    };
    let on_compose = Arc::clone(seen);
    cronus_tui::run_with(backend, Mirror(Arc::clone(seen)), move || {
        // The one this replaces has been let go before this one is made.
        let made = on_compose.composed();
        assert_eq!(
            on_compose.released.load(Ordering::SeqCst),
            made,
            "composition {made} began before the previous one was let go"
        );
        let hosted = cronus_tui::compose_live()?;
        on_compose
            .placements
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(hosted.setting.placement());
        Ok(cronus_tui::Hosted {
            registry: hosted.registry,
            dispatcher: hosted.dispatcher,
            catalog: hosted.catalog,
            // No background read: the screen is only what the commands show.
            source: NoSnapshots,
            setting: hosted.setting,
            // The role goes first, so the count says the seat is already free.
            keep: Box::new((hosted.keep, Released(Arc::clone(&on_compose)))),
        })
    })
    .expect("the loop runs to its end");
}

#[test]
fn a_restore_applied_by_relaunch_rebuilds_the_ui_onto_the_same_view() {
    let (_turn, base) = isolated("relaunch");
    let root = cronus_core::paths::resolve_workspace_root();
    let marker = root.join("marker.txt");
    std::fs::write(&marker, "as it was").expect("writable");

    let seen = Arc::new(Observed::default());
    let steps = VecDeque::from([
        // Back up the state as it is now.
        Step {
            name: "ask for a backup",
            until: Box::new(|seen| seen.frame().is_some()),
            types: Box::new(|_| {
                // The Chat screen, where a slash line is typed whole.
                let mut keys = vec![Key::F(3)];
                keys.extend(line("/backup create"));
                keys
            }),
        },
        Step {
            name: "the backup is asked about",
            until: Box::new(|seen| seen.block_titled("confirm /backup create").is_some()),
            types: Box::new(|_| vec![Key::Char('y')]),
        },
        // Change the state, then restore the backup over it.
        Step {
            name: "the backup is made",
            until: Box::new(|seen| {
                seen.block_titled("/backup create")
                    .is_some_and(|lines| lines.iter().any(|l| l.starts_with("id: ")))
            }),
            types: Box::new({
                let marker = marker.clone();
                move |seen| {
                    std::fs::write(&marker, "changed since").expect("writable");
                    let lines = seen.block_titled("/backup create").expect("made");
                    let id = lines
                        .iter()
                        .find_map(|l| l.strip_prefix("id: "))
                        .expect("the backup has an id")
                        .trim()
                        .to_string();
                    line(&format!("/restore {id}"))
                }
            }),
        },
        Step {
            name: "the restore is asked about",
            until: Box::new(|seen| {
                seen.frame().is_some_and(|frame| {
                    frame
                        .result_block
                        .is_some_and(|block| block.title.starts_with("confirm /restore "))
                })
            }),
            types: Box::new(|_| vec![Key::Char('y')]),
        },
        // The loop rebuilds itself and carries on onto the same view.
        Step {
            name: "the composition is rebuilt and the answer is still showing",
            until: Box::new(|seen| {
                seen.composed() >= 2
                    && seen.frame().is_some_and(|frame| {
                        frame.result_block.is_some_and(|block| {
                            block
                                .lines
                                .iter()
                                .any(|l| l == "restarting onto the changed installation")
                        })
                    })
            }),
            types: Box::new(|_| vec![Key::F(10)]),
        },
    ]);
    run_script(steps, &seen);

    let restored = std::fs::read_to_string(&marker);
    let last = seen.frame().expect("something was drawn");
    finish(&base);

    assert_eq!(
        seen.placements(),
        vec![Placement::Engine, Placement::Engine],
        "composed once to start and once after the relaunch, the engine both times: \
         the seat was let go and taken again"
    );
    assert_eq!(
        seen.released.load(Ordering::SeqCst),
        2,
        "both compositions were let go by the end"
    );
    assert_eq!(
        restored.as_deref().ok(),
        Some("as it was"),
        "the restore really put the backed-up state back"
    );
    assert_eq!(
        last.screen,
        Screen::Chat,
        "the relaunch carried the person's screen over"
    );
    assert_eq!(last.focus, Focus::CommandBar);
}

/// A relaunch is the engine's to make. A terminal UI attached to the engine of
/// another session would rebuild itself and leave the engine as it was, so it
/// is not asked about, and nothing is restored.
#[test]
fn a_terminal_ui_attached_to_another_engine_leaves_a_relaunch_to_that_session() {
    let (_turn, base) = isolated("spoke");
    let root = cronus_core::paths::resolve_workspace_root();
    let marker = root.join("marker.txt");
    std::fs::write(&marker, "untouched").expect("writable");

    // Another session is the engine of this state root.
    let (registry, dispatcher) =
        cronus_core::invocable_bootstrap::bootstrap(cronus_core::Engine::new());
    let engine = join_composition(
        &root,
        registry,
        dispatcher,
        cronus_contract::Surface::Tui,
        DEFAULT_PATIENCE,
    )
    .expect("the first process to ask is the engine");
    assert!(matches!(engine.role, Role::Engine(_)));

    let seen = Arc::new(Observed::default());
    let steps = VecDeque::from([
        Step {
            name: "ask for a restore",
            until: Box::new(|seen| seen.frame().is_some()),
            types: Box::new(|_| {
                let mut keys = vec![Key::F(3)];
                keys.extend(line("/restore backup-1"));
                keys
            }),
        },
        Step {
            name: "the restore is answered, not asked about",
            until: Box::new(|seen| seen.block_titled("/restore backup-1").is_some()),
            types: Box::new(|_| vec![Key::F(10)]),
        },
    ]);
    run_script(steps, &seen);

    let answer = seen.block_titled("/restore backup-1");
    let kept = std::fs::read_to_string(&marker);
    drop(engine);
    finish(&base);

    assert_eq!(seen.placements(), vec![Placement::Spoke]);
    let answer = answer.expect("the answer is still showing");
    assert!(
        answer[0].starts_with("not run from here — class: recompose, applied by relaunch"),
        "{answer:?}"
    );
    assert!(
        answer.iter().any(|l| l.contains("another session")),
        "{answer:?}"
    );
    assert_eq!(kept.as_deref().ok(), Some("untouched"));
}
