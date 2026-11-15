//! Strongly-typed domain models for the SQLite Media Library.

use serde::{Deserialize, Serialize};

/// Type classification of media stored in the catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaType {
    Video,
    Audio,
}

impl MediaType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Video => "video",
            Self::Audio => "audio",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "audio" => Self::Audio,
            _ => Self::Video,
        }
    }
}

/// Filesystem and network reachability state of a media item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AvailabilityState {
    Available,
    Unavailable,
    Corrupted,
}

impl AvailabilityState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Unavailable => "unavailable",
            Self::Corrupted => "corrupted",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "unavailable" => Self::Unavailable,
            "corrupted" => Self::Corrupted,
            _ => Self::Available,
        }
    }
}

/// Metadata extraction lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MetadataStatus {
    Pending,
    Extracted,
    Failed,
}

impl MetadataStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Extracted => "extracted",
            Self::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "extracted" => Self::Extracted,
            "failed" => Self::Failed,
            _ => Self::Pending,
        }
    }
}

/// A configured root directory or network share scanned into the library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryRoot {
    pub id: i64,
    pub display_name: String,
    pub root_path: String,
    pub source_type: String, // "local", "webdav", "smb"
    pub is_recursive: bool,
    pub is_enabled: bool,
    pub exclusion_patterns: Vec<String>,
    pub last_scanned_at: u64,
    pub scan_status: String,
}

/// A media file registered in the library catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaItem {
    pub id: i64,
    pub root_id: i64,
    pub relative_path: String,
    pub canonical_path: String,
    pub file_name: String,
    pub file_extension: String,
    pub media_type: MediaType,
    pub file_size: u64,
    pub modified_time: u64,
    pub added_time: u64,
    pub last_scanned_at: u64,
    pub availability_state: AvailabilityState,
    pub metadata_status: MetadataStatus,
}

/// Comprehensive format and stream metadata extracted for a media file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct MediaMetadata {
    pub media_id: i64,
    pub title: Option<String>,
    pub original_title: Option<String>,
    pub description: Option<String>,
    pub genre: Option<String>,
    pub release_year: Option<i32>,
    pub duration_ms: u64,
    pub overall_bitrate: u64,
    pub container: Option<String>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub hdr_type: Option<String>,
    pub audio_channels: u32,
    pub sample_rate: u32,
    pub bit_depth: u32,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track_number: Option<i32>,
    pub disc_number: Option<i32>,
}

/// Track classification within a media container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackType {
    Video,
    Audio,
    Subtitle,
}

impl TrackType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Video => "video",
            Self::Audio => "audio",
            Self::Subtitle => "sub",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "audio" => Self::Audio,
            "sub" | "subtitle" => Self::Subtitle,
            _ => Self::Video,
        }
    }
}

/// Stream track entry belonging to a media item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaTrack {
    pub id: i64,
    pub media_id: i64,
    pub track_index: i32,
    pub track_type: TrackType,
    pub title: Option<String>,
    pub language: Option<String>,
    pub codec: Option<String>,
    pub channel_layout: Option<String>,
    pub is_default: bool,
    pub is_selected: bool,
}

/// Origin source of cached artwork.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArtworkSource {
    FrameCapture,
    EmbeddedCover,
    ExternalPoster,
}

impl ArtworkSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::FrameCapture => "frame_capture",
            Self::EmbeddedCover => "embedded_cover",
            Self::ExternalPoster => "external_poster",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "embedded_cover" => Self::EmbeddedCover,
            "external_poster" => Self::ExternalPoster,
            _ => Self::FrameCapture,
        }
    }
}

/// Cached artwork or thumbnail entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtworkRecord {
    pub id: i64,
    pub media_id: i64,
    pub cache_key: String,
    pub file_path: String,
    pub source_type: ArtworkSource,
    pub width: u32,
    pub height: u32,
    pub generated_at: u64,
}

/// Persisted user playlist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub item_count: u32,
}

/// Ordered entry in a persisted playlist. Paths may be local files or HTTP(S) URLs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaylistEntry {
    pub id: i64,
    pub playlist_id: i64,
    pub position: i32,
    pub path: String,
    pub title: Option<String>,
    pub duration_ms: u64,
    pub added_at: u64,
}

/// User-created collection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Collection {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub created_at: u64,
    pub updated_at: u64,
}

/// Membership entry in a collection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionItem {
    pub collection_id: i64,
    pub media_id: i64,
    pub position: i32,
    pub added_at: u64,
}

/// User tag label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    pub id: i64,
    pub name: String,
}

/// Scan job execution audit record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanJob {
    pub id: i64,
    pub root_id: i64,
    pub started_at: u64,
    pub completed_at: u64,
    pub status: String,
    pub items_discovered: u64,
    pub items_updated: u64,
    pub items_missing: u64,
    pub errors_count: u64,
}

/// Remote network connection bookmark.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkConnection {
    pub id: i64,
    pub name: String,
    pub protocol: String, // "webdav", "smb", "sftp", "ftp"
    pub host: String,
    pub port: u16,
    pub remote_path: String,
    pub auth_type: String, // "anonymous", "basic", "keyring_ref"
    pub credential_ref: Option<String>,
    pub is_anonymous: bool,
    pub auto_connect: bool,
    pub last_connected_at: u64,
}

/// Unified presentation view for a media item (avoids N+1 query overhead in UI grids).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryItemView {
    pub item: MediaItem,
    pub metadata: Option<MediaMetadata>,
    pub thumbnail_path: Option<String>,
    pub is_favourite: bool,
    pub tags: Vec<String>,
}

/// Criteria for library search and filtering.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LibraryFilter {
    pub search_query: Option<String>,
    pub media_type: Option<MediaType>,
    pub root_id: Option<i64>,
    pub favourites_only: bool,
    pub collection_id: Option<i64>,
    pub tag: Option<String>,
    pub codec: Option<String>,
    pub resolution_tier: Option<String>, // "4K", "1080p", "720p", "SD"
}

/// Fields available for sorting library media queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LibrarySortField {
    #[default]
    Title,
    FileName,
    DateAdded,
    DateModified,
    Duration,
    FileSize,
}

/// Sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortDirection {
    #[default]
    Ascending,
    Descending,
}

/// Pagination bounds for queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pagination {
    pub offset: u64,
    pub limit: u64,
}

impl Default for Pagination {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: 50,
        }
    }
}

/// User-created timestamp bookmark on a media item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: i64,
    pub media_id: i64,
    pub timestamp_ms: i64,
    pub label: String,
    pub note: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Bookmark {
    pub fn timestamp_seconds(&self) -> f64 {
        self.timestamp_ms as f64 / 1000.0
    }
}

/// Persistent SSH host-fingerprint trust record for MITM protection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshHostTrustRecord {
    pub id: i64,
    pub host: String, // Normalized lowercase hostname or IP
    pub port: u16,
    pub key_algorithm: String,      // e.g. "ssh-ed25519", "rsa-sha2-512"
    pub fingerprint_sha256: String, // OpenSSH format: "SHA256:<base64-unpadded>"
    pub trusted_at: i64,            // Unix epoch seconds
    pub last_seen_at: i64,          // Unix epoch seconds
    pub notes: Option<String>,
}

impl SshHostTrustRecord {
    /// Normalizes host strings for consistent scoping.
    pub fn normalize_host(host: &str) -> String {
        host.trim().to_ascii_lowercase()
    }
}
