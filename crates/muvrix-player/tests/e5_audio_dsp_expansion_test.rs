//! Phase E5 Audio DSP Expansion, EBU R128 Loudness Normalization,
//! AutoEq Preset Import/Export & 32-Band Spectrum Visualizer Test Suite.

use muvrix_player::autoeq::{AutoEqError, AutoEqFilter, AutoEqFilterType, AutoEqProfile};
use muvrix_player::equalizer::{
    Bs2bPreset, EbuR128Preset, EbuR128Settings, EqualizerManager, EqualizerPreset,
    EqualizerSettings, NormalizationMode,
};
use muvrix_player::spectrum::{
    SpectrumTelemetry, SpectrumVisualizer, SPECTRUM_BANDS_COUNT, SPECTRUM_FREQUENCIES,
};

// =============================================================================
// TEST 1: AutoEq Preset Parser & Mapping
// =============================================================================

#[test]
fn test_autoeq_preset_parser_and_mapping() {
    let apo_text = r#"
# AutoEq export for Sennheiser HD 650 (Harman Over-Ear target)
Preamp: -6.8 dB
Filter 1: ON PK Fc 28.0 Hz Gain 6.2 dB Q 1.41
Filter 2: ON PK Fc 105.0 Hz Gain -3.1 dB Q 0.70
Filter 3: ON LSC Fc 105.0 Hz Gain 5.5 dB Q 0.70
Filter 4: ON PK Fc 856.0 Hz Gain -4.5 dB Q 1.10
Filter 5: ON HSC Fc 10000.0 Hz Gain -2.0 dB Q 0.70
"#;

    let profile = AutoEqProfile::parse_text(apo_text, Some("Sennheiser HD 650"))
        .expect("Must parse valid text preset");
    assert_eq!(profile.name, "Sennheiser HD 650");
    assert!((profile.preamp_db - (-6.8)).abs() < 1e-4);
    assert_eq!(profile.filters.len(), 5);

    // Verify filter 1
    assert_eq!(profile.filters[0].filter_type, AutoEqFilterType::Peaking);
    assert!((profile.filters[0].frequency_hz - 28.0).abs() < 1e-3);
    assert!((profile.filters[0].gain_db - 6.2).abs() < 1e-3);
    assert!((profile.filters[0].q_factor - 1.41).abs() < 1e-3);
    assert!(profile.filters[0].enabled);

    // Verify shelf filters
    assert_eq!(profile.filters[2].filter_type, AutoEqFilterType::LowShelf);
    assert_eq!(profile.filters[4].filter_type, AutoEqFilterType::HighShelf);

    // Verify 10-band mapping
    let mapped = profile.map_to_10_bands();
    assert_eq!(mapped.len(), 10);
    // At low frequencies (31.25 Hz band), positive boost should be reflected
    assert!(mapped[0] > 0.0);

    // Verify application to EqualizerSettings
    let mut eq_settings = EqualizerSettings::default();
    eq_settings.apply_autoeq_profile(profile);
    assert!(eq_settings.is_enabled);
    assert_eq!(eq_settings.active_preset, EqualizerPreset::Custom);
    assert!((eq_settings.preamp_db - (-6.8)).abs() < 1e-4);
    assert!(eq_settings.autoeq_profile.is_some());

    // Verify filter string construction with AutoEq
    let filter_str = EqualizerManager::build_filter_string(&eq_settings);
    assert!(filter_str.contains("volume=volume=-6.8dB"));
    assert!(filter_str.contains("equalizer=f=28.00:t=q:w=1.410:g=6.2"));
    assert!(filter_str.contains("lowshelf=f=105.00:t=q:w=0.700:g=5.5"));
    assert!(filter_str.contains("highshelf=f=10000.00:t=q:w=0.700:g=-2.0"));
}

#[test]
fn test_autoeq_json_import_export_roundtrip() {
    let json_input = r#"{
  "name": "HiFiMAN Sundara",
  "preamp": -5.5,
  "filters": [
    { "type": "PK", "fc": 35.0, "gain": 4.5, "q": 1.5, "enabled": true },
    { "type": "LSC", "fc": 100.0, "gain": 5.0, "q": 0.7, "enabled": true },
    { "type": "PK", "fc": 2100.0, "gain": -3.2, "q": 2.0, "enabled": true },
    { "type": "HSC", "fc": 8000.0, "gain": -1.5, "q": 0.8, "enabled": false }
  ]
}"#;

    let profile =
        AutoEqProfile::parse_json(json_input, None).expect("Must parse valid JSON profile");
    assert_eq!(profile.name, "HiFiMAN Sundara");
    assert!((profile.preamp_db - (-5.5)).abs() < 1e-4);
    assert_eq!(profile.filters.len(), 4);
    assert!(!profile.filters[3].enabled);

    // Export to JSON and re-parse
    let exported_json = profile.export_json().expect("Export to JSON must succeed");
    let roundtripped =
        AutoEqProfile::parse_json(&exported_json, None).expect("Roundtrip parse must succeed");
    assert_eq!(profile, roundtripped);

    // Export to text and re-parse
    let exported_text = profile.export_text();
    let text_roundtripped =
        AutoEqProfile::parse_text(&exported_text, None).expect("Text roundtrip must succeed");
    assert_eq!(profile.name, text_roundtripped.name);
    assert!((profile.preamp_db - text_roundtripped.preamp_db).abs() < 1e-3);
    assert_eq!(profile.filters.len(), text_roundtripped.filters.len());
}

