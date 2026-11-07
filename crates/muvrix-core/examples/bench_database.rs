//! Reproducible benchmark for SQLite Media Library at 1,000, 10,000, and 50,000 items.

use muvrix_core::database::{
    AvailabilityState, DatabaseManager, LibraryFilter, LibraryRoot, LibrarySortField, MediaItem,
    MediaMetadata, MediaType, MetadataStatus, Pagination, SortDirection,
};
use std::time::Instant;

fn run_scale_benchmark(db: &DatabaseManager, count: usize) {
    println!("\n=======================================================");
    println!("BENCHMARK SCALE: {} ENTRIES", count);
    println!("=======================================================");

    let root_id = db
        .insert_root(&LibraryRoot {
            id: 0,
            display_name: format!("BenchRoot_{}", count),
            root_path: format!("/media/bench_{}", count),
            source_type: "local".to_string(),
            is_recursive: true,
            is_enabled: true,
            exclusion_patterns: vec![],
            last_scanned_at: 1700000000,
            scan_status: "idle".to_string(),
        })
        .unwrap();

    let start_insert = Instant::now();
    for i in 1..=count {
        let is_video = i % 5 != 0;
        let item_id = db
            .upsert_media_item(&MediaItem {
                id: 0,
                root_id,
                relative_path: format!("folder_{}/movie_{}.mp4", i / 100, i),
                canonical_path: format!(
                    "/media/bench_{}/folder_{}/movie_{}.mp4",
                    count,
                    i / 100,
                    i
                ),
                file_name: format!("movie_{}.mp4", i),
                file_extension: if is_video {
                    "mp4".to_string()
                } else {
                    "flac".to_string()
                },
                media_type: if is_video {
                    MediaType::Video
                } else {
                    MediaType::Audio
                },
                file_size: (i as u64) * 1024 * 1024,
                modified_time: 1700000000 + (i as u64),
                added_time: 1700000000 + (i as u64),
                last_scanned_at: 1700000000,
                availability_state: AvailabilityState::Available,
                metadata_status: MetadataStatus::Extracted,
            })
            .unwrap();

        db.upsert_metadata(&MediaMetadata {
            media_id: item_id,
            title: Some(format!("Sci-Fi Feature Movie {}", i)),
            original_title: None,
            description: Some(
                "Benchmark synthetic description for performance profiling.".to_string(),
            ),
            genre: Some(if i % 2 == 0 {
                "Sci-Fi".to_string()
            } else {
                "Drama".to_string()
            }),
            release_year: Some(2020 + (i % 6) as i32),
            duration_ms: ((i % 180) + 20) as u64 * 60 * 1000,
            overall_bitrate: 5_000_000,
            container: Some("mp4".to_string()),
            video_codec: if is_video {
                Some("h264".to_string())
            } else {
                None
            },
            audio_codec: Some(if is_video {
                "aac".to_string()
            } else {
                "flac".to_string()
            }),
            width: if i % 10 == 0 { 3840 } else { 1920 },
            height: if i % 10 == 0 { 2160 } else { 1080 },
            fps: 30.0,
            hdr_type: None,
            audio_channels: 2,
            sample_rate: 48000,
            bit_depth: 16,
            artist: None,
            album: None,
            album_artist: None,
            track_number: None,
            disc_number: None,
        })
        .unwrap();

        if i % 3 == 0 {
            db.set_favourite(item_id, true).unwrap();
        }
    }
    let insert_dur = start_insert.elapsed();
    println!("Populated {} entries in {:?}", count, insert_dur);

    // Benchmarks: 5 warm-ups, 20 measured iterations
    bench_query("1. Paginated Title ASC (page 1, limit 24)", || {
        db.query_library(
            &LibraryFilter::default(),
            LibrarySortField::Title,
            SortDirection::Ascending,
            Pagination {
                offset: 0,
                limit: 24,
            },
        )
    });

    bench_query(
        "2. Paginated Duration DESC (deep page offset 500, limit 24)",
        || {
            db.query_library(
                &LibraryFilter::default(),
                LibrarySortField::Duration,
                SortDirection::Descending,
                Pagination {
                    offset: 500.min((count.saturating_sub(24)) as u64),
                    limit: 24,
                },
            )
        },
    );

    bench_query("3. Search query matching 'Movie 42'", || {
        db.query_library(
            &LibraryFilter {
                search_query: Some("Movie 42".to_string()),
                ..Default::default()
            },
            LibrarySortField::Title,
            SortDirection::Ascending,
            Pagination {
                offset: 0,
                limit: 24,
            },
        )
    });

    bench_query("4. Multi-Criteria: Favourites + 4K UHD", || {
        db.query_library(
            &LibraryFilter {
                favourites_only: true,
                resolution_tier: Some("4K".to_string()),
                ..Default::default()
            },
            LibrarySortField::FileSize,
            SortDirection::Descending,
            Pagination {
                offset: 0,
                limit: 24,
            },
        )
    });

    bench_query("5. Single Item Full Metadata & Track Fetch", || {
        let id = (count / 2) as i64;
        let _ = db.get_media_item_by_id(id);
        let _ = db.get_metadata(id);
        let _ = db.get_tracks(id);
        Ok::<(), rusqlite::Error>(())
    });
}

fn bench_query<F, R, E>(name: &str, mut f: F)
where
    F: FnMut() -> Result<R, E>,
{
    // 5 warm-up iterations
    for _ in 0..5 {
        let _ = f();
    }

    // 20 measured iterations
    let mut times_us = Vec::with_capacity(20);
    for _ in 0..20 {
        let t0 = Instant::now();
        let _ = f();
        times_us.push(t0.elapsed().as_micros() as f64);
    }

    times_us.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median_ms = times_us[times_us.len() / 2] / 1000.0;
    let p95_idx = ((times_us.len() as f64) * 0.95).floor() as usize;
    let p95_ms = times_us[p95_idx.min(times_us.len() - 1)] / 1000.0;

    println!(
        "{: <55} | Median: {:>6.2} ms | P95: {:>6.2} ms",
        name, median_ms, p95_ms
    );
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Muvrix Database & Indexing Scalability Benchmark");
    println!("OS: Fedora 45 | Kernel: 7.2.15 | Rustc: 1.99.0 | SQLite: 3 (WAL)");

    let temp_db_file = std::env::temp_dir().join("muvrix_bench_scale.db");
    if temp_db_file.exists() {
        let _ = std::fs::remove_file(&temp_db_file);
    }

    let db = DatabaseManager::open(&temp_db_file)?;

    run_scale_benchmark(&db, 1_000);
    run_scale_benchmark(&db, 10_000);
    run_scale_benchmark(&db, 50_000);

    let db_size = std::fs::metadata(&temp_db_file)
        .map(|m| m.len())
        .unwrap_or(0);
    println!(
        "\nFinal Database Size on Disk (50k rows): {:.2} MB",
        db_size as f64 / (1024.0 * 1024.0)
    );

    let _ = std::fs::remove_file(&temp_db_file);
    Ok(())
}
