//! `session.list` / `session.show` through the real dispatch path, against
//! journals written into an isolated workspace.
//!
//! The workspace root follows `CRONUS_PORTABLE_DIR`, which is process-global,
//! so the tests here take a lock and each runs set-env → dispatch → clean-up
//! under it.

use cronus_contract::{
    ArgValue, ArgValues, Dispatched, InvocableId, Invocation, Outcome, OutcomeValue, Surface,
};
use cronus_core::Engine;
use cronus_core::invocable_bootstrap::bootstrap;

static PORTABLE_DIR_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn dispatch(id: &str, args: ArgValues) -> Outcome {
    let (registry, dispatcher) = bootstrap(Engine::new());
    let invocation = Invocation {
        id: InvocableId::new(id).expect("well-formed invocable id"),
        args,
        caller: Surface::Cli,
    };
    match dispatcher.dispatch(&registry, &invocation) {
        // A streamed result is waited for and taken at its end: these tests
        // check what a turn settles to, not how it arrived.
        Dispatched::Ran(outcome) => dispatcher.settle(outcome),
        Dispatched::Unknown => panic!("expected {id} to resolve, got Unknown"),
    }
}

/// Run `body` with the workspace root redirected to a fresh directory; the
/// directory's `sessions/` path is handed to it.
fn in_isolated_workspace<T>(tag: &str, body: impl FnOnce(&std::path::Path) -> T) -> T {
    let _env = PORTABLE_DIR_ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let base = std::env::temp_dir().join(format!("cronus-sessions-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    // SAFETY: serialized by `PORTABLE_DIR_ENV_LOCK`.
    unsafe { std::env::set_var("CRONUS_PORTABLE_DIR", &base) };
    let root = cronus_core::paths::resolve_workspace_root();
    let sessions = cronus_core::session::journal::sessions_dir(&root);
    let result = body(&sessions);
    unsafe { std::env::remove_var("CRONUS_PORTABLE_DIR") };
    let _ = std::fs::remove_dir_all(&base);
    result
}

fn field<'a>(fields: &'a [(String, OutcomeValue)], name: &str) -> Option<&'a OutcomeValue> {
    fields.iter().find(|(k, _)| k == name).map(|(_, v)| v)
}

fn id_args(id: &str) -> ArgValues {
    let mut args = ArgValues::new();
    args.insert("id", ArgValue::Text(id.to_string()));
    args
}

const JOURNAL: &str = concat!(
    "{\"type\":\"session\",\"version\":1,\"createdAt\":1700000000}\n",
    "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"hello\"}}\n",
    "{\"type\":\"message\",\"message\":{\"role\":\"assistant\",\"content\":\"hi\"}}\n",
);

#[test]
fn a_workspace_with_no_sessions_lists_none_rather_than_failing() {
    let listed = in_isolated_workspace("none", |_| dispatch("core:session.list", ArgValues::new()));
    assert_eq!(listed, Outcome::Value(OutcomeValue::List(Vec::new())));
}

#[test]
fn a_written_session_is_listed_and_its_conversation_can_be_shown() {
    let (listed, shown, missing) = in_isolated_workspace("written", |sessions| {
        std::fs::create_dir_all(sessions).expect("creatable");
        std::fs::write(sessions.join("s-1.jsonl"), JOURNAL).expect("writable");
        (
            dispatch("core:session.list", ArgValues::new()),
            dispatch("core:session.show", id_args("s-1")),
            dispatch("core:session.show", id_args("absent")),
        )
    });

    let Outcome::Value(OutcomeValue::List(sessions)) = listed else {
        panic!("expected a list, got {listed:?}");
    };
    let [OutcomeValue::Record(session)] = sessions.as_slice() else {
        panic!("expected one session, got {sessions:?}");
    };
    assert_eq!(
        field(session, "id"),
        Some(&OutcomeValue::Text("s-1".to_string()))
    );
    assert_eq!(field(session, "turns"), Some(&OutcomeValue::Integer(1)));
    assert_eq!(field(session, "problem"), Some(&OutcomeValue::Empty));

    let Outcome::Value(OutcomeValue::Record(shown)) = shown else {
        panic!("expected a record, got {shown:?}");
    };
    let Some(OutcomeValue::List(messages)) = field(&shown, "messages") else {
        panic!("a transcript carries its messages: {shown:?}");
    };
    assert_eq!(messages.len(), 2);

    match missing {
        Outcome::Unavailable { reason } => assert!(reason.contains("absent"), "{reason}"),
        other => panic!("a session that does not exist is unavailable, got {other:?}"),
    }
}

