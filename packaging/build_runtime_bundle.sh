#!/usr/bin/env bash
# Muvrix private runtime bundle builder.
#
# Muvrix requires libmpv 0.42 (client API 2.6). No current distribution ships
# that version together with a libc old enough to run on the supported range:
# Ubuntu 24.04 has libmpv 0.37, Fedora has 0.38-0.41, and Debian sid's 0.42 is
# built against glibc 2.43, so it cannot be borrowed for older hosts.
#
# This script builds the media stack from pinned upstream sources inside
# Ubuntu 24.04 (glibc 2.39 baseline) and produces a private runtime payload:
#
#   libmpv 0.42.0        (mpv, -Dlibmpv=true)
#   libplacebo 7.387.0   (built from source; Ubuntu's 6.338 is too old)
#   ffmpeg 6.1           (Ubuntu 24.04 system libraries, bundled privately)
#   + the remaining transitive closure
#
# The payload is shipped under /opt/muvrix/lib (deb/rpm), /app/lib/muvrix
# (Flatpak) and usr/lib/muvrix (AppImage). Host-provided libraries are excluded
# on purpose: glibc and the loader, OpenSSL, D-Bus, and the graphics driver
# stack (EGL/GL/Wayland/DRM/Vulkan), which must match the running system.
#
# Builds are reproducible from the pinned versions and verify every archive
# checksum before compiling.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

OUT_DIR="target/runtime"
LIB_DIR="$OUT_DIR/lib"
LICENSE_DIR="$OUT_DIR/share/licenses"
BUILD_IMAGE="${MUVRIX_RUNTIME_IMAGE:-muvrix-runtime-builder:local}"

MPV_VERSION="0.42.0"
MPV_URL="https://github.com/mpv-player/mpv/archive/refs/tags/v${MPV_VERSION}.tar.gz"
MPV_SHA256="bcfebbfa2b65a6d88dc97ea18117831072373e38b5785a2afb874ed4df5070dd"

WAYLAND_VERSION="1.23.1"
WAYLAND_URL="https://gitlab.freedesktop.org/wayland/wayland/-/releases/${WAYLAND_VERSION}/downloads/wayland-${WAYLAND_VERSION}.tar.xz"
WAYLAND_SHA256="864fb2a8399e2d0ec39d56e9d9b753c093775beadc6022ce81f441929a81e5ed"

PLACEBO_VERSION="7.387.0"
PLACEBO_COMMIT="275f7dea27924cdd34ab2e6c20e448b6bc4fab65"
PLACEBO_REPO="https://github.com/haasn/libplacebo.git"

if [ "${1:-}" = "--force" ]; then
    rm -rf "$OUT_DIR"
fi

echo "=== Building Muvrix private runtime (libmpv ${MPV_VERSION}) ==="

if ! command -v podman >/dev/null 2>&1; then
    echo "[-] podman is required to build the private runtime" >&2
    exit 1
fi

rm -rf "$OUT_DIR"
mkdir -p "$LIB_DIR" "$LICENSE_DIR"

# Build environment: one pinned Ubuntu image with every build dependency, so
# repeated builds (and CI) do not re-resolve packages from the network.
if ! podman image exists "$BUILD_IMAGE" 2>/dev/null; then
    echo "[*] Building runtime builder image ($BUILD_IMAGE)..."
    podman build -q -t "$BUILD_IMAGE" \
        -f packaging/containers/Containerfile.runtime packaging/containers >/dev/null
fi

# Libraries that must come from the host/runtime. Anything matching is not
# bundled: glibc and the loader, OpenSSL, D-Bus, and the graphics driver stack.
DENYLIST_PATTERNS='^(ld-linux.*|libc|libm|libmvec|libresolv|libpthread|librt|libdl|libutil|libnsl|libgcc_s|libz|libssl|libcrypto|libdbus-1|libEGL|libGL|libGLX|libGLdispatch|libOpenGL|libxkbcommon.*|libdrm|libgbm|libvulkan|libva|libva-drm|libva-x11|libva-wayland|libsystemd|libudev|libcap|libselinux|liblzma|libzstd|libgcrypt|libgpg-error|libnettle|libhogweed|libgnutls|libidn2|libunistring|libtasn1|libp11-kit|libffi|libexpat|libelf|libmount|libblkid|libuuid|libattr|libacl)\.so'

podman run --rm -v "$REPO_ROOT:/workspace:z" -w /workspace \
    -e MPV_VERSION="$MPV_VERSION" -e MPV_URL="$MPV_URL" -e MPV_SHA256="$MPV_SHA256" \
    -e PLACEBO_VERSION="$PLACEBO_VERSION" -e PLACEBO_COMMIT="$PLACEBO_COMMIT" \
    -e PLACEBO_REPO="$PLACEBO_REPO" -e DENYLIST_PATTERNS="$DENYLIST_PATTERNS" \
    -e WAYLAND_VERSION="$WAYLAND_VERSION" -e WAYLAND_URL="$WAYLAND_URL" -e WAYLAND_SHA256="$WAYLAND_SHA256" \
    "$BUILD_IMAGE" bash -c '
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
PREFIX=/opt/muvrix

