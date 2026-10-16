//! One conversational turn against a model, recorded in the session's
//! journal: the smallest runner there is.
//!
//! A turn takes the user's message, asks a model for a reply and records both.
//! It has no tools, no background work and no memory beyond the journal — the
//! model sees the conversation so far because it is read back from the
//! journal, and for no other reason. Whatever the model does, the turn ends
//! with an assistant entry: a failed or cut-short reply is recorded as the
//! reply it would have been, with the stop reason saying what happened, so the
//! journal never ends mid-air and the next turn never wonders.
//!
//! The model is reached through [`InferenceBackend`] only. Which backend, which
//! model, and whether to call one at all are the caller's decisions.

use std::path::Path;

use cronus_contract::{
    CancelHandle, GenerateRequest, InferenceBackend, InferenceError, StreamEvent,
};

use super::journal::{self, JournalError, TranscriptMessage};
use super::writer::{JournalWriter, StopReason};

/// What one turn produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnReport {
    /// The session the turn belongs to.
    pub session_id: String,
    /// What the model said; possibly partial, possibly nothing.
    pub reply: String,
    /// How the reply ended.
    pub stop: StopReason,
    /// What went wrong, when `stop` is not [`StopReason::Stop`].
    pub error: Option<String>,
    /// Tokens the model reported reading, when it reported.
    pub prompt_tokens: Option<u64>,
    /// Tokens the model reported writing, when it reported.
    pub completion_tokens: Option<u64>,
    /// Whether an earlier unanswered turn had to be closed first.
    pub closed_interrupted_turn: bool,
}

/// The conversation so far as one prompt: each message labelled by its
/// speaker, then the new user message and the cue for the reply. Replies that
/// were cut short and produced nothing carry no information and are left out.
pub fn prompt_from(history: &[TranscriptMessage], user_text: &str) -> String {
    let label = |role: &str| match role {
        "user" => "User".to_string(),
        "assistant" => "Assistant".to_string(),
        other => other.to_string(),
    };
    let mut prompt = String::new();
    for message in history {
        if message.role == "assistant" && message.text.is_empty() {
            continue;
        }
        prompt.push_str(&format!("{}: {}\n\n", label(&message.role), message.text));
    }
    prompt.push_str(&format!("User: {user_text}\n\nAssistant:"));
    prompt
}

/// Where a turn is recorded: the session to continue, or to start if it does
/// not exist yet.
#[derive(Debug, Clone, Copy)]
pub struct SessionSlot<'a> {
    /// The directory holding the workspace's session journals.
    pub dir: &'a Path,
    /// The session's id.
    pub id: &'a str,
    /// Stamps the journal header when the session is started.
    pub now_secs: u64,
}

