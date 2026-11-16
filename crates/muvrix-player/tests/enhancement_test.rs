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
        aspect_ratio: "16:9".to_string(),
        zoom: 0.5,
        pan_x: 0.1,
        pan_y: -0.1,
        rotate: 90,
        tone_mapping: ToneMappingAlgorithm::Bt2390,
        target_trc: TargetTrc::Srgb,
        target_prim: TargetPrimaries::Bt709,
        gamut_mapping: GamutMappingMode::Perceptual,
        hdr_compute_peak: true,
        tone_mapping_max_boost: 1.5,
        ..Default::default()
    };

    ShaderManager::apply_adjustments(&handle, &settings).expect("Apply adjustments");

    assert_eq!(handle.get_property::<i64>("brightness").unwrap(), 15);
    assert_eq!(handle.get_property::<i64>("contrast").unwrap(), -10);
    assert_eq!(handle.get_property::<i64>("saturation").unwrap(), 25);
    assert_eq!(handle.get_property::<i64>("gamma").unwrap(), 5);
    assert_eq!(handle.get_property::<i64>("hue").unwrap(), -15);
    assert!((handle.get_property::<f64>("sharpen").unwrap() - 1.5).abs() < 0.01);
    assert!(handle.get_property::<bool>("deband").unwrap());
    let aspect = handle.get_property_string("video-aspect-override").unwrap();
    assert!(aspect.starts_with("1.777") || aspect == "16:9");
    assert!((handle.get_property::<f64>("video-zoom").unwrap() - 0.5).abs() < 0.01);
    assert_eq!(handle.get_property::<i64>("video-rotate").unwrap(), 90);
    assert_eq!(
        handle.get_property_string("tone-mapping").unwrap(),
        "bt.2390"
    );
    assert_eq!(handle.get_property_string("target-trc").unwrap(), "srgb");
    assert_eq!(handle.get_property_string("target-prim").unwrap(), "bt.709");
    assert_eq!(
        handle.get_property_string("gamut-mapping-mode").unwrap(),
        "perceptual"
    );

    // Reset
    ShaderManager::reset_adjustments(&handle).expect("Reset adjustments");
    assert_eq!(handle.get_property::<i64>("brightness").unwrap(), 0);
    assert_eq!(handle.get_property::<i64>("contrast").unwrap(), 0);
    assert_eq!(handle.get_property::<i64>("saturation").unwrap(), 0);
    assert!(!handle.get_property::<bool>("deband").unwrap());
    let reset_aspect = handle.get_property_string("video-aspect-override").unwrap();
    assert!(reset_aspect.starts_with("-1"));
    assert_eq!(handle.get_property::<i64>("video-rotate").unwrap(), 0);
}

