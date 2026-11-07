//! Integration tests for the recursive and incremental filesystem scanner in muvrix-core.

use muvrix_core::database::*;
use muvrix_core::scanner::*;
use std::fs::{self, File};
use std::io::Write;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tempfile::TempDir;

fn create_dummy_file(dir: &std::path::Path, name: &str, content: &[u8]) -> std::path::PathBuf {
    let p = dir.join(name);
    if let Some(parent) = p.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut f = File::create(&p).unwrap();
    f.write_all(content).unwrap();
    p
}

#[test]
fn test_scanner_recursive_discovery_and_extensions() {
    let temp = TempDir::new().unwrap();
    let root_path = temp.path();

    // Create a tree:
    // root/
    //   video1.mp4
    //   music1.flac
    //   text.txt (should be ignored)
    //   sub/
    //     video2.mkv
    //     audio2.mp3
    //   .git/
    //     ignored.mp4
    create_dummy_file(root_path, "video1.mp4", b"dummy video 1");
    create_dummy_file(root_path, "music1.flac", b"dummy audio 1");
    create_dummy_file(root_path, "text.txt", b"plain text");
    create_dummy_file(root_path, "sub/video2.mkv", b"dummy video 2");
    create_dummy_file(root_path, "sub/audio2.mp3", b"dummy audio 2");
    create_dummy_file(root_path, ".git/ignored.mp4", b"git ignored");

    let db = DatabaseManager::open_in_memory().unwrap();
    let root = LibraryRoot {
        id: 0,
        display_name: "Test Media".to_string(),
        root_path: root_path.to_string_lossy().to_string(),
        source_type: "local".to_string(),
        is_recursive: true,
        is_enabled: true,
        exclusion_patterns: vec![],
        last_scanned_at: 0,
        scan_status: "idle".to_string(),
    };
    let root_id = db.insert_root(&root).unwrap();
    let root_in_db = db.get_root_by_id(root_id).unwrap().unwrap();

    let cancel = Arc::new(AtomicBool::new(false));
    let options = ScanOptions::default();

    let summary = ScannerService::scan_root(&db, &root_in_db, &options, cancel, None).unwrap();

    assert_eq!(summary.items_discovered, 4); // video1, music1, video2, audio2
    assert_eq!(summary.items_indexed, 4);
    assert_eq!(summary.items_skipped, 0);
    assert_eq!(summary.items_missing, 0);
    assert_eq!(summary.errors_count, 0);

    let items = db.get_media_items_by_root(root_id).unwrap();
    assert_eq!(items.len(), 4);
}

#[test]
fn test_scanner_incremental_scanning_and_mtime_change() {
    let temp = TempDir::new().unwrap();
    let root_path = temp.path();

    let file_path = create_dummy_file(root_path, "test.mp4", b"version 1 content");

    let db = DatabaseManager::open_in_memory().unwrap();
    let root_id = db
        .insert_root(&LibraryRoot {
            id: 0,
            display_name: "Test".to_string(),
            root_path: root_path.to_string_lossy().to_string(),
            source_type: "local".to_string(),
            is_recursive: true,
            is_enabled: true,
            exclusion_patterns: vec![],
            last_scanned_at: 0,
            scan_status: "idle".to_string(),
        })
        .unwrap();
    let root_in_db = db.get_root_by_id(root_id).unwrap().unwrap();

    let cancel = Arc::new(AtomicBool::new(false));
    let options = ScanOptions::default();

    // Scan 1: initial index
    let s1 = ScannerService::scan_root(&db, &root_in_db, &options, cancel.clone(), None).unwrap();
    assert_eq!(s1.items_indexed, 1);
    assert_eq!(s1.items_skipped, 0);

    // Scan 2: unmodified file should be skipped
    let s2 = ScannerService::scan_root(&db, &root_in_db, &options, cancel.clone(), None).unwrap();
    assert_eq!(s2.items_indexed, 0);
    assert_eq!(s2.items_skipped, 1);

    // Modify file
    std::thread::sleep(std::time::Duration::from_millis(1100)); // Ensure mtime advances by at least 1s
    {
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(&file_path)
            .unwrap();
        f.write_all(b" modified extra bytes").unwrap();
    }

    // Scan 3: modified file should be re-indexed
    let s3 = ScannerService::scan_root(&db, &root_in_db, &options, cancel, None).unwrap();
    assert_eq!(s3.items_indexed, 1);
    assert_eq!(s3.items_skipped, 0);

    let item = db.get_media_items_by_root(root_id).unwrap().pop().unwrap();
    assert_eq!(item.metadata_status, MetadataStatus::Pending);
}

#[test]
fn test_scanner_reconciliation_of_deleted_files() {
    let temp = TempDir::new().unwrap();
    let root_path = temp.path();

    let file1 = create_dummy_file(root_path, "kept.mp4", b"kept");
    let file2 = create_dummy_file(root_path, "deleted.mp4", b"to delete");

    let db = DatabaseManager::open_in_memory().unwrap();
    let root_id = db
        .insert_root(&LibraryRoot {
            id: 0,
            display_name: "Test".to_string(),
            root_path: root_path.to_string_lossy().to_string(),
            source_type: "local".to_string(),
            is_recursive: true,
            is_enabled: true,
            exclusion_patterns: vec![],
            last_scanned_at: 0,
            scan_status: "idle".to_string(),
        })
        .unwrap();
    let root_in_db = db.get_root_by_id(root_id).unwrap().unwrap();

    let cancel = Arc::new(AtomicBool::new(false));
    let options = ScanOptions::default();

    // Initial scan
    ScannerService::scan_root(&db, &root_in_db, &options, cancel.clone(), None).unwrap();
    assert_eq!(db.get_media_items_by_root(root_id).unwrap().len(), 2);

    // Delete one file from disk
    fs::remove_file(&file2).unwrap();

    // Second scan
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let s2 = ScannerService::scan_root(&db, &root_in_db, &options, cancel, None).unwrap();
    assert_eq!(s2.items_missing, 1);

    let items = db.get_media_items_by_root(root_id).unwrap();
    assert_eq!(items.len(), 2);

    let kept_item = items.iter().find(|i| i.file_name == "kept.mp4").unwrap();
    let deleted_item = items.iter().find(|i| i.file_name == "deleted.mp4").unwrap();

    assert_eq!(kept_item.availability_state, AvailabilityState::Available);
    assert_eq!(
        deleted_item.availability_state,
        AvailabilityState::Unavailable
    );

    let _ = file1;
}

#[test]
fn test_scanner_cancellation() {
    let temp = TempDir::new().unwrap();
    let root_path = temp.path();

    for i in 0..20 {
        create_dummy_file(root_path, &format!("video_{}.mp4", i), b"bytes");
    }

    let db = DatabaseManager::open_in_memory().unwrap();
    let root_id = db
        .insert_root(&LibraryRoot {
            id: 0,
            display_name: "Test".to_string(),
            root_path: root_path.to_string_lossy().to_string(),
            source_type: "local".to_string(),
            is_recursive: true,
            is_enabled: true,
            exclusion_patterns: vec![],
            last_scanned_at: 0,
            scan_status: "idle".to_string(),
        })
        .unwrap();
    let root_in_db = db.get_root_by_id(root_id).unwrap().unwrap();

    let cancel = Arc::new(AtomicBool::new(true)); // Already cancelled
    let options = ScanOptions::default();

    let summary = ScannerService::scan_root(&db, &root_in_db, &options, cancel, None).unwrap();
    assert!(summary.is_cancelled);
}
