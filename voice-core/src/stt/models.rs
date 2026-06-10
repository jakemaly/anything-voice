/// Unified STT model definitions combining NR Log's model data with Fluid Voice's model list.
///
/// Each model has:
/// - A unique key (used everywhere as the model identifier)
/// - Display name for UI
/// - Description (typically download size)
/// - Download URL
/// - File name (what the downloaded file is named)
/// - Expected file size in bytes
/// - Whether it requires Apple Silicon (CoreML models)

use serde::{Deserialize, Serialize};

// ─── Model Definitions ───────────────────────────────────────────────────────

/// CoreML models (Apple Silicon only) — downloaded as directory bundles from Hugging Face
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CoreModel {
    /// Parakeet TDT v3 — multilingual, ~500MB, 25 languages (DEFAULT)
    ParakeetV3,
    /// Parakeet TDT v2 — English only, ~500MB, higher accuracy
    ParakeetV2,
    /// Parakeet Flash — English streaming, ~250MB
    ParakeetFlash,
}

/// Whisper.cpp models (universal, ggml format) — downloaded as single files
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WhisperModel {
    Tiny,
    TinyEn,
    Base,
    BaseEn,
    Small,
    SmallEn,
    LargeTurbo,
}

/// Unified model enum covering all available STT backends
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SttModel {
    Core(CoreModel),
    Whisper(WhisperModel),
}

// ─── Metadata ────────────────────────────────────────────────────────────────

impl SttModel {
    /// Unique key used across the entire system (matches UDL `model_key`)
    pub fn key(&self) -> &str {
        match self {
            SttModel::Core(CoreModel::ParakeetV3) => "parakeet-v3",
            SttModel::Core(CoreModel::ParakeetV2) => "parakeet-v2",
            SttModel::Core(CoreModel::ParakeetFlash) => "parakeet-flash",
            SttModel::Whisper(WhisperModel::Tiny) => "whisper-tiny",
            SttModel::Whisper(WhisperModel::TinyEn) => "whisper-tiny-en",
            SttModel::Whisper(WhisperModel::Base) => "whisper-base",
            SttModel::Whisper(WhisperModel::BaseEn) => "whisper-base-en",
            SttModel::Whisper(WhisperModel::Small) => "whisper-small",
            SttModel::Whisper(WhisperModel::SmallEn) => "whisper-small-en",
            SttModel::Whisper(WhisperModel::LargeTurbo) => "whisper-large-turbo",
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            SttModel::Core(CoreModel::ParakeetV3) => "Parakeet TDT v3 (Multilingual)",
            SttModel::Core(CoreModel::ParakeetV2) => "Parakeet TDT v2 (English)",
            SttModel::Core(CoreModel::ParakeetFlash) => "Parakeet Flash (Streaming)",
            SttModel::Whisper(WhisperModel::Tiny) => "Whisper Tiny",
            SttModel::Whisper(WhisperModel::TinyEn) => "Whisper Tiny (English)",
            SttModel::Whisper(WhisperModel::Base) => "Whisper Base",
            SttModel::Whisper(WhisperModel::BaseEn) => "Whisper Base (English)",
            SttModel::Whisper(WhisperModel::Small) => "Whisper Small",
            SttModel::Whisper(WhisperModel::SmallEn) => "Whisper Small (English)",
            SttModel::Whisper(WhisperModel::LargeTurbo) => "Whisper Large Turbo",
        }
    }

    pub fn description(&self) -> &str {
        match self {
            SttModel::Core(CoreModel::ParakeetV3) => "25 Languages, ~500 MB",
            SttModel::Core(CoreModel::ParakeetV2) => "English Only, ~500 MB",
            SttModel::Core(CoreModel::ParakeetFlash) => "English Streaming, ~250 MB",
            SttModel::Whisper(WhisperModel::Tiny) => "99 Languages, ~75 MB",
            SttModel::Whisper(WhisperModel::TinyEn) => "English Only, ~75 MB",
            SttModel::Whisper(WhisperModel::Base) => "99 Languages, ~142 MB",
            SttModel::Whisper(WhisperModel::BaseEn) => "English Only, ~142 MB",
            SttModel::Whisper(WhisperModel::Small) => "99 Languages, ~466 MB",
            SttModel::Whisper(WhisperModel::SmallEn) => "English Only, ~466 MB",
            SttModel::Whisper(WhisperModel::LargeTurbo) => "99 Languages, ~1.6 GB",
        }
    }