#[test]
fn test_autoeq_parser_validation_and_errors() {
    // Empty input
    assert_eq!(AutoEqProfile::parse("", None), Err(AutoEqError::EmptyInput));
    assert_eq!(
        AutoEqProfile::parse("   \n\t", None),
        Err(AutoEqError::EmptyInput)
    );

    // No filters found
    assert!(matches!(
        AutoEqProfile::parse_text("Preamp: -5.0 dB\n# only comments\n", None),
        Err(AutoEqError::NoFiltersFound)
    ));

    // Invalid frequency (negative or zero)
    assert!(matches!(
        AutoEqFilter::new(AutoEqFilterType::Peaking, 0.0, 3.0, 1.4),
        Err(AutoEqError::InvalidFrequency(_))
    ));
    assert!(matches!(
        AutoEqFilter::new(AutoEqFilterType::Peaking, -100.0, 3.0, 1.4),
        Err(AutoEqError::InvalidFrequency(_))
    ));

    // NaN / Infinity rejection
    assert!(matches!(
        AutoEqFilter::new(AutoEqFilterType::Peaking, f64::NAN, 3.0, 1.4),
        Err(AutoEqError::InvalidNumber(_))
    ));
    assert!(matches!(
        AutoEqFilter::new(AutoEqFilterType::Peaking, 1000.0, f64::INFINITY, 1.4),
        Err(AutoEqError::InvalidNumber(_))
    ));
    assert!(matches!(
        AutoEqFilter::new(AutoEqFilterType::Peaking, 1000.0, 3.0, f64::NAN),
        Err(AutoEqError::InvalidNumber(_))
    ));

    // Invalid Q factor
    assert!(matches!(
        AutoEqFilter::new(AutoEqFilterType::Peaking, 1000.0, 3.0, 0.0),
        Err(AutoEqError::InvalidQFactor(_))
    ));
    assert!(matches!(
        AutoEqFilter::new(AutoEqFilterType::Peaking, 1000.0, 3.0, -2.0),
        Err(AutoEqError::InvalidQFactor(_))
    ));

    // Malformed JSON
    assert!(matches!(
        AutoEqProfile::parse_json("{ not valid json }", None),
        Err(AutoEqError::JsonParseError(_))
    ));
}

// =============================================================================
// TEST 2: EBU R128 Filter Construction & Normalization Policies
// =============================================================================

#[test]
fn test_ebu_r128_filter_construction() {
    // Default EBU R128 filter construction
    let default_ebu = EbuR128Settings::default();
    assert!(!default_ebu.enabled);
    assert_eq!(default_ebu.integrated_loudness, -16.0);
    assert_eq!(default_ebu.true_peak, -1.5);
    assert_eq!(default_ebu.loudness_range, 11.0);
    assert_eq!(
        default_ebu.build_filter_string(),
        "loudnorm=I=-16.0:TP=-1.5:LRA=11.0"
    );

    // Custom targets (e.g. YouTube target: -14.0 LUFS, -1.0 dBTP, 9.0 LRA)
    let custom_ebu = EbuR128Settings {
        enabled: true,
        preset: EbuR128Preset::Custom,
        integrated_loudness: -14.0,
        true_peak: -1.0,
        loudness_range: 9.0,
    };
    assert_eq!(
        custom_ebu.build_filter_string(),
        "loudnorm=I=-14.0:TP=-1.0:LRA=9.0"
    );

    // Parameter clamping and NaN / Inf defense
    let invalid_ebu = EbuR128Settings {
        enabled: true,
        preset: EbuR128Preset::Custom,
        integrated_loudness: f64::NAN,
        true_peak: f64::INFINITY,
        loudness_range: -50.0, // Should clamp to minimum 1.0
    };
    let sanitized_filter = invalid_ebu.build_filter_string();
    assert_eq!(sanitized_filter, "loudnorm=I=-16.0:TP=-1.5:LRA=1.0");

    // EqualizerSettings integration: mode selection and mutual exclusivity
    let mut eq = EqualizerSettings {
        spectrum_visualizer_enabled: false, // isolate filter testing
        ..Default::default()
    };

    // Case A: No normalization
    eq.set_normalization_mode(NormalizationMode::None);
    assert_eq!(eq.effective_normalization_mode(), NormalizationMode::None);
    let f_none = EqualizerManager::build_filter_string(&eq);
    assert!(!f_none.contains("loudnorm"));
    assert!(!f_none.contains("dynaudnorm"));

    // Case B: EBU R128 active
    eq.set_normalization_mode(NormalizationMode::EbuR128);
    assert_eq!(
        eq.effective_normalization_mode(),
        NormalizationMode::EbuR128
    );
    let f_ebu = EqualizerManager::build_filter_string(&eq);
    assert!(f_ebu.contains("loudnorm=I=-16.0:TP=-1.5:LRA=11.0"));
    assert!(!f_ebu.contains("dynaudnorm")); // Must not conflict!

    // Case C: Dynamic (dynaudnorm) active
    eq.set_normalization_mode(NormalizationMode::Dynamic);
    assert_eq!(
        eq.effective_normalization_mode(),
        NormalizationMode::Dynamic
    );
    let f_dyn = EqualizerManager::build_filter_string(&eq);
    assert!(f_dyn.contains("dynaudnorm=f=150:g=15"));
    assert!(!f_dyn.contains("loudnorm")); // Must not conflict!

    // Case D: Coexistence with 10-band EQ and Bauer crossfeed
    eq.is_enabled = true;
    eq.set_band_gain(4, 3.5); // 500 Hz boost
    eq.crossfeed_enabled = true;
    eq.crossfeed_preset = Bs2bPreset::ChuMoy;
    eq.set_normalization_mode(NormalizationMode::EbuR128);

    let full_chain = EqualizerManager::build_filter_string(&eq);
    assert!(full_chain.contains("equalizer=f=500.00:t=q:w=1.414:g=3.5"));
    assert!(full_chain.contains("bs2b=profile=cmoy"));
    assert!(full_chain.contains("loudnorm=I=-16.0:TP=-1.5:LRA=11.0"));
}

