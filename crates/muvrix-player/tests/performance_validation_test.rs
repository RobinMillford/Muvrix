use std::path::PathBuf;
use std::time::{Duration, Instant};

use muvrix_player::{Event, HwdecMemoryPath, HwdecStatus, LoadFileMode, MpvHandle, SeekMode};

fn find_media_path(relative: &str) -> Option<PathBuf> {
    let candidates = [
        PathBuf::from(relative),
        PathBuf::from("../../").join(relative),
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../")
            .join(relative),
    ];

    for path in &candidates {
        if path.exists() {
            return Some(path.canonicalize().unwrap_or_else(|_| path.clone()));
        }
    }
    None
}

fn create_perf_player(hwdec: &str) -> MpvHandle {
    let handle = MpvHandle::new().expect("Failed to create MpvHandle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.set_option_string("idle", "yes").unwrap();
    handle.set_option_string("hwdec", hwdec).unwrap();
    handle.initialize().unwrap();
    handle
}

#[test]
fn test_perf_1080p_source_playback() {
    let path = find_media_path("tests/MuvrixTestMedia/phase-0.4/A-core/h264-aac.mp4")
        .or_else(|| find_media_path("tests/MuvrixTestMedia/baseline/h264-aac.mp4"))
        .expect("1080p sample missing");

    let player = create_perf_player("auto-safe");
    let start = Instant::now();
    player
        .load_file(path.to_str().unwrap(), LoadFileMode::Replace)
        .unwrap();

    std::thread::sleep(Duration::from_millis(300));
    let diag = player.query_playback_diagnostics("auto-safe").unwrap();
    let elapsed = start.elapsed();

    println!("1080p Performance: Init+Decode in {:?}", elapsed);
    println!(" - HWDEC: {:?}", diag.hwdec_status);
    println!(" - Video Codec: {}", diag.video_codec);
    println!(" - Container FPS: {}", diag.container_fps);

    assert_eq!(diag.video_codec, "h264");
    assert!(matches!(diag.hwdec_status, HwdecStatus::Active { .. }));
}

#[test]
fn test_perf_4k_source_playback() {
    let path = find_media_path("tests/MuvrixTestMedia/phase-0.4/B-performance/h264-4k-2160p.mp4")
        .expect("4K sample missing");

    let player = create_perf_player("auto-safe");
    let start = Instant::now();
    player
        .load_file(path.to_str().unwrap(), LoadFileMode::Replace)
        .unwrap();

    std::thread::sleep(Duration::from_millis(300));
    let diag = player.query_playback_diagnostics("auto-safe").unwrap();
    let elapsed = start.elapsed();

    let width: i64 = player.get_property("width").unwrap_or(0);
    let height: i64 = player.get_property("height").unwrap_or(0);

    println!("4K UHD Performance: Opened in {:?}", elapsed);
    println!(" - Resolution: {width}x{height}");
    println!(" - HWDEC: {:?}", diag.hwdec_status);

    assert_eq!((width, height), (3840, 2160));
    assert!(matches!(
        diag.hwdec_status,
        HwdecStatus::Active {
            memory_path: HwdecMemoryPath::CopyBack,
            ..
        }
    ));
}

#[test]
fn test_perf_4k60_source_playback() {
    let path = find_media_path("tests/MuvrixTestMedia/phase-0.4/generated/h264-4k-60fps.mp4")
        .or_else(|| find_media_path("tests/MuvrixTestMedia/phase-0.4/A-core/av1-4k-cityhall.webm"))
        .expect("4K60 sample missing");

    let player = create_perf_player("auto-safe");
    player
        .load_file(path.to_str().unwrap(), LoadFileMode::Replace)
        .unwrap();

    std::thread::sleep(Duration::from_millis(300));
    let diag = player.query_playback_diagnostics("auto-safe").unwrap();

    let width: i64 = player.get_property("width").unwrap_or(0);
    let height: i64 = player.get_property("height").unwrap_or(0);

    println!("4K 60 FPS Performance:");
    println!(" - Resolution: {width}x{height}");
    println!(" - Container FPS: {}", diag.container_fps);
    println!(" - HWDEC: {:?}", diag.hwdec_status);

    assert_eq!((width, height), (3840, 2160));
    assert!(diag.container_fps >= 59.0);
    assert!(matches!(diag.hwdec_status, HwdecStatus::Active { .. }));
}

#[test]
fn test_perf_120fps_high_frame_rate() {
    let path = find_media_path("tests/MuvrixTestMedia/phase-0.4/B-performance/h264-120fps.mp4")
        .expect("120fps sample missing");

    let player = create_perf_player("auto-safe");
    player
        .load_file(path.to_str().unwrap(), LoadFileMode::Replace)
        .unwrap();

    std::thread::sleep(Duration::from_millis(300));
    let diag = player.query_playback_diagnostics("auto-safe").unwrap();

    println!("120 FPS High-Frame-Rate Test:");
    println!(" - Codec: {}", diag.video_codec);
    println!(" - Container FPS: {}", diag.container_fps);
    println!(" - HWDEC: {:?}", diag.hwdec_status);

    assert_eq!(diag.video_codec, "h264");
    assert!(diag.container_fps >= 119.0);
}

#[test]
fn test_perf_240fps_high_frame_rate() {
    let path = find_media_path("tests/MuvrixTestMedia/phase-0.4/B-performance/h264-240fps.mp4")
        .expect("240fps sample missing");

    let player = create_perf_player("auto-safe");
    player
        .load_file(path.to_str().unwrap(), LoadFileMode::Replace)
        .unwrap();

    std::thread::sleep(Duration::from_millis(300));
    let diag = player.query_playback_diagnostics("auto-safe").unwrap();

    println!("240 FPS Ultra-High-Frame-Rate Test:");
    println!(" - Codec: {}", diag.video_codec);
    println!(" - Container FPS: {}", diag.container_fps);
    println!(" - HWDEC: {:?}", diag.hwdec_status);

    assert_eq!(diag.video_codec, "h264");
    assert!(diag.container_fps >= 239.0);
}

#[test]
fn test_perf_sustained_playback_and_timeline_scrubbing() {
    let path =
        find_media_path("tests/MuvrixTestMedia/phase-0.4/B-performance/h264-5min-sustained.mp4")
            .expect("5-min sustained sample missing");

    let player = create_perf_player("auto-safe");
    player
        .load_file(path.to_str().unwrap(), LoadFileMode::Replace)
        .unwrap();

    std::thread::sleep(Duration::from_millis(200));
    let duration: f64 = player.get_property("duration").unwrap_or(0.0);
    println!("5-Minute Sustained Stream Duration: {:.2}s", duration);
    assert!((duration - 300.0).abs() < 5.0);

    // Scrub across multiple minute marks
    let checkpoints = [60.0, 120.0, 180.0, 240.0, 290.0];
    for &sec in &checkpoints {
        let t0 = Instant::now();
        player.seek(sec, SeekMode::Absolute).unwrap();

        // Wait for seek to resolve
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut final_pos = 0.0;
        while Instant::now() < deadline {
            let ev = player.wait_event(0.05);
            if matches!(ev, Event::PlaybackRestart | Event::Seek) {
                final_pos = player.get_property("time-pos").unwrap_or(0.0);
                if (final_pos - sec).abs() < 5.0 {
                    break;
                }
            }
            final_pos = player.get_property("time-pos").unwrap_or(0.0);
            if (final_pos - sec).abs() < 5.0 {
                break;
            }
        }
        let seek_latency = t0.elapsed();
        println!(" - Seek to {sec}s resolved at {final_pos:.2}s in {seek_latency:?}");
        assert!((final_pos - sec).abs() < 5.0);
    }
}

#[test]
fn test_perf_high_bitrate_prores() {
    let path = find_media_path("tests/MuvrixTestMedia/phase-0.4/D-professional/prores.mov")
        .expect("ProRes sample missing");

    let player = create_perf_player("auto-safe");
    player
        .load_file(path.to_str().unwrap(), LoadFileMode::Replace)
        .unwrap();

    std::thread::sleep(Duration::from_millis(300));
    let diag = player.query_playback_diagnostics("auto-safe").unwrap();

    println!("High-Bitrate ProRes Test:");
    println!(" - Video Codec: {}", diag.video_codec);
    println!(" - HWDEC: {:?}", diag.hwdec_status);

    assert_eq!(diag.video_codec, "prores");
    // ProRes decodes either via CPU software or copy-back GPU hardware (e.g. vulkan-copy)
    assert!(matches!(
        diag.hwdec_status,
        HwdecStatus::Active {
            memory_path: HwdecMemoryPath::CopyBack,
            ..
        } | HwdecStatus::SoftwareFallback { .. }
            | HwdecStatus::Disabled
    ));
}
