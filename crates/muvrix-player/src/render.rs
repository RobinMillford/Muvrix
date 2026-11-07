//! Safe abstractions for `mpv_render_context`.

use std::os::raw::{c_int, c_void};
use std::sync::atomic::{AtomicBool, Ordering};

use libmpv_sys::{
    mpv_opengl_fbo, mpv_opengl_get_proc_address_fn, mpv_opengl_init_params, mpv_render_context,
    mpv_render_param, mpv_render_param_type, mpv_render_update_flag, MPV_RENDER_API_TYPE_OPENGL,
};

use crate::error::MpvError;
use crate::handle::MpvHandle;

/// Safe wrapper around `mpv_render_context`.
pub struct MpvRenderContext {
    raw: *mut mpv_render_context,
    update_cb: Option<Box<Box<dyn Fn() + Send + 'static>>>,
    is_destroyed: AtomicBool,
}

// mpv_render_context can be transferred across threads, but calls to mpv_render_*
// must execute with the owning OpenGL context current on that thread.
unsafe impl Send for MpvRenderContext {}

impl std::fmt::Debug for MpvRenderContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MpvRenderContext")
            .field("raw", &self.raw)
            .field("is_destroyed", &self.is_destroyed.load(Ordering::Relaxed))
            .finish()
    }
}

impl MpvRenderContext {
    /// Creates an OpenGL render context for the given mpv handle.
    ///
    /// # Safety
    ///
    /// The host OpenGL context must be current on the calling thread.
    /// `get_proc_address` must resolve OpenGL function pointers for the current context.
    pub unsafe fn new_gl(
        handle: &MpvHandle,
        get_proc_address: mpv_opengl_get_proc_address_fn,
        get_proc_address_ctx: *mut c_void,
    ) -> Result<Self, MpvError> {
        if get_proc_address.is_none() {
            return Err(MpvError::InvalidParameter);
        }

        let mut init_params = mpv_opengl_init_params {
            get_proc_address,
            get_proc_address_ctx,
        };

        let mut params = [
            mpv_render_param {
                type_: mpv_render_param_type::MPV_RENDER_PARAM_API_TYPE,
                data: MPV_RENDER_API_TYPE_OPENGL.as_ptr() as *mut c_void,
            },
            mpv_render_param {
                type_: mpv_render_param_type::MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
                data: &mut init_params as *mut _ as *mut c_void,
            },
            mpv_render_param {
                type_: mpv_render_param_type::MPV_RENDER_PARAM_INVALID,
                data: std::ptr::null_mut(),
            },
        ];

        let mut raw = std::ptr::null_mut();
        let res =
            libmpv_sys::mpv_render_context_create(&mut raw, handle.as_ptr(), params.as_mut_ptr());

        if res < 0 {
            return Err(MpvError::from_raw(res).unwrap_err());
        }

        Ok(Self {
            raw,
            update_cb: None,
            is_destroyed: AtomicBool::new(false),
        })
    }

    /// Sets the update callback to be notified when a new frame is available.
    ///
    /// libmpv invokes this callback from an internal engine thread.
    /// The callback MUST NOT invoke rendering or libmpv APIs. It should only
    /// trigger a repaint request on the host GUI framework.
    pub fn set_update_callback<F: Fn() + Send + 'static>(&mut self, callback: F) {
        if self.is_destroyed.load(Ordering::Acquire) || self.raw.is_null() {
            return;
        }

        unsafe extern "C" fn trampoline(cb_ctx: *mut c_void) {
            if !cb_ctx.is_null() {
                let cb = &*(cb_ctx as *const Box<dyn Fn() + Send + 'static>);
                cb();
            }
        }

        let boxed: Box<Box<dyn Fn() + Send + 'static>> = Box::new(Box::new(callback));
        let ptr = boxed.as_ref() as *const Box<dyn Fn() + Send + 'static> as *mut c_void;

        unsafe {
            libmpv_sys::mpv_render_context_set_update_callback(self.raw, Some(trampoline), ptr);
        }

        self.update_cb = Some(boxed);
    }

    /// Queries render update flags (`mpv_render_context_update()`).
    ///
    /// Returns bitflags indicating whether a new frame should be rendered.
    pub fn update(&self) -> u64 {
        if self.is_destroyed.load(Ordering::Acquire) || self.raw.is_null() {
            return 0;
        }
        unsafe { libmpv_sys::mpv_render_context_update(self.raw) }
    }

    /// Checks if a new frame is ready to be drawn (`MPV_RENDER_UPDATE_FRAME`).
    pub fn has_frame(&self) -> bool {
        (self.update() & (mpv_render_update_flag::MPV_RENDER_UPDATE_FRAME as u64)) != 0
    }

    /// Renders the video frame into an OpenGL framebuffer.
    ///
    /// # Safety
    ///
    /// Must be called with the OpenGL context current on the calling thread.
    pub fn render_gl(
        &self,
        mut fbo: mpv_opengl_fbo,
        flip_y: bool,
        block_for_target_time: bool,
    ) -> Result<(), MpvError> {
        if self.is_destroyed.load(Ordering::Acquire) || self.raw.is_null() {
            return Err(MpvError::NullHandle);
        }

        let mut flip_y_val: c_int = if flip_y { 1 } else { 0 };
        let mut block_val: c_int = if block_for_target_time { 1 } else { 0 };

        let mut params = [
            mpv_render_param {
                type_: mpv_render_param_type::MPV_RENDER_PARAM_OPENGL_FBO,
                data: &mut fbo as *mut _ as *mut c_void,
            },
            mpv_render_param {
                type_: mpv_render_param_type::MPV_RENDER_PARAM_FLIP_Y,
                data: &mut flip_y_val as *mut _ as *mut c_void,
            },
            mpv_render_param {
                type_: mpv_render_param_type::MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME,
                data: &mut block_val as *mut _ as *mut c_void,
            },
            mpv_render_param {
                type_: mpv_render_param_type::MPV_RENDER_PARAM_INVALID,
                data: std::ptr::null_mut(),
            },
        ];

        let res = unsafe { libmpv_sys::mpv_render_context_render(self.raw, params.as_mut_ptr()) };
        MpvError::from_raw(res)
    }

    /// Informs the renderer that a frame was presented.
    pub fn report_swap(&self) {
        if !self.is_destroyed.load(Ordering::Acquire) && !self.raw.is_null() {
            unsafe { libmpv_sys::mpv_render_context_report_swap(self.raw) };
        }
    }

    /// Destroys the render context.
    ///
    /// Must be called with the OpenGL context current on the calling thread.
    pub fn destroy(&mut self) {
        if !self.is_destroyed.swap(true, Ordering::AcqRel) && !self.raw.is_null() {
            unsafe {
                // 1. Deregister update callback
                libmpv_sys::mpv_render_context_set_update_callback(
                    self.raw,
                    None,
                    std::ptr::null_mut(),
                );
                // 2. Free context
                libmpv_sys::mpv_render_context_free(self.raw);
            }
            self.raw = std::ptr::null_mut();
            self.update_cb = None;
        }
    }
}

impl Drop for MpvRenderContext {
    fn drop(&mut self) {
        self.destroy();
    }
}
