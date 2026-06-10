fn main() {
    uniffi::generate_scaffolding("uniffi/voice_core.udl").expect("Failed to generate UniFFI scaffolding");
}
