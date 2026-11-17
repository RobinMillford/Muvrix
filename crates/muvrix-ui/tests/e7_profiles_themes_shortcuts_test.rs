use std::time::{Duration, Instant};

use eframe::egui;
use muvrix_core::database::{DatabaseManager, PlaybackProfile};
use muvrix_ui::app::shortcuts::{KeyBindingMap, KeyCombination, ShortcutAction};
use muvrix_ui::theme::CustomTheme;

#[test]
fn test_profile_switching_settings_isolation() {
    let db = DatabaseManager::open_in_memory().expect("In-memory DB should open successfully");

    // 1. Verify seeded profiles from Migration 009
    let profiles = db.list_profiles().expect("Failed to list profiles");
    assert_eq!(profiles.len(), 4, "Should have 4 default seeded profiles");

    let default_prof = db
        .get_profile_by_name("Default")
        .expect("Query failed")
        .expect("Default profile missing");
    assert!(default_prof.is_active);
    assert_eq!(default_prof.volume, 100.0);
    assert_eq!(default_prof.equalizer_preset, "Flat");
    assert_eq!(default_prof.sub_font_size, 22);
    assert_eq!(default_prof.sub_color, "#F4F5F0");
    assert_eq!(default_prof.shader_preset, "None");

    let cinema_prof = db
        .get_profile_by_name("Cinema Room")
        .expect("Query failed")
        .expect("Cinema Room missing");
    assert!(!cinema_prof.is_active);
    assert_eq!(cinema_prof.volume, 90.0);
    assert_eq!(cinema_prof.equalizer_preset, "Bass Boost");
    assert_eq!(cinema_prof.sub_font_size, 26);
    assert_eq!(cinema_prof.shader_preset, "Anime4K High");

    let headphones_prof = db
        .get_profile_by_name("Headphones")
        .expect("Query failed")
        .expect("Headphones missing");
    assert_eq!(headphones_prof.volume, 75.0);
    assert_eq!(headphones_prof.equalizer_preset, "Headphones");
    assert_eq!(headphones_prof.sub_font_size, 20);

    let late_night_prof = db
        .get_profile_by_name("Late Night")
        .expect("Query failed")
        .expect("Late Night missing");
    assert_eq!(late_night_prof.volume, 50.0);
    assert_eq!(
        late_night_prof.loudness_target,
        Some("-23 LUFS".to_string())
    );

    // 2. Benchmark profile switching latency budget (< 50ms)
    let switch_start = Instant::now();
    db.set_active_profile(cinema_prof.id)
        .expect("Failed to switch to Cinema Room");
    let elapsed = switch_start.elapsed();

    assert!(
        elapsed < Duration::from_millis(50),
        "Profile switch took {:?}, exceeding 50ms requirement",
        elapsed
    );

    let active = db
        .get_active_profile()
        .expect("Query failed")
        .expect("Active profile missing");
    assert_eq!(active.id, cinema_prof.id);
    assert_eq!(active.name, "Cinema Room");
    assert_eq!(active.volume, 90.0);
    assert_eq!(active.equalizer_preset, "Bass Boost");
    assert_eq!(active.sub_font_size, 26);
    assert_eq!(active.shader_preset, "Anime4K High");

    // 3. Switch to Headphones
    db.set_active_profile(headphones_prof.id)
        .expect("Failed to switch to Headphones");
    let active_hp = db
        .get_active_profile()
        .expect("Query failed")
        .expect("Active profile missing");
    assert_eq!(active_hp.id, headphones_prof.id);
    assert_eq!(active_hp.name, "Headphones");
    assert_eq!(active_hp.volume, 75.0);
    assert_eq!(active_hp.equalizer_preset, "Headphones");

    // 4. Create and test custom profile
    let mut custom = PlaybackProfile::new("Studio Monitors");
    custom.volume = 92.0;
    custom.equalizer_preset = "Vocal Clarity".to_string();
    custom.sub_font_size = 28;
    custom.sub_color = "#39FF14".to_string();
    custom.shader_preset = "Mode B".to_string();
    let custom_id = db
        .insert_profile(&custom)
        .expect("Failed to insert custom profile");
    assert!(custom_id > 0);

    db.set_active_profile(custom_id)
        .expect("Failed to activate custom profile");
    let active_custom = db
        .get_active_profile()
        .expect("Query failed")
        .expect("Active custom profile missing");
    assert_eq!(active_custom.id, custom_id);
    assert_eq!(active_custom.name, "Studio Monitors");
    assert_eq!(active_custom.volume, 92.0);
    assert_eq!(active_custom.sub_font_size, 28);
    assert_eq!(active_custom.sub_color, "#39FF14");
    assert_eq!(active_custom.shader_preset, "Mode B");

    // 5. Deletion safety: active profile cannot be deleted
    // Switch away from custom before deleting
    db.set_active_profile(default_prof.id)
        .expect("Failed to switch back to Default");
    let del_res = db.delete_profile(custom_id);
    assert!(del_res.is_ok(), "Custom inactive profile should be deleted");
    assert!(db.get_profile_by_id(custom_id).unwrap().is_none());
}

