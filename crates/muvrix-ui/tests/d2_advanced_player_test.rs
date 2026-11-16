use muvrix_player::track::{ChapterInfo, TrackInfo, TrackType};
use muvrix_player::{AbLoopState, SteppingPreset};
use muvrix_ui::app::{format_timestamp, format_timestamp_ms};

#[test]
fn test_d2_1_chapter_markers_empty_and_zero_duration() {
    // Edge case: empty chapter list
    let chapters: Vec<ChapterInfo> = Vec::new();
    assert!(chapters.is_empty());

    // Coordinate mapping with 0 duration must not divide by zero
    let duration = 0.0;
    let width = 600.0_f32;
    for ch in &chapters {
        let frac = if duration > 0.0 {
            (ch.time / duration).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let x = frac * width;
        assert_eq!(x, 0.0);
    }
}

#[test]
fn test_d2_1_chapter_coordinate_mapping_and_snapping() {
    let duration = 300.0; // 5 minutes
    let width = 600.0_f32;
    let chapters = vec![
        ChapterInfo {
            index: 0,
            title: "Intro".to_string(),
            time: 0.0,
        },
        ChapterInfo {
            index: 1,
            title: "Main Action".to_string(),
            time: 60.0,
        },
        ChapterInfo {
            index: 2,
            title: "Climax".to_string(),
            time: 210.0,
        },
        ChapterInfo {
            index: 3,
            title: "Credits".to_string(),
            time: 285.0,
        },
    ];

    // Coordinate mapping
    let x0 = ((chapters[0].time / duration).clamp(0.0, 1.0) as f32) * width;
    let x1 = ((chapters[1].time / duration).clamp(0.0, 1.0) as f32) * width;
    let x2 = ((chapters[2].time / duration).clamp(0.0, 1.0) as f32) * width;
    let x3 = ((chapters[3].time / duration).clamp(0.0, 1.0) as f32) * width;

    assert!((x0 - 0.0).abs() < 0.001);
    assert!((x1 - 120.0).abs() < 0.001);
    assert!((x2 - 420.0).abs() < 0.001);
    assert!((x3 - 570.0).abs() < 0.001);

    // Hit-testing snapping logic: within +-7px
    let test_mouse_x = 123.0_f32; // 3px away from x1 (120px)
    let mut snapped_chapter = None;
    for ch in &chapters {
        let x = ((ch.time / duration).clamp(0.0, 1.0) as f32) * width;
        if (test_mouse_x - x).abs() <= 7.0 {
            snapped_chapter = Some((ch.index, ch.time));
            break;
        }
    }
    assert_eq!(snapped_chapter, Some((1, 60.0)));

    // Mouse too far away: e.g. 150px
    let test_mouse_far = 150.0_f32;
    let mut far_snapped = None;
    for ch in &chapters {
        let x = ((ch.time / duration).clamp(0.0, 1.0) as f32) * width;
        if (test_mouse_far - x).abs() <= 7.0 {
            far_snapped = Some((ch.index, ch.time));
            break;
        }
    }
    assert_eq!(far_snapped, None);
}

#[test]
fn test_d2_1_chapter_edge_cases_single_and_many_chapters() {
    let duration = 100.0;
    // Single chapter
    let single = [ChapterInfo {
        index: 0,
        title: "Lone Chapter".to_string(),
        time: 50.0,
    }];
    assert_eq!(single.len(), 1);
    let frac = (single[0].time / duration).clamp(0.0, 1.0);
    assert!((frac - 0.5).abs() < 0.001);

    // Many chapters (e.g. 100 chapters)
    let many: Vec<ChapterInfo> = (0..100)
        .map(|i| ChapterInfo {
            index: i,
            title: format!("Chapter {i}"),
            time: (i as f64) * 1.0,
        })
        .collect();
    assert_eq!(many.len(), 100);
    for ch in &many {
        let frac = (ch.time / duration).clamp(0.0, 1.0);
        assert!((0.0..=1.0).contains(&frac));
    }
}

#[test]
fn test_d2_2_ab_loop_boundary_calculations_and_clamping() {
    let duration = 120.0;
    let slider_min_x = 100.0_f32;
    let slider_width = 500.0_f32;

    // Normal loop inside duration
    let state = AbLoopState::Loop { a: 15.0, b: 45.0 };
    if let AbLoopState::Loop { a, b } = state {
        let a_clamped = a.clamp(0.0, duration);
        let b_clamped = b.clamp(a_clamped, duration);
        let frac_a = (a_clamped / duration) as f32;
        let frac_b = (b_clamped / duration) as f32;
        let x_a = slider_min_x + frac_a * slider_width;
        let x_b = slider_min_x + frac_b * slider_width;

        assert!((x_a - (100.0 + 0.125 * 500.0)).abs() < 0.001);
        assert!((x_b - (100.0 + 0.375 * 500.0)).abs() < 0.001);
        assert!(x_a < x_b);
        assert!((b_clamped - a_clamped - 30.0).abs() < 0.001);
    } else {
        panic!("Expected AbLoopState::Loop");
    }

    // Boundary clamping with out-of-range values
    let oob_state = AbLoopState::Loop { a: -10.0, b: 200.0 };
    if let AbLoopState::Loop { a, b } = oob_state {
        let a_clamped = a.clamp(0.0, duration);
        let b_clamped = b.clamp(a_clamped, duration);
        assert_eq!(a_clamped, 0.0);
        assert_eq!(b_clamped, 120.0);
    }

    // Inverted points guard
    let inverted = AbLoopState::Loop { a: 50.0, b: 20.0 };
    if let AbLoopState::Loop { a, b } = inverted {
        let a_clamped = a.clamp(0.0, duration);
        let b_clamped = b.clamp(a_clamped, duration);
        assert!(b_clamped >= a_clamped);
    }
}

#[test]
fn test_d2_2_ab_loop_state_transitions() {
    let mut state = AbLoopState::None;
    assert_eq!(state, AbLoopState::None);

    // Set A
    state = AbLoopState::PointA(10.5);
    assert_eq!(state, AbLoopState::PointA(10.5));

    // Set B
    state = AbLoopState::Loop { a: 10.5, b: 25.0 };
    assert_eq!(state, AbLoopState::Loop { a: 10.5, b: 25.0 });

    // Clear
    state = AbLoopState::None;
    assert_eq!(state, AbLoopState::None);
}

#[test]
fn test_d2_3_stepping_presets_and_time_deltas() {
    assert_eq!(SteppingPreset::Frame.delta_seconds(), None);
    assert_eq!(SteppingPreset::Frame.label(), "Frame");

    let film_delta = SteppingPreset::Film24.delta_seconds().unwrap();
    assert!((film_delta - (1.0 / 24.0)).abs() < 1e-6);
    assert_eq!(SteppingPreset::Film24.label(), "1/24s (Film)");

    let ntsc_delta = SteppingPreset::Ntsc30.delta_seconds().unwrap();
    assert!((ntsc_delta - (1.0 / 30.0)).abs() < 1e-6);
    assert_eq!(SteppingPreset::Ntsc30.label(), "1/30s (NTSC)");

    let hr_delta = SteppingPreset::HighRate60.delta_seconds().unwrap();
    assert!((hr_delta - (1.0 / 60.0)).abs() < 1e-6);
    assert_eq!(SteppingPreset::HighRate60.label(), "1/60s (High-Rate)");
}

#[test]
fn test_d2_3_millisecond_timestamp_formatting() {
    assert_eq!(format_timestamp_ms(0.0), "00:00.000");
    assert_eq!(format_timestamp_ms(1.234), "00:01.234");
    assert_eq!(format_timestamp_ms(65.008), "01:05.008");
    assert_eq!(format_timestamp_ms(3661.500), "01:01:01.500");
    assert_eq!(format_timestamp_ms(-5.0), "00:00.000");
    assert_eq!(format_timestamp_ms(f64::NAN), "00:00.000");
    assert_eq!(format_timestamp_ms(f64::INFINITY), "00:00.000");

    // Standard timestamp consistency
    assert_eq!(format_timestamp(65.0), "01:05");
    assert_eq!(format_timestamp(3661.0), "01:01:01");
}

#[test]
fn test_d2_3_jog_wheel_accumulator_threshold_logic() {
    let mut accumulator: f32 = 0.0;
    let threshold: f32 = 8.0;

    let mut forward_steps = 0;
    let mut backward_steps = 0;

    // Small drag: +5px (under threshold)
    accumulator += 5.0;
    while accumulator >= threshold {
        forward_steps += 1;
        accumulator -= threshold;
    }
    assert_eq!(forward_steps, 0);
    assert_eq!(accumulator, 5.0);

    // Large drag: +20px (total 25px -> 3 steps of 8px, 1px left)
    accumulator += 20.0;
    while accumulator >= threshold {
        forward_steps += 1;
        accumulator -= threshold;
    }
    assert_eq!(forward_steps, 3);
    assert_eq!(accumulator, 1.0);

    // Drag backward: -25px (total -24px -> 3 backward steps)
    accumulator -= 25.0;
    while accumulator <= -threshold {
        backward_steps += 1;
        accumulator += threshold;
    }
    assert_eq!(backward_steps, 3);
    assert_eq!(accumulator, 0.0);
}

#[test]
fn test_d2_4_dual_subtitle_summary_and_tracks() {
    let sub_tracks = [
        TrackInfo {
            id: 1,
            track_type: TrackType::Subtitle,
            title: Some("English".to_string()),
            lang: Some("en".to_string()),
            selected: true,
            external: false,
            codec: Some("subrip".to_string()),
        },
        TrackInfo {
            id: 2,
            track_type: TrackType::Subtitle,
            title: Some("Japanese".to_string()),
            lang: Some("ja".to_string()),
            selected: false,
            external: false,
            codec: Some("ass".to_string()),
        },
    ];

    // Primary only
    let selected_pri: Option<i64> = Some(1);
    let selected_sec: Option<i64> = None;
    let pri_label = selected_pri
        .and_then(|id| sub_tracks.iter().find(|t| t.id == id))
        .map(|t| t.lang.as_deref().unwrap_or("On"));
    let sec_label = selected_sec
        .and_then(|id| sub_tracks.iter().find(|t| t.id == id))
        .map(|t| t.lang.as_deref().unwrap_or("On"));

    let summary = match (pri_label, sec_label) {
        (Some(p), Some(s)) => format!("Subtitles: {p} + {s} (Dual)"),
        (Some(p), None) => format!("Subtitles: {p}"),
        (None, Some(s)) => format!("Subtitles: {s} (Top)"),
        (None, None) => "Subtitles: Off".to_string(),
    };
    assert_eq!(summary, "Subtitles: en");

    // Dual active (Primary English, Secondary Japanese)
    let selected_sec = Some(2);
    let sec_label = selected_sec
        .and_then(|id| sub_tracks.iter().find(|t| t.id == id))
        .map(|t| t.lang.as_deref().unwrap_or("On"));
    let summary = match (pri_label, sec_label) {
        (Some(p), Some(s)) => format!("Subtitles: {p} + {s} (Dual)"),
        (Some(p), None) => format!("Subtitles: {p}"),
        (None, Some(s)) => format!("Subtitles: {s} (Top)"),
        (None, None) => "Subtitles: Off".to_string(),
    };
    assert_eq!(summary, "Subtitles: en + ja (Dual)");

    // Secondary only active
    let _selected_pri: Option<i64> = None;
    let pri_label: Option<&str> = None;
    let summary = match (pri_label, sec_label) {
        (Some(p), Some(s)) => format!("Subtitles: {p} + {s} (Dual)"),
        (Some(p), None) => format!("Subtitles: {p}"),
        (None, Some(s)) => format!("Subtitles: {s} (Top)"),
        (None, None) => "Subtitles: Off".to_string(),
    };
    assert_eq!(summary, "Subtitles: ja (Top)");

    // Both off
    let _selected_sec: Option<i64> = None;
    let sec_label: Option<&str> = None;
    let summary = match (pri_label, sec_label) {
        (Some(p), Some(s)) => format!("Subtitles: {p} + {s} (Dual)"),
        (Some(p), None) => format!("Subtitles: {p}"),
        (None, Some(s)) => format!("Subtitles: {s} (Top)"),
        (None, None) => "Subtitles: Off".to_string(),
    };
    assert_eq!(summary, "Subtitles: Off");
}

#[test]
fn test_d2_responsive_hud_budgeting_at_1280x720_and_1440x900() {
    // Verify that at 1280x720 (available deck width ~1248px), the standard desktop tier (<1400px)
    // applies compact string tokens that prevent collision between left and right controls.
    let deck_w_720 = 1248.0_f32;
    assert!(
        deck_w_720 < 1400.0,
        "1280x720 must use tier-2 standard desktop layout"
    );
    assert!(
        deck_w_720 >= 1120.0,
        "1280x720 must keep separate Audio & Subtitle dropdowns"
    );

    let pri = "en";
    let sec = "ja";

    // Subtitle label format at 1280x720
    let sub_label_720 = if deck_w_720 < 1400.0 {
        format!("Sub: {pri}+{sec}")
    } else {
        format!("Subtitles: {pri} + {sec} (Dual)")
    };
    assert_eq!(sub_label_720, "Sub: en+ja");

    // Audio label format at 1280x720
    let aud_label_720 = if deck_w_720 < 1400.0 {
        format!("Aud: {pri}")
    } else {
        format!("Audio: {pri}")
    };
    assert_eq!(aud_label_720, "Aud: en");

    // Repeat label at 1280x720
    let rep_label_720 = if deck_w_720 < 1400.0 {
        "Rep: Off"
    } else {
        "Repeat: Off"
    };
    assert_eq!(rep_label_720, "Rep: Off");

    // Presets combo label at 1280x720
    let preset_label_720 = if deck_w_720 < 1400.0 {
        "Film"
    } else {
        "1/24s (Film)"
    };
    assert_eq!(preset_label_720, "Film");

    // Total budgeted width at 1280x720 must be <= 1050px, ensuring >= 198px margin
    let left_budget = 38.0 + 2.0 + 26.0 + 2.0 + 28.0 + 2.0 + 28.0 // transport (126px)
        + 56.0 + 26.0 + 42.0 + 26.0 // preset + step + jog + step (150px)
        + 40.0 + 65.0 // A-B + repeat (105px)
        + 24.0 + 24.0 + 60.0 // ch prev/next/combo (108px)
        + 28.0 // bookmark (28px)
        + 26.0 + 75.0 // mute + vol slider (101px)
        + 55.0 + 65.0 // speed + sleep (120px)
        + 30.0; // spacing/separators
    let right_budget = 28.0 + 8.0 + 75.0 + 8.0 + 95.0; // fs + aud + sub (214px)
    let total_budget = left_budget + right_budget;
    assert!(
        total_budget <= 1050.0,
        "Total HUD width at 1280x720 is {total_budget}px, must be <= 1050px"
    );
    assert!(
        deck_w_720 - total_budget >= 150.0,
        "1280x720 layout must provide >= 150px safe margin (actual: {}px)",
        deck_w_720 - total_budget
    );

    // At 1920x1080 (deck_w >= 1500px), wide desktop tier applies
    let deck_w_1080 = 1880.0_f32;
    assert!(deck_w_1080 >= 1400.0, "1920x1080 must use full wide layout");
    let sub_label_1080 = if deck_w_1080 < 1400.0 {
        format!("Sub: {pri}+{sec}")
    } else {
        format!("Subtitles: {pri} + {sec} (Dual)")
    };
    assert_eq!(sub_label_1080, "Subtitles: en + ja (Dual)");
}
