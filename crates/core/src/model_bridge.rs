//! The nodus↔`InferenceBackend` bridge (§4.1, MR-2).
//!
//! `nodus` is a zero-dependency workflow runtime whose model-backed steps
//! (`GEN`, `ANALYZE`) drive its own `ModelProvider` trait — a minimal
//! synchronous surface (`generate(prompt) -> String`, `analyze`). The
//! transport realizes `contract::InferenceBackend` (a streaming call
//! surface). This bridge, living in the facade so nodus stays
//! dependency-free (LP-1), satisfies `nodus::ModelProvider` by collapsing an
//! `InferenceBackend` stream into the `String` nodus expects — the one place
//! the two provider vocabularies meet.
//!
//! It replaces `nodus`'s built-in `StubProvider` (the `[STUB gen(...)]`
//! label) with real generation once a backend is wired.

use std::sync::Arc;

use cronus_contract::{
    CancelHandle, GenerateRequest, InferenceBackend, InferenceError, StreamEvent,
};
use nodus::{ModelError, ModelProvider, Value};

/// Adapts a `contract::InferenceBackend` to `nodus::ModelProvider`.
///
/// Holds the backend behind an `Arc` (cheap to share and to move into the
/// executor) plus the model name each call targets.
pub struct NodusModelBridge {
    backend: Arc<dyn InferenceBackend>,
    model: String,
}

impl NodusModelBridge {
    pub fn new(backend: Arc<dyn InferenceBackend>, model: impl Into<String>) -> Self {
        NodusModelBridge {
            backend,
            model: model.into(),
        }
    }

    /// Drive one generation to completion, concatenating token text.
    ///
    /// A call is only successful if the stream says so: text that arrived
    /// before an error, a cancellation or a stream that ends without its
    /// completion marker is a fragment, and handing it back as the model's
    /// answer would let a workflow carry on from a truncated plan or an
    /// empty result as though the call had worked. Non-text events (tool
    /// calls, usage) are not folded into the generated text.
    fn collect(
        &self,
        prompt: &str,
        parameters: Vec<(String, String)>,
    ) -> Result<String, ModelError> {
        let request = GenerateRequest {
            model: self.model.clone(),
            prompt: prompt.to_string(),
            parameters,
        };
        let mut out = String::new();
        for event in self.backend.generate_stream(request, CancelHandle::new()) {
            match event {
                StreamEvent::Token(t) => out.push_str(&t),
                StreamEvent::Done => return Ok(out),
                StreamEvent::Error(failure) => return Err(ModelError::new(describe(&failure))),
                StreamEvent::ToolCall { .. } | StreamEvent::Usage { .. } => {}
            }
        }
        Err(ModelError::new("the stream ended without completing"))
    }
}

/// A short, secret-free description of a transport failure. The variant is
/// named, never the raw payload a malformed stream carried — that can hold
/// fragments of the response, and this text lands in the run record.
fn describe(failure: &InferenceError) -> String {
    match failure {
        InferenceError::ConnectRefused => "the model endpoint refused the connection".to_string(),
        InferenceError::Timeout => "the model endpoint timed out".to_string(),
        InferenceError::ClientError(status) => {
            format!("the model endpoint rejected the request (HTTP {status})")
        }
        InferenceError::ServerError(status) => {
            format!("the model endpoint failed (HTTP {status})")
        }
        InferenceError::MalformedStream(_) => "the model endpoint sent a malformed stream".into(),
        InferenceError::Cancelled => "the call was cancelled".to_string(),
        InferenceError::Unsupported => {
            "the model endpoint does not support the operation".to_string()
        }
    }
}

impl ModelProvider for NodusModelBridge {
    fn model_id(&self) -> &str {
        &self.model
    }

    /// The infallible surface cannot report a failure, so a failed call yields
    /// an empty string rather than a fragment of an answer. The executor never
    /// takes this path — it calls [`try_generate`](Self::try_generate).
    fn generate(&self, prompt: &str, modifiers: &[(String, String)]) -> String {
        self.collect(prompt, modifiers.to_vec()).unwrap_or_default()
    }

