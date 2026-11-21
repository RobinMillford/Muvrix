# Linux libmpv Compatibility and Distribution Strategy

Scope: Linux only. Windows and macOS are explicitly out of scope until the Linux
build is mature, stable and physically verified.

This document separates **what is implemented**, **what is source-audited**,
**what is compile-tested**, and **what has been physically run**. It does not
claim support for anything that has only been compiled.

Verification vocabulary used throughout:

| Term | Meaning |
|---|---|
| Source audited | Code inspected; nothing executed |
| Compile tested | The crate builds |
| Unit tested | Automated unit tests pass |
| Runtime tested | The application or libmpv actually executed |
| Physically tested | Run on real hardware and a real desktop session |

---

## 1. Targeted Linux distributions

| Distribution | Channel | Status |
|---|---|---|
| Fedora | RPM | Runtime-tested (development host) |
| Ubuntu | DEB | Packaging present, not runtime-tested |
| Debian | DEB | Packaging present, not runtime-tested |
| Arch Linux | PKGBUILD | Present; compile-checked only, never installed or played |
| Any glibc Linux | AppImage | Packaging present, not runtime-tested |
| Any Flatpak host | Flatpak | Packaging present, not runtime-tested |

## 2. Current Linux platform limitations

Verified by measurement, not assumption:

1. **4K60 at very high bitrate is not solved, and its cause is not proven.**
   A 133 Mbit/s 3840x2160 60 fps H.264 sample presents ~37 fps with ~23 output
   drops/s in MUVRIX, against ~1.3 drops/s in standalone mpv. A 19 Mbit/s 4K60
   sample and a 4K25 sample both reach source rate with zero drops.
   Decode-throughput starvation is one *candidate* explanation — the mpv update
   callback arrives at only ~38/s and its interval distribution is bimodal
   (53% at one vsync, 42% at two) — but it has not been isolated, and the
   133 Mbit/s difference is a correlation, not a measurement of the cause.
   Treat this as an open defect, not as a diagnosed limitation.
2. **X11 has never been run.** The code path exists and is compile-tested only.
3. **AMD and NVIDIA hardware decoding have never been exercised.** The policy
   is built around capability observation rather than vendor assumptions, but
   only Intel VAAPI has been observed working.
4. **Multi-monitor HDR and refresh-rate changes across monitors are unverified.**
5. Only the GNOME/Mutter compositor has been observed for presentation
   feedback (`wp_presentation`).

## 3. Wayland and X11

Selection is centralised in `choose_backend()` in `crates/muvrix-ui/src/runner.rs`:

```
MUVRIX_WINDOW_BACKEND=wayland|x11|auto   (auto by default)
```

`auto` prefers Wayland when `WAYLAND_DISPLAY` is usable, else falls back to X11
when `DISPLAY` is set, else lets winit decide. The native display handle is
resolved in `crates/muvrix-ui/src/native_display.rs` and passed to libmpv as
`MPV_RENDER_PARAM_WL_DISPLAY` or `MPV_RENDER_PARAM_X11_DISPLAY`.

Notes:

- The direct (zero-copy) VAAPI path depends on that native display handle. Under
  X11 the EGL surface is created against XCB; GLX is not used.
- Architecture selection is `egl` on both backends; there is no GLX path and
  none is planned unless X11 measurement shows EGL is unavailable.
- `wp_presentation` feedback is Wayland-only. On X11 the runner falls back to
  acknowledging after a successful swap, which is the documented-safe fallback
  in `mpv/render.h` (no feedback object can be created).
- **X11 status: source-audited and compile-tested. Not runtime-tested.**

## 4. GNOME, KDE and other desktop environments

Nothing in the shared code targets GNOME specifically. The only compositor
protocol used beyond core Wayland is `wp_presentation` (presentation feedback)
and `wp_color_manager_v1` (HDR description). Both are optional:

- Missing presentation feedback → ack-after-swap fallback.
- Missing colour management → SDR path, HDR reported unavailable.

KDE Plasma supports both protocols; Mutter supports both. This has been observed
on GNOME only.

## 5. Linux rendering architecture

Layers, kept strictly separate:

```
muvrix-ui       shared UI + presentation shell (runner, presenter, compositor)
  └ muvrix-player   playback controller + libmpv engine interface
      └ libmpv-sys  raw FFI bindings
```

- `render_engine.rs` owns the OpenGL FBO, the libmpv render context, target-time
  scheduling and presentation tokens. It knows nothing about vendors.
