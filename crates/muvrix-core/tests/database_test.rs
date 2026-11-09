//! Integration tests for the SQLite Media Library foundation in muvrix-core.

use muvrix_core::database::*;

#[test]
fn test_database_initialization_and_schema_version() {
    let db = DatabaseManager::open_in_memory().expect("In-memory DB should open successfully");
    assert!(db.check_integrity().expect("Integrity check should pass"));
}

#[test]
fn test_ssh_host_trust_crud_and_port_isolation() {
    let db = DatabaseManager::open_in_memory().expect("In-memory DB open");

    // 1. Initially no trust records
    assert!(db
        .get_ssh_host_trust("sftp.example.com", 22)
        .unwrap()
        .is_none());

    // 2. Record trust for host on port 22
    let record1 = db
        .record_ssh_host_trust(
            "sftp.example.com",
            22,
            "ssh-ed25519",
            "SHA256:abcd1234efgh5678ijkl9012mnop3456qrst7890uvw",
            Some("Production SFTP server"),
        )
        .expect("Record host trust");
    assert_eq!(record1.host, "sftp.example.com");
    assert_eq!(record1.port, 22);
    assert_eq!(record1.key_algorithm, "ssh-ed25519");

    // 3. Normalized case lookup (SFTP.EXAMPLE.COM should match sftp.example.com)
    let fetched = db
        .get_ssh_host_trust("SFTP.EXAMPLE.COM", 22)
        .expect("Get trust")
        .expect("Record should exist");
    assert_eq!(fetched.fingerprint_sha256, record1.fingerprint_sha256);

    // 4. Port isolation: same hostname on port 2222 should NOT match port 22
    assert!(db
        .get_ssh_host_trust("sftp.example.com", 2222)
        .unwrap()
        .is_none());

    // 5. Record trust for same host on port 2222 with different key
    let _record2 = db
        .record_ssh_host_trust(
            "sftp.example.com",
            2222,
            "ssh-ed25519",
            "SHA256:DIFF9999KEY000000000000000000000000000000",
            Some("Staging SFTP server"),
        )
        .expect("Record host trust for port 2222");

    let list = db.list_ssh_host_trust().expect("List host trust");
    assert_eq!(list.len(), 2);

    // 6. Update last seen timestamp
    let updated = db
        .update_ssh_host_last_seen("sftp.example.com", 22)
        .expect("Update last seen");
    assert!(updated);

    // 7. Revoke trust
    let revoked = db
        .revoke_ssh_host_trust("sftp.example.com", 22)
        .expect("Revoke trust");
    assert!(revoked);
    assert!(db
        .get_ssh_host_trust("sftp.example.com", 22)
        .unwrap()
        .is_none());

    // Port 2222 should still be trusted
    assert!(db
        .get_ssh_host_trust("sftp.example.com", 2222)
        .unwrap()
        .is_some());
}

#[test]
fn test_library_root_crud() {
    let db = DatabaseManager::open_in_memory().expect("DB open");
    let root = LibraryRoot {
        id: 0,
        display_name: "Movies".to_string(),
        root_path: "/media/movies".to_string(),
        source_type: "local".to_string(),
        is_recursive: true,
        is_enabled: true,
        exclusion_patterns: vec!["*.tmp".to_string(), "sample/*".to_string()],
        last_scanned_at: 1700000000,
        scan_status: "idle".to_string(),
    };

    let id = db.insert_root(&root).expect("Insert root");
    assert!(id > 0);

    let retrieved = db
        .get_root_by_id(id)
        .expect("Get root")
        .expect("Found root");
    assert_eq!(retrieved.display_name, "Movies");
    assert_eq!(retrieved.root_path, "/media/movies");
    assert_eq!(retrieved.exclusion_patterns.len(), 2);
    assert!(retrieved.is_recursive);

    // Update status
    db.update_root_status(id, "scanning", 1700000050)
        .expect("Update status");
    let updated = db.get_root_by_id(id).expect("Get root").unwrap();
    assert_eq!(updated.scan_status, "scanning");
    assert_eq!(updated.last_scanned_at, 1700000050);

    // Delete
    db.delete_root(id).expect("Delete root");
    assert!(db.get_root_by_id(id).expect("Get").is_none());
}

