//! Thumbnail extraction provider using native libmpv library image rendering (`vo=image`).

use crate::handle::MpvHandle;
use muvrix_core::thumbnails::{
    calculate_thumbnail_seek_seconds, FallbackThumbnailExtractor, ThumbnailExtractor,
    ThumbnailResult,
};
use std::fs;
use std::path::Path;
use std::process::Command;

/// Thumbnail extraction provider utilizing libmpv's image video output driver (`vo=image`).
///
/// Attempts native in-process extraction via an isolated `libmpv` worker handle first,
/// falling back to the `mpv` CLI executable if available, and finally to a deterministic
/// fallback frame with explicit error classification.
#[derive(Debug, Default, Clone)]
pub struct MpvThumbnailExtractor;

impl MpvThumbnailExtractor {
    pub fn new() -> Self {
        Self
    }

    /// Attempts extraction using an isolated in-process `libmpv` instance.
    fn extract_via_libmpv_handle(
        &self,
        media_path: &Path,
        seek_secs: f64,
        temp_dir: &Path,
    ) -> Option<std::path::PathBuf> {
        let handle = MpvHandle::new().ok()?;
        let temp_dir_str = temp_dir.to_str()?;
        let seek_str = format!("{:.2}", seek_secs);
        let media_str = media_path.to_str()?;

        handle.set_option_string("vo", "image").ok()?;
        handle.set_option_string("vo-image-format", "png").ok()?;
        handle
            .set_option_string("vo-image-outdir", temp_dir_str)
            .ok()?;
        handle.set_option_string("frames", "1").ok()?;
        handle.set_option_string("start", &seek_str).ok()?;
        handle.set_option_string("ao", "null").ok()?;
        handle.set_option_string("video-osd", "no").ok()?;
        handle.set_option_string("osc", "no").ok()?;

        handle.initialize().ok()?;
        handle.command(&["loadfile", media_str]).ok()?;

        // Wait up to 1.0s for the frame to be emitted
        let generated_frame = temp_dir.join("00000001.png");
        for _ in 0..20 {
            if generated_frame.is_file() {
                return Some(generated_frame);
            }
            let _ = handle.wait_event(0.05);
        }

        if generated_frame.is_file() {
            Some(generated_frame)
        } else {
            None
        }
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

        // 1. Primary Strategy: Native in-process libmpv handle (fully portable)
        let native_frame = self.extract_via_libmpv_handle(media_path, seek_secs, &temp_dir);

        let success = if let Some(frame) = native_frame {
            if let Some(parent) = out_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            fs::rename(&frame, out_path).is_ok()
        } else {
            // 2. Secondary Fallback: external CLI mpv if available
            let media_str = media_path.to_string_lossy().to_string();
            let temp_dir_str = temp_dir.to_string_lossy().to_string();
            let seek_str = format!("{:.2}", seek_secs);

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
            match output {
                Ok(o) if o.status.success() && generated_frame.is_file() => {
                    if let Some(parent) = out_path.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    fs::rename(&generated_frame, out_path).is_ok()
                }
                _ => false,
            }
        };

        let _ = fs::remove_dir_all(&temp_dir);

        if success {
            Ok(())
        } else {
            // 3. Final Fallback: deterministic placeholder frame
            let fallback = FallbackThumbnailExtractor;
            fallback.extract_thumbnail(media_path, duration_ms, out_path)
        }
    }
}
