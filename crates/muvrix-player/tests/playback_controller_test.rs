use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use muvrix_player::{MpvHandle, PlaybackController, PlaybackState, SeekMode, TrackType};

fn find_media_path(relative: &str) -> PathBuf {
    let candidates = [
        PathBuf::from(relative),
        PathBuf::from("../../").join(relative),
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../")
            .join(relative),
    ];

    for path in &candidates {
        if path.exists() {
            return path.canonicalize().unwrap_or_else(|_| path.clone());
        }
    }

    panic!(
        "Media fixture not found: {relative} (tried: {:?})",
        candidates
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
    );
}

fn create_test_controller() -> PlaybackController {
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
fn test_controller_initial_state_and_properties() {
    let mut controller = create_test_controller();
    assert_eq!(*controller.state(), PlaybackState::Idle);

    // Test Volume
    controller.set_volume(80.0).expect("Failed to set volume");
    assert!((controller.volume() - 80.0).abs() < 1e-4);

    // Test Mute
    controller.set_mute(true).expect("Failed to set mute");
    assert!(controller.is_muted());
    controller.toggle_mute().expect("Failed to toggle mute");
    assert!(!controller.is_muted());

    // Test Speed
    controller.set_speed(1.5).expect("Failed to set speed");
    assert!((controller.speed() - 1.5).abs() < 1e-4);
}

#[test]
fn test_controller_load_file_and_playback_lifecycle() {
    let path = find_media_path("tests/fixtures/test_video.mp4");
    let mut controller = create_test_controller();

    controller
        .load_file(path.to_str().unwrap())
        .expect("Failed to load file");
    assert_eq!(*controller.state(), PlaybackState::Loading);

    // Wait for playing
    let reached = wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Playing | PlaybackState::Paused),
        Duration::from_secs(5),
    );
    assert!(
        reached,
        "Did not reach Playing state: {:?}",
        controller.state()
    );

    // Verify metadata
    assert!(
        controller.duration() > 2.0,
        "Duration: {}",
        controller.duration()
    );
    assert_eq!(controller.video_format(), "h264");
    let (w, h) = controller.video_dimensions();
    assert_eq!((w, h), (640, 360));

    // Test Pause
    controller.pause().expect("Failed to pause");
    assert_eq!(*controller.state(), PlaybackState::Paused);
    assert!(controller.is_paused());

    // Test Resume
    controller.play().expect("Failed to play");
    assert_eq!(*controller.state(), PlaybackState::Playing);
    assert!(!controller.is_paused());

    // Test Toggle Pause
    controller.toggle_pause().expect("Failed to toggle pause");
    assert_eq!(*controller.state(), PlaybackState::Paused);
    controller.toggle_pause().expect("Failed to toggle pause");
    assert_eq!(*controller.state(), PlaybackState::Playing);
}

#[test]
fn test_controller_load_second_file_while_playing() {
    let path1 = find_media_path("tests/fixtures/test_video.mp4");
    let path2 = find_media_path("tests/fixtures/test_video.mp4");
    let mut controller = create_test_controller();

    // Load first file
    controller
        .load_file(path1.to_str().unwrap())
        .expect("Failed to load file 1");
    assert!(wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Playing),
        Duration::from_secs(5)
    ));
    assert!(controller.duration() < 5.0);

    // Immediately load second file while first is playing
    controller
        .load_file(path2.to_str().unwrap())
        .expect("Failed to load file 2");
    assert_eq!(*controller.state(), PlaybackState::Loading);

    assert!(wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Playing),
        Duration::from_secs(5)
    ));
    // Verify duration updated to ~3.0s
    assert!(
        (controller.duration() - 3.0).abs() < 1.0,
        "Expected ~3s duration, got {}",
        controller.duration()
    );
}

#[test]
fn test_controller_stop_and_reload() {
    let path = find_media_path("tests/fixtures/test_video.mp4");
    let mut controller = create_test_controller();

    controller
        .load_file(path.to_str().unwrap())
        .expect("Failed to load file");
    assert!(wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Playing),
        Duration::from_secs(5)
    ));

    // Stop
    controller.stop().expect("Failed to stop");
    assert_eq!(*controller.state(), PlaybackState::Idle);
    assert_eq!(controller.time_pos(), 0.0);
    assert_eq!(controller.duration(), 0.0);

    // Reload
    controller
        .load_file(path.to_str().unwrap())
        .expect("Failed to reload file");
    assert!(wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Playing),
        Duration::from_secs(5)
    ));
    assert!(controller.duration() > 2.0);
}