#[test]
fn test_anime4k_quality_modes_and_chain_structure() {
    for quality in [
        ShaderQuality::Fast,
        ShaderQuality::Balanced,
        ShaderQuality::High,
    ] {
        let q = quality.suffix();

        // Mode A: 5 unique shaders reaching 4x scaling
        let mode_a =
            ShaderManager::get_anime4k_shader_names_with_quality(Anime4KPreset::ModeA, quality);
        assert_eq!(mode_a.len(), 5);
        assert_eq!(mode_a[0], "Anime4K_Clamp_Highlights.glsl");
        assert_eq!(mode_a[1], format!("Anime4K_Restore_CNN_{q}.glsl"));
        assert_eq!(mode_a[2], format!("Anime4K_Upscale_CNN_x2_{q}.glsl"));
        assert_eq!(mode_a[3], "Anime4K_AutoDownscalePre_x2.glsl");
        assert_eq!(mode_a[4], format!("Anime4K_Upscale_CNN_x2_Pass2_{q}.glsl"));
        assert!(ShaderManager::validate_shader_chain_uniqueness(&mode_a).is_ok());

        // Mode B: 5 unique shaders with soft restoration reaching 4x scaling
        let mode_b =
            ShaderManager::get_anime4k_shader_names_with_quality(Anime4KPreset::ModeB, quality);
        assert_eq!(mode_b.len(), 5);
        assert_eq!(mode_b[0], "Anime4K_Clamp_Highlights.glsl");
        assert_eq!(mode_b[1], format!("Anime4K_Restore_CNN_Soft_{q}.glsl"));
        assert_eq!(mode_b[2], format!("Anime4K_Upscale_CNN_x2_{q}.glsl"));
        assert_eq!(mode_b[3], "Anime4K_AutoDownscalePre_x2.glsl");
        assert_eq!(mode_b[4], format!("Anime4K_Upscale_CNN_x2_Pass2_{q}.glsl"));
        assert!(ShaderManager::validate_shader_chain_uniqueness(&mode_b).is_ok());

        // Mode C: 4 unique shaders (denoise + upscale reaching 4x scaling)
        let mode_c =
            ShaderManager::get_anime4k_shader_names_with_quality(Anime4KPreset::ModeC, quality);
        assert_eq!(mode_c.len(), 4);
        assert_eq!(
            mode_c[1],
            format!("Anime4K_Upscale_Denoise_CNN_x2_{q}.glsl")
        );
        assert_eq!(mode_c[3], format!("Anime4K_Upscale_CNN_x2_{q}.glsl"));
        assert!(ShaderManager::validate_shader_chain_uniqueness(&mode_c).is_ok());

        // Mode A+A: 6 unique shaders (dual line restoration passes + dual upscaling reaching 4x scaling)
        let mode_aa =
            ShaderManager::get_anime4k_shader_names_with_quality(Anime4KPreset::ModeAA, quality);
        assert_eq!(mode_aa.len(), 6);
        assert_eq!(mode_aa[4], format!("Anime4K_Restore_CNN_Pass2_{q}.glsl"));
        assert_eq!(mode_aa[5], format!("Anime4K_Upscale_CNN_x2_Pass2_{q}.glsl"));
        assert!(ShaderManager::validate_shader_chain_uniqueness(&mode_aa).is_ok());

        // Mode B+B: 6 unique shaders (dual soft line restoration + dual upscaling reaching 4x scaling)
        let mode_bb =
            ShaderManager::get_anime4k_shader_names_with_quality(Anime4KPreset::ModeBB, quality);
        assert_eq!(mode_bb.len(), 6);
        assert_eq!(
            mode_bb[4],
            format!("Anime4K_Restore_CNN_Soft_Pass2_{q}.glsl")
        );
        assert_eq!(mode_bb[5], format!("Anime4K_Upscale_CNN_x2_Pass2_{q}.glsl"));
        assert!(ShaderManager::validate_shader_chain_uniqueness(&mode_bb).is_ok());

        // Mode C+A: 5 unique shaders (denoise then restore + dual upscaling reaching 4x scaling)
        let mode_ca =
            ShaderManager::get_anime4k_shader_names_with_quality(Anime4KPreset::ModeCA, quality);
        assert_eq!(mode_ca.len(), 5);
        assert_eq!(
            mode_ca[1],
            format!("Anime4K_Upscale_Denoise_CNN_x2_{q}.glsl")
        );
        assert_eq!(mode_ca[3], format!("Anime4K_Restore_CNN_{q}.glsl"));
        assert_eq!(mode_ca[4], format!("Anime4K_Upscale_CNN_x2_{q}.glsl"));
        assert!(ShaderManager::validate_shader_chain_uniqueness(&mode_ca).is_ok());
    }
}

