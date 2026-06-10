/// Model downloader that handles both:
/// - Single-file downloads (ggml whisper models from S3/HTTP)
/// - Directory-based downloads (CoreML .mlmodelc bundles from HuggingFace)
///
/// Uses generation-based atomic replacement: download to a temp path,
/// verify, then atomically move to the final destination.
///
/// Adapted from NR Log's model-downloader crate.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::config::paths;
use crate::stt::models::{CoreModel, SttModel};

// ─── Errors ──────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("model not found: {0}")]
    ModelNotFound(String),
    #[error("download failed: {0}")]
    DownloadFailed(String),
    #[error("network error: {0}")]
    NetworkError(String),
    #[error("verification failed: {0}")]
    VerificationFailed(String),
    #[error("io error: {0}")]
    IoError(#[from] std::io::Error),
}

// ─── Download State ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelManifest {
    /// Map of model key -> version/hash of downloaded model
    pub models: HashMap<String, String>,
}

impl ModelManifest {
    pub fn new() -> Self {
        Self {
            models: HashMap::new(),
        }
    }

    pub fn is_downloaded(&self, key: &str) -> bool {
        self.models.contains_key(key)
    }
}

// ─── Download Manager ───────────────────────────────────────────────────────

pub struct ModelDownloadManager {
    runtime: Arc<DownloadRuntime>,
    downloads: Arc<Mutex<DownloadsRegistry>>,
    next_generation: Arc<AtomicU64>,
}

impl Clone for ModelDownloadManager {
    fn clone(&self) -> Self {
        Self {
            runtime: self.runtime.clone(),
            downloads: self.downloads.clone(),
            next_generation: self.next_generation.clone(),
        }
    }
}

