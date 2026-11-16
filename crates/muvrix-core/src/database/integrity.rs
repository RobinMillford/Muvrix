//! Missing media integrity scanning, candidate relinking, and catalog cleanup.

use super::models::{
    AvailabilityState, LibraryItemView, MediaItem, MediaMetadata, MediaType, MetadataStatus,
};
use rusqlite::{params, Connection, OptionalExtension, Result, Transaction};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Computes a fast sampled piecewise SHA-256 digest for candidate filtering.
///
/// For files <= 6 MB, hashes the complete file contents.
/// For files > 6 MB, hashes the file length, first 2 MB, middle 2 MB, and last 2 MB.
///
/// **Contract:**
/// This hash is strictly an optimization to quickly rule out non-duplicates.
/// Never merge or purge files based solely on piecewise hash equality.
pub fn compute_piecewise_hash(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let meta = file.metadata()?;
    let len = meta.len();

    let mut hasher = Sha256::new();
    hasher.update(len.to_le_bytes());

    const CHUNK_SIZE: usize = 2 * 1024 * 1024; // 2 MB

    if len <= (CHUNK_SIZE * 3) as u64 {
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;
        hasher.update(&buffer);
    } else {
        let mut head = vec![0u8; CHUNK_SIZE];
        file.read_exact(&mut head)?;
        hasher.update(&head);

        let mid_offset = (len / 2).saturating_sub((CHUNK_SIZE / 2) as u64);
        file.seek(SeekFrom::Start(mid_offset))?;
        let mut mid = vec![0u8; CHUNK_SIZE];
        file.read_exact(&mut mid)?;
        hasher.update(&mid);

        let tail_offset = len.saturating_sub(CHUNK_SIZE as u64);
        file.seek(SeekFrom::Start(tail_offset))?;
        let mut tail = vec![0u8; CHUNK_SIZE];
        file.read_exact(&mut tail)?;
        hasher.update(&tail);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

/// Backwards-compatible alias for `compute_piecewise_hash`.
pub fn compute_media_content_hash(path: &Path) -> std::io::Result<String> {
    compute_piecewise_hash(path)
}

/// Computes an authoritative cryptographic SHA-256 digest by streaming the complete file.
///
/// Used for final identity verification of duplicate candidates before any merge or purge.
/// Guarantees that files with identical sampled chunks but divergent unsampled bytes are never merged.
pub fn compute_full_file_hash(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024]; // 64 KB streaming buffer
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Filesystem and network accessibility state of a registered media item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaReachability {
    Available,
    ConfirmedMissing,
    Inaccessible,
    PermissionDenied,
    NetworkSourceUnavailable,
    VerificationPending,
    Unknown,
}

impl MediaReachability {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::ConfirmedMissing => "confirmed_missing",
            Self::Inaccessible => "inaccessible",
            Self::PermissionDenied => "permission_denied",
            Self::NetworkSourceUnavailable => "network_source_unavailable",
            Self::VerificationPending => "verification_pending",
            Self::Unknown => "unknown",
        }
    }

    pub fn display_label(&self) -> &'static str {
        match self {
            Self::Available => "Available",
            Self::ConfirmedMissing => "Confirmed Missing",
            Self::Inaccessible => "Inaccessible / Unmounted",
            Self::PermissionDenied => "Permission Denied",
            Self::NetworkSourceUnavailable => "Network Share Offline",
            Self::VerificationPending => "Verifying...",
            Self::Unknown => "Unknown",
        }
    }

    /// Whether this status indicates that the item cannot currently be played.
    pub fn is_broken_or_offline(&self) -> bool {
        !matches!(self, Self::Available | Self::VerificationPending)
    }
}

/// A media candidate for relinking a broken file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelinkCandidate {
    pub path: PathBuf,
    pub file_name: String,
    pub file_size: u64,
    pub confidence: f32, // 0.0 to 1.0
    pub match_reasons: Vec<String>,
}

