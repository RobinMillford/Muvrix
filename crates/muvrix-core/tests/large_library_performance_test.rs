use muvrix_core::database::{
    AvailabilityState, DatabaseManager, LibraryFilter, LibraryRoot, LibrarySortField, MediaItem,
    MediaMetadata, MediaType, MetadataStatus, Pagination, SortDirection,
};
use std::fs;
use std::path::Path;
use std::time::Instant;

fn populate_synthetic_library(
    db: &DatabaseManager,
    root_id: i64,
    start_idx: usize,
    count: usize,
) -> (f64, u64) {
    let t0 = Instant::now();
    for i in start_idx..(start_idx + count) {
        let is_audio = i % 5 == 0;
        let is_4k = i % 3 == 0;
        let (width, height, res_tier) = if is_4k {
            (3840, 2160, "4K")
        } else if i % 2 == 0 {
            (1920, 1080, "1080p")
        } else {
            (1280, 720, "720p")
        };

        let file_name = if is_audio {
            format!("Track_{i:04}_Synthetic_Audio_Song.flac")
        } else {
            format!("Movie_{i:04}_High_Bitrate_Cinema_{res_tier}.mkv")
        };

        let path = format!("/simulated/media/root/subfolder_{}/{file_name}", i % 50);

        let item = MediaItem {
            id: 0,
            root_id,
            relative_path: file_name.clone(),
            canonical_path: path.clone(),
            file_name: file_name.clone(),
            file_extension: if is_audio {
                "flac".to_string()
            } else {
                "mkv".to_string()
            },
            file_size: (10_000_000 + (i as u64 * 123_456)),
            media_type: if is_audio {
                MediaType::Audio
            } else {
                MediaType::Video
            },
            modified_time: 1700000000 + i as u64,
            added_time: 1700000000 + i as u64,
            last_scanned_at: 1700000000 + i as u64,
            availability_state: AvailabilityState::Available,
            metadata_status: MetadataStatus::Extracted,
        };

        let media_id = db.upsert_media_item(&item).expect("upsert item");

        let meta = MediaMetadata {
            media_id,
            title: Some(format!("Synthetic Title {i:04} - Cinematic Edition")),
            original_title: Some(format!("Original {i:04}")),
            description: Some("Automated performance test fixture media metadata".to_string()),
            genre: Some(if is_audio { "Jazz" } else { "Sci-Fi" }.to_string()),
            release_year: Some(2000 + (i % 26) as i32),
            duration_ms: (60_000 + (i as u64 * 1500)),
            overall_bitrate: if is_4k { 25_000_000 } else { 8_000_000 },
            container: Some(if is_audio { "FLAC" } else { "Matroska" }.to_string()),
            video_codec: if is_audio {
                None
            } else {
                Some("HEVC".to_string())
            },
            audio_codec: Some(if is_audio { "FLAC" } else { "AAC" }.to_string()),
            width,
            height,
            fps: 23.976,
            hdr_type: if is_4k {
                Some("HDR10".to_string())
            } else {
                None
            },
            audio_channels: if is_audio { 2 } else { 6 },
            sample_rate: 48000,
            bit_depth: if is_4k { 10 } else { 8 },
            artist: if is_audio {
                Some(format!("Artist {}", i % 20))
            } else {
                None
            },
            album: if is_audio {
                Some(format!("Album {}", i % 10))
            } else {
                None
            },
            album_artist: None,
            track_number: Some((i % 12) as i32 + 1),
            disc_number: Some(1),
        };

        db.upsert_metadata(&meta).expect("upsert metadata");
    }
    let elapsed = t0.elapsed().as_secs_f64();
    (elapsed, count as u64)
}

