//! Background library enrichment: metadata, tracks and frame thumbnails.
//!
//! The scanner intentionally only indexes filesystem rows. This worker consumes
//! `Pending` items, uses one reusable headless `MpvMetadataExtractor`, writes
//! metadata/tracks, generates a frame thumbnail, stores it atomically in the
//! thumbnail cache, and upserts the `artwork_cache` row used by the library UI.
//!
//! The worker never touches the UI thread or GL; progress is sent over an mpsc
//! channel and the UI refreshes its query when a batch completes.

use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use muvrix_core::database::{ArtworkRecord, ArtworkSource, DatabaseManager, MetadataStatus};
use muvrix_core::thumbnails::{ThumbnailCache, ThumbnailExtractor};
use muvrix_core::MediaMetadataExtractor;
use muvrix_player::{MpvMetadataExtractor, MpvThumbnailExtractor};

const BATCH_SIZE: usize = 64;

/// Progress emitted after each item and at completion.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnrichmentProgress {
    pub processed: u64,
    pub total: u64,
    pub failed: u64,
    pub thumbnails: u64,
    pub current_file: Option<String>,
    pub finished: bool,
    pub cancelled: bool,
    pub error: Option<String>,
}

/// Result of one enrichment pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnrichmentSummary {
    pub processed: u64,
    pub metadata_extracted: u64,
    pub thumbnails: u64,
    pub failed: u64,
    pub cancelled: bool,
    pub error: Option<String>,
}

/// Enriches all currently pending, available media items.
///
/// `repaint` is optional so the pipeline can be exercised by tests without an
/// egui context. The production UI passes its context to update cards live.
pub fn enrich_pending(
    db: DatabaseManager,
    cache: ThumbnailCache,
    cancel: Arc<AtomicBool>,
    progress_tx: Sender<EnrichmentProgress>,
    repaint: Option<egui::Context>,
) -> EnrichmentSummary {
    let mut summary = EnrichmentSummary::default();
    let total = match db.count_pending_media_items() {
        Ok(total) => total,
        Err(e) => {
            summary.error = Some(format!("count pending media: {e}"));
            send_progress(
                &progress_tx,
                &repaint,
                EnrichmentProgress {
                    finished: true,
                    error: summary.error.clone(),
                    ..Default::default()
                },
            );
            return summary;
        }
    };

    let metadata_extractor = match MpvMetadataExtractor::new() {
        Ok(extractor) => extractor,
        Err(e) => {
            summary.error = Some(format!("metadata engine unavailable: {e}"));
            send_progress(
                &progress_tx,
                &repaint,
                EnrichmentProgress {
                    total,
                    finished: true,
                    error: summary.error.clone(),
                    ..Default::default()
                },
            );
            return summary;
        }
    };
    let thumbnail_extractor = MpvThumbnailExtractor::new();
    let mut progress = EnrichmentProgress {
        total,
        ..Default::default()
    };
    let mut seen = HashSet::new();
    let mut after_id = 0i64;

    loop {
        if cancel.load(Ordering::Acquire) {
            summary.cancelled = true;
            progress.cancelled = true;
            break;
        }
        let items = match db.pending_media_items_after(after_id, BATCH_SIZE) {
            Ok(items) => items,
            Err(e) => {
                summary.error = Some(format!("query pending media: {e}"));
                progress.error = summary.error.clone();
                break;
            }
        };
        if items.is_empty() {
            break;
        }
        after_id = items.last().map(|item| item.id).unwrap_or(after_id);
        let items: Vec<_> = items
            .into_iter()
            .filter(|item| seen.insert(item.id))
            .collect();
        if items.is_empty() {
            continue;
        }

        for item in items {
            if cancel.load(Ordering::Acquire) {
                summary.cancelled = true;
                progress.cancelled = true;
                break;
            }

            progress.current_file = Some(item.file_name.clone());
            let path = Path::new(&item.canonical_path);
            match metadata_extractor.extract_metadata(path, item.id) {
                Ok((metadata, tracks)) => {
                    if let Err(e) = db.upsert_metadata(&metadata) {
                        summary.error = Some(format!("upsert metadata {}: {e}", item.file_name));
                        summary.failed += 1;
                        progress.failed = summary.failed;
                        let _ = db.update_metadata_status(item.id, MetadataStatus::Failed);
                    } else {
                        if let Err(e) = db.replace_tracks(item.id, &tracks) {
                            log::warn!("enrichment: replace tracks for {}: {e}", item.file_name);
                        }
                        let _ = db.update_metadata_status(item.id, MetadataStatus::Extracted);
                        summary.metadata_extracted += 1;

                        if extract_thumbnail(
                            &db,
                            &cache,
                            &thumbnail_extractor,
                            &item,
                            metadata.duration_ms,
                            metadata.width,
                            metadata.height,
                        ) {
                            summary.thumbnails += 1;
                        }
                    }
                }
                Err(e) => {
                    log::warn!("enrichment: metadata failed for {}: {e}", item.file_name);
                    summary.failed += 1;
                    progress.failed = summary.failed;
                    let _ = db.update_metadata_status(item.id, MetadataStatus::Failed);
                }
            }

            summary.processed += 1;
            progress.processed = summary.processed;
            progress.thumbnails = summary.thumbnails;
            // The UI polls the channel, but the callback also makes live card
            // updates possible while the scan is still on screen.
            send_progress(&progress_tx, &repaint, progress.clone());

            if summary.processed.is_multiple_of(8) {
                if let Some(ctx) = &repaint {
                    ctx.request_repaint();
                }
            }
        }
        if summary.cancelled {
            break;
        }
    }

    progress.finished = true;
    progress.error = summary.error.clone();
    send_progress(&progress_tx, &repaint, progress);
    if let Some(ctx) = &repaint {
        ctx.request_repaint();
    }
    summary
}

