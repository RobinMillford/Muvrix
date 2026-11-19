#!/usr/bin/env bash
# Muvrix Debian (.deb) Package Builder
#
# Assembles a genuine binary Debian package with dpkg-deb. The DEBIAN/control
# file is composed from the binary package stanza in packaging/deb/debian/control
# plus the source-level fields that dpkg-gencontrol would normally inherit
# (Version, Maintainer, Section, Priority, Homepage). The source package stanza
# is never written into the binary control file: dpkg rejects such archives with
# "missing 'Package' field".
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "=== Building Muvrix Debian (.deb) Package ==="
cd "$REPO_ROOT"

VERSION="0.1.0"
ARCH="amd64"
PKG_NAME="muvrix_${VERSION}_${ARCH}.deb"
OUTPUT_DIR="target/packages"
STAGING_DIR="target/package_staging/deb_root"
SOURCE_CONTROL="packaging/deb/debian/control"

mkdir -p "$OUTPUT_DIR"
rm -rf "$STAGING_DIR"
mkdir -p "$STAGING_DIR/DEBIAN"
mkdir -p "$STAGING_DIR/usr/bin"
mkdir -p "$STAGING_DIR/usr/share/applications"
mkdir -p "$STAGING_DIR/usr/share/metainfo"
mkdir -p "$STAGING_DIR/usr/share/doc/muvrix"
mkdir -p "$STAGING_DIR/usr/lib/muvrix"

# 0. Private media runtime (libmpv 0.42 + ffmpeg + libplacebo). It is shipped
#    inside the package because no supported distribution provides libmpv 0.42.
if [ ! -d "target/runtime/lib" ]; then
    echo "[*] Private runtime missing; building it first..."
    bash packaging/build_runtime_bundle.sh
fi
cp -a target/runtime/lib "$STAGING_DIR/usr/lib/muvrix/lib"
cp -a target/runtime/share/licenses "$STAGING_DIR/usr/lib/muvrix/licenses"
cp target/runtime/runtime-manifest.json "$STAGING_DIR/usr/lib/muvrix/runtime-manifest.json"
chmod -R go-w "$STAGING_DIR/usr/lib/muvrix"

# 1. Resolve the release binary. Packages must ship a binary built against
#    Ubuntu 24.04's C library and OpenSSL ABI, not the development host's.
if [ -n "${MUVRIX_BINARY:-}" ]; then
    BIN_SOURCE="$MUVRIX_BINARY"
    echo "[*] Using supplied binary: $BIN_SOURCE"
else
    BIN_SOURCE="$(bash packaging/build_native_binary.sh deb)"
fi
if [ ! -f "$BIN_SOURCE" ]; then
    echo "[-] Release binary not found: $BIN_SOURCE" >&2
    exit 1
fi
cp "$BIN_SOURCE" "$STAGING_DIR/usr/lib/muvrix/muvrix"
chmod 755 "$STAGING_DIR/usr/lib/muvrix/muvrix"

# Launcher: prepends the private runtime to LD_LIBRARY_PATH and execs the binary.
sed -e 's|@LIBDIR@|/usr/lib/muvrix/lib|' -e 's|@BINARY@|/usr/lib/muvrix/muvrix|' \
    packaging/launcher/muvrix-launcher.sh > "$STAGING_DIR/usr/bin/muvrix"
chmod 755 "$STAGING_DIR/usr/bin/muvrix"

# 2. Desktop entry and AppStream metainfo
sed 's|^Exec=.*|Exec=muvrix %U|' packaging/appimage/muvrix.desktop > "$STAGING_DIR/usr/share/applications/org.muvrix.Muvrix.desktop"
chmod 644 "$STAGING_DIR/usr/share/applications/org.muvrix.Muvrix.desktop"

cp packaging/metainfo/org.muvrix.Muvrix.metainfo.xml "$STAGING_DIR/usr/share/metainfo/org.muvrix.Muvrix.metainfo.xml"
chmod 644 "$STAGING_DIR/usr/share/metainfo/org.muvrix.Muvrix.metainfo.xml"

# 3. Icons installation
for size in 16 24 32 48 64 128 256 512; do
    ICON_DIR="$STAGING_DIR/usr/share/icons/hicolor/${size}x${size}/apps"
    mkdir -p "$ICON_DIR"
    if [ -f "assets/branding/icons/muvrix-${size}x${size}.png" ]; then
        cp "assets/branding/icons/muvrix-${size}x${size}.png" "$ICON_DIR/org.muvrix.Muvrix.png"
        chmod 644 "$ICON_DIR/org.muvrix.Muvrix.png"
    fi
done

