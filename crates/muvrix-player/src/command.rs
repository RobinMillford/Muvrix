//! High-level command helpers for common playback operations.

use crate::error::MpvError;
use crate::handle::MpvHandle;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum LoadFileMode {
    Replace,
    Append,
    AppendPlay,
}

impl LoadFileMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Replace => "replace",
            Self::Append => "append",
            Self::AppendPlay => "append-play",
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SeekMode {
    Relative,
    Absolute,
    AbsolutePercent,
    Exact,
}

impl SeekMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Relative => "relative",
            Self::Absolute => "absolute",
            Self::AbsolutePercent => "absolute-percent",
            Self::Exact => "exact",
        }
    }
}

impl MpvHandle {
    /// Loads a media file or URL.
    pub fn load_file(&self, url: &str, mode: LoadFileMode) -> Result<(), MpvError> {
        self.command(&["loadfile", url, mode.as_str()])
    }

    /// Resumes playback (`pause=no`).
    pub fn play(&self) -> Result<(), MpvError> {
        self.set_property("pause", false)
    }

    /// Pauses playback (`pause=yes`).
    pub fn pause(&self) -> Result<(), MpvError> {
        self.set_property("pause", true)
    }

    /// Toggles playback pause state.
    pub fn toggle_pause(&self) -> Result<(), MpvError> {
        self.command(&["cycle", "pause"])
    }

    /// Seeks to a given position.
    pub fn seek(&self, seconds: f64, mode: SeekMode) -> Result<(), MpvError> {
        let sec_str = seconds.to_string();
        self.command(&["seek", &sec_str, mode.as_str()])
    }

    /// Stops playback.
    pub fn stop(&self) -> Result<(), MpvError> {
        self.command(&["stop"])
    }
}
