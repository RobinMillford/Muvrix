//! Integration tests verifying all 10 Muvrix GUI Playback Flows (A through J).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use muvrix_core::path::{resolve_media_path, resolve_preset, MediaPathError};
use muvrix_player::{MpvHandle, PlaybackController, PlaybackState};

fn create_test_controller() -> PlaybackController {
    let handle = MpvHandle::new().expect("Failed to create MpvHandle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.set_option_string("idle", "yes").unwrap();
    handle.initialize().unwrap();
    PlaybackController::new(Arc::new(handle)).unwrap()
}

fn wait_for_state(
    controller: &mut PlaybackController,
    predicate: impl Fn(&PlaybackState) -> bool,
    timeout: Duration,
) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        controller.poll_events();
        if predicate(controller.state()) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    controller.poll_events();
    predicate(controller.state())
}

/// FLOW A: Click H.264 preset
#[test]
#[ignore = "Requires extended media corpus in tests/MuvrixTestMedia/ for preset buttons; run with: cargo test -p muvrix-ui --test gui_flows_test -- --ignored"]
fn test_flow_a_h264_preset() {
    let mut controller = create_test_controller();
    let candidates = [
        "tests/MuvrixTestMedia/phase-0.4/A-core/h264-aac.mp4",
        "tests/MuvrixTestMedia/baseline/h264-aac.mp4",
    ];
    let resolved = resolve_preset(&candidates).expect("Flow A: H.264 preset must resolve");
    assert!(
        resolved.is_file(),
        "Resolved path must exist: {:?}",
        resolved
    );

    controller.load_file(resolved.to_str().unwrap()).unwrap();
    assert_eq!(*controller.state(), PlaybackState::Loading);

    let playing = wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Playing),
        Duration::from_secs(3),
    );
    assert!(
        playing,
        "Flow A: Expected Playing state, got {:?}",
        controller.state()
    );
    assert_eq!(controller.video_format(), "h264");
    assert!(controller.duration() > 0.0);
    let (w, h) = controller.video_dimensions();
    assert!(w > 0 && h > 0, "Video dimensions must be positive: {w}x{h}");
}

/// FLOW B: Click HEVC preset
#[test]
#[ignore = "Requires extended media corpus in tests/MuvrixTestMedia/ for preset buttons; run with: cargo test -p muvrix-ui --test gui_flows_test -- --ignored"]
fn test_flow_b_hevc_preset() {
    let mut controller = create_test_controller();
    let candidates = [
        "tests/MuvrixTestMedia/phase-0.4/A-core/hevc-main-aac.mp4",
        "tests/MuvrixTestMedia/codecs/hevc-aac.mp4",
    ];
    let resolved = resolve_preset(&candidates).expect("Flow B: HEVC preset must resolve");
    assert!(resolved.is_file());

    controller.load_file(resolved.to_str().unwrap()).unwrap();
    let playing = wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Playing),
        Duration::from_secs(3),
    );
    assert!(
        playing,
        "Flow B: Expected Playing state, got {:?}",
        controller.state()
    );
    assert_eq!(controller.video_format(), "hevc");
    assert!(controller.duration() > 0.0);
}

/// FLOW C: Click VP9 preset
#[test]
#[ignore = "Requires extended media corpus in tests/MuvrixTestMedia/ for preset buttons; run with: cargo test -p muvrix-ui --test gui_flows_test -- --ignored"]
fn test_flow_c_vp9_preset() {
    let mut controller = create_test_controller();
    let candidates = [
        "tests/MuvrixTestMedia/phase-0.4/A-core/vp9-opus.webm",
        "tests/MuvrixTestMedia/codecs/vp9-opus.webm",
    ];
    let resolved = resolve_preset(&candidates).expect("Flow C: VP9 preset must resolve");
    assert!(resolved.is_file());

    controller.load_file(resolved.to_str().unwrap()).unwrap();
    let playing = wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Playing),
        Duration::from_secs(3),
    );
    assert!(
        playing,
        "Flow C: Expected Playing state, got {:?}",
        controller.state()
    );
    assert_eq!(controller.video_format(), "vp9");
    assert!(controller.duration() > 0.0);
}

/// FLOW D: Click AV1 1080p preset
#[test]
#[ignore = "Requires extended media corpus in tests/MuvrixTestMedia/ for preset buttons; run with: cargo test -p muvrix-ui --test gui_flows_test -- --ignored"]
fn test_flow_d_av1_1080p_preset() {
    let mut controller = create_test_controller();
    let candidates = [
        "tests/MuvrixTestMedia/phase-0.4/generated/av1-1080p-svtav1-synthetic.webm",
        "tests/MuvrixTestMedia/codecs/av1-1080p.webm",
        "tests/MuvrixTestMedia/phase-0.4/A-core/av1-4k-cityhall.webm",
    ];
    let resolved = resolve_preset(&candidates).expect("Flow D: AV1 1080p preset must resolve");
    assert!(resolved.is_file());

    controller.load_file(resolved.to_str().unwrap()).unwrap();
    let playing = wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Playing),
        Duration::from_secs(3),
    );
    assert!(
        playing,
        "Flow D: Expected Playing state, got {:?}",
        controller.state()
    );
    assert_eq!(controller.video_format(), "av1");
    assert!(controller.duration() > 0.0);
}

