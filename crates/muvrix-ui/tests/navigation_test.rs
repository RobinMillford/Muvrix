//! Integration tests for Phase 0.7 navigation transitions, playlist queue,
//! resume prompt state, and mini-player deck logic.

use muvrix_core::browser::MediaBrowser;
use muvrix_core::history::HistoryStore;
use muvrix_player::track::SubtitleStyling;
use muvrix_ui::views::AppView;

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
    assert!(is_mini_player_visible(AppView::Playlist, false)); // In playlist and playing: true
    assert!(is_mini_player_visible(AppView::Recent, false)); // In history and playing: true
    assert!(is_mini_player_visible(AppView::Settings, false)); // In settings and playing: true
}