#[test]
fn test_media_item_upsert_and_availability() {
    let db = DatabaseManager::open_in_memory().expect("DB open");
    let root = LibraryRoot {
        id: 0,
        display_name: "Videos".to_string(),
        root_path: "/media/videos".to_string(),
        source_type: "local".to_string(),
        is_recursive: true,
        is_enabled: true,
        exclusion_patterns: vec![],
        last_scanned_at: 0,
        scan_status: "idle".to_string(),
    };
    let root_id = db.insert_root(&root).unwrap();

    let item = MediaItem {
        id: 0,
        root_id,
        relative_path: "subfolder/movie.mp4".to_string(),
        canonical_path: "/media/videos/subfolder/movie.mp4".to_string(),
        file_name: "movie.mp4".to_string(),
        file_extension: "mp4".to_string(),
        media_type: MediaType::Video,
        file_size: 1024 * 1024 * 500, // 500 MB
        modified_time: 1700000000,
        added_time: 1700000000,
        last_scanned_at: 1700000000,
        availability_state: AvailabilityState::Available,
        metadata_status: MetadataStatus::Pending,
    };

    let item_id = db.upsert_media_item(&item).expect("Upsert item");
    assert!(item_id > 0);

    let retrieved = db.get_media_item_by_id(item_id).expect("Get").unwrap();
    assert_eq!(retrieved.file_name, "movie.mp4");
    assert_eq!(retrieved.availability_state, AvailabilityState::Available);

    // Update availability
    db.update_availability(item_id, AvailabilityState::Unavailable)
        .expect("Update avail");
    let updated = db.get_media_item_by_id(item_id).expect("Get").unwrap();
    assert_eq!(updated.availability_state, AvailabilityState::Unavailable);

    // Test upsert on conflict: same canonical path with modified timestamp sets status to pending
    let mut item_mod = item.clone();
    item_mod.modified_time = 1700000100;
    let item_id2 = db.upsert_media_item(&item_mod).expect("Re-upsert");
    assert_eq!(item_id, item_id2);
    let re_retrieved = db.get_media_item_by_id(item_id).expect("Get").unwrap();
    assert_eq!(re_retrieved.modified_time, 1700000100);
}

#[test]
fn test_metadata_and_tracks() {
    let db = DatabaseManager::open_in_memory().expect("DB open");
    let root_id = db
        .insert_root(&LibraryRoot {
            id: 0,
            display_name: "Videos".to_string(),
            root_path: "/media".to_string(),
            source_type: "local".to_string(),
            is_recursive: true,
            is_enabled: true,
            exclusion_patterns: vec![],
            last_scanned_at: 0,
            scan_status: "idle".to_string(),
        })
        .unwrap();

    let item_id = db
        .upsert_media_item(&MediaItem {
            id: 0,
            root_id,
            relative_path: "film.mkv".to_string(),
            canonical_path: "/media/film.mkv".to_string(),
            file_name: "film.mkv".to_string(),
            file_extension: "mkv".to_string(),
            media_type: MediaType::Video,
            file_size: 1000,
            modified_time: 100,
            added_time: 100,
            last_scanned_at: 100,
            availability_state: AvailabilityState::Available,
            metadata_status: MetadataStatus::Pending,
        })
        .unwrap();

    let meta = MediaMetadata {
        media_id: item_id,
        title: Some("Great Film".to_string()),
        original_title: None,
        description: Some("Epic cinema".to_string()),
        genre: Some("Sci-Fi".to_string()),
        release_year: Some(2025),
        duration_ms: 120 * 60 * 1000,
        overall_bitrate: 5_000_000,
        container: Some("matroska".to_string()),
        video_codec: Some("h264".to_string()),
        audio_codec: Some("aac".to_string()),
        width: 1920,
        height: 1080,
        fps: 24.0,
        hdr_type: None,
        audio_channels: 6,
        sample_rate: 48000,
        bit_depth: 16,
        artist: None,
        album: None,
        album_artist: None,
        track_number: None,
        disc_number: None,
    };

    db.upsert_metadata(&meta).expect("Upsert metadata");

    let saved_meta = db.get_metadata(item_id).expect("Get meta").unwrap();
    assert_eq!(saved_meta.title.as_deref(), Some("Great Film"));
    assert_eq!(saved_meta.width, 1920);
    assert_eq!(saved_meta.height, 1080);

    // Verify parent media item metadata_status transitioned to extracted
    let parent = db.get_media_item_by_id(item_id).expect("Get item").unwrap();
    assert_eq!(parent.metadata_status, MetadataStatus::Extracted);

    // Tracks
    let tracks = vec![
        MediaTrack {
            id: 0,
            media_id: item_id,
            track_index: 0,
            track_type: TrackType::Video,
            title: Some("Main Video".to_string()),
            language: Some("eng".to_string()),
            codec: Some("h264".to_string()),
            channel_layout: None,
            is_default: true,
            is_selected: true,
        },
        MediaTrack {
            id: 0,
            media_id: item_id,
            track_index: 1,
            track_type: TrackType::Audio,
            title: Some("Surround 5.1".to_string()),
            language: Some("eng".to_string()),
            codec: Some("aac".to_string()),
            channel_layout: Some("5.1".to_string()),
            is_default: true,
            is_selected: true,
        },
    ];

    db.replace_tracks(item_id, &tracks).expect("Replace tracks");
    let saved_tracks = db.get_tracks(item_id).expect("Get tracks");
    assert_eq!(saved_tracks.len(), 2);
    assert_eq!(saved_tracks[1].codec.as_deref(), Some("aac"));
}

