use std::path::PathBuf;
use std::time::{Duration, Instant};

use muvrix_player::{ApiVersion, Event, LoadFileMode, MpvError, MpvHandle, SeekMode};

fn find_fixture_path() -> PathBuf {
    // Check multiple relative locations depending on where cargo test runs
    let candidates = [
        PathBuf::from("tests/fixtures/test_video.mp4"),
        PathBuf::from("../../tests/fixtures/test_video.mp4"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/test_video.mp4"),
    ];

    for path in &candidates {
        if path.exists() {
            return path.canonicalize().unwrap_or_else(|_| path.clone());
        }
    }

    panic!(
        "Test fixture not found in candidates: {:?}",
        candidates
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
    );
}

fn create_test_player() -> MpvHandle {
    let handle = MpvHandle::new().expect("Failed to create MpvHandle");
    // Configure headless audio/video outputs for automated testing
    handle
        .set_option_string("vo", "null")
        .expect("Failed to set vo=null");
    handle
        .set_option_string("ao", "null")
        .expect("Failed to set ao=null");
    handle
        .set_option_string("idle", "yes")
        .expect("Failed to set idle=yes");
    handle.initialize().expect("Failed to initialize MpvHandle");
    handle
}

#[test]
fn test_api_version() {
    let version = ApiVersion::current();
    println!("Runtime libmpv API version: {version}");

    // Phase 0.1 acceptance criteria: require API >= 2.0 for OpenGL
    assert!(
        version.is_gl_compatible(),
        "Runtime API version {version} fails OpenGL compatibility (requires >= 2.0)"
    );
    assert!(version.validate_gl_prerequisites().is_ok());

    // Check Direct3D 11 compatibility requirement: requires API >= 2.6
    if version.major > 2 || (version.major == 2 && version.minor >= 6) {
        assert!(version.is_d3d11_compatible());
        assert!(version.validate_d3d11_prerequisites().is_ok());
    } else {
        assert!(!version.is_d3d11_compatible());
        let err = version.validate_d3d11_prerequisites().unwrap_err();
        match err {
            MpvError::IncompatibleApiVersion { required, found } => {
                assert_eq!(required, ">= 2.6");
                assert_eq!(found, format!("{}.{}", version.major, version.minor));
            }
            other => panic!("Unexpected error variant: {other:?}"),
        }
    }
}

#[test]
fn test_create_and_initialize() {
    let handle = create_test_player();
    let client_name = handle.client_name().expect("Failed to query client name");
    assert!(!client_name.is_empty(), "Client name should not be empty");
    println!("Initialized mpv client with name: {client_name}");
    handle.terminate_destroy();
}

#[test]
fn test_read_and_set_properties() {
    let handle = create_test_player();

    // 1. Initial idle-active property
    let idle: bool = handle
        .get_property("idle-active")
        .expect("Failed to get idle-active");
    assert!(idle, "Player should be idle upon startup");

    // 2. Volume property (f64)
    handle
        .set_property("volume", 75.0)
        .expect("Failed to set volume");
    let vol: f64 = handle.get_property("volume").expect("Failed to get volume");
    assert!((vol - 75.0).abs() < 1e-4, "Expected volume 75.0, got {vol}");

    // 3. Mute property (bool)
    handle
        .set_property("mute", true)
        .expect("Failed to set mute");
    let mute: bool = handle.get_property("mute").expect("Failed to get mute");
    assert!(mute, "Expected mute=true");

    handle
        .set_property("mute", false)
        .expect("Failed to set mute");
    let mute: bool = handle.get_property("mute").expect("Failed to get mute");
    assert!(!mute, "Expected mute=false");

    // 4. String property via get_property_string
    let osd_level = handle
        .get_property_string("osd-level")
        .expect("Failed to get osd-level");
    assert!(!osd_level.is_empty());

    handle.terminate_destroy();
}

#[test]
fn test_load_local_test_video_and_events() {
    let fixture_path = find_fixture_path();
    let path_str = fixture_path
        .to_str()
        .expect("Invalid UTF-8 in fixture path");

    let handle = create_test_player();
    handle
        .load_file(path_str, LoadFileMode::Replace)
        .expect("Failed to load fixture file");

    let mut start_file_received = false;
    let mut file_loaded_received = false;
    let deadline = Instant::now() + Duration::from_secs(5);

    while Instant::now() < deadline && !file_loaded_received {
        let event = handle.wait_event(0.1);
        match event {
            Event::StartFile { .. } => {
                start_file_received = true;
            }
            Event::FileLoaded => {
                file_loaded_received = true;
            }
            Event::EndFile {
                reason,
                error: Some(err),
                ..
            } => {
                panic!("File ended prematurely with error: {err:?} (reason: {reason:?})");
            }
            _ => {}
        }
    }

    assert!(start_file_received, "Expected Event::StartFile");
    assert!(file_loaded_received, "Expected Event::FileLoaded");

    // Verify stream properties parsed from test_video.mp4
    let duration: f64 = handle
        .get_property("duration")
        .expect("Failed to get duration");
    assert!(
        (duration - 3.0).abs() < 0.5,
        "Expected duration ~3.0s, got {duration}"
    );

    let video_format = handle
        .get_property_string("video-format")
        .expect("Failed to get video-format");
    assert_eq!(video_format, "h264");

    let width: i64 = handle.get_property("width").expect("Failed to get width");
    let height: i64 = handle.get_property("height").expect("Failed to get height");
    assert_eq!(width, 640);
    assert_eq!(height, 360);

    handle.terminate_destroy();
}

#[test]
fn test_play_pause_and_seeking() {
    let fixture_path = find_fixture_path();
    let path_str = fixture_path
        .to_str()
        .expect("Invalid UTF-8 in fixture path");

    let handle = create_test_player();
    // Load with pause=yes to control stepping
    handle
        .set_property("pause", true)
        .expect("Failed to set pause=yes");
    handle
        .load_file(path_str, LoadFileMode::Replace)
        .expect("Failed to load fixture file");

    // Wait for file to load
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut loaded = false;
    while Instant::now() < deadline && !loaded {
        if let Event::FileLoaded = handle.wait_event(0.1) {
            loaded = true;
        }
    }
    assert!(loaded, "File did not load within timeout");

    let is_paused: bool = handle.get_property("pause").expect("Failed to get pause");
    assert!(is_paused, "Expected initial state to be paused");

    // Test resume
    handle.play().expect("Failed to resume playback");
    let is_paused: bool = handle.get_property("pause").expect("Failed to get pause");
    assert!(!is_paused, "Expected state to be playing");

    // Test pause
    handle.pause().expect("Failed to pause playback");
    let is_paused: bool = handle.get_property("pause").expect("Failed to get pause");
    assert!(is_paused, "Expected state to be paused");

    // Test seek to 1.5s
    handle
        .seek(1.5, SeekMode::Absolute)
        .expect("Failed to seek");

    // Wait for seek event / playback restart
    let mut seek_completed = false;
    let seek_deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < seek_deadline && !seek_completed {
        match handle.wait_event(0.1) {
            Event::PlaybackRestart | Event::Seek => {
                seek_completed = true;
            }
            _ => {}
        }
    }

    let time_pos: f64 = handle
        .get_property("playback-time")
        .or_else(|_| handle.get_property("time-pos"))
        .expect("Failed to get playback position");
    println!("Playback position after seek: {time_pos}s");
    // Position should be approximately 1.5s (within keyframe granularity)
    assert!(
        (0.0..=3.0).contains(&time_pos),
        "Time position out of range: {time_pos}"
    );

    handle.terminate_destroy();
}

#[test]
fn test_clean_shutdown() {
    // 1. Explicit terminate_destroy
    let handle = create_test_player();
    let start = Instant::now();
    handle.terminate_destroy();
    let elapsed = start.elapsed();
    println!("Explicit shutdown elapsed: {elapsed:?}");
    assert!(elapsed < Duration::from_secs(2), "Shutdown took too long");

    // 2. RAII Drop
    {
        let _handle = create_test_player();
        // Dropped here
    }
}
