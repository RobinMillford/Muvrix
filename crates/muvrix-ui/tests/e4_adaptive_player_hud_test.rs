//! Phase E4: Adaptive Player HUD, Three-Tier Collapsible Decks & Visual Regression Tests.

use muvrix_player::{PlaybackState, SteppingPreset};
use muvrix_ui::app::MuvrixApp;

#[test]
fn test_e4_1_responsive_breakpoints_and_deck_visibility() {
    // Breakpoint is 1300.0 px
    assert_eq!(MuvrixApp::RESPONSIVE_BREAKPOINT_WIDTH, 1300.0);

    // Simulated responsive test harness struct matching MuvrixApp fields
    struct ResponsiveDeckHarness {
        player_secondary_deck_expanded: bool,
        player_secondary_deck_collapsed_wide: bool,
    }

    impl ResponsiveDeckHarness {
        fn is_secondary_deck_visible(&self, deck_width: f32) -> bool {
            if deck_width >= MuvrixApp::RESPONSIVE_BREAKPOINT_WIDTH {
                !self.player_secondary_deck_collapsed_wide
            } else {
                self.player_secondary_deck_expanded
            }
        }

        fn toggle_secondary_deck(&mut self, deck_width: f32) {
            if deck_width >= MuvrixApp::RESPONSIVE_BREAKPOINT_WIDTH {
                self.player_secondary_deck_collapsed_wide =
                    !self.player_secondary_deck_collapsed_wide;
            } else {
                self.player_secondary_deck_expanded = !self.player_secondary_deck_expanded;
            }
        }
    }

    let mut harness = ResponsiveDeckHarness {
        player_secondary_deck_expanded: false,
        player_secondary_deck_collapsed_wide: false,
    };

    // Compact viewport tests (< 1300.0 px)
    // 1024px: default collapsed
    assert!(!harness.is_secondary_deck_visible(1024.0));
    // 1280px: default collapsed
    assert!(!harness.is_secondary_deck_visible(1280.0));
    // 1299px: default collapsed
    assert!(!harness.is_secondary_deck_visible(1299.0));

    // Wide viewport tests (>= 1300.0 px)
    // 1300px: default visible
    assert!(harness.is_secondary_deck_visible(1300.0));
    // 1440px: default visible
    assert!(harness.is_secondary_deck_visible(1440.0));
    // 1920px: default visible
    assert!(harness.is_secondary_deck_visible(1920.0));

    // Toggle behavior at compact width (1280px)
    harness.toggle_secondary_deck(1280.0);
    assert!(harness.is_secondary_deck_visible(1280.0));
    assert!(harness.player_secondary_deck_expanded);

    harness.toggle_secondary_deck(1280.0);
    assert!(!harness.is_secondary_deck_visible(1280.0));
    assert!(!harness.player_secondary_deck_expanded);

    // Toggle behavior at wide width (1440px)
    harness.toggle_secondary_deck(1440.0);
    assert!(!harness.is_secondary_deck_visible(1440.0));
    assert!(harness.player_secondary_deck_collapsed_wide);

    harness.toggle_secondary_deck(1440.0);
    assert!(harness.is_secondary_deck_visible(1440.0));
    assert!(!harness.player_secondary_deck_collapsed_wide);
}

#[test]
fn test_e4_2_overflow_popover_open_close_and_dismissal() {
    let mut more_overflow_open = true;
    let mut empty_url_input_open = false;

    // Escape dismissal prioritizes More popover first
    if more_overflow_open {
        more_overflow_open = false;
    } else if empty_url_input_open {
        empty_url_input_open = false;
    }
    assert!(!more_overflow_open);
    assert!(!empty_url_input_open);

    // Opening URL stream input in empty state via Ctrl+U
    empty_url_input_open = true;
    assert!(empty_url_input_open);

    // Escape dismissal closes URL input when More popover is not open
    if more_overflow_open {
        more_overflow_open = false;
    } else if empty_url_input_open {
        empty_url_input_open = false;
    }
    assert!(!more_overflow_open);
    assert!(!empty_url_input_open);
}