#[test]
fn test_collections_and_favourites() {
    let db = DatabaseManager::open_in_memory().expect("DB open");
    let root_id = db
        .insert_root(&LibraryRoot {
            id: 0,
            display_name: "Root".to_string(),
            root_path: "/root".to_string(),
            source_type: "local".to_string(),
            is_recursive: false,
            is_enabled: true,
            exclusion_patterns: vec![],
            last_scanned_at: 0,
            scan_status: "idle".to_string(),
        })
        .unwrap();

    let m1 = db
        .upsert_media_item(&MediaItem {
            id: 0,
            root_id,
            relative_path: "1.mp4".to_string(),
            canonical_path: "/root/1.mp4".to_string(),
            file_name: "1.mp4".to_string(),
            file_extension: "mp4".to_string(),
            media_type: MediaType::Video,
            file_size: 10,
            modified_time: 10,
            added_time: 10,
            last_scanned_at: 10,
            availability_state: AvailabilityState::Available,
            metadata_status: MetadataStatus::Pending,
        })
        .unwrap();

    // Favourite test
    assert!(!db.is_favourite(m1).unwrap());
    db.set_favourite(m1, true).unwrap();
    assert!(db.is_favourite(m1).unwrap());
    db.set_favourite(m1, false).unwrap();
    assert!(!db.is_favourite(m1).unwrap());

    // Collection test
    let col_id = db
        .create_collection("Watch Later", "Movies to watch")
        .unwrap();
    assert!(col_id > 0);
    db.add_to_collection(col_id, m1, 1).unwrap();

    let col_items = db.get_collection_media_ids(col_id).unwrap();
    assert_eq!(col_items, vec![m1]);

    db.remove_from_collection(col_id, m1).unwrap();
    assert!(db.get_collection_media_ids(col_id).unwrap().is_empty());
}

