# R711: Voice Interaction Support

## Question

How should aletheia support voice interaction, allowing users to speak to agents and receive spoken responses? This covers speech-to-text (STT), text-to-speech (TTS), real-time voice streaming, and integration with the existing agora channel architecture.

Specific sub-questions:
1. How does the current agora channel architecture accommodate a voice channel?
2. Which STT approach best fits the "pure Rust, no C++ in the brain" principle?
3. Which TTS approach balances quality, latency, and local-first operation?
4. What audio pipeline (capture, format, VAD, buffering) is needed?
5. What phased implementation path minimizes risk while delivering value early?

---

## Findings

### 1. Current channel architecture

Agora provides a provider-based channel abstraction with four components:

- **`ChannelProvider` trait**: object-safe async trait (`send`, `probe`, `capabilities`) stored as `Arc<dyn ChannelProvider>` in the registry
- **`ChannelRegistry`**: provider lookup, outbound routing, concurrent health probes
- **`MessageRouter`**: 4-level priority cascade (exact group, exact source, channel default, global default) with session key templating
- **`ChannelListener`**: unified inbound polling via mpsc channels, spawns per-provider tasks

Only Signal (`semeion`) is implemented. The architecture is explicitly designed for extension: implement the trait, register the provider, add config bindings.

**Voice integration fit:** The `ChannelProvider` contract is text-centric. `InboundMessage.text` carries the message body; `SendParams.text` carries the outbound reply. Voice fits naturally as a "transcription-first" channel: STT produces the `text` field on inbound, TTS converts the `text` field on outbound. The voice channel wraps an audio transport around the existing text pipeline.

**What needs to change:**

| Component | Change | Scope |
|-----------|--------|-------|
| `ChannelCapabilities` | Add `audio_input: bool`, `audio_output: bool` fields | Minor (additive, `#[non_exhaustive]` would help) |
| `InboundMessage` | Add optional `audio_data: Option<Vec<u8>>` for raw audio passthrough | Minor |
| `SendParams` | Add optional `audio_data: Option<Vec<u8>>` for pre-rendered TTS | Minor |
| `ChannelProvider` | No change needed. Voice provider implements existing trait. | None |
| `ChannelListener` | No change. Voice provider supplies its own mpsc sender. | None |
| `MessageRouter` | No change. Voice bindings use existing routing. | None |

The core abstraction remains text-first. Audio fields are optional extensions for channels that produce or consume audio alongside text.

### 2. STT options

#### 2A. candle whisper (Pure rust)

Candle (`candle-transformers`) includes a full Whisper implementation covering `tiny` through `large-v3` models. Aletheia already depends on candle v0.9.2 for BERT embeddings in mneme.

**Advantages:**
- Aligns with "pure Rust, no C++ in the brain" principle
- No FFI boundary, no C++ build chain
- Shared dependency (candle already in workspace)
- Model weights loaded from Hugging Face Hub (same as BERT embeddings)
- ~22 MB statically linked (no Python, no PyTorch runtime)

**Disadvantages:**
- CPU inference is slower than whisper.cpp (no GGML quantization optimizations)
- Real-time streaming requires chunked inference with context overlap (not built-in)
- GPU acceleration via CUDA/Metal available but adds build complexity
- `tiny` and `base` models are practical for CPU; `small`+ require GPU for real-time

**Performance (CPU, approximate):**
- `tiny`: ~10x real-time on modern CPU (1 minute audio in ~6 seconds)
- `base`: ~5x real-time
- `small`: ~1-2x real-time (borderline for interactive use)
- `medium`/`large`: below real-time without GPU

**Verdict:** Best fit for batch transcription and low-latency interactive use with `tiny`/`base` models. The accuracy trade-off at small model sizes is acceptable for conversational input where the agent can ask for clarification.

#### 2B. whisper-rs (whisper.cpp bindings)

Rust FFI bindings to whisper.cpp. Latest version 0.15.1 (September 2025). Supports CUDA, ROCm, CoreML acceleration.

**Advantages:**
- Fastest CPU inference (GGML quantized models, SIMD optimized)
- Mature ecosystem with broad model support
- Built-in streaming support via whisper_full_with_state
- 2-3x faster than candle Whisper on CPU for equivalent model sizes

**Disadvantages:**
- C++ dependency (violates "no C++ in the brain")
- Complex build chain (cmake, platform-specific acceleration)
- FFI boundary requires unsafe wrappers
- Additional binary size from C++ runtime

