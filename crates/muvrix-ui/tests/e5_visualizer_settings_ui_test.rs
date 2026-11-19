//! Phase E5: Audio DSP Expansion, EBU R128 Loudness Normalization & Real-Time 32-Band Spectrum Visualizer UI Tests.

use muvrix_player::equalizer::{
    EbuR128Settings, EqualizerManager, EqualizerSettings, NormalizationMode,
};
use muvrix_player::{
    AutoEqFilter, AutoEqFilterType, AutoEqProfile, SpectrumTelemetry, SpectrumVisualizer,
    SPECTRUM_BANDS_COUNT, SPECTRUM_FREQUENCIES,
};

#[test]
fn test_e5_autoeq_import_export_state_flow() {
    let mut eq_settings = EqualizerSettings::default();

    // Test text preset parsing
    let txt_preset = r#"
Preamp: -4.5 dB
Filter 1: ON PK Fc 28 Hz Gain 6.2 dB Q 1.41
Filter 2: ON LSC Fc 105 Hz Gain 3.5 dB Q 0.71
Filter 3: ON HSC Fc 10000 Hz Gain -2.0 dB Q 0.70
"#;
    let profile = AutoEqProfile::parse_text(txt_preset, Some("Sennheiser HD 600")).unwrap();
    assert_eq!(profile.name, "Sennheiser HD 600");
    assert_eq!(profile.filters.len(), 3);
    assert!((profile.preamp_db - (-4.5)).abs() < 1e-4);

    // Apply profile
    eq_settings.apply_autoeq_profile(profile);
    assert!(eq_settings.autoeq_profile.is_some());
    let autoeq_status_msg = Some((
        format!(
            "Loaded AutoEq profile '{}' (3 filters, Preamp: -4.5 dB)",
            eq_settings.autoeq_profile.as_ref().unwrap().name
        ),
        false,
    ));

    assert_eq!(
        autoeq_status_msg.as_ref().unwrap().0,
        "Loaded AutoEq profile 'Sennheiser HD 600' (3 filters, Preamp: -4.5 dB)"
    );
    assert!(!autoeq_status_msg.as_ref().unwrap().1); // not error

    // Verify filter generation with AutoEq active
    let filter_str = EqualizerManager::build_filter_string(&eq_settings);
    assert!(filter_str.contains("volume=volume=-4.5dB"));
    assert!(filter_str.contains("equalizer=f=28.00:t=q:w=1.410:g=6.2"));
    assert!(filter_str.contains("lowshelf=f=105.00:t=q:w=0.710:g=3.5"));
    assert!(filter_str.contains("highshelf=f=10000.00:t=q:w=0.700:g=-2.0"));

    // Export preset to JSON
    let exported_json = eq_settings
        .autoeq_profile
        .as_ref()
        .unwrap()
        .export_json()
        .unwrap();
    assert!(exported_json.contains("\"name\": \"Sennheiser HD 600\""));
    assert!(exported_json.contains("\"preamp_db\": -4.5"));

    // Re-import exported JSON (round-trip test)
    let reimported = AutoEqProfile::parse_json(&exported_json, None).unwrap();
    assert_eq!(reimported.name, "Sennheiser HD 600");
    assert_eq!(reimported.filters.len(), 3);
    assert!((reimported.preamp_db - (-4.5)).abs() < 1e-4);

    // Clear AutoEq profile
    eq_settings.clear_autoeq_profile();
    assert!(eq_settings.autoeq_profile.is_none());

    // Verify filter reverts to 10-band equalizer
    let reverted_filter = EqualizerManager::build_filter_string(&eq_settings);
    assert!(!reverted_filter.contains("volume=volume=-4.5dB"));
    assert!(!reverted_filter.contains("bass="));
    assert!(!reverted_filter.contains("treble="));
}

#[test]
fn test_e5_autoeq_error_resilience() {
    // 1. Invalid text with non-numeric gain
    let bad_text = "Filter 1: ON PK Fc 1000 Hz Gain INVALID dB Q 1.0";
    let res = AutoEqProfile::parse_text(bad_text, None);
    assert!(res.is_err());

    // 2. NaN values rejected
    let nan_profile = AutoEqProfile::new(
        "Corrupted".to_string(),
        f64::NAN,
        vec![AutoEqFilter {
            filter_type: AutoEqFilterType::Peaking,
            frequency_hz: 1000.0,
            gain_db: 0.0,
            q_factor: 1.0,
            enabled: true,
        }],
    );
    assert!(nan_profile.is_err());

    // 3. Infinite gain rejected
    let inf_filter = AutoEqFilter {
        filter_type: AutoEqFilterType::Peaking,
        frequency_hz: 1000.0,
        gain_db: f64::INFINITY,
        q_factor: 1.0,
        enabled: true,
    };
    let inf_profile = AutoEqProfile::new("Infinite".to_string(), 0.0, vec![inf_filter]);
    assert!(inf_profile.is_err());

    // 4. Invalid JSON syntax
    let bad_json = "{\"name\": \"Incomplete JSON\"";
    let json_res = AutoEqProfile::parse_json(bad_json, None);
    assert!(json_res.is_err());
}

