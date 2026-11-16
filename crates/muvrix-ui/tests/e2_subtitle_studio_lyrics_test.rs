//! Phase E2: Subtitle Studio & Embedded Lyrics Comprehensive Integration Tests.

use muvrix_core::lyrics::{resolve_lyrics, LrcFile, LyricSource};
use muvrix_player::{
    AssOverrideMode, MpvHandle, PersistedMediaSettings, PlaybackController, SubtitleJustify,
    SubtitleStyling,
};
use std::sync::Arc;

#[test]
fn test_subtitle_studio_styling_controls_and_defaults() {
    let defaults = SubtitleStyling::default();
    assert_eq!(defaults.font, "sans-serif");
    assert_eq!(defaults.font_size, 45);
    assert_eq!(defaults.color, "#FFFFFF");
    assert_eq!(defaults.border_color, "#000000");
    assert_eq!(defaults.border_size, 3.0);
    assert_eq!(defaults.pos, 100);
    assert_eq!(defaults.secondary_pos, 10);
    assert_eq!(defaults.margin_y, 36);
    assert_eq!(defaults.margin_x, 25);
    assert_eq!(defaults.shadow_color, "#80000000");
    assert_eq!(defaults.sub_shadow_offset, 0.0);
    assert_eq!(defaults.sub_speed, 1.0);
    assert_eq!(defaults.sub_justify, SubtitleJustify::Auto);
    assert_eq!(defaults.ass_override_mode, AssOverrideMode::Preserve);
}

