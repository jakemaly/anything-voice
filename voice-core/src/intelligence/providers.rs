/// Provider configuration management.
///
/// Reads/writes `~/.voice-hub/providers.json`.
/// Shared between Swift UI and Rust backend.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::config::paths;

// ─── Data Types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderEntry {
    pub base_url: String,
    pub api_key: String,
    #[serde(default)]
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub selected_provider: String,
    #[serde(default)]
    pub selected_model: String,
    #[serde(default)]
    pub providers: HashMap<String, ProviderEntry>,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            selected_provider: "groq".to_string(),
            selected_model: "llama-3.3-70b-versatile".to_string(),
            providers: Default::default(),
        }
    }
}

// ─── Default Provider URLs ──────────────────────────────────────────────────

impl ProviderConfig {
    /// Returns the default base URL for a built-in provider.
    pub fn default_base_url(provider_id: &str) -> Option<&'static str> {
        match provider_id {
            "openai" => Some("https://api.openai.com/v1"),
            "anthropic" => Some("https://api.anthropic.com/v1"),
            "xai" => Some("https://api.x.ai/v1"),
            "groq" => Some("https://api.groq.com/openai/v1"),
            "cerebras" => Some("https://api.cerebras.ai/v1"),
            "google" => Some("https://generativelanguage.googleapis.com/v1beta/openai"),
            "openrouter" => Some("https://openrouter.ai/api/v1"),
            "ollama" => Some("http://localhost:11434/v1"),
            "lmstudio" => Some("http://localhost:1234/v1"),
            _ => None,
        }
    }

    /// Returns the default model for a built-in provider.
    pub fn default_model(provider_id: &str) -> Option<&'static str> {
        match provider_id {
            "openai" => Some("gpt-4.1"),
            "anthropic" => Some("claude-sonnet-4-20250514"),
            "xai" => Some("grok-3-fast"),
            "groq" => Some("llama-3.3-70b-versatile"),
            "cerebras" => Some("gpt-oss-120b"),
            "google" => Some("gemini-2.5-flash"),
            "openrouter" => Some("openai/gpt-oss-20b"),
            _ => None,
        }
    }

    /// Built-in provider IDs.
    pub fn built_in_providers() -> &'static [&'static str] {
        &[
            "openai",
            "anthropic",
            "xai",
            "groq",
            "cerebras",
            "google",
            "openrouter",
            "ollama",
            "lmstudio",
        ]
    }
}

// ─── SHA256 Fingerprint ─────────────────────────────────────────────────────

impl ProviderConfig {
    /// Compute a SHA256 fingerprint for a provider's credentials.
    /// Used to verify that stored credentials haven't changed.
    pub fn compute_fingerprint(base_url: &str, api_key: &str) -> String {
        use sha2::{Digest, Sha256};
        let input = format!("{}|{}", base_url.trim(), api_key.trim());
        let hash = Sha256::digest(input.as_bytes());
        format!("sha256:{}", hex::encode(hash))
    }
}

// ─── Read/Write ─────────────────────────────────────────────────────────────

impl ProviderConfig {
    /// Read provider config from disk. Returns default if file doesn't exist.
    pub fn load() -> Result<Self, ConfigError> {
        let path = paths::providers_config_path();
        if !path.exists() {
            // Write default config
            let default = Self::default();
            default.save()?;
            return Ok(default);
        }

        let content = std::fs::read_to_string(&path)
            .map_err(|e| ConfigError::ReadFailed(e.to_string()))?;

        serde_json::from_str(&content)
            .map_err(|e| ConfigError::ParseFailed(e.to_string()))
    }

    /// Save provider config to disk.
    pub fn save(&self) -> Result<(), ConfigError> {
        let path = paths::providers_config_path();
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| ConfigError::WriteFailed(e.to_string()))?;

        std::fs::write(&path, json)
            .map_err(|e| ConfigError::WriteFailed(e.to_string()))?;

        Ok(())
    }
}

// ─── Errors ─────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("read failed: {0}")]
    ReadFailed(String),
    #[error("write failed: {0}")]
    WriteFailed(String),
    #[error("parse failed: {0}")]
    ParseFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_groq_selected() {
        let config = ProviderConfig::default();
        assert_eq!(config.selected_provider, "groq");
    }

    #[test]
    fn default_base_urls() {
        assert_eq!(
            ProviderConfig::default_base_url("openai"),
            Some("https://api.openai.com/v1")
        );
        assert_eq!(
            ProviderConfig::default_base_url("groq"),
            Some("https://api.groq.com/openai/v1")
        );
        assert_eq!(ProviderConfig::default_base_url("unknown"), None);
    }

    #[test]
    fn built_in_providers_list() {
        let providers = ProviderConfig::built_in_providers();
        assert!(providers.contains(&"groq"));
        assert!(providers.contains(&"openai"));
        assert!(providers.contains(&"ollama"));
    }

    #[test]
    fn fingerprint_is_deterministic() {
        let fp1 = ProviderConfig::compute_fingerprint("https://api.openai.com/v1", "sk-test");
        let fp2 = ProviderConfig::compute_fingerprint("https://api.openai.com/v1", "sk-test");
        assert_eq!(fp1, fp2);
        assert!(fp1.starts_with("sha256:"));
    }

    #[test]
    fn fingerprint_differs_for_different_keys() {
        let fp1 = ProviderConfig::compute_fingerprint("https://api.openai.com/v1", "sk-test1");
        let fp2 = ProviderConfig::compute_fingerprint("https://api.openai.com/v1", "sk-test2");
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn load_creates_default_if_missing() {
        // This test creates the real ~/.voice-hub dir, which is fine
        let config = ProviderConfig::load();
        assert!(config.is_ok());
    }

    #[test]
    fn save_and_load_roundtrips() {
        let mut config = ProviderConfig::default();
        config.selected_provider = "openai".to_string();
        config.selected_model = "gpt-4".to_string();
        config.save().unwrap();

        let loaded = ProviderConfig::load().unwrap();
        assert_eq!(loaded.selected_provider, "openai");
        assert_eq!(loaded.selected_model, "gpt-4");
    }
}
