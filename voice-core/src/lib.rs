mod config;
mod intelligence;
mod server;
mod stt;

include!(concat!(env!("OUT_DIR"), "voice_core.uniffi.rs"));
