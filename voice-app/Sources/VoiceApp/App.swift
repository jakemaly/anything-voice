import SwiftUI
import AppKit

/// Entry point for the Anything Voice app.
@main
struct VoiceApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) var appDelegate

    var body: some Scene {
        WindowGroup {
            ContentView()
                .frame(minWidth: 420, minHeight: 320)
                .onAppear { verifyRustCore() }
        }
        .windowStyle(.hiddenTitleBar)
        .defaultSize(width: 480, height: 420)
    }

    /// Smoke-test the Rust FFI on first launch.
    private func verifyRustCore() {
        let hubPath = voiceHubDir()
        NSLog("[voice] Rust core linked — voice-hub: %@", hubPath)

        let models = listAvailableModels()
        NSLog("[voice] available STT models: %d", models.count)
        for m in models.prefix(3) {
            NSLog("[voice]   • %@ (%@)", m.displayName, m.key)
        }
    }
}

// ── Minimal content view (placeholder) ─────────────────────────────────────

struct ContentView: View {
    @State private var rustStatus = "Checking Rust core…"

    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: "waveform.circle")
                .font(.system(size: 48))
                .foregroundColor(.accentColor)

            Text("Anything Voice")
                .font(.title)

            Text(rustStatus)
                .font(.caption)
                .foregroundColor(.secondary)

            Divider()

            VStack(alignment: .leading, spacing: 8) {
                Label("Hotkey → speak → release — text at cursor",
                      systemImage: "keyboard")
                Label("Meeting Notes — full transcripts + AI summaries",
                      systemImage: "doc.text.magnifyingglass")
            }
            .font(.subheadline)
            .padding(.horizontal)
        }
        .padding()
        .onAppear {
            let hub = voiceHubDir()
            rustStatus = "Rust core loaded\n\(hub)"
        }
    }
}
