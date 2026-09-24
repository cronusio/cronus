# Voice Input

**Version:** 1.1.1
**Status:** Stable
**Layer:** concept

## Overview

The voice input subsystem lets users dictate ideas, prompts, and commands by speaking. Spoken audio is captured, processed through an on-device voice-activity detection and transcription pipeline, and injected into the active input field after user review. No audio or transcript leaves the device.

Voice input supplements text input; it does not replace it. The user explicitly controls when recording starts and stops.

## Related Specifications

- [l2-technology-stack.md](l2-technology-stack.md) — audio processing stack (VAD, ONNX, cpal, 16 kHz pipeline) that this spec builds on
- [l1-security.md](l1-security.md) — data residency guarantee: audio stays on-device; egress gate for any opt-in remote transform (VI-6)
- [l1-navigation-model.md](l1-navigation-model.md) — Chat tab (primary surface for voice input)
- [l1-model-runtime.md](l1-model-runtime.md) — on-device model lifecycle (acquire/load/idle-unload) reused for speech models (VI-8)
- [l1-file-management.md](l1-file-management.md) — content-addressed, deduplicated model-blob storage for speech models (VI-8)
- [l1-voice-output.md](l1-voice-output.md) — [ADDED v1.1.1] the output sibling that completes the voice loop (text → speech); mirrors VI-1/VI-3/VI-7/VI-8 and adds the voice-identity consent/disclosure contract input does not face.
- [l2-model-router.md](l2-model-router.md) — routing + egress gate for an opt-in language-model post-transcription transform (VI-6)
- [l2-app-ui.md](l2-app-ui.md) — global shortcut binding system; per-binding action mapping (plain vs. post-processed)

## 1. Motivation

Dictating ideas is faster than typing for many users and enables capture of fleeting thoughts without requiring a keyboard. Voice input reduces friction when the user wants to brainstorm with the office orchestrator during activities where typing is inconvenient (commute, whiteboard session, hands-occupied work).

## 2. Constraints & Assumptions

- Transcription runs entirely on-device; no audio or transcript is transmitted to any external service.
- The audio pipeline uses the existing 16 kHz mono capture infrastructure from the technology stack.
- Transcription accuracy depends on the on-device model; the transcript is presented for user review before injection.
- Mobile platforms (iOS, Android) require explicit microphone permission grants per platform policy.
- Voice input does not interpret commands directly — the transcript is plain text injected into the input field; the user (or the agent receiving it) handles intent.

## 3. Core Invariants

- **VI-1 On-device only**: audio data MUST NOT leave the device. Transcription uses a locally stored speech model. No network connection is required for voice input.
- **VI-2 User review before injection**: the transcript is shown to the user for confirmation or editing before being injected into the active input field. Silent auto-injection without review is not permitted.
- **VI-3 Explicit activation**: recording begins only on explicit user gesture (push-to-talk or toggle). The application MUST NOT record continuously or passively without explicit per-session consent.
- **VI-4 Active recording indicator**: a prominent visual indicator (and haptic on mobile) is shown for the full duration of recording. The indicator MUST be visible regardless of which tab is active.
- **VI-5 Cancellation without side effects**: the user may cancel a recording at any point before injection. Cancelled recordings discard all audio and transcript atomically — nothing is written to any log or store.
- **VI-6 Optional post-transcription transform**: a transcript MAY pass through an optional, user-controlled transform stage before review. The stage covers *deterministic shaping* (custom-vocabulary substitution, output filtering, locale/script normalization) and, when explicitly enabled, *language-model assistance* driven by a user prompt template with structured output. The stage MUST default to off and MUST NOT weaken VI-1: deterministic shaping and any local model run on-device; a remote post-processing model is an explicit, consent-gated opt-in routed through the model router's egress gate, never a default. User review (VI-2) applies to the transformed text.
- **VI-7 Pluggable transcription engine**: transcription runs through an engine abstraction, not a single hard-wired model. The subsystem MUST support interchangeable engines (including platform-native transcription where available), use hardware acceleration when present with graceful CPU fallback, and MAY offer streaming and automatic language detection. Engine choice is a user setting; switching engines MUST NOT change the on-device guarantee (VI-1).
- **VI-8 On-device speech-model lifecycle**: speech models follow an explicit acquire → store → load → idle-unload lifecycle on-device. Models are bundled or acquired on demand from a named catalog with integrity verification, stored once (content-addressed, deduplicated), loaded on demand, and unloaded after idle. This reuses the model-runtime and file-management patterns rather than defining a parallel mechanism.
- **VI-9 Local transcription history (optional)**: the subsystem MAY keep an on-device history of *confirmed* transcriptions that the user can view, re-inject, search, and delete. History honors the VI-1 residency boundary (never egressed) and the VI-5 boundary (cancelled or unconfirmed recordings are NEVER recorded). History is fully user-clearable.
- **VI-10 Non-destructive injection**: injecting text MUST NOT corrupt the user's environment. A clipboard-based paste saves and restores any prior clipboard contents, or a synthetic-input path is used; injection targets the currently focused field and never overwrites unrelated state. Injection failures surface to the user rather than silently dropping the transcript.

