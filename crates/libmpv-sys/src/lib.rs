//! `libmpv-sys`: Raw FFI bindings to libmpv C API.
//!
//! Handcrafted and verified directly against upstream headers:
//! - `mpv/client.h`
//! - `mpv/render.h`
//! - `mpv/render_gl.h`
//! - `mpv/render_d3d11.h`
//! - `mpv/stream_cb.h`

pub mod client;
pub mod render;
pub mod render_d3d11;
pub mod render_gl;
pub mod stream_cb;

pub use client::*;
pub use render::*;
pub use render_d3d11::*;
pub use render_gl::*;
pub use stream_cb::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_api_version() {
        let version = unsafe { mpv_client_api_version() };
        let major = version >> 16;
        let minor = version & 0xFFFF;
        println!("Runtime libmpv API version: {major}.{minor} (raw: {version})");
        // Phase 0.1 requires API >= 2.0
        assert!(
            version >= mpv_make_version(2, 0),
            "Runtime libmpv API version {major}.{minor} is older than required 2.0"
        );
    }

    #[test]
    fn test_create_destroy_handle() {
        unsafe {
            let handle = mpv_create();
            assert!(!handle.is_null(), "mpv_create returned null handle");
            mpv_terminate_destroy(handle);
        }
    }
}