#[test]
fn test_query_filtering_sorting_and_pagination() {
    let db = DatabaseManager::open_in_memory().expect("DB open");
    let root_id = db
        .insert_root(&LibraryRoot {
            id: 0,
            display_name: "All".to_string(),
            root_path: "/all".to_string(),
            source_type: "local".to_string(),
            is_recursive: true,
            is_enabled: true,
            exclusion_patterns: vec![],
            last_scanned_at: 0,
            scan_status: "idle".to_string(),
        })
        .unwrap();

    // Insert 5 items
    for i in 1..=5 {
        let item_id = db
            .upsert_media_item(&MediaItem {
                id: 0,
                root_id,
                relative_path: format!("video_{}.mp4", i),
                canonical_path: format!("/all/video_{}.mp4", i),
                file_name: format!("video_{}.mp4", i),
                file_extension: "mp4".to_string(),
                media_type: MediaType::Video,
                file_size: i * 1000,
                modified_time: i * 100,
                added_time: i * 100,
                last_scanned_at: 100,
                availability_state: AvailabilityState::Available,
                metadata_status: MetadataStatus::Pending,
            })
            .unwrap();

        db.upsert_metadata(&MediaMetadata {
            media_id: item_id,
            title: Some(format!("Movie Title {}", i)),
            original_title: None,
            description: None,
            genre: if i % 2 == 0 {
                Some("Action".to_string())
            } else {
                Some("Comedy".to_string())
            },
            release_year: Some(2020 + i as i32),
            duration_ms: i * 60 * 1000,
            overall_bitrate: 1_000_000,
            container: Some("mp4".to_string()),
            video_codec: Some("h264".to_string()),
            audio_codec: Some("aac".to_string()),
            width: if i == 5 { 3840 } else { 1920 },
            height: if i == 5 { 2160 } else { 1080 },
            fps: 30.0,
            hdr_type: None,
            audio_channels: 2,
            sample_rate: 44100,
            bit_depth: 16,
            artist: None,
            album: None,
            album_artist: None,
            track_number: None,
            disc_number: None,
        })
        .unwrap();

        if i % 2 == 1 {
            db.set_favourite(item_id, true).unwrap();
        }
    }

    // 1. Query all
    let (items, total) = db
        .query_library(
            &LibraryFilter::default(),
            LibrarySortField::Title,
            SortDirection::Ascending,
            Pagination {
                offset: 0,
                limit: 10,
            },
        )
        .unwrap();
    assert_eq!(total, 5);
    assert_eq!(items.len(), 5);

    // 2. Query search
    let (items, total) = db
        .query_library(
            &LibraryFilter {
                search_query: Some("Title 3".to_string()),
                ..Default::default()
            },
            LibrarySortField::Title,
            SortDirection::Ascending,
            Pagination::default(),
        )
        .unwrap();
    assert_eq!(total, 1);
    assert_eq!(items[0].item.file_name, "video_3.mp4");

    // 3. Query 4K tier
    let (items, total) = db
        .query_library(
            &LibraryFilter {
                resolution_tier: Some("4K".to_string()),
                ..Default::default()
            },
            LibrarySortField::Title,
            SortDirection::Ascending,
            Pagination::default(),
        )
        .unwrap();
    assert_eq!(total, 1);
    assert_eq!(items[0].item.file_name, "video_5.mp4");

    // 4. Query favourites
    let (items, total) = db
        .query_library(
            &LibraryFilter {
                favourites_only: true,
                ..Default::default()
            },
            LibrarySortField::Title,
            SortDirection::Ascending,
            Pagination::default(),
        )
        .unwrap();
    assert_eq!(total, 3); // 1, 3, 5
    for item in items {
        assert!(item.is_favourite);
    }

    // 5. Pagination: offset 2, limit 2
    let (items, total) = db
        .query_library(
            &LibraryFilter::default(),
            LibrarySortField::FileSize,
            SortDirection::Ascending,
            Pagination {
                offset: 2,
                limit: 2,
            },
        )
        .unwrap();
    assert_eq!(total, 5);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].item.file_size, 3000);
    assert_eq!(items[1].item.file_size, 4000);
}

