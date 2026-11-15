<div align="center">
  <img src="assets/branding/muvrix-logo.svg" width="140" height="140" alt="Muvrix Media Player Logo" />
  <h1>Muvrix Desktop</h1>
  <p><strong>Modern, high-performance native desktop media player built with Rust, egui, and upstream libmpv</strong></p>

  <p>
    <a href="https://github.com/itsyamin/muvrix-desktop/actions"><img src="https://img.shields.io/badge/CI-passing-brightgreen?style=flat-square" alt="CI Status" /></a>
    <a href="docs/PLATFORM-SUPPORT.md"><img src="https://img.shields.io/badge/Platform-Linux%20Wayland%20Beta-blue?style=flat-square" alt="Platform" /></a>
    <a href="docs/TESTING.md"><img src="https://img.shields.io/badge/Tests-389%2B%20passed-success?style=flat-square" alt="Test Suite" /></a>
    <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT%20%2F%20Apache--2.0-orange?style=flat-square" alt="License" /></a>
    <img src="https://img.shields.io/badge/Engine-libmpv%200.42.0%20%2F%20API%202.6-purple?style=flat-square" alt="Engine" />
  </p>
</div>

---

Muvrix is a modern desktop media player engineered for precision playback, zero-copy hardware acceleration, and seamless media organization. Built on top of upstream `libmpv 0.42.0` and a pure Rust/OpenGL rendering pipeline, Muvrix delivers rock-solid audio/video synchronization, wide-gamut HDR color management, customizable Anime4K GLSL shaders, and comprehensive desktop OS integrations.

---

## Documentation Index

- [**Building Guide**](docs/BUILDING.md) — Prerequisites, build commands, and environment setup for Linux, macOS, and Windows.
- [**Architecture Overview**](docs/ARCHITECTURE.md) — Four-crate architecture, rendering pipeline, threading model, and FFI boundaries.
- [**Features & Controls**](docs/FEATURES.md) — Playback controls, timeline scrubbing, track selection, keyboard shortcuts, and diagnostic HUD.
- [**Platform Support**](docs/PLATFORM-SUPPORT.md) — Operating system support, hardware decoding backends, and qualification status.
- [**Testing Guide**](docs/TESTING.md) — Quality gates, test execution, test fixture structure, and test suite breakdown.
- [**Development Roadmap**](docs/ROADMAP.md) — Project milestones from Phase 0.1 through Phase 2.0.
- [**Continuous Integration Pipeline**](.github/workflows/ci.yml) — GitHub Actions multi-platform matrix definition.

---

## Engineering Roadmap

| Phase | Milestone | Scope | Status |
| :--- | :--- | :--- | :--- |
| **Phase 0.1** | Rust Workspace & libmpv FFI | Workspace layout, C FFI bindings, handle lifecycle, Linux linking | **COMPLETE** |
| **Phase 0.2** | Rendering Prototype | eframe + egui_glow, off-screen FBO compositor, OpenGL 3.3+ render | **COMPLETE** |
| **Phase 0.3** | Playback Hardening | Controller state model, async dispatch, property observers, error safety | **COMPLETE** |
| **Phase 0.4** | Engine & HW Acceleration | Codec validation, VA-API copy-back, 4K60/HFR, telemetry HUD, 10 GUI flows | **COMPLETE** |
| **Phase 0.5** | Cross-Platform Feasibility | Multi-target compilation (Linux/Win/Mac), libmpv dependency strategy, CI | **COMPLETE** |
| **Phase 0.6** | Core Player Experience | Complete playback controls, timeline/scrubbing, volume/mute, track selection, vector icons | **COMPLETE** |
| **Phase 0.7** | Advanced Controls & History | Secondary subtitles, PiP mini-player, watch history & resume playback, subtitle styling | **COMPLETE** |
| **Phase 0.8** | Shaders & Media Library | Anime4K GLSL shaders, parametric 10-band equalizer, SQLite media library & background scanner | **COMPLETE** |
| **Phase 1.0** | Linux Wayland Beta & Packaging | Linux packaging pipeline, desktop integration, MPRIS, crash reporting, brand theme | **QUALIFIED BETA** |
| **Phase 2.0** | Ecosystem & Extensibility | Lua IPC bridge, SMB/SFTP/WebDAV remote browser, cross-platform installers | *In Progress* |

---

## Workspace Architecture

The workspace is organized into four distinct crates with clear architectural boundaries:

- [`crates/libmpv-sys`](crates/libmpv-sys/): Raw C FFI bindings to libmpv (`client.h`, `render.h`, `render_gl.h`, `render_d3d11.h`), cross-platform dynamic linker probe, and multi-platform `build.rs`.
- [`crates/muvrix-core`](crates/muvrix-core/): Platform detection (`PlatformInfo`, `SessionType`, `DistroInfo`), cross-platform path resolver, SQLite database (library, playlists, watch state), history store, lyrics parser, and network streaming client abstractions.
- [`crates/muvrix-player`](crates/muvrix-player/): Safe, RAII-managed libmpv client, safe `MpvRenderContext`, `PlaybackController` state machine, track/subtitle controls, audio DSP/equalizer, GLSL shader manager, and exclusive owner of `libmpv-sys`.
- [`crates/muvrix-ui`](crates/muvrix-ui/): Presentation shell, native Linux winit runner with direct EGL 1.5 presentation, OpenGL FBO render engine, branding, MPRIS/sleep OS integrations, and `egui` views. (Depends on `muvrix-player` and `muvrix-core`, **not** `libmpv-sys`).

---

## Building and Testing

### Linux (Fedora, Ubuntu, Debian, Arch)
```bash
# 1. Install system prerequisites (e.g. Fedora)
sudo dnf install mpv-libs-devel mesa-libGL-devel libX11-devel wayland-devel

# 2. Run linters and full test suite (389+ automated tests)
cargo fmt --check
LD_LIBRARY_PATH="$HOME/.local/lib64" cargo clippy --workspace --all-targets -- -D warnings
LD_LIBRARY_PATH="$HOME/.local/lib64" cargo test --workspace

# 3. Launch desktop player
LD_LIBRARY_PATH="$HOME/.local/lib64" cargo run -p muvrix-ui -- tests/fixtures/test_video.mp4
```

### macOS (Apple Silicon & Intel)
```bash
# 1. Install prerequisites via Homebrew
brew install mpv pkg-config

# 2. Compile and test
export PKG_CONFIG_PATH="/opt/homebrew/lib/pkgconfig:$PKG_CONFIG_PATH" # Apple Silicon
cargo check --workspace --all-targets
cargo test --workspace --all-targets
```

### Windows 10/11 x86-64
```powershell
# 1. Extract prebuilt mpv-dev SDK (zhongfly/mpv-winbuild) to C:\mpv-dev
# 2. Set environment variables
$env:MPV_DIR = "C:\mpv-dev"
$env:PATH = "C:\mpv-dev;" + $env:PATH
$env:LIB = "C:\mpv-dev;C:\mpv-dev\lib;" + $env:LIB

# 3. Compile and check
cargo check --workspace --all-targets
cargo test --workspace --all-targets
```
