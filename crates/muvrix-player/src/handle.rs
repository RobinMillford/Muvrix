//! Safe, RAII-managed wrapper around `mpv_handle`.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::MpvError;
use crate::event::Event;
use crate::property::{GetProperty, PropertyFormat, SetProperty};
use crate::version::ApiVersion;

/// Safe, RAII-managed owner of an mpv playback engine instance.
pub struct MpvHandle {
    ctx: *mut libmpv_sys::mpv_handle,
    is_destroyed: AtomicBool,
}

// mpv_handle instances are thread-safe for API operations per upstream client.h
unsafe impl Send for MpvHandle {}
unsafe impl Sync for MpvHandle {}

impl MpvHandle {
    /// Creates an uninitialized mpv instance (`mpv_create()`).
    pub fn new() -> Result<Self, MpvError> {
        let ctx = unsafe { libmpv_sys::mpv_create() };
        if ctx.is_null() {
            return Err(MpvError::NoMem);
        }
        Ok(Self {
            ctx,
            is_destroyed: AtomicBool::new(false),
        })
    }

    /// Queries the runtime libmpv API version (`mpv_client_api_version()`).
    pub fn client_api_version() -> ApiVersion {
        ApiVersion::current()
    }

    /// Returns the raw pointer to the underlying C `mpv_handle`.
    pub fn as_ptr(&self) -> *mut libmpv_sys::mpv_handle {
        self.ctx
    }

    /// Checks if the handle is active.
    fn check_active(&self) -> Result<*mut libmpv_sys::mpv_handle, MpvError> {
        if self.is_destroyed.load(Ordering::Acquire) || self.ctx.is_null() {
            Err(MpvError::NullHandle)
        } else {
            Ok(self.ctx)
        }
    }

    /// Initializes the mpv core (`mpv_initialize()`).
    ///
    /// Must be called after setting initial options and before issuing playback commands.
    pub fn initialize(&self) -> Result<(), MpvError> {
        let ctx = self.check_active()?;
        let res = unsafe { libmpv_sys::mpv_initialize(ctx) };
        MpvError::from_raw(res)
    }

    /// Sets an mpv option string (`mpv_set_option_string()`).
    pub fn set_option_string(&self, name: &str, value: &str) -> Result<(), MpvError> {
        let ctx = self.check_active()?;
        let c_name = CString::new(name).map_err(|e| MpvError::InvalidString(e.to_string()))?;
        let c_val = CString::new(value).map_err(|e| MpvError::InvalidString(e.to_string()))?;
        let res =
            unsafe { libmpv_sys::mpv_set_option_string(ctx, c_name.as_ptr(), c_val.as_ptr()) };
        MpvError::from_raw(res)
    }

    /// Executes an mpv command synchronously (`mpv_command()`).
    pub fn command(&self, args: &[&str]) -> Result<(), MpvError> {
        let ctx = self.check_active()?;
        let mut c_args: Vec<CString> = Vec::with_capacity(args.len());
        for &arg in args {
            c_args.push(CString::new(arg).map_err(|e| MpvError::InvalidString(e.to_string()))?);
        }

        let mut ptrs: Vec<*const c_char> = c_args.iter().map(|s| s.as_ptr()).collect();
        ptrs.push(std::ptr::null()); // NULL-terminated array

        let res = unsafe { libmpv_sys::mpv_command(ctx, ptrs.as_mut_ptr()) };
        MpvError::from_raw(res)
    }

    /// Issues an asynchronous mpv command (`mpv_command_async()`).
    pub fn command_async(&self, reply_userdata: u64, args: &[&str]) -> Result<(), MpvError> {
        let ctx = self.check_active()?;
        let mut c_args: Vec<CString> = Vec::with_capacity(args.len());
        for &arg in args {
            c_args.push(CString::new(arg).map_err(|e| MpvError::InvalidString(e.to_string()))?);
        }

        let mut ptrs: Vec<*const c_char> = c_args.iter().map(|s| s.as_ptr()).collect();
        ptrs.push(std::ptr::null()); // NULL-terminated array

        let res = unsafe { libmpv_sys::mpv_command_async(ctx, reply_userdata, ptrs.as_mut_ptr()) };
        MpvError::from_raw(res)
    }

    /// Retrieves a typed property value (`mpv_get_property()`).
    pub fn get_property<T: GetProperty>(&self, name: &str) -> Result<T, MpvError> {
        let ctx = self.check_active()?;
        let c_name = CString::new(name).map_err(|e| MpvError::InvalidString(e.to_string()))?;
        unsafe { T::get_from_handle(ctx, c_name.as_ptr()) }
    }

