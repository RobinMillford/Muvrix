#!/usr/bin/env bash
# ==============================================================================
# Muvrix macOS App Bundle Packaging Script (Phase 1.3)
# Assembles Muvrix.app bundle for macOS (Apple Silicon ARM64 / Intel x86_64).
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
DIST_DIR="$REPO_ROOT/target/dist"
APP_BUNDLE="$DIST_DIR/Muvrix.app"

echo "=== Muvrix macOS Packaging Pipeline ==="
echo "Repository Root: $REPO_ROOT"
echo "Target App Bundle: $APP_BUNDLE"

# 1. Build release binary
echo "Building release binary..."
cd "$REPO_ROOT"
cargo build --release --bin muvrix-ui

# 2. Stage .app directory structure
rm -rf "$APP_BUNDLE"
mkdir -p "$APP_BUNDLE/Contents/MacOS"
mkdir -p "$APP_BUNDLE/Contents/Frameworks"
mkdir -p "$APP_BUNDLE/Contents/Resources/assets"

# 3. Copy executable
cp "$REPO_ROOT/target/release/muvrix-ui" "$APP_BUNDLE/Contents/MacOS/muvrix"

# 4. Locate and bundle libmpv dylib
MPV_DYLIB=""
if [ -f "/opt/homebrew/lib/libmpv.2.dylib" ]; then
    MPV_DYLIB="/opt/homebrew/lib/libmpv.2.dylib"
elif [ -f "/usr/local/lib/libmpv.2.dylib" ]; then
    MPV_DYLIB="/usr/local/lib/libmpv.2.dylib"
elif [ -n "${MPV_LIB_DIR:-}" ] && [ -f "$MPV_LIB_DIR/libmpv.2.dylib" ]; then
    MPV_DYLIB="$MPV_LIB_DIR/libmpv.2.dylib"
fi

if [ -n "$MPV_DYLIB" ]; then
    echo "Bundling libmpv dylib from $MPV_DYLIB..."
    cp "$MPV_DYLIB" "$APP_BUNDLE/Contents/Frameworks/"
    
    # Adjust install names and RPATH
    install_name_tool -id "@rpath/libmpv.2.dylib" "$APP_BUNDLE/Contents/Frameworks/libmpv.2.dylib"
    install_name_tool -change "$MPV_DYLIB" "@rpath/libmpv.2.dylib" "$APP_BUNDLE/Contents/MacOS/muvrix" || true
    install_name_tool -add_rpath "@executable_path/../Frameworks" "$APP_BUNDLE/Contents/MacOS/muvrix" || true
else
    echo "Warning: libmpv.2.dylib not found in standard paths; relying on host dyld search."
fi

# 5. Copy shaders and assets
cp -r "$REPO_ROOT/assets/shaders" "$APP_BUNDLE/Contents/Resources/assets/"
cp "$REPO_ROOT/assets/branding/muvrix.icns" "$APP_BUNDLE/Contents/Resources/Muvrix.icns"

# 6. Create Info.plist
cat << 'EOF' > "$APP_BUNDLE/Contents/Info.plist"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleExecutable</key>
    <string>muvrix</string>
    <key>CFBundleIconFile</key>
    <string>Muvrix</string>
    <key>CFBundleIdentifier</key>
    <string>org.muvrix.Muvrix</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>Muvrix</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key>
    <true/>
</dict>
</plist>
EOF

# 7. Copy licenses and notices
cp "$REPO_ROOT/LICENSE" "$APP_BUNDLE/Contents/Resources/"
cp "$REPO_ROOT/THIRD-PARTY-NOTICES.md" "$APP_BUNDLE/Contents/Resources/"

echo "macOS App Bundle staged successfully at: $APP_BUNDLE"
