<div align="center">
  <img src="assets/branding/muvrix-logo.svg" width="120" height="120" alt="Muvrix logo" />

  <h1>Muvrix</h1>

  <p><strong>A beautiful, powerful desktop media player for Linux</strong></p>

  <p>
    <a href="https://github.com/RobinMillford/Muvrix/releases"><img src="https://img.shields.io/github/v/release/RobinMillford/Muvrix?display_name=tag&style=flat-square&color=C8EE75" alt="Latest release" /></a>
    <a href="./LICENSE"><img src="https://img.shields.io/badge/License-LGPL--2.1-purple?style=flat-square" alt="License" /></a>
    <a href="https://github.com/RobinMillford/Muvrix/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/RobinMillford/Muvrix/ci.yml?branch=main&style=flat-square" alt="CI status" /></a>
    <a href="https://github.com/RobinMillford/Muvrix/actions/workflows/packaging.yml"><img src="https://img.shields.io/github/actions/workflow/status/RobinMillford/Muvrix/packaging.yml?branch=main&style=flat-square" alt="Packaging CI" /></a>
    <a href="docs/BUILDING.md"><img src="https://img.shields.io/badge/Built%20with%20Rust%20%E2%80%94%20stable-red?style=flat-square" alt="Rust" /></a>
    <a href="https://github.com/RobinMillford/Muvrix"><img src="https://img.shields.io/badge/Platform-Linux%20(Wayland)-blue?style=flat-square" alt="Platform" /></a>
  </p>
</div>

<div align="center">

  <p><strong>4K HDR. Hardware-accelerated. Pixel-perfect.</strong></p>

</div>

---

