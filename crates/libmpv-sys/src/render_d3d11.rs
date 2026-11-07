//! Raw FFI declarations matching `mpv/render_d3d11.h`.

#![allow(non_camel_case_types, non_upper_case_globals, non_snake_case)]

use std::os::raw::c_void;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct mpv_d3d11_init_params {
    pub device: *mut c_void, // ID3D11Device*
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct mpv_d3d11_fbo {
    pub tex: *mut c_void, // ID3D11Texture2D*
}
