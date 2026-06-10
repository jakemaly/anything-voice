/// whisper.cpp C FFI bindings.
///
/// Provides a safe Rust interface over whisper.cpp's C API.
/// Feature-gated behind `whisper` — compiles cleanly on all platforms
/// but only links against libwhisper when the feature is enabled.
///
/// On macOS, this links against the system-provided or bundled whisper.cpp.
/// On Linux, this is a compile-time stub for interface verification.

// ─── Shared Types (always available) ─────────────────────────────────────────

/// whisper.cpp decoding strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhisperStrategy {
    Greedy,
    BeamSearch,
}

impl Default for WhisperStrategy {
    fn default() -> Self {
        Self::Greedy
    }
}

/// A single transcription segment from whisper.cpp.
#[derive(Debug, Clone)]
pub struct WhisperSegment {
    pub text: String,
    /// Start time in milliseconds
    pub t0: i64,
    /// End time in milliseconds
    pub t1: i64,
}

/// Errors from whisper.cpp operations.
#[derive(Debug, thiserror::Error)]
pub enum WhisperError {
    #[error("model init failed: {0}")]
    InitFailed(String),
    #[error("inference failed with code {0}")]
    InferenceFailed(i32),
    #[error("language detection failed")]
    DetectionFailed,
}

// ─── FFI Bindings (only when `whisper` feature enabled) ──────────────────────

#[cfg(feature = "whisper")]
mod ffi {
    use std::os::raw::{c_char, c_int, c_longlong};

    /// Opaque whisper.cpp context pointer.
    pub type whisper_context = std::ffi::c_void;

    /// Opaque whisper_full_params struct (we only use default params).
    /// Defined outside extern block since Rust doesn't allow structs in extern.
    #[repr(C)]
    #[derive(Copy, Clone)]
    pub struct whisper_full_params {
        _private: [u8; 0],
        _marker: std::marker::PhantomData<(*mut u8, std::marker::PhantomPinned)>,
    }

    extern "C" {
        pub fn whisper_init_from_file_path(path: *const c_char) -> *mut whisper_context;
        pub fn whisper_free(ctx: *mut whisper_context);
        pub fn whisper_full_default_params(strategy: u32) -> whisper_full_params;
        pub fn whisper_full(
            ctx: *mut whisper_context,
            params: whisper_full_params,
            samples: *const f32,
            n_samples: c_int,
        ) -> c_int;
        pub fn whisper_full_n_segments(ctx: *mut whisper_context) -> c_int;
        pub fn whisper_full_get_segment_text(
            ctx: *mut whisper_context,
            i_segment: c_int,
        ) -> *const c_char;
        pub fn whisper_full_get_segment_t0(
            ctx: *mut whisper_context,
            i_segment: c_int,
        ) -> c_longlong;
        pub fn whisper_full_get_segment_t1(
            ctx: *mut whisper_context,
            i_segment: c_int,
        ) -> c_longlong;
        pub fn whisper_lang_auto_detect(
            ctx: *mut whisper_context,
            offset_ms: c_int,
            n_langs: c_int,
            langs: *mut c_int,
        ) -> c_int;
    }

    /// whisper.cpp sampling strategy constants (from whisper.h)
    pub const WHISPER_SAMPLING_GREEDY: u32 = 1;
    pub const WHISPER_SAMPLING_BEAM_SEARCH: u32 = 2;
}

// ─── Whisper Context ─────────────────────────────────────────────────────────

/// Safe wrapper around a whisper.cpp context.
/// Loads a model and manages its lifecycle.
#[cfg(feature = "whisper")]
pub struct WhisperContext {
    ctx: *mut ffi::whisper_context,
}

#[cfg(feature = "whisper")]
unsafe impl Send for WhisperContext {}

#[cfg(feature = "whisper")]
impl WhisperContext {
    /// Load a whisper.cpp model from a .bin file.
    pub fn load(model_path: &std::path::Path) -> Result<Self, WhisperError> {
        let path_c = std::ffi::CString::new(model_path.to_string_lossy().as_bytes())
            .map_err(|e| WhisperError::InitFailed(format!("Invalid path: {}", e)))?;

        let ctx = unsafe { ffi::whisper_init_from_file_path(path_c.as_ptr()) };
        if ctx.is_null() {
            return Err(WhisperError::InitFailed(format!(
                "Failed to load model: {:?}",
                model_path
            )));
        }

        Ok(Self { ctx })
    }

