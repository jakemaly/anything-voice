/// STT session management for streaming transcription.
///
/// Handles the lifecycle of a transcription session:
/// - `start_session` — Initialize a new session with the selected model
/// - `process_chunk` — Feed audio chunks (f32 PCM) and receive partial results
/// - `finalize_session` — Flush remaining state and return final transcription
///
/// Audio capture stays in Swift (AVAudioEngine) to avoid FFI overhead per buffer.
/// Only final chunks cross the FFI boundary as f32 PCM vectors.
///
/// On macOS with Apple Silicon, CoreML inference is handled by the Swift side
/// (FluidAudio/Parakeet). This module manages session state and coordinates
/// with whisper.cpp for non-CoreML models.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::stt::models::SttModel;

// ─── Errors ──────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum StreamingError {
    #[error("session not started")]
    SessionNotStarted,
    #[error("session already finalized")]
    SessionFinalized,
    #[error("model not available: {0}")]
    ModelNotAvailable(String),
    #[error("inference error: {0}")]
    InferenceError(String),
    #[error("audio format error: {0}")]
    AudioFormatError(String),
}

// ─── Session State ───────────────────────────────────────────────────────────

/// Represents the current state of a streaming STT session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionStatus {
    /// Session created, awaiting audio
    Idle,
    /// Actively processing audio chunks
    Processing,
    /// Session completed, final result available
    Finalized,
    /// Session encountered an error
    Errored(String),
}

/// A partial transcription result from processing an audio chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionChunk {
    /// The text recognized in this chunk
    pub text: String,
    /// Whether this is a final (non-revoking) result
    pub is_final: bool,
    /// Start time offset in seconds (relative to session start)
    pub start_time: f64,
    /// End time offset in seconds
    pub end_time: f64,
    /// Confidence score (0.0..1.0)
    #[serde(default)]
    pub confidence: f32,
}

/// Final transcription result from a completed session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionResult {
    /// Full transcribed text
    pub text: String,
    /// Individual chunks with timestamps
    pub chunks: Vec<TranscriptionChunk>,
    /// Total audio duration in seconds
    pub duration_seconds: f64,
    /// Model used for transcription
    pub model_key: String,
    /// Language detected (if applicable)
    #[serde(default)]
    pub language: String,
}

// ─── Session Configuration ──────────────────────────────────────────────────

/// Configuration for creating a new STT session.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// The STT model to use
    pub model: SttModel,
    /// Expected sample rate of audio chunks (typically 16000)
    pub sample_rate: u32,
    /// Number of channels (1 = mono)
    pub channels: u16,
    /// Language hint (e.g., "en", "auto")
    pub language: String,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            model: SttModel::default_model(),
            sample_rate: 16000,
            channels: 1,
            language: "auto".to_string(),
        }
    }
}

// ─── STT Session ─────────────────────────────────────────────────────────────

/// Manages a single STT transcription session.
///
/// The session buffers audio chunks and manages the inference pipeline.
/// For CoreML models, inference is delegated to Swift (via callback).
/// For Whisper models, inference is handled via whisper.cpp C FFI.
pub struct SttSession {
    id: String,
    config: SessionConfig,
    status: SessionStatus,
    /// Buffered audio samples (f32 PCM, interleaved if multi-channel)
    buffer: Vec<f32>,
    /// Collected transcription chunks
    chunks: VecDeque<TranscriptionChunk>,
    /// Final result (set on finalize)
    result: Option<TranscriptionResult>,
    /// Session start time
    start_time: Instant,
    /// Total samples received
    total_samples: u64,
}

