# Installing Muvrix

This guide covers all supported installation paths for Muvrix.

## Quick reference

| Method | Command | Notes |
|---|---|---|
| One-command | `curl -fsSL https://raw.githubusercontent.com/RobinMillford/Muvrix/main/install.sh \| sh` | Detects host, installs Flatpak |
| Flatpak | `flatpak install flathub org.muvrix.Muvrix` | Recommended; sandboxed |
| DEB | `sudo dpkg -i muvrix_0.1.0_amd64.deb` | Debian/Ubuntu |
| RPM | `sudo rpm -i muvrix-0.1.0-1.x86_64.rpm` | Fedora/RHEL |
| AppImage | `chmod +x Muvrix-0.1.0-x86_64.AppImage && ./Muvrix-0.1.0-x86_64.AppImage` | Portable |
| Source | See below | For contributors |

## Prerequisites

- **Flatpak** (recommended): install from your distribution's repositories, or
  follow the official setup at <https://flatpak.org/setup>.
- **DEB**: a Debian- or Ubuntu-based system with `sudo dpkg -i` access.
- **RPM**: a Fedora- or RHEL-based system with `sudo rpm -i` or `sudo dnf install`.
- **AppImage**: any Linux x86-64 with a working FUSE or `--appimage-extract-and-run`.

## Flatpak

```sh
flatpak remote-add --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install flathub org.muvrix.Muvrix
flatpak run org.muvrix.Muvrix
```

The Flatpak bundles a private libmpv 0.42.0 runtime; you do not need to install
mpv or FFmpeg separately.

### Updating

```sh
flatpak update org.muvrix.Muvrix
```

### Uninstalling

```sh
flatpak uninstall org.muvrix.Muvrix
```

User data under `~/.var/app/org.muvrix.Muvrix/` is preserved by default. To
remove it as well:

```sh
rm -rf ~/.var/app/org.muvrix.Muvrix
```

## DEB

```sh
sudo dpkg -i muvrix_0.1.0_amd64.deb
```

If you have dependency issues:

```sh
sudo apt-get install -f
```

The package installs `/usr/bin/muvrix`, pulls in required system libraries
(SDL via mpv, Wayland/X11 client libs, VA-API client libs), and installs the
desktop entry under `/usr/share/applications/org.muvrix.Muvrix.desktop`.

## RPM

```sh
sudo rpm -i muvrix-0.1.0-1.x86_64.rpm
```

Or with dnf:

```sh
sudo dnf install ./muvrix-0.1.0-1.x86_64.rpm
```

## AppImage

```sh
chmod +x Muvrix-0.1.0-x86_64.AppImage
./Muvrix-0.1.0-x86_64.AppImage
```

If AppImage extraction or FUSE fails:

```sh
./Muvrix-0.1.0-x86_64.AppImage --appimage-extract-and-run
```

## From source

Requirements:

- Rust stable >= 1.75
- git
- FFmpeg development libraries
- libmpv development libraries (or build them -- see `scripts/ci/build-mpv-0.42.sh`)

```sh
git clone https://github.com/RobinMillford/Muvrix.git
cd Muvrix
cargo build --release -p muvrix-ui
./target/release/muvrix-ui
```

## Uninstalling a source build

```sh
cargo clean
rm -rf target/
```

## Troubleshooting

**Binary won't start: missing libmpv.**

The DEB/RPM packages declare libmpv as a dependency. If you built from source,
run with `LD_LIBRARY_PATH=/path/to/mpv/lib` or install libmpv 0.42.0 from your
packager.

**AppImage fails with FUSE error.**

Use `--appimage-extract-and-run` or install `fuse`/`fuse2` from your
distribution.

**Black video on AMD/Intel with VA-API.**

Run with `MUVRIX_MPV_LOG=v` and look for `hwdec-current` in the log. If it
reports `vaapi` and the video is still black, try `hwdec=vaapi-copy` as a
diagnostic fallback.
