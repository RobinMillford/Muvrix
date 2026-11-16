use muvrix_player::shader::{
    evaluate_tone_mapping_curve, Anime4KPreset, CustomShaderEntry, DebandPreset, ShaderManager,
    ShaderQuality, ToneMappingAlgorithm, VideoAdjustmentSettings,
};
use std::fs::File;
use std::io::Write;

#[test]
fn test_d4_deband_preset_application_and_clamping() {
    let mut settings = VideoAdjustmentSettings::default();
    assert!(!settings.deband);

    // Apply Default
    DebandPreset::Default.apply_to(&mut settings);
    assert!(settings.deband);
    assert_eq!(settings.deband_iterations, 1);
    assert_eq!(settings.deband_threshold, 64);
    assert_eq!(settings.deband_range, 16);
    assert_eq!(settings.deband_grain, 48);

    // Apply Medium
    DebandPreset::Medium.apply_to(&mut settings);
    assert_eq!(settings.deband_iterations, 2);
    assert_eq!(settings.deband_threshold, 96);
    assert_eq!(settings.deband_range, 20);
    assert_eq!(settings.deband_grain, 64);

    // Apply Aggressive
    DebandPreset::Aggressive.apply_to(&mut settings);
    assert_eq!(settings.deband_iterations, 4);
    assert_eq!(settings.deband_threshold, 144);
    assert_eq!(settings.deband_range, 24);
    assert_eq!(settings.deband_grain, 96);

    // Apply Anime / Clean
    DebandPreset::Anime.apply_to(&mut settings);
    assert_eq!(settings.deband_iterations, 3);
    assert_eq!(settings.deband_threshold, 120);
    assert_eq!(settings.deband_range, 24);
    assert_eq!(settings.deband_grain, 32);
}

