//! Integration tests for restart-level settings persistence,
//! serialization, corrupt file recovery, and audio device fallback.

use muvrix_player::equalizer::{
    AudioDeviceInfo, EqualizerManager, EqualizerPreset, EqualizerSettings,
};
use muvrix_player::handle::MpvHandle;
use muvrix_player::settings::{PersistedMediaSettings, CURRENT_SETTINGS_VERSION};
use muvrix_player::shader::{
    Anime4KPreset, GamutMappingMode, ShaderManager, ShaderQuality, TargetPrimaries, TargetTrc,
    ToneMappingAlgorithm, VideoAdjustmentSettings,
};
use muvrix_player::PlaybackController;

#[test]
fn test_full_restart_persistence_lifecycle() {
    let temp_dir = std::env::temp_dir();
    let settings_file = temp_dir.join(format!(
        "muvrix_settings_test_{}_{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    // 1. Configure non-default media and audio settings
    let video = VideoAdjustmentSettings {
        brightness: 18,
        contrast: -12,
        saturation: 25,
        gamma: 4,
        hue: -8,
        sharpen: 1.5,
        deband: true,
        aspect_ratio: "16:9".to_string(),
        zoom: 0.25,
        pan_x: 0.05,
        pan_y: -0.05,
        rotate: 90,
        tone_mapping: ToneMappingAlgorithm::Bt2390,
        target_trc: TargetTrc::Srgb,
        target_prim: TargetPrimaries::Bt709,
        gamut_mapping: GamutMappingMode::Perceptual,
        hdr_compute_peak: true,
        tone_mapping_max_boost: 2.0,
        active_shaders: vec!["Anime4K_Restore_CNN_L.glsl".to_string()],
        ..Default::default()
    };

    let mut eq = EqualizerSettings {
        is_enabled: true,
        preamp_db: -2.5,
        dynaudnorm: true,
        active_preset: EqualizerPreset::Custom,
        pitch_correction: false,
        audio_delay: 0.125,
        channel_layout: "5.1".to_string(),
        volume_boost: 140.0,
        ..Default::default()
    };
    eq.set_band_gain(0, 5.0);
    eq.set_band_q(0, 2.0);
    eq.set_band_gain(5, -3.5);
    eq.set_band_q(5, 0.8);
    eq.set_band_gain(9, 4.0);
    eq.save_custom_preset("Studio Monitor");

    let original = PersistedMediaSettings {
        schema_version: CURRENT_SETTINGS_VERSION,
        video_adjustments: video,
        anime4k_preset: Anime4KPreset::ModeCA,
        anime4k_quality: ShaderQuality::High,
        equalizer: eq,
        selected_audio_device: "pipewire/alsa_output.pci-0000_00_1f.3.HiFi__Headphones__sink"
            .to_string(),
        volume: 115.0,
        subtitle_styling: Default::default(),
        theme_mode: Default::default(),
        accent_color: Default::default(),
        ..Default::default()
    };

    // 2. Persist to disk
    original.save(Some(&settings_file)).expect("Save settings");
    assert!(settings_file.exists());

    // 3. Simulate application restart: load from disk
    let loaded = PersistedMediaSettings::load(Some(&settings_file));

    // 4. Assert full equality of loaded configuration
    assert_eq!(loaded.schema_version, CURRENT_SETTINGS_VERSION);
    assert_eq!(loaded.volume, 115.0);
    assert_eq!(loaded.anime4k_preset, Anime4KPreset::ModeCA);
    assert_eq!(loaded.anime4k_quality, ShaderQuality::High);

    // Video adjustments
    assert_eq!(loaded.video_adjustments.brightness, 18);
    assert_eq!(loaded.video_adjustments.contrast, -12);
    assert_eq!(loaded.video_adjustments.saturation, 25);
    assert_eq!(
        loaded.video_adjustments.tone_mapping,
        ToneMappingAlgorithm::Bt2390
    );
    assert_eq!(loaded.video_adjustments.target_trc, TargetTrc::Srgb);
    assert_eq!(
        loaded.video_adjustments.gamut_mapping,
        GamutMappingMode::Perceptual
    );
    assert!(loaded.video_adjustments.deband);

    // Equalizer & Audio DSP
    assert!(loaded.equalizer.is_enabled);
    assert_eq!(loaded.equalizer.preamp_db, -2.5);
    assert_eq!(loaded.equalizer.gains[0], 5.0);
    assert_eq!(loaded.equalizer.bands[0].q_factor, 2.0);
    assert_eq!(loaded.equalizer.gains[5], -3.5);
    assert_eq!(loaded.equalizer.bands[5].q_factor, 0.8);
    assert_eq!(loaded.equalizer.gains[9], 4.0);
    assert!(loaded.equalizer.dynaudnorm);
    assert!(!loaded.equalizer.pitch_correction);
    assert!((loaded.equalizer.audio_delay - 0.125).abs() < 0.001);
    assert_eq!(loaded.equalizer.channel_layout, "5.1");
    assert_eq!(loaded.equalizer.volume_boost, 140.0);

    // Custom Presets
    assert_eq!(loaded.equalizer.custom_presets.len(), 1);
    assert_eq!(loaded.equalizer.custom_presets[0].name, "Studio Monitor");
    assert_eq!(loaded.equalizer.custom_presets[0].preamp_db, -2.5);

    // Device selection
    assert_eq!(
        loaded.selected_audio_device,
        "pipewire/alsa_output.pci-0000_00_1f.3.HiFi__Headphones__sink"
    );

    let _ = std::fs::remove_file(&settings_file);
}

#[test]
fn test_settings_missing_or_corrupted_file_falls_back_safely() {
    let temp_dir = std::env::temp_dir();

    // 1. Missing file -> returns default
    let non_existent = temp_dir.join("nonexistent_settings_file_9999.json");
    let fallback = PersistedMediaSettings::load(Some(&non_existent));
    assert_eq!(fallback.schema_version, CURRENT_SETTINGS_VERSION);
    assert_eq!(fallback.anime4k_preset, Anime4KPreset::Disabled);
    assert_eq!(fallback.selected_audio_device, "auto");
    assert!(!fallback.equalizer.is_enabled);

    // 2. Corrupted file -> returns default
    let corrupt_file = temp_dir.join(format!("corrupt_settings_{}.json", std::process::id()));
    std::fs::write(&corrupt_file, b"NOT VALID JSON {[[}").unwrap();

    let recovered = PersistedMediaSettings::load(Some(&corrupt_file));
    assert_eq!(recovered.schema_version, CURRENT_SETTINGS_VERSION);
    assert_eq!(recovered.anime4k_preset, Anime4KPreset::Disabled);
    assert_eq!(recovered.selected_audio_device, "auto");

    let _ = std::fs::remove_file(&corrupt_file);
}

#[test]
fn test_disconnected_audio_device_fallback_to_auto() {
    let mut settings = PersistedMediaSettings {
        selected_audio_device: "usb-dac-disconnected-model-999".to_string(),
        ..Default::default()
    };

    let available = vec![
        AudioDeviceInfo {
            name: "auto".to_string(),
            description: "Auto-select".to_string(),
        },
        AudioDeviceInfo {
            name: "pipewire".to_string(),
            description: "Default pipewire".to_string(),
        },
    ];

    // Disconnected device must fall back to "auto"
    settings.validate_and_fallback(&available);
    assert_eq!(settings.selected_audio_device, "auto");

    // When device is present, selection is preserved
    let mut settings_valid = PersistedMediaSettings {
        selected_audio_device: "pipewire".to_string(),
        ..Default::default()
    };
    settings_valid.validate_and_fallback(&available);
    assert_eq!(settings_valid.selected_audio_device, "pipewire");
}

#[test]
#[allow(clippy::chunks_exact_to_as_chunks)]
fn test_real_application_settings_persistence_and_libmpv_restoration_lifecycle() {
    let temp_dir = std::env::temp_dir();
    let settings_file = temp_dir.join(format!(
        "muvrix_live_restore_{}_{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    // 1. Create original application settings with non-default state
    let video = VideoAdjustmentSettings {
        brightness: 12,
        contrast: 8,
        saturation: 15,
        gamma: 3,
        hue: -5,
        sharpen: 1.0,
        deband: true,
        aspect_ratio: "16:9".to_string(),
        zoom: 0.1,
        pan_x: 0.0,
        pan_y: 0.0,
        rotate: 0,
        tone_mapping: ToneMappingAlgorithm::Bt2390,
        target_trc: TargetTrc::Srgb,
        target_prim: TargetPrimaries::Bt709,
        gamut_mapping: GamutMappingMode::Perceptual,
        hdr_compute_peak: true,
        tone_mapping_max_boost: 2.0,
        active_shaders: vec![],
        ..Default::default()
    };

    let mut eq = EqualizerSettings {
        is_enabled: true,
        preamp_db: -3.0,
        dynaudnorm: true,
        active_preset: EqualizerPreset::BassBoost,
        pitch_correction: true,
        audio_delay: 0.250,
        channel_layout: "stereo".to_string(),
        volume_boost: 130.0,
        ..Default::default()
    };
    eq.apply_preset(EqualizerPreset::BassBoost);
    eq.save_custom_preset("My Rock EQ");

    // Detect an available non-default audio device if present on the test machine
    let probe_handle = MpvHandle::new().unwrap();
    probe_handle.set_option_string("vo", "null").unwrap();
    probe_handle.initialize().unwrap();
    let probed_devices = EqualizerManager::query_audio_devices(&probe_handle).unwrap_or_default();
    println!("Probed audio devices count: {}", probed_devices.len());
    for d in &probed_devices {
        println!(" - Device: '{}' ({})", d.name, d.description);
    }
    // Select a demonstrably non-default device (e.g. specific physical sink or non-generic backend)
    let non_default_sink = probed_devices.iter().find(|d| {
        d.name != "auto" && d.name != "pipewire" && d.name != "pulse" && d.name != "alsa"
    });
    let target_device = if let Some(dev) = non_default_sink {
        dev.name.clone()
    } else if probed_devices.len() > 1 {
        probed_devices[1].name.clone()
    } else {
        "auto".to_string()
    };
    println!(
        "Selected non-default target audio device: '{}'",
        target_device
    );
    drop(probe_handle);

    let original = PersistedMediaSettings {
        schema_version: CURRENT_SETTINGS_VERSION,
        video_adjustments: video,
        anime4k_preset: Anime4KPreset::ModeA,
        anime4k_quality: ShaderQuality::Balanced,
        equalizer: eq,
        selected_audio_device: target_device.clone(),
        volume: 115.0,
        subtitle_styling: Default::default(),
        theme_mode: Default::default(),
        accent_color: Default::default(),
        ..Default::default()
    };

    // 2. Persist to disk
    original
        .save(Some(&settings_file))
        .expect("Save settings to disk");

    // 3. Simulate new application process launch: create a fresh MpvHandle
    let handle = std::sync::Arc::new({
        let h = MpvHandle::new().expect("Create fresh MpvHandle");
        h.set_option_string("vo", "null").unwrap();
        h.set_option_string("ao", "null").unwrap();
        h.initialize().unwrap();
        h
    });
    let mut controller =
        PlaybackController::new(handle.clone()).expect("Create PlaybackController");

    // 4. Load persisted settings from disk
    let loaded = PersistedMediaSettings::load(Some(&settings_file));

    // 5. Restore settings into the live libmpv handle using engine managers
    ShaderManager::apply_adjustments(&handle, &loaded.video_adjustments)
        .expect("Apply adjustments");
    ShaderManager::apply_anime4k_mode(&handle, loaded.anime4k_preset, loaded.anime4k_quality, None)
        .expect("Apply Anime4K");
    EqualizerManager::apply_to_handle(&handle, &loaded.equalizer).expect("Apply EQ to handle");
    EqualizerManager::apply_dsp_options(&handle, &loaded.equalizer).expect("Apply DSP options");
    controller
        .set_volume_boost(loaded.equalizer.volume_boost)
        .expect("Apply volume boost ceiling (volume-max)");
    controller
        .set_volume(loaded.volume)
        .expect("Apply persisted software playback volume");
    EqualizerManager::set_audio_device_with_fallback(&handle, &loaded.selected_audio_device)
        .expect("Apply audio device");

    // 6. Query and assert properties directly on the live libmpv engine
    let b: i64 = handle.get_property("brightness").unwrap();
    let c: i64 = handle.get_property("contrast").unwrap();
    let s: i64 = handle.get_property("saturation").unwrap();
    assert_eq!(b, 12);
    assert_eq!(c, 8);
    assert_eq!(s, 15);
    assert_eq!(
        handle.get_property_string("tone-mapping").unwrap(),
        "bt.2390"
    );
    assert_eq!(handle.get_property_string("target-trc").unwrap(), "srgb");

    // Verify Anime4K shaders in glsl-shaders
    let live_shaders = handle.get_property_string("glsl-shaders").unwrap();
    assert!(
        live_shaders.contains("Anime4K_Clamp_Highlights.glsl"),
        "Shaders must contain Clamp_Highlights: {live_shaders}"
    );
    assert!(
        live_shaders.contains("Anime4K_Restore_CNN_M.glsl"),
        "Shaders must contain Restore_CNN_M: {live_shaders}"
    );

    // Verify Audio filters
    let live_af = handle.get_property_string("af").unwrap();
    assert!(
        live_af.contains("-3.0dB") && live_af.contains("volume"),
        "af must contain preamp: {live_af}"
    );
    assert!(
        live_af.contains("dynaudnorm"),
        "af must contain dynaudnorm: {live_af}"
    );
    assert!(
        live_af.contains("31.25"),
        "af must contain 31.25 Hz bass band: {live_af}"
    );

    // Verify DSP properties
    let audio_delay: f64 = handle.get_property("audio-delay").unwrap();
    assert!((audio_delay - 0.250).abs() < 0.01);
    assert_eq!(
        handle
            .get_property_string("audio-pitch-correction")
            .unwrap(),
        "yes"
    );
    assert_eq!(
        handle.get_property_string("audio-channels").unwrap(),
        "stereo"
    );

    // Verify volume boost maximum ceiling and software playback volume
    assert_eq!(controller.volume_boost_max(), 130.0);
    let vol_max: f64 = handle.get_property("volume-max").unwrap();
    assert!(
        (vol_max - 130.0).abs() < 0.1,
        "volume-max must be 130.0, got {vol_max}"
    );

    assert_eq!(controller.volume(), 115.0);
    let vol: f64 = handle.get_property("volume").unwrap();
    assert!(
        (vol - 115.0).abs() < 0.1,
        "software volume must be 115.0, got {vol}"
    );

    // Verify audio device matches target_device
    assert_eq!(
        handle.get_property_string("audio-device").unwrap(),
        target_device
    );

    // 7. Verify measurable software gain restoration via PCM output capture
    // Synthesize a known 1000 Hz sine wave with amplitude 0.50
    let temp_gain_wav = temp_dir.join(format!("muvrix_gain_test_{}.wav", std::process::id()));
    {
        let sample_rate = 44100u32;
        let num_samples = (sample_rate as f64 * 0.4) as usize;
        let mut raw = Vec::with_capacity(num_samples * 2);
        for i in 0..num_samples {
            let t = i as f64 / sample_rate as f64;
            let s = (0.50 * (2.0 * std::f64::consts::PI * 1000.0 * t).sin() * 32767.0) as i16;
            raw.extend_from_slice(&s.to_le_bytes());
        }
        let subchunk2_size = raw.len() as u32;
        let chunk_size = 36 + subchunk2_size;
        let mut header = Vec::with_capacity(44);
        header.extend_from_slice(b"RIFF");
        header.extend_from_slice(&chunk_size.to_le_bytes());
        header.extend_from_slice(b"WAVEfmt ");
        header.extend_from_slice(&16u32.to_le_bytes());
        header.extend_from_slice(&1u16.to_le_bytes());
        header.extend_from_slice(&1u16.to_le_bytes()); // Mono
        header.extend_from_slice(&sample_rate.to_le_bytes());
        header.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        header.extend_from_slice(&2u16.to_le_bytes());
        header.extend_from_slice(&16u16.to_le_bytes());
        header.extend_from_slice(b"data");
        header.extend_from_slice(&subchunk2_size.to_le_bytes());
        header.extend_from_slice(&raw);
        std::fs::write(&temp_gain_wav, header).unwrap();
    }

    // Capture PCM at baseline volume 100%
    let baseline_out = temp_dir.join(format!("baseline_out_{}.wav", std::process::id()));
    {
        let p = MpvHandle::new().unwrap();
        p.set_option_string("vo", "null").unwrap();
        p.set_option_string("ao", "pcm").unwrap();
        p.set_option_string("audio-format", "s16").unwrap();
        p.set_option_string("ao-pcm-file", baseline_out.to_str().unwrap())
            .unwrap();
        p.set_option_string("volume", "100").unwrap();
        p.initialize().unwrap();
        p.command(&["loadfile", temp_gain_wav.to_str().unwrap()])
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    let data_base = std::fs::read(&baseline_out).unwrap();
    let _ = std::fs::remove_file(&baseline_out);
    let pcm_base_pos = data_base.windows(4).position(|w| w == b"data").unwrap() + 8;
    let peak_base = data_base[pcm_base_pos..]
        .chunks_exact(2)
        .map(|c| (i16::from_le_bytes([c[0], c[1]]) as f64 / 32768.0).abs())
        .fold(0.0f64, |acc, x| acc.max(x));

    // Capture PCM using the restored configuration (volume 115%, volume-max 130%)
    let restored_out = temp_dir.join(format!("restored_out_{}.wav", std::process::id()));
    {
        let p = MpvHandle::new().unwrap();
        p.set_option_string("vo", "null").unwrap();
        p.set_option_string("ao", "pcm").unwrap();
        p.set_option_string("audio-format", "s16").unwrap();
        p.set_option_string("ao-pcm-file", restored_out.to_str().unwrap())
            .unwrap();
        p.set_option_string(
            "volume-max",
            &format!("{:.1}", loaded.equalizer.volume_boost),
        )
        .unwrap();
        p.set_option_string("volume", &format!("{:.1}", loaded.volume))
            .unwrap();
        p.initialize().unwrap();
        p.command(&["loadfile", temp_gain_wav.to_str().unwrap()])
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    let data_restored = std::fs::read(&restored_out).unwrap();
    let _ = std::fs::remove_file(&restored_out);
    let _ = std::fs::remove_file(&temp_gain_wav);
    let pcm_rest_pos = data_restored.windows(4).position(|w| w == b"data").unwrap() + 8;
    let peak_restored = data_restored[pcm_rest_pos..]
        .chunks_exact(2)
        .map(|c| (i16::from_le_bytes([c[0], c[1]]) as f64 / 32768.0).abs())
        .fold(0.0f64, |acc, x| acc.max(x));

    let measured_gain_ratio = peak_restored / peak_base.max(0.001);
    let expected_gain_ratio = 1.15f64.powi(3); // mpv cubic volume curve
    println!(
        "\nSoftware Gain Restoration Measurement: baseline peak={:.4}, restored (115%) peak={:.4}, measured ratio={:.4}, theoretical (1.15)^3={:.4}",
        peak_base, peak_restored, measured_gain_ratio, expected_gain_ratio
    );
    assert!(
        (measured_gain_ratio - expected_gain_ratio).abs() < 0.05,
        "Restored volume 115% must scale output samples by (1.15)^3 = {:.4}, got {:.4}",
        expected_gain_ratio,
        measured_gain_ratio
    );

    let _ = std::fs::remove_file(&settings_file);
}
