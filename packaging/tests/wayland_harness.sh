#!/usr/bin/env bash
# Wayland test harness shared by the release verification scripts.
#
# Provides a headless Wayland compositor for launch tests. If weston is
# installed on the host it is started directly; otherwise the
# muvrix-weston:local container image is used, writing its socket into a
# host-visible directory so a sandboxed Flatpak or an installed package can
# connect to it.
#
# Usage (source this file):
#   start_weston <runtime-dir>
#   stop_weston <runtime-dir>
#   WAYLAND_SOCKET_PATH=<runtime-dir>/wayland-0
set -uo pipefail

WESTON_IMAGE="${WESTON_IMAGE:-muvrix-weston:local}"
WESTON_CONTAINER=""
WESTON_PID=""

start_weston() {
    local dir="$1"
    mkdir -p "$dir"
    chmod 700 "$dir"
    rm -f "$dir/wayland-0" "$dir/wayland-0.lock"

    if command -v weston >/dev/null 2>&1; then
        XDG_RUNTIME_DIR="$dir" weston --backend=headless-backend.so \
            --socket=wayland-0 --idle-time=0 --log="$dir/weston.log" >/dev/null 2>&1 &
        WESTON_PID=$!
    else
        if ! command -v podman >/dev/null 2>&1; then
            echo "[-] No weston and no podman: cannot create a Wayland test display" >&2
            return 1
        fi
        if ! podman image exists "$WESTON_IMAGE" 2>/dev/null; then
            podman build -q -t "$WESTON_IMAGE" \
                -f packaging/containers/Containerfile.weston packaging/containers >/dev/null || return 1
        fi
        # Rootless podman maps container root to the invoking user, so files
        # and the Wayland socket land on the host owned by us and are writable
        # by the package-under-test containers later.
        #
        # --security-opt label=disable is required on SELinux hosts (Fedora,
        # RHEL, CentOS). A confined container_t compositor has the file
        # descriptors it receives over the Wayland socket dropped by the
        # SELinux fd-passing check: wl_shm.create_pool arrives without its
        # payload fd and the compositor reports
        #   "libwayland: file descriptor expected, object (N), message create_pool(nhi)"
        # Verified on Fedora 45 (enforcing, targeted): the unconfined
        # compositor receives every fd, the confined one receives none. This
        # relaxes only the *test compositor* container; the application under
        # test keeps its own Flatpak/package sandbox unchanged.
        WESTON_CONTAINER="muvrix-weston-$$-$(date +%s)"
        podman run -d --rm --name "$WESTON_CONTAINER" \
            --security-opt label=disable \
            -v "$dir:/wl:z" \
            -e XDG_RUNTIME_DIR=/wl \
            "$WESTON_IMAGE" --backend=headless-backend.so \
            --socket=wayland-0 --idle-time=0 --log=/wl/weston.log >/dev/null || return 1
    fi

    local i
    for i in $(seq 1 120); do
        [ -S "$dir/wayland-0" ] && return 0
        sleep 1
    done
    echo "[-] weston did not create $dir/wayland-0" >&2
    [ -f "$dir/weston.log" ] && tail -20 "$dir/weston.log" >&2
    return 1
}

stop_weston() {
    local dir="${1:-}"
    if [ -n "$WESTON_CONTAINER" ]; then
        podman rm -f "$WESTON_CONTAINER" >/dev/null 2>&1 || true
        WESTON_CONTAINER=""
    fi
    if [ -n "$WESTON_PID" ]; then
        kill "$WESTON_PID" >/dev/null 2>&1 || true
        wait "$WESTON_PID" 2>/dev/null || true
        WESTON_PID=""
    fi
    [ -n "$dir" ] && rm -f "$dir/wayland-0" "$dir/wayland-0.lock" 2>/dev/null || true
    return 0
}
