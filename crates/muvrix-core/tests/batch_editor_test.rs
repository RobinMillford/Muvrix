use muvrix_core::database::{
    execute_batch_metadata_update, BatchMetadataRequest, DatabaseManager, FieldAction,
    FieldMixedState, MediaItem, MediaMetadata, MediaType, SelectedMetadataSummary,
};
use tempfile::tempdir;

fn setup_test_db() -> (DatabaseManager, tempfile::TempDir) {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("batch_test.db");
    let db = DatabaseManager::open(&db_path).expect("open test db");
    (db, dir)
}

fn seed_batch_test_media(db: &DatabaseManager) -> Vec<i64> {
    let root_id = db
        .insert_root(&muvrix_core::database::LibraryRoot {
            id: 0,
            display_name: "Test Root".to_string(),
            root_path: "/media".to_string(),
            source_type: "local".to_string(),
            is_recursive: true,
            is_enabled: true,
            exclusion_patterns: Vec::new(),
            last_scanned_at: 1000,
            scan_status: "idle".to_string(),
        })
        .expect("insert root");

    let mut ids = Vec::new();
    let tracks = vec![
        ("Song A", "Artist One", "Album X", "Rock", 1990),
        ("Song B", "Artist Two", "Album X", "Pop", 1995),
        ("Song C", "Artist One", "Album Y", "Jazz", 2000),
    ];

    for (title, artist, album, genre, year) in tracks {
        let id = db
            .upsert_media_item(&MediaItem {
                id: 0,
                root_id,
                relative_path: format!("{title}.mp3"),
                canonical_path: format!("/media/{title}.mp3"),
                file_name: format!("{title}.mp3"),
                file_extension: "mp3".to_string(),
                media_type: MediaType::Audio,
                file_size: 1024 * 1024,
                modified_time: 1000,
                added_time: 1000,
                last_scanned_at: 1000,
                availability_state: muvrix_core::database::AvailabilityState::Available,
                metadata_status: muvrix_core::database::MetadataStatus::Extracted,
            })
            .expect("upsert media");

        db.upsert_metadata(&MediaMetadata {
            media_id: id,
            title: Some(title.to_string()),
            original_title: None,
            description: None,
            genre: Some(genre.to_string()),
            release_year: Some(year),
            duration_ms: 180_000,
            overall_bitrate: 320000,
            container: Some("mp3".to_string()),
            video_codec: None,
            audio_codec: Some("mp3".to_string()),
            width: 0,
            height: 0,
            fps: 0.0,
            hdr_type: None,
            audio_channels: 2,
            sample_rate: 44100,
            bit_depth: 16,
            artist: Some(artist.to_string()),
            album: Some(album.to_string()),
            album_artist: Some(artist.to_string()),
            track_number: Some(1),
            disc_number: Some(1),
        })
        .expect("upsert metadata");

        ids.push(id);
    }

    ids
}

#[test]
fn test_mixed_value_detection() {
    let (db, _dir) = setup_test_db();
    let ids = seed_batch_test_media(&db);

    let m0 = db.get_metadata(ids[0]).unwrap();
    let m1 = db.get_metadata(ids[1]).unwrap();

    // Comparing Song A and Song B:
    // Album is uniform ("Album X"), Artist is mixed ("Artist One" vs "Artist Two")
    let summary = SelectedMetadataSummary::analyze(&[m0, m1]);
    assert_eq!(summary.count, 2);
    assert_eq!(
        summary.album,
        FieldMixedState::Uniform(Some("Album X".to_string()))
    );
    assert_eq!(summary.artist, FieldMixedState::Mixed);
    assert_eq!(summary.genre, FieldMixedState::Mixed);
}

#[test]
fn test_batch_metadata_selective_update() {
    let (db, _dir) = setup_test_db();
    let ids = seed_batch_test_media(&db);

    // Update Genre to "Electronic" for all 3 tracks, leaving Artist, Album, Title UNTOUCHED
    let req = BatchMetadataRequest {
        genre: FieldAction::Set("Electronic".to_string()),
        ..Default::default()
    };

    let result = db.batch_update_metadata(&ids, &req).expect("batch update");
    assert_eq!(result.affected_items, 3);
    assert_eq!(result.modified_fields, vec!["Genre"]);

    // Verify all 3 tracks now have genre = "Electronic"
    for id in &ids {
        let meta = db.get_metadata(*id).unwrap().unwrap();
        assert_eq!(meta.genre.as_deref(), Some("Electronic"));
    }

    // Verify untouched fields are intact
    let meta0 = db.get_metadata(ids[0]).unwrap().unwrap();
    assert_eq!(meta0.title.as_deref(), Some("Song A"));
    assert_eq!(meta0.artist.as_deref(), Some("Artist One"));
    assert_eq!(meta0.album.as_deref(), Some("Album X"));
    assert_eq!(meta0.release_year, Some(1990));
}

