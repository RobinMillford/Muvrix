# Building and Running Muvrix

This document details the build prerequisites and compilation procedures for Muvrix on supported desktop platforms.

---

## 1. System Prerequisites

### Linux (Fedora 45 / 41+)
```bash
sudo dnf install -y \
    mpv-libs-devel \
    mesa-libGL-devel \
    libX11-devel \
    wayland-devel \
    gcc \
    pkgconf-pkg-config
```

### Linux (Ubuntu 24.04 LTS / Debian 12+)
```bash
sudo apt-get update && sudo apt-get install -y \
    libmpv-dev \
    libgl1-mesa-dev \
    libx11-dev \
    libwayland-dev \
    libxkbcommon-dev \
    pkg-config \
    build-essential
```

### macOS 15+ (Apple Silicon & Intel)
Install dependencies via Homebrew:
```bash
brew install mpv pkg-config
```
Configure `PKG_CONFIG_PATH` according to your architecture:
- **Apple Silicon (arm64)**:
  ```bash
  export PKG_CONFIG_PATH="/opt/homebrew/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
  ```
- **Intel (x86_64)**:
  ```bash
  export PKG_CONFIG_PATH="/usr/local/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
  ```

### Windows 10/11 (x86_64 MSVC)
1. Install **Visual Studio Build Tools 2022** with the C++ Desktop workload (provides MSVC `cl.exe`, `lib.exe`, Windows SDK).
2. Download a pinned 64-bit `libmpv` development package (e.g. from upstream mpv Windows builds).
3. Generate the MSVC-compatible import library (`mpv.lib`) using the bundled module definition file:
   ```cmd
   lib.exe /def:crates\libmpv-sys\windows\libmpv-2.def /out:mpv.lib /machine:x64
   ```
4. Set environment variables to locate the headers, import library, and dynamic library:
   ```powershell
   $env:MPV_DIR = "C:\mpv-dev"
   $env:LIB = "$env:MPV_DIR;$env:LIB"
   $env:PATH = "$env:MPV_DIR;$env:PATH"
   ```

---

## 2. Compilation and Quality Gates

### Quick Check
```bash
cargo check --workspace --all-targets
```

### Strict Linting
```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

### Running Test Suite
```bash
# Run deterministic test suite (59 passed, 0 failed, 22 ignored)
cargo test --workspace --all-targets -- --test-threads=1
```

---

## 3. Running the Player

Launch with a test fixture or local media file:
```bash
# Launch with the tracked synthetic test video
cargo run -p muvrix-ui -- tests/fixtures/test_video.mp4

# Launch into idle state (welcome screen)
cargo run -p muvrix-ui
```

### Environment Overrides for Testing
- `MUVRIX_WINDOW_WIDTH` / `MUVRIX_WINDOW_HEIGHT`: Override initial window dimensions (e.g. `MUVRIX_WINDOW_WIDTH=1024 MUVRIX_WINDOW_HEIGHT=768 cargo run -p muvrix-ui`).
- `MUVRIX_CAPTURE_WINDOW_PNG`: Automatically save a window screenshot after rendering begins and exit cleanly (used in automated UI verification).
