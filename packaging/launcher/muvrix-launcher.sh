#!/bin/sh
# Muvrix launcher.
#
# The player requires libmpv 0.42 (client API 2.6), which no supported
# distribution ships. The private runtime installed next to the binary provides
# libmpv 0.42 plus the ffmpeg/libplacebo stack; glibc, the graphics drivers and
# the sound server are always taken from the host so driver negotiation stays
# intact. Nothing is installed into the system library path.
set -eu

MUVRIX_LIBDIR="@LIBDIR@"
MUVRIX_BINARY="@BINARY@"

if [ ! -x "$MUVRIX_BINARY" ]; then
    echo "muvrix: missing executable $MUVRIX_BINARY" >&2
    exit 1
fi

LD_LIBRARY_PATH="${MUVRIX_LIBDIR}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export LD_LIBRARY_PATH
exec "$MUVRIX_BINARY" "$@"