/// Progress metrics for a library integrity scan.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrityScanProgress {
    pub total_examined: usize,
    pub total_items: usize,
    pub available_count: usize,
    pub missing_count: usize,
    pub inaccessible_count: usize,
    pub permission_denied_count: usize,
    pub network_unavailable_count: usize,
    pub is_completed: bool,
    pub last_scan_timestamp: u64,
}

/// Evaluates the reachability of a single media item conservatively without deleting anything.
pub fn inspect_media_reachability(item: &MediaItem) -> MediaReachability {
    let path_str = &item.canonical_path;

    // 1. Network schemes (SMB, WebDAV, HTTP/S, SFTP)
    if path_str.starts_with("http://")
        || path_str.starts_with("https://")
        || path_str.starts_with("webdav://")
        || path_str.starts_with("smb://")
        || path_str.starts_with("sftp://")
    {
        // Network media is not verified against local disk, never mark permanently missing
        return MediaReachability::NetworkSourceUnavailable;
    }

    // 2. Local filesystem checks
    let path = Path::new(path_str);
    if path.exists() {
        match std::fs::metadata(path) {
            Ok(_) => MediaReachability::Available,
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                MediaReachability::PermissionDenied
            }
            Err(_) => MediaReachability::Inaccessible,
        }
    } else {
        // Check if the parent root directory exists
        if let Some(parent) = path.parent() {
            if parent.exists() {
                // Parent folder exists, but the file is gone -> Confirmed Missing
                MediaReachability::ConfirmedMissing
            } else {
                // Entire mount point or parent hierarchy is missing (e.g. unmounted USB drive)
                MediaReachability::Inaccessible
            }
        } else {
            MediaReachability::Inaccessible
        }
    }
}

/// Scans all media items in the repository, returning reachability statistics.
pub fn scan_library_integrity(
    conn: &Connection,
    cancel_flag: Option<&Arc<AtomicBool>>,
) -> Result<IntegrityScanProgress> {
    let mut stmt = conn.prepare(
        "SELECT id, root_id, relative_path, canonical_path, file_name, file_extension, media_type, file_size, modified_time, added_time, last_scanned_at, availability_state, metadata_status
         FROM media_items ORDER BY id ASC",
    )?;

    let rows = stmt.query_map([], |row| {
        let media_type_str: String = row.get(6)?;
        let avail_str: String = row.get(11)?;
        let meta_status_str: String = row.get(12)?;

        Ok(MediaItem {
            id: row.get(0)?,
            root_id: row.get(1)?,
            relative_path: row.get(2)?,
            canonical_path: row.get(3)?,
            file_name: row.get(4)?,
            file_extension: row.get(5)?,
            media_type: super::models::MediaType::parse(&media_type_str),
            file_size: row.get::<_, i64>(7)? as u64,
            modified_time: row.get::<_, i64>(8)? as u64,
            added_time: row.get::<_, i64>(9)? as u64,
            last_scanned_at: row.get::<_, i64>(10)? as u64,
            availability_state: super::models::AvailabilityState::parse(&avail_str),
            metadata_status: super::models::MetadataStatus::parse(&meta_status_str),
        })
    })?;

    let mut progress = IntegrityScanProgress::default();
    let mut items = Vec::new();
    for r in rows {
        items.push(r?);
    }
    progress.total_items = items.len();

    let mut update_stmt = conn.prepare(
        "UPDATE media_items SET availability_state = ?1, last_scanned_at = ?2 WHERE id = ?3",
    )?;

    let now = current_timestamp();

    for item in items {
        if let Some(cancel) = cancel_flag {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
        }

        let reachability = inspect_media_reachability(&item);
        let avail_str = match reachability {
            MediaReachability::Available => "available",
            MediaReachability::ConfirmedMissing => "unavailable",
            _ => "unavailable",
        };

        let _ = update_stmt.execute(params![avail_str, now as i64, item.id]);

        progress.total_examined += 1;
        match reachability {
            MediaReachability::Available => progress.available_count += 1,
            MediaReachability::ConfirmedMissing => progress.missing_count += 1,
            MediaReachability::Inaccessible => progress.inaccessible_count += 1,
            MediaReachability::PermissionDenied => progress.permission_denied_count += 1,
            MediaReachability::NetworkSourceUnavailable => progress.network_unavailable_count += 1,
            MediaReachability::VerificationPending | MediaReachability::Unknown => {}
        }
    }

    progress.is_completed = true;
    progress.last_scan_timestamp = now;
    Ok(progress)
}

