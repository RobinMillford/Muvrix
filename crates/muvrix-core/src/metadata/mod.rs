//! Pluggable media metadata extraction provider interfaces and lightweight fallbacks.

use crate::database::{MediaMetadata, MediaTrack, TrackType};
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum MetadataError {
    #[error("I/O error during metadata extraction: {0}")]
    Io(#[from] std::io::Error),

    #[error("Engine extraction failure: {0}")]
    Engine(String),

    #[error("File format not supported or invalid media: {0}")]
    UnsupportedFormat(String),

    #[error("Metadata extraction timeout: {0}")]
    Timeout(String),
}

pub type MetadataResult<T> = std::result::Result<T, MetadataError>;

/// Abstract provider interface for extracting container, stream, and tag metadata.
pub trait MediaMetadataExtractor: Send + Sync {
    /// Extracts media metadata and stream track records for a given media file.
    fn extract_metadata(
        &self,
        path: &Path,
        media_id: i64,
    ) -> MetadataResult<(MediaMetadata, Vec<MediaTrack>)>;
}

/// A lightweight, deterministic fallback extractor that inspects standard filesystem attributes.
#[derive(Debug, Default, Clone)]
pub struct FallbackMetadataExtractor;

impl MediaMetadataExtractor for FallbackMetadataExtractor {
    fn extract_metadata(
        &self,
        path: &Path,
        media_id: i64,
    ) -> MetadataResult<(MediaMetadata, Vec<MediaTrack>)> {
        let file_stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled")
            .to_string();

        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        let mut tracks = Vec::new();

        // Baseline track inference from extension
        let is_audio = matches!(
            ext.as_str(),
            "mp3" | "flac" | "wav" | "aac" | "ogg" | "m4a" | "opus"
        );

        if is_audio {
            tracks.push(MediaTrack {
                id: 0,
                media_id,
                track_index: 0,
                track_type: TrackType::Audio,
                title: Some(file_stem.clone()),
                language: None,
                codec: Some(ext.clone()),
                channel_layout: Some("stereo".to_string()),
                is_default: true,
                is_selected: true,
            });
        } else {
            tracks.push(MediaTrack {
                id: 0,
                media_id,
                track_index: 0,
                track_type: TrackType::Video,
                title: Some("Video".to_string()),
                language: None,
                codec: Some(ext.clone()),
                channel_layout: None,
                is_default: true,
                is_selected: true,
            });
        }

        let metadata = MediaMetadata {
            media_id,
            title: Some(file_stem),
            original_title: None,
            description: None,
            genre: None,
            release_year: None,
            duration_ms: 0,
            overall_bitrate: 0,
            container: Some(ext),
            video_codec: if is_audio {
                None
            } else {
                Some("unknown".to_string())
            },
            audio_codec: if is_audio {
                Some("unknown".to_string())
            } else {
                None
            },
            width: if is_audio { 0 } else { 1920 },
            height: if is_audio { 0 } else { 1080 },
            fps: if is_audio { 0.0 } else { 24.0 },
            hdr_type: None,
            audio_channels: if is_audio { 2 } else { 0 },
            sample_rate: if is_audio { 44100 } else { 0 },
            bit_depth: 16,
            artist: None,
            album: None,
            album_artist: None,
            track_number: None,
            disc_number: None,
        };

        Ok((metadata, tracks))
    }
}
