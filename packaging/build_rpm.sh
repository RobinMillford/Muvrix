#!/usr/bin/env bash
# Muvrix Fedora/RHEL (.rpm) Package Builder
#
# Builds a genuine binary RPM with native rpmbuild. The build runs inside the
# Fedora packaging container (packaging/containers/Containerfile.rpm) so the
# payload is produced by the distribution's own RPM tooling. If rpmbuild cannot
# be provided, the build fails: this script never emits a synthetic RPM-like
# archive.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "=== Building Muvrix Fedora/RHEL (.rpm) Package ==="
cd "$REPO_ROOT"

VERSION="0.1.0"
RELEASE="1"
ARCH="x86_64"
PKG_NAME="muvrix-${VERSION}-${RELEASE}.${ARCH}.rpm"
OUTPUT_DIR="target/packages"
STAGING_DIR="target/package_staging/rpm_root"
RPMBUILD_DIR="$REPO_ROOT/target/package_staging/rpmbuild"
BUILDER_IMAGE="muvrix-rpm-builder:local"

mkdir -p "$OUTPUT_DIR"
rm -rf "$STAGING_DIR" "$RPMBUILD_DIR"
mkdir -p "$STAGING_DIR"

# 1. Resolve the release binary. The RPM must ship a binary compiled against
#    Fedora 40's C library and OpenSSL ABI.
if [ -n "${MUVRIX_BINARY:-}" ]; then
    BIN_SOURCE="$MUVRIX_BINARY"
    echo "[*] Using supplied binary: $BIN_SOURCE"
else
    BIN_SOURCE="$(bash packaging/build_native_binary.sh rpm)"
fi
if [ ! -f "$BIN_SOURCE" ]; then
    echo "[-] Release binary not found: $BIN_SOURCE" >&2
    exit 1
fi

# 2. Stage the installation tree
mkdir -p "$STAGING_DIR/usr/bin"
mkdir -p "$STAGING_DIR/usr/share/applications"
mkdir -p "$STAGING_DIR/usr/share/metainfo"
mkdir -p "$STAGING_DIR/usr/share/icons/hicolor/scalable/apps"
mkdir -p "$STAGING_DIR/usr/lib64/muvrix"

# Private media runtime (libmpv 0.42 + ffmpeg + libplacebo). No Fedora release
# ships mpv-libs 0.42, so the payload carries it.
if [ ! -d "target/runtime/lib" ]; then
    echo "[*] Private runtime missing; building it first..."
    bash packaging/build_runtime_bundle.sh
fi
cp -a target/runtime/lib "$STAGING_DIR/usr/lib64/muvrix/lib"
cp -a target/runtime/share/licenses "$STAGING_DIR/usr/lib64/muvrix/licenses"
cp target/runtime/runtime-manifest.json "$STAGING_DIR/usr/lib64/muvrix/runtime-manifest.json"
chmod -R go-w "$STAGING_DIR/usr/lib64/muvrix"

cp "$BIN_SOURCE" "$STAGING_DIR/usr/lib64/muvrix/muvrix"
chmod 755 "$STAGING_DIR/usr/lib64/muvrix/muvrix"

sed -e 's|@LIBDIR@|/usr/lib64/muvrix/lib|' -e 's|@BINARY@|/usr/lib64/muvrix/muvrix|' \
    packaging/launcher/muvrix-launcher.sh > "$STAGING_DIR/usr/bin/muvrix"
chmod 755 "$STAGING_DIR/usr/bin/muvrix"

sed 's|^Exec=.*|Exec=muvrix %U|' packaging/appimage/muvrix.desktop > "$STAGING_DIR/usr/share/applications/org.muvrix.Muvrix.desktop"
chmod 644 "$STAGING_DIR/usr/share/applications/org.muvrix.Muvrix.desktop"

cp packaging/metainfo/org.muvrix.Muvrix.metainfo.xml "$STAGING_DIR/usr/share/metainfo/org.muvrix.Muvrix.metainfo.xml"
chmod 644 "$STAGING_DIR/usr/share/metainfo/org.muvrix.Muvrix.metainfo.xml"

for size in 16 24 32 48 64 128 256 512; do
    ICON_DIR="$STAGING_DIR/usr/share/icons/hicolor/${size}x${size}/apps"
    mkdir -p "$ICON_DIR"
    if [ -f "assets/branding/icons/muvrix-${size}x${size}.png" ]; then
        cp "assets/branding/icons/muvrix-${size}x${size}.png" "$ICON_DIR/org.muvrix.Muvrix.png"
        chmod 644 "$ICON_DIR/org.muvrix.Muvrix.png"
    fi
done

cp assets/branding/muvrix-logo.svg "$STAGING_DIR/usr/share/icons/hicolor/scalable/apps/org.muvrix.Muvrix.svg"
chmod 644 "$STAGING_DIR/usr/share/icons/hicolor/scalable/apps/org.muvrix.Muvrix.svg"

# 3. Binary RPM specfile. The payload is staged above; %build is intentionally
#    empty because the binary was compiled in the target distribution.
SPEC_FILE="target/package_staging/muvrix-binary.spec"
cat << EOF > "$SPEC_FILE"
Name:           muvrix
Version:        ${VERSION}
Release:        ${RELEASE}
Summary:        Modern native desktop media player
License:        GPL-2.0-or-later
URL:            https://github.com/RobinMillford/Muvrix
AutoReqProv:    no
Requires:       glibc >= 2.39, mesa-libEGL, mesa-libGL, dbus-libs, openssl-libs, libwayland-client, libwayland-egl, libxkbcommon, libxkbcommon-x11, libX11, libva, hicolor-icon-theme

