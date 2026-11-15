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
# Phase 1.4 (0.42.0): copy ONLY the exact SONAME-resolved 0.42.0 artifacts,
# plus libplacebo and other direct runtime deps used by the built libmpv,
# so the package runs outside ~/.local.
#
# Why not `libmpv.so*`: the old glob also staged the obsolete pre-migration
# backup `libmpv.so.2.5.0` (kept in ~/.local/lib64 for rollback). The glob
# was replaced by exact names below; the dev-environment backup is untouched.
stage_lib() {
    # $1 = file name under source dir; $2 = source dir
    local src="$2"
    if [ -e "$src/$1" ] || [ -L "$src/$1" ]; then
        cp -d "$src/$1" "$DIST_DIR/lib64/"
    fi
}

MPV_SRC=""
if [ -f "$HOME/.local/lib64/libmpv.so.2" ]; then
    MPV_SRC="$HOME/.local/lib64"
elif [ -f "/usr/lib64/libmpv.so.2" ]; then
    MPV_SRC="/usr/lib64"
fi

if [ -n "$MPV_SRC" ]; then
    # Resolve the exact file behind the libmpv.so.2 SONAME and stage it plus
    # its soname symlinks. This never picks up libmpv.so.2.5.0 leftovers.
    MPV_REAL="$(readlink -f "$MPV_SRC/libmpv.so.2")"
    MPV_REAL_NAME="$(basename "$MPV_REAL")"
    echo "Staging libmpv runtime: ${MPV_REAL_NAME} (SONAME libmpv.so.2)"
    cp "$MPV_REAL" "$DIST_DIR/lib64/$MPV_REAL_NAME"
    ln -sf "$MPV_REAL_NAME" "$DIST_DIR/lib64/libmpv.so.2"
    ln -sf "$MPV_REAL_NAME" "$DIST_DIR/lib64/libmpv.so"
    # Direct transitive deps of the 0.42.0 build (bundled so the package
    # runs outside ~/.local; the long tail of FFmpeg-family libs must be
    # provided by the target system — see DEPENDENCIES.md staged below).
    for lib in libplacebo.so.387 libplacebo.so libluajit-5.1.so.2 \
               libluajit-5.1.so libluajit-5.1.so.2.1.1767980792 \
               libdisplay-info.so.3 libdisplay-info.so.0.3.0 \
               libdisplay-info.so; do
        stage_lib "$lib" "$MPV_SRC"
    done
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

# 5b. Copy branding icons
mkdir -p "$DIST_DIR/share/icons/hicolor/scalable/apps"
cp "$REPO_ROOT/assets/branding/muvrix-logo.svg" "$DIST_DIR/share/icons/hicolor/scalable/apps/io.muvrix.Muvrix.svg"
for size in 16 24 32 48 64 128 256 512; do
    mkdir -p "$DIST_DIR/share/icons/hicolor/${size}x${size}/apps"
    cp "$REPO_ROOT/assets/branding/icons/muvrix-${size}x${size}.png" "$DIST_DIR/share/icons/hicolor/${size}x${size}/apps/io.muvrix.Muvrix.png"
done

# 6. Copy documentation and licenses
cp "$REPO_ROOT/LICENSE" "$DIST_DIR/"
cp "$REPO_ROOT/THIRD-PARTY-NOTICES.md" "$DIST_DIR/"
cp "$REPO_ROOT/README.md" "$DIST_DIR/"

# 6b. Stage the exact system prerequisite list for this package.
cat << 'EOF' > "$DIST_DIR/DEPENDENCIES.md"
# Muvrix Linux package — runtime dependencies

## Bundled in `lib64/`
- `libmpv.so.2.6.0` (SONAME `libmpv.so.2`) — exact pinned libmpv 0.42.0 runtime
- `libplacebo.so.387` — libplacebo 7.387.0
- `libdisplay-info.so.0.3.0` / `libdisplay-info.so.3`
- `libluajit-5.1.so.2.1.1767980792` / `libluajit-5.1.so.2`

## Required from the target system (not bundled)
The bundled `libmpv` still links a long transitive tail that this package
does **not** vendor. On the build host these come from Fedora 45 RPMs
(`ffmpeg-libs`, `libass`, `libva`, `wayland-libs`, `libdrm`, `libasound`,
`libX11`, `libxkbcommon`, `libxss`, `vulkan-loader`, `libxxhash`,
`libunibreak`, `harfbuzz`, `fribidi`, `freetype`, `fontconfig`,
`libcdio`, `libdvdnav`, `libdvdread`, `libarchive`, `libbluray`,
`liblcms2`, `libmujs`, `libuchardet`, `librubberband`, `libvapoursynth`,
`libzimg`, `libdovi`, `zlib`, `libjpeg`, `libgomp`, `libstdc++`,
plus glibc/libm/pthread and the Intel/Mesa GPU stack).

Install the ffmpeg-libs family for your distribution, or re-run the
packaging script on a host where those are present. The package must
therefore be described as **launcher-portable with system prerequisites**,
not as fully self-contained.
EOF

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

# 8. Use the tracked, lint-checked desktop entry and launcher assets.
cp "$REPO_ROOT/packaging/muvrix.desktop" "$DIST_DIR/share/applications/muvrix.desktop"
cp "$REPO_ROOT/packaging/muvrix" "$DIST_DIR/bin/muvrix"
chmod +x "$DIST_DIR/bin/muvrix"
"$REPO_ROOT/scripts/packaging/validate-linux-desktop.sh"

echo "Packaging complete: $DIST_DIR"
echo "To create tarball: tar -czvf target/dist/muvrix-linux-x86_64.tar.gz -C target/dist muvrix-linux-x86_64"
