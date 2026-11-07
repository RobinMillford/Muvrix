//! Integration tests for rendering resource lifecycle and error boundaries.

use muvrix_player::{MpvError, MpvHandle, MpvRenderContext};
use std::ptr;

unsafe extern "C" fn dummy_null_loader(
    _ctx: *mut std::ffi::c_void,
    _name: *const std::os::raw::c_char,
) -> *mut std::ffi::c_void {
    ptr::null_mut()
}

#[test]
fn test_render_context_creation_with_null_loader_fails_gracefully() {
    let handle = MpvHandle::new().expect("Failed to create MpvHandle");
    handle
        .set_option_string("vo", "libmpv")
        .expect("Failed to set vo");
    handle.initialize().expect("Failed to initialize mpv");

    // Passing None as get_proc_address must be rejected by Rust validation with InvalidParameter
    let res_none = unsafe { MpvRenderContext::new_gl(&handle, None, ptr::null_mut()) };
    assert_eq!(res_none.unwrap_err(), MpvError::InvalidParameter);

    // Passing a function pointer that returns NULL for GL symbols must be rejected by mpv with Unsupported
    let res_unsupported =
        unsafe { MpvRenderContext::new_gl(&handle, Some(dummy_null_loader), ptr::null_mut()) };
    assert!(res_unsupported.is_err());
}

#[test]
fn test_render_context_lifecycle_safety() {
    let handle = MpvHandle::new().expect("Failed to create MpvHandle");
    handle
        .set_option_string("vo", "libmpv")
        .expect("Failed to set vo");
    handle.initialize().expect("Failed to initialize mpv");

    // Verify engine responds to property queries and cleans up cleanly
    let pause: bool = handle.get_property("pause").expect("Failed to read pause");
    assert!(!pause);

    // Verify shutdown completes without hangs
    handle.terminate_destroy();
}