echo "[*] Building inside the pinned Ubuntu 24.04 runtime builder image" >&2

fetch() {
    url="$1"; sha="$2"; dest="$3"
    curl -fsSL -o "$dest" "$url"
    echo "$sha  $dest" | sha256sum -c - >/dev/null
}

cd /tmp
echo "[*] Fetching pinned sources" >&2
fetch "$MPV_URL" "$MPV_SHA256" mpv.tar.gz
fetch "$WAYLAND_URL" "$WAYLAND_SHA256" wayland.tar.xz
tar xzf mpv.tar.gz
tar xJf wayland.tar.xz
MPV_SRC="mpv-${MPV_VERSION}"

echo "[*] Building libwayland ${WAYLAND_VERSION} (mpv needs wayland-client >= 1.23)" >&2
cd "/tmp/wayland-${WAYLAND_VERSION}"
meson setup build --buildtype=release --prefix="$PREFIX" \
    -Ddocumentation=false -Dtests=false
ninja -C build
ninja -C build install
export PATH="$PREFIX/bin:$PATH"
export PKG_CONFIG_PATH="$PREFIX/lib/x86_64-linux-gnu/pkgconfig:$PREFIX/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
export LD_LIBRARY_PATH="$PREFIX/lib/x86_64-linux-gnu:$PREFIX/lib:${LD_LIBRARY_PATH:-}"

# libplacebo vendors glad/SPIRV as git submodules, so the release tarball
# cannot build its OpenGL backend. Clone the pinned commit with submodules and
# verify the commit afterwards.
echo "[*] Fetching libplacebo ${PLACEBO_VERSION} (${PLACEBO_COMMIT}) with submodules" >&2
git clone --quiet --branch "v${PLACEBO_VERSION}" --depth 1 --recurse-submodules \
    "$PLACEBO_REPO" /tmp/libplacebo-src
git -C /tmp/libplacebo-src submodule update --init --recursive --depth 1 >/dev/null 2>&1
ACTUAL_COMMIT="$(git -C /tmp/libplacebo-src rev-parse HEAD)"
if [ "$ACTUAL_COMMIT" != "$PLACEBO_COMMIT" ]; then
    echo "[-] libplacebo commit mismatch: expected $PLACEBO_COMMIT, got $ACTUAL_COMMIT" >&2
    exit 1
fi
PLACEBO_SRC="libplacebo-src"

echo "[*] Building libplacebo ${PLACEBO_VERSION}" >&2
cd "/tmp/$PLACEBO_SRC"
meson setup build --buildtype=release --prefix="$PREFIX" \
    -Ddemos=false -Dtests=false -Dbench=false -Dfuzz=false \
    -Dvulkan=enabled -Dshaderc=enabled -Dglslang=enabled \
    -Dlcms=enabled -Dd3d11=disabled -Dopengl=enabled
ninja -C build
ninja -C build install

echo "[*] Building libmpv ${MPV_VERSION}" >&2
cd "/tmp/$MPV_SRC"
export PKG_CONFIG_PATH="$PREFIX/lib/x86_64-linux-gnu/pkgconfig:$PREFIX/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
export LD_LIBRARY_PATH="$PREFIX/lib/x86_64-linux-gnu:$PREFIX/lib:${LD_LIBRARY_PATH:-}"
meson setup build --buildtype=release --prefix="$PREFIX" \
    -Dlibmpv=true -Dcplayer=false -Dbuild-date=false \
    -Dmanpage-build=disabled -Dhtml-build=disabled -Dpdf-build=disabled \
    -Dtests=false -Dvapoursynth=disabled -Djavascript=enabled -Dlua=luajit \
    -Dx11=enabled -Dwayland=enabled
ninja -C build
ninja -C build install

LIBMPV="$(readlink -f "$PREFIX/lib/x86_64-linux-gnu/libmpv.so.2")"
echo "[*] Built libmpv: $LIBMPV ($(readelf -d "$LIBMPV" | sed -n "s/.*SONAME.*\[\(.*\)\].*/\1/p"))" >&2

DST=/workspace/target/runtime/lib
mkdir -p "$DST"

CLOSURE="$(lddtree -l "$LIBMPV")"
COUNT=0
for path in $CLOSURE; do
    base="$(basename "$path")"
    [[ "$base" =~ $DENYLIST_PATTERNS ]] && continue
    [ -f "$path" ] || continue
    cp -L "$path" "$DST/$base"
    chmod 0644 "$DST/$base"
    COUNT=$((COUNT + 1))
done
echo "[*] Bundled $COUNT shared objects" >&2