- `platform/presenter.rs` owns EGL config selection, the Wayland/X11 surface and
  buffer format negotiation.
- `decode.rs` owns hardware-decoding *observation*; it never assumes a vendor.
- `vaapi.rs` probes VAAPI capability via `dlopen` so a host without libva does
  not fail to start.

The paint loop is driven by libmpv's update callback. A frame whose target time
has already elapsed must **not** trigger an extra repaint; doing so commits
redundant buffers that compete with real frames for compositor slots.

## 6. Intel, AMD and NVIDIA decoding strategy

Policy in `crates/muvrix-player/src/decode.rs`:

- Ordered decoder candidates are per-platform, not per-vendor.
- `auto-safe` is the initial mode; libmpv chooses what actually works.
- The **active** decoder is read back from `hwdec-current` and the decoded pixel
  format after initialisation, and classified as `DirectHw`, `HwCopyBack`,
  `Software`, `Inactive` or `Unknown`.
- A request is never reported as a success. Diagnostics show what ran.
- Users can override the mode; the override is honoured and its result observed.

Per vendor, expected libmpv names (source-audited, only Intel observed):

| Vendor | Backends libmpv may select | Observed |
|---|---|---|
| Intel | `vaapi`, `vaapi-copy` | Yes — direct `vaapi` |
| AMD | `vaapi`, `vaapi-copy` | No |
| NVIDIA | `nvdec`, `cuda`, `vdpau` | No |

Fallback order is always: direct → copy-back → software. Software decoding is a
supported state, not an error.

## 7. libmpv runtime dependency strategy

Two mutually exclusive models, chosen by packaging channel.

### A. System-managed (RPM, DEB, Arch)

- Depend on the distribution's libmpv.
- Never download, replace or shadow the system library.
- Declare the minimum client-API version in package metadata.
- Let the package manager perform updates.

### B. Bundled (AppImage, Flatpak)

- Ship a pinned, tested libmpv next to the binary.
- The launcher prefers the private runtime, then falls back to the system one.
- The bundled library is updated only as part of a tested MUVRIX release.
- A loaded library is never overwritten in place.

## 8. Minimum supported API version

From `crates/muvrix-player/src/version.rs`:

- **OpenGL render backend: `MPV_CLIENT_API_VERSION >= 2.0`** — the floor for the
  entire application.
- `>= 2.6` additionally unlocks the Direct3D 11 renderer, which is irrelevant to
  the Linux build.

The minimum is derived from the oldest API MUVRIX actually calls, not picked
arbitrarily. Currently the binary links `libmpv.so.2` directly, so the dynamic
loader enforces the soname before `main()` runs.

**Startup enforcement (implemented).** `ApiVersion::current()` is read and
`validate_gl_prerequisites()` is called in `crates/muvrix-ui/src/app/init.rs`
before any libmpv handle, option or render context is created. An unsupported
runtime aborts startup with `MpvError::IncompatibleApiVersion`, carrying both
the required and the detected version, and logs the raw encoding. The supported
version is logged on every successful start:

```
libmpv runtime: client API 2.6 (raw: 131078)
```

Status: unit tested (6 cases) and runtime tested on the development host.

**Remaining limitation, stated plainly.** The check only runs if the dynamic
loader can resolve the library at all. The binary links `libmpv.so.2` directly,
so a host whose `libmpv.so.2` is older than the client API floor — or which has
no `libmpv.so.2` — fails before `main()` with the loader's own message, not
MUVRIX's. This is inherent to direct linking and is not fixed by the startup
check. Closing it requires `dlopen`-based loading or a launcher-side probe; both
are deferred, and the packaging strategy in §11 mitigates it by shipping a
tested libmpv with the portable formats and declaring the system dependency for
distro packages.

## 9. Compatibility and feature detection

Current state:

- Runtime version: `mpv_client_api_version()` in `version.rs`.
- Capability probing: `capability_test.rs` integration test.
- Optional properties: reads use `get_property(...).ok()` style so a missing
  property degrades rather than aborts.
- Optional commands: executed through the command API with error results
  surfaced, not unwrapped.

Guidance for new code: prefer **capability detection and observing actual
results** over version comparisons. Do not scatter `if version >= x` through the
codebase; add a predicate to `version.rs` or a probe helper instead.

## 10. System-managed versus bundled libmpv

Decision rule:

- Distributions with a maintained libmpv → system-managed.
- Portable/self-contained formats → bundled.
- Flatpak → bundled inside the sandbox, because no runtime ships libmpv 0.42.

Never both at once for the same installation. The launcher implements the
bundled-first, system-fallback order and is the only place that decision lives.

