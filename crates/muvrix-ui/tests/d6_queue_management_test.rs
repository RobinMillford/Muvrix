//! Integration tests for Milestone D6: Advanced Queue Management, Smart Shuffle & Playlist Engine.

use muvrix_core::database::{ActiveQueueEntry, DatabaseManager};
use muvrix_core::playlist::{export_pls, export_xspf, load_playlist_file_or_url, M3uEntry};
use muvrix_player::shuffle::{ShuffleTrackInfo, SmartShuffleEngine};

#[test]
fn test_d6_pls_roundtrip_parsing_and_generation() {
    let pls_path = std::env::temp_dir().join(format!("test_playlist_{}.pls", std::process::id()));

    let entries = vec![
        M3uEntry {
            path: "/media/music/track1.flac".to_string(),
            title: Some("Symphony No. 5".to_string()),
            duration_ms: 180_000,
            logo: None,
            group: None,
        },
        M3uEntry {
            path: "/media/music/track2.flac".to_string(),
            title: Some("Moonlight Sonata".to_string()),
            duration_ms: 320_000,
            logo: None,
            group: None,
        },
    ];

    export_pls(&entries, pls_path.to_str().unwrap()).expect("PLS export failed");

    let imported =
        load_playlist_file_or_url(pls_path.to_str().unwrap()).expect("PLS import failed");
    let _ = std::fs::remove_file(&pls_path);

    assert_eq!(imported.len(), 2);
    assert_eq!(imported[0].path, "/media/music/track1.flac");
    assert_eq!(imported[0].title.as_deref(), Some("Symphony No. 5"));
    assert_eq!(imported[0].duration_ms, 180_000);
    assert_eq!(imported[1].path, "/media/music/track2.flac");
    assert_eq!(imported[1].title.as_deref(), Some("Moonlight Sonata"));
    assert_eq!(imported[1].duration_ms, 320_000);
}

#[test]
fn test_d6_xspf_roundtrip_parsing_and_generation() {
    let xspf_path = std::env::temp_dir().join(format!("test_playlist_{}.xspf", std::process::id()));

    let entries = vec![
        M3uEntry {
            path: "file:///media/music/song_one.mp3".to_string(),
            title: Some("Song One".to_string()),
            duration_ms: 215_000,
            logo: None,
            group: None,
        },
        M3uEntry {
            path: "file:///media/music/song_two.mp3".to_string(),
            title: Some("Song Two".to_string()),
            duration_ms: 190_000,
            logo: None,
            group: None,
        },
    ];

    export_xspf("My Test XSPF", &entries, xspf_path.to_str().unwrap()).expect("XSPF export failed");

    let imported =
        load_playlist_file_or_url(xspf_path.to_str().unwrap()).expect("XSPF import failed");
    let _ = std::fs::remove_file(&xspf_path);

    assert_eq!(imported.len(), 2);
    assert_eq!(imported[0].path, "/media/music/song_one.mp3");
    assert_eq!(imported[0].title.as_deref(), Some("Song One"));
    assert_eq!(imported[0].duration_ms, 215_000);
    assert_eq!(imported[1].path, "/media/music/song_two.mp3");
    assert_eq!(imported[1].title.as_deref(), Some("Song Two"));
    assert_eq!(imported[1].duration_ms, 190_000);
}

