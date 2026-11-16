//! Integration tests for Library Integrity Scanner, Relinking, and Catalog Cleanup.

use muvrix_core::database::{
    compute_full_file_hash, compute_piecewise_hash, find_relink_candidates,
    inspect_media_reachability, AvailabilityState, DatabaseManager, LibraryRoot, MediaItem,
    MediaReachability, MediaType, MetadataStatus,
};
use std::fs::File;
use std::io::Write;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tempfile::tempdir;

fn setup_test_db() -> (DatabaseManager, tempfile::TempDir) {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("integrity_test.db");
    let db = DatabaseManager::open(&db_path).expect("open test db");
    (db, dir)
}

fn make_dummy_item(id: i64, path: &str) -> MediaItem {
    MediaItem {
        id,
        root_id: 1,
        relative_path: "item".to_string(),
        canonical_path: path.to_string(),
        file_name: "item.mp3".to_string(),
        file_extension: "mp3".to_string(),
        media_type: MediaType::Audio,
        file_size: 1024,
        modified_time: 1000,
        added_time: 1000,
        last_scanned_at: 1000,
        availability_state: AvailabilityState::Available,
        metadata_status: MetadataStatus::Extracted,
    }
}

#[test]
fn test_media_reachability_classification() {
    let dir = tempdir().unwrap();
    let existing_file = dir.path().join("existing_track.mp3");
    {
        let mut f = File::create(&existing_file).unwrap();
        f.write_all(b"audio data").unwrap();
    }

    let missing_file = dir.path().join("deleted_video.mkv");

    // 1. Existing local file
    let item_avail = make_dummy_item(1, &existing_file.to_string_lossy());
    assert_eq!(
        inspect_media_reachability(&item_avail),
        MediaReachability::Available
    );

    // 2. Confirmed missing local file (parent exists, file doesn't)
    let item_missing = make_dummy_item(2, &missing_file.to_string_lossy());
    assert_eq!(
        inspect_media_reachability(&item_missing),
        MediaReachability::ConfirmedMissing
    );

    // 3. Network protocols must NEVER be marked ConfirmedMissing
    let smb_item = make_dummy_item(3, "smb://nas.local/media/movie.mp4");
    assert_eq!(
        inspect_media_reachability(&smb_item),
        MediaReachability::NetworkSourceUnavailable
    );

    let webdav_item = make_dummy_item(4, "webdav://storage.lan/albums/track.flac");
    assert_eq!(
        inspect_media_reachability(&webdav_item),
        MediaReachability::NetworkSourceUnavailable
    );

    let http_item = make_dummy_item(5, "http://192.168.1.100:8080/stream.m3u8");
    assert_eq!(
        inspect_media_reachability(&http_item),
        MediaReachability::NetworkSourceUnavailable
    );
}

#[test]
fn test_scan_library_integrity() {
    let (db, dir) = setup_test_db();

    let root_path = dir.path().join("music");
    std::fs::create_dir_all(&root_path).unwrap();

    let existing_file = root_path.join("song1.mp3");
    {
        let mut f = File::create(&existing_file).unwrap();
        f.write_all(b"music content").unwrap();
    }
    let missing_file = root_path.join("song2.mp3");

    // Seed library root
    let root_id = db
        .insert_root(&LibraryRoot {
            id: 0,
            display_name: "Music Root".to_string(),
            root_path: root_path.to_string_lossy().to_string(),
            source_type: "local".to_string(),
            is_recursive: true,
            is_enabled: true,
            exclusion_patterns: Vec::new(),
            last_scanned_at: 1000,
            scan_status: "idle".to_string(),
        })
        .expect("insert root");

    // Seed item 1 (available)
    let item1_id = db
        .upsert_media_item(&MediaItem {
            id: 0,
            root_id,
            relative_path: "song1.mp3".to_string(),
            canonical_path: existing_file.to_string_lossy().to_string(),
            file_name: "song1.mp3".to_string(),
            file_extension: "mp3".to_string(),
            media_type: MediaType::Audio,
            file_size: 100,
            modified_time: 1000,
            added_time: 1000,
            last_scanned_at: 1000,
            availability_state: AvailabilityState::Available,
            metadata_status: MetadataStatus::Extracted,
        })
        .unwrap();

    // Seed item 2 (missing)
    let item2_id = db
        .upsert_media_item(&MediaItem {
            id: 0,
            root_id,
            relative_path: "song2.mp3".to_string(),
            canonical_path: missing_file.to_string_lossy().to_string(),
            file_name: "song2.mp3".to_string(),
            file_extension: "mp3".to_string(),
            media_type: MediaType::Audio,
            file_size: 200,
            modified_time: 1000,
            added_time: 1000,
            last_scanned_at: 1000,
            availability_state: AvailabilityState::Available,
            metadata_status: MetadataStatus::Extracted,
        })
        .unwrap();

    let cancel_flag = Arc::new(AtomicBool::new(false));
    let progress = db
        .scan_library_integrity(Some(&cancel_flag))
        .expect("scan library integrity");

    assert_eq!(progress.total_examined, 2);
    assert_eq!(progress.available_count, 1);
    assert_eq!(progress.missing_count, 1);

    // Item 1 state should remain available
    let item1 = db.get_media_item_by_id(item1_id).unwrap().unwrap();
    assert_eq!(item1.availability_state, AvailabilityState::Available);

    // Item 2 should now be updated to unavailable
    let item2 = db.get_media_item_by_id(item2_id).unwrap().unwrap();
    assert_eq!(item2.availability_state, AvailabilityState::Unavailable);
}

