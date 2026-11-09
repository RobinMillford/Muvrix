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
#[cfg(target_os = "windows")]
pub mod render_d3d11;
pub mod render_gl;
pub mod stream_cb;

pub use client::*;
pub use render::*;
#[cfg(target_os = "windows")]
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

    #[test]
    fn test_d3d11_param_constants_match_upstream() {
        // MPV_RENDER_PARAM_D3D11_INIT_PARAMS = 21, MPV_RENDER_PARAM_D3D11_FBO = 22
        // (mpv/include/mpv/render.h, API 2.6)
        assert_eq!(
            mpv_render_param_type::MPV_RENDER_PARAM_D3D11_INIT_PARAMS as i32,
            21
        );
        assert_eq!(mpv_render_param_type::MPV_RENDER_PARAM_D3D11_FBO as i32, 22);
        assert_eq!(MPV_RENDER_API_TYPE_D3D11, b"d3d11\0");
    }

    #[test]
    fn test_opengl_fbo_layout() {
        // mpv_opengl_fbo { int fbo, int w, int h, int internal_format }
        assert_eq!(std::mem::size_of::<mpv_opengl_fbo>(), 16);
        assert_eq!(std::mem::align_of::<mpv_opengl_fbo>(), 4);
    }

    #[test]
    fn test_opengl_init_params_layout() {
        // mpv_opengl_init_params { void* get_proc_address; void* get_proc_address_ctx }
        assert_eq!(std::mem::size_of::<mpv_opengl_init_params>(), 16);
        assert_eq!(std::mem::align_of::<mpv_opengl_init_params>(), 8);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_d3d11_struct_layout() {
        // mpv_d3d11_init_params { void *device } and mpv_d3d11_fbo { void *tex }
        // (mpv/include/mpv/render_d3d11.h, API 2.6) — a single pointer each.
        assert_eq!(std::mem::size_of::<mpv_d3d11_init_params>(), 8);
        assert_eq!(std::mem::align_of::<mpv_d3d11_init_params>(), 8);
        assert_eq!(std::mem::size_of::<mpv_d3d11_fbo>(), 8);
        assert_eq!(std::mem::align_of::<mpv_d3d11_fbo>(), 8);
    }
}
