//! Strongly typed mpv property format traits and conversions.

use crate::error::MpvError;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum PropertyFormat {
    None,
    String,
    OsdString,
    Flag,
    Int64,
    Double,
    Node,
}

impl PropertyFormat {
    pub fn to_raw(self) -> libmpv_sys::mpv_format {
        match self {
            Self::None => libmpv_sys::mpv_format::MPV_FORMAT_NONE,
            Self::String => libmpv_sys::mpv_format::MPV_FORMAT_STRING,
            Self::OsdString => libmpv_sys::mpv_format::MPV_FORMAT_OSD_STRING,
            Self::Flag => libmpv_sys::mpv_format::MPV_FORMAT_FLAG,
            Self::Int64 => libmpv_sys::mpv_format::MPV_FORMAT_INT64,
            Self::Double => libmpv_sys::mpv_format::MPV_FORMAT_DOUBLE,
            Self::Node => libmpv_sys::mpv_format::MPV_FORMAT_NODE,
        }
    }

    pub fn from_raw(raw: libmpv_sys::mpv_format) -> Self {
        match raw {
            libmpv_sys::mpv_format::MPV_FORMAT_NONE => Self::None,
            libmpv_sys::mpv_format::MPV_FORMAT_STRING => Self::String,
            libmpv_sys::mpv_format::MPV_FORMAT_OSD_STRING => Self::OsdString,
            libmpv_sys::mpv_format::MPV_FORMAT_FLAG => Self::Flag,
            libmpv_sys::mpv_format::MPV_FORMAT_INT64 => Self::Int64,
            libmpv_sys::mpv_format::MPV_FORMAT_DOUBLE => Self::Double,
            libmpv_sys::mpv_format::MPV_FORMAT_NODE => Self::Node,
            _ => Self::None,
        }
    }
}

pub trait GetProperty: Sized {
    fn format() -> PropertyFormat;

    /// Retrieves the property value from a raw `mpv_handle`.
    ///
    /// # Safety
    /// `ctx` must be a valid, initialized `mpv_handle` and `c_name` must point to a valid
    /// null-terminated C string.
    unsafe fn get_from_handle(
        ctx: *mut libmpv_sys::mpv_handle,
        c_name: *const c_char,
    ) -> Result<Self, MpvError>;
}

impl GetProperty for bool {
    fn format() -> PropertyFormat {
        PropertyFormat::Flag
    }

    unsafe fn get_from_handle(
        ctx: *mut libmpv_sys::mpv_handle,
        c_name: *const c_char,
    ) -> Result<Self, MpvError> {
        let mut val: c_int = 0;
        let res = libmpv_sys::mpv_get_property(
            ctx,
            c_name,
            libmpv_sys::mpv_format::MPV_FORMAT_FLAG,
            &mut val as *mut _ as *mut c_void,
        );
        MpvError::from_raw(res)?;
        Ok(val != 0)
    }
}

impl GetProperty for i64 {
    fn format() -> PropertyFormat {
        PropertyFormat::Int64
    }

    unsafe fn get_from_handle(
        ctx: *mut libmpv_sys::mpv_handle,
        c_name: *const c_char,
    ) -> Result<Self, MpvError> {
        let mut val: i64 = 0;
        let res = libmpv_sys::mpv_get_property(
            ctx,
            c_name,
            libmpv_sys::mpv_format::MPV_FORMAT_INT64,
            &mut val as *mut _ as *mut c_void,
        );
        MpvError::from_raw(res)?;
        Ok(val)
    }
}

impl GetProperty for f64 {
    fn format() -> PropertyFormat {
        PropertyFormat::Double
    }

    unsafe fn get_from_handle(
        ctx: *mut libmpv_sys::mpv_handle,
        c_name: *const c_char,
    ) -> Result<Self, MpvError> {
        let mut val: f64 = 0.0;
        let res = libmpv_sys::mpv_get_property(
            ctx,
            c_name,
            libmpv_sys::mpv_format::MPV_FORMAT_DOUBLE,
            &mut val as *mut _ as *mut c_void,
        );
        MpvError::from_raw(res)?;
        Ok(val)
    }
}

