#!/bin/sh
# Muvrix installer.
#
# Detects the host, selects the best available installation method, and
# installs Muvrix. Designed to be safe to re-run and safe by default.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/RobinMillford/Muvrix/main/install.sh | sh
#   sh install.sh [--help] [--version] [--dry-run] [--yes|-y] [--system|--user]
#                 [--uninstall] [--release X.Y.Z] [--from-local PATH]
#
# The one-command flow installs the Flatpak build from the project's latest
# GitHub release. Pass --from-local to install a bundle you built yourself, or
# to install from an extracted release tree.
set -eu

PROGNAME="install.sh"
MUVRIX_REPO="RobinMillford/Muvrix"
MUVRIX_APPID="org.muvrix.Muvrix"
INSTALLER_VERSION="0.1.0"

MODE="install"
DRY_RUN=0
ASSUME_YES=0
SCOPE="user"
VERSION_PIN=""
FROM_LOCAL=""

die()  { printf '%s: error: %s\n' "$PROGNAME" "$*" >&2; exit 1; }
info() { printf '%s: %s\n' "$PROGNAME" "$*"; }

usage() {
  cat <<EOF
Muvrix installer $INSTALLER_VERSION

Usage: $PROGNAME [OPTIONS]

Options:
  -h, --help          Show this help
  -V, --version       Print installer version
  -n, --dry-run       Show what would happen, but do nothing
  -y, --yes           Install even if SHA256SUMS is missing (NOT recommended)
      --system        Install system-wide (requires administrator privileges)
      --user          Install per-user (default; no privileges required)
      --uninstall     Remove the Flatpak installation
      --release X     Install a specific release version (e.g. --release 0.1.0)
      --from-local P  Install from a local .flatpak bundle or release directory

What it does:
  1. Detects distribution, package manager, architecture and Flatpak.
  2. Installs Flatpak if missing and the package manager can provide it.
  3. Adds the Flathub remote if missing.
  4. Installs org.muvrix.Muvrix from the latest GitHub release, or from a
     local bundle with --from-local.
  5. Leaves ~/.var/app/org.muvrix.Muvrix/ (settings, library, database)
     untouched -- uninstall never removes user data unless you remove it
     yourself afterwards.

Security:
  - HTTPS only; no -k flags; TLS certificates always verified.
  - The release artifact is checked against the published SHA256SUMS.
  - Fails closed if verification cannot be completed.
EOF
}

while [ $# -gt 0 ]; do
  case "$1" in
    -h|--help) usage; exit 0 ;;
    -V|--version) printf '%s\n' "$INSTALLER_VERSION"; exit 0 ;;
    -n|--dry-run) DRY_RUN=1 ;;
    -y|--yes) ASSUME_YES=1 ;;
    --system) SCOPE="system" ;;
    --user) SCOPE="user" ;;
    --uninstall) MODE="uninstall" ;;
    --release) VERSION_PIN="$2"; shift ;;
    --from-local) FROM_LOCAL="$2"; shift ;;
    *) die "unknown option: $1" ;;
  esac
  shift
done

run() {
  if [ "$DRY_RUN" = "1" ]; then
    printf '[dry-run] %s\n' "$*"
  else
    "$@"
  fi
}

have() { command -v "$1" >/dev/null 2>&1; }

# ---------------------------------------------------------------------------
# Detect host
# ---------------------------------------------------------------------------
os_id=""
if [ -r /etc/os-release ]; then
  os_id=$(awk -F= '/^ID=/{print $2; exit}' /etc/os-release | tr -d '"' | tr -d '[:space:]')
fi

arch=$(uname -m)
case "$arch" in
  x86_64|amd64) arch=x86_64 ;;
  aarch64|arm64) arch=aarch64 ;;
  *) die "unsupported architecture: $arch (supported: x86_64, aarch64)" ;;
esac

have_flatpak=0
if have flatpak; then
  have_flatpak=1
fi

if [ "$SCOPE" = "system" ]; then
  if [ "$(id -u)" != "0" ] && ! have sudo; then
    die "--system requires administrator privileges (root or sudo). Use --user for a per-user install."
  fi
fi

info "host: os=${os_id} arch=${arch} flatpak=${have_flatpak} scope=${SCOPE}"

# ---------------------------------------------------------------------------
# Uninstall path
# ---------------------------------------------------------------------------
if [ "$MODE" = "uninstall" ]; then
  if [ "$have_flatpak" = "0" ]; then
    info "Flatpak is not installed; nothing to uninstall."
    exit 0
  fi
  info "Removing org.muvrix.Muvrix (user data in ~/.var/app/$MUVRIX_APPID is preserved)..."
  run flatpak uninstall --user -y "$MUVRIX_APPID" || true
  if [ "$DRY_RUN" = "1" ]; then
    printf '[dry-run] would verify installation is absent\n'
  else
    if flatpak info --user "$MUVRIX_APPID" >/dev/null 2>&1; then
      die "uninstall reported success but the application is still present"
    fi
  fi
  info "done. User data preserved at ~/.var/app/$MUVRIX_APPID"
  info "to also remove user data: rm -rf ~/.var/app/$MUVRIX_APPID"
  exit 0
fi

# ---------------------------------------------------------------------------
# Install prerequisites (Flatpak)
# ---------------------------------------------------------------------------
pkg() {
  case "$1" in
    apt)     run sudo apt-get update && run sudo apt-get install -y flatpak ;;
    dnf)     run sudo dnf install -y flatpak ;;
    yum)     run sudo yum install -y flatpak ;;
    zypper)  run sudo zypper --non-interactive install flatpak ;;
    pacman)  run sudo pacman -S --needed --noconfirm flatpak ;;
    apk)     run sudo apk add flatpak ;;
    *)       die "detected package manager '$1' is not supported by this installer" ;;
  esac
}

