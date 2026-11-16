use muvrix_player::{
    AccentColor, PersistedMediaSettings, SubtitleJustify, SubtitleStyling, ThemeMode,
};

#[test]
fn theme_and_subtitle_styling_roundtrip_with_backward_compatible_defaults() {
    let path = std::env::temp_dir().join(format!(
        "muvrix-settings-theme-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let styling = SubtitleStyling {
        sub_speed: 1.4,
        sub_shadow_offset: 6.0,
        sub_back_color: "#000000AA".to_string(),
        sub_justify: SubtitleJustify::Right,
        sub_fonts_dir: "/media/fonts".to_string(),
        margin_y: 42,
        margin_x: 28,
        shadow_color: "#80334455".to_string(),
        ..SubtitleStyling::default()
    };
    let settings = PersistedMediaSettings {
        theme_mode: ThemeMode::Amoled,
        accent_color: AccentColor::Purple,
        subtitle_styling: styling.clone(),
        ..Default::default()
    };
    settings.save(Some(&path)).expect("save");
    let loaded = PersistedMediaSettings::load(Some(&path));
    assert_eq!(loaded.theme_mode, ThemeMode::Amoled);
    assert_eq!(loaded.accent_color, AccentColor::Purple);
    assert_eq!(loaded.subtitle_styling, styling);

    let legacy = std::env::temp_dir().join(format!(
        "muvrix-settings-legacy-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::write(
        &legacy,
        r#"{"schema_version":1,"video_adjustments":{"brightness":0,"contrast":0,"saturation":0,"gamma":0,"hue":0,"sharpen":0.0,"deband":false,"aspect_ratio":"auto","zoom":0.0,"pan_x":0.0,"pan_y":0.0,"rotate":0,"tone_mapping":"clip","tone_mapping_max_boost":1.0,"target_primaries":"auto","target_trc":"auto","target_peak_nits":1000.0},"anime4k_preset":"disabled","anime4k_quality":"fast","equalizer":{"bands":[],"preamp_db":0.0,"is_enabled":false,"pitch_correction":false,"audio_delay":0.0,"channel_layout":"auto","volume_boost":100.0},"selected_audio_device":"auto","volume":100.0}"#,
    )
    .expect("write legacy");
    let legacy_loaded = PersistedMediaSettings::load(Some(&legacy));
    assert_eq!(legacy_loaded.theme_mode, ThemeMode::Obsidian);
    assert_eq!(legacy_loaded.accent_color, AccentColor::Lime);
    assert_eq!(legacy_loaded.subtitle_styling, SubtitleStyling::default());

    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(legacy);
}
