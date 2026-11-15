use muvrix_ui::app::{mini_player_window_state, sleep_deadline_for, LibrarySelection, SleepTimer};
use muvrix_ui::config_editor::{load_path, save_path};
use muvrix_ui::views::SettingsTab;

#[test]
fn sleep_timer_modes_have_stable_labels() {
    assert_eq!(SleepTimer::Off.label(), "Off");
    assert_eq!(SleepTimer::Minutes(15).label(), "15 min");
    assert_eq!(SleepTimer::Minutes(60).label(), "60 min");
    assert_eq!(SleepTimer::EndOfFile.label(), "End of file");
}

#[test]
fn timed_sleep_deadline_is_relative_to_now() {
    let now = std::time::Instant::now();
    assert_eq!(sleep_deadline_for(SleepTimer::Off, now), None);
    assert_eq!(sleep_deadline_for(SleepTimer::EndOfFile, now), None);
    assert_eq!(
        sleep_deadline_for(SleepTimer::Minutes(15), now),
        Some(now + std::time::Duration::from_secs(15 * 60))
    );
}

#[test]
fn settings_search_matches_sections_and_keywords() {
    assert!(SettingsTab::Appearance.matches_search("theme"));
    assert!(SettingsTab::Appearance.matches_search("amoled"));
    assert!(SettingsTab::ConfigEditor.matches_search("mpv.conf"));
    assert!(SettingsTab::Subtitles.matches_search("shadow"));
    assert!(SettingsTab::Preferences.matches_search("sleep"));
    assert!(!SettingsTab::NetworkMedia.matches_search("subtitle"));
    assert!(SettingsTab::NetworkMedia.matches_search(""));
}

#[test]
fn config_editor_file_roundtrip() {
    let path = std::env::temp_dir().join(format!(
        "muvrix-config-ui-{}-{}.conf",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    save_path(&path, "input-default-bindings=yes\n").expect("save");
    assert_eq!(
        load_path(&path).expect("load"),
        "input-default-bindings=yes\n"
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn library_multi_selection_supports_toggle_select_all_and_clear() {
    let mut selection = LibrarySelection::default();
    selection.toggle(4);
    selection.toggle(7);
    assert_eq!(selection.len(), 2);
    assert!(selection.contains(4));
    selection.toggle(4);
    assert!(!selection.contains(4));
    let mut ids = selection.ids();
    ids.sort_unstable();
    assert_eq!(ids, vec![7]);
    selection.select_all([1, 2, 3]);
    let mut ids = selection.ids();
    ids.sort_unstable();
    assert_eq!(ids, vec![1, 2, 3]);
    selection.clear();
    assert!(selection.is_empty());
}

#[test]
fn mini_player_requests_borderless_always_on_top_and_restores_normal() {
    assert_eq!(
        mini_player_window_state(true),
        (false, eframe::egui::WindowLevel::AlwaysOnTop)
    );
    assert_eq!(
        mini_player_window_state(false),
        (true, eframe::egui::WindowLevel::Normal)
    );
}
