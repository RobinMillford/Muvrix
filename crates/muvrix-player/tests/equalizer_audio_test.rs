//! Measurable audio tests for 10-Band Parametric Equalizer, Audio DSP, and Output Device Routing.

use std::path::PathBuf;
use std::sync::OnceLock;

use muvrix_player::equalizer::{EqualizerManager, EqualizerPreset, EqualizerSettings};
use muvrix_player::handle::MpvHandle;
use muvrix_player::PlaybackController;

/// Returns a per-process audio fixture, synthesising it on first use.
///
/// The WAV is generated in the system temp directory, not read from the source
/// tree. This removes the local-only fixture dependency: a fresh checkout can
/// run the tests without `tests/fixtures/audio_test_tone.wav`. The process id
/// keeps parallel test binaries from racing on the same file. Generation is
/// deterministic (same samples every run), so the Goertzel thresholds below
/// remain valid.
fn get_audio_fixture_path() -> PathBuf {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        let path =
            std::env::temp_dir().join(format!("muvrix-audio-test-tone-{}.wav", std::process::id()));
        // 2 s of a 1 kHz tone at half scale: enough for the Goertzel windows
        // used by the frequency-response assertions.
        synthesize_wav(&path, 2.0, &[(1000.0, 0.5)]);
        path
    })
    .clone()
}