#[test]
fn test_database_disk_reopen_wal_mode_and_foreign_keys_cascade() {
    let temp_dir = tempfile::tempdir().expect("Create temp dir");
    let db_path = temp_dir.path().join("test_reopen.db");

    // Scope 1: Create, configure, populate
    {
        let db = DatabaseManager::open(&db_path).expect("Open disk DB");
        assert!(db.check_integrity().expect("Integrity check"));

        // Insert Root
        let root = LibraryRoot {
            id: 0,
            display_name: "Disk Root".to_string(),
            root_path: "/storage/videos".to_string(),
            source_type: "local".to_string(),
            is_recursive: true,
            is_enabled: true,
            exclusion_patterns: vec![],
            last_scanned_at: 1000,
            scan_status: "idle".to_string(),
        };
        let root_id = db.insert_root(&root).expect("Insert root");

        // Insert MediaItem
        let item = MediaItem {
            id: 0,
            root_id,
            relative_path: "clip.mp4".to_string(),
            canonical_path: "/storage/videos/clip.mp4".to_string(),
            file_name: "clip.mp4".to_string(),
            file_extension: "mp4".to_string(),
            media_type: MediaType::Video,
            file_size: 1024 * 1024 * 50,
            modified_time: 1700000000,
            added_time: 1700000000,
            last_scanned_at: 1700000000,
            availability_state: AvailabilityState::Available,
            metadata_status: MetadataStatus::Extracted,
        };
        let media_id = db.upsert_media_item(&item).expect("Upsert item");

        // Insert Metadata
        let meta = MediaMetadata {
            media_id,
            title: Some("Persisted Movie".to_string()),
            original_title: None,
            description: Some("A persistent disk test.".to_string()),
            genre: Some("SciFi".to_string()),
            release_year: Some(2026),
            duration_ms: 120000,
            overall_bitrate: 5000000,
            container: Some("mp4".to_string()),
            video_codec: Some("h264".to_string()),
            audio_codec: Some("aac".to_string()),
            width: 1920,
            height: 1080,
            fps: 60.0,
            hdr_type: None,
            audio_channels: 2,
            sample_rate: 48000,
            bit_depth: 16,
            artist: None,
            album: None,
            album_artist: None,
            track_number: None,
            disc_number: None,
        };
        db.upsert_metadata(&meta).expect("Upsert metadata");

        // Insert Artwork
        let artwork = ArtworkRecord {
            id: 0,
            media_id,
            cache_key: "disk_thumb_1".to_string(),
            file_path: "/tmp/disk_thumb.png".to_string(),
            source_type: ArtworkSource::FrameCapture,
            width: 320,
            height: 180,
            generated_at: 1700000000,
        };
        db.upsert_artwork(&artwork).expect("Upsert artwork");
        db.set_favourite(media_id, true).expect("Set fav");
        db.add_tag_to_media(media_id, "Featured").expect("Add tag");
    }

    // Scope 2: Reopen from disk and verify persistence + WAL
    {
        let db = DatabaseManager::open(&db_path).expect("Reopen disk DB");
        assert!(db.check_integrity().expect("Integrity check on reopen"));

        let roots = db.get_roots().expect("Get roots");
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].display_name, "Disk Root");

        let (items, count) = db
            .query_library(
                &LibraryFilter::default(),
                LibrarySortField::DateAdded,
                SortDirection::Descending,
                Pagination::default(),
            )
            .expect("Query library");
        assert_eq!(count, 1);
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].metadata.as_ref().and_then(|m| m.title.as_deref()),
            Some("Persisted Movie")
        );
        assert!(items[0].is_favourite);
        assert_eq!(
            items[0].thumbnail_path.as_deref(),
            Some("/tmp/disk_thumb.png")
        );

        // Delete Root and verify CASCADE to media_items, metadata, artwork, favourites
        db.delete_root(roots[0].id).expect("Delete root");

        let (items_after, count_after) = db
            .query_library(
                &LibraryFilter::default(),
                LibrarySortField::DateAdded,
                SortDirection::Descending,
                Pagination::default(),
            )
            .expect("Query library after root delete");
        assert_eq!(count_after, 0);
        assert!(items_after.is_empty());
    }
}

