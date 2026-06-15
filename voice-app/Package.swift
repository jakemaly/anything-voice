// swift-tools-version: 5.9
//
// Anything Voice — Unified SwiftUI shell for dictation + meeting notes.
//
// Architecture:
//   VoiceApp (executable)
//   └── VoiceCoreBindings (UniFFI-generated Swift bindings)
//       └── VoiceCoreRS (prebuilt XCFramework wrapping voice-core Rust dylib)
//
// Before opening in Xcode, run ./build-rust.sh to build the Rust core
// and generate the XCFramework + Swift bindings.
//

import PackageDescription

let package = Package(
    name: "VoiceApp",
    platforms: [
        .macOS(.v14)
    ],
    dependencies: [
        // CoreML STT (Parakeet v2/v3/Flash)
        .package(url: "https://github.com/altic-dev/FluidAudio.git",
                 branch: "B/cohere-coreml-asr"),
        // Notch overlay for recording status
        .package(url: "https://github.com/altic-dev/DynamicNotchKit.git",
                 branch: "main"),
        // Auto-update via Sparkle
        .package(url: "https://github.com/mxcl/AppUpdater.git",
                 from: "1.0.0"),
    ],
    targets: [

        // ── Rust binary library (built by build-rust.sh) ─────────────────
        .binaryTarget(
            name: "VoiceCoreRS",
            path: "./voice-core.xcframework"
        ),

        // ── UniFFI-generated Swift bindings ──────────────────────────────
        // voice_core.swift does `import voice_coreFFI`, resolved via
        // the XCFramework's modulemap exposed by VoiceCoreRS.
        .target(
            name: "VoiceCoreBindings",
            dependencies: ["VoiceCoreRS"],
            path: "voice-core-generated"
        ),

        // ── Main application ────────────────────────────────────────────
        .executableTarget(
            name: "VoiceApp",
            dependencies: [
                .target(name: "VoiceCoreBindings"),
                .product(name: "FluidAudio", package: "FluidAudio"),
                .product(name: "DynamicNotchKit", package: "DynamicNotchKit"),
                .product(name: "AppUpdater", package: "AppUpdater"),
            ],
            path: "Sources/VoiceApp",
            linkerSettings: [
                .linkedFramework("AVFoundation"),
                .linkedFramework("CoreAudio"),
                .linkedFramework("CoreML"),
                .linkedFramework("Accelerate"),
                .linkedFramework("WebKit"),
                .linkedFramework("UniformTypeIdentifiers"),
            ]
        ),
    ]
)