#[test]
fn test_e5_ebu_r128_controls_and_mutual_exclusion() {
    let mut eq = EqualizerSettings::default();

    // Default EBU R128 settings
    assert_eq!(eq.ebu_r128.integrated_loudness, -16.0);
    assert_eq!(eq.ebu_r128.true_peak, -1.5);
    assert_eq!(eq.ebu_r128.loudness_range, 11.0);

    // Filter string under EBU R128 default
    let default_filter = eq.ebu_r128.build_filter_string();
    assert_eq!(default_filter, "loudnorm=I=-16.0:TP=-1.5:LRA=11.0");

    // Clamping limits verification
    let mut custom_ebu = EbuR128Settings {
        enabled: true,
        preset: muvrix_player::EbuR128Preset::Custom,
        integrated_loudness: -100.0, // below min -70.0
        true_peak: 5.0,              // above max 0.0
        loudness_range: 0.2,         // below min 1.0
    };
    assert_eq!(
        custom_ebu.build_filter_string(),
        "loudnorm=I=-70.0:TP=0.0:LRA=1.0"
    );

    custom_ebu.integrated_loudness = 10.0; // above max -5.0
    custom_ebu.true_peak = -20.0; // below min -9.0
    custom_ebu.loudness_range = 99.0; // above max 50.0
    assert_eq!(
        custom_ebu.build_filter_string(),
        "loudnorm=I=-5.0:TP=-9.0:LRA=50.0"
    );

    // Mutual exclusion test: NormalizationMode transitions
    eq.spectrum_visualizer_enabled = false;

    // Mode 1: None
    eq.set_normalization_mode(NormalizationMode::None);
    assert_eq!(eq.effective_normalization_mode(), NormalizationMode::None);
    let str_none = EqualizerManager::build_filter_string(&eq);
    assert!(!str_none.contains("loudnorm"));
    assert!(!str_none.contains("dynaudnorm"));

    // Mode 2: EbuR128
    eq.set_normalization_mode(NormalizationMode::EbuR128);
    assert_eq!(
        eq.effective_normalization_mode(),
        NormalizationMode::EbuR128
    );
    let str_ebu = EqualizerManager::build_filter_string(&eq);
    assert!(str_ebu.contains("loudnorm=I=-16.0:TP=-1.5:LRA=11.0"));
    assert!(!str_ebu.contains("dynaudnorm"));

    // Mode 3: Dynamic Normalizer (dynaudnorm)
    eq.set_normalization_mode(NormalizationMode::Dynamic);
    assert_eq!(
        eq.effective_normalization_mode(),
        NormalizationMode::Dynamic
    );
    let str_dyn = EqualizerManager::build_filter_string(&eq);
    assert!(!str_dyn.contains("loudnorm"));
    assert!(str_dyn.contains("dynaudnorm"));

    // Reset action restores standard EBU R128 values
    eq.ebu_r128 = EbuR128Settings::default();
    assert_eq!(eq.ebu_r128.integrated_loudness, -16.0);
    assert_eq!(eq.ebu_r128.true_peak, -1.5);
    assert_eq!(eq.ebu_r128.loudness_range, 11.0);

    // Preset selection: Broadcast reference
    eq.ebu_r128.reset_to_broadcast_standards();
    assert_eq!(eq.ebu_r128.preset, muvrix_player::EbuR128Preset::Broadcast);
    assert_eq!(eq.ebu_r128.integrated_loudness, -23.0);
    assert_eq!(eq.ebu_r128.true_peak, -2.0);
    assert_eq!(eq.ebu_r128.loudness_range, 7.0);
    assert_eq!(
        eq.ebu_r128.build_filter_string(),
        "loudnorm=I=-23.0:TP=-2.0:LRA=7.0"
    );

    // Preset selection: Streaming defaults
    eq.ebu_r128.reset_to_streaming_defaults();
    assert_eq!(eq.ebu_r128.preset, muvrix_player::EbuR128Preset::Streaming);
    assert_eq!(eq.ebu_r128.integrated_loudness, -16.0);
    assert_eq!(eq.ebu_r128.true_peak, -1.5);
    assert_eq!(eq.ebu_r128.loudness_range, 11.0);
    assert_eq!(
        eq.ebu_r128.build_filter_string(),
        "loudnorm=I=-16.0:TP=-1.5:LRA=11.0"
    );
}