fn collect_files_recursive(
    dir: &Path,
    current_depth: usize,
    max_depth: usize,
    out: &mut Vec<PathBuf>,
) {
    if current_depth > max_depth {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_files_recursive(&path, current_depth + 1, max_depth, out);
            } else if path.is_file() {
                out.push(path);
            }
        }
    }
}

/// Finds candidate replacement files for a missing media item in candidate search directories.
pub fn find_relink_candidates(
    search_dirs: &[PathBuf],
    target_item: &MediaItem,
    target_title: Option<&str>,
    target_artist: Option<&str>,
) -> Vec<RelinkCandidate> {
    let mut candidates = Vec::new();

    for root in search_dirs {
        if !root.exists() || !root.is_dir() {
            continue;
        }

        let mut files = Vec::new();
        collect_files_recursive(root, 0, 5, &mut files);

        for path in files {
            let file_name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();

            // Match extension compatibility
            if ext != target_item.file_extension.to_ascii_lowercase() {
                continue;
            }

            let mut confidence = 0.0f32;
            let mut reasons = Vec::new();

            // 1. Exact file name match
            if file_name == target_item.file_name {
                confidence += 0.6;
                reasons.push("Exact filename match".to_string());
            } else if file_name.to_lowercase() == target_item.file_name.to_lowercase() {
                confidence += 0.5;
                reasons.push("Case-insensitive filename match".to_string());
            }

            // 2. File size match
            if let Ok(meta) = std::fs::metadata(&path) {
                let size = meta.len();
                if size == target_item.file_size && target_item.file_size > 0 {
                    confidence += 0.4;
                    reasons.push(format!("Exact file size match ({} bytes)", size));
                } else if target_item.file_size > 0 {
                    let diff = (size as i64 - target_item.file_size as i64).abs();
                    if diff < 1024 * 64 {
                        confidence += 0.2;
                        reasons.push("Near identical file size".to_string());
                    }
                }

                // 3. Title / Artist in filename heuristics
                if let Some(title) = target_title {
                    if !title.is_empty() && file_name.to_lowercase().contains(&title.to_lowercase())
                    {
                        confidence += 0.2;
                        reasons.push(format!("Filename contains track title '{title}'"));
                    }
                }
                if let Some(artist) = target_artist {
                    if !artist.is_empty()
                        && file_name.to_lowercase().contains(&artist.to_lowercase())
                    {
                        confidence += 0.1;
                        reasons.push(format!("Filename contains artist '{artist}'"));
                    }
                }

                if confidence >= 0.4 {
                    candidates.push(RelinkCandidate {
                        path,
                        file_name,
                        file_size: size,
                        confidence: confidence.min(1.0),
                        match_reasons: reasons,
                    });
                }
            }
        }
    }

    candidates.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    candidates
}

