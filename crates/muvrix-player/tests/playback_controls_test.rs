use std::sync::Arc;

use muvrix_player::controller::{parse_mpvex_command, AbLoopState, MpvexCommand, RepeatMode};
use muvrix_player::{MpvHandle, PlaybackController};

fn fixture() -> String {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/test_video.mp4")
        .to_string_lossy()
        .to_string()
}

fn create_controller() -> PlaybackController {
    let handle = MpvHandle::new().expect("create handle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().expect("initialize");
    PlaybackController::new(Arc::new(handle)).expect("controller")
}

fn wait_for_file(controller: &mut PlaybackController) {
    controller.load_file(&fixture()).expect("load fixture");
    for _ in 0..80 {
        for event in controller.poll_events() {
            if matches!(event, muvrix_player::Event::FileLoaded) {
                return;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    panic!("FileLoaded not observed");
}

#[test]
fn frame_step_commands_are_available_and_return_success() {
    let mut controller = create_controller();
    wait_for_file(&mut controller);
    let _ = controller.pause();
    let _ = controller.seek(1.0, muvrix_player::SeekMode::Absolute);
    controller.frame_step().expect("frame-step");
    controller.frame_back_step().expect("frame-back-step");
}

#[test]
fn ab_loop_cycles_and_sets_properties() {
    let mut controller = create_controller();
    wait_for_file(&mut controller);
    controller.cycle_ab_loop().expect("set A");
    assert!(matches!(controller.ab_loop_state(), AbLoopState::PointA(_)));
    controller.set_ab_loop(0.5, 1.5).expect("set A/B");
    assert!(matches!(
        controller.ab_loop_state(),
        AbLoopState::Loop { .. }
    ));
    assert!(!controller
        .handle()
        .get_property_string("ab-loop-a")
        .expect("ab-loop-a")
        .is_empty());
    controller.cycle_ab_loop().expect("clear");
    assert_eq!(controller.ab_loop_state(), AbLoopState::None);
    assert_eq!(
        controller
            .handle()
            .get_property_string("ab-loop-a")
            .expect("ab-loop-a"),
        "no"
    );
}

#[test]
fn repeat_mode_maps_to_loop_properties() {
    let mut controller = create_controller();
    controller.set_repeat_mode(RepeatMode::One).expect("one");
    assert_eq!(
        controller
            .handle()
            .get_property_string("loop-file")
            .expect("loop-file"),
        "inf"
    );
    controller.set_repeat_mode(RepeatMode::All).expect("all");
    assert_eq!(
        controller
            .handle()
            .get_property_string("loop-playlist")
            .expect("loop-playlist"),
        "inf"
    );
    controller.set_repeat_mode(RepeatMode::Off).expect("off");
    assert_eq!(
        controller
            .handle()
            .get_property_string("loop-playlist")
            .expect("loop-playlist"),
        "no"
    );
}

#[test]
fn panscan_and_volume_boost_clamp_to_supported_ranges() {
    let mut controller = create_controller();
    controller.set_panscan(2.0).expect("panscan high");
    assert_eq!(controller.panscan(), 1.0);
    controller.set_panscan(-1.0).expect("panscan low");
    assert_eq!(controller.panscan(), 0.0);

    controller.set_volume_boost(130.0).expect("boost");
    assert_eq!(controller.volume_boost_max(), 130.0);
    controller.set_volume(125.0).expect("boosted volume");
    assert_eq!(controller.volume(), 125.0);
}

#[test]
fn mpvex_ipc_parser_validates_and_bounds_payloads() {
    assert_eq!(
        parse_mpvex_command("user-data/mpvex/show_text", "hello", 0.0),
        Some(MpvexCommand::ShowText("hello".to_string()))
    );
    assert_eq!(
        parse_mpvex_command("user-data/mpvex/toggle_ui", "", 0.0),
        Some(MpvexCommand::ToggleUi)
    );
    assert_eq!(
        parse_mpvex_command("user-data/mpvex/show_panel", "settings", 0.0),
        Some(MpvexCommand::ShowPanel("settings".to_string()))
    );
    assert_eq!(
        parse_mpvex_command("user-data/mpvex/seek_by", "-5.5", 0.0),
        Some(MpvexCommand::SeekBy(-5.5))
    );
    assert_eq!(
        parse_mpvex_command("user-data/mpvex/seek_to", "42", 60.0),
        Some(MpvexCommand::SeekTo(42.0))
    );
    assert_eq!(
        parse_mpvex_command("user-data/mpvex/seek_to", "61", 60.0),
        None
    );
    assert_eq!(
        parse_mpvex_command("user-data/mpvex/seek_to", "NaN", 60.0),
        None
    );
    assert_eq!(
        parse_mpvex_command("user-data/mpvex/seek_by", "999999", 0.0),
        None
    );
    assert_eq!(parse_mpvex_command("user-data/other", "x", 0.0), None);
}

#[test]
fn mpvex_user_data_property_is_dispatched_through_controller() {
    let mut controller = create_controller();
    wait_for_file(&mut controller);
    controller
        .handle()
        .set_property_string("user-data/mpvex/show_text", "Lua says hello")
        .expect("set IPC property");
    let mut commands = Vec::new();
    for _ in 0..80 {
        let _ = controller.poll_events();
        commands.extend(controller.drain_mpvex_commands());
        if !commands.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    assert!(
        commands.contains(&MpvexCommand::ShowText("Lua says hello".to_string())),
        "observed commands: {commands:?}"
    );
}
