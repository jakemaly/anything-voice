mod config;
mod intelligence;
mod server;
mod stt;

pub fn voice_hub_dir() -> String {
    config::paths::voice_hub_dir().to_string_lossy().to_string()
}

include!(concat!(env!("OUT_DIR"), "/voice_core.uniffi.rs"));
