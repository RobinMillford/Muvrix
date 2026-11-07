//! Native Link Smoke Test for libmpv-sys
//!
//! This integration test verifies that the native linker successfully resolves C API
//! symbols against the target platform's import library or shared object (`mpv.lib` on
//! Windows MSVC, `libmpv.so.2` on Linux, `libmpv.dylib` on macOS), and that the OS
//! dynamic loader successfully loads the runtime library and executes C entrypoints.

use libmpv_sys::{
    mpv_client_api_version, mpv_create, mpv_error_string, mpv_make_version, mpv_terminate_destroy,
};
use std::ffi::CStr;

#[test]
fn test_native_symbol_link_and_invocation() {
    // 1. Symbol resolution and invocation: mpv_client_api_version
    let version = unsafe { mpv_client_api_version() };
    let major = version >> 16;
    let minor = version & 0xFFFF;
    println!("Linked native libmpv API version: {major}.{minor} (raw: {version:#x})");

    assert!(
        version >= mpv_make_version(2, 0),
        "Linked libmpv version ({major}.{minor}) is older than required 2.0"
    );

    // 2. String return symbol resolution: mpv_error_string
    let err_str = unsafe {
        let ptr = mpv_error_string(0); // 0 = MPV_ERROR_SUCCESS
        assert!(!ptr.is_null(), "mpv_error_string returned null");
        CStr::from_ptr(ptr).to_string_lossy().into_owned()
    };
    assert_eq!(err_str, "success", "Expected 'success' for error code 0");

    // 3. Lifecycle creation and teardown: mpv_create and mpv_terminate_destroy
    unsafe {
        let handle = mpv_create();
        assert!(!handle.is_null(), "mpv_create returned null handle");
        mpv_terminate_destroy(handle);
    }
}
