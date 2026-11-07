//! Integration tests for Video Shaders and Audio Equalizer in muvrix-player.

use muvrix_player::equalizer::*;
use muvrix_player::handle::MpvHandle;
use muvrix_player::shader::*;

#[test]
fn test_video_adjustments_and_reset() {
    let handle = MpvHandle::new().expect("Create handle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().unwrap();

    let settings = VideoAdjustmentSettings {
        brightness: 15,
        contrast: -10,
        saturation: 25,
        gamma: 5,
        hue: -15,
        sharpen: 1.5,
        deband: true,
        active_shaders: vec![],
    };

    ShaderManager::apply_adjustments(&handle, &settings).expect("Apply adjustments");

    assert_eq!(handle.get_property::<i64>("brightness").unwrap(), 15);
    assert_eq!(handle.get_property::<i64>("contrast").unwrap(), -10);
    assert_eq!(handle.get_property::<i64>("saturation").unwrap(), 25);
    assert_eq!(handle.get_property::<i64>("gamma").unwrap(), 5);
    assert_eq!(handle.get_property::<i64>("hue").unwrap(), -15);
    assert!((handle.get_property::<f64>("sharpen").unwrap() - 1.5).abs() < 0.01);
    assert!(handle.get_property::<bool>("deband").unwrap());

    // Reset
    ShaderManager::reset_adjustments(&handle).expect("Reset adjustments");
    assert_eq!(handle.get_property::<i64>("brightness").unwrap(), 0);
    assert_eq!(handle.get_property::<i64>("contrast").unwrap(), 0);
    assert_eq!(handle.get_property::<i64>("saturation").unwrap(), 0);
    assert!(!handle.get_property::<bool>("deband").unwrap());
}

#[test]
fn test_anime4k_shader_preset_names() {
    let mode_a = ShaderManager::get_anime4k_shader_names(Anime4KPreset::ModeA);
    assert_eq!(mode_a.len(), 5);
    assert_eq!(mode_a[0], "Anime4K_Clamp_Highlights.glsl");
    assert_eq!(mode_a[1], "Anime4K_Restore_CNN_M.glsl");

    let disabled = ShaderManager::get_anime4k_shader_names(Anime4KPreset::Disabled);
    assert!(disabled.is_empty());
}

#[test]
fn test_equalizer_filter_string_construction() {
    // 1. Disabled equalizer -> empty filter string
    let flat = EqualizerSettings::default();
    assert_eq!(EqualizerManager::build_filter_string(&flat), "");

    // 2. Bass Boost preset
    let bass_boost = EqualizerSettings {
        is_enabled: true,
        preamp_db: -2.0,
        gains: EqualizerPreset::BassBoost.default_gains(),
        dynaudnorm: false,
        active_preset: EqualizerPreset::BassBoost,
    };

    let filter_str = EqualizerManager::build_filter_string(&bass_boost);
    assert!(filter_str.contains("volume=volume=-2.0dB"));
    assert!(filter_str.contains("equalizer=f=31.25:t=q:w=1.414:g=5.0"));
    assert!(filter_str.contains("equalizer=f=62.50:t=q:w=1.414:g=4.0"));

    // 3. Dynamic Audio Normalizer
    let mut with_dyn = bass_boost.clone();
    with_dyn.dynaudnorm = true;
    let dyn_filter_str = EqualizerManager::build_filter_string(&with_dyn);
    assert!(dyn_filter_str.contains("dynaudnorm=f=150:g=15"));
}

#[test]
fn test_equalizer_application_to_mpv_handle() {
    let handle = MpvHandle::new().expect("Create handle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().unwrap();

    let settings = EqualizerSettings {
        is_enabled: true,
        preamp_db: 1.5,
        gains: EqualizerPreset::VocalClarity.default_gains(),
        dynaudnorm: false,
        active_preset: EqualizerPreset::VocalClarity,
    };

    EqualizerManager::apply_to_handle(&handle, &settings).expect("Apply equalizer to handle");
    let current_af = handle.get_property_string("af").unwrap();
    assert!(current_af.contains("equalizer="));

    EqualizerManager::reset(&handle).expect("Reset equalizer");
    let reset_af = handle.get_property_string("af").unwrap();
    assert_eq!(reset_af, "");
}

#[test]
fn test_audio_device_enumeration() {
    let handle = MpvHandle::new().expect("Create handle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().unwrap();

    let devices = EqualizerManager::query_audio_devices(&handle).expect("Query devices");
    assert!(!devices.is_empty());
    assert_eq!(devices[0].name, "auto");
}

#[test]
fn test_equalizer_state_transitions_and_filter_consistency() {
    let handle = MpvHandle::new().expect("Create handle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().unwrap();

    // 1. Flat preset guarantees 0 dB preamp and 0 dB band gains
    let mut eq = EqualizerSettings::default();
    assert_eq!(eq.active_preset, EqualizerPreset::Flat);
    assert_eq!(eq.preamp_db, 0.0);
    assert_eq!(eq.gains, [0.0; 10]);
    // When Flat with 0.0 gains, filter string should contain zero equalizer filters
    assert_eq!(EqualizerManager::build_filter_string(&eq), "");

    // 2. Bass Boost preset
    eq.active_preset = EqualizerPreset::BassBoost;
    eq.gains = EqualizerPreset::BassBoost.default_gains();
    eq.is_enabled = true;
    let bb_filter = EqualizerManager::build_filter_string(&eq);
    assert!(bb_filter.contains("equalizer=f=31.25"));

    // 3. Vocal Clarity preset
    eq.active_preset = EqualizerPreset::VocalClarity;
    eq.gains = EqualizerPreset::VocalClarity.default_gains();
    let vocal_filter = EqualizerManager::build_filter_string(&eq);
    assert!(vocal_filter.contains("equalizer=f=1000.00"));

    // 4. Custom changes: modifying a band or preamp switches preset to Custom
    eq.gains[0] = 7.5;
    eq.active_preset = EqualizerPreset::Custom;
    assert_eq!(eq.active_preset, EqualizerPreset::Custom);
    eq.preamp_db = 1.5;
    assert_eq!(eq.active_preset, EqualizerPreset::Custom);

    // 5. Reset to Flat
    eq.gains = EqualizerPreset::Flat.default_gains();
    eq.preamp_db = 0.0;
    eq.active_preset = EqualizerPreset::Flat;
    assert_eq!(eq.active_preset, EqualizerPreset::Flat);
    assert_eq!(eq.preamp_db, 0.0);
    assert_eq!(eq.gains, [0.0; 10]);

    // 6. Filter application to mpv handle
    EqualizerManager::apply_to_handle(&handle, &eq).expect("Apply flat");
    assert_eq!(handle.get_property_string("af").unwrap(), "");

    // 7. Error recovery after invalid filter injection
    let _ = handle.set_property_string("af", "invalid_filter_name=xyz");
    EqualizerManager::reset(&handle).expect("Reset after failure");
    assert_eq!(handle.get_property_string("af").unwrap(), "");
}
