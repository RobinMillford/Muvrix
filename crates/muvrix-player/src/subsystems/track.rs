//! Track, Subtitle, and Chapter domain subsystem.

use crate::error::MpvError;
use crate::handle::MpvHandle;
use crate::track::{ChapterInfo, SubtitleStyling, TrackInfo, TrackType};

/// Subsystem managing audio/video/subtitle track selection, dual subtitles,
/// subtitle styling, and chapter navigation.
#[derive(Debug, Clone)]
pub struct TrackSubsystem {
    tracks: Vec<TrackInfo>,
    chapters: Vec<ChapterInfo>,
    current_chapter: Option<i64>,
    secondary_sub_track_id: Option<i64>,
    sub_delay: f64,
    sub_visibility: bool,
    subtitle_styling: SubtitleStyling,
}

impl Default for TrackSubsystem {
    fn default() -> Self {
        Self::new()
    }
}

impl TrackSubsystem {
    /// Creates a new `TrackSubsystem` with default values.
    pub fn new() -> Self {
        Self {
            tracks: Vec::new(),
            chapters: Vec::new(),
            current_chapter: None,
            secondary_sub_track_id: None,
            sub_delay: 0.0,
            sub_visibility: true,
            subtitle_styling: SubtitleStyling::default(),
        }
    }

    /// Returns all available media tracks.
    pub fn tracks(&self) -> &[TrackInfo] {
        &self.tracks
    }

    /// Returns all chapters for the currently loaded file.
    pub fn chapters(&self) -> &[ChapterInfo] {
        &self.chapters
    }

    /// Returns the active chapter index, if known.
    pub fn current_chapter(&self) -> Option<i64> {
        self.current_chapter
    }

    /// Returns the total number of chapters in the current file.
    pub fn chapter_count(&self) -> usize {
        self.chapters.len()
    }

    /// Returns the currently active track of the requested type.
    pub fn selected_track(&self, track_type: TrackType) -> Option<&TrackInfo> {
        self.tracks
            .iter()
            .find(|t| t.track_type == track_type && t.selected)
    }

    /// Returns all tracks matching the requested type.
    pub fn tracks_by_type(&self, track_type: TrackType) -> Vec<&TrackInfo> {
        self.tracks
            .iter()
            .filter(|t| t.track_type == track_type)
            .collect()
    }

    /// Selects a track by ID, or deselects when `id` is `None`.
    pub fn set_track(
        &mut self,
        handle: &MpvHandle,
        track_type: TrackType,
        id: Option<i64>,
    ) -> Result<(), MpvError> {
        match track_type {
            TrackType::Video => handle.set_video_track(id),
            TrackType::Audio => handle.set_audio_track(id),
            TrackType::Subtitle => handle.set_sub_track(id),
        }
    }

    /// Sets the active primary subtitle track.
    pub fn set_subtitle_track(
        &mut self,
        handle: &MpvHandle,
        id: Option<i64>,
    ) -> Result<(), MpvError> {
        self.set_track(handle, TrackType::Subtitle, id)
    }

    /// Sets the active secondary subtitle track.
    pub fn set_secondary_subtitle_track(
        &mut self,
        handle: &MpvHandle,
        id: Option<i64>,
    ) -> Result<(), MpvError> {
        handle.set_secondary_sub_track(id)?;
        self.secondary_sub_track_id = id;
        Ok(())
    }

    /// Sets the active audio track.
    pub fn set_audio_track(&mut self, handle: &MpvHandle, id: Option<i64>) -> Result<(), MpvError> {
        self.set_track(handle, TrackType::Audio, id)
    }

    /// Sets the active video track.
    pub fn set_video_track(&mut self, handle: &MpvHandle, id: Option<i64>) -> Result<(), MpvError> {
        self.set_track(handle, TrackType::Video, id)
    }

    /// Loads an external subtitle file.
    pub fn add_subtitle_file(
        &mut self,
        handle: &MpvHandle,
        path: &str,
        select: bool,
    ) -> Result<(), MpvError> {
        let flag = if select { "select" } else { "auto" };
        handle.command(&["sub-add", path, flag])
    }

    /// Sets the primary subtitle delay in seconds.
    pub fn set_subtitle_delay(
        &mut self,
        handle: &MpvHandle,
        delay_seconds: f64,
    ) -> Result<(), MpvError> {
        handle.set_sub_delay(delay_seconds)?;
        self.sub_delay = delay_seconds;
        Ok(())
    }

