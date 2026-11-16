//! `muvrix-core`: Core shared data structures, platform detection and error handling.

pub mod backup;
pub mod browser;
pub mod database;
pub mod error;
pub mod history;
pub mod lyrics;
pub mod metadata;
pub mod network;
pub mod path;
pub mod platform;
pub mod playlist;
pub mod scanner;
pub mod search;
pub mod thumbnails;

pub use backup::{BackupError, BackupSettings, CURRENT_BACKUP_SCHEMA};
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
    credential_ref_username, generate_credential_ref, redact_credentials_from_url, CredentialError,
    CredentialVault, MockNetworkProvider, NetworkError, NetworkProvider, NetworkResult,
    PresentedHostKey, RemoteFileEntry, SftpProvider, SmbProvider, WebDavProvider,
};
pub use path::{
    expand_tilde, find_project_root, is_streaming_url, resolve_media_path, resolve_preset,
    MediaPathError,
};
pub use platform::{DistroInfo, PlatformInfo, SessionType};
pub use playlist::{
    export_m3u, export_pls, export_xspf, generate_m3u, generate_pls, generate_xspf,
    load_playlist_file_or_url, parse_m3u, parse_playlist_auto, parse_pls, parse_xspf, M3uEntry,
    PlaylistEntry,
};
pub use scanner::{ScanOptions, ScanProgress, ScanSummary, ScannerService};
pub use search::sanitize_fts5_query;
pub use thumbnails::{
    calculate_thumbnail_seek_seconds, FallbackThumbnailExtractor, ThumbnailCache, ThumbnailError,
    ThumbnailExtractor, ThumbnailResult,
};
