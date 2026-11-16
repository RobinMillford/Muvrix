//! Strongly-typed representations of media tracks and chapters.

use crate::error::MpvError;
use crate::handle::MpvHandle;
use serde::{Deserialize, Serialize};

/// Media track category.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum TrackType {
    Video,
    Audio,
    Subtitle,
}

impl TrackType {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "video" => Some(Self::Video),
            "audio" => Some(Self::Audio),
            "sub" => Some(Self::Subtitle),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Video => "video",
            Self::Audio => "audio",
            Self::Subtitle => "sub",
        }
    }
}

/// Metadata and state for an individual media stream track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackInfo {
    pub id: i64,
    pub track_type: TrackType,
    pub title: Option<String>,
    pub lang: Option<String>,
    pub selected: bool,
    pub external: bool,
    pub codec: Option<String>,
}

/// Media chapter marker.
#[derive(Debug, Clone, PartialEq)]
pub struct ChapterInfo {
    pub index: i64,
    pub title: String,
    pub time: f64,
}

impl MpvHandle {
    /// Queries the full list of media tracks currently present in the demuxer.
    pub fn get_track_list(&self) -> Result<Vec<TrackInfo>, MpvError> {
        let count: i64 = match self.get_property("track-list/count") {
            Ok(c) => c,
            Err(_) => return Ok(Vec::new()),
        };

        let mut tracks = Vec::with_capacity(count as usize);
        for i in 0..count {
            let id: i64 = self
                .get_property(&format!("track-list/{i}/id"))
                .unwrap_or(i + 1);
            let type_str = self
                .get_property_string(&format!("track-list/{i}/type"))
                .unwrap_or_default();
            let track_type = TrackType::parse(&type_str).unwrap_or(TrackType::Video);
            let title = self
                .get_property_string(&format!("track-list/{i}/title"))
                .ok();
            let lang = self
                .get_property_string(&format!("track-list/{i}/lang"))
                .ok();
            let selected: bool = self
                .get_property(&format!("track-list/{i}/selected"))
                .unwrap_or(false);
            let external: bool = self
                .get_property(&format!("track-list/{i}/external"))
                .unwrap_or(false);
            let codec = self
                .get_property_string(&format!("track-list/{i}/codec"))
                .ok();

            tracks.push(TrackInfo {
                id,
                track_type,
                title,
                lang,
                selected,
                external,
                codec,
            });
        }
        Ok(tracks)
    }

    /// Queries the full list of chapters embedded in the media file.
    pub fn get_chapter_list(&self) -> Result<Vec<ChapterInfo>, MpvError> {
        let count: i64 = match self.get_property("chapter-list/count") {
            Ok(c) => c,
            Err(_) => return Ok(Vec::new()),
        };

        let mut chapters = Vec::with_capacity(count as usize);
        for i in 0..count {
            let title = self
                .get_property_string(&format!("chapter-list/{i}/title"))
                .unwrap_or_else(|_| format!("Chapter {}", i + 1));
            let time: f64 = self
                .get_property(&format!("chapter-list/{i}/time"))
                .unwrap_or(0.0);
            chapters.push(ChapterInfo {
                index: i,
                title,
                time,
            });
        }
        Ok(chapters)
    }

    /// Adds an external subtitle file (`sub-add`).
    pub fn add_sub(&self, path: &str, select: bool) -> Result<(), MpvError> {
        let flag = if select { "select" } else { "auto" };
        self.command(&["sub-add", path, flag])
    }

    /// Sets the primary subtitle track by ID, or disables subtitles if `None`.
    pub fn set_sub_track(&self, track_id: Option<i64>) -> Result<(), MpvError> {
        match track_id {
            Some(id) => self.set_property_string("sid", &id.to_string()),
            None => self.set_property_string("sid", "no"),
        }
    }

    /// Sets the secondary subtitle track by ID, or disables secondary subtitles if `None`.
    pub fn set_secondary_sub_track(&self, track_id: Option<i64>) -> Result<(), MpvError> {
        match track_id {
            Some(id) => self.set_property_string("secondary-sid", &id.to_string()),
            None => self.set_property_string("secondary-sid", "no"),
        }
    }

    /// Sets the audio track by ID, or disables audio if `None`.
    pub fn set_audio_track(&self, track_id: Option<i64>) -> Result<(), MpvError> {
        match track_id {
            Some(id) => self.set_property_string("aid", &id.to_string()),
            None => self.set_property_string("aid", "no"),
        }
    }

    /// Sets the video track by ID, or disables video if `None`.
    pub fn set_video_track(&self, track_id: Option<i64>) -> Result<(), MpvError> {
        match track_id {
            Some(id) => self.set_property_string("vid", &id.to_string()),
            None => self.set_property_string("vid", "no"),
        }
    }