#[test]
fn test_e5_spectrum_visualizer_rendering_and_physics() {
    let mut visualizer = SpectrumVisualizer::new();
    assert_eq!(SPECTRUM_BANDS_COUNT, 32);
    assert_eq!(SPECTRUM_FREQUENCIES.len(), 32);

    // Verify frequency range: from 20 Hz sub-bass to 20 kHz air band
    assert_eq!(SPECTRUM_FREQUENCIES[0], 20.0);
    assert_eq!(SPECTRUM_FREQUENCIES[31], 20000.0);

    // Initial state: all bands zero
    assert_eq!(visualizer.smoothed_bands(), &[0.0; 32]);

    // Frame 1: Stimulate with a 1000 Hz tone (centroid = 1000.0 Hz, RMS = -12 dB)
    let telem = SpectrumTelemetry {
        rms_db: -12.0,
        peak_db: -6.0,
        spectral_centroid: 1000.0,
        spectral_spread: 150.0,
        spectral_rolloff: 3000.0,
        spectral_flatness: 0.1,
        spectral_slope: -0.05,
        is_playing: true,
        is_paused: false,
    };

    visualizer.update(&telem);

    let bands_f1 = *visualizer.smoothed_bands();
    // Band corresponding to 1000 Hz is index 17
    let mid_band = SpectrumVisualizer::bucket_index_for_freq(1000.0);
    assert_eq!(mid_band, 17);
    assert!(
        bands_f1[mid_band] > 0.3,
        "1000Hz band should react strongly to 1000Hz tone"
    );

    // Distant bands (e.g. 20 Hz, 20 kHz) should have lower energy than 1000 Hz band
    assert!(bands_f1[0] < bands_f1[mid_band]);
    assert!(bands_f1[31] < bands_f1[mid_band]);

    // Frame 2: Audio signal goes quiet while playback remains active (silence decay)
    let quiet = SpectrumTelemetry {
        rms_db: -100.0,
        peak_db: -100.0,
        is_playing: true,
        is_paused: false,
        ..Default::default()
    };

    visualizer.update(&quiet);
    let bands_f2 = *visualizer.smoothed_bands();

    // Decay physics: bands decay smoothly without instant drop to 0
    assert!(
        bands_f2[mid_band] < bands_f1[mid_band],
        "Band should decay on quiet audio"
    );
    assert!(
        bands_f2[mid_band] > 0.05,
        "Band should not drop immediately to 0.0 due to release smoothing"
    );

    // Multi-frame decay simulation
    for _ in 0..30 {
        visualizer.update(&quiet);
    }
    let bands_f32 = *visualizer.smoothed_bands();
    assert!(
        bands_f32[mid_band] < 0.01,
        "After 30 frames of quiet audio, band energy should dissipate"
    );

    // Playback pause: smooth pause decay
    visualizer.update(&telem); // re-energize
    let paused = SpectrumTelemetry {
        is_playing: true,
        is_paused: true,
        ..Default::default()
    };
    visualizer.update(&paused);
    let bands_paused = *visualizer.smoothed_bands();
    assert!(bands_paused[mid_band] < bands_f1[mid_band]);

    // Playback stop: immediate reset
    let stopped = SpectrumTelemetry {
        is_playing: false,
        is_paused: false,
        ..Default::default()
    };
    visualizer.update(&stopped);
    assert_eq!(visualizer.smoothed_bands(), &[0.0; 32]);
}

#[test]
fn test_e5_spectrum_filter_inclusion_toggle() {
    let mut eq = EqualizerSettings {
        spectrum_visualizer_enabled: true,
        ..Default::default()
    };

    // When enabled, visualizer passthrough filters must be present
    let enabled_filters = EqualizerManager::build_filter_string(&eq);
    assert!(enabled_filters.contains("@vis:lavfi=[astats=metadata=1:reset=1]"));
    assert!(enabled_filters.contains("@spec:lavfi=[aspectralstats=win_size=8192:overlap=0]"));

    // When disabled, visualizer filters must NOT be injected
    eq.spectrum_visualizer_enabled = false;
    let disabled_filters = EqualizerManager::build_filter_string(&eq);
    assert!(!disabled_filters.contains("@vis:lavfi="));
    assert!(!disabled_filters.contains("@spec:lavfi="));
}