#[test]
fn test_d6_smart_shuffle_artist_and_album_separation() {
    let tracks = vec![
        ShuffleTrackInfo {
            index: 0,
            title: Some("Track A1".to_string()),
            artist: Some("Artist Alpha".to_string()),
            album: Some("Album 1".to_string()),
            genre: Some("Rock".to_string()),
        },
        ShuffleTrackInfo {
            index: 1,
            title: Some("Track A2".to_string()),
            artist: Some("Artist Alpha".to_string()),
            album: Some("Album 1".to_string()),
            genre: Some("Rock".to_string()),
        },
        ShuffleTrackInfo {
            index: 2,
            title: Some("Track A3".to_string()),
            artist: Some("Artist Alpha".to_string()),
            album: Some("Album 1".to_string()),
            genre: Some("Rock".to_string()),
        },
        ShuffleTrackInfo {
            index: 3,
            title: Some("Track B1".to_string()),
            artist: Some("Artist Beta".to_string()),
            album: Some("Album 2".to_string()),
            genre: Some("Jazz".to_string()),
        },
        ShuffleTrackInfo {
            index: 4,
            title: Some("Track B2".to_string()),
            artist: Some("Artist Beta".to_string()),
            album: Some("Album 2".to_string()),
            genre: Some("Jazz".to_string()),
        },
        ShuffleTrackInfo {
            index: 5,
            title: Some("Track B3".to_string()),
            artist: Some("Artist Beta".to_string()),
            album: Some("Album 2".to_string()),
            genre: Some("Jazz".to_string()),
        },
        ShuffleTrackInfo {
            index: 6,
            title: Some("Track C1".to_string()),
            artist: Some("Artist Gamma".to_string()),
            album: Some("Album 3".to_string()),
            genre: Some("Electronic".to_string()),
        },
        ShuffleTrackInfo {
            index: 7,
            title: Some("Track C2".to_string()),
            artist: Some("Artist Gamma".to_string()),
            album: Some("Album 3".to_string()),
            genre: Some("Electronic".to_string()),
        },
        ShuffleTrackInfo {
            index: 8,
            title: Some("Track C3".to_string()),
            artist: Some("Artist Gamma".to_string()),
            album: Some("Album 3".to_string()),
            genre: Some("Electronic".to_string()),
        },
    ];

    let engine = SmartShuffleEngine::default();
    let seed = 123456789;
    let order = engine.calculate_order(&tracks, seed);

    assert_eq!(order.len(), 9);
    let mut seen = std::collections::HashSet::new();
    for &idx in &order {
        assert!(idx < 9);
        seen.insert(idx);
    }
    assert_eq!(seen.len(), 9, "All 9 tracks must be present exactly once");

    // Count consecutive artist matches
    let mut consecutive_artist_matches = 0;
    for i in 1..order.len() {
        let prev_artist = &tracks[order[i - 1]].artist;
        let curr_artist = &tracks[order[i]].artist;
        if prev_artist == curr_artist {
            consecutive_artist_matches += 1;
        }
    }

    assert_eq!(
        consecutive_artist_matches, 0,
        "Smart Shuffle with 3 balanced artists must achieve zero adjacent repeats"
    );
}

#[test]
fn test_d6_active_queue_persistence_lifecycle() {
    let db = DatabaseManager::open_in_memory().expect("In-memory db failed");

    let initial = db.load_active_queue().expect("Load failed");
    assert!(initial.is_empty(), "Initial active queue should be empty");

    let entries = vec![
        ActiveQueueEntry {
            position: 0,
            path: "/path/video1.mp4".to_string(),
            title: Some("Video One".to_string()),
            duration_ms: 60_000,
            is_current: false,
        },
        ActiveQueueEntry {
            position: 1,
            path: "/path/video2.mkv".to_string(),
            title: Some("Video Two".to_string()),
            duration_ms: 120_000,
            is_current: true,
        },
        ActiveQueueEntry {
            position: 2,
            path: "/path/video3.webm".to_string(),
            title: Some("Video Three".to_string()),
            duration_ms: 180_000,
            is_current: false,
        },
    ];

    db.save_active_queue(&entries)
        .expect("Save active queue failed");

    let loaded = db.load_active_queue().expect("Load active queue failed");
    assert_eq!(loaded.len(), 3);
    assert_eq!(loaded[0].position, 0);
    assert_eq!(loaded[0].path, "/path/video1.mp4");
    assert_eq!(loaded[0].title.as_deref(), Some("Video One"));
    assert!(!loaded[0].is_current);

    assert_eq!(loaded[1].position, 1);
    assert_eq!(loaded[1].path, "/path/video2.mkv");
    assert!(loaded[1].is_current);

    assert_eq!(loaded[2].position, 2);
    assert_eq!(loaded[2].path, "/path/video3.webm");
    assert!(!loaded[2].is_current);

    db.clear_active_queue().expect("Clear active queue failed");
    let after_clear = db.load_active_queue().expect("Load active queue failed");
    assert!(
        after_clear.is_empty(),
        "Active queue should be empty after clear"
    );
}

#[test]
fn test_d6_queue_drag_drop_target_math() {
    // Math validation:
    // When dragging from index `src` to hover index `tgt`:
    // If tgt > src, inserting after tgt requires index tgt + 1.
    // If tgt <= src, inserting before tgt requires index tgt.
    let reorder = |src: usize, tgt: usize| -> usize {
        if tgt > src {
            tgt + 1
        } else {
            tgt
        }
    };

    assert_eq!(reorder(0, 3), 4);
    assert_eq!(reorder(3, 0), 0);
    assert_eq!(reorder(1, 2), 3);
    assert_eq!(reorder(5, 5), 5);
}
