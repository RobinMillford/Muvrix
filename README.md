# Muvrix Desktop

Muvrix is a modern, high-performance cross-platform desktop media player built with Rust, egui, and upstream libmpv.

---

## Documentation Index

### Phase 0.5 — Cross-Platform Build & Integration Feasibility
- [**Phase 0.5 Cross-Platform Portability & Architecture Audit**](docs/PHASE-0.5-CROSS-PLATFORM-PORTABILITY-AUDIT.md)
- [**Phase 0.5 libmpv Cross-Platform Dependency & Linkage Strategy**](docs/PHASE-0.5-LIBMPV-DEPENDENCY-STRATEGY.md)
- [**Phase 0.5 Cross-Platform Compatibility Matrix (JSON)**](docs/PHASE-0.5-PLATFORM-COMPATIBILITY-MATRIX.json)
- [**Continuous Integration Pipeline (`.github/workflows/ci.yml`)**](.github/workflows/ci.yml)

### Phase 0.4 — Playback Engine, Hardware Decoding & Verification
- [**Phase 0.4 Final Verification Sign-Off**](docs/FINAL-VERIFICATION-SIGN-OFF.md) *(Authoritative Record)*
- [**Phase 0.4 Post Playback Failure Investigation**](docs/POST-PHASE-0.4-PLAYBACK-FAILURE-INVESTIGATION.md) *(Historical Incident Record)*
- [**Phase 0.4 Hardware Decoding & Cross-Platform Validation**](docs/PHASE-0.4-HARDWARE-DECODING-AND-PLATFORM-VALIDATION.md)
- [**Phase 0.4 Hardware Capability & Telemetry Architecture**](docs/PHASE-0.4-HARDWARE-CAPABILITY-ARCHITECTURE.md)
- [**Phase 0.4 Media Test Collection Inventory**](docs/PHASE-0.4-MEDIA-INVENTORY.md)
- [**Phase 0.4 Media Compatibility Matrix (JSON)**](docs/PHASE-0.4-MEDIA-COMPATIBILITY-MATRIX.json)

### Earlier Phases & Architectural Audits
- [**Phase 0.3 Playback Subsystem Hardening & Validation**](docs/PHASE-0.3-PLAYBACK-VALIDATION.md)
- [**Phase 0.2 Rendering Prototype Review**](docs/RENDERING-PROTOTYPE.md)
- [**Final Architecture Cross-Audit Review**](docs/FINAL-ARCHITECTURE-REVIEW.md)
- [**Phase 0.1 Developer Guide**](docs/DEVELOPER-GUIDE-PHASE-0.1.md)
- [**Android Reference Architecture Audit**](docs/architecture/README.md)
- [**Upstream mpv Core Engine Audit**](docs/mpv-engine/README.md)

---

## Engineering Roadmap

| Phase | Milestone | Scope | Status |
| :--- | :--- | :--- | :--- |
| **Phase 0.1** | Rust Workspace & libmpv FFI | Workspace layout, C FFI bindings, handle lifecycle, Linux linking | **COMPLETE** |
| **Phase 0.2** | Rendering Prototype | eframe + egui_glow, off-screen FBO compositor, OpenGL 3.3+ render | **COMPLETE** |
| **Phase 0.3** | Playback Hardening | Controller state model, async dispatch, property observers, error safety | **COMPLETE** |
| **Phase 0.4** | Engine & HW Acceleration | Codec validation, VA-API copy-back, 4K60/HFR, telemetry HUD, 10 GUI flows | **COMPLETE** |
| **Phase 0.5** | Cross-Platform Feasibility | Multi-target compilation (Linux/Win/Mac), libmpv dependency strategy, CI | **COMPLETE** |
| **Phase 1.0** | Full Desktop UI & Media Shell | Advanced controls, playlist management, full OSD, theme system, audio sinks | *Planned* |
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

# 2. Run linters and full test suite (71 automated tests)
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
