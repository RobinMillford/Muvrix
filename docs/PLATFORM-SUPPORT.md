# Platform Support and Hardware Acceleration

This document outlines the current operating system compatibility, hardware acceleration status, and continuous integration validation for Muvrix.

---

## 1. Operating System Compatibility Matrix

| Platform | Target Architecture | Display Backend | Audio Driver | Validation Level |
| :--- | :--- | :--- | :--- | :--- |
| **Fedora Linux (45 / 41+)** | `x86_64` | Wayland native runner (verified); X11 compile/link only | PipeWire / PulseAudio | **Wayland + direct VAAPI physically verified** |
| **Ubuntu Linux (24.04 LTS)** | `x86_64` | Wayland / X11 | PipeWire / PulseAudio | **Compile/test CI only; no runtime qualification** |
| **macOS 15+ (Apple Silicon)** | `aarch64-apple-darwin` | Cocoa / OpenGL | CoreAudio | **Compile/link CI only; no runtime qualification** |
| **Windows 10/11 (MSVC)** | `x86_64-pc-windows-msvc` | Win32 / WGL (OpenGL presentation; libmpv may use D3D11/SW decode) | WASAPI | **Compile/link CI only; no runtime qualification** |
| **macOS (Intel x86_64)** | `x86_64-apple-darwin` | Cocoa / OpenGL | CoreAudio | Architecturally supported; no Metal backend claim; no runtime qualification |

---

## 2. Hardware Decoding (`hwdec`) Capabilities

Muvrix configures `libmpv` with `hwdec=auto-safe`, defaulting to copy-back hardware decoding to ensure frames can be safely rendered into an OpenGL FBO across arbitrary window contexts:

| Platform | Decoder Backend | Supported Codecs | Verification Status |
| :--- | :--- | :--- | :--- |
| **Linux (Intel / AMD)** | `vaapi-copy` | H.264, HEVC 8/10-bit, VP9, AV1 | **Physically Verified** (Intel Iris Xe ADL GT2) |
| **Linux (NVIDIA)** | `nvdec-copy` | H.264, HEVC, VP9, AV1 | Driver dependent |
| **macOS** | `videotoolbox-copy` | H.264, HEVC, ProRes | Supported via libmpv |
| **Windows** | `d3d11va-copy` | H.264, HEVC, VP9, AV1 | Supported via libmpv |
| **All Platforms** | `software (no)` | All FFmpeg formats | Verified as automatic fallback |

---

## 3. Continuous Integration Pipeline

Cross-platform compilation, linkage, and unit tests are enforced on every push and pull request via [`.github/workflows/ci.yml`](../.github/workflows/ci.yml):

1. **Fedora 45 (`fedora:45` container)**:
   - Validates Linux compilation and automated tests against modern distribution packages (`mpv-libs-devel`).
2. **Ubuntu 24.04 (`ubuntu-24.04` runner)**:
   - Validates Debian-family packaging, linker discovery via `pkg-config`, and smoke tests.
3. **macOS 15 (`macos-15` Apple Silicon runner)**:
   - Validates Homebrew `libmpv` dynamic linking, `aarch64-apple-darwin` compilation, and unit test suites.
4. **Windows MSVC (`windows-latest` runner)**:
   - Validates Windows MSVC toolchain, verifies pinned development archive checksum, generates native MSVC import library (`mpv.lib`) using `lib.exe /def:libmpv-2.def`, and runs full workspace compilation.

> [!NOTE]
> All Phase 0.5 portability infrastructure and test suites pass in hosted CI. The latest Phase 0.6 desktop UI refinements have been locally validated on Fedora 45 and will be tested on hosted runners upon remote branch push.


---

## 4. Current Verification Truth (2026-11-15)

- **Linux Wayland native runner:** physically verified with the winit-owned window/EGL surface and direct VAAPI zero-copy path.
- **Linux X11:** builds and links in CI; native X11 presentation is not runtime-qualified here.
- **Windows and macOS:** compile/link in CI; neither platform has runtime playback qualification in this repository.
- **Render backends:** Muvrix's presentation path is OpenGL (FBO + quad). libmpv may use D3D11 or software decode on Windows. macOS uses Cocoa/OpenGL; there is **no Metal presentation backend**.
- **MPRIS and screen-saver inhibit:** Linux session-D-Bus only; both are optional and no-op without a session bus.
