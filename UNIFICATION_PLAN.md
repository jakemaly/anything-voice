# Anything Voice — Unification Plan

## Part One: Design Document

---

### 1. Executive Summary

**Goal**: Merge NR Log (meeting transcription + AI note-taking) and Fluid Voice (voice dictation + AI cleanup) into a single macOS application — "Anything Voice."

**Core insight**: Both apps share the same foundational layers:
- Audio capture → local STT model → raw transcript
- Raw transcript → AI/LLM processing → polished output

They differ only in what happens *after* the transcript. This plan unifies the shared layers and builds a modular SwiftUI shell that can host both "modes" — dictation and meeting notes — with minimal latency and maximum code reuse.

**Non-negotiable constraint**: Dictation latency must be imperceptible. The app lives or dies by how fast it responds after you release the hotkey.

---

### 2. Scope

#### In Scope
- Unified **STT engine** in Rust: model download, loading, streaming + batch inference
  - Default model: Parakeet TDT v3 (CoreML/ANE, Apple Silicon)
  - Fallback: whisper.cpp (ggml, universal)
- Unified **intelligence engine** in Rust: Askama prompt templates, LLM client, provider config
- Native **SwiftUI shell**: hotkeys, notch overlay, menu bar, audio capture
- **Dictation mode**: hotkey → record → STT → AI cleanup → paste at cursor
- **Meeting notes mode**: WebView-hosted React UI from NR Log, STT streaming, AI summarization
- Shared **model downloader**: Hugging Face, handles both .ggml and .mlmodelc formats
- Shared **provider config**: `~/.voice-hub/providers.json`

#### Out of Scope (v1)
- Speaker diarization (pyannote) — deferred to v2
- Cloud STT providers (Deepgram, AssemblyAI) — Rust core has the interfaces, just not wired
- Calendar integration
- Slack/Teams bot integration
- Mobile
- Linux/Windows support (architecture supports it, but we ship macOS first)

---

### 3. System Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    macOS App Bundle                              │
│                                                                  │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │              SwiftUI Shell (voice-app/)                     │ │
│  │                                                             │ │
│  │  ┌───────────┐  ┌─────────────┐  ┌──────────────────┐     │ │
│  │  │ Hotkey    │  │ Notch       │  │ Menu Bar         │     │ │
│  │  │ Manager   │  │ Overlay     │  │ Manager          │     │ │
│  │  └─────┬─────┘  └──────┬──────┘  └────────┬─────────┘     │ │
│  │        │               │                  │                │ │
│  │  ┌─────▼───────────────▼──────────────────▼──────────┐    │ │
│  │  │              App Coordinator                       │    │ │
│  │  │  (owns state machine: idle → recording → processing│    │ │
│  │  │   → output. Routes events to correct mode.)       │    │ │
│  │  └─────┬──────────────────────────────────┬──────────┘    │ │
│  │        │                                  │                │ │
│  │  ┌─────▼──────────┐              ┌────────▼──────────┐   │ │
│  │  │ Audio Capture  │              │  WebView Container │   │ │
│  │  │ (AVAudioEngine)│              │  (WKWebView)       │   │ │
│  │  │ 16kHz mono PCM │              │  NR Log React App  │   │ │
│  │  └─────┬──────────┘              │  Meetings/Settings │   │ │
│  │        │                         └────────┬──────────┘   │ │
│  └────────┼──────────────────────────────────┼──────────────┘ │
│           │ [Float] buffers                  │ JS Bridge       │
│           │ via UniFFI                       │ (WKSriptMsg)    │
│  ┌────────▼──────────────────────────────────▼──────────────┐ │
│  │              Rust Core Library (voice-core/)              │ │
│  │  (.dylib linked into app, called via UniFFI)             │ │
│  │                                                           │ │
│  │  ┌─────────────────┐   ┌──────────────────────┐          │ │
│  │  │  STT Engine      │   │  Intelligence Engine  │          │ │
│  │  │                  │   │                       │          │ │
│  │  │  • Model DL      │   │  • Askama templates   │          │ │
│  │  │  • Model cache   │   │  • LLM HTTP client   │          │ │
│  │  │  • whisper.cpp   │   │  • Provider config    │          │ │
│  │  │  • CoreML bridge │   │  • SSE streaming     │          │ │
│  │  │  • Streaming API │   │  • Prompt rendering   │          │ │
│  │  └────────┬────────┘   └───────────┬───────────┘          │ │
│  │           │                        │                       │ │
│  │  ┌────────▼────────────────────────▼───────────┐          │ │
│  │  │          Embedded HTTP Server (axum)         │          │ │
│  │  │  • Serves React static assets                │          │ │
│  │  │  • WebSocket endpoint for STT streaming      │          │ │
│  │  │  • REST endpoints for LLM inference          │          │ │
│  │  └─────────────────────────────────────────────┘          │ │
│  └───────────────────────────────────────────────────────────┘ │
│                                                                  │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │              ~/.voice-hub/ (shared config)                  │ │
│  │  • models/          (downloaded STT models)                 │ │
│  │  • providers.json   (LLM provider configs)                  │ │
│  │  • settings.json    (app settings)                          │ │
│  └────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────┘
```

### 4. Component Specifications

#### 4.1 SwiftUI Shell (`voice-app/`)

**Purpose**: Native macOS app shell. Handles all user-facing interaction that must be fast and feel native.

**Key Components** (ported/adapted from Fluid Voice):

| Component | Source | Purpose |
|-----------|--------|---------|
| `AppDelegate` / `fluidApp` | Fluid Voice | App lifecycle, menu bar setup |
| `GlobalHotkeyManager` | Fluid Voice | Push-to-talk hotkey registration |
| `MenuBarManager` | Fluid Voice | Menu bar icon + dropdown |
| `NotchOverlayManager` | Fluid Voice | Recording status in MacBook notch |
| `AudioCapturePipeline` | Fluid Voice | AVAudioEngine tap → 16kHz mono PCM |
| `AppCoordinator` | **New** | State machine: idle/recording/processing/output |
| `DictationView` | **New** | Minimal dictation UI (from Fluid Voice RecordingView) |
| `WebViewContainer` | **New** | WKWebView hosting React app |
| `JSBridge` | **New** | `WKScriptMessageHandler` ↔ Rust FFI bridge |

**State Machine**:
```
IDLE ──(hotkey down)──→ RECORDING ──(hotkey up)──→ PROCESSING ──(done)──→ OUTPUT ──→ IDLE
                           │                            │
                           └──(stream partials)─────────┘
