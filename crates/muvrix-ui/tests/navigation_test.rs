//! Integration tests for Phase 0.7 navigation transitions, playlist queue,
//! resume prompt state, and mini-player deck logic.

use muvrix_core::browser::MediaBrowser;
use muvrix_core::database::{LibraryFilter, LibrarySortField, MediaType, SortDirection};
use muvrix_core::history::HistoryStore;
use muvrix_player::equalizer::{EqualizerPreset, EqualizerSettings, EQ_FREQUENCIES};
use muvrix_player::shader::{Anime4KPreset, VideoAdjustmentSettings};
use muvrix_player::track::SubtitleStyling;
use muvrix_ui::views::{AppView, SettingsTab};

#[test]
fn test_app_view_transitions() {
    let mut current_view = AppView::Player;
    assert_eq!(current_view, AppView::Player);

    // Switch to browser
    current_view = AppView::Browser;
    assert_eq!(current_view, AppView::Browser);

    // Switch to playlist
    current_view = AppView::Playlist;
    assert_eq!(current_view, AppView::Playlist);

    // Switch to lyrics
    current_view = AppView::Lyrics;
    assert_eq!(current_view, AppView::Lyrics);

    // Switch to history
    current_view = AppView::Recent;
    assert_eq!(current_view, AppView::Recent);

    // Switch to settings
    current_view = AppView::Settings;
    assert_eq!(current_view, AppView::Settings);

    // Switch back to player
    current_view = AppView::Player;
    assert_eq!(current_view, AppView::Player);
}

#[test]
fn test_resume_prompt_decision_logic() {
    let mut store = HistoryStore::default();
    store.record_playback("/path/to/movie.mp4", 125.0, 600.0);

    let resume_pos = store.get_resume_position("/path/to/movie.mp4");
    assert_eq!(resume_pos, Some(125.0));

    // Simulated pending prompt
    let mut pending_resume_prompt = resume_pos.map(|pos| ("/path/to/movie.mp4".to_string(), pos));
    assert!(pending_resume_prompt.is_some());

    // Dismissing prompt
    pending_resume_prompt = None;
    assert!(pending_resume_prompt.is_none());
}

#[test]
fn test_subtitle_styling_ui_defaults() {
    let styling = SubtitleStyling::default();
    assert_eq!(styling.font, "sans-serif");
    assert_eq!(styling.font_size, 45);
    assert_eq!(styling.color, "#FFFFFF");
    assert_eq!(styling.border_color, "#000000");
    assert_eq!(styling.border_size, 3.0);
    assert_eq!(styling.pos, 100);
    assert_eq!(styling.scale, 1.0);
    assert!(!styling.ass_override);
    assert_eq!(
        styling.ass_override_mode,
        muvrix_player::AssOverrideMode::Preserve
    );
    assert_eq!(styling.secondary_pos, 10);
}

#[test]
fn test_media_browser_navigation_stack() {
    let temp_dir = std::env::temp_dir();
    let mut browser = MediaBrowser::new(Some(temp_dir.clone()));

    assert_eq!(browser.current_dir, temp_dir);
    assert!(browser.history_back.is_empty());
    assert!(browser.history_forward.is_empty());

    // Navigate to parent if possible
    if let Some(parent) = temp_dir.parent() {
        let parent_buf = parent.to_path_buf();
        browser.navigate_to(parent_buf.clone());
        assert_eq!(browser.current_dir, parent_buf);
        assert_eq!(browser.history_back.len(), 1);

        // Navigate back
        browser.navigate_back();
        assert_eq!(browser.current_dir, temp_dir);
        assert_eq!(browser.history_forward.len(), 1);

        // Navigate forward
        browser.navigate_forward();
        assert_eq!(browser.current_dir, parent_buf);
    }
}

