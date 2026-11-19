//! Raw FFI declarations matching `mpv/client.h`.

#![allow(non_camel_case_types, non_upper_case_globals, non_snake_case)]

use std::os::raw::{c_char, c_int, c_ulong, c_void};

pub const fn mpv_make_version(major: u32, minor: u32) -> c_ulong {
    ((major as c_ulong) << 16) | (minor as c_ulong)
}

#[repr(C)]
pub struct mpv_handle {
    _unused: [u8; 0],
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum mpv_error {
    MPV_ERROR_SUCCESS = 0,
    MPV_ERROR_EVENT_QUEUE_FULL = -1,
    MPV_ERROR_NOMEM = -2,
    MPV_ERROR_UNINITIALIZED = -3,
    MPV_ERROR_INVALID_PARAMETER = -4,
    MPV_ERROR_OPTION_NOT_FOUND = -5,
    MPV_ERROR_OPTION_FORMAT = -6,
    MPV_ERROR_OPTION_ERROR = -7,
    MPV_ERROR_PROPERTY_NOT_FOUND = -8,
    MPV_ERROR_PROPERTY_FORMAT = -9,
    MPV_ERROR_PROPERTY_UNAVAILABLE = -10,
    MPV_ERROR_PROPERTY_ERROR = -11,
    MPV_ERROR_COMMAND = -12,
    MPV_ERROR_LOADING_FAILED = -13,
    MPV_ERROR_AO_INIT_FAILED = -14,
    MPV_ERROR_VO_INIT_FAILED = -15,
    MPV_ERROR_NOTHING_TO_PLAY = -16,
    MPV_ERROR_UNKNOWN_FORMAT = -17,
    MPV_ERROR_UNSUPPORTED = -18,
    MPV_ERROR_NOT_IMPLEMENTED = -19,
    MPV_ERROR_GENERIC = -20,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum mpv_format {
    MPV_FORMAT_NONE = 0,
    MPV_FORMAT_STRING = 1,
    MPV_FORMAT_OSD_STRING = 2,
    MPV_FORMAT_FLAG = 3,
    MPV_FORMAT_INT64 = 4,
    MPV_FORMAT_DOUBLE = 5,
    MPV_FORMAT_NODE = 6,
    MPV_FORMAT_NODE_ARRAY = 7,
    MPV_FORMAT_NODE_MAP = 8,
    MPV_FORMAT_BYTE_ARRAY = 9,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub union mpv_node_u {
    pub string: *mut c_char,
    pub flag: c_int,
    pub int64: i64,
    pub double_: f64,
    pub list: *mut mpv_node_list,
    pub ba: *mut mpv_byte_array,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct mpv_node {
    pub u: mpv_node_u,
    pub format: mpv_format,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct mpv_node_list {
    pub num: c_int,
    pub values: *mut mpv_node,
    pub keys: *mut *mut c_char,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct mpv_byte_array {
    pub data: *mut c_void,
    pub size: usize,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum mpv_event_id {
    MPV_EVENT_NONE = 0,
    MPV_EVENT_SHUTDOWN = 1,
    MPV_EVENT_LOG_MESSAGE = 2,
    MPV_EVENT_GET_PROPERTY_REPLY = 3,
    MPV_EVENT_SET_PROPERTY_REPLY = 4,
    MPV_EVENT_COMMAND_REPLY = 5,
    MPV_EVENT_START_FILE = 6,
    MPV_EVENT_END_FILE = 7,
    MPV_EVENT_FILE_LOADED = 8,
    MPV_EVENT_IDLE = 11,
    MPV_EVENT_TICK = 14,
    MPV_EVENT_CLIENT_MESSAGE = 16,
    MPV_EVENT_VIDEO_RECONFIG = 17,
    MPV_EVENT_AUDIO_RECONFIG = 18,
    MPV_EVENT_SEEK = 20,
    MPV_EVENT_PLAYBACK_RESTART = 21,
    MPV_EVENT_PROPERTY_CHANGE = 22,
    MPV_EVENT_QUEUE_OVERFLOW = 24,
    MPV_EVENT_HOOK = 25,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum mpv_log_level {
    MPV_LOG_LEVEL_NONE = 0,
    MPV_LOG_LEVEL_FATAL = 10,
    MPV_LOG_LEVEL_ERROR = 20,
    MPV_LOG_LEVEL_WARN = 30,
    MPV_LOG_LEVEL_INFO = 40,
    MPV_LOG_LEVEL_V = 50,
    MPV_LOG_LEVEL_DEBUG = 60,
    MPV_LOG_LEVEL_TRACE = 70,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum mpv_end_file_reason {
    MPV_END_FILE_REASON_EOF = 0,
    MPV_END_FILE_REASON_STOP = 2,
    MPV_END_FILE_REASON_QUIT = 3,
    MPV_END_FILE_REASON_ERROR = 4,
    MPV_END_FILE_REASON_REDIRECT = 5,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct mpv_event_property {
    pub name: *const c_char,
    pub format: mpv_format,
    pub data: *mut c_void,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct mpv_event_log_message {
    pub prefix: *const c_char,
    pub level: *const c_char,
    pub text: *const c_char,
    pub log_level: mpv_log_level,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct mpv_event_start_file {
    pub playlist_entry_id: i64,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct mpv_event_end_file {
    pub reason: mpv_end_file_reason,
    pub error: c_int,
    pub playlist_entry_id: i64,
    pub playlist_insert_id: i64,
    pub playlist_insert_num_entries: c_int,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct mpv_event_client_message {
    pub num_args: c_int,
    pub args: *mut *const c_char,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct mpv_event_hook {
    pub name: *const c_char,
    pub id: u64,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct mpv_event_command {
    pub result: mpv_node,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct mpv_event {
    pub event_id: mpv_event_id,
    pub error: c_int,
    pub reply_userdata: u64,
    pub data: *mut c_void,
}

extern "C" {
    pub fn mpv_client_api_version() -> c_ulong;
    pub fn mpv_error_string(error: c_int) -> *const c_char;
    pub fn mpv_free(data: *mut c_void);
    /// Subscribes (or unsubscribes, with a null/`"no"` level) this handle to
    /// the engine's log stream. The level is one of `no`, `fatal`, `error`,
    /// `warn`, `info`, `v`, `debug`, `trace`.
    pub fn mpv_request_log_messages(ctx: *mut mpv_handle, min_level: *const c_char) -> c_int;
    pub fn mpv_client_name(ctx: *mut mpv_handle) -> *const c_char;
    pub fn mpv_client_id(ctx: *mut mpv_handle) -> i64;
    pub fn mpv_create() -> *mut mpv_handle;
    pub fn mpv_initialize(ctx: *mut mpv_handle) -> c_int;
    pub fn mpv_destroy(ctx: *mut mpv_handle);
    pub fn mpv_terminate_destroy(ctx: *mut mpv_handle);
    pub fn mpv_create_client(ctx: *mut mpv_handle, name: *const c_char) -> *mut mpv_handle;
    pub fn mpv_load_config_file(ctx: *mut mpv_handle, filename: *const c_char) -> c_int;
    pub fn mpv_set_option(
        ctx: *mut mpv_handle,
        name: *const c_char,
        format: mpv_format,
        data: *mut c_void,
    ) -> c_int;
    pub fn mpv_set_option_string(
        ctx: *mut mpv_handle,
        name: *const c_char,
        data: *const c_char,
    ) -> c_int;
    pub fn mpv_command(ctx: *mut mpv_handle, args: *mut *const c_char) -> c_int;
    pub fn mpv_command_node(
        ctx: *mut mpv_handle,
        args: *mut mpv_node,
        result: *mut mpv_node,
    ) -> c_int;
    pub fn mpv_command_ret(
        ctx: *mut mpv_handle,
        args: *mut *const c_char,
        result: *mut mpv_node,
    ) -> c_int;
    pub fn mpv_command_string(ctx: *mut mpv_handle, args: *const c_char) -> c_int;
    pub fn mpv_command_async(
        ctx: *mut mpv_handle,
        reply_userdata: u64,
        args: *mut *const c_char,
    ) -> c_int;
    pub fn mpv_command_node_async(
        ctx: *mut mpv_handle,
        reply_userdata: u64,
        args: *mut mpv_node,
    ) -> c_int;
    pub fn mpv_set_property(
        ctx: *mut mpv_handle,
        name: *const c_char,
        format: mpv_format,
        data: *mut c_void,
    ) -> c_int;
    pub fn mpv_set_property_string(
        ctx: *mut mpv_handle,
        name: *const c_char,
        data: *const c_char,
    ) -> c_int;
    pub fn mpv_set_property_async(
        ctx: *mut mpv_handle,
        reply_userdata: u64,
        name: *const c_char,
        format: mpv_format,
        data: *mut c_void,
    ) -> c_int;
    pub fn mpv_get_property(
        ctx: *mut mpv_handle,
        name: *const c_char,
        format: mpv_format,
        data: *mut c_void,
    ) -> c_int;
    pub fn mpv_get_property_string(ctx: *mut mpv_handle, name: *const c_char) -> *mut c_char;
    pub fn mpv_get_property_osd_string(ctx: *mut mpv_handle, name: *const c_char) -> *mut c_char;
    pub fn mpv_get_property_async(
        ctx: *mut mpv_handle,
        reply_userdata: u64,
        name: *const c_char,
        format: mpv_format,
    ) -> c_int;
    pub fn mpv_observe_property(
        mpv: *mut mpv_handle,
        reply_userdata: u64,
        name: *const c_char,
        format: mpv_format,
    ) -> c_int;
    pub fn mpv_unobserve_property(mpv: *mut mpv_handle, registered_reply_userdata: u64) -> c_int;
    pub fn mpv_event_name(event: mpv_event_id) -> *const c_char;
    pub fn mpv_free_node_contents(node: *mut mpv_node);
    pub fn mpv_set_wakeup_callback(
        ctx: *mut mpv_handle,
        cb: Option<unsafe extern "C" fn(d: *mut c_void)>,
        d: *mut c_void,
    );
    pub fn mpv_wait_event(ctx: *mut mpv_handle, timeout: f64) -> *mut mpv_event;
    pub fn mpv_wakeup(ctx: *mut mpv_handle);
    /// Monotonic engine clock in nanoseconds (`mpv/client.h`).
    pub fn mpv_get_time_ns(ctx: *mut mpv_handle) -> i64;
    /// Monotonic engine clock in microseconds (`mpv/client.h`).
    pub fn mpv_get_time_us(ctx: *mut mpv_handle) -> i64;
}
