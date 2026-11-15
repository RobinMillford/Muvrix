//! Unit and integration tests for UI helpers, progress calculations, and formatting.

use muvrix_ui::app::{
    calculate_progress, format_codec_name, format_speed, format_timestamp, format_track_label,
    format_volume,
};
use muvrix_ui::views::{format_file_size, format_timestamp_relative, AppView};

#[test]
fn test_timestamp_formatting() {
    assert_eq!(format_timestamp(0.0), "00:00");
    assert_eq!(format_timestamp(5.2), "00:05");
    assert_eq!(format_timestamp(59.9), "00:59");
    assert_eq!(format_timestamp(60.0), "01:00");
    assert_eq!(format_timestamp(3599.0), "59:59");
    assert_eq!(format_timestamp(3600.0), "01:00:00");
    assert_eq!(format_timestamp(3665.0), "01:01:05");
    assert_eq!(format_timestamp(7325.0), "02:02:05");
    assert_eq!(format_timestamp(-1.0), "00:00");
    assert_eq!(format_timestamp(f64::NAN), "00:00");
    assert_eq!(format_timestamp(f64::INFINITY), "00:00");
}

#[test]
fn test_calculate_progress() {
    // Zero / NaN / Negative guards
    assert_eq!(calculate_progress(0.0, 0.0), 0.0);
    assert_eq!(calculate_progress(-10.0, 100.0), 0.0);
    assert_eq!(calculate_progress(50.0, -100.0), 0.0);
    assert_eq!(calculate_progress(f64::NAN, 100.0), 0.0);
    assert_eq!(calculate_progress(50.0, f64::NAN), 0.0);

    // Standard fractions
    assert!((calculate_progress(25.0, 100.0) - 0.25).abs() < 1e-5);
    assert!((calculate_progress(50.0, 100.0) - 0.50).abs() < 1e-5);
    assert!((calculate_progress(75.0, 100.0) - 0.75).abs() < 1e-5);
    assert!((calculate_progress(100.0, 100.0) - 1.0).abs() < 1e-5);

    // Clamping over 100%
    assert_eq!(calculate_progress(120.0, 100.0), 1.0);
}

#[test]
fn test_format_speed() {
    assert_eq!(format_speed(1.0), "1.0x (Normal)");
    assert_eq!(format_speed(1.0001), "1.0x (Normal)");
    assert_eq!(format_speed(0.5), "0.50x");
    assert_eq!(format_speed(0.75), "0.75x");
    assert_eq!(format_speed(1.25), "1.25x");
    assert_eq!(format_speed(1.5), "1.50x");
    assert_eq!(format_speed(2.0), "2.00x");
    assert_eq!(format_speed(3.5), "3.50x");
}

#[test]
fn test_format_volume() {
    assert_eq!(format_volume(100.0, false), "100%");
    assert_eq!(format_volume(75.4, false), "75%");
    assert_eq!(format_volume(0.0, false), "0%");
    assert_eq!(format_volume(50.0, true), "Muted (0%)");
    assert_eq!(format_volume(100.0, true), "Muted (0%)");

    // Clamping check
    assert_eq!(format_volume(150.0, false), "100%");
    assert_eq!(format_volume(-10.0, false), "0%");
}

#[test]
fn test_volume_clamping_logic() {
    let clamp_vol = |vol: f64, delta: f64| (vol + delta).clamp(0.0, 100.0);
    assert_eq!(clamp_vol(95.0, 10.0), 100.0);
    assert_eq!(clamp_vol(5.0, -10.0), 0.0);
    assert_eq!(clamp_vol(50.0, 5.0), 55.0);
    assert_eq!(clamp_vol(50.0, -5.0), 45.0);
}

#[test]
fn test_speed_adjustment_logic() {
    let adjust_spd = |spd: f64, delta: f64| {
        let rounded = ((spd + delta) * 100.0).round() / 100.0;
        rounded.clamp(0.25, 4.0)
    };
    assert_eq!(adjust_spd(1.0, 0.25), 1.25);
    assert_eq!(adjust_spd(1.0, -0.25), 0.75);
    assert_eq!(adjust_spd(0.25, -0.25), 0.25); // Clamped minimum
    assert_eq!(adjust_spd(4.0, 0.5), 4.0); // Clamped maximum
}

#[test]
fn test_format_file_size() {
    assert_eq!(format_file_size(0), "-");
    assert_eq!(format_file_size(512), "512 B");
    assert_eq!(format_file_size(1024), "1.0 KB");
    assert_eq!(format_file_size(1536), "1.5 KB");
    assert_eq!(format_file_size(1024 * 1024), "1.0 MB");
    assert_eq!(format_file_size(25 * 1024 * 1024), "25.0 MB");
    assert_eq!(format_file_size(1024 * 1024 * 1024), "1.0 GB");
    assert_eq!(format_file_size(2684354560), "2.5 GB");
}

#[test]
fn test_format_timestamp_relative() {
    assert_eq!(format_timestamp_relative(0), "Recently");

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    assert_eq!(format_timestamp_relative(now), "Just now");
    assert_eq!(format_timestamp_relative(now - 30), "Just now");
    assert_eq!(format_timestamp_relative(now - 120), "2m ago");
    assert_eq!(format_timestamp_relative(now - 7200), "2h ago");
    assert_eq!(format_timestamp_relative(now - 172800), "2d ago");
}

#[test]
fn test_app_view_labels_and_icons() {
    assert_eq!(AppView::Home.label(), "Home");
    assert_eq!(AppView::Player.label(), "Player");
    assert_eq!(AppView::Browser.label(), "Browser");
    assert_eq!(AppView::Playlist.label(), "Playlist");
    assert_eq!(AppView::Lyrics.label(), "Lyrics");
    assert_eq!(AppView::Recent.label(), "History");
    assert_eq!(AppView::Settings.label(), "Settings");

    // All views must provide distinct vector icons
    let views = [
        AppView::Player,
        AppView::Browser,
        AppView::Playlist,
        AppView::Lyrics,
        AppView::Recent,
        AppView::Settings,
    ];
    for i in 0..views.len() {
        for j in (i + 1)..views.len() {
            assert_ne!(views[i].icon(), views[j].icon());
        }
    }
}

#[test]
fn test_codec_and_track_formatting() {
    assert_eq!(format_codec_name("h264"), "H.264");
    assert_eq!(format_codec_name("hevc"), "HEVC");
    assert_eq!(format_codec_name("vp9"), "VP9");
    assert_eq!(format_codec_name("av1"), "AV1");
    assert_eq!(format_codec_name("flac"), "FLAC");
    assert_eq!(format_codec_name("opus"), "Opus");

    let track = muvrix_player::TrackInfo {
        id: 1,
        track_type: muvrix_player::TrackType::Audio,
        title: Some("Surround Sound".to_string()),
        lang: Some("eng".to_string()),
        selected: true,
        external: false,
        codec: Some("aac".to_string()),
    };
    let formatted = format_track_label(&track);
    assert!(formatted.contains("Surround Sound"));
    assert!(formatted.contains("English"));
    assert!(formatted.contains("[AAC]"));
}
