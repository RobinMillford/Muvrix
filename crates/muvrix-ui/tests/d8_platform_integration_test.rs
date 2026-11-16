use std::path::Path;

use muvrix_ui::os_integration::{should_inhibit, MediaAction, MediaSnapshot, SleepInhibitBackend};

#[test]
fn test_media_snapshot_extended_metadata() {
    let snapshot = MediaSnapshot {
        title: "Test Track Title".to_string(),
        artist: "Test Artist".to_string(),
        album: "Test Album 2026".to_string(),
        art_url: Some("file:///tmp/artwork.jpg".to_string()),
        track_number: Some(7),
        playback_status: "Playing".to_string(),
        duration_us: 180_000_000,
        position_us: 45_000_000,
        volume: 0.85,
        can_next: true,
        can_previous: false,
    };

    assert!(snapshot.is_playing());
    assert_eq!(snapshot.title, "Test Track Title");
    assert_eq!(snapshot.artist, "Test Artist");
    assert_eq!(snapshot.album, "Test Album 2026");
    assert_eq!(snapshot.art_url.as_deref(), Some("file:///tmp/artwork.jpg"));
    assert_eq!(snapshot.track_number, Some(7));
    assert_eq!(snapshot.duration_us, 180_000_000);
    assert_eq!(snapshot.position_us, 45_000_000);
    assert!((snapshot.volume - 0.85).abs() < 1e-6);
    assert!(snapshot.can_next);
    assert!(!snapshot.can_previous);
}

#[test]
fn test_media_action_variants() {
    let action_play = MediaAction::Play;
    let action_vol = MediaAction::SetVolume(0.5);
    let action_seek = MediaAction::SeekTo(12_000_000);

    assert_eq!(action_play, MediaAction::Play);
    assert_eq!(action_vol, MediaAction::SetVolume(0.5));
    assert_eq!(action_seek, MediaAction::SeekTo(12_000_000));
}

#[test]
fn test_sleep_inhibit_backend_labels() {
    assert_eq!(
        SleepInhibitBackend::ScreenSaver.label(),
        "org.freedesktop.ScreenSaver"
    );
    assert_eq!(
        SleepInhibitBackend::PowerManagement.label(),
        "org.freedesktop.PowerManagement.Inhibit"
    );
    assert_eq!(
        SleepInhibitBackend::None.label(),
        "None (Uninhibited / Standby)"
    );
}

#[test]
fn test_should_inhibit_logic() {
    // Playing with an active file: must inhibit
    assert!(should_inhibit(true, false, true));

    // Paused media: must NOT inhibit
    assert!(!should_inhibit(false, true, true));
    assert!(!should_inhibit(true, true, true));

    // Stopped/idle (no active file): must NOT inhibit
    assert!(!should_inhibit(true, false, false));
    assert!(!should_inhibit(false, false, false));
}

#[test]
fn test_packaging_manifests_exist_and_non_empty() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let manifests = [
        root.join("packaging/appimage/AppRun"),
        root.join("packaging/appimage/muvrix.desktop"),
        root.join("packaging/flatpak/org.muvrix.Muvrix.yaml"),
        root.join("packaging/deb/debian/control"),
        root.join("packaging/deb/debian/rules"),
        root.join("packaging/deb/debian/changelog"),
        root.join("packaging/rpm/muvrix.spec"),
        root.join("packaging/metainfo/org.muvrix.Muvrix.metainfo.xml"),
        root.join("packaging/build_all_packages.sh"),
    ];

    for manifest in &manifests {
        assert!(
            manifest.exists(),
            "Manifest file must exist: {}",
            manifest.display()
        );
        let len = std::fs::metadata(manifest)
            .unwrap_or_else(|_| panic!("Failed to read metadata for {}", manifest.display()))
            .len();
        assert!(
            len > 50,
            "Manifest file is suspiciously small ({} bytes): {}",
            len,
            manifest.display()
        );
    }
}
