#!/usr/bin/env bash
# ==============================================================================
# Muvrix Phase 1.4 — pinned libmpv 0.42.0 source build for Linux CI.
#
# Distro packages do not yet provide Client API 2.6:
#   - Ubuntu 24.04 (noble) ships mpv 0.37.0 (API 2.2)
#   - Fedora 45 ships mpv 0.41.0 (API 2.5)
# so CI builds the exact upstream tag into a local prefix and runs the
# workspace tests against it. The same recipe is documented in
# docs/phase-1.4/phase-1.4-engine-baseline-manifest.md.
#
# Usage:
#   MPV_PREFIX=/opt/mpv042 scripts/ci/build-mpv-0.42.sh <workdir>
#
# Environment:
#   MPV_PREFIX  install prefix (default /opt/mpv042)
# ==============================================================================

set -euo pipefail

MPV_TAG="v0.42.0"
# Full upstream commit resolved for the tag:
#   git ls-remote https://github.com/mpv-player/mpv.git refs/tags/v0.42.0^{}
MPV_COMMIT="b0841e8c4d156b8403dc1876c4cbd9011eb583f3"
LIBPLACEBO_TAG="v7.387.0"
LIBPLACEBO_COMMIT="275f7dea27924cdd34ab2e6c20e448b6bc4fab65"
MPV_PREFIX="${MPV_PREFIX:-/opt/mpv042}"
WORKDIR="${1:-$(pwd)}"

echo "=== Muvrix pinned engine build (Phase 1.4) ==="
echo "mpv tag:        ${MPV_TAG}"
echo "mpv commit:     ${MPV_COMMIT}"
echo "libplacebo:     ${LIBPLACEBO_TAG} (${LIBPLACEBO_COMMIT})"
echo "prefix:         ${MPV_PREFIX}"

# Required build tools and development packages must already be installed by
# the caller (see the CI workflow for the apt/dnf package list):
#   meson ninja pkg-config ffmpeg dev headers libass dev wayland-protocols
#   libdrm-dev libva-dev vulkan headers (vulkan-headers / libvulkan-dev)

mkdir -p "${MPV_PREFIX}"

if [ ! -d "${WORKDIR}/mpv" ]; then
    git clone --depth 1 --branch "${MPV_TAG}" https://github.com/mpv-player/mpv.git "${WORKDIR}/mpv"
fi
cd "${WORKDIR}/mpv"
RESOLVED="$(git rev-parse HEAD)"
if [ "${RESOLVED}" != "${MPV_COMMIT}" ]; then
    echo "ERROR: mpv tag resolved to ${RESOLVED}, expected ${MPV_COMMIT}"
    exit 1
fi
echo "Verified mpv HEAD: ${RESOLVED}"

if [ ! -d "${WORKDIR}/libplacebo" ]; then
    git clone --depth 1 --branch "${LIBPLACEBO_TAG}" https://code.videolan.org/videolan/libplacebo.git "${WORKDIR}/libplacebo"
fi
cd "${WORKDIR}/libplacebo"
git submodule update --init --depth 1
RESOLVED_PL="$(git rev-parse HEAD)"
if [ "${RESOLVED_PL}" != "${LIBPLACEBO_COMMIT}" ]; then
    echo "ERROR: libplacebo tag resolved to ${RESOLVED_PL}, expected ${LIBPLACEBO_COMMIT}"
    exit 1
fi
echo "Verified libplacebo HEAD: ${RESOLVED_PL}"

echo "Building libplacebo ${LIBPLACEBO_TAG}..."
meson setup "${WORKDIR}/libplacebo/build" --prefix="${MPV_PREFIX}" --buildtype=release
ninja -C "${WORKDIR}/libplacebo/build"
ninja -C "${WORKDIR}/libplacebo/build" install

export PKG_CONFIG_PATH="${MPV_PREFIX}/lib64/pkgconfig:${MPV_PREFIX}/lib/pkgconfig:${PKG_CONFIG_PATH:-}"

echo "Building mpv ${MPV_TAG} (libmpv only, cplayer disabled)..."
meson setup "${WORKDIR}/mpv/build" --prefix="${MPV_PREFIX}" --buildtype=release -Dcplayer=false
ninja -C "${WORKDIR}/mpv/build"
ninja -C "${WORKDIR}/mpv/build" install

echo "=== Engine build complete ==="
echo "mpv.pc version: $(pkg-config --modversion mpv)"
echo "mpv.pc path:     $(pkg-config --variable=libdir mpv)/libmpv.so.2"
PKG_CONFIG_PATH="${MPV_PREFIX}/lib64/pkgconfig:${PKG_CONFIG_PATH:-}" pkg-config --modversion libplacebo