#[test]
fn test_controller_end_of_file() {
    let path = find_media_path("tests/fixtures/test_video.mp4");
    let mut controller = create_test_controller();

    controller
        .load_file(path.to_str().unwrap())
        .expect("Failed to load file");
    assert!(wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Playing),
        Duration::from_secs(5)
    ));

    // Seek right before EOF (~2.8s in a 3s file)
    let dur = controller.duration();
    controller
        .seek(dur - 0.2, SeekMode::Absolute)
        .expect("Failed to seek");

    // Wait for EOF
    let reached_eof = wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Ended),
        Duration::from_secs(5),
    );
    assert!(
        reached_eof,
        "Did not transition to Ended: {:?}",
        controller.state()
    );
}

#[test]
fn test_controller_rapid_seeking() {
    let path = find_media_path("tests/fixtures/test_video.mp4");
    let mut controller = create_test_controller();

    controller
        .load_file(path.to_str().unwrap())
        .expect("Failed to load file");
    assert!(wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Playing),
        Duration::from_secs(5)
    ));

    // Perform 50 rapid seeks across timeline
    for i in 0..50 {
        let target = (i as f64 * 0.05) % 2.5;
        controller
            .seek(target, SeekMode::Absolute)
            .expect("Seek command failed");
        if i % 10 == 0 {
            controller.poll_events();
        }
    }

    // Ensure engine remains responsive
    controller.poll_events();
    assert!(matches!(
        *controller.state(),
        PlaybackState::Playing | PlaybackState::Paused
    ));
}

#[test]
fn test_controller_missing_file_error() {
    let mut controller = create_test_controller();
    let missing_path = "/tmp/muvrix_non_existent_media_fixture_987654.mp4";

    controller
        .load_file(missing_path)
        .expect("Load file call itself succeeds");
    assert_eq!(*controller.state(), PlaybackState::Loading);

    // Wait for error state
    let reached_error = wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Error(_)),
        Duration::from_secs(5),
    );
    assert!(
        reached_error,
        "Expected PlaybackState::Error, got {:?}",
        controller.state()
    );
}

#[test]
fn test_controller_corrupted_file_handling() {
    let path = find_media_path("tests/fixtures/corrupted.mp4");
    let mut controller = create_test_controller();

    controller
        .load_file(path.to_str().unwrap())
        .expect("Load file call succeeds");

    // Corrupted file must either trigger Error or Ended gracefully without crashing
    let resolved = wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Error(_) | PlaybackState::Ended),
        Duration::from_secs(5),
    );
    assert!(
        resolved,
        "Expected Error or Ended for corrupted file, got: {:?}",
        controller.state()
    );
}

#[test]
fn test_controller_unsupported_media_handling() {
    let path = find_media_path("tests/fixtures/unsupported.txt");
    let mut controller = create_test_controller();

    controller
        .load_file(path.to_str().unwrap())
        .expect("Load file call succeeds");

    let resolved = wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Error(_)),
        Duration::from_secs(5),
    );
    assert!(
        resolved,
        "Expected Error for unsupported media, got: {:?}",
        controller.state()
    );
}

