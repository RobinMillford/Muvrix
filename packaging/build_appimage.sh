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

# 1. Portable payload: a binary built against the oldest supported C library
#    (Ubuntu 24.04, glibc 2.39) rather than the development host.
if [ -n "${MUVRIX_BINARY:-}" ]; then
    BIN_SOURCE="$MUVRIX_BINARY"
else
    BIN_SOURCE="$(bash packaging/build_native_binary.sh deb)"
fi
if [ ! -f "$BIN_SOURCE" ]; then
    echo "[-] Release binary not found: $BIN_SOURCE" >&2
    exit 1
fi
cp "$BIN_SOURCE" "$APPDIR/usr/bin/muvrix"
chmod 755 "$APPDIR/usr/bin/muvrix"

# 1b. Private media runtime, so the AppImage does not depend on a host libmpv.
if [ ! -d "target/runtime/lib" ]; then
    echo "[*] Private runtime missing; building it first..."
    bash packaging/build_runtime_bundle.sh
fi
mkdir -p "$APPDIR/usr/lib/muvrix"
cp -a target/runtime/lib "$APPDIR/usr/lib/muvrix/lib"
cp -a target/runtime/share/licenses "$APPDIR/usr/lib/muvrix/licenses"
cp target/runtime/runtime-manifest.json "$APPDIR/usr/lib/muvrix/runtime-manifest.json"

# 2. AppRun entrypoint
cp packaging/appimage/AppRun "$APPDIR/AppRun"
chmod 755 "$APPDIR/AppRun"

# 3. Desktop entry and icons
cp packaging/appimage/muvrix.desktop "$APPDIR/muvrix.desktop"
cp packaging/appimage/muvrix.desktop "$APPDIR/org.muvrix.Muvrix.desktop"
cp packaging/appimage/muvrix.desktop "$APPDIR/usr/share/applications/org.muvrix.Muvrix.desktop"

# Icons in root of AppDir (standard AppImage requirement)
cp assets/branding/muvrix-logo.png "$APPDIR/muvrix.png"
cp assets/branding/muvrix-logo.png "$APPDIR/org.muvrix.Muvrix.png"
cp assets/branding/muvrix-logo.png "$APPDIR/.DirIcon"

for size in 16 24 32 48 64 128 256 512; do
    ICON_DIR="$APPDIR/usr/share/icons/hicolor/${size}x${size}/apps"
    mkdir -p "$ICON_DIR"
    if [ -f "assets/branding/icons/muvrix-${size}x${size}.png" ]; then
        cp "assets/branding/icons/muvrix-${size}x${size}.png" "$ICON_DIR/org.muvrix.Muvrix.png"
    fi
done

cp assets/branding/muvrix-logo.svg "$APPDIR/usr/share/icons/hicolor/scalable/apps/org.muvrix.Muvrix.svg"
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
if [ -z "${SOURCE_DATE_EPOCH:-}" ]; then
    SOURCE_DATE_EPOCH="$(git log -1 --format=%ct 2>/dev/null || date +%s)"
fi
export SOURCE_DATE_EPOCH
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 "${APPIMAGETOOL_CMD[@]}" --no-appstream "$APPDIR" "$APPIMAGE_PATH"

# 6. Verification: the artifact must be an ELF with the "AI\x02" type-2 magic.
ELF_MAGIC="$(head -c 4 "$APPIMAGE_PATH" | od -An -tx1 | tr -d ' \n')"
TYPE_MAGIC="$(dd if="$APPIMAGE_PATH" bs=1 skip=8 count=3 status=none | od -An -tx1 | tr -d ' \n')"
if [ "$ELF_MAGIC" != "7f454c46" ] || [ "$TYPE_MAGIC" != "414902" ]; then
    echo "[-] $APPIMAGE_PATH is not a genuine type-2 AppImage (ELF=$ELF_MAGIC, type=$TYPE_MAGIC)" >&2
    exit 1
fi
APP_SIZE=$(stat -c%s "$APPIMAGE_PATH")
echo "[+] Successfully generated genuine type-2 AppImage: $APPIMAGE_PATH ($APP_SIZE bytes)"

# 7. The bundled runtime must actually be inside the image.
EXTRACT_DIR="$(mktemp -d)"
APPIMAGE_ABS="$(cd "$(dirname "$APPIMAGE_PATH")" && pwd)/$(basename "$APPIMAGE_PATH")"
trap 'rm -rf "$EXTRACT_DIR"' EXIT
(
    cd "$EXTRACT_DIR"
    "$APPIMAGE_ABS" --appimage-extract >/dev/null
)
if [ ! -f "$EXTRACT_DIR/squashfs-root/usr/lib/muvrix/lib/libmpv.so.2" ]; then
    echo "[-] AppImage does not contain the bundled libmpv runtime" >&2
    exit 1
fi
echo "[+] Bundled runtime verified inside the AppImage ($(ls "$EXTRACT_DIR/squashfs-root/usr/lib/muvrix/lib" | wc -l) libraries)"
