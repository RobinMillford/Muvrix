#!/usr/bin/env bash
# ==============================================================================
# Muvrix Linux Packaging Script (Phase 1.3)
# Assembles portable distribution tarball and staging tree for Linux x86_64.
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
DIST_DIR="$REPO_ROOT/target/dist/muvrix-linux-x86_64"

echo "=== Muvrix Linux Packaging Pipeline ==="
echo "Repository Root: $REPO_ROOT"
echo "Target Dist Dir: $DIST_DIR"

# 1. Build release binary
echo "Building release binary..."
cd "$REPO_ROOT"
cargo build --release --bin muvrix-ui

# 2. Clean and create distribution directory structure
rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR/bin"
mkdir -p "$DIST_DIR/lib64"
mkdir -p "$DIST_DIR/share/muvrix/assets"
mkdir -p "$DIST_DIR/share/applications"
mkdir -p "$DIST_DIR/share/icons/hicolor/scalable/apps"

# 3. Copy executable
cp "$REPO_ROOT/target/release/muvrix-ui" "$DIST_DIR/bin/muvrix-bin"

# 3b. Bundle libmpv runtime if available in local prefix or system
if [ -f "$HOME/.local/lib64/libmpv.so.2" ]; then
    cp -d "$HOME/.local/lib64"/libmpv.so* "$DIST_DIR/lib64/" 2>/dev/null || true
elif [ -f "/usr/lib64/libmpv.so.2" ]; then
    cp -d /usr/lib64/libmpv.so* "$DIST_DIR/lib64/" 2>/dev/null || true
fi

# 4. Create launcher wrapper script
cat << 'EOF' > "$DIST_DIR/bin/muvrix"
#!/usr/bin/env bash
set -e
SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_ROOT="$(cd "$SELF_DIR/.." && pwd)"

# Prioritize bundled or local libmpv if present
if [ -d "$APP_ROOT/lib64" ]; then
    export LD_LIBRARY_PATH="$APP_ROOT/lib64:${LD_LIBRARY_PATH:-}"
fi

# Set default assets path if not specified
if [ -z "${MUVRIX_ASSETS_DIR:-}" ]; then
    export MUVRIX_ASSETS_DIR="$APP_ROOT/share/muvrix/assets"
fi

exec "$APP_ROOT/bin/muvrix-bin" "$@"
EOF
chmod +x "$DIST_DIR/bin/muvrix"

# 5. Copy assets (Anime4K shaders, profiles, manifests)
cp -r "$REPO_ROOT/assets/shaders" "$DIST_DIR/share/muvrix/assets/"

# 6. Copy documentation and licenses
cp "$REPO_ROOT/LICENSE" "$DIST_DIR/"
cp "$REPO_ROOT/THIRD-PARTY-NOTICES.md" "$DIST_DIR/"
cp "$REPO_ROOT/README.md" "$DIST_DIR/"

# 7. Create desktop entry file
cat << 'EOF' > "$DIST_DIR/share/applications/io.muvrix.Muvrix.desktop"
[Desktop Entry]
Name=Muvrix
Comment=Modern Native Desktop Media Player
Exec=muvrix %F
Icon=io.muvrix.Muvrix
Terminal=false
Type=Application
Categories=AudioVideo;Player;Video;
MimeType=video/mp4;video/mkv;video/webm;video/quicktime;audio/flac;audio/mp3;audio/ogg;audio/wav;
EOF

echo "Packaging complete: $DIST_DIR"
echo "To create tarball: tar -czvf target/dist/muvrix-linux-x86_64.tar.gz -C target/dist muvrix-linux-x86_64"