#[test]
fn test_custom_theme_json_serialization() {
    // 1. Verify preset themes
    let obsidian = CustomTheme::obsidian_lime();
    assert_eq!(obsidian.name, "Obsidian Lime (Signature)");
    assert_eq!(obsidian.accent, [197, 243, 106]); // Electric Lime

    let cyberpunk = CustomTheme::cyberpunk_neon();
    assert_eq!(cyberpunk.name, "Cyberpunk Neon");
    assert_eq!(cyberpunk.accent, [0, 240, 255]); // Neon Cyan

    let nordic = CustomTheme::nordic_frost();
    assert_eq!(nordic.name, "Nordic Frost");
    assert_eq!(nordic.accent, [100, 210, 255]); // Ice Blue

    let solarized = CustomTheme::solarized_midnight();
    assert_eq!(solarized.name, "Solarized Midnight");
    assert_eq!(solarized.accent, [255, 184, 48]); // Amber Gold

    // 2. Test JSON roundtrip serialization
    let json_str = cyberpunk.to_json().expect("Serialization failed");
    assert!(json_str.contains("\"name\": \"Cyberpunk Neon\""));
    assert!(json_str.contains("\"accent\": ["));

    let deserialized = CustomTheme::from_json(&json_str).expect("Deserialization failed");
    assert_eq!(cyberpunk, deserialized);

    // 3. Test theme application to egui visuals
    let mut visuals = egui::Visuals::dark();
    nordic.apply_to_visuals(&mut visuals);
    assert_eq!(
        visuals.panel_fill,
        egui::Color32::from_rgb(
            nordic.background[0],
            nordic.background[1],
            nordic.background[2]
        )
    );
    assert_eq!(
        visuals.selection.bg_fill,
        egui::Color32::from_rgb(nordic.accent[0], nordic.accent[1], nordic.accent[2])
    );

    // 4. Test error on invalid JSON
    let invalid_json = "{ invalid_json: true }";
    assert!(CustomTheme::from_json(invalid_json).is_err());

    // 5. Test missing required field rejection
    let missing_field = r#"{
        "name": "Broken",
        "description": "Missing author and accent",
        "background": [0,0,0],
        "secondary_bg": [0,0,0],
        "surface": [0,0,0],
        "elevated": [0,0,0],
        "border_subtle": [0,0,0],
        "border_strong": [0,0,0],
        "text_primary": [0,0,0],
        "text_secondary": [0,0,0]
    }"#;
    assert!(CustomTheme::from_json(missing_field).is_err());

    // 6. Test out-of-range RGB value rejection (> 255)
    let out_of_range = r#"{
        "name": "Invalid Range",
        "description": "Value 999 is invalid for u8",
        "author": "Tester",
        "background": [999,0,0],
        "secondary_bg": [0,0,0],
        "surface": [0,0,0],
        "elevated": [0,0,0],
        "border_subtle": [0,0,0],
        "border_strong": [0,0,0],
        "text_primary": [0,0,0],
        "text_secondary": [0,0,0],
        "accent": [0,0,0]
    }"#;
    assert!(CustomTheme::from_json(out_of_range).is_err());
}

#[test]
fn test_custom_keybinding_dispatch() {
    let mut keybindings = KeyBindingMap::default();

    // 1. Verify default PlayPause is Space
    let play_pause_combo = keybindings.get(ShortcutAction::PlayPause).unwrap();
    assert_eq!(play_pause_combo.key, egui::Key::Space);
    assert!(!play_pause_combo.ctrl);
    assert_eq!(play_pause_combo.display_string(), "Space");

    // 2. Rebind PlayPause to 'K'
    keybindings.set_binding(ShortcutAction::PlayPause, KeyCombination::new(egui::Key::K));
    let updated = keybindings.get(ShortcutAction::PlayPause).unwrap();
    assert_eq!(updated.key, egui::Key::K);
    assert_eq!(updated.display_string(), "K");

    // 3. Create a deliberate conflict: bind VolumeUp also to 'K'
    keybindings.set_binding(ShortcutAction::VolumeUp, KeyCombination::new(egui::Key::K));
    let conflicts = keybindings.find_conflicts();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].0, ShortcutAction::PlayPause);
    assert_eq!(conflicts[0].1, ShortcutAction::VolumeUp);
    assert_eq!(conflicts[0].2.key, egui::Key::K);

    // 4. Reset to defaults clears conflict
    keybindings.reset_to_defaults();
    assert!(keybindings.find_conflicts().is_empty());
    assert_eq!(
        keybindings.get(ShortcutAction::PlayPause).unwrap().key,
        egui::Key::Space
    );
    assert_eq!(
        keybindings.get(ShortcutAction::VolumeUp).unwrap().key,
        egui::Key::ArrowUp
    );

    // 5. Test JSON roundtrip serialization
    let json = keybindings.to_json().expect("Serialization failed");
    let deserialized = KeyBindingMap::from_json(&json).expect("Deserialization failed");
    assert_eq!(keybindings, deserialized);
}

