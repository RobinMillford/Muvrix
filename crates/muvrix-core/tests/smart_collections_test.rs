use muvrix_core::database::{
    compile_smart_rules, DatabaseManager, DateOperator, LibraryOperator, MediaItem, MediaMetadata,
    MediaType, NumericOperator, Pagination, RuleCombinationMode, RuleDefinition, RuleField,
    SmartCollection, SmartRule, TextOperator,
};
use tempfile::tempdir;

fn setup_test_db() -> (DatabaseManager, tempfile::TempDir) {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test_library.db");
    let db = DatabaseManager::open(&db_path).expect("open test db");
    (db, dir)
}

fn seed_test_media(db: &DatabaseManager) -> Vec<i64> {
    let root_id = db
        .insert_root(&muvrix_core::database::LibraryRoot {
            id: 0,
            display_name: "Music Root".to_string(),
            root_path: "/music".to_string(),
            source_type: "local".to_string(),
            is_recursive: true,
            is_enabled: true,
            exclusion_patterns: Vec::new(),
            last_scanned_at: 1000,
            scan_status: "idle".to_string(),
        })
        .expect("insert root");

    let items = vec![
        (
            "Comfortably Numb",
            "Pink Floyd",
            "The Wall",
            "Rock",
            1979,
            382,
            1000000,
            "audio",
            "available",
        ),
        (
            "Time",
            "Pink Floyd",
            "The Dark Side of the Moon",
            "Rock",
            1973,
            413,
            2000000,
            "audio",
            "available",
        ),
        (
            "Billie Jean",
            "Michael Jackson",
            "Thriller",
            "Pop",
            1982,
            294,
            3000000,
            "audio",
            "available",
        ),
        (
            "Bohemian Rhapsody",
            "Queen",
            "A Night at the Opera",
            "Rock",
            1975,
            354,
            4000000,
            "audio",
            "unavailable",
        ),
        (
            "Yesterday",
            "The Beatles",
            "Help!",
            "Pop",
            1965,
            125,
            5000000,
            "audio",
            "available",
        ),
    ];

    let mut ids = Vec::new();
    for (title, artist, album, genre, year, dur_sec, added_time, media_type, avail) in items {
        let id = db
            .upsert_media_item(&MediaItem {
                id: 0,
                root_id,
                relative_path: format!("{artist}/{title}.mp3"),
                canonical_path: format!("/music/{artist}/{title}.mp3"),
                file_name: format!("{title}.mp3"),
                file_extension: "mp3".to_string(),
                media_type: MediaType::parse(media_type),
                file_size: 5 * 1024 * 1024,
                modified_time: added_time,
                added_time,
                last_scanned_at: added_time,
                availability_state: muvrix_core::database::AvailabilityState::parse(avail),
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
            duration_ms: dur_sec * 1000,
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

    // Mark Billie Jean as watched/played
    let _ = db.set_watched(ids[2], true);
    // Mark Comfortably Numb as favourite
    let _ = db.set_favourite(ids[0], true);

    ids
}

#[test]
fn test_smart_collection_crud_and_persistence() {
    let (db, _dir) = setup_test_db();

    let mut rules = RuleDefinition::new(RuleCombinationMode::And);
    rules.add_rule(SmartRule::Text {
        field: RuleField::Genre,
        operator: TextOperator::Contains,
        value: "Rock".to_string(),
    });
    rules.add_rule(SmartRule::Numeric {
        field: RuleField::ReleaseYear,
        operator: NumericOperator::GreaterThan,
        value: 1970.0,
        value_to: None,
    });

    let sc = SmartCollection {
        id: 0,
        name: "Classic 70s Rock".to_string(),
        description: "Greatest rock anthems of the 70s".to_string(),
        icon: Some("music-note".to_string()),
        rules,
        combination_mode: RuleCombinationMode::And,
        sort_field: Some("mm.release_year".to_string()),
        sort_direction: Some("ASC".to_string()),
        display_order: 1,
        created_at: 0,
        updated_at: 0,
    };

    let col_id = db
        .create_smart_collection(&sc)
        .expect("create smart collection");
    assert!(col_id > 0);

    let loaded = db
        .get_smart_collection_by_id(col_id)
        .expect("query collection")
        .expect("collection exists");

    assert_eq!(loaded.name, "Classic 70s Rock");
    assert_eq!(loaded.rules.rules.len(), 2);
    assert_eq!(loaded.combination_mode, RuleCombinationMode::And);

    // Update collection
    let mut updated = loaded.clone();
    updated.description = "Updated description".to_string();
    updated.display_order = 5;
    db.update_smart_collection(&updated)
        .expect("update collection");

    let reloaded = db.get_smart_collection_by_id(col_id).unwrap().unwrap();
    assert_eq!(reloaded.description, "Updated description");
    assert_eq!(reloaded.display_order, 5);

    // List collections
    let list = db.list_smart_collections().expect("list collections");
    assert_eq!(list.len(), 1);

    // Delete collection
    db.delete_smart_collection(col_id)
        .expect("delete collection");
    assert!(db.get_smart_collection_by_id(col_id).unwrap().is_none());
}

#[test]
fn test_smart_collection_text_matching_rules() {
    let (db, _dir) = setup_test_db();
    let _ids = seed_test_media(&db);

    // 1. Artist equals "Pink Floyd"
    let mut rules = RuleDefinition::new(RuleCombinationMode::And);
    rules.add_rule(SmartRule::Text {
        field: RuleField::Artist,
        operator: TextOperator::Equals,
        value: "Pink Floyd".to_string(),
    });
    let sc = SmartCollection {
        id: 1,
        name: "Pink Floyd Tracks".to_string(),
        description: "".to_string(),
        icon: None,
        rules,
        combination_mode: RuleCombinationMode::And,
        sort_field: None,
        sort_direction: None,
        display_order: 0,
        created_at: 0,
        updated_at: 0,
    };
    let count = db.count_smart_collection_items(&sc).expect("count items");
    assert_eq!(count, 2);

    let (items, total) = db
        .query_smart_collection_items(&sc, Pagination::default())
        .expect("query items");
    assert_eq!(total, 2);
    assert_eq!(items.len(), 2);
    assert!(items
        .iter()
        .all(|i| i.metadata.as_ref().unwrap().artist.as_deref() == Some("Pink Floyd")));

    // 2. Genre contains "Pop"
    let mut pop_rules = RuleDefinition::new(RuleCombinationMode::And);
    pop_rules.add_rule(SmartRule::Text {
        field: RuleField::Genre,
        operator: TextOperator::Contains,
        value: "Pop".to_string(),
    });
    let pop_sc = SmartCollection {
        id: 2,
        name: "Pop Collection".to_string(),
        description: "".to_string(),
        icon: None,
        rules: pop_rules,
        combination_mode: RuleCombinationMode::And,
        sort_field: None,
        sort_direction: None,
        display_order: 0,
        created_at: 0,
        updated_at: 0,
    };
    let pop_count = db.count_smart_collection_items(&pop_sc).expect("count pop");
    assert_eq!(pop_count, 2); // Billie Jean, Yesterday
}

#[test]
fn test_smart_collection_numeric_and_between_rules() {
    let (db, _dir) = setup_test_db();
    let _ids = seed_test_media(&db);

    // 1. Duration > 300s (5 minutes)
    let mut rules = RuleDefinition::new(RuleCombinationMode::And);
    rules.add_rule(SmartRule::Numeric {
        field: RuleField::Duration,
        operator: NumericOperator::GreaterThan,
        value: 300.0,
        value_to: None,
    });
    let sc = SmartCollection {
        id: 1,
        name: "Epic Length Tracks".to_string(),
        description: "".to_string(),
        icon: None,
        rules,
        combination_mode: RuleCombinationMode::And,
        sort_field: None,
        sort_direction: None,
        display_order: 0,
        created_at: 0,
        updated_at: 0,
    };
    let count = db.count_smart_collection_items(&sc).expect("count items");
    assert_eq!(count, 3); // Comfortably Numb (382s), Time (413s), Bohemian Rhapsody (354s)

    // 2. Release Year between 1970 and 1980
    let mut year_rules = RuleDefinition::new(RuleCombinationMode::And);
    year_rules.add_rule(SmartRule::Numeric {
        field: RuleField::ReleaseYear,
        operator: NumericOperator::Between,
        value: 1970.0,
        value_to: Some(1980.0),
    });
    let year_sc = SmartCollection {
        id: 2,
        name: "70s Anthems".to_string(),
        description: "".to_string(),
        icon: None,
        rules: year_rules,
        combination_mode: RuleCombinationMode::And,
        sort_field: None,
        sort_direction: None,
        display_order: 0,
        created_at: 0,
        updated_at: 0,
    };
    let year_count = db
        .count_smart_collection_items(&year_sc)
        .expect("count items");
    assert_eq!(year_count, 3); // 1979, 1973, 1975
}

#[test]
fn test_smart_collection_library_conditions() {
    let (db, _dir) = setup_test_db();
    let _ids = seed_test_media(&db);

    // 1. Is Missing
    let mut missing_rules = RuleDefinition::new(RuleCombinationMode::And);
    missing_rules.add_rule(SmartRule::Library {
        operator: LibraryOperator::IsMissing,
        days: None,
    });
    let missing_sc = SmartCollection {
        id: 1,
        name: "Missing Media".to_string(),
        description: "".to_string(),
        icon: None,
        rules: missing_rules,
        combination_mode: RuleCombinationMode::And,
        sort_field: None,
        sort_direction: None,
        display_order: 0,
        created_at: 0,
        updated_at: 0,
    };
    let count = db
        .count_smart_collection_items(&missing_sc)
        .expect("count missing");
    assert_eq!(count, 1); // Bohemian Rhapsody

    // 2. Is Favourite
    let mut fav_rules = RuleDefinition::new(RuleCombinationMode::And);
    fav_rules.add_rule(SmartRule::Library {
        operator: LibraryOperator::IsFavourite,
        days: None,
    });
    let fav_sc = SmartCollection {
        id: 2,
        name: "Favourites Smart Collection".to_string(),
        description: "".to_string(),
        icon: None,
        rules: fav_rules,
        combination_mode: RuleCombinationMode::And,
        sort_field: None,
        sort_direction: None,
        display_order: 0,
        created_at: 0,
        updated_at: 0,
    };
    let fav_count = db
        .count_smart_collection_items(&fav_sc)
        .expect("count favs");
    assert_eq!(fav_count, 1); // Comfortably Numb
}

#[test]
fn test_smart_collection_or_combination_mode() {
    let (db, _dir) = setup_test_db();
    let _ids = seed_test_media(&db);

    // Artist = "Beatles" OR Genre = "Pop"
    let mut rules = RuleDefinition::new(RuleCombinationMode::Or);
    rules.add_rule(SmartRule::Text {
        field: RuleField::Artist,
        operator: TextOperator::Contains,
        value: "Beatles".to_string(),
    });
    rules.add_rule(SmartRule::Text {
        field: RuleField::Genre,
        operator: TextOperator::Equals,
        value: "Pop".to_string(),
    });

    let sc = SmartCollection {
        id: 1,
        name: "Beatles or Pop".to_string(),
        description: "".to_string(),
        icon: None,
        rules,
        combination_mode: RuleCombinationMode::Or,
        sort_field: None,
        sort_direction: None,
        display_order: 0,
        created_at: 0,
        updated_at: 0,
    };

    let count = db.count_smart_collection_items(&sc).expect("count items");
    assert_eq!(count, 2); // Billie Jean (Pop) and Yesterday (Beatles + Pop)
}

#[test]
fn test_smart_collection_date_conditions() {
    let (db, _dir) = setup_test_db();
    let _ids = seed_test_media(&db);

    // Never Played
    let mut unplayed_rules = RuleDefinition::new(RuleCombinationMode::And);
    unplayed_rules.add_rule(SmartRule::Date {
        field: RuleField::LastPlayed,
        operator: DateOperator::NeverPlayed,
        timestamp: None,
        days: None,
    });
    let sc = SmartCollection {
        id: 1,
        name: "Unplayed Tracks".to_string(),
        description: "".to_string(),
        icon: None,
        rules: unplayed_rules,
        combination_mode: RuleCombinationMode::And,
        sort_field: None,
        sort_direction: None,
        display_order: 0,
        created_at: 0,
        updated_at: 0,
    };
    let count = db
        .count_smart_collection_items(&sc)
        .expect("count unplayed");
    // 5 total - 1 played (Billie Jean) = 4 unplayed
    assert_eq!(count, 4);
}

#[test]
fn test_sql_injection_defense_and_complexity_rejection() {
    // Malicious SQL injection payloads in user values
    let injection_payload = "'; DROP TABLE media_items; --";

    let mut rules = RuleDefinition::new(RuleCombinationMode::And);
    rules.add_rule(SmartRule::Text {
        field: RuleField::Artist,
        operator: TextOperator::Equals,
        value: injection_payload.to_string(),
    });

    let compiled = compile_smart_rules(&rules).expect("compile rules safely");
    // SQL fragment MUST contain placeholder '?', NOT raw payload
    assert!(!compiled.where_clause.contains("DROP TABLE"));
    assert!(compiled.where_clause.contains("LOWER(?)"));
    assert_eq!(compiled.params.len(), 1);

    // Complexity limit: reject > 20 rules
    let mut oversized = RuleDefinition::new(RuleCombinationMode::And);
    for i in 0..25 {
        oversized.add_rule(SmartRule::Numeric {
            field: RuleField::ReleaseYear,
            operator: NumericOperator::Equals,
            value: 2000.0 + i as f64,
            value_to: None,
        });
    }
    let res = compile_smart_rules(&oversized);
    assert!(res.is_err());
    match res {
        Err(err) => assert!(err.contains("Maximum number of rules exceeded")),
        Ok(_) => panic!("Expected error for oversized rules"),
    }
}