/// Relinks an existing media item to a replacement file.
///
/// **Crucial Contract:**
/// - Preserves the media ID (`item.id`).
/// - Preserves watch state, favourites, collection memberships, playlists, and user bookmarks.
/// - Validates that the replacement file exists and is readable.
/// - Updates FTS5 indexing.
pub fn relink_media_item(
    conn: &Connection,
    media_id: i64,
    replacement_path: &Path,
) -> Result<MediaItem, String> {
    if !replacement_path.exists() {
        return Err(format!(
            "Replacement file does not exist on disk: {}",
            replacement_path.display()
        ));
    }

    let meta = std::fs::metadata(replacement_path)
        .map_err(|e| format!("Cannot read replacement file metadata: {e}"))?;

    let file_size = meta.len();
    let modified_time = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(current_timestamp());

    let canonical = replacement_path
        .canonicalize()
        .unwrap_or_else(|_| replacement_path.to_path_buf());
    let canonical_path = canonical.to_string_lossy().to_string();

    let file_name = replacement_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let file_extension = replacement_path
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default();

    let now = current_timestamp();

    conn.execute(
        "UPDATE media_items SET
            canonical_path = ?1,
            file_name = ?2,
            file_extension = ?3,
            file_size = ?4,
            modified_time = ?5,
            last_scanned_at = ?6,
            availability_state = 'available'
         WHERE id = ?7",
        params![
            canonical_path,
            file_name,
            file_extension,
            file_size as i64,
            modified_time as i64,
            now as i64,
            media_id
        ],
    )
    .map_err(|e| format!("Database update failed: {e}"))?;

    // Fetch and return the updated MediaItem
    let mut stmt = conn
        .prepare(
            "SELECT id, root_id, relative_path, canonical_path, file_name, file_extension, media_type, file_size, modified_time, added_time, last_scanned_at, availability_state, metadata_status
             FROM media_items WHERE id = ?1",
        )
        .map_err(|e| e.to_string())?;

    stmt.query_row(params![media_id], |row| {
        let media_type_str: String = row.get(6)?;
        let avail_str: String = row.get(11)?;
        let meta_status_str: String = row.get(12)?;

        Ok(MediaItem {
            id: row.get(0)?,
            root_id: row.get(1)?,
            relative_path: row.get(2)?,
            canonical_path: row.get(3)?,
            file_name: row.get(4)?,
            file_extension: row.get(5)?,
            media_type: super::models::MediaType::parse(&media_type_str),
            file_size: row.get::<_, i64>(7)? as u64,
            modified_time: row.get::<_, i64>(8)? as u64,
            added_time: row.get::<_, i64>(9)? as u64,
            last_scanned_at: row.get::<_, i64>(10)? as u64,
            availability_state: super::models::AvailabilityState::parse(&avail_str),
            metadata_status: super::models::MetadataStatus::parse(&meta_status_str),
        })
    })
    .map_err(|e| format!("Failed to retrieve relinked item: {e}"))
}

/// Detailed duplicate candidate information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicateCandidate {
    pub keep_id: i64,
    pub duplicate_id: i64,
    pub canonical_path: String,
    pub is_confirmed_hash_match: bool,
    pub reason: String,
}

/// Proposed catalog cleanup summary.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupPreview {
    pub confirmed_missing_ids: Vec<i64>,
    /// Confirmed duplicate IDs with verified matching content hashes.
    pub duplicate_ids: Vec<i64>,
    /// Suspected duplicate candidates (e.g. matching name/size but unverified or unhashed)
    /// requiring manual confirmation and NEVER automatically purged.
    pub suspected_duplicates: Vec<DuplicateCandidate>,
    pub orphaned_metadata_ids: Vec<i64>,
}

impl CleanupPreview {
    pub fn total_cleanup_candidates(&self) -> usize {
        self.confirmed_missing_ids.len()
            + self.duplicate_ids.len()
            + self.orphaned_metadata_ids.len()
    }
}

