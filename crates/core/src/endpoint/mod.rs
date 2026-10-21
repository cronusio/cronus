//! One engine per state root, and the means for everything else to attach to
//! it.
//!
//! A process over a state root either holds the seat and is the **engine**
//! (it composes the core, serves the endpoint, and owns the work ledger), or
//! finds the seat held and becomes a **spoke** of the engine that holds it.
//! This is the facade-tier realization of the attach rule (the domain tier does
//! no I/O): the lock is the operating system's, the channel is loopback, and
//! what crosses it is the shared dispatch contract unchanged.
//!
//! The pieces, each in its own file:
//!
//! - [`seat`] — the exclusive claim on a state root and the record that lets
//!   others find the engine.
//! - [`wire`] — how an invocation and its answer are written on the channel.
//! - [`server`] / [`client`] — the engine's listener and a spoke's connection.
//! - [`spoke`] — re-pointing a composition's handlers at the engine.
//! - [`worklock`] — whether any work is running anywhere over a state root,
//!   decided by the operating system: work holds a lock shared, a change that
//!   recomposes the office holds it exclusive.
//! - [`ledger`] — the names of the work running in a process, so a refusal can
//!   say which.

pub mod client;
pub mod ledger;
pub mod seat;
pub mod server;
pub mod spoke;
pub mod wire;
pub mod worklock;

use std::net::TcpListener;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use cronus_contract::LocusKind;
use cronus_domain::invocable::{Dispatcher, InvocableRegistry};

use self::client::{Link, LinkError};
use self::ledger::{Ledger, WorkItem};
use self::seat::{Claim, Read, Record, Seat};
use self::server::Server;

/// How long a join waits for an engine that holds the seat to publish where it
/// listens, before saying it did not.
pub const DEFAULT_PATIENCE: Duration = Duration::from_secs(3);

/// Why a process could not take a role over a state root.
#[derive(Debug)]
pub enum JoinError {
    /// The seat or its record could not be read or written; the system's
    /// reason, in words.
    Io(std::io::Error),
    /// An engine of another release holds the seat. Never a second engine and
    /// never a guess.
    Incompatible {
        engine: String,
        ours: String,
        pid: u32,
    },
    /// An engine holds the seat but published nowhere to reach it.
    NoEndpoint,
    /// The record is there but not a record; nothing is assumed from it.
    Corrupt,
    /// The record names an engine that did not answer.
    Unreachable(LinkError),
}

impl std::fmt::Display for JoinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JoinError::Io(err) => write!(f, "could not claim the state root: {err}"),
            JoinError::Incompatible { engine, ours, pid } => write!(
                f,
                "an engine of another release (process {pid}, {engine}) owns this state root; \
                 this is {ours}. Close it, or run the same release"
            ),
            JoinError::NoEndpoint => {
                f.write_str("an engine owns this state root but has not said where it listens")
            }
            JoinError::Corrupt => {
                f.write_str("the engine's endpoint record is unreadable; refusing to guess")
            }
            JoinError::Unreachable(err) => write!(f, "the engine that owns this state root {err}"),
        }
    }
}

impl std::error::Error for JoinError {}

/// The engine's half: the seat, and the listener once serving has begun.
pub struct EngineHost {
    // Declared first so the listener stops before the seat lets go.
    server: Option<Server>,
    seat: Seat,
    version: String,
    ledger: Ledger,
}

impl EngineHost {
    /// Keep this engine's work in `ledger` instead of the process-wide one.
    /// For an embedding that runs more than one engine in a process; the
    /// product has one.
    pub fn use_ledger(&mut self, ledger: Ledger) {
        self.ledger = ledger;
    }

    /// The state root this engine owns.
    pub fn root(&self) -> &Path {
        self.seat.root()
    }

