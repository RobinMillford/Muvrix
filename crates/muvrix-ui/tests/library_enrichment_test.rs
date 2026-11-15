use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use muvrix_core::database::{
    DatabaseManager, LibraryFilter, LibraryRoot, LibrarySortField, MetadataStatus, Pagination,
    SortDirection,
};
use muvrix_core::scanner::{ScanOptions, ScannerService};
use muvrix_core::thumbnails::ThumbnailCache;
use muvrix_ui::library_enrichment::enrich_pending;

#[test]
fn scanned_media_is_enriched_and_visible_in_library_query() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let base =
        std::env::temp_dir().join(format!("muvrix-enrichment-{}-{nonce}", std::process::id()));
    let media_dir = base.join("media");
    std::fs::create_dir_all(&media_dir).expect("create temp media dir");
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/test_video.mp4");
    let media_path = media_dir.join("test_video.mp4");
    std::fs::copy(&fixture, &media_path).expect("copy media fixture");

    let db = DatabaseManager::open(&base.join("library.db")).expect("open database");
    let root_id = db
        .insert_root(&LibraryRoot {
            id: 0,
            display_name: "Test Library".to_string(),
            root_path: media_dir.to_string_lossy().to_string(),
            source_type: "local".to_string(),
            is_recursive: true,
            is_enabled: true,
            exclusion_patterns: Vec::new(),
            last_scanned_at: 0,
            scan_status: "idle".to_string(),
        })
        .expect("insert root");
    let root = db
        .get_root_by_id(root_id)
        .expect("get root")
        .expect("root exists");

    let scan = ScannerService::scan_root(
        &db,
        &root,
        &ScanOptions::default(),
        Arc::new(AtomicBool::new(false)),
        None,
    )
    .expect("scan");
    assert_eq!(scan.items_indexed, 1);
    let pending = db.pending_media_items(10).expect("pending items");
    assert_eq!(pending.len(), 1);
    let media_id = pending[0].id;

    let cache = ThumbnailCache::new(base.join("thumbnails"), 64 * 1024 * 1024);
    let (tx, _rx) = mpsc::channel();
    let summary = enrich_pending(
        db.clone(),
        cache,
        Arc::new(AtomicBool::new(false)),
        tx,
        None,
    );

    assert_eq!(summary.processed, 1, "all discovered items were processed");
    assert_eq!(
        summary.metadata_extracted, 1,
        "metadata extraction succeeded"
    );
    assert_eq!(summary.thumbnails, 1, "thumbnail extraction succeeded");
    assert_eq!(summary.failed, 0);

    let item = db
        .get_media_item_by_id(media_id)
        .expect("read item")
        .expect("item exists");
    assert_eq!(item.metadata_status, MetadataStatus::Extracted);
    let metadata = db
        .get_metadata(media_id)
        .expect("read metadata")
        .expect("metadata exists");
    assert!(metadata.duration_ms > 0);
    assert!(metadata.width > 0);
    assert!(metadata.height > 0);
    assert!(!db.get_tracks(media_id).expect("read tracks").is_empty());

    let artwork = db
        .get_artwork(media_id)
        .expect("read artwork")
        .expect("artwork exists");
    assert!(std::path::Path::new(&artwork.file_path).is_file());

    let (items, total) = db
        .query_library(
            &LibraryFilter::default(),
            LibrarySortField::default(),
            SortDirection::default(),
            Pagination::default(),
        )
        .expect("query library");
    assert_eq!(total, 1);
    let view = items.first().expect("library row");
    assert!(view.metadata.is_some());
    assert_eq!(
        view.thumbnail_path.as_deref(),
        Some(artwork.file_path.as_str())
    );

    let _ = std::fs::remove_dir_all(&base);
}
