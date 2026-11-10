//! `muvrix-core`: Core shared data structures, platform detection and error handling.

pub mod browser;
pub mod database;
pub mod error;
pub mod history;
pub mod lyrics;
pub mod metadata;
pub mod network;
pub mod path;
pub mod platform;
pub mod scanner;
pub mod thumbnails;

pub use browser::{EntryKind, MediaBrowser, MediaBrowserEntry, MediaFilter, SortBy};
pub use database::{
    models::*, DatabaseError, DatabaseManager, DbResult, Repository, LATEST_SCHEMA_VERSION,
};
pub use error::{MuvrixError, Result};
pub use history::{
    default_history_store_path, HistoryStore, MediaHistoryEntry, CURRENT_HISTORY_SCHEMA_VERSION,
};
pub use lyrics::{detector::find_sidecar_lrc, LrcFile, LrcParseError, LyricLine};
pub use metadata::{
    FallbackMetadataExtractor, MediaMetadataExtractor, MetadataError, MetadataResult,
};
pub use network::{
    generate_credential_ref, redact_credentials_from_url, CredentialError, CredentialVault,
    MockNetworkProvider, NetworkError, NetworkProvider, NetworkResult, PresentedHostKey,
    RemoteFileEntry, SftpProvider, SmbProvider, WebDavProvider,
};
pub use path::{
    expand_tilde, find_project_root, is_streaming_url, resolve_media_path, resolve_preset,
    MediaPathError,
};
pub use platform::{DistroInfo, PlatformInfo, SessionType};
pub use scanner::{ScanOptions, ScanProgress, ScanSummary, ScannerService};
pub use thumbnails::{
    calculate_thumbnail_seek_seconds, FallbackThumbnailExtractor, ThumbnailCache, ThumbnailError,
    ThumbnailExtractor, ThumbnailResult,
};
