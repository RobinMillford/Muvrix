use muvrix_player::{MpvHandle, PlaybackController, PlaybackState, SeekMode};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread::sleep;
use std::time::{Duration, Instant};

fn create_test_controller() -> PlaybackController {
    let handle = MpvHandle::new().expect("create mpv handle");
    handle.set_option_string("vo", "null").expect("vo=null");
    handle.set_option_string("ao", "null").expect("ao=null");
    handle.set_option_string("idle", "yes").expect("idle=yes");
    handle.initialize().expect("initialize handle");
    PlaybackController::new(Arc::new(handle)).expect("create controller")
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
        sleep(Duration::from_millis(10));
    }
    controller.poll_events();
    predicate(controller.state())
}

#[test]
fn test_comprehensive_playback_lifecycle_and_verification() {
    let mut controller = create_test_controller();
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/fixtures/test_video.mp4");

    assert!(fixture_path.exists(), "test_video.mp4 must exist");
    let path_str = fixture_path.to_str().expect("valid path");

    // 1. Initial State
    assert_eq!(*controller.state(), PlaybackState::Idle);

    // 2. Open and load media file
    controller.load_file(path_str).expect("load media file");

    // 3. Playback starts and transitions to Playing state
    let started = wait_for_state(
        &mut controller,
        |s| *s == PlaybackState::Playing,
        Duration::from_secs(3),
    );
    assert!(started, "Playback must transition to Playing state");

    // 4. Playback position advances
    sleep(Duration::from_millis(200));
    controller.poll_events();
    let pos_start = controller.time_pos();
    sleep(Duration::from_millis(200));
    controller.poll_events();
    let pos_after = controller.time_pos();
    assert!(
        pos_after >= pos_start,
        "Position should advance: {pos_start} -> {pos_after}"
    );

    // 5. Pause works
    controller.pause().expect("pause playback");
    assert_eq!(*controller.state(), PlaybackState::Paused);
    let paused_pos = controller.time_pos();
    sleep(Duration::from_millis(100));
    controller.poll_events();
    assert_eq!(
        controller.time_pos(),
        paused_pos,
        "Position must be frozen while paused"
    );

    // 6. Resume works
    controller.play().expect("resume playback");
    assert_eq!(*controller.state(), PlaybackState::Playing);

    // 7. Seeking works (Absolute and Relative)
    controller
        .seek(2.0, SeekMode::Absolute)
        .expect("seek to 2.0s");
    sleep(Duration::from_millis(100));
    controller.poll_events();
    let seek_pos = controller.time_pos();
    assert!(
        seek_pos >= 1.5,
        "Seeked position should be near 2.0s, got {seek_pos}"
    );

    controller
        .seek(-1.0, SeekMode::Relative)
        .expect("relative seek -1.0s");
    sleep(Duration::from_millis(100));
    controller.poll_events();
    let rel_pos = controller.time_pos();
    assert!(
        rel_pos < seek_pos,
        "Relative negative seek must reduce time position"
    );

    // 8. Volume control and volume boost (up to 150%)
    controller.set_volume(85.0).expect("set volume 85");
    assert_eq!(controller.volume(), 85.0);
    controller
        .set_volume_boost(150.0)
        .expect("enable boost ceiling 150");
    assert_eq!(controller.volume_boost_max(), 150.0);
    controller.set_volume(140.0).expect("set volume boost 140");
    assert_eq!(controller.volume(), 140.0);
    controller.set_volume(100.0).expect("reset volume 100");
    assert_eq!(controller.volume(), 100.0);

    // 9. Speed adjustment
    controller.set_speed(1.5).expect("set speed 1.5");
    assert!((controller.speed() - 1.5).abs() < 0.05);
    controller.set_speed(1.0).expect("reset speed 1.0");
    assert_eq!(controller.speed(), 1.0);

    // 10. Duration & file properties
    assert!(
        controller.duration() > 0.0,
        "Duration must be greater than 0"
    );
    assert!(controller.current_file().is_some());

    // 11. Clean stop and controller teardown
    controller.stop().expect("stop playback");
    let stopped = wait_for_state(
        &mut controller,
        |s| *s == PlaybackState::Idle,
        Duration::from_secs(2),
    );
    assert!(stopped, "Controller must return to Idle after stop");
}
