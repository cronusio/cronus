use std::sync::Arc;

use cronus_contract::CancelHandle;
use cronus_contract::{Binder, BinderKind, Invocable, Locus, Outcome, OutcomeValue, Stability};
use cronus_domain::invocable::{Dispatcher, InvocableRegistry, Registrant};
use cronus_domain::session::chat;
use cronus_domain::session::journal::{self, JournalError, SessionSummary};
use cronus_domain::session::writer::StopReason;

use crate::endpoint::ledger::WorkKind;
use crate::model::{Capabilities, EndpointProfile, ProtocolFamily};

use super::{begin_work, core_id, opt_text_arg, text_arg};

/// A session belongs to a specific project's work — its journals live in the
/// current workspace's `sessions/`, like every other project-scoped verb's
/// state.
fn sessions_dir() -> std::path::PathBuf {
    journal::sessions_dir(&cronus_domain::paths::resolve_workspace_root())
}

/// The most messages a transcript carries; earlier ones are counted, not sent.
const TRANSCRIPT_CAP: usize = 200;

pub(super) fn register(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    register_list(registry, dispatcher);
    register_show(registry, dispatcher);
    register_say(registry, dispatcher);
    register_stop(registry, dispatcher);
}

fn optional_text(value: Option<&str>) -> OutcomeValue {
    match value {
        Some(text) => OutcomeValue::Text(text.to_string()),
        None => OutcomeValue::Empty,
    }
}

fn optional_seconds(value: Option<u64>) -> OutcomeValue {
    match value {
        Some(seconds) => OutcomeValue::Integer(i64::try_from(seconds).unwrap_or(i64::MAX)),
        None => OutcomeValue::Empty,
    }
}

fn count(n: usize) -> OutcomeValue {
    OutcomeValue::Integer(i64::try_from(n).unwrap_or(i64::MAX))
}

/// The fields a listing and a transcript share.
fn summary_fields(summary: &SessionSummary) -> Vec<(String, OutcomeValue)> {
    vec![
        ("id".to_string(), OutcomeValue::Text(summary.id.clone())),
        ("name".to_string(), optional_text(summary.name.as_deref())),
        (
            "created_at".to_string(),
            optional_seconds(summary.created_at),
        ),
        (
            "updated_at".to_string(),
            optional_seconds(summary.updated_at),
        ),
        ("entries".to_string(), count(summary.entries)),
        ("turns".to_string(), count(summary.turns)),
        (
            "problem".to_string(),
            optional_text(summary.problem.as_deref()),
        ),
    ]
}

fn unavailable(error: JournalError) -> Outcome {
    Outcome::Unavailable {
        reason: error.to_string(),
    }
}

fn register_list(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("session.list");
    let invocable = Invocable {
        id: id.clone(),
        name: "List",
        summary: "List this workspace's sessions, most recently written first.",
        group: "session",
        locus: Locus::Semantic,
        binders: Vec::<Binder>::new(),
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:session.list registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|_args| match journal::list(&sessions_dir()) {
            Ok(sessions) => Outcome::Value(OutcomeValue::List(
                sessions
                    .iter()
                    .map(|s| OutcomeValue::Record(summary_fields(s)))
                    .collect(),
            )),
            Err(e) => unavailable(e),
        }),
    );
}

fn register_show(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("session.show");
    let invocable = Invocable {
        id: id.clone(),
        name: "Show",
        summary: "Show a session and its conversation.",
        group: "session",
        locus: Locus::Semantic,
        binders: vec![Binder {
            name: "id",
            kind: BinderKind::Text,
            optional: false,
        }],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:session.show registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(
            |args| match journal::read(&sessions_dir(), text_arg(args, "id")) {
                Ok(transcript) => {
                    let omitted = transcript.messages.len().saturating_sub(TRANSCRIPT_CAP);
                    let mut fields = summary_fields(&transcript.summary);
                    fields.push(("omitted".to_string(), count(omitted)));
                    fields.push((
                        "messages".to_string(),
                        OutcomeValue::List(
                            transcript
                                .messages
                                .iter()
                                .skip(omitted)
                                .map(|m| {
                                    OutcomeValue::Record(vec![
                                        ("role".to_string(), OutcomeValue::Text(m.role.clone())),
                                        ("text".to_string(), OutcomeValue::Text(m.text.clone())),
                                        ("stop".to_string(), optional_text(m.stop.as_deref())),
                                        ("error".to_string(), optional_text(m.error.as_deref())),
                                    ])
                                })
                                .collect(),
                        ),
                    ));
                    Outcome::Value(OutcomeValue::Record(fields))
                }
                Err(e) => unavailable(e),
            },
        ),
    );
}

/// Where the local model server is expected unless `CRONUS_CHAT_ENDPOINT`
/// names another loopback address (the Ollama convention).
const DEFAULT_CHAT_ENDPOINT: &str = "http://localhost:11434";

fn chat_endpoint() -> String {
    std::env::var("CRONUS_CHAT_ENDPOINT").unwrap_or_else(|_| DEFAULT_CHAT_ENDPOINT.to_string())
}

/// The turns running in this process, by session, so another call can stop
/// one. A turn in another process cannot be reached from here.
static IN_FLIGHT: std::sync::Mutex<Vec<(String, CancelHandle)>> = std::sync::Mutex::new(Vec::new());

fn in_flight() -> std::sync::MutexGuard<'static, Vec<(String, CancelHandle)>> {
    IN_FLIGHT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A turn's entry in [`IN_FLIGHT`], removed when the turn ends however it ends.
struct RunningTurn {
    session: String,
}