#[test]
fn test_batch_metadata_clear_field() {
    let (db, _dir) = setup_test_db();
    let ids = seed_batch_test_media(&db);

    // Clear release year across all items
    let req = BatchMetadataRequest {
        release_year: FieldAction::Clear,
        ..Default::default()
    };

    let result = db.batch_update_metadata(&ids, &req).expect("batch update");
    assert_eq!(result.affected_items, 3);

    for id in &ids {
        let meta = db.get_metadata(*id).unwrap().unwrap();
        assert_eq!(meta.release_year, None);
    }
}

#[test]
fn test_batch_metadata_transactional_rollback() {
    let (db, _dir) = setup_test_db();
    let ids = seed_batch_test_media(&db);

    // Simulate an error inside transaction
    let res: Result<(), _> = db.with_transaction(|tx| {
        let req = BatchMetadataRequest {
            genre: FieldAction::Set("Should Rollback".to_string()),
            ..Default::default()
        };
        execute_batch_metadata_update(tx, &ids[0..1], &req)?;
        // Force deliberate failure
        Err(muvrix_core::database::DatabaseError::Migration(
            "Deliberate failure".to_string(),
        ))
    });

    assert!(res.is_err());

    // Verify item 0 genre was NOT updated
    let meta0 = db.get_metadata(ids[0]).unwrap().unwrap();
    assert_eq!(meta0.genre.as_deref(), Some("Rock"));
}

#[test]
fn test_batch_metadata_two_items_workflow_and_fts5_sync() {
    let (db, _dir) = setup_test_db();
    let ids = seed_batch_test_media(&db);

    // 1. Select two catalog records (Song A and Song B)
    let selected_ids = vec![ids[0], ids[1]];

    let m0 = db.get_metadata(selected_ids[0]).unwrap();
    let m1 = db.get_metadata(selected_ids[1]).unwrap();

    // 2. Confirm mixed and uniform metadata detection
    let summary = SelectedMetadataSummary::analyze(&[m0, m1]);
    assert_eq!(summary.count, 2);
    assert_eq!(summary.title, FieldMixedState::Mixed);
    assert_eq!(summary.artist, FieldMixedState::Mixed);
    assert_eq!(
        summary.album,
        FieldMixedState::Uniform(Some("Album X".to_string()))
    );

    // 3. Change one field only: Genre -> "Synthwave"
    let req = BatchMetadataRequest {
        genre: FieldAction::Set("Synthwave".to_string()),
        ..Default::default()
    };

    // 4. Execute real database update
    let result = db
        .batch_update_metadata(&selected_ids, &req)
        .expect("batch update should succeed");
    assert_eq!(result.affected_items, 2);
    assert_eq!(result.modified_fields, vec!["Genre"]);

    // 5. Verify untouched fields remain unchanged
    let meta_a = db.get_metadata(selected_ids[0]).unwrap().unwrap();
    assert_eq!(meta_a.title.as_deref(), Some("Song A"));
    assert_eq!(meta_a.artist.as_deref(), Some("Artist One"));
    assert_eq!(meta_a.album.as_deref(), Some("Album X"));
    assert_eq!(meta_a.genre.as_deref(), Some("Synthwave"));
    assert_eq!(meta_a.release_year, Some(1990));

    let meta_b = db.get_metadata(selected_ids[1]).unwrap().unwrap();
    assert_eq!(meta_b.title.as_deref(), Some("Song B"));
    assert_eq!(meta_b.artist.as_deref(), Some("Artist Two"));
    assert_eq!(meta_b.album.as_deref(), Some("Album X"));
    assert_eq!(meta_b.genre.as_deref(), Some("Synthwave"));
    assert_eq!(meta_b.release_year, Some(1995));

    // Item C was not in the batch and remains completely untouched
    let meta_c = db.get_metadata(ids[2]).unwrap().unwrap();
    assert_eq!(meta_c.genre.as_deref(), Some("Jazz"));

    // 6. Verify FTS5 search index synchronization
    db.with_transaction(|tx| {
        let mut stmt = tx
            .prepare("SELECT rowid FROM media_items_fts WHERE media_items_fts MATCH 'Synthwave'")
            .map_err(muvrix_core::database::DatabaseError::Sqlite)?;
        let rows = stmt
            .query_map([], |row| row.get(0))
            .map_err(muvrix_core::database::DatabaseError::Sqlite)?;
        let mut matched_ids: Vec<i64> = Vec::new();
        for r in rows {
            matched_ids.push(r.map_err(muvrix_core::database::DatabaseError::Sqlite)?);
        }

        assert_eq!(matched_ids.len(), 2);
        assert!(matched_ids.contains(&selected_ids[0]));
        assert!(matched_ids.contains(&selected_ids[1]));
        assert!(!matched_ids.contains(&ids[2]));
        Ok(())
    })
    .expect("FTS5 query verification");
}