## 4. Detailed Design

### 4.1 Pipeline

```text
[REFERENCE]
ACTIVATE   → user gesture (hold or tap microphone button)
CAPTURE    → cpal audio stream at 16 kHz mono
VAD        → ONNX voice-activity detection (smoothed); segments speech / silence
TRANSCRIBE → on-device speech-to-text engine converts segments to text
TRANSFORM? → optional, off by default: deterministic shaping and/or opt-in
             language-model assistance over the transcript (VI-6)
REVIEW     → overlay displays (transformed) transcript; user confirms, edits, cancels
INJECT     → confirmed text inserted at cursor in active field, clipboard-safe (VI-10)
HISTORY?   → on confirm only: append to optional on-device history (VI-9)
```

The `TRANSFORM?` and `HISTORY?` stages are optional and user-controlled. The
`CANCEL` path (VI-5) at any point before `INJECT` discards everything and writes
nothing — including to history.

### 4.2 Activation Modes

| Mode | Gesture | Suited for |
| --- | --- | --- |
| Push-to-talk | Hold button; release to transcribe | Short phrases, single sentences |
| Toggle | Tap to start; tap again to stop and transcribe | Longer dictation, multi-sentence |

The active mode is a user preference in Global Settings → Appearance.

### 4.3 Review Overlay

After recording stops, an overlay presents:

- Editable transcript text (with word-level confidence indicators where the model provides them)
- **Confirm** action — inject the (optionally edited) text at the cursor
- **Cancel** action — discard everything (VI-5)

The user may edit the transcript freely before confirming; editing does not re-run transcription.

### 4.4 On-Device Model

The speech model is a quantized model stored on-device (per `l2-filesystem-layout.md`).
Model selection and update lifecycle (VI-8): a default model is bundled with the
application so voice input works on first run with no network; additional models are
acquired on demand from a named catalog with integrity verification, stored once
(content-addressed and deduplicated per `l1-file-management.md`), loaded on demand,
and unloaded after a configurable idle timeout. This reuses the model-runtime
acquire/load/evict lifecycle rather than defining a parallel one.

### 4.5 Transcription Engines (VI-7)

Transcription is performed through an engine abstraction so models are interchangeable:

| Engine family | Strength | Notes |
| --- | --- | --- |
| General-accuracy | Highest quality, accelerated | Uses GPU/accelerator when available, CPU fallback otherwise |
| CPU-optimized | Strong quality without a GPU | Often pairs with automatic language detection |
| Streaming | Low-latency incremental output | Optional; for live partial transcripts |
| Platform-native | Zero-download, OS-provided | Used where the platform exposes on-device transcription |

Accelerator selection is a user setting with a safe default (accelerate-if-present,
else CPU). The active engine and its idle-unload behavior are visible in settings.
Switching engines never changes the on-device guarantee (VI-1).

### 4.6 Post-Transcription Transform (VI-6)

Two transform kinds, both off by default and independently toggleable:

- **Deterministic shaping** — custom-vocabulary substitution (force preferred
  spellings of names/terms), output filtering (strip filler/noise tokens), and
  locale/script normalization. Always on-device, no model required.