    /// Returns the cached primary subtitle delay.
    pub fn sub_delay(&self) -> f64 {
        self.sub_delay
    }

    /// Sets primary subtitle visibility.
    pub fn set_subtitle_visibility(
        &mut self,
        handle: &MpvHandle,
        visible: bool,
    ) -> Result<(), MpvError> {
        handle.set_sub_visibility(visible)?;
        self.sub_visibility = visible;
        Ok(())
    }

    /// Returns the cached primary subtitle visibility.
    pub fn sub_visibility(&self) -> bool {
        self.sub_visibility
    }

    /// Returns the active secondary subtitle track ID, if any.
    pub fn secondary_sub_track_id(&self) -> Option<i64> {
        self.secondary_sub_track_id
    }

    /// Sets secondary subtitle delay in seconds.
    pub fn set_secondary_sub_delay(
        &self,
        handle: &MpvHandle,
        delay_seconds: f64,
    ) -> Result<(), MpvError> {
        handle.set_secondary_sub_delay(delay_seconds)
    }

    /// Sets secondary subtitle visibility.
    pub fn set_secondary_sub_visibility(
        &self,
        handle: &MpvHandle,
        visible: bool,
    ) -> Result<(), MpvError> {
        handle.set_secondary_sub_visibility(visible)
    }

    /// Configures dual-subtitle collision avoidance so primary subtitles render
    /// at the bottom and secondary subtitles at the top.
    pub fn configure_dual_subtitle_collision_avoidance(
        &self,
        handle: &MpvHandle,
    ) -> Result<(), MpvError> {
        handle.configure_dual_subtitle_collision_avoidance()
    }

    /// Returns the current subtitle styling settings.
    pub fn subtitle_styling(&self) -> &SubtitleStyling {
        &self.subtitle_styling
    }

    /// Applies custom subtitle styling across primary and secondary subtitles.
    pub fn set_subtitle_styling(
        &mut self,
        handle: &MpvHandle,
        styling: SubtitleStyling,
    ) -> Result<(), MpvError> {
        handle.apply_subtitle_styling(&styling)?;
        self.subtitle_styling = styling;
        Ok(())
    }

    /// Resets subtitle styling to engine defaults.
    pub fn reset_subtitle_styling(&mut self, handle: &MpvHandle) -> Result<(), MpvError> {
        handle.reset_subtitle_styling()?;
        self.subtitle_styling = SubtitleStyling::default();
        Ok(())
    }

    /// Jumps to the next chapter.
    pub fn next_chapter(&mut self, handle: &MpvHandle) -> Result<(), MpvError> {
        handle.command(&["add", "chapter", "1"])
    }

    /// Jumps to the previous chapter.
    pub fn prev_chapter(&mut self, handle: &MpvHandle) -> Result<(), MpvError> {
        handle.command(&["add", "chapter", "-1"])
    }

    /// Jumps to an explicit chapter index.
    pub fn set_chapter(&mut self, handle: &MpvHandle, index: i64) -> Result<(), MpvError> {
        let index_str = index.to_string();
        handle.command(&["set", "chapter", &index_str])
    }

    // Property and Event synchronization callbacks
    pub fn on_track_list_changed(&mut self, tracks: Vec<TrackInfo>) {
        self.tracks = tracks;
    }

    pub fn on_chapter_list_changed(&mut self, chapters: Vec<ChapterInfo>) {
        self.chapters = chapters;
    }

    pub fn on_chapter_changed(&mut self, chapter: Option<i64>) {
        self.current_chapter = chapter;
    }

    pub fn on_sub_delay_changed(&mut self, delay: f64) {
        self.sub_delay = delay;
    }

    pub fn on_sub_visibility_changed(&mut self, visibility: bool) {
        self.sub_visibility = visibility;
    }

    pub fn on_secondary_sid_changed(&mut self, sid: Option<i64>) {
        self.secondary_sub_track_id = sid;
    }

    /// Refreshes track and chapter caches from the mpv handle.
    pub fn refresh(&mut self, handle: &MpvHandle) {
        if let Ok(tracks) = handle.get_track_list() {
            self.tracks = tracks;
        }
        if let Ok(chapters) = handle.get_chapter_list() {
            self.chapters = chapters;
        }
        if let Ok(ch) = handle.get_property::<i64>("chapter") {
            self.current_chapter = if ch >= 0 { Some(ch) } else { None };
        }
        self.secondary_sub_track_id = handle.get_secondary_sub_track();
    }
}