# SONAME links (lddtree resolves libmpv.so.2.6.0, consumers open libmpv.so.2).
LINKED=0
for file in "$DST"/*.so*; do
    [ -f "$file" ] || continue
    [ -L "$file" ] && continue
    soname="$(readelf -d "$file" 2>/dev/null | sed -n "s/.*SONAME.*\[\(.*\)\].*/\1/p" | head -1)"
    [ -n "$soname" ] || continue
    if [ "$soname" != "$(basename "$file")" ]; then
        ln -sf "$(basename "$file")" "$DST/$soname"
        LINKED=$((LINKED + 1))
    fi
done
echo "[*] Created $LINKED SONAME links" >&2

# License texts for everything redistributed, plus the two source builds.
for path in $CLOSURE; do
    base="$(basename "$path")"
    [[ "$base" =~ $DENYLIST_PATTERNS ]] && continue
    [ -f "/workspace/target/runtime/lib/$base" ] || continue
    pkg="$({ dpkg -S "$path" 2>/dev/null || true; } | cut -d: -f1 | head -1)"
    [ -n "$pkg" ] || continue
    dest="/workspace/target/runtime/share/licenses/$pkg"
    if [ ! -d "$dest" ] && [ -f "/usr/share/doc/$pkg/copyright" ]; then
        mkdir -p "$dest"
        cp "/usr/share/doc/$pkg/copyright" "$dest/copyright"
    fi
done
mkdir -p "/workspace/target/runtime/share/licenses/mpv" "/workspace/target/runtime/share/licenses/libplacebo"
cp "/tmp/$MPV_SRC/Copyright" "/workspace/target/runtime/share/licenses/mpv/Copyright" 2>/dev/null || \
    cp "/tmp/$MPV_SRC/LICENSE" "/workspace/target/runtime/share/licenses/mpv/LICENSE" 2>/dev/null || true
cp "/tmp/$PLACEBO_SRC/LICENSE" "/workspace/target/runtime/share/licenses/libplacebo/LICENSE" 2>/dev/null || true
python3 - "$PREFIX" <<"PY"
import hashlib, json, os, subprocess, sys

prefix = sys.argv[1]
root = "/workspace/target/runtime"
lib = os.path.join(root, "lib")

files = []
for name in sorted(os.listdir(lib)):
    path = os.path.join(lib, name)
    # isfile() follows symlinks, so SONAME aliases (e.g. libmpv.so.2 ->
    # libmpv.so.2.6.0) are inventoried too. They are what the dynamic loader
    # actually resolves, so a runtime inventory that omitted them would be
    # incomplete. Broken links are skipped.
    if not os.path.isfile(path):
        continue
    digest = hashlib.sha256(open(path, "rb").read()).hexdigest()
    pkg, ver = None, None
    for candidate in (path.replace("/workspace/target/runtime", ""), f"/usr/lib/x86_64-linux-gnu/{name}"):
        out = subprocess.run(["dpkg", "-S", candidate], capture_output=True, text=True).stdout
        if out:
            pkg = out.split(":")[0].strip()
            ver = subprocess.run(["dpkg-query", "-W", "-f=${Version}", pkg],
                                 capture_output=True, text=True).stdout.strip()
            break
    if pkg is None:
        pkg = "muvrix-source-build"
        ver = "0.42.0" if name.startswith("libmpv") else "7.387.0"
    entry = {"soname": name, "package": pkg, "version": ver,
             "sha256": digest, "size": os.path.getsize(path)}
    if os.path.islink(path):
        entry["symlink_to"] = os.readlink(path)
    files.append(entry)

def dpkg_version(pkg):
    return subprocess.run(["dpkg-query", "-W", "-f=${Version}", pkg],
                          capture_output=True, text=True).stdout.strip()

manifest = {
    "schema": "muvrix-runtime-bundle/2",
    "baseline_glibc": "2.39 (Ubuntu 24.04)",
    "libmpv": {"version": "0.42.0", "built_from": "mpv v0.42.0 source", "client_api": "2.6"},
    "libplacebo": {"version": "7.387.0", "built_from": "libplacebo v7.387.0 source"},
    "libwayland": {"version": "1.23.1", "built_from": "wayland 1.23.1 source (mpv requires wayland-client >= 1.23)"},
    "ffmpeg": {"version": dpkg_version("libavcodec-dev"), "source": "Ubuntu 24.04 archive"},
    "bundled_libraries": len(files),
    "excluded": "glibc/loader, OpenSSL, D-Bus, EGL/GL/DRM/Vulkan drivers, VA-API client library",
    "files": files,
}
with open(os.path.join(root, "runtime-manifest.json"), "w", encoding="utf-8") as fh:
    json.dump(manifest, fh, indent=2, sort_keys=True)
    fh.write("\n")
print(f"[*] Manifest written: {len(files)} libraries")
PY
'

TOTAL=$(du -sh "$OUT_DIR" | cut -f1)
echo "[+] Runtime bundle ready: $OUT_DIR ($TOTAL)"
python3 -c "
import json
m = json.load(open('$OUT_DIR/runtime-manifest.json'))
print('[+] libmpv', m['libmpv']['version'], '| libplacebo', m['libplacebo']['version'],
      '| ffmpeg', m['ffmpeg']['version'], '| libraries:', m['bundled_libraries'])
"