%description
Muvrix is a high-performance desktop media player written in Rust and powered by libmpv.

%prep

%build

%install
rm -rf %{buildroot}
mkdir -p %{buildroot}
cp -a %{_stagedir}/usr %{buildroot}/

%files
%{_bindir}/muvrix
%{_libdir}/muvrix
%{_datadir}/applications/org.muvrix.Muvrix.desktop
%{_datadir}/metainfo/org.muvrix.Muvrix.metainfo.xml
%{_datadir}/icons/hicolor/*/apps/org.muvrix.Muvrix.*

%changelog
* Tue Nov 17 2026 Muvrix Desktop Team <dev@muvrix.org> - ${VERSION}-${RELEASE}
- Production release for Muvrix Phase E8 corrective pass.
EOF

mkdir -p "$RPMBUILD_DIR"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}
cp "$SPEC_FILE" "$RPMBUILD_DIR/SPECS/muvrix-binary.spec"

# 4. Deterministic timestamps.
if [ -z "${SOURCE_DATE_EPOCH:-}" ]; then
    SOURCE_DATE_EPOCH="$(git log -1 --format=%ct 2>/dev/null || date +%s)"
fi
export SOURCE_DATE_EPOCH
find "$STAGING_DIR" -exec touch -h -d "@${SOURCE_DATE_EPOCH}" {} +

RPMBUILD_ARGS=(
    --define "_topdir /workspace/target/package_staging/rpmbuild"
    --define "_stagedir /workspace/$STAGING_DIR"
    --define "use_source_date_epoch_as_buildtime 1"
    --define "clamp_mtime_to_source_date_epoch 1"
    --define "_buildhost reproducible.muvrix"
    -bb /workspace/target/package_staging/rpmbuild/SPECS/muvrix-binary.spec
)

# 5. Build with genuine rpmbuild, on the host or in the Fedora container.
if command -v rpmbuild >/dev/null 2>&1; then
    echo "[*] Building genuine RPM package using host rpmbuild..."
    rpmbuild "${RPMBUILD_ARGS[@]/\/workspace\//$REPO_ROOT/}"
elif command -v podman >/dev/null 2>&1; then
    echo "[*] Building genuine RPM package inside the Fedora packaging container..."
    if ! podman image exists "$BUILDER_IMAGE" 2>/dev/null; then
        podman build -q -t "$BUILDER_IMAGE" -f packaging/containers/Containerfile.rpm packaging/containers >/dev/null
    fi
    podman run --rm \
        -v "$REPO_ROOT:/workspace:z" \
        -w /workspace \
        -e SOURCE_DATE_EPOCH="$SOURCE_DATE_EPOCH" \
        "$BUILDER_IMAGE" \
        rpmbuild "${RPMBUILD_ARGS[@]}"
else
    echo "[-] Error: native rpmbuild (or Podman with the Fedora packaging container) is required." >&2
    echo "[-] Refusing to generate a synthetic RPM archive." >&2
    exit 1
fi

RPM_FILE="$(find "$RPMBUILD_DIR/RPMS" -name '*.rpm' -type f | head -1)"
if [ -z "$RPM_FILE" ]; then
    echo "[-] rpmbuild produced no binary RPM" >&2
    exit 1
fi
cp "$RPM_FILE" "$OUTPUT_DIR/$PKG_NAME"

# 6. Verification with native RPM tooling.
rpm_query() {
    if command -v rpm >/dev/null 2>&1; then
        rpm "$@"
    else
        podman run --rm -v "$REPO_ROOT:/workspace:z" -w /workspace "$BUILDER_IMAGE" rpm "$@"
    fi
}

echo "[*] Verifying RPM metadata and payload..."
rpm_query -qip "$OUTPUT_DIR/$PKG_NAME"
rpm_query -qlp "$OUTPUT_DIR/$PKG_NAME" | tee /tmp/muvrix-rpm-contents.txt

for expected in "/usr/bin/muvrix" "/usr/lib64/muvrix/muvrix" "/usr/lib64/muvrix/lib/libmpv.so.2" "/usr/share/applications/org.muvrix.Muvrix.desktop" "/usr/share/metainfo/org.muvrix.Muvrix.metainfo.xml"; do
    if ! grep -qx "$expected" /tmp/muvrix-rpm-contents.txt; then
        echo "[-] RPM payload missing expected path: $expected" >&2
        exit 1
    fi
done

if rpm_query -qp --requires "$OUTPUT_DIR/$PKG_NAME" | grep -q "mpv-libs"; then
    echo "[-] RPM still depends on host mpv-libs although libmpv 0.42 is bundled" >&2
    exit 1
fi
if ! rpm_query -qp --requires "$OUTPUT_DIR/$PKG_NAME" | grep -q "glibc >= 2.39"; then
    echo "[-] RPM is missing the glibc floor required by the private runtime" >&2
    exit 1
fi

RPM_SIZE=$(stat -c%s "$OUTPUT_DIR/$PKG_NAME" 2>/dev/null || stat -f%z "$OUTPUT_DIR/$PKG_NAME")
echo "[+] Successfully generated: $OUTPUT_DIR/$PKG_NAME ($RPM_SIZE bytes)"
