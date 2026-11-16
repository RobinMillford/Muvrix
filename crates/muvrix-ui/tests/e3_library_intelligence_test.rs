//! Phase E3: Library Intelligence, Smart Collections, Batch Metadata Editor & Integrity Locator Tests.

use muvrix_core::database::{
    find_relink_candidates, AvailabilityState, BatchMetadataRequest, DatabaseManager, FieldAction,
    FieldMixedState, LibraryRoot, MediaItem, MediaMetadata, MediaType, MetadataStatus, Pagination,
    RuleCombinationMode, RuleDefinition, RuleField, SelectedMetadataSummary, SmartCollection,
    SmartRule, TextOperator,
};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

struct TestTempDir {
    path: PathBuf,
}

impl TestTempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "muvrix_e3_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestTempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn setup_test_db() -> (DatabaseManager, TestTempDir) {
    let dir = TestTempDir::new();
    let db_path = dir.path().join("e3_test.db");
    let db = DatabaseManager::open(&db_path).expect("open test db");
    (db, dir)
}

fn seed_library_catalog(db: &DatabaseManager, dir: &TestTempDir) -> Vec<i64> {
    let root_path = dir.path().join("library_root");
    std::fs::create_dir_all(&root_path).unwrap();

    let root_id = db
        .insert_root(&LibraryRoot {
            id: 0,
            display_name: "Test Media Root".to_string(),
            root_path: root_path.to_string_lossy().to_string(),
            source_type: "local".to_string(),
            is_recursive: true,
            is_enabled: true,
            exclusion_patterns: Vec::new(),
            last_scanned_at: 1000,
            scan_status: "idle".to_string(),
        })
        .expect("insert root");

    let entries = [
        (
            "Dune Part Two",
            "Denis Villeneuve",
            "Sci-Fi",
            2024,
            "mkv",
            MediaType::Video,
            166 * 60 * 1000,
            15_000_000_000u64,
        ),
        (
            "Interstellar",
            "Christopher Nolan",
            "Sci-Fi",
            2014,
            "mp4",
            MediaType::Video,
            169 * 60 * 1000,
            12_000_000_000u64,
        ),
        (
            "Oppenheimer",
            "Christopher Nolan",
            "Drama",
            2023,
            "mkv",
            MediaType::Video,
            180 * 60 * 1000,
            14_000_000_000u64,
        ),
        (
            "Time",
            "Pink Floyd",
            "Rock",
            1973,
            "flac",
            MediaType::Audio,
            425 * 1000,
            45_000_000u64,
        ),
        (
            "Money",
            "Pink Floyd",
            "Rock",
            1973,
            "flac",
            MediaType::Audio,
            382 * 1000,
            40_000_000u64,
        ),
    ];

    let mut ids = Vec::new();
    for (title, artist, genre, year, ext, mtype, duration_ms, file_size) in entries {
        let file_path = root_path.join(format!("{title}.{ext}"));
        {
            let mut f = File::create(&file_path).unwrap();
            f.write_all(b"sample content").unwrap();
        }

        let id = db
            .upsert_media_item(&MediaItem {
                id: 0,
                root_id,
                relative_path: format!("{title}.{ext}"),
                canonical_path: file_path.to_string_lossy().to_string(),
                file_name: format!("{title}.{ext}"),
                file_extension: ext.to_string(),
                media_type: mtype,
                file_size,
                modified_time: 1700000000,
                added_time: 1700000000,
                last_scanned_at: 1700000000,
                availability_state: AvailabilityState::Available,
                metadata_status: MetadataStatus::Extracted,
            })
            .expect("upsert media item");

        db.upsert_metadata(&MediaMetadata {
            media_id: id,
            title: Some(title.to_string()),
            artist: Some(artist.to_string()),
            genre: Some(genre.to_string()),
            release_year: Some(year),
            duration_ms,
            overall_bitrate: (file_size * 8 / (duration_ms / 1000).max(1)),
            ..Default::default()
        })
        .expect("upsert metadata");

        ids.push(id);
    }

    ids
}

