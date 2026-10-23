//! The diagnostic log: the product's record of what it did, written for a
//! reader who arrives a day later.
//!
//! Library crates emit `tracing` events and never install a subscriber. A
//! binary calls [`install`] as its first act, which wires one subscriber over
//! two projections of every event:
//!
//! * the **file** — one JSON object per line ([`line`]), every field, written
//!   through a bounded non-blocking queue to a size-rotating file family
//!   `<logs>/<kind>.jsonl` ([`rotate`]); when the queue is full events are
//!   dropped and counted, and the count is written as one `log.dropped` event
//!   as soon as the queue accepts a line again;
//! * the **console** — one human line per event on standard error at `warn`
//!   and above, for one-shot command-line runs only. The terminal UI owns the
//!   terminal and installs none.
//!
//! The sink is installed before configuration resolves, so the product's own
//! verbosity starts at the default and [`DiagnosticsGuard::apply_level`] moves
//! it once the `log.level` setting is known. Nothing here can fail the work it
//! describes: a log directory that cannot be written costs lines, not errors.

pub mod line;
pub mod project;
pub mod rotate;

use std::io::Write;
use std::panic::{self, PanicHookInfo};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::time::{Duration, Instant, SystemTime};

use serde_json::{Map, Value};
use tracing::dispatcher::Dispatch;
use tracing::{Event, Subscriber};
use tracing_appender::non_blocking::{ErrorCounter, NonBlocking, NonBlockingBuilder, WorkerGuard};
use tracing_subscriber::filter::{EnvFilter, LevelFilter};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};
use tracing_subscriber::{Registry, reload};

use line::{Attribution, Fields};
use project::{Entry, Folded, Folder, Payload, PayloadPolicy, Projector};

/// Environment variable that replaces the product's own filter directives.
pub const FILTER_ENV: &str = "CRONUS_LOG";

/// Target used by events the diagnostics module itself emits.
const SELF_TARGET: &str = "cronus_core::diagnostics";

/// Everything that is not the product's own code is held at this level unless
/// a directive names it: raising the product's verbosity must not flood the
/// file with its dependencies.
const DEPENDENCY_FLOOR: &str = "warn";

/// The product's own default.
const DEFAULT_OWN: &str = "cronus=info";

/// What kind of process is writing: one file family per kind, so a long-lived
/// engine and short-lived command-line runs never contend for one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessKind {
    /// A one-shot `cronus <group> <verb>` run.
    Cli,
    /// The interactive terminal UI.
    Tui,
    /// The long-running engine. Declared ahead of its entry point; nothing
    /// installs this kind yet.
    Engine,
    /// The desktop shell's host process.
    Desktop,
}

impl ProcessKind {
    /// The `kind` carried by every line and the file family's name.
    pub fn as_str(self) -> &'static str {
        match self {
            ProcessKind::Cli => "cli",
            ProcessKind::Tui => "tui",
            ProcessKind::Engine => "engine",
            ProcessKind::Desktop => "desktop",
        }
    }

    /// Whether this kind writes warnings to standard error. Only the one-shot
    /// command line does: the terminal UI would have its screen corrupted by a
    /// stray line, and the engine and the desktop host have no terminal of
    /// their own.
    pub fn has_console(self) -> bool {
        matches!(self, ProcessKind::Cli)
    }
}

/// Tunables the caller may set before the sink exists.
#[derive(Debug, Clone)]
pub struct Settings {
    /// Size at which the active file rotates (`log.max_size`).
    pub max_size: u64,
    /// Rotations kept beside the active file (`log.max_files`).
    pub max_files: usize,
    /// Lines the non-blocking queue holds before it starts dropping.
    pub queue_lines: usize,
    /// Directives for the product's own scope, replacing the default
    /// `cronus=info`. The dependency floor is applied separately.
    pub filter: Option<String>,
    /// How long a run of identical events may be held back in the hope of
    /// folding the next one into it. A quiet process still writes its last
    /// event within about twice this.
    pub fold_hold: Duration,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            max_size: rotate::DEFAULT_MAX_SIZE,
            max_files: rotate::DEFAULT_MAX_FILES,
            queue_lines: 8192,
            filter: None,
            fold_hold: Duration::from_millis(250),
        }
    }
}

impl Settings {
    /// Defaults, with [`FILTER_ENV`] applied when it is set and not blank.
    pub fn from_env() -> Self {
        Settings {
            filter: std::env::var(FILTER_ENV)
                .ok()
                .filter(|value| !value.trim().is_empty()),
            ..Settings::default()
        }
    }
}

fn directives(own: &str) -> String {
    format!("{DEPENDENCY_FLOOR},{own}")
}

/// Keeps the sink alive and flushes it when dropped.
#[must_use = "dropping the guard stops the log writer and flushes its queue"]
pub struct DiagnosticsGuard {
    // Declared first so the folded run is flushed into the queue (in `drop`)
    // before the worker below is told to drain it.
    shared: Arc<FileShared>,
    projector: Arc<Projector>,
    _worker: WorkerGuard,
    reload: reload::Handle<EnvFilter, Registry>,
    session: String,
    kind: ProcessKind,
    env_override: bool,
    filter: String,
    filter_error: Option<String>,
    active: bool,
}

impl std::fmt::Debug for DiagnosticsGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiagnosticsGuard")
            .field("kind", &self.kind)
            .field("session", &self.session)
            .field("filter", &self.filter)
            .field("active", &self.active)
            .finish_non_exhaustive()
    }
}