#[test]
#[ignore = "Requires synthetic multi-sub MKV in tests/MuvrixTestMedia/tracks/; run with: cargo test -p muvrix-player --test playback_controller_test -- test_controller_subtitles_embedded_and_external --ignored --nocapture"]
fn test_controller_subtitles_embedded_and_external() {
    let path = find_media_path("tests/MuvrixTestMedia/tracks/synthetic-multi-sub.mkv");
    let srt_path = find_media_path("tests/MuvrixTestMedia/tracks/sample.srt");
    let ass_path = find_media_path("tests/MuvrixTestMedia/tracks/sample.ass");

    let mut controller = create_test_controller();
    controller
        .load_file(path.to_str().unwrap())
        .expect("Failed to load synthetic mkv");

    assert!(wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Playing),
        Duration::from_secs(5)
    ));

    // Verify tracks present: 1 video, 1 audio, 2 embedded subtitles
    let sub_tracks = controller.tracks_by_type(TrackType::Subtitle);
    assert_eq!(sub_tracks.len(), 2, "Expected 2 embedded subtitle tracks");

    // Track 1 should be SRT, Track 2 should be ASS
    let sub1_id = sub_tracks[0].id;
    let sub2_id = sub_tracks[1].id;

    // Test selecting subtitle track 1
    controller
        .set_subtitle_track(Some(sub1_id))
        .expect("Failed to select subtitle 1");
    // Verify sid property
    let sid: String = controller
        .handle()
        .get_property_string("sid")
        .expect("Failed to read sid");
    assert_eq!(sid, sub1_id.to_string());

    // Test secondary subtitle selection
    controller
        .set_secondary_subtitle_track(Some(sub2_id))
        .expect("Failed to set secondary subtitle");
    let sec_sid: String = controller
        .handle()
        .get_property_string("secondary-sid")
        .expect("Failed to read secondary-sid");
    assert_eq!(sec_sid, sub2_id.to_string());

    // Test disabling secondary subtitle
    controller
        .set_secondary_subtitle_track(None)
        .expect("Failed to disable secondary subtitle");
    let sec_sid: String = controller
        .handle()
        .get_property_string("secondary-sid")
        .unwrap_or_else(|_| "no".to_string());
    assert!(sec_sid == "no" || sec_sid == "auto" || sec_sid == "0");

    // Test disabling primary subtitles
    controller
        .set_subtitle_track(None)
        .expect("Failed to disable subtitles");
    let sid: String = controller
        .handle()
        .get_property_string("sid")
        .unwrap_or_else(|_| "no".to_string());
    assert!(sid == "no" || sid == "0");

    // Test subtitle delay (+0.500s)
    controller
        .set_subtitle_delay(0.5)
        .expect("Failed to set subtitle delay");
    assert!((controller.sub_delay() - 0.5).abs() < 1e-4);

    // Test subtitle visibility
    controller
        .set_subtitle_visibility(false)
        .expect("Failed to hide subtitles");
    assert!(!controller.sub_visibility());
    controller
        .set_subtitle_visibility(true)
        .expect("Failed to show subtitles");
    assert!(controller.sub_visibility());

    // Test dynamically adding external SRT subtitle
    controller
        .add_subtitle_file(srt_path.to_str().unwrap(), true)
        .expect("Failed to add external srt");

    // Wait for track list to update
    std::thread::sleep(Duration::from_millis(100));
    controller.poll_events();
    let updated_subs = controller.tracks_by_type(TrackType::Subtitle);
    assert!(
        updated_subs.len() >= 3,
        "Expected at least 3 subtitle tracks after external add, got {}",
        updated_subs.len()
    );

    // Test dynamically adding external ASS subtitle
    controller
        .add_subtitle_file(ass_path.to_str().unwrap(), false)
        .expect("Failed to add external ass");
    std::thread::sleep(Duration::from_millis(100));
    controller.poll_events();
    let final_subs = controller.tracks_by_type(TrackType::Subtitle);
    assert!(
        final_subs.len() >= 4,
        "Expected at least 4 subtitle tracks after adding second external, got {}",
        final_subs.len()
    );
}

#[test]
fn test_controller_repeated_playback_sessions() {
    let path = find_media_path("tests/fixtures/test_video.mp4");
    let path_str = path.to_str().unwrap();

    let mut controller = create_test_controller();

    for i in 1..=10 {
        controller
            .load_file(path_str)
            .unwrap_or_else(|e| panic!("Session {i} failed to load: {e}"));

        assert!(
            wait_for_state(
                &mut controller,
                |s| matches!(s, PlaybackState::Playing),
                Duration::from_secs(3)
            ),
            "Session {i} failed to start playing"
        );

        controller.seek(1.0, SeekMode::Absolute).unwrap();
        controller.poll_events();

        controller.stop().unwrap();
        assert_eq!(*controller.state(), PlaybackState::Idle);
    }
}

#[test]
fn test_controller_clean_shutdown() {
    let path = find_media_path("tests/fixtures/test_video.mp4");
    let mut controller = create_test_controller();
    controller.load_file(path.to_str().unwrap()).unwrap();

    let start = Instant::now();
    drop(controller);
    let elapsed = start.elapsed();
    println!("Clean shutdown elapsed: {elapsed:?}");
    assert!(
        elapsed < Duration::from_millis(500),
        "Shutdown exceeded 500ms"
    );
}