#[test]
fn test_equalizer_settings_serialization_backward_compatibility() {
    // Older JSON without E5 fields must deserialize seamlessly with correct defaults
    let legacy_json = r#"{
  "is_enabled": true,
  "preamp_db": -2.0,
  "gains": [0.0, 1.0, 2.0, 0.0, -1.0, 0.0, 1.5, 2.0, 0.0, -0.5],
  "bands": [
    {"enabled": true, "frequency_hz": 31.25, "gain_db": 0.0, "q_factor": 1.414},
    {"enabled": true, "frequency_hz": 62.5, "gain_db": 1.0, "q_factor": 1.414},
    {"enabled": true, "frequency_hz": 125.0, "gain_db": 2.0, "q_factor": 1.414},
    {"enabled": true, "frequency_hz": 250.0, "gain_db": 0.0, "q_factor": 1.414},
    {"enabled": true, "frequency_hz": 500.0, "gain_db": -1.0, "q_factor": 1.414},
    {"enabled": true, "frequency_hz": 1000.0, "gain_db": 0.0, "q_factor": 1.414},
    {"enabled": true, "frequency_hz": 2000.0, "gain_db": 1.5, "q_factor": 1.414},
    {"enabled": true, "frequency_hz": 4000.0, "gain_db": 2.0, "q_factor": 1.414},
    {"enabled": true, "frequency_hz": 8000.0, "gain_db": 0.0, "q_factor": 1.414},
    {"enabled": true, "frequency_hz": 16000.0, "gain_db": -0.5, "q_factor": 1.414}
  ],
  "dynaudnorm": true,
  "active_preset": "Custom",
  "pitch_correction": true,
  "audio_delay": 0.0,
  "channel_layout": "auto",
  "volume_boost": 100.0,
  "custom_presets": []
}"#;

    let deserialized: EqualizerSettings =
        serde_json::from_str(legacy_json).expect("Legacy settings must deserialize");
    assert!(deserialized.is_enabled);
    assert_eq!(deserialized.preamp_db, -2.0);
    // Backward compatibility: dynaudnorm: true maps to Dynamic normalization
    assert_eq!(
        deserialized.effective_normalization_mode(),
        NormalizationMode::Dynamic
    );
    // EBU R128 default is present and intact
    assert_eq!(deserialized.ebu_r128.integrated_loudness, -16.0);
    // AutoEq profile default is None
    assert!(deserialized.autoeq_profile.is_none());
    // Spectrum visualizer defaults to disabled in bare EqualizerSettings (enabled in MuvrixApp UI)
    assert!(!deserialized.spectrum_visualizer_enabled);
}

// =============================================================================
// TEST 3: 32-Band Spectrum Visualizer Bucket Calculations & Animation
// =============================================================================

#[test]
fn test_spectrum_visualizer_bucket_calculations() {
    // 1. Output bands count must be strictly 32
    assert_eq!(SPECTRUM_BANDS_COUNT, 32);
    assert_eq!(SPECTRUM_FREQUENCIES.len(), 32);

    // 2. Frequency distribution must be strictly monotonically increasing
    for i in 0..SPECTRUM_BANDS_COUNT - 1 {
        assert!(
            SPECTRUM_FREQUENCIES[i] < SPECTRUM_FREQUENCIES[i + 1],
            "Frequency at band {} must be strictly less than band {}",
            i,
            i + 1
        );
    }
    // Lowest band must be 20 Hz, highest 20,000 Hz
    assert_eq!(SPECTRUM_FREQUENCIES[0], 20.0);
    assert_eq!(SPECTRUM_FREQUENCIES[31], 20000.0);

    // 3. Frequency-to-bucket mapping
    assert_eq!(SpectrumVisualizer::bucket_index_for_freq(15.0), 0);
    assert_eq!(SpectrumVisualizer::bucket_index_for_freq(20.0), 0);
    assert_eq!(SpectrumVisualizer::bucket_index_for_freq(1000.0), 17); // 1 kHz band index
    assert_eq!(SpectrumVisualizer::bucket_index_for_freq(20000.0), 31);
    assert_eq!(SpectrumVisualizer::bucket_index_for_freq(24000.0), 31);

    // 4. Silence handling: when stopped or silent, all 32 bands must be exactly 0.0
    let vis = SpectrumVisualizer::new();
    let silent_telemetry = SpectrumTelemetry {
        rms_db: -100.0,
        peak_db: -100.0,
        is_playing: true,
        is_paused: false,
        ..SpectrumTelemetry::default()
    };
    let silent_mags = vis.compute_magnitudes(&silent_telemetry);
    assert_eq!(silent_mags, [0.0; 32]);

    let stopped_telemetry = SpectrumTelemetry {
        rms_db: -10.0,
        peak_db: -5.0,
        is_playing: false,
        ..SpectrumTelemetry::default()
    };
    let stopped_mags = vis.compute_magnitudes(&stopped_telemetry);
    assert_eq!(stopped_mags, [0.0; 32]);

    // 5. Centroid peak evaluation: 1 kHz tone concentrates peak around 1 kHz band
    let tone_telemetry = SpectrumTelemetry {
        rms_db: -18.0,
        peak_db: -15.0,
        spectral_centroid: 1000.0,
        spectral_spread: 250.0,
        spectral_rolloff: 2000.0,
        spectral_flatness: 0.05,
        spectral_slope: 0.0,
        is_playing: true,
        is_paused: false,
    };
    let tone_mags = vis.compute_magnitudes(&tone_telemetry);
    assert_eq!(tone_mags.len(), 32);

    // The peak must be at or immediately adjacent to the 1 kHz band (index 17)
    let max_idx = tone_mags
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
        .map(|(idx, _)| idx)
        .unwrap();
    assert!(
        (max_idx as isize - 17).abs() <= 1,
        "Peak was at index {max_idx}, expected near index 17 (1 kHz)"
    );
    assert!(tone_mags[max_idx] > 0.3);

    // Bands far away (e.g. 20 Hz, 20 kHz) must have significantly lower magnitude
    assert!(tone_mags[0] < tone_mags[max_idx] * 0.3);
    assert!(tone_mags[31] < tone_mags[max_idx] * 0.3);

    // 6. Smoothing calculations (Attack and Release)
    let mut dynamic_vis = SpectrumVisualizer::new();
    // Initially zero
    assert_eq!(dynamic_vis.smoothed_bands(), &[0.0; 32]);

    // Step with active tone: values must rise smoothly
    let step1 = dynamic_vis.update(&tone_telemetry);
    assert!(step1[max_idx] > 0.0);
    assert!(step1[max_idx] <= tone_mags[max_idx]);

    // Subsequent step: approaches target via attack smoothing
    let step2 = dynamic_vis.update(&tone_telemetry);
    assert!(step2[max_idx] >= step1[max_idx]);

    // Step with pause: values must decay smoothly towards zero
    let paused_telemetry = SpectrumTelemetry {
        is_playing: true,
        is_paused: true,
        ..tone_telemetry
    };
    let step_pause = dynamic_vis.update(&paused_telemetry);
    assert!(step_pause[max_idx] < step2[max_idx]);

    // Reset clears all 32 bands
    dynamic_vis.reset();
    assert_eq!(dynamic_vis.smoothed_bands(), &[0.0; 32]);
}

