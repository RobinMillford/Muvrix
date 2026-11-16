Name:           muvrix
Version:        0.1.0
Release:        1%{?dist}
Summary:        Modern native desktop media player

License:        MIT OR Apache-2.0
URL:            https://github.com/muvrix/muvrix-desktop
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  cargo
BuildRequires:  rust >= 1.75.0
BuildRequires:  mpv-libs-devel >= 0.35.0
BuildRequires:  libplacebo-devel
BuildRequires:  mesa-libEGL-devel
BuildRequires:  mesa-libGL-devel
BuildRequires:  alsa-lib-devel
BuildRequires:  pulseaudio-libs-devel
BuildRequires:  dbus-devel

Requires:       mpv-libs >= 0.35.0
Requires:       hicolor-icon-theme

%description
Muvrix is a high-performance desktop media player written in Rust and powered by libmpv.
It features hardware-accelerated presentation, 10-band pro audio DSP with Bauer crossfeed,
real-time video shaders and HDR calibration, SQLite-backed library indexing with FTS5 search,
gapless playlist playback, resilient HTTP/HLS streaming with auto-reconnect, and full D-Bus MPRIS v2 integration.

%prep
%autosetup

%build
cargo build --release --bin muvrix

%install
rm -rf %{buildroot}
install -d %{buildroot}%{_bindir}
install -p -m 755 target/release/muvrix %{buildroot}%{_bindir}/muvrix

install -d %{buildroot}%{_datadir}/applications
install -p -m 644 packaging/appimage/muvrix.desktop %{buildroot}%{_datadir}/applications/io.muvrix.Muvrix.desktop

install -d %{buildroot}%{_datadir}/metainfo
install -p -m 644 packaging/metainfo/org.muvrix.Muvrix.metainfo.xml %{buildroot}%{_datadir}/metainfo/org.muvrix.Muvrix.metainfo.xml

%files
%license LICENSE*
%{_bindir}/muvrix
%{_datadir}/applications/io.muvrix.Muvrix.desktop
%{_datadir}/metainfo/org.muvrix.Muvrix.metainfo.xml

%changelog
* Sun Nov 16 2026 Muvrix Desktop Team <dev@muvrix.org> - 0.1.0-1
- Initial release for Muvrix Phase D production baseline.
