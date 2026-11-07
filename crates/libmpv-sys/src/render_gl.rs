//! Raw FFI declarations matching `mpv/render_gl.h`.

#![allow(non_camel_case_types, non_upper_case_globals, non_snake_case)]

use std::os::raw::{c_char, c_int, c_void};

pub type mpv_opengl_get_proc_address_fn =
    Option<unsafe extern "C" fn(ctx: *mut c_void, name: *const c_char) -> *mut c_void>;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct mpv_opengl_init_params {
    pub get_proc_address: mpv_opengl_get_proc_address_fn,
    pub get_proc_address_ctx: *mut c_void,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct mpv_opengl_fbo {
    pub fbo: c_int,
    pub w: c_int,
    pub h: c_int,
    pub internal_format: c_int,
}
