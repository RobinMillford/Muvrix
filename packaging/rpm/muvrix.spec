# Muvrix RPM spec template.
#
# This file is a human-readable reference for the RPM packaging. The actual
# binary RPM is assembled by packaging/build_rpm.sh, which generates its
# buildroot spec on the fly with the correct paths and deterministic
# timestamps. This file documents the same metadata for reviewers.
#
# License: LGPL-2.1-or-later

Name:           muvrix
Version:        0.1.0
Release:        1%{?dist}
Summary:        Modern native desktop media player
License:        LGPL-2.1-or-later
URL:            https://github.com/RobinMillford/Muvrix
Source0:        muvrix-%{version}.tar.gz

# The private runtime (libmpv 0.42 + FFmpeg + libplacebo) is bundled under
# /usr/lib64/muvrix/ and is not declared as a dependency. See
# packaging/build_rpm.sh for the staged buildroot and verification flow.
Requires:       glibc >= 2.39, mesa-libEGL, mesa-libGL, dbus-libs, openssl-libs, hicolor-icon-theme, libva, libva-drm, libva-x11

%description
Muvrix is a high-performance desktop media player written in Rust and
powered by libmpv. Features include hardware-accelerated presentation,
10-band pro audio DSP with Bauer crossfeed, real-time video shaders and
HDR calibration, SQLite-backed library indexing with FTS5 search, gapless
playback, network streaming with auto-reconnect, and full D-Bus MPRIS v2
integration.

%changelog
* Tue Nov 17 2026 Muvrix Desktop Team <dev@muvrix.org> - 0.1.0-1
- Production release for Muvrix Phase D/E baseline.