impl GetProperty for String {
    fn format() -> PropertyFormat {
        PropertyFormat::String
    }

    unsafe fn get_from_handle(
        ctx: *mut libmpv_sys::mpv_handle,
        c_name: *const c_char,
    ) -> Result<Self, MpvError> {
        let ptr = libmpv_sys::mpv_get_property_string(ctx, c_name);
        if ptr.is_null() {
            return Err(MpvError::PropertyUnavailable);
        }
        let s = CStr::from_ptr(ptr).to_string_lossy().into_owned();
        libmpv_sys::mpv_free(ptr as *mut c_void);
        Ok(s)
    }
}

pub trait SetProperty {
    fn format(&self) -> PropertyFormat;

    /// Sets the property value on a raw `mpv_handle`.
    ///
    /// # Safety
    /// `ctx` must be a valid, initialized `mpv_handle` and `c_name` must point to a valid
    /// null-terminated C string.
    unsafe fn set_to_handle(
        &self,
        ctx: *mut libmpv_sys::mpv_handle,
        c_name: *const c_char,
    ) -> Result<(), MpvError>;
}

impl SetProperty for bool {
    fn format(&self) -> PropertyFormat {
        PropertyFormat::Flag
    }

    unsafe fn set_to_handle(
        &self,
        ctx: *mut libmpv_sys::mpv_handle,
        c_name: *const c_char,
    ) -> Result<(), MpvError> {
        let mut val: c_int = if *self { 1 } else { 0 };
        let res = libmpv_sys::mpv_set_property(
            ctx,
            c_name,
            libmpv_sys::mpv_format::MPV_FORMAT_FLAG,
            &mut val as *mut _ as *mut c_void,
        );
        MpvError::from_raw(res)
    }
}

impl SetProperty for i64 {
    fn format(&self) -> PropertyFormat {
        PropertyFormat::Int64
    }

    unsafe fn set_to_handle(
        &self,
        ctx: *mut libmpv_sys::mpv_handle,
        c_name: *const c_char,
    ) -> Result<(), MpvError> {
        let mut val = *self;
        let res = libmpv_sys::mpv_set_property(
            ctx,
            c_name,
            libmpv_sys::mpv_format::MPV_FORMAT_INT64,
            &mut val as *mut _ as *mut c_void,
        );
        MpvError::from_raw(res)
    }
}

impl SetProperty for f64 {
    fn format(&self) -> PropertyFormat {
        PropertyFormat::Double
    }

    unsafe fn set_to_handle(
        &self,
        ctx: *mut libmpv_sys::mpv_handle,
        c_name: *const c_char,
    ) -> Result<(), MpvError> {
        let mut val = *self;
        let res = libmpv_sys::mpv_set_property(
            ctx,
            c_name,
            libmpv_sys::mpv_format::MPV_FORMAT_DOUBLE,
            &mut val as *mut _ as *mut c_void,
        );
        MpvError::from_raw(res)
    }
}

impl SetProperty for &str {
    fn format(&self) -> PropertyFormat {
        PropertyFormat::String
    }

    unsafe fn set_to_handle(
        &self,
        ctx: *mut libmpv_sys::mpv_handle,
        c_name: *const c_char,
    ) -> Result<(), MpvError> {
        let c_val = CString::new(*self).map_err(|e| MpvError::InvalidString(e.to_string()))?;
        let res = libmpv_sys::mpv_set_property_string(ctx, c_name, c_val.as_ptr());
        MpvError::from_raw(res)
    }
}

impl SetProperty for String {
    fn format(&self) -> PropertyFormat {
        PropertyFormat::String
    }

    unsafe fn set_to_handle(
        &self,
        ctx: *mut libmpv_sys::mpv_handle,
        c_name: *const c_char,
    ) -> Result<(), MpvError> {
        self.as_str().set_to_handle(ctx, c_name)
    }
}