// =============================================================================
// TEST 4: Finding A — Spectrum Accuracy Across Known Frequency Tones
// =============================================================================

#[test]
fn test_spectrum_accuracy_known_frequency_tones() {
    let vis = SpectrumVisualizer::new();

    // 1. 100 Hz Pure Tone (Sub-bass / Bass boundary, Band 7 = 100 Hz)
    let telem_100hz = SpectrumTelemetry {
        rms_db: -12.0,
        peak_db: -9.0,
        spectral_centroid: 100.0,
        spectral_spread: 30.0,
        spectral_rolloff: 250.0,
        spectral_flatness: 0.003, // Highly tonal
        spectral_slope: -0.1,
        is_playing: true,
        is_paused: false,
    };
    let mags_100hz = vis.compute_magnitudes(&telem_100hz);
    let peak_100hz = mags_100hz
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
        .map(|(idx, _)| idx)
        .unwrap();
    assert_eq!(
        peak_100hz, 7,
        "100 Hz tone must peak precisely at band 7 (100 Hz)"
    );
    assert!(mags_100hz[7] > 0.5);
    // Distant bands (1 kHz = band 17, 10 kHz = band 27) must have near-zero energy
    assert!(mags_100hz[17] < 0.05);
    assert!(mags_100hz[27] < 0.01);

    // 2. 1 kHz Pure Tone (Midrange reference, Band 17 = 1000 Hz)
    let telem_1khz = SpectrumTelemetry {
        rms_db: -12.0,
        peak_db: -9.0,
        spectral_centroid: 1000.0,
        spectral_spread: 50.0,
        spectral_rolloff: 2000.0,
        spectral_flatness: 0.003,
        spectral_slope: 0.0,
        is_playing: true,
        is_paused: false,
    };
    let mags_1khz = vis.compute_magnitudes(&telem_1khz);
    let peak_1khz = mags_1khz
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
        .map(|(idx, _)| idx)
        .unwrap();
    assert_eq!(
        peak_1khz, 17,
        "1 kHz tone must peak precisely at band 17 (1000 Hz)"
    );
    assert!(mags_1khz[17] > 0.5);
    assert!(mags_1khz[7] < 0.05);
    assert!(mags_1khz[27] < 0.05);

    // 3. 10 kHz Pure Tone (Brilliance, Band 27 = 10000 Hz)
    let telem_10khz = SpectrumTelemetry {
        rms_db: -12.0,
        peak_db: -9.0,
        spectral_centroid: 10000.0,
        spectral_spread: 150.0,
        spectral_rolloff: 16000.0,
        spectral_flatness: 0.003,
        spectral_slope: 0.1,
        is_playing: true,
        is_paused: false,
    };
    let mags_10khz = vis.compute_magnitudes(&telem_10khz);
    let peak_10khz = mags_10khz
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
        .map(|(idx, _)| idx)
        .unwrap();
    assert_eq!(
        peak_10khz, 27,
        "10 kHz tone must peak precisely at band 27 (10000 Hz)"
    );
    assert!(mags_10khz[27] > 0.5);
    assert!(mags_10khz[7] < 0.01);
    assert!(mags_10khz[17] < 0.05);

    // 4. Silence & Noise Floor (< -60 dBFS)
    let telem_silence = SpectrumTelemetry {
        rms_db: -65.0,
        peak_db: -60.0,
        spectral_centroid: 1000.0,
        spectral_spread: 500.0,
        spectral_rolloff: 8000.0,
        spectral_flatness: 0.5,
        spectral_slope: 0.0,
        is_playing: true,
        is_paused: false,
    };
    let mags_silence = vis.compute_magnitudes(&telem_silence);
    assert_eq!(mags_silence, [0.0; 32]);

    // 5. Broadband White Noise (High flatness ~ 0.85)
    let telem_noise = SpectrumTelemetry {
        rms_db: -18.0,
        peak_db: -12.0,
        spectral_centroid: 8000.0,
        spectral_spread: 6000.0,
        spectral_rolloff: 18000.0,
        spectral_flatness: 0.85,
        spectral_slope: 0.0,
        is_playing: true,
        is_paused: false,
    };
    let mags_noise = vis.compute_magnitudes(&telem_noise);
    // Noise must activate all frequency bands across the entire spectrum
    let active_bands_count = mags_noise.iter().filter(|&&m| m > 0.05).count();
    assert_eq!(
        active_bands_count, 32,
        "White noise must excite all 32 frequency bands"
    );
}

