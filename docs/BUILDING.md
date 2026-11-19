# Building Muvrix

This document describes how to build Muvrix from source.

## Prerequisites

### Linux (Debian/Ubuntu)

```sh
sudo apt update
sudo apt install build-essential pkg-config libmpv-dev libgl1-mesa-dev libegl1-mesa-dev libxkbcommon-dev libwayland-dev libx11-dev libdbus-1-dev libasound2-dev
```

### Linux (Fedora)

```sh
sudo dnf install gcc make pkg-config mpv-libs-devel mesa-libGL-devel mesa-libEGL-devel wayland-devel libxkbcommon-devel libX11-devel alsa-lib-devel dbus-devel
```

### macOS

```sh
brew install mpv pkg-config libgl
```

### Rust

Install Rust via rustup: <https://rustup.rs>

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

## Building

```sh
git clone https://github.com/RobinMillford/Muvrix.git
cd Muvrix
cargo build --release -p muvrix-ui
```

The binary is produced at `target/release/muvrix-ui`.

## Running from build

```sh
./target/release/muvrix-ui
```

## Running tests

```sh
cargo test --workspace
```

Tests that require hardware will skip if hardware is not available.

## Building with a locally built libmpv

If your distribution does not ship libmpv 0.42+, you can build it from source:

```sh
./scripts/ci/build-mpv-0.42.sh /tmp/engine
export PKG_CONFIG_PATH="/tmp/engine/lib64/pkgconfig:/tmp/engine/lib/pkgconfig:$PKG_CONFIG_PATH"
export LD_LIBRARY_PATH="/tmp/engine/lib64:$LD_LIBRARY_PATH"
cargo build --release -p muvrix-ui
```

## Packaging

See `docs/RELEASING.md` for packaging and release instructions.
