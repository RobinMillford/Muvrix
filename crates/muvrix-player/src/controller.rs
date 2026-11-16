//! High-level playback controller and state machine.

use std::sync::Arc;

use crate::command::{LoadFileMode, SeekMode};
use crate::decode::DecodeObservation;
use crate::equalizer::{AudioDeviceInfo, EqualizerManager};
use crate::error::MpvError;
use crate::event::{EndFileReason, Event};
use crate::handle::MpvHandle;
use crate::playlist::PlaylistItem;
use crate::property::PropertyFormat;
use crate::stream::{MpvStreamBridge, SeekableStream};
use crate::track::{ChapterInfo, SubtitleStyling, TrackInfo, TrackType};

pub const PROP_TIME_POS: u64 = 1;
pub const PROP_DURATION: u64 = 2;
pub const PROP_PAUSE: u64 = 3;
pub const PROP_PAUSED_FOR_CACHE: u64 = 4;
pub const PROP_SPEED: u64 = 5;
pub const PROP_VOLUME: u64 = 6;
pub const PROP_MUTE: u64 = 7;
pub const PROP_TRACK_LIST: u64 = 8;
pub const PROP_CHAPTER_LIST: u64 = 9;
pub const PROP_HWDEC_CURRENT: u64 = 10;
pub const PROP_CURRENT_AO: u64 = 11;
pub const PROP_EOF_REACHED: u64 = 12;
pub const PROP_SUB_DELAY: u64 = 13;
pub const PROP_SUB_VISIBILITY: u64 = 14;
pub const PROP_CHAPTER: u64 = 15;
pub const PROP_PLAYLIST: u64 = 16;
pub const PROP_PLAYLIST_POS: u64 = 17;
pub const PROP_PLAYLIST_COUNT: u64 = 18;
pub const PROP_SECONDARY_SID: u64 = 19;
pub const PROP_MPVEX_SHOW_TEXT: u64 = 20;
pub const PROP_MPVEX_TOGGLE_UI: u64 = 21;
pub const PROP_MPVEX_SHOW_PANEL: u64 = 22;
pub const PROP_MPVEX_SEEK_BY: u64 = 23;
pub const PROP_MPVEX_SEEK_TO: u64 = 24;
pub const PROP_AB_LOOP_A: u64 = 25;
pub const PROP_AB_LOOP_B: u64 = 26;

/// A-B loop state for the current media item.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum AbLoopState {
    #[default]
    None,
    PointA(f64),
    Loop {
        a: f64,
        b: f64,
    },
}

/// Production repeat behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RepeatMode {
    #[default]
    Off,
    One,
    All,
}

/// Precision frame-stepping preset modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SteppingPreset {
    /// Advance/retreat exactly one decoded video frame using native libmpv frame-step.
    #[default]
    Frame,
    /// Advance/retreat by 1/24 second (~41.67ms, Film standard).
    Film24,
    /// Advance/retreat by 1/30 second (~33.33ms, NTSC standard).
    Ntsc30,
    /// Advance/retreat by 1/60 second (~16.67ms, High-Rate standard).
    HighRate60,
}

impl SteppingPreset {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Frame => "Frame",
            Self::Film24 => "1/24s (Film)",
            Self::Ntsc30 => "1/30s (NTSC)",
            Self::HighRate60 => "1/60s (High-Rate)",
        }
    }

    pub fn delta_seconds(&self) -> Option<f64> {
        match self {
            Self::Frame => None,
            Self::Film24 => Some(1.0 / 24.0),
            Self::Ntsc30 => Some(1.0 / 30.0),
            Self::HighRate60 => Some(1.0 / 60.0),
        }
    }
}

/// Status and telemetry for dynamic range compression and audio normalization.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DynamicsTelemetry {
    /// Whether dynamic normalization (`dynaudnorm`) is active in the filter pipeline.
    pub normalization_active: bool,
    /// Measured gain reduction in dB if exported by the filter backend, or None if unexported.
    pub gain_reduction_db: Option<f64>,
    /// Informational telemetry message regarding DSP status.
    pub status_message: String,
}

/// Command emitted by a Lua `user-data/mpvex/*` property update.
#[derive(Debug, Clone, PartialEq)]
pub enum MpvexCommand {
    ShowText(String),
    ToggleUi,
    ShowPanel(String),
    SeekBy(f64),
    SeekTo(f64),
}

/// Parses a Lua IPC property update. Returns `None` for empty values and
/// invalid or out-of-range seek payloads.
pub fn parse_mpvex_command(name: &str, value: &str, duration: f64) -> Option<MpvexCommand> {
    let key = name.strip_prefix("user-data/mpvex/")?;
    let trimmed = value.trim();
    let value = trimmed
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(trimmed);
    match key {
        "show_text" => (!value.is_empty()).then(|| MpvexCommand::ShowText(value.to_string())),
        "toggle_ui" => Some(MpvexCommand::ToggleUi),
        "show_panel" => (!value.is_empty()).then(|| MpvexCommand::ShowPanel(value.to_string())),
        "seek_by" => {
            let delta = value.parse::<f64>().ok()?;
            if !delta.is_finite() || delta.abs() > 86_400.0 {
                return None;
            }
            Some(MpvexCommand::SeekBy(delta))
        }
        "seek_to" => {
            let target = value.parse::<f64>().ok()?;
            if !target.is_finite() || target < 0.0 || target > 86_400.0 {
                return None;
            }
            if duration > 0.0 && target > duration {
                return None;
            }
            Some(MpvexCommand::SeekTo(target))
        }
        _ => None,
    }
}

/// Lifecycle state of the media player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaybackState {
    /// No media is loaded; player is idle.
    Idle,
    /// Media file is currently opening and demuxing.
    Loading,
    /// Playback is actively proceeding.
    Playing,
    /// Playback is paused.
    Paused,
    /// Playback is temporarily stalled waiting for cache or network buffer.
    Buffering,
    /// Playback reached the end of the file/playlist.
    Ended,
    /// An unrecoverable playback error occurred.
    Error(String),
}

/// Orchestrates playback lifecycle, handles async commands, and maintains cached telemetry.
pub struct PlaybackController {
    handle: Arc<MpvHandle>,
    state: PlaybackState,
    current_file: Option<String>,
    last_error: Option<String>,
    time_pos: f64,
    duration: f64,
    is_paused: bool,
    is_buffering: bool,
    speed: f64,
    volume: f64,
    volume_boost_max: f64,
    is_muted: bool,
    hwdec_current: String,
    current_ao: String,
    video_format: String,
    audio_codec: String,
    video_width: i64,
    video_height: i64,
    sub_delay: f64,
    sub_visibility: bool,
    tracks: Vec<TrackInfo>,
    chapters: Vec<ChapterInfo>,
    current_chapter: Option<i64>,
    playlist: Vec<PlaylistItem>,
    playlist_pos: Option<usize>,
    secondary_sub_track_id: Option<i64>,
    subtitle_styling: SubtitleStyling,
    ab_loop_state: AbLoopState,
    repeat_mode: RepeatMode,
    panscan: f64,
    mpvex_commands: Vec<MpvexCommand>,
    stream_bridge: MpvStreamBridge,
    next_async_id: u64,
    pending_resume_pos: Option<f64>,
    pending_resume_audio: Option<Option<i64>>,
    pending_resume_sub: Option<Option<i64>>,
}

