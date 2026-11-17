//! Navigation views, panels, and secondary UI components for Muvrix.

pub mod batch_editor;
pub mod browser;
pub mod drawers;
pub mod history;
pub mod home;
pub mod integrity;
pub mod library;
pub mod lyrics;
pub mod mini_player;
pub mod player;
pub mod playlist;
pub mod settings;
pub mod smart_collections;

use crate::icons::Icon;

/// Top-level application view modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppView {
    #[default]
    Home,
    Player,
    Library,
    Browser,
    Playlist,
    Lyrics,
    Recent,
    Settings,
}

impl AppView {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Player => "Player",
            Self::Library => "Library",
            Self::Browser => "Browser",
            Self::Playlist => "Playlist",
            Self::Lyrics => "Lyrics",
            Self::Recent => "History",
            Self::Settings => "Settings",
        }
    }

    pub fn icon(&self) -> Icon {
        match self {
            Self::Home => Icon::Play,
            Self::Player => Icon::Play,
            Self::Library => Icon::Library,
            Self::Browser => Icon::Folder,
            Self::Playlist => Icon::Playlist,
            Self::Lyrics => Icon::Lyrics,
            Self::Recent => Icon::History,
            Self::Settings => Icon::Settings,
        }
    }
}

/// Tab selector for the Settings view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    Audio,
    Subtitles,
    VideoEnhancement,
    Appearance,
    Profiles,
    Shortcuts,
    NetworkMedia,
    ConfigEditor,
    Advanced,
    About,
    EqualizerAudio,
    Preferences,
}

impl SettingsTab {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Audio | Self::EqualizerAudio => "Audio & DSP",
            Self::Subtitles => "Subtitles & Fonts",
            Self::VideoEnhancement => "Video & Shaders",
            Self::Appearance => "Appearance",
            Self::Profiles => "Playback Profiles",
            Self::Shortcuts => "Keyboard Shortcuts",
            Self::NetworkMedia => "Network Sources",
            Self::ConfigEditor => "Config Editor",
            Self::Advanced | Self::Preferences => "Advanced & Backup",
            Self::About => "About Muvrix",
        }
    }

    pub fn matches_search(&self, query: &str) -> bool {
        let query = query.trim().to_ascii_lowercase();
        if query.is_empty() {
            return true;
        }
        let keywords = match self {
            Self::Audio | Self::EqualizerAudio => {
                "audio equalizer volume boost device delay channels normalization pitch alang routing"
            }
            Self::Subtitles => "subtitle font shadow color justify speed fonts ass dual",
            Self::VideoEnhancement => {
                "video shader zoom pan panscan crop hdr brightness deband anime4k"
            }
            Self::Appearance => "theme appearance accent light dark amoled color obsidian lime palette custom",
            Self::Profiles => "profile profiles user cinema headphones room late night presets volume switcher",
            Self::Shortcuts => "shortcuts keyboard hotkey keybinding mapping bind rebind keys conflict",
            Self::NetworkMedia => "network webdav smb sftp remote stream",
            Self::ConfigEditor => "config mpv.conf input.conf editor file apply",
            Self::Advanced | Self::Preferences => {
                "advanced backup restore xml export import history cache logging dump sleep timer privacy mini player pip"
            }
            Self::About => "about version libmpv ffmpeg placebo device author license",
        };
        self.label().to_ascii_lowercase().contains(&query) || keywords.contains(&query)
    }
}

/// Formats a file size in bytes to a human-readable string.
pub fn format_file_size(bytes: u64) -> String {
    if bytes == 0 {
        return "-".to_string();
    }
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Formats a UNIX timestamp into a relative human-readable label.
pub fn format_timestamp_relative(timestamp_secs: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if timestamp_secs == 0 || now < timestamp_secs {
        return "Recently".to_string();
    }
    let diff = now - timestamp_secs;
    if diff < 60 {
        "Just now".to_string()
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else {
        format!("{}d ago", diff / 86400)
    }
}