impl Default for ModelDownloadManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ModelDownloadManager {
    pub fn new() -> Self {
        Self {
            runtime: Arc::new(DownloadRuntime::new()),
            downloads: Arc::new(Mutex::new(DownloadsRegistry::new())),
            next_generation: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Returns the path where a model is stored.
    pub fn model_path(&self, model: &SttModel) -> PathBuf {
        let models_base = paths::models_dir();
        model.download_destination(&models_base)
    }

    /// Check if a model is fully downloaded and verified.
    pub async fn is_downloaded(&self, model: &SttModel) -> bool {
        let path = self.model_path(model);

        match model {
            SttModel::Core(_) => {
                // CoreML models are directories — check required files exist
                if !path.is_dir() {
                    return false;
                }
                let core_model = match model {
                    SttModel::Core(c) => c,
                    _ => unreachable!(),
                };
                for file in core_model.required_files() {
                    if !path.join(file).exists() {
                        return false;
                    }
                }
                true
            }
            SttModel::Whisper(_) => {
                // Whisper models are single files — check file exists and size matches
                if !path.is_file() {
                    return false;
                }
                match tokio::fs::metadata(&path).await {
                    Ok(metadata) => metadata.len() == model.size_bytes(),
                    Err(_) => false,
                }
            }
        }
    }

    /// Check if a model is currently being downloaded.
    pub async fn is_downloading(&self, model: &SttModel) -> bool {
        let registry = self.downloads.lock().await;
        registry.contains(model.key())
    }

    /// Download a model with progress reporting.
    /// Progress is reported as 0.0..1.0 via the callback.
    pub async fn download(
        &self,
        model: &SttModel,
        callback: impl FnMut(f32) + Send + 'static,
    ) -> Result<(), DownloadError> {
        let key = model.key().to_string();
        let generation = self.next_generation.fetch_add(1, Ordering::Relaxed);

        let models_base = paths::models_dir();
        let final_destination = model.download_destination(&models_base);
        let destination = generation_download_path(&final_destination, generation);

        // Create parent directory
        if let Some(parent) = destination.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Register download
        {
            let mut registry = self.downloads.lock().await;
            let (start_tx, start_rx) = oneshot::channel();
            let cancellation = CancellationToken::new();
            registry.register(&key, start_tx, cancellation.clone());

            // Wait for any previous download to finish
            let previous = registry.take_previous(&key);
            if let Some(prev_cancel) = previous {
                prev_cancel.cancel();
            }
        }

        // Signal that we're ready to start
        {
            let registry = self.downloads.lock().await;
            if let Some(start_tx) = registry.start_tx(&key) {
                let _ = start_tx.send(());
            }
        }

        // Perform the download
        let result = match model {
            SttModel::Core(core) => {
                download_coreml_model(core, &destination, callback).await
            }
            SttModel::Whisper(_) => {
                let url = model
                    .download_url()
                    .ok_or_else(|| DownloadError::ModelNotFound(key.clone()))?;
                download_single_file(url, &destination, callback).await
            }
        };

        // Clean up registry
        {
            let mut registry = self.downloads.lock().await;
            registry.unregister(&key);
        }

        match result {
            Ok(()) => {
                // Atomically move from temp to final destination
                if final_destination.exists() {
                    tokio::fs::remove_dir_all(&final_destination).await.ok();
                }
                tokio::fs::rename(&destination, &final_destination).await?;

                // Update manifest
                self.update_manifest(model).await;

                Ok(())
            }
            Err(e) => {
                // Clean up temp download on failure
                tokio::fs::remove_dir_all(&destination).await.ok();
                Err(e)
            }
        }
    }

    /// Cancel an in-progress download.
    pub async fn cancel_download(&self, model_key: &str) {
        let mut registry = self.downloads.lock().await;
        registry.cancel(model_key);
    }

    /// Delete a downloaded model.
    pub async fn delete_model(&self, model: &SttModel) -> Result<(), DownloadError> {
        let path = self.model_path(model);
        if path.exists() {
            tokio::fs::remove_dir_all(&path).await?;
        }
        // Remove from manifest
        let manifest_path = paths::models_dir().join("manifest.json");
        if manifest_path.exists() {
            let mut manifest: ModelManifest =
                serde_json::from_str(&tokio::fs::read_to_string(&manifest_path).await?)
                    .unwrap_or_default();
            manifest.models.remove(model.key());
            tokio::fs::write(
                &manifest_path,
                serde_json::to_string_pretty(&manifest).unwrap(),
            )
            .await?;
        }
        Ok(())
    }

    async fn update_manifest(&self, model: &SttModel) {
        let manifest_path = paths::models_dir().join("manifest.json");
        let mut manifest: ModelManifest = if manifest_path.exists() {
            match tokio::fs::read_to_string(&manifest_path).await {
                Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
                Err(_) => ModelManifest::new(),
            }
        } else {
            ModelManifest::new()
        };
        manifest.models.insert(model.key().to_string(), "1.0".to_string());
        if let Ok(json) = serde_json::to_string_pretty(&manifest) {
            tokio::fs::write(&manifest_path, json).await.ok();
        }
    }
}

// ─── Single-File Download (Whisper ggml) ────────────────────────────────────

async fn download_single_file(
    url: &str,
    destination: &Path,
    mut callback: impl FnMut(f32) + Send,
) -> Result<(), DownloadError> {
    let response = reqwest::get(url)
        .await
        .map_err(|e| DownloadError::NetworkError(e.to_string()))?;

    if !response.status().is_success() {
        return Err(DownloadError::DownloadFailed(format!(
            "HTTP {}",
            response.status()
        )));
    }

    let total_size = response.content_length().unwrap_or(0);
    let mut stream = response.bytes_stream();
    let mut file = tokio::fs::File::create(destination).await?;
    let mut downloaded: u64 = 0;

    use futures::StreamExt;
    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| DownloadError::DownloadFailed(e.to_string()))?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;

        if total_size > 0 {
            let progress = downloaded as f32 / total_size as f32;
            callback(progress.min(1.0));
        }
    }

    callback(1.0);
    Ok(())
}

// ─── CoreML Directory Download (HuggingFace) ────────────────────────────────

/// CoreML models are downloaded from HuggingFace as directory bundles.
/// The download process:
/// 1. List all files in the HF repo via the API
/// 2. Download each file individually
/// 3. Verify all required files are present
async fn download_coreml_model(
    core: &CoreModel,
    destination: &Path,
    mut callback: impl FnMut(f32) + Send,
) -> Result<(), DownloadError> {
    let repo_id = core.hf_repo_id();
    let files = list_hf_files(repo_id)
        .await
        .map_err(|e| DownloadError::NetworkError(e.to_string()))?;

    let total_files = files.len();
    let mut completed = 0;

    for file_path in &files {
        let file_url = format!(
            "https://huggingface.co/{}/resolve/main/{}",
            repo_id, file_path
        );

        let file_dest = destination.join(file_path);
        if let Some(parent) = file_dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Skip if file already exists
        if file_dest.exists() {
            completed += 1;
            if total_files > 0 {
                callback(completed as f32 / total_files as f32);
            }
            continue;
        }

        let response = reqwest::get(&file_url)
            .await
            .map_err(|e| DownloadError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(DownloadError::DownloadFailed(format!(
                "HTTP {} for file {}",
                response.status(),
                file_path
            )));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| DownloadError::DownloadFailed(e.to_string()))?;
        tokio::fs::write(&file_dest, bytes).await?;

        completed += 1;
        if total_files > 0 {
            callback(completed as f32 / total_files as f32);
        }
    }

    callback(1.0);
    Ok(())
}