# Scalable SVG icon
mkdir -p "$STAGING_DIR/usr/share/icons/hicolor/scalable/apps"
cp assets/branding/muvrix-logo.svg "$STAGING_DIR/usr/share/icons/hicolor/scalable/apps/org.muvrix.Muvrix.svg"
chmod 644 "$STAGING_DIR/usr/share/icons/hicolor/scalable/apps/org.muvrix.Muvrix.svg"

# 4. Compose the binary control file from the binary package stanza.
#
# Debian binary control files require the binary stanza first and must not carry
# the source stanza. Fields that dpkg-gencontrol inherits from the source stanza
# are merged back in explicitly.
BINARY_STANZA="$(awk 'BEGIN { RS = "" } /(^|\n)Package:[[:space:]]/ { print; exit }' "$SOURCE_CONTROL")"
SOURCE_STANZA="$(awk 'BEGIN { RS = "" } NR == 1 { print }' "$SOURCE_CONTROL")"

if [ -z "$BINARY_STANZA" ]; then
    echo "[-] No binary 'Package:' stanza found in $SOURCE_CONTROL" >&2
    exit 1
fi

# Extracts the first single-line value of a control field.
control_field() {
    printf '%s\n' "$1" | awk -v key="$2" '
        index($0, key ":") == 1 {
            sub("^[^:]*:[ \t]*", "");
            print;
            exit;
        }'
}

# Extracts a field including its continuation lines (used for Description).
control_field_block() {
    printf '%s\n' "$1" | awk -v key="$2" '
        $0 ~ "^" key ":" { capturing = 1; print; next }
        capturing && /^[ \t]/ { print; next }
        capturing { exit }'
}

PKG_FIELD="$(control_field "$BINARY_STANZA" "Package")"
MAINTAINER="$(control_field "$SOURCE_STANZA" "Maintainer")"
SECTION="$(control_field "$SOURCE_STANZA" "Section")"
PRIORITY="$(control_field "$SOURCE_STANZA" "Priority")"
HOMEPAGE="$(control_field "$SOURCE_STANZA" "Homepage")"
DESCRIPTION_BLOCK="$(control_field_block "$BINARY_STANZA" "Description")"

if [ -z "$PKG_FIELD" ] || [ -z "$MAINTAINER" ] || [ -z "$DESCRIPTION_BLOCK" ]; then
    echo "[-] Binary control stanza is missing Package, Maintainer or Description" >&2
    exit 1
fi

# ${shlibs:Depends} is produced by dpkg-shlibdeps during a source build. This
# package is assembled from a prebuilt binary, so the runtime library set is
# declared explicitly and merged into the binary stanza's dependency list.
# libmpv is bundled privately, so the package does not depend on the host's
# libmpv2 (which would be 0.37 on Ubuntu 24.04 and crashes this build). What it
# does need from the host is a C library new enough for the private runtime, the
# graphics stack, D-Bus and OpenSSL.
SHLIBS_DEPENDS="libc6 (>= 2.39), libegl1, libgl1, libdbus-1-3, libssl3 (>= 3.0.0), libwayland-client0, libwayland-egl1, libxkbcommon0, libxkbcommon-x11-0, libx11-6, libva2, libva-drm2, libva-x11-2"
DEPENDS="$(control_field "$BINARY_STANZA" "Depends")"
DEPENDS="${DEPENDS//\$\{shlibs:Depends\}/$SHLIBS_DEPENDS}"
DEPENDS="${DEPENDS//\$\{misc:Depends\}/}"
DEPENDS="$(printf '%s' "$DEPENDS" | sed -E 's/[[:space:]]*,[[:space:]]*,/,/g; s/^[[:space:],]+//; s/[[:space:],]+$//')"
# Deduplicate dependency entries while preserving order and alternatives.
DEPENDS="$(printf '%s' "$DEPENDS" | awk -F', *' '
    {
        out = "";
        for (i = 1; i <= NF; i++) {
            dep = $i;
            gsub(/^ +| +$/, "", dep);
            if (dep != "" && !seen[dep]++) {
                out = out (out == "" ? "" : ", ") dep;
            }
        }
        print out;
    }')"

INSTALLED_SIZE=$(du -sk "$STAGING_DIR/usr" | cut -f1)

# The private runtime is large; keep md5sums and the archive in sync with it.
if [ ! -f "$STAGING_DIR/usr/lib/muvrix/lib/libmpv.so.2" ]; then
    echo "[-] Private runtime payload is incomplete (libmpv.so.2 missing)" >&2
    exit 1
fi