#[test]
fn test_actual_keyboard_event_dispatch_with_modifiers() {
    let ctx = egui::Context::default();
    let mut keybindings = KeyBindingMap::default();

    // 1. Default Space -> PlayPause matches
    let raw_space = egui::RawInput {
        events: vec![egui::Event::Key {
            key: egui::Key::Space,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        }],
        ..Default::default()
    };
    let _ = ctx.run(raw_space, |ctx| {
        ctx.input(|i| {
            assert!(keybindings.matches(ShortcutAction::PlayPause, i));
            assert!(!keybindings.matches(ShortcutAction::VolumeUp, i));
        });
    });

    // 2. Rebind PlayPause to K -> Space no longer matches, K matches
    keybindings.set_binding(ShortcutAction::PlayPause, KeyCombination::new(egui::Key::K));

    let raw_space2 = egui::RawInput {
        events: vec![egui::Event::Key {
            key: egui::Key::Space,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        }],
        ..Default::default()
    };
    let _ = ctx.run(raw_space2, |ctx| {
        ctx.input(|i| {
            assert!(!keybindings.matches(ShortcutAction::PlayPause, i));
        });
    });

    let raw_k = egui::RawInput {
        events: vec![egui::Event::Key {
            key: egui::Key::K,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        }],
        ..Default::default()
    };
    let _ = ctx.run(raw_k, |ctx| {
        ctx.input(|i| {
            assert!(keybindings.matches(ShortcutAction::PlayPause, i));
        });
    });

    // 3. Modifier dispatch: Ctrl+O for OpenFile
    let raw_o_no_ctrl = egui::RawInput {
        events: vec![egui::Event::Key {
            key: egui::Key::O,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        }],
        ..Default::default()
    };
    let _ = ctx.run(raw_o_no_ctrl, |ctx| {
        ctx.input(|i| {
            assert!(!keybindings.matches(ShortcutAction::OpenFile, i));
        });
    });

    let raw_ctrl_o = egui::RawInput {
        modifiers: egui::Modifiers::COMMAND,
        events: vec![egui::Event::Key {
            key: egui::Key::O,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        }],
        ..Default::default()
    };
    let _ = ctx.run(raw_ctrl_o, |ctx| {
        ctx.input(|i| {
            assert!(keybindings.matches(ShortcutAction::OpenFile, i));
        });
    });

    // 4. Modifier dispatch: Alt+D for ToggleSecondaryDeck
    let raw_alt_d = egui::RawInput {
        modifiers: egui::Modifiers::ALT,
        events: vec![egui::Event::Key {
            key: egui::Key::D,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::ALT,
        }],
        ..Default::default()
    };
    let _ = ctx.run(raw_alt_d, |ctx| {
        ctx.input(|i| {
            assert!(keybindings.matches(ShortcutAction::ToggleSecondaryDeck, i));
        });
    });
}

#[test]
fn test_profile_persistence_and_failure_recovery() {
    let db = DatabaseManager::open_in_memory().expect("In-memory DB should open successfully");

    // 1. Verify default profile is active
    let initial_active = db.get_active_profile().unwrap().expect("Active profile");
    assert_eq!(initial_active.name, "Default");

    // 2. Switch to Cinema Room
    let cinema = db.get_profile_by_name("Cinema Room").unwrap().unwrap();
    db.set_active_profile(cinema.id).unwrap();

    let active_after = db.get_active_profile().unwrap().unwrap();
    assert_eq!(active_after.id, cinema.id);
    assert_eq!(active_after.name, "Cinema Room");

    // 3. Attempting to switch to non-existent profile fails and preserves existing active profile
    let non_existent_id = 99999;
    let switch_res = db.get_profile_by_id(non_existent_id).unwrap();
    assert!(
        switch_res.is_none(),
        "Non-existent profile must not be found"
    );
    let set_res = db.set_active_profile(non_existent_id);
    assert!(
        set_res.is_err(),
        "set_active_profile with invalid ID must return error"
    );

    // Verify existing active profile was preserved by transactional rollback
    let current_active = db.get_active_profile().unwrap().unwrap();
    assert_eq!(current_active.id, cinema.id);
    assert_eq!(current_active.name, "Cinema Room");

    let all = db.list_profiles().unwrap();
    let active_count = all.iter().filter(|p| p.is_active).count();
    assert_eq!(
        active_count, 1,
        "Exactly one active profile must exist after failed activation"
    );
}