    pub fn size_bytes(&self) -> u64 {
        match self {
            SttModel::Core(CoreModel::ParakeetV3) => 500 * 1024 * 1024,
            SttModel::Core(CoreModel::ParakeetV2) => 500 * 1024 * 1024,
            SttModel::Core(CoreModel::ParakeetFlash) => 250 * 1024 * 1024,
            SttModel::Whisper(WhisperModel::Tiny) => 43_537_433,
            SttModel::Whisper(WhisperModel::TinyEn) => 43_550_795,
            SttModel::Whisper(WhisperModel::Base) => 81_768_585,
            SttModel::Whisper(WhisperModel::BaseEn) => 81_781_811,
            SttModel::Whisper(WhisperModel::Small) => 264_464_607,
            SttModel::Whisper(WhisperModel::SmallEn) => 264_477_561,
            SttModel::Whisper(WhisperModel::LargeTurbo) => 874_188_075,
        }
    }

    pub fn requires_apple_silicon(&self) -> bool {
        matches!(self, SttModel::Core(_))
    }

    /// Returns the download URL for this model.
    /// CoreML models download from Hugging Face.
    /// Whisper models download from the Hyprnote S3 mirror of whisper.cpp.
    pub fn download_url(&self) -> Option<&'static str> {
        match self {
            // CoreML models are downloaded via the HuggingFace directory downloader
            SttModel::Core(_) => None,
            SttModel::Whisper(WhisperModel::Tiny) => {
                Some("https://hyprnote.s3.us-east-1.amazonaws.com/v0/ggerganov/whisper.cpp/main/ggml-tiny-q8_0.bin")
            }
            SttModel::Whisper(WhisperModel::TinyEn) => {
                Some("https://hyprnote.s3.us-east-1.amazonaws.com/v0/ggerganov/whisper.cpp/main/ggml-tiny.en-q8_0.bin")
            }
            SttModel::Whisper(WhisperModel::Base) => {
                Some("https://hyprnote.s3.us-east-1.amazonaws.com/v0/ggerganov/whisper.cpp/main/ggml-base-q8_0.bin")
            }
            SttModel::Whisper(WhisperModel::BaseEn) => {
                Some("https://hyprnote.s3.us-east-1.amazonaws.com/v0/ggerganov/whisper.cpp/main/ggml-base.en-q8_0.bin")
            }
            SttModel::Whisper(WhisperModel::Small) => {
                Some("https://hyprnote.s3.us-east-1.amazonaws.com/v0/ggerganov/whisper.cpp/main/ggml-small-q8_0.bin")
            }
            SttModel::Whisper(WhisperModel::SmallEn) => {
                Some("https://hyprnote.s3.us-east-1.amazonaws.com/v0/ggerganov/whisper.cpp/main/ggml-small.en-q8_0.bin")
            }
            SttModel::Whisper(WhisperModel::LargeTurbo) => {
                Some("https://hyprnote.s3.us-east-1.amazonaws.com/v0/ggerganov/whisper.cpp/main/ggml-large-v3-turbo-q8_0.bin")
            }
        }
    }

    /// The file name the model is stored as on disk.
    pub fn file_name(&self) -> &str {
        match self {
            SttModel::Core(CoreModel::ParakeetV3) => "parakeet-tdt-0.6b-v3-coreml",
            SttModel::Core(CoreModel::ParakeetV2) => "parakeet-tdt-0.6b-v2-coreml",
            SttModel::Core(CoreModel::ParakeetFlash) => "parakeet-flash-coreml",
            SttModel::Whisper(WhisperModel::Tiny) => "ggml-tiny-q8_0.bin",
            SttModel::Whisper(WhisperModel::TinyEn) => "ggml-tiny.en-q8_0.bin",
            SttModel::Whisper(WhisperModel::Base) => "ggml-base-q8_0.bin",
            SttModel::Whisper(WhisperModel::BaseEn) => "ggml-base.en-q8_0.bin",
            SttModel::Whisper(WhisperModel::Small) => "ggml-small-q8_0.bin",
            SttModel::Whisper(WhisperModel::SmallEn) => "ggml-small.en-q8_0.bin",
            SttModel::Whisper(WhisperModel::LargeTurbo) => "ggml-large-v3-turbo-q8_0.bin",
        }
    }

    /// Returns all selectable models.
    pub fn all() -> &'static [SttModel] {
        &[
            SttModel::Core(CoreModel::ParakeetV3),
            SttModel::Core(CoreModel::ParakeetV2),
            SttModel::Core(CoreModel::ParakeetFlash),
            SttModel::Whisper(WhisperModel::Tiny),
            SttModel::Whisper(WhisperModel::TinyEn),
            SttModel::Whisper(WhisperModel::Base),
            SttModel::Whisper(WhisperModel::BaseEn),
            SttModel::Whisper(WhisperModel::Small),
            SttModel::Whisper(WhisperModel::SmallEn),
            SttModel::Whisper(WhisperModel::LargeTurbo),
        ]
    }

    /// Default model: Parakeet v3 on Apple Silicon, Whisper base on Intel.
    pub fn default_model() -> SttModel {
        #[cfg(target_arch = "aarch64")]
        {
            SttModel::Core(CoreModel::ParakeetV3)
        }
        #[cfg(not(target_arch = "aarch64"))]
        {
            SttModel::Whisper(WhisperModel::Base)
        }
    }

    /// Parse a model key string into an SttModel.
    pub fn from_key(key: &str) -> Option<SttModel> {
        Self::all().iter().find(|m| m.key() == key).copied()
    }
}

