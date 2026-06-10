use std::path::PathBuf;

/// Returns the path to the voice-hub directory (~/.voice-hub/)
/// Creates the directory and subdirectories if they don't exist.
pub fn voice_hub_dir() -> PathBuf {
    let dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".voice-hub");

    // Create directory structure if it doesn't exist
    std::fs::create_dir_all(&dir.join("models")).ok();

    dir
}

/// Returns the models cache directory path
pub fn models_dir() -> PathBuf {
    voice_hub_dir().join("models")
}

/// Returns the path to providers.json
pub fn providers_config_path() -> PathBuf {
    voice_hub_dir().join("providers.json")
}

/// Returns the path to settings.json
pub fn settings_config_path() -> PathBuf {
    voice_hub_dir().join("settings.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_hub_dir_exists() {
        let dir = voice_hub_dir();
        assert!(dir.exists(), "voice-hub dir should exist after calling voice_hub_dir()");
        assert!(dir.is_dir());
    }

    #[test]
    fn models_subdir_exists() {
        let models = models_dir();
        assert!(models.exists(), "models subdir should exist");
    }

    #[test]
    fn providers_config_path_is_correct() {
        let path = providers_config_path();
        assert!(path.to_string_lossy().ends_with(".voice-hub/providers.json"));
    }
}