/// Run one turn in `slot`: continue the session if it exists (closing a turn
/// an earlier crash left unanswered), start it if it does not. `on_token` is
/// handed each piece of the reply as it arrives, before the turn is finished
/// and recorded.
pub fn run_turn(
    slot: SessionSlot<'_>,
    backend: &dyn InferenceBackend,
    model: &str,
    user_text: &str,
    cancel: CancelHandle,
    on_token: &mut dyn FnMut(&str),
) -> Result<TurnReport, JournalError> {
    let SessionSlot {
        dir,
        id: session_id,
        now_secs,
    } = slot;
    let (mut writer, history, closed) = match journal::read(dir, session_id) {
        Ok(transcript) => {
            let mut writer = JournalWriter::open(dir, session_id)?;
            let closed = writer.close_interrupted_turn()?;
            (writer, transcript.messages, closed)
        }
        Err(JournalError::NotFound(_)) => (
            JournalWriter::create(dir, session_id, now_secs)?,
            Vec::new(),
            false,
        ),
        Err(other) => return Err(other),
    };

    writer.append_message("user", user_text)?;
    let request = GenerateRequest {
        model: model.to_string(),
        prompt: prompt_from(&history, user_text),
        parameters: Vec::new(),
    };

    let mut reply = String::new();
    let mut stop = StopReason::Stop;
    let mut error = None;
    let mut prompt_tokens = None;
    let mut completion_tokens = None;
    let mut finished = false;
    for event in backend.generate_stream(request, cancel) {
        match event {
            StreamEvent::Token(text) => {
                on_token(&text);
                reply.push_str(&text);
            }
            StreamEvent::Usage {
                prompt_tokens: p,
                completion_tokens: c,
            } => {
                prompt_tokens = Some(p);
                completion_tokens = Some(c);
            }
            StreamEvent::ToolCall { name, .. } => {
                stop = StopReason::Error;
                error = Some(format!(
                    "the model asked for the tool '{name}'; tools are not available in this turn"
                ));
                finished = true;
                break;
            }
            StreamEvent::Done => {
                finished = true;
                break;
            }
            StreamEvent::Error(InferenceError::Cancelled) => {
                stop = StopReason::Aborted;
                error = Some("cancelled".to_string());
                finished = true;
                break;
            }
            StreamEvent::Error(failure) => {
                stop = StopReason::Error;
                error = Some(format!("{failure:?}"));
                finished = true;
                break;
            }
        }
    }
    if !finished {
        stop = StopReason::Error;
        error = Some("the model's stream ended without completing".to_string());
    }

    writer.append_reply(&reply, stop, error.as_deref())?;
    Ok(TurnReport {
        session_id: session_id.to_string(),
        reply,
        stop,
        error,
        prompt_tokens,
        completion_tokens,
        closed_interrupted_turn: closed,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Mutex;

    use cronus_contract::{ModelDescriptor, PullProgress, ResidencyHint};

    use super::*;

    /// A backend that plays back one scripted stream per call and records the
    /// prompts it was given.
    struct Scripted {
        streams: Mutex<Vec<Vec<StreamEvent>>>,
        prompts: Mutex<Vec<String>>,
    }

    impl Scripted {
        fn new(streams: Vec<Vec<StreamEvent>>) -> Self {
            Scripted {
                streams: Mutex::new(streams),
                prompts: Mutex::new(Vec::new()),
            }
        }
    }

    impl InferenceBackend for Scripted {
        fn generate_stream(
            &self,
            request: GenerateRequest,
            _cancel: CancelHandle,
        ) -> Box<dyn Iterator<Item = StreamEvent> + Send> {
            self.prompts.lock().expect("lock").push(request.prompt);
            let events = self.streams.lock().expect("lock").remove(0);
            Box::new(events.into_iter())
        }

        fn embed(&self, _model: &str, _input: &str) -> Result<Vec<f32>, InferenceError> {
            Err(InferenceError::Unsupported)
        }

        fn describe(&self, _model: &str) -> Result<ModelDescriptor, InferenceError> {
            Err(InferenceError::Unsupported)
        }

        fn pull(&self, _model: &str) -> Box<dyn Iterator<Item = PullProgress> + Send> {
            Box::new(std::iter::empty())
        }

        fn set_residency(&self, _model: &str, _hint: ResidencyHint) -> Result<(), InferenceError> {
            Err(InferenceError::Unsupported)
        }
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cronus-chat-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn token(text: &str) -> StreamEvent {
        StreamEvent::Token(text.to_string())
    }

    fn turn(dir: &Path, backend: &Scripted, text: &str) -> Result<TurnReport, JournalError> {
        run_turn(
            SessionSlot {
                dir,
                id: "s-1",
                now_secs: 100,
            },
            backend,
            "m",
            text,
            CancelHandle::new(),
            &mut |_| {},
        )
    }

    #[test]
    fn a_turn_records_the_question_and_the_streamed_answer() {
        let dir = temp_dir("happy");
        let backend = Scripted::new(vec![vec![
            token("the gate "),
            token("is red"),
            StreamEvent::Usage {
                prompt_tokens: 7,
                completion_tokens: 4,
            },
            StreamEvent::Done,
        ]]);
        let report = turn(&dir, &backend, "what blocks the card?").expect("runs");
        assert_eq!(report.reply, "the gate is red");
        assert_eq!(report.stop, StopReason::Stop);
        assert_eq!(
            (report.prompt_tokens, report.completion_tokens),
            (Some(7), Some(4))
        );

        let transcript = journal::read(&dir, "s-1").expect("readable");
        let said: Vec<(&str, &str)> = transcript
            .messages
            .iter()
            .map(|m| (m.role.as_str(), m.text.as_str()))
            .collect();
        assert_eq!(
            said,
            [
                ("user", "what blocks the card?"),
                ("assistant", "the gate is red")
            ]
        );
        assert_eq!(transcript.messages[1].stop.as_deref(), Some("stop"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn each_token_is_handed_over_as_it_arrives_before_the_turn_is_recorded() {
        let dir = temp_dir("tokens");
        let backend = Scripted::new(vec![vec![token("a"), token("b"), StreamEvent::Done]]);
        let mut seen = Vec::new();
        let on_disk_when_first_token_arrived = std::cell::Cell::new(None);
        let report = run_turn(
            SessionSlot {
                dir: &dir,
                id: "s-1",
                now_secs: 0,
            },
            &backend,
            "m",
            "q",
            CancelHandle::new(),
            &mut |piece| {
                if on_disk_when_first_token_arrived.get().is_none() {
                    on_disk_when_first_token_arrived.set(Some(dir.join("s-1.jsonl").exists()));
                }
                seen.push(piece.to_string());
            },
        )
        .expect("runs");
        assert_eq!(seen, ["a", "b"]);
        assert_eq!(report.reply, "ab");
        assert_eq!(
            on_disk_when_first_token_arrived.get(),
            Some(false),
            "the journal is written once the reply is whole, not token by token"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_model_sees_the_conversation_so_far_because_it_is_read_back_from_the_journal() {
        let dir = temp_dir("history");
        let backend = Scripted::new(vec![
            vec![token("first answer"), StreamEvent::Done],
            vec![token("second answer"), StreamEvent::Done],
        ]);
        turn(&dir, &backend, "first question").expect("runs");
        turn(&dir, &backend, "second question").expect("runs");

        let prompts = backend.prompts.lock().expect("lock").clone();
        assert_eq!(prompts[0], "User: first question\n\nAssistant:");
        assert_eq!(
            prompts[1],
            "User: first question\n\nAssistant: first answer\n\nUser: second question\n\nAssistant:"
        );
        assert_eq!(
            journal::read(&dir, "s-1").expect("readable").summary.turns,
            2
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_stream_is_a_recorded_failure_with_whatever_was_said_first() {
        let dir = temp_dir("failed");
        let backend = Scripted::new(vec![vec![
            token("half an ans"),
            StreamEvent::Error(InferenceError::Timeout),
        ]]);
        let report = turn(&dir, &backend, "q").expect("the turn is recorded, not lost");
        assert_eq!(report.stop, StopReason::Error);
        assert_eq!(report.error.as_deref(), Some("Timeout"));
        assert_eq!(report.reply, "half an ans");

        let reply = &journal::read(&dir, "s-1").expect("readable").messages[1];
        assert_eq!(reply.stop.as_deref(), Some("error"));
        assert_eq!(reply.text, "half an ans");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancellation_is_an_aborted_reply_not_an_error() {
        let dir = temp_dir("cancel");
        let backend = Scripted::new(vec![vec![
            token("par"),
            StreamEvent::Error(InferenceError::Cancelled),
        ]]);
        let report = turn(&dir, &backend, "q").expect("recorded");
        assert_eq!(report.stop, StopReason::Aborted);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stream_that_just_stops_and_a_tool_request_are_both_failures_not_answers() {
        let dir = temp_dir("odd");
        let backend = Scripted::new(vec![
            vec![token("cut")],
            vec![StreamEvent::ToolCall {
                name: "shell".to_string(),
                arguments: "{}".to_string(),
            }],
        ]);
        let silent = turn(&dir, &backend, "q1").expect("recorded");
        assert_eq!(silent.stop, StopReason::Error);
        assert!(
            silent
                .error
                .as_deref()
                .is_some_and(|e| e.contains("without completing"))
        );

        let tool = turn(&dir, &backend, "q2").expect("recorded");
        assert_eq!(tool.stop, StopReason::Error);
        assert!(tool.error.as_deref().is_some_and(|e| e.contains("'shell'")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_turn_left_unanswered_by_a_crash_is_closed_before_the_next_one_and_not_shown_to_the_model()
    {
        let dir = temp_dir("crash");
        let mut writer = JournalWriter::create(&dir, "s-1", 0).expect("creates");
        writer
            .append_message("assistant", "earlier")
            .expect("appends");
        writer
            .append_message("user", "lost question")
            .expect("appends");
        drop(writer);

        let backend = Scripted::new(vec![vec![token("ok"), StreamEvent::Done]]);
        let report = turn(&dir, &backend, "new question").expect("runs");
        assert!(report.closed_interrupted_turn);

        let prompt = backend.prompts.lock().expect("lock")[0].clone();
        assert!(
            prompt.contains("User: lost question"),
            "the question is history: {prompt}"
        );
        assert!(
            !prompt.contains("Assistant: \n"),
            "the empty closer carries nothing: {prompt}"
        );
        let roles: Vec<String> = journal::read(&dir, "s-1")
            .expect("readable")
            .messages
            .into_iter()
            .map(|m| m.role)
            .collect();
        assert_eq!(
            roles,
            ["assistant", "user", "assistant", "user", "assistant"],
            "nothing removed, the gap closed"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