#[test]
fn test_transaction_rollback_safety() {
    let db = DatabaseManager::open_in_memory().expect("Open in-memory DB");

    let root = LibraryRoot {
        id: 0,
        display_name: "Rollback Test Root".to_string(),
        root_path: "/test/rollback".to_string(),
        source_type: "local".to_string(),
        is_recursive: true,
        is_enabled: true,
        exclusion_patterns: vec![],
        last_scanned_at: 0,
        scan_status: "idle".to_string(),
    };
    let root_id = db.insert_root(&root).expect("Insert root");

    let item_before = MediaItem {
        id: 0,
        root_id,
        relative_path: "item1.mp4".to_string(),
        canonical_path: "/test/rollback/item1.mp4".to_string(),
        file_name: "item1.mp4".to_string(),
        file_extension: "mp4".to_string(),
        media_type: MediaType::Video,
        file_size: 1000,
        modified_time: 100,
        added_time: 100,
        last_scanned_at: 100,
        availability_state: AvailabilityState::Available,
        metadata_status: MetadataStatus::Extracted,
    };
    db.upsert_media_item(&item_before).expect("Insert item 1");

    let (_, count_before) = db
        .query_library(
            &LibraryFilter::default(),
            LibrarySortField::DateAdded,
            SortDirection::Descending,
            Pagination::default(),
        )
        .expect("Initial count");
    assert_eq!(count_before, 1);

    // Attempt transactional insertion that fails midway
    let tx_res: Result<(), DatabaseError> = db.with_transaction(|tx| {
        let item_temp = MediaItem {
            id: 0,
            root_id,
            relative_path: "item2.mp4".to_string(),
            canonical_path: "/test/rollback/item2.mp4".to_string(),
            file_name: "item2.mp4".to_string(),
            file_extension: "mp4".to_string(),
            media_type: MediaType::Video,
            file_size: 2000,
            modified_time: 200,
            added_time: 200,
            last_scanned_at: 200,
            availability_state: AvailabilityState::Available,
            metadata_status: MetadataStatus::Extracted,
        };
        Repository::upsert_media_item(tx, &item_temp).map_err(DatabaseError::Sqlite)?;

        // Intentionally abort transaction with an error
        Err(DatabaseError::Corruption(
            "Simulated failure to trigger rollback".to_string(),
        ))
    });

    assert!(tx_res.is_err());

    // Verify item2 was rolled back and did not persist
    let (items_after, count_after) = db
        .query_library(
            &LibraryFilter::default(),
            LibrarySortField::DateAdded,
            SortDirection::Descending,
            Pagination::default(),
        )
        .expect("Count after rollback");
    assert_eq!(count_after, 1);
    assert_eq!(items_after.len(), 1);
    assert_eq!(items_after[0].item.file_name, "item1.mp4");
}