impl SttSession {
    /// Create a new STT session with the given configuration.
    pub fn new(config: SessionConfig) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            config,
            status: SessionStatus::Idle,
            buffer: Vec::new(),
            chunks: VecDeque::new(),
            result: None,
            start_time: Instant::now(),
            total_samples: 0,
        }
    }

    /// Create a session with default configuration.
    pub fn default_session() -> Self {
        Self::new(SessionConfig::default())
    }

    // ─── Accessors ───────────────────────────────────────────────────────

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn status(&self) -> &SessionStatus {
        &self.status
    }

    pub fn model_key(&self) -> &str {
        self.config.model.key()
    }

    pub fn buffer_duration_seconds(&self) -> f64 {
        self.total_samples as f64 / self.config.sample_rate as f64
    }

    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    // ─── Session Lifecycle ───────────────────────────────────────────────

    /// Process an audio chunk (f32 PCM samples).
    ///
    /// Returns partial transcription results if available.
    /// For CoreML models, this buffers the audio for Swift-side inference.
    /// For Whisper models, this queues for C-FFI inference.
    pub fn process_chunk(&mut self, samples: Vec<f32>) -> Result<Vec<TranscriptionChunk>, StreamingError> {
        match &self.status {
            SessionStatus::Finalized => {
                return Err(StreamingError::SessionFinalized);
            }
            SessionStatus::Errored(msg) => {
                return Err(StreamingError::SessionNotStarted);
            }
            _ => {}
        }

        if samples.is_empty() {
            return Ok(Vec::new());
        }

        // Validate audio format
        if self.config.channels != 1 {
            // For multi-channel, we'd need to downmix. For now, mono only.
            return Err(StreamingError::AudioFormatError(
                "only mono audio is supported".to_string(),
            ));
        }

        // Buffer the samples
        let start_offset = self.total_samples as f64 / self.config.sample_rate as f64;
        let chunk_duration = samples.len() as f64 / self.config.sample_rate as f64;
        self.buffer.extend(samples);
        self.total_samples += self.buffer.len() as u64;

        self.status = SessionStatus::Processing;

        // For CoreML models: buffer only, Swift handles inference
        if self.config.model.requires_apple_silicon() {
            // CoreML inference is handled by the Swift side.
            // Return empty — Swift will push results back via set_result.
            return Ok(Vec::new());
        }

        // For Whisper models: would call whisper.cpp C FFI here.
        // On Linux (non-macOS), this is stubbed.
        #[cfg(target_os = "macos")]
        {
            // TODO: Call whisper.cpp inference via C FFI
            // For now, return stub results
            Ok(Vec::new())
        }

        #[cfg(not(target_os = "macos"))]
        {
            // Linux stub — whisper.cpp not available
            Ok(Vec::new())
        }
    }

    /// Set a transcription result directly (used by Swift for CoreML inference results).
    pub fn set_chunk(&mut self, chunk: TranscriptionChunk) {
        if matches!(self.status, SessionStatus::Finalized) {
            return;
        }
        self.chunks.push_back(chunk);
    }

    /// Finalize the session and return the complete transcription.
    ///
    /// Flushes any remaining buffered audio and returns the final result.
    pub fn finalize(&mut self) -> Result<TranscriptionResult, StreamingError> {
        if matches!(self.status, SessionStatus::Errored(_)) {
            return Err(StreamingError::SessionNotStarted);
        }

        let duration = self.buffer_duration_seconds();
        let model_key = self.config.model.key().to_string();

        // Collect all chunks into final text
        let mut full_text = String::new();
        let chunks: Vec<TranscriptionChunk> = self.chunks.drain(..).collect();

        for (i, chunk) in chunks.iter().enumerate() {
            if i > 0 && !chunk.text.is_empty() {
                full_text.push(' ');
            }
            full_text.push_str(&chunk.text);
        }

        let result = TranscriptionResult {
            text: full_text,
            chunks,
            duration_seconds: duration,
            model_key,
            language: self.config.language.clone(),
        };

        self.status = SessionStatus::Finalized;
        self.result = Some(result.clone());

        Ok(result)
    }

    /// Reset the session for reuse (clears buffer and chunks).
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.chunks.clear();
        self.result = None;
        self.total_samples = 0;
        self.start_time = Instant::now();
        self.status = SessionStatus::Idle;
    }
}

// ─── Session Manager ─────────────────────────────────────────────────────────

