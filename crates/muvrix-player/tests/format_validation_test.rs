use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use muvrix_player::{MpvHandle, PlaybackController, PlaybackState, SeekMode, TrackType};

fn get_test_video_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/test_video.mp4")
        .canonicalize()
        .expect("Tracked test fixture 'tests/fixtures/test_video.mp4' must exist")
}

fn find_media_path(relative: &str) -> Option<PathBuf> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../")
        .join(relative);
    if path.exists() {
        Some(path.canonicalize().unwrap_or(path))
    } else {
        None
    }
}

fn create_format_test_controller(hwdec: &str) -> PlaybackController {
    let handle = MpvHandle::new().expect("Failed to create MpvHandle");
    handle
        .set_option_string("vo", "null")
        .expect("Failed to set vo=null");
    handle
        .set_option_string("ao", "null")
        .expect("Failed to set ao=null");
    handle
        .set_option_string("idle", "yes")
        .expect("Failed to set idle=yes");
    handle
        .set_option_string("hwdec", hwdec)
        .expect("Failed to set hwdec option");
    handle.initialize().expect("Failed to initialize MpvHandle");

    PlaybackController::new(Arc::new(handle)).expect("Failed to create PlaybackController")
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
fn test_format_h264_aac_mp4() {
    let path = get_test_video_fixture();
    let mut controller = create_format_test_controller("auto-safe");

    controller.load_file(path.to_str().unwrap()).unwrap();
    assert!(wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Playing),
        Duration::from_secs(5)
    ));

    println!("--- H.264 + AAC MP4 ---");
    println!("Duration:    {:.2}s", controller.duration());
    println!("Video fmt:   {}", controller.video_format());
    println!("Audio codec: {}", controller.audio_codec());
    println!("HWDEC:       {}", controller.hwdec_current());
    println!("AO:          {}", controller.current_ao());
    println!("Dimensions:  {:?}", controller.video_dimensions());

    assert_eq!(controller.video_format(), "h264");
    assert_eq!(controller.audio_codec(), "aac");
    assert!((controller.duration() - 3.0).abs() < 1.0);
    let (w, h) = controller.video_dimensions();
    assert_eq!((w, h), (640, 360));
}

#[test]
#[ignore = "Requires extended media corpus in tests/MuvrixTestMedia/; run with: cargo test -p muvrix-player --test format_validation_test -- --ignored"]
fn test_format_hevc_aac_mp4() {
    let path =
        find_media_path("tests/MuvrixTestMedia/codecs/hevc-aac.mp4").expect("hevc-aac.mp4 missing");
    let mut controller = create_format_test_controller("auto-safe");

    controller.load_file(path.to_str().unwrap()).unwrap();
    assert!(wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Playing),
        Duration::from_secs(5)
    ));

    println!("--- HEVC + AAC MP4 ---");
    println!("Duration:    {:.2}s", controller.duration());
    println!("Video fmt:   {}", controller.video_format());
    println!("Audio codec: {}", controller.audio_codec());
    println!("HWDEC:       {}", controller.hwdec_current());

    assert_eq!(controller.video_format(), "hevc");
    assert_eq!(controller.audio_codec(), "aac");
    assert!((controller.duration() - 10.0).abs() < 1.0);
}

#[test]
#[ignore = "Requires extended media corpus in tests/MuvrixTestMedia/; run with: cargo test -p muvrix-player --test format_validation_test -- --ignored"]
fn test_format_vp9_opus_webm() {
    let path = find_media_path("tests/MuvrixTestMedia/codecs/vp9-opus.webm")
        .expect("vp9-opus.webm missing");
    let mut controller = create_format_test_controller("auto-safe");

    controller.load_file(path.to_str().unwrap()).unwrap();
    assert!(wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Playing),
        Duration::from_secs(5)
    ));

    println!("--- VP9 + Opus WebM ---");
    println!("Duration:    {:.2}s", controller.duration());
    println!("Video fmt:   {}", controller.video_format());
    println!("Audio codec: {}", controller.audio_codec());
    println!("HWDEC:       {}", controller.hwdec_current());

    assert_eq!(controller.video_format(), "vp9");
    assert_eq!(controller.audio_codec(), "opus");
    assert!((controller.duration() - 10.0).abs() < 1.0);
}

#[test]
#[ignore = "Requires extended media corpus in tests/MuvrixTestMedia/; run with: cargo test -p muvrix-player --test format_validation_test -- --ignored"]
fn test_format_av1_1080p_webm() {
    let path = find_media_path("tests/MuvrixTestMedia/codecs/av1-1080p.webm")
        .expect("av1-1080p.webm missing");
    let mut controller = create_format_test_controller("auto-safe");

    controller.load_file(path.to_str().unwrap()).unwrap();
    assert!(wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Playing),
        Duration::from_secs(5)
    ));

    println!("--- AV1 1080p WebM ---");
    println!("Duration:    {:.2}s", controller.duration());
    println!("Video fmt:   {}", controller.video_format());
    println!("HWDEC:       {}", controller.hwdec_current());
    println!("Dimensions:  {:?}", controller.video_dimensions());

    assert_eq!(controller.video_format(), "av1");
    assert!((controller.duration() - 10.0).abs() < 1.0);
    assert_eq!(controller.video_dimensions(), (1920, 1080));
}