impl Drop for DiagnosticsGuard {
    fn drop(&mut self) {
        self.shared.flush_all();
        self.shared.stop();
    }
}

impl DiagnosticsGuard {
    /// This process's run id, the `session` carried by every line.
    pub fn session(&self) -> &str {
        &self.session
    }

    /// The kind of process this sink was installed for.
    pub fn kind(&self) -> ProcessKind {
        self.kind
    }

    /// The directives in force when the sink was installed.
    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// Whether this sink is the process's subscriber. It is not when another
    /// subscriber was already installed; the guard is then inert.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Hand the sink the live secret list, the same one the dispatcher masks
    /// outcomes against. Every projection masks these values wherever they
    /// appear in a message or a text field. Until a caller supplies a list the
    /// log protects secrets by field name only (see [`project`]).
    pub fn set_secrets(&self, secrets: Vec<String>) {
        self.projector.set_secrets(secrets);
    }

    /// Apply the `log.payload` setting.
    pub fn set_payload(&self, payload: Payload) {
        self.projector.set_payload(payload);
    }

    /// Apply the `log.level` setting to the product's own scope once
    /// configuration has resolved.
    ///
    /// Returns `Ok(true)` when applied and `Ok(false)` when [`FILTER_ENV`]
    /// holds the filter — the environment overrides the setting — and `Err`
    /// when `level` is not a level name.
    pub fn apply_level(&self, level: &str) -> Result<bool, String> {
        let level = level.trim().to_ascii_lowercase();
        if !["error", "warn", "info", "debug", "trace", "off"].contains(&level.as_str()) {
            return Err(format!("`{level}` is not a log level"));
        }
        if self.env_override {
            return Ok(false);
        }
        let filter = EnvFilter::try_new(directives(&format!("cronus={level}")))
            .map_err(|error| error.to_string())?;
        self.reload
            .reload(filter)
            .map(|()| true)
            .map_err(|error| error.to_string())
    }
}

/// When an entry was seen, on the wall clock for the line and on the monotonic
/// clock for how long it has been held.
#[derive(Debug, Clone, Copy)]
struct Stamp {
    wall: SystemTime,
    mono: Instant,
}

impl Stamp {
    fn now() -> Self {
        Stamp {
            wall: SystemTime::now(),
            mono: Instant::now(),
        }
    }
}

struct FileState {
    /// How many dropped lines have already been told in a notice.
    reported: usize,
    /// The run of identical events not yet written.
    folder: Folder<Stamp>,
}

/// The file projection's writing side, shared by the layer, the guard and the
/// thread that releases a held run when nothing else arrives to end it.
struct FileShared {
    writer: NonBlocking,
    counter: ErrorCounter,
    who: Attribution,
    state: Mutex<FileState>,
    hold: Duration,
    wake: mpsc::Sender<()>,
    stopped: AtomicBool,
}

impl FileShared {
    fn state(&self) -> MutexGuard<'_, FileState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Record one event. An identical event extends the held run; a different
    /// one writes the run it ends and is held in turn. An error, and the
    /// module's own lifecycle events, are written at once and never held: they
    /// may be the last thing the process says.
    fn record(&self, entry: Entry) {
        let at = Stamp::now();
        let mut state = self.state();
        if entry.level == "ERROR" || entry.target == SELF_TARGET {
            if let Some(run) = state.folder.take() {
                self.write(&mut state, run);
            }
            let single = Folded {
                entry,
                first: at,
                last: at,
                count: 1,
            };
            self.write(&mut state, single);
            return;
        }
        let was_empty = state.folder.is_empty();
        let ended = state.folder.offer(entry, at);
        let begins_a_run = ended.is_some() || was_empty;
        if let Some(run) = ended {
            self.write(&mut state, run);
        }
        if begins_a_run {
            let _ = self.wake.send(());
        }
    }

    /// Write the held run if it has waited long enough. Returns whether a run
    /// is still held afterwards.
    fn flush_due(&self) -> bool {
        let mut state = self.state();
        let hold = self.hold;
        if let Some(run) = state
            .folder
            .take_if(|held| held.first.mono.elapsed() >= hold)
        {
            self.write(&mut state, run);
        }
        !state.folder.is_empty()
    }

    fn flush_all(&self) {
        let mut state = self.state();
        if let Some(run) = state.folder.take() {
            self.write(&mut state, run);
        }
    }

    fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        let _ = self.wake.send(());
    }

    /// Render a run as one line and put it on the queue, telling first how
    /// many lines the queue has had to drop since it last had room.
    fn write(&self, state: &mut FileState, run: Folded<Stamp>) {
        let mut fields = run.entry.fields;
        if run.count > 1 {
            fields.insert("repeat".to_owned(), Value::from(run.count));
            fields.insert(
                "last_ts".to_owned(),
                Value::String(line::utc_millis(run.last.wall)),
            );
        }
        let rendered = line::render_parts(
            &self.who,
            &run.entry.level,
            &run.entry.target,
            &run.entry.msg,
            fields,
            run.first.wall,
        );

        let mut writer = self.writer.clone();
        let dropped = self.counter.dropped_lines();
        if dropped > state.reported {
            let mut fields = Map::new();
            fields.insert("count".to_owned(), Value::from(dropped - state.reported));
            let notice = line::render_parts(
                &self.who,
                "WARN",
                SELF_TARGET,
                "log.dropped",
                fields,
                SystemTime::now(),
            );
            let before = self.counter.dropped_lines();
            let _ = writer.write_all(notice.as_bytes());
            let after = self.counter.dropped_lines();
            if after == before {
                // The queue took the notice: everything counted so far is told.
                state.reported = dropped;
            } else {
                // The queue refused the notice itself. That refusal is not a
                // lost event, so it must not inflate the count the next
                // attempt reports; the events already counted stay pending.
                state.reported += after - before;
            }
        }
        let _ = writer.write_all(rendered.as_bytes());
    }
}

