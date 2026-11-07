//! Disk thumbnail cache manager, deterministic hashing, and atomic write operations.

use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ThumbnailError {
    #[error("I/O error during thumbnail caching: {0}")]
    Io(#[from] std::io::Error),

    #[error("Thumbnail extraction error: {0}")]
    Extraction(String),

    #[error("Invalid cached image data")]
    InvalidData,
}

pub type ThumbnailResult<T> = std::result::Result<T, ThumbnailError>;

/// Central manager for on-disk thumbnail storage and eviction.
#[derive(Debug, Clone)]
pub struct ThumbnailCache {
    cache_dir: PathBuf,
    max_size_bytes: u64,
}

impl ThumbnailCache {
    /// Initializes a new thumbnail cache at `cache_dir` with a max byte ceiling.
    pub fn new(cache_dir: PathBuf, max_size_bytes: u64) -> Self {
        let _ = fs::create_dir_all(&cache_dir);
        Self {
            cache_dir,
            max_size_bytes,
        }
    }

    /// Default platform-specific cache directory for Muvrix thumbnails.
    pub fn default_cache_dir() -> PathBuf {
        if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
            PathBuf::from(xdg).join("muvrix").join("thumbnails")
        } else if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home)
                .join(".cache")
                .join("muvrix")
                .join("thumbnails")
        } else if let Ok(appdata) = std::env::var("LOCALAPPDATA") {
            PathBuf::from(appdata)
                .join("Muvrix")
                .join("cache")
                .join("thumbnails")
        } else {
            std::env::temp_dir().join("muvrix").join("thumbnails")
        }
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Computes a deterministic SHA-256 cache key based on file metadata and dimensions.
    pub fn compute_cache_key(
        canonical_path: &str,
        file_size: u64,
        modified_time: u64,
        width: u32,
        height: u32,
    ) -> String {
        let mut hasher = Sha256::new();
        let payload = format!(
            "{}|{}|{}|{}x{}",
            canonical_path, file_size, modified_time, width, height
        );
        hasher.update(payload.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Checks if a valid cached thumbnail already exists for `cache_key`.
    pub fn get_cached_thumbnail(&self, cache_key: &str) -> Option<PathBuf> {
        let thumb_path = self.cache_dir.join(format!("{}.png", cache_key));
        if thumb_path.is_file() {
            if let Ok(meta) = fs::metadata(&thumb_path) {
                if meta.len() > 0 {
                    return Some(thumb_path);
                }
            }
        }
        None
    }

    /// Atomically writes thumbnail bytes to the cache directory using `.tmp` and rename.
    pub fn store_thumbnail_atomic(
        &self,
        cache_key: &str,
        image_bytes: &[u8],
    ) -> ThumbnailResult<PathBuf> {
        let _ = fs::create_dir_all(&self.cache_dir);
        let final_path = self.cache_dir.join(format!("{}.png", cache_key));
        let pid = std::process::id();
        let tmp_path = self.cache_dir.join(format!("{}.tmp.{}", cache_key, pid));

        {
            let mut file = File::create(&tmp_path)?;
            file.write_all(image_bytes)?;
            file.flush()?;
        }

        fs::rename(&tmp_path, &final_path)?;
        let _ = self.prune_if_needed();

        Ok(final_path)
    }

    /// Removes a specific cached thumbnail.
    pub fn invalidate(&self, cache_key: &str) -> ThumbnailResult<()> {
        let thumb_path = self.cache_dir.join(format!("{}.png", cache_key));
        if thumb_path.exists() {
            fs::remove_file(thumb_path)?;
        }
        Ok(())
    }

    /// Prunes the oldest thumbnails if total cache size exceeds `max_size_bytes`.
    pub fn prune_if_needed(&self) -> ThumbnailResult<u64> {
        let mut files = Vec::new();
        let mut total_bytes = 0u64;

        if let Ok(entries) = fs::read_dir(&self.cache_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("png") {
                    if let Ok(meta) = p.metadata() {
                        let len = meta.len();
                        total_bytes += len;
                        let mtime = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                        files.push((p, len, mtime));
                    }
                }
            }
        }

        if total_bytes <= self.max_size_bytes {
            return Ok(0);
        }

        // Sort by modification time ascending (oldest first for LRU eviction)
        files.sort_by_key(|(_, _, mtime)| *mtime);

        let mut bytes_freed = 0u64;
        for (path, len, _) in files {
            if total_bytes - bytes_freed <= self.max_size_bytes {
                break;
            }
            if fs::remove_file(path).is_ok() {
                bytes_freed += len;
            }
        }

        Ok(bytes_freed)
    }
}

/// Fallback 1x1 transparent PNG data used for mock testing and fallback rendering.
pub const MINIMAL_PNG_BYTES: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG magic
    0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, // 1x1 px
    0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, // 8-bit RGBA
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, // IDAT
    0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
    0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, // IEND
    0x42, 0x60, 0x82,
];