#[test]
fn a_sessions_path_that_cannot_be_read_is_unavailable_not_an_empty_list() {
    let listed = in_isolated_workspace("unreadable", |sessions| {
        std::fs::create_dir_all(sessions.parent().expect("has a parent")).expect("creatable");
        std::fs::write(sessions, "this is a file, not a directory").expect("writable");
        dispatch("core:session.list", ArgValues::new())
    });
    assert!(
        matches!(listed, Outcome::Unavailable { .. }),
        "an unreadable store must not look like having no sessions: {listed:?}"
    );
}

// ── session.say: a turn against a (mock) local model server ─────────────────

use std::io::{Read, Write};
use std::net::TcpListener;

/// A one-shot mock server on loopback that streams `tokens` back as
/// OpenAI-shaped chunks and then finishes. Returns its URL.
fn spawn_mock_model(tokens: &'static [&'static str]) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let url = format!(
        "http://127.0.0.1:{}",
        listener.local_addr().expect("addr").port()
    );
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut acc = Vec::new();
        let mut buf = [0u8; 4096];
        while !acc.windows(4).any(|w| w == b"\r\n\r\n") {
            match stream.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => acc.extend_from_slice(&buf[..n]),
            }
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n")
            .expect("preamble");
        for token in tokens {
            let chunk = format!("{{\"choices\":[{{\"delta\":{{\"content\":\"{token}\"}}}}]}}");
            write!(stream, "data: {chunk}\n\n").expect("token");
        }
        stream.write_all(b"data: [DONE]\n\n").expect("done");
    });
    (url, handle)
}

fn say_args(text: &str, model: &str, session: Option<&str>) -> ArgValues {
    let mut args = ArgValues::new();
    args.insert("text", ArgValue::Text(text.to_string()));
    args.insert("model", ArgValue::Text(model.to_string()));
    if let Some(session) = session {
        args.insert("session", ArgValue::Text(session.to_string()));
    }
    args
}

#[test]
fn saying_something_records_both_sides_and_the_reply_can_be_read_back() {
    let (url, server) = spawn_mock_model(&["the gate ", "is red"]);
    let (said, shown) = in_isolated_workspace("say", |_| {
        // SAFETY: the same lock that guards CRONUS_PORTABLE_DIR is held.
        unsafe { std::env::set_var("CRONUS_CHAT_ENDPOINT", &url) };
        let said = dispatch(
            "core:session.say",
            say_args("what blocks the card?", "any-model", Some("s-say")),
        );
        unsafe { std::env::remove_var("CRONUS_CHAT_ENDPOINT") };
        (said, dispatch("core:session.show", id_args("s-say")))
    });
    server.join().expect("server thread");

    let Outcome::Value(OutcomeValue::Record(reply)) = said else {
        panic!("expected a reply record, got {said:?}");
    };
    assert_eq!(
        field(&reply, "reply"),
        Some(&OutcomeValue::Text("the gate is red".to_string()))
    );
    assert_eq!(
        field(&reply, "session"),
        Some(&OutcomeValue::Text("s-say".to_string()))
    );

    let Outcome::Value(OutcomeValue::Record(shown)) = shown else {
        panic!("expected the session, got {shown:?}");
    };
    let Some(OutcomeValue::List(messages)) = field(&shown, "messages") else {
        panic!("a transcript carries its messages");
    };
    assert_eq!(
        messages.len(),
        2,
        "the question and the answer: {messages:?}"
    );
    let OutcomeValue::Record(answer) = &messages[1] else {
        panic!("a message is a record");
    };
    assert_eq!(
        field(answer, "stop"),
        Some(&OutcomeValue::Text("stop".to_string()))
    );
}