## 10.5 Bundling boundary and the glibc floor

Two rules decide what ships inside the package and what is taken from the host.
Both were violated by the bundle as first built, and both are now enforced in
`packaging/build_runtime_bundle.sh` via an explicit denylist.

**Host-owned, never bundled:** the C library and loader, OpenSSL, D-Bus, the
graphics driver stack (EGL/GL/DRM/Vulkan/VA-API), and - newly added - the X11,
audio-server and desktop-integration families (`libX11*`, `libxcb*`, `libXau`,
`libXdmcp`, `libXext`, `libXfixes`, `libXi`, `libXrandr`, `libXrender`,
`libXcursor`, `libXScrnSaver`, `libXcomposite`, `libXdamage`, `libXinerama`,
`libXtst`, `libasound`, `libpulse*`, `libpipewire*`, `libSDL2*`, `libapparmor`).

The reason the last group matters is not tidiness. Each of those talks to a
service that is already running on the host - the X server over its socket,
PipeWire or PulseAudio over their protocol. A private copy pins a client that
may not match the daemon the user actually has, and because the launcher
prepends the bundle to `LD_LIBRARY_PATH` the private copy silently wins. That
would surface as broken X11 or broken audio on a host the package claims to
support.

**Bundled:** libmpv, FFmpeg, libplacebo, libass and the rest of the media
decode/convert closure - anything that does not negotiate with a host daemon.

**The glibc floor.** The shipped binary required `GLIBC_2.39` for exactly two
symbols: `pidfd_spawnp` and `pidfd_getpid`, which Rust's standard library uses
for process spawning and which Ubuntu 24.04 added to glibc. Building the same
source on Debian 12 produces a `2.36` floor with no source change.

The bundle is therefore built on `debian:12`. That release was chosen by
measurement, not preference:

| Baseline | glibc | Viable | Why |
|---|---|---|---|
| Ubuntu 24.04 | 2.39 | yes | previous baseline; excludes Debian 12, Ubuntu 22.04 |
| Debian 12 | 2.36 | **chosen** | has `libshaderc-dev`; libplacebo needs `-Dshaderc=enabled` |
| Ubuntu 22.04 | 2.35 | no | no `libshaderc-dev` package at all |

Meson is pinned to 1.4.2 from PyPI (Debian 12 ships 1.0.1; libplacebo requires
>= 1.3.0) and FFmpeg 6.1 is built from the pinned release tarball (libplacebo
7.387 needs `libavcodec >= 60.31.102`; Debian 12 has 59.x).

Status: the baseline change is source-audited and the `2.36` build was verified
in a container, but **no package has been regenerated from it yet** - the FFmpeg
build was interrupted. The packages currently in `target/packages` still contain
the 2.39 bundle. Treat 2.36 as the intended floor, not the shipped one, until
the runtime is rebuilt and the packages regenerated.

An AppImage is not independent of glibc: it inherits exactly this floor.

## 11. Packaging strategy

| Format | libmpv source | Status |
|---|---|---|
| RPM | **bundled** under `/usr/lib/muvrix/lib` | Script present; `Requires` names host libs only |
| DEB | **bundled** under `/usr/lib/muvrix/lib` | Script present; built and inspected (see below) |
| Arch | **system** libmpv (see §11) | `packaging/arch/PKGBUILD` present; compile-checked only |
| AppImage | bundled under `usr/lib/muvrix/lib` | `AppRun` prepends it to `LD_LIBRARY_PATH` |
| Flatpak | bundled, pinned sources | Manifest present; ships `runtime/lib` to `/app/lib/muvrix` |

**The traditional packages bundle libmpv, they do not use the system copy.**
This corrects an assumption in earlier drafts. `packaging/launcher/muvrix-launcher.sh`
states the reason: MUVRIX requires client API 2.6 (libmpv 0.42) and no supported
distribution ships it. Ubuntu 24.04 provides 0.37, which would fail the startup
check added in §8.

Consequence for dependencies: `${shlibs:Depends}` must **not** expand to a
system `libmpv2` dependency, because the package does not link against the host
library. `packaging/build_deb.sh` resolves `${shlibs:Depends}` explicitly to a
host-runtime list and deliberately omits libmpv. Verified against a real build:

```
Package: muvrix
Version: 0.1.0
Depends: libc6 (>= 2.39), libegl1, libgl1, libdbus-1-3, libssl3 (>= 3.0.0),
         libwayland-client0, libwayland-egl1, libxkbcommon0, libxkbcommon-x11-0,
         libx11-6, libva2, libva-drm2, libva-x11-2
```