#[test]
fn test_large_library_performance_at_scale() {
    let temp_db_dir = std::env::temp_dir().join(format!("muvrix_perf_{}", std::process::id()));
    fs::create_dir_all(&temp_db_dir).expect("create temp dir");
    let db_path = temp_db_dir.join("performance_test.db");

    let db = DatabaseManager::open(Path::new(&db_path)).expect("open db");

    let root_id = db
        .insert_root(&LibraryRoot {
            id: 0,
            display_name: "Simulated Root".to_string(),
            root_path: "/simulated/media/root".to_string(),
            source_type: "local".to_string(),
            is_recursive: true,
            is_enabled: true,
            exclusion_patterns: vec![],
            last_scanned_at: 0,
            scan_status: "idle".to_string(),
        })
        .expect("insert root");

    // 1. Benchmark 100 items
    println!("\n=== BENCHMARK: 100 Items ===");
    let (ins_100_time, _) = populate_synthetic_library(&db, root_id, 1, 100);
    println!(
        "100 Items Inserted in: {:.4}s ({:.1} items/sec)",
        ins_100_time,
        100.0 / ins_100_time
    );

    let t_q100 = Instant::now();
    let (page_100, total_100) = db
        .query_library(
            &LibraryFilter::default(),
            LibrarySortField::Title,
            SortDirection::Ascending,
            Pagination {
                offset: 0,
                limit: 24,
            },
        )
        .expect("query 100");
    let q100_ms = t_q100.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(total_100, 100);
    assert_eq!(page_100.len(), 24);
    println!("Query Initial Page (24 of 100): {:.3}ms", q100_ms);

    // 2. Scale to 1,000 items
    println!("\n=== BENCHMARK: 1,000 Items ===");
    let (ins_900_time, _) = populate_synthetic_library(&db, root_id, 101, 900); // 100 + 900 = 1000
    println!(
        "Additional 900 Items Inserted in: {:.4}s ({:.1} items/sec)",
        ins_900_time,
        900.0 / ins_900_time
    );

    let t_q1000 = Instant::now();
    let (page_1000, total_1000) = db
        .query_library(
            &LibraryFilter::default(),
            LibrarySortField::Title,
            SortDirection::Ascending,
            Pagination {
                offset: 0,
                limit: 24,
            },
        )
        .expect("query 1000");
    let q1000_ms = t_q1000.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(total_1000, 1000);
    assert_eq!(page_1000.len(), 24);
    println!("Query Initial Page (24 of 1,000): {:.3}ms", q1000_ms);

    // Search query on 1,000 items
    let t_search1000 = Instant::now();
    let search_filter = LibraryFilter {
        search_query: Some("Synthetic Title 045".to_string()),
        ..Default::default()
    };
    let (search_res, search_count) = db
        .query_library(
            &search_filter,
            LibrarySortField::Title,
            SortDirection::Ascending,
            Pagination {
                offset: 0,
                limit: 24,
            },
        )
        .expect("search 1000");
    let s1000_ms = t_search1000.elapsed().as_secs_f64() * 1000.0;
    assert!(!search_res.is_empty());
    println!(
        "Search (query='Synthetic Title 045', matched={search_count}): {:.3}ms",
        s1000_ms
    );

    // Filter by Resolution Tier (4K UHD)
    let t_filter1000 = Instant::now();
    let res_filter = LibraryFilter {
        resolution_tier: Some("4K".to_string()),
        ..Default::default()
    };
    let (res_items, res_total) = db
        .query_library(
            &res_filter,
            LibrarySortField::Duration,
            SortDirection::Descending,
            Pagination {
                offset: 0,
                limit: 24,
            },
        )
        .expect("filter 4K 1000");
    let f1000_ms = t_filter1000.elapsed().as_secs_f64() * 1000.0;
    assert!(!res_items.is_empty());
    println!(
        "Filter by 4K UHD Tier (matched={res_total}): {:.3}ms",
        f1000_ms
    );

    // 3. Scale to 5,000 items
    println!("\n=== BENCHMARK: 5,000 Items ===");
    let (ins_4000_time, _) = populate_synthetic_library(&db, root_id, 1001, 4000); // 1000 + 4000 = 5000
    println!(
        "Additional 4,000 Items Inserted in: {:.4}s ({:.1} items/sec)",
        ins_4000_time,
        4000.0 / ins_4000_time
    );

    let t_q5000 = Instant::now();
    let (page_5000, total_5000) = db
        .query_library(
            &LibraryFilter::default(),
            LibrarySortField::FileName,
            SortDirection::Ascending,
            Pagination {
                offset: 0,
                limit: 24,
            },
        )
        .expect("query 5000");
    let q5000_ms = t_q5000.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(total_5000, 5000);
    assert_eq!(page_5000.len(), 24);
    println!("Query Initial Page (24 of 5,000): {:.3}ms", q5000_ms);

    // Search query on 5,000 items
    let t_search5000 = Instant::now();
    let search_filter5000 = LibraryFilter {
        search_query: Some("High_Bitrate_Cinema".to_string()),
        ..Default::default()
    };
    let (search_res5000, count5000) = db
        .query_library(
            &search_filter5000,
            LibrarySortField::Title,
            SortDirection::Ascending,
            Pagination {
                offset: 0,
                limit: 24,
            },
        )
        .expect("search 5000");
    let s5000_ms = t_search5000.elapsed().as_secs_f64() * 1000.0;
    assert!(!search_res5000.is_empty());
    println!(
        "Search on 5,000 items (matched={count5000}): {:.3}ms",
        s5000_ms
    );

    // Sort by duration descending on 5,000 items
    let t_sort5000 = Instant::now();
    let (sorted_items, _) = db
        .query_library(
            &LibraryFilter::default(),
            LibrarySortField::Duration,
            SortDirection::Descending,
            Pagination {
                offset: 0,
                limit: 24,
            },
        )
        .expect("sort 5000");
    let sort5000_ms = t_sort5000.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(sorted_items.len(), 24);
    println!("Sort 5,000 items by Duration DESC: {:.3}ms", sort5000_ms);

    // Database file size on disk
    let db_file_size = fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);
    println!(
        "SQLite Disk Footprint for 5,000 items + metadata: {:.2} MB",
        db_file_size as f64 / (1024.0 * 1024.0)
    );

    // Performance assertion targets (< 50ms per interaction)
    assert!(
        q5000_ms < 50.0,
        "Initial page query on 5,000 items must be < 50ms, took {q5000_ms}ms"
    );
    assert!(
        s5000_ms < 50.0,
        "Search query on 5,000 items must be < 50ms, took {s5000_ms}ms"
    );
    assert!(
        sort5000_ms < 50.0,
        "Sort query on 5,000 items must be < 50ms, took {sort5000_ms}ms"
    );

    // Cleanup
    drop(db);
    let _ = fs::remove_dir_all(&temp_db_dir);
}