**Verdict:** Technically superior for CPU performance. Conflicts with the pure Rust principle. Could serve as a feature-gated alternative for deployments where latency is critical.

#### 2C. cloud STT APIs

| Provider | Latency | Streaming | Cost | Notes |
|----------|---------|-----------|------|-------|
| OpenAI Whisper API | ~2-5s batch | No | $0.006/min | High accuracy, no streaming |
| OpenAI Realtime API | ~500ms TTFB | Yes (WebSocket) | ~$0.06/min | Speech-to-speech, bypasses text |
| Google Cloud STT | ~300ms streaming | Yes (gRPC) | $0.006-0.024/min | Best streaming support |
| Anthropic | N/A | N/A | N/A | No public STT API as of March 2026 |

**Advantages:**
- Highest accuracy (large model inference)
- No local compute requirements
- Streaming transcription available (Google, OpenAI Realtime)

**Disadvantages:**
- Network latency added to every utterance
- Privacy: audio leaves the machine
- Cost scales with usage
- Dependency on external service availability
- OpenAI Realtime API bypasses the LLM layer entirely (speech-to-speech), which conflicts with aletheia's nous/hermeneus architecture

**Verdict:** Useful as a fallback or for deployments where accuracy is critical. Not suitable as the primary path given the local-first principle.

#### 2D. vosk (Local, c-Based)

Open-source offline STT using Kaldi models. Rust bindings exist but are unmaintained.

**Verdict:** Not recommended. Unmaintained Rust bindings, large model sizes, C/C++ dependency, accuracy behind Whisper.

#### STT recommendation

**Primary:** Candle Whisper with `base` model (best balance of accuracy, latency, and purity). Feature-gated as `voice-stt-candle`.

**Optional:** whisper-rs behind `voice-stt-whisper-cpp` feature gate for latency-sensitive deployments. Same trait interface, different backend.

**Fallback:** Cloud STT behind `voice-stt-cloud` for maximum accuracy when privacy constraints allow.

### 3. TTS options

#### 3A. piper (Local neural TTS)

Fast, local neural TTS. Models are small (15-75 MB). Quality is good for a local engine. Rust bindings exist (`piper-rs`, `piper-tts-rust`) but depend on ONNX Runtime (C++ library).

**Advantages:**
- Natural-sounding voices
- Small models, fast inference (~10x real-time on CPU)
- Offline operation
- Multiple voice options

**Disadvantages:**
- ONNX Runtime dependency (C++ library, conflicts with pure Rust)
- Rust bindings are young (piper-rs released 2025)
- No streaming output (generates full utterance, then plays)

#### 3B. eSpeak-ng (Local, formant-Based)

Mature formant synthesizer. Rust bindings via `espeak-rs`. Compact (~2 MB). Robotic voice quality.