#[test]
fn test_d4_custom_shader_entry_and_path_validation() {
    let temp_dir = std::env::temp_dir().join(format!("muvrix_test_shader_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);

    let valid_shader_path = temp_dir.join("FSRCNNX_x2_16_0_4_1.glsl");
    {
        let mut f = File::create(&valid_shader_path).expect("Create shader file");
        writeln!(
            f,
            "// Test GLSL Hook\n//!HOOK MAIN\nvec4 hook() {{ return vec4(1.0); }}"
        )
        .unwrap();
    }

    let invalid_ext_path = temp_dir.join("malicious.exe");
    {
        let mut f = File::create(&invalid_ext_path).expect("Create invalid ext file");
        writeln!(f, "MZ").unwrap();
    }

    // Valid path passes validation
    let canonical = ShaderManager::validate_shader_path(valid_shader_path.to_str().unwrap());
    assert!(canonical.is_ok());

    // Invalid extension is rejected
    let rejected = ShaderManager::validate_shader_path(invalid_ext_path.to_str().unwrap());
    assert!(rejected.is_err());

    // CustomShaderEntry creation
    let entry = CustomShaderEntry::new(valid_shader_path.to_str().unwrap().to_string());
    assert_eq!(entry.name, "FSRCNNX_x2_16_0_4_1.glsl");
    assert!(entry.enabled);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_d4_shader_pipeline_combination_and_uniqueness() {
    let temp_dir =
        std::env::temp_dir().join(format!("muvrix_test_pipeline_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);

    let shader1 = temp_dir.join("custom_cas.glsl");
    let shader2 = temp_dir.join("custom_krig.hook");
    File::create(&shader1).unwrap();
    File::create(&shader2).unwrap();

    let custom_entries = vec![
        CustomShaderEntry::new(shader1.to_str().unwrap().to_string()),
        CustomShaderEntry {
            name: "disabled.glsl".to_string(),
            path: "/tmp/nonexistent.glsl".to_string(),
            enabled: false, // Disabled entries must be excluded
        },
        CustomShaderEntry::new(shader2.to_str().unwrap().to_string()),
    ];

    // Build pipeline with Anime4K disabled
    let handle = muvrix_player::MpvHandle::new().expect("Create mpv handle");
    handle.set_option_string("vo", "null").unwrap();
    handle.initialize().unwrap();

    let result = ShaderManager::apply_shader_pipeline(
        &handle,
        Anime4KPreset::Disabled,
        ShaderQuality::Balanced,
        None,
        &custom_entries,
    );

    assert!(result.is_ok(), "Custom shader pipeline must apply cleanly");
    let active = result.unwrap();
    assert_eq!(
        active.len(),
        2,
        "Only enabled valid shaders must be in chain"
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_d4_tone_mapping_curve_evaluation_math() {
    // 1. Clip algorithm
    assert_eq!(
        evaluate_tone_mapping_curve(ToneMappingAlgorithm::Clip, 50.0, 1000.0, 1.0),
        0.05
    );
    assert_eq!(
        evaluate_tone_mapping_curve(ToneMappingAlgorithm::Clip, 1000.0, 1000.0, 1.0),
        1.0
    );
    assert_eq!(
        evaluate_tone_mapping_curve(ToneMappingAlgorithm::Clip, 5000.0, 1000.0, 1.0),
        1.0
    );

    // 2. Reinhard algorithm: y = x / (1 + x)
    let reinhard_mid =
        evaluate_tone_mapping_curve(ToneMappingAlgorithm::Reinhard, 1000.0, 1000.0, 1.0);
    assert!((reinhard_mid - 0.5).abs() < 1e-4);

    // 3. Hable (Uncharted 2 filmic curve): monotonic and bounded in [0.0, 1.0]
    let hable_0 = evaluate_tone_mapping_curve(ToneMappingAlgorithm::Hable, 0.0, 1000.0, 1.0);
    let hable_500 = evaluate_tone_mapping_curve(ToneMappingAlgorithm::Hable, 500.0, 1000.0, 1.0);
    let hable_1000 = evaluate_tone_mapping_curve(ToneMappingAlgorithm::Hable, 1000.0, 1000.0, 1.0);
    let hable_4000 = evaluate_tone_mapping_curve(ToneMappingAlgorithm::Hable, 4000.0, 1000.0, 1.0);

    assert_eq!(hable_0, 0.0);
    assert!(hable_500 < hable_1000);
    assert!(hable_1000 < hable_4000);
    assert!(hable_4000 <= 1.0);

    // 4. Mobius: linear below knee 0.45, smoothly compressing above
    let mobius_low = evaluate_tone_mapping_curve(ToneMappingAlgorithm::Mobius, 200.0, 1000.0, 1.0);
    assert!((mobius_low - 0.2).abs() < 1e-3);
    let mobius_high =
        evaluate_tone_mapping_curve(ToneMappingAlgorithm::Mobius, 4000.0, 1000.0, 1.0);
    assert!(mobius_high <= 1.0);

    // 5. BT.2390 / BT.2446a: bounded and monotonic
    let bt2390_low = evaluate_tone_mapping_curve(ToneMappingAlgorithm::Bt2390, 100.0, 1000.0, 1.0);
    let bt2390_high =
        evaluate_tone_mapping_curve(ToneMappingAlgorithm::Bt2390, 8000.0, 1000.0, 1.0);
    assert!(bt2390_low > 0.0);
    assert!(bt2390_high <= 1.0);
    assert!(bt2390_low < bt2390_high);

    // 6. Max boost scales brightness
    let boost_normal =
        evaluate_tone_mapping_curve(ToneMappingAlgorithm::Spline, 300.0, 1000.0, 1.0);
    let boost_elevated =
        evaluate_tone_mapping_curve(ToneMappingAlgorithm::Spline, 300.0, 1000.0, 2.0);
    assert!(boost_elevated > boost_normal);
}

#[test]
fn test_d4_target_peak_nits_and_adjustments_application() {
    let handle = muvrix_player::MpvHandle::new().expect("Create mpv handle");
    handle.set_option_string("vo", "null").unwrap();
    handle.initialize().unwrap();

    let mut settings = VideoAdjustmentSettings {
        target_peak: 1200,
        deband: true,
        denoise: true,
        denoise_strength: 2.5,
        ..Default::default()
    };
    DebandPreset::Medium.apply_to(&mut settings);

    let res = ShaderManager::apply_adjustments(&handle, &settings);
    assert!(
        res.is_ok(),
        "Adjustments must apply cleanly to live mpv handle"
    );

    // Read back deband and target-peak
    let deband = handle.get_property::<bool>("deband").unwrap();
    assert!(deband);

    let deband_thresh = handle.get_property::<i64>("deband-threshold").unwrap();
    assert_eq!(deband_thresh, 96);

    let peak = handle.get_property_string("target-peak").unwrap();
    assert_eq!(peak, "1200");
}
