//! Safe abstractions for `mpv_render_context`.

use std::os::raw::{c_int, c_void};
use std::sync::atomic::{AtomicBool, Ordering};

use libmpv_sys::{
    mpv_opengl_fbo, mpv_opengl_get_proc_address_fn, mpv_opengl_init_params, mpv_render_context,
    mpv_render_frame_info, mpv_render_param, mpv_render_param_type, mpv_render_update_flag,
    MPV_RENDER_API_TYPE_OPENGL, MPV_RENDER_FRAME_INFO_PRESENT,
};

use crate::error::MpvError;
use crate::handle::MpvHandle;

/// Native display resource that the engine needs in order to initialise a
/// hardware-surface interop context.
///
/// `mpv/render_gl.h` (v0.42.0) requires the native display for direct hardware
/// decoding: `MPV_RENDER_PARAM_X11_DISPLAY` on X11 and
/// `MPV_RENDER_PARAM_WL_DISPLAY` on Wayland. Without it the engine cannot create
/// a VAAPI interop context at all and silently falls back to software decoding.
///
/// # Lifetimes
///
/// mpv does **not** copy the resource; it keeps the pointer for the lifetime of
/// the render context. The caller must guarantee that the windowing-system
/// connection outlives this render context. `MpvRenderContext::destroy` (also
/// run from `Drop`) releases mpv's reference; the host connection must still be
/// alive at that point, so the render context has to be destroyed *before* the
/// window/display is torn down.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NativeDisplay {
    /// `wl_display*` of the connection owning the application surface.
    Wayland(*mut c_void),
    /// `Display*` (Xlib/XCB) of the connection owning the application window.
    X11(*mut c_void),
}

impl NativeDisplay {
    /// Short label for diagnostics.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Wayland(_) => "wayland",
            Self::X11(_) => "x11",
        }
    }

    /// Checks that the handle is non-null.
    pub fn is_valid(&self) -> bool {
        match self {
            Self::Wayland(p) | Self::X11(p) => !p.is_null(),
        }
    }
}

impl std::fmt::Debug for NativeDisplay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Wayland(p) => write!(f, "NativeDisplay::Wayland({p:p})"),
            Self::X11(p) => write!(f, "NativeDisplay::X11({p:p})"),
        }
    }
}

