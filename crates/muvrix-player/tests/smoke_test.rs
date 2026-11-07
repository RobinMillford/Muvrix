//! Cross-platform native smoke tests for Phase 0.5 validation.
//!
//! Verifies basic libmpv runtime linkage, initialization, event observation,
//! rendering boundary checks, and clean shutdown using minimal test fixtures.

use std::ptr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use muvrix_core::path::resolve_media_path;
use muvrix_player::{MpvError, MpvHandle, MpvRenderContext, PlaybackController, PlaybackState};

/// 1. Smoke test: Loading libmpv and verifying API version
#[test]
fn test_smoke_load_libmpv_and_api_version() {
    let version = MpvHandle::client_api_version();
    log::info!("libmpv client API version: {version}");
    assert!(
        version.major >= 2,
        "libmpv major API version must be >= 2, got: {}",
        version.major
    );
    assert!(
        version.is_gl_compatible(),
        "libmpv must satisfy OpenGL render API compatibility (>= 2.0)"
    );
}

/// 2. Smoke test: Creating, configuring, initializing, and releasing an mpv handle
#[test]
fn test_smoke_create_and_initialize_handle() {
    let handle = MpvHandle::new().expect("Smoke: Failed to create mpv handle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.set_option_string("idle", "yes").unwrap();
    handle
        .initialize()
        .expect("Smoke: Failed to initialize mpv");

    let pause: bool = handle.get_property("pause").expect("Read pause property");
    assert!(!pause, "Default pause state should be false");

    let idle: bool = handle.get_property("idle-active").expect("Read idle");
    assert!(idle, "Player should be idle initially");

    handle.terminate_destroy();
}

/// 3. Smoke test: OpenGL render context creation error boundaries
#[test]
fn test_smoke_render_context_creation_boundaries() {
    let handle = MpvHandle::new().expect("Failed to create MpvHandle");
    handle.set_option_string("vo", "libmpv").unwrap();
    handle.initialize().expect("Failed to initialize mpv");

    // Passing None must fail with InvalidParameter
    let res_none = unsafe { MpvRenderContext::new_gl(&handle, None, ptr::null_mut()) };
    assert_eq!(res_none.unwrap_err(), MpvError::InvalidParameter);

    // Passing dummy loader that returns null must be rejected cleanly by mpv (Unsupported)
    unsafe extern "C" fn dummy_null(
        _ctx: *mut std::ffi::c_void,
        _name: *const std::os::raw::c_char,
    ) -> *mut std::ffi::c_void {
        ptr::null_mut()
    }
    let res_null = unsafe { MpvRenderContext::new_gl(&handle, Some(dummy_null), ptr::null_mut()) };
    assert!(res_null.is_err());

    handle.terminate_destroy();
}

/// 4. Smoke test: Loading a short H.264 sample and observing FileLoaded
#[test]
fn test_smoke_load_short_h264_and_observe_file_loaded() {
    let handle = MpvHandle::new().expect("Failed to create MpvHandle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.set_option_string("idle", "yes").unwrap();
    handle.initialize().expect("Failed to initialize mpv");

    let mut controller =
        PlaybackController::new(Arc::new(handle)).expect("Failed to create PlaybackController");

    let fixture = resolve_media_path("tests/fixtures/test_video.mp4")
        .expect("Failed to resolve tests/fixtures/test_video.mp4");
    assert!(fixture.is_file(), "Fixture must exist: {:?}", fixture);

    controller
        .load_file(fixture.to_str().unwrap())
        .expect("Failed to dispatch load_file");

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut loaded = false;

    while Instant::now() < deadline {
        controller.poll_events();
        if *controller.state() == PlaybackState::Playing
            || *controller.state() == PlaybackState::Paused
        {
            loaded = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    assert!(
        loaded,
        "File must reach Playing/Paused state within timeout. Final state: {:?}",
        controller.state()
    );

    // Duration should be positive
    let duration = controller.duration();
    assert!(
        duration > 0.0,
        "Video duration must be > 0.0, got: {duration}"
    );

    controller.stop().expect("Stop command dispatch");
    drop(controller);
}

/// 5. Smoke test: Clean resource creation, rapid playback cycles, and release
#[test]
fn test_smoke_clean_resource_release() {
    for _ in 0..3 {
        let handle = MpvHandle::new().expect("Create MpvHandle");
        handle.set_option_string("vo", "null").unwrap();
        handle.set_option_string("ao", "null").unwrap();
        handle.initialize().unwrap();

        let controller = PlaybackController::new(Arc::new(handle)).unwrap();
        assert_eq!(*controller.state(), PlaybackState::Idle);
        drop(controller);
    }
}
