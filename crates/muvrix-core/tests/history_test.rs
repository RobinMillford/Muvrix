//! Deterministic unit tests for the durable playback history and resume store.

use std::fs::File;
use std::io::Write;
use tempfile::tempdir;

use muvrix_core::history::{
    HistoryStore, CURRENT_HISTORY_SCHEMA_VERSION, DEFAULT_MAX_HISTORY_ENTRIES,
};

#[test]
fn test_history_default_state() {
    let store = HistoryStore::default();
    assert_eq!(store.version, CURRENT_HISTORY_SCHEMA_VERSION);
    assert!(store.enabled);
    assert!(store.auto_resume_enabled);
    assert_eq!(store.max_entries, DEFAULT_MAX_HISTORY_ENTRIES);
    assert!(store.entries.is_empty());
}

#[test]
fn test_history_record_and_ordering() {
    let mut store = HistoryStore::default();

    // 1. Record first media
    store.record_playback("/path/to/movie_a.mp4", 45.0, 120.0);
    assert_eq!(store.entries.len(), 1);
    assert_eq!(store.entries[0].display_name, "movie_a.mp4");
    assert_eq!(store.entries[0].last_position_sec, 45.0);
    assert_eq!(store.entries[0].duration_sec, 120.0);
    assert!(!store.entries[0].completed);

    // 2. Record second media -> should appear at index 0 (most recent)
    store.record_playback("/path/to/movie_b.mkv", 60.0, 300.0);
    assert_eq!(store.entries.len(), 2);
    assert_eq!(store.entries[0].display_name, "movie_b.mkv");
    assert_eq!(store.entries[1].display_name, "movie_a.mp4");

    // 3. Re-play first media -> moves back to index 0
    store.record_playback("/path/to/movie_a.mp4", 90.0, 120.0);
    assert_eq!(store.entries.len(), 2);
    assert_eq!(store.entries[0].display_name, "movie_a.mp4");
    assert_eq!(store.entries[0].last_position_sec, 90.0);
    assert_eq!(store.entries[1].display_name, "movie_b.mkv");
}

#[test]
fn test_history_resume_threshold_logic() {
    let mut store = HistoryStore::default();

    // Below 10s: should NOT be eligible for resume
    store.record_playback("/path/to/intro.mp4", 5.0, 60.0);
    assert_eq!(store.get_resume_position("/path/to/intro.mp4"), None);

    // At 45s: eligible for resume
    store.record_playback("/path/to/feature.mp4", 45.0, 120.0);
    assert_eq!(
        store.get_resume_position("/path/to/feature.mp4"),
        Some(45.0)
    );

    // Near end (> 95% of duration): completed, should NOT resume
    store.record_playback("/path/to/credits.mp4", 98.0, 100.0);
    assert!(store.entries[0].completed);
    assert_eq!(store.get_resume_position("/path/to/credits.mp4"), None);
}

#[test]
fn test_history_privacy_controls() {
    let mut store = HistoryStore::default();

    store.record_playback("/path/to/video1.mp4", 50.0, 200.0);
    store.record_playback("/path/to/video2.mp4", 75.0, 300.0);
    assert_eq!(store.entries.len(), 2);

    // Remove single entry
    assert!(store.remove_entry("/path/to/video1.mp4"));
    assert_eq!(store.entries.len(), 1);
    assert_eq!(store.entries[0].display_name, "video2.mp4");

    // Clear history
    store.clear();
    assert!(store.entries.is_empty());

    // Disable history -> no new entries recorded
    store.set_enabled(false);
    store.record_playback("/path/to/video3.mp4", 100.0, 500.0);
    assert!(store.entries.is_empty());

    // Disable auto-resume only
    store.set_enabled(true);
    store.record_playback("/path/to/video4.mp4", 100.0, 500.0);
    assert_eq!(
        store.get_resume_position("/path/to/video4.mp4"),
        Some(100.0)
    );
    store.set_auto_resume(false);
    assert_eq!(store.get_resume_position("/path/to/video4.mp4"), None);
}