#[test]
fn test_controller_volume_and_speed_adjustments() {
    let mut controller = create_test_controller();

    // Volume adjustments
    controller.set_volume(80.0).expect("set_volume succeeds");
    assert_eq!(controller.volume(), 80.0);

    controller
        .adjust_volume(15.0)
        .expect("adjust_volume +15 succeeds");
    assert_eq!(controller.volume(), 95.0);

    // Clamping at ceiling 100.0
    controller
        .adjust_volume(20.0)
        .expect("adjust_volume +20 succeeds");
    assert_eq!(controller.volume(), 100.0);

    // Clamping at floor 0.0
    controller
        .adjust_volume(-150.0)
        .expect("adjust_volume -150 succeeds");
    assert_eq!(controller.volume(), 0.0);

    // Mute toggle
    assert!(!controller.is_muted());
    controller.toggle_mute().expect("toggle_mute succeeds");
    assert!(controller.is_muted());
    controller
        .toggle_mute()
        .expect("toggle_mute un-mute succeeds");
    assert!(!controller.is_muted());

    // Speed adjustments
    controller.set_speed(1.0).expect("set_speed succeeds");
    assert_eq!(controller.speed(), 1.0);

    controller
        .adjust_speed(0.25)
        .expect("adjust_speed +0.25 succeeds");
    assert_eq!(controller.speed(), 1.25);

    controller
        .adjust_speed(-0.50)
        .expect("adjust_speed -0.50 succeeds");
    assert_eq!(controller.speed(), 0.75);

    controller.reset_speed().expect("reset_speed succeeds");
    assert_eq!(controller.speed(), 1.0);
}

#[test]
fn test_controller_volume_boost_up_to_150_percent() {
    let mut controller = create_test_controller();
    assert_eq!(controller.volume_boost_max(), 100.0);

    // Enable 150% volume boost
    controller
        .set_volume_boost(150.0)
        .expect("Enable 150% boost");
    assert_eq!(controller.volume_boost_max(), 150.0);

    // Can set volume to 140%
    controller.set_volume(140.0).expect("Set volume to 140%");
    assert_eq!(controller.volume(), 140.0);

    // Adjusting beyond 150 clamps at 150%
    controller.adjust_volume(20.0).expect("Adjust +20");
    assert_eq!(controller.volume(), 150.0);
}

#[test]
fn test_controller_chapter_and_relative_seek() {
    let path = find_media_path("tests/fixtures/test_video.mp4");
    let mut controller = create_test_controller();
    controller
        .load_file(path.to_str().unwrap())
        .expect("Load file succeeds");

    assert!(wait_for_state(
        &mut controller,
        |s| matches!(s, PlaybackState::Playing),
        Duration::from_secs(3)
    ));

    // Relative seek should succeed without error
    controller
        .seek_relative(1.0)
        .expect("Relative seek forward succeeds");
    controller
        .seek_relative(-0.5)
        .expect("Relative seek backward succeeds");

    // Chapter navigation commands on file without chapters should not crash
    assert_eq!(controller.chapter_count(), 0);
    assert!(controller.current_chapter().is_none());
    let _ = controller.next_chapter();
    let _ = controller.prev_chapter();
    let _ = controller.set_chapter(0);

    controller.stop().expect("Stop succeeds");
    assert_eq!(*controller.state(), PlaybackState::Idle);
}