#[test]
fn test_smart_collection_engine_end_to_end() {
    let (db, dir) = setup_test_db();
    let ids = seed_library_catalog(&db, &dir);
    assert_eq!(ids.len(), 5);

    // Mark Dune Part Two as favourite
    db.set_favourite(ids[0], true).unwrap();

    // 1. Create Smart Collection: "Sci-Fi Movies Released After 2020"
    let mut rules = RuleDefinition::new(RuleCombinationMode::And);
    rules.add_rule(SmartRule::Text {
        field: RuleField::Genre,
        operator: TextOperator::Equals,
        value: "Sci-Fi".to_string(),
    });
    rules.add_rule(SmartRule::Numeric {
        field: RuleField::ReleaseYear,
        operator: muvrix_core::database::NumericOperator::GreaterThan,
        value: 2020.0,
        value_to: None,
    });

    let sc = SmartCollection {
        id: 0,
        name: "Modern Sci-Fi".to_string(),
        description: "Sci-Fi released after 2020".to_string(),
        icon: Some("🚀".to_string()),
        rules,
        combination_mode: RuleCombinationMode::And,
        sort_field: Some("title".to_string()),
        sort_direction: Some("asc".to_string()),
        display_order: 1,
        created_at: 1000,
        updated_at: 1000,
    };

    let sc_id = db
        .create_smart_collection(&sc)
        .expect("create smart collection");
    let loaded_sc = db.get_smart_collection_by_id(sc_id).unwrap().unwrap();
    assert_eq!(loaded_sc.name, "Modern Sci-Fi");

    // 2. Count matching items
    let count = db.count_smart_collection_items(&loaded_sc).unwrap();
    assert_eq!(count, 1);

    // 3. Query items
    let (items, total) = db
        .query_smart_collection_items(
            &loaded_sc,
            Pagination {
                offset: 0,
                limit: 10,
            },
        )
        .unwrap();
    assert_eq!(total, 1);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].item.file_name, "Dune Part Two.mkv");

    // 4. Test OR combination mode: "Pink Floyd Tracks OR Favourites"
    let mut or_rules = RuleDefinition::new(RuleCombinationMode::Or);
    or_rules.add_rule(SmartRule::Text {
        field: RuleField::Artist,
        operator: TextOperator::Contains,
        value: "Pink Floyd".to_string(),
    });
    or_rules.add_rule(SmartRule::Library {
        operator: muvrix_core::database::LibraryOperator::IsFavourite,
        days: None,
    });

    let or_sc = SmartCollection {
        id: 0,
        name: "Rock or Star".to_string(),
        description: "Pink Floyd or starred media".to_string(),
        icon: Some("⭐".to_string()),
        rules: or_rules,
        combination_mode: RuleCombinationMode::Or,
        sort_field: Some("title".to_string()),
        sort_direction: Some("asc".to_string()),
        display_order: 2,
        created_at: 1000,
        updated_at: 1000,
    };

    let or_count = db.count_smart_collection_items(&or_sc).unwrap();
    // 2 Pink Floyd tracks + 1 favourite Dune Part Two = 3 items
    assert_eq!(or_count, 3);
}

#[test]
fn test_batch_metadata_editor_mixed_value_workflow() {
    let (db, dir) = setup_test_db();
    let ids = seed_library_catalog(&db, &dir);

    // Select items 3 and 4: "Time" and "Money" by Pink Floyd, Genre "Rock", Year 1973
    let pink_floyd_ids = vec![ids[3], ids[4]];
    let mut metadata_opts = Vec::new();
    for &id in &pink_floyd_ids {
        metadata_opts.push(db.get_metadata(id).unwrap());
    }

    let summary = SelectedMetadataSummary::analyze(&metadata_opts);
    assert_eq!(summary.count, 2);
    // Title is mixed ("Time" vs "Money")
    assert_eq!(summary.title, FieldMixedState::Mixed);
    // Artist is uniform ("Pink Floyd")
    assert_eq!(
        summary.artist,
        FieldMixedState::Uniform(Some("Pink Floyd".to_string()))
    );
    // Genre is uniform ("Rock")
    assert_eq!(
        summary.genre,
        FieldMixedState::Uniform(Some("Rock".to_string()))
    );
    // Year is uniform (1973)
    assert_eq!(summary.release_year, FieldMixedState::Uniform(Some(1973)));

    // Execute batch update: Keep mixed title, set genre to "Classic Rock", clear year
    let req = BatchMetadataRequest {
        title: FieldAction::Keep,
        artist: FieldAction::Keep,
        genre: FieldAction::Set("Classic Rock".to_string()),
        release_year: FieldAction::Clear,
        ..Default::default()
    };

    let result = db.batch_update_metadata(&pink_floyd_ids, &req).unwrap();
    assert_eq!(result.affected_items, 2);

    // Verify item 3
    let meta3 = db.get_metadata(ids[3]).unwrap().unwrap();
    assert_eq!(meta3.title.as_deref(), Some("Time")); // Preserved
    assert_eq!(meta3.artist.as_deref(), Some("Pink Floyd")); // Preserved
    assert_eq!(meta3.genre.as_deref(), Some("Classic Rock")); // Updated
    assert_eq!(meta3.release_year, None); // Cleared

    // Verify item 4
    let meta4 = db.get_metadata(ids[4]).unwrap().unwrap();
    assert_eq!(meta4.title.as_deref(), Some("Money")); // Preserved
    assert_eq!(meta4.artist.as_deref(), Some("Pink Floyd")); // Preserved
    assert_eq!(meta4.genre.as_deref(), Some("Classic Rock")); // Updated
    assert_eq!(meta4.release_year, None); // Cleared
}

