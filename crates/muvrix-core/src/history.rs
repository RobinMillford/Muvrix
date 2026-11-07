//! Durable, versioned local playback history and resume state store.

use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Current schema version for history storage.
pub const CURRENT_HISTORY_SCHEMA_VERSION: u32 = 1;

/// Default maximum number of recent media entries retained.
pub const DEFAULT_MAX_HISTORY_ENTRIES: usize = 100;

/// Minimum playback threshold (seconds) before a resume position is recorded.
pub const RESUME_MIN_POSITION_SECONDS: f64 = 10.0;

/// Threshold percentage of duration (e.g. 95%) after which media is marked completed.
pub const RESUME_COMPLETION_RATIO: f64 = 0.95;

/// Serialized record of a single played media file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaHistoryEntry {
    /// Absolute or canonical media path/URL.
    pub path: String,
    /// Basename or display title of the media file.
    pub display_name: String,
    /// Last playback position in seconds.
    pub last_position_sec: f64,
    /// Total duration in seconds, if known.
    pub duration_sec: f64,
    /// Unix timestamp (seconds) of last playback.
    pub last_played_timestamp: u64,
    /// Whether playback reached the end threshold.
    pub completed: bool,
}

/// Persistent store managing playback history and resume states.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryStore {
    /// Schema version for forward/backward compatibility.
    pub version: u32,
    /// Global toggle to enable or disable history recording.
    pub enabled: bool,
    /// Whether automatic playback position resume is active.
    pub auto_resume_enabled: bool,
    /// Maximum number of records retained.
    pub max_entries: usize,
    /// Chronologically ordered history entries (most recent first).
    pub entries: Vec<MediaHistoryEntry>,
    /// Filesystem storage location (omitted from serialized JSON).
    #[serde(skip)]
    pub store_path: PathBuf,
}

impl Default for HistoryStore {
    fn default() -> Self {
        Self {
            version: CURRENT_HISTORY_SCHEMA_VERSION,
            enabled: true,
            auto_resume_enabled: true,
            max_entries: DEFAULT_MAX_HISTORY_ENTRIES,
            entries: Vec::new(),
            store_path: default_history_store_path(),
        }
    }
}

/// Resolves the platform-standard application data directory for Muvrix history.
pub fn default_history_store_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(app_data) = std::env::var("APPDATA") {
            return PathBuf::from(app_data).join("Muvrix").join("history.json");
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("Muvrix")
                .join("history.json");
        }
    }

    // Linux and Unix default: follow XDG Base Directory specification
    if let Ok(xdg_data) = std::env::var("XDG_DATA_HOME") {
        if !xdg_data.trim().is_empty() {
            return PathBuf::from(xdg_data).join("muvrix").join("history.json");
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("muvrix")
            .join("history.json")
    } else {
        PathBuf::from(".muvrix_history.json")
    }
}

impl HistoryStore {
    /// Loads the history store from disk, falling back gracefully to defaults on missing or corrupt files.
    pub fn load(custom_path: Option<PathBuf>) -> Self {
        let store_path = custom_path.unwrap_or_else(default_history_store_path);

        if !store_path.exists() {
            return Self {
                store_path,
                ..Default::default()
            };
        }

        match File::open(&store_path) {
            Ok(file) => {
                let reader = BufReader::new(file);
                match serde_json::from_reader::<_, HistoryStore>(reader) {
                    Ok(mut store) => {
                        store.store_path = store_path;
                        // Ensure version migration if needed in future
                        if store.version != CURRENT_HISTORY_SCHEMA_VERSION {
                            log::info!(
                                "Migrating history store schema from v{} to v{}",
                                store.version,
                                CURRENT_HISTORY_SCHEMA_VERSION
                            );
                            store.version = CURRENT_HISTORY_SCHEMA_VERSION;
                        }
                        store
                    }
                    Err(e) => {
                        log::warn!(
                            "Failed to parse history JSON at '{}': {e}. Initializing clean store.",
                            store_path.display()
                        );
                        Self {
                            store_path,
                            ..Default::default()
                        }
                    }
                }
            }
            Err(e) => {
                log::warn!(
                    "Failed to open history store at '{}': {e}. Using in-memory default.",
                    store_path.display()
                );
                Self {
                    store_path,
                    ..Default::default()
                }
            }
        }
    }

    /// Atomically persists the history store to disk using a temporary sibling file.
    pub fn save(&self) -> Result<(), std::io::Error> {
        if let Some(parent) = self.store_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        let temp_path = self.store_path.with_extension("tmp");
        {
            let file = File::create(&temp_path)?;
            let mut writer = BufWriter::new(file);
            serde_json::to_writer_pretty(&mut writer, self)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            writer.flush()?;
        }

        // Atomic file replacement
        fs::rename(&temp_path, &self.store_path)?;
        Ok(())
    }

    /// Records or updates the playback position for a given media file.
    pub fn record_playback(&mut self, path: &str, position_sec: f64, duration_sec: f64) {
        if !self.enabled || path.trim().is_empty() {
            return;
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let completed =
            duration_sec > 0.0 && position_sec >= (duration_sec * RESUME_COMPLETION_RATIO);

        let display_name = Path::new(path)
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());

        // Check if an existing entry matches the path
        if let Some(pos) = self.entries.iter().position(|e| e.path == path) {
            let mut entry = self.entries.remove(pos);
            entry.display_name = display_name;
            entry.last_position_sec = position_sec;
            if duration_sec > 0.0 {
                entry.duration_sec = duration_sec;
            }
            entry.last_played_timestamp = now;
            entry.completed = completed;
            self.entries.insert(0, entry);
        } else {
            let entry = MediaHistoryEntry {
                path: path.to_string(),
                display_name,
                last_position_sec: position_sec,
                duration_sec: duration_sec.max(0.0),
                last_played_timestamp: now,
                completed,
            };
            self.entries.insert(0, entry);
        }

        if self.entries.len() > self.max_entries {
            self.entries.truncate(self.max_entries);
        }
    }

    /// Queries the saved resume position for a media file, if eligible.
    pub fn get_resume_position(&self, path: &str) -> Option<f64> {
        if !self.enabled || !self.auto_resume_enabled {
            return None;
        }

        let entry = self.entries.iter().find(|e| e.path == path)?;
        if entry.completed {
            return None;
        }

        if entry.last_position_sec >= RESUME_MIN_POSITION_SECONDS {
            Some(entry.last_position_sec)
        } else {
            None
        }
    }

    /// Removes an individual entry from the history.
    pub fn remove_entry(&mut self, path: &str) -> bool {
        if let Some(pos) = self.entries.iter().position(|e| e.path == path) {
            self.entries.remove(pos);
            true
        } else {
            false
        }
    }

    /// Clears all recorded history entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Toggles global history tracking.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Toggles automatic playback resume.
    pub fn set_auto_resume(&mut self, enabled: bool) {
        self.auto_resume_enabled = enabled;
    }
}