    /// Retrieves a property value as a raw string (`mpv_get_property_string()`).
    pub fn get_property_string(&self, name: &str) -> Result<String, MpvError> {
        let ctx = self.check_active()?;
        let c_name = CString::new(name).map_err(|e| MpvError::InvalidString(e.to_string()))?;
        unsafe {
            let ptr = libmpv_sys::mpv_get_property_string(ctx, c_name.as_ptr());
            if ptr.is_null() {
                return Err(MpvError::PropertyUnavailable);
            }
            let s = CStr::from_ptr(ptr).to_string_lossy().into_owned();
            libmpv_sys::mpv_free(ptr as *mut c_void);
            Ok(s)
        }
    }

    /// Retrieves an OSD-formatted property string (`mpv_get_property_osd_string()`).
    pub fn get_property_osd_string(&self, name: &str) -> Result<String, MpvError> {
        let ctx = self.check_active()?;
        let c_name = CString::new(name).map_err(|e| MpvError::InvalidString(e.to_string()))?;
        unsafe {
            let ptr = libmpv_sys::mpv_get_property_osd_string(ctx, c_name.as_ptr());
            if ptr.is_null() {
                return Err(MpvError::PropertyUnavailable);
            }
            let s = CStr::from_ptr(ptr).to_string_lossy().into_owned();
            libmpv_sys::mpv_free(ptr as *mut c_void);
            Ok(s)
        }
    }

    /// Sets a typed property value (`mpv_set_property()`).
    pub fn set_property<T: SetProperty>(&self, name: &str, value: T) -> Result<(), MpvError> {
        let ctx = self.check_active()?;
        let c_name = CString::new(name).map_err(|e| MpvError::InvalidString(e.to_string()))?;
        unsafe { value.set_to_handle(ctx, c_name.as_ptr()) }
    }

    /// Sets a property value as a string (`mpv_set_property_string()`).
    pub fn set_property_string(&self, name: &str, value: &str) -> Result<(), MpvError> {
        self.set_property(name, value)
    }

    /// Waits for an event from the mpv event queue (`mpv_wait_event()`).
    ///
    /// Pass `0.0` for non-blocking poll, or a positive timeout in seconds.
    pub fn wait_event(&self, timeout_secs: f64) -> Event {
        if let Ok(ctx) = self.check_active() {
            unsafe {
                let event_ptr = libmpv_sys::mpv_wait_event(ctx, timeout_secs);
                Event::from_raw(event_ptr)
            }
        } else {
            Event::Shutdown
        }
    }

    /// Registers a property change observation (`mpv_observe_property()`).
    pub fn observe_property(
        &self,
        reply_userdata: u64,
        name: &str,
        format: PropertyFormat,
    ) -> Result<(), MpvError> {
        let ctx = self.check_active()?;
        let c_name = CString::new(name).map_err(|e| MpvError::InvalidString(e.to_string()))?;
        let res = unsafe {
            libmpv_sys::mpv_observe_property(ctx, reply_userdata, c_name.as_ptr(), format.to_raw())
        };
        MpvError::from_raw(res)
    }

    /// Unregisters a property change observation (`mpv_unobserve_property()`).
    pub fn unobserve_property(&self, reply_userdata: u64) -> Result<(), MpvError> {
        let ctx = self.check_active()?;
        let res = unsafe { libmpv_sys::mpv_unobserve_property(ctx, reply_userdata) };
        MpvError::from_raw(res)
    }

    /// Wakes up the mpv event loop thread (`mpv_wakeup()`).
    pub fn wakeup(&self) {
        if let Ok(ctx) = self.check_active() {
            unsafe {
                libmpv_sys::mpv_wakeup(ctx);
            }
        }
    }

    /// Queries the client name assigned to this handle (`mpv_client_name()`).
    pub fn client_name(&self) -> Result<String, MpvError> {
        let ctx = self.check_active()?;
        unsafe {
            let ptr = libmpv_sys::mpv_client_name(ctx);
            if ptr.is_null() {
                Ok(String::new())
            } else {
                Ok(CStr::from_ptr(ptr).to_string_lossy().into_owned())
            }
        }
    }

    /// Performs clean, deterministic engine shutdown (`mpv_terminate_destroy()`).
    pub fn terminate_destroy(mut self) {
        self.destroy_internal();
    }

    fn destroy_internal(&mut self) {
        if !self.is_destroyed.swap(true, Ordering::AcqRel) && !self.ctx.is_null() {
            unsafe {
                libmpv_sys::mpv_terminate_destroy(self.ctx);
            }
            self.ctx = std::ptr::null_mut();
        }
    }
}

impl Drop for MpvHandle {
    fn drop(&mut self) {
        self.destroy_internal();
    }
}
