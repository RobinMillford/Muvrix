//! Automated regression tests for GUI path resolution, preset discovery,
//! and controller error recovery.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use muvrix_core::path::{expand_tilde, resolve_media_path, resolve_preset, MediaPathError};
use muvrix_player::{MpvHandle, PlaybackController, PlaybackState};

fn create_headless_controller() -> PlaybackController {
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
        std::thread::sleep(Duration::from_millis(10));
    }
    controller.poll_events();
    predicate(controller.state())
}

#[test]
#[ignore = "Requires extended media corpus in tests/MuvrixTestMedia/ for preset buttons; run with: cargo test -p muvrix-ui --test gui_path_test -- --ignored"]
fn test_preset_path_resolution_all_buttons() {
    // 1. H.264
    let h264 = resolve_preset(&[
        "tests/MuvrixTestMedia/phase-0.4/A-core/h264-aac.mp4",
        "tests/MuvrixTestMedia/baseline/h264-aac.mp4",
    ]);
    assert!(h264.is_ok(), "H.264 preset failed to resolve: {:?}", h264);
    assert!(h264.unwrap().is_file());

    // 2. HEVC
    let hevc = resolve_preset(&[
        "tests/MuvrixTestMedia/phase-0.4/A-core/hevc-main-aac.mp4",
        "tests/MuvrixTestMedia/codecs/hevc-aac.mp4",
    ]);
    assert!(hevc.is_ok(), "HEVC preset failed to resolve: {:?}", hevc);
    assert!(hevc.unwrap().is_file());

    // 3. VP9
    let vp9 = resolve_preset(&[
        "tests/MuvrixTestMedia/phase-0.4/A-core/vp9-opus.webm",
        "tests/MuvrixTestMedia/codecs/vp9-opus.webm",
    ]);
    assert!(vp9.is_ok(), "VP9 preset failed to resolve: {:?}", vp9);
    assert!(vp9.unwrap().is_file());

    // 4. AV1 1080p
    let av1 = resolve_preset(&[
        "tests/MuvrixTestMedia/phase-0.4/generated/av1-1080p-svtav1-synthetic.webm",
        "tests/MuvrixTestMedia/codecs/av1-1080p.webm",
        "tests/MuvrixTestMedia/phase-0.4/A-core/av1-4k-cityhall.webm",
    ]);
    assert!(av1.is_ok(), "AV1 1080p preset failed to resolve: {:?}", av1);
    assert!(av1.unwrap().is_file());

    // 5. Multi-Audio MOV
    let multi_audio = resolve_preset(&[
        "tests/MuvrixTestMedia/phase-0.4/D-edge-cases/multiple-audio-streams.mov",
        "tests/MuvrixTestMedia/tracks/multiple-audio-streams.mov",
    ]);
    assert!(
        multi_audio.is_ok(),
        "Multi-Audio MOV preset failed to resolve: {:?}",
        multi_audio
    );
    assert!(multi_audio.unwrap().is_file());

    // 6. Video-Only
    let video_only = resolve_preset(&[
        "tests/MuvrixTestMedia/phase-0.4/D-edge-cases/video-only.mp4",
        "tests/MuvrixTestMedia/tracks/video-only.mp4",
    ]);
    assert!(
        video_only.is_ok(),
        "Video-Only preset failed to resolve: {:?}",
        video_only
    );
    assert!(video_only.unwrap().is_file());

    // 7. Audio-Only FLAC
    let flac = resolve_preset(&[
        "tests/MuvrixTestMedia/phase-0.4/C-audio/flac-24bit-96khz.flac",
        "tests/MuvrixTestMedia/audio/audio.flac",
    ]);
    assert!(
        flac.is_ok(),
        "Audio-Only FLAC preset failed to resolve: {:?}",
        flac
    );
    assert!(flac.unwrap().is_file());

    // 8. Multi-Sub MKV
    let multi_sub = resolve_preset(&[
        "tests/MuvrixTestMedia/phase-0.4/C-subtitles/embedded-srt-ass.mkv",
        "tests/MuvrixTestMedia/tracks/synthetic-multi-sub.mkv",
    ]);
    assert!(
        multi_sub.is_ok(),
        "Multi-Sub MKV preset failed to resolve: {:?}",
        multi_sub
    );
    assert!(multi_sub.unwrap().is_file());

    // 9. Colony 7.1 MKV (with local fallback)
    let colony = resolve_preset(&[
        "~/Videos/Colony.2026.1080p.BluRay.DUAL.DDP7.1.x264-PRiME.mkv",
        "tests/MuvrixTestMedia/phase-0.4/C-audio/TrueHD Sample 1 (Final Fantasy VII Advent Children).mkv",
    ]);
    assert!(
        colony.is_ok(),
        "Colony 7.1 preset (with TrueHD fallback) failed to resolve: {:?}",
        colony
    );
    assert!(colony.unwrap().is_file());
}