    /// Start serving `dispatcher` for spokes and publish where. Returns once
    /// the record is in place, so a spoke that reads it can connect.
    pub fn serve(
        &mut self,
        registry: Arc<InvocableRegistry>,
        dispatcher: Arc<Dispatcher>,
    ) -> std::io::Result<()> {
        let listener: TcpListener = server::bind()?;
        let addr = listener.local_addr()?;
        let token = fresh_token()?;
        let record =
            Record::for_this_process(addr.to_string(), token.clone(), self.version.clone());
        let server = server::serve(
            listener,
            token,
            self.version.clone(),
            registry,
            dispatcher,
            self.ledger.clone(),
        )?;
        self.seat.publish(&record)?;
        self.server = Some(server);
        Ok(())
    }
}

/// 128 bits from the operating system's entropy source, as hex.
fn fresh_token() -> std::io::Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// What a process is over a state root.
pub enum Role {
    /// This process owns the state root.
    Engine(EngineHost),
    /// Another process owns it; invocations go there.
    Spoke(Arc<Link>),
}

impl Role {
    /// What the engine is running, by name.
    pub fn work(&self) -> Result<Vec<WorkItem>, LinkError> {
        match self {
            Role::Engine(host) => Ok(host.ledger.snapshot()),
            Role::Spoke(link) => link.work(),
        }
    }
}

/// A word that names what this build's engine-run catalog is: the crate's
/// version and a fingerprint of every identity and binder that runs in an
/// engine (`Semantic` and `Installation`). A spoke composes the same catalog
/// as its engine, and this is what makes that checkable: two builds that
/// differ in what an invocation means differ here, and do not attach.
pub fn release(registry: &InvocableRegistry) -> String {
    let mut rows: Vec<String> = registry
        .all()
        .filter(|invocable| {
            matches!(
                invocable.locus.kind(),
                LocusKind::Semantic | LocusKind::Installation
            )
        })
        .map(|invocable| {
            format!(
                "{}|{:?}|{:?}",
                invocable.id, invocable.binders, invocable.locus
            )
        })
        .collect();
    rows.sort();
    // FNV-1a over the rows: not a security hash, a stable fingerprint that
    // every process of one build computes the same way.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in rows.join("\n").bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{}+{hash:016x}", env!("CARGO_PKG_VERSION"))
}

