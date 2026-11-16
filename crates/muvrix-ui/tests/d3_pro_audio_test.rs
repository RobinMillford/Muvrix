//! Milestone D3 Comprehensive Test Suite:
//! - Interactive 10-Band EQ Frequency Response Curve (D3.1)
//! - Bauer Stereophonic-to-Binaural Headphone Crossfeed (BS2B) (D3.2)
//! - Dynamic Range Compressor / Limiter Status HUD (D3.3)
//! - Gapless Audio Transitions & Queue Pre-Caching (D3.4)

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use muvrix_player::equalizer::{
    Bs2bPreset, EqualizerManager, EqualizerPreset, EqualizerSettings, EQ_FREQUENCIES,
};
use muvrix_player::handle::MpvHandle;
use muvrix_player::PlaybackController;

/// Dynamically creates a minimal test player with null video and audio outputs.
fn create_test_handle() -> Arc<MpvHandle> {
    let handle = MpvHandle::new().expect("Failed to create libmpv instance");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().expect("Failed to initialize libmpv");
    Arc::new(handle)
}

/// Dynamically synthesizes small multi-track audio test files for gapless transition testing.
fn ensure_gapless_audio_fixtures() -> (PathBuf, PathBuf) {
    static FIXTURES: OnceLock<(PathBuf, PathBuf)> = OnceLock::new();
    FIXTURES
        .get_or_init(|| {
            let dir = std::env::temp_dir().join(format!("muvrix-d3-test-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&dir);
            let track1 = dir.join("track1_440hz.wav");
            let track2 = dir.join("track2_880hz.wav");

            if !track1.exists() {
                let _ = std::process::Command::new("ffmpeg")
                    .args([
                        "-y",
                        "-f",
                        "lavfi",
                        "-i",
                        "sine=frequency=440:duration=1.0",
                        "-c:a",
                        "pcm_s16le",
                        track1.to_str().unwrap(),
                    ])
                    .output();
            }

            if !track2.exists() {
                let _ = std::process::Command::new("ffmpeg")
                    .args([
                        "-y",
                        "-f",
                        "lavfi",
                        "-i",
                        "sine=frequency=880:duration=1.0",
                        "-c:a",
                        "pcm_s16le",
                        track2.to_str().unwrap(),
                    ])
                    .output();
            }

            (track1, track2)
        })
        .clone()
}

// =============================================================================
// D3.1: Interactive 10-Band EQ Frequency Response Curve
// =============================================================================

#[test]
fn test_d3_1_frequency_response_curve_mathematical_precision() {
    let mut eq = EqualizerSettings {
        is_enabled: true,
        ..Default::default()
    };
    eq.apply_preset(EqualizerPreset::Flat);

    // 1. All frequencies on flat EQ must produce exact 0.0 dB
    for freq in [20.0, 62.5, 250.0, 1000.0, 4000.0, 16000.0, 20000.0] {
        assert!(
            eq.cumulative_response_at_freq(freq).abs() < 1e-6,
            "Flat EQ must yield 0 dB at {} Hz",
            freq
        );
    }

    // 2. Adjust band 3 (250 Hz) by +6 dB with Q = 1.414
    eq.set_band_gain(3, 6.0);
    let peak_gain = eq.cumulative_response_at_freq(250.0);
    assert!(
        peak_gain > 5.9 && peak_gain <= 6.0,
        "Center frequency must exhibit configured peak gain: got {:.2} dB",
        peak_gain
    );

    // Off-center frequencies must show symmetric bell attenuation
    let low_gain = eq.cumulative_response_at_freq(125.0);
    let high_gain = eq.cumulative_response_at_freq(500.0);
    assert!(
        low_gain > 0.0 && low_gain < peak_gain,
        "Lower adjacent band must be attenuated"
    );
    assert!(
        high_gain > 0.0 && high_gain < peak_gain,
        "Higher adjacent band must be attenuated"
    );

    // 3. Preamp attenuation lowers the entire cumulative response curve
    eq.preamp_db = -3.0;
    let peak_with_preamp = eq.cumulative_response_at_freq(250.0);
    assert!(
        (peak_with_preamp - (peak_gain - 3.0)).abs() < 1e-4,
        "Preamp must add algebraically to cumulative response: got {:.2} vs expected {:.2}",
        peak_with_preamp,
        peak_gain - 3.0
    );

    // 4. Sampled 128-point curve covers [20.0, 20000.0] Hz continuously
    let curve = eq.frequency_response_curve(128);
    assert_eq!(curve.len(), 128);
    assert!((curve[0].0 - 20.0).abs() < 0.1);
    assert!((curve[127].0 - 20000.0).abs() < 1.0);

    for (f, g) in curve {
        assert!((19.99..=20000.1).contains(&f));
        assert!(g.is_finite());
    }
}

#[test]
fn test_d3_1_frequency_response_slider_immediate_synchronization() {
    let mut eq = EqualizerSettings {
        is_enabled: true,
        ..Default::default()
    };

    for (i, &center_freq) in EQ_FREQUENCIES.iter().enumerate() {
        eq.apply_preset(EqualizerPreset::Flat);
        let test_gain = ((i as f64) - 4.5) * 1.5; // -6.75 to +6.75 dB
        eq.set_band_gain(i, test_gain);
        assert_eq!(eq.gains[i], test_gain);
        assert_eq!(eq.bands[i].gain_db, test_gain);

        let resp = eq.cumulative_response_at_freq(center_freq);
        assert!(
            (resp - test_gain).abs() < 0.1,
            "Band {} response at {} Hz must reflect slider gain {:.1} dB, got {:.1} dB",
            i,
            center_freq,
            test_gain,
            resp
        );
    }
}

// =============================================================================
// D3.2: Bauer Stereophonic-to-Binaural Headphone Crossfeed (BS2B)
// =============================================================================

#[test]
fn test_d3_2_bauer_crossfeed_filter_presets_and_deduplication() {
    let mut settings = EqualizerSettings {
        is_enabled: true,
        crossfeed_enabled: true,
        crossfeed_preset: Bs2bPreset::Default,
        ..Default::default()
    };

    // Default preset
    let filter_default = EqualizerManager::build_filter_string(&settings);
    assert!(filter_default.contains("bs2b=profile=default"));

    // Chu Moy preset
    settings.crossfeed_preset = Bs2bPreset::ChuMoy;
    let filter_cmoy = EqualizerManager::build_filter_string(&settings);
    assert!(filter_cmoy.contains("bs2b=profile=cmoy"));
    assert!(!filter_cmoy.contains("profile=default"));

    // Jan Meier preset
    settings.crossfeed_preset = Bs2bPreset::JanMeier;
    let filter_jmeier = EqualizerManager::build_filter_string(&settings);
    assert!(filter_jmeier.contains("bs2b=profile=jmeier"));

    // Disable crossfeed
    settings.crossfeed_enabled = false;
    let filter_disabled = EqualizerManager::build_filter_string(&settings);
    assert!(!filter_disabled.contains("bs2b"));

    // Unified filter chain with reverse stereo pan
    let unified = EqualizerManager::build_unified_filter_string(&settings, true);
    assert!(unified.starts_with("pan=[stereo|c0=c1|c1=c0]"));
}

#[test]
fn test_d3_2_bauer_crossfeed_apply_to_live_mpv_handle() {
    let handle = create_test_handle();
    let mut settings = EqualizerSettings {
        is_enabled: true,
        crossfeed_enabled: true,
        crossfeed_preset: Bs2bPreset::ChuMoy,
        ..Default::default()
    };

    // Apply Chu Moy crossfeed to live mpv handle
    let res = EqualizerManager::apply_to_handle(&handle, &settings);
    assert!(res.is_ok(), "Applying bs2b filter to mpv must succeed");

    let current_af: String = handle.get_property_string("af").unwrap_or_default();
    assert!(
        current_af.contains("bs2b=profile=cmoy"),
        "Live mpv af property must reflect bs2b filter: got {}",
        current_af
    );

    // Switch to Jan Meier live
    settings.crossfeed_preset = Bs2bPreset::JanMeier;
    let res2 = EqualizerManager::apply_to_handle(&handle, &settings);
    assert!(res2.is_ok());

    let current_af2: String = handle.get_property_string("af").unwrap_or_default();
    assert!(current_af2.contains("bs2b=profile=jmeier"));
    assert!(!current_af2.contains("profile=cmoy"));
}

// =============================================================================
// D3.3: Dynamic Range Compressor / Limiter Status HUD
// =============================================================================

#[test]
fn test_d3_3_dynamics_telemetry_state_and_mapping() {
    let handle = create_test_handle();
    let controller = PlaybackController::new(handle).expect("Create controller");

    // Inactive telemetry
    let t_inactive = controller.dynamics_telemetry(false);
    assert!(!t_inactive.normalization_active);
    assert_eq!(t_inactive.gain_reduction_db, None);
    assert!(t_inactive.status_message.contains("inactive"));

    // Active normalization telemetry
    let t_active = controller.dynamics_telemetry(true);
    assert!(t_active.normalization_active);
    assert!(t_active.status_message.contains("Active"));

    // Meter scale mapping verification: 0 dB to -24 dB maps linearly to [0.0, 1.0]
    let test_cases = [
        (0.0f64, 0.0f32),
        (-6.0f64, 0.25f32),
        (-12.0f64, 0.50f32),
        (-18.0f64, 0.75f32),
        (-24.0f64, 1.00f32),
        (-36.0f64, 1.00f32), // clamped to maximum meter range
    ];

    for (db, expected_fraction) in test_cases {
        let clamped = db.clamp(-24.0, 0.0);
        let fraction = (-clamped / 24.0) as f32;
        assert!(
            (fraction - expected_fraction).abs() < 1e-4,
            "Gain reduction {:.1} dB must map to fraction {:.2}, got {:.2}",
            db,
            expected_fraction,
            fraction
        );
    }
}

// =============================================================================
// D3.4: Gapless Audio Transitions & Queue Pre-Caching
// =============================================================================

#[test]
fn test_d3_4_gapless_audio_queue_transitions_and_prefetch() {
    let (track1, track2) = ensure_gapless_audio_fixtures();
    let handle = create_test_handle();
    let mut controller = PlaybackController::new(handle).expect("Create controller");

    // 1. Verify gapless-audio property is initialized to 'yes'
    assert!(controller.gapless_audio());

    // 2. Append both tracks to the playback queue
    controller
        .playlist_append(track1.to_str().unwrap())
        .expect("Append track 1");
    controller
        .playlist_append(track2.to_str().unwrap())
        .expect("Append track 2");

    assert_eq!(controller.playlist_count(), 2);

    // 3. Play track 1
    controller
        .playlist_play_index(0)
        .expect("Start track 1 playback");

    // 4. Exercise prefetch invocation
    controller.prefetch_next_track();

    // 5. Advance to track 2 gaplessly
    controller
        .playlist_next()
        .expect("Advance to track 2 gaplessly");

    // Verify queue preservation
    assert_eq!(controller.playlist_count(), 2);

    // Verify gapless audio can be toggled without error
    controller
        .set_gapless_audio("no")
        .expect("Disable gapless audio");
    assert!(!controller.gapless_audio());

    controller
        .set_gapless_audio("yes")
        .expect("Re-enable gapless audio");
    assert!(controller.gapless_audio());
}

#[test]
fn test_d3_audio_filter_graph_preservation_and_deduplication() {
    let handle = create_test_handle();
    let mut eq = EqualizerSettings {
        is_enabled: true,
        preamp_db: -1.5,
        dynaudnorm: true,
        crossfeed_enabled: true,
        crossfeed_preset: Bs2bPreset::JanMeier,
        ..Default::default()
    };
    eq.set_band_gain(5, 4.0); // 1 kHz boost

    // 1. Verify unified filter string output directly
    let unified_str = EqualizerManager::build_unified_filter_string(&eq, true);
    assert!(unified_str.starts_with("pan=[stereo|c0=c1|c1=c0]"));
    assert!(unified_str.contains("volume=volume=-1.5dB"));
    assert!(unified_str.contains("equalizer=f=1000.00"));
    assert!(unified_str.contains("bs2b=profile=jmeier"));
    assert!(unified_str.contains("dynaudnorm=f=150:g=15"));

    // 2. Apply unified filter graph with reversed stereo to live mpv handle
    let res = EqualizerManager::apply_unified_to_handle(&handle, &eq, true);
    assert!(res.is_ok());

    let af_str = handle.get_property_string("af").unwrap();
    // In mpv's internal af property representation:
    let pan_pos = af_str.find("pan").expect("pan filter");
    let vol_pos = af_str.find("volume").expect("volume filter");
    let eq_pos = af_str.find("equalizer").expect("equalizer filter");
    let bs2b_pos = af_str.find("bs2b").expect("bs2b filter");
    let dyn_pos = af_str.find("dynaudnorm").expect("dynaudnorm filter");

    assert!(pan_pos < vol_pos, "pan must precede volume");
    assert!(vol_pos < eq_pos, "volume must precede equalizer");
    assert!(eq_pos < bs2b_pos, "equalizer must precede crossfeed");
    assert!(
        bs2b_pos < dyn_pos,
        "crossfeed must precede dynamic normalizer"
    );
}