#[test]
fn test_user_bookmarks_crud_and_foreign_key_cascade() {
    let db = DatabaseManager::open_in_memory().expect("In-memory DB");

    // 1. Insert root and media item
    let root = LibraryRoot {
        id: 0,
        display_name: "BookmarksTestRoot".to_string(),
        root_path: "/test/bookmarks".to_string(),
        source_type: "local".to_string(),
        is_recursive: false,
        is_enabled: true,
        exclusion_patterns: vec![],
        last_scanned_at: 0,
        scan_status: "idle".to_string(),
    };
    let root_id = db.insert_root(&root).expect("Insert root");

    let item = MediaItem {
        id: 0,
        root_id,
        relative_path: "lecture.mp4".to_string(),
        canonical_path: "/test/bookmarks/lecture.mp4".to_string(),
        file_name: "lecture.mp4".to_string(),
        file_extension: "mp4".to_string(),
        media_type: MediaType::Video,
        file_size: 50_000_000,
        modified_time: 1700000000,
        added_time: 1700000000,
        last_scanned_at: 1700000000,
        availability_state: AvailabilityState::Available,
        metadata_status: MetadataStatus::Extracted,
    };
    let media_id = db.upsert_media_item(&item).expect("Insert media item");

    // 2. Insert bookmarks out of order
    let b2_id = db
        .insert_bookmark(
            media_id,
            120_000,
            "Conclusion & Summary",
            Some("Important exam review topic"),
        )
        .expect("Insert bookmark 2");

    let b1_id = db
        .insert_bookmark(media_id, 15_000, "Introduction to Neural Nets", None)
        .expect("Insert bookmark 1");

    let b3_id = db
        .insert_bookmark(
            media_id,
            65_000,
            "Gradient Descent Algorithm",
            Some("Step-by-step formula derivation"),
        )
        .expect("Insert bookmark 3");

    // 3. List bookmarks: must be sorted by timestamp_ms ASC
    let bookmarks = db
        .list_bookmarks_for_media(media_id)
        .expect("List bookmarks");

    assert_eq!(bookmarks.len(), 3);
    assert_eq!(bookmarks[0].id, b1_id);
    assert_eq!(bookmarks[0].timestamp_ms, 15_000);
    assert_eq!(bookmarks[0].label, "Introduction to Neural Nets");

    assert_eq!(bookmarks[1].id, b3_id);
    assert_eq!(bookmarks[1].timestamp_ms, 65_000);
    assert_eq!(bookmarks[1].label, "Gradient Descent Algorithm");

    assert_eq!(bookmarks[2].id, b2_id);
    assert_eq!(bookmarks[2].timestamp_ms, 120_000);
    assert_eq!(bookmarks[2].label, "Conclusion & Summary");

    // 4. Update bookmark 2
    let updated = db
        .update_bookmark(
            b2_id,
            "Final Conclusion (Revised)",
            Some("Updated homework deadlines"),
        )
        .expect("Update bookmark");
    assert!(updated);

    let retrieved = db
        .get_bookmark(b2_id)
        .expect("Get bookmark")
        .expect("Found bookmark");
    assert_eq!(retrieved.label, "Final Conclusion (Revised)");
    assert_eq!(
        retrieved.note.as_deref(),
        Some("Updated homework deadlines")
    );

    // 5. Delete single bookmark
    let deleted = db.delete_bookmark(b1_id).expect("Delete bookmark");
    assert!(deleted);

    let remaining = db
        .list_bookmarks_for_media(media_id)
        .expect("Remaining bookmarks");
    assert_eq!(remaining.len(), 2);

    // 6. Test Multi-Item Isolation under Shared Root:
    // Create a second distinct media item under the exact same root_id.
    let item_b = MediaItem {
        id: 0,
        root_id,
        relative_path: "seminar.mp4".to_string(),
        canonical_path: "/test/bookmarks/seminar.mp4".to_string(),
        file_name: "seminar.mp4".to_string(),
        file_extension: "mp4".to_string(),
        media_type: MediaType::Video,
        file_size: 75_000_000,
        modified_time: 1700000000,
        added_time: 1700000000,
        last_scanned_at: 1700000000,
        availability_state: AvailabilityState::Available,
        metadata_status: MetadataStatus::Extracted,
    };
    let media_id_b = db.upsert_media_item(&item_b).expect("Insert media item B");
    assert_ne!(
        media_id, media_id_b,
        "Media item IDs under the same root must be distinct"
    );

    let bm_b1 = db
        .insert_bookmark(
            media_id_b,
            45_000,
            "Seminar Keynote",
            Some("Guest speaker notes"),
        )
        .expect("Insert bookmark on item B");

    // Verify isolation: listing bookmarks for media A returns only A's bookmarks, and B returns B's
    let bms_a = db
        .list_bookmarks_for_media(media_id)
        .expect("Query bookmarks A");
    let bms_b = db
        .list_bookmarks_for_media(media_id_b)
        .expect("Query bookmarks B");
    assert_eq!(bms_a.len(), 2);
    assert_eq!(bms_b.len(), 1);
    assert_eq!(bms_b[0].id, bm_b1);
    assert_eq!(bms_b[0].media_id, media_id_b);
    assert_eq!(bms_b[0].label, "Seminar Keynote");

    // 7. Test Foreign Key Cascade: delete media item A.
    // Media item A bookmarks must cascade delete, while Media item B bookmarks must remain intact.
    db.delete_media_item(media_id).expect("Delete media item A");

    let post_delete_a = db
        .list_bookmarks_for_media(media_id)
        .expect("Query post-delete A");
    assert_eq!(
        post_delete_a.len(),
        0,
        "All bookmarks must cascade-delete when parent media_item A is removed"
    );

    let post_delete_b = db
        .list_bookmarks_for_media(media_id_b)
        .expect("Query post-delete B");
    assert_eq!(
        post_delete_b.len(),
        1,
        "Media item B bookmarks must remain untouched when media item A is deleted"
    );
    assert_eq!(post_delete_b[0].id, bm_b1);
}