#[test]
fn test_find_relink_candidates_and_relink_item() {
    let (db, dir) = setup_test_db();

    let old_dir = dir.path().join("old_dir");
    let new_dir = dir.path().join("new_dir");
    std::fs::create_dir_all(&old_dir).unwrap();
    std::fs::create_dir_all(&new_dir).unwrap();

    let old_path = old_dir.join("video.mp4");
    let replacement_path = new_dir.join("video.mp4");
    {
        let mut f = File::create(&replacement_path).unwrap();
        f.write_all(b"1234567890").unwrap();
    }

    let root_id = db
        .insert_root(&LibraryRoot {
            id: 0,
            display_name: "Video Root".to_string(),
            root_path: dir.path().to_string_lossy().to_string(),
            source_type: "local".to_string(),
            is_recursive: true,
            is_enabled: true,
            exclusion_patterns: Vec::new(),
            last_scanned_at: 1000,
            scan_status: "idle".to_string(),
        })
        .expect("insert root");

    let media_id = db
        .upsert_media_item(&MediaItem {
            id: 0,
            root_id,
            relative_path: "video.mp4".to_string(),
            canonical_path: old_path.to_string_lossy().to_string(),
            file_name: "video.mp4".to_string(),
            file_extension: "mp4".to_string(),
            media_type: MediaType::Video,
            file_size: 10,
            modified_time: 1000,
            added_time: 1000,
            last_scanned_at: 1000,
            availability_state: AvailabilityState::Unavailable,
            metadata_status: MetadataStatus::Extracted,
        })
        .unwrap();

    let missing_item = db.get_media_item_by_id(media_id).unwrap().unwrap();

    // 1. Candidate discovery
    let candidates = find_relink_candidates(&[dir.path().to_path_buf()], &missing_item, None, None);
    assert!(!candidates.is_empty());
    assert_eq!(candidates[0].path, replacement_path);
    assert!(candidates[0].confidence >= 0.85);

    // 2. Add watch state, favourite, collection, and bookmark to missing item
    db.set_watched(media_id, true).unwrap();
    db.set_favourite(media_id, true).unwrap();
    let coll_id = db.create_collection("My Collection", "Desc").unwrap();
    db.add_to_collection(coll_id, media_id, 0).unwrap();
    let bookmark_id = db
        .insert_bookmark(media_id, 45000, "Epic scene", None)
        .unwrap();

    // 3. Relink media item
    let updated = db.relink_media_item(media_id, &replacement_path).unwrap();

    // Validate ID is preserved
    assert_eq!(updated.id, media_id);
    assert_eq!(updated.file_name, "video.mp4");
    assert_eq!(
        updated.canonical_path,
        replacement_path.canonicalize().unwrap().to_string_lossy()
    );
    assert_eq!(updated.availability_state, AvailabilityState::Available);

    // Validate watched state, favourite, collection, and bookmark are preserved
    let is_watched = db.is_watched(media_id).unwrap();
    assert!(is_watched);

    let is_fav = db.is_favourite(media_id).unwrap();
    assert!(is_fav);

    let coll_items = db.get_collection_media_ids(coll_id).unwrap();
    assert_eq!(coll_items, vec![media_id]);

    let bookmarks = db.list_bookmarks_for_media(media_id).unwrap();
    assert_eq!(bookmarks.len(), 1);
    assert_eq!(bookmarks[0].id, bookmark_id);
    assert_eq!(bookmarks[0].timestamp_ms, 45000);
}