/// Take a role over `root`. `release` is what this process's engine-run
/// catalog is ([`release`]); `None` is for a process that only asks about the
/// work being done (a gate on an installation change) and so can talk to an
/// engine of any release that speaks the protocol.
///
/// Becoming the engine never waits. Attaching waits up to `patience` for an
/// engine that has claimed the seat but not yet said where it listens, and
/// retries from the claim if the engine it found has gone — the seat is the
/// operating system's, so a dead engine's claim is simply free.
pub fn join(root: &Path, release: Option<&str>, patience: Duration) -> Result<Role, JoinError> {
    let deadline = Instant::now() + patience;
    let mut last: JoinError;
    loop {
        match seat::claim(root).map_err(JoinError::Io)? {
            Claim::Held(seat) => {
                return Ok(Role::Engine(EngineHost {
                    server: None,
                    seat,
                    version: release.unwrap_or_default().to_string(),
                    ledger: ledger::global().clone(),
                }));
            }
            Claim::Occupied => {}
        }
        match seat::read_record(root).map_err(JoinError::Io)? {
            Read::Corrupt => return Err(JoinError::Corrupt),
            Read::Found(record) => {
                if let Some(ours) = release
                    && record.version != ours
                {
                    return Err(JoinError::Incompatible {
                        engine: record.version,
                        ours: ours.to_string(),
                        pid: record.pid,
                    });
                }
                match Link::from_record(&record).and_then(|link| link.ping().map(|_| link)) {
                    Ok(link) => return Ok(Role::Spoke(Arc::new(link))),
                    // The engine may be exiting; the claim is looked at again.
                    Err(err) => last = JoinError::Unreachable(err),
                }
            }
            Read::Absent => last = JoinError::NoEndpoint,
        }
        if Instant::now() >= deadline {
            return Err(last);
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// The engine over `root`, if one is running and answers — without becoming
/// one and without claiming anything. For a process that only wants to ask.
pub fn reach_engine(root: &Path) -> Option<Link> {
    let Read::Found(record) = seat::read_record(root).ok()? else {
        return None;
    };
    let link = Link::from_record(&record).ok()?;
    link.ping().ok()?;
    Some(link)
}

/// A composition that has taken its role: the registry and dispatcher to run
/// invocations through (an engine's own, or a spoke's, re-pointed), and the
/// role, which a process keeps for as long as it runs — dropping it stops
/// serving and lets the seat go.
pub struct Joined {
    pub registry: Arc<InvocableRegistry>,
    pub dispatcher: Arc<Dispatcher>,
    pub role: Role,
}

/// Take a role over `root` for a composed `registry`/`dispatcher` of the
/// surface `surface`. The engine serves the composition it was given; a spoke
/// has its engine-run handlers re-pointed at the engine, and everything else
/// stays as composed.
pub fn join_composition(
    root: &Path,
    registry: InvocableRegistry,
    mut dispatcher: Dispatcher,
    surface: cronus_contract::Surface,
    patience: Duration,
) -> Result<Joined, JoinError> {
    let release = release(&registry);
    let role = join(root, Some(&release), patience)?;
    match role {
        Role::Spoke(link) => {
            spoke::point_at_engine(&registry, &mut dispatcher, &link, surface);
            Ok(Joined {
                registry: Arc::new(registry),
                dispatcher: Arc::new(dispatcher),
                role: Role::Spoke(link),
            })
        }
        Role::Engine(mut host) => {
            let (registry, dispatcher) = (Arc::new(registry), Arc::new(dispatcher));
            host.serve(Arc::clone(&registry), Arc::clone(&dispatcher))
                .map_err(JoinError::Io)?;
            Ok(Joined {
                registry,
                dispatcher,
                role: Role::Engine(host),
            })
        }
    }
}

/// A composition for a short-lived process: attached to the engine when there
/// is one, otherwise standing alone.
pub struct Composition {
    pub registry: Arc<InvocableRegistry>,
    pub dispatcher: Arc<Dispatcher>,
    /// Whether the engine-run handlers go to an engine (`true`) or run here.
    pub attached: bool,
}

/// Compose for a process that comes and goes — a command-line invocation. When
/// an engine owns the state root it attaches, as a frontend must (an engine is
/// never duplicated); when none does, it runs on its own **without claiming the
/// seat**: a command that lasts milliseconds is not somewhere a terminal UI that
/// starts meanwhile should find itself a spoke of. Work it runs is still seen
/// by anything that must know, through the work lock ([`begin_work`]).
pub fn attach_or_stand_alone(
    root: &Path,
    registry: InvocableRegistry,
    mut dispatcher: Dispatcher,
    surface: cronus_contract::Surface,
    patience: Duration,
) -> Result<Composition, JoinError> {
    let release = release(&registry);
    let attached = match join(root, Some(&release), patience)? {
        Role::Spoke(link) => {
            spoke::point_at_engine(&registry, &mut dispatcher, &link, surface);
            true
        }
        // Nobody owned the root: the seat taken for the asking is let go.
        Role::Engine(host) => {
            drop(host);
            false
        }
    };
    Ok(Composition {
        registry: Arc::new(registry),
        dispatcher: Arc::new(dispatcher),
        attached,
    })
}

/// Work that has begun: held for as long as the work lasts. Dropping it ends
/// the work in both places that know of it — the operating system's lock, so
/// a change cannot begin over it, and this process's ledger, so a refusal can
/// name it.
pub struct WorkGuard {
    // The ledger entry first: by the time the locks are released the name is gone.
    _entry: ledger::Entry,
    _shares: Vec<worklock::Share>,
}

/// Why work could not begin.
#[derive(Debug)]
pub enum BeginError {
    /// An installation change that recomposes the office is being applied.
    ChangeInProgress,
    /// The work could not be registered, so it is not started: work nobody can
    /// see is work a change could be applied over.
    Io(std::io::Error),
}

impl std::fmt::Display for BeginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BeginError::ChangeInProgress => {
                f.write_str("an installation change is being applied, so no new work can begin")
            }
            BeginError::Io(err) => write!(
                f,
                "the work could not be registered, so it was not started: {err}"
            ),
        }
    }
}

