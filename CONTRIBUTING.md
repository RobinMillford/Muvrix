# Contributing to Muvrix

Thanks for your interest in Muvrix. This guide describes how to set up a development environment, submit contributions, and work with the project.

## Development setup

### Prerequisites

- Rust stable (>= 1.75) — use `rustup` or your distribution's package
- system deps for Linux:
  - Fedora: `mpv-libs-devel mesa-libGL-devel libX11-devel wayland-devel libxkbcommon-devel alsa-lib-devel dbus-devel`
  - Debian/Ubuntu: `libmpv-dev libgl1-mesa-dev libx11-dev libwayland-dev libxkbcommon-dev libasound2-dev libdbus-1-dev`
- `git`, `curl`, `flac`, `mpv` (for testing)

### Clone and build

```sh
git clone https://github.com/RobinMillford/Muvrix.git
cd Muvrix
cargo build --release -p muvrix-ui
```

The release binary is at `target/release/muvrix-ui`.

## Running tests

```sh
cargo test --workspace --all-targets
```

Tests cover unit, integration, and UI tests. Some tests require hardware (GPU, VA-API) and will skip when unavailable.

## Code style

- Run `cargo fmt` before committing
- Run `cargo clippy --workspace --all-targets -- -D warnings` and fix all warnings
- Follow the style guide: snake_case for functions, PascalCase for types
- Document public API in crate-level and module-level docs

## Submitting changes

1. Open an issue or reply to an existing one — align on scope before coding
2. Fork the repository and create a topic branch
3. Work in small, focused commits
4. Run tests and clippy before pushing
5. Open a pull request against `main` with a clear description
6. Wait for CI to pass — merges require green CI

## Reporting bugs

Open an issue at <https://github.com/RobinMillford/Muvrix/issues>. Include:

- OS and compositor (e.g. Fedora 45 / GNOME Wayland)
- Hardware (GPU, VA-API driver version)
- Logs with `RUST_LOG=music_player=debug` if possible
- Steps to reproduce

For security issues, see `SECURITY.md`.

## License

By contributing you agree that your contributions are licensed under LGPL-2.1-or-later, matching the project.