// =============================================================================
// TEST 5: Finding C — AutoEq Transfer Function Mathematical Fidelity
// =============================================================================

#[test]
fn test_autoeq_transfer_function_mathematical_fidelity() {
    // Construct a multi-filter parametric profile with known parameters
    let f1 = AutoEqFilter::new(AutoEqFilterType::LowShelf, 100.0, 4.0, 0.707).unwrap();
    let f2 = AutoEqFilter::new(AutoEqFilterType::Peaking, 1000.0, -5.0, 2.0).unwrap();
    let f3 = AutoEqFilter::new(AutoEqFilterType::HighShelf, 8000.0, 3.0, 0.707).unwrap();

    let profile =
        AutoEqProfile::new("Fidelity Test Profile".to_string(), -2.5, vec![f1, f2, f3]).unwrap();

    // 1. Check cumulative response at exact frequencies
    // At very low frequency (20 Hz): LowShelf (+4 dB) dominates, Peaking ~0, HighShelf ~0, Preamp = -2.5
    let resp_20hz = profile.cumulative_response_at_freq(20.0);
    assert!(
        (resp_20hz - (-2.5 + 4.0)).abs() < 0.5,
        "20 Hz response should reflect low shelf boost + preamp"
    );

    // At center of peaking cut (1000 Hz): Peaking gives -5.0 dB, LowShelf is attenuated, Preamp = -2.5
    let resp_1khz = profile.cumulative_response_at_freq(1000.0);
    assert!(
        resp_1khz < -6.0,
        "1 kHz response should reflect -5.0 dB cut + preamp (-2.5 dB)"
    );

    // At high frequency (16000 Hz): HighShelf (+3 dB) dominates, Peaking ~0, LowShelf ~0, Preamp = -2.5
    let resp_16khz = profile.cumulative_response_at_freq(16000.0);
    assert!(
        (resp_16khz - (-2.5 + 3.0)).abs() < 1.0,
        "16 kHz response should reflect high shelf boost + preamp (got {resp_16khz})"
    );

    // 2. Verify filter chain serialization into mpv `af` filter graph retains all biquad parameters
    let mut eq_settings = EqualizerSettings::default();
    eq_settings.apply_autoeq_profile(profile.clone());

    let af_string = EqualizerManager::build_filter_string(&eq_settings);
    assert!(af_string.contains("volume=volume=-2.5dB"));
    assert!(af_string.contains("lowshelf=f=100.00:t=q:w=0.707:g=4.0"));
    assert!(af_string.contains("equalizer=f=1000.00:t=q:w=2.000:g=-5.0"));
    assert!(af_string.contains("highshelf=f=8000.00:t=q:w=0.707:g=3.0"));

    // 3. Round-trip fidelity: Export text -> parse -> compare response
    let text_export = profile.export_text();
    let reparsed = AutoEqProfile::parse_text(&text_export, None).unwrap();
    for f in [
        20.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0, 20000.0,
    ] {
        let orig = profile.cumulative_response_at_freq(f);
        let rep = reparsed.cumulative_response_at_freq(f);
        assert!(
            (orig - rep).abs() < 0.05,
            "Transfer function response at {f} Hz must be identical after roundtrip"
        );
    }
}

// =============================================================================
// TEST 6: Finding D — EBU R128 Preset Semantics & Reset Controls
// =============================================================================

#[test]
fn test_ebu_r128_preset_semantics() {
    let mut settings = EbuR128Settings::default();
    // Default must be Streaming (-16 LUFS, -1.5 dBTP, 11 LU)
    assert_eq!(settings.preset, EbuR128Preset::Streaming);
    assert_eq!(settings.integrated_loudness, -16.0);
    assert_eq!(settings.true_peak, -1.5);
    assert_eq!(settings.loudness_range, 11.0);
    assert_eq!(
        settings.build_filter_string(),
        "loudnorm=I=-16.0:TP=-1.5:LRA=11.0"
    );

    // Switch to Broadcast Reference
    settings.apply_preset(EbuR128Preset::Broadcast);
    assert_eq!(settings.preset, EbuR128Preset::Broadcast);
    assert_eq!(settings.integrated_loudness, -23.0);
    assert_eq!(settings.true_peak, -2.0);
    assert_eq!(settings.loudness_range, 7.0);
    assert_eq!(
        settings.build_filter_string(),
        "loudnorm=I=-23.0:TP=-2.0:LRA=7.0"
    );

    // Reset to Streaming
    settings.reset_to_streaming_defaults();
    assert_eq!(settings.preset, EbuR128Preset::Streaming);
    assert_eq!(settings.integrated_loudness, -16.0);
    assert_eq!(settings.true_peak, -1.5);
    assert_eq!(settings.loudness_range, 11.0);

    // Reset to Broadcast
    settings.reset_to_broadcast_standards();
    assert_eq!(settings.preset, EbuR128Preset::Broadcast);
    assert_eq!(settings.integrated_loudness, -23.0);
    assert_eq!(settings.true_peak, -2.0);
    assert_eq!(settings.loudness_range, 7.0);

    // Manual custom modification
    settings.integrated_loudness = -20.0;
    settings.preset = EbuR128Preset::Custom;
    assert_eq!(
        settings.build_filter_string(),
        "loudnorm=I=-20.0:TP=-2.0:LRA=7.0"
    );
}

