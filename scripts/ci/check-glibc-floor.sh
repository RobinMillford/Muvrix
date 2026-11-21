#!/usr/bin/env bash
# ELF portability gate.
#
# Fails when a packaged executable or bundled library requires a glibc symbol
# version newer than the declared supported baseline. Without this, a build that
# happens to run on a new distribution produces a package that will not start on
# an older one, and nothing reports it until a user does.
#
# Usage: scripts/ci/check-glibc-floor.sh <max-glibc> <path> [<path>...]
#   e.g. scripts/ci/check-glibc-floor.sh 2.35 target/packages/staging
#
# Exit codes: 0 = every object is within the floor, 1 = a violation, 2 = usage
# or tooling error (treated as failure by CI so a silent skip cannot pass).
set -euo pipefail

if [ "$#" -lt 2 ]; then
    echo "usage: $0 <max-glibc, e.g. 2.35> <path>..." >&2
    exit 2
fi

FLOOR="$1"
shift

if ! command -v readelf >/dev/null 2>&1; then
    echo "[-] readelf not found; cannot verify the glibc floor" >&2
    exit 2
fi

# Collect regular files and symlink targets that are ELF objects, skipping the
# dynamic loader and the C library itself: bundling or measuring those is
# meaningless, and they legitimately define the highest versions.
mapfile -t FILES < <(
    find "$@" -type f 2>/dev/null | while read -r f; do
        case "$(basename "$f")" in
            ld-linux*|ld-musl*|libc.so*|libc-*.so|libm.so*|libpthread.so*|libdl.so*)
                continue ;;
        esac
        if head -c 4 "$f" 2>/dev/null | grep -q $'\x7fELF'; then
            printf '%s\n' "$f"
        fi
    done
)

if [ "${#FILES[@]}" -eq 0 ]; then
    echo "[-] no ELF objects found under: $*" >&2
    exit 2
fi

echo "==> glibc floor check: maximum permitted GLIBC_$FLOOR"
echo "==> inspecting ${#FILES[@]} ELF objects"

FAILED=0
for f in "${FILES[@]}"; do
    # Highest GLIBC_x.y version this object requires.
    maxreq="$(
        readelf --version-info "$f" 2>/dev/null \
        | grep -oE 'GLIBC_[0-9]+\.[0-9]+' \
        | sed 's/GLIBC_//' \
        | sort -uV \
        | tail -1
    )"
    [ -n "$maxreq" ] || continue

    # sort -V comparison: is maxreq greater than FLOOR?
    if [ "$(printf '%s\n%s\n' "$FLOOR" "$maxreq" | sort -V | tail -1)" != "$FLOOR" ]; then
        echo "  VIOLATION $maxreq  $f"
        # Name the offending symbols so the cause is actionable rather than a
        # bare version number.
        readelf --version-info "$f" 2>/dev/null \
            | grep -E "GLIBC_${maxreq}\b" \
            | sed 's/^/            /' | head -5
        FAILED=1
    fi
done

if [ "$FAILED" -ne 0 ]; then
    echo
    echo "[-] One or more objects require glibc newer than the $FLOOR baseline." >&2
    echo "    Rebuild inside the baseline container (packaging/containers/) rather" >&2
    echo "    than on a newer host, or bump the baseline and every package's" >&2
    echo "    declared dependency together." >&2
    exit 1
fi

echo "==> OK: all objects within GLIBC_$FLOOR"