#[test]
fn test_media_path_resolution_spaces_and_tilde() {
    // 1. Path with spaces
    let space_rel = "tests/fixtures/path-resolution/test video with spaces.mp4";
    let space_res = resolve_media_path(space_rel);
    assert!(
        space_res.is_ok(),
        "Expected space-containing path to resolve: {:?}",
        space_res
    );
    assert!(space_res.unwrap().exists());

    // 2. Tilde expansion
    if let Some(home) = std::env::var_os("HOME") {
        let tilde_path = "~/Videos";
        let expanded = expand_tilde(tilde_path);
        assert_eq!(expanded, PathBuf::from(home).join("Videos"));
    }

    // 3. Absolute path resolution
    let h264_resolved = resolve_media_path("tests/fixtures/test_video.mp4").unwrap();
    let abs_str = h264_resolved.to_str().unwrap();
    let abs_res = resolve_media_path(abs_str);
    assert!(abs_res.is_ok());
    assert_eq!(abs_res.unwrap(), h264_resolved);

    // 4. Nonexistent file returns descriptive error
    let bad_path = "non_existent_folder_abc123/bad_video_xyz.mp4";
    let bad_res = resolve_media_path(bad_path);
    assert!(matches!(bad_res, Err(MediaPathError::FileNotFound { .. })));
    if let Err(MediaPathError::FileNotFound {
        attempted,
        candidates,
    }) = bad_res
    {
        assert_eq!(attempted, bad_path);
        assert!(
            !candidates.is_empty(),
            "Expected candidate paths to be logged"
        );
    }
}

#[test]
fn test_failed_load_followed_by_successful_load() {
    let mut controller = create_headless_controller();

    // 1. Attempt invalid file
    let bad_file = "/tmp/muvrix_invalid_nonexistent_test_file_404.mp4";
    controller.load_file(bad_file).unwrap();
    assert_eq!(*controller.state(), PlaybackState::Loading);

    let reached_error = wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Error(_)),
        Duration::from_secs(3),
    );
    assert!(
        reached_error,
        "Expected Error state for nonexistent file, got: {:?}",
        controller.state()
    );

    // Verify rich diagnostic error message
    if let PlaybackState::Error(msg) = controller.state() {
        assert!(
            msg.contains("bad_file")
                || msg.contains("muvrix_invalid_nonexistent")
                || msg.contains("open/demux"),
            "Error message should contain useful diagnostics: {msg}"
        );
    }

    // 2. Load valid file immediately following error
    let valid_path = resolve_media_path("tests/fixtures/test_video.mp4").unwrap();
    controller.load_file(valid_path.to_str().unwrap()).unwrap();
    assert_eq!(*controller.state(), PlaybackState::Loading);

    let reached_playing = wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Playing),
        Duration::from_secs(3),
    );
    assert!(
        reached_playing,
        "Expected Playing state after recovery, got: {:?}",
        controller.state()
    );
    assert!(controller.duration() > 0.0);
    assert!(controller.video_dimensions().0 > 0);
}

#[test]
fn test_sequential_valid_file_replacement() {
    let mut controller = create_headless_controller();

    let file1 = resolve_media_path("tests/fixtures/test_video.mp4").unwrap();
    let file2 =
        resolve_media_path("tests/fixtures/path-resolution/test video with spaces.mp4").unwrap();

    // Load file 1
    controller.load_file(file1.to_str().unwrap()).unwrap();
    let file1_ready = wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Playing),
        Duration::from_secs(3),
    );
    assert!(file1_ready);
    let dur1 = controller.duration();
    assert_eq!(controller.video_format(), "h264");

    // Replace with file 2 while file 1 is playing
    controller.load_file(file2.to_str().unwrap()).unwrap();
    assert_eq!(*controller.state(), PlaybackState::Loading);

    let file2_ready = wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Playing),
        Duration::from_secs(3),
    );
    assert!(file2_ready);
    assert_eq!(controller.video_format(), "h264");
    assert!(controller.duration() > 0.0);
    log::info!("File 1 dur: {dur1}, File 2 dur: {}", controller.duration());
}

#[test]
fn test_event_ordering_and_corrupted_file() {
    let mut controller = create_headless_controller();

    let corrupted_path = resolve_media_path("tests/fixtures/corrupted.mp4").unwrap();

    controller
        .load_file(corrupted_path.to_str().unwrap())
        .unwrap();
    assert_eq!(*controller.state(), PlaybackState::Loading);

    // Corrupted file must cleanly transition to Error or Ended without hang
    let resolved = wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Error(_) | PlaybackState::Ended),
        Duration::from_secs(3),
    );
    assert!(
        resolved,
        "Expected Error or Ended for corrupted file, got: {:?}",
        controller.state()
    );
}
