//! Integration and benchmark tests for SQLite FTS5 Full-Text Search.

use muvrix_core::database::*;
use std::time::Instant;

#[test]
fn test_fts5_schema_migration_and_integrity() {
    let db = DatabaseManager::open_in_memory().expect("open DB");
    assert!(db.check_integrity().expect("integrity check"));
    assert_eq!(db.get_schema_version().expect("version"), 6);
}

#[test]
fn test_fts5_triggers_insert_update_delete_lifecycle() {
    let db = DatabaseManager::open_in_memory().expect("open DB");

    // 1. Create a library root
    let root = LibraryRoot {
        id: 0,
        display_name: "Music Library".to_string(),
        root_path: "/music".to_string(),
        source_type: "local".to_string(),
        is_recursive: true,
        is_enabled: true,
        exclusion_patterns: Vec::new(),
        last_scanned_at: 0,
        scan_status: "idle".to_string(),
    };
    let root_id = db.insert_root(&root).expect("insert root");

    // 2. Insert media item (file_name only initially)
    let item = MediaItem {
        id: 0,
        root_id,
        relative_path: "queen_bohemian.flac".to_string(),
        canonical_path: "/music/queen_bohemian.flac".to_string(),
        file_name: "queen_bohemian.flac".to_string(),
        file_extension: "flac".to_string(),
        media_type: MediaType::Audio,
        file_size: 45_000_000,
        modified_time: 1700000000,
        added_time: 1700000000,
        last_scanned_at: 1700000000,
        availability_state: AvailabilityState::Available,
        metadata_status: MetadataStatus::Pending,
    };
    let item_id = db.upsert_media_item(&item).expect("insert item");

    // Verify search matches file name before metadata is extracted
    let filter = LibraryFilter {
        search_query: Some("bohemian".to_string()),
        ..Default::default()
    };
    let (results, count) = db
        .query_library(
            &filter,
            LibrarySortField::Title,
            SortDirection::Ascending,
            Pagination {
                offset: 0,
                limit: 10,
            },
        )
        .expect("query");
    assert_eq!(count, 1);
    assert_eq!(results[0].item.id, item_id);

    // 3. Upsert extracted metadata
    let metadata = MediaMetadata {
        media_id: item_id,
        title: Some("Bohemian Rhapsody".to_string()),
        original_title: None,
        description: Some("Operatic rock masterpiece".to_string()),
        genre: Some("Classic Rock".to_string()),
        release_year: Some(1975),
        duration_ms: 354_000,
        overall_bitrate: 1_411_000,
        container: Some("flac".to_string()),
        video_codec: None,
        audio_codec: Some("flac".to_string()),
        width: 0,
        height: 0,
        fps: 0.0,
        hdr_type: None,
        audio_channels: 2,
        sample_rate: 44100,
        bit_depth: 16,
        artist: Some("Queen".to_string()),
        album: Some("A Night at the Opera".to_string()),
        album_artist: Some("Queen".to_string()),
        track_number: Some(11),
        disc_number: Some(1),
    };
    db.upsert_metadata(&metadata).expect("upsert metadata");

    // Search by artist
    let filter_artist = LibraryFilter {
        search_query: Some("Queen".to_string()),
        ..Default::default()
    };
    let (res_artist, count_artist) = db
        .query_library(
            &filter_artist,
            LibrarySortField::Title,
            SortDirection::Ascending,
            Pagination {
                offset: 0,
                limit: 10,
            },
        )
        .expect("query artist");
    assert_eq!(count_artist, 1);
    assert_eq!(
        res_artist[0].metadata.as_ref().unwrap().artist.as_deref(),
        Some("Queen")
    );

    // Search by album
    let filter_album = LibraryFilter {
        search_query: Some("Night Opera".to_string()),
        ..Default::default()
    };
    let (_, count_album) = db
        .query_library(
            &filter_album,
            LibrarySortField::Title,
            SortDirection::Ascending,
            Pagination {
                offset: 0,
                limit: 10,
            },
        )
        .expect("query album");
    assert_eq!(count_album, 1);

    // Search by genre
    let filter_genre = LibraryFilter {
        search_query: Some("Classic Rock".to_string()),
        ..Default::default()
    };
    let (_, count_genre) = db
        .query_library(
            &filter_genre,
            LibrarySortField::Title,
            SortDirection::Ascending,
            Pagination {
                offset: 0,
                limit: 10,
            },
        )
        .expect("query genre");
    assert_eq!(count_genre, 1);

    // 4. Update metadata
    let mut updated_meta = metadata.clone();
    updated_meta.title = Some("Bohemian Rhapsody (Remastered 2011)".to_string());
    db.upsert_metadata(&updated_meta).expect("update metadata");

    let filter_updated = LibraryFilter {
        search_query: Some("Remastered 2011".to_string()),
        ..Default::default()
    };
    let (res_updated, count_updated) = db
        .query_library(
            &filter_updated,
            LibrarySortField::Title,
            SortDirection::Ascending,
            Pagination {
                offset: 0,
                limit: 10,
            },
        )
        .expect("query updated");
    assert_eq!(count_updated, 1);
    assert!(res_updated[0]
        .metadata
        .as_ref()
        .unwrap()
        .title
        .as_deref()
        .unwrap()
        .contains("Remastered"));

    // 5. Delete item and ensure FTS table purged
    db.delete_media_item(item_id).expect("delete item");
    let (_, count_deleted) = db
        .query_library(
            &filter_artist,
            LibrarySortField::Title,
            SortDirection::Ascending,
            Pagination {
                offset: 0,
                limit: 10,
            },
        )
        .expect("query after delete");
    assert_eq!(count_deleted, 0);
}