#[test]
fn an_unreachable_model_is_reported_and_the_failed_turn_is_still_recorded() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let url = format!(
        "http://127.0.0.1:{}",
        listener.local_addr().expect("addr").port()
    );
    drop(listener); // nothing is listening there now

    let (said, shown) = in_isolated_workspace("say-down", |_| {
        unsafe { std::env::set_var("CRONUS_CHAT_ENDPOINT", &url) };
        let said = dispatch(
            "core:session.say",
            say_args("anyone there?", "any-model", Some("s-down")),
        );
        unsafe { std::env::remove_var("CRONUS_CHAT_ENDPOINT") };
        (said, dispatch("core:session.show", id_args("s-down")))
    });

    match said {
        Outcome::Unavailable { reason } => {
            assert!(
                reason.contains("model server at") && reason.contains(&url),
                "{reason}"
            );
            assert!(
                reason.contains("s-down"),
                "it says where the turn was recorded: {reason}"
            );
        }
        other => panic!("an unreachable model is unavailable, got {other:?}"),
    }
    let Outcome::Value(OutcomeValue::Record(shown)) = shown else {
        panic!("the failed turn is recorded, got {shown:?}");
    };
    let Some(OutcomeValue::List(messages)) = field(&shown, "messages") else {
        panic!("messages");
    };
    let OutcomeValue::Record(last) = messages.last().expect("a reply") else {
        panic!("a message is a record");
    };
    assert_eq!(
        field(last, "stop"),
        Some(&OutcomeValue::Text("error".to_string()))
    );
}

// ── session.stop ────────────────────────────────────────────────────────────

/// A mock model that sends one token, then holds the stream open for
/// `hold_ms` before finishing — long enough for a stop to arrive mid-turn.
fn spawn_slow_model(hold_ms: u64) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let url = format!(
        "http://127.0.0.1:{}",
        listener.local_addr().expect("addr").port()
    );
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut acc = Vec::new();
        let mut buf = [0u8; 4096];
        while !acc.windows(4).any(|w| w == b"\r\n\r\n") {
            match stream.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => acc.extend_from_slice(&buf[..n]),
            }
        }
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n");
        let _ = stream.write_all(b"data: {\"choices\":[{\"delta\":{\"content\":\"start\"}}]}\n\n");
        let _ = stream.flush();
        std::thread::sleep(std::time::Duration::from_millis(hold_ms));
        let _ = stream.write_all(b"data: {\"choices\":[{\"delta\":{\"content\":\" end\"}}]}\n\n");
        let _ = stream.write_all(b"data: [DONE]\n\n");
    });
    (url, handle)
}

#[test]
fn stopping_a_session_with_no_turn_running_says_so() {
    let stopped = in_isolated_workspace("stop-idle", |_| {
        dispatch("core:session.stop", id_args("s-nothing"))
    });
    match stopped {
        Outcome::Unavailable { reason } => assert!(reason.contains("s-nothing"), "{reason}"),
        other => panic!("nothing to stop is unavailable, got {other:?}"),
    }
}

