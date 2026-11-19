#!/usr/bin/env bash
# Muvrix target-distribution native binary builder.
#
# Release packages must contain a binary compiled against the libraries and C
# library ABI of the distribution that will install them. Building on the
# development host (for example Fedora 45) produces a binary requiring
# GLIBC_2.43 and libssl.so.4, which cannot start on Ubuntu 24.04 or Fedora 40.
#
# Usage: packaging/build_native_binary.sh <deb|rpm> [--force]
# Prints the absolute path of the resulting muvrix-ui binary.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

TARGET_KIND="${1:-}"
FORCE="${2:-}"

case "$TARGET_KIND" in
    deb)
        IMAGE="ubuntu:24.04"
        TAG="ubuntu-24.04"
        INSTALL_CMD='export DEBIAN_FRONTEND=noninteractive
            apt-get update -qq
            apt-get install -y -qq build-essential pkg-config cmake curl \
                libmpv-dev libegl1-mesa-dev libgl1-mesa-dev libwayland-dev \
                libxkbcommon-dev libx11-dev libdbus-1-dev libssl-dev libasound2-dev'
        ;;
    rpm)
        IMAGE="fedora:40"
        TAG="fedora-40"
        INSTALL_CMD='dnf install -y -q gcc gcc-c++ make cmake pkgconf-pkg-config curl tar gzip \
                mpv-libs-devel mesa-libEGL-devel mesa-libGL-devel wayland-devel \
                libxkbcommon-devel libX11-devel dbus-devel openssl-devel alsa-lib-devel'
        ;;
    *)
        echo "[-] Usage: $0 <deb|rpm> [--force]" >&2
        exit 2
        ;;
esac

OUTPUT_DIR="$REPO_ROOT/target/native/$TAG"
OUTPUT_BIN="$OUTPUT_DIR/muvrix-ui"

if [ -f "$OUTPUT_BIN" ] && [ "$FORCE" != "--force" ]; then
    echo "[*] Reusing cached $TAG native binary: $OUTPUT_BIN" >&2
    printf '%s\n' "$OUTPUT_BIN"
    exit 0
fi

if ! command -v podman >/dev/null 2>&1; then
    echo "[-] Podman is required to build a $TAG native binary on this host." >&2
    echo "[-] Refusing to package a host binary for a different distribution ABI." >&2
    exit 1
fi

mkdir -p "$OUTPUT_DIR"
echo "[*] Compiling muvrix-ui inside $IMAGE (this may take several minutes)..." >&2

podman run --rm \
    -v "$REPO_ROOT:/workspace:z" \
    -w /workspace \
    -e CARGO_TARGET_DIR="/workspace/target/native/$TAG/build" \
    "$IMAGE" bash -c "
        set -e
        $INSTALL_CMD
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
            sh -s -- -y --profile minimal --default-toolchain stable >/dev/null
        . \"\$HOME/.cargo/env\"
        cargo build --release --locked -p muvrix-ui
        cp \"\$CARGO_TARGET_DIR/release/muvrix-ui\" '/workspace/target/native/$TAG/muvrix-ui'
    " >&2

if [ ! -f "$OUTPUT_BIN" ]; then
    echo "[-] $TAG build did not produce $OUTPUT_BIN" >&2
    exit 1
fi

echo "[+] Built $TAG native binary: $OUTPUT_BIN" >&2
printf '%s\n' "$OUTPUT_BIN"
