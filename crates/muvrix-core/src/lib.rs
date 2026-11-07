//! `muvrix-core`: Core shared data structures, platform detection and error handling.

pub mod browser;
pub mod error;
pub mod history;
pub mod path;
pub mod platform;

pub use browser::{EntryKind, MediaBrowser, MediaBrowserEntry, MediaFilter, SortBy};
pub use error::{MuvrixError, Result};
pub use history::{
    default_history_store_path, HistoryStore, MediaHistoryEntry, CURRENT_HISTORY_SCHEMA_VERSION,
};
pub use path::{
    expand_tilde, find_project_root, is_streaming_url, resolve_media_path, resolve_preset,
    MediaPathError,
};
pub use platform::{DistroInfo, PlatformInfo, SessionType};
