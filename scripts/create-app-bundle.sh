#!/bin/bash
# Create a macOS .app bundle for Audio Separator
# Usage: ./scripts/create-app-bundle.sh [path-to-binary] [path-to-dylib]
#
# If no binary path is given, defaults to target/release/audio-separator
# If no dylib path is given, searches common locations

set -euo pipefail

BINARY="${1:-target/release/audio-separator}"
APP_NAME="Audio Separator"
APP_BUNDLE="${APP_NAME}.app"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

if [ ! -f "$BINARY" ]; then
    echo "Error: Binary not found at $BINARY"
    echo "Build first with: cargo build --release"
    exit 1
fi

# Find libonnxruntime.dylib
DYLIB="${2:-}"
if [ -z "$DYLIB" ]; then
    for candidate in \
        /opt/homebrew/lib/libonnxruntime.dylib \
        /usr/local/lib/libonnxruntime.dylib \
        "$(dirname "$BINARY")/libonnxruntime.dylib"; do
        if [ -f "$candidate" ]; then
            DYLIB="$candidate"
            break
        fi
    done
fi

if [ -z "$DYLIB" ] || [ ! -f "$DYLIB" ]; then
    echo "Error: libonnxruntime.dylib not found"
    echo "Install with: brew install onnxruntime"
    echo "Or pass the path: $0 $BINARY /path/to/libonnxruntime.dylib"
    exit 1
fi

echo "Creating .app bundle..."
echo "  Binary: $BINARY"
echo "  Dylib:  $DYLIB"

# Clean previous bundle
rm -rf "${APP_BUNDLE}"

# Create bundle structure
mkdir -p "${APP_BUNDLE}/Contents/MacOS"
mkdir -p "${APP_BUNDLE}/Contents/Frameworks"
mkdir -p "${APP_BUNDLE}/Contents/Resources"

# Copy binary
cp "$BINARY" "${APP_BUNDLE}/Contents/MacOS/audio-separator"
chmod +x "${APP_BUNDLE}/Contents/MacOS/audio-separator"

# Copy ONNX Runtime dylib (resolve symlinks to get the actual file)
cp -L "$DYLIB" "${APP_BUNDLE}/Contents/Frameworks/libonnxruntime.dylib"

# Copy Info.plist
cp "${PROJECT_DIR}/macos/Info.plist" "${APP_BUNDLE}/Contents/Info.plist"

echo "Created: ${APP_BUNDLE}"
echo "To open: open '${APP_BUNDLE}'"
