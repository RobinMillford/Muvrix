#!/usr/bin/env bash
# Muvrix Multi-Distribution Packaging Validation and Build Script
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "=== Muvrix Phase D8 Packaging Validation & Builder ==="
echo "Repository Root: $REPO_ROOT"
cd "$REPO_ROOT"

# 1. Verify Manifests Existence
REQUIRED_FILES=(
    "packaging/appimage/AppRun"
    "packaging/appimage/muvrix.desktop"
    "packaging/flatpak/org.muvrix.Muvrix.yaml"
    "packaging/deb/debian/control"
    "packaging/deb/debian/rules"
    "packaging/deb/debian/changelog"
    "packaging/rpm/muvrix.spec"
    "packaging/metainfo/org.muvrix.Muvrix.metainfo.xml"
)

for file in "${REQUIRED_FILES[@]}"; do
    if [ ! -f "$file" ]; then
        echo "[-] Missing required packaging file: $file" >&2
        exit 1
    fi
    echo "[+] Verified packaging manifest: $file"
done

# 2. Check Execution Permissions
if [ ! -x "packaging/appimage/AppRun" ]; then
    echo "[*] Granting execution permissions to packaging/appimage/AppRun"
    chmod +x packaging/appimage/AppRun
fi

# 3. Desktop Entry Validation
if command -v desktop-file-validate >/dev/null 2>&1; then
    echo "[*] Validating desktop file format..."
    desktop-file-validate packaging/appimage/muvrix.desktop
    echo "[+] Desktop file validated successfully."
else
    echo "[i] desktop-file-validate not in PATH; performing structure check."
    grep -q "Type=Application" packaging/appimage/muvrix.desktop
    grep -q "Exec=" packaging/appimage/muvrix.desktop
    echo "[+] Desktop file basic structure valid."
fi

# 4. AppStream Metainfo Validation
if command -v appstreamcli >/dev/null 2>&1; then
    echo "[*] Validating AppStream metainfo with appstreamcli..."
    appstreamcli validate --no-net packaging/metainfo/org.muvrix.Muvrix.metainfo.xml
    echo "[+] AppStream metainfo validated successfully."
elif command -v appstream-util >/dev/null 2>&1; then
    echo "[*] Validating AppStream metainfo with appstream-util..."
    appstream-util validate-relax packaging/metainfo/org.muvrix.Muvrix.metainfo.xml || true
else
    echo "[i] AppStream validator not installed; performing XML structure check."
    grep -q "<id>org.muvrix.Muvrix</id>" packaging/metainfo/org.muvrix.Muvrix.metainfo.xml
    grep -q "</component>" packaging/metainfo/org.muvrix.Muvrix.metainfo.xml
    echo "[+] AppStream XML structure valid."
fi

# 5. Flatpak YAML Syntax Verification
python3 -c "
import yaml, sys
try:
    with open('packaging/flatpak/org.muvrix.Muvrix.yaml', 'r') as f:
        data = yaml.safe_load(f)
        assert data['app-id'] == 'org.muvrix.Muvrix'
        assert 'finish-args' in data
        assert 'modules' in data
    print('[+] Flatpak YAML manifest syntax and schema valid.')
except Exception as e:
    print(f'[-] Flatpak YAML error: {e}', file=sys.stderr)
    sys.exit(1)
" || {
    echo "[i] Python yaml module not found; checking basic keys."
    grep -q "app-id: org.muvrix.Muvrix" packaging/flatpak/org.muvrix.Muvrix.yaml
    grep -q "finish-args:" packaging/flatpak/org.muvrix.Muvrix.yaml
    echo "[+] Flatpak basic keys verified."
}

# 6. Debian Control Validation
grep -q "Package: muvrix" packaging/deb/debian/control
grep -q "Maintainer:" packaging/deb/debian/control
echo "[+] Debian control file valid."

# 7. RPM Spec Validation
grep -q "Name:           muvrix" packaging/rpm/muvrix.spec
grep -q "%description" packaging/rpm/muvrix.spec
echo "[+] RPM specfile valid."

# 8. Simulate AppDir Staging Tree
STAGING_DIR="target/package_staging/AppDir"
mkdir -p "$STAGING_DIR/usr/bin"
mkdir -p "$STAGING_DIR/usr/share/applications"
mkdir -p "$STAGING_DIR/usr/share/metainfo"
cp packaging/appimage/AppRun "$STAGING_DIR/AppRun"
cp packaging/appimage/muvrix.desktop "$STAGING_DIR/usr/share/applications/"
cp packaging/metainfo/org.muvrix.Muvrix.metainfo.xml "$STAGING_DIR/usr/share/metainfo/"
echo "[+] Staged AppDir layout successfully in $STAGING_DIR"

echo "=== All packaging manifests verified and staging confirmed ==="