- **Language-model assistance** — an opt-in pass that sends the transcript to a
  language model under a user prompt template (with an `${output}` placeholder) and
  requests structured output, then strips invisible/zero-width characters the model
  might introduce. A *local* model keeps everything on-device; a *remote* model is an
  explicit consent-gated choice routed through the model router and the egress gate
  (`l1-security.md`), never the default.

A shortcut binding MAY map to either plain transcription or transcription-with-
transform, so the user picks per gesture (e.g. one key dictates verbatim, another
dictates-then-cleans-up). The transformed text still goes through review (VI-2).

### 4.7 Injection Mechanism (VI-10)

Injection inserts the confirmed text into the currently focused field without
corrupting environment state. Two paths:

- **Clipboard-safe paste** — save the user's current clipboard, set the transcript,
  issue paste, then restore the prior clipboard contents.
- **Synthetic input** — emit the text as synthetic keystrokes where clipboard paste
  is unavailable or undesirable.

Injection failures (no focused field, permission denied) surface to the user; the
transcript is never silently dropped.

### 4.8 Transcription History (VI-9)

An optional on-device store of confirmed transcriptions. The user can browse,
search, re-inject a past entry, and delete individual entries or clear all. Only
confirmed transcriptions are stored; cancelled or unconfirmed recordings are never
written (VI-5). History never leaves the device (VI-1).

## 5. Implementation Notes

1. The VAD component from the technology stack handles silence detection and auto-stops recording after a configurable silence gap (default: 1.5 s).
2. On mobile, request microphone permission on first use via the platform's standard permission API; never at application launch.
3. The review overlay is a system-level layer (not tab-specific), so it remains visible if the user switches tabs while reviewing.

## 6. Drawbacks & Alternatives

**Alternative: cloud transcription** — higher accuracy via an external STT API. Rejected: violates VI-1 (on-device only) and the client security principle.

**Alternative: direct injection without review** — inject transcript immediately without confirmation. Rejected: violates VI-2; transcription errors would silently corrupt prompts.

**Remote language-model post-processing as default** — higher cleanup quality by sending every transcript to a cloud model. Rejected as a default: it would breach VI-1 for the transform path. Kept only as an explicit, consent-gated opt-in routed through the egress gate (VI-6); the on-device path (local model or deterministic shaping) is always available.

**Bundle-only speech model (no catalog)** — simplest, but locks users to one model and one language profile. Rejected: VI-8 keeps a bundled default for offline first-run while allowing on-demand acquisition of better or language-specific engines.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[AUDIO-STACK]` | `.design/main/specifications/l2-technology-stack.md` | 16 kHz pipeline, VAD/ONNX, cpal — audio capture foundation |
| `[FS-LAYOUT]` | `.design/main/specifications/l2-filesystem-layout.md` | Speech model storage location |
| `[SECURITY]` | `.design/main/specifications/l1-security.md` | On-device residency requirement (VI-1); egress gate for opt-in remote transform (VI-6) |
| `[MODEL-RT]` | `.design/main/specifications/l1-model-runtime.md` | Acquire/load/idle-unload lifecycle reused for speech models (VI-8) |
| `[FILE-MGMT]` | `.design/main/specifications/l1-file-management.md` | Content-addressed, deduplicated model-blob storage (VI-8) |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.0 | 2026-06-24 | Core Team | Initial spec — VI-1…VI-5, pipeline stages, activation modes, review overlay |
| 1.1.0 | 2026-06-25 | Core Team | VI-6…VI-10 added — optional post-transcription transform (deterministic + opt-in consent-gated LM), pluggable transcription engine (accel-when-available, streaming, auto-language, platform-native), on-device speech-model lifecycle (resolves §4.4 TBD: bundled default + on-demand catalog), optional local transcription history, non-destructive clipboard-safe injection. Pipeline extended with optional TRANSFORM/HISTORY stages; §4.5–4.8 added. |
| 1.1.1 | 2026-07-23 | Core Team | Related Specifications gains `l1-voice-output` — the output sibling that completes the voice loop. No invariant changed. (Row reconstructed from version control in the 2026-09-24 consistency pass.) |