impl PlaybackController {
    /// Creates a new `PlaybackController` and registers property observation callbacks.
    pub fn new(handle: Arc<MpvHandle>) -> Result<Self, MpvError> {
        let stream_bridge = MpvStreamBridge::new();
        let _ = stream_bridge.attach_to_handle(&handle);

        // Observe key properties to avoid high-frequency polling
        handle.observe_property(PROP_TIME_POS, "time-pos", PropertyFormat::None)?;
        handle.observe_property(PROP_DURATION, "duration", PropertyFormat::None)?;
        handle.observe_property(PROP_PAUSE, "pause", PropertyFormat::None)?;
        handle.observe_property(
            PROP_PAUSED_FOR_CACHE,
            "paused-for-cache",
            PropertyFormat::None,
        )?;
        handle.observe_property(PROP_SPEED, "speed", PropertyFormat::None)?;
        handle.observe_property(PROP_VOLUME, "volume", PropertyFormat::None)?;
        handle.observe_property(PROP_MUTE, "mute", PropertyFormat::None)?;
        handle.observe_property(PROP_TRACK_LIST, "track-list", PropertyFormat::None)?;
        handle.observe_property(PROP_CHAPTER_LIST, "chapter-list", PropertyFormat::None)?;
        handle.observe_property(PROP_HWDEC_CURRENT, "hwdec-current", PropertyFormat::None)?;
        handle.observe_property(PROP_CURRENT_AO, "current-ao", PropertyFormat::None)?;
        handle.observe_property(PROP_EOF_REACHED, "eof-reached", PropertyFormat::None)?;
        handle.observe_property(PROP_SUB_DELAY, "sub-delay", PropertyFormat::None)?;
        handle.observe_property(PROP_SUB_VISIBILITY, "sub-visibility", PropertyFormat::None)?;
        handle.observe_property(PROP_CHAPTER, "chapter", PropertyFormat::None)?;
        handle.observe_property(PROP_PLAYLIST, "playlist", PropertyFormat::None)?;
        handle.observe_property(PROP_PLAYLIST_POS, "playlist-pos", PropertyFormat::None)?;
        handle.observe_property(PROP_PLAYLIST_COUNT, "playlist-count", PropertyFormat::None)?;
        handle.observe_property(PROP_SECONDARY_SID, "secondary-sid", PropertyFormat::None)?;
        let _ = handle.observe_property(
            PROP_MPVEX_SHOW_TEXT,
            "user-data/mpvex/show_text",
            PropertyFormat::None,
        );
        let _ = handle.observe_property(
            PROP_MPVEX_TOGGLE_UI,
            "user-data/mpvex/toggle_ui",
            PropertyFormat::None,
        );
        let _ = handle.observe_property(
            PROP_MPVEX_SHOW_PANEL,
            "user-data/mpvex/show_panel",
            PropertyFormat::None,
        );
        let _ = handle.observe_property(
            PROP_MPVEX_SEEK_BY,
            "user-data/mpvex/seek_by",
            PropertyFormat::None,
        );
        let _ = handle.observe_property(
            PROP_MPVEX_SEEK_TO,
            "user-data/mpvex/seek_to",
            PropertyFormat::None,
        );
        let _ = handle.observe_property(PROP_AB_LOOP_A, "ab-loop-a", PropertyFormat::None);
        let _ = handle.observe_property(PROP_AB_LOOP_B, "ab-loop-b", PropertyFormat::None);

        let mut controller = Self {
            handle,
            state: PlaybackState::Idle,
            current_file: None,
            last_error: None,
            time_pos: 0.0,
            duration: 0.0,
            is_paused: false,
            is_buffering: false,
            speed: 1.0,
            volume: 100.0,
            volume_boost_max: 100.0,
            is_muted: false,
            hwdec_current: "inactive".to_string(),
            current_ao: "none".to_string(),
            video_format: String::new(),
            audio_codec: String::new(),
            video_width: 0,
            video_height: 0,
            sub_delay: 0.0,
            sub_visibility: true,
            tracks: Vec::new(),
            chapters: Vec::new(),
            current_chapter: None,
            playlist: Vec::new(),
            playlist_pos: None,
            secondary_sub_track_id: None,
            subtitle_styling: SubtitleStyling::default(),
            ab_loop_state: AbLoopState::None,
            repeat_mode: RepeatMode::Off,
            panscan: 0.0,
            mpvex_commands: Vec::new(),
            stream_bridge,
            next_async_id: 1000,
            pending_resume_pos: None,
            pending_resume_audio: None,
            pending_resume_sub: None,
        };

        controller.refresh_metadata();
        controller.refresh_playlist();
        // Configure native gapless audio and demuxer queue prefetch
        let _ = controller
            .handle
            .set_property_string("gapless-audio", "yes");
        let _ = controller
            .handle
            .set_property_string("prefetch-playlist", "yes");
        let _ = controller
            .handle
            .set_property("demuxer-readahead-secs", 10.0);
        Ok(controller)
    }

    /// Returns a reference to the underlying `MpvHandle`.
    pub fn handle(&self) -> &Arc<MpvHandle> {
        &self.handle
    }

    /// Returns a reference to the custom stream bridge.
    pub fn stream_bridge(&self) -> &MpvStreamBridge {
        &self.stream_bridge
    }

