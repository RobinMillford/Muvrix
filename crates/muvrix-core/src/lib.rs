//! `muvrix-core`: Core shared data structures, platform detection and error handling.

pub mod error;
pub mod path;
pub mod platform;

pub use error::{MuvrixError, Result};
pub use path::{
    expand_tilde, find_project_root, is_streaming_url, resolve_media_path, resolve_preset,
    MediaPathError,
};
pub use platform::{DistroInfo, PlatformInfo, SessionType};
