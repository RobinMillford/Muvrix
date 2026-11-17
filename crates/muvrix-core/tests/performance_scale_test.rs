//! Large-Library Scale and Performance Benchmarks for Phase E3.
//!
//! Evaluates Smart Collection compilation and execution, collection item count queries,
//! paginated catalog retrieval, batch metadata update duration, integrity scanning throughput,
//! and process memory consumption across 10k, 50k, and 100k synthetic media records.

use muvrix_core::database::{
    BatchMetadataRequest, DatabaseManager, FieldAction, LibraryRoot, NumericOperator, Pagination,
    RuleCombinationMode, RuleDefinition, RuleField, SmartCollection, SmartRule, TextOperator,
};
use std::time::Instant;
use tempfile::tempdir;

fn get_process_rss_mb() -> f64 {
    if let Ok(content) = std::fs::read_to_string("/proc/self/status") {
        for line in content.lines() {
            if line.starts_with("VmRSS:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(kb) = parts[1].parse::<f64>() {
                        return kb / 1024.0;
                    }
                }
            }
        }
    }
    0.0
}

struct ScaleBenchmarkResult {
    scale: usize,
    seed_duration_secs: f64,
    collection_eval_duration_ms: f64,
    collection_count_duration_ms: f64,
    paginated_fetch_duration_ms: f64,
    batch_update_duration_ms: f64,
    integrity_scan_throughput_items_per_sec: f64,
    memory_rss_mb: f64,
}

fn run_benchmark_for_scale(scale: usize) -> ScaleBenchmarkResult {
    let dir = tempdir().expect("create temp dir");
    let db_path = dir.path().join("scale_benchmark.db");
    let db = DatabaseManager::open(&db_path).expect("open database");

    // 1. Seed library root
    let root_id = db
        .insert_root(&LibraryRoot {
            id: 0,
            display_name: "Scale Benchmark Root".to_string(),
            root_path: "/media/benchmark".to_string(),
            source_type: "local".to_string(),
            is_recursive: true,
            is_enabled: true,
            exclusion_patterns: Vec::new(),
            last_scanned_at: 1000,
            scan_status: "idle".to_string(),
        })
        .expect("insert root");

    // 2. Bulk insert media items and metadata within a single transaction
    let t_seed_start = Instant::now();
    db.with_transaction(|tx| {
        let mut item_stmt = tx.prepare(
            "INSERT INTO media_items (
                root_id, relative_path, canonical_path, file_name, file_extension,
                media_type, file_size, modified_time, added_time, last_scanned_at,
                availability_state, metadata_status
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        )?;

        let mut meta_stmt = tx.prepare(
            "INSERT INTO media_metadata (
                media_id, title, genre, release_year, duration_ms, overall_bitrate,
                container, video_codec, audio_codec, width, height, fps,
                artist, album, track_number, disc_number
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        )?;

        for i in 1..=scale {
            let is_scifi = (i % 3) == 0;
            let genre = if is_scifi {
                "Sci-Fi"
            } else if i % 2 == 0 {
                "Action"
            } else {
                "Drama"
            };
            let year = 2000 + ((i as i32) % 25);
            let file_name = format!("Media_Item_{:06}.mkv", i);
            let path = format!("/media/benchmark/Media_Item_{:06}.mkv", i);
            let title = format!("Benchmark Title {:06}", i);
            let artist = format!("Director {:03}", i % 50);
            let album = format!("Collection {:03}", i % 20);

            item_stmt.execute(rusqlite::params![
                root_id,
                &file_name,
                &path,
                &file_name,
                "mkv",
                "video",
                (100_000_000 + (i * 1024)) as i64,
                1_700_000_000 + (i as i64),
                1_700_000_000 + (i as i64),
                1_700_000_000,
                "available",
                "extracted"
            ])?;

            let media_id = tx.last_insert_rowid();

            meta_stmt.execute(rusqlite::params![
                media_id,
                &title,
                genre,
                year,
                7_200_000i64,
                8_000_000i64,
                "mkv",
                "h264",
                "aac",
                1920i64,
                1080i64,
                24.0f64,
                &artist,
                &album,
                1i32,
                1i32
            ])?;
        }

        Ok(())
    })
    .expect("seed bulk data");
    let seed_duration_secs = t_seed_start.elapsed().as_secs_f64();

    // 3. Define Smart Collection: Genre contains 'Sci-Fi' AND ReleaseYear >= 2010
    let sc = SmartCollection {
        id: 0,
        name: format!("Sci-Fi Hits {scale}"),
        description: "Benchmark smart collection".to_string(),
        icon: Some("🚀".to_string()),
        rules: RuleDefinition {
            mode: RuleCombinationMode::And,
            rules: vec![
                SmartRule::Text {
                    field: RuleField::Genre,
                    operator: TextOperator::Contains,
                    value: "Sci-Fi".to_string(),
                },
                SmartRule::Numeric {
                    field: RuleField::ReleaseYear,
                    operator: NumericOperator::AtLeast,
                    value: 2010.0,
                    value_to: None,
                },
            ],
        },
        combination_mode: RuleCombinationMode::And,
        sort_field: Some("title".to_string()),
        sort_direction: Some("asc".to_string()),
        display_order: 1,
        created_at: 1000,
        updated_at: 1000,
    };
    let _sc_id = db
        .create_smart_collection(&sc)
        .expect("create smart collection");

    // 4. Measure Collection Count Query
    let t_count_start = Instant::now();
    let count_matches = db
        .count_smart_collection_items(&sc)
        .expect("count smart collection items");
    let collection_count_duration_ms = t_count_start.elapsed().as_secs_f64() * 1000.0;
    assert!(count_matches > 0);

    // 5. Measure Collection Evaluation (Query items)
    let t_eval_start = Instant::now();
    let (eval_items, total_count) = db
        .query_smart_collection_items(
            &sc,
            Pagination {
                offset: 0,
                limit: 50,
            },
        )
        .expect("evaluate smart collection items");
    let collection_eval_duration_ms = t_eval_start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(eval_items.len(), 50);
    assert_eq!(total_count, count_matches);

    // 6. Measure Paginated Fetch (page 2: offset 50, limit 50)
    let t_page_start = Instant::now();
    let (page_items, _) = db
        .query_smart_collection_items(
            &sc,
            Pagination {
                offset: 50,
                limit: 50,
            },
        )
        .expect("fetch page 2");
    let paginated_fetch_duration_ms = t_page_start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(page_items.len(), 50);

    // 7. Measure Batch Metadata Update (batch updating 100 records with FTS5 re-sync)
    let batch_target_ids: Vec<i64> = (1..=100).map(|i| i as i64).collect();
    let batch_req = BatchMetadataRequest {
        genre: FieldAction::Set("Sci-Fi Special Edition".to_string()),
        ..Default::default()
    };
    let t_batch_start = Instant::now();
    let batch_res = db
        .batch_update_metadata(&batch_target_ids, &batch_req)
        .expect("execute batch update");
    let batch_update_duration_ms = t_batch_start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(batch_res.affected_items, 100);

    // 8. Measure Integrity Scanning Throughput
    let t_scan_start = Instant::now();
    let scan_progress = db
        .scan_library_integrity(None)
        .expect("run library integrity scan");
    let scan_duration_secs = t_scan_start.elapsed().as_secs_f64();
    let scan_throughput = if scan_duration_secs > 0.0 {
        scan_progress.total_examined as f64 / scan_duration_secs
    } else {
        0.0
    };

    let memory_rss_mb = get_process_rss_mb();

    ScaleBenchmarkResult {
        scale,
        seed_duration_secs,
        collection_eval_duration_ms,
        collection_count_duration_ms,
        paginated_fetch_duration_ms,
        batch_update_duration_ms,
        integrity_scan_throughput_items_per_sec: scan_throughput,
        memory_rss_mb,
    }
}

