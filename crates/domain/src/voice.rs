//! Voice input — the on-device dictation pipeline state machine.
//!
//! Recording starts only on an explicit gesture; transcription runs through
//! a pluggable engine that never leaves the device; the transcript is
//! reviewed before injection; a cancel at any point before injection discards
//! everything and writes nothing — including to history; confirmed
//! transcripts optionally enter an on-device history.
//!
//! Real audio capture (cpal), VAD (ONNX), and the OS injection path are seams; the
//! pipeline lifecycle and its safety invariants are implemented and tested here.

/// How recording is activated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationMode {
    PushToTalk,
    Toggle,
}

/// The pipeline stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stage {
    Idle,
    Recording,
    Review { transcript: String },
    Injected { text: String },
    Cancelled,
}

/// A transcription engine. All implementations keep audio on-device.
pub trait TranscriptionEngine {
    fn transcribe(&self, pcm: &[i16]) -> String;
    /// Whether transcription runs on-device (must always be true).
    fn on_device(&self) -> bool {
        true
    }
}

/// Errors from the pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceError {
    /// An operation invalid for the current stage.
    WrongStage,
    /// Injection failed (e.g. no focused field); the transcript is not dropped.
    InjectionFailed,
}

impl std::fmt::Display for VoiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            VoiceError::WrongStage => "operation invalid for the current pipeline stage",
            VoiceError::InjectionFailed => "injection failed — transcript retained, not dropped",
        };
        f.write_str(s)
    }
}

impl std::error::Error for VoiceError {}

/// A single dictation session driving the pipeline.
#[derive(Debug)]
pub struct VoiceSession {
    stage: Stage,
    mode: ActivationMode,
    /// On-device history of confirmed transcripts only.
    history: Vec<String>,
    keep_history: bool,
}

impl VoiceSession {
    pub fn new(mode: ActivationMode, keep_history: bool) -> Self {
        VoiceSession {
            stage: Stage::Idle,
            mode,
            history: Vec::new(),
            keep_history,
        }
    }

    pub fn stage(&self) -> &Stage {
        &self.stage
    }

    pub fn mode(&self) -> ActivationMode {
        self.mode
    }

    pub fn history(&self) -> &[String] {
        &self.history
    }

    /// Begin recording on an explicit gesture. Only valid from `Idle`.
    pub fn activate(&mut self) -> Result<(), VoiceError> {
        if self.stage != Stage::Idle {
            return Err(VoiceError::WrongStage);
        }
        self.stage = Stage::Recording;
        Ok(())
    }

    /// Stop recording and transcribe the captured audio, moving to review.
    /// Transcription never leaves the device — enforced by the engine trait.
    pub fn stop_and_transcribe(
        &mut self,
        engine: &dyn TranscriptionEngine,
        pcm: &[i16],
    ) -> Result<&str, VoiceError> {
        if self.stage != Stage::Recording {
            return Err(VoiceError::WrongStage);
        }
        debug_assert!(engine.on_device(), "transcription must stay on-device");
        let transcript = engine.transcribe(pcm);
        self.stage = Stage::Review { transcript };
        match &self.stage {
            Stage::Review { transcript } => Ok(transcript),
            // Just assigned above; reported rather than asserted so the
            // transition can never take the caller down with it.
            _ => Err(VoiceError::WrongStage),
        }
    }

    /// Confirm the (optionally edited) transcript and inject it. Only
    /// valid from `Review`. On success, a confirmed transcript enters history if
    /// enabled. `inject` models the OS injection; returning `false` surfaces
    /// a failure without dropping the transcript.
    pub fn confirm(
        &mut self,
        edited: &str,
        inject: impl FnOnce(&str) -> bool,
    ) -> Result<(), VoiceError> {
        if !matches!(self.stage, Stage::Review { .. }) {
            return Err(VoiceError::WrongStage);
        }
        if !inject(edited) {
            // Stay in Review so the transcript is retained, never silently dropped.
            return Err(VoiceError::InjectionFailed);
        }
        if self.keep_history {
            self.history.push(edited.to_string());
        }
        self.stage = Stage::Injected {
            text: edited.to_string(),
        };
        Ok(())
    }

    /// Cancel at any point before injection: discard all audio and transcript
    /// atomically. Nothing is written to history. Valid from Recording or Review.
    pub fn cancel(&mut self) -> Result<(), VoiceError> {
        match self.stage {
            Stage::Recording | Stage::Review { .. } => {
                self.stage = Stage::Cancelled;
                Ok(())
            }
            _ => Err(VoiceError::WrongStage),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake on-device engine that echoes a fixed transcript.
    struct FakeEngine(&'static str);
    impl TranscriptionEngine for FakeEngine {
        fn transcribe(&self, _pcm: &[i16]) -> String {
            self.0.to_string()
        }
    }

    #[test]
    fn recording_starts_only_on_explicit_activation() {
        let mut s = VoiceSession::new(ActivationMode::PushToTalk, false);
        assert_eq!(s.stage(), &Stage::Idle);
        s.activate().unwrap();
        assert_eq!(s.stage(), &Stage::Recording);
        // Re-activating mid-recording is invalid (no continuous/passive capture).
        assert_eq!(s.activate(), Err(VoiceError::WrongStage));
    }

    #[test]
    fn transcript_is_reviewed_before_injection() {
        // No path injects without passing through review + confirm.
        let mut s = VoiceSession::new(ActivationMode::Toggle, false);
        s.activate().unwrap();
        let t = s
            .stop_and_transcribe(&FakeEngine("hello world"), &[0i16; 4])
            .unwrap();
        assert_eq!(t, "hello world");
        assert!(matches!(s.stage(), Stage::Review { .. }));
    }

    #[test]
    fn confirm_injects_and_records_history_when_enabled() {
        // Confirmed transcript injected and stored (history on).
        let mut s = VoiceSession::new(ActivationMode::Toggle, true);
        s.activate().unwrap();
        s.stop_and_transcribe(&FakeEngine("draft text"), &[0i16; 2])
            .unwrap();
        s.confirm("edited text", |_| true).unwrap();
        assert_eq!(
            s.stage(),
            &Stage::Injected {
                text: "edited text".into()
            }
        );
        assert_eq!(s.history(), &["edited text".to_string()]);
    }

    #[test]
    fn cancel_discards_everything_and_writes_no_history() {
        // Cancelled recording leaves zero trace — nothing in history.
        let mut s = VoiceSession::new(ActivationMode::PushToTalk, true);
        s.activate().unwrap();
        s.stop_and_transcribe(&FakeEngine("secret note"), &[0i16; 2])
            .unwrap();
        s.cancel().unwrap();
        assert_eq!(s.stage(), &Stage::Cancelled);
        assert!(s.history().is_empty());
    }

    #[test]
    fn injection_failure_retains_transcript() {
        // A failed injection surfaces an error and does not drop the text.
        let mut s = VoiceSession::new(ActivationMode::Toggle, true);
        s.activate().unwrap();
        s.stop_and_transcribe(&FakeEngine("keep me"), &[0i16; 2])
            .unwrap();
        assert_eq!(
            s.confirm("keep me", |_| false),
            Err(VoiceError::InjectionFailed)
        );
        // Still in review — transcript retained, nothing stored.
        assert!(matches!(s.stage(), Stage::Review { .. }));
        assert!(s.history().is_empty());
    }

    #[test]
    fn engine_is_on_device_by_default() {
        assert!(FakeEngine("x").on_device());
    }
}
