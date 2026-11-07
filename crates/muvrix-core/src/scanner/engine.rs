//! Core scanning engine with recursive directory traversal, cycle prevention, and incremental reconciliation.

use super::options::*;
use crate::database::{
    AvailabilityState, DatabaseManager, LibraryRoot, MediaItem, MediaType, MetadataStatus,
};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Final summary statistics produced when a scan terminates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanSummary {
    pub root_id: i64,
    pub items_discovered: u64,
    pub items_indexed: u64,
    pub items_skipped: u64,
    pub items_missing: u64,
    pub errors_count: u64,
    pub is_cancelled: bool,
}

pub struct ScannerService;

impl ScannerService {
    /// Executes a scan against the specified library root.
    pub fn scan_root(
        db: &DatabaseManager,
        root: &LibraryRoot,
        options: &ScanOptions,
        cancel_token: Arc<AtomicBool>,
        progress_tx: Option<&Sender<ScanProgress>>,
    ) -> crate::Result<ScanSummary> {
        let scan_start_time = current_timestamp();
        let _ = db.update_root_status(root.id, "scanning", scan_start_time);

        let mut progress = ScanProgress::new(root.id);
        let mut visited_dirs = HashSet::new();

        let root_path = PathBuf::from(&root.root_path);
        let canonical_root = match root_path.canonicalize() {
            Ok(p) => p,
            Err(_) => {
                let _ = db.update_root_status(root.id, "error", scan_start_time);
                return Ok(ScanSummary {
                    root_id: root.id,
                    items_discovered: 0,
                    items_indexed: 0,
                    items_skipped: 0,
                    items_missing: 0,
                    errors_count: 1,
                    is_cancelled: false,
                });
            }
        };

        visited_dirs.insert(canonical_root.clone());

        // Merge root-specific exclusions with scanner default exclusions
        let mut all_exclusions = options.exclusions.clone();
        for excl in &root.exclusion_patterns {
            if !all_exclusions.contains(excl) {
                all_exclusions.push(excl.clone());
            }
        }

        Self::crawl_directory(
            db,
            root,
            &canonical_root,
            &canonical_root,
            options,
            &all_exclusions,
            &mut visited_dirs,
            &cancel_token,
            &mut progress,
            progress_tx,
            scan_start_time,
        );

        let is_cancelled = cancel_token.load(Ordering::Relaxed);

        // Reconciliation pass: identify media items that no longer exist on disk
        if !is_cancelled {
            if let Ok(existing_items) = db.get_media_items_by_root(root.id) {
                for item in existing_items {
                    if item.last_scanned_at < scan_start_time
                        && item.availability_state != AvailabilityState::Unavailable
                    {
                        let _ = db.update_availability(item.id, AvailabilityState::Unavailable);
                        progress.missing_count += 1;
                    }
                }
            }
        }

        let status = if is_cancelled { "cancelled" } else { "idle" };
        let _ = db.update_root_status(root.id, status, current_timestamp());

        progress.is_finished = true;
        progress.is_cancelled = is_cancelled;
        if let Some(tx) = progress_tx {
            let _ = tx.send(progress.clone());
        }

        Ok(ScanSummary {
            root_id: root.id,
            items_discovered: progress.discovered_count,
            items_indexed: progress.indexed_count,
            items_skipped: progress.skipped_count,
            items_missing: progress.missing_count,
            errors_count: progress.error_count,
            is_cancelled,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn crawl_directory(
        db: &DatabaseManager,
        root: &LibraryRoot,
        canonical_root: &Path,
        current_dir: &Path,
        options: &ScanOptions,
        exclusions: &[String],
        visited_dirs: &mut HashSet<PathBuf>,
        cancel_token: &Arc<AtomicBool>,
        progress: &mut ScanProgress,
        progress_tx: Option<&Sender<ScanProgress>>,
        scan_time: u64,
    ) {
        if cancel_token.load(Ordering::Relaxed) {
            return;
        }

        let entries = match fs::read_dir(current_dir) {
            Ok(iter) => iter,
            Err(_) => {
                progress.error_count += 1;
                return;
            }
        };

        for entry in entries.flatten() {
            if cancel_token.load(Ordering::Relaxed) {
                return;
            }

            let path = entry.path();
            let file_name = entry.file_name().to_string_lossy().to_string();

            // Check exclusion patterns
            if Self::is_excluded(&file_name, exclusions) {
                continue;
            }

            let file_type = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => {
                    progress.error_count += 1;
                    continue;
                }
            };

            if file_type.is_dir() {
                if !options.recursive {
                    continue;
                }

                // Check symlink policy
                if file_type.is_symlink() && !options.follow_symlinks {
                    continue;
                }

                let canonical = match path.canonicalize() {
                    Ok(p) => p,
                    Err(_) => {
                        progress.error_count += 1;
                        continue;
                    }
                };

                // Cycle prevention: only recurse into unvisited directories
                if visited_dirs.insert(canonical.clone()) {
                    Self::crawl_directory(
                        db,
                        root,
                        canonical_root,
                        &canonical,
                        options,
                        exclusions,
                        visited_dirs,
                        cancel_token,
                        progress,
                        progress_tx,
                        scan_time,
                    );
                }
            } else if file_type.is_file() {
                Self::process_file(
                    db,
                    root,
                    canonical_root,
                    &path,
                    &file_name,
                    options,
                    progress,
                    progress_tx,
                    scan_time,
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn process_file(
        db: &DatabaseManager,
        root: &LibraryRoot,
        canonical_root: &Path,
        path: &Path,
        file_name: &str,
        options: &ScanOptions,
        progress: &mut ScanProgress,
        progress_tx: Option<&Sender<ScanProgress>>,
        scan_time: u64,
    ) {
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        let media_type = if options
            .video_extensions
            .iter()
            .any(|v| v.eq_ignore_ascii_case(&ext))
        {
            MediaType::Video
        } else if options
            .audio_extensions
            .iter()
            .any(|a| a.eq_ignore_ascii_case(&ext))
        {
            MediaType::Audio
        } else {
            return; // Not a supported media file
        };

        progress.discovered_count += 1;

        let meta = match fs::metadata(path) {
            Ok(m) => m,
            Err(_) => {
                progress.error_count += 1;
                return;
            }
        };

        let file_size = meta.len();
        let modified_time = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let canonical_path = match path.canonicalize() {
            Ok(p) => p.to_string_lossy().to_string(),
            Err(_) => path.to_string_lossy().to_string(),
        };

        let relative_path = path
            .strip_prefix(canonical_root)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| file_name.to_string());

        // Check for incremental change
        let existing = db
            .get_media_item_by_canonical_path(&canonical_path)
            .unwrap_or(None);

        let mut is_skipped = false;

        if let Some(mut item) = existing {
            if item.file_size == file_size && item.modified_time == modified_time {
                // Item unchanged: update last_scanned_at and restore availability if needed
                item.last_scanned_at = scan_time;
                if item.availability_state != AvailabilityState::Available {
                    item.availability_state = AvailabilityState::Available;
                }
                let _ = db.upsert_media_item(&item);
                progress.skipped_count += 1;
                is_skipped = true;
            } else {
                // File modified: reset metadata to pending
                item.file_size = file_size;
                item.modified_time = modified_time;
                item.last_scanned_at = scan_time;
                item.availability_state = AvailabilityState::Available;
                item.metadata_status = MetadataStatus::Pending;
                let _ = db.upsert_media_item(&item);
                progress.indexed_count += 1;
            }
        } else {
            // New media item discovered
            let item = MediaItem {
                id: 0,
                root_id: root.id,
                relative_path,
                canonical_path,
                file_name: file_name.to_string(),
                file_extension: ext,
                media_type,
                file_size,
                modified_time,
                added_time: scan_time,
                last_scanned_at: scan_time,
                availability_state: AvailabilityState::Available,
                metadata_status: MetadataStatus::Pending,
            };
            let _ = db.upsert_media_item(&item);
            progress.indexed_count += 1;
        }

        if !is_skipped && progress.indexed_count.is_multiple_of(10) {
            progress.current_path = Some(file_name.to_string());
            if let Some(tx) = progress_tx {
                let _ = tx.send(progress.clone());
            }
        }
    }

    fn is_excluded(name: &str, exclusions: &[String]) -> bool {
        for excl in exclusions {
            if excl.starts_with("*.") {
                let suffix = &excl[1..];
                if name.ends_with(suffix) {
                    return true;
                }
            } else if name == excl || name.starts_with(excl) {
                return true;
            }
        }
        false
    }
}