/// The state root that belongs to the machine and not to one workspace: where
/// the agent definitions, the extension registry and the developer-office
/// admission live. A change to any of them alters work in every workspace.
pub fn machine_root() -> std::path::PathBuf {
    crate::paths::Paths::os_native().resolve(crate::paths::Root::State)
}

/// Register a piece of work over `root` before it starts: take the work lock
/// shared — over the workspace and over the machine, so a change to either is
/// refused while it runs — and enter the process ledger. Every producer of long-running work
/// calls this first and holds the guard until the work has ended.
pub fn begin_work(
    root: &Path,
    kind: ledger::WorkKind,
    label: impl Into<String>,
) -> Result<WorkGuard, BeginError> {
    begin_work_in(ledger::global(), root, &machine_root(), kind, label)
}

/// [`begin_work`] against a ledger and a machine root of the caller's choosing.
pub fn begin_work_in(
    ledger: &Ledger,
    root: &Path,
    machine: &Path,
    kind: ledger::WorkKind,
    label: impl Into<String>,
) -> Result<WorkGuard, BeginError> {
    // One lock when the workspace root is the machine's own.
    let mut roots = vec![root.to_path_buf()];
    if machine != root {
        roots.push(machine.to_path_buf());
    }
    let mut shares = Vec::with_capacity(roots.len());
    for lock_root in &roots {
        shares.push(worklock::share(lock_root).map_err(|err| match err {
            worklock::Contended::Held => BeginError::ChangeInProgress,
            worklock::Contended::Io(err) => BeginError::Io(err),
        })?);
    }
    Ok(WorkGuard {
        _entry: ledger.enter(kind, label),
        _shares: shares,
    })
}

#[cfg(test)]
mod tests {
    use cronus_contract::{
        ArgValue, ArgValues, Binder, BinderKind, Invocable, InvocableId, Invocation, Locus,
        Outcome, OutcomeValue, Stability, Surface,
    };
    use cronus_domain::invocable::Registrant;