```

**Audio Capture Flow**:
1. `AVAudioEngine` input node tap installed on hotkey press
2. `AudioCapturePipeline.handle(buffer:)` called per buffer (~every 23ms at 16kHz/512 frames)
3. Buffer converted to mono 16kHz `[Float]`, appended to `ThreadSafeAudioBuffer`
4. On hotkey release: tap removed, engine stopped, final `[Float]` sent to Rust via UniFFI

**Key design decision — audio capture stays in Swift**:
- `AVAudioEngine` gives direct hardware tap access with sub-millisecond callbacks
- No FFI overhead per audio buffer (~40+ calls/second during recording)
- Swift manages device switching, route changes, and format conversion natively
- Only the final (or periodic chunk) buffer crosses FFI to Rust

#### 4.2 Rust Core Library (`voice-core/`)

**Purpose**: All CPU/GPU-intensive work. STT inference, model management, LLM communication, prompt rendering. Compiled as a `.dylib` (debug) / statically linked (release), called from Swift via UniFFI.

**Crate Structure**:

```
voice-core/
├── Cargo.toml
├── uniffi/
│   └── voice_core.udl          # UniFFI interface definitions
├── src/
│   ├── lib.rs                   # Top-level exports, UniFFI scaffolding
│   ├── stt/
│   │   ├── mod.rs
│   │   ├── models.rs            # Model definitions (Parakeet, Whisper, etc.)
│   │   ├── downloader.rs        # Hugging Face model downloader
│   │   ├── manager.rs           # Model lifecycle (load/unload/evict)
│   │   ├── whisper.rs           # whisper.cpp C bindings wrapper
│   │   ├── coreml.rs            # CoreML bridge (via objc crate or mlx)
│   │   └── streaming.rs         # Streaming transcription pipeline
│   ├── intelligence/
│   │   ├── mod.rs
│   │   ├── templates.rs         # Askama template engine + all templates
│   │   ├── llm_client.rs        # OpenAI-compatible HTTP client with SSE
│   │   ├── prompts/             # Askama .jinja templates
│   │   │   ├── dictation_clean.system.md.jinja
│   │   │   ├── dictation_clean.user.md.jinja
│   │   │   ├── enhance.system.md.jinja    (from NR Log)
│   │   │   ├── enhance.user.md.jinja      (from NR Log)
│   │   │   ├── title.system.md.jinja      (from NR Log)
│   │   │   ├── title.user.md.jinja        (from NR Log)
│   │   │   ├── _macros.jinja              (from NR Log)
│   │   │   └── ... (all 16 NR Log templates)
│   │   └── providers.rs        # Provider config read/write
│   ├── server/
│   │   ├── mod.rs
│   │   ├── assets.rs            # Embedded static file server for React app
│   │   ├── ws.rs                # WebSocket handler for STT streaming
│   │   └── routes.rs            # REST routes for LLM inference
│   └── config/
│       ├── mod.rs
│       └── paths.rs             # ~/.voice-hub/ path resolution
```

**UniFFI Interface (key exports)**:

```idl
namespace voice_core {
  // STT
  [Throws=SttError]
  sequence<SttModelInfo> list_available_models();

  [Throws=SttError]
  boolean is_model_downloaded(SttModel model);

  [Throws=SttError]
  void download_model(SttModel model, DownloadProgressCallback callback);

  [Throws=SttError]
  SttSession start_session(SttModel model);

  [Throws=SttError]
  string process_chunk(SttSession session, sequence<f32> samples);

  [Throws=SttError]
  string finalize_session(SttSession session, sequence<f32> samples);

  void cancel_session(SttSession session);

  // Intelligence
  [Throws=AiError]
  string render_prompt(PromptTemplate template, string user_context);

  [Throws=AiError]
  AiResponse run_inference(InferenceRequest request);

