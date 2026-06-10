/// Axum-based HTTP server with REST and WebSocket endpoints.
///
/// Endpoints:
/// - GET  /api/models           — list available STT models
/// - GET  /api/providers        — load provider config
/// - POST /api/providers        — save provider config
/// - POST /api/infer            — LLM inference (non-streaming)
/// - WS   /api/stt-stream       — WebSocket for STT chunk streaming
/// - GET  /                     — serve React app (index.html)

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info, warn};

use crate::intelligence::providers::ProviderConfig;
use crate::stt::models::SttModel;

// ─── Server State ────────────────────────────────────────────────────────────

/// Shared state accessible to all handlers.
#[derive(Clone)]
pub struct ServerState {
    /// Number of active WebSocket connections
    pub active_connections: Arc<AtomicUsize>,
}

impl ServerState {
    pub fn new() -> Self {
        Self {
            active_connections: Arc::new(AtomicUsize::new(0)),
        }
    }
}

// ─── Server ──────────────────────────────────────────────────────────────────

pub struct VoiceServer {
    base_url: String,
    shutdown_tx: Option<tokio::sync::watch::Sender<()>>,
    server_task: Option<tokio::task::JoinHandle<()>>,
}

impl VoiceServer {
    /// Start the server on localhost with a random port.
    pub async fn start() -> std::io::Result<Self> {
        let state = ServerState::new();
        let router = Self::build_router(state.clone());

        let listener =
            tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).await?;
        let server_addr = listener.local_addr()?;
        let base_url = format!("http://{}", server_addr);

        info!(base_url = %base_url, "VoiceServer starting");

        let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(());
        let server_task = tokio::spawn(async move {
            let result = axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    shutdown_rx.changed().await.ok();
                })
                .await;

            if let Err(e) = result {
                error!(error = %e, "server error");
            }
        });

        info!(base_url = %base_url, "VoiceServer ready");

        Ok(Self {
            base_url,
            shutdown_tx: Some(shutdown_tx),
            server_task: Some(server_task),
        })
    }

    fn build_router(state: ServerState) -> Router {
        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);

        Router::new()
            .route("/api/models", get(list_models))
            .route("/api/providers", get(get_providers).post(set_providers))
            .route("/api/infer", post(infer))
            .route("/api/stt-stream", get(stt_stream_ws))
            .route("/", get(serve_index))
            .layer(cors)
            .with_state(state)
    }

    /// Get the base URL (e.g., "http://127.0.0.1:54321").
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Get the port the server is listening on.
    pub fn port(&self) -> u16 {
        self.base_url
            .trim_start_matches("http://127.0.0.1:")
            .parse()
            .unwrap_or(0)
    }

    /// Stop the server gracefully.
    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(task) = self.server_task.take() {
            task.abort();
        }
    }
}

impl Drop for VoiceServer {
    fn drop(&mut self) {
        self.stop();
    }
}

// ─── REST Handlers ───────────────────────────────────────────────────────────

/// GET /api/models — list all available STT models
async fn list_models() -> impl IntoResponse {
    let models: Vec<ModelInfo> = SttModel::all()
        .iter()
        .map(|m| ModelInfo {
            key: m.key().to_string(),
            display_name: m.display_name().to_string(),
            description: m.description().to_string(),
            size_bytes: m.size_bytes() as i64,
            requires_apple_silicon: m.requires_apple_silicon(),
        })
        .collect();

    Json(ApiResponse {
        success: true,
        data: Some(serde_json::to_value(models).unwrap()),
        error: None,
    })
}

/// GET /api/providers — load provider config
async fn get_providers() -> impl IntoResponse {
    match ProviderConfig::load() {
        Ok(config) => Json(ApiResponse {
            success: true,
            data: Some(serde_json::to_value(config).unwrap()),
            error: None,
        }),
        Err(e) => Json(ApiResponse {
            success: false,
            data: None,
            error: Some(e.to_string()),
        }),
    }
}

/// POST /api/providers — save provider config
async fn set_providers(Json(config): Json<ProviderConfig>) -> impl IntoResponse {
    match config.save() {
        Ok(()) => Json(ApiResponse {
            success: true,
            data: None,
            error: None,
        }),
        Err(e) => Json(ApiResponse {
            success: false,
            data: None,
            error: Some(e.to_string()),
        }),
    }
}