Muvrix is a polished native desktop media player built in stable Rust on top of [libmpv](https://mpv.io/). It delivers stutter-free playback of ultra-high-definition video, true wide-gamut HDR presentation with BT.2020/PQ on supported compositors, and a player surface designed to stay out of the way of the movie.

Muvrix's architecture separates concerns into four audited crates — a hand-written FFI layer for a pinned upstream engine (libmpv 0.42.0), a pure-Rust core with an embedded SQLite episode catalog, a safe playback controller driving the engine's render API, and an immediate-mode UI rendered through egui with a custom OpenGL ES presentation path.

<div align="center">

|  |  |  |
|---|---|---|
| **4K / HDR**<br />HEVC Main10, HDR10, PQ transfer, BT.2020 primaries, 10-bit presentation<br />Verified on Intel VA-API | **Fast by default**<br />Frames presented by the compositor's own cadence — no vsync fighting, no dropped frames<br />`report_swap` acknowledges frames only after the compositor shows them | **Lean**<br />Zero blocking round trips into the engine on the render path<br />Audio DSP tuned for hardware decode headroom |
| **Cross-format**<br />HEVC, VP9, AV1, H.264, MP4, MKV, MOV, WebM | **Rich UI**<br />Queue, watch history, SQLite FTS5 search, subtitle offset, chapters<br />MPRIS2 integration, sleep inhibition | **Open**<br />LGPL-2.1 — fork it, audit it, ship it<br />No telemetry |

</div>

<div align="center">
<sub>A live screenshot will replace this placeholder. Desktop captures of the current build have not been produced yet — see <a href="#screenshots">Screenshots</a>.</sub>
</div>

---

## Table of contents

- [Why Muvrix](#why-muvrix)
- [Features](#features)
- [Screenshots](#screenshots)
- [Supported platforms](#supported-platforms)
- [System requirements](#system-requirements)
- [Installation](#installation)
  - [One-command install](#one-command-install)
  - [Flatpak (recommended)](#flatpak-recommended)
  - [DEB / RPM / AppImage](#deb--rpm--appimage)
  - [From source](#from-source)
- [Hardware acceleration](#hardware-acceleration)
- [HDR playback](#hdr-playback)
- [Supported media formats](#supported-media-formats)
- [Keyboard shortcuts](#keyboard-shortcuts)
- [Configuration](#configuration)
- [Troubleshooting](#troubleshooting)
- [Architecture](#architecture)
- [Development](#development)
- [Testing](#testing)
- [Packaging and releases](#packaging-and-releases)
- [Contributing](#contributing)
- [Security](#security)
- [License](#license)
- [Acknowledgements](#acknowledgements)

---

## Why Muvrix

Video playback on the desktop is usually a trade-off between correctness and latency. Most players either render frames into the compositor as fast as possible (tearing, waste) or play video back at a fraction of the source rate when the pipeline stalls. Muvrix targets *correct* presentation on real hardware:

- **Hardware-first decoding.** HEVC Main10 decodes through Intel VA-API on compatible hardware, with an intelligent fallback ladder to software decoding if a driver can't initialise.
- **Compositor-paced frames.** Muvrix does not run its own timer fight against the compositor. It presents each frame exactly when the compositor is ready to show it, acknowledges frames only once the compositor has actually presented them, and never drops frames to "catch up".
- **Genuine HDR.** The HDR path is opt-in at the OS level and end-to-end where supported (10-bit surface, PQ transfer function, BT.2020 primaries). Not an approximation, not a tone-map guess — a verified output path.
- **Deterministic builds.** Every release artifact is built from pinned sources against a known toolchain, inside throwaway containers, with checksums recorded and verified.

Muvrix is a fresh start designed for modern Linux desktops. It is not a fork of anything; the engine is a pinned upstream libmpv build used as a library, not a runtime download.

---

## Features

**Playback**
- Native transport controls with full-screen, click-to-toggle, and double-click fullscreen
- Interactive timeline with hover tooltips, chapter markers, and drag-scrubbing without mid-drag seeking
- A/B loop points, playback-speed presets (0.25×–2×), instant 1.0× reset
- Per-track audio and subtitle switching, external subtitle loading, subtitle delay tuning
- Gapless playlist queues with shuffle and smart weighted ordering
- Watch history and resume-position persistence
- Chapter navigation and chapter bookmarks

**Audio**
- 10-band parametric equalizer (peaking/low-shelf/high-shelf) plus AutoEq profile import
- 32-band real-time spectrum visualizer
- Bauer crossfeed, dynamics compressor
- EBU R128 loudness normalisation (`loudnorm`) with a pre-flight headroom check

**Video**
- Anime4K v4.0.1 GLSL shader pipeline (upscaling, restoration, denoising presets)
- Hardware tone mapping where the driver supports it
- Hardware and software decode paths with an automatic fallback ladder
- Screenshot capture and clip snapshot

**Organisation**
- SQLite FTS5 search over the embedded media catalog
- Background directory scanner and metadata enrichment
- Smart playlists, queue management, playlist import/export (M3U, M3U8, etc.)
- Gapless playback and cross-format queues

**Desktop integration**
- MPRIS2 (media controls from GNOME/KDE, lock screen, headphones)
- Screen-saver and sleep inhibition during playback
- Native file dialogs
- Window icon and taskbar integration via Wayland app-id
- Letterbox-preserving viewport scaling

**Engineering**
- Deterministic, pinned builds with reproducible checksums
- No telemetry; configuration and data stay on your machine
- CI: fmt / clippy / full test matrix / packaging verification / reproducibility check
- Security audit-ready: no network access by default beyond what you open

---

## Screenshots

> **Note:** No desktop screenshots of the current build have been produced yet. The screenshots in the historical `docs/screenshots/` directory show a different (Android) application and are not used here. When you capture a desktop screenshot, drop it in `docs/screenshots/` and add it to this section.

Suggested placeholder (replace with real captures):

```markdown
![Muvrix playing a 4K HDR movie](docs/screenshots/player.png)
*Muvrix playing an HDR10 movie — 10-bit presentation with BT.2020 primaries.*
```

---

## Supported platforms

| Platform | Status | Notes |
|---|---|---|
| Linux — Wayland (GNOME) | **Primary, verified** | End-to-end HDR presentation, VA-API hardware decode, full-screen UX |
| Linux — X11 | **Supported, not runtime-qualified** | Same code path; not runtime-qualified on this machine |
| macOS (Apple Silicon / Intel) | **Build-verified** | Engine builds; presentation path not yet qualified |
| Windows 10/11 (x86-64) | **Build-verified** | Engine builds; presentation path not yet qualified |

Hardware acceleration is **only** qualified on Intel VA-API on Linux. Other drivers may work; treat them as experimental until qualified.

---

## System requirements

- **OS:** Linux (Wayland recommended). Kernel with Wayland ≥ 1.23 and a compositor that supports `wp_image_description_v1`/`wp_color_manager_v1` for HDR output, or any Mesa/Mutter-based display stack.
- **CPU:** Any x86-64 with a working OpenGL ES 3 driver. Hardware decode additionally needs a working VA-API driver for your GPU (Intel are the only ones runtime-verified).
- **Memory:** 2 GB RAM minimum; 4 GB+ for 4K HDR workflows.
- **Disk:** ~300 MB installed; more for SD/HEVC content caches.
- **HDR:** A compositor that advertises `wp_color_manager_v1` with PQ/BT.2020 features (Mutter ≥ 47 / KWin ≥ 6 on capable hardware) for true HDR output. Without it, Muvrix falls back to SDR tone-mapping.

---

## Installation

### One-command install

```sh
curl -fsSL https://raw.githubusercontent.com/RobinMillford/Muvrix/main/install.sh | sh
```

This detects your distribution, package manager, and whether Flatpak is present, then installs the Flatpak build of Muvrix — or offers a native package where available. See [`install.sh`](install.sh) for the full, readable source, and [`docs/INSTALLATION.md`](docs/INSTALLATION.md) for the detailed guide.

Muvrix data (`~/.var/app/org.muvrix.Muvrix/`) is never touched by the installer.

### Flatpak (recommended)

```sh
flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
flatpak install flathub org.muvrix.Muvrix
flatpak run org.muvrix.Muvrix
```

The Flatpak build is the reference distribution. It includes a private media runtime (libmpv 0.42.0 + FFmpeg + libplacebo), uses your Wayland graphics stack through `org.freedesktop.Platform.GL.default`, and uses your VA-API drivers through `org.freedesktop.Platform.VAAPI.Intel` or `org.freedesktop.Platform.GL.default`.

### DEB / RPM / AppImage

Download the latest release artifacts from [the releases page](https://github.com/RobinMillford/Muvrix/releases). Every artifact is verified against `SHA256SUMS` before installation. If you prefer to inspect the installer first:

```sh
curl -fsSL https://raw.githubusercontent.com/RobinMillford/Muvrix/main/install.sh -o install.sh
less install.sh
sh install.sh
```

### From source

```sh
git clone https://github.com/RobinMillford/Muvrix.git
cd Muvrix
./packaging/build_runtime_bundle.sh   # builds the private media runtime (pinned)
cargo build --release -p muvrix-ui
./target/release/muvrix-ui "/path/to/movie.mkv"
```

See [docs/BUILDING.md](docs/BUILDING.md) for the full build matrix.

---

## Hardware acceleration

Muvrix uses Intel VA-API for hardware HEVC Main10 decode, including P010 10-bit surfaces.

- **Driver:** Intel iHD for Intel Iris Xe and later; i965 for older Intel GPUs.
- **Decode:** HEVC (Main10), VP9, AV1 (when the driver exposes it).
- **Fallback ladder:** if VA-API cannot initialise, Muvrix falls back to `vaapi-copy` (decoded frames copied to CPU-accessible memory), then to software decoding. You can force a mode with `MUVRIX_TEST_HWDEC` for diagnostics.
- **Verification:** set `MUVRIX_MPV_LOG=warn` and look for `Decoder observed: ... hwdec='vaapi' ... path=HwDirect` (or `HwCopyBack` / `HwSoftwareFallback`). `Decoder state unknown` means neither was observed.
- **Flatpak:** VA-API works through the freedesktop GL/VAAPI extensions — the Intel iHD driver (`/usr/lib/x86_64-linux-gnu/dri/intel-vaapi-driver/iHD_drv_video.so`) is provided by `org.freedesktop.Platform.VAAPI.Intel`, which must be installed (usually pulled in automatically).

Hardware decode is the only qualified path today: NVIDIA or AMD drivers will work with Mesa's `vaapi` drivers, but have not been runtime-verified.

---

## HDR playback

Muvrix's HDR pipeline is transport-verbatim: source HDR10 (PQ) video is decoded with its original transfer function intact, presented to a 10-bit surface, and handed to the compositor under a PQ/BT.2020 image description — no tone-mapping, no clamping.

Requirements on the display stack:
- a 10-bit capable framebuffer (Muvrix probes for one and refuses 8-bit PQ output)
- a compositor advertising `wp_color_manager_v1` with PQ and BT.2020 features (Mutter ≥ 47/49 / KWin ≥ 6)
- `wp_presentation` feedback so frame acknowledgement stays in order

If the compositor refuses or doesn't advertise a PQ/BT.2020 description, Muvrix falls back to SDR with a clearly logged reason (`HDR refused: ...`). HDR and SDR paths are explicit; you never get silently degraded color.

> **Note:** HDR was developed and smoke-tested against Mutter on Iris Xe. Other compositors and GPUs will likely work but are not runtime-qualified; treat them as experimental.

---

## Supported media formats

Container and codec support is inherited from the pinned mpv/FFmpeg engine. In practice, every common open format plays:

- **Containers:** MKV, MP4, WebM, MOV, AVI, TS, OGG, FLAC, M4A
- **Video:** HEVC (Main10 HDR10, SDR), VP9, AV1, H.264/AVC, MPEG-2, Theora
- **Audio:** FLAC, AAC, MP3, Opus, Vorbis, DTS(-HD MA), TrueHD, AC-3, ALAC
- **10-bit content:** HEVC Main10 (P010) decodes and presents as 10-bit where the surface allows

Subtitle tracks (SRT, ASS/SSA, VTT, PGS/SUB) are supported; external `.srt/.ass/.vtt` loading works from the player UI.

---

## Keyboard shortcuts

| Key | Action | Key | Action |
|---|---|---|---|
| `Space` / `K` | Play / Pause | `M` | Toggle mute |
| `←` / `→` | Seek −5 s / +5 s | `[` / `]` | Speed down / up |
| `↑` / `↓` | Volume up / down | `F` | Toggle fullscreen |
| `Esc` | Exit fullscreen | `F1` | Keyboard shortcuts modal |
| `Enter` | Play / Pause | `,` / `.` | Frame step (where supported) |
| `O` | Open file | `L` | Toggle loop A/B |
| `I` | Toggle media info | `B` | Add bookmark |

The authoritative list lives in the in-app shortcuts modal (`F1`).

---

## Configuration

Muvrix stores user configuration under `~/.config/muvrix/` (native) and `~/.var/app/org.muvrix.Muvrix/config/muvrix/` (Flatpak). Notable keys:

| Key | Default | Notes |
|---|---|---|
| `video_adjustments.brightness` / `contrast` / `saturation` / `gamma` | 0 | Per-file image adjustments |
| `equalizer.bands[]` | Flat | 10-band graphic EQ |
| `theme_mode` | `Obsidian` | `Obsidian`, `Dark`, `Amoled`, `Light` |
| `accent_color` | `Lime` | `Lime`, `Blue`, `Purple`, `Teal`, `Green`, `Orange`, `Red` |

Delete `~/.config/muvrix/settings.json` to reset. All data stays on your machine — nothing is uploaded anywhere.

---

## Troubleshooting

**"Hardware decoding unavailable: ..." in the log.** Your VA-API driver could not initialise. Run `vainfo` (host) or check inside the Flatpak (`org.freedesktop.Platform.VAAPI.Intel` must be installed). Muvrix falls back to `vaapi-copy` then to software decoding automatically; the log line tells you exactly which path was selected and why.

**"HDR refused: ..." in the log.** Your compositor did not advertise `wp_color_manager_v1` with PQ/BT.2020 features, or the EGL surface probe found only 8-bit. Check that your Wayland compositor is Mutter ≥ 47 or KWin ≥ 6 and that the display is HDR-capable. Muvrix will fall back to SDR tone-mapping.

**No sound / wrong device.** Check `pavucontrol` or `wpctl status`; the Flatpak uses PulseAudio socket.

**Stuttery playback of very large files.** This was a bug in early builds (per-paint engine round trips stalled the render loop ~5×). Fixed: the render path performs zero synchronous engine round trips. If you still see stutter, enable `MUVRIX_MPV_LOG=v` and attach the log to an issue.

**The window appears but no video renders (black screen).** Check `MUVRIX_MPV_LOG=warn` for the `Decoder observed:` line. If missing, the engine may have failed to hand the render context a GLX/GLES surface — try `MUVRIX_NO_HDR=1` (SDR path) to isolate the HDR description.

Attach the telemetry CSV (`MUVRIX_RUNNER_TELEMETRY=/tmp/run.csv ...`) to any performance-related issue.

---

## Architecture

Muvrix is a four-crate Cargo workspace:

```
crates/
├── libmpv-sys    Raw FFI to a pinned upstream libmpv 0.42.0 (client API 2.6).
├── muvrix-core   Cross-platform paths, SQLite catalog, network streaming, lyrics.
├── muvrix-player Safe playback controller over libmpv-sys. Owns the engine.
└── muvrix-ui     Native presentation: winit runner, EGL surface, egui UI.
```

Engine is a **pinned upstream libmpv 0.42.0**, built from source at package time — never downloaded at runtime. The presentation path is direct: EGL → GL FBO → compositor, no intermediate windowing abstraction, with HDR10/PQ output where the compositor supports it.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the full design.

---

## Development

```sh
git clone https://github.com/RobinMillford/Muvrix.git
cd Muvrix
./packaging/build_runtime_bundle.sh   # pinned libmpv 0.42.0, one-time
cargo build
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

On first build, the runtime bundle script downloads pinned sources (libmpv 0.42.0, libplacebo 7.387.0, FFmpeg 6.1) and verifies their checksums. Subsequent builds reuse `target/runtime/`.

---

## Testing

```sh
cargo test --workspace --all-targets
```

Test coverage spans the workspace (currently ~310 tests), including an embedded Wayland headless smoke test and a genuine 4-format packaging verification suite (`cargo test --test e8_container_packaging_test` / `--test e9_release_readiness_test`). Tests that require a compositor or container engine skip gracefully when unavailable.

---

## Packaging and releases

Every release is built from the exact tag, inside a clean Ubuntu 24.04 container, with the same pinned runtime bundle used in development. The pipeline (`.github/workflows/packaging.yml`) builds the Flatpak (primary), DEB, RPM, and AppImage, verifies each with its native tooling, and generates `SHA256SUMS`, an SBOM (`docs/release/sbom.cdx.json`), and a release manifest. Artifacts are checksummed before publishing.

Release signing is wired in when a GPG key is available (`MUVRIX_SIGNING_KEY_FILE`); otherwise it records `SIGNING_STATUS: PENDING`. See [docs/RELEASING.md](docs/RELEASING.md) for the full release procedure.

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). In short:

- `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` must pass.
- Tests must pass (`cargo test --workspace --all-targets`).
- Keep changes small and focused; prefer one logical change per PR.
- Follow conventional commit messages (`feat:`, `fix:`, `docs:`, `build:`, `test:`).
- Do not merge failing CI.

---

## Security

See [SECURITY.md](SECURITY.md) for supported versions and how to report vulnerabilities privately. Do not file public issues for security-relevant bugs.

---

## License

[LGPL-2.1-or-later](LICENSE), same license as the upstream libmpv engine Muvrix ships with. All third-party dependencies retain their own licenses; see [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

---

## Acknowledgements

Muvrix builds on excellent open-source work:

- [mpv](https://mpv.io/) — the playback engine and libmpv API.
- [libplacebo](https://github.com/haasn/libplacebo) — GPU shader compilation used by mpv.
- [FFmpeg](https://ffmpeg.org/) — demuxing and decoding.
- [egui](https://github.com/emilk/egui) — immediate-mode UI.
- [winit](https://github.com/rust-windowing/winit) — windowing.
- [Mesa](https://mesa3d.org/), Intel iHD — VA-API hardware decode.
- [SQLite](https://sqlite.org/) — the embedded catalog.
