/// Embedded HTTP server for the WKWebView React app.
///
/// Provides:
/// - REST endpoints for LLM inference, model management, settings
/// - WebSocket endpoint for real-time STT streaming to the WebView
/// - Static file serving for the React app
///
/// Adapted from NR Log's local-stt-server crate.
/// Feature-gated behind `server`.

#[cfg(feature = "server")]
mod http;
#[cfg(feature = "server")]
pub use http::VoiceServer;

#[cfg(not(feature = "server"))]
/// Stub when server feature is disabled.
pub struct VoiceServer;

#[cfg(not(feature = "server"))]
impl VoiceServer {
    pub fn new() -> Self {
        Self
    }
}