#[test]
fn test_synthetic_scale_benchmarks_10k_50k_100k() {
    println!("\n==========================================================================================");
    println!("MUVRIX PHASE E3 — LARGE-LIBRARY SYNTHETIC BENCHMARK RESULTS (10k, 50k, 100k)");
    println!("==========================================================================================");
    println!(
        "{:<8} | {:<10} | {:<12} | {:<12} | {:<12} | {:<12} | {:<14} | {:<10}",
        "Records",
        "Seed Time",
        "Count Query",
        "Eval (50)",
        "Page 2 (50)",
        "Batch (100)",
        "Scan Throughput",
        "RSS Memory"
    );
    println!("------------------------------------------------------------------------------------------");

    for &scale in &[10_000, 50_000, 100_000] {
        let res = run_benchmark_for_scale(scale);
        println!(
            "{:<8} | {:<10.2}s | {:<12.2}ms | {:<12.2}ms | {:<12.2}ms | {:<12.2}ms | {:<14.0}/s | {:<10.1}MB",
            res.scale,
            res.seed_duration_secs,
            res.collection_count_duration_ms,
            res.collection_eval_duration_ms,
            res.paginated_fetch_duration_ms,
            res.batch_update_duration_ms,
            res.integrity_scan_throughput_items_per_sec,
            res.memory_rss_mb
        );

        // Sanity assertions on latency:
        // Count queries and 50-item paginated fetches on indexed 100k records must complete under 500ms in unoptimized debug mode
        assert!(res.collection_count_duration_ms < 500.0);
        assert!(res.collection_eval_duration_ms < 500.0);
        assert!(res.paginated_fetch_duration_ms < 500.0);
        assert!(res.batch_update_duration_ms < 600.0);
    }
    println!("==========================================================================================\n");
}
