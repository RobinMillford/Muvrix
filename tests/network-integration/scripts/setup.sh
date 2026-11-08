#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
FIXTURES_DIR="${BASE_DIR}/fixtures"

echo "=== [Muvrix Network Integration] Provisioning Test Infrastructure ==="

# 1. Generate Deterministic Test Media Fixtures
mkdir -p "${FIXTURES_DIR}/nested"

if [ ! -f "${FIXTURES_DIR}/sample.mp4" ]; then
    echo "Generating synthetic 5s H.264/AAC media fixture..."
    ffmpeg -y -v error \
        -f lavfi -i testsrc=duration=5:size=320x240:rate=30 \
        -f lavfi -i sine=frequency=1000:duration=5 \
        -c:v libx264 -pix_fmt yuv420p -c:a aac \
        "${FIXTURES_DIR}/sample.mp4"
fi

cp "${FIXTURES_DIR}/sample.mp4" "${FIXTURES_DIR}/nested/deep_video.mp4"
cp "${FIXTURES_DIR}/sample.mp4" "${FIXTURES_DIR}/nested/unicode_файл.mp4"
cp "${FIXTURES_DIR}/sample.mp4" "${FIXTURES_DIR}/nested/special [name] #1 + test.mp4"

cat << 'EOF' > "${FIXTURES_DIR}/document.txt"
Muvrix Deterministic Test Document for Remote Network Verification.
Protocol: WebDAV, SMB2/3, SFTP.
Validation: Byte-for-byte stream integrity.
EOF

cat << 'EOF' > "${FIXTURES_DIR}/subtitles.srt"
1
00:00:01,000 --> 00:00:04,000
Muvrix Remote Subtitle Stream
EOF

# 2. Build Container Images
echo "Building WebDAV test image..."
podman build -q -t muvrix-test-webdav "${BASE_DIR}/webdav"

echo "Building Samba test image..."
podman build -q -t muvrix-test-samba "${BASE_DIR}/samba"

echo "Building OpenSSH test image..."
podman build -q -t muvrix-test-openssh "${BASE_DIR}/openssh"

# 3. Teardown any existing instances
"${SCRIPT_DIR}/teardown.sh" >/dev/null 2>&1 || true

# 4. Start Containers Bound Exclusively to 127.0.0.1
echo "Starting rootless Podman containers on 127.0.0.1..."
podman run -d --name muvrix-test-webdav \
    -p 127.0.0.1:18080:8080 \
    -v "${FIXTURES_DIR}":/var/www/webdav:ro,z \
    muvrix-test-webdav >/dev/null

podman run -d --name muvrix-test-samba \
    -p 127.0.0.1:18445:445 \
    -v "${FIXTURES_DIR}":/srv/media:ro,z \
    -v "${FIXTURES_DIR}":/srv/guest_media:ro,z \
    muvrix-test-samba >/dev/null

podman run -d --name muvrix-test-openssh \
    -p 127.0.0.1:18222:2222 \
    -v "${FIXTURES_DIR}":/home/muvrix_sftp/media:ro,z \
    muvrix-test-openssh >/dev/null

# 5. Wait for Readiness
echo "Waiting for services on loopback..."
for port in 18080 18445 18222; do
    timeout=15
    while ! nc -z 127.0.0.1 "$port" 2>/dev/null; do
        sleep 0.2
        timeout=$((timeout - 1))
        if [ "$timeout" -le 0 ]; then
            echo "ERROR: Service on 127.0.0.1:$port failed to become responsive."
            podman logs "muvrix-test-$(case "$port" in 18080) echo "webdav";; 18445) echo "samba";; 18222) echo "openssh";; esac)"
            exit 1
        fi
    done
done

# 6. Extract dynamic OpenSSH host key fingerprint
FINGERPRINT=$(podman exec muvrix-test-openssh ssh-keygen -l -f /etc/ssh/ssh_host_ed25519_key.pub -E sha256 | awk '{print $2}')
echo "$FINGERPRINT" > "${BASE_DIR}/sftp_fingerprint.txt"
export MUVRIX_TEST_SFTP_FINGERPRINT="$FINGERPRINT"

echo "=== All services are ready on 127.0.0.1 ==="
echo "  WebDAV:  http://127.0.0.1:18080/webdav/ [authenticated]"
echo "  Samba:   smb://127.0.0.1:18445/media    [authenticated & guest]"
echo "  OpenSSH: sftp://127.0.0.1:18222/media   [authenticated password & pubkey]"
echo "  SFTP Host Key Fingerprint: $FINGERPRINT"