/// Releases a held run once it has waited `hold`, so a process that goes quiet
/// still writes its last event. It sleeps until a run is first held, and exits
/// when the guard stops it.
fn spawn_release_thread(shared: Arc<FileShared>, woken: mpsc::Receiver<()>) {
    let _ = std::thread::Builder::new()
        .name("cronus-diagnostics-fold".to_owned())
        .spawn(move || {
            while woken.recv().is_ok() {
                while !shared.stopped.load(Ordering::SeqCst) {
                    std::thread::sleep(shared.hold);
                    if !shared.flush_due() {
                        break;
                    }
                }
                if shared.stopped.load(Ordering::SeqCst) {
                    break;
                }
            }
        });
}

/// The file projection: every event as one JSON line through the queue.
struct FileLayer {
    shared: Arc<FileShared>,
    projector: Arc<Projector>,
}

impl<S: Subscriber> Layer<S> for FileLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let fields = self.projector.project(fields, None);
        let metadata = event.metadata();
        self.shared.record(Entry {
            level: metadata.level().as_str().to_owned(),
            target: metadata.target().to_owned(),
            msg: fields.message.unwrap_or_default(),
            fields: fields.map,
        });
    }
}

/// Where console lines go: standard error in the product, a buffer in tests.
pub(crate) type ConsoleSink = Box<dyn Fn(&str) + Send + Sync>;

/// The console projection: one human line per event, never held back and
/// always reduced to descriptors, because a person is reading it as it happens.
struct ConsoleLayer {
    sink: ConsoleSink,
    projector: Arc<Projector>,
}

impl<S: Subscriber> Layer<S> for ConsoleLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let fields = self
            .projector
            .project(fields, Some(PayloadPolicy::Descriptor));
        let metadata = event.metadata();
        let mut text = format!(
            "{:<5} {}: {}",
            metadata.level(),
            metadata.target(),
            fields.message.as_deref().unwrap_or("")
        );
        for (key, value) in &fields.map {
            text.push(' ');
            text.push_str(key);
            text.push('=');
            match value {
                Value::String(plain) => text.push_str(plain),
                other => text.push_str(&other.to_string()),
            }
        }
        text.push('\n');
        (self.sink)(&text);
    }
}

fn stderr_sink(text: &str) {
    let _ = std::io::stderr().lock().write_all(text.as_bytes());
}

/// A subscriber and the guard that keeps its writer alive, before either is
/// made the process's default.
pub(crate) struct Built {
    pub(crate) dispatch: Dispatch,
    pub(crate) guard: DiagnosticsGuard,
}

pub(crate) fn build(
    kind: ProcessKind,
    file: impl Write + Send + 'static,
    console: Option<ConsoleSink>,
    settings: &Settings,
) -> Built {
    let own = settings.filter.as_deref().map(directives);
    let (filter, effective, filter_error) = match own.as_deref().map(EnvFilter::try_new) {
        Some(Ok(filter)) => (filter, own.unwrap_or_default(), None),
        Some(Err(error)) => (
            EnvFilter::new(directives(DEFAULT_OWN)),
            directives(DEFAULT_OWN),
            Some(error.to_string()),
        ),
        None => (
            EnvFilter::new(directives(DEFAULT_OWN)),
            directives(DEFAULT_OWN),
            None,
        ),
    };
    let env_override = settings.filter.is_some() && filter_error.is_none();

    let (writer, worker) = NonBlockingBuilder::default()
        .buffered_lines_limit(settings.queue_lines.max(1))
        .lossy(true)
        .thread_name("cronus-diagnostics")
        .finish(file);
    let who = Attribution {
        kind: kind.as_str().to_owned(),
        session: fresh_session_id(),
    };
    let projector = Arc::new(Projector::default());
    let (wake, woken) = mpsc::channel();
    let shared = Arc::new(FileShared {
        counter: writer.error_counter(),
        writer,
        who: who.clone(),
        state: Mutex::new(FileState {
            reported: 0,
            folder: Folder::new(),
        }),
        hold: settings.fold_hold,
        wake,
        stopped: AtomicBool::new(false),
    });
    spawn_release_thread(Arc::clone(&shared), woken);

    let (filter_layer, reload) = reload::Layer::new(filter);
    let file_layer = FileLayer {
        shared: Arc::clone(&shared),
        projector: Arc::clone(&projector),
    }
    .with_filter(filter_layer);
    let console_layer = console.map(|sink| {
        ConsoleLayer {
            sink,
            projector: Arc::clone(&projector),
        }
        .with_filter(LevelFilter::WARN)
    });
    let subscriber = Registry::default().with(file_layer).with(console_layer);

    Built {
        dispatch: Dispatch::new(subscriber),
        guard: DiagnosticsGuard {
            shared,
            projector,
            _worker: worker,
            reload,
            session: who.session,
            kind,
            env_override,
            filter: effective,
            filter_error,
            active: false,
        },
    }
}

