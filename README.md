# Muvrix Desktop

Muvrix is a modern, high-performance cross-platform desktop media player built with Rust, egui, and upstream libmpv.

---

## Documentation Index

- [**Building Guide**](docs/BUILDING.md) — Prerequisites, build commands, and environment setup for Linux, macOS, and Windows.
- [**Architecture Overview**](docs/ARCHITECTURE.md) — Three-tier architecture, rendering pipeline, threading model, and design principles.
- [**Features & Controls**](docs/FEATURES.md) — Playback controls, timeline scrubbing, track selection, keyboard shortcuts, and diagnostic HUD.
- [**Platform Support**](docs/PLATFORM-SUPPORT.md) — Operating system support, hardware decoding backends, and CI verification status.
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
| **Phase 0.6** | Core Player Experience | Complete playback controls, timeline/scrubbing, volume/mute, audio/subtitle selection, track info OSD, debug HUD, vector icons | **COMPLETE** *(Local)* |
| **Phase 1.0** | Advanced Media Shell | Playlist management, full OSD, theme system, audio sinks | *Planned* |
| **Phase 2.0** | Distribution & Packaging | Flatpak, AppImage, Windows Inno Setup/MSIX, macOS `.app` bundle | *Planned* |

---

## Workspace Architecture

- [`crates/libmpv-sys`](crates/libmpv-sys/): Raw C FFI bindings to libmpv (`client.h`, `render.h`, `render_gl.h`, `render_d3d11.h`), cross-platform dynamic linker probe, and multi-platform `build.rs`.
- [`crates/muvrix-core`](crates/muvrix-core/): Platform detection (`PlatformInfo`, `SessionType`, `DistroInfo`), cross-platform path resolver (`resolve_media_path`, tilde expansion), error primitives, and core traits.
- [`crates/muvrix-player`](crates/muvrix-player/): Safe, RAII-managed libmpv client, `mpv_render_context`, `PlaybackController` state machine, track/subtitle controls, `PlaybackDiagnostics`, and native smoke tests.
- [`crates/muvrix-ui`](crates/muvrix-ui/): Desktop UI presentation shell, OpenGL 3.3+ FBO compositor, multi-platform GL loader, and `eframe` / `egui_glow` desktop application.

---

## Building and Testing

### Linux (Fedora, Ubuntu, Debian, Arch)
```bash
# 1. Install system prerequisites (e.g. Fedora)
sudo dnf install mpv-libs-devel mesa-libGL-devel libX11-devel wayland-devel

# 2. Run linters and full test suite (59 automated tests)
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets

# 3. Launch desktop player
cargo run -p muvrix-ui -- tests/fixtures/test_video.mp4
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