**Advantages:**
- Tiny footprint, fast
- Pure C library, stable FFI
- Useful for phonemization (used as Piper's phonemizer)

**Disadvantages:**
- Robotic voice quality, not suitable as primary TTS
- C dependency

#### 3C. cloud TTS APIs

| Provider | Quality | Latency | Streaming | Cost |
|----------|---------|---------|-----------|------|
| ElevenLabs | Best | ~200ms TTFB | Yes (WebSocket) | $0.15/1K chars |
| Google Cloud TTS | Good | ~100ms | Yes (gRPC) | $0.004-0.016/char |
| OpenAI TTS | Good | ~300ms | Yes (chunked) | $0.015/1K chars |

Anthropic's own Claude voice mode uses ElevenLabs as a subcontractor.

**Advantages:**
- Highest voice quality (ElevenLabs in particular)
- Streaming output reduces perceived latency
- Voice cloning and customization options

**Disadvantages:**
- Network dependency
- Privacy (text leaves the machine)
- Cost per character

#### 3D. candle-Based TTS (Future)

No production-ready TTS model exists in candle today. SpeechT5 and VITS implementations are experimental. This gap may close in 2026-2027 as the Rust ML ecosystem matures.

**Verdict:** Not viable today. Worth monitoring.

#### TTS recommendation

**Phase 1:** No TTS. Text responses only. Voice is input-only. Skipping TTS halves the integration surface and still delivers hands-free interaction.

**Phase 2:** Cloud TTS (ElevenLabs or Google) behind `voice-tts-cloud` feature gate. Best quality, least integration work.

**Phase 3:** Local Piper TTS behind `voice-tts-piper` feature gate when/if pure-Rust ONNX alternatives mature, or accept the C++ dependency behind a gate.

### 4. voice channel design

#### 4.1 audio capture

**Crate:** `cpal` (v0.16, 8.7M downloads, pure Rust, cross-platform). Provides real-time audio input/output with platform-native backends (ALSA, PulseAudio, WASAPI, CoreAudio).

**Format:** Capture at 16kHz mono 16-bit PCM (Whisper's native format). This avoids resampling overhead. cpal handles format negotiation with the hardware.

**Buffer size:** 512 samples (32ms at 16kHz). Balances latency against callback overhead.

#### 4.2 voice activity detection (VAD)

VAD determines when the user starts and stops speaking. Critical for turn-taking.

**Recommended:** Silero VAD via `voice_activity_detector` crate. Silero V5 achieves 87.7% TPR at 5% FPR, significantly outperforming WebRTC VAD (50% TPR at same FPR). The crate uses ONNX Runtime for inference.

**Alternative (pure Rust):** Energy-based VAD as a fallback. RMS threshold with hangover timer. Less accurate but zero dependencies.

**Parameters:**
- Speech threshold: 0.5 probability (Silero default)
- Min speech duration: 250ms (filters coughs, clicks)
- Max silence within speech: 800ms (allows natural pauses)
- Post-speech silence: 1.5s (triggers end-of-turn)

#### 4.3 streaming vs batch transcription

| Approach | Latency | Complexity | Accuracy |
|----------|---------|------------|----------|
| Batch (full utterance) | 1-5s after end-of-speech | Low | Best (full context) |
| Chunked streaming (5s windows) | ~2s rolling | Medium | Good (limited context) |
| True streaming (token-by-token) | ~500ms | High | Lower (no future context) |

**Recommendation:** Start with batch. VAD detects end-of-turn, buffers the full utterance, sends to Whisper in one shot. This requires the least code and is most accurate. Move to chunked streaming only if latency is unacceptable in practice.

**Batch pipeline:**

```
Microphone (cpal, 16kHz PCM)
    → Ring buffer (accumulates audio)
    → VAD (Silero, per-frame classification)
    → Speech segment extraction
    → Whisper inference (candle, base model)
    → Text (InboundMessage.text)
    → Agora router → Nous turn
    → Text response
    → [Optional TTS] → Speaker (cpal output)
```

#### 4.4 latency budget (Batch mode)

| Stage | Target | Notes |
|-------|--------|-------|
| End-of-speech detection | 1.5s | Post-speech silence threshold |
| STT inference (10s utterance) | 2.0s | Candle `base`, CPU |
| Nous turn (LLM round-trip) | 3-8s | Hermeneus provider-dependent |
| TTS rendering (if enabled) | 0.5s | Cloud streaming TTFB |
| **Total voice-to-first-audio** | **7-12s** | Acceptable for assistant interaction |
| **Total voice-to-text** | **3.5-4s** | Without TTS, text appears faster |

For comparison, a typical voice assistant (Alexa, Siri) targets 2-3s total. Aletheia's latency is higher due to the full LLM turn, but this is inherent to the agent model. The STT portion (1.5s + 2.0s = 3.5s) is the controllable overhead.

#### 4.5 audio codec for network transport

If voice data needs to travel over the network (e.g., from a remote theatron client to the server):

**Recommended:** Opus codec via `opus-codec` crate (vendored libopus v1.5.2). Opus is the standard for real-time voice: 6-510 kbps, <5ms algorithmic latency, speech-optimized modes.

**Wire protocol:** WebSocket frame containing Opus-encoded audio chunks. Pylon adds a `WS /api/v1/sessions/{id}/voice` endpoint.

**Alternative (pure Rust):** `mousiki` crate provides a `no_std` Opus decoder. Encode server-side with `opus-codec`, decode client-side with `mousiki` for embedded targets.

### 5. agora integration pattern

Voice integrates as a `VoiceProvider` implementing `ChannelProvider`:

```
VoiceProvider
├── AudioCapture (cpal, 16kHz PCM)
├── VadProcessor (Silero or energy-based)
├── SttEngine (candle Whisper or whisper-rs)
├── TtsEngine (optional: cloud or local)
└── implements ChannelProvider
    ├── id() → "voice"
    ├── capabilities() → { audio_input: true, audio_output: true, ... }
    ├── send() → TTS render + audio output (or text fallback)
    └── probe() → microphone + STT health check
```

**Inbound flow:**
1. `AudioCapture` feeds PCM frames to `VadProcessor`
2. VAD detects speech boundaries, emits `SpeechSegment { audio: Vec<f32>, duration_ms: u64 }`
3. `SttEngine.transcribe(segment)` returns text
4. `VoiceProvider` constructs `InboundMessage { channel: "voice", text, ... }`
5. Sends to shared mpsc channel consumed by `ChannelListener`
6. Normal agora routing applies

**Outbound flow:**
1. Nous produces text response via normal turn pipeline
2. Agora dispatcher calls `VoiceProvider.send(SendParams { text, ... })`
3. If TTS enabled: render to audio, play via cpal output stream
4. If TTS disabled: text response displayed in TUI or logged

**Session key convention:** `voice:{device_id}` or `voice:local` for single-device.

**Config example:**
```toml
[channels.voice]
enabled = true
stt_engine = "candle"         # or "whisper-cpp", "cloud"
stt_model = "base"            # whisper model size
tts_engine = "none"           # or "elevenlabs", "google", "piper"
vad = "silero"                # or "energy"
sample_rate = 16000
silence_threshold_ms = 1500
```

### 6. remote voice (Theatron/Pylon)

For the TUI client (theatron) running on a different machine than the server:

**Phase 1 (local only):** Voice capture and STT run on the server machine. Microphone must be physically connected. This is the batch-mode implementation.

**Phase 2 (remote):** Theatron captures audio locally, streams Opus-encoded frames over WebSocket to pylon. Server-side STT processes the audio. Response text streams back via existing SSE. Optional: TTS audio streams back over the same WebSocket.

**Pylon changes for Phase 2:**
- New endpoint: `WS /api/v1/sessions/{id}/voice`
- WebSocket message types: `AudioChunk` (client → server), `Transcript` (server → client), `TtsChunk` (server → client)
- Reuse existing session management and authentication

This mirrors how the existing SSE streaming works for text but adds a bidirectional audio channel.

---

## Recommendations

### Phased Implementation plan

**Phase 1: Local Batch Voice Input (4-6 weeks)**
- New crate: `phonesis` (from phon/voice + aisthesis/perception)
- Audio capture via cpal (16kHz mono PCM)
- Energy-based VAD (zero additional dependencies)
- Candle Whisper STT with `base` model
- Feature-gated: `voice` in workspace, `voice-stt-candle` in phonesis
- No TTS (text responses only)
- Integration via `ChannelProvider` trait in agora
- Config via taxis
- Target: working local voice input for a single agent

**Phase 2: Improved VAD and Optional TTS (2-3 weeks)**
- Upgrade VAD to Silero (better turn detection)
- Add cloud TTS option (ElevenLabs or Google)
- Feature-gated: `voice-vad-silero`, `voice-tts-cloud`
- Tune silence thresholds based on Phase 1 experience

**Phase 3: Remote Voice via WebSocket (3-4 weeks)**
- Opus encoding/decoding for network transport
- WebSocket endpoint in pylon
- Theatron audio capture and streaming
- Server-side STT for remote clients

**Phase 4: Streaming STT (2-3 weeks)**
- Chunked Whisper inference (5s overlapping windows)
- Partial transcript display during speech
- Reduces perceived latency by showing interim results

### Crate structure

```
crates/phonesis/
├── src/
│   ├── lib.rs           # Module organization, feature gates
│   ├── capture.rs       # cpal audio capture abstraction
│   ├── vad/
│   │   ├── mod.rs       # VadEngine trait
│   │   ├── energy.rs    # RMS-based VAD (default)
│   │   └── silero.rs    # Silero VAD (feature-gated)
│   ├── stt/
│   │   ├── mod.rs       # SttEngine trait
│   │   ├── candle.rs    # Candle Whisper (default)
│   │   └── cloud.rs     # Cloud STT (feature-gated)
│   ├── tts/
│   │   ├── mod.rs       # TtsEngine trait
│   │   └── cloud.rs     # Cloud TTS (feature-gated)
│   ├── provider.rs      # VoiceProvider: ChannelProvider impl
│   └── codec.rs         # Opus encode/decode (Phase 3)
└── Cargo.toml
```

### Dependency budget

| Crate | Purpose | Phase | Pure Rust | Size Impact |
|-------|---------|-------|-----------|-------------|
| `cpal` | Audio I/O | 1 | Yes (Rust + system audio) | ~200KB |
| `candle-transformers` | Whisper STT | 1 | Yes (already in workspace) | Shared |
| `voice_activity_detector` | Silero VAD | 2 | No (ONNX Runtime) | ~15MB |
| `opus-codec` | Network audio | 3 | No (vendored libopus C) | ~500KB |
| `tokio-tungstenite` | WebSocket | 3 | Yes | ~100KB |

Phase 1 adds only `cpal` as a new dependency (candle already present). Each subsequent phase adds dependencies behind feature gates.

---

## Gotchas

1. **cpal platform variance.** Linux audio is fragmented (ALSA, PulseAudio, PipeWire, JACK). cpal supports ALSA natively. PipeWire compatibility requires the PipeWire ALSA plugin. Headless servers (systemd units) may lack a PulseAudio session. Test on the target deployment environment early.

2. **Whisper model download.** Candle Whisper downloads model weights from Hugging Face Hub on first use (~150 MB for `base`). This must happen at startup or be pre-cached. The existing HF Hub integration in mneme's embedding provider provides a pattern.

3. **Microphone permissions.** On Linux, the process needs access to the audio device (typically `audio` group membership). Systemd units may need `SupplementaryGroups=audio`. Landlock (organon's sandbox) blocks device access by default; voice capture must be exempted.

4. **Thread priority.** cpal's audio callback runs on a real-time priority thread. Long operations in the callback cause audio glitches. The callback should only copy PCM data to a ring buffer; all processing (VAD, STT) happens on a separate tokio task.

5. **Candle Whisper memory.** The `base` model loads ~290 MB of weights into RAM. Combined with the existing BERT embedding model (~130 MB), total ML memory footprint reaches ~420 MB. Ensure deployment targets have sufficient RAM.

6. **VAD false positives.** Background noise, music, or other people talking can trigger transcription. The energy-based VAD is especially susceptible. Silero is much better but still imperfect. Consider a wake-word or push-to-talk mode as an alternative to continuous listening.

7. **Opus codec is C.** The recommended `opus-codec` crate vendors C source. This technically violates "no C++ in the brain," though Opus is C (not C++), stable, audited, and an IETF standard. The pure-Rust `mousiki` alternative is decode-only. Accept the C dependency for codec or implement a simpler codec (raw PCM over WebSocket) for Phase 3.

8. **ONNX Runtime for Silero VAD.** The `voice_activity_detector` crate depends on ONNX Runtime (C++ library). This is the same dependency that was removed when fastembed was replaced with candle. Consider implementing Silero's ONNX model via candle (the model is a single-layer LSTM, feasible to port) to maintain the pure-Rust constraint.

9. **Single-user assumption.** Phase 1 assumes one microphone, one user, one agent. Multi-user voice (e.g., a shared device with speaker identification) requires diarization, which is a substantially harder problem. Defer this.

10. **Existing `transcribe` script.** The `shared/bin/transcribe` bash script already does batch audio transcription via the Whisper CLI. Phase 1 replaces this with an in-process Rust implementation. The script can remain for ad-hoc use but should not be the basis for the voice channel.

---

## References

- [candle-transformers Whisper models](https://docs.rs/candle-transformers/latest/candle_transformers/models/whisper/) (v0.9.2, Hugging Face)
- [cpal: Cross-platform audio I/O](https://crates.io/crates/cpal) (v0.16.0, 8.7M downloads)
- [whisper-rs: Rust bindings for whisper.cpp](https://crates.io/crates/whisper-rs) (v0.15.1)
- [voice_activity_detector (Silero VAD)](https://crates.io/crates/voice_activity_detector)
- [Silero VAD repository](https://github.com/snakers4/silero-vad) (6000+ languages, V5 model)
- [piper-rs: Piper TTS for Rust](https://github.com/thewh1teagle/piper-rs)
- [opus-codec: Rust bindings for libopus](https://crates.io/crates/opus-codec) (v0.1.1, vendored v1.5.2)
- [mousiki: Pure Rust Opus decoder](https://lib.rs/crates/mousiki) (no_std, zero-alloc)
- [OpenAI Realtime API](https://platform.openai.com/docs/guides/realtime) (~500ms TTFB, GA August 2025)
- [VAD comparison 2025](https://picovoice.ai/blog/best-voice-activity-detection-vad-2025/) (Silero 87.7% TPR vs WebRTC 50% at 5% FPR)
- Existing: `crates/agora/src/types.rs` (ChannelProvider trait, InboundMessage, SendParams)
- Existing: `crates/mneme/src/embedding.rs` (candle BERT pattern, HF Hub model loading)
- Existing: `shared/bin/transcribe` (bash Whisper CLI wrapper)
- Existing: `standards/CPP.md` (real-time audio callback patterns, lock-free queues)
