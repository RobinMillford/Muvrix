//! Headless mpv engine metadata extraction provider.

use crate::error::MpvError;
use crate::event::Event;
use crate::handle::MpvHandle;
use muvrix_core::database::{MediaMetadata, MediaTrack, TrackType};
use muvrix_core::metadata::{MediaMetadataExtractor, MetadataError, MetadataResult};
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

/// Metadata extraction provider backed by a headless libmpv instance.
pub struct MpvMetadataExtractor {
    handle: Mutex<MpvHandle>,
}

impl MpvMetadataExtractor {
    /// Creates a new headless mpv metadata extraction provider.
    pub fn new() -> Result<Self, MpvError> {
        let handle = MpvHandle::new()?;
        handle.set_option_string("vo", "null")?;
        handle.set_option_string("ao", "null")?;
        handle.set_option_string("idle", "yes")?;
        handle.set_option_string("video-osd", "no")?;
        handle.set_option_string("osc", "no")?;
        handle.set_option_string("load-scripts", "no")?;
        handle.initialize()?;

        Ok(Self {
            handle: Mutex::new(handle),
        })
    }
}

impl MediaMetadataExtractor for MpvMetadataExtractor {
    fn extract_metadata(
        &self,
        path: &Path,
        media_id: i64,
    ) -> MetadataResult<(MediaMetadata, Vec<MediaTrack>)> {
        let handle = self.handle.lock().map_err(|_| {
            MetadataError::Engine("Failed to acquire metadata engine lock".to_string())
        })?;

        let path_str = path.to_string_lossy().to_string();
        let _ = handle.set_property("pause", true);
        handle
            .command(&["loadfile", &path_str, "replace"])
            .map_err(|e| MetadataError::Engine(format!("Failed to loadfile in engine: {}", e)))?;

        // Wait for FileLoaded or timeout (2.0s ceiling)
        let start = Instant::now();
        let timeout = std::time::Duration::from_millis(2000);
        let mut loaded = false;

        while start.elapsed() < timeout {
            match handle.wait_event(0.05) {
                Event::FileLoaded | Event::PlaybackRestart => {
                    loaded = true;
                    break;
                }
                Event::EndFile { .. } => {
                    let _ = handle.command(&["stop"]);
                    return Err(MetadataError::UnsupportedFormat(
                        "File playback ended immediately without load".to_string(),
                    ));
                }
                _ => {}
            }
        }

        if !loaded {
            let _ = handle.command(&["stop"]);
            return Err(MetadataError::Timeout(
                "Timed out waiting for file-loaded event from engine".to_string(),
            ));
        }

        // Query container and format properties
        let duration_sec = handle.get_property::<f64>("duration").unwrap_or(0.0);
        let duration_ms = (duration_sec * 1000.0).max(0.0) as u64;

        let title = handle
            .get_property_string("media-title")
            .ok()
            .or_else(|| path.file_stem().map(|s| s.to_string_lossy().to_string()));

        let width = handle.get_property::<i64>("width").unwrap_or(0).max(0) as u32;
        let height = handle.get_property::<i64>("height").unwrap_or(0).max(0) as u32;

        let video_codec = handle.get_property_string("video-format").ok();
        let audio_codec = handle.get_property_string("audio-codec-name").ok();

        let fps = handle
            .get_property::<f64>("container-fps")
            .or_else(|_| handle.get_property::<f64>("estimated-vf-fps"))
            .unwrap_or(0.0);

        let overall_bitrate = handle
            .get_property::<i64>("video-bitrate")
            .unwrap_or(0)
            .max(0) as u64;

        let audio_channels = handle
            .get_property::<i64>("audio-params/channels")
            .unwrap_or(0)
            .max(0) as u32;

        let sample_rate = handle
            .get_property::<i64>("audio-params/samplerate")
            .unwrap_or(0)
            .max(0) as u32;

        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        // Extract metadata tags if present
        let artist = handle.get_property_string("metadata/by-key/artist").ok();
        let album = handle.get_property_string("metadata/by-key/album").ok();
        let album_artist = handle
            .get_property_string("metadata/by-key/album_artist")
            .ok();
        let genre = handle.get_property_string("metadata/by-key/genre").ok();
        let release_year = handle
            .get_property_string("metadata/by-key/date")
            .ok()
            .and_then(|s| s.chars().take(4).collect::<String>().parse::<i32>().ok());

        // Extract tracks
        let track_count = handle
            .get_property::<i64>("track-list/count")
            .unwrap_or(0)
            .max(0);

        let mut tracks = Vec::new();
        for i in 0..track_count {
            let track_type_str = handle
                .get_property_string(&format!("track-list/{}/type", i))
                .unwrap_or_default();
            let track_type = TrackType::parse(&track_type_str);

            let track_id = handle
                .get_property::<i64>(&format!("track-list/{}/id", i))
                .unwrap_or(i) as i32;
            let track_title = handle
                .get_property_string(&format!("track-list/{}/title", i))
                .ok();
            let track_lang = handle
                .get_property_string(&format!("track-list/{}/lang", i))
                .ok();
            let track_codec = handle
                .get_property_string(&format!("track-list/{}/codec", i))
                .ok();
            let channel_layout = handle
                .get_property_string(&format!("track-list/{}/demux-channel-count", i))
                .ok();
            let is_default = handle
                .get_property::<bool>(&format!("track-list/{}/default", i))
                .unwrap_or(false);
            let is_selected = handle
                .get_property::<bool>(&format!("track-list/{}/selected", i))
                .unwrap_or(false);

            tracks.push(MediaTrack {
                id: 0,
                media_id,
                track_index: track_id,
                track_type,
                title: track_title,
                language: track_lang,
                codec: track_codec,
                channel_layout,
                is_default,
                is_selected,
            });
        }

        // Unload file from headless engine
        let _ = handle.command(&["stop"]);

        let metadata = MediaMetadata {
            media_id,
            title,
            original_title: None,
            description: None,
            genre,
            release_year,
            duration_ms,
            overall_bitrate,
            container: Some(ext),
            video_codec,
            audio_codec,
            width,
            height,
            fps,
            hdr_type: None,
            audio_channels,
            sample_rate,
            bit_depth: 16,
            artist,
            album,
            album_artist,
            track_number: None,
            disc_number: None,
        };

        Ok((metadata, tracks))
    }
}
