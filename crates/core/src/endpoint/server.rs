//! The engine's listener: where spokes bring their invocations.
//!
//! One connection carries one request. The listener binds the loopback
//! interface only; a request that does not present the engine's token is
//! refused before anything is read from it. The number of connections served
//! at once and the time a silent connection may take are both bounded, so a
//! misbehaving local process cannot hold the engine hostage.

use std::collections::HashMap;
use std::io::{BufReader, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use cronus_contract::{Dispatched, Invocation, Outcome};
use cronus_domain::invocable::{Dispatcher, InvocableRegistry, StreamItem};

use super::ledger::Ledger;
use super::wire::{
    Frame, PROTOCOL, Request, frame_line, outcome_to_json, parse_request, read_frame,
};

/// How many connections are served at once.
const MAX_CONNECTIONS: usize = 64;

/// How long a connection may take to send its request line.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// What the server answers with and for.
struct Context {
    token: String,
    version: String,
    registry: Arc<InvocableRegistry>,
    dispatcher: Arc<Dispatcher>,
    ledger: Ledger,
    active: AtomicUsize,
    /// The connections being served, so stopping the server can close them: a
    /// spoke learns at once that its engine is going rather than when its
    /// next write fails.
    live: Mutex<HashMap<u64, TcpStream>>,
    next_id: AtomicUsize,
}

/// A running listener. Dropping it stops accepting and closes the address.
pub struct Server {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    context: Arc<Context>,
    accept: Option<JoinHandle<()>>,
}

/// Bind the loopback interface on a port the system chooses.
pub fn bind() -> std::io::Result<TcpListener> {
    TcpListener::bind(("127.0.0.1", 0))
}

/// Serve `listener` for an engine. `token` is what every request must present.
pub fn serve(
    listener: TcpListener,
    token: String,
    version: String,
    registry: Arc<InvocableRegistry>,
    dispatcher: Arc<Dispatcher>,
    ledger: Ledger,
) -> std::io::Result<Server> {
    let addr = listener.local_addr()?;
    let stop = Arc::new(AtomicBool::new(false));
    let context = Arc::new(Context {
        token,
        version,
        registry,
        dispatcher,
        ledger,
        active: AtomicUsize::new(0),
        live: Mutex::new(HashMap::new()),
        next_id: AtomicUsize::new(0),
    });
    let accept_stop = Arc::clone(&stop);
    let accept_context = Arc::clone(&context);
    let accept = std::thread::Builder::new()
        .name("engine-endpoint".to_string())
        .spawn(move || accept_loop(listener, accept_stop, accept_context))?;
    Ok(Server {
        addr,
        stop,
        context,
        accept: Some(accept),
    })
}

impl Server {
    /// Where it listens.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // A blocked `accept` only returns for a connection; make one.
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(500));
        if let Some(accept) = self.accept.take() {
            let _ = accept.join();
        }
        // Close what is still being served. A relay blocked on its stream keeps
        // waiting for the work behind it, which is the work's to finish, and
        // fails at its next write.
        let live: Vec<TcpStream> = self
            .context
            .live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain()
            .map(|(_, stream)| stream)
            .collect();
        for stream in live {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

fn accept_loop(listener: TcpListener, stop: Arc<AtomicBool>, context: Arc<Context>) {
    for stream in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let Ok(stream) = stream else { continue };
        if context.active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            context.active.fetch_sub(1, Ordering::SeqCst);
            refuse(
                stream,
                "the engine is serving as many requests as it will at once",
            );
            continue;
        }
        let id = context.next_id.fetch_add(1, Ordering::SeqCst) as u64;
        if let Ok(clone) = stream.try_clone() {
            context
                .live
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(id, clone);
        }
        let served = Arc::clone(&context);
        let spawned = std::thread::Builder::new()
            .name("engine-request".to_string())
            .spawn(move || {
                let _ = serve_connection(stream, &served);
                served
                    .live
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(&id);
                served.active.fetch_sub(1, Ordering::SeqCst);
            });
        if spawned.is_err() {
            context.active.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

fn refuse(mut stream: TcpStream, reason: &str) {
    if let Ok(line) = frame_line(&Frame::Refused(reason.to_string())) {
        let _ = writeln!(stream, "{line}");
    }
    let _ = stream.shutdown(Shutdown::Both);
}

/// Compare two secrets over their whole length, whatever the first difference.
fn same_secret(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut difference = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        difference |= usize::from(a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0));
    }
    difference == 0
}

fn send(stream: &mut TcpStream, frame: &Frame) -> std::io::Result<()> {
    let line = frame_line(frame)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    writeln!(stream, "{line}")?;
    stream.flush()
}

fn serve_connection(stream: TcpStream, context: &Context) -> std::io::Result<()> {
    stream.set_read_timeout(Some(REQUEST_TIMEOUT))?;
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);

    let line = match read_frame(&mut reader) {
        Ok(Some(line)) => line,
        Ok(None) => return Ok(()),
        Err(err) => {
            return send(
                &mut writer,
                &Frame::Refused(format!("unreadable request: {err}")),
            );
        }
    };
    let received = match parse_request(&line) {
        Ok(received) => received,
        // Nothing about a request that does not parse can have presented a
        // token, so it is not told what a good one looks like.
        Err(_) => {
            return send(
                &mut writer,
                &Frame::Refused("malformed request".to_string()),
            );
        }
    };
    if !same_secret(&received.token, &context.token) {
        return send(&mut writer, &Frame::Refused("not welcome".to_string()));
    }
    if received.version != PROTOCOL {
        return send(
            &mut writer,
            &Frame::Refused(format!(
                "this engine speaks protocol {PROTOCOL}, not {}",
                received.version
            )),
        );
    }

    match received.request {
        Request::Ping => send(
            &mut writer,
            &Frame::Pong {
                version: context.version.clone(),
            },
        ),
        Request::Work => send(&mut writer, &Frame::Work(context.ledger.snapshot())),
        Request::Invoke {
            id,
            caller,
            args,
            cwd,
        } => {
            let invocation = Invocation { id, args, caller };
            // The handler runs for a caller standing in `cwd`.
            let dispatched = cronus_domain::paths::with_caller_dir(&cwd, || {
                context.dispatcher.dispatch(&context.registry, &invocation)
            });
            match dispatched {
                Dispatched::Unknown => send(&mut writer, &Frame::Unknown),
                Dispatched::Ran(Outcome::Stream(handle)) => {
                    relay(&mut writer, &context.dispatcher, &handle)
                }
                Dispatched::Ran(outcome) => match outcome_to_json(&outcome) {
                    Ok(json) => send(&mut writer, &Frame::Outcome(json)),
                    Err(err) => send(&mut writer, &Frame::Refused(err.to_string())),
                },
            }
        }
    }
}