#[test]
fn test_catalog_cleanup_preview_and_execution() {
    let (db, dir) = setup_test_db();
    let root_id = db
        .insert_root(&LibraryRoot {
            id: 0,
            display_name: "Music Root".to_string(),
            root_path: dir.path().to_string_lossy().to_string(),
            source_type: "local".to_string(),
            is_recursive: true,
            is_enabled: true,
            exclusion_patterns: Vec::new(),
            last_scanned_at: 1000,
            scan_status: "idle".to_string(),
        })
        .expect("insert root");

    // 1. Create a confirmed missing item
    let missing_path = dir.path().join("deleted.flac");
    let missing_id = db
        .upsert_media_item(&MediaItem {
            id: 0,
            root_id,
            relative_path: "deleted.flac".to_string(),
            canonical_path: missing_path.to_string_lossy().to_string(),
            file_name: "deleted.flac".to_string(),
            file_extension: "flac".to_string(),
            media_type: MediaType::Audio,
            file_size: 500,
            modified_time: 1000,
            added_time: 1000,
            last_scanned_at: 1000,
            availability_state: AvailabilityState::Unavailable,
            metadata_status: MetadataStatus::Extracted,
        })
        .unwrap();

    // 2. Create duplicate items with exact same canonical path
    let existing_file = dir.path().join("valid.flac");
    {
        let mut f = File::create(&existing_file).unwrap();
        f.write_all(b"sound").unwrap();
    }
    let _id1 = db
        .upsert_media_item(&MediaItem {
            id: 0,
            root_id,
            relative_path: "valid.flac".to_string(),
            canonical_path: existing_file.to_string_lossy().to_string(),
            file_name: "valid.flac".to_string(),
            file_extension: "flac".to_string(),
            media_type: MediaType::Audio,
            file_size: 5,
            modified_time: 1000,
            added_time: 1000,
            last_scanned_at: 1000,
            availability_state: AvailabilityState::Available,
            metadata_status: MetadataStatus::Extracted,
        })
        .unwrap();

    // Insert duplicate record (same file_name and file_size under another path)
    let copy_file = dir.path().join("sub").join("valid.flac");
    std::fs::create_dir_all(dir.path().join("sub")).unwrap();
    {
        let mut f = File::create(&copy_file).unwrap();
        f.write_all(b"sound").unwrap();
    }
    let id2 = db
        .upsert_media_item(&MediaItem {
            id: 0,
            root_id,
            relative_path: "sub/valid.flac".to_string(),
            canonical_path: copy_file.to_string_lossy().to_string(),
            file_name: "valid.flac".to_string(),
            file_extension: "flac".to_string(),
            media_type: MediaType::Audio,
            file_size: 5,
            modified_time: 1000,
            added_time: 1000,
            last_scanned_at: 1000,
            availability_state: AvailabilityState::Available,
            metadata_status: MetadataStatus::Extracted,
        })
        .unwrap();

    // 3. Create orphaned metadata record (using a raw connection where foreign_keys = OFF)
    {
        let raw_conn = rusqlite::Connection::open(dir.path().join("integrity_test.db")).unwrap();
        raw_conn.execute("PRAGMA foreign_keys = OFF;", []).unwrap();
        raw_conn
            .execute(
                "INSERT INTO media_metadata (media_id, title, artist) VALUES (999999, 'Orphaned Title', 'Ghost Artist')",
                [],
            )
            .unwrap();
    }

    // Preview cleanup
    let preview = db.generate_cleanup_preview().unwrap();
    assert!(preview.confirmed_missing_ids.contains(&missing_id));
    assert!(preview.duplicate_ids.contains(&id2));
    assert!(preview.orphaned_metadata_ids.contains(&999999));
    assert_eq!(preview.total_cleanup_candidates(), 3);

    // Execute cleanup
    let deleted_count = db.execute_catalog_cleanup(&preview).unwrap();
    assert_eq!(deleted_count, 3);

    // Verify disk file was NOT touched
    assert!(existing_file.exists());

    // Verify database records were removed
    let item_missing = db.get_media_item_by_id(missing_id).unwrap();
    assert!(item_missing.is_none());

    let orphan_meta = db.get_metadata(999999).unwrap();
    assert!(orphan_meta.is_none());
}

