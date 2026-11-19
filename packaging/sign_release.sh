#!/usr/bin/env bash
# Sign the release manifest and record the provenance metadata.
#
# Signing keys are never stored in the repository. The key is supplied through
# the environment (CI secret or a local keyring path):
#
#   MUVRIX_SIGNING_KEY        fingerprint/key id of the signing key in the keyring
#   MUVRIX_SIGNING_KEY_FILE   path to an armored private key to import (optional)
#   MUVRIX_SIGNING_PASSPHRASE passphrase for that key (optional)
#
# When no key is configured the script does NOT fabricate a signature: it writes
# SIGNING_STATUS=pending and exits 0, so an unsigned build is still auditable.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PKG_DIR="${1:-$REPO_ROOT/target/packages}"

cd "$PKG_DIR"

if [ ! -f SHA256SUMS ]; then
    echo "[-] SHA256SUMS missing in $PKG_DIR; run packaging/build_all_packages.sh first" >&2
    exit 1
fi

rm -f SHA256SUMS.asc muvrix-release-key.asc SIGNING_STATUS

if [ -z "${MUVRIX_SIGNING_KEY:-}" ]; then
    cat > SIGNING_STATUS <<EOF
status=pending
reason=no signing key configured (set MUVRIX_SIGNING_KEY)
manifest=SHA256SUMS
manifest_sha256=$(sha256sum SHA256SUMS | cut -d' ' -f1)
signed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
EOF
    echo "[!] Signing status: PENDING (no key configured)."
    echo "[!] SHA256SUMS is unsigned; nothing was fabricated."
    if [ -n "${CI:-}" ]; then
        echo "[!] In CI this is expected until the release signing secret is provisioned."
    fi
    exit 0
fi

if ! command -v gpg >/dev/null 2>&1; then
    echo "[-] gpg is required to sign the release manifest" >&2
    exit 1
fi

GNUPGHOME_DIR="${GNUPGHOME:-$(mktemp -d)}"
export GNUPGHOME="$GNUPGHOME_DIR"
trap 'rm -rf "$GNUPGHOME_DIR"' EXIT

if [ -n "${MUVRIX_SIGNING_KEY_FILE:-}" ]; then
    gpg --batch --import "$MUVRIX_SIGNING_KEY_FILE" 2>/dev/null
fi

if [ -n "${MUVRIX_SIGNING_PASSPHRASE:-}" ]; then
    printf '%s' "$MUVRIX_SIGNING_PASSPHRASE" > "$GNUPGHOME_DIR/passphrase"
    SIGN_ARGS=(--pinentry-mode loopback --passphrase-file "$GNUPGHOME_DIR/passphrase")
else
    SIGN_ARGS=(--pinentry-mode loopback --passphrase "")
fi

gpg --batch --yes "${SIGN_ARGS[@]}" --local-user "$MUVRIX_SIGNING_KEY" \
    --armor --detach-sign --output SHA256SUMS.asc SHA256SUMS
gpg --batch --verify SHA256SUMS.asc SHA256SUMS 2>/dev/null || {
    echo "[-] Freshly produced signature does not verify; refusing to publish it" >&2
    exit 1
}

gpg --batch --armor --export "$MUVRIX_SIGNING_KEY" > muvrix-release-key.asc

cat > SIGNING_STATUS <<EOF
status=signed
key=$(gpg --batch --with-colons --list-keys "$MUVRIX_SIGNING_KEY" 2>/dev/null | awk -F: '/^fpr:/ {print $10; exit}')
manifest=SHA256SUMS
manifest_sha256=$(sha256sum SHA256SUMS | cut -d' ' -f1)
signature_sha256=$(sha256sum SHA256SUMS.asc | cut -d' ' -f1)
signed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
EOF

echo "[+] Signed SHA256SUMS -> SHA256SUMS.asc"
echo "[+] Exported public key -> muvrix-release-key.asc"
cat SIGNING_STATUS
