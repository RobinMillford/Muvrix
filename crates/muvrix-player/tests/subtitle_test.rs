//! Subtitle and dual-subtitle collision avoidance integration tests.

use muvrix_player::{AssOverrideMode, MpvHandle, PlaybackController, SubtitleStyling, TrackType};
use std::sync::Arc;

#[test]
fn test_subtitle_ass_override_modes_and_mpv_strings() {
    assert_eq!(AssOverrideMode::Preserve.as_mpv_str(), "no");
    assert_eq!(AssOverrideMode::SafeScale.as_mpv_str(), "scale");
    assert_eq!(AssOverrideMode::Force.as_mpv_str(), "force");
    assert_eq!(AssOverrideMode::Strip.as_mpv_str(), "strip");
}

#[test]
fn test_dual_subtitle_collision_avoidance_and_independent_styling() {
    let handle = Arc::new(MpvHandle::new().expect("Create handle"));
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().expect("Initialize handle");

    let mut controller = PlaybackController::new(handle).expect("Create controller");

    // 1. Verify default styling values
    let defaults = SubtitleStyling::default();
    assert_eq!(
        defaults.pos, 100,
        "Primary subtitles default to bottom (100)"
    );
    assert_eq!(
        defaults.secondary_pos, 10,
        "Secondary subtitles default to top (10)"
    );
    assert_eq!(defaults.ass_override_mode, AssOverrideMode::Preserve);
    assert_eq!(defaults.color, "#FFFFFF");

    // 2. Configure dual-subtitle collision avoidance
    controller
        .configure_dual_subtitle_collision_avoidance()
        .expect("Configure dual subtitle collision avoidance");

    // 3. Apply custom styling with distinct top/bottom positions and SafeScale override
    let custom = SubtitleStyling {
        font: "Cantarell".to_string(),
        font_size: 50,
        color: "#E0E0E0".to_string(),
        pos: 95,
        secondary_pos: 12,
        ass_override_mode: AssOverrideMode::SafeScale,
        ..Default::default()
    };

    controller
        .set_subtitle_styling(custom.clone())
        .expect("Apply custom styling");
    assert_eq!(controller.subtitle_styling(), &custom);

    // 4. Test secondary delay and visibility
    controller
        .set_secondary_sub_delay(0.25)
        .expect("Set secondary sub delay");
    controller
        .set_secondary_sub_visibility(true)
        .expect("Set secondary sub visibility");
    controller
        .set_secondary_sub_visibility(false)
        .expect("Hide secondary sub");

    // 5. Reset styling
    controller
        .reset_subtitle_styling()
        .expect("Reset subtitle styling");
    assert_eq!(controller.subtitle_styling(), &SubtitleStyling::default());
}

#[test]
fn test_dual_track_subtitle_loading_and_selection() {
    let handle = Arc::new(MpvHandle::new().expect("Create handle"));
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().expect("Initialize handle");

    let mut controller = PlaybackController::new(handle).expect("Create controller");

    // Create temporary SRT and ASS subtitle files
    let tmp_dir = std::env::temp_dir().join(format!("muvrix_sub_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp_dir);
    let srt_path = tmp_dir.join("primary.srt");
    let ass_path = tmp_dir.join("secondary.ass");

    std::fs::write(
        &srt_path,
        "1\n00:00:00,000 --> 00:00:05,000\nPrimary Subtitle Line\n\n",
    )
    .expect("write srt");

    std::fs::write(
        &ass_path,
        "[Script Info]\nTitle: Test\nScriptType: v4.00+\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,,Secondary Subtitle Line\n",
    )
    .expect("write ass");

    // Load media fixture
    let media_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/fixtures/test_video.mp4");
    if media_path.exists() {
        controller
            .load_file(media_path.to_str().unwrap())
            .expect("load media");

        // Wait briefly for file to load
        for _ in 0..10 {
            let _ = controller.poll_events();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        // Add primary subtitle track
        controller
            .add_subtitle_file(srt_path.to_str().unwrap(), true)
            .expect("add primary subtitle");

        // Add secondary subtitle track
        controller
            .add_subtitle_file(ass_path.to_str().unwrap(), false)
            .expect("add secondary subtitle");

        // Poll events to let tracks register
        for _ in 0..10 {
            let _ = controller.poll_events();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        // Configure collision avoidance and verify dual-track properties
        controller
            .configure_dual_subtitle_collision_avoidance()
            .expect("configure collision avoidance");

        let tracks = controller.tracks_by_type(TrackType::Subtitle);
        if tracks.len() >= 2 {
            let sub1_id = tracks[0].id;
            let sub2_id = tracks[1].id;
            controller
                .set_subtitle_track(Some(sub1_id))
                .expect("set primary sub");
            controller
                .set_secondary_sub_track(Some(sub2_id))
                .expect("set secondary sub");

            for _ in 0..10 {
                let _ = controller.poll_events();
                std::thread::sleep(std::time::Duration::from_millis(20));
            }

            let primary_sid = controller
                .handle()
                .get_property_string("sid")
                .ok()
                .and_then(|s| s.parse::<i64>().ok());
            let secondary_sid = controller
                .handle()
                .get_property_string("secondary-sid")
                .ok()
                .and_then(|s| s.parse::<i64>().ok());

            assert_eq!(primary_sid, Some(sub1_id));
            assert_eq!(secondary_sid, Some(sub2_id));

            // Verify independent secondary subtitle visibility
            controller
                .set_secondary_sub_visibility(false)
                .expect("hide secondary sub");
            controller
                .set_secondary_sub_visibility(true)
                .expect("show secondary sub");

            // Disable secondary subtitle
            controller
                .set_secondary_sub_track(None)
                .expect("disable secondary sub");
            assert_eq!(controller.secondary_sub_track_id(), None);
        }
    }

    let _ = std::fs::remove_dir_all(&tmp_dir);
}