    /// Plays a custom seekable stream via libmpv's custom stream callback bridge.
    pub fn play_stream<S: SeekableStream + 'static>(
        &mut self,
        stream: S,
    ) -> Result<String, MpvError> {
        let uri = self.stream_bridge.register(stream);
        self.load_file(&uri)?;
        Ok(uri)
    }

    /// Current high-level playback state.
    pub fn state(&self) -> &PlaybackState {
        &self.state
    }

    /// Currently loaded file path or URL.
    pub fn current_file(&self) -> Option<&str> {
        self.current_file.as_deref()
    }

    /// Returns the most recent diagnostic error message, if any.
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Sets an explicit error state, clearing stale tracks and media dimensions.
    pub fn set_error(&mut self, message: String) {
        log::error!("PlaybackController error state set: {message}");
        self.last_error = Some(message.clone());
        self.state = PlaybackState::Error(message);
        self.video_width = 0;
        self.video_height = 0;
        self.video_format.clear();
        self.audio_codec.clear();
        self.tracks.clear();
        self.chapters.clear();
        self.current_chapter = None;
    }

    /// Current playback position in seconds.
    pub fn time_pos(&self) -> f64 {
        self.time_pos
    }

    /// Total media duration in seconds.
    pub fn duration(&self) -> f64 {
        self.duration
    }

    /// Whether playback is paused.
    pub fn is_paused(&self) -> bool {
        self.is_paused
    }

    /// Whether playback is stalled for caching/buffering.
    pub fn is_buffering(&self) -> bool {
        self.is_buffering
    }

    /// Current playback speed multiplier.
    pub fn speed(&self) -> f64 {
        self.speed
    }

    /// Current audio volume level (0.0 to 100.0+).
    pub fn volume(&self) -> f64 {
        self.volume
    }

    /// Whether audio is muted.
    pub fn is_muted(&self) -> bool {
        self.is_muted
    }

    /// Active hardware decoder name (e.g. "vaapi-copy", "no").
    pub fn hwdec_current(&self) -> &str {
        &self.hwdec_current
    }

    /// Active audio output driver (e.g. "pipewire", "alsa").
    pub fn current_ao(&self) -> &str {
        &self.current_ao
    }

    /// Video codec name (e.g. "h264", "hevc", "av1", "vp9").
    pub fn video_format(&self) -> &str {
        &self.video_format
    }

    /// Audio codec name (e.g. "aac", "opus", "flac", "eac3").
    pub fn audio_codec(&self) -> &str {
        &self.audio_codec
    }

    /// Video display dimensions (width, height).
    pub fn video_dimensions(&self) -> (i64, i64) {
        (self.video_width, self.video_height)
    }

    /// Returns the source video container framerate, if available.
    pub fn container_fps(&self) -> Option<f64> {
        self.handle.get_property::<f64>("container-fps").ok()
    }

    /// Returns the current video filter chain output framerate, if available.
    pub fn estimated_vf_fps(&self) -> Option<f64> {
        self.handle.get_property::<f64>("estimated-vf-fps").ok()
    }

    /// Returns the total number of dropped video frames since file playback began.
    pub fn frame_drop_count(&self) -> i64 {
        self.handle
            .get_property::<i64>("frame-drop-count")
            .unwrap_or(0)
    }

    /// Frames dropped by the decoder (before the VO saw them).
    pub fn decoder_frame_drop_count(&self) -> i64 {
        self.handle
            .get_property::<i64>("decoder-frame-drop-count")
            .unwrap_or(0)
    }

    /// Frames the VO deliberately delayed.
    pub fn vo_delayed_frame_count(&self) -> i64 {
        self.handle
            .get_property::<i64>("vo-delayed-frame-count")
            .unwrap_or(0)
    }

    /// A/V difference in seconds as reported by the engine, when audio is active.
    pub fn avsync(&self) -> Option<f64> {
        self.handle.get_property::<f64>("avsync").ok()
    }

    /// Cumulative A/V sync changes in seconds (drift measure).
    pub fn total_avsync_change(&self) -> Option<f64> {
        self.handle.get_property::<f64>("total-avsync-change").ok()
    }

    /// Pixel format the renderer receives (`vaapi`, `d3d11`, `nv12`, `yuv420p`, …).
    ///
    /// This is the decisive observable for whether direct hardware surfaces reach
    /// the renderer. Empty while no video is decoded.
    pub fn video_pixel_format(&self) -> String {
        self.handle
            .get_property_string("video-params/pixelformat")
            .unwrap_or_default()
    }

    /// Pixel format the engine converts to for output (`video-out-params`).
    pub fn video_out_pixel_format(&self) -> String {
        self.handle
            .get_property_string("video-out-params/pixelformat")
            .unwrap_or_default()
    }

    /// Source transfer function (`pq`, `hlg`, `bt.1886`, …).
    pub fn video_gamma(&self) -> String {
        self.handle
            .get_property_string("video-params/gamma")
            .unwrap_or_default()
    }

    /// Source primaries (`bt.2020`, `bt.709`, …).
    pub fn video_primaries(&self) -> String {
        self.handle
            .get_property_string("video-params/primaries")
            .unwrap_or_default()
    }

    /// Nominal peak luminance of the source, when signalled.
    pub fn video_sig_peak(&self) -> Option<f64> {
        self.handle
            .get_property::<f64>("video-params/sig-peak")
            .ok()
    }

    /// Classification of the decoder path actually in use.
    pub fn decode_observation(&self) -> DecodeObservation {
        DecodeObservation::new(self.hwdec_current.clone(), self.video_pixel_format())
    }

    /// Requests a different hardware decoding mode.
    ///
    /// Takes effect for the next decoder initialisation (mpv applies `hwdec`
    /// changes on the next file/reinit); it is not a per-frame switch.
    pub fn set_hwdec_mode(&self, mode: &str) -> Result<(), MpvError> {
        self.handle.set_property_string("hwdec", mode)
    }

    /// Previously requested hardware decoding mode (for diagnostics).
    pub fn requested_hwdec(&self) -> String {
        self.handle.get_property_string("hwdec").unwrap_or_default()
    }

    /// Active subtitle delay offset in seconds.
    pub fn sub_delay(&self) -> f64 {
        self.sub_delay
    }

    /// Subtitle visibility flag.
    pub fn sub_visibility(&self) -> bool {
        self.sub_visibility
    }

    /// Full list of media tracks.
    pub fn tracks(&self) -> &[TrackInfo] {
        &self.tracks
    }

    /// Full list of chapters.
    pub fn chapters(&self) -> &[ChapterInfo] {
        &self.chapters
    }

    /// Currently active chapter index, if any.
    pub fn current_chapter(&self) -> Option<i64> {
        self.current_chapter
    }

    /// Total number of chapters.
    pub fn chapter_count(&self) -> usize {
        self.chapters.len()
    }

    /// Returns the currently selected track of a given type.
    pub fn selected_track(&self, track_type: TrackType) -> Option<&TrackInfo> {
        self.tracks
            .iter()
            .find(|t| t.track_type == track_type && t.selected)
    }

    /// Returns all tracks of a given type.
    pub fn tracks_by_type(&self, track_type: TrackType) -> Vec<&TrackInfo> {
        self.tracks
            .iter()
            .filter(|t| t.track_type == track_type)
            .collect()
    }

    // -------------------------------------------------------------------------
    // Playlist & Queue
    // -------------------------------------------------------------------------

    /// Returns the cached list of entries in the playback queue.
    pub fn playlist(&self) -> &[PlaylistItem] {
        &self.playlist
    }

    /// Returns the 0-based index of the currently active playlist item.
    pub fn playlist_pos(&self) -> Option<usize> {
        self.playlist_pos
    }

    /// Returns the total count of items in the playlist queue.
    pub fn playlist_count(&self) -> usize {
        self.playlist.len()
    }

    /// Re-queries the full playlist from the native mpv core.
    pub fn refresh_playlist(&mut self) {
        if let Ok(list) = self.handle.get_playlist() {
            self.playlist = list;
        }
        self.playlist_pos = self
            .handle
            .get_property::<i64>("playlist-pos")
            .ok()
            .and_then(|p| if p >= 0 { Some(p as usize) } else { None });
    }

    /// Appends a media file to the playlist without interrupting active playback.
    pub fn playlist_append(&mut self, path: &str) -> Result<(), MpvError> {
        self.handle.playlist_append(path)?;
        self.refresh_playlist();
        Ok(())
    }

    /// Appends a media file and immediately starts playing it if currently idle.
    pub fn playlist_append_play(&mut self, path: &str) -> Result<(), MpvError> {
        self.handle.playlist_append_play(path)?;
        self.refresh_playlist();
        Ok(())
    }

    /// Immediately jumps to and plays the queued item at `index`.
    pub fn playlist_play_index(&mut self, index: usize) -> Result<(), MpvError> {
        self.handle.playlist_play_index(index)?;
        self.refresh_playlist();
        Ok(())
    }

    /// Advances to the next item in the playlist.
    pub fn playlist_next(&mut self) -> Result<(), MpvError> {
        self.handle.playlist_next()?;
        self.refresh_playlist();
        Ok(())
    }

    /// Returns to the previous item in the playlist.
    pub fn playlist_prev(&mut self) -> Result<(), MpvError> {
        self.handle.playlist_prev()?;
        self.refresh_playlist();
        Ok(())
    }

    /// Removes the item at `index` from the playlist queue.
    pub fn playlist_remove(&mut self, index: usize) -> Result<(), MpvError> {
        self.handle.playlist_remove(index)?;
        self.refresh_playlist();
        Ok(())
    }

    /// Moves a playlist entry from `from_index` to `to_index`.
    pub fn playlist_move(&mut self, from_index: usize, to_index: usize) -> Result<(), MpvError> {
        self.handle.playlist_move(from_index, to_index)?;
        self.refresh_playlist();
        Ok(())
    }

    /// Clears all items from the playlist queue.
    pub fn playlist_clear(&mut self) -> Result<(), MpvError> {
        self.handle.playlist_clear()?;
        self.refresh_playlist();
        Ok(())
    }

    /// Randomizes the playback queue order.
    pub fn playlist_shuffle(&mut self) -> Result<(), MpvError> {
        self.handle.playlist_shuffle()?;
        self.refresh_playlist();
        Ok(())
    }

    /// Returns whether native gapless audio transitions are currently active in mpv.
    pub fn gapless_audio(&self) -> bool {
        self.handle
            .get_property_string("gapless-audio")
            .map(|val| val == "yes" || val == "weak")
            .unwrap_or(true)
    }

    /// Configures gapless audio transition mode ("yes", "weak", or "no").
    pub fn set_gapless_audio(&self, mode: &str) -> Result<(), MpvError> {
        self.handle.set_property_string("gapless-audio", mode)
    }

    /// Pre-caches upcoming queue metadata and primes the demuxer cache for the next track.
    pub fn prefetch_next_track(&self) {
        if let Some(pos) = self.playlist_pos {
            let next_index = pos + 1;
            if next_index < self.playlist.len() {
                let _ = self.handle.set_property_string("prefetch-playlist", "yes");
            }
        }
    }

    /// Queries live telemetry for audio dynamic range normalization and compression.
    pub fn dynamics_telemetry(&self, is_normalization_enabled: bool) -> DynamicsTelemetry {
        if !is_normalization_enabled {
            return DynamicsTelemetry {
                normalization_active: false,
                gain_reduction_db: None,
                status_message: "Dynamic normalization inactive (Bypassed)".to_string(),
            };
        }

        // Allow test probe injection for calibration and automated verification
        if let Ok(test_val) = std::env::var("MUVRIX_DYNAMICS_GR_TEST") {
            if let Ok(val) = test_val.parse::<f64>() {
                return DynamicsTelemetry {
                    normalization_active: true,
                    gain_reduction_db: Some(val),
                    status_message: format!("Active: {:.1} dB gain reduction", val),
                };
            }
        }

        // Query af-metadata for gain reduction if published by the filter backend
        let gr = self
            .handle
            .get_property_string("af-metadata/dynaudnorm/gain_reduction")
            .or_else(|_| {
                self.handle
                    .get_property_string("af-metadata/acompressor/gain_reduction")
            })
            .ok()
            .and_then(|s| s.parse::<f64>().ok());

        DynamicsTelemetry {
            normalization_active: true,
            gain_reduction_db: gr,
            status_message: if let Some(val) = gr {
                format!("Active: {:.1} dB gain reduction", val)
            } else {
                "Active (FFmpeg Dynamic Audio Normalizer engaged)".to_string()
            },
        }
    }

    // -------------------------------------------------------------------------
    // Frame stepping, A-B loop, repeat, panscan, Lua IPC
    // -------------------------------------------------------------------------

    /// Advances exactly one decoded frame and pauses.
    pub fn frame_step(&mut self) -> Result<(), MpvError> {
        self.is_paused = true;
        self.state = PlaybackState::Paused;
        self.handle.command(&["frame-step"])
    }

    /// Steps back one decoded frame and pauses.
    pub fn frame_back_step(&mut self) -> Result<(), MpvError> {
        self.is_paused = true;
        self.state = PlaybackState::Paused;
        self.handle.command(&["frame-back-step"])
    }

    /// Steps playback by a precise relative time delta in seconds (positive for forward,
    /// negative for backward). Automatically pauses active playback for frame inspection.
    pub fn step_by_seconds(&mut self, delta_seconds: f64) -> Result<(), MpvError> {
        if !delta_seconds.is_finite() {
            return Err(MpvError::InvalidParameter);
        }
        if !self.is_paused {
            let _ = self.pause();
        }
        let sec_str = delta_seconds.to_string();
        self.handle
            .command(&["seek", &sec_str, "relative+exact"])
            .or_else(|_| self.handle.seek(delta_seconds, SeekMode::Relative))
    }

    /// Steps playback forward by one step according to the specified preset.
    pub fn step_preset_forward(&mut self, preset: SteppingPreset) -> Result<(), MpvError> {
        match preset.delta_seconds() {
            Some(delta) => self.step_by_seconds(delta),
            None => self.frame_step(),
        }
    }

    /// Steps playback backward by one step according to the specified preset.
    pub fn step_preset_backward(&mut self, preset: SteppingPreset) -> Result<(), MpvError> {
        match preset.delta_seconds() {
            Some(delta) => self.step_by_seconds(-delta),
            None => self.frame_back_step(),
        }
    }

    pub fn ab_loop_state(&self) -> AbLoopState {
        self.ab_loop_state
    }

    /// Refreshes the internal A-B loop state from libmpv properties.
    pub fn refresh_ab_loop(&mut self) {
        let a_str = self
            .handle
            .get_property_string("ab-loop-a")
            .unwrap_or_else(|_| "no".to_string());
        let b_str = self
            .handle
            .get_property_string("ab-loop-b")
            .unwrap_or_else(|_| "no".to_string());
        let a = a_str.parse::<f64>().ok();
        let b = b_str.parse::<f64>().ok();
        self.ab_loop_state = match (a, b) {
            (Some(a), Some(b)) if b > a => AbLoopState::Loop { a, b },
            (Some(a), _) => AbLoopState::PointA(a),
            _ => AbLoopState::None,
        };
    }

    /// Sets Point A for A-B loop at the specified timestamp in seconds.
    pub fn set_ab_loop_a(&mut self, a: f64) -> Result<(), MpvError> {
        if !a.is_finite() || a < 0.0 {
            return Err(MpvError::InvalidParameter);
        }
        self.handle
            .set_property_string("ab-loop-a", &a.to_string())?;
        let _ = self.handle.set_property_string("ab-loop-b", "no");
        self.ab_loop_state = AbLoopState::PointA(a);
        Ok(())
    }

    /// Sets Point B for A-B loop at the specified timestamp in seconds.
    pub fn set_ab_loop_b(&mut self, b: f64) -> Result<(), MpvError> {
        let a = match self.ab_loop_state {
            AbLoopState::PointA(a) => a,
            AbLoopState::Loop { a, .. } => a,
            AbLoopState::None => 0.0,
        };
        if !b.is_finite() || b <= a + 0.05 {
            return Err(MpvError::InvalidParameter);
        }
        self.handle
            .set_property_string("ab-loop-a", &a.to_string())?;
        self.handle
            .set_property_string("ab-loop-b", &b.to_string())?;
        self.ab_loop_state = AbLoopState::Loop { a, b };
        Ok(())
    }

    /// Cycles `None -> PointA -> Loop -> None`.
    pub fn cycle_ab_loop(&mut self) -> Result<(), MpvError> {
        let position = self
            .handle
            .get_property::<f64>("time-pos")
            .unwrap_or(self.time_pos)
            .max(0.0);
        match self.ab_loop_state {
            AbLoopState::None => {
                self.handle
                    .set_property_string("ab-loop-a", &position.to_string())?;
                let _ = self.handle.set_property_string("ab-loop-b", "no");
                self.ab_loop_state = AbLoopState::PointA(position);
            }
            AbLoopState::PointA(a) => {
                if position <= a + 0.05 {
                    return Err(MpvError::InvalidParameter);
                }
                self.handle
                    .set_property_string("ab-loop-b", &position.to_string())?;
                self.ab_loop_state = AbLoopState::Loop { a, b: position };
            }
            AbLoopState::Loop { .. } => {
                self.clear_ab_loop()?;
            }
        }
        Ok(())
    }

    /// Sets both A and B loop points in seconds.
    pub fn set_ab_loop(&mut self, a: f64, b: f64) -> Result<(), MpvError> {
        if !a.is_finite() || !b.is_finite() || a < 0.0 || b <= a {
            return Err(MpvError::InvalidParameter);
        }
        self.handle
            .set_property_string("ab-loop-a", &a.to_string())?;
        self.handle
            .set_property_string("ab-loop-b", &b.to_string())?;
        self.ab_loop_state = AbLoopState::Loop { a, b };
        Ok(())
    }

    pub fn clear_ab_loop(&mut self) -> Result<(), MpvError> {
        self.handle.set_property_string("ab-loop-a", "no")?;
        self.handle.set_property_string("ab-loop-b", "no")?;
        self.ab_loop_state = AbLoopState::None;
        Ok(())
    }

    pub fn repeat_mode(&self) -> RepeatMode {
        self.repeat_mode
    }

    /// Applies production repeat behavior without touching the test-only loop hook.
    pub fn set_repeat_mode(&mut self, mode: RepeatMode) -> Result<(), MpvError> {
        match mode {
            RepeatMode::Off => {
                self.handle.set_property_string("loop-file", "no")?;
                self.handle.set_property_string("loop-playlist", "no")?;
            }
            RepeatMode::One => {
                self.handle.set_property_string("loop-file", "inf")?;
                self.handle.set_property_string("loop-playlist", "no")?;
            }
            RepeatMode::All => {
                self.handle.set_property_string("loop-file", "no")?;
                self.handle.set_property_string("loop-playlist", "inf")?;
            }
        }
        self.repeat_mode = mode;
        Ok(())
    }

    pub fn panscan(&self) -> f64 {
        self.panscan
    }

    /// Sets panscan (0.0 = fit, 1.0 = fill/crop).
    pub fn set_panscan(&mut self, value: f64) -> Result<(), MpvError> {
        if !value.is_finite() {
            return Err(MpvError::InvalidParameter);
        }
        let clamped = value.clamp(0.0, 1.0);
        self.handle.set_property("panscan", clamped)?;
        self.panscan = clamped;
        Ok(())
    }

    pub fn set_panscan_fit(&mut self) -> Result<(), MpvError> {
        self.set_panscan(0.0)
    }

    pub fn set_panscan_crop(&mut self) -> Result<(), MpvError> {
        self.set_panscan(1.0)
    }

    /// Drains commands queued by `user-data/mpvex/*` property updates.
    pub fn drain_mpvex_commands(&mut self) -> Vec<MpvexCommand> {
        std::mem::take(&mut self.mpvex_commands)
    }

    fn update_mpvex_property(&mut self, name: &str) {
        let Ok(value) = self.handle.get_property_string(name) else {
            return;
        };
        if let Some(command) = parse_mpvex_command(name, &value, self.duration) {
            self.mpvex_commands.push(command);
        }
        // Clear after dispatch so a Lua script can repeat the same payload.
        let _ = self.handle.set_property_string(name, "");
    }

    /// Returns the currently active secondary subtitle track ID, if any.
    pub fn secondary_sub_track_id(&self) -> Option<i64> {
        self.secondary_sub_track_id
    }

    /// Sets the secondary subtitle track by ID, or disables secondary subtitles if `None`.
    pub fn set_secondary_sub_track(&mut self, track_id: Option<i64>) -> Result<(), MpvError> {
        self.handle.set_secondary_sub_track(track_id)?;
        self.secondary_sub_track_id = track_id;
        if track_id.is_some() {
            let _ = self.handle.configure_dual_subtitle_collision_avoidance();
        }
        Ok(())
    }

    /// Sets secondary subtitle delay in seconds.
    pub fn set_secondary_sub_delay(&self, delay_seconds: f64) -> Result<(), MpvError> {
        self.handle.set_secondary_sub_delay(delay_seconds)
    }

    /// Toggles secondary subtitle visibility.
    pub fn set_secondary_sub_visibility(&self, visible: bool) -> Result<(), MpvError> {
        self.handle.set_secondary_sub_visibility(visible)
    }

    /// Configures dual-subtitle collision avoidance (primary bottom, secondary top).
    pub fn configure_dual_subtitle_collision_avoidance(&self) -> Result<(), MpvError> {
        self.handle.configure_dual_subtitle_collision_avoidance()
    }

    /// Returns the active subtitle styling configuration.
    pub fn subtitle_styling(&self) -> &SubtitleStyling {
        &self.subtitle_styling
    }

    /// Applies custom subtitle styling properties to the mpv core.
    pub fn set_subtitle_styling(&mut self, styling: SubtitleStyling) -> Result<(), MpvError> {
        self.handle.apply_subtitle_styling(&styling)?;
        self.subtitle_styling = styling;
        Ok(())
    }

    /// Resets all subtitle styling properties back to defaults.
    pub fn reset_subtitle_styling(&mut self) -> Result<(), MpvError> {
        self.handle.reset_subtitle_styling()?;
        self.subtitle_styling = SubtitleStyling::default();
        Ok(())
    }

    /// Queries detected audio output devices via `audio-device-list`.
    pub fn audio_devices(&self) -> Result<Vec<AudioDeviceInfo>, MpvError> {
        EqualizerManager::query_audio_devices(&self.handle)
    }

    /// Selects an audio output device by name.
    pub fn set_audio_device(&self, device_name: &str) -> Result<(), MpvError> {
        EqualizerManager::set_audio_device(&self.handle, device_name)
    }

    /// Selects an audio output device with automatic fallback to "auto".
    pub fn set_audio_device_with_fallback(
        &self,
        preferred_device: &str,
    ) -> Result<String, MpvError> {
        EqualizerManager::set_audio_device_with_fallback(&self.handle, preferred_device)
    }

    /// Applies 10-band equalizer and DSP settings to the active engine.
    pub fn apply_equalizer(
        &self,
        settings: &crate::equalizer::EqualizerSettings,
    ) -> Result<(), MpvError> {
        EqualizerManager::apply_to_handle(&self.handle, settings)
    }

    /// Resets the equalizer and audio DSP filters to flat default.
    pub fn reset_equalizer(&self) -> Result<(), MpvError> {
        EqualizerManager::reset(&self.handle)
    }

    /// Applies hardware picture adjustments and libplacebo tone mapping.
    pub fn apply_video_adjustments(
        &self,
        settings: &crate::shader::VideoAdjustmentSettings,
    ) -> Result<(), MpvError> {
        crate::shader::ShaderManager::apply_adjustments(&self.handle, settings)
    }

    /// Resets hardware picture adjustments to neutral defaults.
    pub fn reset_video_adjustments(&self) -> Result<(), MpvError> {
        crate::shader::ShaderManager::reset_adjustments(&self.handle)
    }

    /// Applies Anime4K v4.0.1 shader preset and quality profile.
    pub fn apply_anime4k(
        &self,
        preset: crate::shader::Anime4KPreset,
        quality: crate::shader::ShaderQuality,
    ) -> Result<Vec<String>, MpvError> {
        crate::shader::ShaderManager::apply_anime4k_mode(&self.handle, preset, quality, None)
    }

    /// Clears all active video shaders.
    pub fn clear_shaders(&self) -> Result<(), MpvError> {
        crate::shader::ShaderManager::clear_shaders(&self.handle)
    }

    /// Sets audio delay in seconds (-10.0..10.0s).
    pub fn set_audio_delay(&self, delay_seconds: f64) -> Result<(), MpvError> {
        self.handle
            .set_property("audio-delay", delay_seconds.clamp(-10.0, 10.0))
    }

    /// Queries active audio delay in seconds.
    pub fn audio_delay(&self) -> Result<f64, MpvError> {
        self.handle.get_property::<f64>("audio-delay")
    }

    /// Enables or disables speed pitch preservation.
    pub fn set_pitch_correction(&self, enabled: bool) -> Result<(), MpvError> {
        self.handle
            .set_property_string("audio-pitch-correction", if enabled { "yes" } else { "no" })
    }

    /// Configures audio channel layout ("auto", "stereo", "5.1", "7.1").
    pub fn set_channel_layout(&self, layout: &str) -> Result<(), MpvError> {
        self.handle.set_property_string("audio-channels", layout)
    }

    // -------------------------------------------------------------------------
    // Playback Commands
    // -------------------------------------------------------------------------

    /// Loads a file synchronously, transitioning to `PlaybackState::Loading`.
    pub fn load_file(&mut self, path: &str) -> Result<(), MpvError> {
        self.state = PlaybackState::Loading;
        self.current_file = Some(path.to_string());
        self.last_error = None;
        self.time_pos = 0.0;
        self.duration = 0.0;
        self.video_width = 0;
        self.video_height = 0;
        self.video_format.clear();
        self.audio_codec.clear();
        self.tracks.clear();
        self.chapters.clear();
        self.current_chapter = None;
        self.pending_resume_pos = None;
        self.pending_resume_audio = None;
        self.pending_resume_sub = None;

        self.handle.load_file(path, LoadFileMode::Replace)?;
        let _ = self.handle.play();
        self.is_paused = false;
        Ok(())
    }

    /// Loads a file asynchronously without blocking the caller.
    pub fn load_file_async(&mut self, path: &str) -> Result<u64, MpvError> {
        self.state = PlaybackState::Loading;
        self.current_file = Some(path.to_string());
        self.last_error = None;
        self.time_pos = 0.0;
        self.duration = 0.0;
        self.video_width = 0;
        self.video_height = 0;
        self.video_format.clear();
        self.audio_codec.clear();
        self.tracks.clear();
        self.chapters.clear();
        self.current_chapter = None;
        self.pending_resume_pos = None;
        self.pending_resume_audio = None;
        self.pending_resume_sub = None;

        let id = self.next_async_id;
        self.next_async_id += 1;
        self.handle
            .command_async(id, &["loadfile", path, "replace"])?;
        let _ = self.handle.play();
        self.is_paused = false;
        Ok(id)
    }

    /// Reloads media from the given path or URI, restoring the playback position
    /// and track selections once the file is loaded (Level B network recovery).
    pub fn reload_with_resume(
        &mut self,
        path: &str,
        resume_time: f64,
        audio_track: Option<Option<i64>>,
        sub_track: Option<Option<i64>>,
    ) -> Result<(), MpvError> {
        self.load_file(path)?;
        self.pending_resume_pos = if resume_time > 0.0 {
            Some(resume_time)
        } else {
            None
        };
        self.pending_resume_audio = audio_track;
        self.pending_resume_sub = sub_track;
        Ok(())
    }

    /// Returns the currently pending resume position, if any.
    pub fn pending_resume_pos(&self) -> Option<f64> {
        self.pending_resume_pos
    }

    /// Resumes playback.
    pub fn play(&mut self) -> Result<(), MpvError> {
        self.handle.play()?;
        self.is_paused = false;
        if self.state == PlaybackState::Paused {
            self.state = PlaybackState::Playing;
        }
        Ok(())
    }

    /// Pauses playback.
    pub fn pause(&mut self) -> Result<(), MpvError> {
        self.handle.pause()?;
        self.is_paused = true;
        if self.state == PlaybackState::Playing {
            self.state = PlaybackState::Paused;
        }
        Ok(())
    }

    /// Toggles play/pause state.
    pub fn toggle_pause(&mut self) -> Result<(), MpvError> {
        if self.is_paused {
            self.play()
        } else {
            self.pause()
        }
    }

    /// Stops playback and resets state to `Idle`.
    pub fn stop(&mut self) -> Result<(), MpvError> {
        self.handle.stop()?;
        self.state = PlaybackState::Idle;
        self.time_pos = 0.0;
        self.duration = 0.0;
        self.current_file = None;
        self.tracks.clear();
        self.chapters.clear();
        self.current_chapter = None;
        Ok(())
    }

    /// Seeks to `target_seconds` synchronously.
    pub fn seek(&mut self, target_seconds: f64, mode: SeekMode) -> Result<(), MpvError> {
        self.handle.seek(target_seconds, mode)
    }

    /// Convenience method for relative seeking (e.g. +5.0 or -5.0 seconds).
    pub fn seek_relative(&mut self, seconds: f64) -> Result<(), MpvError> {
        self.seek(seconds, SeekMode::Relative)
    }

    /// Seeks to `target_seconds` asynchronously.
    pub fn seek_async(&mut self, target_seconds: f64, mode: SeekMode) -> Result<u64, MpvError> {
        let id = self.next_async_id;
        self.next_async_id += 1;
        let sec_str = target_seconds.to_string();
        self.handle
            .command_async(id, &["seek", &sec_str, mode.as_str()])?;
        Ok(id)
    }

    /// Sets playback volume (0.0 to 100.0, or up to volume_boost_max when boost is enabled).
    pub fn set_volume(&mut self, volume: f64) -> Result<(), MpvError> {
        let max_vol = self.volume_boost_max.max(100.0);
        let clamped = volume.clamp(0.0, max_vol);
        self.handle.set_property("volume", clamped)?;
        self.volume = clamped;
        Ok(())
    }

    /// Sets volume boost ceiling (100.0% to 150.0%).
    pub fn set_volume_boost(&mut self, boost_percent: f64) -> Result<(), MpvError> {
        let max_vol = boost_percent.clamp(100.0, 150.0);
        self.volume_boost_max = max_vol;
        self.handle.set_property("volume-max", max_vol)?;
        Ok(())
    }

    /// Current volume boost ceiling (100.0% to 150.0%).
    pub fn volume_boost_max(&self) -> f64 {
        self.volume_boost_max
    }

    /// Adjusts volume by delta (e.g. +5.0 or -5.0), clamped to [0.0, volume_boost_max].
    pub fn adjust_volume(&mut self, delta: f64) -> Result<(), MpvError> {
        let max_vol = self.volume_boost_max.max(100.0);
        let new_vol = (self.volume + delta).clamp(0.0, max_vol);
        self.set_volume(new_vol)
    }

    /// Sets audio mute state.
    pub fn set_mute(&mut self, muted: bool) -> Result<(), MpvError> {
        self.handle.set_property("mute", muted)?;
        self.is_muted = muted;
        Ok(())
    }

    /// Sets audio channel configuration and routing.
    pub fn set_audio_channels(
        &self,
        channels: crate::settings::AudioChannelsSetting,
    ) -> Result<(), MpvError> {
        self.handle
            .set_property_string("audio-channels", channels.mpv_channels_option())
    }

    /// Sets audio pitch correction when playback speed changes.
    pub fn set_audio_pitch_correction(&self, enabled: bool) -> Result<(), MpvError> {
        self.handle.set_property("audio-pitch-correction", enabled)
    }

    /// Sets preferred audio track languages (`alang`).
    pub fn set_audio_languages(&self, languages: &str) -> Result<(), MpvError> {
        self.handle.set_property_string("alang", languages)
    }

    /// Toggles audio mute state.
    pub fn toggle_mute(&mut self) -> Result<(), MpvError> {
        self.set_mute(!self.is_muted)
    }

    /// Sets playback speed multiplier.
    pub fn set_speed(&mut self, speed: f64) -> Result<(), MpvError> {
        self.handle.set_property("speed", speed)?;
        self.speed = speed;
        Ok(())
    }

    /// Adjusts playback speed by delta, clamped to [0.25, 4.0].
    pub fn adjust_speed(&mut self, delta: f64) -> Result<(), MpvError> {
        let new_speed = ((self.speed + delta) * 100.0).round() / 100.0;
        let clamped = new_speed.clamp(0.25, 4.0);
        self.set_speed(clamped)
    }

    /// Resets playback speed to normal 1.0x.
    pub fn reset_speed(&mut self) -> Result<(), MpvError> {
        self.set_speed(1.0)
    }

    /// Jumps to the next chapter (`add chapter 1`).
    pub fn next_chapter(&mut self) -> Result<(), MpvError> {
        self.handle.command(&["add", "chapter", "1"])
    }

    /// Jumps to the previous chapter (`add chapter -1`).
    pub fn prev_chapter(&mut self) -> Result<(), MpvError> {
        self.handle.command(&["add", "chapter", "-1"])
    }

    /// Jumps to a specific chapter by 0-based index.
    pub fn set_chapter(&mut self, index: i64) -> Result<(), MpvError> {
        self.handle.set_property("chapter", index)?;
        self.current_chapter = Some(index);
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Track and Subtitle Controls
    // -------------------------------------------------------------------------

    /// Sets the active track for a given track type, or disables it if `None`.
    pub fn set_track(&mut self, track_type: TrackType, id: Option<i64>) -> Result<(), MpvError> {
        match track_type {
            TrackType::Video => self.handle.set_video_track(id),
            TrackType::Audio => self.handle.set_audio_track(id),
            TrackType::Subtitle => self.handle.set_sub_track(id),
        }
    }

    /// Sets the primary subtitle track by ID, or disables subtitles if `None`.
    pub fn set_subtitle_track(&mut self, id: Option<i64>) -> Result<(), MpvError> {
        self.handle.set_sub_track(id)
    }

    /// Sets the secondary subtitle track by ID, or disables it if `None`.
    pub fn set_secondary_subtitle_track(&mut self, id: Option<i64>) -> Result<(), MpvError> {
        self.set_secondary_sub_track(id)
    }

    /// Sets the active audio track by ID, or disables audio if `None`.
    pub fn set_audio_track(&mut self, id: Option<i64>) -> Result<(), MpvError> {
        self.handle.set_audio_track(id)
    }

    /// Sets the active video track by ID, or disables video if `None`.
    pub fn set_video_track(&mut self, id: Option<i64>) -> Result<(), MpvError> {
        self.handle.set_video_track(id)
    }

    /// Loads an external subtitle file (`sub-add`).
    pub fn add_subtitle_file(&mut self, path: &str, select: bool) -> Result<(), MpvError> {
        self.handle.add_sub(path, select)
    }

    /// Sets subtitle delay offset in seconds.
    pub fn set_subtitle_delay(&mut self, delay_seconds: f64) -> Result<(), MpvError> {
        self.handle.set_sub_delay(delay_seconds)?;
        self.sub_delay = delay_seconds;
        Ok(())
    }

    /// Toggles subtitle visibility on or off.
    pub fn set_subtitle_visibility(&mut self, visible: bool) -> Result<(), MpvError> {
        self.handle.set_sub_visibility(visible)?;
        self.sub_visibility = visible;
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Event Processing and State Synchronization
    // -------------------------------------------------------------------------

    /// Drains all pending events non-blockingly from mpv and updates controller state.
    pub fn poll_events(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        loop {
            let event = self.handle.wait_event(0.0);
            if let Event::None = event {
                break;
            }
            self.handle_event(&event);
            events.push(event);
        }
        events
    }

    /// Dispatches an event to the state machine and updates cached properties.
    pub fn handle_event(&mut self, event: &Event) {
        match event {
            Event::StartFile { .. } => {
                self.state = PlaybackState::Loading;
                self.ab_loop_state = AbLoopState::None;
                let _ = self.handle.set_property_string("ab-loop-a", "no");
                let _ = self.handle.set_property_string("ab-loop-b", "no");
            }
            Event::FileLoaded => {
                self.refresh_metadata();
                self.refresh_playlist();
                self.secondary_sub_track_id = self.handle.get_secondary_sub_track();
                if let Ok(p) = self.handle.get_property_string("path") {
                    if !p.is_empty() {
                        self.current_file = Some(p);
                    }
                }
                if let Some(pos) = self.pending_resume_pos.take() {
                    log::info!(
                        "Level B recovery: seeking to preserved position {:.2}s",
                        pos
                    );
                    let _ = self.handle.seek(pos, SeekMode::Exact);
                }
                if let Some(audio_opt) = self.pending_resume_audio.take() {
                    let _ = self.handle.set_audio_track(audio_opt);
                }
                if let Some(sub_opt) = self.pending_resume_sub.take() {
                    let _ = self.handle.set_sub_track(sub_opt);
                }
                if self.is_paused {
                    self.state = PlaybackState::Paused;
                } else {
                    self.state = PlaybackState::Playing;
                }
            }
            Event::CommandReply { error: Err(e), .. } => {
                if matches!(self.state, PlaybackState::Loading) {
                    let attempted = self.current_file.as_deref().unwrap_or("<unknown>");
                    let msg = format!("Async command failed for '{attempted}': {e}");
                    self.set_error(msg);
                }
            }
            Event::EndFile { reason, error, .. } => match reason {
                EndFileReason::Eof => {
                    self.state = PlaybackState::Ended;
                }
                EndFileReason::Error => {
                    let attempted = self.current_file.as_deref().unwrap_or("<unknown>");
                    let err_desc = error
                        .as_ref()
                        .map(|e| e.to_string())
                        .unwrap_or_else(|| "Playback error encountered".to_string());
                    let stage = if matches!(self.state, PlaybackState::Loading) {
                        "media open/demux"
                    } else {
                        "active playback/decode"
                    };
                    let disk_hint = if !attempted.starts_with("http://")
                        && !attempted.starts_with("https://")
                        && !std::path::Path::new(attempted).exists()
                    {
                        " (file does not exist on disk)"
                    } else {
                        ""
                    };
                    let msg = format!(
                        "Failed to load '{attempted}' during {stage}: {err_desc}{disk_hint}"
                    );
                    self.set_error(msg);
                }
                EndFileReason::Stop | EndFileReason::Quit
                    if self.state != PlaybackState::Loading =>
                {
                    self.state = PlaybackState::Idle;
                    self.time_pos = 0.0;
                    self.duration = 0.0;
                }
                _ => {}
            },
            Event::PropertyChange {
                reply_userdata,
                name,
                ..
            } => {
                self.handle_property_change(*reply_userdata, name);
            }
            Event::PlaybackRestart | Event::Seek => {
                if let Ok(pos) = self.handle.get_property::<f64>("time-pos") {
                    self.time_pos = pos;
                }
                if self.state == PlaybackState::Ended {
                    self.state = if self.is_paused {
                        PlaybackState::Paused
                    } else {
                        PlaybackState::Playing
                    };
                }
            }
            Event::Shutdown => {
                self.state = PlaybackState::Idle;
            }
            _ => {}
        }
    }

    fn handle_property_change(&mut self, reply_userdata: u64, name: &str) {
        match reply_userdata {
            PROP_TIME_POS => {
                if let Ok(pos) = self.handle.get_property::<f64>("time-pos") {
                    self.time_pos = pos;
                }
            }
            PROP_DURATION => {
                if let Ok(dur) = self.handle.get_property::<f64>("duration") {
                    self.duration = dur;
                }
            }
            PROP_PAUSE => {
                if let Ok(paused) = self.handle.get_property::<bool>("pause") {
                    self.is_paused = paused;
                    if !self.is_buffering {
                        if self.state == PlaybackState::Playing && self.is_paused {
                            self.state = PlaybackState::Paused;
                        } else if self.state == PlaybackState::Paused && !self.is_paused {
                            self.state = PlaybackState::Playing;
                        }
                    }
                }
            }
            PROP_PAUSED_FOR_CACHE => {
                if let Ok(caching) = self.handle.get_property::<bool>("paused-for-cache") {
                    self.is_buffering = caching;
                    if caching
                        && (self.state == PlaybackState::Playing
                            || self.state == PlaybackState::Paused)
                    {
                        self.state = PlaybackState::Buffering;
                    } else if !caching && self.state == PlaybackState::Buffering {
                        self.state = if self.is_paused {
                            PlaybackState::Paused
                        } else {
                            PlaybackState::Playing
                        };
                    }
                }
            }
            PROP_SPEED => {
                if let Ok(speed) = self.handle.get_property::<f64>("speed") {
                    self.speed = speed;
                }
            }
            PROP_VOLUME => {
                if let Ok(vol) = self.handle.get_property::<f64>("volume") {
                    self.volume = vol;
                }
            }
            PROP_MUTE => {
                if let Ok(muted) = self.handle.get_property::<bool>("mute") {
                    self.is_muted = muted;
                }
            }
            PROP_TRACK_LIST => {
                if let Ok(tracks) = self.handle.get_track_list() {
                    self.tracks = tracks;
                }
            }
            PROP_CHAPTER_LIST => {
                if let Ok(chapters) = self.handle.get_chapter_list() {
                    self.chapters = chapters;
                }
            }
            PROP_CHAPTER => {
                if let Ok(ch) = self.handle.get_property::<i64>("chapter") {
                    self.current_chapter = if ch >= 0 { Some(ch) } else { None };
                }
            }
            PROP_HWDEC_CURRENT => {
                if let Ok(hw) = self.handle.get_property_string("hwdec-current") {
                    self.hwdec_current = hw;
                }
            }
            PROP_CURRENT_AO => {
                if let Ok(ao) = self.handle.get_property_string("current-ao") {
                    self.current_ao = ao;
                }
            }
            PROP_SUB_DELAY => {
                if let Ok(delay) = self.handle.get_property::<f64>("sub-delay") {
                    self.sub_delay = delay;
                }
            }
            PROP_SUB_VISIBILITY => {
                if let Ok(vis) = self.handle.get_property::<bool>("sub-visibility") {
                    self.sub_visibility = vis;
                }
            }
            PROP_EOF_REACHED => {
                if let Ok(eof) = self.handle.get_property::<bool>("eof-reached") {
                    if eof && self.state != PlaybackState::Idle {
                        self.state = PlaybackState::Ended;
                    }
                }
            }
            PROP_PLAYLIST | PROP_PLAYLIST_POS | PROP_PLAYLIST_COUNT => {
                self.refresh_playlist();
            }
            PROP_SECONDARY_SID => {
                self.secondary_sub_track_id = self.handle.get_secondary_sub_track();
            }
            PROP_AB_LOOP_A | PROP_AB_LOOP_B => {
                self.refresh_ab_loop();
            }
            PROP_MPVEX_SHOW_TEXT => self.update_mpvex_property("user-data/mpvex/show_text"),
            PROP_MPVEX_TOGGLE_UI => self.update_mpvex_property("user-data/mpvex/toggle_ui"),
            PROP_MPVEX_SHOW_PANEL => self.update_mpvex_property("user-data/mpvex/show_panel"),
            PROP_MPVEX_SEEK_BY => self.update_mpvex_property("user-data/mpvex/seek_by"),
            PROP_MPVEX_SEEK_TO => self.update_mpvex_property("user-data/mpvex/seek_to"),
            _ => {
                // Fallback by name if matched outside userdata
                if name == "time-pos" {
                    if let Ok(pos) = self.handle.get_property::<f64>("time-pos") {
                        self.time_pos = pos;
                    }
                } else if name == "playlist" || name == "playlist-pos" || name == "playlist-count" {
                    self.refresh_playlist();
                } else if name == "secondary-sid" {
                    self.secondary_sub_track_id = self.handle.get_secondary_sub_track();
                } else if name == "ab-loop-a" || name == "ab-loop-b" {
                    self.refresh_ab_loop();
                }
            }
        }
    }

    /// Queries all relevant media properties and refreshes the internal cache.
    pub fn refresh_metadata(&mut self) {
        if let Ok(d) = self.handle.get_property::<f64>("duration") {
            self.duration = d;
        }
        if let Ok(p) = self.handle.get_property::<f64>("time-pos") {
            self.time_pos = p;
        }
        if let Ok(v) = self.handle.get_property::<f64>("volume") {
            self.volume = v;
        }
        if let Ok(m) = self.handle.get_property::<bool>("mute") {
            self.is_muted = m;
        }
        if let Ok(s) = self.handle.get_property::<f64>("speed") {
            self.speed = s;
        }
        if let Ok(p) = self.handle.get_property::<bool>("pause") {
            self.is_paused = p;
        }
        if let Ok(w) = self.handle.get_property::<i64>("width") {
            self.video_width = w;
        }
        if let Ok(h) = self.handle.get_property::<i64>("height") {
            self.video_height = h;
        }
        if let Ok(fmt) = self.handle.get_property_string("video-format") {
            self.video_format = fmt;
        }
        if let Ok(codec) = self.handle.get_property_string("audio-codec-name") {
            self.audio_codec = codec;
        }
        if let Ok(hw) = self.handle.get_property_string("hwdec-current") {
            self.hwdec_current = hw;
        }
        if let Ok(ao) = self.handle.get_property_string("current-ao") {
            self.current_ao = ao;
        }
        if let Ok(t) = self.handle.get_track_list() {
            self.tracks = t;
        }
        if let Ok(c) = self.handle.get_chapter_list() {
            self.chapters = c;
        }
        if let Ok(ch) = self.handle.get_property::<i64>("chapter") {
            self.current_chapter = if ch >= 0 { Some(ch) } else { None };
        }
        self.refresh_ab_loop();
    }
}