    use super::ledger::WorkKind;
    use super::*;

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("cronus-join-{tag}-{}-{nanos}", std::process::id()))
    }

    /// A composition with one echoing identity, shared by an engine and the
    /// spokes of these tests (as one release's composition is).
    fn composition() -> (InvocableRegistry, Dispatcher, InvocableId) {
        let id = InvocableId::new("core:test.echo").expect("well-formed");
        let mut registry = InvocableRegistry::new();
        registry
            .register(
                &Registrant::core(),
                Invocable {
                    id: id.clone(),
                    name: "Echo",
                    summary: "Test-only.",
                    group: "test",
                    locus: Locus::Semantic,
                    binders: vec![Binder {
                        name: "value",
                        kind: BinderKind::Text,
                        optional: false,
                    }],
                    stability: Stability::Shipped,
                    journal_raw_input: true,
                },
            )
            .expect("registers");
        let mut dispatcher = Dispatcher::new();
        dispatcher.attach(
            id.clone(),
            Arc::new(|args| match args.get("value") {
                Some(ArgValue::Text(text)) => {
                    Outcome::Value(OutcomeValue::Text(format!("engine says {text}")))
                }
                _ => Outcome::Unavailable {
                    reason: "no value".to_string(),
                },
            }),
        );
        // An identity that says where the invocation was made from.
        let whereabouts = InvocableId::new("core:test.where").expect("well-formed");
        registry
            .register(
                &Registrant::core(),
                Invocable {
                    id: whereabouts.clone(),
                    name: "Where",
                    summary: "Test-only.",
                    group: "test",
                    locus: Locus::Semantic,
                    binders: Vec::new(),
                    stability: Stability::Shipped,
                    journal_raw_input: true,
                },
            )
            .expect("registers");
        dispatcher.attach(
            whereabouts,
            Arc::new(|_| {
                Outcome::Value(OutcomeValue::Text(
                    cronus_domain::paths::from_caller(Path::new("notes/a.md"))
                        .display()
                        .to_string(),
                ))
            }),
        );
        // A second identity that answers with a stream.
        let streaming = InvocableId::new("core:test.streams").expect("well-formed");
        registry
            .register(
                &Registrant::core(),
                Invocable {
                    id: streaming.clone(),
                    name: "Streams",
                    summary: "Test-only.",
                    group: "test",
                    locus: Locus::Semantic,
                    binders: Vec::new(),
                    stability: Stability::Shipped,
                    journal_raw_input: true,
                },
            )
            .expect("registers");
        let hub = dispatcher.streams();
        dispatcher.attach(
            streaming,
            Arc::new(move |_| {
                let (handle, sink) = hub.open();
                std::thread::spawn(move || {
                    for piece in ["one ", "two ", "three"] {
                        sink.chunk(OutcomeValue::Text(piece.to_string()));
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    sink.end(Outcome::Value(OutcomeValue::Text(
                        "one two three".to_string(),
                    )));
                });
                Outcome::Stream(handle)
            }),
        );
        (registry, dispatcher, id)
    }

    fn serving_engine(root: &Path) -> EngineHost {
        serving_engine_with(root, Ledger::new())
    }

    fn serving_engine_with(root: &Path, ledger: Ledger) -> EngineHost {
        let Role::Engine(mut host) =
            join(root, Some("1.0.0"), Duration::from_millis(50)).expect("joins")
        else {
            panic!("expected to be the engine");
        };
        host.use_ledger(ledger);
        let (registry, dispatcher, _) = composition();
        host.serve(Arc::new(registry), Arc::new(dispatcher))
            .expect("serves");
        host
    }

    #[test]
    fn the_first_process_is_the_engine_and_the_second_a_spoke_that_reaches_it() {
        let root = temp_root("roles");
        let engine = serving_engine(&root);

        let Role::Spoke(link) = join(&root, Some("1.0.0"), DEFAULT_PATIENCE).expect("joins") else {
            panic!("expected to attach");
        };
        assert_eq!(link.ping().expect("answers"), "1.0.0");

        drop(engine);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_spoke_dispatches_through_the_engine_and_gets_the_engines_answer() {
        let root = temp_root("forward");
        let engine = serving_engine(&root);
        let Role::Spoke(link) = join(&root, Some("1.0.0"), DEFAULT_PATIENCE).expect("joins") else {
            panic!("expected to attach");
        };

        // The spoke composes the same catalog; only what runs is re-pointed.
        let (registry, mut dispatcher, id) = composition();
        dispatcher.attach(
            id.clone(),
            Arc::new(|_| Outcome::Unavailable {
                reason: "this handler must not run in a spoke".to_string(),
            }),
        );
        spoke::point_at_engine(&registry, &mut dispatcher, &link, Surface::Cli);

        let mut args = ArgValues::new();
        args.insert("value", ArgValue::Text("hello".to_string()));
        let outcome = match dispatcher.dispatch(
            &registry,
            &Invocation {
                id,
                args,
                caller: Surface::Cli,
            },
        ) {
            cronus_contract::Dispatched::Ran(outcome) => outcome,
            cronus_contract::Dispatched::Unknown => panic!("the spoke knows the identity"),
        };

        assert_eq!(
            outcome,
            Outcome::Value(OutcomeValue::Text("engine says hello".to_string()))
        );
        drop(engine);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_relative_path_in_a_forwarded_invocation_means_the_spokes_directory() {
        let root = temp_root("where");
        let engine = serving_engine(&root);
        let Role::Spoke(link) = join(&root, Some("1.0.0"), DEFAULT_PATIENCE).expect("joins") else {
            panic!("expected to attach");
        };
        let (registry, mut dispatcher, _) = composition();
        spoke::point_at_engine(&registry, &mut dispatcher, &link, Surface::Cli);
        let ask = || match dispatcher.dispatch(
            &registry,
            &Invocation {
                id: InvocableId::new("core:test.where").expect("well-formed"),
                args: ArgValues::new(),
                caller: Surface::Cli,
            },
        ) {
            cronus_contract::Dispatched::Ran(Outcome::Value(OutcomeValue::Text(text))) => text,
            other => panic!("expected text, got {other:?}"),
        };

        // A spoke standing somewhere other than the engine.
        let elsewhere = std::env::temp_dir().join("cronus-spoke-elsewhere");
        let answered = cronus_domain::paths::with_caller_dir(&elsewhere, ask);
        assert_eq!(
            std::path::PathBuf::from(answered),
            elsewhere.join("notes").join("a.md"),
            "the engine resolved the path against the spoke's directory"
        );
        drop(engine);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_streamed_result_is_carried_to_the_spoke_piece_by_piece_and_ends_with_the_settled_result() {
        let root = temp_root("stream");
        let engine = serving_engine(&root);
        let Role::Spoke(link) = join(&root, Some("1.0.0"), DEFAULT_PATIENCE).expect("joins") else {
            panic!("expected to attach");
        };
        let (registry, mut dispatcher, _) = composition();
        spoke::point_at_engine(&registry, &mut dispatcher, &link, Surface::Tui);

        let outcome = match dispatcher.dispatch(
            &registry,
            &Invocation {
                id: InvocableId::new("core:test.streams").expect("well-formed"),
                args: ArgValues::new(),
                caller: Surface::Tui,
            },
        ) {
            cronus_contract::Dispatched::Ran(outcome) => outcome,
            cronus_contract::Dispatched::Unknown => panic!("the spoke knows the identity"),
        };
        let Outcome::Stream(handle) = outcome else {
            panic!("a streamed answer stays a stream in the spoke, got {outcome:?}");
        };
        let mut subscription = dispatcher.subscribe(&handle).expect("subscribes");
        let mut said = String::new();
        let end = loop {
            match subscription
                .next_item()
                .expect("the stream ends with an end")
            {
                cronus_domain::invocable::StreamItem::Chunk(OutcomeValue::Text(piece)) => {
                    said.push_str(&piece);
                }
                cronus_domain::invocable::StreamItem::Chunk(other) => {
                    panic!("unexpected piece {other:?}")
                }
                cronus_domain::invocable::StreamItem::End(outcome) => break outcome,
            }
        };

        assert_eq!(said, "one two three");
        assert_eq!(
            end,
            Outcome::Value(OutcomeValue::Text("one two three".to_string()))
        );
        drop(engine);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_stream_cut_off_by_the_engine_going_away_ends_saying_so() {
        let root = temp_root("stream-cut");
        let Role::Engine(mut host) =
            join(&root, Some("1.0.0"), Duration::from_millis(50)).expect("joins")
        else {
            panic!("expected to be the engine");
        };
        // An engine whose stream starts and then never finishes.
        let id = InvocableId::new("core:test.hangs").expect("well-formed");
        let descriptor = Invocable {
            id: id.clone(),
            name: "Hangs",
            summary: "Test-only.",
            group: "test",
            locus: Locus::Semantic,
            binders: Vec::new(),
            stability: Stability::Shipped,
            journal_raw_input: true,
        };
        let mut registry = InvocableRegistry::new();
        registry
            .register(&Registrant::core(), descriptor.clone())
            .expect("registers");
        let mut dispatcher = Dispatcher::new();
        let hub = dispatcher.streams();
        dispatcher.attach(
            id.clone(),
            Arc::new(move |_| {
                let (handle, sink) = hub.open();
                std::thread::spawn(move || {
                    sink.chunk(OutcomeValue::Text("begun".to_string()));
                    std::thread::sleep(Duration::from_secs(30));
                    drop(sink);
                });
                Outcome::Stream(handle)
            }),
        );
        host.serve(Arc::new(registry), Arc::new(dispatcher))
            .expect("serves");

        let Role::Spoke(link) = join(&root, Some("1.0.0"), DEFAULT_PATIENCE).expect("joins") else {
            panic!("expected to attach");
        };
        let mut spoke_registry = InvocableRegistry::new();
        spoke_registry
            .register(&Registrant::core(), descriptor)
            .expect("registers");
        let mut spoke_dispatcher = Dispatcher::new();
        spoke_dispatcher.attach(
            id.clone(),
            Arc::new(|_| Outcome::Value(OutcomeValue::Empty)),
        );
        spoke::point_at_engine(&spoke_registry, &mut spoke_dispatcher, &link, Surface::Cli);
        let cronus_contract::Dispatched::Ran(Outcome::Stream(handle)) = spoke_dispatcher.dispatch(
            &spoke_registry,
            &Invocation {
                id,
                args: ArgValues::new(),
                caller: Surface::Cli,
            },
        ) else {
            panic!("expected a stream");
        };
        let mut subscription = spoke_dispatcher.subscribe(&handle).expect("subscribes");

        // The engine goes away mid-stream.
        drop(host);

        let end = loop {
            match subscription.next_item().expect("the stream ends") {
                cronus_domain::invocable::StreamItem::Chunk(_) => {}
                cronus_domain::invocable::StreamItem::End(outcome) => break outcome,
            }
        };
        match end {
            Outcome::Unavailable { reason } => {
                assert!(reason.contains("did not answer"), "{reason}")
            }
            other => panic!("expected the loss to be named, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_spoke_whose_engine_has_gone_says_so_and_does_not_run_the_verb_itself() {
        let root = temp_root("gone");
        let engine = serving_engine(&root);
        let Role::Spoke(link) = join(&root, Some("1.0.0"), DEFAULT_PATIENCE).expect("joins") else {
            panic!("expected to attach");
        };
        let (registry, mut dispatcher, id) = composition();
        spoke::point_at_engine(&registry, &mut dispatcher, &link, Surface::Tui);
        drop(engine); // the engine exits

        let mut args = ArgValues::new();
        args.insert("value", ArgValue::Text("hello".to_string()));
        let outcome = match dispatcher.dispatch(
            &registry,
            &Invocation {
                id,
                args,
                caller: Surface::Tui,
            },
        ) {
            cronus_contract::Dispatched::Ran(outcome) => outcome,
            cronus_contract::Dispatched::Unknown => panic!("the spoke knows the identity"),
        };

        match outcome {
            Outcome::Unavailable { reason } => {
                assert!(reason.contains("did not answer"), "{reason}");
            }
            other => panic!("expected the loss to be named, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn once_the_engine_is_gone_the_next_process_becomes_the_engine() {
        let root = temp_root("succession");
        let engine = serving_engine(&root);
        drop(engine);

        let role = join(&root, Some("1.0.0"), Duration::from_millis(200)).expect("joins");
        assert!(
            matches!(role, Role::Engine(_)),
            "the claim is free once its holder is gone"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_engine_of_another_release_is_not_attached_to_and_not_replaced() {
        let root = temp_root("version");
        let engine = serving_engine(&root);

        match join(&root, Some("2.0.0"), Duration::from_millis(200)) {
            Err(JoinError::Incompatible { engine, ours, .. }) => {
                assert_eq!(engine, "1.0.0");
                assert_eq!(ours, "2.0.0");
            }
            Err(other) => panic!("expected a version refusal, got {other}"),
            Ok(_) => panic!("must not join an engine of another release"),
        }
        drop(engine);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_engine_that_has_claimed_the_seat_but_not_yet_served_is_waited_for() {
        let root = temp_root("starting");
        let Role::Engine(mut host) =
            join(&root, Some("1.0.0"), Duration::from_millis(50)).expect("joins")
        else {
            panic!("expected to be the engine");
        };
        let serving_root = root.clone();
        let starter = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            let (registry, dispatcher, _) = composition();
            host.serve(Arc::new(registry), Arc::new(dispatcher))
                .expect("serves");
            (host, serving_root)
        });

        let role = join(&root, Some("1.0.0"), DEFAULT_PATIENCE).expect("waits for the record");
        assert!(matches!(role, Role::Spoke(_)));

        let (host, _) = starter.join().expect("joins");
        drop(host);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_engine_that_never_says_where_it_listens_is_reported_after_the_patience() {
        let root = temp_root("silent");
        let Role::Engine(_host) =
            join(&root, Some("1.0.0"), Duration::from_millis(50)).expect("joins")
        else {
            panic!("expected to be the engine");
        };
        match join(&root, Some("1.0.0"), Duration::from_millis(150)) {
            Err(JoinError::NoEndpoint) => {}
            Err(other) => panic!("expected the silence to be named, got {other}"),
            Ok(_) => panic!("there is nothing to attach to"),
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_unreadable_record_is_refused_not_guessed_at_nor_deleted() {
        let root = temp_root("corrupt");
        let Role::Engine(_host) =
            join(&root, Some("1.0.0"), Duration::from_millis(50)).expect("joins")
        else {
            panic!("expected to be the engine");
        };
        std::fs::write(root.join("engine.endpoint"), "garbage").expect("writable");

        assert!(matches!(
            join(&root, Some("1.0.0"), Duration::from_millis(100)),
            Err(JoinError::Corrupt)
        ));
        assert!(root.join("engine.endpoint").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A machine root private to a test, beside its workspace root.
    fn machine_of(root: &Path) -> std::path::PathBuf {
        root.with_extension("machine")
    }

    #[test]
    fn work_that_has_begun_is_named_in_the_ledger_and_keeps_a_change_out_until_it_ends() {
        let root = temp_root("begun");
        let ledger = Ledger::new();

        let work = begin_work_in(
            &ledger,
            &root,
            &machine_of(&root),
            WorkKind::AgentTurn,
            "s-1",
        )
        .expect("begins");
        assert_eq!(ledger.snapshot().len(), 1);
        assert!(matches!(
            worklock::exclusive(&root),
            Err(worklock::Contended::Held)
        ));

        drop(work);
        assert!(ledger.snapshot().is_empty());
        assert!(worklock::exclusive(&root).is_ok());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn work_does_not_begin_while_a_change_is_applied_and_leaves_no_name_behind() {
        let root = temp_root("refused");
        let ledger = Ledger::new();
        let change = worklock::exclusive(&root).expect("the change begins");

        match begin_work_in(
            &ledger,
            &root,
            &machine_of(&root),
            WorkKind::WorkflowRun,
            "nightly",
        ) {
            Err(BeginError::ChangeInProgress) => {}
            Err(other) => panic!("expected the change to be named, got {other}"),
            Ok(_) => panic!("work must not begin under a change"),
        }
        assert!(ledger.snapshot().is_empty());

        drop(change);
        assert!(
            begin_work_in(
                &ledger,
                &root,
                &machine_of(&root),
                WorkKind::WorkflowRun,
                "nightly"
            )
            .is_ok()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_engines_work_is_named_to_a_spoke_that_asks() {
        let root = temp_root("named");
        let ledger = Ledger::new();
        let engine = serving_engine_with(&root, ledger.clone());
        let Role::Spoke(link) = join(&root, Some("1.0.0"), DEFAULT_PATIENCE).expect("joins") else {
            panic!("expected to attach");
        };
        let _turn = begin_work_in(
            &ledger,
            &root,
            &machine_of(&root),
            WorkKind::AgentTurn,
            "s-9",
        )
        .expect("begins");

        let named = Role::Spoke(link).work().expect("lists");

        assert_eq!(named.len(), 1);
        assert_eq!(named[0].label, "s-9");
        drop(engine);
        let _ = std::fs::remove_dir_all(&root);
    }
}
