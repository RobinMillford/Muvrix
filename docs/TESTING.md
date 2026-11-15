# Testing and Quality Gates

This document explains the testing architecture, quality gates, and automated test execution for Muvrix.

---

## 1. Quality Gates

Muvrix enforces four mandatory quality gates before any code is committed:

| Gate | Command | Requirement |
| :--- | :--- | :--- |
| **Code Formatting** | `cargo fmt --check` | 0 formatting violations |
| **Workspace Compilation** | `cargo check --workspace --all-targets` | 0 compilation errors |
| **Clippy Linting** | `cargo clippy --workspace --all-targets --all-features -- -D warnings` | 0 warnings |
| **Automated Test Suite** | `cargo test --workspace` | 389+ passed, 0 failed |

---

## 2. Test Suite Architecture

The workspace contains 400+ tests organized into deterministic CI tests and optional extended tests. The current verified baseline is **389+ passed, 0 failed, 24 ignored**:

```text
Suite Summary: 389+ passed, 0 failed, 24 ignored
├── libmpv-sys (3 tests)
│   ├── client_api_version (Unit)
│   ├── create_destroy_handle (Unit)
│   └── link_smoke_test (Integration)
├── muvrix-core (7 tests)
│   ├── path_resolution (Spaces, relative, fallback, errors)
│   ├── tilde_expansion
│   └── streaming_urls
├── muvrix-player (50 tests: 33 passed, 17 ignored)
│   ├── capability_test (HWDEC copyback, software fallback, distro parsing)
│   ├── format_validation_test (H.264/AAC baseline; 7 extended formats ignored)
│   ├── performance_validation_test (Playback baseline; 7 extended perf ignored)
│   ├── playback_controller_test (Lifecycle, seek, tracks, volume, chapters)
│   ├── playback_test (Properties, events, pause)
│   ├── render_test (Context safety, null loader fallback)
│   └── smoke_test (Handle initialization, release, version)
└── muvrix-ui (21 tests: 16 passed, 5 ignored)
    ├── gui_flows_test (File loading, path handling, recovery, presets)
    ├── gui_path_test (Sequential replacement, event ordering)
    └── ui_test (Progress calculation, volume clamping, speed formatting)
```

---

## 3. Running the Test Suite

### Standard Test Execution
Run all deterministic tests across the workspace:
```bash
cargo test --workspace --all-targets -- --test-threads=1
```

> [!NOTE]
> `--test-threads=1` is recommended when running player integration tests locally to avoid libmpv OpenGL context conflicts across concurrent threads.

### Running Specific Crates
```bash
# Core path and platform tests
cargo test -p muvrix-core

# Player engine and controller tests
cargo test -p muvrix-player

# UI logic and flow tests
cargo test -p muvrix-ui
```

---

## 4. Test Fixtures and Extended Media

### Tracked CI Fixtures
Muvrix includes small synthetic test fixtures committed in `tests/fixtures/` to enable fully deterministic, offline testing without external dependencies:
- `tests/fixtures/test_video.mp4`: 52 KB synthetic 3-second H.264/AAC SMPTE test pattern.
- `tests/fixtures/corrupted.mp4`: Invalid header file for verifying graceful error recovery.
- `tests/fixtures/unsupported.txt`: Plain text file for verifying format rejection.
- `tests/fixtures/path-resolution/`: Fixtures for verifying path handling (spaces, tilde expansion).

### Running Ignored Extended Tests
Tests marked `#[ignore]` validate capabilities requiring physical GPU hardware or an extended media corpus:
```bash
# Run a specific ignored test with output
cargo test -p muvrix-player --test format_validation_test -- test_format_eac3_7_1_sample --ignored --nocapture
```
Configuration variables for extended tests:
- `MUVRIX_TEST_MEDIA_DIR`: Directory containing extended media corpus.
- `MUVRIX_SAMPLE_7_1_MKV`: Explicit path to multi-channel audio sample.


---

## 5. Ignored Tests and Current Truth (2026-11-15)

The 24 ignored tests are deliberate hardware/CI gates, not failures:

- Graphical Wayland native-runner and embedded-render tests require a real compositor, EGL, and GPU.
- Direct VAAPI, HDR fixture, and sequential SDR/HDR tests require Intel VAAPI plus the local media corpus.
- Extended codec/performance/GUI preset tests require `tests/MuvrixTestMedia/` or the named external media variables.
- Live SMB/SFTP/WebDAV real-server tests require external services.
- Long-duration C1/C3 qualification remains a manual hardware run.

The automated suite is now **389+ passed, 0 failed, 24 ignored** on the verified Linux development host. Tests must not be reported as runtime platform qualification merely because they compile or pass headlessly.