#[test]
fn test_database_migration_from_phase_1_1_preserves_bookmarks_and_adds_ssh_trust() {
    let temp_dir = std::env::temp_dir();
    let db_path = temp_dir.join(format!(
        "muvrix_migration_test_{}_{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    // 1. Manually build a genuine Phase 1.1 database (Schema version 2)
    {
        let conn = rusqlite::Connection::open(&db_path).expect("Open raw sqlite");
        conn.execute_batch(
            "
            PRAGMA foreign_keys = ON;
            CREATE TABLE schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at INTEGER NOT NULL,
                description TEXT NOT NULL
            );
            INSERT INTO schema_migrations (version, applied_at, description)
            VALUES (1, 1700000000, 'Initial core schema'),
                   (2, 1700000100, 'User bookmarks schema');

            CREATE TABLE library_roots (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                display_name TEXT NOT NULL,
                root_path TEXT NOT NULL UNIQUE,
                source_type TEXT NOT NULL DEFAULT 'local',
                is_recursive INTEGER NOT NULL DEFAULT 1,
                is_enabled INTEGER NOT NULL DEFAULT 1,
                exclusion_patterns TEXT NOT NULL DEFAULT '[]',
                last_scanned_at INTEGER NOT NULL DEFAULT 0,
                scan_status TEXT NOT NULL DEFAULT 'idle'
            );

            CREATE TABLE media_items (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                root_id INTEGER NOT NULL REFERENCES library_roots(id) ON DELETE CASCADE,
                relative_path TEXT NOT NULL,
                canonical_path TEXT NOT NULL UNIQUE,
                file_name TEXT NOT NULL,
                file_extension TEXT NOT NULL,
                media_type TEXT NOT NULL,
                file_size INTEGER NOT NULL,
                modified_time INTEGER NOT NULL,
                added_time INTEGER NOT NULL,
                duration_seconds REAL,
                is_available INTEGER NOT NULL DEFAULT 1
            );

            CREATE TABLE user_bookmarks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                media_id INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
                timestamp_ms INTEGER NOT NULL,
                label TEXT NOT NULL,
                note TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            INSERT INTO library_roots (id, display_name, root_path)
            VALUES (1, 'Archive', '/media/archive');

            INSERT INTO media_items (id, root_id, relative_path, canonical_path, file_name, file_extension, media_type, file_size, modified_time, added_time)
            VALUES (1, 1, 'video.mp4', '/media/archive/video.mp4', 'video.mp4', 'mp4', 'video', 1048576, 1700000000, 1700000000);

            INSERT INTO user_bookmarks (id, media_id, timestamp_ms, label, note, created_at, updated_at)
            VALUES (101, 1, 125500, 'Phase 1.1 Bookmark', 'Key intro scene', 1700000200, 1700000200);
            ",
        ).expect("Create Phase 1.1 DB");
    }

    // 2. Open via DatabaseManager::open, which runs apply_migrations()
    let db = DatabaseManager::open(&db_path).expect("Open via DatabaseManager");
    assert!(db.check_integrity().expect("Integrity check"));

    // 3. Verify schema upgraded from 2 to 3
    let version = db.get_schema_version().expect("Get schema version");
    assert_eq!(
        version, 3,
        "Database must successfully upgrade to Schema Version 3"
    );

    // 4. Verify existing Phase 1.1 bookmarks are preserved intact
    let bookmarks = db.list_bookmarks_for_media(1).expect("Query bookmarks");
    assert_eq!(bookmarks.len(), 1);
    assert_eq!(bookmarks[0].id, 101);
    assert_eq!(bookmarks[0].label, "Phase 1.1 Bookmark");
    assert_eq!(bookmarks[0].timestamp_seconds(), 125.5);

    // 5. Verify new migration 003 table (ssh_host_trust) is fully operational
    let trust = db
        .record_ssh_host_trust(
            "sftp.legacy-migrated.com",
            22,
            "ssh-ed25519",
            "SHA256:MIGRATED000000000000000000000000000000000",
            Some("Migrated server"),
        )
        .expect("Record trust in upgraded DB");
    assert_eq!(trust.host, "sftp.legacy-migrated.com");

    let queried = db
        .get_ssh_host_trust("sftp.legacy-migrated.com", 22)
        .unwrap()
        .unwrap();
    assert_eq!(
        queried.fingerprint_sha256,
        "SHA256:MIGRATED000000000000000000000000000000000"
    );

    let _ = std::fs::remove_file(&db_path);
}
