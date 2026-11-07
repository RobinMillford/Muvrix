//! Lightweight local filesystem media browser.

use std::fs;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

/// Classification of a filesystem entry.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum EntryKind {
    Directory,
    VideoFile,
    AudioFile,
    OtherFile,
}

impl EntryKind {
    pub fn is_media(&self) -> bool {
        matches!(self, Self::VideoFile | Self::AudioFile)
    }
}

/// Metadata representation of a filesystem item within the browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaBrowserEntry {
    /// File or directory base name.
    pub name: String,
    /// Absolute or canonical path.
    pub path: PathBuf,
    /// Type classification.
    pub kind: EntryKind,
    /// File size in bytes (0 for directories).
    pub size_bytes: u64,
    /// Unix modification timestamp in seconds.
    pub modified_timestamp: u64,
}

/// Media category filter.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum MediaFilter {
    #[default]
    AllMedia,
    VideoOnly,
    AudioOnly,
}

/// Sorting criteria for directory contents.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum SortBy {
    #[default]
    NameAsc,
    NameDesc,
    DateAsc,
    DateDesc,
    SizeAsc,
    SizeDesc,
}

/// State machine and navigator for browsing local directories.
#[derive(Debug, Clone)]
pub struct MediaBrowser {
    /// Currently displayed directory path.
    pub current_dir: PathBuf,
    /// Backward navigation history stack.
    pub history_back: Vec<PathBuf>,
    /// Forward navigation history stack.
    pub history_forward: Vec<PathBuf>,
    /// Enumerated filesystem entries in `current_dir`.
    pub entries: Vec<MediaBrowserEntry>,
    /// Active media filter.
    pub filter: MediaFilter,
    /// Active sort criterion.
    pub sort_by: SortBy,
    /// Filter substring searched within current directory.
    pub search_query: String,
    /// Diagnostic error message if the current path could not be read.
    pub error: Option<String>,
}

impl Default for MediaBrowser {
    fn default() -> Self {
        Self::new(None)
    }
}

