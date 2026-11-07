//! Thumbnail extraction provider using headless mpv video rendering.

use muvrix_core::thumbnails::{
    calculate_thumbnail_seek_seconds, FallbackThumbnailExtractor, ThumbnailExtractor,
    ThumbnailResult,
};
use std::fs;
use std::path::Path;
use std::process::Command;

/// Thumbnail extraction provider utilizing mpv's image video output driver (`vo=image`).
#[derive(Debug, Default, Clone)]
pub struct MpvThumbnailExtractor;

impl MpvThumbnailExtractor {
    pub fn new() -> Self {
        Self
    }
}

impl ThumbnailExtractor for MpvThumbnailExtractor {
    fn extract_thumbnail(
        &self,
        media_path: &Path,
        duration_ms: u64,
        out_path: &Path,
    ) -> ThumbnailResult<()> {
        let seek_secs = calculate_thumbnail_seek_seconds(duration_ms);
        let pid = std::process::id();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("muvrix_thumb_{}_{}", pid, timestamp));
        let _ = fs::create_dir_all(&temp_dir);

        let media_str = media_path.to_string_lossy().to_string();
        let temp_dir_str = temp_dir.to_string_lossy().to_string();
        let seek_str = format!("{:.2}", seek_secs);

        // Attempt frame extraction via mpv CLI if available in PATH
        let output = Command::new("mpv")
            .arg("--vo=image")
            .arg("--vo-image-format=png")
            .arg(format!("--vo-image-outdir={}", temp_dir_str))
            .arg("--frames=1")
            .arg(format!("--start={}", seek_str))
            .arg("--ao=null")
            .arg("--video-osd=no")
            .arg("--osc=no")
            .arg(&media_str)
            .output();

        let generated_frame = temp_dir.join("00000001.png");

        let success = match output {
            Ok(o) if o.status.success() && generated_frame.is_file() => {
                if let Some(parent) = out_path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                fs::rename(&generated_frame, out_path).is_ok()
            }
            _ => false,
        };

        let _ = fs::remove_dir_all(&temp_dir);

        if success {
            Ok(())
        } else {
            // Graceful fallback to deterministic minimal frame
            let fallback = FallbackThumbnailExtractor;
            fallback.extract_thumbnail(media_path, duration_ms, out_path)
        }
    }
}