// =============================================================================
// TEST 7: Finding B — Visualizer Performance Benchmark & Zero Overhead Check
// =============================================================================

#[test]
fn test_visualizer_performance_benchmark() {
    let mut vis = SpectrumVisualizer::new();
    let telem = SpectrumTelemetry {
        rms_db: -15.0,
        peak_db: -10.0,
        spectral_centroid: 1200.0,
        spectral_spread: 450.0,
        spectral_rolloff: 9000.0,
        spectral_flatness: 0.15,
        spectral_slope: -0.05,
        is_playing: true,
        is_paused: false,
    };

    // Warmup
    for _ in 0..100 {
        let _ = vis.update(&telem);
    }

    // Benchmark 1,000 visualizer update frames
    let start = std::time::Instant::now();
    const ITERATIONS: u32 = 1_000;
    for _ in 0..ITERATIONS {
        let _ = vis.update(&telem);
    }
    let elapsed = start.elapsed();
    let time_per_frame_micros = elapsed.as_micros() as f64 / ITERATIONS as f64;

    println!(
        "Spectrum visualizer performance: {:.2} µs / frame ({:.0} frames/sec throughput)",
        time_per_frame_micros,
        1_000_000.0 / time_per_frame_micros.max(0.001)
    );

    // Calculation must take less than 100 microseconds per frame (< 0.1 ms)
    // 60 FPS frame budget is 16,666 µs, so 100 µs is < 0.6% of frame budget!
    assert!(
        time_per_frame_micros < 100.0,
        "Spectrum frame calculation took {:.2} µs, expected < 100 µs",
        time_per_frame_micros
    );

    // Zero-overhead verification: when disabled, no filters exist in chain
    let eq_disabled = EqualizerSettings {
        spectrum_visualizer_enabled: false,
        ..Default::default()
    };
    let filter_disabled = EqualizerManager::build_filter_string(&eq_disabled);
    assert!(
        !filter_disabled.contains("@vis"),
        "Disabled visualizer must not attach @vis"
    );
    assert!(
        !filter_disabled.contains("@spec"),
        "Disabled visualizer must not attach @spec"
    );
}

// =============================================================================
// TEST 8: Finding B — Live Playback CPU, Memory, and Frame Time Profiling
// =============================================================================

fn get_process_cpu_ticks() -> u64 {
    if let Ok(stat) = std::fs::read_to_string("/proc/self/stat") {
        let parts: Vec<&str> = stat.split_whitespace().collect();
        if parts.len() > 14 {
            let utime: u64 = parts[13].parse().unwrap_or(0);
            let stime: u64 = parts[14].parse().unwrap_or(0);
            return utime + stime;
        }
    }
    0
}

fn get_process_rss_kb() -> u64 {
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status.lines() {
            if line.starts_with("VmRSS:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    return parts[1].parse().unwrap_or(0);
                }
            }
        }
    }
    0
}

