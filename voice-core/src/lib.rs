mod config;
mod intelligence;
mod server;
mod stt;

use std::sync::{LazyLock, Mutex};

// Global session manager
static SESSION_MANAGER: LazyLock<Mutex<stt::streaming::SessionManager>> =
    LazyLock::new(|| Mutex::new(stt::streaming::SessionManager::new()));

// ─── Rust structs matching UDL dictionaries ─────────────────────────────────
// These must match the UDL dictionary field names and types exactly.
// The `setup_scaffolding!` macro wires them to the UDL definitions.

pub struct SttModelInfo {
    pub key: String,
    pub display_name: String,
    pub description: String,
    pub size_bytes: i64,
    pub requires_apple_silicon: bool,
}

pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

pub struct AiResponse {
    pub content: String,
    pub thinking: String,
}

pub struct TranscriptionChunk {
    pub text: String,
    pub is_final: bool,
    pub start_time: f64,
    pub end_time: f64,
    pub confidence: f32,
}

pub struct TranscriptionResult {
    pub text: String,
    pub chunks: Vec<TranscriptionChunk>,
    pub duration_seconds: f64,
    pub model_key: String,
    pub language: String,
}

// ─── SttSession (UDL interface) ─────────────────────────────────────────────

pub struct SttSession {
    inner: std::sync::Arc<std::sync::Mutex<stt::streaming::SttSession>>,
}

impl SttSession {
    fn id(&self) -> String {
        self.inner.lock().unwrap().id().to_string()
    }

    fn status(&self) -> String {
        format!("{:?}", self.inner.lock().unwrap().status())
    }

    fn model_key(&self) -> String {
        self.inner.lock().unwrap().model_key().to_string()
    }

    fn buffer_duration_seconds(&self) -> f64 {
        self.inner.lock().unwrap().buffer_duration_seconds()
    }

    fn chunk_count(&self) -> u32 {
        self.inner.lock().unwrap().chunk_count() as u32
    }

    fn process_chunk(&self, samples: Vec<f32>) -> Vec<TranscriptionChunk> {
        let mut session = self.inner.lock().unwrap();
        match session.process_chunk(samples) {
            Ok(chunks) => chunks
                .into_iter()
                .map(|c| TranscriptionChunk {
                    text: c.text,
                    is_final: c.is_final,
                    start_time: c.start_time,
                    end_time: c.end_time,
                    confidence: c.confidence,
                })
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    fn set_chunk(&self, chunk: TranscriptionChunk) {
        let mut session = self.inner.lock().unwrap();
        session.set_chunk(stt::streaming::TranscriptionChunk {
            text: chunk.text,
            is_final: chunk.is_final,
            start_time: chunk.start_time,
            end_time: chunk.end_time,
            confidence: chunk.confidence,
        });
    }

    fn finalize(&self) -> TranscriptionResult {
        let mut session = self.inner.lock().unwrap();
        match session.finalize() {
            Ok(result) => TranscriptionResult {
                text: result.text,
                chunks: result
                    .chunks
                    .into_iter()
                    .map(|c| TranscriptionChunk {
                        text: c.text,
                        is_final: c.is_final,
                        start_time: c.start_time,
                        end_time: c.end_time,
                        confidence: c.confidence,
                    })
                    .collect(),
                duration_seconds: result.duration_seconds,
                model_key: result.model_key,
                language: result.language,
            },
            Err(_) => TranscriptionResult {
                text: String::new(),
                chunks: Vec::new(),
                duration_seconds: 0.0,
                model_key: String::new(),
                language: String::new(),
            },
        }
    }

    fn reset(&self) {
        self.inner.lock().unwrap().reset();
    }
}

// ─── UniFFI Namespace Functions ─────────────────────────────────────────────

/// Returns the path to the voice-hub directory (~/.voice-hub/)
pub fn voice_hub_dir() -> String {
    config::paths::voice_hub_dir().to_string_lossy().to_string()
}

/// List all available STT models.
pub fn list_available_models() -> Vec<SttModelInfo> {
    stt::models::SttModel::all()
        .iter()
        .map(|m| SttModelInfo {
            key: m.key().to_string(),
            display_name: m.display_name().to_string(),
            description: m.description().to_string(),
            size_bytes: m.size_bytes() as i64,
            requires_apple_silicon: m.requires_apple_silicon(),
        })
        .collect()
}

/// Start a new STT session and return its ID.
pub fn start_stt_session(
    model_key: String,
    sample_rate: u32,
    language: String,
) -> String {
    let model = stt::models::SttModel::from_key(&model_key)
        .unwrap_or(stt::models::SttModel::default_model());

    let config = stt::streaming::SessionConfig {
        model,
        sample_rate,
        channels: 1,
        language,
    };

    let mut manager = SESSION_MANAGER.lock().unwrap();
    manager.start_session(config)
}

/// Get a session by ID.
pub fn get_session(session_id: String) -> std::sync::Arc<SttSession> {
    let manager = SESSION_MANAGER.lock().unwrap();
    let inner = manager
        .get_session(&session_id)
        .expect("Session not found");

    std::sync::Arc::new(SttSession { inner })
}

/// Render a prompt template.
pub fn render_template(template_name: String, context: String) -> String {
    intelligence::templates::render_template(&template_name, &context)
        .unwrap_or_else(|e| format!("Error: {}", e))
}

/// Make a non-streaming LLM inference request.
/// Note: Blocks the current thread via tokio runtime. Use from a background thread.
pub fn infer(
    model: String,
    messages: Vec<ChatMessage>,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    base_url: String,
    api_key: String,
) -> AiResponse {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    rt.block_on(async {
        let client = intelligence::llm_client::LlmClient::new();
        let request = intelligence::llm_client::ChatRequest {
            model,
            messages: messages
                .into_iter()
                .map(|m| intelligence::llm_client::ChatMessage {
                    role: m.role,
                    content: m.content,
                })
                .collect(),
            temperature: temperature.map(|t| t as f64),
            max_tokens,
            stream: false,
        };

        match client.inference(&request, &base_url, &api_key).await {
            Ok(response) => AiResponse {
                content: response.content,
                thinking: response.thinking,
            },
            Err(e) => AiResponse {
                content: format!("Error: {}", e),
                thinking: String::new(),
            },
        }
    })
}

/// Load provider config as JSON string.
pub fn load_provider_config_json() -> String {
    match intelligence::providers::ProviderConfig::load() {
        Ok(config) => serde_json::to_string_pretty(&config).unwrap_or_default(),
        Err(e) => format!("{{\"error\":\"{}\"}}", e),
    }
}

/// Save provider config from JSON string.
pub fn save_provider_config_json(json: String) {
    let _ = serde_json::from_str::<intelligence::providers::ProviderConfig>(&json)
        .map(|config| config.save())
        .map(|_| ());
}

include!(concat!(env!("OUT_DIR"), "/voice_core.uniffi.rs"));
