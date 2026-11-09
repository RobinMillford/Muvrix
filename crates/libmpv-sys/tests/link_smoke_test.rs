//! Native Link Smoke Test and Strict ABI Baseline Verification for libmpv-sys
//!
//! Verifies:
//! 1. Native linker successfully resolves C API symbols against the target import lib / shared object.
//! 2. Strict Phase 1.4 Client API baseline: 0x00020006 (Client API 2.6).
//! 3. Inspection and logging of loaded library path, engine version, and architecture.
//! 4. Basic handle lifecycle creation and destruction.

use libmpv_sys::{
    mpv_client_api_version, mpv_create, mpv_error_string, mpv_free, mpv_get_property_string,
    mpv_initialize, mpv_make_version, mpv_terminate_destroy,
};
use std::ffi::CStr;

#[test]
fn test_native_symbol_link_and_invocation() {
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;
    println!("=== Muvrix libmpv Engine ABI Verification ===");
    println!("Target OS:           {os}");
    println!("Target Architecture: {arch}");

    // 1. Symbol resolution and invocation: mpv_client_api_version
    let version = unsafe { mpv_client_api_version() };
    let major = version >> 16;
    let minor = version & 0xFFFF;
    println!("Runtime Client API:  {major}.{minor} (raw: {version:#010x})");

    // 2. Report actual loaded library path
    #[cfg(unix)]
    unsafe {
        let mut dlinfo: libc::Dl_info = std::mem::zeroed();
        if libc::dladdr(mpv_client_api_version as *const libc::c_void, &mut dlinfo) != 0
            && !dlinfo.dli_fname.is_null()
        {
            let path = CStr::from_ptr(dlinfo.dli_fname).to_string_lossy();
            println!("Loaded Library Path: {path}");
        }
    }

    #[cfg(windows)]
    unsafe {
        extern "system" {
            fn GetModuleHandleA(lpModuleName: *const std::os::raw::c_char)
                -> *mut std::ffi::c_void;
            fn GetModuleFileNameA(
                hModule: *mut std::ffi::c_void,
                lpFilename: *mut std::os::raw::c_char,
                nSize: u32,
            ) -> u32;
        }
        let h_mod = GetModuleHandleA(b"libmpv-2.dll\0".as_ptr() as *const _);
        if !h_mod.is_null() {
            let mut buf = [0u8; 1024];
            let len = GetModuleFileNameA(h_mod, buf.as_mut_ptr() as *mut _, buf.len() as u32);
            if len > 0 {
                let path = std::str::from_utf8_lossy(&buf[..len as usize]);
                println!("Loaded DLL Path:     {path}");
            }
        }
    }

    // 3. String return symbol resolution: mpv_error_string
    let err_str = unsafe {
        let ptr = mpv_error_string(0); // 0 = MPV_ERROR_SUCCESS
        assert!(!ptr.is_null(), "mpv_error_string returned null");
        CStr::from_ptr(ptr).to_string_lossy().into_owned()
    };
    assert_eq!(err_str, "success", "Expected 'success' for error code 0");

    // 4. Lifecycle creation, engine version inspection, and teardown
    unsafe {
        let handle = mpv_create();
        assert!(!handle.is_null(), "mpv_create returned null handle");
        assert_eq!(mpv_initialize(handle), 0, "mpv_initialize failed");

        let ver_prop = std::ffi::CString::new("mpv-version").unwrap();
        let ver_ptr = mpv_get_property_string(handle, ver_prop.as_ptr());
        assert!(
            !ver_ptr.is_null(),
            "mpv-version property unavailable; engine is not a libmpv >= 0.42 build"
        );

        let mpv_ver = CStr::from_ptr(ver_ptr).to_string_lossy().into_owned();
        println!("Engine Version:      {mpv_ver}");
        assert!(
            mpv_ver.starts_with("mpv v0.42.0"),
            "Engine version drift: expected mpv v0.42.0 (post-release describe suffix allowed), got: {mpv_ver}"
        );
        mpv_free(ver_ptr as *mut _);

        mpv_terminate_destroy(handle);
    }

    // 5. Strict Phase 1.4 ABI assertion: Client API 2.6 (0x00020006)
    let expected_version = mpv_make_version(2, 6);
    assert_eq!(
        version, expected_version,
        "Strict Phase 1.4 ABI assertion failed: expected Client API 2.6 (0x00020006), got {major}.{minor} ({version:#010x})"
    );
    println!("ABI Baseline Check:  PASSED (Strict Client API 2.6 Pinned)");
}
