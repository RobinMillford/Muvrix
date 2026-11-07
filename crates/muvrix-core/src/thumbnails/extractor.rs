//! Thumbnail extraction provider abstraction and representative timestamp selection.

use super::cache::{ThumbnailError, ThumbnailResult, MINIMAL_PNG_BYTES};
use std::fs;
use std::path::Path;

/// Selects a representative seek position in seconds to avoid black intro frames.
pub fn calculate_thumbnail_seek_seconds(duration_ms: u64) -> f64 {
    let dur_sec = duration_ms as f64 / 1000.0;
    if dur_sec >= 30.0 {
        3.0
    } else if dur_sec >= 10.0 {
        (dur_sec * 0.15).max(1.0)
    } else if dur_sec > 1.0 {
        0.5
    } else {
        0.0
    }
}

/// Provider interface for capturing representative frames from media files.
pub trait ThumbnailExtractor: Send + Sync {
    /// Extracts a representative video frame or embedded artwork into `out_path`.
    fn extract_thumbnail(
        &self,
        media_path: &Path,
        duration_ms: u64,
        out_path: &Path,
    ) -> ThumbnailResult<()>;
}

/// Lightweight deterministic fallback thumbnail generator.
#[derive(Debug, Default, Clone)]
pub struct FallbackThumbnailExtractor;

impl ThumbnailExtractor for FallbackThumbnailExtractor {
    fn extract_thumbnail(
        &self,
        _media_path: &Path,
        _duration_ms: u64,
        out_path: &Path,
    ) -> ThumbnailResult<()> {
        if let Some(parent) = out_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        fs::write(out_path, MINIMAL_PNG_BYTES).map_err(ThumbnailError::Io)
    }
}
