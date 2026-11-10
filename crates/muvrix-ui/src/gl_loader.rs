//! OpenGL function pointer loader bridging host context to libmpv.

use std::ffi::{c_void, CStr};
use std::os::raw::c_char;

/// C-compatible trampoline for `mpv_opengl_init_params.get_proc_address`.
///
/// Resolves function pointers first through the host GUI loader (`get_proc_address` from `eframe`),
/// and falls back to dynamic symbol lookup (`dlsym` via `RTLD_DEFAULT`) on Linux.
///
/// # Safety
///
/// `name` must be a valid null-terminated C string representing an OpenGL symbol.
/// `ctx`, if non-null, must point to a valid `&dyn Fn(&CStr) -> *const c_void`.
pub unsafe extern "C" fn gl_get_proc_address_trampoline(
    ctx: *mut c_void,
    name: *const c_char,
) -> *mut c_void {
    if name.is_null() {
        return std::ptr::null_mut();
    }

    let cstr = CStr::from_ptr(name);

    if !ctx.is_null() {
        let loader = &*(ctx as *const &dyn Fn(&CStr) -> *const c_void);
        let ptr = loader(cstr);
        if !ptr.is_null() {
            return ptr as *mut c_void;
        }
    }

    // Dynamic library symbol lookup fallback on Unix platforms
    #[cfg(unix)]
    {
        let sym = libc::dlsym(libc::RTLD_DEFAULT, name);
        if !sym.is_null() {
            return sym;
        }
    }

    // Windows opengl32.dll symbol lookup fallback for core GL 1.1 symbols
    #[cfg(windows)]
    {
        extern "system" {
            fn GetModuleHandleA(lpModuleName: *const c_char) -> *mut c_void;
            fn GetProcAddress(hModule: *mut c_void, lpProcName: *const c_char) -> *mut c_void;
        }
        let h_gl = GetModuleHandleA(c"opengl32.dll".as_ptr());
        if !h_gl.is_null() {
            let proc = GetProcAddress(h_gl, name);
            if !proc.is_null() {
                return proc;
            }
        }
    }

    std::ptr::null_mut()
}
