/// OpenAI-compatible LLM HTTP client with SSE streaming support.
///
/// Handles:
/// - Non-streaming requests (POST → JSON response)
/// - Streaming requests (SSE → chunk callback)
/// - Thinking token extraction (think tags)
/// - Retry with exponential backoff
///
/// Adapted from Fluid Voice's LLMClient.swift and NR Log's llm-proxy crate.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

// ─── Request Types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

// ─── Response Types ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ChatResponse {
    pub choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
pub struct Choice {
    pub message: ChatMessage,
}

#[derive(Debug, Deserialize)]
pub struct StreamChunk {
    pub choices: Vec<StreamChoice>,
}

#[derive(Debug, Deserialize)]
pub struct StreamChoice {
    pub delta: StreamDelta,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StreamDelta {
    #[serde(default)]
    pub content: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AiResponse {
    pub content: String,
    #[serde(default)]
    pub thinking: String,
}

// ─── LLM Client ─────────────────────────────────────────────────────────────

pub struct LlmClient {
    http: Client,
    max_retries: u32,
    base_retry_delay_ms: u64,
}

impl Default for LlmClient {
    fn default() -> Self {
        Self::new()
    }
}

impl LlmClient {
    pub fn new() -> Self {
        Self {
            http: Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .expect("Failed to build HTTP client"),
            max_retries: 3,
            base_retry_delay_ms: 200,
        }
    }

    /// Make a non-streaming inference request.
    pub async fn inference(
        &self,
        request: &ChatRequest,
        base_url: &str,
        api_key: &str,
    ) -> Result<AiResponse, LlmError> {
        let url = self.build_url(base_url);
        let mut request = request.clone();
        request.stream = false;

        let body = serde_json::to_string(&request)
            .map_err(|e| LlmError::EncodingFailed(e.to_string()))?;

        let mut last_error = LlmError::Unknown("no attempts made".to_string());

        for attempt in 0..self.max_retries {
            match self.do_request(&url, &body, api_key, false).await {
                Ok(response) => {
                    let content = response
                        .choices
                        .first()
                        .map(|c| c.message.content.clone())
                        .unwrap_or_default();
                    let thinking = Self::extract_thinking(&content);
                    let cleaned = Self::strip_thinking(&content);
                    return Ok(AiResponse {
                        content: cleaned,
                        thinking,
                    });
                }
                Err(e) => {
                    last_error = e;
                    if attempt < self.max_retries - 1 {
                        let delay =
                            self.base_retry_delay_ms * 2u64.pow(attempt);
                        tokio::time::sleep(Duration::from_millis(delay)).await;
                    }
                }
            }
        }

        Err(last_error)
    }

    /// Make a streaming inference request.
    /// Calls the callback for each content chunk, then on_done with final content.
    pub async fn stream_inference(
        &self,
        request: &ChatRequest,
        base_url: &str,
        api_key: &str,
        mut callback: impl FnMut(String) + Send + 'static,
    ) -> Result<AiResponse, LlmError> {
        let url = self.build_url(base_url);
        let mut request = request.clone();
        request.stream = true;

        let body = serde_json::to_string(&request)
            .map_err(|e| LlmError::EncodingFailed(e.to_string()))?;

        let response = self.http
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", api_key))
            .body(body)
            .send()
            .await
            .map_err(|e| LlmError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(LlmError::HttpError(status.as_u16(), body));
        }

        let mut full_content = String::new();
        let mut thinking_buf = String::new();
        let mut in_thinking = false;

        let mut stream = response.bytes_stream();
        use futures::StreamExt;

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result
                .map_err(|e| LlmError::StreamError(e.to_string()))?;
            let text = String::from_utf8_lossy(&chunk);

            for line in text.lines() {
                let line = line.trim();
                if line.starts_with("data: ") {
                    let data = &line[6..];
                    if data == "[DONE]" {
                        break;
                    }

                    if let Ok(chunk) = serde_json::from_str::<StreamChunk>(data) {
                        if let Some(choice) = chunk.choices.first() {
                            if let Some(content) = &choice.delta.content {
                                // Handle thinking tags
                                if content.contains("<think") {
                                    in_thinking = true;
                                }
                                if content.contains("</think") {
                                    in_thinking = false;
                                    if let Some(end) = content.find("</think") {
                                        thinking_buf.push_str(&content[..end + 8]);
                                    }
                                    continue;
                                }

                                if in_thinking {
                                    thinking_buf.push_str(content);
                                } else {
                                    full_content.push_str(content);
                                    callback(content.clone());
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(AiResponse {
            content: full_content,
            thinking: thinking_buf,
        })
    }

    fn build_url(&self, base_url: &str) -> String {
        let trimmed = base_url.trim_end_matches('/');
        format!("{}/chat/completions", trimmed)
    }

    async fn do_request(
        &self,
        url: &str,
        body: &str,
        api_key: &str,
        _stream: bool,
    ) -> Result<ChatResponse, LlmError> {
        let response = self.http
            .post(url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", api_key))
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| LlmError::NetworkError(e.to_string()))?;

        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|e| LlmError::NetworkError(e.to_string()))?;

        if !status.is_success() {
            let body_str = String::from_utf8_lossy(&bytes).to_string();
            return Err(LlmError::HttpError(status.as_u16(), body_str));
        }

        serde_json::from_slice(&bytes)
            .map_err(|e| LlmError::DecodeFailed(e.to_string()))
    }

    fn extract_thinking(content: &str) -> String {
        let mut thinking = String::new();
        let mut in_thinking = false;

        for line in content.lines() {
            if line.contains("<think") {
                in_thinking = true;
                continue;
            }
            if line.contains("</think") {
                in_thinking = false;
                continue;
            }
            if in_thinking {
                thinking.push_str(line);
                thinking.push('\n');
            }
        }

        thinking.trim().to_string()
    }

    fn strip_thinking(content: &str) -> String {
        let mut result = String::new();
        let mut in_thinking = false;

        for line in content.lines() {
            if line.contains("<think") {
                in_thinking = true;
                continue;
            }
            if line.contains("</think") {
                in_thinking = false;
                continue;
            }
            if !in_thinking {
                result.push_str(line);
                result.push('\n');
            }
        }

        result.trim().to_string()
    }
}

// ─── Errors ─────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("network error: {0}")]
    NetworkError(String),
    #[error("HTTP {0}: {1}")]
    HttpError(u16, String),
    #[error("encoding failed: {0}")]
    EncodingFailed(String),
    #[error("decode failed: {0}")]
    DecodeFailed(String),
    #[error("stream error: {0}")]
    StreamError(String),
    #[error("unknown: {0}")]
    Unknown(String),
}

// ─── Stream Callback Type Alias ─────────────────────────────────────────────

pub type StreamCallbackFn = Box<dyn FnMut(String) + Send>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_thinking_finds_thinking_block() {
        let content = "some text\n<think\nreasoning here\n</think\nmore text";
        let thinking = LlmClient::extract_thinking(content);
        assert_eq!(thinking, "reasoning here");
    }

    #[test]
    fn extract_thinking_returns_empty_when_none() {
        let content = "just normal text\nno thinking here";
        let thinking = LlmClient::extract_thinking(content);
        assert!(thinking.is_empty());
    }

    #[test]
    fn strip_thinking_removes_thinking_block() {
        let content = "hello\n<think\nreasoning\n</think\nworld";
        let cleaned = LlmClient::strip_thinking(content);
        assert!(cleaned.contains("hello"));
        assert!(cleaned.contains("world"));
        assert!(!cleaned.contains("reasoning"));
        assert!(!cleaned.contains("<think"));
    }

    #[test]
    fn strip_thinking_preserves_content_when_no_thinking() {
        let content = "just normal text";
        let cleaned = LlmClient::strip_thinking(content);
        assert_eq!(cleaned, "just normal text");
    }

    #[test]
    fn build_url_appends_chat_completions() {
        let client = LlmClient::new();
        let url = client.build_url("https://api.openai.com/v1");
        assert_eq!(url, "https://api.openai.com/v1/chat/completions");
    }

    #[test]
    fn build_url_handles_trailing_slash() {
        let client = LlmClient::new();
        let url = client.build_url("https://api.openai.com/v1/");
        assert_eq!(url, "https://api.openai.com/v1/chat/completions");
    }

    #[test]
    fn chat_request_serializes() {
        let request = ChatRequest {
            model: "gpt-4".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: "hello".to_string(),
            }],
            temperature: Some(0.7),
            max_tokens: None,
            stream: false,
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("gpt-4"));
        assert!(json.contains("hello"));
        assert!(json.contains("0.7"));
    }
}