#[test]
fn test_subtitle_studio_immediate_synchronization_with_mpv() {
    let handle = MpvHandle::new().expect("create mpv handle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().expect("initialize handle");

    let mut controller = PlaybackController::new(Arc::new(handle)).expect("controller");

    // Apply custom subtitle studio styling
    let studio_styling = SubtitleStyling {
        font: "Roboto".to_string(),
        font_size: 52,
        color: "#FFFF00".to_string(),
        border_color: "#1E293B".to_string(),
        border_size: 4.5,
        pos: 92,
        secondary_pos: 15,
        scale: 1.2,
        margin_y: 50,
        margin_x: 40,
        shadow_color: "#8000FFFF".to_string(),
        sub_shadow_offset: 3.5,
        sub_back_color: "#80000000".to_string(),
        sub_justify: SubtitleJustify::Center,
        sub_speed: 1.1,
        ass_override_mode: AssOverrideMode::SafeScale,
        sub_fonts_dir: "/tmp/custom_fonts".to_string(),
        ass_override: false,
    };

    controller
        .set_subtitle_styling(studio_styling.clone())
        .expect("sync styling");

    // Verify controller cached styling matches
    assert_eq!(controller.subtitle_styling(), &studio_styling);

    // Verify libmpv properties updated immediately
    assert_eq!(
        controller.handle().get_property_string("sub-font").unwrap(),
        "Roboto"
    );
    assert_eq!(
        controller
            .handle()
            .get_property::<i64>("sub-font-size")
            .unwrap(),
        52
    );
    assert_eq!(
        controller
            .handle()
            .get_property::<i64>("sub-margin-y")
            .unwrap(),
        50
    );
    assert_eq!(
        controller
            .handle()
            .get_property::<i64>("sub-margin-x")
            .unwrap(),
        40
    );
    assert_eq!(
        controller
            .handle()
            .get_property::<f64>("sub-shadow-offset")
            .unwrap(),
        3.5
    );
    assert_eq!(
        controller
            .handle()
            .get_property_string("sub-justify")
            .unwrap(),
        "center"
    );

    // Test Reset to defaults
    controller.reset_subtitle_styling().expect("reset styling");
    assert_eq!(controller.subtitle_styling(), &SubtitleStyling::default());
    assert_eq!(
        controller
            .handle()
            .get_property::<i64>("sub-margin-y")
            .unwrap(),
        36
    );
}

#[test]
fn test_subtitle_studio_settings_persistence() {
    let dir = std::env::temp_dir().join(format!("muvrix_sub_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let file = dir.join("sub_settings.json");

    let custom = SubtitleStyling {
        font: "Liberation Sans".to_string(),
        font_size: 60,
        color: "#00FF66".to_string(),
        border_color: "#222222".to_string(),
        border_size: 2.5,
        pos: 88,
        secondary_pos: 12,
        margin_y: 42,
        margin_x: 30,
        shadow_color: "#60000000".to_string(),
        sub_shadow_offset: 2.0,
        ..SubtitleStyling::default()
    };

    let settings = PersistedMediaSettings {
        subtitle_styling: custom.clone(),
        ..Default::default()
    };
    settings.save(Some(&file)).expect("save settings");

    let loaded = PersistedMediaSettings::load(Some(&file));
    assert_eq!(loaded.subtitle_styling, custom);
    assert_eq!(loaded.subtitle_styling.margin_y, 42);
    assert_eq!(loaded.subtitle_styling.margin_x, 30);
    assert_eq!(loaded.subtitle_styling.shadow_color, "#60000000");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_embedded_lyrics_four_tier_precedence() {
    let dir = std::env::temp_dir().join(format!("muvrix_lyr_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let media = dir.join("audio.m4a");
    let lrc = dir.join("audio.lrc");
    let txt = dir.join("audio.txt");

    // Tier 4: Embedded unsynchronized text (e.g. from MP4 ©lyr or ID3v2 USLT)
    let embedded_unsynced = "Line 1: Plain embedded lyric\nLine 2: Without timing";
    let res4 = resolve_lyrics(&media, Some(embedded_unsynced)).expect("Tier 4 resolution");
    assert_eq!(res4.source, LyricSource::EmbeddedUnsynced);
    assert!(!res4.is_synchronized);
    assert_eq!(res4.lines.len(), 2);
    assert_eq!(res4.lines[0].text, "Line 1: Plain embedded lyric");
    // Honest: no fake timestamps
    assert_eq!(res4.find_active_index(1000), None);

    // Tier 3: External unsynchronized text file (.txt) overrides embedded unsynced
    std::fs::write(&txt, "External TXT: Acoustic Verse\nExternal TXT: Chorus").unwrap();
    let res3 = resolve_lyrics(&media, Some(embedded_unsynced)).expect("Tier 3 resolution");
    assert_eq!(res3.source, LyricSource::ExternalUnsynced);
    assert!(!res3.is_synchronized);
    assert_eq!(res3.lines[0].text, "External TXT: Acoustic Verse");
    assert_eq!(res3.find_active_index(1000), None);

    // Tier 2: Embedded synchronized lyrics overrides external unsynchronized text
    let embedded_synced = "[00:04.00]Embedded synced verse\n[00:08.50]Embedded synced chorus";
    let res2 = resolve_lyrics(&media, Some(embedded_synced)).expect("Tier 2 resolution");
    assert_eq!(res2.source, LyricSource::EmbeddedSynced);
    assert!(res2.is_synchronized);
    assert_eq!(res2.lines[0].text, "Embedded synced verse");
    assert_eq!(res2.lines[0].timestamp_ms, 4000);
    assert_eq!(res2.find_active_index(5000), Some(0));

    // Tier 1: External synchronized .lrc sidecar has the absolute highest precedence
    std::fs::write(
        &lrc,
        "[00:02.00]External sidecar synced verse\n[00:06.00]External sidecar synced chorus",
    )
    .unwrap();
    let res1 = resolve_lyrics(&media, Some(embedded_synced)).expect("Tier 1 resolution");
    assert_eq!(res1.source, LyricSource::ExternalSynced);
    assert!(res1.is_synchronized);
    assert_eq!(res1.lines[0].text, "External sidecar synced verse");
    assert_eq!(res1.lines[0].timestamp_ms, 2000);
    assert_eq!(res1.find_active_index(3000), Some(0));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_embedded_lyrics_unicode_and_formatting_preservation() {
    let unicode_content = r#"
[ti:星空のワルツ]
[ar:栗林みな実]
[al:君が望む永遠]
静かな夜の空に
煌めく星たちが見つめている

À travers les étoiles lointaines,
L'amour brille d'un éclat éternel.

Café crème & naïveté: 100% 🎶
"#;

    // Test honest unsynchronized parsing with preserved Unicode, accents, emojis, and stanzas
    let lrc = LrcFile::from_unsynced_text(unicode_content, LyricSource::EmbeddedUnsynced)
        .expect("parse unsynced");
    assert!(!lrc.is_synchronized);
    assert_eq!(lrc.source, LyricSource::EmbeddedUnsynced);
    assert_eq!(lrc.find_active_index(0), None);

    // Japanese line preserved
    assert!(lrc.lines.iter().any(|l| l.text.contains("星空のワルツ")));
    // French line with accents preserved
    assert!(lrc
        .lines
        .iter()
        .any(|l| l.text.contains("À travers les étoiles lointaines")));
    // Emoji line preserved
    assert!(lrc.lines.iter().any(|l| l.text.contains("100% 🎶")));
}