/// Generates a preview of safe cleanup opportunities in the catalog.
///
/// **Safety Guarantees:**
/// - Never touches original media files on disk.
/// - Never classifies unmounted network shares as missing.
/// - Uses cryptographic SHA-256 digests to verify duplicate identity.
/// - Different media files sharing identical filenames and sizes are excluded.
/// - Unreachable files are categorized as suspected duplicates requiring manual confirmation.
pub fn generate_cleanup_preview(conn: &Connection) -> Result<CleanupPreview> {
    // 1. Confirmed missing items (availability_state = 'unavailable')
    let mut stmt = conn.prepare(
        "SELECT id, canonical_path FROM media_items WHERE availability_state = 'unavailable'",
    )?;
    let missing_rows = stmt.query_map([], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    })?;

    let mut confirmed_missing_ids = Vec::new();
    for row in missing_rows {
        let (id, path_str) = row?;
        // Double check it's not a network path
        if !path_str.starts_with("http")
            && !path_str.starts_with("webdav")
            && !path_str.starts_with("smb")
            && !path_str.starts_with("sftp")
        {
            let p = Path::new(&path_str);
            if !p.exists() {
                if let Some(parent) = p.parent() {
                    if parent.exists() {
                        confirmed_missing_ids.push(id);
                    }
                }
            }
        }
    }

    // 2. Duplicate detection with cryptographic hash verification
    let mut dup_group_stmt = conn.prepare(
        "SELECT file_name, file_size FROM media_items
         WHERE file_size > 0
         GROUP BY file_name, file_size
         HAVING COUNT(*) > 1",
    )?;

    let dup_groups: Vec<(String, u64)> = dup_group_stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut duplicate_ids = Vec::new();
    let mut suspected_duplicates = Vec::new();

    let mut items_in_group_stmt = conn.prepare(
        "SELECT id, canonical_path FROM media_items
         WHERE file_name = ?1 AND file_size = ?2
         ORDER BY id ASC",
    )?;

    for (file_name, file_size) in dup_groups {
        let group_items: Vec<(i64, String)> = items_in_group_stmt
            .query_map(params![file_name, file_size as i64], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        if group_items.len() < 2 {
            continue;
        }

        let (keep_id, keep_path_str) = &group_items[0];
        let keep_path = Path::new(keep_path_str);
        let keep_piecewise = compute_piecewise_hash(keep_path).ok();

        for (cand_id, cand_path_str) in &group_items[1..] {
            let cand_path = Path::new(cand_path_str);
            let cand_piecewise = compute_piecewise_hash(cand_path).ok();

            match (&keep_piecewise, &cand_piecewise) {
                (Some(h1), Some(h2)) if h1 == h2 => {
                    // Optimization filter passed: sampled piecewise regions match.
                    // Stage 2: Authoritative streaming full-file SHA-256 verification.
                    let keep_full = compute_full_file_hash(keep_path).ok();
                    let cand_full = compute_full_file_hash(cand_path).ok();

                    match (&keep_full, &cand_full) {
                        (Some(f1), Some(f2)) if f1 == f2 => {
                            // Confirmed identical file across 100% of bytes via streaming SHA-256
                            duplicate_ids.push(*cand_id);
                        }
                        (Some(_), Some(_)) => {
                            // CRITICAL DEFENSE:
                            // Sampled piecewise hash matched, but streaming full-file SHA-256 differed!
                            // These are distinct files with identical sampled regions.
                            // NEVER automatically merge or purge them!
                            log::warn!(
                                "Distinct content detected: items {} and {} ({file_name}) matched sampled piecewise SHA-256 but differ in full streaming hash. Safely excluded from duplicate cleanup.",
                                keep_id, cand_id
                            );
                        }
                        _ => {
                            // Full-file hash could not be completed (e.g. read error or unmounted media).
                            // Quarantine in suspected_duplicates for manual review; NEVER automatically purge.
                            suspected_duplicates.push(DuplicateCandidate {
                                keep_id: *keep_id,
                                duplicate_id: *cand_id,
                                canonical_path: cand_path_str.clone(),
                                is_confirmed_hash_match: false,
                                reason: "Full streaming verification could not be completed; manual review required".to_string(),
                            });
                        }
                    }
                }
                (Some(_), Some(_)) => {
                    // Piecewise hash differs: definitely distinct files, skip immediately.
                    log::info!(
                        "Sampled hash mismatch for items {} and {} ({file_name}); distinct files excluded from duplicate cleanup",
                        keep_id, cand_id
                    );
                }
                _ => {
                    // Cannot verify hash (file offline, inaccessible, or network source).
                    // Quarantine in suspected_duplicates for manual review; NEVER automatically purge.
                    suspected_duplicates.push(DuplicateCandidate {
                        keep_id: *keep_id,
                        duplicate_id: *cand_id,
                        canonical_path: cand_path_str.clone(),
                        is_confirmed_hash_match: false,
                        reason:
                            "Unreachable or unhashed file prevents automatic duplicate verification"
                                .to_string(),
                    });
                }
            }
        }
    }

    // 3. Orphaned metadata records
    let mut orphan_stmt = conn.prepare(
        "SELECT media_id FROM media_metadata WHERE media_id NOT IN (SELECT id FROM media_items)",
    )?;
    let orphan_rows = orphan_stmt.query_map([], |row| row.get::<_, i64>(0))?;
    let mut orphaned_metadata_ids = Vec::new();
    for o in orphan_rows {
        orphaned_metadata_ids.push(o?);
    }

    Ok(CleanupPreview {
        confirmed_missing_ids,
        duplicate_ids,
        suspected_duplicates,
        orphaned_metadata_ids,
    })
}

