#!/usr/bin/env bash
# Muvrix Flatpak Bundle Builder
#
# Produces a genuine single-file OSTree Flatpak bundle:
#   1. export the application tree into an OSTree archive repository
#   2. build a bundle with `flatpak build-bundle`
#   3. verify the bundle imports with `flatpak build-import-bundle`
#
# The sandbox metadata is generated from packaging/flatpak/org.muvrix.Muvrix.yaml
# so the manifest remains the single source of truth for the application ID,
# runtime, SDK and least-privilege finish-args.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "=== Building Muvrix Flatpak Bundle ==="
cd "$REPO_ROOT"

MANIFEST="packaging/flatpak/org.muvrix.Muvrix.yaml"
APP_ID="$(awk -F': *' '/^app-id:/ {print $2; exit}' "$MANIFEST")"
if [ "$APP_ID" != "org.muvrix.Muvrix" ]; then
    echo "[-] Unexpected app-id in manifest: '$APP_ID'" >&2
    exit 1
fi

VERSION="0.1.0"
OUTPUT_DIR="target/packages"
PKG_NAME="${APP_ID}.flatpak"
REF_NAME="${APP_ID}.flatpakref"
STAGING_DIR="target/package_staging/flatpak_root"
REPO_DIR="target/package_staging/flatpak_repo"

mkdir -p "$OUTPUT_DIR"
rm -rf "$STAGING_DIR" "$REPO_DIR"
mkdir -p "$STAGING_DIR/export/share/applications"
mkdir -p "$STAGING_DIR/export/share/metainfo"
mkdir -p "$STAGING_DIR/export/share/icons/hicolor/scalable/apps"
mkdir -p "$STAGING_DIR/files/bin" "$STAGING_DIR/files/libexec" "$STAGING_DIR/files/lib/muvrix"

# 1. Application binary. It is compiled against the glibc 2.39 baseline (Ubuntu
#    24.04), which the 25.08 runtime satisfies; a binary built on the Fedora
#    development host requires GLIBC_2.43 and cannot start inside the sandbox.
if [ -n "${MUVRIX_FLATPAK_BINARY:-}" ]; then
    BIN_SOURCE="$MUVRIX_FLATPAK_BINARY"
else
    BIN_SOURCE="$(bash packaging/build_native_binary.sh deb)"
fi
if [ ! -f "$BIN_SOURCE" ]; then
    echo "[-] Flatpak payload binary not found: $BIN_SOURCE" >&2
    exit 1
fi
cp "$BIN_SOURCE" "$STAGING_DIR/files/libexec/muvrix"
chmod 755 "$STAGING_DIR/files/libexec/muvrix"

# 1b. Private media runtime: the Flatpak runtime contains no libmpv at all.
if [ ! -d "target/runtime/lib" ]; then
    echo "[*] Private runtime missing; building it first..."
    bash packaging/build_runtime_bundle.sh
