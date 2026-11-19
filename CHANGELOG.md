# Changelog

All notable changes to Muvrix are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## 0.1.0 — 2026-11-16

First public release.

### Added
- Native media playback via libmpv 0.42.0
- Hardware-accelerated rendering with VA-API (Intel Iris Xe) and OpenGL
- Wayland-native presentation path with EGL surface
- X11 support as fallback
- HDR10 (PQ) presentation with HDR detection
- Audio DSP: 10-band parametric equalizer, Bauer crossfeed, dynamics compressor
- 32-band real-time spectrum visualizer
- SQLite-backed media catalog with FTS5 search
- Anime4K v4.0.1 video shader pipeline
- MPRIS2 integration for desktop media controls
- Network streaming: HTTP, HTTPS, HLS
- Watch history and resume playback
- Gapless playback with cross-format queue
- Linux desktop integration: XDG dirs, MIME system, thumbnails

### Performance
- Zero blocking round trips into libmpv on the render path (~5 FPS → ~28 FPS on 4K HDR with VA-API)
- Presentation pacing tightly aligned to compositor frame cadence

### Packaging
- Flatpak (primary), DEB, RPM, AppImage
- Reproducible builds via pinned runtime bundle and containerized toolchains
- SBOM generation via CycloneDX