    /// Sets subtitle delay in seconds (negative plays earlier, positive plays later).
    pub fn set_sub_delay(&self, delay_seconds: f64) -> Result<(), MpvError> {
        self.set_property("sub-delay", delay_seconds)
    }

    /// Toggles subtitle visibility without altering the selected subtitle track.
    pub fn set_sub_visibility(&self, visible: bool) -> Result<(), MpvError> {
        self.set_property("sub-visibility", visible)
    }

    /// Queries the currently active secondary subtitle track ID, if any.
    pub fn get_secondary_sub_track(&self) -> Option<i64> {
        self.get_property_string("secondary-sid")
            .ok()
            .and_then(|s| s.parse::<i64>().ok())
    }

    /// Sets secondary subtitle delay in seconds.
    ///
    /// On older libmpv versions (< 0.38.0) where `secondary-sub-delay` is not supported,
    /// `PropertyNotFound` is gracefully ignored.
    pub fn set_secondary_sub_delay(&self, delay_seconds: f64) -> Result<(), MpvError> {
        match self.set_property("secondary-sub-delay", delay_seconds) {
            Ok(()) | Err(MpvError::PropertyNotFound) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Toggles secondary subtitle visibility.
    ///
    /// On older libmpv versions (< 0.38.0) where `secondary-sub-visibility` is not supported,
    /// `PropertyNotFound` is gracefully ignored.
    pub fn set_secondary_sub_visibility(&self, visible: bool) -> Result<(), MpvError> {
        match self.set_property("secondary-sub-visibility", visible) {
            Ok(()) | Err(MpvError::PropertyNotFound) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Configures dual-subtitle positioning to mitigate overlap by placing primary subtitles
    /// at their original/default position (`sub-pos=100`) and secondary subtitles towards the
    /// top edge (`secondary-sub-pos=10`). Note: In libmpv, `sub-pos=100` specifies original
    /// position rather than guaranteed absolute bottom; this reduces likely overlap but does
    /// not guarantee collision-free rendering for all arbitrary ASS script positions.
    ///
    /// On older libmpv versions (< 0.38.0, e.g. Ubuntu 24.04 package archives) where
    /// `secondary-sub-pos` is not recognized, `PropertyNotFound` is gracefully ignored so
    /// primary subtitle positioning (`sub-pos=100`) succeeds without returning an error.
    pub fn configure_dual_subtitle_collision_avoidance(&self) -> Result<(), MpvError> {
        self.set_property("sub-pos", 100i64)?;
        match self.set_property("secondary-sub-pos", 10i64) {
            Ok(()) | Err(MpvError::PropertyNotFound) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Applies subtitle styling preferences to the mpv core.
    pub fn apply_subtitle_styling(&self, styling: &SubtitleStyling) -> Result<(), MpvError> {
        if !styling.font.trim().is_empty() {
            let _ = self.set_property_string("sub-font", &styling.font);
        }
        let _ = self.set_property("sub-font-size", styling.font_size);
        if !styling.color.trim().is_empty() {
            let _ = self.set_property_string("sub-color", &styling.color);
        }
        if !styling.border_color.trim().is_empty() {
            let _ = self.set_property_string("sub-border-color", &styling.border_color);
        }
        let _ = self.set_property("sub-border-size", styling.border_size);
        let _ = self.set_property("sub-pos", styling.pos);
        let _ = self.set_property("sub-scale", styling.scale);
        let override_val = styling.ass_override_mode.as_mpv_str();
        let _ = self.set_property_string("sub-ass-override", override_val);

        // Secondary subtitle distinct positioning and styling
        let _ = self.set_property("secondary-sub-pos", styling.secondary_pos);
        let _ = self.set_property_string("secondary-sub-ass-override", override_val);

        let _ = self.set_property("sub-speed", styling.sub_speed.clamp(0.5, 2.0));
        let _ = self.set_property(
            "sub-shadow-offset",
            styling.sub_shadow_offset.clamp(0.0, 10.0),
        );
        if !styling.shadow_color.trim().is_empty() {
            let _ = self.set_property_string("sub-shadow-color", &styling.shadow_color);
        }
        let _ = self.set_property_string("sub-back-color", &styling.sub_back_color);
        let _ = self.set_property_string("sub-justify", styling.sub_justify.as_mpv_str());
        let _ = self.set_property_string("sub-fonts-dir", &styling.sub_fonts_dir);
        let _ = self.set_property("sub-margin-y", styling.margin_y.clamp(0, 300));
        let _ = self.set_property("sub-margin-x", styling.margin_x.clamp(0, 300));
        Ok(())
    }

    /// Resets all subtitle styling properties back to engine defaults.
    pub fn reset_subtitle_styling(&self) -> Result<(), MpvError> {
        self.apply_subtitle_styling(&SubtitleStyling::default())
    }

    /// Extracts embedded lyrics from container metadata tags if available.
    ///
    /// Inspects standard container metadata keys across ID3v2 (USLT / lyrics),
    /// Vorbis comments (LYRICS / UNSYNCEDLYRICS), and MP4 atoms (©lyr).
    pub fn get_embedded_lyrics(&self) -> Option<String> {
        // 1. Direct query on standard metadata keys
        const KEYS: &[&str] = &[
            "lyrics",
            "LYRICS",
            "unsyncedlyrics",
            "UNSYNCEDLYRICS",
            "©lyr",
            "\u{00a9}lyr",
            "USLT",
            "lyrics-eng",
            "lyrics-und",
        ];
        for key in KEYS {
            if let Ok(val) = self.get_property_string(&format!("metadata/by-key/{key}")) {
                let trimmed = val.trim();
                if !trimmed.is_empty() {
                    return Some(val);
                }
            }
        }

        // 2. Scan indexed metadata list if available
        if let Ok(count) = self.get_property::<i64>("metadata/list/count") {
            for i in 0..count {
                if let Ok(key) = self.get_property_string(&format!("metadata/list/{i}/key")) {
                    let k = key.to_ascii_lowercase();
                    if k == "lyrics"
                        || k == "unsyncedlyrics"
                        || k == "uslt"
                        || k == "©lyr"
                        || k.contains("lyric")
                    {
                        if let Ok(val) =
                            self.get_property_string(&format!("metadata/list/{i}/value"))
                        {
                            let trimmed = val.trim();
                            if !trimmed.is_empty() {
                                return Some(val);
                            }
                        }
                    }
                }
            }
        }

        None
    }
}

/// ASS/SSA script styling override policy.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssOverrideMode {
    /// Strictly preserves all author styling, positioning, signs, and karaoke animations.
    #[default]
    Preserve,
    /// Proportional font scaling while preserving author colors and layout.
    SafeScale,
    /// Overrides author fonts and colors with user styling preferences.
    Force,
    /// Strips all ASS styling completely into basic plain text.
    Strip,
}

impl AssOverrideMode {
    pub fn as_mpv_str(&self) -> &'static str {
        match self {
            Self::Preserve => "no",
            Self::SafeScale => "scale",
            Self::Force => "force",
            Self::Strip => "strip",
        }
    }
}

/// Horizontal subtitle justification.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubtitleJustify {
    #[default]
    Auto,
    Left,
    Center,
    Right,
}

impl SubtitleJustify {
    pub fn as_mpv_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Left => "left",
            Self::Center => "center",
            Self::Right => "right",
        }
    }
}

/// User-configurable subtitle appearance settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SubtitleStyling {
    /// Subtitle font family name.
    pub font: String,
    /// Font size in points (e.g. 20..90, default 45).
    pub font_size: i64,
    /// Primary font fill color (hex format `#RRGGBB` or `#AARRGGBB`).
    pub color: String,
    /// Border outline color.
    pub border_color: String,
    /// Border outline thickness in points (0.0..10.0).
    pub border_size: f64,
    /// Vertical placement position (0 = top, 100 = bottom).
    pub pos: i64,
    /// Scale multiplier factor (0.5..2.5).
    pub scale: f64,
    /// Whether to force these styles over embedded ASS script styles (legacy flag).
    pub ass_override: bool,
    /// Detailed ASS styling override policy.
    pub ass_override_mode: AssOverrideMode,
    /// Secondary subtitle vertical position (default 10 = top).
    pub secondary_pos: i64,
    /// Subtitle rendering speed multiplier (0.5..=2.0).
    pub sub_speed: f64,
    /// Subtitle shadow offset in points (0.0..=10.0).
    pub sub_shadow_offset: f64,
    /// Subtitle background color (`#RRGGBB` or `#AARRGGBB`).
    pub sub_back_color: String,
    /// Horizontal justification mode.
    pub sub_justify: SubtitleJustify,
    /// Optional custom fonts directory.
    pub sub_fonts_dir: String,
    /// Vertical margin in pixels (0..=300, default 36).
    pub margin_y: i64,
    /// Horizontal margin in pixels (0..=300, default 25).
    pub margin_x: i64,
    /// Shadow color (hex format `#RRGGBB` or `#AARRGGBB`, default `#80000000`).
    pub shadow_color: String,
}

impl Default for SubtitleStyling {
    fn default() -> Self {
        Self {
            font: "sans-serif".to_string(),
            font_size: 45,
            color: "#FFFFFF".to_string(),
            border_color: "#000000".to_string(),
            border_size: 3.0,
            pos: 100,
            scale: 1.0,
            ass_override: false,
            ass_override_mode: AssOverrideMode::Preserve,
            secondary_pos: 10,
            sub_speed: 1.0,
            sub_shadow_offset: 0.0,
            sub_back_color: "#00000000".to_string(),
            sub_justify: SubtitleJustify::Auto,
            sub_fonts_dir: String::new(),
            margin_y: 36,
            margin_x: 25,
            shadow_color: "#80000000".to_string(),
        }
    }
}