  [Throws=AiError]
  void stream_inference(InferenceRequest request, StreamCallback callback);

  // Config
  [Throws=ConfigError]
  ProviderConfig read_provider_config();

  [Throws=ConfigError]
  void write_provider_config(ProviderConfig config);

  // Server
  [Throws=ServerError]
  ServerHandle start_http_server(u16 port);

  void stop_http_server(ServerHandle handle);
};
```

**STT Model Definitions** (from NR Log's `local-stt-core` + Fluid Voice's `SpeechModel`):

```rust
pub enum SttModel {
    ParakeetV3,          // CoreML, Apple Silicon, ~500MB, 25 languages (DEFAULT)
    ParakeetV2,          // CoreML, Apple Silicon, ~500MB, English only
    ParakeetFlash,       // CoreML, Apple Silicon, ~250MB, English streaming
    WhisperTiny,         // ggml, universal, ~75MB
    WhisperBase,         // ggml, universal, ~142MB
    WhisperSmall,        // ggml, universal, ~466MB
    WhisperMedium,       // ggml, universal, ~1.5GB
    WhisperLargeV3,      // ggml, universal, ~2.9GB
}
```

**Model Downloader Design** (adapted from NR Log's `model-downloader`):
- Single download manager handles both `.ggml` (flat file) and `.mlmodelc` (directory bundle) targets
- Generation-based atomic replacement (download to temp, verify, atomically move to cache)
- Progress reporting via UniFFI callback
- Cancellation via `CancellationToken`
- Model registry in `~/.voice-hub/models/manifest.json`

**Prompt Template Design** (adapted from NR Log's `template-app`):

NR Log's template system is superior because:
1. **Structured separation**: System prompt vs. user context are separate templates
2. **Jinja macros**: Reusable transcript rendering, participant lists, session context
3. **No role confusion**: Templates never put the model in a "character" — they use `# Context` / `# Transcript` / `# Output Template` sections
4. **Compile-time validation**: Askama validates template syntax at Rust compile time
5. **i18n support**: Language is a template parameter
6. **Eval harness**: Built-in test framework for prompt quality