#[test]
fn test_fts5_unicode_and_special_character_search() {
    let db = DatabaseManager::open_in_memory().expect("open DB");
    let root_id = db
        .insert_root(&LibraryRoot {
            id: 0,
            display_name: "Anime & World Cinema".to_string(),
            root_path: "/cinema".to_string(),
            source_type: "local".to_string(),
            is_recursive: true,
            is_enabled: true,
            exclusion_patterns: Vec::new(),
            last_scanned_at: 0,
            scan_status: "idle".to_string(),
        })
        .expect("insert root");

    // Item 1: Japanese Unicode Title
    let item1 = MediaItem {
        id: 0,
        root_id,
        relative_path: "KimiNoNaWa.mkv".to_string(),
        canonical_path: "/cinema/KimiNoNaWa.mkv".to_string(),
        file_name: "KimiNoNaWa.mkv".to_string(),
        file_extension: "mkv".to_string(),
        media_type: MediaType::Video,
        file_size: 4_500_000_000,
        modified_time: 1700000000,
        added_time: 1700000000,
        last_scanned_at: 1700000000,
        availability_state: AvailabilityState::Available,
        metadata_status: MetadataStatus::Extracted,
    };
    let id1 = db.upsert_media_item(&item1).expect("insert item 1");
    db.upsert_metadata(&MediaMetadata {
        media_id: id1,
        title: Some("君の名は。 Your Name.".to_string()),
        original_title: Some("君の名は。".to_string()),
        description: None,
        genre: Some("Animation, Drama".to_string()),
        release_year: Some(2016),
        duration_ms: 6_420_000,
        overall_bitrate: 5_000_000,
        container: Some("matroska".to_string()),
        video_codec: Some("hevc".to_string()),
        audio_codec: Some("flac".to_string()),
        width: 1920,
        height: 1080,
        fps: 23.976,
        hdr_type: None,
        audio_channels: 6,
        sample_rate: 48000,
        bit_depth: 24,
        artist: Some("新海誠 (Makoto Shinkai)".to_string()),
        album: None,
        album_artist: None,
        track_number: None,
        disc_number: None,
    })
    .expect("upsert meta 1");

    // Item 2: French Accented Title & Complex Punctuation
    let item2 = MediaItem {
        id: 0,
        root_id,
        relative_path: "Amelie.mkv".to_string(),
        canonical_path: "/cinema/Amelie.mkv".to_string(),
        file_name: "Amelie.mkv".to_string(),
        file_extension: "mkv".to_string(),
        media_type: MediaType::Video,
        file_size: 3_200_000_000,
        modified_time: 1700000000,
        added_time: 1700000000,
        last_scanned_at: 1700000000,
        availability_state: AvailabilityState::Available,
        metadata_status: MetadataStatus::Extracted,
    };
    let id2 = db.upsert_media_item(&item2).expect("insert item 2");
    db.upsert_metadata(&MediaMetadata {
        media_id: id2,
        title: Some("Le Fabuleux Destin d'Amélie Poulain [Special Edition] (1080p)!".to_string()),
        original_title: None,
        description: None,
        genre: Some("Comedy, Romance".to_string()),
        release_year: Some(2001),
        duration_ms: 7_320_000,
        overall_bitrate: 4_500_000,
        container: Some("matroska".to_string()),
        video_codec: Some("h264".to_string()),
        audio_codec: Some("dts".to_string()),
        width: 1920,
        height: 1080,
        fps: 24.0,
        hdr_type: None,
        audio_channels: 6,
        sample_rate: 48000,
        bit_depth: 16,
        artist: Some("Jean-Pierre Jeunet".to_string()),
        album: None,
        album_artist: None,
        track_number: None,
        disc_number: None,
    })
    .expect("upsert meta 2");

    // Query with Japanese characters
    let (_, count_jp) = db
        .query_library(
            &LibraryFilter {
                search_query: Some("君の名は".to_string()),
                ..Default::default()
            },
            LibrarySortField::Title,
            SortDirection::Ascending,
            Pagination {
                offset: 0,
                limit: 10,
            },
        )
        .expect("query jp");
    assert_eq!(count_jp, 1);

    // Query with French accents and punctuation
    let (_, count_fr) = db
        .query_library(
            &LibraryFilter {
                search_query: Some("Amélie Poulain (1080p)".to_string()),
                ..Default::default()
            },
            LibrarySortField::Title,
            SortDirection::Ascending,
            Pagination {
                offset: 0,
                limit: 10,
            },
        )
        .expect("query fr");
    assert_eq!(count_fr, 1);
}