#[test]
#[ignore = "Requires extended media corpus in tests/MuvrixTestMedia/; run with: cargo test -p muvrix-player --test format_validation_test -- --ignored"]
fn test_format_multiple_audio_streams_mov() {
    let path = find_media_path("tests/MuvrixTestMedia/tracks/multiple-audio-streams.mov")
        .expect("multiple-audio-streams.mov missing");
    let mut controller = create_format_test_controller("no");

    controller.load_file(path.to_str().unwrap()).unwrap();
    assert!(wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Playing),
        Duration::from_secs(5)
    ));

    println!("--- Multiple Audio Streams MOV ---");
    println!("Duration:    {:.2}s", controller.duration());
    println!("Video fmt:   {}", controller.video_format());
    let audio_track_ids: Vec<i64> = controller
        .tracks_by_type(TrackType::Audio)
        .into_iter()
        .map(|t| t.id)
        .collect();
    println!("Audio tracks count: {}", audio_track_ids.len());

    assert_eq!(controller.video_format(), "prores");
    assert_eq!(audio_track_ids.len(), 4, "Expected 4 separate audio tracks");

    // Test switching through each audio track
    for (idx, id) in audio_track_ids.iter().enumerate() {
        println!("Switching to audio track {idx} (ID {id})");
        controller.set_audio_track(Some(*id)).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        controller.poll_events();
        let selected_audio = controller.selected_track(TrackType::Audio);
        assert!(selected_audio.is_some());
        assert_eq!(selected_audio.unwrap().id, *id);
    }
}

#[test]
#[ignore = "Requires extended media corpus in tests/MuvrixTestMedia/; run with: cargo test -p muvrix-player --test format_validation_test -- --ignored"]
fn test_format_video_only_mp4() {
    let path = find_media_path("tests/MuvrixTestMedia/tracks/video-only.mp4")
        .expect("video-only.mp4 missing");
    let mut controller = create_format_test_controller("auto-safe");

    controller.load_file(path.to_str().unwrap()).unwrap();
    assert!(wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Playing),
        Duration::from_secs(5)
    ));

    println!("--- Video-only MP4 ---");
    println!("Duration:    {:.2}s", controller.duration());
    println!("Video fmt:   {}", controller.video_format());
    println!("Audio codec: '{}'", controller.audio_codec());

    assert_eq!(controller.video_format(), "h264");
    let audio_tracks = controller.tracks_by_type(TrackType::Audio);
    assert_eq!(audio_tracks.len(), 0);
}

#[test]
#[ignore = "Requires extended media corpus in tests/MuvrixTestMedia/; run with: cargo test -p muvrix-player --test format_validation_test -- --ignored"]
fn test_format_audio_only_flac() {
    let path =
        find_media_path("tests/MuvrixTestMedia/audio/audio.flac").expect("audio.flac missing");
    let mut controller = create_format_test_controller("no");

    controller.load_file(path.to_str().unwrap()).unwrap();
    assert!(wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Playing),
        Duration::from_secs(5)
    ));

    println!("--- Audio-only FLAC ---");
    println!("Duration:    {:.2}s", controller.duration());
    println!("Audio codec: {}", controller.audio_codec());
    println!("Video fmt:   '{}'", controller.video_format());

    assert_eq!(controller.audio_codec(), "flac");
    let video_tracks = controller.tracks_by_type(TrackType::Video);
    assert_eq!(video_tracks.len(), 0);
}

#[test]
#[ignore = "Requires external BluRay MKV sample; run with: cargo test -p muvrix-player --test format_validation_test -- --ignored"]
fn test_format_eac3_7_1_sample() {
    let sample_path =
        Path::new("/home/itsyamin/Videos/Colony.2026.1080p.BluRay.DUAL.DDP7.1.x264-PRiME.mkv");
    if !sample_path.exists() {
        println!("Skipping 7.1 E-AC-3 test: sample file not present on this machine.");
        return;
    }

    let mut controller = create_format_test_controller("auto-safe");
    controller.load_file(sample_path.to_str().unwrap()).unwrap();
    assert!(wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Playing),
        Duration::from_secs(5)
    ));

    println!("--- 7.1 E-AC-3 BluRay MKV ---");
    println!("Duration:    {:.2}s", controller.duration());
    println!("Video fmt:   {}", controller.video_format());
    println!("Audio codec: {}", controller.audio_codec());
    println!("HWDEC:       {}", controller.hwdec_current());
    let audio_tracks = controller.tracks_by_type(TrackType::Audio);
    println!("Audio tracks: {}", audio_tracks.len());
    for track in &audio_tracks {
        println!(
            "  - Track {}: codec={:?}, title={:?}, lang={:?}",
            track.id, track.codec, track.title, track.lang
        );
    }

    assert_eq!(controller.video_format(), "h264");
    assert!(controller.duration() > 7000.0);
    assert!(audio_tracks.len() >= 2);

    // Seek to 120s and verify playback continuity
    controller.seek(120.0, SeekMode::Absolute).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    controller.poll_events();
    assert_eq!(*controller.state(), PlaybackState::Playing);
}