fn send_progress(
    tx: &Sender<EnrichmentProgress>,
    repaint: &Option<egui::Context>,
    progress: EnrichmentProgress,
) {
    let _ = tx.send(progress);
    if let Some(ctx) = repaint {
        ctx.request_repaint();
    }
}

fn extract_thumbnail(
    db: &DatabaseManager,
    cache: &ThumbnailCache,
    extractor: &MpvThumbnailExtractor,
    item: &muvrix_core::database::MediaItem,
    duration_ms: u64,
    width: u32,
    height: u32,
) -> bool {
    let tmp = std::env::temp_dir().join(format!(
        "muvrix-enrich-{}-{}.png",
        std::process::id(),
        item.id
    ));
    let _ = std::fs::remove_file(&tmp);

    if let Err(e) = extractor.extract_thumbnail(Path::new(&item.canonical_path), duration_ms, &tmp)
    {
        log::warn!("enrichment: thumbnail failed for {}: {e}", item.file_name);
        let _ = std::fs::remove_file(&tmp);
        return false;
    }
    let Ok(bytes) = std::fs::read(&tmp) else {
        let _ = std::fs::remove_file(&tmp);
        return false;
    };
    let (thumb_width, thumb_height) = image::image_dimensions(&tmp).unwrap_or((width, height));
    let _ = std::fs::remove_file(&tmp);

    let cache_key = ThumbnailCache::compute_cache_key(
        &item.canonical_path,
        item.file_size,
        item.modified_time,
        thumb_width.max(1),
        thumb_height.max(1),
    );
    let stored = match cache.store_thumbnail_atomic(&cache_key, &bytes) {
        Ok(path) => path,
        Err(e) => {
            log::warn!("enrichment: cache thumbnail for {}: {e}", item.file_name);
            return false;
        }
    };
    let _ = cache.prune_if_needed();

    let artwork = ArtworkRecord {
        id: 0,
        media_id: item.id,
        cache_key,
        file_path: stored.to_string_lossy().to_string(),
        source_type: ArtworkSource::FrameCapture,
        width: thumb_width,
        height: thumb_height,
        generated_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    };
    match db.upsert_artwork(&artwork) {
        Ok(_) => true,
        Err(e) => {
            log::warn!("enrichment: artwork row for {}: {e}", item.file_name);
            false
        }
    }
}