// ─── HuggingFace CoreML Model Repos ─────────────────────────────────────────

impl CoreModel {
    /// HuggingFace repo ID for the model (owner/repo)
    pub fn hf_repo_id(&self) -> &'static str {
        match self {
            CoreModel::ParakeetV3 => "FluidInference/parakeet-tdt-0.6b-v3-coreml",
            CoreModel::ParakeetV2 => "FluidInference/parakeet-tdt-0.6b-v2-coreml",
            CoreModel::ParakeetFlash => "FluidInference/parakeet-flash-coreml",
        }
    }

    /// Required files for this CoreML model (checked after download)
    pub fn required_files(&self) -> &'static [&'static str] {
        match self {
            CoreModel::ParakeetV3 => &[
                "Preprocessor.mlmodelc",
                "Encoder.mlmodelc",
                "Decoder.mlmodelc",
                "JointDecision.mlmodelc",
                "parakeet_v3_vocab.json",
            ],
            CoreModel::ParakeetV2 => &[
                "MelEncoder.mlmodelc",
                "Decoder.mlmodelc",
                "JointDecision.mlmodelc",
                "parakeet_v2_vocab.json",
            ],
            CoreModel::ParakeetFlash => &[
                "MelEncoder.mlmodelc",
                "Decoder.mlmodelc",
                "JointDecision.mlmodelc",
                "parakeet_flash_vocab.json",
            ],
        }
    }
}

// ─── UniFFI Dictionary Conversions ──────────────────────────────────────────

/// Convert SttModel to the UniFFI SttModelInfo dictionary
impl SttModel {
    pub fn to_uniffi_info(&self) -> uniffi::RustBuffer {
        let info = uniffi::RustBuffer::from_vec(
            serde_json::to_string(&serde_json::json!({
                "key": self.key(),
                "display_name": self.display_name(),
                "description": self.description(),
                "size_bytes": self.size_bytes() as i64,
                "requires_apple_silicon": self.requires_apple_silicon(),
                "download_size": self.description(),
            }))
            .unwrap()
            .into_bytes(),
        );
        info
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_models_have_unique_keys() {
        let keys: Vec<_> = SttModel::all().iter().map(|m| m.key()).collect();
        let unique: std::collections::HashSet<_> = keys.iter().collect();
        assert_eq!(keys.len(), unique.len(), "Duplicate model keys found");
    }

    #[test]
    fn from_key_roundtrips() {
        for model in SttModel::all() {
            let parsed = SttModel::from_key(model.key());
            assert_eq!(Some(*model), parsed, "Failed to roundtrip key for {:?}", model);
        }
    }

    #[test]
    fn default_model_is_valid() {
        let default = SttModel::default_model();
        assert!(
            SttModel::all().contains(&default),
            "Default model {:?} not in all()",
            default
        );
    }

    #[test]
    fn core_models_require_apple_silicon() {
        for model in SttModel::all() {
            if matches!(model, SttModel::Core(_)) {
                assert!(model.requires_apple_silicon());
            } else {
                assert!(!model.requires_apple_silicon());
            }
        }
    }

    #[test]
    fn whisper_models_have_download_urls() {
        for model in SttModel::all() {
            if matches!(model, SttModel::Whisper(_)) {
                assert!(
                    model.download_url().is_some(),
                    "Whisper model {:?} missing download URL",
                    model
                );
            }
        }
    }

    #[test]
    fn core_models_have_hf_repo() {
        for model in SttModel::all() {
            if let SttModel::Core(core) = model {
                let repo = core.hf_repo_id();
                assert!(!repo.is_empty(), "Core model {:?} missing HF repo", core);
                assert!(
                    repo.contains('/'),
                    "HF repo ID should contain '/'"
                );
            }
        }
    }
}