    fn analyze(&self, text: &str, flags: &[String]) -> Value {
        self.try_analyze(text, flags)
            .unwrap_or_else(|_| null_verdict(flags))
    }

    fn try_generate(
        &self,
        prompt: &str,
        modifiers: &[(String, String)],
    ) -> Result<String, ModelError> {
        self.collect(prompt, modifiers.to_vec())
    }

    fn try_analyze(&self, text: &str, flags: &[String]) -> Result<Value, ModelError> {
        // Realize `analyze` over the one call surface the backend exposes
        // (generation): ask for a JSON verdict, parse it, and project each
        // requested flag. A flag the model did not answer stays `Null` —
        // never a fabricated score (contrast the stub's constant 0.9).
        let raw = self.collect(&build_analysis_prompt(text, flags), Vec::new())?;
        Ok(parse_analysis(&raw, flags))
    }
}

/// The verdict of a call that failed: every requested flag unanswered.
fn null_verdict(flags: &[String]) -> Value {
    Value::Map(flags.iter().map(|f| (f.clone(), Value::Null)).collect())
}

fn build_analysis_prompt(text: &str, flags: &[String]) -> String {
    let flag_list = flags.join(", ");
    format!(
        "Analyze the following text and respond with a single JSON object whose keys are \
         exactly [{flag_list}]. Each value is a number in 0.0..1.0 for a score, or a short \
         string for a label. Respond with only the JSON object.\n\nText:\n{text}"
    )
}

/// Parse a model's analysis response into a `nodus::Value::Map`, one entry
/// per requested flag in order. Robust to prose or code-fence wrapping: the
/// first `{`..last `}` span is parsed as JSON. Any flag absent from the
/// parsed object — or a wholly unparseable response — yields `Value::Null`
/// for that flag, so the map never invents a verdict.
fn parse_analysis(raw: &str, flags: &[String]) -> Value {
    let json =
        extract_json_object(raw).and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());
    let entries = flags
        .iter()
        .map(|flag| {
            let value = json
                .as_ref()
                .and_then(|j| j.get(flag))
                .map(json_to_nodus)
                .unwrap_or(Value::Null);
            (flag.clone(), value)
        })
        .collect();
    Value::Map(entries)
}

/// Return the `{`..`}` substring (inclusive) of the first JSON object in
/// `raw`, or `None` if there is no balanced-looking object.
fn extract_json_object(raw: &str) -> Option<&str> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    if end > start {
        Some(&raw[start..=end])
    } else {
        None
    }
}

