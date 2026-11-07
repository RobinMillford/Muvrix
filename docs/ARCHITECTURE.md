# Muvrix Architecture Overview

This document presents the high-level architecture of Muvrix, a desktop media player built with Rust, egui, and upstream libmpv.

---

## 1. Architectural Layers

Muvrix follows a strict unidirectional data flow and three-tier layer encapsulation model:

```
+--------------------------------------------------------------+
|                         muvrix-ui                            |
|  - eframe / egui Desktop Shell                               |
|  - glow OpenGL 3.3+ Offscreen FBO Compositor & Quad Shader   |
|  - Procedural Vector Icon Layer (egui::Painter)              |
|  - Interactive Controls, Dedicated Timeline, Overlays       |
+------------------------------+-------------------------------+
                               | Command Dispatch & State Sync
                               v
+--------------------------------------------------------------+
|                       muvrix-player                          |
|  - Authoritative PlaybackController                          |
|  - Safe RAII MpvHandle Lifecycle Wrapper                     |
|  - Non-blocking Event Pump (wait_event(0.0))                 |
|  - Property Observation (time-pos, duration, pause, etc.)   |
|  - Audio / Subtitle / Chapter Domain Abstractions            |
+------------------------------+-------------------------------+
                               | Safe FFI Boundary
                               v
+--------------------------------------------------------------+
|                       libmpv-sys                             |
|  - Raw C FFI Bindings (client.h, render.h, render_gl.h)      |
|  - Cross-Platform Dynamic Linker Probing                     |
|  - Windows MSVC libmpv-2.def Module Definition               |
+------------------------------+-------------------------------+
                               | Dynamic Linkage
                               v
               Upstream libmpv Native Shared Library
               (libmpv.so.2 / libmpv.dylib / mpv-2.dll)
```

Supporting crate:
- **`muvrix-core`**: Cross-platform path resolution (`resolve_media_path`), tilde expansion, platform telemetry (`PlatformInfo`, `SessionType`), and shared error types.

---

## 2. Core Architectural Principles

1. **Sole Media Engine**:
   `libmpv` is the sole engine for media demuxing, video decoding, audio decoding, and subtitle rendering. No duplicate decoding pipelines or secondary media engines are introduced.

2. **Authoritative Boundary**:
   The UI crate (`muvrix-ui`) never communicates directly with raw C pointers (`*mut mpv_handle`) or invokes unsafe FFI functions. All commands, property queries, and lifecycle controls pass through `PlaybackController`.

3. **Non-Blocking UI Frame Loop**:
   libmpv events are retrieved non-blockingly using `wait_event(0.0)` during egui's frame update. Registered property observers with userdata IDs eliminate synchronous property polling overhead.

4. **Off-Screen OpenGL FBO Compositor**:
   `libmpv`'s OpenGL Render API (`mpv_render_context`) draws directly into an off-screen Framebuffer Object (FBO) managed by Muvrix. An OpenGL 3.3+ quad shader then samples this texture and paints it inside egui's CentralPanel, preserving the video's native aspect ratio with clean letterboxing.

5. **Portable Vector Iconography**:
   All UI icons are rendered procedurally using `egui::Painter` geometry ([`crates/muvrix-ui/src/icons.rs`](../crates/muvrix-ui/src/icons.rs)). This eliminates dependencies on external raster assets or inconsistent platform emoji fonts.
