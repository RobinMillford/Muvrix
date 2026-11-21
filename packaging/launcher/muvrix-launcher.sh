#!/bin/sh
# Muvrix launcher.
#
# The player requires libmpv 0.42 (client API 2.6), which no supported
# distribution ships. The private runtime installed next to the binary provides
# libmpv 0.42 plus the ffmpeg/libplacebo stack; glibc, the graphics drivers and
# the sound server are always taken from the host so driver negotiation stays
# intact. Nothing is installed into the system library path.
#
# The executable links libmpv.so.2 directly, so if that soname cannot be
# resolved the dynamic loader terminates the process before main() runs and
# MUVRIX's own API check never executes. This script is therefore the last place
# a useful message can be produced, and it is produced here rather than left to
# the loader's "error while loading shared libraries".
set -eu

MUVRIX_LIBDIR="@LIBDIR@"
MUVRIX_BINARY="@BINARY@"

if [ ! -x "$MUVRIX_BINARY" ]; then
    echo "muvrix: missing executable $MUVRIX_BINARY" >&2
    echo "muvrix: this installation looks incomplete or was moved; reinstall the package." >&2
    exit 1
fi

LD_LIBRARY_PATH="${MUVRIX_LIBDIR}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export LD_LIBRARY_PATH

# Preflight: confirm the loader can actually resolve libmpv before exec, so the
# failure is explained in MUVRIX's own words with an install hint, instead of a
# bare loader error. `ldd` is the same resolution the loader will perform, with
# the same LD_LIBRARY_PATH. If ldd is unavailable the check is skipped: it is a
# diagnostic, never a gate on systems that lack it.
if command -v ldd >/dev/null 2>&1; then
    missing="$(ldd "$MUVRIX_BINARY" 2>/dev/null | awk '/not found/ {print $1}' | sort -u)"
    if [ -n "$missing" ]; then
        {
            echo "muvrix: cannot start - required shared libraries are missing:"
            echo "$missing" | sed 's/^/  - /'
            echo
            echo "muvrix looked in: $MUVRIX_LIBDIR"
            echo
            echo "If libmpv is listed above, the private runtime did not install."
            echo "Reinstall the MUVRIX package, or install your distribution's"
            echo "mpv/libmpv package if you built MUVRIX without the bundled runtime."
            echo
            echo "Diagnostics: MUVRIX_DEBUG=1 muvrix"
        } >&2
        exit 1
    fi
fi

exec "$MUVRIX_BINARY" "$@"