/// POST /api/infer — LLM inference (non-streaming)
async fn infer(Json(req): Json<InferRequest>) -> impl IntoResponse {
    let client = crate::intelligence::llm_client::LlmClient::new();
    let request = crate::intelligence::llm_client::ChatRequest {
        model: req.model,
        messages: req
            .messages
            .into_iter()
            .map(|m| crate::intelligence::llm_client::ChatMessage {
                role: m.role,
                content: m.content,
            })
            .collect(),
        temperature: req.temperature.map(|t| t as f64),
        max_tokens: req.max_tokens,
        stream: false,
    };

    match client
        .inference(&request, &req.base_url, &req.api_key)
        .await
    {
        Ok(response) => Json(ApiResponse {
            success: true,
            data: Some(serde_json::json!({
                "content": response.content,
                "thinking": response.thinking,
            })),
            error: None,
        }),
        Err(e) => Json(ApiResponse {
            success: false,
            data: None,
            error: Some(e.to_string()),
        }),
    }
}

// ─── WebSocket Handler ───────────────────────────────────────────────────────

/// GET /api/stt-stream — WebSocket for real-time STT chunk streaming.
///
/// The WebView connects here and receives transcription chunks as JSON:
/// ```json
/// {"text":"hello world","is_final":true,"start_time":0.0,"end_time":1.5,"confidence":0.92}
/// ```
async fn stt_stream_ws(
    State(state): State<ServerState>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(mut socket: WebSocket, state: ServerState) {
    state.active_connections.fetch_add(1, Ordering::Relaxed);
    info!("WebSocket client connected");

    // Send ready message
    if socket.send(Message::Text(r#"{"type":"ready"}"#.into())).await.is_err() {
        warn!("Failed to send ready message");
        return;
    }

    // Echo mode: forward any received messages back
    while let Some(Ok(msg)) = socket.recv().await {
        match msg {
            Message::Text(text) => {
                let _ = socket.send(Message::Text(text)).await;
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    state.active_connections.fetch_sub(1, Ordering::Relaxed);
    info!("WebSocket client disconnected");
}

// ─── Static File Serving ─────────────────────────────────────────────────────

/// GET / — serve a minimal index.html (placeholder for React app)
async fn serve_index() -> impl IntoResponse {
    let html = r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>Voice Hub</title></head>
<body><div id="root"><p>Voice Hub — React app not yet deployed.</p></div></body>
</html>"#;
    axum::response::Html(html.to_string())
}

// ─── Request/Response Types ──────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct ModelInfo {
    pub key: String,
    pub display_name: String,
    pub description: String,
    pub size_bytes: i64,
    pub requires_apple_silicon: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InferRequest {
    pub model: String,
    pub messages: Vec<InferMessage>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    pub base_url: String,
    pub api_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InferMessage {
    pub role: String,
    pub content: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_info_serializes() {
        let info = ModelInfo {
            key: "whisper-tiny".to_string(),
            display_name: "Whisper Tiny".to_string(),
            description: "99 Languages, ~75 MB".to_string(),
            size_bytes: 75_000_000,
            requires_apple_silicon: false,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("whisper-tiny"));
    }

    #[test]
    fn api_response_success() {
        let resp = ApiResponse {
            success: true,
            data: Some(serde_json::json!({"key": "value"})),
            error: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("success"));
        assert!(!json.contains("error"));
    }

    #[test]
    fn api_response_error() {
        let resp = ApiResponse {
            success: false,
            data: None,
            error: Some("something went wrong".to_string()),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("something went wrong"));
    }

    #[test]
    fn infer_request_serializes() {
        let req = InferRequest {
            model: "gpt-4".to_string(),
            messages: vec![InferMessage {
                role: "user".to_string(),
                content: "hello".to_string(),
            }],
            temperature: Some(0.7),
            max_tokens: Some(100),
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: "sk-test".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("gpt-4"));
        assert!(json.contains("hello"));
    }

    #[test]
    fn server_state_tracks_connections() {
        let state = ServerState::new();
        assert_eq!(state.active_connections.load(Ordering::Relaxed), 0);
        state.active_connections.fetch_add(1, Ordering::Relaxed);
        assert_eq!(state.active_connections.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn server_state_clones() {
        let state1 = ServerState::new();
        let state2 = state1.clone();
        assert_eq!(state1.active_connections.load(Ordering::Relaxed), 0);
        assert_eq!(state2.active_connections.load(Ordering::Relaxed), 0);
    }
}
