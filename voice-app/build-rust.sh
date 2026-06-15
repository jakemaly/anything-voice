#!/usr/bin/env bash
#
# Build voice-core Rust library and generate UniFFI Swift bindings.
#
# Run this every time you change Rust code in voice-core/.
# Must be run on macOS (produces .dylib + XCFramework for Xcode).
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CORE_DIR="$SCRIPT_DIR/../voice-core"
GENERATED_DIR="$SCRIPT_DIR/voice-core-generated"
XCFRAMEWORK_DIR="$SCRIPT_DIR/voice-core.xcframework"

cd "$CORE_DIR"

# ── 1. Build the Rust library ────────────────────────────────────────────────

echo "🦀 Building voice-core (release)..."
cargo build --release

DYLIB="target/release/libvoice_core.dylib"
if [ ! -f "$DYLIB" ]; then
    echo "❌ Expected dylib not found at $DYLIB"
    echo "   Is this running on macOS? (Linux produces .so, not .dylib)"
    exit 1
fi

# ── 2. Generate Swift bindings from the UDL ─────────────────────────────────

echo "🔗 Generating Swift bindings..."
mkdir -p "$GENERATED_DIR"

cargo run -q --bin uniffi-bindgen --features uniffi/cli -- \
    generate uniffi/voice_core.udl \
    --language swift \
    --out-dir "$GENERATED_DIR"

# ── 3. Create XCFramework for SwiftPM consumption ───────────────────────────

echo "📦 Creating XCFramework..."
rm -rf "$XCFRAMEWORK_DIR"

# The modulemap must be named module.modulemap for XCFramework
MODULEMAP_SRC="$GENERATED_DIR/voice_coreFFI.modulemap"
MODULEMAP_DST="$GENERATED_DIR/module.modulemap"
cp "$MODULEMAP_SRC" "$MODULEMAP_DST"

xcodebuild -create-xcframework \
    -library "$DYLIB" \
    -headers "$GENERATED_DIR" \
    -output "$XCFRAMEWORK_DIR"

# Restore the original modulemap name (the XCFramework has its own copy)
rm -f "$MODULEMAP_DST"

# ── 4. Verify outputs ───────────────────────────────────────────────────────

echo ""
echo "✅ Build complete!"
echo ""
echo "Generated bindings:"
ls -lh "$GENERATED_DIR"
echo ""
echo "XCFramework:"
ls -lh "$XCFRAMEWORK_DIR"
echo ""
echo "Next: Open voice-app/Package.swift in Xcode and build."