/// Safe wrapper around `mpv_render_context`.
pub struct MpvRenderContext {
    raw: *mut mpv_render_context,
    update_cb: Option<Box<Box<dyn Fn() + Send + 'static>>>,
    is_destroyed: AtomicBool,
    native_display_kind: Option<&'static str>,
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
        Self::new_gl_with_display(handle, get_proc_address, get_proc_address_ctx, None)
    }

    /// Creates an OpenGL render context and supplies the native display that
    /// enables direct hardware-surface import.
    ///
    /// # Safety
    ///
    /// The host OpenGL context must be current on the calling thread, and the
    /// native display connection must outlive the returned render context
    /// (see [`NativeDisplay`]).
    pub unsafe fn new_gl_with_display(
        handle: &MpvHandle,
        get_proc_address: mpv_opengl_get_proc_address_fn,
        get_proc_address_ctx: *mut c_void,
        display: Option<NativeDisplay>,
    ) -> Result<Self, MpvError> {
        if get_proc_address.is_none() {
            return Err(MpvError::InvalidParameter);
        }

        let mut init_params = mpv_opengl_init_params {
            get_proc_address,
            get_proc_address_ctx,
        };

        let mut params = vec![
            mpv_render_param {
                type_: mpv_render_param_type::MPV_RENDER_PARAM_API_TYPE,
                data: MPV_RENDER_API_TYPE_OPENGL.as_ptr() as *mut c_void,
            },
            mpv_render_param {
                type_: mpv_render_param_type::MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
                data: &mut init_params as *mut _ as *mut c_void,
            },
        ];

        // The native display parameter is what makes a direct (zero-copy)
        // hardware decode path possible. It is deliberately optional so that
        // platforms/paths without a usable handle keep the previous behaviour
        // (which resolves to copy-back or software decoding).
        if let Some(display) = display.filter(NativeDisplay::is_valid) {
            let (type_, data) = match display {
                NativeDisplay::Wayland(p) => {
                    (mpv_render_param_type::MPV_RENDER_PARAM_WL_DISPLAY, p)
                }
                NativeDisplay::X11(p) => (mpv_render_param_type::MPV_RENDER_PARAM_X11_DISPLAY, p),
            };
            params.push(mpv_render_param { type_, data });
        }

        params.push(mpv_render_param {
            type_: mpv_render_param_type::MPV_RENDER_PARAM_INVALID,
            data: std::ptr::null_mut(),
        });

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
            native_display_kind: display.map(|d| d.kind()),
        })
    }

    /// Native display family the context was created with, if any.
    pub fn native_display_kind(&self) -> Option<&'static str> {
        self.native_display_kind
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
        fbo: mpv_opengl_fbo,
        flip_y: bool,
        block_for_target_time: bool,
    ) -> Result<(), MpvError> {
        self.render_gl_ex(fbo, flip_y, block_for_target_time, false)
            .map(|_| ())
    }

    /// Advances the engine's render/timing path without drawing anything.
    ///
    /// `MPV_RENDER_PARAM_SKIP_RENDERING` tells the engine to treat the call as a
    /// completed render while skipping the actual draw, keeping frame timing and
    /// consumption intact. This is the intended way for an embedder to avoid
    /// redundant draw work when no new frame is pending.
    ///
    /// # Safety
    ///
    /// Must be called with the OpenGL context current on the calling thread.
    pub fn render_gl_skipped(&self, block_for_target_time: bool) -> Result<(), MpvError> {
        if self.is_destroyed.load(Ordering::Acquire) || self.raw.is_null() {
            return Err(MpvError::NullHandle);
        }
        let mut flip_y_val: c_int = 0;
        let mut block_val: c_int = if block_for_target_time { 1 } else { 0 };
        let mut skip_val: c_int = 1;
        let mut params = [
            mpv_render_param {
                type_: mpv_render_param_type::MPV_RENDER_PARAM_FLIP_Y,
                data: &mut flip_y_val as *mut _ as *mut c_void,
            },
            mpv_render_param {
                type_: mpv_render_param_type::MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME,
                data: &mut block_val as *mut _ as *mut c_void,
            },
            mpv_render_param {
                type_: mpv_render_param_type::MPV_RENDER_PARAM_SKIP_RENDERING,
                data: &mut skip_val as *mut _ as *mut c_void,
            },
            mpv_render_param {
                type_: mpv_render_param_type::MPV_RENDER_PARAM_INVALID,
                data: std::ptr::null_mut(),
            },
        ];
        let res = unsafe { libmpv_sys::mpv_render_context_render(self.raw, params.as_mut_ptr()) };
        MpvError::from_raw(res)
    }

    /// Renders a frame and optionally reports the engine's intended display
    /// time for it (`MPV_RENDER_PARAM_NEXT_FRAME_INFO`).
    ///
    /// The returned target time is an absolute timestamp in microseconds (same
    /// base as `mpv_get_time_us()`), or `None` when the engine reports that the
    /// next frame is not scheduled to a specific time (redraw, or vsync-locked
    /// timing). See `mpv/render.h` v0.42.0, `mpv_render_frame_info`.
    ///
    /// # Safety
    ///
    /// Must be called with the OpenGL context current on the calling thread.
    pub fn render_gl_ex(
        &self,
        mut fbo: mpv_opengl_fbo,
        flip_y: bool,
        block_for_target_time: bool,
        query_frame_info: bool,
    ) -> Result<Option<i64>, MpvError> {
        if self.is_destroyed.load(Ordering::Acquire) || self.raw.is_null() {
            return Err(MpvError::NullHandle);
        }

        let mut flip_y_val: c_int = if flip_y { 1 } else { 0 };
        let mut block_val: c_int = if block_for_target_time { 1 } else { 0 };
        let mut frame_info = mpv_render_frame_info::default();

        let mut params = vec![
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
        ];

        if query_frame_info {
            params.push(mpv_render_param {
                type_: mpv_render_param_type::MPV_RENDER_PARAM_NEXT_FRAME_INFO,
                data: &mut frame_info as *mut _ as *mut c_void,
            });
        }

        params.push(mpv_render_param {
            type_: mpv_render_param_type::MPV_RENDER_PARAM_INVALID,
            data: std::ptr::null_mut(),
        });

        let res = unsafe { libmpv_sys::mpv_render_context_render(self.raw, params.as_mut_ptr()) };
        MpvError::from_raw(res)?;

        if !query_frame_info || (frame_info.flags & MPV_RENDER_FRAME_INFO_PRESENT) == 0 {
            return Ok(None);
        }
        if frame_info.target_time <= 0 {
            return Ok(None);
        }
        Ok(Some(frame_info.target_time))
    }

    /// Informs the renderer that a frame was presented.
    pub fn report_swap(&self) {
        if !self.is_destroyed.load(Ordering::Acquire) && !self.raw.is_null() {
            unsafe { libmpv_sys::mpv_render_context_report_swap(self.raw) };
        }
    }

    /// Destroys the render context and releases mpv's reference to the native
    /// display.
    ///
    /// Must be called with the OpenGL context current on the calling thread,
    /// and while the native display connection is still alive.
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