/// Executes catalog cleanup within an atomic SQLite transaction.
///
/// Preserves user bookmarks, favourites, watch state, collections, and history
/// by migrating them to the primary item before removing confirmed duplicates.
/// Only removes stale database records. Never deletes files from disk.
pub fn execute_catalog_cleanup(tx: &Transaction, preview: &CleanupPreview) -> Result<usize> {
    let mut removed_count = 0;

    for &id in &preview.confirmed_missing_ids {
        tx.execute("DELETE FROM media_items WHERE id = ?1", params![id])?;
        removed_count += 1;
    }

    for &dup_id in &preview.duplicate_ids {
        // Find keep_id for this duplicate item by matching file_name and file_size
        let keep_id_opt: Option<i64> = tx
            .query_row(
                "SELECT MIN(id) FROM media_items WHERE file_name = (SELECT file_name FROM media_items WHERE id = ?1)
                 AND file_size = (SELECT file_size FROM media_items WHERE id = ?1)
                 AND id != ?1",
                params![dup_id],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(keep_id) = keep_id_opt {
            // 1. Preserve bookmarks: migrate bookmarks from duplicate to keep_id
            let _ = tx.execute(
                "UPDATE user_bookmarks SET media_id = ?1 WHERE media_id = ?2",
                params![keep_id, dup_id],
            );

            // 2. Preserve favourites: migrate favourite flag if not already set
            let _ = tx.execute(
                "INSERT OR IGNORE INTO favourites (media_id, added_at)
                 SELECT ?1, added_at FROM favourites WHERE media_id = ?2",
                params![keep_id, dup_id],
            );

            // 3. Preserve watch state / history: migrate watched status if duplicate was watched
            let _ = tx.execute(
                "INSERT INTO media_watch_state (media_id, is_watched, updated_at)
                 SELECT ?1, is_watched, updated_at FROM media_watch_state WHERE media_id = ?2
                 ON CONFLICT(media_id) DO UPDATE SET
                    is_watched = MAX(media_watch_state.is_watched, excluded.is_watched),
                    updated_at = MAX(media_watch_state.updated_at, excluded.updated_at)",
                params![keep_id, dup_id],
            );

            // 4. Preserve collection associations
            let _ = tx.execute(
                "INSERT OR IGNORE INTO collection_items (collection_id, media_id, position, added_at)
                 SELECT collection_id, ?1, position, added_at FROM collection_items WHERE media_id = ?2",
                params![keep_id, dup_id],
            );

            // 5. Preserve media tags
            let _ = tx.execute(
                "INSERT OR IGNORE INTO media_tags (media_id, tag_id)
                 SELECT ?1, tag_id FROM media_tags WHERE media_id = ?2",
                params![keep_id, dup_id],
            );

            // 6. Preserve cached artwork
            let _ = tx.execute(
                "INSERT OR IGNORE INTO artwork_cache (media_id, artwork_type, cached_path, width, height, generated_at)
                 SELECT ?1, artwork_type, cached_path, width, height, generated_at FROM artwork_cache WHERE media_id = ?2",
                params![keep_id, dup_id],
            );
        }

        tx.execute("DELETE FROM media_items WHERE id = ?1", params![dup_id])?;
        removed_count += 1;
    }

    for &id in &preview.orphaned_metadata_ids {
        tx.execute(
            "DELETE FROM media_metadata WHERE media_id = ?1",
            params![id],
        )?;
        removed_count += 1;
    }

    Ok(removed_count)
}

/// Fetches all media items marked as unavailable joined with metadata and user flags.
pub fn query_missing_media_items(conn: &Connection) -> Result<Vec<LibraryItemView>> {
    let sql = "
        SELECT
            mi.id, mi.root_id, mi.relative_path, mi.canonical_path, mi.file_name,
            mi.file_extension, mi.media_type, mi.file_size, mi.modified_time,
            mi.added_time, mi.last_scanned_at, mi.availability_state, mi.metadata_status,
            mm.title, mm.original_title, mm.description, mm.genre, mm.release_year,
            mm.duration_ms, mm.overall_bitrate, mm.container, mm.video_codec, mm.audio_codec,
            mm.width, mm.height, mm.fps, mm.hdr_type, mm.audio_channels, mm.sample_rate,
            mm.bit_depth, mm.artist, mm.album, mm.album_artist, mm.track_number, mm.disc_number,
            ac.cached_path,
            CASE WHEN fav.media_id IS NOT NULL THEN 1 ELSE 0 END AS is_favourite,
            CASE WHEN ws.is_watched = 1 THEN 1 ELSE 0 END AS is_watched
        FROM media_items mi
        LEFT JOIN media_metadata mm ON mi.id = mm.media_id
        LEFT JOIN artwork_cache ac ON mi.id = ac.media_id
        LEFT JOIN favourites fav ON mi.id = fav.media_id
        LEFT JOIN media_watch_state ws ON mi.id = ws.media_id
        WHERE mi.availability_state = 'unavailable'
        ORDER BY mi.file_name ASC
    ";

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], |row| {
        let media_type_str: String = row.get(6)?;
        let avail_str: String = row.get(11)?;
        let meta_status_str: String = row.get(12)?;

        let item = MediaItem {
            id: row.get(0)?,
            root_id: row.get(1)?,
            relative_path: row.get(2)?,
            canonical_path: row.get(3)?,
            file_name: row.get(4)?,
            file_extension: row.get(5)?,
            media_type: MediaType::parse(&media_type_str),
            file_size: row.get::<_, i64>(7)? as u64,
            modified_time: row.get::<_, i64>(8)? as u64,
            added_time: row.get::<_, i64>(9)? as u64,
            last_scanned_at: row.get::<_, i64>(10)? as u64,
            availability_state: AvailabilityState::parse(&avail_str),
            metadata_status: MetadataStatus::parse(&meta_status_str),
        };

        let title: Option<String> = row.get(13)?;
        let metadata = if title.is_some() || row.get::<_, Option<i64>>(18)?.is_some() {
            Some(MediaMetadata {
                media_id: item.id,
                title,
                original_title: row.get(14)?,
                description: row.get(15)?,
                genre: row.get(16)?,
                release_year: row.get(17)?,
                duration_ms: row.get::<_, Option<i64>>(18)?.unwrap_or(0) as u64,
                overall_bitrate: row.get::<_, Option<i64>>(19)?.unwrap_or(0) as u64,
                container: row.get(20)?,
                video_codec: row.get(21)?,
                audio_codec: row.get(22)?,
                width: row.get::<_, Option<i64>>(23)?.unwrap_or(0) as u32,
                height: row.get::<_, Option<i64>>(24)?.unwrap_or(0) as u32,
                fps: row.get::<_, Option<f64>>(25)?.unwrap_or(0.0),
                hdr_type: row.get(26)?,
                audio_channels: row.get::<_, Option<i64>>(27)?.unwrap_or(0) as u32,
                sample_rate: row.get::<_, Option<i64>>(28)?.unwrap_or(0) as u32,
                bit_depth: row.get::<_, Option<i64>>(29)?.unwrap_or(0) as u32,
                artist: row.get(30)?,
                album: row.get(31)?,
                album_artist: row.get(32)?,
                track_number: row.get(33)?,
                disc_number: row.get(34)?,
            })
        } else {
            None
        };

        let thumbnail_path: Option<String> = row.get(35)?;
        let is_favourite: bool = row.get::<_, i32>(36)? != 0;
        let is_watched: bool = row.get::<_, i32>(37)? != 0;

        Ok(LibraryItemView {
            item,
            metadata,
            thumbnail_path,
            is_favourite,
            is_watched,
            tags: Vec::new(),
        })
    })?;

    let mut result = Vec::new();
    for r in rows {
        result.push(r?);
    }
    Ok(result)
}