impl RunningTurn {
    fn register(session: &str, cancel: &CancelHandle) -> Self {
        in_flight().push((session.to_string(), cancel.clone()));
        RunningTurn {
            session: session.to_string(),
        }
    }
}

impl Drop for RunningTurn {
    fn drop(&mut self) {
        in_flight().retain(|(session, _)| *session != self.session);
    }
}

fn register_stop(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("session.stop");
    let invocable = Invocable {
        id: id.clone(),
        name: "Stop",
        summary: "Stop the turn running in a session. The reply is recorded as cut short.",
        group: "session",
        locus: Locus::Semantic,
        binders: vec![Binder {
            name: "id",
            kind: BinderKind::Text,
            optional: false,
        }],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:session.stop registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let session = text_arg(args, "id");
            let running = in_flight()
                .iter()
                .find(|(running, _)| running == session)
                .map(|(_, cancel)| cancel.clone());
            match running {
                Some(cancel) => {
                    cancel.cancel();
                    Outcome::Value(OutcomeValue::Record(vec![
                        (
                            "session".to_string(),
                            OutcomeValue::Text(session.to_string()),
                        ),
                        ("stopped".to_string(), OutcomeValue::Boolean(true)),
                    ]))
                }
                None => Outcome::Unavailable {
                    reason: format!("no turn is running in session {session} in this process"),
                },
            }
        }),
    );
}

fn register_say(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("session.say");
    let invocable = Invocable {
        id: id.clone(),
        name: "Say",
        summary: "Say something to a local model in a session and record the reply. A new \
                  session is started unless --session names one.",
        group: "session",
        locus: Locus::Semantic,
        binders: vec![
            Binder {
                name: "text",
                kind: BinderKind::Text,
                optional: false,
            },
            Binder {
                name: "model",
                kind: BinderKind::NamedText,
                optional: false,
            },
            Binder {
                name: "session",
                kind: BinderKind::NamedText,
                optional: true,
            },
        ],
        stability: Stability::Shipped,
        // What a person says to a model may itself be a secret; the dispatch
        // journal must not keep it. The conversation's own journal does.
        journal_raw_input: false,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:session.say registers cleanly at bootstrap — a duplicate id here is a bug");
    let streams = dispatcher.streams();
    dispatcher.attach(
        id,
        Arc::new(move |args| {
            let endpoint = chat_endpoint();
            let backend = match EndpointProfile::new(
                endpoint.clone(),
                ProtocolFamily::OpenAiCompatible,
                Capabilities {
                    streaming: true,
                    ..Default::default()
                },
            ) {
                Ok(backend) => backend,
                Err(e) => {
                    return Outcome::Unavailable {
                        reason: format!("the model endpoint '{endpoint}' is not usable: {e:?}"),
                    };
                }
            };
            let session = opt_text_arg(args, "session")
                .map(str::to_string)
                .unwrap_or_else(journal::new_session_id);
            let (model, text) = (
                text_arg(args, "model").to_string(),
                text_arg(args, "text").to_string(),
            );
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);

            // The turn runs on a thread of its own and answers as it goes: each
            // token is a chunk, and the settled result is the stream's end.
            // It is registered as running before this call returns, so a stop
            // that follows at once finds it.
            //
            // It also enters the engine's work ledger first: while an
            // installation change is being applied no turn begins, and the
            // change is refused while one is running.
            let work = match begin_work(WorkKind::AgentTurn, session.clone()) {
                Ok(work) => work,
                Err(refused) => return refused,
            };
            let (handle, sink) = streams.open();
            let cancel = CancelHandle::new();
            let running = RunningTurn::register(&session, &cancel);
            std::thread::spawn(move || {
                let _running = running;
                let _work = work;
                let dir = sessions_dir();
                let result = chat::run_turn(
                    chat::SessionSlot {
                        dir: &dir,
                        id: &session,
                        now_secs: now,
                    },
                    &backend,
                    &model,
                    &text,
                    cancel,
                    &mut |token| {
                        sink.chunk(OutcomeValue::Text(token.to_string()));
                    },
                );
                sink.end(settle_turn(result, &endpoint));
            });
            Outcome::Stream(handle)
        }),
    );
}

/// What a finished turn settles to: the reply and the session it is in, or —
/// when the turn did not complete — the reason, naming where it was recorded.
fn settle_turn(result: Result<chat::TurnReport, JournalError>, endpoint: &str) -> Outcome {
    let report = match result {
        Ok(report) => report,
        Err(e) => return unavailable(e),
    };
    if report.stop != StopReason::Stop {
        let why = match report.error.as_deref() {
            Some("ConnectRefused") => format!("cannot reach the model server at {endpoint}"),
            // Some hosts report a closed port as a timeout.
            Some("Timeout") => {
                format!("the model server at {endpoint} did not answer — is it running?")
            }
            Some(other) => other.to_string(),
            None => report.stop.as_str().to_string(),
        };
        return Outcome::Unavailable {
            reason: format!(
                "{why} — the turn is recorded in session {}; read it with `session show {}`",
                report.session_id, report.session_id
            ),
        };
    }
    let tokens = |n: Option<u64>| match n.and_then(|n| i64::try_from(n).ok()) {
        Some(n) => OutcomeValue::Integer(n),
        None => OutcomeValue::Empty,
    };
    Outcome::Value(OutcomeValue::Record(vec![
        (
            "session".to_string(),
            OutcomeValue::Text(report.session_id.clone()),
        ),
        ("reply".to_string(), OutcomeValue::Text(report.reply)),
        ("prompt_tokens".to_string(), tokens(report.prompt_tokens)),
        (
            "completion_tokens".to_string(),
            tokens(report.completion_tokens),
        ),
    ]))
}