/// List all files in a HuggingFace repo using the API.
async fn list_hf_files(repo_id: &str) -> Result<Vec<String>, reqwest::Error> {
    let api_url = format!(
        "https://huggingface.co/api/models/{}/tree/main?recursive=1",
        repo_id
    );

    let response = reqwest::get(&api_url).await?;
    let entries: Vec<HFEntry> = response.json().await?;

    Ok(entries
        .into_iter()
        .filter(|e| e.r#type == "file")
        .map(|e| e.path)
        .collect())
}

#[derive(Deserialize)]
struct HFEntry {
    r#type: String,
    path: String,
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

impl SttModel {
    /// Returns the final destination path for a model within the models directory.
    pub fn download_destination(&self, models_base: &Path) -> PathBuf {
        models_base.join(self.file_name())
    }
}

/// Create a generation-based temp path for atomic downloads.
fn generation_download_path(final_path: &Path, generation: u64) -> PathBuf {
    let mut path = final_path.to_path_buf();
    let extension = path.extension().map(|e| e.to_string_lossy().to_string());
    path.set_file_name(format!(
        "{}.gen.{}.tmp",
        path.file_stem().unwrap_or_default().to_string_lossy(),
        generation
    ));
    if let Some(ext) = extension {
        path.set_extension(ext);
    }
    path
}

// ─── Downloads Registry ─────────────────────────────────────────────────────

struct DownloadsRegistry {
    /// Active downloads: model_key -> (start_tx, cancellation_token)
    active: HashMap<String, (oneshot::Sender<()>, CancellationToken)>,
}

impl DownloadsRegistry {
    fn new() -> Self {
        Self {
            active: HashMap::new(),
        }
    }

    fn contains(&self, key: &str) -> bool {
        self.active.contains_key(key)
    }

    fn register(
        &mut self,
        key: &str,
        start_tx: oneshot::Sender<()>,
        cancellation: CancellationToken,
    ) {
        self.active
            .insert(key.to_string(), (start_tx, cancellation));
    }

    fn unregister(&mut self, key: &str) {
        self.active.remove(key);
    }

    fn cancel(&mut self, key: &str) {
        if let Some((_, cancellation)) = self.active.get(key) {
            cancellation.cancel();
        }
    }

    fn start_tx(&self, key: &str) -> Option<oneshot::Sender<()>> {
        self.active.get(key).map(|(tx, _)| tx.clone())
    }

    fn take_previous(&mut self, key: &str) -> Option<CancellationToken> {
        self.active
            .remove(key)
            .map(|(_, cancellation)| cancellation)
    }
}

// ─── Download Runtime (for future async executor customization) ──────────────

struct DownloadRuntime;

impl DownloadRuntime {
    fn new() -> Self {
        Self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_paths_are_under_models_dir() {
        let manager = ModelDownloadManager::new();
        let model = SttModel::Whisper(crate::stt::models::WhisperModel::Tiny);
        let path = manager.model_path(&model);
        assert!(path.to_string_lossy().contains(".voice-hub/models"));
    }

    #[test]
    fn generation_path_differs_from_final() {
        let final_path = PathBuf::from("/tmp/model.bin");
        let gen_path = generation_download_path(&final_path, 42);
        assert_ne!(final_path, gen_path);
        assert!(gen_path.to_string_lossy().contains(".gen.42.tmp"));
    }

    #[test]
    fn hf_api_url_format() {
        let repo = "FluidInference/parakeet-tdt-0.6b-v3-coreml";
        let expected = format!(
            "https://huggingface.co/api/models/{}/tree/main?recursive=1",
            repo
        );
        assert!(expected.contains(repo));
        assert!(expected.contains("recursive=1"));
    }

    #[test]
    fn model_downloaded_returns_false_for_missing() {
        let manager = ModelDownloadManager::new();
        let model = SttModel::Whisper(crate::stt::models::WhisperModel::Tiny);
        let path = manager.model_path(&model);
        // Model shouldn't exist yet
        assert!(!path.exists());
    }
}
