# Voice Input

**Version:** 1.0.1
**Status:** Stable
**Layer:** implementation
**Implements:** l1-voice-input.md

## Overview

The concrete voice-input pipeline: on-device audio capture (`cpal`, 16 kHz mono), ONNX voice-activity detection, a pluggable transcription engine abstraction, the optional post-transcription transform, the system-level review overlay, clipboard-safe injection, and the optional on-device history. Capture, VAD, transcription, and model lifecycle live in `crates/core` (Rust); the review overlay and activation gestures are the desktop shell (React/Tauri). No audio or transcript leaves the device unless the user explicitly opts into a remote transform routed through the egress gate.

## Related Specifications

- [l1-voice-input.md](l1-voice-input.md) — the model this implements (VI-1…VI-10).
- [l2-technology-stack.md](l2-technology-stack.md) — cpal/ONNX/VAD audio stack this builds on.
- [l2-security.md](l2-security.md) — on-device residency enforcement + egress gate for the opt-in remote transform (VI-6).
- [l2-model-router.md](l2-model-router.md) — routes the opt-in language-model transform through the egress gate.
- [l2-app-ui.md](l2-app-ui.md) — global shortcut bindings and the review overlay host.
- [l2-filesystem-layout.md](l2-filesystem-layout.md) — speech-model storage location (VI-8).

## 1. Motivation

The model requires an on-device-only pipeline with explicit control and review. Keeping capture/VAD/transcription in the Rust core reuses the technology-stack audio infrastructure and the model-runtime lifecycle; the overlay stays presentation-only. The engine abstraction keeps VI-1 invariant across interchangeable models.

## 2. Constraints & Assumptions

- Transcription runs on-device; the remote transform is the only egress path and is consent-gated (VI-6).
- Audio capture reuses the 16 kHz mono cpal stream; VAD auto-stops after a configurable silence gap (default 1.5 s).
- The review overlay is a system-level layer, visible regardless of active tab.
- Cancelled/unconfirmed recordings write nothing anywhere (VI-5).

## 3. Invariant Compliance (Layer 2)

| L1 Invariant | Implementation |
| --- | --- |
| VI-1 On-device only | Capture→VAD→transcribe run in-process against a locally stored model; no network path except the explicit VI-6 remote transform. |
| VI-2 User review before injection | The pipeline halts at the `REVIEW` overlay; injection fires only on the user's Confirm. No auto-injection path exists. |
| VI-3 Explicit activation | Recording starts only on a push-to-talk hold or toggle tap bound to a shortcut; there is no continuous-listen code path. |
| VI-4 Active recording indicator | A system-level recording indicator renders for the full capture duration (+ haptic on mobile), independent of the active tab. |
| VI-5 Cancellation w/o side effects | `cancel()` drops the audio buffer + transcript atomically before `INJECT`; nothing is written to history or any log. |
| VI-6 Optional transform | A `TransformStage` (off by default): deterministic shaping (vocab/filter/normalize, on-device) + opt-in LM assist; a remote model is consent-gated through model-router's egress gate. Transformed text still hits review. |
| VI-7 Pluggable engine | `TranscriptionEngine` trait with general-accuracy/CPU/streaming/platform-native impls; accelerator-if-present with CPU fallback; engine is a user setting; swapping never adds a network path. |
| VI-8 Model lifecycle | Speech models follow acquire→store→load→idle-unload via the model-runtime lifecycle; content-addressed dedup storage (file-management); integrity-verified catalog acquisition; a bundled default works offline on first run. |
| VI-9 Local history (optional) | An opt-in on-device store of confirmed transcripts only (browse/search/re-inject/delete/clear); never egressed; cancelled recordings never recorded. |
| VI-10 Non-destructive injection | Clipboard-safe paste (save→set→paste→restore) or synthetic-input path; targets the focused field; failures surface to the user, never silently drop the transcript. |

## 4. Detailed Design

### 4.1 Pipeline module

```text
[REFERENCE]
voice::pipeline (crates/core):
  activate(mode)        -> RecordingHandle          // push-to-talk | toggle (VI-3)
  capture               -> cpal 16 kHz mono stream
  vad(onnx)             -> speech/silence segments; auto-stop on silence gap
  transcribe(engine)    -> text                     // TranscriptionEngine trait (VI-7)
  transform?(stage)     -> text                     // off by default (VI-6)
  → emit ReviewReady{ text } to the shell overlay   // pipeline halts here (VI-2)
  on confirm(edited)    -> inject(edited)            // clipboard-safe (VI-10); history? (VI-9)
  on cancel             -> drop all; write nothing   // (VI-5)
```

### 4.2 Engine abstraction (VI-7)

`trait TranscriptionEngine { fn transcribe(&self, pcm: &[f32]) -> Result<Transcript>; fn supports_streaming(&self) -> bool; }`. Registry resolves the user-selected engine; accelerator detection picks GPU-if-present else CPU. Platform-native engines wrap OS transcription where available.

### 4.3 Model lifecycle (VI-8)

Delegates to the model-runtime acquire/load/evict path; speech-model blobs stored content-addressed (file-management dedup). A bundled quantized default ships with the app for offline first-run; better/language-specific models are acquired from an integrity-verified catalog when the user asks for one — an explicit download the user starts, never a fetch triggered silently by a recording (`l1-model-runtime` MR-4) — then loaded on demand and unloaded after idle.

### 4.4 Transform & injection

The transform stage is two independently-toggled kinds (deterministic shaping, LM assist); a shortcut may bind to plain vs transform-then-clean. Injection uses clipboard-safe paste by default, synthetic input where paste is unavailable; both restore prior environment state. The paste path puts the transcript on the system clipboard for an instant, where clipboard managers and other applications can read it; where the platform offers synthetic input into the focused field, that path is preferred for a transcript the user marks sensitive, and the window is otherwise kept as short as the platform allows.

## 5. Implementation Notes

1. VAD auto-stop threshold is a setting (default 1.5 s); the review overlay is a system layer so tab switches don't dismiss it.
2. Mobile requests mic permission on first use via the platform API, never at launch.
3. The remote transform is the only egress; it goes through model-router + egress gate and is off by default.

## 6. Drawbacks & Alternatives

**Alternative — cloud transcription**: violates VI-1. Rejected; on-device only, remote is an explicit transform opt-in.

**Alternative — inject without review**: violates VI-2; transcription errors would corrupt prompts silently. Rejected.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[MODEL]` | `.design/main/specifications/l1-voice-input.md` | Invariants VI-1…VI-10 |
| `[STACK]` | `.design/main/specifications/l2-technology-stack.md` | cpal/ONNX/VAD audio foundation |
| `[SECURITY]` | `.design/main/specifications/l2-security.md` | Residency + egress gate (VI-6) |
| `[ROUTER]` | `.design/main/specifications/l2-model-router.md` | Opt-in remote transform routing |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.1 | 2026-09-24 | Core Team | Consistency pass (2026-09-24): Better speech models were "acquired on demand" — acquisition is an explicit download the user starts (MR-4). The clipboard-paste injection exposed the transcript to clipboard readers without saying so — stated, with synthetic input preferred for a transcript marked sensitive. |
| 1.0.0 | 2026-07-03 | Core Team | Initial implementation spec — cpal/VAD/transcription pipeline, engine abstraction, model lifecycle, optional transform, review overlay, clipboard-safe injection, optional history; maps VI-1…VI-10. |