#[test]
fn test_broken_file_locator_and_relink_preserves_history() {
    let (db, dir) = setup_test_db();
    let ids = seed_library_catalog(&db, &dir);
    let target_id = ids[0]; // Dune Part Two.mkv

    // Add playback state, favourite, and bookmark
    db.set_watched(target_id, true).unwrap();
    db.set_favourite(target_id, true).unwrap();
    let bookmark_id = db
        .insert_bookmark(target_id, 95000, "Worm riding scene", Some("Peak cinema"))
        .unwrap();

    let old_item = db.get_media_item_by_id(target_id).unwrap().unwrap();
    let old_path = PathBuf::from(&old_item.canonical_path);

    // Move file to a new location to simulate broken file path
    let new_folder = dir.path().join("relocated_movies");
    std::fs::create_dir_all(&new_folder).unwrap();
    let new_path = new_folder.join("Dune Part Two.mkv");
    std::fs::rename(&old_path, &new_path).unwrap();

    // 1. Scan integrity -> detects missing file
    let progress = db.scan_library_integrity(None).unwrap();
    assert_eq!(progress.missing_count, 1);
    let item_after_scan = db.get_media_item_by_id(target_id).unwrap().unwrap();
    assert_eq!(
        item_after_scan.availability_state,
        AvailabilityState::Unavailable
    );

    // 2. Discover replacement candidates
    let candidates =
        find_relink_candidates(&[new_folder], &item_after_scan, Some("Dune Part Two"), None);
    assert!(!candidates.is_empty());
    assert_eq!(candidates[0].path, new_path);
    assert!(candidates[0].confidence >= 0.80);

    // 3. Relink item to replacement path
    let relinked = db.relink_media_item(target_id, &new_path).unwrap();

    // Verify media ID is strictly preserved
    assert_eq!(relinked.id, target_id);
    assert_eq!(relinked.availability_state, AvailabilityState::Available);

    // Verify watched state, favourite, and bookmarks are 100% preserved
    assert!(db.is_watched(target_id).unwrap());
    assert!(db.is_favourite(target_id).unwrap());
    let bookmarks = db.list_bookmarks_for_media(target_id).unwrap();
    assert_eq!(bookmarks.len(), 1);
    assert_eq!(bookmarks[0].id, bookmark_id);
    assert_eq!(bookmarks[0].label, "Worm riding scene");
}

#[test]
fn test_catalog_cleanup_preview_and_atomic_execution() {
    let (db, dir) = setup_test_db();
    let ids = seed_library_catalog(&db, &dir);
    let deleted_item_id = ids[1]; // Interstellar.mp4

    // Physically delete file from disk
    let item = db.get_media_item_by_id(deleted_item_id).unwrap().unwrap();
    std::fs::remove_file(&item.canonical_path).unwrap();

    // Run integrity scan so it's flagged as unavailable
    let _ = db.scan_library_integrity(None).unwrap();

    // Inspect preview
    let preview = db.generate_cleanup_preview().unwrap();
    assert!(preview.confirmed_missing_ids.contains(&deleted_item_id));

    // Execute cleanup
    let cleaned_count = db.execute_catalog_cleanup(&preview).unwrap();
    assert!(cleaned_count >= 1);

    // Confirmed deleted from database
    let removed = db.get_media_item_by_id(deleted_item_id).unwrap();
    assert!(removed.is_none());

    // Other items are untouched
    let valid_item = db.get_media_item_by_id(ids[0]).unwrap();
    assert!(valid_item.is_some());
}