#[test]
fn test_anime4k_upstream_assets_exist_and_apply_to_mpv() {
    let handle = MpvHandle::new().expect("Create handle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().unwrap();

    let shader_dir = ShaderManager::resolve_anime4k_shader_dir()
        .expect("Anime4K upstream assets must be discoverable in assets/shaders/Anime4K/upstream");

    // Apply Mode A at Fast quality
    let applied = ShaderManager::apply_anime4k_mode(
        &handle,
        Anime4KPreset::ModeA,
        ShaderQuality::Fast,
        Some(&shader_dir),
    )
    .expect("Apply Mode A Fast");
    assert_eq!(applied.len(), 5);

    let active_glsl = handle.get_property_string("glsl-shaders").unwrap();
    assert!(active_glsl.contains("Anime4K_Clamp_Highlights.glsl"));
    assert!(active_glsl.contains("Anime4K_Restore_CNN_S.glsl"));
    assert!(active_glsl.contains("Anime4K_Upscale_CNN_x2_S.glsl"));
    assert!(active_glsl.contains("Anime4K_Upscale_CNN_x2_Pass2_S.glsl"));

    // Switch live to Mode C at High quality
    let applied_c = ShaderManager::apply_anime4k_mode(
        &handle,
        Anime4KPreset::ModeC,
        ShaderQuality::High,
        Some(&shader_dir),
    )
    .expect("Apply Mode C High");
    assert_eq!(applied_c.len(), 4);

    let active_glsl_c = handle.get_property_string("glsl-shaders").unwrap();
    assert!(active_glsl_c.contains("Anime4K_Upscale_Denoise_CNN_x2_L.glsl"));

    // Disable clears shaders completely
    let cleared = ShaderManager::apply_anime4k_mode(
        &handle,
        Anime4KPreset::Disabled,
        ShaderQuality::Balanced,
        Some(&shader_dir),
    )
    .expect("Disable Anime4K");
    assert!(cleared.is_empty());
    assert_eq!(handle.get_property_string("glsl-shaders").unwrap(), "");
}

#[test]
fn test_shader_security_path_validation() {
    let shader_dir = ShaderManager::resolve_anime4k_shader_dir()
        .expect("Anime4K upstream assets must be discoverable");
    let real_shader = shader_dir.join("Anime4K_Clamp_Highlights.glsl");

    // Existing valid shader succeeds
    assert!(ShaderManager::validate_shader_path(real_shader.to_str().unwrap()).is_ok());

    // Nonexistent file fails
    assert!(ShaderManager::validate_shader_path("/nonexistent/file/shader.glsl").is_err());

    // Security rejections: empty, null byte, newlines, bad extensions
    assert!(ShaderManager::validate_shader_path("").is_err());
    assert!(ShaderManager::validate_shader_path("/bad/null\0byte.glsl").is_err());
    assert!(ShaderManager::validate_shader_path("/bad/newline\n.glsl").is_err());
    assert!(ShaderManager::validate_shader_path("/evil/script.sh").is_err());
    assert!(ShaderManager::validate_shader_path("/evil/binary.exe").is_err());
}

#[test]
fn test_shader_confinement_and_symlink_escape_prevention() {
    let temp_root = std::env::temp_dir().join(format!("muvrix_shader_sec_{}", std::process::id()));
    let allowed_dir = temp_root.join("allowed_shaders");
    let outside_dir = temp_root.join("outside_vault");
    std::fs::create_dir_all(&allowed_dir).unwrap();
    std::fs::create_dir_all(&outside_dir).unwrap();

    let outside_file = outside_dir.join("secret.glsl");
    std::fs::write(&outside_file, "// dummy shader content").unwrap();

    // 1. Direct path outside allowed dir is rejected with SecurityViolation
    let outside_res = ShaderManager::validate_shader_confined_to_dir(
        outside_file.to_str().unwrap(),
        &allowed_dir,
    );
    assert!(matches!(
        outside_res,
        Err(muvrix_player::error::MpvError::SecurityViolation(_))
    ));

    // 2. Symlink inside allowed dir pointing outside allowed dir is resolved and rejected
    #[cfg(unix)]
    {
        let symlink_path = allowed_dir.join("symlink_escape.glsl");
        let _ = std::fs::remove_file(&symlink_path);
        std::os::unix::fs::symlink(&outside_file, &symlink_path).unwrap();

        let symlink_res = ShaderManager::validate_shader_confined_to_dir(
            symlink_path.to_str().unwrap(),
            &allowed_dir,
        );
        assert!(matches!(
            symlink_res,
            Err(muvrix_player::error::MpvError::SecurityViolation(_))
        ));
    }

    // 3. Legitimate file inside allowed dir is accepted
    let valid_file = allowed_dir.join("legit.glsl");
    std::fs::write(&valid_file, "// legit shader").unwrap();
    assert!(ShaderManager::validate_shader_confined_to_dir(
        valid_file.to_str().unwrap(),
        &allowed_dir
    )
    .is_ok());

    let _ = std::fs::remove_dir_all(&temp_root);
}

#[test]
fn test_shader_fallback_safeguards_pipeline() {
    let handle = MpvHandle::new().expect("Create handle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().unwrap();

    let shader_dir = ShaderManager::resolve_anime4k_shader_dir().expect("Shader dir");

    // Apply valid Mode A
    let applied = ShaderManager::apply_anime4k_mode(
        &handle,
        Anime4KPreset::ModeA,
        ShaderQuality::Balanced,
        Some(&shader_dir),
    )
    .expect("Apply Mode A");
    let prev_refs: Vec<&str> = applied.iter().map(|s| s.as_str()).collect();

    // Attempt to apply from an empty/invalid directory
    let invalid_dir = std::env::temp_dir().join(format!("muvrix_empty_{}", std::process::id()));
    std::fs::create_dir_all(&invalid_dir).unwrap();

    let res = ShaderManager::apply_anime4k_mode_with_fallback(
        &handle,
        Anime4KPreset::ModeB,
        ShaderQuality::Balanced,
        Some(&invalid_dir),
        &prev_refs,
    );
    assert!(res.is_err());

    // Fallback must have restored the previous valid shaders
    let current_glsl = handle.get_property_string("glsl-shaders").unwrap();
    assert!(current_glsl.contains("Anime4K_Restore_CNN_M.glsl"));

    // If no previous shaders, fallback clears glsl-shaders
    let res_empty = ShaderManager::apply_anime4k_mode_with_fallback(
        &handle,
        Anime4KPreset::ModeC,
        ShaderQuality::Balanced,
        Some(&invalid_dir),
        &[],
    );
    assert!(res_empty.is_err());
    assert_eq!(handle.get_property_string("glsl-shaders").unwrap(), "");

    let _ = std::fs::remove_dir_all(&invalid_dir);
}

#[test]
fn test_deterministic_rendered_output_with_and_without_shaders() {
    let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/test_video.mp4")
        .canonicalize()
        .expect("test_video.mp4 must exist");

    let shader_dir = ShaderManager::resolve_anime4k_shader_dir().expect("Shader dir");

    let capture_frame_for_preset = |preset: Anime4KPreset| -> Option<String> {
        let handle = MpvHandle::new().ok()?;
        handle.set_option_string("vo", "gpu").ok()?;
        handle.set_option_string("ao", "null").ok()?;
        handle.initialize().ok()?;

        if preset != Anime4KPreset::Disabled {
            let _ = ShaderManager::apply_anime4k_mode(
                &handle,
                preset,
                ShaderQuality::Balanced,
                Some(&shader_dir),
            );
        }

        handle.command(&["loadfile", fixture.to_str()?]).ok()?;
        std::thread::sleep(std::time::Duration::from_millis(350));

        let out_path = std::env::temp_dir().join(format!(
            "frame_{:?}_{}_{}.png",
            preset,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = handle.command(&["screenshot-to-file", out_path.to_str()?, "subtitles"]);
        std::thread::sleep(std::time::Duration::from_millis(150));

        let bytes = std::fs::read(&out_path).ok()?;
        let _ = std::fs::remove_file(&out_path);
        if bytes.is_empty() {
            return None;
        }
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        Some(format!("{:x}", hasher.finalize()))
    };

    let hash_plain = capture_frame_for_preset(Anime4KPreset::Disabled);
    let hash_mode_a = capture_frame_for_preset(Anime4KPreset::ModeA);
    let hash_mode_b = capture_frame_for_preset(Anime4KPreset::ModeB);
    let hash_mode_c = capture_frame_for_preset(Anime4KPreset::ModeC);

    if let (Some(h_plain), Some(h_a), Some(h_b), Some(h_c)) =
        (hash_plain, hash_mode_a, hash_mode_b, hash_mode_c)
    {
        assert_ne!(
            h_plain, h_a,
            "Mode A must produce rendered output different from unshaded"
        );
        assert_ne!(
            h_plain, h_b,
            "Mode B must produce rendered output different from unshaded"
        );
        assert_ne!(
            h_plain, h_c,
            "Mode C must produce rendered output different from unshaded"
        );
        assert_ne!(
            h_a, h_b,
            "Mode A and Mode B must produce distinct rendered outputs"
        );
        assert_ne!(
            h_a, h_c,
            "Mode A and Mode C must produce distinct rendered outputs"
        );
    }
}

#[test]
fn test_all_18_anime4k_combinations_render_and_scale() {
    let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/test_video.mp4")
        .canonicalize()
        .expect("test_video.mp4 must exist");

    let shader_dir = ShaderManager::resolve_anime4k_shader_dir().expect("Shader dir");

    let modes = [
        (Anime4KPreset::ModeA, "ModeA"),
        (Anime4KPreset::ModeB, "ModeB"),
        (Anime4KPreset::ModeC, "ModeC"),
        (Anime4KPreset::ModeAA, "ModeAA"),
        (Anime4KPreset::ModeBB, "ModeBB"),
        (Anime4KPreset::ModeCA, "ModeCA"),
    ];

    let qualities = [
        (ShaderQuality::Fast, "S"),
        (ShaderQuality::Balanced, "M"),
        (ShaderQuality::High, "L"),
    ];

    for (mode, mode_name) in modes {
        for (q, q_name) in qualities {
            let handle = MpvHandle::new().expect("Create handle");
            handle.set_option_string("vo", "gpu").unwrap();
            handle.set_option_string("ao", "null").unwrap();
            handle.initialize().unwrap();

            let applied = ShaderManager::apply_anime4k_mode(&handle, mode, q, Some(&shader_dir))
                .unwrap_or_else(|e| panic!("Apply {mode_name} {q_name} failed: {e:?}"));
            assert!(!applied.is_empty());

            // Verify unique paths
            assert!(
                ShaderManager::validate_shader_chain_uniqueness(&applied).is_ok(),
                "All paths in {mode_name} {q_name} must be unique"
            );

            // Verify 4x scaling stages (must contain at least two upscale stages)
            let upscale_stages = applied.iter().filter(|s| s.contains("Upscale")).count();
            assert_eq!(
                upscale_stages, 2,
                "{mode_name} {q_name} must contain exactly two 2x upscale passes to achieve 4x scaling"
            );

            handle
                .command(&["loadfile", fixture.to_str().unwrap()])
                .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(150));

            let out_path = std::env::temp_dir().join(format!(
                "test_render_{}_{}_{}.png",
                mode_name,
                q_name,
                std::process::id()
            ));
            let _ = handle.command(&[
                "screenshot-to-file",
                out_path.to_str().unwrap(),
                "subtitles",
            ]);
            std::thread::sleep(std::time::Duration::from_millis(100));

            if let Ok(bytes) = std::fs::read(&out_path) {
                assert!(
                    !bytes.is_empty(),
                    "Rendered frame for {mode_name} {q_name} must produce non-empty image"
                );
                let _ = std::fs::remove_file(&out_path);
            }
        }
    }
}

#[test]
fn test_anime4k_shader_preset_names() {
    let mode_a = ShaderManager::get_anime4k_shader_names(Anime4KPreset::ModeA);
    assert_eq!(mode_a.len(), 5);
    assert_eq!(mode_a[0], "Anime4K_Clamp_Highlights.glsl");
    assert_eq!(mode_a[1], "Anime4K_Restore_CNN_M.glsl");
    assert_eq!(mode_a[2], "Anime4K_Upscale_CNN_x2_M.glsl");
    assert_eq!(mode_a[3], "Anime4K_AutoDownscalePre_x2.glsl");
    assert_eq!(mode_a[4], "Anime4K_Upscale_CNN_x2_Pass2_M.glsl");
    assert!(ShaderManager::validate_shader_chain_uniqueness(&mode_a).is_ok());

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
        ..Default::default()
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
        ..Default::default()
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

#[test]
fn test_controller_audio_device_selection_and_routing() {
    let handle = std::sync::Arc::new(MpvHandle::new().expect("Create handle"));
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().unwrap();

    let controller = muvrix_player::PlaybackController::new(handle).expect("Create controller");

    let devices = controller.audio_devices().expect("Query audio devices");
    assert!(!devices.is_empty(), "Should contain at least auto device");
    assert_eq!(devices[0].name, "auto");

    // Select auto device
    controller
        .set_audio_device("auto")
        .expect("Select auto audio device");
}

#[test]
fn test_anime4k_chain_uniqueness_for_all_modes_and_qualities() {
    let shader_dir = ShaderManager::resolve_anime4k_shader_dir()
        .expect("Anime4K upstream assets must be discoverable");

    let modes = [
        Anime4KPreset::ModeA,
        Anime4KPreset::ModeB,
        Anime4KPreset::ModeC,
        Anime4KPreset::ModeAA,
        Anime4KPreset::ModeBB,
        Anime4KPreset::ModeCA,
    ];
    let qualities = [
        ShaderQuality::Fast,
        ShaderQuality::Balanced,
        ShaderQuality::High,
    ];

    for mode in modes {
        for quality in qualities {
            let names = ShaderManager::get_anime4k_shader_names_with_quality(mode, quality);
            assert!(
                !names.is_empty(),
                "Mode {:?} at {:?} must produce non-empty shader list",
                mode,
                quality
            );

            // Automated validation that detects duplicated shader paths
            assert!(
                ShaderManager::validate_shader_chain_uniqueness(&names).is_ok(),
                "Duplicate shader found in Mode {:?} at {:?}",
                mode,
                quality
            );

            // Verify every file exists in upstream directory
            for name in &names {
                let file_path = shader_dir.join(name);
                assert!(
                    file_path.is_file(),
                    "Referenced shader '{}' for Mode {:?} at {:?} must exist on disk at {:?}",
                    name,
                    mode,
                    quality,
                    file_path
                );
            }
        }
    }
}

#[test]
fn test_cross_platform_shader_path_serialization_and_handling() {
    // 1. Windows path list serialization uses ';' to protect drive letter colons
    let win_paths = [
        r"C:\Program Files\Muvrix\shaders\Anime4K_Clamp_Highlights.glsl",
        r"C:\Program Files\Muvrix\shaders\Anime4K_Restore_CNN_M.glsl",
    ];
    let win_serialized = ShaderManager::serialize_shader_paths_for_target(&win_paths, true);
    assert_eq!(
        win_serialized,
        r"C:\Program Files\Muvrix\shaders\Anime4K_Clamp_Highlights.glsl;C:\Program Files\Muvrix\shaders\Anime4K_Restore_CNN_M.glsl"
    );
    assert!(win_serialized.contains(';'));
    assert_eq!(win_serialized.matches(';').count(), 1);

    // 2. Unix path list serialization uses ':'
    let unix_paths = [
        "/usr/share/muvrix/shaders/Anime4K_Clamp_Highlights.glsl",
        "/usr/share/muvrix/shaders/Anime4K_Restore_CNN_M.glsl",
    ];
    let unix_serialized = ShaderManager::serialize_shader_paths_for_target(&unix_paths, false);
    assert_eq!(
        unix_serialized,
        "/usr/share/muvrix/shaders/Anime4K_Clamp_Highlights.glsl:/usr/share/muvrix/shaders/Anime4K_Restore_CNN_M.glsl"
    );
    assert!(unix_serialized.contains(':'));

    // 3. Duplicate paths in chain must be detected and rejected
    let dup_paths = [
        "/tmp/shader_a.glsl",
        "/tmp/shader_b.glsl",
        "/tmp/shader_a.glsl",
    ];
    assert!(ShaderManager::validate_shader_chain_uniqueness(&dup_paths).is_err());

    // 4. Live mpv change-list test: handles paths with spaces and multiple files cleanly
    let handle = MpvHandle::new().expect("Create handle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().unwrap();

    let shader_dir = ShaderManager::resolve_anime4k_shader_dir().expect("Shader dir");
    let shader1 = shader_dir.join("Anime4K_Clamp_Highlights.glsl");
    let shader2 = shader_dir.join("Anime4K_Restore_CNN_M.glsl");

    let s1 = shader1.to_str().unwrap();
    let s2 = shader2.to_str().unwrap();

    // Set via change-list
    ShaderManager::set_shaders_via_change_list(&handle, &[s1, s2])
        .expect("Set shaders via change-list");
    let active = handle.get_property_string("glsl-shaders").unwrap();
    assert!(active.contains("Anime4K_Clamp_Highlights.glsl"));
    assert!(active.contains("Anime4K_Restore_CNN_M.glsl"));

    // Clearing
    ShaderManager::clear_shaders(&handle).expect("Clear shaders");
    assert_eq!(handle.get_property_string("glsl-shaders").unwrap(), "");
}

#[test]
fn hdr_target_lifecycle_and_transitions() {
    let handle = MpvHandle::new().expect("Create handle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().unwrap();

    // A/C: explicit output target initialization.
    ShaderManager::apply_output_target(&handle, true, Some(617)).expect("HDR init");
    assert_eq!(handle.get_property_string("target-trc").unwrap(), "pq");
    assert_eq!(
        handle.get_property_string("target-prim").unwrap(),
        "bt.2020"
    );

    // B/F: default and non-default adjustments must not clear an HDR target.
    let mut settings = VideoAdjustmentSettings {
        brightness: 12,
        contrast: -8,
        gamma: 4,
        saturation: 15,
        ..Default::default()
    };
    ShaderManager::apply_adjustments(&handle, &settings).expect("Apply adjustments");
    assert_eq!(handle.get_property_string("target-trc").unwrap(), "pq");
    assert_eq!(
        handle.get_property_string("target-prim").unwrap(),
        "bt.2020"
    );

    // Explicit adjustment targets are allowed, then the shell reasserts.
    settings.target_trc = TargetTrc::Srgb;
    settings.target_prim = TargetPrimaries::Bt709;
    ShaderManager::apply_adjustments(&handle, &settings).expect("Explicit adjustment target");
    assert_eq!(handle.get_property_string("target-trc").unwrap(), "srgb");
    assert_eq!(handle.get_property_string("target-prim").unwrap(), "bt.709");
    ShaderManager::apply_output_target(&handle, true, Some(617)).expect("Reassert HDR");
    assert_eq!(handle.get_property_string("target-trc").unwrap(), "pq");
    assert_eq!(
        handle.get_property_string("target-prim").unwrap(),
        "bt.2020"
    );

    // D: HDR surface -> SDR output target.
    ShaderManager::apply_output_target(&handle, false, None).expect("HDR to SDR");
    assert_eq!(handle.get_property_string("target-trc").unwrap(), "auto");
    assert_eq!(handle.get_property_string("target-prim").unwrap(), "auto");

    // E: SDR -> HDR surface target.
    ShaderManager::apply_output_target(&handle, true, Some(617)).expect("SDR to HDR");
    assert_eq!(handle.get_property_string("target-trc").unwrap(), "pq");
    assert_eq!(
        handle.get_property_string("target-prim").unwrap(),
        "bt.2020"
    );
}

#[test]
fn default_adjustments_preserve_output_hdr_target() {
    let handle = MpvHandle::new().expect("Create handle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().unwrap();

    handle
        .set_property_string("target-trc", "pq")
        .expect("Set PQ target");
    handle
        .set_property_string("target-prim", "bt.2020")
        .expect("Set BT.2020 primaries");

    // Default enhancement settings use `Auto`; they must not reset the
    // shell-owned PQ/BT.2020 target.
    ShaderManager::apply_adjustments(&handle, &VideoAdjustmentSettings::default())
        .expect("Apply default adjustments");
    assert_eq!(handle.get_property_string("target-trc").unwrap(), "pq");
    assert_eq!(
        handle.get_property_string("target-prim").unwrap(),
        "bt.2020"
    );

    ShaderManager::reset_adjustments(&handle).expect("Reset adjustments");
    assert_eq!(handle.get_property_string("target-trc").unwrap(), "pq");
    assert_eq!(
        handle.get_property_string("target-prim").unwrap(),
        "bt.2020"
    );

    ShaderManager::apply_output_target(&handle, true, Some(617)).expect("Apply HDR output target");
    assert_eq!(handle.get_property_string("target-trc").unwrap(), "pq");
    assert_eq!(
        handle.get_property_string("target-prim").unwrap(),
        "bt.2020"
    );

    ShaderManager::apply_output_target(&handle, false, None).expect("Apply SDR output target");
    assert_eq!(handle.get_property_string("target-trc").unwrap(), "auto");
    assert_eq!(handle.get_property_string("target-prim").unwrap(), "auto");
}

#[test]
fn test_d4_video_adjustments_roundtrip_persistence() {
    use muvrix_player::shader::{
        CustomShaderEntry, GamutMappingMode, TargetPrimaries, TargetTrc, ToneMappingAlgorithm,
    };

    let original = VideoAdjustmentSettings {
        brightness: 12,
        contrast: 8,
        saturation: 15,
        gamma: 3,
        hue: -5,
        sharpen: 1.0,
        deband: true,
        deband_iterations: 3,
        deband_threshold: 110,
        deband_range: 22,
        deband_grain: 36,
        denoise: true,
        denoise_strength: 3.5,
        aspect_ratio: "16:9".to_string(),
        zoom: 0.1,
        pan_x: 0.02,
        pan_y: -0.02,
        rotate: 90,
        tone_mapping: ToneMappingAlgorithm::Bt2446a,
        target_trc: TargetTrc::Bt1886,
        target_prim: TargetPrimaries::DciP3,
        gamut_mapping: GamutMappingMode::Relative,
        hdr_compute_peak: true,
        tone_mapping_max_boost: 1.8,
        target_peak: 800,
        active_shaders: vec!["Anime4K_Restore_CNN_M.glsl".to_string()],
        custom_shaders: vec![CustomShaderEntry {
            name: "CAS.glsl".to_string(),
            path: "/path/to/CAS.glsl".to_string(),
            enabled: true,
        }],
    };

    let serialized = serde_json::to_string_pretty(&original).expect("Serialize settings");
    let deserialized: VideoAdjustmentSettings =
        serde_json::from_str(&serialized).expect("Deserialize settings");

    assert_eq!(original, deserialized);
    assert_eq!(deserialized.deband_iterations, 3);
    assert_eq!(deserialized.deband_threshold, 110);
    assert_eq!(deserialized.target_peak, 800);
    assert_eq!(deserialized.custom_shaders.len(), 1);
    assert_eq!(deserialized.custom_shaders[0].name, "CAS.glsl");
}

#[test]
fn test_d4_deband_iterations_range_grain_mpv_property_reflection() {
    let handle = MpvHandle::new().expect("Create handle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().unwrap();

    let settings = VideoAdjustmentSettings {
        deband: true,
        deband_iterations: 4,
        deband_threshold: 128,
        deband_range: 32,
        deband_grain: 64,
        ..Default::default()
    };

    ShaderManager::apply_adjustments(&handle, &settings).expect("Apply deband adjustments");

    assert!(handle.get_property::<bool>("deband").unwrap());
    assert_eq!(handle.get_property::<i64>("deband-iterations").unwrap(), 4);
    assert_eq!(handle.get_property::<i64>("deband-threshold").unwrap(), 128);
    assert_eq!(handle.get_property::<i64>("deband-range").unwrap(), 32);
    assert_eq!(handle.get_property::<i64>("deband-grain").unwrap(), 64);
}

#[test]
fn test_d4_denoise_hqdn3d_vf_property_reflection() {
    let handle = MpvHandle::new().expect("Create handle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().unwrap();

    let mut settings = VideoAdjustmentSettings {
        denoise: true,
        denoise_strength: 4.0,
        ..Default::default()
    };

    let res = ShaderManager::apply_adjustments(&handle, &settings);
    eprintln!("apply_adjustments result: {:?}", res);
    let vf = handle.get_property_string("vf");
    eprintln!("vf readback: {:?}", vf);
    assert!(
        vf.is_ok() && vf.as_ref().unwrap().contains("hqdn3d"),
        "vf must contain hqdn3d filter string"
    );

    // Turning off denoise clears vf
    settings.denoise = false;
    ShaderManager::apply_adjustments(&handle, &settings).expect("Apply denoise off");
    let vf_cleared = handle.get_property_string("vf").unwrap();
    assert_eq!(vf_cleared, "");
}

#[test]
fn test_d4_target_peak_auto_vs_explicit_nits() {
    let handle = MpvHandle::new().expect("Create handle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().unwrap();

    let mut settings = VideoAdjustmentSettings {
        target_peak: 1500,
        ..Default::default()
    };
    ShaderManager::apply_adjustments(&handle, &settings).expect("Apply 1500 nits");
    assert_eq!(handle.get_property_string("target-peak").unwrap(), "1500");

    settings.target_peak = 0;
    ShaderManager::apply_adjustments(&handle, &settings).expect("Apply auto nits");
    assert_eq!(handle.get_property_string("target-peak").unwrap(), "auto");
}

#[test]
fn test_d4_custom_shader_uniqueness_rejection() {
    use muvrix_player::shader::CustomShaderEntry;

    let entries = [
        CustomShaderEntry::new("/path/to/cas.glsl".to_string()),
        CustomShaderEntry::new("/path/to/cas.glsl".to_string()),
    ];

    let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
    let res = ShaderManager::validate_shader_chain_uniqueness(&paths);
    assert!(res.is_err(), "Duplicate shader path must be rejected");
}
