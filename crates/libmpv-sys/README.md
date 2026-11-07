# libmpv-sys

Raw FFI declarations for `libmpv` C API in Rust.

## Scope & Header Mapping

Bindings are verified against the upstream mpv headers:
- `client.h` (`mpv_create`, `mpv_initialize`, `mpv_command`, properties, events, errors)
- `render.h` (`mpv_render_context`, parameters, flags)
- `render_gl.h` (`mpv_opengl_init_params`, `mpv_opengl_fbo`)
- `render_d3d11.h` (`mpv_d3d11_init_params`, `mpv_d3d11_fbo`)
- `stream_cb.h` (custom stream protocols)

## Linkage

Discovered via `pkg-config` using `pkg_config::probe("mpv")`. Supports environment overrides `MPV_LIB_DIR` and `PKG_CONFIG_PATH`.