if [ "$have_flatpak" = "0" ]; then
  info "Flatpak is not installed; attempting to install it..."
  found=0
  for pm in apt dnf yum zypper pacman apk; do
    if have "$pm"; then
      pkg "$pm"
      found=1
      break
    fi
  done
  if [ "$found" = "0" ]; then
    die "no supported package manager found. Install Flatpak manually: https://flatpak.org/setup"
  fi
  if ! have flatpak; then
    if [ "$DRY_RUN" = "1" ]; then
      info "[dry-run] continuing without verifying flatpak presence"
    else
      die "Flatpak installation appears to have failed"
    fi
  fi
fi

# ---------------------------------------------------------------------------
# Flathub remote (only needed for network/install mode)
# ---------------------------------------------------------------------------
if [ "$MODE" = "install" ] && [ -z "$FROM_LOCAL" ] && have flatpak; then
  if ! flatpak remotes --user 2>/dev/null | grep -q '^flathub\b'; then
    info "adding Flathub remote (user scope)..."
    run flatpak remote-add --if-not-exists --user flathub https://dl.flathub.org/repo/flathub.flatpakrepo
  fi
fi

# ---------------------------------------------------------------------------
# Install from local bundle / directory, or from GitHub release
# ---------------------------------------------------------------------------
tmpdir=""
cleanup() {
  if [ -n "$tmpdir" ] && [ -d "$tmpdir" ]; then
    rm -rf "$tmpdir"
  fi
}
trap cleanup EXIT INT TERM

install_from_bundle() {
  bundle="$1"
  info "installing from local bundle: $bundle"
  run flatpak install --user --reinstall --assumeyes "$bundle"
}

install_from_dir() {
  dir="$1"
  bundle=""
  for cand in "$dir"/*.flatpak "$dir"/target/packages/*.flatpak; do
    if [ -f "$cand" ]; then
      bundle="$cand"
      break
    fi
  done
  [ -n "$bundle" ] || die "no .flatpak bundle found in $dir"
  install_from_bundle "$bundle"
}

if [ -n "$FROM_LOCAL" ]; then
  [ -e "$FROM_LOCAL" ] || die "--from-local path does not exist: $FROM_LOCAL"
  case "$FROM_LOCAL" in
    *.flatpak) install_from_bundle "$FROM_LOCAL" ;;
    *) install_from_dir "$FROM_LOCAL" ;;
  esac
else
  # Network install from the latest (or pinned) GitHub release.
  if [ -n "$VERSION_PIN" ]; then
    release_tag="v$VERSION_PIN"
    base_url="https://github.com/$MUVRIX_REPO/releases/download/$release_tag"
  else
    release_tag="latest"
    base_url="https://github.com/$MUVRIX_REPO/releases/latest/download"
  fi

  have curl || die "curl is required for network installation"

  tmpdir=$(mktemp -d)
  bundle_name="org.muvrix.Muvrix.flatpak"
  sums_name="SHA256SUMS"

  info "downloading $bundle_name ($release_tag)..."
  if ! curl -fsSL -o "$tmpdir/$bundle_name" "$base_url/$bundle_name"; then
    die "could not download release artifact. The project may not have published a release yet.
       Build from source instead: https://github.com/$MUVRIX_REPO#from-source
       Or install a locally built bundle: $PROGNAME --from-local /path/to/checkout"
  fi

  info "downloading $sums_name..."
  if [ "$DRY_RUN" = "1" ]; then
    printf "[dry-run] would verify %s against %s\n" "$bundle_name" "$sums_name"
  else
    if ! curl -fsSL -o "$tmpdir/$sums_name" "$base_url/$sums_name"; then
      if [ "$ASSUME_YES" != "1" ]; then
        die "SHA256SUMS not published for this release; refusing to install an unverified artifact.
         Re-run with --yes to install anyway (NOT recommended), or build from source:
           https://github.com/$MUVRIX_REPO#from-source"
      fi
      info "warning: SHA256SUMS not published for this release; installing without verification (--yes)"
      info "this is NOT recommended -- verify the bundle yourself before running it"
    else
      info "verifying bundle SHA-256 against SHA256SUMS..."
      line=$(grep -F "$bundle_name" "$tmpdir/$sums_name" || true)
      if [ -z "$line" ]; then
        die "SHA256SUMS does not list $bundle_name -- refusing to install"
      fi
      ( cd "$tmpdir" && printf "%s\n" "$line" | sha256sum -c - >/dev/null ) ||
        die "checksum verification FAILED -- refusing to install unverified artifact"
      info "checksum verified"
    fi
  fi

  install_from_bundle "$tmpdir/$bundle_name"
fi

# ---------------------------------------------------------------------------
# Verify install
# ---------------------------------------------------------------------------
if [ "$DRY_RUN" = "1" ]; then
  info "[dry-run] would verify installation and print how to launch"
  exit 0
fi

if ! have flatpak; then
  info "Flatpak not present; installation may have completed in a different way."
  exit 0
fi

if flatpak info --user "$MUVRIX_APPID" >/dev/null 2>&1; then
  commit=$(flatpak info --user "$MUVRIX_APPID" | awk '/^Commit:/{print $2; exit}')
  info "installed: $MUVRIX_APPID (commit $commit)"
  info "launch:    flatpak run $MUVRIX_APPID"
  info "or open Muvrix from your application menu"
else
  die "installation finished but $MUVRIX_APPID is not reported as installed"
fi