fn user_default_media_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("MUVRIX_INITIAL_BROWSER_DIR") {
        let p = PathBuf::from(dir);
        if p.exists() {
            return p;
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(profile) = std::env::var("USERPROFILE") {
            let p = PathBuf::from(&profile).join("Videos");
            if p.exists() {
                return p;
            }
            return PathBuf::from(profile);
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            let p = PathBuf::from(&home).join("Movies");
            if p.exists() {
                return p;
            }
            return PathBuf::from(home);
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        let p = PathBuf::from(&home).join("Videos");
        if p.exists() {
            return p;
        }
        return PathBuf::from(home);
    }

    PathBuf::from(".")
}

impl MediaBrowser {
    /// Creates a new `MediaBrowser` initialized to the given path or platform default.
    pub fn new(initial_dir: Option<PathBuf>) -> Self {
        let dir = initial_dir.unwrap_or_else(user_default_media_dir);

        let mut browser = Self {
            current_dir: dir,
            history_back: Vec::new(),
            history_forward: Vec::new(),
            entries: Vec::new(),
            filter: MediaFilter::AllMedia,
            sort_by: SortBy::NameAsc,
            search_query: String::new(),
            error: None,
        };

        browser.refresh();
        browser
    }

    /// Determines if an extension corresponds to a known media format.
    pub fn classify_extension(ext: &str) -> EntryKind {
        let ext_lower = ext.to_lowercase();
        match ext_lower.as_str() {
            "mp4" | "mkv" | "avi" | "mov" | "webm" | "flv" | "ts" | "m4v" | "wmv" | "3gp"
            | "mpg" | "mpeg" | "vob" | "ogv" => EntryKind::VideoFile,
            "mp3" | "flac" | "wav" | "aac" | "m4a" | "ogg" | "opus" | "wma" | "alac" | "aiff" => {
                EntryKind::AudioFile
            }
            _ => EntryKind::OtherFile,
        }
    }

    /// Navigates to `new_path`, pushing the current directory onto `history_back`.
    pub fn navigate_to(&mut self, new_path: PathBuf) {
        if new_path == self.current_dir {
            return;
        }

        self.history_back.push(self.current_dir.clone());
        self.history_forward.clear();
        self.current_dir = new_path;
        self.search_query.clear();
        self.refresh();
    }

    /// Navigates up to the parent directory.
    pub fn navigate_parent(&mut self) {
        if let Some(parent) = self.current_dir.parent() {
            let parent_buf = parent.to_path_buf();
            self.navigate_to(parent_buf);
        }
    }

    /// Steps back in the navigation history.
    pub fn navigate_back(&mut self) {
        if let Some(prev) = self.history_back.pop() {
            self.history_forward.push(self.current_dir.clone());
            self.current_dir = prev;
            self.search_query.clear();
            self.refresh();
        }
    }

    /// Steps forward in the navigation history.
    pub fn navigate_forward(&mut self) {
        if let Some(next) = self.history_forward.pop() {
            self.history_back.push(self.current_dir.clone());
            self.current_dir = next;
            self.search_query.clear();
            self.refresh();
        }
    }

    /// Reads immediate filesystem entries in `current_dir` and sorts them.
    pub fn refresh(&mut self) {
        self.entries.clear();
        self.error = None;

        if !self.current_dir.exists() {
            self.error = Some(format!(
                "Directory does not exist: {}",
                self.current_dir.display()
            ));
            return;
        }

        let read_dir = match fs::read_dir(&self.current_dir) {
            Ok(rd) => rd,
            Err(e) => {
                self.error = Some(format!("Permission denied or read error: {e}"));
                return;
            }
        };

        for entry_res in read_dir {
            let entry = match entry_res {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            // Skip hidden dotfiles
            if name.starts_with('.') {
                continue;
            }

            let file_type = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };

            let (kind, size_bytes, modified_timestamp) = if file_type.is_dir() {
                (EntryKind::Directory, 0, 0)
            } else {
                let ext = path
                    .extension()
                    .map(|e| e.to_string_lossy().to_string())
                    .unwrap_or_default();
                let kind = Self::classify_extension(&ext);
                let metadata = entry.metadata().ok();
                let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
                let modified = metadata
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                (kind, size, modified)
            };

            // Only add directories and media files
            if kind == EntryKind::Directory || kind.is_media() {
                self.entries.push(MediaBrowserEntry {
                    name,
                    path,
                    kind,
                    size_bytes,
                    modified_timestamp,
                });
            }
        }

        self.sort_entries();
    }

    /// Sorts internal entries: directories always first, followed by media files sorted by `sort_by`.
    fn sort_entries(&mut self) {
        self.entries.sort_by(|a, b| {
            // Directories always precede files
            match (
                a.kind == EntryKind::Directory,
                b.kind == EntryKind::Directory,
            ) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => match self.sort_by {
                    SortBy::NameAsc => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                    SortBy::NameDesc => b.name.to_lowercase().cmp(&a.name.to_lowercase()),
                    SortBy::DateAsc => a.modified_timestamp.cmp(&b.modified_timestamp),
                    SortBy::DateDesc => b.modified_timestamp.cmp(&a.modified_timestamp),
                    SortBy::SizeAsc => a.size_bytes.cmp(&b.size_bytes),
                    SortBy::SizeDesc => b.size_bytes.cmp(&a.size_bytes),
                },
            }
        });
    }

    /// Sets the sort criterion and re-sorts entries.
    pub fn set_sort_by(&mut self, sort_by: SortBy) {
        self.sort_by = sort_by;
        self.sort_entries();
    }

    /// Returns references to entries matching the active filter and search query.
    pub fn filtered_entries(&self) -> Vec<&MediaBrowserEntry> {
        let query_lower = self.search_query.trim().to_lowercase();

        self.entries
            .iter()
            .filter(|e| {
                // Apply media filter to files
                if e.kind != EntryKind::Directory {
                    match self.filter {
                        MediaFilter::AllMedia => true,
                        MediaFilter::VideoOnly => e.kind == EntryKind::VideoFile,
                        MediaFilter::AudioOnly => e.kind == EntryKind::AudioFile,
                    }
                } else {
                    true
                }
            })
            .filter(|e| {
                if query_lower.is_empty() {
                    true
                } else {
                    e.name.to_lowercase().contains(&query_lower)
                }
            })
            .collect()
    }

    /// Returns breadcrumb segments `(display_name, path)` from root to `current_dir`.
    pub fn breadcrumbs(&self) -> Vec<(String, PathBuf)> {
        let mut crumbs = Vec::new();
        let mut curr = Some(self.current_dir.as_path());

        while let Some(p) = curr {
            let name = p
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| p.to_string_lossy().to_string());

            if !name.is_empty() {
                crumbs.push((name, p.to_path_buf()));
            }
            curr = p.parent();
        }

        crumbs.reverse();
        crumbs
    }
}
