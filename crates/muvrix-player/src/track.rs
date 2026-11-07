//! Strongly-typed representations of media tracks and chapters.

use crate::error::MpvError;
use crate::handle::MpvHandle;

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
}