and the payload contains the private runtime:

```
./usr/bin/muvrix                              (launcher)
./usr/lib/muvrix/lib/libmpv.so.2 -> libmpv.so.2.6.0
./usr/lib/muvrix/lib/libmpv.so.2.6.0
```

Note that this makes the DEB/RPM packages **larger** than a purely
system-dependent build and means CVE fixes for libmpv arrive through MUVRIX
releases, not through the distribution. That is a deliberate trade: correctness
against a fixed, tested engine beats a smaller download at this stage. If a
future distribution ships a new-enough libmpv, a system-dependent variant
becomes viable and should be added as a separate package flavour rather than by
changing this one.

Required host runtime beyond libmpv: a C library new enough for the private
runtime (glibc ≥ 2.36 after the Debian 12 rebuild; the shipped packages currently
still contain a 2.39-built bundle until they are regenerated), EGL/OpenGL, D-Bus, OpenSSL, the
Wayland/X11 client libraries and libva. Audio is delegated to libmpv's own
outputs, so the host's PipeWire/PulseAudio/ALSA stack is used rather than linked
directly.

## 12. Automatic application update policy

**Not implemented. Architecture only.**

Intended model:

1. Check a signed release manifest on the stable channel; beta channel opt-in.
2. Verify checksum and signature before any install.
3. Stage the new version and apply it on restart, never mid-playback.
4. Keep the previous version for rollback if the new one fails to start.
5. For system-managed installs, notify only — the package manager updates, not
   MUVRIX. MUVRIX must never replace a package-manager-owned library.
6. For bundled installs, update the application and its pinned libmpv together.

## 13. CI testing matrix

Planned, with current status:

| Job | Purpose | Status |
|---|---|---|
| `linux-ubuntu` | baseline compile + tests | Present |
| `linux-glibc-floor` (debian:12) | assert no object needs glibc > 2.36 | **Added** |
| `linux-fedora` (container) | Fedora/RPM variance | Present |
| `packaging` | RPM/DEB/AppImage/Flatpak artifacts | Present |
| clippy + fmt | hygiene | Present |
| unit tests | pure logic | Present (150 in `muvrix-ui`) |
| libmpv link smoke test | soname/API presence | Present |
| **libmpv runtime playback test** | actually decode a media fixture | **Added** — `generate deterministic playback fixture` + `cargo test -p muvrix-player --test playback_test` |
| **X11 playback test (Xvfb)** | X11 path | **Missing** |
| **old-libmpv job** | minimum supported version | **Missing** |

Note: `ci.yml` also contains `macos-apple-silicon` and a Windows job. Those are
outside the declared Linux scope of this document and are not part of the
supported matrix described here; they are left untouched by this audit.

The runtime playback job closes the gap where every libmpv-touching job either
compiled or linked and none decoded a file. It uses `vo=null`/`ao=null` with
software decoding, so it needs no GPU, no driver and no VAAPI, and it asserts
video format, width/height, audio parameters, duration and an advancing playback
position. It is deliberately a **decode-and-initialisation** test, not a GPU
rendering test — nothing in it exercises the OpenGL path or presentation
feedback.

The fixture (3 s, 640x360, H.264 + AAC) is generated in the job with FFmpeg
rather than committed, because `docs/tests/` is gitignored by project
convention.

## 14. Linux implementation roadmap

Ordered by risk, not by novelty. Items 1 and 2 are done.

1. ~~**Call the version gate at startup.**~~ **Done.** `validate_gl_prerequisites()`
   now runs in `app/init.rs` before any engine state exists, and logs the
   detected API version. Unit tested and runtime tested. The loader-level gap
   (library absent or too old) remains open — see §8.
2. ~~**Runtime playback CI job.**~~ **Done.** See §13.
3. **`dlopen`-based loading** (or a launcher probe) so a missing or too-old
   `libmpv.so.2` produces a MUVRIX message instead of a loader error.
4. **X11 runtime verification** on a real X server, including fullscreen
   transition and presentation timing.
5. ~~**Arch packaging** (PKGBUILD).~~ **Added**, compile-checked only — still needs a
   real install and playback test on Arch.
6. **AMD and NVIDIA decoding verification** on real hardware, recording the
   observed `hwdec-current` for each.
7. **High-bitrate 4K60 investigation**, as a separate scoped effort. Cause is
   **not established**; it must not be "fixed" by disabling frame dropping.
8. **Multi-monitor and HDR transition** verification.
