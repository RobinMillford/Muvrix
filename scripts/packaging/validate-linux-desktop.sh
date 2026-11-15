#!/usr/bin/env bash
# Validate tracked Linux desktop integration assets without building a package.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DESKTOP="$REPO_ROOT/packaging/muvrix.desktop"
LAUNCHER="$REPO_ROOT/packaging/muvrix"

test -f "$DESKTOP"
test -f "$LAUNCHER"

grep -qx 'Type=Application' "$DESKTOP"
grep -qx 'Exec=muvrix %F' "$DESKTOP"
grep -qx 'Icon=io.muvrix.Muvrix' "$DESKTOP"
grep -q 'video/mp4;video/x-matroska;video/webm;video/x-msvideo;video/quicktime;' "$DESKTOP"
grep -q 'LD_LIBRARY_PATH=.*lib64' "$LAUNCHER"
grep -q 'muvrix-bin' "$LAUNCHER"

test -s "$REPO_ROOT/assets/branding/muvrix-logo.svg"
test -s "$REPO_ROOT/assets/branding/muvrix-logo.png"
test -s "$REPO_ROOT/assets/branding/muvrix.ico"
test -s "$REPO_ROOT/assets/branding/muvrix.icns"
for size in 16 24 32 48 64 128 256 512; do
    test -s "$REPO_ROOT/assets/branding/icons/muvrix-${size}x${size}.png"
done

if command -v desktop-file-validate >/dev/null 2>&1; then
    desktop-file-validate "$DESKTOP"
fi

echo "Linux desktop assets validated: packaging/muvrix.desktop + packaging/muvrix"
