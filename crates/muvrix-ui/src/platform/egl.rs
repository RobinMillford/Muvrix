//! Narrow raw EGL binding for the Muvrix presentation runner.
//!
//! Only the subset the runner needs is declared here. Everything is direct
//! linking against `libEGL`/`libwayland-egl`; no loader crate sits between the
//! runner and the driver, so the EGL config/context/surface the runner selects
//! is the one that is actually used.
//!
//! # Why not `eframe`'s EGL path
//!
//! Phase 1.9 established (see `docs/phase-1.9/`) that a client which does not
//! own its EGL surface cannot (a) select a 10-bit configuration, (b) describe
//! the surface through `wp_color_manager_v1`, or (c) control the swap interval.
//! All three are requirements for the presentation and HDR paths, so the runner
//! owns them directly.

#![allow(non_camel_case_types, non_upper_case_globals, dead_code)]

use std::ffi::c_void;
use std::os::raw::{c_char, c_int, c_uint};

pub type EGLBoolean = c_uint;
pub type EGLenum = c_uint;
pub type EGLint = c_int;
pub type EGLAttrib = isize;
pub type EGLDisplay = *mut c_void;
pub type EGLConfig = *mut c_void;
pub type EGLSurface = *mut c_void;
pub type EGLContext = *mut c_void;
pub type EGLNativeWindowType = *mut c_void;

pub const EGL_FALSE: EGLBoolean = 0;
pub const EGL_TRUE: EGLBoolean = 1;

pub const EGL_PLATFORM_WAYLAND_KHR: EGLenum = 0x31D8;
pub const EGL_PLATFORM_X11_KHR: EGLenum = 0x31D5;

pub const EGL_NONE: EGLint = 0x3038;
pub const EGL_SUCCESS: EGLint = 0x3000;

pub const EGL_SURFACE_TYPE: EGLint = 0x3033;
pub const EGL_WINDOW_BIT: EGLint = 0x0004;
pub const EGL_RENDERABLE_TYPE: EGLint = 0x3040;
pub const EGL_OPENGL_ES2_BIT: EGLint = 0x0004;
pub const EGL_OPENGL_ES3_BIT: EGLint = 0x0040;
pub const EGL_RED_SIZE: EGLint = 0x3024;
pub const EGL_GREEN_SIZE: EGLint = 0x3023;
pub const EGL_BLUE_SIZE: EGLint = 0x3022;
pub const EGL_ALPHA_SIZE: EGLint = 0x3021;
pub const EGL_DEPTH_SIZE: EGLint = 0x3025;
pub const EGL_STENCIL_SIZE: EGLint = 0x3026;
pub const EGL_SAMPLES: EGLint = 0x3031;
pub const EGL_CONFIG_CAVEAT: EGLint = 0x3027;
pub const EGL_COLOR_BUFFER_TYPE: EGLint = 0x303F;
pub const EGL_RGB_BUFFER: EGLint = 0x308E;
pub const EGL_CONFIG_ID: EGLint = 0x3028;
pub const EGL_MIN_SWAP_INTERVAL: EGLint = 0x303B;
pub const EGL_MAX_SWAP_INTERVAL: EGLint = 0x303C;
pub const EGL_WIDTH: EGLint = 0x3057;
pub const EGL_HEIGHT: EGLint = 0x3056;

pub const EGL_CONTEXT_CLIENT_VERSION: EGLint = 0x3098;
pub const EGL_CONTEXT_MAJOR_VERSION: EGLint = 0x3098;
pub const EGL_CONTEXT_MINOR_VERSION: EGLint = 0x30FB;

pub const EGL_OPENGL_ES_API: EGLenum = 0x30A0;
pub const EGL_OPENGL_API: EGLenum = 0x30A2;

pub type EGLGetProcAddressFn = Option<unsafe extern "C" fn(*const c_char) -> *mut c_void>;