/// FLOW E: Open File and select an existing MP4 manually
#[test]
fn test_flow_e_open_file_manual_selection() {
    let mut controller = create_test_controller();
    // Simulate path returned from native file picker rfd::FileDialog
    let manual_selection =
        resolve_media_path("tests/fixtures/test_video.mp4").expect("Flow E: Fixture exists");
    let canonical = manual_selection.canonicalize().unwrap();

    controller.load_file(canonical.to_str().unwrap()).unwrap();
    let playing = wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Playing),
        Duration::from_secs(3),
    );
    assert!(playing, "Flow E: Expected Playing state for picked file");

    let actual_str = controller
        .current_file()
        .expect("Flow E: Current file should be set on controller");
    let actual_path = PathBuf::from(actual_str);
    assert!(
        actual_path.is_file(),
        "Flow E: Current file path must point to an existing file"
    );

    let actual_canonical = actual_path
        .canonicalize()
        .expect("Flow E: Failed to canonicalize actual path");
    let expected_canonical = manual_selection
        .canonicalize()
        .expect("Flow E: Failed to canonicalize expected fixture path");
    assert_eq!(actual_canonical, expected_canonical);
}

/// FLOW F: Enter an absolute media path and click Load
#[test]
fn test_flow_f_absolute_path_load() {
    let mut controller = create_test_controller();
    let abs_path = resolve_media_path("tests/fixtures/test_video.mp4")
        .unwrap()
        .canonicalize()
        .unwrap();
    assert!(abs_path.is_absolute());

    let resolved = resolve_media_path(abs_path.to_str().unwrap()).unwrap();
    assert_eq!(resolved, abs_path);

    controller.load_file(resolved.to_str().unwrap()).unwrap();
    let playing = wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Playing),
        Duration::from_secs(3),
    );
    assert!(playing, "Flow F: Expected Playing state for absolute path");
    assert_eq!(controller.video_format(), "h264");
}

/// FLOW G: Load a file whose name contains spaces
#[test]
fn test_flow_g_path_with_spaces() {
    let mut controller = create_test_controller();
    let spaced_file = "tests/fixtures/path-resolution/test video with spaces.mp4";
    let resolved = resolve_media_path(spaced_file).expect("Flow G: Spaced file must resolve");
    assert!(resolved.is_file());

    controller.load_file(resolved.to_str().unwrap()).unwrap();
    let playing = wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Playing),
        Duration::from_secs(3),
    );
    assert!(
        playing,
        "Flow G: Expected Playing state for file with spaces"
    );
    assert!(!controller.tracks().is_empty(), "Tracks should be detected");
}

/// FLOW H: Attempt a nonexistent file
#[test]
fn test_flow_h_nonexistent_file() {
    let mut controller = create_test_controller();
    let missing_rel = "non_existent_folder_xyz/fake_movie_123.mp4";

    // 1. Path resolver detects failure and returns candidate locations
    let res = resolve_media_path(missing_rel);
    assert!(matches!(res, Err(MediaPathError::FileNotFound { .. })));

    // 2. If path is directly passed to controller, it catches error with diagnostics
    controller.load_file(missing_rel).unwrap();
    let reached_error = wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Error(_)),
        Duration::from_secs(3),
    );
    assert!(
        reached_error,
        "Flow H: Expected Error state, got {:?}",
        controller.state()
    );

    if let PlaybackState::Error(err_msg) = controller.state() {
        assert!(
            err_msg.contains("fake_movie_123") || err_msg.contains("open/demux"),
            "Error message must contain path and diagnostic details: {err_msg}"
        );
    }
}

/// FLOW I: Attempt a deliberately corrupted file
#[test]
fn test_flow_i_deliberately_corrupted_file() {
    let mut controller = create_test_controller();
    let corrupted_path = resolve_media_path("tests/fixtures/corrupted.mp4").unwrap();

    controller
        .load_file(corrupted_path.to_str().unwrap())
        .unwrap();
    let resolved = wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Error(_) | PlaybackState::Ended),
        Duration::from_secs(3),
    );
    assert!(
        resolved,
        "Flow I: Corrupted file must resolve gracefully without hang"
    );
}

/// FLOW J: Load another valid file after a failed load
#[test]
fn test_flow_j_recovery_after_failure() {
    let mut controller = create_test_controller();

    // 1. Induce failure with missing file
    controller
        .load_file("/tmp/missing_file_flow_j.mp4")
        .unwrap();
    let error_state = wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Error(_)),
        Duration::from_secs(3),
    );
    assert!(error_state, "Must enter error state first");

    // 2. Immediately load valid file
    let valid_path = resolve_media_path("tests/fixtures/test_video.mp4").unwrap();
    controller.load_file(valid_path.to_str().unwrap()).unwrap();
    assert_eq!(
        *controller.state(),
        PlaybackState::Loading,
        "State must immediately transition to Loading"
    );
    assert!(
        controller.last_error().is_none(),
        "Previous error must be cleared"
    );

    // 3. Confirm playback succeeds
    let recovered = wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Playing),
        Duration::from_secs(3),
    );
    assert!(
        recovered,
        "Flow J: Player must recover and reach Playing state"
    );
    assert_eq!(controller.video_format(), "h264");
    assert!(controller.duration() > 0.0);
}