/// 64 bits from the operating system's entropy source, as hex. An identifier
/// for correlating one run's lines, not a secret; if entropy is unavailable it
/// falls back to the process id and the clock.
fn fresh_session_id() -> String {
    let mut bytes = [0u8; 8];
    if getrandom::fill(&mut bytes).is_ok() {
        return bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    }
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    format!("{:x}{nanos:x}", std::process::id())
}

/// Whether a flag or field name looks like it carries a secret.
///
/// The list names what secrets are called, not every word a secret might be
/// stored under: a bare `key` or `session` would also hide cache keys and this
/// log's own `session` id, so key-like names are listed in their qualified
/// forms.
pub(crate) fn is_secret_name(name: &str) -> bool {
    const NEEDLES: [&str; 14] = [
        "token",
        "secret",
        "password",
        "passwd",
        "passphrase",
        "authorization",
        "credential",
        "cookie",
        "bearer",
        "api_key",
        "api-key",
        "apikey",
        "private_key",
        "private-key",
    ];
    let name = name.to_ascii_lowercase();
    NEEDLES.iter().any(|needle| name.contains(needle))
}

/// The longest argument the session header carries whole.
const MAX_LOGGED_ARG: usize = 256;

fn shorten_arg(arg: String) -> String {
    if arg.len() <= MAX_LOGGED_ARG {
        return arg;
    }
    let end = arg
        .char_indices()
        .map(|(index, ch)| index + ch.len_utf8())
        .take_while(|end| *end <= MAX_LOGGED_ARG)
        .last()
        .unwrap_or(0);
    format!("{}…[{} bytes]", &arg[..end], arg.len())
}

/// Replace the values of secret-named flags: `--token x` and `--token=x` both
/// lose the `x`. A secret-named flag followed by another flag has no value to
/// hide and the next flag is left alone.
pub(crate) fn redact_args(args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    let mut hide_next = false;
    for arg in args {
        if hide_next {
            hide_next = false;
            if !arg.starts_with('-') {
                out.push("<redacted>".to_owned());
                continue;
            }
        }
        match arg.strip_prefix('-') {
            Some(flag) => match flag.split_once('=') {
                Some((name, _)) if is_secret_name(name) => {
                    out.push(format!("-{name}=<redacted>"));
                }
                None if is_secret_name(flag) => {
                    out.push(arg.clone());
                    hide_next = true;
                }
                _ => out.push(arg.clone()),
            },
            None => out.push(arg.clone()),
        }
    }
    out
}

/// What a process says about itself as its first event.
pub(crate) struct SessionFacts<'a> {
    pub(crate) args: &'a [String],
    pub(crate) cwd: &'a Path,
    pub(crate) state_root: &'a Path,
    pub(crate) started: SystemTime,
}

/// Write the `session.start` header, and a warning when the requested filter
/// was not usable.
pub(crate) fn emit_session_start(
    dispatch: &Dispatch,
    guard: &DiagnosticsGuard,
    facts: &SessionFacts<'_>,
) {
    let build = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let args = redact_args(facts.args)
        .into_iter()
        .map(shorten_arg)
        .collect::<Vec<_>>()
        .join(" ");
    let started = line::utc_millis(facts.started);
    tracing::dispatcher::with_default(dispatch, || {
        tracing::info!(
            target: "cronus_core::diagnostics",
            os = std::env::consts::OS,
            arch = std::env::consts::ARCH,
            version = env!("CARGO_PKG_VERSION"),
            build = build,
            cwd = %facts.cwd.display(),
            args = %args,
            state_root = %facts.state_root.display(),
            started = %started,
            filter = %guard.filter,
            "session.start"
        );
        if let Some(error) = &guard.filter_error {
            tracing::warn!(
                target: "cronus_core::diagnostics",
                env = FILTER_ENV,
                error = %error,
                "log.filter.invalid"
            );
        }
    });
}