#[link(name = "EGL")]
extern "C" {
    pub fn eglGetProcAddress(procname: *const c_char) -> *mut c_void;
    pub fn eglGetPlatformDisplay(
        platform: EGLenum,
        native_display: *mut c_void,
        attrib_list: *const EGLAttrib,
    ) -> EGLDisplay;
    pub fn eglGetError() -> EGLint;
    pub fn eglInitialize(dpy: EGLDisplay, major: *mut EGLint, minor: *mut EGLint) -> EGLBoolean;
    pub fn eglTerminate(dpy: EGLDisplay) -> EGLBoolean;
    pub fn eglChooseConfig(
        dpy: EGLDisplay,
        attrib_list: *const EGLint,
        configs: *mut EGLConfig,
        config_size: EGLint,
        num_config: *mut EGLint,
    ) -> EGLBoolean;
    pub fn eglGetConfigAttrib(
        dpy: EGLDisplay,
        config: EGLConfig,
        attribute: EGLint,
        value: *mut EGLint,
    ) -> EGLBoolean;
    pub fn eglBindAPI(api: EGLenum) -> EGLBoolean;
    pub fn eglCreateContext(
        dpy: EGLDisplay,
        config: EGLConfig,
        share_context: EGLContext,
        attrib_list: *const EGLint,
    ) -> EGLContext;
    pub fn eglDestroyContext(dpy: EGLDisplay, ctx: EGLContext) -> EGLBoolean;
    pub fn eglCreateWindowSurface(
        dpy: EGLDisplay,
        config: EGLConfig,
        win: EGLNativeWindowType,
        attrib_list: *const EGLint,
    ) -> EGLSurface;
    pub fn eglDestroySurface(dpy: EGLDisplay, surface: EGLSurface) -> EGLBoolean;
    pub fn eglMakeCurrent(
        dpy: EGLDisplay,
        draw: EGLSurface,
        read: EGLSurface,
        ctx: EGLContext,
    ) -> EGLBoolean;
    pub fn eglReleaseThread() -> EGLBoolean;
    pub fn eglSwapBuffers(dpy: EGLDisplay, surface: EGLSurface) -> EGLBoolean;
    pub fn eglSwapInterval(dpy: EGLDisplay, interval: EGLint) -> EGLBoolean;
    pub fn eglQuerySurface(
        dpy: EGLDisplay,
        surface: EGLSurface,
        attribute: EGLint,
        value: *mut EGLint,
    ) -> EGLBoolean;
    pub fn eglQueryString(dpy: EGLDisplay, name: EGLint) -> *const c_char;
}

/// `wl_egl_window` handle returned by `libwayland-egl`.
pub type WlEglWindow = *mut c_void;

#[link(name = "wayland-egl")]
extern "C" {
    pub fn wl_egl_window_create(surface: *mut c_void, width: c_int, height: c_int) -> WlEglWindow;
    pub fn wl_egl_window_destroy(egl_window: WlEglWindow);
    pub fn wl_egl_window_resize(
        egl_window: WlEglWindow,
        width: c_int,
        height: c_int,
        dx: c_int,
        dy: c_int,
    );
    pub fn wl_egl_window_get_width(egl_window: WlEglWindow) -> c_int;
    pub fn wl_egl_window_get_height(egl_window: WlEglWindow) -> c_int;
}

/// Xlib `Display*`, used only by the X11 presenter.
pub type X11Display = *mut c_void;

// Minimal Xlib binding.
//
// The X11 presenter opens its *own* connection instead of borrowing winit's:
// winit's X11 backend is XCB-based, and EGL's X11 platform requires an Xlib
// `Display*`. The drawable itself is a server-side XID, so a second connection
// referring to the same window is valid — unlike the Wayland case, where the
// surface is tied to one connection and must never be duplicated.
#[link(name = "X11")]
extern "C" {
    pub fn XOpenDisplay(name: *const c_char) -> X11Display;
    pub fn XCloseDisplay(display: X11Display) -> c_int;
    pub fn XSync(display: X11Display, discard: c_int) -> c_int;
}

/// Human-readable EGL error name for diagnostics.
pub fn error_name(code: EGLint) -> &'static str {
    match code {
        0x3000 => "EGL_SUCCESS",
        0x3001 => "EGL_NOT_INITIALIZED",
        0x3002 => "EGL_BAD_ACCESS",
        0x3003 => "EGL_BAD_ALLOC",
        0x3004 => "EGL_BAD_ATTRIBUTE",
        0x3005 => "EGL_BAD_CONFIG",
        0x3006 => "EGL_BAD_CONTEXT",
        0x3007 => "EGL_BAD_CURRENT_SURFACE",
        0x3008 => "EGL_BAD_DISPLAY",
        0x3009 => "EGL_BAD_MATCH",
        0x300A => "EGL_BAD_NATIVE_PIXMAP",
        0x300B => "EGL_BAD_NATIVE_WINDOW",
        0x300C => "EGL_BAD_PARAMETER",
        0x300D => "EGL_BAD_SURFACE",
        0x300E => "EGL_CONTEXT_LOST",
        _ => "EGL_UNKNOWN",
    }
}