{
    echo "Package: ${PKG_FIELD}"
    echo "Version: ${VERSION}"
    echo "Architecture: ${ARCH}"
    echo "Maintainer: ${MAINTAINER}"
    echo "Installed-Size: ${INSTALLED_SIZE}"
    [ -n "$DEPENDS" ] && echo "Depends: ${DEPENDS}"
    [ -n "$SECTION" ] && echo "Section: ${SECTION}"
    [ -n "$PRIORITY" ] && echo "Priority: ${PRIORITY}"
    [ -n "$HOMEPAGE" ] && echo "Homepage: ${HOMEPAGE}"
    printf '%s\n' "$DESCRIPTION_BLOCK"
} > "$STAGING_DIR/DEBIAN/control"
chmod 644 "$STAGING_DIR/DEBIAN/control"

# 5. Generate md5sums
(
    cd "$STAGING_DIR"
    find usr -type f -exec md5sum {} + > DEBIAN/md5sums
    chmod 644 DEBIAN/md5sums
)

# 6. Maintainer scripts must be executable; metadata files must not be.
for script in preinst postinst prerm postrm config templates triggers; do
    if [ -f "$STAGING_DIR/DEBIAN/$script" ]; then
        chmod 755 "$STAGING_DIR/DEBIAN/$script"
    fi
done

# 7. Deterministic timestamps so repeated builds are bit-for-bit identical.
if [ -z "${SOURCE_DATE_EPOCH:-}" ]; then
    SOURCE_DATE_EPOCH="$(git log -1 --format=%ct 2>/dev/null || date +%s)"
fi
export SOURCE_DATE_EPOCH
find "$STAGING_DIR" -exec touch -h -d "@${SOURCE_DATE_EPOCH}" {} +

# 8. Build the .deb package with genuine dpkg-deb, on the host or in a
#    disposable Ubuntu container. Never fall back to a hand-rolled archive.
build_deb() {
    local builder="$1" deb_path="$2" stage_path="$3"
    if [ "$builder" = "host" ]; then
        dpkg-deb --build --root-owner-group "$stage_path" "$deb_path"
    else
        podman run --rm -v "$REPO_ROOT:/workspace:z" -w /workspace \
            -e SOURCE_DATE_EPOCH="$SOURCE_DATE_EPOCH" ubuntu:24.04 \
            dpkg-deb --build --root-owner-group "$stage_path" "$deb_path"
    fi
}

inspect_deb() {
    local deb_path="$1"
    if command -v dpkg-deb >/dev/null 2>&1; then
        dpkg-deb -I "$deb_path"
        dpkg-deb --ctrl-tarfile "$deb_path" | tar -xO ./control
    else
        podman run --rm -v "$REPO_ROOT:/workspace:z" -w /workspace ubuntu:24.04 \
            bash -c "dpkg-deb -I '$deb_path' && dpkg-deb --ctrl-tarfile '$deb_path' | tar -xO ./control"
    fi
}

if command -v dpkg-deb >/dev/null 2>&1; then
    echo "[*] Assembling package with host dpkg-deb..."
    build_deb host "$OUTPUT_DIR/$PKG_NAME" "$STAGING_DIR"
elif command -v podman >/dev/null 2>&1; then
    echo "[*] Assembling package with containerized dpkg-deb (Ubuntu 24.04)..."
    build_deb container "$OUTPUT_DIR/$PKG_NAME" "$STAGING_DIR"
else
    echo "[-] Error: dpkg-deb or Podman is required to build a valid .deb package." >&2
    echo "[-] Install dpkg-dev/dpkg or a container engine; refusing to emit a synthetic archive." >&2
    exit 1
fi

# 9. Verification: the control file must parse and start with the binary stanza.
echo "[*] Verifying binary control metadata..."
CONTROL_DUMP="$(inspect_deb "$OUTPUT_DIR/$PKG_NAME")"
printf '%s\n' "$CONTROL_DUMP"

FIRST_LINE="$(printf '%s\n' "$CONTROL_DUMP" | grep -E '^(Package|Version|Architecture):' | head -1)"
if [ "$FIRST_LINE" != "Package: ${PKG_FIELD}" ]; then
    echo "[-] DEBIAN/control does not begin with the binary Package stanza (got: $FIRST_LINE)" >&2
    exit 1
fi
for field in "Version: ${VERSION}" "Architecture: ${ARCH}" "Maintainer: ${MAINTAINER}" "Depends:" "Description:"; do
    if ! printf '%s\n' "$CONTROL_DUMP" | grep -qF "$field"; then
        echo "[-] DEBIAN/control missing expected field: ${field}" >&2
        exit 1
    fi
done

DEB_SIZE=$(stat -c%s "$OUTPUT_DIR/$PKG_NAME" 2>/dev/null || stat -f%z "$OUTPUT_DIR/$PKG_NAME")
echo "[+] Successfully generated: $OUTPUT_DIR/$PKG_NAME ($DEB_SIZE bytes)"
