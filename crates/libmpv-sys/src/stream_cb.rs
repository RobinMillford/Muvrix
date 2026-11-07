//! Raw FFI declarations matching `mpv/stream_cb.h`.

#![allow(non_camel_case_types, non_upper_case_globals, non_snake_case)]

use crate::client::mpv_handle;
use std::os::raw::{c_char, c_int, c_void};

pub type mpv_stream_cb_read_fn =
    Option<unsafe extern "C" fn(cookie: *mut c_void, buf: *mut c_char, nbytes: u64) -> i64>;
pub type mpv_stream_cb_seek_fn =
    Option<unsafe extern "C" fn(cookie: *mut c_void, offset: i64) -> i64>;
pub type mpv_stream_cb_size_fn = Option<unsafe extern "C" fn(cookie: *mut c_void) -> i64>;
pub type mpv_stream_cb_close_fn = Option<unsafe extern "C" fn(cookie: *mut c_void)>;
pub type mpv_stream_cb_cancel_fn = Option<unsafe extern "C" fn(cookie: *mut c_void)>;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct mpv_stream_cb_info {
    pub cookie: *mut c_void,
    pub read_fn: mpv_stream_cb_read_fn,
    pub seek_fn: mpv_stream_cb_seek_fn,
    pub size_fn: mpv_stream_cb_size_fn,
    pub close_fn: mpv_stream_cb_close_fn,
    pub cancel_fn: mpv_stream_cb_cancel_fn,
}

pub type mpv_stream_cb_open_ro_fn = Option<
    unsafe extern "C" fn(
        user_data: *mut c_void,
        uri: *mut c_char,
        info: *mut mpv_stream_cb_info,
    ) -> c_int,
>;

extern "C" {
    pub fn mpv_stream_cb_add_ro(
        ctx: *mut mpv_handle,
        protocol: *const c_char,
        user_data: *mut c_void,
        open_fn: mpv_stream_cb_open_ro_fn,
    ) -> c_int;
}