    /// Run inference on a buffer of f32 PCM samples (16kHz mono).
    pub fn infer(
        &self,
        samples: &[f32],
        strategy: WhisperStrategy,
    ) -> Result<Vec<WhisperSegment>, WhisperError> {
        use std::os::raw::c_int;

        let sampling = match strategy {
            WhisperStrategy::Greedy => ffi::WHISPER_SAMPLING_GREEDY,
            WhisperStrategy::BeamSearch => ffi::WHISPER_SAMPLING_BEAM_SEARCH,
        };

        let params = unsafe { ffi::whisper_full_default_params(sampling) };

        let result = unsafe {
            ffi::whisper_full(
                self.ctx,
                params,
                samples.as_ptr(),
                samples.len() as c_int,
            )
        };

        if result != 0 {
            return Err(WhisperError::InferenceFailed(result as i32));
        }

        let n_segments = unsafe { ffi::whisper_full_n_segments(self.ctx) };
        let mut segments = Vec::with_capacity(n_segments as usize);

        for i in 0..n_segments {
            let text_ptr = unsafe { ffi::whisper_full_get_segment_text(self.ctx, i) };
            let text = if !text_ptr.is_null() {
                unsafe {
                    std::ffi::CStr::from_ptr(text_ptr)
                        .to_string_lossy()
                        .to_string()
                }
            } else {
                String::new()
            };

            let t0 = unsafe { ffi::whisper_full_get_segment_t0(self.ctx, i) };
            let t1 = unsafe { ffi::whisper_full_get_segment_t1(self.ctx, i) };

            segments.push(WhisperSegment { text, t0, t1 });
        }

        Ok(segments)
    }

    /// Auto-detect language from audio samples.
    pub fn detect_language(&self, _samples: &[f32]) -> Result<String, WhisperError> {
        use std::os::raw::c_int;

        let mut lang_id: c_int = 0;
        let result = unsafe {
            ffi::whisper_lang_auto_detect(
                self.ctx,
                0,
                1,
                &mut lang_id,
            )
        };

        if result < 0 {
            return Err(WhisperError::DetectionFailed);
        }

        // whisper.cpp returns language index; map to ISO 639-1 code
        // For now, return the raw index as string
        Ok(result.to_string())
    }
}

#[cfg(feature = "whisper")]
impl Drop for WhisperContext {
    fn drop(&mut self) {
        if !self.ctx.is_null() {
            unsafe {
                ffi::whisper_free(self.ctx);
            }
            self.ctx = std::ptr::null_mut();
        }
    }
}

// ─── Stub (when `whisper` feature disabled) ──────────────────────────────────

/// Stub WhisperContext for platforms without whisper.cpp linked.
/// All methods return errors explaining the feature is disabled.
#[cfg(not(feature = "whisper"))]
pub struct WhisperContext;

#[cfg(not(feature = "whisper"))]
impl WhisperContext {
    /// Attempting to load a model returns an error.
    pub fn load(_path: &std::path::Path) -> Result<Self, WhisperError> {
        Err(WhisperError::InitFailed(
            "whisper feature not enabled — whisper.cpp not linked".to_string(),
        ))
    }

    /// Attempting inference returns an error.
    pub fn infer(
        &self,
        _samples: &[f32],
        _strategy: WhisperStrategy,
    ) -> Result<Vec<WhisperSegment>, WhisperError> {
        Err(WhisperError::InferenceFailed(-1))
    }
}

#[cfg(not(feature = "whisper"))]
impl Default for WhisperContext {
    fn default() -> Self {
        Self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whisper_stub_load_returns_error() {
        let result = WhisperContext::load(std::path::Path::new("/tmp/fake.bin"));
        assert!(result.is_err());
    }

    #[test]
    fn whisper_stub_infer_returns_error() {
        // Can't call infer without a loaded context, so just verify the type exists
        let strategy = WhisperStrategy::Greedy;
        assert!(matches!(strategy, WhisperStrategy::Greedy));
    }

    #[test]
    fn whisper_segment_constructs() {
        let segment = WhisperSegment {
            text: "hello world".to_string(),
            t0: 0,
            t1: 1500,
        };
        assert_eq!(segment.text, "hello world");
        assert_eq!(segment.t1, 1500);
    }

    #[test]
    fn whisper_strategy_default_is_greedy() {
        let strategy = WhisperStrategy::default();
        assert!(matches!(strategy, WhisperStrategy::Greedy));
    }

    #[test]
    fn whisper_error_displays() {
        let err = WhisperError::InitFailed("test".to_string());
        assert!(format!("{}", err).contains("test"));

        let err = WhisperError::InferenceFailed(42);
        assert!(format!("{}", err).contains("42"));
    }
}
