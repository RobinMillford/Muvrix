#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

echo "=== [Muvrix Network Integration] Tearing down Test Containers ==="
for c in muvrix-test-webdav muvrix-test-samba muvrix-test-openssh; do
    if podman ps -a --format '{{.Names}}' | grep -q "^${c}$"; then
        echo "Stopping and removing container $c..."
        podman rm -f "$c" >/dev/null 2>&1 || true
    fi
done

# Clean up ephemeral fingerprint file
rm -f "${BASE_DIR}/sftp_fingerprint.txt"
echo "=== Teardown complete ==="