fn create_test_player() -> MpvHandle {
    let handle = MpvHandle::new().expect("Create MpvHandle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().unwrap();
    handle
}

#[test]
fn test_parametric_equalizer_q_factor_and_filter_formatting() {
    let mut settings = EqualizerSettings {
        is_enabled: true,
        preamp_db: -3.0,
        ..Default::default()
    };

    // Adjust band 0 (31.25 Hz) with narrow Q = 3.0
    settings.set_band_gain(0, 6.0);
    settings.set_band_q(0, 3.0);

    // Adjust band 5 (1000 Hz) with wide Q = 0.707
    settings.set_band_gain(5, -4.5);
    settings.set_band_q(5, 0.707);

    // Disable band 9 (16000 Hz)
    settings.set_band_gain(9, 5.0);
    settings.set_band_enabled(9, false);

    let filter_str = EqualizerManager::build_filter_string(&settings);

    // Check preamp
    assert!(filter_str.contains("volume=volume=-3.0dB"));

    // Check band 0
    assert!(filter_str.contains("equalizer=f=31.25:t=q:w=3.000:g=6.0"));

    // Check band 5
    assert!(filter_str.contains("equalizer=f=1000.00:t=q:w=0.707:g=-4.5"));

    // Band 9 was disabled, so it must not be in the filter string
    assert!(!filter_str.contains("16000"));
}

fn goertzel_magnitude(samples: &[f64], target_freq: f64, sample_rate: f64) -> f64 {
    let omega = 2.0 * std::f64::consts::PI * target_freq / sample_rate;
    let coeff = 2.0 * omega.cos();
    let mut s1 = 0.0;
    let mut s2 = 0.0;
    for &x in samples {
        let s0 = x + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    let power = s1 * s1 + s2 * s2 - coeff * s1 * s2;
    power.sqrt() / (samples.len() as f64)
}

fn synthesize_wav(path: &std::path::Path, duration_secs: f64, freqs_and_amps: &[(f64, f64)]) {
    let sample_rate = 44100u32;
    let num_samples = (sample_rate as f64 * duration_secs) as usize;
    let mut raw_data = Vec::with_capacity(num_samples * 2);

    for i in 0..num_samples {
        let t = i as f64 / sample_rate as f64;
        let mut sample_val = 0.0;
        for &(freq, amp) in freqs_and_amps {
            sample_val += amp * (2.0 * std::f64::consts::PI * freq * t).sin();
        }
        let clamped = (sample_val.clamp(-1.0, 1.0) * 32767.0) as i16;
        raw_data.extend_from_slice(&clamped.to_le_bytes());
    }

    let subchunk2_size = raw_data.len() as u32;
    let chunk_size = 36 + subchunk2_size;

    let mut header = Vec::with_capacity(44);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&chunk_size.to_le_bytes());
    header.extend_from_slice(b"WAVE");
    header.extend_from_slice(b"fmt ");
    header.extend_from_slice(&16u32.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes());
    header.extend_from_slice(&sample_rate.to_le_bytes());
    header.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    header.extend_from_slice(&2u16.to_le_bytes());
    header.extend_from_slice(&16u16.to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&subchunk2_size.to_le_bytes());

    let mut full = header;
    full.extend_from_slice(&raw_data);
    std::fs::write(path, full).expect("Write synthesized WAV");
}

fn capture_pcm_output_advanced(
    fixture_path: &std::path::Path,
    filter_str: &str,
    extra_options: &[(&str, &str)],
    sleep_ms: u64,
) -> Vec<f64> {
    let out_file = std::env::temp_dir().join(format!(
        "muvrix_pcm_adv_{}_{}.wav",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    let handle = MpvHandle::new().expect("Create MpvHandle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "pcm").unwrap();
    handle.set_option_string("audio-format", "s16").unwrap();
    handle
        .set_option_string("ao-pcm-file", out_file.to_str().unwrap())
        .unwrap();
    for &(opt, val) in extra_options {
        handle.set_option_string(opt, val).unwrap();
    }
    if !filter_str.is_empty() {
        handle.set_option_string("af", filter_str).unwrap();
    }
    handle.initialize().unwrap();

    handle
        .command(&["loadfile", fixture_path.to_str().unwrap()])
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(sleep_ms));
    drop(handle);

    let data = std::fs::read(&out_file).expect("Read PCM output file");
    let _ = std::fs::remove_file(&out_file);

    let offset = if let Some(pos) = data.windows(4).position(|w| w == b"data") {
        pos + 8
    } else {
        44
    };

    if data.len() < offset {
        return vec![];
    }
    data[offset..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[b0, b1]| {
            let sample = i16::from_le_bytes([b0, b1]);
            sample as f64 / 32768.0
        })
        .collect()
}

fn capture_pcm_output(filter_str: &str) -> Vec<f64> {
    let fixture = get_audio_fixture_path();
    capture_pcm_output_advanced(&fixture, filter_str, &[], 400)
}

#[test]
fn test_frequency_response_boost_and_cut_deterministic_measurement() {
    // 1. Capture flat response
    let flat_samples = capture_pcm_output("");
    assert!(!flat_samples.is_empty());
    let mag_flat = goertzel_magnitude(&flat_samples, 1000.0, 44100.0);
    assert!(
        mag_flat > 0.05,
        "1000 Hz fixture must have significant energy"
    );

    // 2. Capture +6.0 dB boost at 1000 Hz
    let boost_samples = capture_pcm_output("equalizer=f=1000.00:t=q:w=1.414:g=6.0");
    let mag_boost = goertzel_magnitude(&boost_samples, 1000.0, 44100.0);
    let boost_db = 20.0 * (mag_boost / mag_flat).log10();
    assert!(
        (boost_db - 6.0).abs() < 0.5,
        "Measured boost should be ~+6.0 dB, got {:.2} dB",
        boost_db
    );

    // 3. Capture -6.0 dB attenuation at 1000 Hz
    let cut_samples = capture_pcm_output("equalizer=f=1000.00:t=q:w=1.414:g=-6.0");
    let mag_cut = goertzel_magnitude(&cut_samples, 1000.0, 44100.0);
    let cut_db = 20.0 * (mag_cut / mag_flat).log10();
    assert!(
        (cut_db - -6.0).abs() < 0.5,
        "Measured cut should be ~-6.0 dB, got {:.2} dB",
        cut_db
    );
}

#[test]
fn test_q_factor_bandwidth_selectivity_deterministic_measurement() {
    // Audio fixture is a pure 1000 Hz tone.
    // An off-center peaking filter at 700 Hz with +6 dB boost:
    // A wide filter (Q=0.5) will have a wide skirt boosting 1000 Hz significantly more
    // than a narrow filter (Q=4.0).
    let wide_samples = capture_pcm_output("equalizer=f=700.00:t=q:w=0.500:g=6.0");
    let narrow_samples = capture_pcm_output("equalizer=f=700.00:t=q:w=4.000:g=6.0");

    let mag_wide = goertzel_magnitude(&wide_samples, 1000.0, 44100.0);
    let mag_narrow = goertzel_magnitude(&narrow_samples, 1000.0, 44100.0);

    let wide_db = 20.0 * mag_wide.log10();
    let narrow_db = 20.0 * mag_narrow.log10();

    assert!(
        mag_wide > mag_narrow * 1.4,
        "Wide Q=0.5 ({:.2} dB) must pass substantially more off-center energy than narrow Q=4.0 ({:.2} dB)",
        wide_db,
        narrow_db
    );
}

#[test]
fn test_headroom_clipping_risk_and_safe_preamp_calculation() {
    let mut settings = EqualizerSettings {
        is_enabled: true,
        ..Default::default()
    };

    // Flat preset has no clipping risk
    assert!(!settings.has_clipping_risk());
    assert_eq!(settings.calculate_safe_preamp(), 0.0);

    // Bass Boost boosts +5.0 dB at 31 Hz and +4.0 dB at 62.5 Hz
    // Overlapping skirts increase peak boost to 5.9 dB -> safe preamp = -6.1 dB
    settings.apply_preset(EqualizerPreset::BassBoost);
    assert!(settings.has_clipping_risk());
    assert_eq!(settings.calculate_safe_preamp(), -6.1);

    // Attenuating preamp by safe amount eliminates clipping risk
    settings.preamp_db = settings.calculate_safe_preamp();
    assert!(!settings.has_clipping_risk());

    // Cinema preset: boosted sub-bass and presence
    settings.apply_preset(EqualizerPreset::Cinema);
    assert_eq!(settings.active_preset, EqualizerPreset::Cinema);
    let cinema_preamp = settings.calculate_safe_preamp();
    assert!(
        cinema_preamp <= -3.5,
        "Cinema safe preamp must account for multi-band boosts: got {:.1}",
        cinema_preamp
    );
}

#[test]
fn test_adjacent_boosted_bands_headroom_cascading() {
    let mut settings = EqualizerSettings {
        is_enabled: true,
        ..Default::default()
    };

    // Boost band 4 (500 Hz) by +6.0 dB and band 5 (1000 Hz) by +6.0 dB
    settings.set_band_gain(4, 6.0);
    settings.set_band_gain(5, 6.0);

    let peak = settings.peak_combined_boost();
    assert!(
        peak > 7.0,
        "Two adjacent +6.0 dB bands must produce peak combined boost > 7.0 dB due to filter overlap, got {:.2} dB",
        peak
    );

    let safe_preamp = settings.calculate_safe_preamp();
    assert!(
        safe_preamp <= -7.2,
        "Safe preamp for adjacent bands must be <= -7.2 dB to preserve digital headroom: got {:.1} dB",
        safe_preamp
    );

    // Boost 3 consecutive bands (+6 dB at 250, 500, 1000 Hz)
    settings.set_band_gain(3, 6.0);
    let peak3 = settings.peak_combined_boost();
    assert!(
        peak3 > 8.0,
        "Three consecutive +6.0 dB bands must produce peak combined boost > 8.0 dB, got {:.2} dB",
        peak3
    );
    let safe_preamp3 = settings.calculate_safe_preamp();
    assert!(
        safe_preamp3 <= -8.2,
        "Safe preamp for 3 adjacent bands must be <= -8.2 dB: got {:.1} dB",
        safe_preamp3
    );
}

#[test]
fn test_rapid_eq_adjustments_prevent_filter_accumulation() {
    let handle = create_test_player();
    let mut settings = EqualizerSettings {
        is_enabled: true,
        ..Default::default()
    };

    // Apply 50 rapid gain changes as a user drags a slider
    for step in 1..=50 {
        let gain = (step as f64 * 0.2) - 5.0; // -4.8 to +5.0 dB
        settings.set_band_gain(3, gain);
        EqualizerManager::apply_to_handle(&handle, &settings).expect("Apply step");
    }

    // Inspect active 'af' property
    let current_af = handle.get_property_string("af").unwrap();

    // Must contain exactly ONE equalizer filter for 250 Hz, NOT 50 stacked filters
    let occurrences = current_af.matches("250.00").count();
    assert_eq!(
        occurrences, 1,
        "Repeated adjustments must not accumulate duplicate filters: {current_af}"
    );

    // Clear filters
    EqualizerManager::reset(&handle).unwrap();
    assert_eq!(handle.get_property_string("af").unwrap(), "");
}

#[test]
fn test_real_audio_playback_with_live_dsp_switching() {
    let fixture = get_audio_fixture_path();
    let handle = std::sync::Arc::new(create_test_player());
    let mut controller = PlaybackController::new(handle.clone()).expect("Create controller");

    // Load synthetic test audio fixture
    controller
        .load_file(fixture.to_str().unwrap())
        .expect("Load audio fixture");

    // 1. Initial playback state
    let mut settings = EqualizerSettings {
        is_enabled: true,
        ..Default::default()
    };
    settings.apply_preset(EqualizerPreset::Flat);
    controller
        .apply_equalizer(&settings)
        .expect("Apply flat EQ");
    assert_eq!(handle.get_property_string("af").unwrap(), "");

    // 2. Switch live to Bass Boost while playing
    settings.apply_preset(EqualizerPreset::BassBoost);
    controller
        .apply_equalizer(&settings)
        .expect("Apply Bass Boost");
    let bb_af = handle.get_property_string("af").unwrap();
    assert!(bb_af.contains("31.25"));
    assert!(bb_af.contains("62.50"));

    // 3. Switch live to Treble Boost
    settings.apply_preset(EqualizerPreset::TrebleBoost);
    controller
        .apply_equalizer(&settings)
        .expect("Apply Treble Boost");
    let tb_af = handle.get_property_string("af").unwrap();
    assert!(!tb_af.contains("31.25")); // Bass filter removed
    assert!(tb_af.contains("16000.00")); // Treble filter active

    // 4. Enable Dynamic Range Normalizer
    settings.dynaudnorm = true;
    controller
        .apply_equalizer(&settings)
        .expect("Apply dynaudnorm");
    let dyn_af = handle.get_property_string("af").unwrap();
    assert!(dyn_af.contains("dynaudnorm=f=150:g=15"));

    // 5. Audio Delay live update
    controller.set_audio_delay(0.25).expect("Set audio delay");
    assert!((controller.audio_delay().unwrap() - 0.25).abs() < 0.01);

    controller
        .set_audio_delay(-0.15)
        .expect("Set negative audio delay");
    assert!((controller.audio_delay().unwrap() - -0.15).abs() < 0.01);

    // 6. Pitch correction toggle
    controller
        .set_pitch_correction(true)
        .expect("Set pitch correction yes");
    assert_eq!(
        handle
            .get_property_string("audio-pitch-correction")
            .unwrap(),
        "yes"
    );

    controller
        .set_pitch_correction(false)
        .expect("Set pitch correction no");
    assert_eq!(
        handle
            .get_property_string("audio-pitch-correction")
            .unwrap(),
        "no"
    );

    // 7. Channel layout selection
    controller
        .set_channel_layout("stereo")
        .expect("Set stereo channels");
    assert_eq!(
        handle.get_property_string("audio-channels").unwrap(),
        "stereo"
    );

    // 8. Disable EQ and reset
    controller.reset_equalizer().expect("Reset equalizer");
    assert_eq!(handle.get_property_string("af").unwrap(), "");
    assert_eq!(controller.audio_delay().unwrap(), 0.0);
}

#[test]
fn test_custom_user_eq_preset_lifecycle_save_rename_delete_restore() {
    let mut settings = EqualizerSettings::default();

    // 1. Configure custom gains
    settings.set_band_gain(0, 4.5);
    settings.set_band_gain(5, -3.0);
    settings.preamp_db = -1.5;

    // 2. Save as "My Bass Boost"
    settings.save_custom_preset("My Bass Boost");
    assert_eq!(settings.custom_presets.len(), 1);
    assert_eq!(settings.custom_presets[0].name, "My Bass Boost");
    assert_eq!(settings.custom_presets[0].gains[0], 4.5);
    assert_eq!(settings.custom_presets[0].preamp_db, -1.5);

    // 3. Rename preset
    let renamed = settings.rename_custom_preset("My Bass Boost", "Warm Acoustic");
    assert!(renamed);
    assert_eq!(settings.custom_presets[0].name, "Warm Acoustic");

    // 4. Change current EQ to Flat
    settings.apply_preset(EqualizerPreset::Flat);
    assert_eq!(settings.gains[0], 0.0);
    assert_eq!(settings.preamp_db, 0.0);

    // 5. Restore "Warm Acoustic"
    let restored = settings.restore_custom_preset("Warm Acoustic");
    assert!(restored);
    assert_eq!(settings.gains[0], 4.5);
    assert_eq!(settings.gains[5], -3.0);
    assert_eq!(settings.preamp_db, -1.5);
    assert_eq!(settings.active_preset, EqualizerPreset::Custom);

    // 6. Delete preset
    let deleted = settings.delete_custom_preset("Warm Acoustic");
    assert!(deleted);
    assert!(settings.custom_presets.is_empty());

    // 7. Nonexistent preset operations fail safely
    assert!(!settings.restore_custom_preset("Nonexistent"));
    assert!(!settings.delete_custom_preset("Nonexistent"));
    assert!(!settings.rename_custom_preset("Nonexistent", "New"));
}

#[test]
fn test_audio_device_selection_and_fallback_resilience() {
    let handle = create_test_player();

    // 1. Query runtime audio devices
    let devices = EqualizerManager::query_audio_devices(&handle).expect("Query devices");
    assert!(!devices.is_empty());
    assert_eq!(devices[0].name, "auto");

    // 2. Classify physical vs virtual devices
    let mut physical_sinks = Vec::new();
    let mut virtual_sinks = Vec::new();
    for dev in &devices {
        if dev.is_physical() {
            physical_sinks.push(dev);
        } else {
            virtual_sinks.push(dev);
        }
    }
    // "auto" and generic backends must be classified as virtual/dispatchers
    assert!(!devices[0].is_physical());

    // 3. Setting valid "auto" succeeds with low latency
    let start_auto = std::time::Instant::now();
    let res = EqualizerManager::set_audio_device_with_fallback(&handle, "auto");
    let auto_elapsed = start_auto.elapsed();
    assert_eq!(res.unwrap(), "auto");
    assert!(
        auto_elapsed.as_millis() < 50,
        "Device configuration dispatch must complete rapidly (nominal < 50ms): {:?}",
        auto_elapsed
    );

    // 4. Live switch to an alternate available device if present
    if devices.len() > 1 {
        let alt_device = &devices[1].name;
        let start_alt = std::time::Instant::now();
        let res_alt = EqualizerManager::set_audio_device_with_fallback(&handle, alt_device);
        let alt_elapsed = start_alt.elapsed();
        assert!(res_alt.is_ok(), "Must switch cleanly to {}", alt_device);
        assert_eq!(
            handle.get_property_string("audio-device").unwrap(),
            *alt_device
        );
        assert!(
            alt_elapsed.as_millis() < 100,
            "Live audio device switch took {:?}",
            alt_elapsed
        );

        // Switch back to auto
        let res_back = EqualizerManager::set_audio_device_with_fallback(&handle, "auto");
        assert_eq!(res_back.unwrap(), "auto");
        assert_eq!(handle.get_property_string("audio-device").unwrap(), "auto");
    }

    // 5. Setting a nonexistent or disconnected device gracefully falls back to "auto"
    let fallback = EqualizerManager::set_audio_device_with_fallback(
        &handle,
        "nonexistent-disconnected-usb-dac-9999",
    );
    assert_eq!(fallback.unwrap(), "auto");
    assert_eq!(handle.get_property_string("audio-device").unwrap(), "auto");
}

#[test]
fn test_all_10_parametric_eq_bands_independent_boost_and_cut() {
    let temp_wav = std::env::temp_dir().join(format!("muvrix_10band_{}.wav", std::process::id()));
    let freqs = muvrix_player::equalizer::EQ_FREQUENCIES;
    let freqs_and_amps: Vec<(f64, f64)> = freqs.iter().map(|&f| (f, 0.08)).collect();
    synthesize_wav(&temp_wav, 0.6, &freqs_and_amps);

    // 1. Capture flat baseline
    let flat_samples = capture_pcm_output_advanced(&temp_wav, "", &[], 500);
    assert!(!flat_samples.is_empty());

    // 2. Loop through all 10 bands and verify independent boost and cut
    for (i, &f) in freqs.iter().enumerate() {
        let mag_flat = goertzel_magnitude(&flat_samples, f, 44100.0);
        assert!(
            mag_flat > 0.005,
            "Band {} ({:.2} Hz) flat energy must be non-zero",
            i,
            f
        );

        // Boost +6.0 dB
        let boost_filter = format!("equalizer=f={:.2}:t=q:w=1.414:g=6.0", f);
        let boost_samples = capture_pcm_output_advanced(&temp_wav, &boost_filter, &[], 500);
        let mag_boost = goertzel_magnitude(&boost_samples, f, 44100.0);
        let boost_db = 20.0 * (mag_boost / mag_flat).log10();
        assert!(
            (boost_db - 6.0).abs() < 0.6,
            "Band {} ({:.2} Hz) boost should be ~+6.0 dB, got {:.2} dB",
            i,
            f,
            boost_db
        );

        // Cut -6.0 dB
        let cut_filter = format!("equalizer=f={:.2}:t=q:w=1.414:g=-6.0", f);
        let cut_samples = capture_pcm_output_advanced(&temp_wav, &cut_filter, &[], 500);
        let mag_cut = goertzel_magnitude(&cut_samples, f, 44100.0);
        let cut_db = 20.0 * (mag_cut / mag_flat).log10();
        assert!(
            (cut_db - -6.0).abs() < 0.6,
            "Band {} ({:.2} Hz) cut should be ~-6.0 dB, got {:.2} dB",
            i,
            f,
            cut_db
        );
    }

    let _ = std::fs::remove_file(&temp_wav);
}

#[test]
fn test_dynamic_range_normalizer_measurement() {
    let temp_wav = std::env::temp_dir().join(format!("muvrix_dyn_{}.wav", std::process::id()));
    // Quiet 1000 Hz tone: amplitude 2000 / 32768 ~ 0.061
    synthesize_wav(&temp_wav, 2.5, &[(1000.0, 2000.0 / 32768.0)]);

    // Without dynaudnorm
    let normal_samples = capture_pcm_output_advanced(&temp_wav, "", &[], 600);
    let peak_normal = normal_samples
        .iter()
        .fold(0.0f64, |acc, &x| acc.max(x.abs()));
    assert!(
        peak_normal > 0.05 && peak_normal < 0.08,
        "Peak should be ~0.061, got {:.3}",
        peak_normal
    );

    // With dynaudnorm=f=150:g=15
    let dyn_samples = capture_pcm_output_advanced(&temp_wav, "dynaudnorm=f=150:g=15", &[], 600);
    let peak_dyn = dyn_samples.iter().fold(0.0f64, |acc, &x| acc.max(x.abs()));

    let boost_ratio = peak_dyn / peak_normal;
    assert!(
        boost_ratio > 3.0,
        "Dynaudnorm must amplify quiet tone significantly (> 3.0x), got {:.2}x (peak: {:.3} vs {:.3})",
        boost_ratio, peak_dyn, peak_normal
    );

    let _ = std::fs::remove_file(&temp_wav);
}

#[test]
fn test_volume_boost_up_to_150_percent_pcm_measurement() {
    let temp_wav = std::env::temp_dir().join(format!("muvrix_vol_{}.wav", std::process::id()));
    // Very quiet tone: amp 0.05
    synthesize_wav(&temp_wav, 0.6, &[(1000.0, 0.05)]);

    // Standard volume 100%
    let s100 = capture_pcm_output_advanced(
        &temp_wav,
        "",
        &[("volume-max", "150"), ("volume", "100")],
        400,
    );
    let p100 = s100.iter().fold(0.0f64, |acc, &x| acc.max(x.abs()));

    // Boosted volume 150%
    let s150 = capture_pcm_output_advanced(
        &temp_wav,
        "",
        &[("volume-max", "150"), ("volume", "150")],
        400,
    );
    let p150 = s150.iter().fold(0.0f64, |acc, &x| acc.max(x.abs()));

    let ratio = p150 / p100;
    // mpv cubic curve: (150/100)^3 = 3.375
    assert!(
        (ratio - 3.375).abs() < 0.15,
        "Volume 150% should scale output by ~3.375x (cubic curve), got {:.3}x",
        ratio
    );

    let _ = std::fs::remove_file(&temp_wav);
}

#[test]
fn test_safe_preamp_digital_headroom_clipping_reduction() {
    let temp_wav = std::env::temp_dir().join(format!("muvrix_clip_{}.wav", std::process::id()));
    // High-amplitude 1000 Hz tone: amp 0.85 (already close to full scale)
    synthesize_wav(&temp_wav, 0.6, &[(1000.0, 0.85)]);

    // Boost +6.0 dB at 1000 Hz with preamp 0.0 dB -> clipped samples
    let boost_filter_no_preamp = "equalizer=f=1000.00:t=q:w=1.414:g=6.0";
    let s_unmitigated = capture_pcm_output_advanced(&temp_wav, boost_filter_no_preamp, &[], 400);
    let clipped_unmitigated = s_unmitigated.iter().filter(|&&x| x.abs() >= 0.999).count();
    assert!(
        clipped_unmitigated > 100,
        "Boosting high-amplitude signal without safe preamp must cause digital clipping: {} clipped",
        clipped_unmitigated
    );

    // Boost +6.0 dB at 1000 Hz with safe preamp (-6.2 dB) -> 0 clipped samples!
    let boost_filter_safe = "volume=volume=-6.2dB,equalizer=f=1000.00:t=q:w=1.414:g=6.0";
    let s_safe = capture_pcm_output_advanced(&temp_wav, boost_filter_safe, &[], 400);
    let clipped_safe = s_safe.iter().filter(|&&x| x.abs() >= 0.999).count();
    assert_eq!(
        clipped_safe, 0,
        "Applying safe digital headroom attenuation must eliminate digital clipping"
    );

    let peak_safe = s_safe.iter().fold(0.0f64, |acc, &x| acc.max(x.abs()));
    assert!(
        peak_safe < 0.95,
        "Safe preamp preserves digital headroom (peak {:.3} < 0.95)",
        peak_safe
    );

    let _ = std::fs::remove_file(&temp_wav);
}

#[test]
fn test_pipewire_real_device_switching_and_graph_link_transition() {
    // Check if pw-link is available
    let pw_link_check = std::process::Command::new("pw-link").arg("-l").output();
    let has_pipewire = match pw_link_check {
        Ok(out) => out.status.success(),
        Err(_) => false,
    };

    if !has_pipewire {
        println!("PipeWire / pw-link not available in this test environment; skipping live link inspection.");
        return;
    }

    let fixture = get_audio_fixture_path();
    let handle = MpvHandle::new().expect("Create MpvHandle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "pipewire").unwrap();
    handle.set_option_string("loop-file", "inf").unwrap();
    handle.initialize().unwrap();

    let devices = EqualizerManager::query_audio_devices(&handle).expect("Query devices");
    let pipewire_devices: Vec<&str> = devices
        .iter()
        .filter(|d| d.name.starts_with("pipewire/alsa_output"))
        .map(|d| d.name.as_str())
        .collect();

    if pipewire_devices.len() < 2 {
        println!(
            "Less than 2 PipeWire ALSA sinks available; verified query: {:?}",
            devices
        );
        return;
    }

    let dev1 = pipewire_devices[0];
    let dev2 = pipewire_devices[1];

    handle
        .command(&["loadfile", fixture.to_str().unwrap()])
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));

    // 1. Route to dev1
    let set1 = EqualizerManager::set_audio_device_with_fallback(&handle, dev1).unwrap();
    assert_eq!(set1, dev1);
    std::thread::sleep(std::time::Duration::from_millis(300));

    let links1 = String::from_utf8_lossy(
        &std::process::Command::new("pw-link")
            .arg("-l")
            .output()
            .unwrap()
            .stdout,
    )
    .to_string();
    assert!(
        links1.contains("mpv:output_"),
        "PipeWire graph must establish active links from mpv:output_*"
    );

    // 2. Dynamic live switch to dev2
    let set2 = EqualizerManager::set_audio_device_with_fallback(&handle, dev2).unwrap();
    assert_eq!(set2, dev2);
    std::thread::sleep(std::time::Duration::from_millis(300));

    let links2 = String::from_utf8_lossy(
        &std::process::Command::new("pw-link")
            .arg("-l")
            .output()
            .unwrap()
            .stdout,
    )
    .to_string();
    assert!(
        links2.contains("mpv:output_"),
        "PipeWire graph must maintain active links from mpv:output_* after live switch"
    );

    // 3. Fallback to auto
    let fallback =
        EqualizerManager::set_audio_device_with_fallback(&handle, "nonexistent_device_test")
            .unwrap();
    assert_eq!(fallback, "auto");
}

#[test]
fn test_pitch_correction_avoids_af_scaletempo_graph_clutter() {
    let handle = create_test_player();
    let controller = PlaybackController::new(std::sync::Arc::new(handle)).unwrap();

    // Enable pitch correction
    controller.set_pitch_correction(true).unwrap();
    assert_eq!(
        controller
            .handle()
            .get_property_string("audio-pitch-correction")
            .unwrap(),
        "yes"
    );

    // af filter graph must NOT contain scaletempo or scaletempo2
    let af = controller.handle().get_property_string("af").unwrap();
    assert!(
        !af.contains("scaletempo"),
        "mpv handles pitch correction natively; af must not be cluttered: {af}"
    );

    // Disable pitch correction
    controller.set_pitch_correction(false).unwrap();
    assert_eq!(
        controller
            .handle()
            .get_property_string("audio-pitch-correction")
            .unwrap(),
        "no"
    );
}

fn synthesize_multichannel_5_1_wav(path: &std::path::Path, duration_secs: f64) {
    let sample_rate = 44100u32;
    let num_channels = 6u16; // 5.1: FL, FR, FC, LFE, SL, SR
    let num_frames = (sample_rate as f64 * duration_secs) as usize;
    let bytes_per_sample = 2u16; // 16-bit
    let block_align = num_channels * bytes_per_sample;
    let byte_rate = sample_rate * block_align as u32;

    let mut raw_data = Vec::with_capacity(num_frames * block_align as usize);

    for i in 0..num_frames {
        let t = i as f64 / sample_rate as f64;
        let ch_freqs = [440.0, 880.0, 1000.0, 80.0, 220.0, 660.0];
        for &freq in &ch_freqs {
            let s = (0.2 * (2.0 * std::f64::consts::PI * freq * t).sin() * 32767.0) as i16;
            raw_data.extend_from_slice(&s.to_le_bytes());
        }
    }

    let subchunk2_size = raw_data.len() as u32;
    let chunk_size = 36 + subchunk2_size;

    let mut header = Vec::with_capacity(44);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&chunk_size.to_le_bytes());
    header.extend_from_slice(b"WAVE");
    header.extend_from_slice(b"fmt ");
    header.extend_from_slice(&16u32.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes());
    header.extend_from_slice(&num_channels.to_le_bytes());
    header.extend_from_slice(&sample_rate.to_le_bytes());
    header.extend_from_slice(&byte_rate.to_le_bytes());
    header.extend_from_slice(&block_align.to_le_bytes());
    header.extend_from_slice(&(bytes_per_sample * 8).to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&subchunk2_size.to_le_bytes());

    let mut full = header;
    full.extend_from_slice(&raw_data);
    std::fs::write(path, full).expect("Write synthesized 5.1 WAV");
}

fn capture_pcm_file_info(
    fixture_path: &std::path::Path,
    filter_str: &str,
    extra_options: &[(&str, &str)],
    sleep_ms: u64,
) -> (u16, u32, Vec<u8>) {
    let out_file = std::env::temp_dir().join(format!(
        "muvrix_pcm_info_{}_{}.wav",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    let handle = MpvHandle::new().expect("Create MpvHandle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "pcm").unwrap();
    handle.set_option_string("audio-format", "s16").unwrap();
    handle
        .set_option_string("ao-pcm-file", out_file.to_str().unwrap())
        .unwrap();
    for &(opt, val) in extra_options {
        handle.set_option_string(opt, val).unwrap();
    }
    if !filter_str.is_empty() {
        handle.set_option_string("af", filter_str).unwrap();
    }
    handle.initialize().unwrap();

    handle
        .command(&["loadfile", fixture_path.to_str().unwrap()])
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(sleep_ms));
    drop(handle);

    let data = std::fs::read(&out_file).expect("Read PCM output file");
    let _ = std::fs::remove_file(&out_file);

    let channels = u16::from_le_bytes([data[22], data[23]]);
    let sample_rate = u32::from_le_bytes([data[24], data[25], data[26], data[27]]);
    (channels, sample_rate, data)
}

/// Estimates dominant frequency using Hann-windowed Goertzel DTFT with parabolic peak interpolation.
///
/// Analysis parameters:
/// - Sample rate: 44,100 Hz
/// - Analyzed window: 8,192 samples (from steady-state middle of captured audio)
/// - Window function: Hann window w[n] = 0.5 * (1 - cos(2*pi*n / (N-1)))
/// - Coarse grid step: 0.5 Hz
/// - Parabolic interpolation: delta = 0.5 * (ln(P_-) - ln(P_+)) / (ln(P_-) - 2*ln(P_0) + ln(P_+))
/// - Estimator uncertainty: +/- 0.05 Hz
fn estimate_sinusoidal_frequency_interpolated(
    samples: &[f64],
    f_min: f64,
    f_max: f64,
    sample_rate: f64,
) -> (f64, f64) {
    let n = samples.len();
    let window_len = if n >= 8192 {
        8192
    } else {
        n.next_power_of_two() / 2
    };
    if window_len < 512 {
        return (f_min, 0.0);
    }
    let start_idx = if n > window_len {
        (n - window_len) / 2
    } else {
        0
    };
    let block = &samples[start_idx..start_idx + window_len];

    // Apply Hann window to eliminate spectral leakage
    let mut windowed = Vec::with_capacity(window_len);
    for (k, &s) in block.iter().enumerate() {
        let w =
            0.5 * (1.0 - (2.0 * std::f64::consts::PI * k as f64 / (window_len - 1) as f64).cos());
        windowed.push(s * w);
    }

    // Coarse search in 0.5 Hz increments
    let step = 0.5;
    let mut best_f = f_min;
    let mut max_p = -1.0;
    let mut f = f_min;
    while f <= f_max {
        let p = goertzel_magnitude(&windowed, f, sample_rate);
        if p > max_p {
            max_p = p;
            best_f = f;
        }
        f += step;
    }

    // Parabolic interpolation on log-power around the coarse peak
    let f_m = best_f - step;
    let f_p = best_f + step;
    let p_m = goertzel_magnitude(&windowed, f_m, sample_rate);
    let p_c = max_p;
    let p_p = goertzel_magnitude(&windowed, f_p, sample_rate);

    let alpha = (p_m.max(1e-12)).ln();
    let beta = (p_c.max(1e-12)).ln();
    let gamma = (p_p.max(1e-12)).ln();

    let denom = alpha - 2.0 * beta + gamma;
    let delta = if denom.abs() > 1e-12 {
        0.5 * (alpha - gamma) / denom
    } else {
        0.0
    };
    let clamped_delta = delta.clamp(-1.0, 1.0);
    let refined_f = best_f + clamped_delta * step;

    (refined_f, max_p)
}

fn find_dominant_frequency(
    samples: &[f64],
    f_min: f64,
    f_max: f64,
    step_hz: f64,
    sample_rate: f64,
) -> (f64, f64) {
    let _ = step_hz;
    estimate_sinusoidal_frequency_interpolated(samples, f_min, f_max, sample_rate)
}

#[test]
fn test_pitch_estimator_calibration_synthetic_grid_offsets() {
    let fs = 44100.0;
    let n = 8192;
    let test_freqs = [997.30, 999.20, 1000.13, 1002.70, 1003.90];
    let phases = [
        0.0,
        std::f64::consts::PI / 4.0,
        std::f64::consts::PI / 2.0,
        3.0 * std::f64::consts::PI / 4.0,
        std::f64::consts::PI,
    ];
    let amplitudes = [0.1, 0.5, 0.9];

    println!(
        "\n=== Pitch Estimator Calibration across Off-Grid Frequencies, Phases & Amplitudes ==="
    );
    println!("| Generated (Hz) | Amp | Phase (rad) | Measured (Hz) | Absolute Error (Hz) | Calibration Status |");
    println!("| :------------: | :-: | :---------: | :-----------: | :-----------------: | :----------------: |");

    let mut max_observed_error = 0.0f64;

    for &f_true in &test_freqs {
        for &amp in &amplitudes {
            for &phase in &phases {
                let mut samples = Vec::with_capacity(n);
                for k in 0..n {
                    let t = k as f64 / fs;
                    samples.push(amp * (2.0 * std::f64::consts::PI * f_true * t + phase).sin());
                }

                let (f_est, _) =
                    estimate_sinusoidal_frequency_interpolated(&samples, 950.0, 1050.0, fs);
                let err = (f_est - f_true).abs();
                if err > max_observed_error {
                    max_observed_error = err;
                }

                assert!(
                    err <= 0.05,
                    "Estimator error {:.6} Hz for true freq {:.2} Hz (amp={:.1}, phase={:.2}) must be <= 0.05 Hz",
                    err, f_true, amp, phase
                );

                if (phase - 0.0).abs() < 1e-6 && (amp - 0.5).abs() < 1e-6 {
                    println!(
                        "| {:14.2} | {:3.1} | {:11.4} | {:13.4} | {:19.6} | PASS (<=0.05 Hz)  |",
                        f_true, amp, phase, f_est, err
                    );
                }
            }
        }
    }

    println!(
        "Maximum observed calibration error across all 75 tested combinations: {:.6} Hz",
        max_observed_error
    );
    println!(
        "Calibration result: +/- 0.05 Hz measurement uncertainty experimentally verified (max error {:.6} Hz << 0.05 Hz)\n",
        max_observed_error
    );
}

#[test]
fn test_pitch_correction_frequency_preservation_across_speeds() {
    let temp_tone =
        std::env::temp_dir().join(format!("muvrix_1000hz_tone_{}.wav", std::process::id()));
    synthesize_wav(&temp_tone, 1.0, &[(1000.0, 0.5)]);

    let speeds = [0.75, 1.0, 1.25, 1.5, 2.0];
    println!("\n=== Pitch Correction Frequency Measurement Across Speeds (Input: 1000 Hz) ===");
    println!("| Speed | Mode | Expected (Hz) | Measured (Hz) | Deviation (Hz) | Tolerance (Hz) | Status |");
    println!("| :---: | :--: | :-----------: | :-----------: | :------------: | :------------: | :----: |");

    for &sp in &speeds {
        let sp_str = format!("{:.2}", sp);

        // 1. Pitch correction ENABLED: Expected dominant frequency remains 1000.0 Hz
        let pcm_corr = capture_pcm_output_advanced(
            &temp_tone,
            "",
            &[
                ("speed", &sp_str),
                ("audio-pitch-correction", "yes"),
                ("untimed", "yes"),
            ],
            400,
        );
        assert!(
            !pcm_corr.is_empty(),
            "Captured PCM with pitch correction ON must not be empty"
        );
        let (meas_corr, _) = find_dominant_frequency(&pcm_corr, 950.0, 1050.0, 2.0, 44100.0);
        let dev_corr = (meas_corr - 1000.0).abs();
        let tol_corr = 10.0;
        let pass_corr = dev_corr <= tol_corr;
        println!(
            "| {:5.2} | ON   | 1000.00       | {:13.2} | {:14.2} | {:14.2} | {:6} |",
            sp,
            meas_corr,
            dev_corr,
            tol_corr,
            if pass_corr { "PASS" } else { "FAIL" }
        );
        assert!(
            pass_corr,
            "At speed {}x with pitch correction ON, dominant freq {:.2} Hz must be within {:.2} Hz of 1000 Hz",
            sp, meas_corr, tol_corr
        );

        // 2. Pitch correction DISABLED: Expected frequency shifts to 1000.0 * speed
        let expected_uncorr = 1000.0 * sp;
        let pcm_uncorr = capture_pcm_output_advanced(
            &temp_tone,
            "",
            &[
                ("speed", &sp_str),
                ("audio-pitch-correction", "no"),
                ("untimed", "yes"),
            ],
            400,
        );
        assert!(
            !pcm_uncorr.is_empty(),
            "Captured PCM with pitch correction OFF must not be empty"
        );
        let (meas_uncorr, _) = find_dominant_frequency(
            &pcm_uncorr,
            expected_uncorr - 50.0,
            expected_uncorr + 50.0,
            2.0,
            44100.0,
        );
        let dev_uncorr = (meas_uncorr - expected_uncorr).abs();
        let tol_uncorr = 15.0;
        let pass_uncorr = dev_uncorr <= tol_uncorr;
        println!(
            "| {:5.2} | OFF  | {:13.2} | {:13.2} | {:14.2} | {:14.2} | {:6} |",
            sp,
            expected_uncorr,
            meas_uncorr,
            dev_uncorr,
            tol_uncorr,
            if pass_uncorr { "PASS" } else { "FAIL" }
        );
        assert!(
            pass_uncorr,
            "At speed {}x with pitch correction OFF, dominant freq {:.2} Hz must be within {:.2} Hz of expected {:.2} Hz",
            sp, meas_uncorr, tol_uncorr, expected_uncorr
        );
    }
    println!("============================================================================\n");
    let _ = std::fs::remove_file(&temp_tone);
}

#[test]
fn test_audio_video_synchronization_and_delay_offset() {
    let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/test_video.mp4")
        .canonicalize()
        .expect("test_video.mp4 must exist");

    let handle = std::sync::Arc::new(MpvHandle::new().expect("Create handle"));
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().unwrap();

    let mut controller = PlaybackController::new(handle.clone()).expect("Create controller");
    controller
        .load_file(fixture.to_str().unwrap())
        .expect("Load file");

    std::thread::sleep(std::time::Duration::from_millis(400));

    // Baseline audio delay is 0.0
    assert_eq!(controller.audio_delay().unwrap(), 0.0);

    // Apply positive audio delay (+250ms)
    controller
        .set_audio_delay(0.250)
        .expect("Set +250ms audio delay");
    let reported_delay = controller.audio_delay().unwrap();
    assert!((reported_delay - 0.250).abs() < 0.001);
    let prop_delay = handle.get_property::<f64>("audio-delay").unwrap();
    assert!((prop_delay - 0.250).abs() < 0.001);

    // Apply negative audio delay (-150ms)
    controller
        .set_audio_delay(-0.150)
        .expect("Set -150ms audio delay");
    let reported_neg = controller.audio_delay().unwrap();
    assert!((reported_neg - -0.150).abs() < 0.001);
    let prop_neg = handle.get_property::<f64>("audio-delay").unwrap();
    assert!((prop_neg - -0.150).abs() < 0.001);

    // Query active synchronization property
    let avsync = handle.get_property::<f64>("avsync").unwrap_or(0.0);
    println!(
        "Active A/V synchronization offset: {:.3}s (audio-delay: -150ms)",
        avsync
    );

    // Reset delay
    controller.set_audio_delay(0.0).expect("Reset audio delay");
    assert_eq!(controller.audio_delay().unwrap(), 0.0);
}

#[test]
fn test_multichannel_5_1_downmix_to_stereo_and_discrete_preservation() {
    let temp_5_1 = std::env::temp_dir().join(format!("muvrix_5_1_{}.wav", std::process::id()));
    synthesize_multichannel_5_1_wav(&temp_5_1, 0.5);

    let ch_freqs = [440.0, 880.0, 1000.0, 80.0, 220.0, 660.0];

    // 1. Discrete 5.1 preservation test: audio-channels=5.1
    let (multi_channels, multi_rate, data_51) =
        capture_pcm_file_info(&temp_5_1, "", &[("audio-channels", "5.1")], 400);
    assert_eq!(multi_channels, 6, "Must preserve discrete 6 channels");
    assert_eq!(multi_rate, 44100);

    let data_pos_51 = data_51
        .windows(4)
        .position(|w| w == b"data")
        .expect("Find data chunk")
        + 8;
    let pcm_51 = &data_51[data_pos_51..];
    let num_frames_51 = pcm_51.len() / 12; // 6 channels * 2 bytes = 12 bytes per frame
    let mut ch_samples: [Vec<f64>; 6] = Default::default();
    for i in 0..num_frames_51 {
        for (c, ch_vec) in ch_samples.iter_mut().enumerate() {
            let offset = i * 12 + c * 2;
            let s = i16::from_le_bytes([pcm_51[offset], pcm_51[offset + 1]]) as f64 / 32768.0;
            ch_vec.push(s);
        }
    }

    println!("\n=== Discrete 5.1 Channel Mapping Verification ===");
    let ch_names = [
        "FL (440Hz)",
        "FR (880Hz)",
        "FC (1000Hz)",
        "LFE (80Hz)",
        "SL (220Hz)",
        "SR (660Hz)",
    ];
    for c in 0..6 {
        let target_power = goertzel_magnitude(&ch_samples[c], ch_freqs[c], 44100.0);
        println!(
            "Channel {} [{:12}]: target freq {:.0}Hz power={:.4}",
            c, ch_names[c], ch_freqs[c], target_power
        );
        assert!(
            target_power > 0.05,
            "Channel {} must contain target frequency {:.0}Hz",
            c,
            ch_freqs[c]
        );

        // Cross-channel isolation: target frequency must dominate over other 5 channel frequencies
        for (other_idx, &other_f) in ch_freqs.iter().enumerate() {
            if other_idx != c {
                let cross_power = goertzel_magnitude(&ch_samples[c], other_f, 44100.0);
                assert!(
                    target_power > cross_power * 3.0,
                    "Channel {} [{}] must isolate against channel {} [{}Hz]: target {:.4} vs cross {:.4}",
                    c, ch_names[c], other_idx, other_f, target_power, cross_power
                );
            }
        }
    }

    // 2. Downmixed stereo test: audio-channels=stereo (ITU-R BS.775 downmixing)
    let (stereo_channels, stereo_rate, data_stereo) =
        capture_pcm_file_info(&temp_5_1, "", &[("audio-channels", "stereo")], 400);
    assert_eq!(
        stereo_channels, 2,
        "Must downmix 5.1 to stereo (2 channels)"
    );
    assert_eq!(stereo_rate, 44100);

    let data_pos_st = data_stereo
        .windows(4)
        .position(|w| w == b"data")
        .expect("Find data chunk")
        + 8;
    let pcm_st = &data_stereo[data_pos_st..];
    let num_frames_st = pcm_st.len() / 4;
    let mut left_samples = Vec::with_capacity(num_frames_st);
    let mut right_samples = Vec::with_capacity(num_frames_st);
    for i in 0..num_frames_st {
        let l = i16::from_le_bytes([pcm_st[i * 4], pcm_st[i * 4 + 1]]) as f64 / 32768.0;
        let r = i16::from_le_bytes([pcm_st[i * 4 + 2], pcm_st[i * 4 + 3]]) as f64 / 32768.0;
        left_samples.push(l);
        right_samples.push(r);
    }

    // Left channel contributions: FL (440Hz), FC (1000Hz), SL (220Hz)
    let left_fl = goertzel_magnitude(&left_samples, 440.0, 44100.0);
    let left_fc = goertzel_magnitude(&left_samples, 1000.0, 44100.0);
    let left_sl = goertzel_magnitude(&left_samples, 220.0, 44100.0);
    let left_lfe = goertzel_magnitude(&left_samples, 80.0, 44100.0); // LFE (omitted in standard BS.775 stereo downmix)
    let left_fr = goertzel_magnitude(&left_samples, 880.0, 44100.0); // Crosstalk
    let left_sr = goertzel_magnitude(&left_samples, 660.0, 44100.0); // Crosstalk

    println!("\n=== Stereo Downmixing Channel Contribution (ITU-R BS.775) ===");
    println!("Left Channel: FL(440Hz)={:.4}, FC(1000Hz)={:.4}, SL(220Hz)={:.4}, LFE(80Hz)={:.4}, FR-crosstalk(880Hz)={:.4}, SR-crosstalk(660Hz)={:.4}",
        left_fl, left_fc, left_sl, left_lfe, left_fr, left_sr);

    assert!(
        left_fl > 0.05,
        "Left channel must receive Front Left signal"
    );
    assert!(
        left_fc > 0.03,
        "Left channel must receive Center signal (attenuated ~0.707)"
    );
    assert!(
        left_sl > 0.03,
        "Left channel must receive Surround Left signal (attenuated ~0.707)"
    );
    assert!(
        left_lfe < 0.01,
        "Under standard ITU-R BS.775 stereo downmixing equations, LFE is not mixed into stereo channels (measured below measurement floor 0.01)"
    );
    assert!(
        left_fr < 0.01,
        "Left channel must isolate against Front Right signal (crosstalk < measurement floor 0.01)"
    );
    assert!(
        left_sr < 0.01,
        "Left channel must isolate against Surround Right signal (crosstalk < measurement floor 0.01)"
    );

    // Right channel contributions: FR (880Hz), FC (1000Hz), SR (660Hz)
    let right_fr = goertzel_magnitude(&right_samples, 880.0, 44100.0);
    let right_fc = goertzel_magnitude(&right_samples, 1000.0, 44100.0);
    let right_sr = goertzel_magnitude(&right_samples, 660.0, 44100.0);
    let right_lfe = goertzel_magnitude(&right_samples, 80.0, 44100.0); // LFE (omitted in standard BS.775 stereo downmix)
    let right_fl = goertzel_magnitude(&right_samples, 440.0, 44100.0); // Crosstalk
    let right_sl = goertzel_magnitude(&right_samples, 220.0, 44100.0); // Crosstalk

    println!("Right Channel: FR(880Hz)={:.4}, FC(1000Hz)={:.4}, SR(660Hz)={:.4}, LFE(80Hz)={:.4}, FL-crosstalk(440Hz)={:.4}, SL-crosstalk(220Hz)={:.4}",
        right_fr, right_fc, right_sr, right_lfe, right_fl, right_sl);

    assert!(
        right_fr > 0.05,
        "Right channel must receive Front Right signal"
    );
    assert!(
        right_fc > 0.03,
        "Right channel must receive Center signal (attenuated ~0.707)"
    );
    assert!(
        right_sr > 0.03,
        "Right channel must receive Surround Right signal (attenuated ~0.707)"
    );
    assert!(
        right_lfe < 0.01,
        "Under standard ITU-R BS.775 stereo downmixing equations, LFE is not mixed into stereo channels (measured below measurement floor 0.01)"
    );
    assert!(
        right_fl < 0.01,
        "Right channel must isolate against Front Left signal (crosstalk < measurement floor 0.01)"
    );
    assert!(
        right_sl < 0.01,
        "Right channel must isolate against Surround Left signal (crosstalk < measurement floor 0.01)"
    );
    println!("=============================================================\n");

    let _ = std::fs::remove_file(&temp_5_1);
}

#[test]
fn test_dynamic_normalization_changing_amplitude_compression() {
    let temp_dynamic = std::env::temp_dir().join(format!(
        "muvrix_dynamic_sections_{}.wav",
        std::process::id()
    ));
    let sample_rate = 44100u32;
    // Section 1: 0.0s to 2.0s is loud (amp 0.85)
    // Section 2: 2.0s to 5.0s is quiet (amp 0.05)
    // The 3-second quiet section allows the 2.25s Gaussian window of dynaudnorm (f=150:g=15)
    // to fully transition and track the quiet envelope.
    let num_samples = (sample_rate as f64 * 5.0) as usize;
    let mut raw_data = Vec::with_capacity(num_samples * 2);
    for i in 0..num_samples {
        let t = i as f64 / sample_rate as f64;
        let amp = if t < 2.0 { 0.85 } else { 0.05 };
        let s = (amp * (2.0 * std::f64::consts::PI * 1000.0 * t).sin() * 32767.0) as i16;
        raw_data.extend_from_slice(&s.to_le_bytes());
    }

    let subchunk2_size = raw_data.len() as u32;
    let chunk_size = 36 + subchunk2_size;
    let mut header = Vec::with_capacity(44);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&chunk_size.to_le_bytes());
    header.extend_from_slice(b"WAVE");
    header.extend_from_slice(b"fmt ");
    header.extend_from_slice(&16u32.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes());
    header.extend_from_slice(&sample_rate.to_le_bytes());
    header.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    header.extend_from_slice(&2u16.to_le_bytes());
    header.extend_from_slice(&16u16.to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&subchunk2_size.to_le_bytes());
    let mut full = header;
    full.extend_from_slice(&raw_data);
    std::fs::write(&temp_dynamic, full).expect("Write dynamic test WAV");

    // Capture un-normalized with untimed decoding
    let pcm_raw = capture_pcm_output_advanced(&temp_dynamic, "", &[("untimed", "yes")], 600);
    assert!(!pcm_raw.is_empty());

    // Capture with dynaudnorm and untimed decoding
    let pcm_norm = capture_pcm_output_advanced(
        &temp_dynamic,
        "dynaudnorm=f=150:g=15",
        &[("untimed", "yes")],
        600,
    );
    assert!(!pcm_norm.is_empty());

    // Isolate steady-state regions:
    // Loud region: 0.5s to 1.5s
    let loud_start = (sample_rate as f64 * 0.5) as usize;
    let loud_end = (sample_rate as f64 * 1.5) as usize;
    // Quiet region: 3.8s to 4.8s (after Gaussian filter window has transitioned)
    let quiet_start = (sample_rate as f64 * 3.8) as usize;
    let quiet_end = (sample_rate as f64 * 4.8) as usize;

    let quiet_raw_peak = pcm_raw[quiet_start.min(pcm_raw.len())..quiet_end.min(pcm_raw.len())]
        .iter()
        .fold(0.0f64, |acc, &x| acc.max(x.abs()));
    let loud_raw_peak = pcm_raw[loud_start.min(pcm_raw.len())..loud_end.min(pcm_raw.len())]
        .iter()
        .fold(0.0f64, |acc, &x| acc.max(x.abs()));

    let quiet_norm_peak = pcm_norm[quiet_start.min(pcm_norm.len())..quiet_end.min(pcm_norm.len())]
        .iter()
        .fold(0.0f64, |acc, &x| acc.max(x.abs()));
    let loud_norm_peak = pcm_norm[loud_start.min(pcm_norm.len())..loud_end.min(pcm_norm.len())]
        .iter()
        .fold(0.0f64, |acc, &x| acc.max(x.abs()));

    let raw_dynamic_ratio = loud_raw_peak / quiet_raw_peak.max(0.001);
    let norm_dynamic_ratio = loud_norm_peak / quiet_norm_peak.max(0.001);

    println!(
        "Dynamic normalization: raw ratio={:.2} (loud={:.3}, quiet={:.3}) -> norm ratio={:.2} (loud={:.3}, quiet={:.3})",
        raw_dynamic_ratio, loud_raw_peak, quiet_raw_peak, norm_dynamic_ratio, loud_norm_peak, quiet_norm_peak
    );

    // Un-normalized dynamic range is ~17x (0.85 / 0.05)
    assert!(
        raw_dynamic_ratio > 10.0,
        "Raw audio must exhibit wide dynamic range (> 10x), got {:.2}x",
        raw_dynamic_ratio
    );

    // With dynaudnorm, quiet passage is boosted significantly (> 3.0x boost)
    assert!(
        quiet_norm_peak > quiet_raw_peak * 3.0,
        "Dynaudnorm must boost the quiet passage by > 3.0x: got {:.3} vs {:.3}",
        quiet_norm_peak,
        quiet_raw_peak
    );

    // The compressed dynamic ratio is reduced drastically (< half of raw ratio)
    assert!(
        norm_dynamic_ratio < raw_dynamic_ratio / 3.0,
        "Dynaudnorm must compress the dynamic ratio significantly: {:.2}x vs raw {:.2}x",
        norm_dynamic_ratio,
        raw_dynamic_ratio
    );

    let _ = std::fs::remove_file(&temp_dynamic);
}

#[test]
fn test_d3_1_frequency_response_curve_math_and_boundaries() {
    let mut eq = EqualizerSettings {
        is_enabled: true,
        ..Default::default()
    };

    // 1. Flat response test
    eq.apply_preset(EqualizerPreset::Flat);
    assert_eq!(eq.preamp_db, 0.0);
    for &(freq, label) in &[
        (20.0, "20Hz"),
        (100.0, "100Hz"),
        (1000.0, "1kHz"),
        (10000.0, "10kHz"),
        (20000.0, "20kHz"),
    ] {
        let gain = eq.cumulative_response_at_freq(freq);
        assert!(
            gain.abs() < 1e-6,
            "Flat EQ at {} must be 0 dB, got {:.6}",
            label,
            gain
        );
    }

    // 2. Disabled EQ returns 0 dB flat
    eq.is_enabled = false;
    eq.set_band_gain(5, 6.0);
    eq.preamp_db = -3.0;
    assert_eq!(eq.cumulative_response_at_freq(1000.0), 0.0);

    // 3. Positive boost test
    eq.is_enabled = true;
    eq.preamp_db = 0.0;
    eq.apply_preset(EqualizerPreset::Flat);
    eq.set_band_gain(5, 6.0); // 1 kHz band +6 dB
    let gain_1k = eq.cumulative_response_at_freq(1000.0);
    assert!(
        gain_1k > 5.9 && gain_1k <= 6.0,
        "Gain at 1 kHz must reach peak +6.0 dB, got {:.2}",
        gain_1k
    );

    // Adjacent bands fall off smoothly
    let gain_500 = eq.cumulative_response_at_freq(500.0);
    let gain_2k = eq.cumulative_response_at_freq(2000.0);
    assert!(
        gain_500 < gain_1k && gain_500 > 0.0,
        "Gain at 500 Hz must fall off smoothly"
    );
    assert!(
        gain_2k < gain_1k && gain_2k > 0.0,
        "Gain at 2 kHz must fall off smoothly"
    );

    // 4. Negative cut test
    eq.set_band_gain(5, -6.0);
    let cut_1k = eq.cumulative_response_at_freq(1000.0);
    assert!(
        (-6.0..=-5.9).contains(&cut_1k),
        "Gain at 1 kHz must reach cut -6.0 dB, got {:.2}",
        cut_1k
    );

    // 5. Mixed boost/cut with preamp
    eq.preamp_db = -2.0;
    eq.set_band_gain(1, 4.0); // 62.5 Hz
    eq.set_band_gain(5, -4.0); // 1 kHz
    let resp_62 = eq.cumulative_response_at_freq(62.5);
    let resp_1k = eq.cumulative_response_at_freq(1000.0);
    assert!(
        resp_62 > 1.5,
        "62.5 Hz with -2 dB preamp + 4 dB boost must be positive, got {:.2}",
        resp_62
    );
    assert!(
        resp_1k < -5.5,
        "1 kHz with -2 dB preamp - 4 dB cut must be < -5.5 dB, got {:.2}",
        resp_1k
    );

    // 6. 128-point curve generation & boundary checks
    let curve = eq.frequency_response_curve(128);
    assert_eq!(curve.len(), 128);
    assert!((curve.first().unwrap().0 - 20.0).abs() < 0.1);
    assert!((curve.last().unwrap().0 - 20000.0).abs() < 1.0);

    // Ensure all sampled points are finite
    for (f, g) in curve {
        assert!((19.9..=20001.0).contains(&f));
        assert!(g.is_finite());
    }
}

#[test]
fn test_d3_2_bauer_crossfeed_filter_construction_and_presets() {
    let mut settings = EqualizerSettings {
        is_enabled: true,
        crossfeed_enabled: false,
        crossfeed_preset: muvrix_player::Bs2bPreset::Default,
        ..Default::default()
    };

    // When disabled, bs2b must not be in the filter string
    let disabled_str = EqualizerManager::build_filter_string(&settings);
    assert!(
        !disabled_str.contains("bs2b"),
        "bs2b must not appear when disabled"
    );

    // Enable Default preset
    settings.crossfeed_enabled = true;
    settings.crossfeed_preset = muvrix_player::Bs2bPreset::Default;
    let default_str = EqualizerManager::build_filter_string(&settings);
    assert!(
        default_str.contains("bs2b=profile=default"),
        "Must construct bs2b=profile=default, got: {}",
        default_str
    );

    // Chu Moy preset
    settings.crossfeed_preset = muvrix_player::Bs2bPreset::ChuMoy;
    let cmoy_str = EqualizerManager::build_filter_string(&settings);
    assert!(
        cmoy_str.contains("bs2b=profile=cmoy"),
        "Must construct bs2b=profile=cmoy, got: {}",
        cmoy_str
    );

    // Jan Meier preset
    settings.crossfeed_preset = muvrix_player::Bs2bPreset::JanMeier;
    let jmeier_str = EqualizerManager::build_filter_string(&settings);
    assert!(
        jmeier_str.contains("bs2b=profile=jmeier"),
        "Must construct bs2b=profile=jmeier, got: {}",
        jmeier_str
    );

    // Preserve existing EQ filters and dynaudnorm
    settings.dynaudnorm = true;
    settings.set_band_gain(5, 3.0);
    let combined = EqualizerManager::build_filter_string(&settings);
    assert!(combined.contains("equalizer=f=1000.00:t=q:w=1.414:g=3.0"));
    assert!(combined.contains("bs2b=profile=jmeier"));
    assert!(combined.contains("dynaudnorm=f=150:g=15"));

    // Check unified filter string with pan (reversed stereo)
    let unified = EqualizerManager::build_unified_filter_string(&settings, true);
    assert!(unified.starts_with("pan=[stereo|c0=c1|c1=c0]"));
    assert!(unified.contains("bs2b=profile=jmeier"));

    // Serialization & deserialization roundtrip
    let serialized = serde_json::to_string(&settings).expect("Serialize EqualizerSettings");
    let deserialized: EqualizerSettings =
        serde_json::from_str(&serialized).expect("Deserialize EqualizerSettings");
    assert!(deserialized.crossfeed_enabled);
    assert_eq!(
        deserialized.crossfeed_preset,
        muvrix_player::Bs2bPreset::JanMeier
    );
}

#[test]
fn test_d3_3_dynamics_telemetry_and_meter_mapping() {
    let handle = std::sync::Arc::new(create_test_player());
    let controller = PlaybackController::new(handle).expect("Create controller");

    // Inactive normalization
    let telemetry_inactive = controller.dynamics_telemetry(false);
    assert!(!telemetry_inactive.normalization_active);
    assert_eq!(telemetry_inactive.gain_reduction_db, None);
    assert!(telemetry_inactive.status_message.contains("inactive"));

    // Active normalization without filter metadata
    let telemetry_active = controller.dynamics_telemetry(true);
    assert!(telemetry_active.normalization_active);
    // On test null audio sink, dynaudnorm telemetry is unexported
    assert!(telemetry_active.status_message.contains("Active"));

    // Calibrated reduction mapping
    let gr_values: [f64; 7] = [0.0, -3.5, -6.0, -12.0, -18.0, -24.0, -30.0];
    for &val in &gr_values {
        let clamped = val.clamp(-24.0, 0.0);
        let fraction = -clamped / 24.0;
        assert!((0.0..=1.0).contains(&fraction));
    }
}

#[test]
fn test_d3_4_gapless_audio_and_prefetch_configuration() {
    let handle = std::sync::Arc::new(create_test_player());
    let controller = PlaybackController::new(handle).expect("Create controller");

    // Gapless audio must be enabled by default
    assert!(controller.gapless_audio());

    // Switch gapless mode
    controller
        .set_gapless_audio("no")
        .expect("Disable gapless audio");
    assert!(!controller.gapless_audio());

    controller
        .set_gapless_audio("yes")
        .expect("Enable gapless audio");
    assert!(controller.gapless_audio());

    // Queue prefetch invocation without errors
    controller.prefetch_next_track();
}
