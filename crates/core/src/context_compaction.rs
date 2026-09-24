//! Transport-backed context compaction.
//!
//! `cronus-domain` ships `NoOpCompactor` — the inert default that returns the
//! fixed `"[context compacted]"` placeholder because no model was reachable.
//! `TransportCompactor` is the facade wiring that makes context compaction a
//! real model-consuming path: it drives the `contract::Compactor` seam over a
//! `contract::InferenceBackend` (the model-local transport), summarizing the
//! trimmed context through a real generation call.
//!
//! Degrade-without-panic: a transport failure (unreachable backend, error
//! event, empty output) is returned as `Err(..)` from `compact`, never a
//! panic — the caller falls back to the trim cascade or the no-op default.

use std::sync::Arc;

use cronus_contract::{
    CancelHandle, Compactor, ContextEntry, GenerateRequest, InferenceBackend, StreamEvent,
};

/// A `Compactor` that summarizes context by calling a model over the
/// transport, replacing `NoOpCompactor`'s placeholder with a real summary.
pub struct TransportCompactor {
    backend: Arc<dyn InferenceBackend>,
    model: String,
}

impl TransportCompactor {
    /// Wire a compactor over any inference backend (an `EndpointProfile`, or a
    /// test double). The model name is the one each compaction call targets.
    pub fn new(backend: impl InferenceBackend + 'static, model: impl Into<String>) -> Self {
        TransportCompactor {
            backend: Arc::new(backend),
            model: model.into(),
        }
    }
}

impl Compactor for TransportCompactor {
    fn compact(&self, context: &[ContextEntry], keep_recent_tokens: u64) -> Result<String, String> {
        let request = GenerateRequest {
            model: self.model.clone(),
            prompt: build_compaction_prompt(context, keep_recent_tokens),
            parameters: Vec::new(),
        };
        let mut summary = String::new();
        for event in self.backend.generate_stream(request, CancelHandle::new()) {
            match event {
                StreamEvent::Token(t) => summary.push_str(&t),
                StreamEvent::Done => break,
                // Degrade gracefully: surface the failure so the caller can
                // fall back to the trim cascade / no-op — never panic.
                StreamEvent::Error(e) => return Err(format!("compaction transport failed: {e:?}")),
                StreamEvent::ToolCall { .. } | StreamEvent::Usage { .. } => {}
            }
        }
        if summary.is_empty() {
            return Err("compaction produced no summary".to_string());
        }
        Ok(summary)
    }
}

fn build_compaction_prompt(context: &[ContextEntry], keep_recent_tokens: u64) -> String {
    let mut prompt = String::from(
        "Summarize the conversation so far into a concise summary that preserves decisions, \
         facts, and open threads. Output only the summary.\n",
    );
    prompt.push_str(&format!(
        "Keep roughly the most recent {keep_recent_tokens} tokens' worth of detail.\n\n",
    ));
    for entry in context {
        prompt.push_str(&entry.role);
        prompt.push_str(": ");
        prompt.push_str(&entry.body);
        prompt.push('\n');
    }
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;
    use cronus_contract::{InferenceError, ModelDescriptor, PullProgress, ResidencyHint};

    /// A backend that yields a scripted summary — or an error, to exercise the
    /// degrade path — for every generation.
    struct ScriptedBackend {
        events: Vec<StreamEvent>,
    }

    impl InferenceBackend for ScriptedBackend {
        fn generate_stream(
            &self,
            _request: GenerateRequest,
            _cancel: CancelHandle,
        ) -> Box<dyn Iterator<Item = StreamEvent> + Send> {
            Box::new(self.events.clone().into_iter())
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

    fn entries() -> Vec<ContextEntry> {
        vec![
            ContextEntry::new("user", "let's build the transport", 8),
            ContextEntry::new("assistant", "done — streaming works", 6),
        ]
    }

    #[test]
    fn compact_returns_the_real_model_summary_not_a_placeholder() {
        let backend = ScriptedBackend {
            events: vec![
                StreamEvent::Token("Summary: ".to_string()),
                StreamEvent::Token("built the transport".to_string()),
                StreamEvent::Done,
            ],
        };
        let compactor = TransportCompactor::new(backend, "test-model");
        let out = compactor.compact(&entries(), 1000).expect("compaction");
        assert_eq!(out, "Summary: built the transport");
        assert_ne!(
            out, "[context compacted]",
            "must not be the no-op placeholder"
        );
    }

    #[test]
    fn compact_degrades_to_err_on_transport_error_never_panics() {
        let backend = ScriptedBackend {
            events: vec![StreamEvent::Error(InferenceError::ConnectRefused)],
        };
        let compactor = TransportCompactor::new(backend, "test-model");
        assert!(compactor.compact(&entries(), 1000).is_err());
    }

    #[test]
    fn compact_treats_empty_output_as_err() {
        let backend = ScriptedBackend {
            events: vec![StreamEvent::Done],
        };
        let compactor = TransportCompactor::new(backend, "test-model");
        assert!(compactor.compact(&entries(), 1000).is_err());
    }
}
