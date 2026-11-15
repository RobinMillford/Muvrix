//! Restart-level persistence and restoration for video adjustments, Anime4K shaders,
//! libplacebo tone-mapping, 10-band equalizer, audio DSP, and audio device preferences.

use crate::equalizer::{AudioDeviceInfo, EqualizerSettings};
use crate::shader::{Anime4KPreset, ShaderQuality, VideoAdjustmentSettings};
use crate::track::SubtitleStyling;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const CURRENT_SETTINGS_VERSION: u32 = 2;

fn default_volume() -> f64 {
    100.0
}

fn default_true() -> bool {
    true
}

fn default_audio_boost_cap() -> i32 {
    30
}

/// Audio channel routing mode for the playback engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioChannelsSetting {
    #[default]
    Auto,
    AutoSafe,
    Mono,
    Stereo,
    ReverseStereo,
}

impl AudioChannelsSetting {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::AutoSafe => "Auto Safe",
            Self::Mono => "Mono",
            Self::Stereo => "Stereo",
            Self::ReverseStereo => "Reversed Stereo",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Auto => "Allow playback engine to determine appropriate channel configuration.",
            Self::AutoSafe => "Use a safe automatic channel configuration avoiding exotic layouts.",
            Self::Mono => "Downmix audio stream into a single mono channel.",
            Self::Stereo => "Downmix audio stream into standard 2-channel stereo.",
            Self::ReverseStereo => {
                "Swap left and right audio channels for inverted speaker/headphone setups."
            }
        }
    }

    pub fn mpv_channels_option(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::AutoSafe => "auto-safe",
            Self::Mono => "mono",
            Self::Stereo => "stereo",
            Self::ReverseStereo => "stereo",
        }
    }

    pub fn needs_pan_filter(&self) -> bool {
        matches!(self, Self::ReverseStereo)
    }
}

/// Application appearance mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    Obsidian,
    Dark,
    Light,
    Amoled,
}

/// Accent color used by the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccentColor {
    #[default]
    Lime,
    Blue,
    Purple,
    Teal,
    Green,
    Orange,
    Red,
}

/// Top-level persisted multimedia and engine configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersistedMediaSettings {
    pub schema_version: u32,
    pub video_adjustments: VideoAdjustmentSettings,
    pub anime4k_preset: Anime4KPreset,
    pub anime4k_quality: ShaderQuality,
    pub equalizer: EqualizerSettings,
    pub selected_audio_device: String,
    #[serde(default = "default_volume")]
    pub volume: f64,
    #[serde(default)]
    pub subtitle_styling: SubtitleStyling,
    #[serde(default)]
    pub theme_mode: ThemeMode,
    #[serde(default)]
    pub accent_color: AccentColor,
    #[serde(default)]
    pub audio_channels: AudioChannelsSetting,
    #[serde(default)]
    pub volume_normalization: bool,
    #[serde(default = "default_true")]
    pub audio_pitch_correction: bool,
    #[serde(default)]
    pub preferred_audio_languages: String,
    #[serde(default)]
    pub default_audio_delay_ms: i32,
    #[serde(default = "default_audio_boost_cap")]
    pub volume_boost_cap: i32,
    #[serde(default = "default_true")]
    pub background_audio: bool,
    #[serde(default)]
    pub verbose_logging: bool,
}

impl Default for PersistedMediaSettings {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SETTINGS_VERSION,
            video_adjustments: VideoAdjustmentSettings::default(),
            anime4k_preset: Anime4KPreset::default(),
            anime4k_quality: ShaderQuality::default(),
            equalizer: EqualizerSettings::default(),
            selected_audio_device: "auto".to_string(),
            volume: 100.0,
            subtitle_styling: SubtitleStyling::default(),
            theme_mode: ThemeMode::Obsidian,
            accent_color: AccentColor::Lime,
            audio_channels: AudioChannelsSetting::default(),
            volume_normalization: false,
            audio_pitch_correction: true,
            preferred_audio_languages: String::new(),
            default_audio_delay_ms: 0,
            volume_boost_cap: 30,
            background_audio: true,
            verbose_logging: false,
        }
    }
}

impl PersistedMediaSettings {
    /// Resolves the default persistent settings file path following platform standards.
    pub fn default_settings_path() -> PathBuf {
        #[cfg(target_os = "windows")]
        {
            if let Ok(app_data) = std::env::var("APPDATA") {
                return PathBuf::from(app_data).join("Muvrix").join("settings.json");
            }
        }

        #[cfg(target_os = "macos")]
        {
            if let Ok(home) = std::env::var("HOME") {
                return PathBuf::from(home)
                    .join("Library")
                    .join("Application Support")
                    .join("Muvrix")
                    .join("settings.json");
            }
        }

        if let Ok(xdg_config) = std::env::var("XDG_CONFIG_HOME") {
            if !xdg_config.trim().is_empty() {
                return PathBuf::from(xdg_config)
                    .join("muvrix")
                    .join("settings.json");
            }
        }

        if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home)
                .join(".config")
                .join("muvrix")
                .join("settings.json")
        } else {
            PathBuf::from(".muvrix_settings.json")
        }
    }

    /// Loads settings from disk, falling back safely to defaults if missing or corrupted.
    pub fn load(path: Option<&Path>) -> Self {
        let file_path = path
            .map(|p| p.to_path_buf())
            .unwrap_or_else(Self::default_settings_path);

        if !file_path.exists() {
            return Self::default();
        }

        match fs::read_to_string(&file_path) {
            Ok(content) => match serde_json::from_str::<PersistedMediaSettings>(&content) {
                Ok(settings) => settings,
                Err(e) => {
                    log::warn!(
                        "Failed to deserialize settings from {}: {e}. Falling back to defaults.",
                        file_path.display()
                    );
                    Self::default()
                }
            },
            Err(e) => {
                log::warn!(
                    "Failed to read settings from {}: {e}. Falling back to defaults.",
                    file_path.display()
                );
                Self::default()
            }
        }
    }

    /// Atomically persists settings to disk using a temporary file and rename.
    pub fn save(&self, path: Option<&Path>) -> io::Result<()> {
        let file_path = path
            .map(|p| p.to_path_buf())
            .unwrap_or_else(Self::default_settings_path);

        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        let temp_path = file_path.with_extension("json.tmp");
        fs::write(&temp_path, json.as_bytes())?;
        fs::rename(&temp_path, &file_path)?;

        Ok(())
    }

    /// Validates loaded configuration against available audio hardware and system state.
    /// If a previously selected audio device is disconnected or unavailable,
    /// safely falls back to `"auto"`.
    pub fn validate_and_fallback(&mut self, available_devices: &[AudioDeviceInfo]) {
        if self.selected_audio_device != "auto" {
            let is_available = available_devices
                .iter()
                .any(|d| d.name == self.selected_audio_device);
            if !is_available {
                log::info!(
                    "Previously selected audio device '{}' not available at startup; falling back to 'auto'",
                    self.selected_audio_device
                );
                self.selected_audio_device = "auto".to_string();
            }
        }
    }
}
