#!/usr/bin/env bash
# Installer behavior tests (mocked).
#
# These tests exercise control-flow paths in install.sh without modifying the
# host. Package managers and flatpak are replaced by instrumented stubs on
# PATH. A real installation is covered by live_install_test.sh against a
# disposable container; this suite never installs or removes system software.
#
# Run: bash packaging/tests/install_test.sh
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
INSTALLER="$REPO_ROOT/install.sh"
BUNDLE="$REPO_ROOT/target/packages/org.muvrix.Muvrix.flatpak"

pass=0
fail=0

check() {
  local name="$1"; shift
  if "$@"; then
    printf 'ok   %s\n' "$name"
    pass=$((pass + 1))
  else
    printf 'FAIL %s\n' "$name"
    fail=$((fail + 1))
  fi
}

make_testbed() {
  local with_flatpak="${1:-with}" root="${2:-no}" no_sudo="${3:-no}" apt_stub="${4:-no}" curl_stub="${5:-no}"
  local d
  d=$(mktemp -d)
  mkdir -p "$d/bin" "$d/var"
  printf '' > "$d/calls"
  printf '' > "$d/log"
  local uid=1000
  if [ "$root" = yes ]; then uid=0; fi
  cat > "$d/bin/id" <<EOF
#!/usr/bin/env bash
echo $uid
EOF
  chmod +x "$d/bin/id"
  cat > "$d/bin/uname" <<'EOF'
#!/usr/bin/env bash
if [ "$1" = "-m" ]; then echo x86_64; exit 0; fi
echo Linux
EOF
  chmod +x "$d/bin/uname"
  if [ "$no_sudo" = no ]; then
    cat > "$d/bin/sudo" <<'EOF'
#!/usr/bin/env bash
shift; exec "$@"
EOF
    chmod +x "$d/bin/sudo"
  fi
  if [ "$apt_stub" = yes ]; then
    cat > "$d/bin/apt" <<'EOF'
#!/usr/bin/env bash
printf 'apt %s\n' "$*" >> "$d/calls"
exit 0
EOF
    chmod +x "$d/bin/apt"
  fi
  if [ "$curl_stub" = yes ]; then
    cat > "$d/bin/curl" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
    chmod +x "$d/bin/curl"
  fi
  for name in sh env bash awk tr grep cat mktemp mkdir rm chmod sha256sum; do
    src="/usr/bin/$name"
    [ -e "$src" ] && ln -sf "$src" "$d/bin/$name"
  done
  if [ "$with_flatpak" = with ]; then
    cat > "$d/bin/flatpak" <<EOF
#!/usr/bin/env bash
printf 'flatpak %s\n' "\$*" >> $d/calls
case "\$1" in
  info)
    if [ -f $d/var/installed ]; then echo "Commit: deadbeef"; exit 0; fi
    echo "not installed" >&2; exit 1 ;;
  uninstall)
    rm -f $d/var/installed; exit 0 ;;
  remotes)
    printf 'flathub\n'; exit 0 ;;
  install)
    touch $d/var/installed; exit 0 ;;
  remote-add)
    exit 0 ;;
esac
exit 0
EOF
    chmod +x "$d/bin/flatpak"
  fi
  printf '%s' "$d"
}

run() {
  local d="$1"; shift
  PATH="$d/bin" sh "$INSTALLER" "$@" > "$d/log" 2>&1
}

# 1. --help exits 0
test_help() {
  local d; d=$(make_testbed with no no no no)
  run "$d" --help && grep -q "Usage:" "$d/log"
}

# 2. --version prints 0.1.0
test_version() {
  local d; d=$(make_testbed with no no no no)
  run "$d" --version && [ "$(cat "$d/log")" = "0.1.0" ]
}

# 3. unknown option -> non-zero
test_unknown_option() {
  local d; d=$(make_testbed with no no no no)
  run "$d" --bogus; local rc=$?
  [ "$rc" -ne 0 ] && grep -q "unknown option" "$d/log"
}

# 4. uninstall preserves user data path in output
test_uninstall_preserves_data() {
  local d; d=$(make_testbed with no no no no)
  touch "$d/var/installed"
  run "$d" --uninstall && grep -q ".var/app/org.muvrix.Muvrix" "$d/log"
}

# 5. --release pin accepted
test_release_pin() {
  local d; d=$(make_testbed with no no yes yes)
  run "$d" --dry-run --release 0.1.0 && grep -q "v0.1.0" "$d/log"
}

# 6. --dry-run prints commands without invoking flatpak
test_dry_run_prints_commands() {
  local d; d=$(make_testbed with no no no no)
  printf 'installed' > "$d/var/installed"
  run "$d" --dry-run --from-local "$BUNDLE"
  grep -q "\[dry-run\]" "$d/log" && grep -q "flatpak install" "$d/log"
}

# 7. --from-local with nonexistent bundle fails clean
test_missing_local_bundle() {
  local d; d=$(make_testbed with no no no no)
  run "$d" --from-local /no/such.bundle
  local rc=$?
  [ "$rc" -ne 0 ] && grep -q "does not exist" "$d/log"
}

# 8. --system with root allowed (dry-run)
test_system_root_allowed() {
  local d; d=$(make_testbed with yes no no no)
  printf 'installed' > "$d/var/installed"
  run "$d" --system --dry-run --from-local "$BUNDLE"
  local rc=$?
  [ "$rc" -eq 0 ] && grep -q "scope=system" "$d/log"
}

# 9. --system without privilege and without sudo blocked
test_system_blocked() {
  local d; d=$(make_testbed with no yes no no)
  run "$d" --system --dry-run
  local rc=$?
  [ "$rc" -ne 0 ] && grep -q "administrator" "$d/log"
}

# 10. dry-run with flatpak missing prints apt attempt (Linux Debian/Ubuntu/Fedora fn)
test_apt_attempt_when_flatpak_missing() {
  local d; d=$(make_testbed without no no yes no)
  run "$d" --dry-run
  grep -q "apt" "$d/log"
}

# 11. uninstall when not installed
test_uninstall_when_not_installed() {
  local d; d=$(make_testbed without no no no no)
  run "$d" --uninstall
  local rc=$?
  [ "$rc" -eq 0 ]
}

# 12. --from-local with real bundle (dry-run)
test_real_bundle_dry_run() {
  local d; d=$(make_testbed with no no no no)
  run "$d" --dry-run --from-local "$BUNDLE"
  local rc=$?
  [ "$rc" -eq 0 ] && grep -q "installing from local bundle" "$d/log"
}

check "help" test_help
check "version" test_version
check "unknown option" test_unknown_option
check "uninstall preserves data" test_uninstall_preserves_data
check "release pin" test_release_pin
check "dry-run prints commands" test_dry_run_prints_commands
check "missing local bundle" test_missing_local_bundle
check "system root allowed" test_system_root_allowed
check "system blocked without privilege" test_system_blocked
check "apt attempt when flatpak missing" test_apt_attempt_when_flatpak_missing
check "uninstall when not installed" test_uninstall_when_not_installed
check "real bundle dry-run" test_real_bundle_dry_run

printf '\npassed=%d failed=%d\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
