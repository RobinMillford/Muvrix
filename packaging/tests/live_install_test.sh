#!/usr/bin/env bash
# Real installation and launch tests for the Muvrix release artifacts.
#
# These tests install the package in a disposable container (or, for Flatpak,
# in an isolated user installation), start it against a headless Wayland
# compositor, require a rendered window capture, then remove the package and
# verify the removal was clean.
#
# Usage: packaging/tests/live_install_test.sh <deb|rpm|appimage|flatpak>
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=wayland_harness.sh
. "$SCRIPT_DIR/wayland_harness.sh"

FORMAT="${1:-}"
PKG_DIR="$REPO_ROOT/target/packages"
RESULTS_DIR="${MUVRIX_TEST_RESULTS:-$REPO_ROOT/target/test-results/$FORMAT}"
WL_DIR="$(mktemp -d "/tmp/muvrix-wl-$FORMAT.XXXXXX")"
CAPTURE_DIR="$(mktemp -d "/tmp/muvrix-shot-$FORMAT.XXXXXX")"
mkdir -p "$RESULTS_DIR"

STATUS="FAIL"
CLEANED="no"

cleanup() {
    stop_weston "$WL_DIR"
    rm -rf "$WL_DIR"
}
trap cleanup EXIT

fail() {
    echo "RESULT=$FORMAT status=FAIL reason=$1"
    exit 1
}

require_artifact() {
    local path="$1"
    [ -f "$path" ] || fail "artifact_missing:$(basename "$path")"
}

start_display() {
    start_weston "$WL_DIR" || fail "weston_unavailable"
    echo "[*] Wayland socket: $WL_DIR/wayland-0"
}

DEB="$PKG_DIR/muvrix_0.1.0_amd64.deb"
RPM="$PKG_DIR/muvrix-0.1.0-1.x86_64.rpm"
APPIMAGE="$PKG_DIR/Muvrix-0.1.0-x86_64.AppImage"
FLATPAK="$PKG_DIR/org.muvrix.Muvrix.flatpak"

case "$FORMAT" in
deb)
    require_artifact "$DEB"
    start_display
    podman run --rm -v "$REPO_ROOT:/workspace:z" -v "$WL_DIR:$WL_DIR:z" \
        -v "$CAPTURE_DIR:/out:z" -w /workspace ubuntu:24.04 bash -c "
set -e
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq >/dev/null 2>&1
apt-get install -y -qq --no-install-recommends weston libgl1-mesa-dri >/dev/null 2>&1
echo '[*] Installing .deb with apt (dependency resolution enabled)'
apt-get install -y -qq '/workspace/target/packages/$(basename "$DEB")' >/tmp/apt.log 2>&1 || { tail -20 /tmp/apt.log; exit 10; }
dpkg-query -W -f='installed=\${Version}\n' muvrix
echo '[*] Host libmpv present?' \$(ldconfig -p | grep -c 'libmpv' || true)
XDG_RUNTIME_DIR=$WL_DIR WAYLAND_DISPLAY=wayland-0 \
  MUVRIX_CAPTURE_WINDOW_PNG=/out/deb-launch.png MUVRIX_CAPTURE_AND_EXIT=1 \
  muvrix > /tmp/app.log 2>&1
echo launch_exit_code=\$?
tail -5 /tmp/app.log
ls -l /out
apt-get remove -y -qq muvrix >/dev/null 2>&1
test ! -e /usr/bin/muvrix && echo UNINSTALL_CLEAN
" || fail "deb_container_failed"
    ;;
rpm)
    require_artifact "$RPM"
    start_display
    podman run --rm -v "$REPO_ROOT:/workspace:z" -v "$WL_DIR:$WL_DIR:z" \
        -v "$CAPTURE_DIR:/out:z" -w /workspace fedora:40 bash -c "
set -e
echo '[*] Installing .rpm with dnf (dependency resolution enabled)'
dnf install -y -q '/workspace/target/packages/$(basename "$RPM")' >/tmp/dnf.log 2>&1 || { tail -20 /tmp/dnf.log; exit 10; }
rpm -q muvrix
dnf install -y -q weston mesa-dri-drivers >/tmp/dnf2.log 2>&1 || { tail -20 /tmp/dnf2.log; exit 11; }
echo '[*] Host libmpv present?' \$(ldconfig -p | grep -c 'libmpv' || true)
XDG_RUNTIME_DIR=$WL_DIR WAYLAND_DISPLAY=wayland-0 \
  MUVRIX_CAPTURE_WINDOW_PNG=/out/rpm-launch.png MUVRIX_CAPTURE_AND_EXIT=1 \
  muvrix > /tmp/app.log 2>&1
echo launch_exit_code=\$?
tail -5 /tmp/app.log
ls -l /out
dnf remove -y -q muvrix >/dev/null 2>&1
test ! -e /usr/bin/muvrix && echo UNINSTALL_CLEAN
" || fail "rpm_container_failed"
    ;;
appimage)
    require_artifact "$APPIMAGE"
    start_display
    podman run --rm -v "$REPO_ROOT:/workspace:z" -v "$WL_DIR:$WL_DIR:z" \
        -v "$CAPTURE_DIR:/out:z" -w /workspace ubuntu:24.04 bash -c "