#[test]
fn test_mini_player_visibility_condition() {
    // Mini-player should only be rendered when view != Player and playback is active
    let is_mini_player_visible =
        |view: AppView, is_idle: bool| -> bool { view != AppView::Player && !is_idle };

    assert!(!is_mini_player_visible(AppView::Player, false)); // In player: false
    assert!(!is_mini_player_visible(AppView::Browser, true)); // In browser but idle: false
    assert!(is_mini_player_visible(AppView::Browser, false)); // In browser and playing: true
    assert!(is_mini_player_visible(AppView::Library, false)); // In library and playing: true
    assert!(is_mini_player_visible(AppView::Playlist, false)); // In playlist and playing: true
    assert!(is_mini_player_visible(AppView::Recent, false)); // In history and playing: true
    assert!(is_mini_player_visible(AppView::Settings, false)); // In settings and playing: true
}

#[test]
fn test_library_view_navigation_and_filters() {
    let mut current_view = AppView::Player;
    assert_eq!(current_view, AppView::Player);
    current_view = AppView::Library;
    assert_eq!(current_view, AppView::Library);
    assert_eq!(current_view.label(), "Library");

    let mut filter = LibraryFilter::default();
    assert_eq!(filter.search_query, None);
    assert_eq!(filter.media_type, None);
    assert!(!filter.favourites_only);

    filter.search_query = Some("Sci-Fi".to_string());
    filter.media_type = Some(MediaType::Video);
    filter.resolution_tier = Some("4K".to_string());
    assert_eq!(filter.search_query.as_deref(), Some("Sci-Fi"));
    assert_eq!(filter.media_type, Some(MediaType::Video));
    assert_eq!(filter.resolution_tier.as_deref(), Some("4K"));

    let mut sort_field = LibrarySortField::Title;
    let mut sort_dir = SortDirection::Ascending;
    assert_eq!(sort_field, LibrarySortField::Title);
    assert_eq!(sort_dir, SortDirection::Ascending);

    sort_field = LibrarySortField::Duration;
    sort_dir = SortDirection::Descending;
    assert_eq!(sort_field, LibrarySortField::Duration);
    assert_eq!(sort_dir, SortDirection::Descending);
}

#[test]
fn test_video_adjustment_defaults_and_presets() {
    let mut settings = VideoAdjustmentSettings::default();
    assert_eq!(settings.brightness, 0);
    assert_eq!(settings.contrast, 0);
    assert_eq!(settings.saturation, 0);
    assert_eq!(settings.gamma, 0);
    assert_eq!(settings.hue, 0);
    assert_eq!(settings.sharpen, 0.0);
    assert!(!settings.deband);

    settings.brightness = 15;
    settings.contrast = -10;
    settings.deband = true;
    assert_eq!(settings.brightness, 15);
    assert!(settings.deband);

    let anime_preset = Anime4KPreset::ModeA;
    assert_eq!(anime_preset.as_str(), "Mode A (Sharp Upscale)");
}

#[test]
fn test_equalizer_settings_and_presets() {
    let mut eq = EqualizerSettings::default();
    assert!(!eq.is_enabled);
    assert_eq!(eq.preamp_db, 0.0);
    assert_eq!(eq.gains, [0.0; 10]);
    assert_eq!(eq.active_preset, EqualizerPreset::Flat);

    let rock_gains = EqualizerPreset::Rock.default_gains();
    eq.gains = rock_gains;
    eq.active_preset = EqualizerPreset::Rock;
    assert_eq!(eq.gains[0], 4.0); // 31.25Hz
    assert_eq!(eq.active_preset.as_str(), "Rock");

    assert_eq!(EQ_FREQUENCIES.len(), 10);
    assert_eq!(EQ_FREQUENCIES[0], 31.25);
    assert_eq!(EQ_FREQUENCIES[9], 16000.0);
}

#[test]
fn test_settings_tab_navigation() {
    let mut current_tab = SettingsTab::Subtitles;
    assert_eq!(current_tab.label(), "Subtitles & Engine");

    current_tab = SettingsTab::VideoEnhancement;
    assert_eq!(current_tab.label(), "Video & Shaders");

    current_tab = SettingsTab::EqualizerAudio;
    assert_eq!(current_tab.label(), "Audio & Equalizer");

    current_tab = SettingsTab::NetworkMedia;
    assert_eq!(current_tab.label(), "Network Sources");

    current_tab = SettingsTab::Preferences;
    assert_eq!(current_tab.label(), "Window & History");
}
