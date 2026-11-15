# Muvrix Engineering Roadmap

This document outlines the development phases, completed milestones, and future releases for the Muvrix media player.

---

## Development Milestones

| Phase | Milestone | Focus Areas | Status |
| :--- | :--- | :--- | :--- |
| **Phase 0.1** | **Rust Workspace & libmpv FFI** | Workspace layout, raw C FFI declarations (`libmpv-sys`), handle lifecycle, dynamic library linking on Linux. | **COMPLETE** |
| **Phase 0.2** | **Rendering Prototype** | `eframe` + `egui_glow` integration, off-screen OpenGL FBO compositor, GLSL quad shader, aspect-ratio letterboxing. | **COMPLETE** |
| **Phase 0.3** | **Playback Subsystem Hardening** | `PlaybackController` state machine, property observer subscriptions, non-blocking event pump, error handling. | **COMPLETE** |
| **Phase 0.4** | **Hardware Acceleration & Decoding** | `auto-safe` copy-back decoding (`vaapi-copy`), format compatibility verification, telemetry HUD, automated GUI flows. | **COMPLETE** |
| **Phase 0.5** | **Cross-Platform Portability & CI** | Multi-target compilation (Linux, macOS 15, Windows MSVC), `libmpv-2.def` import library generation, GitHub Actions CI. | **COMPLETE** |
| **Phase 0.6** | **Core Player Experience** | Native file dialogs (`rfd`), dedicated timeline row, volume/speed/track controls, keyboard shortcuts, procedural vector icons. | **COMPLETE (Local Validation)** |
| **Phase 0.7** | **Advanced Playback & Controls** | Secondary subtitles, Picture-in-Picture (PiP), watch history & resume playback, subtitle style customization, playlist queue. | **COMPLETE (local)** |
| **Phase 0.8** | **Shaders, Audio DSP & Media Library** | Anime4K GLSL upscaling shaders, parametric audio equalizer, local SQLite media library with background file scanner. | **COMPLETE (local)** |
| **Phase 1.0** | **Desktop Distribution & Packaging** | Linux launcher/desktop assets, portable tarball, platform packaging scripts, release sign-off. | **PARTIAL — Linux assets verified; installers pending** |
| **Phase 2.0** | **Ecosystem & Extensibility** | Lua `user-data/mpvex` IPC bridge, SMB/SFTP/WebDAV browser, MPRIS/desktop controls, persistent playlists. | **COMPLETE (local)** |

---

## Detailed Phase Scope

### Phase 0.6 — Core Player Experience (Current)
- Completed desktop player controls: Play/Pause, Stop, Seek (+/-5s), chapter navigation.
- Dedicated timeline with hover seek preview.
- Audio and subtitle track selection with external subtitle file loading.
- Desktop keyboard shortcuts with input-focus isolation.
- Procedural vector iconography (`egui::Painter`) eliminating platform emoji discrepancies.
- Current verified automated suite: **389+ passed, 0 failed, 24 ignored**; ignored tests require GPU, ignored media corpus, live servers, or long-run hardware qualification.

### Phase 0.7 — Advanced Controls & History
- Secondary subtitle rendering for bilingual display.
- Detached Picture-in-Picture window mode.
- Local configuration and playback position resume.
- Subtitle styling panel (font, size, color, margin).
- Sequential playback and drag-and-drop playlist drawer.

### Phase 0.8 — Media Processing & Library
- Custom shader pipeline integrating Anime4K GLSL filters.
- Audio filter graph support (10-band equalizer, compressor).
- Embedded SQLite database for indexing local media directories.
- Network streaming hub with URL bookmarking and cache configuration.


---

## Current Status Snapshot (2026-11-15)

Muvrix is a native Linux development player. The Phase 2/4/5/8 feature set is locally complete (playback controls, subtitles, playlists, themes, config editor, Lua IPC). Phase 9 adds MPRIS media controls, screen-saver inhibition, crash reporting, batch library actions, and packaging/docs truth.

Verified automation: **389+ passed, 0 failed, 24 ignored**, strict Clippy clean, release build and graphical native smoke green. Windows/macOS remain compile/link targets only; C1 (30-minute 1080p60) and C3 (2-hour lifecycle) remain open hardware qualifications. Linux X11 is compile/link only.
