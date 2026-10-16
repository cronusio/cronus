//! Terminal backend and raw-mode lifecycle for the TUI.
//!
//! This module is pure terminal plumbing — it performs no core calls and holds
//! no domain state. The render loop drives it; panels render through it.
//!
//! The backend is abstracted behind [`TerminalBackend`] so the lifecycle can be
//! exercised without a real TTY: the production path drives `crossterm`, while
//! tests inject a recording fake. Raw mode and the alternate screen cannot be
//! entered in a headless test runner, so the trait seam is the only way to keep
//! the enter/restore contract under test.

use std::io::{self, Write};
use std::time::Duration;

/// A terminal-sourced event the render loop reacts to.
///
/// The variants are deliberately minimal — only what the loop (resize → redraw)
/// and the command bar (text entry, submission, cancellation) need. Mapping from
/// the backend's native event type happens inside the backend, keeping this
/// vocabulary independent of any one terminal library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TermEvent {
    /// A key press, already folded to the essentials the UI consumes.
    Key(Key),
    /// The terminal was resized to `(cols, rows)`. The loop turns this into a
    /// redraw request; the new size is authoritative.
    Resize(u16, u16),
}

/// The folded key vocabulary the TUI acts on.
///
/// Unmapped physical keys are dropped at the backend boundary rather than
/// surfaced as a catch-all, so consumers never branch on keys they ignore.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A printable character.
    Char(char),
    /// Enter / Return — command submission.
    Enter,
    /// Backspace — delete the previous character.
    Backspace,
    /// Escape — cancel the current input / dismiss.
    Esc,
    /// Tab — forward focus cycling.
    Tab,
    /// Shift+Tab — backward focus cycling.
    BackTab,
    /// Arrow up.
    Up,
    /// Arrow down.
    Down,
    /// Arrow left.
    Left,
    /// Arrow right.
    Right,
    /// A function key, `F(1)` … `F(10)` (the bar at the bottom of the
    /// screen names them).
    F(u8),
}

/// Abstraction over the host terminal: lifecycle, size, and event polling.
///
/// Implementors must make [`leave`](TerminalBackend::leave) safe to call more
/// than once; the [`Tui`] guard already suppresses redundant calls, but a
/// defensive `leave` keeps direct use sound.
pub trait TerminalBackend {
    /// Switch into raw mode and the alternate screen.
    fn enter(&mut self) -> io::Result<()>;

    /// Restore cooked mode and leave the alternate screen. Idempotent.
    fn leave(&mut self) -> io::Result<()>;

    /// Current terminal size as `(cols, rows)`.
    fn size(&self) -> io::Result<(u16, u16)>;

    /// Block up to `timeout` for the next event. `Ok(None)` means the timeout
    /// elapsed with nothing actionable — the loop uses this as its tick.
    fn poll_event(&mut self, timeout: Duration) -> io::Result<Option<TermEvent>>;
}

/// RAII guard owning the terminal lifecycle.
///
/// Construction enters raw mode + the alternate screen; `Drop` restores them.
/// Because restoration runs in `Drop`, it happens on every exit path — normal
/// return, early `?` error, and stack unwinding from a panic — so the terminal
/// is never left in raw mode for the user's shell.
pub struct Tui<B: TerminalBackend> {
    backend: B,
    active: bool,
}

impl<B: TerminalBackend> Tui<B> {
    /// Enter raw mode + the alternate screen, returning the guard. On entry
    /// failure the backend is dropped without a paired `leave`, matching the
    /// fact that entry did not complete.
    pub fn new(mut backend: B) -> io::Result<Self> {
        backend.enter()?;
        Ok(Self {
            backend,
            active: true,
        })
    }

    /// Current terminal size as `(cols, rows)`.
    pub fn size(&self) -> io::Result<(u16, u16)> {
        self.backend.size()
    }

    /// Poll for the next terminal event, blocking up to `timeout`.
    pub fn poll_event(&mut self, timeout: Duration) -> io::Result<Option<TermEvent>> {
        self.backend.poll_event(timeout)
    }

    /// Restore the terminal eagerly. Safe to call repeatedly; the paired `Drop`
    /// becomes a no-op afterwards.
    pub fn restore(&mut self) -> io::Result<()> {
        if self.active {
            self.active = false;
            self.backend.leave()
        } else {
            Ok(())
        }
    }
}

impl<B: TerminalBackend> Drop for Tui<B> {
    fn drop(&mut self) {
        // Best-effort teardown: during unwinding we cannot propagate an error,
        // and failing to restore would still leave the terminal usable enough
        // for the shell to recover, so the result is intentionally discarded.
        let _ = self.restore();
    }
}

/// Production [`TerminalBackend`] driving `crossterm` over stdout.
pub struct CrosstermBackend {
    out: io::Stdout,
}

impl CrosstermBackend {
    /// Construct a backend writing to the process stdout.
    pub fn new() -> Self {
        Self { out: io::stdout() }
    }
}

impl Default for CrosstermBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalBackend for CrosstermBackend {
    fn enter(&mut self) -> io::Result<()> {
        crossterm::terminal::enable_raw_mode()?;
        crossterm::execute!(self.out, crossterm::terminal::EnterAlternateScreen)?;
        self.out.flush()
    }