/// Read a stream the engine's own dispatcher opened — so its secrets are
/// masked, chunk boundaries included — and relay it frame by frame. A failed
/// write ends the relay and nothing else: the work behind the stream runs to
/// its end and is recorded where it runs.
fn relay(
    writer: &mut TcpStream,
    dispatcher: &Dispatcher,
    handle: &cronus_contract::StreamHandle,
) -> std::io::Result<()> {
    let Some(mut subscription) = dispatcher.subscribe(handle) else {
        let unavailable = Outcome::Unavailable {
            reason: format!("nobody can read the stream {}", handle.channel),
        };
        return match outcome_to_json(&unavailable) {
            Ok(json) => send(writer, &Frame::Outcome(json)),
            Err(err) => send(writer, &Frame::Refused(err.to_string())),
        };
    };
    while let Some(item) = subscription.next_item() {
        match item {
            StreamItem::Chunk(value) => match send(writer, &Frame::Chunk(value)) {
                Ok(()) => {}
                // Not a number a reader could take: the stream ends there,
                // saying so, rather than going on with a hole in it.
                Err(err) if err.kind() == std::io::ErrorKind::InvalidData => {
                    let lost = Outcome::Unavailable {
                        reason: format!("a streamed piece could not be sent: {err}"),
                    };
                    if let Ok(json) = outcome_to_json(&lost) {
                        send(writer, &Frame::End(json))?;
                    }
                    return Ok(());
                }
                Err(err) => return Err(err),
            },
            StreamItem::End(outcome) => {
                return match outcome_to_json(&outcome) {
                    Ok(json) => send(writer, &Frame::End(json)),
                    Err(err) => send(writer, &Frame::Refused(err.to_string())),
                };
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use cronus_contract::{
        ArgValue, ArgValues, Binder, BinderKind, Invocable, InvocableId, Locus, OutcomeValue,
        Stability, Surface,
    };
    use cronus_domain::invocable::Registrant;

    use super::super::ledger::WorkKind;
    use super::super::wire::request_line;
    use super::*;

    const TOKEN: &str = "feedface";

    fn invocable(tail: &str, binders: Vec<Binder>) -> Invocable {
        Invocable {
            id: InvocableId::new(format!("core:{tail}")).expect("well-formed"),
            name: "Probe",
            summary: "Test-only.",
            group: "test",
            locus: Locus::Semantic,
            binders,
            stability: Stability::Shipped,
            journal_raw_input: true,
        }
    }

    /// A server over a registry holding an echo and a streaming probe.
    fn server_with(ledger: Ledger) -> (Server, Arc<Dispatcher>) {
        let mut registry = InvocableRegistry::new();
        let mut dispatcher = Dispatcher::new();
        let text = |name| Binder {
            name,
            kind: BinderKind::Text,
            optional: false,
        };
        let echo = invocable("test.echo", vec![text("value")]);
        let streams = invocable("test.streams", Vec::new());
        for descriptor in [echo.clone(), streams.clone()] {
            registry
                .register(&Registrant::core(), descriptor)
                .expect("registers");
        }
        dispatcher.attach(
            echo.id,
            Arc::new(|args| {
                let said = match args.get("value") {
                    Some(ArgValue::Text(text)) => text.clone(),
                    _ => String::new(),
                };
                Outcome::Value(OutcomeValue::Text(format!("echo: {said}")))
            }),
        );
        let hub = dispatcher.streams();
        dispatcher.attach(
            streams.id,
            Arc::new(move |_| {
                let (handle, sink) = hub.open();
                std::thread::spawn(move || {
                    sink.chunk(OutcomeValue::Text("par".to_string()));
                    sink.chunk(OutcomeValue::Text("tial".to_string()));
                    sink.end(Outcome::Value(OutcomeValue::Text("partial".to_string())));
                });
                Outcome::Stream(handle)
            }),
        );
        dispatcher.set_secrets(vec!["hunter-two".to_string()]);
        let dispatcher = Arc::new(dispatcher);
        let server = serve(
            bind().expect("binds"),
            TOKEN.to_string(),
            "9.9.9".to_string(),
            Arc::new(registry),
            Arc::clone(&dispatcher),
            ledger,
        )
        .expect("serves");
        (server, dispatcher)
    }

    /// Send one request line and read every reply line until the server closes.
    fn ask(addr: SocketAddr, line: &str) -> Vec<Frame> {
        let mut stream = TcpStream::connect(addr).expect("connects");
        writeln!(stream, "{line}").expect("writes");
        let mut frames = Vec::new();
        let mut reader = BufReader::new(stream);
        while let Some(text) = read_frame(&mut reader).expect("reads") {
            frames.push(super::super::wire::parse_frame(&text).expect("parses"));
        }
        frames
    }

    fn invoke_line(token: &str, id: &str, value: Option<&str>) -> String {
        let mut args = ArgValues::new();
        if let Some(value) = value {
            args.insert("value", ArgValue::Text(value.to_string()));
        }
        request_line(
            token,
            &Request::Invoke {
                id: InvocableId::new(id).expect("well-formed"),
                caller: Surface::Cli,
                args,
                cwd: std::env::temp_dir(),
            },
        )
        .expect("writes")
    }

    #[test]
    fn an_invocation_runs_in_the_engine_and_its_outcome_comes_back() {
        let (server, _) = server_with(Ledger::new());
        let frames = ask(
            server.addr(),
            &invoke_line(TOKEN, "core:test.echo", Some("hi")),
        );
        let [Frame::Outcome(json)] = frames.as_slice() else {
            panic!("expected one outcome, got {frames:?}");
        };
        assert_eq!(
            super::super::wire::outcome_from_json(json, &[]).expect("parses"),
            Outcome::Value(OutcomeValue::Text("echo: hi".to_string()))
        );
    }

    #[test]
    fn a_missing_required_argument_comes_back_as_a_rejection_naming_the_binder() {
        let (server, _) = server_with(Ledger::new());
        let frames = ask(server.addr(), &invoke_line(TOKEN, "core:test.echo", None));
        let [Frame::Outcome(json)] = frames.as_slice() else {
            panic!("expected one outcome, got {frames:?}");
        };
        let binders = [Binder {
            name: "value",
            kind: BinderKind::Text,
            optional: false,
        }];
        match super::super::wire::outcome_from_json(json, &binders).expect("parses") {
            Outcome::Rejected(rejection) => assert_eq!(rejection.binder, "value"),
            other => panic!("expected a rejection, got {other:?}"),
        }
    }

    #[test]
    fn an_identity_nothing_registered_is_unknown_not_unavailable() {
        let (server, _) = server_with(Ledger::new());
        let frames = ask(
            server.addr(),
            &invoke_line(TOKEN, "core:test.nothing", None),
        );
        assert_eq!(frames, vec![Frame::Unknown]);
    }

    #[test]
    fn a_streamed_result_arrives_as_pieces_and_then_its_settled_end() {
        let (server, _) = server_with(Ledger::new());
        let frames = ask(
            server.addr(),
            &invoke_line(TOKEN, "core:test.streams", None),
        );
        // The engine masks across chunk boundaries, so it may hold the tail of
        // a piece back and release it with the next; what is guaranteed is the
        // order and the text, not where the pieces are cut.
        let Some((Frame::End(end), pieces)) = frames.split_last() else {
            panic!("expected pieces and then an end, got {frames:?}");
        };
        let said: String = pieces
            .iter()
            .map(|frame| match frame {
                Frame::Chunk(OutcomeValue::Text(piece)) => piece.as_str(),
                other => panic!("expected only pieces before the end, got {other:?}"),
            })
            .collect();
        assert_eq!(said, "partial");
        assert_eq!(
            super::super::wire::outcome_from_json(end, &[]).expect("parses"),
            Outcome::Value(OutcomeValue::Text("partial".to_string()))
        );
    }

    #[test]
    fn a_secret_is_masked_before_it_leaves_the_engine() {
        let (server, _) = server_with(Ledger::new());
        let frames = ask(
            server.addr(),
            &invoke_line(TOKEN, "core:test.echo", Some("the key is hunter-two")),
        );
        let [Frame::Outcome(json)] = frames.as_slice() else {
            panic!("expected one outcome, got {frames:?}");
        };
        let text = json.to_string();
        assert!(!text.contains("hunter-two"), "{text}");
        assert!(text.contains("***"), "{text}");
    }

    #[test]
    fn a_request_with_the_wrong_token_is_refused_and_nothing_runs() {
        let ledger = Ledger::new();
        let (server, _) = server_with(ledger);
        for line in [
            invoke_line("wrong", "core:test.echo", Some("hi")),
            request_line("", &Request::Ping).expect("writes"),
            request_line("feedfacf", &Request::Work).expect("writes"),
        ] {
            let frames = ask(server.addr(), &line);
            assert!(
                matches!(frames.as_slice(), [Frame::Refused(_)]),
                "{line}: {frames:?}"
            );
        }
    }

    #[test]
    fn a_request_that_is_not_a_request_is_refused_without_a_hint() {
        let (server, _) = server_with(Ledger::new());
        let frames = ask(server.addr(), "hello there");
        assert_eq!(
            frames,
            vec![Frame::Refused("malformed request".to_string())]
        );
    }

    #[test]
    fn a_request_for_another_protocol_version_is_refused_naming_ours() {
        let (server, _) = server_with(Ledger::new());
        let line = format!(r#"{{"v":99,"token":"{TOKEN}","op":"ping"}}"#);
        match ask(server.addr(), &line).as_slice() {
            [Frame::Refused(reason)] => assert!(reason.contains("protocol 1"), "{reason}"),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn a_ping_says_which_release_answered() {
        let (server, _) = server_with(Ledger::new());
        let frames = ask(
            server.addr(),
            &request_line(TOKEN, &Request::Ping).expect("writes"),
        );
        assert_eq!(
            frames,
            vec![Frame::Pong {
                version: "9.9.9".to_string()
            }]
        );
    }

    #[test]
    fn the_work_a_ledger_holds_is_what_the_engine_reports() {
        let ledger = Ledger::new();
        let _turn = ledger.enter(WorkKind::AgentTurn, "s-1");
        let (server, _) = server_with(ledger);
        let frames = ask(
            server.addr(),
            &request_line(TOKEN, &Request::Work).expect("writes"),
        );
        match frames.as_slice() {
            [Frame::Work(items)] => {
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].label, "s-1");
            }
            other => panic!("expected the work, got {other:?}"),
        }
    }

    #[test]
    fn dropping_the_server_closes_the_address() {
        let (server, _) = server_with(Ledger::new());
        let addr = server.addr();
        drop(server);
        assert!(
            TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_err(),
            "nothing answers once the engine has stopped serving"
        );
    }

    #[test]
    fn secrets_are_compared_over_their_whole_length() {
        assert!(same_secret("abc", "abc"));
        assert!(!same_secret("abc", "abd"));
        assert!(!same_secret("abc", "abcd"));
        assert!(!same_secret("", "a"));
    }
}