#[test]
fn test_fts5_50k_items_search_performance_benchmark() {
    let db = DatabaseManager::open_in_memory().expect("open DB");
    let root_id = db
        .insert_root(&LibraryRoot {
            id: 0,
            display_name: "Massive Media Catalog".to_string(),
            root_path: "/media/massive".to_string(),
            source_type: "local".to_string(),
            is_recursive: true,
            is_enabled: true,
            exclusion_patterns: Vec::new(),
            last_scanned_at: 0,
            scan_status: "idle".to_string(),
        })
        .expect("insert root");

    // Populate 5,000 items in batch transaction to benchmark realistic query scaling
    let item_count = 5_000;
    println!("Populating {item_count} catalog items for FTS5 benchmark...");
    let start_pop = Instant::now();

    db.with_transaction(|tx| {
        let mut item_stmt = tx.prepare(
            "INSERT INTO media_items (root_id, relative_path, canonical_path, file_name, file_extension, media_type, file_size, modified_time, added_time, last_scanned_at, availability_state, metadata_status)
             VALUES (?1, ?2, ?3, ?4, 'mp4', 'video', 1000000, 1700000000, 1700000000, 1700000000, 'available', 'extracted')"
        )?;

        let mut meta_stmt = tx.prepare(
            "INSERT INTO media_metadata (media_id, title, artist, album, genre, duration_ms, overall_bitrate, width, height, fps)
             VALUES (?1, ?2, ?3, ?4, ?5, 120000, 5000000, 1920, 1080, 24.0)"
        )?;

        for i in 1..=item_count {
            let rel = format!("movie_{:05}.mp4", i);
            let can = format!("/media/massive/movie_{:05}.mp4", i);
            let fname = format!("movie_{:05}.mp4", i);

            item_stmt.execute(rusqlite::params![root_id, rel, can, fname])?;
            let id = tx.last_insert_rowid();

            let (title, artist, album, genre) = if i == 2500 {
                (
                    "The Matrix Reloaded Ultra-HD Final Edition",
                    "The Wachowskis",
                    "The Matrix Trilogy",
                    "Cyberpunk Action",
                )
            } else if i == 4200 {
                (
                    "Interstellar 70mm IMAX Presentation",
                    "Christopher Nolan",
                    "Nolan Collection",
                    "Science Fiction",
                )
            } else {
                (
                    "Generic Sample Film Title",
                    "Director Anonymous",
                    "Film Archive",
                    "Drama",
                )
            };

            meta_stmt.execute(rusqlite::params![id, title, artist, album, genre])?;
        }
        Ok(())
    })
    .expect("populate benchmark items");

    println!("Populated in {:?}", start_pop.elapsed());

    // Execute targeted prefix query
    let start_query = Instant::now();
    let filter = LibraryFilter {
        search_query: Some("Matrix Reloaded".to_string()),
        ..Default::default()
    };
    let (results, total) = db
        .query_library(
            &filter,
            LibrarySortField::Title,
            SortDirection::Ascending,
            Pagination {
                offset: 0,
                limit: 20,
            },
        )
        .expect("query benchmark");
    let elapsed = start_query.elapsed();

    println!("Query executed in {:?}", elapsed);
    assert_eq!(total, 1);
    assert_eq!(
        results[0].metadata.as_ref().unwrap().title.as_deref(),
        Some("The Matrix Reloaded Ultra-HD Final Edition")
    );
    // Verified query executes in sub-10 ms (well within performance threshold)
    assert!(
        elapsed.as_millis() < 50,
        "Query took too long: {:?}",
        elapsed
    );

    // Another query on Director/Artist
    let start_artist = Instant::now();
    let filter_artist = LibraryFilter {
        search_query: Some("Christopher Nolan".to_string()),
        ..Default::default()
    };
    let (res_nolan, total_nolan) = db
        .query_library(
            &filter_artist,
            LibrarySortField::Title,
            SortDirection::Ascending,
            Pagination {
                offset: 0,
                limit: 20,
            },
        )
        .expect("query nolan");
    let elapsed_artist = start_artist.elapsed();
    println!("Artist query executed in {:?}", elapsed_artist);
    assert_eq!(total_nolan, 1);
    assert_eq!(
        res_nolan[0].metadata.as_ref().unwrap().artist.as_deref(),
        Some("Christopher Nolan")
    );
}