set -e
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq >/dev/null 2>&1
apt-get install -y -qq --no-install-recommends weston libgl1-mesa-dri libegl1 libgl1 \
    libdbus-1-3 libssl3 libwayland-client0 libwayland-egl1 libxkbcommon0 libxkbcommon-x11-0 \
    libwayland-server0 libdrm2 >/dev/null 2>&1
echo '[*] Host libmpv present?' \$(ldconfig -p | grep -c 'libmpv' || true)
cp '/workspace/target/packages/$(basename "$APPIMAGE")' /tmp/muvrix.AppImage
chmod +x /tmp/muvrix.AppImage
/tmp/muvrix.AppImage --appimage-extract >/dev/null
XDG_RUNTIME_DIR=$WL_DIR WAYLAND_DISPLAY=wayland-0 APPIMAGE_EXTRACT_AND_RUN=1 \
  MUVRIX_CAPTURE_WINDOW_PNG=/out/appimage-launch.png MUVRIX_CAPTURE_AND_EXIT=1 \
  /tmp/muvrix.AppImage > /tmp/app.log 2>&1
echo launch_exit_code=\$?
tail -5 /tmp/app.log
ls -l /out
rm -rf /tmp/muvrix.AppImage squashfs-root
" || fail "appimage_container_failed"
    ;;
flatpak)
    require_artifact "$FLATPAK"
    start_display
    # An isolated *user* installation. This tree holds the whole Freedesktop
    # runtime (~1.5 GB), so it goes on the real filesystem rather than /tmp,
    # which is a small tmpfs on many hosts.
    export FLATPAK_USER_DIR="$REPO_ROOT/target/flatpak-user-test"
    rm -rf "$FLATPAK_USER_DIR"
    mkdir -p "$FLATPAK_USER_DIR"
    echo "[*] Isolated flatpak user dir: $FLATPAK_USER_DIR"
    flatpak --user remote-add --if-not-exists flathub \
        https://dl.flathub.org/repo/flathub.flatpakrepo >/dev/null 2>&1 || \
        flatpak remote-add --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
    echo '[*] Installing org.freedesktop.Platform 25.08'
    flatpak --user install -y --noninteractive flathub \
        org.freedesktop.Platform/x86_64/25.08 >/tmp/flatpak-runtime.log 2>&1 || {
            tail -20 /tmp/flatpak-runtime.log; fail "runtime_install_failed"; }
    echo '[*] Installing Muvrix bundle'
    flatpak --user install -y --noninteractive --bundle "$FLATPAK" >/tmp/flatpak-install.log 2>&1 || {
        tail -20 /tmp/flatpak-install.log; fail "bundle_install_failed"; }
    flatpak --user info org.muvrix.Muvrix | sed -n '1,8p'
    # Flatpak proxies the compositor socket through the sandbox runtime dir, so
    # the harness runtime dir is handed to flatpak itself and --socket=wayland
    # forwards wayland-0 from there.
    XDG_RUNTIME_DIR="$WL_DIR" flatpak --user run --nosocket=x11 --socket=wayland \
        --filesystem="$CAPTURE_DIR" \
        --env=WAYLAND_DISPLAY=wayland-0 \
        --env=MUVRIX_CAPTURE_WINDOW_PNG="$CAPTURE_DIR/flatpak-launch.png" \
        --env=MUVRIX_CAPTURE_AND_EXIT=1 \
        org.muvrix.Muvrix > /tmp/flatpak-run.log 2>&1
    echo "launch_exit_code=$?"
    tail -10 /tmp/flatpak-run.log
    ls -l "$CAPTURE_DIR"
    flatpak --user uninstall -y org.muvrix.Muvrix >/dev/null 2>&1
    flatpak --user list --app --columns=application | grep -qx org.muvrix.Muvrix && fail "uninstall_incomplete" || echo UNINSTALL_CLEAN
    ;;
*)
    echo "Usage: $0 <deb|rpm|appimage|flatpak>" >&2
    exit 2
    ;;
esac

# Verify the launch produced a real window capture.
SHOT="$(ls "$CAPTURE_DIR"/*.png 2>/dev/null | head -1)"
if [ -z "$SHOT" ]; then
    fail "no_window_capture"
fi
SHOT_SIZE=$(stat -c%s "$SHOT")
if [ "$SHOT_SIZE" -lt 10000 ]; then
    fail "capture_too_small:$SHOT_SIZE"
fi
SHOT_SHA="$(sha256sum "$SHOT" | cut -d' ' -f1)"
cp "$SHOT" "$RESULTS_DIR/$(basename "$SHOT")"

cat > "$RESULTS_DIR/result.txt" <<EOF
format=$FORMAT
status=PASS
capture=$(basename "$SHOT")
capture_bytes=$SHOT_SIZE
capture_sha256=$SHOT_SHA
tested_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
EOF

echo "RESULT=$FORMAT status=PASS capture=$SHOT_SHA bytes=$SHOT_SIZE"