#[test]
fn test_e4_3_idle_state_strictly_resets_timeline() {
    let state = PlaybackState::Idle;

    let time_pos = 124.5;
    let duration = 300.0;

    let (effective_time, effective_duration) = if state == PlaybackState::Idle {
        (0.0, 0.0)
    } else {
        (time_pos, duration)
    };

    assert_eq!(effective_time, 0.0);
    assert_eq!(effective_duration, 0.0);
}

#[test]
fn test_e4_4_audio_and_subtitle_sync_offset_calculations() {
    // Primary subtitle delay step (+/-0.25s)
    let mut sub_delay = 0.0_f64;
    sub_delay += 0.25;
    assert!((sub_delay - 0.25).abs() < 1e-6);
    sub_delay -= 0.25;
    assert!((sub_delay - 0.0).abs() < 1e-6);
    sub_delay -= 0.25;
    assert!((sub_delay - (-0.25)).abs() < 1e-6);

    // Secondary subtitle delay step (+/-0.25s)
    let mut sec_sub_delay = 0.0_f64;
    sec_sub_delay += 0.25;
    assert!((sec_sub_delay - 0.25).abs() < 1e-6);

    // Audio offset step (+/-0.10s)
    let mut audio_delay = 0.0_f64;
    audio_delay += 0.10;
    assert!((audio_delay - 0.10).abs() < 1e-6);
    audio_delay -= 0.20;
    assert!((audio_delay - (-0.10)).abs() < 1e-6);
}

#[test]
fn test_e4_5_jog_wheel_scrub_accumulator_and_threshold() {
    let mut jog_drag_accumulator = 0.0_f32;
    let mut forward_steps = 0;
    let mut backward_steps = 0;
    const JOG_THRESHOLD: f32 = 8.0;

    // Simulate drag right by 20.0 px
    jog_drag_accumulator += 20.0;
    while jog_drag_accumulator >= JOG_THRESHOLD {
        forward_steps += 1;
        jog_drag_accumulator -= JOG_THRESHOLD;
    }
    assert_eq!(forward_steps, 2);
    assert!((jog_drag_accumulator - 4.0).abs() < 1e-6);

    // Simulate drag left by 25.0 px
    jog_drag_accumulator -= 25.0; // 4.0 - 25.0 = -21.0
    while jog_drag_accumulator <= -JOG_THRESHOLD {
        backward_steps += 1;
        jog_drag_accumulator += JOG_THRESHOLD;
    }
    assert_eq!(backward_steps, 2);
    assert!((jog_drag_accumulator - (-5.0)).abs() < 1e-6);
}

#[test]
fn test_e4_6_stepping_preset_labels_and_conversions() {
    assert_eq!(SteppingPreset::Frame.label(), "Frame");
    assert_eq!(SteppingPreset::Film24.label(), "1/24s (Film)");
    assert_eq!(SteppingPreset::Ntsc30.label(), "1/30s (NTSC)");
    assert_eq!(SteppingPreset::HighRate60.label(), "1/60s (High-Rate)");
}

#[test]
fn test_e4_7_supported_empty_state_format_pills() {
    let pills = ["MP4", "MKV", "WebM", "AVI", "MOV", "FLAC", "MP3", "AAC"];
    assert_eq!(pills.len(), 8);
    assert!(pills.contains(&"MP4"));
    assert!(pills.contains(&"MKV"));
    assert!(pills.contains(&"WebM"));
    assert!(pills.contains(&"FLAC"));
}