    fn leave(&mut self) -> io::Result<()> {
        crossterm::execute!(self.out, crossterm::terminal::LeaveAlternateScreen)?;
        crossterm::terminal::disable_raw_mode()?;
        self.out.flush()
    }

    fn size(&self) -> io::Result<(u16, u16)> {
        crossterm::terminal::size()
    }

    fn poll_event(&mut self, timeout: Duration) -> io::Result<Option<TermEvent>> {
        use crossterm::event::{self, Event, KeyCode, KeyEventKind};

        if !event::poll(timeout)? {
            return Ok(None);
        }
        match event::read()? {
            Event::Resize(cols, rows) => Ok(Some(TermEvent::Resize(cols, rows))),
            // On Windows a key produces both Press and Release; act on Press only
            // so a single physical keystroke is not handled twice.
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                let folded = match key.code {
                    KeyCode::Char(c) => Some(Key::Char(c)),
                    KeyCode::Enter => Some(Key::Enter),
                    KeyCode::Backspace => Some(Key::Backspace),
                    KeyCode::Esc => Some(Key::Esc),
                    KeyCode::Tab => Some(Key::Tab),
                    KeyCode::BackTab => Some(Key::BackTab),
                    KeyCode::Up => Some(Key::Up),
                    KeyCode::Down => Some(Key::Down),
                    KeyCode::Left => Some(Key::Left),
                    KeyCode::Right => Some(Key::Right),
                    KeyCode::F(n @ 1..=10) => Some(Key::F(n)),
                    // Unmapped keys are not part of the TUI's vocabulary.
                    _ => None,
                };
                Ok(folded.map(TermEvent::Key))
            }
            // Other events (release keys, focus, paste, mouse) are not consumed.
            _ => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::panic::{self, AssertUnwindSafe};
    use std::sync::{Arc, Mutex};

    /// Lifecycle operations the fake records, in call order.
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Op {
        Enter,
        Leave,
    }

    /// A recording fake terminal: it logs lifecycle calls to a shared buffer so
    /// the log survives the guard's `Drop`, and replays a queued event script.
    #[derive(Clone)]
    struct FakeBackend {
        log: Arc<Mutex<Vec<Op>>>,
        events: Arc<Mutex<VecDeque<TermEvent>>>,
        size: (u16, u16),
    }

    impl FakeBackend {
        fn new() -> Self {
            Self {
                log: Arc::new(Mutex::new(Vec::new())),
                events: Arc::new(Mutex::new(VecDeque::new())),
                size: (80, 24),
            }
        }

        fn with_events(events: Vec<TermEvent>) -> Self {
            let me = Self::new();
            *me.events.lock().unwrap() = events.into();
            me
        }

        fn log(&self) -> Vec<Op> {
            self.log.lock().unwrap().clone()
        }
    }

    impl TerminalBackend for FakeBackend {
        fn enter(&mut self) -> io::Result<()> {
            self.log.lock().unwrap().push(Op::Enter);
            Ok(())
        }

        fn leave(&mut self) -> io::Result<()> {
            self.log.lock().unwrap().push(Op::Leave);
            Ok(())
        }

        fn size(&self) -> io::Result<(u16, u16)> {
            Ok(self.size)
        }

        fn poll_event(&mut self, _timeout: Duration) -> io::Result<Option<TermEvent>> {
            Ok(self.events.lock().unwrap().pop_front())
        }
    }

    #[test]
    fn terminal_lifecycle_enters_then_restores_on_drop() {
        let backend = FakeBackend::new();
        let probe = backend.clone();
        {
            let _tui = Tui::new(backend).expect("enter raw mode");
            // Still inside the alternate screen: only the enter has run.
            assert_eq!(probe.log(), vec![Op::Enter]);
        }
        // Dropping the guard restored cooked mode + left the alternate screen.
        assert_eq!(probe.log(), vec![Op::Enter, Op::Leave]);
    }

    #[test]
    fn terminal_lifecycle_restores_on_panic() {
        let backend = FakeBackend::new();
        let probe = backend.clone();

        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            let _tui = Tui::new(backend).expect("enter raw mode");
            panic!("simulated render-loop crash");
        }));

        assert!(result.is_err(), "the panic should propagate out");
        // The RAII guard ran restore while unwinding — no leaked raw mode.
        assert_eq!(probe.log(), vec![Op::Enter, Op::Leave]);
    }

    #[test]
    fn terminal_lifecycle_restore_is_idempotent() {
        let backend = FakeBackend::new();
        let probe = backend.clone();

        let mut tui = Tui::new(backend).expect("enter raw mode");
        tui.restore().expect("eager restore");
        drop(tui); // Drop must not leave a second time.

        assert_eq!(probe.log(), vec![Op::Enter, Op::Leave]);
    }

    #[test]
    fn terminal_lifecycle_surfaces_resize_events() {
        let backend = FakeBackend::with_events(vec![TermEvent::Resize(120, 40)]);
        let mut tui = Tui::new(backend).expect("enter raw mode");

        let event = tui
            .poll_event(Duration::from_millis(0))
            .expect("poll succeeds");

        assert_eq!(event, Some(TermEvent::Resize(120, 40)));
    }
}