fn json_to_nodus(v: &serde_json::Value) -> Value {
    match v {
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else {
                Value::Float(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => Value::Text(s.clone()),
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cronus_contract::{
        GenerateRequest, InferenceError, ModelDescriptor, PullProgress, ResidencyHint,
    };

    /// A test backend that yields a fixed script of tokens for every
    /// generation — enough to exercise the bridge without a network.
    struct ScriptedBackend {
        tokens: Vec<&'static str>,
    }

    impl InferenceBackend for ScriptedBackend {
        fn generate_stream(
            &self,
            _request: GenerateRequest,
            _cancel: CancelHandle,
        ) -> Box<dyn Iterator<Item = StreamEvent> + Send> {
            let mut events: Vec<StreamEvent> = self
                .tokens
                .iter()
                .map(|t| StreamEvent::Token(t.to_string()))
                .collect();
            events.push(StreamEvent::Done);
            Box::new(events.into_iter())
        }

        fn embed(&self, _model: &str, _input: &str) -> Result<Vec<f32>, InferenceError> {
            Err(InferenceError::Unsupported)
        }

        fn describe(&self, model: &str) -> Result<ModelDescriptor, InferenceError> {
            Ok(ModelDescriptor {
                name: model.to_string(),
                ..Default::default()
            })
        }

        fn pull(&self, _model: &str) -> Box<dyn Iterator<Item = PullProgress> + Send> {
            Box::new(std::iter::once(PullProgress::Done { digest: None }))
        }

        fn set_residency(&self, _model: &str, _hint: ResidencyHint) -> Result<(), InferenceError> {
            Ok(())
        }
    }

    fn bridge(tokens: Vec<&'static str>) -> NodusModelBridge {
        NodusModelBridge::new(Arc::new(ScriptedBackend { tokens }), "test-model")
    }

    /// A backend whose stream ends the way `ending` says, after a few tokens.
    struct EndingBackend {
        ending: Option<StreamEvent>,
    }

    impl InferenceBackend for EndingBackend {
        fn generate_stream(
            &self,
            _request: GenerateRequest,
            _cancel: CancelHandle,
        ) -> Box<dyn Iterator<Item = StreamEvent> + Send> {
            let mut events = vec![
                StreamEvent::Token("The plan has ".to_string()),
                StreamEvent::Token("three ".to_string()),
            ];
            events.extend(self.ending.clone());
            Box::new(events.into_iter())
        }

        fn embed(&self, _model: &str, _input: &str) -> Result<Vec<f32>, InferenceError> {
            Err(InferenceError::Unsupported)
        }

        fn describe(&self, model: &str) -> Result<ModelDescriptor, InferenceError> {
            Ok(ModelDescriptor {
                name: model.to_string(),
                ..Default::default()
            })
        }

        fn pull(&self, _model: &str) -> Box<dyn Iterator<Item = PullProgress> + Send> {
            Box::new(std::iter::once(PullProgress::Done { digest: None }))
        }

        fn set_residency(&self, _model: &str, _hint: ResidencyHint) -> Result<(), InferenceError> {
            Ok(())
        }
    }

    fn ending_with(ending: Option<StreamEvent>) -> NodusModelBridge {
        NodusModelBridge::new(Arc::new(EndingBackend { ending }), "test-model")
    }

    #[test]
    fn an_error_event_fails_the_call_and_no_fragment_is_returned() {
        let b = ending_with(Some(StreamEvent::Error(InferenceError::ConnectRefused)));
        let failure = b.try_generate("plan", &[]).expect_err("the call failed");
        assert!(
            failure.reason.contains("refused"),
            "the reason names the failure: {}",
            failure.reason
        );
        assert!(
            !failure.reason.contains("three"),
            "the fragment that arrived before the error is not part of the record"
        );
        // The infallible surface cannot report the failure; it must not hand
        // back the fragment as if it were the answer.
        assert_eq!(b.generate("plan", &[]), "");
    }

    #[test]
    fn a_cancelled_call_fails_rather_than_returning_what_arrived() {
        let b = ending_with(Some(StreamEvent::Error(InferenceError::Cancelled)));
        assert!(b.try_generate("plan", &[]).is_err());
    }

    #[test]
    fn a_stream_that_ends_without_completing_fails_the_call() {
        let b = ending_with(None);
        let failure = b.try_generate("plan", &[]).expect_err("truncated stream");
        assert!(failure.reason.contains("without completing"));
    }

    #[test]
    fn a_malformed_stream_error_does_not_leak_its_payload_into_the_reason() {
        let b = ending_with(Some(StreamEvent::Error(InferenceError::MalformedStream(
            "secret-bearing raw response fragment".to_string(),
        ))));
        let failure = b.try_generate("plan", &[]).expect_err("malformed");
        assert!(!failure.reason.contains("secret"), "{}", failure.reason);
    }

    #[test]
    fn a_failed_analysis_reports_the_failure_and_answers_no_flag() {
        let b = ending_with(Some(StreamEvent::Error(InferenceError::Timeout)));
        let flags = vec!["intent".to_string()];
        assert!(b.try_analyze("text", &flags).is_err());
        assert_eq!(
            b.analyze("text", &flags),
            Value::Map(vec![("intent".to_string(), Value::Null)]),
            "the infallible surface leaves every flag unanswered, never invents one"
        );
    }

    #[test]
    fn a_failed_call_inside_a_workflow_is_a_typed_step_error_that_reaches_err() {
        let source = r#"
§wf:plan v1.0
§runtime: { core: schema.nodus }
@out: $out
@err: ESCALATE(human)
@steps:
  1. GEN(plan) → $out
  2. LOG($out)
"#;
        let b = ending_with(Some(StreamEvent::Error(InferenceError::ServerError(503))));
        let result = nodus::run_with_provider(source, "plan.nodus", None, b).expect("runs");

        assert_eq!(result.status, nodus::Status::Partial);
        let error = result.errors.first().expect("the failure is recorded");
        assert_eq!(error.code, nodus::vocab::error_code::MODEL_CALL_FAILED);
        assert!(error.reason.contains("503"), "{}", error.reason);
        assert!(
            result.flags.iter().any(|f| f.starts_with("ESCALATE:")),
            "the workflow's @err handler ran: {:?}",
            result.flags
        );
        assert_ne!(
            result.vars.get("out"),
            Some(&Value::Text("The plan has three ".to_string())),
            "the truncated plan must never be bound as the answer"
        );
    }

    #[test]
    fn nodus_bridge_generate_concatenates_the_stream() {
        let b = bridge(vec!["Hello", ", ", "world"]);
        assert_eq!(b.generate("greet", &[]), "Hello, world");
        assert_eq!(b.model_id(), "test-model");
    }

    #[test]
    fn nodus_bridge_analyze_returns_a_real_flag_map_from_the_model() {
        // The backend "returns" a JSON verdict; the bridge projects the
        // requested flags — a real map, not the stub's constant 0.9.
        let b = bridge(vec![r#"{"intent": "buy", "urgency": 0.8}"#]);
        let flags = vec!["intent".to_string(), "urgency".to_string()];
        let result = b.analyze("I want this now", &flags);
        assert_eq!(
            result,
            Value::Map(vec![
                ("intent".to_string(), Value::Text("buy".to_string())),
                ("urgency".to_string(), Value::Float(0.8)),
            ])
        );
    }

    #[test]
    fn nodus_bridge_analyze_leaves_unanswered_flags_null_never_fabricated() {
        // The model answered only "intent"; "toxicity" was not in the
        // response, so it stays Null rather than getting a made-up score.
        let b = bridge(vec![r#"here you go: {"intent": "browse"} — done"#]);
        let flags = vec!["intent".to_string(), "toxicity".to_string()];
        let result = b.analyze("just looking", &flags);
        assert_eq!(
            result,
            Value::Map(vec![
                ("intent".to_string(), Value::Text("browse".to_string())),
                ("toxicity".to_string(), Value::Null),
            ])
        );
    }

    #[test]
    fn nodus_bridge_drives_a_real_nodus_gen_step() {
        // End-to-end: a nodus workflow with a GEN step, run with the bridge
        // as its ModelProvider, writes the concatenated stream into `$out` —
        // proving the bridge replaces the built-in stub in the real runtime.
        let source = r#"
§wf:greet v1.0
§runtime: { core: schema.nodus }
@in:  { name: text }
@out: $out
@err: ESCALATE(human)
@steps:
  1. GEN($in.name) → $out
"#;
        let input = Value::Map(vec![("name".to_string(), Value::Text("Ada".to_string()))]);
        let result = nodus::run_with_provider(
            source,
            "greet.nodus",
            Some(input),
            bridge(vec!["Hi ", "there"]),
        )
        .expect("workflow runs");

        assert_eq!(result.status, nodus::Status::Ok);
        assert_eq!(
            result.vars.get("out"),
            Some(&Value::Text("Hi there".to_string()))
        );
        // And crucially NOT the stub label.
        assert_ne!(
            result.vars.get("out"),
            Some(&Value::Text("[STUB gen(Ada) tone=brand]".to_string()))
        );
    }
}