#[test]
fn a_turn_stopped_midway_is_recorded_as_cut_short_with_what_had_arrived() {
    let (url, server) = spawn_slow_model(600);
    let (said, shown) = in_isolated_workspace("stop-mid", |_| {
        unsafe { std::env::set_var("CRONUS_CHAT_ENDPOINT", &url) };
        let turn = std::thread::spawn(|| {
            dispatch(
                "core:session.say",
                say_args("tell me a long story", "any-model", Some("s-long")),
            )
        });
        // Let the first token arrive, then stop the turn while it is held open.
        std::thread::sleep(std::time::Duration::from_millis(250));
        let mut stopped = false;
        for _ in 0..200 {
            if let Outcome::Value(_) = dispatch("core:session.stop", id_args("s-long")) {
                stopped = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(stopped, "the running turn was found and stopped");
        let said = turn.join().expect("the turn thread finishes");
        unsafe { std::env::remove_var("CRONUS_CHAT_ENDPOINT") };
        (said, dispatch("core:session.show", id_args("s-long")))
    });
    server.join().expect("server thread");

    match said {
        Outcome::Unavailable { reason } => assert!(reason.contains("cancelled"), "{reason}"),
        other => panic!("a stopped turn is not a finished one, got {other:?}"),
    }
    let Outcome::Value(OutcomeValue::Record(shown)) = shown else {
        panic!("the stopped turn is recorded, got {shown:?}");
    };
    let Some(OutcomeValue::List(messages)) = field(&shown, "messages") else {
        panic!("messages");
    };
    let OutcomeValue::Record(last) = messages.last().expect("a reply") else {
        panic!("a message is a record");
    };
    assert_eq!(
        field(last, "stop"),
        Some(&OutcomeValue::Text("aborted".to_string()))
    );
    assert_eq!(
        field(last, "text"),
        Some(&OutcomeValue::Text("start".to_string())),
        "what arrived before the stop is kept"
    );
}

#[test]
fn a_reply_arrives_token_by_token_while_the_model_is_still_writing() {
    use cronus_core::invocable::{Polled, StreamItem};

    let (url, server) = spawn_slow_model(700);
    let (first_at, chunks, end) = in_isolated_workspace("streams", |_| {
        unsafe { std::env::set_var("CRONUS_CHAT_ENDPOINT", &url) };
        let (registry, dispatcher) = bootstrap(Engine::new());
        let started = std::time::Instant::now();
        let outcome = match dispatcher.dispatch(
            &registry,
            &Invocation {
                id: InvocableId::new("core:session.say").expect("well-formed"),
                args: say_args("tell me", "any-model", Some("s-live")),
                caller: Surface::Cli,
            },
        ) {
            Dispatched::Ran(outcome) => outcome,
            Dispatched::Unknown => panic!("session.say must resolve"),
        };
        let Outcome::Stream(handle) = outcome else {
            panic!("a turn answers with a stream, got {outcome:?}");
        };
        let mut subscription = dispatcher.subscribe(&handle).expect("subscribes");
        let mut first_at = None;
        let mut chunks = Vec::new();
        let end = loop {
            match subscription.poll() {
                Polled::Item(StreamItem::Chunk(OutcomeValue::Text(t))) => {
                    first_at.get_or_insert(started.elapsed());
                    chunks.push(t);
                }
                Polled::Item(StreamItem::Chunk(other)) => panic!("{other:?}"),
                Polled::Item(StreamItem::End(end)) => break end,
                Polled::Empty => std::thread::sleep(std::time::Duration::from_millis(5)),
                Polled::Finished => panic!("finished without an end"),
            }
        };
        unsafe { std::env::remove_var("CRONUS_CHAT_ENDPOINT") };
        (first_at, chunks, end)
    });
    server.join().expect("server thread");

    assert_eq!(chunks.concat(), "start end");
    assert!(chunks.len() >= 2, "more than one piece: {chunks:?}");
    assert!(
        first_at.is_some_and(|at| at < std::time::Duration::from_millis(600)),
        "the first token arrived before the model finished: {first_at:?}"
    );
    let Outcome::Value(OutcomeValue::Record(settled)) = end else {
        panic!("the end is the settled result, got {end:?}");
    };
    assert_eq!(
        field(&settled, "reply"),
        Some(&OutcomeValue::Text("start end".to_string()))
    );
}
