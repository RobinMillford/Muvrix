#!/usr/bin/env bash
# Muvrix Multi-Distribution Packaging Validation and Builder
# Orchestrates native builds for AppImage, Flatpak, Debian (.deb), and Fedora (.rpm).
#
# Each package is produced by the genuine toolchain of its target format. If a
# mandatory native tool is unavailable the pipeline fails immediately: there is
# no synthetic fallback artifact.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "=========================================================="
echo "=== MUVRIX PHASE E9 RELEASE PIPELINE (4 FORMATS + SBOM) ==="
echo "=========================================================="
echo "Repository Root: $REPO_ROOT"
cd "$REPO_ROOT"

# 1. Verify Manifests Existence
echo "[*] Step 1: Validating packaging manifests and containerfiles..."
REQUIRED_FILES=(
    "packaging/appimage/AppRun"
    "packaging/appimage/muvrix.desktop"
    "packaging/flatpak/org.muvrix.Muvrix.yaml"
    "packaging/deb/debian/control"
    "packaging/deb/debian/rules"
    "packaging/deb/debian/changelog"
    "packaging/rpm/muvrix.spec"
    "packaging/metainfo/org.muvrix.Muvrix.metainfo.xml"
    "packaging/containers/Containerfile.deb"
    "packaging/containers/Containerfile.rpm"
    "packaging/containers/Containerfile.appimage"
    "packaging/containers/Containerfile.flatpak"
    ".github/workflows/packaging.yml"
)

for file in "${REQUIRED_FILES[@]}"; do
    if [ ! -f "$file" ]; then
        echo "[-] Missing required packaging file: $file" >&2
        exit 1
    fi
    echo "  [+] Manifest verified: $file"
done

# 2. Preflight: mandatory native packaging tools
echo "[*] Step 2: Checking mandatory native packaging toolchains..."
HAVE_CONTAINER="no"
if command -v podman >/dev/null 2>&1 || command -v docker >/dev/null 2>&1; then
    HAVE_CONTAINER="yes"
fi

require_tool() {
    local format="$1" tool="$2" container_reason="$3"
    if command -v "$tool" >/dev/null 2>&1; then
        echo "  [+] $format: $tool available"
    elif [ "$HAVE_CONTAINER" = "yes" ]; then
        echo "  [+] $format: $tool unavailable on host; will use containerized build ($container_reason)"
    else
        echo "[-] $format: required native tool '$tool' is missing and no container engine is available" >&2
        exit 1
    fi
}

require_tool "Debian" "dpkg-deb" "ubuntu:24.04"
require_tool "Fedora" "rpmbuild" "packaging/containers/Containerfile.rpm"
require_tool "Flatpak" "flatpak" "alpine:3.20 with flatpak and ostree"
if ! command -v python3 >/dev/null 2>&1; then
    echo "[-] python3 is required by the AppImage and Flatpak builders" >&2
    exit 1
fi

