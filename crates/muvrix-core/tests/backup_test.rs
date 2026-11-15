use muvrix_core::backup::{BackupError, BackupSettings};

#[test]
fn test_backup_xml_complete_roundtrip() {
    let settings = BackupSettings {
        schema_version: 1,
        app_version: "0.1.0".to_string(),
        created_at_unix: 1731675000,
        audio_channels: "stereo".to_string(),
        volume_normalization: true,
        audio_pitch_correction: true,
        preferred_audio_languages: "ja,jpn,en".to_string(),
        default_audio_delay_ms: 250,
        volume_boost_cap: 50,
        volume: 110.0,
        theme_mode: "obsidian".to_string(),
        accent_color: "lime".to_string(),
        sub_font: "Noto Sans".to_string(),
        sub_font_size: 60,
        sub_color: "#FFFFFFFF".to_string(),
        sub_border_color: "#000000FF".to_string(),
        sub_border_size: 4.0,
        sub_shadow_offset: 2.0,
        sub_back_color: "#111A2280".to_string(),
        sub_margin_y: 30,
        sub_pos: 90,
        sub_speed: 1.0,
        sub_justify: "left".to_string(),
        ass_override: true,
        anime4k_preset: "mode_a".to_string(),
        anime4k_quality: "fast".to_string(),
        tone_mapping: "bt.2446a".to_string(),
        deband: true,
        history_enabled: true,
        auto_resume_enabled: true,
        verbose_logging: false,
        background_audio: true,
    };

    let xml = settings.to_xml();
    let parsed = BackupSettings::from_xml(&xml).expect("parse valid backup");
    assert_eq!(settings, parsed);
}

#[test]
fn test_backup_xml_escapes_special_characters() {
    let settings = BackupSettings {
        app_version: "0.1.0 & <beta> \"test\"".to_string(),
        preferred_audio_languages: "en & fr > de".to_string(),
        sub_font: "Font <Special> & 'Family'".to_string(),
        ..Default::default()
    };

    let xml = settings.to_xml();
    assert!(xml.contains("&amp;"));
    assert!(xml.contains("&lt;"));
    assert!(xml.contains("&gt;"));

    let parsed = BackupSettings::from_xml(&xml).expect("parse escaped xml");
    assert_eq!(parsed.app_version, settings.app_version);
    assert_eq!(
        parsed.preferred_audio_languages,
        settings.preferred_audio_languages
    );
    assert_eq!(parsed.sub_font, settings.sub_font);
}

#[test]
fn test_backup_xml_invalid_structures() {
    assert_eq!(
        BackupSettings::from_xml(""),
        Err(BackupError::EmptyDocument)
    );
    assert_eq!(
        BackupSettings::from_xml("not xml"),
        Err(BackupError::MissingRoot)
    );
    assert_eq!(
        BackupSettings::from_xml("<root></root>"),
        Err(BackupError::MissingRoot)
    );
    assert_eq!(
        BackupSettings::from_xml("<muvrix_backup schema_version=\"5\"></muvrix_backup>"),
        Err(BackupError::UnsupportedVersion(5))
    );
}