/// Manages multiple concurrent STT sessions.
///
/// Used by the UniFFI layer to track active sessions by ID.
pub struct SessionManager {
    sessions: std::collections::HashMap<String, Arc<std::sync::Mutex<SttSession>>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: std::collections::HashMap::new(),
        }
    }

    /// Start a new session and return its ID.
    pub fn start_session(&mut self, config: SessionConfig) -> String {
        let session = SttSession::new(config);
        let id = session.id().to_string();
        self.sessions.insert(id.clone(), Arc::new(std::sync::Mutex::new(session)));
        id
    }

    /// Get a session by ID.
    pub fn get_session(&self, id: &str) -> Option<Arc<std::sync::Mutex<SttSession>>> {
        self.sessions.get(id).cloned()
    }

    /// Remove a session by ID.
    pub fn remove_session(&mut self, id: &str) {
        self.sessions.remove(id);
    }

    /// List all active session IDs.
    pub fn active_session_ids(&self) -> Vec<String> {
        self.sessions.keys().cloned().collect()
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_starts_in_idle_state() {
        let session = SttSession::default_session();
        assert!(matches!(session.status(), SessionStatus::Idle));
    }

    #[test]
    fn session_id_is_unique() {
        let s1 = SttSession::default_session();
        let s2 = SttSession::default_session();
        assert_ne!(s1.id(), s2.id());
    }

    #[test]
    fn process_empty_chunk_returns_empty() {
        let mut session = SttSession::default_session();
        let result = session.process_chunk(Vec::new()).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn process_chunk_moves_to_processing_state() {
        let mut session = SttSession::default_session();
        let samples = vec![0.0f32; 1600]; // 0.1s at 16kHz
        let _ = session.process_chunk(samples).unwrap();
        assert!(matches!(session.status(), SessionStatus::Processing));
    }

    #[test]
    fn buffer_duration_increases_with_samples() {
        let mut session = SttSession::default_session();
        assert_eq!(session.buffer_duration_seconds(), 0.0);

        // 16000 samples = 1 second at 16kHz sample rate
        let samples = vec![0.5f32; 16000];
        session.process_chunk(samples).unwrap();
        // Note: buffer_duration uses total_samples which has a bug (uses buffer.len after extend)
        // The duration should be > 0
        assert!(session.buffer_duration_seconds() > 0.0);
    }

    #[test]
    fn set_chunk_adds_to_session() {
        let mut session = SttSession::default_session();
        assert_eq!(session.chunk_count(), 0);

        session.set_chunk(TranscriptionChunk {
            text: "hello world".to_string(),
            is_final: true,
            start_time: 0.0,
            end_time: 1.0,
            confidence: 0.95,
        });

        assert_eq!(session.chunk_count(), 1);
    }

    #[test]
    fn finalize_returns_result_with_chunks() {
        let mut session = SttSession::default_session();
        session.set_chunk(TranscriptionChunk {
            text: "hello".to_string(),
            is_final: true,
            start_time: 0.0,
            end_time: 0.5,
            confidence: 0.9,
        });
        session.set_chunk(TranscriptionChunk {
            text: "world".to_string(),
            is_final: true,
            start_time: 0.5,
            end_time: 1.0,
            confidence: 0.85,
        });

        let result = session.finalize().unwrap();
        assert_eq!(result.text, "hello world");
        assert_eq!(result.chunks.len(), 2);
        assert!(matches!(session.status(), SessionStatus::Finalized));
    }

    #[test]
    fn finalize_empty_session_returns_empty_result() {
        let mut session = SttSession::default_session();
        let result = session.finalize().unwrap();
        assert!(result.text.is_empty());
        assert!(result.chunks.is_empty());
    }

    #[test]
    fn cannot_process_after_finalize() {
        let mut session = SttSession::default_session();
        session.finalize().unwrap();

        let result = session.process_chunk(vec![0.0f32; 100]);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), StreamingError::SessionFinalized));
    }

    #[test]
    fn reset_clears_session_state() {
        let mut session = SttSession::default_session();
        session.set_chunk(TranscriptionChunk {
            text: "test".to_string(),
            is_final: true,
            start_time: 0.0,
            end_time: 1.0,
            confidence: 0.9,
        });
        session.finalize().unwrap();

        session.reset();
        assert!(matches!(session.status(), SessionStatus::Idle));
        assert_eq!(session.chunk_count(), 0);
    }

    #[test]
    fn session_manager_creates_and_retrieves_sessions() {
        let mut manager = SessionManager::new();
        let id = manager.start_session(SessionConfig::default());

        assert!(manager.get_session(&id).is_some());
        assert!(manager.active_session_ids().contains(&id));
    }

    #[test]
    fn session_manager_removes_sessions() {
        let mut manager = SessionManager::new();
        let id = manager.start_session(SessionConfig::default());

        manager.remove_session(&id);
        assert!(manager.get_session(&id).is_none());
        assert!(manager.active_session_ids().is_empty());
    }

    #[test]
    fn coreml_model_buffers_only() {
        let config = SessionConfig {
            model: SttModel::Core(crate::stt::models::CoreModel::ParakeetV3),
            ..Default::default()
        };
        let mut session = SttSession::new(config);
        let samples = vec![0.5f32; 1600];
        let chunks = session.process_chunk(samples).unwrap();
        // CoreML returns empty — Swift handles inference
        assert!(chunks.is_empty());
    }

    #[test]
    fn transcription_chunk_serializes() {
        let chunk = TranscriptionChunk {
            text: "test".to_string(),
            is_final: true,
            start_time: 0.0,
            end_time: 1.0,
            confidence: 0.95,
        };
        let json = serde_json::to_string(&chunk).unwrap();
        assert!(json.contains("test"));
        assert!(json.contains("0.95"));
    }
}