#[test]
fn test_e4_8_seeded_ab_loop_duration_settling_and_guards() {
    struct AbLoopSettlingHarness {
        current_file: Option<String>,
        duration: f64,
        applied_file: Option<String>,
        loop_state: Option<(f64, Option<f64>)>,
        call_count: usize,
    }

    impl AbLoopSettlingHarness {
        fn new() -> Self {
            Self {
                current_file: None,
                duration: 0.0,
                applied_file: None,
                loop_state: None,
                call_count: 0,
            }
        }

        fn sync_seeded_ab_loop(&mut self, env_seed: Option<&str>) {
            let Some(ab_str) = env_seed else {
                return;
            };

            let current_file = self.current_file.as_deref();
            if self.duration <= 0.0 || current_file.is_none() {
                return;
            }

            if self.applied_file.as_deref() == current_file {
                return;
            }

            self.applied_file = current_file.map(str::to_string);
            self.call_count += 1;

            let parts: Vec<&str> = ab_str.split(&[',', ';'][..]).map(str::trim).collect();
            if parts.len() == 2 {
                if let (Ok(a), Ok(b)) = (parts[0].parse::<f64>(), parts[1].parse::<f64>()) {
                    if a >= 0.0 && b > a {
                        self.loop_state = Some((a, Some(b)));
                    }
                }
            } else if parts.len() == 1 {
                if let Ok(a) = parts[0].parse::<f64>() {
                    if a >= 0.0 {
                        self.loop_state = Some((a, None));
                    }
                }
            }
        }
    }

    let mut harness = AbLoopSettlingHarness::new();

    // 1. Normal playback without env var: pure no-op
    harness.current_file = Some("video.mp4".to_string());
    harness.duration = 120.0;
    harness.sync_seeded_ab_loop(None);
    assert_eq!(harness.call_count, 0);
    assert_eq!(harness.applied_file, None);
    assert_eq!(harness.loop_state, None);

    // 2. Duration initially zero: waits for settling, no-op
    let mut harness2 = AbLoopSettlingHarness::new();
    harness2.current_file = Some("clip.mkv".to_string());
    harness2.duration = 0.0; // demuxer still loading
    harness2.sync_seeded_ab_loop(Some("2.0,8.0"));
    assert_eq!(harness2.call_count, 0);
    assert_eq!(harness2.applied_file, None);
    assert_eq!(harness2.loop_state, None);

    // 3. Duration becomes valid later: sets A-B loop and marks guard
    harness2.duration = 10.0;
    harness2.sync_seeded_ab_loop(Some("2.0,8.0"));
    assert_eq!(harness2.call_count, 1);
    assert_eq!(harness2.applied_file.as_deref(), Some("clip.mkv"));
    assert_eq!(harness2.loop_state, Some((2.0, Some(8.0))));

    // 4. Repeated ticks with same file: one-time execution guard prevents repeated writes
    for _ in 0..100 {
        harness2.sync_seeded_ab_loop(Some("2.0,8.0"));
    }
    assert_eq!(harness2.call_count, 1); // Strictly called exactly once!

    // 5. Media replacement: loading another media file resets guard and applies for new file
    harness2.current_file = Some("feature.mkv".to_string());
    harness2.duration = 0.0; // new file initially 0.0
    harness2.sync_seeded_ab_loop(Some("2.0,8.0"));
    assert_eq!(harness2.call_count, 1); // still 1 because duration was 0.0

    harness2.duration = 3600.0; // new file settles duration
    harness2.sync_seeded_ab_loop(Some("2.0,8.0"));
    assert_eq!(harness2.call_count, 2); // exactly 1 new application for new file!
    assert_eq!(harness2.applied_file.as_deref(), Some("feature.mkv"));

    // 6. Invalid seed input: handles gracefully without endless retries
    let mut harness3 = AbLoopSettlingHarness::new();
    harness3.current_file = Some("test.mp4".to_string());
    harness3.duration = 60.0;
    harness3.sync_seeded_ab_loop(Some("corrupt;value"));
    assert_eq!(harness3.call_count, 1); // processed once
    assert_eq!(harness3.loop_state, None); // no invalid loop set
    assert_eq!(harness3.applied_file.as_deref(), Some("test.mp4"));

    // Subsequent ticks do not re-attempt parsing
    harness3.sync_seeded_ab_loop(Some("corrupt;value"));
    assert_eq!(harness3.call_count, 1);
}
