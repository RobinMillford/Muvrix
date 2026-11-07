//! Scanner configuration options and progress reporting structures.

use serde::{Deserialize, Serialize};

/// Configuration options controlling a library filesystem scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanOptions {
    /// Whether to descend recursively into child directories.
    pub recursive: bool,
    /// Whether to follow directory symlinks (default false for cycle prevention).
    pub follow_symlinks: bool,
    /// List of directory or file exclusion globs (e.g. `.git`, `node_modules`, `*.part`).
    pub exclusions: Vec<String>,
    /// Extensions classified as video.
    pub video_extensions: Vec<String>,
    /// Extensions classified as audio.
    pub audio_extensions: Vec<String>,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            recursive: true,
            follow_symlinks: false,
            exclusions: vec![
                ".git".to_string(),
                ".cache".to_string(),
                "node_modules".to_string(),
                "*.part".to_string(),
                "*.tmp".to_string(),
            ],
            video_extensions: vec![
                "mp4".to_string(),
                "mkv".to_string(),
                "webm".to_string(),
                "avi".to_string(),
                "mov".to_string(),
                "flv".to_string(),
                "ts".to_string(),
                "m4v".to_string(),
                "wmv".to_string(),
                "3gp".to_string(),
            ],
            audio_extensions: vec![
                "mp3".to_string(),
                "flac".to_string(),
                "wav".to_string(),
                "aac".to_string(),
                "ogg".to_string(),
                "m4a".to_string(),
                "opus".to_string(),
            ],
        }
    }
}

/// Real-time progress update produced during an active scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanProgress {
    pub root_id: i64,
    pub discovered_count: u64,
    pub indexed_count: u64,
    pub skipped_count: u64,
    pub missing_count: u64,
    pub error_count: u64,
    pub current_path: Option<String>,
    pub is_finished: bool,
    pub is_cancelled: bool,
}

impl ScanProgress {
    pub fn new(root_id: i64) -> Self {
        Self {
            root_id,
            discovered_count: 0,
            indexed_count: 0,
            skipped_count: 0,
            missing_count: 0,
            error_count: 0,
            current_path: None,
            is_finished: false,
            is_cancelled: false,
        }
    }
}