#[test]
fn test_duplicate_detection_distinguishes_different_content() {
    let (db, dir) = setup_test_db();
    let root_id = db
        .insert_root(&LibraryRoot {
            id: 0,
            display_name: "Test Root".to_string(),
            root_path: dir.path().to_string_lossy().to_string(),
            source_type: "local".to_string(),
            is_recursive: true,
            is_enabled: true,
            exclusion_patterns: Vec::new(),
            last_scanned_at: 1000,
            scan_status: "idle".to_string(),
        })
        .expect("insert root");

    // File A: 10 bytes, content "Track AAAA"
    let file_a = dir.path().join("album1").join("Track01.flac");
    std::fs::create_dir_all(dir.path().join("album1")).unwrap();
    {
        let mut f = File::create(&file_a).unwrap();
        f.write_all(b"Track AAAA").unwrap();
    }
    let id_a = db
        .upsert_media_item(&MediaItem {
            id: 0,
            root_id,
            relative_path: "album1/Track01.flac".to_string(),
            canonical_path: file_a.to_string_lossy().to_string(),
            file_name: "Track01.flac".to_string(),
            file_extension: "flac".to_string(),
            media_type: MediaType::Audio,
            file_size: 10,
            modified_time: 1000,
            added_time: 1000,
            last_scanned_at: 1000,
            availability_state: AvailabilityState::Available,
            metadata_status: MetadataStatus::Extracted,
        })
        .unwrap();

    // File B: 10 bytes, SAME FILENAME Track01.flac, SAME SIZE 10, but DIFFERENT CONTENT "Track BBBB"!
    let file_b = dir.path().join("album2").join("Track01.flac");
    std::fs::create_dir_all(dir.path().join("album2")).unwrap();
    {
        let mut f = File::create(&file_b).unwrap();
        f.write_all(b"Track BBBB").unwrap();
    }
    let id_b = db
        .upsert_media_item(&MediaItem {
            id: 0,
            root_id,
            relative_path: "album2/Track01.flac".to_string(),
            canonical_path: file_b.to_string_lossy().to_string(),
            file_name: "Track01.flac".to_string(),
            file_extension: "flac".to_string(),
            media_type: MediaType::Audio,
            file_size: 10,
            modified_time: 1000,
            added_time: 1000,
            last_scanned_at: 1000,
            availability_state: AvailabilityState::Available,
            metadata_status: MetadataStatus::Extracted,
        })
        .unwrap();

    // 1. Preview cleanup: File B must NOT be detected as a duplicate of File A because their content hashes differ!
    let preview1 = db.generate_cleanup_preview().unwrap();
    assert!(
        !preview1.duplicate_ids.contains(&id_b),
        "Files with distinct content must never be treated as duplicates even with identical name and size!"
    );
    assert_eq!(preview1.duplicate_ids.len(), 0);

    // File C: 10 bytes, content "Track AAAA" (IDENTICAL content and hash to File A)
    let file_c = dir.path().join("backup").join("Track01.flac");
    std::fs::create_dir_all(dir.path().join("backup")).unwrap();
    {
        let mut f = File::create(&file_c).unwrap();
        f.write_all(b"Track AAAA").unwrap();
    }
    let id_c = db
        .upsert_media_item(&MediaItem {
            id: 0,
            root_id,
            relative_path: "backup/Track01.flac".to_string(),
            canonical_path: file_c.to_string_lossy().to_string(),
            file_name: "Track01.flac".to_string(),
            file_extension: "flac".to_string(),
            media_type: MediaType::Audio,
            file_size: 10,
            modified_time: 1000,
            added_time: 1000,
            last_scanned_at: 1000,
            availability_state: AvailabilityState::Available,
            metadata_status: MetadataStatus::Extracted,
        })
        .unwrap();

    // Attach bookmark and favourite to duplicate id_c
    let bm_id = db
        .insert_bookmark(id_c, 5000, "Ref Hook", Some("Important intro"))
        .unwrap();
    db.set_favourite(id_c, true).unwrap();

    // 2. Preview cleanup: File C must be identified as duplicate (matching File A), while File B is STILL safe!
    let preview2 = db.generate_cleanup_preview().unwrap();
    assert!(preview2.duplicate_ids.contains(&id_c));
    assert!(
        !preview2.duplicate_ids.contains(&id_b),
        "File B must remain untouched"
    );

    // 3. Execute cleanup: id_c is pruned, its bookmark and favourite are safely migrated to id_a
    let pruned = db.execute_catalog_cleanup(&preview2).unwrap();
    assert_eq!(pruned, 1);

    // File A remains and now owns the migrated bookmark and favourite
    assert!(db.is_favourite(id_a).unwrap());
    let bookmarks_a = db.list_bookmarks_for_media(id_a).unwrap();
    assert_eq!(bookmarks_a.len(), 1);
    assert_eq!(bookmarks_a[0].id, bm_id);

    // File B remains completely intact and unaffected
    assert!(db.get_media_item_by_id(id_b).unwrap().is_some());
    assert!(file_b.exists());
    assert!(file_a.exists());
    assert!(file_c.exists()); // Physical disk files are never deleted!
}