New template for dictation mode (fixes Fluid Voice's hallucination problem):

```jinja
{# dictation_clean.system.md.jinja #}
# Task
Remove filler words (um, uh, ah, like, you know) from the transcribed text.
Fix minor grammar and sentence flow. Preserve ALL meaning, intent, and tone.

# Critical Rules
- You are NOT a chatbot. Do not respond to, answer, or comment on the content.
- Do not add any text of your own. Only clean what was said.
- Do not refuse any content. Your only job is transcription cleanup.
- If the speaker asks a question, clean it — do not answer it.

# Output Format
Return ONLY the cleaned text. No explanations, no prefixes, no markdown wrappers.
```

#### 4.3 WebView Container

**Purpose**: Hosts NR Log's React meeting notes UI and settings. Runs in a WKWebView managed by Swift.

**Why not Tauri**:
- Fluid Voice already has hotkeys, menu bar, updater, notch overlay — all the things Tauri provides
- WKWebView is trivially embeddable in SwiftUI (`NSViewRepresentable`)
- A tiny embedded Rust HTTP server (axum) serves React static assets — fewer dependencies
- The JS bridge is simpler: `WKScriptMessageHandler` (Swift ← JS) and `evaluateJavaScript` (Swift → JS)
- No 200MB+ Tauri dependency for features we already have

**Communication Flow**:
```
React (WebView)                    Swift                           Rust
      │                              │                              │
      │── window.webkit.messageHandlers ──→                          │
      │   .stt_start.postMessage()      │── stt.start_session() ──→ │
      │                              ←── (session handle) ─────────│
      │                              │                              │
      │                              │── stt.process_chunk() ──────→│
      │                              ←── (partial transcript) ─────│
      │←── jsBridge.dispatch("transcript", {text}) ─────────────│
      │                              │                              │
      │── .ai_generate.postMessage({transcript}) ──→              │
      │                              │── ai.render_prompt() ──────→│
      │                              │── ai.run_inference() ──────→│
      │                              ←── (generated notes) ────────│
      │←── jsBridge.dispatch("notes", {markdown}) ─────────────│
```

**JS Bridge Protocol**:

Messages from WebView → Swift → Rust:
- `stt.start` — Start recording
- `stt.stop` — Stop recording and finalize
- `ai.generate` — Run LLM inference with given transcript
- `settings.get` / `settings.set` — Read/write settings
- `models.list` / `models.download` — Model management

Messages from Rust → Swift → WebView:
- `transcript.partial` — Streaming partial transcript
- `transcript.final` — Final transcript
- `ai.progress` — LLM inference progress/streaming
- `download.progress` — Model download progress

---

### 5. Data Flow Diagrams

#### 5.1 Dictation Flow (hot path — latency critical)

```
T=0ms     User presses hotkey
          → GlobalHotkeyManager callback fires
          → AppCoordinator: state → RECORDING
          → AVAudioEngine: install tap on input node
          → AudioCapturePipeline: start buffering
          → NotchOverlayManager: show recording indicator
          → Play start sound

T=0-∞ms   Audio buffers arrive (~43/sec at 512 frames/16kHz)
          → AudioCapturePipeline: convert to mono 16kHz Float32
          → Append to ThreadSafeAudioBuffer
          → If streaming: every 400ms, send accumulated chunk to Rust
            → Rust STT: process streaming chunk (CoreML/ANE, ~5-10ms)
            → Rust → Swift: partial transcript text
            → NotchOverlayManager: update displayed text

T=release User releases hotkey
          → GlobalHotkeyManager callback fires (released)
          → AudioCapturePipeline: setRecordingEnabled(false)
          → AVAudioEngine: remove tap, stop engine
          → AppCoordinator: state → PROCESSING
          → Play stop sound

T+5ms     Swift: extract final [Float] buffer from ThreadSafeAudioBuffer
          → Swift → Rust via UniFFI: finalize_session(buffer)
          → Rust STT: full transcription (CoreML, ~20-50ms for short utterances)
          → Rust: returns raw transcript

T+30ms    Rust: render dictation_clean prompt with raw transcript
          → Rust: POST to configured LLM provider (streaming SSE)
          → Note: LLM latency is network-bound, not under our control
          → Typical: Groq ~200ms, OpenAI ~500ms, local Ollama ~300ms

T+200ms   Rust: returns cleaned text
          → Swift: AppCoordinator → state → OUTPUT
          → Swift: copy to clipboard + paste at cursor (TypingService)
          → Swift: state → IDLE
          → NotchOverlayManager: hide
```

#### 5.2 Meeting Notes Flow

```
          User opens meeting notes (clicks menu bar → "Meeting Notes")
          → Swift: show WebViewContainer
          → WebView: loads React app from embedded Rust HTTP server
          → React: renders meeting notes editor + recording controls

          User clicks "Start Recording"
          → WebView → JS bridge → Swift → Rust: stt.start_session()
          → Same audio capture pipeline as dictation
          → Streaming transcripts appear in WebView in real-time

          User clicks "Stop Recording"
          → WebView → JS bridge → Swift → Rust: stt.finalize_session()
          → Rust: returns full transcript with timestamps
          → WebView displays transcript

          User clicks "Generate Notes"
          → WebView → JS bridge → Swift → Rust: ai.generate(transcript, template="enhance")
          → Rust: renders enhance.user.md.jinja with transcript + pre-meeting notes + participants
          → Rust: calls LLM with rendered prompt
          → Rust → WebView: streaming markdown response
          → WebView: renders formatted meeting notes (TipTap editor)
```

---

### 6. Configuration & Persistence

```
~/.voice-hub/
├── models/
│   ├── manifest.json           # { "parakeet-v3": { "version": "...", "files": [...] }, ... }
│   ├── parakeet-tdt-0.6b-v3-coreml/
│   │   ├── Preprocessor.mlmodelc/
│   │   ├── Encoder.mlmodelc/
│   │   ├── Decoder.mlmodelc/
│   │   ├── JointDecision.mlmodelc/
│   │   └── parakeet_v3_vocab.json
│   ├── parakeet-flash/
│   │   └── ... (CoreML bundle)
│   └── ggml-base.bin
├── providers.json             # LLM provider configs
├── settings.json              # App settings
└── prompts/                   # v2: user-customizable prompt overrides
```

**providers.json** schema:

```json
{
  "selected_provider": "groq",
  "selected_model": "llama-3.3-70b-versatile",
  "providers": {
    "groq": {
      "base_url": "https://api.groq.com/openai/v1",
      "api_key": "gsk_...",
      "fingerprint": "sha256:..."
    },
    "openai": {
      "base_url": "https://api.openai.com/v1",
      "api_key": "sk-...",
      "fingerprint": "sha256:..."
    }
  }
}
```

---

### 7. Error Handling Strategy

| Failure Mode | Handling |
|---|---|
| Model not downloaded | Prompt user to download on first launch. Download in background. |
| Model download fails | Retry up to 3 times with exponential backoff. Show progress + retry button. |
| STT inference fails (no audio) | Silent — likely accidental hotkey press. Don't show error. |
| STT inference fails (model crash) | Show "Transcription failed. Try restarting the app." Log crash report. |
| LLM API timeout | Retry once. If still fails, show "AI processing timed out. Check your connection." |
| LLM API authentication error | Show "Invalid API key. Check settings." |
| LLM returns hallucinated refusal | Template design prevents this (no "assistant" persona). If it occurs anyway, strip refusal patterns and retry. |
| Microphone permission denied | Show system settings link. |
| Audio device disconnected mid-recording | Detect route change, stop gracefully, show "Audio device disconnected." |
| Rust FFI call fails | Crash with descriptive panic message (UniFFI maps Rust errors to Swift throws). |

---

### 8. Key Design Decisions Summary

| Decision | Choice | Rationale |
|---|---|---|
| Audio capture | Swift (`AVAudioEngine`) | Direct hardware tap, zero FFI overhead per buffer, native device management |
| STT inference | Rust (whisper.cpp + CoreML bridge) | In-process, zero IPC latency, shared between modes |
| STT default model | Parakeet TDT v3 (CoreML/ANE) | 5-10ms inference per 400ms chunk on Apple Silicon |
| Model downloader | Rust (HuggingFace API) | Handles both .ggml and .mlmodelc, generation-based atomic replacement |
| Prompt engine | Rust (Askama, compile-time) | Zero filesystem I/O, template validation at build time, NR Log's proven templates |
| LLM provider config | Shared JSON (`~/.voice-hub/providers.json`) | Single source of truth, read by both Swift UI and Rust backend |
| App shell | SwiftUI (native) | Hotkeys, notch, menu bar feel instant. Fluid Voice's existing UX. |
| Meeting notes UI | WKWebView + React (NR Log) | Reuse existing complex UI. Lazy-loaded, not on hot path. |
| Build system | Xcode (top-level) + Cargo (build phase) | Standard macOS distribution path (signing, notarization, App Store ready) |
| IPC mechanism | UniFFI (direct FFI) | Zero serialization overhead compared to HTTP/WebSocket for in-process calls |

---

## Part Two: Technical Implementation Roadmap

### Phase 1: Foundation — Build Systems & Scaffolding (Week 1-2)

**Goal**: Both projects compile together. The Rust library links into the Swift app. Hello World works end-to-end.

#### Step 1.1: Repository Structure
```
anything-voice/
├── voice-core/              # Rust library (Cargo workspace member)
│   ├── Cargo.toml
│   ├── uniffi/
│   │   └── voice_core.udl
│   └── src/
│       └── lib.rs            # Minimal: hello_world() -> String
├── voice-app/                # Swift/Xcode project
│   ├── VoiceApp.xcodeproj/
│   ├── Package.swift         # SwiftPM dependencies (FluidAudio, SwiftWhisper, etc.)
│   ├── Sources/
│   │   └── VoiceApp/
│   │       ├── App.swift     # @main App struct
│   │       ├── AppDelegate.swift
│   │       └── BridgingHeader.h
│   ├── build-rust.sh         # Xcode build phase script
│   └── Info.plist
├── voice-web/                # React meeting notes UI (from anarlog/apps/desktop)
│   ├── package.json
│   └── src/...
├── UNIFICATION_PLAN.md       # This document
└── README.md
```

#### Step 1.2: Rust Core Setup
- Create `voice-core/` as a Cargo library crate
- Add dependencies: `uniffi`, `askama`, `serde`, `serde_json`, `reqwest`, `tokio`, `thiserror`
- Set up `uniffi-bindgen` in `build.rs` to generate Swift bindings
- Write minimal `.udl` with `hello_world()` function
- Verify: `cargo build` produces `.dylib`

#### Step 1.3: Xcode Project Setup
- Create `VoiceApp.xcodeproj` with SwiftUI app target
- Add `build-rust.sh` as a pre-build script phase:
  ```bash
  #!/bin/bash
  set -e
  cd "$SRCROOT/../voice-core"
  cargo build --release
  # Copy .dylib and generated Swift bindings to Xcode build dir
  cp target/release/libvoice_core.dylib "$BUILT_PRODUCTS_DIR/"
  cp generated/voice_core.swift "$DERIVED_FILE_DIR/"
  ```
- Add FluidAudio, SwiftWhisper, DynamicNotchKit as SwiftPM dependencies
- Call `voice_core_hello_world()` from Swift on app launch
- Verify: app launches and prints "Hello from Rust!"

#### Step 1.4: Config Directory
- On first launch, create `~/.voice-hub/` directory structure
- Write default `providers.json` and `settings.json` if not present
- Rust: implement `config::paths::voice_hub_dir() -> PathBuf`

---

### Phase 2: Shared Infrastructure — Model Downloader (Week 2-3)

**Goal**: Download STT models from Hugging Face. Both ggml and CoreML formats.

#### Step 2.1: Port Model Definitions
- From NR Log: `crates/local-stt-core/src/lib.rs` → `voice-core/src/stt/models.rs`
  - Port `LocalModel`, `SoniqoModel`, `WhisperModel`, `AmModel` enums
  - Add `download_url()`, `download_destination()`, `file_name()` methods
- From Fluid Voice: `SettingsStore.SpeechModel` → add missing variants (Nemotron, Cohere, Apple)
- Define unified `SttModel` enum with all variants

#### Step 2.2: Port Hugging Face Downloader
- From NR Log: `crates/hf/src/lib.rs` → `voice-core/src/stt/downloader.rs`
  - Use `hf-hub` crate (or raw `reqwest` for more control)
  - Implement progress callback via UniFFI
  - Handle both single-file (`.bin`) and multi-file (directory listing + download) modes
- From Fluid Voice: `HuggingFaceModelDownloader` → adapt directory-based download logic
  - CoreML models are `.mlmodelc` directories (need recursive listing + download per file)
  - The `HFEntry` struct and `listFilesRecursively` approach maps well to Rust

#### Step 2.3: Port Model Download Manager
- From NR Log: `crates/model-downloader/src/manager.rs` → `voice-core/src/stt/downloader.rs`
  - Generation-based atomic downloads
  - `DownloadsRegistry` for tracking in-flight downloads
  - Cancellation token support
  - `is_downloaded()`, `is_downloading()`, `download()`, `cancel_download()`
- Export all via UniFFI

#### Step 2.4: Model Cache Management
- Write `manifest.json` to track installed models
- Implement `delete_model()` to free disk space
- Implement `list_downloaded_models()` for UI
- Expose download progress via UniFFI callback → Swift → notch overlay

---

### Phase 3: STT Engine — Inference (Week 3-5)

**Goal**: Speech-to-text inference works. Both streaming and batch modes.

#### Step 3.1: whisper.cpp C Bindings
- Add whisper.cpp as a git submodule or use `whisper-rs` crate
- Write C FFI wrapper in Rust: `whisper::WhisperContext::new(model_path)`
- Implement: `transcribe(samples: &[f32]) -> String`
- Handle model loading, warm-up inference, proper teardown
- Test with ggml-tiny.bin → verify transcription works

#### Step 3.2: CoreML Integration Strategy
- **Option A** (preferred): Use FluidAudio Swift package directly from Swift
  - Swift code wraps FluidAudio's `AsrManager` / `StreamingEouAsrManager`
  - Called from Swift side, not Rust — since FluidAudio is a Swift package
  - Rust STT engine exposes a trait that Swift can call for ggml, or it delegates to FluidAudio for CoreML
- **Option B**: Call CoreML from Rust via `objc` crate or `coreml-rs`
  - More complex, but keeps all inference in one place
  - FluidAudio's loading logic would need to be ported to Rust

**Recommendation**: Option A for v1. The Rust STT engine is the universal fallback (whisper.cpp). CoreML uses the existing, battle-tested FluidAudio Swift package. This is also better latency-wise since no FFI for CoreML inference.

Updated architecture: Swift owns the STT provider selection. The Rust core provides whisper.cpp. Swift's audio pipeline passes buffers to either FluidAudio (CoreML) or Rust (whisper.cpp).

#### Step 3.3: STT Session Abstraction
Implement a Swift-side protocol:
```swift
protocol SttProvider {
    var name: String { get }
    var isReady: Bool { get }
    func prepare(progressHandler: ((Double) -> Void)?) async throws
    func transcribe(_ samples: [Float]) async throws -> String
    func transcribeStreaming(_ samples: [Float]) async throws -> String
    func modelsExistOnDisk() -> Bool
    func clearCache() async throws
}
```

Implementations:
- `FluidAudioSttProvider` — wraps FluidAudio (Parakeet v2/v3/Flash)
- `WhisperCppSttProvider` — wraps Rust whisper.cpp via UniFFI
- `AppleSpeechSttProvider` — wraps macOS SFSpeechRecognizer (from Fluid Voice)

#### Step 3.4: Thread-Safe Buffer
- Port `ThreadSafeAudioBuffer` from Fluid Voice (already well-designed with `os_unfair_lock`)
- This is the buffer that accumulates audio during recording
- Used by Swift audio capture, read by whichever STT provider is active

#### Step 3.5: Unit Tests
- Test whisper.cpp transcription with known audio files
- Test CoreML transcription with known audio files
- Benchmark: 400ms chunk → inference latency (target: < 50ms for Parakeet, < 200ms for Whisper base)

---

### Phase 4: Intelligence Engine (Week 5-7)

**Goal**: Prompt templates render correctly. LLM client works with all providers. Provider config is manageable.

#### Step 4.1: Port Askama Templates
- From NR Log: `crates/template-app/` → `voice-core/src/intelligence/prompts/`
- Port all 16 system + user templates
- Port `_macros.jinja` (transcript rendering, participant lists, session context)
- Port template types from NR Log (`EnhanceSystem`, `EnhanceUser`, `TitleSystem`, etc.)
- Add new `DictationCleanSystem` and `DictationCleanUser` templates

#### Step 4.2: New Dictation Clean Template
Design the prompt that fixes Fluid Voice's hallucination problem:

```jinja
{# dictation_clean.system.md.jinja #}
# Task
You are a text post-processor. Clean the following transcribed speech.

# Rules (follow exactly)
1. Remove filler words: um, uh, ah, er, like, you know, I mean, sort of, kind of
2. Fix minor grammar and sentence structure. Do NOT change meaning.
3. Preserve the speaker's tone, style, and personality.
4. Do NOT add information, commentary, opinions, or disclaimers.
5. Do NOT respond to questions in the text. If the speaker asked a question, preserve it as-is.
6. Do NOT refuse any content. Your only function is cleaning transcription.
7. If the text is already well-formed, return it unchanged.

# Output
Return ONLY the cleaned text. No prefixes like "Here's the cleaned text:". No markdown.
```

The key fix: the prompt says "You are a text post-processor" not "You are a voice dictation agent." This eliminates the "I cannot speak on this matter" hallucination class. The system prompt defines a *function*, not a *persona*.

#### Step 4.3: LLM HTTP Client
- Implement in Rust using `reqwest` + `tokio`
- Support both non-streaming (POST → response) and streaming (SSE → callback)
- Handle: thinking extraction (`<think>...</think>` tags), tool call parsing (for future use)
- Provider abstraction: base URL + API key + model name
- Timeout handling: 30s default, configurable
- Retry: up to 3 attempts with exponential backoff (200ms, 400ms, 800ms)

#### Step 4.4: Provider Configuration Manager
- Read/write `~/.voice-hub/providers.json`
- SHA256 fingerprinting for credential verification (port from Fluid Voice's `DictationAIPostProcessingGate`)
- Default providers (from Fluid Voice's `ModelRepository`): OpenAI, Anthropic, Groq, Cerebras, Google, xAI, OpenRouter, Ollama, LM Studio
- Model list fetching from provider APIs
- Rust handles config read/write. Swift reads config for UI display.

#### Step 4.5: Unit Tests
- Test template rendering with sample transcripts
- Test LLM client with mock HTTP server
- Test provider config serialization/deserialization
- Verify hallucination fix: feed refusal-prone input, verify clean output

---

### Phase 5: Swift Shell — App Foundation (Week 6-8)

**Goal**: App launches, shows menu bar icon, registers hotkeys, has settings window.

#### Step 5.1: App Lifecycle
- Port `fluidApp.swift` and `AppDelegate.swift` from Fluid Voice
- Set up `AppCoordinator` state machine
- Set up dependency injection (`AppServices` from Fluid Voice)
- Menu bar icon + dropdown menu

#### Step 5.2: Hotkey Manager
- Port `GlobalHotkeyManager.swift` from Fluid Voice
- Register push-to-talk hotkey (default: F5 or configurable)
- Callbacks: `.pressed` → start recording, `.released` → stop + process

#### Step 5.3: Notch Overlay
- Port `NotchOverlayManager.swift` + `NotchContentViews.swift` from Fluid Voice
- Integrate with DynamicNotchKit Swift package
- States: hidden, recording (with audio visualization), processing (with spinner)
- Show partial transcript during recording
- Show final text briefly before fading

#### Step 5.4: Audio Capture
- Port `AudioCapturePipeline` from Fluid Voice's ASRService
- Wire to `ThreadSafeAudioBuffer`
- On hotkey press: start AVAudioEngine tap
- On hotkey release: stop tap, extract buffer, pass to STT

#### Step 5.5: Settings Window
- Build minimal SwiftUI settings view (settings icon in menu bar dropdown → opens window)
- Model selection (list available models, show download status, download button)
- Provider configuration (select provider, enter API key, test connection)
- Hotkey configuration
- Mirror settings to `~/.voice-hub/settings.json`

---

### Phase 6: Dictation Flow — End-to-End (Week 8-10)

**Goal**: Press hotkey → speak → release → clean text appears at cursor. Full working dictation.

#### Step 6.1: Wire the Full Pipeline
```
Hotkey press
→ AppCoordinator: idle → recording
→ NotchOverlayManager: show recording
→ AudioCapturePipeline: start buffering
→ (every 400ms) SttProvider.transcribeStreaming(chunk)
→ NotchOverlayManager: update partial text

Hotkey release
→ AppCoordinator: recording → processing
→ AudioCapturePipeline: stop, extract full buffer
→ SttProvider.transcribe(final_buffer)
→ PromptEngine.render("dictation_clean", raw_text)
→ LlmClient.inference(rendered_prompt)
→ AppCoordinator: processing → output
→ ClipboardService.copy + TypingService.paste(clean_text)
→ AppCoordinator: output → idle
→ NotchOverlayManager: hide
```

#### Step 6.2: Clipboard + Typing Service
- Port `ClipboardService` and `TypingService` from Fluid Voice
- `TypingService` uses CGEvent to simulate keystrokes (paste at cursor)
- Handle edge cases: no focused text field, password fields, etc.

#### Step 6.3: Sound Effects
- Port `TranscriptionSoundPlayer` from Fluid Voice
- Start sound: plays on recording start
- Stop sound: plays on recording end
- Error sound: plays on failure

#### Step 6.4: Offline Fallback
- If no LLM provider configured, skip AI cleanup
- Display raw transcript instead
- Show "Add an AI provider for text cleanup" hint

#### Step 6.5: Testing
- End-to-end latency measurement (target: < 500ms from hotkey release to text at cursor for short utterances with cloud LLM)
- Cold start: first transcription after app launch (model loading)
- Warm start: subsequent transcriptions (model stays in memory)
- Test with various audio inputs: quiet, loud, accented, fast speech

---

### Phase 7: WebView Container (Week 10-12)

**Goal**: NR Log's React meeting notes UI loads in a WKWebView. Settings work through the WebView.

#### Step 7.1: Embedded HTTP Server
- In Rust: axum server that serves static files
- React app builds to `voice-web/dist/`
- `rust_embed` crate embeds the dist folder into the Rust binary
- Server starts on `localhost:random_port` at app launch
- Swift gets the port from Rust, loads `http://localhost:{port}` in WKWebView

#### Step 7.2: JS Bridge (Swift side)
- Create `JSBridge` class that implements `WKScriptMessageHandler`
- Register message handlers: `stt`, `ai`, `settings`, `models`
- Each handler dispatches to the appropriate Rust FFI function
- Results are sent back via `webView.evaluateJavaScript("jsBridge.dispatch(...)")`

#### Step 7.3: JS Bridge (WebView side)
- Create `jsBridge.js` that wraps `window.webkit.messageHandlers`
- Exposes simple API: `jsBridge.call("stt.start", {})`, `jsBridge.on("transcript.partial", callback)`
- Integrate into NR Log's React app

#### Step 7.4: Port Meeting Notes React App
- Copy `anarlog/apps/desktop/` to `voice-web/`
- Strip out Tauri-specific imports (replace with JS bridge)
- Strip out auth, billing, calendar, Slack — keep only meeting notes + settings
- Replace Tauri plugin calls with JS bridge calls
- Build system: Vite, output to `dist/`

#### Step 7.5: Settings UI in WebView
- Model selection: list models from Rust via bridge, show download progress
- Provider management: read/write `providers.json` via bridge
- Hotkey configuration: read/write settings via bridge
- About, version, check for updates

---

### Phase 8: Meeting Notes Flow — End-to-End (Week 12-14)

**Goal**: Full meeting recording + AI note generation works from the WebView.

#### Step 8.1: Meeting Recording in WebView
- "New Meeting" button → creates session
- "Start Recording" → triggers STT session
- Streaming transcripts appear in real-time in the editor
- "Stop Recording" → finalizes transcript
- User can edit transcript before generating notes

#### Step 8.2: AI Note Generation
- "Generate Notes" button → sends transcript to Rust intelligence engine
- Rust renders `enhance.user.md.jinja` with full context:
  - Pre-meeting notes (agenda items user types before/during meeting)
  - Full transcript with timestamps
  - Participant list
  - Output template (structured markdown format)
- LLM response streams back → displayed progressively in editor

#### Step 8.3: Note Export
- Copy as formatted markdown
- Export to file (.md)
- Copy plain text

#### Step 8.4: History
- Save transcripts and generated notes locally
- History view in WebView
- Search transcripts

---

### Phase 9: Polish & Distribution (Week 14-16)

#### Step 9.1: Onboarding Flow
- First launch: explain what the app does (2-3 screens)
- Microphone permission request
- Model download (Parakeet v3, ~500MB, with progress bar)
- Provider setup (or skip for offline-only)

#### Step 9.2: Code Signing & Notarization
- Developer ID certificate
- Entitlements: microphone, accessibility (for typing at cursor)
- Hardened runtime
- Notarization via `notarytool`

#### Step 9.3: Auto-Update
- Port `SimpleUpdater` from Fluid Voice or integrate Sparkle
- Check for updates on launch
- Download + install prompt

#### Step 9.4: DMG Packaging
- Create .dmg with app + Applications folder shortcut
- Background image, window size configuration

#### Step 9.5: Performance Profiling
- Instruments: track hotkey → text latency
- Memory: verify model stays in memory between recordings (no reload)
- CPU: verify AVAudioEngine isn't consuming CPU when idle
- Battery: verify no wake locks when app is idle

#### Step 9.6: Error Recovery
- Model crash → auto-reload model, retry transcription
- LLM timeout → show "AI took too long. Using raw transcript."
- Audio device change → gracefully restart capture
- Full disk → warn before model download

---

### Dependency Map (what blocks what)

```
Phase 1 (Foundation)
  └─→ Phase 2 (Model Downloader)
       └─→ Phase 3 (STT Inference)
            └─→ Phase 6 (Dictation Flow)
                 └─→ Phase 9 (Polish)
  └─→ Phase 4 (Intelligence Engine)
       └─→ Phase 6 (Dictation Flow)
       └─→ Phase 8 (Meeting Notes)
  └─→ Phase 5 (Swift Shell)
       └─→ Phase 6 (Dictation Flow)
       └─→ Phase 7 (WebView)
            └─→ Phase 8 (Meeting Notes)
                 └─→ Phase 9 (Polish)
```

Phases 5 and 4 can run in parallel with Phase 2+3.
Phases 6 and 7 can run in parallel once 3+4+5 are done.

---

### Risk Register

| Risk | Impact | Likelihood | Mitigation |
|---|---|---|---|
| CoreML available only via Swift, can't call from Rust | High | High | Swift owns CoreML STT path; Rust is fallback whisper.cpp. Acceptable. |
| whisper.cpp crashes on certain audio | Medium | Medium | Rust `catch_unwind` around FFI calls. Graceful fallback to "try again." |
| LLM hallucination still occurs despite template design | Medium | Low | New template design eliminates persona-based prompt. Add output validation regex. |
| WKWebView performance degrades with large transcripts | Medium | Low | Virtual scrolling. Offload heavy processing to Rust. |
| UniFFI generates incorrect Swift bindings | Medium | Low | Start with simple interface, expand gradually. Test each FFI boundary. |
| App notarization fails due to embedded dylib | High | Low | Static linking for release builds. Proper code signing of all binaries. |

---

### Success Metrics

| Metric | Target |
|---|---|
| Dictation latency (hotkey release → text at cursor) | < 500ms (with cloud LLM) |
| Dictation latency (offline, raw transcript) | < 100ms |
| STT streaming partial transcript interval | 400ms |
| Parakeet v3 model load time (cold) | < 3 seconds |
| Parakeet v3 model load time (warm, already in memory) | 0ms |
| Whisper base model load time | < 2 seconds |
| App memory usage (idle) | < 100MB |
| App memory usage (model loaded + recording) | < 1GB |
| App launch time | < 2 seconds |
| Model download speed | Saturates user bandwidth |