type PreviousHook = Arc<dyn Fn(&PanicHookInfo<'_>) + Send + Sync + 'static>;

/// A panic hook that writes the panic to the log, then runs `previous`.
///
/// The event is queued before the default hook prints, so it reaches the file
/// whenever the process lives long enough to flush: always for a panic on a
/// spawned thread, and through the guard's drop when the main thread unwinds.
pub(crate) fn chained_panic_hook(
    dispatch: Dispatch,
    previous: PreviousHook,
) -> Box<dyn Fn(&PanicHookInfo<'_>) + Send + Sync + 'static> {
    Box::new(move |info| {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .map(|text| (*text).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "non-text panic payload".to_owned());
        let location = info
            .location()
            .map(|place| format!("{}:{}:{}", place.file(), place.line(), place.column()))
            .unwrap_or_else(|| "unknown".to_owned());
        let thread = std::thread::current();
        let backtrace = std::backtrace::Backtrace::force_capture();
        tracing::dispatcher::with_default(&dispatch, || {
            tracing::error!(
                target: "cronus_core::diagnostics",
                thread = thread.name().unwrap_or("unnamed"),
                panic.message = %message,
                panic.location = %location,
                panic.backtrace = %backtrace,
                "panic"
            );
        });
        previous(info);
    })
}

fn install_panic_hook(dispatch: Dispatch) {
    let previous: PreviousHook = Arc::from(panic::take_hook());
    panic::set_hook(chained_panic_hook(dispatch, previous));
}

/// The directory the log files live in: the `<logs>` root, resolved like every
/// other root (so portable mode puts it under the portable directory).
pub fn logs_root() -> std::path::PathBuf {
    cronus_domain::paths::Paths::os_native().resolve(cronus_domain::paths::Root::Logs)
}

/// Install the diagnostic sink for this process: the first statement of every
/// binary's `main`. Reads [`FILTER_ENV`] and uses default sizes; see
/// [`install_with`].
pub fn install(kind: ProcessKind, logs_root: &Path) -> DiagnosticsGuard {
    install_with(kind, logs_root, &Settings::from_env())
}

/// [`install`] with explicit [`Settings`].
///
/// Makes the sink the process's subscriber, writes the `session.start` header
/// and chains a panic hook that logs before the previous hook runs. If another
/// subscriber is already installed the returned guard is inert
/// ([`DiagnosticsGuard::is_active`] is false) and nothing else changes.
pub fn install_with(kind: ProcessKind, logs_root: &Path, settings: &Settings) -> DiagnosticsGuard {
    let file = rotate::RotatingWriter::new(
        logs_root,
        kind.as_str(),
        settings.max_size,
        settings.max_files,
    );
    let console = kind
        .has_console()
        .then(|| Box::new(stderr_sink) as ConsoleSink);
    let Built {
        dispatch,
        mut guard,
    } = build(kind, file, console, settings);
    if tracing::dispatcher::set_global_default(dispatch.clone()).is_err() {
        return guard;
    }
    guard.active = true;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let cwd = std::env::current_dir().unwrap_or_default();
    let state_root =
        cronus_domain::paths::Paths::os_native().resolve(cronus_domain::paths::Root::State);
    emit_session_start(
        &dispatch,
        &guard,
        &SessionFacts {
            args: &args,
            cwd: &cwd,
            state_root: &state_root,
            started: SystemTime::now(),
        },
    );
    install_panic_hook(dispatch);
    guard
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Condvar;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<u8>>>);

    impl Write for Sink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("sink lock").extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Sink {
        fn lines(&self) -> Vec<Value> {
            let bytes = self.0.lock().expect("sink lock").clone();
            String::from_utf8(bytes)
                .expect("log is UTF-8")
                .lines()
                .map(|text| serde_json::from_str(text).expect("every line is JSON"))
                .collect()
        }
    }

    fn messages(lines: &[Value]) -> Vec<String> {
        lines
            .iter()
            .map(|line| line["msg"].as_str().unwrap_or_default().to_owned())
            .collect()
    }

    fn with_sink(settings: &Settings) -> (Built, Sink) {
        let sink = Sink::default();
        (build(ProcessKind::Cli, sink.clone(), None, settings), sink)
    }

    fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn an_event_becomes_one_line_with_the_attribution_and_its_own_fields() {
        let (built, sink) = with_sink(&Settings::default());
        tracing::dispatcher::with_default(&built.dispatch, || {
            tracing::info!(target: "cronus_core::demo", bytes = 482_113u64, backup.id = "b-1", "backup created");
        });
        let session = built.guard.session().to_owned();
        drop(built.guard);

        let lines = sink.lines();
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        assert_eq!(line["v"], 1);
        assert_eq!(line["level"], "INFO");
        assert_eq!(line["target"], "cronus_core::demo");
        assert_eq!(line["kind"], "cli");
        assert_eq!(line["session"], session.as_str());
        assert_eq!(line["msg"], "backup created");
        assert_eq!(line["backup.id"], "b-1");
        assert_eq!(line["bytes"], 482_113);
        assert!(line["ts"].as_str().is_some_and(|ts| ts.ends_with('Z')));
    }

    #[test]
    fn by_default_the_product_logs_at_info_and_its_dependencies_at_warn() {
        let (built, sink) = with_sink(&Settings::default());
        tracing::dispatcher::with_default(&built.dispatch, || {
            tracing::debug!(target: "cronus_core::filter_default", "own debug");
            tracing::info!(target: "cronus_core::filter_default", "own info");
            tracing::info!(target: "some_dependency::client", "dependency info");
            tracing::warn!(target: "some_dependency::client", "dependency warn");
        });
        drop(built.guard);

        assert_eq!(messages(&sink.lines()), ["own info", "dependency warn"]);
    }

    #[test]
    fn the_environment_filter_raises_the_product_without_raising_its_dependencies() {
        let settings = Settings {
            filter: Some("cronus=debug".to_owned()),
            ..Settings::default()
        };
        let (built, sink) = with_sink(&settings);
        tracing::dispatcher::with_default(&built.dispatch, || {
            tracing::debug!(target: "cronus_core::filter_env", "own debug");
            tracing::info!(target: "some_dependency::client", "dependency info");
        });
        drop(built.guard);

        assert_eq!(messages(&sink.lines()), ["own debug"]);
    }

    #[test]
    fn an_unusable_filter_falls_back_to_the_default_and_says_so() {
        let settings = Settings {
            filter: Some("cronus=loud".to_owned()),
            ..Settings::default()
        };
        let (built, sink) = with_sink(&settings);
        assert_eq!(built.guard.filter(), "warn,cronus=info");
        let facts = SessionFacts {
            args: &[],
            cwd: Path::new("."),
            state_root: Path::new("."),
            started: SystemTime::now(),
        };
        emit_session_start(&built.dispatch, &built.guard, &facts);
        drop(built.guard);

        let lines = sink.lines();
        assert_eq!(messages(&lines), ["session.start", "log.filter.invalid"]);
        assert_eq!(lines[1]["env"], FILTER_ENV);
        assert!(
            lines[1]["error"]
                .as_str()
                .is_some_and(|error| !error.is_empty())
        );
    }

    #[test]
    fn the_first_line_describes_the_process_and_hides_secret_arguments() {
        let (built, sink) = with_sink(&Settings::default());
        let args: Vec<String> = ["--token", "s3cret", "--name", "demo"]
            .iter()
            .map(|arg| (*arg).to_owned())
            .collect();
        let facts = SessionFacts {
            args: &args,
            cwd: Path::new("/work/dir"),
            state_root: Path::new("/state/root"),
            started: SystemTime::UNIX_EPOCH,
        };
        emit_session_start(&built.dispatch, &built.guard, &facts);
        tracing::dispatcher::with_default(&built.dispatch, || {
            tracing::info!(target: "cronus_core::demo", "later event");
        });
        drop(built.guard);

        let lines = sink.lines();
        let header = &lines[0];
        assert_eq!(header["msg"], "session.start");
        assert_eq!(header["os"], std::env::consts::OS);
        assert_eq!(header["arch"], std::env::consts::ARCH);
        assert_eq!(header["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(header["kind"], "cli");
        assert_eq!(header["cwd"], Path::new("/work/dir").display().to_string());
        assert_eq!(
            header["state_root"],
            Path::new("/state/root").display().to_string()
        );
        assert_eq!(header["started"], "1970-01-01T00:00:00.000Z");
        assert_eq!(header["args"], "--token <redacted> --name demo");
        assert!(
            !sink
                .0
                .lock()
                .expect("sink lock")
                .windows(6)
                .any(|w| w == b"s3cret")
        );
        assert_eq!(lines[1]["msg"], "later event");
    }

    #[test]
    fn secret_values_are_hidden_in_both_flag_forms_and_only_when_there_is_a_value() {
        let args =
            |list: &[&str]| -> Vec<String> { list.iter().map(|a| (*a).to_owned()).collect() };
        assert_eq!(
            redact_args(&args(&[
                "--api-token=abc",
                "--password",
                "p",
                "plain",
                "--secret"
            ])),
            [
                "--api-token=<redacted>",
                "--password",
                "<redacted>",
                "plain",
                "--secret"
            ]
        );
        assert_eq!(
            redact_args(&args(&["--token", "--verbose", "x"])),
            ["--token", "--verbose", "x"]
        );
        assert_eq!(
            redact_args(&args(&["-Authorization", "Bearer z"])),
            ["-Authorization", "<redacted>"]
        );
    }

    #[test]
    fn a_panic_is_logged_with_message_and_location_before_the_previous_hook_runs() {
        let order: Arc<Mutex<Vec<String>>> = Arc::default();
        let console_order = Arc::clone(&order);
        let sink = Sink::default();
        let built = build(
            ProcessKind::Cli,
            sink.clone(),
            Some(Box::new(move |text: &str| {
                // Other tests may panic while this hook is installed; only this
                // test's own panic is tracked.
                if text.contains("boom for the log") {
                    console_order
                        .lock()
                        .expect("order lock")
                        .push("logged".to_owned());
                }
            })),
            &Settings::default(),
        );

        let original: PreviousHook = Arc::from(panic::take_hook());
        let previous_order = Arc::clone(&order);
        let original_for_chain = Arc::clone(&original);
        panic::set_hook(chained_panic_hook(
            built.dispatch.clone(),
            Arc::new(move |info| {
                if std::thread::current().name() == Some("panic-log-test") {
                    previous_order
                        .lock()
                        .expect("order lock")
                        .push("previous hook".to_owned());
                }
                // Keep the harness's own reporting of unrelated failures intact.
                original_for_chain(info);
            }),
        ));
        let outcome = std::thread::Builder::new()
            .name("panic-log-test".to_owned())
            .spawn(|| {
                panic!("boom for the log");
            })
            .expect("spawn the panicking thread")
            .join();
        let _ = panic::take_hook();
        let restore = Arc::clone(&original);
        panic::set_hook(Box::new(move |info| restore(info)));
        drop(built.guard);

        assert!(outcome.is_err());
        assert_eq!(
            *order.lock().expect("order lock"),
            ["logged", "previous hook"]
        );
        let lines = sink.lines();
        let panic_line = lines
            .iter()
            .find(|line| line["msg"] == "panic" && line["thread"] == "panic-log-test")
            .expect("a panic event");
        assert_eq!(panic_line["level"], "ERROR");
        assert_eq!(panic_line["panic.message"], "boom for the log");
        assert!(
            panic_line["panic.location"]
                .as_str()
                .is_some_and(|place| place.contains("mod.rs"))
        );
    }

    /// A sink whose writes block until released, so a test can fill the queue.
    struct GateSink {
        bytes: Arc<Mutex<Vec<u8>>>,
        started: Arc<AtomicUsize>,
        gate: Arc<(Mutex<bool>, Condvar)>,
    }

    impl Write for GateSink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.started.fetch_add(1, Ordering::SeqCst);
            let (lock, wake) = &*self.gate;
            let mut open = lock.lock().expect("gate lock");
            while !*open {
                open = wake.wait(open).expect("gate wait");
            }
            drop(open);
            self.bytes
                .lock()
                .expect("bytes lock")
                .extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_full_queue_drops_lines_and_reports_the_count_once_it_has_room() {
        let bytes: Arc<Mutex<Vec<u8>>> = Arc::default();
        let started = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        // A run is written when a different event arrives to end it, and the
        // release thread must not do that for us mid-test.
        let settings = Settings {
            queue_lines: 2,
            fold_hold: Duration::from_secs(3600),
            ..Settings::default()
        };
        let built = build(
            ProcessKind::Cli,
            GateSink {
                bytes: Arc::clone(&bytes),
                started: Arc::clone(&started),
                gate: Arc::clone(&gate),
            },
            None,
            &settings,
        );
        let emit = |text: &'static str| {
            tracing::dispatcher::with_default(&built.dispatch, || {
                tracing::info!(target: "cronus_core::queue", "{}", text);
            });
        };
        let written = |bytes: &Arc<Mutex<Vec<u8>>>| {
            bytes
                .lock()
                .expect("bytes lock")
                .iter()
                .filter(|b| **b == b'\n')
                .count()
        };

        emit("one");
        emit("two");
        wait_for("the writer to pick up the first line", || {
            started.load(Ordering::SeqCst) >= 1
        });
        // Two lines fit in the queue; the next three are dropped, and the last
        // event is still held, waiting for a different one to end its run.
        for text in ["three", "four", "five", "six", "seven"] {
            emit(text);
        }
        {
            let (lock, wake) = &*gate;
            *lock.lock().expect("gate lock") = true;
            wake.notify_all();
        }
        wait_for("the queue to drain", || written(&bytes) >= 3);
        emit("eight");
        // The notice and the line it precedes fill the two-slot queue; let the
        // writer take them before the held last event is flushed behind them.
        wait_for("the notice and the seventh line to be written", || {
            written(&bytes) >= 5
        });
        drop(built.guard);

        let parsed: Vec<Value> = String::from_utf8(bytes.lock().expect("bytes lock").clone())
            .expect("log is UTF-8")
            .lines()
            .map(|text| serde_json::from_str(text).expect("every line is JSON"))
            .collect();
        let dropped: Vec<&Value> = parsed
            .iter()
            .filter(|line| line["msg"] == "log.dropped")
            .collect();
        assert_eq!(dropped.len(), 1, "exactly one notice: {parsed:?}");
        assert_eq!(dropped[0]["count"], 3);
        assert_eq!(dropped[0]["level"], "WARN");
        assert_eq!(
            messages(&parsed),
            ["one", "two", "three", "log.dropped", "seven", "eight"]
        );
    }

    #[test]
    fn the_level_setting_moves_the_product_unless_the_environment_holds_the_filter() {
        let (built, sink) = with_sink(&Settings::default());
        tracing::dispatcher::with_default(&built.dispatch, || {
            tracing::debug!(target: "cronus_core::level", "before");
        });
        assert_eq!(built.guard.apply_level("debug"), Ok(true));
        tracing::dispatcher::with_default(&built.dispatch, || {
            tracing::debug!(target: "cronus_core::level", "after");
        });
        assert!(built.guard.apply_level("loud").is_err());
        drop(built.guard);
        assert_eq!(messages(&sink.lines()), ["after"]);

        let settings = Settings {
            filter: Some("cronus=warn".to_owned()),
            ..Settings::default()
        };
        let (held, _) = with_sink(&settings);
        assert_eq!(held.guard.apply_level("debug"), Ok(false));
    }

    #[test]
    fn the_console_shows_warnings_only_and_the_terminal_ui_has_none() {
        let seen: Arc<Mutex<Vec<String>>> = Arc::default();
        let console_seen = Arc::clone(&seen);
        let built = build(
            ProcessKind::Cli,
            Sink::default(),
            Some(Box::new(move |text: &str| {
                console_seen
                    .lock()
                    .expect("seen lock")
                    .push(text.to_owned());
            })),
            &Settings::default(),
        );
        tracing::dispatcher::with_default(&built.dispatch, || {
            tracing::info!(target: "cronus_core::console", "quiet");
            tracing::warn!(target: "cronus_core::console", bytes = 5u64, path = "a b", "degraded");
        });
        drop(built.guard);

        assert_eq!(
            *seen.lock().expect("seen lock"),
            ["WARN  cronus_core::console: degraded bytes=5 path=a b\n"]
        );
        assert!(ProcessKind::Cli.has_console());
        assert!(!ProcessKind::Tui.has_console());
        assert!(!ProcessKind::Engine.has_console());
        assert!(!ProcessKind::Desktop.has_console());
    }

    #[test]
    fn process_kinds_name_their_file_families() {
        let names: Vec<&str> = [
            ProcessKind::Cli,
            ProcessKind::Tui,
            ProcessKind::Engine,
            ProcessKind::Desktop,
        ]
        .iter()
        .map(|kind| kind.as_str())
        .collect();
        assert_eq!(names, ["cli", "tui", "engine", "desktop"]);
    }

    fn line_count(sink: &Sink) -> usize {
        sink.0
            .lock()
            .expect("sink lock")
            .iter()
            .filter(|byte| **byte == b'\n')
            .count()
    }

    fn holding(fold_hold: Duration) -> Settings {
        Settings {
            fold_hold,
            ..Settings::default()
        }
    }

    #[test]
    fn identical_consecutive_events_fold_into_one_line_with_a_repeat_count() {
        let (built, sink) = with_sink(&holding(Duration::from_secs(3600)));
        tracing::dispatcher::with_default(&built.dispatch, || {
            for _ in 0..3 {
                tracing::info!(target: "cronus_core::fold", detail = "same", "tick");
            }
            tracing::info!(target: "cronus_core::fold", detail = "same", "different message");
            tracing::info!(target: "cronus_core::fold", detail = "same", "tick");
        });
        drop(built.guard);

        let lines = sink.lines();
        assert_eq!(messages(&lines), ["tick", "different message", "tick"]);
        assert_eq!(lines[0]["repeat"], 3);
        assert!(lines[0]["last_ts"].is_string());
        assert!(lines[1].get("repeat").is_none());
        assert!(lines[2].get("repeat").is_none());
    }

    #[test]
    fn a_held_run_is_released_when_nothing_arrives_to_end_it() {
        let (built, sink) = with_sink(&holding(Duration::from_millis(20)));
        tracing::dispatcher::with_default(&built.dispatch, || {
            tracing::info!(target: "cronus_core::fold", "alone");
        });
        wait_for("the lone event to be written while the guard lives", || {
            line_count(&sink) == 1
        });
        drop(built.guard);
        assert_eq!(messages(&sink.lines()), ["alone"]);
    }

    #[test]
    fn an_error_is_written_at_once_and_ends_the_run_held_before_it() {
        let (built, sink) = with_sink(&holding(Duration::from_secs(3600)));
        tracing::dispatcher::with_default(&built.dispatch, || {
            tracing::info!(target: "cronus_core::fold", "held");
            tracing::error!(target: "cronus_core::fold", "failed");
        });
        wait_for("both lines to be written while the guard lives", || {
            line_count(&sink) == 2
        });
        assert_eq!(messages(&sink.lines()), ["held", "failed"]);
        drop(built.guard);
    }

    #[test]
    fn secret_named_fields_and_known_secret_values_are_hidden_in_both_projections() {
        let seen: Arc<Mutex<Vec<String>>> = Arc::default();
        let console_seen = Arc::clone(&seen);
        let sink = Sink::default();
        let built = build(
            ProcessKind::Cli,
            sink.clone(),
            Some(Box::new(move |text: &str| {
                console_seen
                    .lock()
                    .expect("seen lock")
                    .push(text.to_owned());
            })),
            &Settings::default(),
        );
        built.guard.set_secrets(vec!["sk-LIVE-777".to_owned()]);
        tracing::dispatcher::with_default(&built.dispatch, || {
            tracing::warn!(
                target: "cronus_core::secrets",
                api_token = "abc123",
                detail = "call with sk-LIVE-777 failed",
                "request failed"
            );
        });
        drop(built.guard);

        let lines = sink.lines();
        assert_eq!(lines[0]["api_token"], "<redacted>");
        assert_eq!(lines[0]["detail"], "call with *** failed");
        let console = seen.lock().expect("seen lock").join("");
        assert!(console.contains("api_token=<redacted>"), "{console}");
        assert!(console.contains("call with *** failed"), "{console}");
        assert!(!console.contains("abc123") && !console.contains("sk-LIVE"));
        let file = String::from_utf8(sink.0.lock().expect("sink lock").clone()).expect("UTF-8");
        assert!(!file.contains("abc123") && !file.contains("sk-LIVE"));
    }

    #[test]
    fn large_bodies_follow_the_payload_policy_in_the_file_and_are_always_reduced_on_the_console() {
        let seen: Arc<Mutex<Vec<String>>> = Arc::default();
        let console_seen = Arc::clone(&seen);
        let sink = Sink::default();
        let built = build(
            ProcessKind::Cli,
            sink.clone(),
            Some(Box::new(move |text: &str| {
                console_seen
                    .lock()
                    .expect("seen lock")
                    .push(text.to_owned());
            })),
            &Settings::default(),
        );
        built.guard.set_payload(Payload {
            policy: PayloadPolicy::Excerpt,
            ..Payload::default()
        });
        let body = "b".repeat(4096);
        tracing::dispatcher::with_default(&built.dispatch, || {
            tracing::warn!(target: "cronus_core::payload", body = body.as_str(), "response received");
        });
        drop(built.guard);

        let lines = sink.lines();
        assert_eq!(lines[0]["body"].as_str().map(str::len), Some(256));
        assert_eq!(lines[0]["body.truncated_from"], 4096);
        let console = seen.lock().expect("seen lock").join("");
        assert!(console.contains("body=<4096 bytes>"), "{console}");
    }

    #[test]
    fn more_secret_names_are_recognised_without_hiding_the_log_s_own_fields() {
        for name in [
            "passwd",
            "--passphrase",
            "DB_CREDENTIALS",
            "cookie",
            "x-api-key",
            "OPENAI_API_KEY",
            "private_key_path",
            "Bearer",
        ] {
            assert!(is_secret_name(name), "{name}");
        }
        for name in ["session", "cache_key", "key", "bytes", "backup.id", "pin"] {
            assert!(!is_secret_name(name), "{name}");
        }
    }

    #[test]
    fn a_very_long_argument_is_shortened_in_the_header() {
        let long = "x".repeat(1000);
        let shown = shorten_arg(long);
        assert!(
            shown.len() < 300 && shown.ends_with("[1000 bytes]"),
            "{shown}"
        );
        assert_eq!(shorten_arg("short".to_owned()), "short");
    }
}