#[test]
fn test_playback_lifecycle_50_cycles_stress() {
    let path = find_media_path("tests/fixtures/test_video.mp4");
    let mut controller = create_test_controller();

    fn get_process_metrics() -> (usize, usize, usize) {
        #[cfg(target_os = "linux")]
        {
            let fd_count = std::fs::read_dir("/proc/self/fd")
                .map(|entries| entries.count())
                .unwrap_or(0);

            let mut threads = 0;
            let mut vmrss_kb = 0;
            if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
                for line in status.lines() {
                    if let Some(rest) = line.strip_prefix("Threads:") {
                        threads = rest.trim().parse::<usize>().unwrap_or(0);
                    } else if let Some(rest) = line.strip_prefix("VmRSS:") {
                        let val_str = rest.trim().trim_end_matches(" kB").trim();
                        vmrss_kb = val_str.parse::<usize>().unwrap_or(0);
                    }
                }
            }
            (fd_count, threads, vmrss_kb)
        }
        #[cfg(not(target_os = "linux"))]
        {
            (0, 0, 0)
        }
    }

    let (initial_fds, initial_threads, initial_rss) = get_process_metrics();
    println!(
        "\n[Stress Test Baseline] FDs: {}, Threads: {}, VmRSS: {} kB",
        initial_fds, initial_threads, initial_rss
    );

    let mut metrics_log = Vec::new();

    for cycle in 1..=50 {
        controller
            .load_file(path.to_str().unwrap())
            .expect("Load file in cycle succeeds");

        let playing = wait_for_state(
            &mut controller,
            |s| matches!(s, PlaybackState::Playing),
            Duration::from_millis(500),
        );
        assert!(playing, "Failed to reach Playing state in cycle {cycle}");

        controller.stop().expect("Stop in cycle succeeds");

        let stopped = wait_for_state(
            &mut controller,
            |s| matches!(s, PlaybackState::Idle),
            Duration::from_millis(500),
        );
        assert!(stopped, "Failed to reach Idle state in cycle {cycle}");

        if cycle % 10 == 0 || cycle == 1 {
            let (fds, threads, rss) = get_process_metrics();
            println!(
                "  Cycle {:02}/50: FDs={}, Threads={}, VmRSS={} kB",
                cycle, fds, threads, rss
            );
            metrics_log.push((cycle, fds, threads, rss));
        }
    }

    let (c50_fds, c50_threads, c50_rss) = get_process_metrics();
    println!(
        "[Stress Test Cycle 50 State] FDs: {}, Threads: {}, VmRSS: {} kB",
        c50_fds, c50_threads, c50_rss
    );

    // Drop the controller to exercise complete engine tear-down
    drop(controller);
    // Allow brief time for thread termination
    std::thread::sleep(Duration::from_millis(100));

    let (final_fds, final_threads, final_rss) = get_process_metrics();
    println!(
        "[Stress Test Post-Tear-down] FDs: {}, Threads: {}, VmRSS: {} kB",
        final_fds, final_threads, final_rss
    );

    #[cfg(target_os = "linux")]
    {
        // 1. Thread stability across playback cycles: worker threads must not
        // accumulate. Compare the post-warmup plateau (cycle 10, after mpv's
        // thread pool has spawned) to cycle 50; comparing against cycle 1
        // flags legitimate pool warmup (+3 threads) as a false "leak" when
        // FDs/RSS are flat. A real leak still grows past the plateau.
        if metrics_log.len() >= 2 {
            let c10_threads = metrics_log[1].2; // cycle 10
            assert!(
                c50_threads <= c10_threads + 2,
                "Thread leak across cycles: cycle 10 had {}, cycle 50 had {}",
                c10_threads,
                c50_threads
            );
        }

        // Post tear-down: worker threads should terminate back near baseline
        assert!(
            final_threads <= initial_threads + 2,
            "Worker threads failed to terminate on drop: initial={}, final={}",
            initial_threads,
            final_threads
        );

        // 2. File descriptors: must not accumulate across cycles
        assert!(
            c50_fds <= initial_fds + 4,
            "File descriptor leak across cycles: initial={}, cycle50={}",
            initial_fds,
            c50_fds
        );
        assert!(
            final_fds <= initial_fds + 2,
            "File descriptor leak after tear-down: initial={}, final={}",
            initial_fds,
            final_fds
        );

        // 3. RSS growth check: RSS at cycle 50 compared to cycle 10 must plateau
        if metrics_log.len() >= 2 {
            let rss_c10 = metrics_log[1].3; // cycle 10
            let rss_c50 = c50_rss; // cycle 50
            let growth_kb = rss_c50.saturating_sub(rss_c10);
            println!(
                "  Memory stability delta (Cycle 10 -> Cycle 50): +{} kB ({:.2} MB)",
                growth_kb,
                growth_kb as f64 / 1024.0
            );
            // Must plateau within 25 MB between cycle 10 and cycle 50
            assert!(
                growth_kb < 25 * 1024,
                "Monotonic memory leak detected: growth of {} kB between cycle 10 and 50",
                growth_kb
            );
        }
    }
}
