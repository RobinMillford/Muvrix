//! Deterministic unit tests for the lightweight local filesystem media browser.

use std::fs::{self, File};
use tempfile::tempdir;

use muvrix_core::browser::{EntryKind, MediaBrowser, MediaFilter, SortBy};

#[test]
fn test_browser_extension_classification() {
    assert_eq!(
        MediaBrowser::classify_extension("mp4"),
        EntryKind::VideoFile
    );
    assert_eq!(
        MediaBrowser::classify_extension("MKV"),
        EntryKind::VideoFile
    );
    assert_eq!(
        MediaBrowser::classify_extension("webm"),
        EntryKind::VideoFile
    );
    assert_eq!(
        MediaBrowser::classify_extension("flac"),
        EntryKind::AudioFile
    );
    assert_eq!(
        MediaBrowser::classify_extension("MP3"),
        EntryKind::AudioFile
    );
    assert_eq!(
        MediaBrowser::classify_extension("wav"),
        EntryKind::AudioFile
    );
    assert_eq!(
        MediaBrowser::classify_extension("txt"),
        EntryKind::OtherFile
    );
    assert_eq!(MediaBrowser::classify_extension("rs"), EntryKind::OtherFile);
}

#[test]
fn test_browser_directory_listing_and_filtering() {
    let dir = tempdir().expect("Failed to create tempdir");
    let root = dir.path();

    // Create folders and mock files
    fs::create_dir(root.join("SubFolder")).unwrap();
    File::create(root.join("video1.mp4")).unwrap();
    File::create(root.join("video2.mkv")).unwrap();
    File::create(root.join("audio1.flac")).unwrap();
    File::create(root.join("readme.txt")).unwrap();
    File::create(root.join(".hidden.mp4")).unwrap(); // Hidden dotfile

    let mut browser = MediaBrowser::new(Some(root.to_path_buf()));
    assert!(browser.error.is_none());

    // 1. AllMedia: should contain SubFolder + 2 videos + 1 audio (ignores readme.txt and .hidden.mp4)
    let all = browser.filtered_entries();
    assert_eq!(all.len(), 4);
    assert_eq!(all[0].name, "SubFolder");
    assert_eq!(all[0].kind, EntryKind::Directory);

    // 2. VideoOnly filter: should contain SubFolder + 2 videos
    browser.filter = MediaFilter::VideoOnly;
    let videos = browser.filtered_entries();
    assert_eq!(videos.len(), 3);
    assert_eq!(videos[0].name, "SubFolder");
    assert!(videos.iter().any(|e| e.name == "video1.mp4"));
    assert!(videos.iter().any(|e| e.name == "video2.mkv"));
    assert!(!videos.iter().any(|e| e.name == "audio1.flac"));

    // 3. AudioOnly filter: should contain SubFolder + 1 audio
    browser.filter = MediaFilter::AudioOnly;
    let audios = browser.filtered_entries();
    assert_eq!(audios.len(), 2);
    assert_eq!(audios[0].name, "SubFolder");
    assert!(audios.iter().any(|e| e.name == "audio1.flac"));
}

#[test]
fn test_browser_sorting() {
    let dir = tempdir().expect("Failed to create tempdir");
    let root = dir.path();

    fs::create_dir(root.join("Z_Folder")).unwrap();
    fs::create_dir(root.join("A_Folder")).unwrap();
    File::create(root.join("b_video.mp4")).unwrap();
    File::create(root.join("a_video.mp4")).unwrap();

    let mut browser = MediaBrowser::new(Some(root.to_path_buf()));

    // NameAsc: Directories first (A_Folder, Z_Folder), then files (a_video, b_video)
    browser.set_sort_by(SortBy::NameAsc);
    let names: Vec<&str> = browser
        .filtered_entries()
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec!["A_Folder", "Z_Folder", "a_video.mp4", "b_video.mp4"]
    );

    // NameDesc: Directories first (Z_Folder, A_Folder), then files (b_video, a_video)
    browser.set_sort_by(SortBy::NameDesc);
    let names_desc: Vec<&str> = browser
        .filtered_entries()
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    assert_eq!(
        names_desc,
        vec!["Z_Folder", "A_Folder", "b_video.mp4", "a_video.mp4"]
    );
}

#[test]
fn test_browser_search_filter() {
    let dir = tempdir().expect("Failed to create tempdir");
    let root = dir.path();

    File::create(root.join("Iron Man.mp4")).unwrap();
    File::create(root.join("Spider Man.mkv")).unwrap();
    File::create(root.join("Batman.mp4")).unwrap();

    let mut browser = MediaBrowser::new(Some(root.to_path_buf()));
    assert_eq!(browser.filtered_entries().len(), 3);

    browser.search_query = "spider".to_string();
    let res = browser.filtered_entries();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].name, "Spider Man.mkv");

    browser.search_query = "man".to_string();
    let res2 = browser.filtered_entries();
    assert_eq!(res2.len(), 3); // "Iron Man", "Spider Man", "Batman"
}

#[test]
fn test_browser_navigation_history() {
    let dir = tempdir().expect("Failed to create tempdir");
    let root = dir.path();
    let sub = root.join("Sub");
    fs::create_dir(&sub).unwrap();

    let mut browser = MediaBrowser::new(Some(root.to_path_buf()));
    assert_eq!(browser.current_dir, root);
    assert!(browser.history_back.is_empty());

    // Navigate to Sub
    browser.navigate_to(sub.clone());
    assert_eq!(browser.current_dir, sub);
    assert_eq!(browser.history_back.len(), 1);

    // Navigate Back
    browser.navigate_back();
    assert_eq!(browser.current_dir, root);
    assert_eq!(browser.history_forward.len(), 1);

    // Navigate Forward
    browser.navigate_forward();
    assert_eq!(browser.current_dir, sub);

    // Navigate Parent
    browser.navigate_parent();
    assert_eq!(browser.current_dir, root);
}

#[test]
fn test_browser_missing_directory_graceful_error() {
    let dir = tempdir().expect("Failed to create tempdir");
    let non_existent = dir.path().join("does_not_exist");

    let browser = MediaBrowser::new(Some(non_existent));
    assert!(browser.error.is_some());
    assert!(browser.entries.is_empty());
}
