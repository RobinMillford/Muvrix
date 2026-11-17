#!/usr/bin/env bash
# Muvrix Standalone AppImage Builder
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "=== Building Muvrix AppImage (x86_64) ==="
cd "$REPO_ROOT"

VERSION="0.1.0"
ARCH="x86_64"
PKG_NAME="Muvrix-${VERSION}-${ARCH}.AppImage"
OUTPUT_DIR="target/packages"
APPDIR="target/package_staging/AppDir"

mkdir -p "$OUTPUT_DIR"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin"
mkdir -p "$APPDIR/usr/share/applications"
mkdir -p "$APPDIR/usr/share/metainfo"
mkdir -p "$APPDIR/usr/share/icons/hicolor/scalable/apps"

# 1. Ensure release binary exists
BIN_SOURCE="target/release/muvrix-ui"
if [ ! -f "$BIN_SOURCE" ]; then
    echo "[*] Compiling release binary..."
    cargo build --release -p muvrix-ui
fi
cp "$BIN_SOURCE" "$APPDIR/usr/bin/muvrix"
chmod 755 "$APPDIR/usr/bin/muvrix"

# 2. AppRun entrypoint
cp packaging/appimage/AppRun "$APPDIR/AppRun"
chmod 755 "$APPDIR/AppRun"

# 3. Desktop entry and icons
cp packaging/appimage/muvrix.desktop "$APPDIR/muvrix.desktop"
cp packaging/appimage/muvrix.desktop "$APPDIR/io.muvrix.Muvrix.desktop"
cp packaging/appimage/muvrix.desktop "$APPDIR/usr/share/applications/io.muvrix.Muvrix.desktop"

# Icons in root of AppDir (standard AppImage requirement)
cp assets/branding/muvrix-logo.png "$APPDIR/muvrix.png"
cp assets/branding/muvrix-logo.png "$APPDIR/io.muvrix.Muvrix.png"
cp assets/branding/muvrix-logo.png "$APPDIR/.DirIcon"

for size in 16 24 32 48 64 128 256 512; do
    ICON_DIR="$APPDIR/usr/share/icons/hicolor/${size}x${size}/apps"
    mkdir -p "$ICON_DIR"
    if [ -f "assets/branding/icons/muvrix-${size}x${size}.png" ]; then
        cp "assets/branding/icons/muvrix-${size}x${size}.png" "$ICON_DIR/io.muvrix.Muvrix.png"
    fi
done

cp assets/branding/muvrix-logo.svg "$APPDIR/usr/share/icons/hicolor/scalable/apps/io.muvrix.Muvrix.svg"
cp packaging/metainfo/org.muvrix.Muvrix.metainfo.xml "$APPDIR/usr/share/metainfo/org.muvrix.Muvrix.metainfo.xml"

# 4. Resolve a genuine appimagetool; never emit a self-extracting stub in its place.
APPIMAGETOOL_VERSION="continuous"
APPIMAGETOOL_URL="https://github.com/AppImage/appimagetool/releases/download/${APPIMAGETOOL_VERSION}/appimagetool-x86_64.AppImage"
APPIMAGETOOL_SHA256="e8c1169c4af66c3184d7ee110713d4be04befe671fa358e2c1591ee840553b03"
TOOLS_DIR="target/tools"
mkdir -p "$TOOLS_DIR"

APPIMAGETOOL="${APPIMAGETOOL:-}"
if [ -z "$APPIMAGETOOL" ] && command -v appimagetool >/dev/null 2>&1; then
    APPIMAGETOOL="$(command -v appimagetool)"
fi
if [ -z "$APPIMAGETOOL" ] && [ -x "$TOOLS_DIR/appimagetool" ]; then
    APPIMAGETOOL="$TOOLS_DIR/appimagetool"
fi
if [ -z "$APPIMAGETOOL" ]; then
    echo "[*] Downloading pinned appimagetool ($APPIMAGETOOL_VERSION)..."
    curl -fsSL -o "$TOOLS_DIR/appimagetool.part" "$APPIMAGETOOL_URL"
    echo "$APPIMAGETOOL_SHA256  $TOOLS_DIR/appimagetool.part" | sha256sum -c - >/dev/null
    mv "$TOOLS_DIR/appimagetool.part" "$TOOLS_DIR/appimagetool"
    APPIMAGETOOL="$TOOLS_DIR/appimagetool"
fi
chmod +x "$APPIMAGETOOL"

# appimagetool is itself an AppImage and needs FUSE; extract and run its AppRun instead.
if "$APPIMAGETOOL" --appimage-version >/dev/null 2>&1; then
    APPIMAGETOOL_CMD=("$APPIMAGETOOL")
else
    rm -rf "target/tools/appimagetool-extracted"
    ( cd "target/tools" && "$APPIMAGETOOL" --appimage-extract >/dev/null )
    mv "target/tools/squashfs-root" "target/tools/appimagetool-extracted"
    APPIMAGETOOL_CMD=("target/tools/appimagetool-extracted/AppRun")
fi

# 5. Assemble a genuine type-2 AppImage from the AppDir payload.
APPIMAGE_PATH="$OUTPUT_DIR/$PKG_NAME"
rm -f "$APPIMAGE_PATH"
ARCH=x86_64 "${APPIMAGETOOL_CMD[@]}" --no-appstream "$APPDIR" "$APPIMAGE_PATH"

# 6. Verification: the artifact must be an ELF with the "AI\x02" type-2 magic.
ELF_MAGIC="$(head -c 4 "$APPIMAGE_PATH" | od -An -tx1 | tr -d ' \n')"
TYPE_MAGIC="$(dd if="$APPIMAGE_PATH" bs=1 skip=8 count=3 status=none | od -An -tx1 | tr -d ' \n')"
if [ "$ELF_MAGIC" != "7f454c46" ] || [ "$TYPE_MAGIC" != "414902" ]; then
    echo "[-] $APPIMAGE_PATH is not a genuine type-2 AppImage (ELF=$ELF_MAGIC, type=$TYPE_MAGIC)" >&2
    exit 1
fi
APP_SIZE=$(stat -c%s "$APPIMAGE_PATH")
echo "[+] Successfully generated genuine type-2 AppImage: $APPIMAGE_PATH ($APP_SIZE bytes)"
