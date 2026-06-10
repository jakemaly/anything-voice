use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    uniffi::generate_scaffolding("uniffi/voice_core.udl")
        .expect("Failed to generate UniFFI scaffolding");
    // The generated file is placed in OUT_DIR by uniffi_build
    // Verify it exists
    let generated = out_dir.join("voice_core.uniffi.rs");
    if !generated.exists() {
        // Try alternative naming
        let alt = out_dir.join("voice_core.uniffi.uniffi.rs");
        if alt.exists() {
            println!("cargo:warning=UniFFI generated file at alternative path");
        }
    }
}