#[test]
fn test_real_playback_visualizer_cpu_and_memory_benchmark() {
    let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/spectrum_demo.flac")
        .canonicalize()
        .expect("Tracked fixture spectrum_demo.flac must exist");

    let handle = muvrix_player::MpvHandle::new().expect("Create MpvHandle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().unwrap();

    // 1. Benchmark playback with visualizer DISABLED
    let eq_disabled = EqualizerSettings {
        spectrum_visualizer_enabled: false,
        ..Default::default()
    };
    EqualizerManager::apply_to_handle(&handle, &eq_disabled).unwrap();

    let initial_rss = get_process_rss_kb();
    let cpu_start_disabled = get_process_cpu_ticks();
    let wall_start_disabled = std::time::Instant::now();

    handle
        .load_file(
            fixture_path.to_str().unwrap(),
            muvrix_player::LoadFileMode::Replace,
        )
        .unwrap();

    // Play for 500 ms without visualizer
    std::thread::sleep(std::time::Duration::from_millis(500));
    let cpu_end_disabled = get_process_cpu_ticks();
    let wall_elapsed_disabled = wall_start_disabled.elapsed();
    let rss_disabled = get_process_rss_kb();

    let ticks_per_sec = 100.0; // standard Linux sysconf(_SC_CLK_TCK)
    let cpu_sec_disabled =
        (cpu_end_disabled.saturating_sub(cpu_start_disabled)) as f64 / ticks_per_sec;
    let cpu_pct_disabled = (cpu_sec_disabled / wall_elapsed_disabled.as_secs_f64()) * 100.0;

    // 2. Benchmark playback with visualizer ENABLED (60 FPS polling & smoothing)
    let eq_enabled = EqualizerSettings {
        spectrum_visualizer_enabled: true,
        ..Default::default()
    };
    EqualizerManager::apply_to_handle(&handle, &eq_enabled).unwrap();

    let mut vis = SpectrumVisualizer::new();
    let mut frame_times_micros = Vec::with_capacity(30);

    let cpu_start_enabled = get_process_cpu_ticks();
    let wall_start_enabled = std::time::Instant::now();

    // Emulate 30 UI frames (~500 ms at 60 FPS) with real property queries & smoothing
    for _ in 0..30 {
        let frame_start = std::time::Instant::now();

        // Query real-time RMS from af-metadata
        let rms_str = handle
            .get_property_string("af-metadata/vis/lavfi.astats.Overall.RMS_level")
            .unwrap_or_else(|_| "-100.0".to_string());
        let rms = rms_str.parse::<f64>().unwrap_or(-100.0);

        let telem = SpectrumTelemetry {
            rms_db: rms,
            peak_db: rms + 3.0,
            spectral_centroid: 1200.0,
            spectral_spread: 450.0,
            spectral_rolloff: 9000.0,
            spectral_flatness: 0.15,
            spectral_slope: -0.05,
            is_playing: true,
            is_paused: false,
        };

        let _ = vis.update(&telem);
        frame_times_micros.push(frame_start.elapsed().as_micros() as f64);
        std::thread::sleep(std::time::Duration::from_millis(16));
    }

    let cpu_end_enabled = get_process_cpu_ticks();
    let wall_elapsed_enabled = wall_start_enabled.elapsed();
    let rss_enabled = get_process_rss_kb();

    let cpu_sec_enabled =
        (cpu_end_enabled.saturating_sub(cpu_start_enabled)) as f64 / ticks_per_sec;
    let cpu_pct_enabled = (cpu_sec_enabled / wall_elapsed_enabled.as_secs_f64()) * 100.0;

    frame_times_micros.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mean_time: f64 = frame_times_micros.iter().sum::<f64>() / frame_times_micros.len() as f64;
    let p50 = frame_times_micros[frame_times_micros.len() / 2];
    let p95 = frame_times_micros[(frame_times_micros.len() as f64 * 0.95) as usize];
    let p99 = frame_times_micros[frame_times_micros.len() - 1];

    println!("\n=== MUVRIX PHASE E5 VISUALIZER PERFORMANCE BENCHMARK ===");
    println!("Media Fixture: spectrum_demo.flac (44.1 kHz, 24-bit FLAC, 30s)");
    println!("Initial VmRSS:           {} KB", initial_rss);
    println!(
        "Visualizer Disabled:     {:.2}% CPU, VmRSS: {} KB",
        cpu_pct_disabled, rss_disabled
    );
    println!(
        "Visualizer Enabled (60 FPS): {:.2}% CPU, VmRSS: {} KB",
        cpu_pct_enabled, rss_enabled
    );
    println!(
        "Delta Memory:            {} KB",
        rss_enabled.saturating_sub(rss_disabled)
    );
    println!("Frame Calculation Times:");
    println!(
        "  Mean: {:.2} µs ({:.0} FPS potential)",
        mean_time,
        1_000_000.0 / mean_time.max(0.001)
    );
    println!("  P50:  {:.2} µs", p50);
    println!("  P95:  {:.2} µs", p95);
    println!("  P99:  {:.2} µs", p99);
    println!("=======================================================\n");

    // Assertions:
    // Frame computation must be under 2,000 microseconds (well below the 16,666 µs frame budget)
    assert!(
        mean_time < 2000.0,
        "Mean frame time was {:.2} µs, expected < 2000 µs",
        mean_time
    );
    // Visualizer memory delta must be negligible (< 10 MB)
    assert!(
        rss_enabled.saturating_sub(rss_disabled) < 10240,
        "Visualizer memory leak detected"
    );
}

// =============================================================================
// AUDIT CORRECTION: SHELF FILTER Q PRESERVATION & DISABLED DSP VERIFICATION
// =============================================================================

#[test]
fn test_autoeq_shelf_filter_q_preservation_and_response() {
    // 1. Verify LowShelf Q Preservation across low Q (0.5), standard Q (0.707), and high Q (2.0)
    let ls_low_q = AutoEqFilter::new(AutoEqFilterType::LowShelf, 105.0, 5.5, 0.5).unwrap();
    let ls_std_q = AutoEqFilter::new(AutoEqFilterType::LowShelf, 105.0, 5.5, 0.707).unwrap();
    let ls_hi_q = AutoEqFilter::new(AutoEqFilterType::LowShelf, 105.0, 5.5, 2.0).unwrap();

    assert_eq!(ls_low_q.q_factor, 0.5);
    assert_eq!(ls_std_q.q_factor, 0.707);
    assert_eq!(ls_hi_q.q_factor, 2.0);

    // At exact center frequency (105 Hz), 2nd order shelving biquad gives exactly half the dB gain
    let half_gain = 5.5 / 2.0; // 2.75 dB
    assert!(
        (ls_low_q.gain_at_freq(105.0) - half_gain).abs() < 1e-4,
        "Low Q LowShelf at f0 must equal gain/2"
    );
    assert!(
        (ls_std_q.gain_at_freq(105.0) - half_gain).abs() < 1e-4,
        "Std Q LowShelf at f0 must equal gain/2"
    );
    assert!(
        (ls_hi_q.gain_at_freq(105.0) - half_gain).abs() < 1e-4,
        "High Q LowShelf at f0 must equal gain/2"
    );

    // High Q shelf exhibits steeper slope and transition resonance
    // At 70 Hz (below f0), high Q provides greater boost than low Q
    assert!(
        ls_hi_q.gain_at_freq(70.0) > ls_low_q.gain_at_freq(70.0),
        "High Q shelf must exhibit steeper transition slope"
    );

    // 2. Verify HighShelf Q Preservation across low Q (0.5), standard Q (0.707), and high Q (2.0)
    let hs_low_q = AutoEqFilter::new(AutoEqFilterType::HighShelf, 8000.0, 3.0, 0.5).unwrap();
    let hs_std_q = AutoEqFilter::new(AutoEqFilterType::HighShelf, 8000.0, 3.0, 0.707).unwrap();
    let hs_hi_q = AutoEqFilter::new(AutoEqFilterType::HighShelf, 8000.0, 3.0, 2.0).unwrap();

    assert_eq!(hs_low_q.q_factor, 0.5);
    assert_eq!(hs_std_q.q_factor, 0.707);
    assert_eq!(hs_hi_q.q_factor, 2.0);

    let hs_half_gain = 3.0 / 2.0; // 1.50 dB
    assert!(
        (hs_low_q.gain_at_freq(8000.0) - hs_half_gain).abs() < 1e-4,
        "Low Q HighShelf at f0 must equal gain/2"
    );
    assert!(
        (hs_std_q.gain_at_freq(8000.0) - hs_half_gain).abs() < 1e-4,
        "Std Q HighShelf at f0 must equal gain/2"
    );
    assert!(
        (hs_hi_q.gain_at_freq(8000.0) - hs_half_gain).abs() < 1e-4,
        "High Q HighShelf at f0 must equal gain/2"
    );

    // 3. Verify FFmpeg filter string generation explicitly preserves Q with width_type=q:w={Q}
    let profile = AutoEqProfile::new(
        "Q Preservation Test".to_string(),
        0.0,
        vec![ls_hi_q.clone(), hs_low_q.clone()],
    )
    .unwrap();

    let mut settings = EqualizerSettings::default();
    settings.apply_autoeq_profile(profile);
    let filter_str = EqualizerManager::build_filter_string(&settings);

    assert!(
        filter_str.contains("lowshelf=f=105.00:t=q:w=2.000:g=5.5"),
        "Filter string must explicitly preserve LowShelf Q factor: {filter_str}"
    );
    assert!(
        filter_str.contains("highshelf=f=8000.00:t=q:w=0.500:g=3.0"),
        "Filter string must explicitly preserve HighShelf Q factor: {filter_str}"
    );

    // 4. Numerical deviation evaluation across 20 Hz to 20,000 Hz
    let mut max_deviation = 0.0f64;
    for step in 0..=100 {
        let log_f = 1.30103 + (step as f64 / 100.0) * (4.30103 - 1.30103);
        let freq = 10.0f64.powf(log_f);
        let calculated = ls_std_q.gain_at_freq(freq);

        // Theoretical RBJ analog prototype magnitude response
        let a = 10.0f64.powf(5.5 / 40.0);
        let w = freq / 105.0;
        let w2 = w * w;
        let q = 0.707;
        let num = (a - w2).powi(2) + (a / (q * q)) * w2;
        let den = (1.0 - a * w2).powi(2) + (a / (q * q)) * w2;
        let expected = 10.0 * ((a * a) * (num / den)).log10();

        let dev = (calculated - expected).abs();
        if dev > max_deviation {
            max_deviation = dev;
        }
    }
    assert!(
        max_deviation < 1e-5,
        "Max deviation between internal model and RBJ biquad prototype must be < 1e-5 dB (got {max_deviation})"
    );
}

#[test]
fn test_visualizer_disabled_preserves_other_dsp_filters() {
    let mut settings = EqualizerSettings {
        is_enabled: true,
        crossfeed_enabled: true,
        spectrum_visualizer_enabled: false,
        ..Default::default()
    };
    settings.set_normalization_mode(NormalizationMode::EbuR128);
    settings.apply_preset(EqualizerPreset::Rock);

    let filter_str = EqualizerManager::build_filter_string(&settings);

    // 1. Must NOT contain visualizer telemetry analysis filters
    assert!(
        !filter_str.contains("@vis:lavfi=[astats"),
        "Disabled visualizer must not inject @vis"
    );
    assert!(
        !filter_str.contains("@spec:lavfi=[aspectralstats]"),
        "Disabled visualizer must not inject @spec"
    );

    // 2. Must RETAIN active user DSP filters
    assert!(
        filter_str.contains("equalizer="),
        "Equalizer filters must remain active"
    );
    assert!(
        filter_str.contains("bs2b="),
        "Crossfeed filter must remain active"
    );
    assert!(
        filter_str.contains("loudnorm="),
        "EBU R128 loudness normalization must remain active"
    );

    // 3. When visualizer is toggled ON, analysis filters are appended
    settings.spectrum_visualizer_enabled = true;
    let enabled_str = EqualizerManager::build_filter_string(&settings);
    assert!(enabled_str.contains("@vis:lavfi=[astats"));
    assert!(
        enabled_str.contains("@spec:lavfi=[aspectralstats=win_size=8192:overlap=0]"),
        "aspectralstats must use the CPU-cheap configuration (win_size=8192:overlap=0)"
    );

    // 4. When all DSP is disabled, filter string is completely empty (bit-perfect pass-through)
    let empty_settings = EqualizerSettings::default();
    assert_eq!(
        EqualizerManager::build_filter_string(&empty_settings),
        "",
        "With all DSP disabled, filter string must be completely empty"
    );
}