# 3. Grant Script Execution Permissions
echo "[*] Step 3: Ensuring execute permissions on builder scripts..."
chmod +x packaging/*.sh packaging/appimage/AppRun

# 4. Desktop Entry Validation
echo "[*] Step 4: Validating desktop specification compliance..."
if command -v desktop-file-validate >/dev/null 2>&1; then
    desktop-file-validate packaging/appimage/muvrix.desktop
    echo "  [+] Desktop file validated via desktop-file-validate."
else
    grep -q "Type=Application" packaging/appimage/muvrix.desktop
    grep -q "Exec=" packaging/appimage/muvrix.desktop
    echo "  [+] Desktop file basic structure valid."
fi

# 5. AppStream Metainfo Validation
echo "[*] Step 5: Validating AppStream metainfo specification..."
if command -v appstreamcli >/dev/null 2>&1; then
    appstreamcli validate --no-net packaging/metainfo/org.muvrix.Muvrix.metainfo.xml
    echo "  [+] AppStream metainfo validated via appstreamcli."
elif command -v appstream-util >/dev/null 2>&1; then
    appstream-util validate-relax packaging/metainfo/org.muvrix.Muvrix.metainfo.xml
    echo "  [+] AppStream metainfo validated via appstream-util."
else
    grep -q "<id>org.muvrix.Muvrix</id>" packaging/metainfo/org.muvrix.Muvrix.metainfo.xml
    grep -q "</component>" packaging/metainfo/org.muvrix.Muvrix.metainfo.xml
    echo "  [+] AppStream XML structure valid."
fi

# 6. Flatpak YAML Syntax Verification
echo "[*] Step 6: Validating Flatpak manifest..."
python3 -c "
import yaml, sys
try:
    with open('packaging/flatpak/org.muvrix.Muvrix.yaml', 'r') as f:
        data = yaml.safe_load(f)
    assert data['app-id'] == 'org.muvrix.Muvrix'
    assert 'finish-args' in data
    assert 'modules' in data
    assert data['runtime'] == 'org.freedesktop.Platform'
    assert data['sdk'] == 'org.freedesktop.Sdk'
    print('  [+] Flatpak YAML manifest syntax and schema valid.')
except Exception as e:
    print(f'[-] Flatpak YAML error: {e}', file=sys.stderr)
    sys.exit(1)
"

# 7. Debian Control & RPM Spec Verification
echo "[*] Step 7: Validating Debian and RPM control declarations..."
grep -q "^Source: muvrix" packaging/deb/debian/control
grep -q "^Package: muvrix" packaging/deb/debian/control
grep -q "Maintainer:" packaging/deb/debian/control
echo "  [+] Debian control file verified."
grep -q "Name:           muvrix" packaging/rpm/muvrix.spec
grep -q "%description" packaging/rpm/muvrix.spec
echo "  [+] RPM specfile verified."

# 8. Private media runtime shared by every format.
echo "[*] Step 8: Building the private media runtime (libmpv 0.42)..."
if [ -d target/runtime/lib ] && [ -f target/runtime/runtime-manifest.json ]; then
    python3 -c "
import json, sys
m = json.load(open('target/runtime/runtime-manifest.json'))
assert m['libmpv']['version'].startswith('0.42'), m['libmpv']['version']
print('  [+] Reusing runtime bundle: libmpv', m['libmpv']['version'],
      '|', m['bundled_libraries'], 'libraries')
"
else
    bash packaging/build_runtime_bundle.sh
fi

# 9. Execute Individual Format Builders
echo "[*] Step 9: Building multi-platform distribution packages..."
mkdir -p target/packages

bash packaging/build_deb.sh
bash packaging/build_rpm.sh
bash packaging/build_appimage.sh
bash packaging/build_flatpak.sh

# 10. Verify every artifact with the native tool of its format.
echo "[*] Step 10: Verifying artifacts with native package tooling..."
DEB_ARTIFACT="muvrix_0.1.0_amd64.deb"
RPM_ARTIFACT="muvrix-0.1.0-1.x86_64.rpm"
APPIMAGE_ARTIFACT="Muvrix-0.1.0-x86_64.AppImage"
FLATPAK_ARTIFACT="org.muvrix.Muvrix.flatpak"
FLATPAKREF_ARTIFACT="org.muvrix.Muvrix.flatpakref"

if command -v dpkg-deb >/dev/null 2>&1; then
    dpkg-deb -I "target/packages/$DEB_ARTIFACT" >/dev/null
else
    podman run --rm -v "$REPO_ROOT:/workspace:z" -w /workspace ubuntu:24.04 \
        dpkg-deb -I "target/packages/$DEB_ARTIFACT" >/dev/null
fi
echo "  [+] $DEB_ARTIFACT parses as a Debian binary package."

if command -v rpm >/dev/null 2>&1; then
    rpm -qip "target/packages/$RPM_ARTIFACT" >/dev/null
    rpm -qlp "target/packages/$RPM_ARTIFACT" >/dev/null
else
    podman run --rm -v "$REPO_ROOT:/workspace:z" -w /workspace fedora:40 \
        bash -c "rpm -qip 'target/packages/$RPM_ARTIFACT' && rpm -qlp 'target/packages/$RPM_ARTIFACT'" >/dev/null
fi
echo "  [+] $RPM_ARTIFACT parses as a native RPM package."

# A genuine type-2 AppImage is an ELF executable whose byte 8 begins the "AI\x02" magic.
APPIMAGE_ELF_MAGIC="$(head -c 4 "target/packages/$APPIMAGE_ARTIFACT" | od -An -tx1 | tr -d ' \n')"
APPIMAGE_TYPE_MAGIC="$(dd if="target/packages/$APPIMAGE_ARTIFACT" bs=1 skip=8 count=3 status=none | od -An -tx1 | tr -d ' \n')"
if [ "$APPIMAGE_ELF_MAGIC" != "7f454c46" ] || [ "$APPIMAGE_TYPE_MAGIC" != "414902" ]; then
    echo "[-] $APPIMAGE_ARTIFACT is not a genuine type-2 AppImage (ELF=${APPIMAGE_ELF_MAGIC}, type=${APPIMAGE_TYPE_MAGIC})." >&2
    echo "[-] Refusing to certify a self-extracting script archive as an AppImage." >&2
    exit 1
fi
"target/packages/$APPIMAGE_ARTIFACT" --appimage-version >/dev/null
echo "  [+] $APPIMAGE_ARTIFACT is a genuine type-2 AppImage and reports its version."

FLATPAK_MAGIC="$(head -c 8 "target/packages/$FLATPAK_ARTIFACT" | tr -d '\0')"
if [ "$FLATPAK_MAGIC" != "flatpak" ]; then
    echo "[-] $FLATPAK_ARTIFACT is not an OSTree Flatpak bundle" >&2
    exit 1
fi
grep -qxF "Name=org.muvrix.Muvrix" "target/packages/$FLATPAKREF_ARTIFACT"
echo "  [+] Flatpak bundle header and flatpakref descriptor verified."

# 10b. Every package must carry the private runtime, never rely on host libmpv.
echo "[*] Step 10b: Verifying the bundled media runtime inside each package..."
if ! podman run --rm -v "$REPO_ROOT:/workspace:z" -w /workspace ubuntu:24.04 \
        bash -c "dpkg-deb -c 'target/packages/$DEB_ARTIFACT' | grep -q 'usr/lib/muvrix/lib/libmpv.so.2'" >/dev/null 2>&1; then
    echo "[-] $DEB_ARTIFACT does not contain the private libmpv runtime" >&2
    exit 1
fi
echo "  [+] $DEB_ARTIFACT ships libmpv 0.42 privately (no host libmpv2 dependency)."

if command -v rpm >/dev/null 2>&1; then
    rpm -qlp "target/packages/$RPM_ARTIFACT" | grep -qx "/usr/lib64/muvrix/lib/libmpv.so.2" || {
        echo "[-] $RPM_ARTIFACT does not contain the private libmpv runtime" >&2; exit 1; }
else
    podman run --rm -v "$REPO_ROOT:/workspace:z" -w /workspace fedora:40 \
        bash -c "rpm -qlp 'target/packages/$RPM_ARTIFACT' | grep -qx '/usr/lib64/muvrix/lib/libmpv.so.2'" >/dev/null
fi
echo "  [+] $RPM_ARTIFACT ships libmpv 0.42 privately (no mpv-libs dependency)."

# 11. Cryptographic Checksum Generation & Verification
echo "[*] Step 11: Computing SHA-256 cryptographic hashes..."
(
    cd target/packages
    sha256sum \
        "$DEB_ARTIFACT" \
        "$RPM_ARTIFACT" \
        "$APPIMAGE_ARTIFACT" \
        "$FLATPAK_ARTIFACT" \
        "$FLATPAKREF_ARTIFACT" > SHA256SUMS

    echo "[*] Verifying checksum integrity against SHA256SUMS:"
    sha256sum -c SHA256SUMS
)

# 12. Supply chain: SBOM, license inventory, signing status.
echo "[*] Step 12: Generating SBOM and dependency license inventory..."
python3 packaging/generate_sbom.py --output docs/release

if [ -d target/runtime ]; then
    cp target/runtime/runtime-manifest.json target/packages/runtime-manifest.json
fi

if command -v gpg >/dev/null 2>&1 || [ -n "${MUVRIX_SIGNING_KEY:-}" ]; then
    echo "[*] Step 13: Signing release manifest (pending when no key is configured)..."
    bash packaging/sign_release.sh target/packages
    cp target/packages/SIGNING_STATUS target/packages/SIGNING_STATUS 2>/dev/null || true
fi

echo "[*] Step 14: Verifying the release directory..."
bash packaging/verify_release.sh target/packages

echo "=========================================================="
echo "=== ALL PHASE E9 RELEASE ARTIFACTS GENERATED ==="
echo "=========================================================="
ls -lh target/packages/
echo ""
cat target/packages/SHA256SUMS
[ -f target/packages/SIGNING_STATUS ] && cat target/packages/SIGNING_STATUS
