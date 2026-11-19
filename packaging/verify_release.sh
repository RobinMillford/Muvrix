#!/usr/bin/env bash
# Verify a Muvrix release directory: checksums, signature (if present) and the
# signing status recorded at build time.
#
# Usage: packaging/verify_release.sh [DIRECTORY]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PKG_DIR="${1:-$REPO_ROOT/target/packages}"

cd "$PKG_DIR"
FAILED=0

echo "=== Verification: $PKG_DIR ==="

if [ ! -f SHA256SUMS ]; then
    echo "[-] SHA256SUMS missing" >&2
    exit 1
fi

echo "[*] Checksums"
if sha256sum -c SHA256SUMS; then
    echo "[+] All artifacts match SHA256SUMS"
else
    echo "[-] Checksum mismatch" >&2
    FAILED=1
fi

if [ -f SHA256SUMS.asc ]; then
    echo "[*] Signature"
    GNUPGHOME_DIR="$(mktemp -d)"
    export GNUPGHOME="$GNUPGHOME_DIR"
    trap 'rm -rf "$GNUPGHOME_DIR"' EXIT
    if [ -f muvrix-release-key.asc ]; then
        gpg --batch --import muvrix-release-key.asc 2>/dev/null
    fi
    if gpg --batch --verify SHA256SUMS.asc SHA256SUMS 2>/dev/null; then
        echo "[+] Signature valid"
    else
        echo "[-] Signature invalid" >&2
        FAILED=1
    fi
    trap - EXIT
    rm -rf "$GNUPGHOME_DIR"
else
    STATUS="$(sed -n 's/^status=//p' SIGNING_STATUS 2>/dev/null || true)"
    echo "[!] No signature present (status: ${STATUS:-unknown}) — artifact integrity is checksum-only"
fi

if [ -f SIGNING_STATUS ]; then
    echo "[*] Signing status"
    sed 's/^/    /' SIGNING_STATUS
fi

if [ "$FAILED" -ne 0 ]; then
    echo "[-] Verification FAILED"
    exit 1
fi

echo "[+] Verification passed"