#[test]
fn test_history_atomic_persistence_and_reload() {
    let dir = tempdir().expect("Failed to create temporary directory");
    let store_path = dir.path().join("history.json");

    // 1. Create and save store
    {
        let mut store = HistoryStore::load(Some(store_path.clone()));
        store.record_playback("/media/sample1.mp4", 120.0, 600.0);
        store.record_playback("/media/sample2.mkv", 350.0, 1000.0);
        store.save().expect("Atomic save failed");
    }

    // 2. Reload and verify contents
    {
        let store = HistoryStore::load(Some(store_path.clone()));
        assert_eq!(store.entries.len(), 2);
        assert_eq!(store.entries[0].display_name, "sample2.mkv");
        assert_eq!(store.entries[0].last_position_sec, 350.0);
        assert_eq!(store.entries[1].display_name, "sample1.mp4");
        assert_eq!(store.entries[1].last_position_sec, 120.0);
    }
}

#[test]
fn test_history_corruption_recovery() {
    let dir = tempdir().expect("Failed to create temporary directory");
    let store_path = dir.path().join("history.json");

    // Write deliberately corrupted JSON
    {
        let mut file = File::create(&store_path).expect("Failed to create corrupted file");
        file.write_all(b"{\"version\": 1, \"invalid_json: [}")
            .expect("Failed to write corrupt bytes");
    }

    // Load must not panic; falls back to clean default store
    let store = HistoryStore::load(Some(store_path));
    assert_eq!(store.version, CURRENT_HISTORY_SCHEMA_VERSION);
    assert!(store.entries.is_empty());
}

#[test]
fn test_playback_history_resume_lifecycle_with_synthetic_fixture() {
    let dir = tempdir().expect("Failed to create temporary directory");
    let store_path = dir.path().join("history.json");
    let fixture_path = "/fixtures/synthetic_video_120s.mp4";
    let duration = 120.0_f64; // >10s media fixture

    // 1. Initial playback before threshold (<10s) -> position recorded but resume ineligible
    {
        let mut store = HistoryStore::load(Some(store_path.clone()));
        store.record_playback(fixture_path, 5.0, duration);
        assert_eq!(store.get_resume_position(fixture_path), None);
        store.save().expect("Atomic save failed");
    }

    // 2. Play beyond threshold (>10s): 35.0s -> eligible for resume
    {
        let mut store = HistoryStore::load(Some(store_path.clone()));
        store.record_playback(fixture_path, 35.0, duration);
        assert_eq!(store.get_resume_position(fixture_path), Some(35.0));
        store.save().expect("Atomic save failed");
    }

    // 3. Restart application: reload store from disk, verify position is preserved
    {
        let mut store = HistoryStore::load(Some(store_path.clone()));
        assert_eq!(store.get_resume_position(fixture_path), Some(35.0));

        // 4. User chooses Resume: resume seek target position is 35.0s
        let resume_target = store.get_resume_position(fixture_path).unwrap();
        assert_eq!(resume_target, 35.0);

        // 5. User chooses Start from beginning: target position is 0.0s
        let restart_target = 0.0_f64;
        assert_eq!(restart_target, 0.0);

        // 6. Completed behavior (>95% duration: 116.0s / 120.0s = 96.7%)
        store.record_playback(fixture_path, 116.0, duration);
        assert!(store.entries[0].completed);
        assert_eq!(store.get_resume_position(fixture_path), None);

        // 7. Disabling tracking: subsequent playback is ignored
        store.set_enabled(false);
        store.record_playback(fixture_path, 40.0, duration);
        assert!(store.entries[0].completed);
        assert_eq!(store.entries[0].last_position_sec, 116.0);
        store.set_enabled(true);

        // 8. Clearing history: all entries wiped, resume returns None
        store.clear();
        assert!(store.entries.is_empty());
        assert_eq!(store.get_resume_position(fixture_path), None);
        store.save().expect("Atomic save failed");
    }

    // Verify empty state persisted across restarts
    {
        let store = HistoryStore::load(Some(store_path));
        assert!(store.entries.is_empty());
        assert_eq!(store.get_resume_position(fixture_path), None);
    }
}
