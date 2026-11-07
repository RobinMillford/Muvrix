# Platform Support and Hardware Acceleration

This document outlines the current operating system compatibility, hardware acceleration status, and continuous integration validation for Muvrix.

---

## 1. Operating System Compatibility Matrix

| Platform | Target Architecture | Display Backend | Audio Driver | Validation Level |
| :--- | :--- | :--- | :--- | :--- |
| **Fedora Linux (45 / 41+)** | `x86_64` | Wayland / X11 | PipeWire / PulseAudio | **Physically Verified on Hardware & Container CI** |
| **Ubuntu Linux (24.04 LTS)** | `x86_64` | Wayland / X11 | PipeWire / PulseAudio | **Verified in Hosted CI** |
| **macOS 15+ (Apple Silicon)** | `aarch64-apple-darwin` | Cocoa / Metal / GL | CoreAudio | **Verified in Hosted CI (`macos-15`)** |
| **Windows 10/11 (MSVC)** | `x86_64-pc-windows-msvc` | Win32 / WGL | WASAPI | **Verified in Hosted CI (`windows-latest`)** |
| **macOS (Intel x86_64)** | `x86_64-apple-darwin` | Cocoa / GL | CoreAudio | Architecturally supported; pending dedicated Intel CI runner |

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