fi
mkdir -p "$STAGING_DIR/files/lib/muvrix/lib"
# The private runtime must not carry the Wayland client libraries. They are part
# of the graphics stack that build_runtime_bundle.sh documents as host/runtime
# provided, and inside Flatpak the 25.08 runtime already ships Wayland >= 1.23.
# Shipping the builder's copies makes the launcher's LD_LIBRARY_PATH shadow the
# runtime's newer libwayland-client/libwayland-egl, which makes glvnd load no
# EGL vendor at all and eglGetPlatformDisplay(Wayland) fail with
# EGL_BAD_PARAMETER (verified inside the sandbox).
EXCLUDED_WAYLAND=0
for lib in target/runtime/lib/*; do
    base="$(basename "$lib")"
    case "$base" in
        libwayland-*)
            EXCLUDED_WAYLAND=$((EXCLUDED_WAYLAND + 1))
            continue
            ;;
    esac
    cp -a "$lib" "$STAGING_DIR/files/lib/muvrix/lib/$base"
done
echo "[*] Left $EXCLUDED_WAYLAND Wayland client libraries to the Flatpak runtime"
cp -a target/runtime/share/licenses "$STAGING_DIR/files/lib/muvrix/licenses"
cp target/runtime/runtime-manifest.json "$STAGING_DIR/files/lib/muvrix/runtime-manifest.json"

# Launcher (sets LD_LIBRARY_PATH to the bundled runtime, then execs the binary).
sed -e 's|@LIBDIR@|/app/lib/muvrix/lib|' -e 's|@BINARY@|/app/libexec/muvrix|' \
    packaging/launcher/muvrix-launcher.sh > "$STAGING_DIR/files/bin/muvrix"
chmod 755 "$STAGING_DIR/files/bin/muvrix"

# 2. Export desktop entry, icons, and metainfo
sed -e 's|^Exec=.*|Exec=muvrix %U|' \
    -e 's|^Icon=.*|Icon=org.muvrix.Muvrix|' \
    -e 's|^StartupWMClass=.*|StartupWMClass=org.muvrix.Muvrix|' \
    packaging/appimage/muvrix.desktop > "$STAGING_DIR/export/share/applications/${APP_ID}.desktop"
chmod 644 "$STAGING_DIR/export/share/applications/${APP_ID}.desktop"

cp packaging/metainfo/org.muvrix.Muvrix.metainfo.xml "$STAGING_DIR/export/share/metainfo/${APP_ID}.metainfo.xml"
chmod 644 "$STAGING_DIR/export/share/metainfo/${APP_ID}.metainfo.xml"

cp assets/branding/muvrix-logo.svg "$STAGING_DIR/export/share/icons/hicolor/scalable/apps/${APP_ID}.svg"
chmod 644 "$STAGING_DIR/export/share/icons/hicolor/scalable/apps/${APP_ID}.svg"

for size in 16 24 32 48 64 128 256 512; do
    ICON_DIR="$STAGING_DIR/export/share/icons/hicolor/${size}x${size}/apps"
    mkdir -p "$ICON_DIR"
    if [ -f "assets/branding/icons/muvrix-${size}x${size}.png" ]; then
        cp "assets/branding/icons/muvrix-${size}x${size}.png" "$ICON_DIR/${APP_ID}.png"
        chmod 644 "$ICON_DIR/${APP_ID}.png"
    fi
done

# 3. Generate Flatpak metadata from the manifest finish-args.
python3 - "$MANIFEST" > "$STAGING_DIR/metadata" <<'PY'
import sys
import yaml

manifest_path = sys.argv[1]
with open(manifest_path, "r", encoding="utf-8") as handle:
    manifest = yaml.safe_load(handle)

app_id = manifest["app-id"]
runtime = manifest["runtime"]
runtime_version = str(manifest["runtime-version"])
sdk = manifest["sdk"]
command = manifest["command"]

shared, sockets, devices, filesystems = [], [], [], []
env_vars = []
bus_policy = []

for arg in manifest.get("finish-args", []):
    if arg.startswith("--share="):
        shared.append(arg.split("=", 1)[1])
    elif arg.startswith("--socket="):
        sockets.append(arg.split("=", 1)[1])
    elif arg.startswith("--device="):
        devices.append(arg.split("=", 1)[1])
    elif arg.startswith("--filesystem="):
        filesystems.append(arg.split("=", 1)[1])
    elif arg.startswith("--env="):
        # Metadata cannot express --env directly; it becomes [Environment].
        env_vars.append(arg.split("=", 1)[1])
    elif arg.startswith("--own-name="):
        bus_policy.append((arg.split("=", 1)[1], "own"))
    elif arg.startswith("--talk-name="):
        bus_policy.append((arg.split("=", 1)[1], "talk"))
    else:
        # No silent drops: an unmapped finish-arg would not reach the metadata.
        raise SystemExit(f"Unsupported finish-arg in manifest: {arg}")

# Least privilege guard: the whole home directory must not be shared.
if any(fs == "home" or fs.startswith("home:") for fs in filesystems):
    raise SystemExit("Manifest shares the whole home directory; refusing least-privilege build")

lines = [
    "[Application]",
    f"name={app_id}",
    f"runtime={runtime}/x86_64/{runtime_version}",
    f"sdk={sdk}/x86_64/{runtime_version}",
    f"command={command}",
    "",
    "[Context]",
]
if shared:
    lines.append("shared=" + ";".join(shared) + ";")
if sockets:
    lines.append("sockets=" + ";".join(sockets) + ";")
if devices:
    lines.append("devices=" + ";".join(devices) + ";")
if filesystems:
    lines.append("filesystems=" + ";".join(filesystems) + ";")
lines.append("")
if env_vars:
    lines.append("[Environment]")
    for item in env_vars:
        if "=" not in item:
            raise SystemExit(f"Malformed --env finish-arg: {item}")
        key, value = item.split("=", 1)
        lines.append(f"{key}={value}")
    lines.append("")
lines.append("[Session Bus Policy]")
for name, policy in bus_policy:
    lines.append(f"{name}={policy}")
print("\n".join(lines))
PY
chmod 644 "$STAGING_DIR/metadata"

# 4. Initialize an OSTree archive repository, export the application, and build
#    the single-file bundle with native Flatpak tooling.
run_flatpak() {
    if command -v flatpak >/dev/null 2>&1; then
        flatpak "$@"
    else
        local engine
        engine="$(command -v podman || command -v docker)" || {
            echo "[-] flatpak or a container engine is required" >&2
            exit 1
        }
        "$engine" run --rm -v "$REPO_ROOT:/workspace:z" -w /workspace alpine:3.20 \
            sh -c 'apk add --no-cache flatpak ostree >/dev/null 2>&1 && exec flatpak "$@"' sh "$@"
    fi
}

# OSTree commits carry a creation timestamp, which would otherwise make every
# bundle unique. Pin it the same way the deb/rpm/AppImage builds do so that two
# builds of an unchanged tree produce identical bundles.
if [ -z "${SOURCE_DATE_EPOCH:-}" ]; then
    SOURCE_DATE_EPOCH="$(git -C "$REPO_ROOT" log -1 --format=%ct 2>/dev/null || date +%s)"
fi
export SOURCE_DATE_EPOCH

# flatpak/ostree parse --timestamp as an ISO-8601 instant, not a raw epoch.
COMMIT_TIMESTAMP="$(date -u -d "@${SOURCE_DATE_EPOCH}" +%Y-%m-%dT%H:%M:%SZ)"

echo "[*] Exporting application tree into OSTree archive repository (timestamp $COMMIT_TIMESTAMP)..."
run_flatpak build-export --disable-sandbox --timestamp="$COMMIT_TIMESTAMP" \
    "$REPO_DIR" "$STAGING_DIR" stable

# The sandbox must actually carry the media runtime, otherwise the app cannot
# start inside Flatpak (the runtime provides no libmpv).
if [ ! -f "$STAGING_DIR/files/lib/muvrix/lib/libmpv.so.2" ]; then
    echo "[-] Flatpak tree is missing the bundled libmpv runtime" >&2
    exit 1
fi

if [ ! -d "$REPO_DIR/objects" ] || [ ! -d "$REPO_DIR/refs" ]; then
    echo "[-] OSTree repository was not created at $REPO_DIR" >&2
    exit 1
fi

echo "[*] Creating single-file Flatpak bundle..."
run_flatpak build-bundle "$REPO_DIR" "$OUTPUT_DIR/$PKG_NAME" "$APP_ID" stable

# 5. Verify the bundle is a real OSTree Flatpak, not an archive.
BUNDLE_MAGIC="$(head -c 8 "$OUTPUT_DIR/$PKG_NAME" | tr -d '\0')"
if [ "$BUNDLE_MAGIC" != "flatpak" ]; then
    echo "[-] $PKG_NAME is not an OSTree Flatpak bundle (header: '$BUNDLE_MAGIC')" >&2
    exit 1
fi

TEST_IMPORT_DIR="target/package_staging/flatpak_test_import"
rm -rf "$TEST_IMPORT_DIR"
mkdir -p "$TEST_IMPORT_DIR/objects" "$TEST_IMPORT_DIR/refs/heads" "$TEST_IMPORT_DIR/refs/mirrors" "$TEST_IMPORT_DIR/refs/remotes"
printf "[core]\nrepo_version=1\nmode=archive-z2\n" > "$TEST_IMPORT_DIR/config"
run_flatpak build-import-bundle "$TEST_IMPORT_DIR" "$OUTPUT_DIR/$PKG_NAME"
run_flatpak repo "$TEST_IMPORT_DIR" --branches
rm -rf "$TEST_IMPORT_DIR"

# 6. Generate and validate the .flatpakref descriptor.
cat <<EOF > "$OUTPUT_DIR/$REF_NAME"
[Flatpak Ref]
Name=${APP_ID}
Branch=stable
Title=Muvrix Media Player
Comment=Modern native desktop media player with advanced video shaders and pro audio DSP
Description=Muvrix is a fast, beautiful, and extensible native media player built in Rust and powered by libmpv.
Icon=https://raw.githubusercontent.com/RobinMillford/Muvrix/main/assets/branding/muvrix-logo.png
Url=https://repo.muvrix.org/flatpak/
RuntimeRepo=https://dl.flathub.org/repo/flathub.flatpakrepo
IsRuntime=false
EOF

for key in "Name=${APP_ID}" "Branch=stable" "IsRuntime=false" "RuntimeRepo=https://dl.flathub.org/repo/flathub.flatpakrepo"; do
    if ! grep -qxF "$key" "$OUTPUT_DIR/$REF_NAME"; then
        echo "[-] flatpakref descriptor missing: $key" >&2
        exit 1
    fi
done

BUNDLE_SIZE=$(stat -c%s "$OUTPUT_DIR/$PKG_NAME" 2>/dev/null || stat -f%z "$OUTPUT_DIR/$PKG_NAME")
echo "[+] Successfully generated Flatpak bundle: $OUTPUT_DIR/$PKG_NAME ($BUNDLE_SIZE bytes)"
echo "[+] Successfully generated Flatpak reference: $OUTPUT_DIR/$REF_NAME"
echo "[+] OSTree export, bundle import and flatpakref validation verified."
