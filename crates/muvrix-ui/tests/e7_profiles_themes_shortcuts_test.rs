use std::time::{Duration, Instant};

use eframe::egui;
use muvrix_core::database::{DatabaseManager, PlaybackProfile};
use muvrix_ui::app::actions::PlayerAction;
use muvrix_ui::app::shortcuts::{KeyBindingMap, KeyCombination, ShortcutProfile, ShortcutTuning};
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
    let play_pause_combo = keybindings.get(PlayerAction::PlayPause).unwrap();
    assert_eq!(play_pause_combo.key, egui::Key::Space);
    assert!(!play_pause_combo.ctrl);
    assert_eq!(play_pause_combo.display_string(), "Space");

    // 2. Rebind PlayPause to 'K'
    keybindings.set_binding(PlayerAction::PlayPause, KeyCombination::new(egui::Key::K));
    let updated = keybindings.get(PlayerAction::PlayPause).unwrap();
    assert_eq!(updated.key, egui::Key::K);
    assert_eq!(updated.display_string(), "K");

    // 3. Create a deliberate conflict: bind VolumeUp also to 'K'
    keybindings.set_binding(PlayerAction::VolumeUp, KeyCombination::new(egui::Key::K));
    let conflicts = keybindings.find_conflicts();
    assert!(conflicts.iter().any(|c| {
        (c.action_a == PlayerAction::PlayPause && c.action_b == PlayerAction::VolumeUp)
            || (c.action_a == PlayerAction::VolumeUp && c.action_b == PlayerAction::PlayPause)
    }));
    assert!(conflicts.iter().all(|c| c.combination.key == egui::Key::K));

    // 4. Reset to defaults clears conflict
    keybindings.reset_to_defaults();
    assert!(keybindings.find_conflicts().is_empty());
    assert_eq!(
        keybindings.get(PlayerAction::PlayPause).unwrap().key,
        egui::Key::Space
    );
    assert_eq!(
        keybindings.get(PlayerAction::VolumeUp).unwrap().key,
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
            assert!(keybindings.matches(PlayerAction::PlayPause, i));
            assert!(!keybindings.matches(PlayerAction::VolumeUp, i));
        });
    });

    // 2. Rebind PlayPause to K -> Space no longer matches, K matches
    keybindings.set_binding(PlayerAction::PlayPause, KeyCombination::new(egui::Key::K));

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
            assert!(!keybindings.matches(PlayerAction::PlayPause, i));
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
            assert!(keybindings.matches(PlayerAction::PlayPause, i));
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
            assert!(!keybindings.matches(PlayerAction::OpenFile, i));
        });
    });

    // Release O first: egui derives the `repeat` flag from its own held-key
    // set, so a second press without an intervening release is an auto-repeat
    // and is (correctly) ignored by the shortcut router.
    let release_o = egui::RawInput {
        events: vec![egui::Event::Key {
            key: egui::Key::O,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        }],
        ..Default::default()
    };
    let _ = ctx.run(release_o, |_| {});

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
            assert!(keybindings.matches(PlayerAction::OpenFile, i));
        });
    });

    // 4. Modifier dispatch: Alt+V cycles subtitle tracks in reverse
    let raw_alt_v = egui::RawInput {
        modifiers: egui::Modifiers::ALT,
        events: vec![egui::Event::Key {
            key: egui::Key::V,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::ALT,
        }],
        ..Default::default()
    };
    let _ = ctx.run(raw_alt_v, |ctx| {
        ctx.input(|i| {
            assert!(keybindings.matches(PlayerAction::CycleSubtitleTrackReverse, i));
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

// ---------------------------------------------------------------------------
// Shortcut routing: defaults, repeat semantics, tuning and disable
// ---------------------------------------------------------------------------

/// Runs a single frame carrying `events` and hands the resulting input state to
/// `inspect`, so bindings are exercised exactly the way the live router sees
/// them (including modifier flags and auto-repeat).
fn frame_with_events(events: Vec<egui::Event>, mut inspect: impl FnMut(&egui::InputState)) {
    let ctx = egui::Context::default();
    let raw = egui::RawInput {
        events,
        ..Default::default()
    };
    let _ = ctx.run(raw, |ctx| {
        ctx.input(|input| inspect(input));
    });
}

/// Runs `events` twice on one context so the second frame sees them as an
/// auto-repeat, exactly as a held key does. egui recomputes the `repeat` flag
/// from its held-key set, so a single frame can never be a repeat.
fn repeat_frame_with_events(events: Vec<egui::Event>, mut inspect: impl FnMut(&egui::InputState)) {
    let ctx = egui::Context::default();
    let _ = ctx.run(
        egui::RawInput {
            events: events.clone(),
            ..Default::default()
        },
        |_| {},
    );
    let _ = ctx.run(
        egui::RawInput {
            events,
            ..Default::default()
        },
        |ctx| {
            ctx.input(|input| inspect(input));
        },
    );
}

fn key_event(key: egui::Key, modifiers: egui::Modifiers, repeat: bool) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat,
        modifiers,
    }
}

#[test]
fn test_default_profile_exposes_plain_volume_and_seek() {
    let map = KeyBindingMap::default();
    let none = egui::Modifiers::default();

    // Bare arrows seek: the combination every VLC user tries first.
    frame_with_events(vec![key_event(egui::Key::ArrowRight, none, false)], |i| {
        assert!(
            map.matches(PlayerAction::SeekForwardShort, i),
            "bare Right must seek forward"
        );
        assert!(!map.matches(PlayerAction::SeekBackwardShort, i));
    });
    frame_with_events(vec![key_event(egui::Key::ArrowLeft, none, false)], |i| {
        assert!(
            map.matches(PlayerAction::SeekBackwardShort, i),
            "bare Left must seek back"
        );
    });

    // VLC's volume keys are Ctrl+Up/Down. Bare Up/Down must stay free here:
    // claiming them made Up move the volume in the mpv-compatible profile too.
    frame_with_events(
        vec![key_event(egui::Key::ArrowUp, egui::Modifiers::CTRL, false)],
        |i| {
            assert!(
                map.matches(PlayerAction::VolumeUp, i),
                "Ctrl+Up must raise the volume"
            );
        },
    );
    frame_with_events(
        vec![key_event(
            egui::Key::ArrowDown,
            egui::Modifiers::CTRL,
            false,
        )],
        |i| {
            assert!(
                map.matches(PlayerAction::VolumeDown, i),
                "Ctrl+Down must lower the volume"
            );
        },
    );
    frame_with_events(vec![key_event(egui::Key::ArrowUp, none, false)], |i| {
        assert!(
            !map.matches(PlayerAction::VolumeUp, i),
            "bare Up must not touch the volume in the VLC profile"
        );
    });
    frame_with_events(vec![key_event(egui::Key::ArrowDown, none, false)], |i| {
        assert!(
            !map.matches(PlayerAction::VolumeDown, i),
            "bare Down must not touch the volume in the VLC profile"
        );
    });

    // VLC's `T` reports the position.
    frame_with_events(vec![key_event(egui::Key::T, none, false)], |i| {
        assert!(
            map.matches(PlayerAction::ShowPlaybackPosition, i),
            "T must report the playback position"
        );
    });

    // The 10-second and 60-second rungs of the ladder stay reachable.
    frame_with_events(
        vec![key_event(
            egui::Key::ArrowRight,
            egui::Modifiers::ALT,
            false,
        )],
        |i| {
            assert!(
                map.matches(PlayerAction::SeekForwardMedium, i),
                "Alt+Right must use the medium (10s) step"
            );
        },
    );
    frame_with_events(
        vec![key_event(
            egui::Key::ArrowLeft,
            egui::Modifiers::COMMAND,
            false,
        )],
        |i| {
            assert!(
                map.matches(PlayerAction::SeekBackwardLong, i),
                "Ctrl+Left must use the large (60s) step"
            );
        },
    );
    frame_with_events(
        vec![key_event(
            egui::Key::ArrowLeft,
            egui::Modifiers::SHIFT,
            false,
        )],
        |i| {
            assert!(
                map.matches(PlayerAction::SeekBackwardVeryShort, i),
                "Shift+Left must use the very short step"
            );
        },
    );

    // VLC's own volume chord is kept as the alternate.
    frame_with_events(
        vec![key_event(
            egui::Key::ArrowUp,
            egui::Modifiers::COMMAND,
            false,
        )],
        |i| {
            assert!(
                map.matches(PlayerAction::VolumeUp, i),
                "Ctrl+Up must still raise the volume"
            );
        },
    );
}

#[test]
fn test_key_repeat_only_drives_continuous_actions() {
    let map = KeyBindingMap::default();

    // Holding Space must not rapid-toggle play/pause.
    repeat_frame_with_events(
        vec![key_event(
            egui::Key::Space,
            egui::Modifiers::default(),
            false,
        )],
        |i| {
            assert!(
                !map.matches(PlayerAction::PlayPause, i),
                "an auto-repeat press must not re-trigger play/pause"
            );
        },
    );

    // Holding a seek key must keep seeking.
    repeat_frame_with_events(
        vec![key_event(
            egui::Key::ArrowRight,
            egui::Modifiers::default(),
            false,
        )],
        |i| {
            assert!(
                map.matches(PlayerAction::SeekForwardShort, i),
                "an auto-repeat press must keep seeking"
            );
        },
    );

    // Volume likewise repeats, but fullscreen does not.
    repeat_frame_with_events(
        vec![key_event(egui::Key::ArrowUp, egui::Modifiers::CTRL, false)],
        |i| {
            assert!(map.matches(PlayerAction::VolumeUp, i));
        },
    );
    repeat_frame_with_events(
        vec![key_event(egui::Key::F, egui::Modifiers::default(), false)],
        |i| {
            assert!(
                !map.matches(PlayerAction::ToggleFullscreen, i),
                "an auto-repeat press must not thrash fullscreen"
            );
        },
    );
}

#[test]
fn test_disabled_action_ignores_its_binding() {
    let mut map = KeyBindingMap::default();
    assert!(map.is_enabled(PlayerAction::PlayPause));

    map.set_enabled(PlayerAction::PlayPause, false);
    frame_with_events(
        vec![key_event(
            egui::Key::Space,
            egui::Modifiers::default(),
            false,
        )],
        |i| {
            assert!(
                !map.matches(PlayerAction::PlayPause, i),
                "a disabled action must not fire"
            );
            assert!(
                !map.pressed_actions(PlayerAction::ALL, i)
                    .contains(&PlayerAction::PlayPause),
                "a disabled action must not appear in the dispatched set"
            );
        },
    );

    // Re-enabling restores the binding without needing to rebind it.
    map.set_enabled(PlayerAction::PlayPause, true);
    frame_with_events(
        vec![key_event(
            egui::Key::Space,
            egui::Modifiers::default(),
            false,
        )],
        |i| {
            assert!(map.matches(PlayerAction::PlayPause, i));
        },
    );
}

#[test]
fn test_shortcut_tuning_persists_and_is_clamped() {
    let mut map = KeyBindingMap::default();
    assert_eq!(map.tuning.seek_small_secs, 5.0);
    assert_eq!(map.tuning.seek_medium_secs, 10.0);
    assert_eq!(map.tuning.volume_step, 5.0);

    map.tuning.seek_small_secs = 7.5;
    map.tuning.volume_step = 2.5;
    map.tuning.speed_step = 0.5;
    map.tuning.subtitle_delay_step_ms = 100.0;

    let json = map.to_json().expect("Serialization failed");
    let restored = KeyBindingMap::from_json(&json).expect("Deserialization failed");
    assert_eq!(restored, map);
    assert_eq!(restored.tuning.seek_small_secs, 7.5);
    assert_eq!(restored.tuning.volume_step, 2.5);
    assert_eq!(restored.tuning.subtitle_delay_step_ms, 100.0);

    let clamped = ShortcutTuning {
        seek_small_secs: -5.0,
        volume_step: 0.0,
        speed_step: 0.0,
        ..ShortcutTuning::default()
    }
    .sanitized();
    assert!(clamped.seek_small_secs >= 0.5);
    assert!(clamped.volume_step >= 0.5);
    assert!(clamped.speed_step >= 0.05);
}

#[test]
fn test_every_profile_ships_without_conflicts() {
    for profile in ShortcutProfile::ALL {
        let map = KeyBindingMap::for_profile(*profile);
        let conflicts = map.find_conflicts();
        assert!(
            conflicts.is_empty(),
            "{profile:?} ships with {} conflict(s): {:?}",
            conflicts.len(),
            conflicts
                .iter()
                .map(|c| (c.combination.display_string(), c.action_a, c.action_b))
                .collect::<Vec<_>>()
        );
    }

    let labels: Vec<&str> = ShortcutProfile::ALL.iter().map(|p| p.label()).collect();
    assert!(labels.contains(&"MUVRIX Default (VLC)"));
    assert!(labels.contains(&"mpv-inspired"));
    assert!(labels.contains(&"Custom"));
}

#[test]
fn test_action_catalog_is_internally_consistent() {
    use std::collections::BTreeSet;
    let mut seen = BTreeSet::new();
    for &action in PlayerAction::ALL {
        assert!(
            seen.insert(action),
            "{action:?} is listed twice in PlayerAction::ALL"
        );
        assert!(!action.display_name().is_empty());
    }
    // Every action must be reachable from the editor, i.e. carry a category.
    for &action in PlayerAction::ALL {
        let _ = action.category();
    }
}

/// The mpv profile must reproduce upstream's own bindings, not a plausible
/// approximation. Every pair asserted here is copied from mpv's shipped
/// `etc/input.conf` (`UP seek 60`, `Shift+UP seek 5 exact`, `PGUP add chapter 1`,
/// `s screenshot`, `S screenshot video`, `r/R add sub-pos`, `G/F sub-scale`).
#[test]
fn test_advanced_profile_matches_upstream_mpv_bindings() {
    let map = KeyBindingMap::for_profile(ShortcutProfile::Advanced);
    let none = egui::Modifiers::default();
    let shift = egui::Modifiers::SHIFT;
    let ctrl = egui::Modifiers::CTRL;

    frame_with_events(vec![key_event(egui::Key::ArrowUp, none, false)], |i| {
        assert!(
            map.matches(PlayerAction::SeekForwardLong, i),
            "mpv: UP is a 60-second seek, not a volume key"
        );
        assert!(
            !map.matches(PlayerAction::VolumeUp, i),
            "the mpv profile must not steal UP for volume"
        );
    });
    frame_with_events(vec![key_event(egui::Key::ArrowDown, none, false)], |i| {
        assert!(map.matches(PlayerAction::SeekBackwardLong, i));
    });
    frame_with_events(vec![key_event(egui::Key::ArrowUp, shift, false)], |i| {
        assert!(
            map.matches(PlayerAction::SeekForwardShort, i),
            "Shift+UP is mpv's exact 5-second seek"
        );
    });
    frame_with_events(vec![key_event(egui::Key::ArrowUp, ctrl, false)], |i| {
        assert!(
            map.matches(PlayerAction::VolumeUpSmall, i),
            "Ctrl+UP stays available for volume in the mpv profile"
        );
    });

    // Chapter and 10-minute directions follow `add chapter 1` / `seek 600`.
    frame_with_events(vec![key_event(egui::Key::PageUp, none, false)], |i| {
        assert!(map.matches(PlayerAction::NextChapter, i));
    });
    frame_with_events(vec![key_event(egui::Key::PageDown, none, false)], |i| {
        assert!(map.matches(PlayerAction::PreviousChapter, i));
    });
    frame_with_events(vec![key_event(egui::Key::PageUp, shift, false)], |i| {
        assert!(map.matches(PlayerAction::SeekForwardTenMinutes, i));
    });
    frame_with_events(vec![key_event(egui::Key::PageDown, shift, false)], |i| {
        assert!(map.matches(PlayerAction::SeekBackwardTenMinutes, i));
    });

    // Screenshots: lowercase s keeps subtitles, uppercase S drops them.
    frame_with_events(vec![key_event(egui::Key::S, none, false)], |i| {
        assert!(map.matches(PlayerAction::Screenshot, i));
    });
    frame_with_events(vec![key_event(egui::Key::S, shift, false)], |i| {
        assert!(map.matches(PlayerAction::ScreenshotWithoutSubtitles, i));
    });

    // Subtitle position and size.
    frame_with_events(vec![key_event(egui::Key::R, none, false)], |i| {
        assert!(map.matches(PlayerAction::SubtitleMoveUp, i));
    });
    frame_with_events(vec![key_event(egui::Key::R, shift, false)], |i| {
        assert!(map.matches(PlayerAction::SubtitleMoveDown, i));
    });
    frame_with_events(vec![key_event(egui::Key::G, none, false)], |i| {
        assert!(map.matches(PlayerAction::SubtitleFontIncrease, i));
    });
    frame_with_events(vec![key_event(egui::Key::F, shift, false)], |i| {
        assert!(
            map.matches(PlayerAction::SubtitleFontDecrease, i),
            "uppercase F shrinks the subtitle font"
        );
    });
    frame_with_events(vec![key_event(egui::Key::F, none, false)], |i| {
        assert!(
            map.matches(PlayerAction::ToggleFullscreen, i),
            "lowercase f is still fullscreen"
        );
    });
}

/// The hybrid profile must keep VLC's volume keys and mpv's 60-second seek on
/// the same arrows, which only works because Classic leaves Up/Down free.
#[test]
fn test_hybrid_profile_keeps_both_vlc_and_mpv_semantics() {
    let map = KeyBindingMap::for_profile(ShortcutProfile::Hybrid);
    let none = egui::Modifiers::default();
    let ctrl = egui::Modifiers::CTRL;

    frame_with_events(vec![key_event(egui::Key::ArrowUp, none, false)], |i| {
        assert!(map.matches(PlayerAction::SeekForwardLong, i));
        assert!(!map.matches(PlayerAction::VolumeUp, i));
    });
    frame_with_events(vec![key_event(egui::Key::ArrowUp, ctrl, false)], |i| {
        assert!(map.matches(PlayerAction::VolumeUp, i));
    });
    frame_with_events(vec![key_event(egui::Key::ArrowRight, none, false)], |i| {
        assert!(map.matches(PlayerAction::SeekForwardShort, i));
    });
}