#[test]
fn test_duplicate_detection_rejects_identical_samples_with_divergent_unsampled_bytes() {
    let (db, dir) = setup_test_db();
    let root_id = db
        .insert_root(&LibraryRoot {
            id: 0,
            display_name: "Test Root".to_string(),
            root_path: dir.path().to_string_lossy().to_string(),
            source_type: "local".to_string(),
            is_recursive: true,
            is_enabled: true,
            exclusion_patterns: Vec::new(),
            last_scanned_at: 1000,
            scan_status: "idle".to_string(),
        })
        .expect("insert root");

    // File size > 6 MB triggers piecewise sampling: 7 MB = 7,340,032 bytes
    const FILE_SIZE: usize = 7 * 1024 * 1024;
    let buffer_a = vec![0x55u8; FILE_SIZE];
    let mut buffer_b = vec![0x55u8; FILE_SIZE];

    // In piecewise sampling:
    // Head: 0 .. 2,097,152 (2 MB)
    // Mid: centered around 3.5 MB: 2,621,440 .. 4,718,592
    // Tail: 5,242,880 .. 7,340,032
    // Unsampled Gap 1: 2,097,152 .. 2,621,440 (e.g. byte 2,300,000)
    buffer_b[2_300_000] = 0x99;

    let file_a_path = dir.path().join("dir_a").join("SampleMovie.mkv");
    let file_b_path = dir.path().join("dir_b").join("SampleMovie.mkv");
    std::fs::create_dir_all(dir.path().join("dir_a")).unwrap();
    std::fs::create_dir_all(dir.path().join("dir_b")).unwrap();

    std::fs::write(&file_a_path, &buffer_a).unwrap();
    std::fs::write(&file_b_path, &buffer_b).unwrap();

    // 1. Verify that piecewise hashes match because sampled regions (head, mid, tail) are identical
    let pw_a = compute_piecewise_hash(&file_a_path).expect("piecewise hash A");
    let pw_b = compute_piecewise_hash(&file_b_path).expect("piecewise hash B");
    assert_eq!(
        pw_a, pw_b,
        "Piecewise hashes MUST match because sampled head, mid, and tail chunks are identical"
    );

    // 2. Verify that authoritative streaming full-file SHA-256 hashes differ
    let full_a = compute_full_file_hash(&file_a_path).expect("full hash A");
    let full_b = compute_full_file_hash(&file_b_path).expect("full hash B");
    assert_ne!(
        full_a, full_b,
        "Full-file streaming hashes MUST differ because unsampled region at byte 2,300,000 differs"
    );

    // 3. Register items A and B into database
    let id_a = db
        .upsert_media_item(&MediaItem {
            id: 0,
            root_id,
            relative_path: "dir_a/SampleMovie.mkv".to_string(),
            canonical_path: file_a_path.to_string_lossy().to_string(),
            file_name: "SampleMovie.mkv".to_string(),
            file_extension: "mkv".to_string(),
            media_type: MediaType::Video,
            file_size: FILE_SIZE as u64,
            modified_time: 1000,
            added_time: 1000,
            last_scanned_at: 1000,
            availability_state: AvailabilityState::Available,
            metadata_status: MetadataStatus::Extracted,
        })
        .unwrap();

    let id_b = db
        .upsert_media_item(&MediaItem {
            id: 0,
            root_id,
            relative_path: "dir_b/SampleMovie.mkv".to_string(),
            canonical_path: file_b_path.to_string_lossy().to_string(),
            file_name: "SampleMovie.mkv".to_string(),
            file_extension: "mkv".to_string(),
            media_type: MediaType::Video,
            file_size: FILE_SIZE as u64,
            modified_time: 1000,
            added_time: 1000,
            last_scanned_at: 1000,
            availability_state: AvailabilityState::Available,
            metadata_status: MetadataStatus::Extracted,
        })
        .unwrap();

    // 4. Verify preview does NOT classify file B as duplicate despite identical piecewise sample
    let preview1 = db.generate_cleanup_preview().unwrap();
    assert!(
        !preview1.duplicate_ids.contains(&id_b),
        "File B must NEVER be classified as duplicate because full streaming SHA-256 differed!"
    );
    assert_eq!(preview1.duplicate_ids.len(), 0);

    // 5. Create File C: 100% identical to File A across all bytes
    let file_c_path = dir.path().join("dir_c").join("SampleMovie.mkv");
    std::fs::create_dir_all(dir.path().join("dir_c")).unwrap();
    std::fs::write(&file_c_path, &buffer_a).unwrap();

    let id_c = db
        .upsert_media_item(&MediaItem {
            id: 0,
            root_id,
            relative_path: "dir_c/SampleMovie.mkv".to_string(),
            canonical_path: file_c_path.to_string_lossy().to_string(),
            file_name: "SampleMovie.mkv".to_string(),
            file_extension: "mkv".to_string(),
            media_type: MediaType::Video,
            file_size: FILE_SIZE as u64,
            modified_time: 1000,
            added_time: 1000,
            last_scanned_at: 1000,
            availability_state: AvailabilityState::Available,
            metadata_status: MetadataStatus::Extracted,
        })
        .unwrap();

    // Attach bookmark, favourite, watch state, and collection to id_c
    let bm_c = db
        .insert_bookmark(id_c, 120_000, "Scene 4", Some("Great VFX"))
        .unwrap();
    db.set_favourite(id_c, true).unwrap();
    db.set_watched(id_c, true).unwrap();
    let coll_id = db.create_collection("Sci-Fi Vault", "Best Sci-Fi").unwrap();
    db.add_to_collection(coll_id, id_c, 0).unwrap();

    // 6. Register uncertain/offline candidate File D: path does not exist on disk
    let file_d_path = dir.path().join("dir_d").join("SampleMovie.mkv");
    let id_d = db
        .upsert_media_item(&MediaItem {
            id: 0,
            root_id,
            relative_path: "dir_d/SampleMovie.mkv".to_string(),
            canonical_path: file_d_path.to_string_lossy().to_string(),
            file_name: "SampleMovie.mkv".to_string(),
            file_extension: "mkv".to_string(),
            media_type: MediaType::Video,
            file_size: FILE_SIZE as u64,
            modified_time: 1000,
            added_time: 1000,
            last_scanned_at: 1000,
            availability_state: AvailabilityState::Available,
            metadata_status: MetadataStatus::Extracted,
        })
        .unwrap();

    // 7. Preview cleanup
    let preview2 = db.generate_cleanup_preview().unwrap();

    // File C is confirmed duplicate of File A
    assert!(preview2.duplicate_ids.contains(&id_c));
    // File B is NOT a duplicate (distinct content)
    assert!(!preview2.duplicate_ids.contains(&id_b));
    // File D (offline) is quarantined in suspected_duplicates for manual review, NEVER auto-purged
    assert!(!preview2.duplicate_ids.contains(&id_d));
    let has_suspected_d = preview2
        .suspected_duplicates
        .iter()
        .any(|s| s.duplicate_id == id_d && !s.is_confirmed_hash_match);
    assert!(
        has_suspected_d,
        "Unreachable candidate D must be kept in suspected_duplicates for manual review"
    );

    // 8. Execute cleanup
    let cleaned = db.execute_catalog_cleanup(&preview2).unwrap();
    assert_eq!(cleaned, 1); // Only id_c is pruned

    // Verify transactional preservation on id_a:
    // - Bookmarks preserved: id_a now has bookmark migrated from id_c
    let bookmarks_a = db.list_bookmarks_for_media(id_a).unwrap();
    assert_eq!(bookmarks_a.len(), 1);
    assert_eq!(bookmarks_a[0].id, bm_c);
    assert_eq!(bookmarks_a[0].timestamp_ms, 120_000);

    // - Favourite preserved
    assert!(db.is_favourite(id_a).unwrap());

    // - Watch state / history preserved
    assert!(db.is_watched(id_a).unwrap());

    // - Collection association preserved
    let coll_items = db.get_collection_media_ids(coll_id).unwrap();
    assert_eq!(coll_items, vec![id_a]);

    // - File B and File D remain in catalog
    assert!(db.get_media_item_by_id(id_b).unwrap().is_some());
    assert!(db.get_media_item_by_id(id_d).unwrap().is_some());

    // - Physical media files on disk were never deleted
    assert!(file_a_path.exists());
    assert!(file_b_path.exists());
    assert!(file_c_path.exists());
}
