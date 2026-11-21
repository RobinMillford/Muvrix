//! High-level playback controller and state machine.

use std::path::Path;
use std::sync::Arc;

use crate::command::{LoadFileMode, SeekMode};
use crate::decode::DecodeObservation;
use crate::equalizer::{AudioDeviceInfo, EqualizerSettings};
use crate::error::MpvError;
use crate::event::{EndFileReason, Event};
use crate::handle::MpvHandle;
use crate::playlist::PlaylistItem;
use crate::property::PropertyFormat;
use crate::shader::{
    Anime4KPreset, CustomShaderEntry, DebandPreset, ShaderQuality, VideoAdjustmentSettings,
};
use crate::stream::{MpvStreamBridge, RecoveryState, SeekableStream, StreamTelemetry};
use crate::subsystems::audio::AudioSubsystem;
use crate::subsystems::track::TrackSubsystem;
use crate::subsystems::video::{CropPreset, ScalingMode, ScreenshotMode, VideoSubsystem};
use crate::track::{ChapterInfo, SubtitleStyling, TrackInfo, TrackType};

pub use crate::spectrum::SpectrumTelemetry;
pub use crate::subsystems::audio::{
    AudioDeviceChangeEvent, AudioRecoveryStatus, DynamicsTelemetry,
};
pub use crate::subsystems::video::{
    DecoderFallbackReason, DecoderWatchdog, SteppingPreset, WatchdogStatus,
};

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
pub const PROP_AUDIO_DEVICE_LIST: u64 = 27;
/// `frame-drop-count`, observed with an inline `Int64` so the decoder watchdog
/// can be fed on the event path instead of polling the engine from a paint.
pub const PROP_FRAME_DROP_COUNT: u64 = 28;

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

/// Orchestrates playback lifecycle, handles async commands, and coordinates
/// domain-specific subsystems (Audio, Video, Track).
pub struct PlaybackController {
    handle: Arc<MpvHandle>,
    state: PlaybackState,
    current_file: Option<String>,
    last_error: Option<String>,
    time_pos: f64,
    duration: f64,
    is_paused: bool,
    is_buffering: bool,
    /// True once the current item has reached end-of-file. mpv with
    /// `idle=yes` emits the idle unload right after the EOF event, and that
    /// unload used to overwrite [`PlaybackState::Ended`] with `Idle`; the
    /// sticky flag keeps the finished state observable.
    eof_ended: bool,

    // Specialized domain subsystems
    audio: AudioSubsystem,
    video: VideoSubsystem,
    track: TrackSubsystem,

    // Lifecycle, playlist & commands
    ab_loop_state: AbLoopState,
    repeat_mode: RepeatMode,
    playlist: Vec<PlaylistItem>,
    playlist_pos: Option<usize>,
    mpvex_commands: Vec<MpvexCommand>,
    stream_bridge: MpvStreamBridge,
    stream_telemetry: Option<StreamTelemetry>,
    next_async_id: u64,
    pending_resume_pos: Option<f64>,
    pending_resume_audio: Option<Option<i64>>,
    pending_resume_sub: Option<Option<i64>>,
}

impl PlaybackController {
    /// Creates a new `PlaybackController` and registers property observation callbacks.
    pub fn new(handle: Arc<MpvHandle>) -> Result<Self, MpvError> {
        let stream_bridge = MpvStreamBridge::new();
        if let Err(e) = stream_bridge.attach_to_handle(&handle) {
            log::warn!("Failed to attach stream bridge to mpv handle: {e}");
        }

        // Observe key properties to avoid high-frequency polling
        handle.observe_property(PROP_TIME_POS, "time-pos", PropertyFormat::Double)?;
        handle.observe_property(PROP_DURATION, "duration", PropertyFormat::Double)?;
        handle.observe_property(PROP_PAUSE, "pause", PropertyFormat::Flag)?;
        handle.observe_property(
            PROP_PAUSED_FOR_CACHE,
            "paused-for-cache",
            PropertyFormat::Flag,
        )?;
        handle.observe_property(PROP_SPEED, "speed", PropertyFormat::Double)?;
        handle.observe_property(PROP_VOLUME, "volume", PropertyFormat::Double)?;
        handle.observe_property(PROP_MUTE, "mute", PropertyFormat::Flag)?;
        handle.observe_property(PROP_TRACK_LIST, "track-list", PropertyFormat::None)?;
        handle.observe_property(PROP_CHAPTER_LIST, "chapter-list", PropertyFormat::None)?;
        handle.observe_property(PROP_HWDEC_CURRENT, "hwdec-current", PropertyFormat::None)?;
        handle.observe_property(PROP_CURRENT_AO, "current-ao", PropertyFormat::None)?;
        // Typed so the value arrives inline in the event. The decoder watchdog
        // consumes this instead of performing its own `frame-drop-count` read on
        // whatever thread drains events (which is the paint thread).
        handle.observe_property(
            PROP_FRAME_DROP_COUNT,
            "frame-drop-count",
            PropertyFormat::Int64,
        )?;
        handle.observe_property(PROP_EOF_REACHED, "eof-reached", PropertyFormat::Flag)?;
        handle.observe_property(PROP_SUB_DELAY, "sub-delay", PropertyFormat::Double)?;
        handle.observe_property(PROP_SUB_VISIBILITY, "sub-visibility", PropertyFormat::Flag)?;
        handle.observe_property(PROP_CHAPTER, "chapter", PropertyFormat::Int64)?;
        handle.observe_property(PROP_PLAYLIST, "playlist", PropertyFormat::None)?;
        handle.observe_property(PROP_PLAYLIST_POS, "playlist-pos", PropertyFormat::Int64)?;
        handle.observe_property(PROP_PLAYLIST_COUNT, "playlist-count", PropertyFormat::Int64)?;
        handle.observe_property(PROP_SECONDARY_SID, "secondary-sid", PropertyFormat::None)?;
        handle.observe_property(
            PROP_AUDIO_DEVICE_LIST,
            "audio-device-list",
            PropertyFormat::None,
        )?;

        // Non-critical property observation
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
            eof_ended: false,
            current_file: None,
            last_error: None,
            time_pos: 0.0,
            duration: 0.0,
            is_paused: false,
            is_buffering: false,
            audio: AudioSubsystem::new(),
            video: VideoSubsystem::new(),
            track: TrackSubsystem::new(),
            ab_loop_state: AbLoopState::None,
            repeat_mode: RepeatMode::Off,
            playlist: Vec::new(),
            playlist_pos: None,
            mpvex_commands: Vec::new(),
            stream_bridge,
            stream_telemetry: None,
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

    /// Access the Audio subsystem.
    pub fn audio(&self) -> &AudioSubsystem {
        &self.audio
    }

    /// Mutably access the Audio subsystem.
    pub fn audio_mut(&mut self) -> &mut AudioSubsystem {
        &mut self.audio
    }

    /// Access the Video subsystem.
    pub fn video(&self) -> &VideoSubsystem {
        &self.video
    }

    /// Mutably access the Video subsystem.
    pub fn video_mut(&mut self) -> &mut VideoSubsystem {
        &mut self.video
    }

    /// Access the Track subsystem.
    pub fn track(&self) -> &TrackSubsystem {
        &self.track
    }

    /// Mutably access the Track subsystem.
    pub fn track_mut(&mut self) -> &mut TrackSubsystem {
        &mut self.track
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
        self.video.on_video_dimensions_changed(0, 0);
        self.video.on_video_format_changed(String::new());
        self.audio.on_codec_changed(String::new());
        self.track.on_track_list_changed(Vec::new());
        self.track.on_chapter_list_changed(Vec::new());
        self.track.on_chapter_changed(None);
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

    /// Duration of demuxer cached data ahead of the current playback position in seconds.
    pub fn demuxer_cache_duration(&self) -> f64 {
        self.handle
            .get_property::<f64>("demuxer-cache-duration")
            .unwrap_or(0.0)
    }

    /// Absolute timestamp in seconds up to which the demuxer has cached packets.
    pub fn demuxer_cache_time(&self) -> f64 {
        self.handle
            .get_property::<f64>("demuxer-cache-time")
            .unwrap_or(0.0)
    }

    /// Whether the demuxer cache is currently idle (not actively fetching).
    pub fn demuxer_cache_idle(&self) -> bool {
        self.handle
            .get_property::<bool>("demuxer-cache-idle")
            .unwrap_or(true)
    }

    /// Whether playback is paused waiting for cache refill.
    ///
    /// Reads the event-maintained cache only: `paused-for-cache` is observed
    /// with a typed `PropertyFormat::Flag`, and the observation carries the
    /// fresh value inline, so a synchronous round trip here is both
    /// unnecessary and expensive (it blocks until the engine core answers,
    /// which under decode pressure is tens to hundreds of milliseconds).
    pub fn is_paused_for_cache(&self) -> bool {
        self.is_buffering
    }

    /// Configures demuxer cache limits and read-ahead bounds for network streaming.
    pub fn configure_stream_buffering(
        &mut self,
        max_bytes: u64,
        max_back_bytes: u64,
        readahead_secs: f64,
    ) -> Result<(), MpvError> {
        let _ = self
            .handle
            .set_property("demuxer-max-bytes", max_bytes as i64);
        let _ = self
            .handle
            .set_property("demuxer-max-back-bytes", max_back_bytes as i64);
        let _ = self
            .handle
            .set_property("demuxer-readahead-secs", readahead_secs);
        Ok(())
    }

    /// Sets or clears active stream recovery telemetry tracking.
    pub fn set_stream_telemetry(&mut self, telemetry: Option<StreamTelemetry>) {
        self.stream_telemetry = telemetry;
    }

    /// Current stream recovery telemetry handle, if streaming.
    pub fn stream_telemetry(&self) -> Option<StreamTelemetry> {
        self.stream_telemetry.clone()
    }

    /// Checks if a remote stream is currently reconnecting or stalled for cache.
    pub fn is_stream_reconnecting(&self) -> bool {
        if let Some(t) = &self.stream_telemetry {
            if let RecoveryState::Reconnecting { .. } = t.current_state() {
                return true;
            }
        }
        self.is_paused_for_cache()
    }

    // -------------------------------------------------------------------------
    // Audio Delegations
    // -------------------------------------------------------------------------

    pub fn speed(&self) -> f64 {
        self.audio.speed()
    }

    pub fn volume(&self) -> f64 {
        self.audio.volume()
    }

    pub fn volume_boost_max(&self) -> f64 {
        self.audio.volume_boost_max()
    }

    pub fn is_muted(&self) -> bool {
        self.audio.is_muted()
    }

    pub fn current_ao(&self) -> &str {
        self.audio.current_ao()
    }

    pub fn audio_codec(&self) -> &str {
        self.audio.audio_codec()
    }

    pub fn set_volume(&mut self, volume: f64) -> Result<(), MpvError> {
        self.audio.set_volume(&self.handle, volume)
    }

    pub fn set_volume_boost(&mut self, boost_percent: f64) -> Result<(), MpvError> {
        self.audio.set_volume_boost(&self.handle, boost_percent)
    }

    pub fn adjust_volume(&mut self, delta: f64) -> Result<(), MpvError> {
        self.audio.adjust_volume(&self.handle, delta)
    }

    pub fn set_mute(&mut self, muted: bool) -> Result<(), MpvError> {
        self.audio.set_mute(&self.handle, muted)
    }

    pub fn toggle_mute(&mut self) -> Result<(), MpvError> {
        self.audio.toggle_mute(&self.handle)
    }

    pub fn set_speed(&mut self, speed: f64) -> Result<(), MpvError> {
        self.audio.set_speed(&self.handle, speed)
    }

    pub fn adjust_speed(&mut self, delta: f64) -> Result<(), MpvError> {
        self.audio.adjust_speed(&self.handle, delta)
    }

    pub fn reset_speed(&mut self) -> Result<(), MpvError> {
        self.audio.reset_speed(&self.handle)
    }

    pub fn set_audio_delay(&self, delay_seconds: f64) -> Result<(), MpvError> {
        self.handle.set_property("audio-delay", delay_seconds)
    }

    pub fn audio_delay(&self) -> Result<f64, MpvError> {
        self.audio.audio_delay(&self.handle)
    }

    pub fn set_pitch_correction(&self, enabled: bool) -> Result<(), MpvError> {
        let val = if enabled { "yes" } else { "no" };
        self.handle
            .set_property_string("audio-pitch-correction", val)
    }

    pub fn set_audio_pitch_correction(&self, enabled: bool) -> Result<(), MpvError> {
        self.set_pitch_correction(enabled)
    }

    pub fn set_channel_layout(&self, layout: &str) -> Result<(), MpvError> {
        let target = match layout.to_lowercase().as_str() {
            "stereo" => "stereo",
            "5.1" | "surround51" => "5.1",
            "7.1" | "surround71" => "7.1",
            _ => "auto",
        };
        self.handle.set_property_string("audio-channels", target)
    }

    pub fn set_audio_channels(
        &self,
        channels: crate::settings::AudioChannelsSetting,
    ) -> Result<(), MpvError> {
        self.audio.set_audio_channels(&self.handle, channels)
    }

    pub fn set_audio_languages(&self, languages: &str) -> Result<(), MpvError> {
        self.audio.set_audio_languages(&self.handle, languages)
    }

    pub fn audio_devices(&self) -> Result<Vec<AudioDeviceInfo>, MpvError> {
        self.audio.audio_devices(&self.handle)
    }

    pub fn set_audio_device(&self, device_name: &str) -> Result<(), MpvError> {
        self.audio.set_audio_device(&self.handle, device_name)
    }

    pub fn set_audio_device_with_fallback(
        &self,
        preferred_device: &str,
    ) -> Result<String, MpvError> {
        self.audio
            .set_audio_device_with_fallback(&self.handle, preferred_device)
    }

    pub fn apply_equalizer(&self, settings: &EqualizerSettings) -> Result<(), MpvError> {
        self.audio.apply_equalizer(&self.handle, settings)
    }

    pub fn reset_equalizer(&self) -> Result<(), MpvError> {
        self.audio.reset_equalizer(&self.handle)
    }

    pub fn dynamics_telemetry(&self, is_normalization_enabled: bool) -> DynamicsTelemetry {
        self.audio
            .dynamics_telemetry(&self.handle, is_normalization_enabled)
    }

    /// Queries real-time audio spectrum telemetry from the active media stream.
    pub fn spectrum_telemetry(&self) -> SpectrumTelemetry {
        let is_playing = *self.state() == PlaybackState::Playing;
        let is_paused = self.is_paused();
        self.audio
            .spectrum_telemetry(&self.handle, is_playing, is_paused)
    }

    pub fn gapless_audio(&self) -> bool {
        self.audio.gapless_audio()
    }

    pub fn set_gapless_audio(&self, mode: &str) -> Result<(), MpvError> {
        self.audio.set_gapless_audio(&self.handle, mode)
    }

    pub fn prefetch_next_track(&self) {
        self.audio.prefetch_next_track(&self.handle)
    }

    // -------------------------------------------------------------------------
    // Video Delegations
    // -------------------------------------------------------------------------

    pub fn hwdec_current(&self) -> &str {
        self.video.hwdec_current()
    }

    pub fn requested_hwdec(&self) -> String {
        self.video.requested_hwdec()
    }

    pub fn set_hwdec_mode(&self, mode: &str) -> Result<(), MpvError> {
        self.video.set_hwdec_mode(&self.handle, mode)
    }

    pub fn decode_observation(&self) -> DecodeObservation {
        self.video.decode_observation(&self.handle)
    }

    pub fn video_format(&self) -> &str {
        self.video.video_format()
    }

    pub fn video_dimensions(&self) -> (i64, i64) {
        self.video.video_dimensions()
    }

    pub fn container_fps(&self) -> Option<f64> {
        self.video.container_fps(&self.handle)
    }

    pub fn estimated_vf_fps(&self) -> Option<f64> {
        self.video.estimated_vf_fps(&self.handle)
    }

    pub fn frame_drop_count(&self) -> i64 {
        self.video.frame_drop_count(&self.handle)
    }

    pub fn decoder_frame_drop_count(&self) -> i64 {
        self.video.decoder_frame_drop_count(&self.handle)
    }

    pub fn vo_delayed_frame_count(&self) -> i64 {
        self.video.vo_delayed_frame_count(&self.handle)
    }

    pub fn avsync(&self) -> Option<f64> {
        self.video.avsync(&self.handle)
    }

    pub fn total_avsync_change(&self) -> Option<f64> {
        self.video.total_avsync_change(&self.handle)
    }

    pub fn video_pixel_format(&self) -> String {
        self.video.video_pixel_format(&self.handle)
    }

    pub fn video_out_pixel_format(&self) -> String {
        self.video.video_out_pixel_format(&self.handle)
    }

    pub fn video_gamma(&self) -> String {
        self.video.video_gamma(&self.handle)
    }

    pub fn video_primaries(&self) -> String {
        self.video.video_primaries(&self.handle)
    }

    pub fn video_sig_peak(&self) -> Option<f64> {
        self.video.video_sig_peak(&self.handle)
    }

    pub fn panscan(&self) -> f64 {
        self.video.panscan()
    }

    pub fn set_panscan(&mut self, value: f64) -> Result<(), MpvError> {
        self.video.set_panscan(&self.handle, value)
    }

    pub fn set_panscan_fit(&mut self) -> Result<(), MpvError> {
        self.video.set_panscan_fit(&self.handle)
    }

    pub fn set_panscan_crop(&mut self) -> Result<(), MpvError> {
        self.video.set_panscan_crop(&self.handle)
    }

    /// Returns the effective aspect override (`"auto"` when unset).
    pub fn aspect_override(&self) -> String {
        self.video.aspect_override(&self.handle)
    }

    /// Applies an aspect override; `"auto"` restores the source ratio.
    pub fn set_aspect_override(&mut self, aspect: &str) -> Result<(), MpvError> {
        self.video.set_aspect_override(&self.handle, aspect)
    }

    /// Returns the current `video-zoom` factor.
    pub fn video_zoom(&self) -> f64 {
        self.video.video_zoom(&self.handle)
    }

    /// Sets the `video-zoom` factor.
    pub fn set_video_zoom(&mut self, zoom: f64) -> Result<(), MpvError> {
        self.video.set_video_zoom(&self.handle, zoom)
    }

    /// Adjusts the `video-zoom` factor by `delta`.
    pub fn adjust_video_zoom(&mut self, delta: f64) -> Result<(), MpvError> {
        self.video.adjust_video_zoom(&self.handle, delta)
    }

    /// Returns the current `(x, y)` video pan offsets.
    pub fn video_pan(&self) -> (f64, f64) {
        self.video.video_pan(&self.handle)
    }

    /// Sets the video pan offsets.
    pub fn set_video_pan(&mut self, x: f64, y: f64) -> Result<(), MpvError> {
        self.video.set_video_pan(&self.handle, x, y)
    }

    /// Recentres the video.
    pub fn reset_video_pan(&mut self) -> Result<(), MpvError> {
        self.video.reset_video_pan(&self.handle)
    }

    /// Applies a fit/fill/stretch/original scaling mode.
    pub fn set_scaling_mode(&mut self, mode: ScalingMode) -> Result<(), MpvError> {
        self.video.apply_scaling_mode(&self.handle, mode)
    }

    /// Returns the scaling mode derived from the live engine properties.
    pub fn scaling_mode(&self) -> ScalingMode {
        self.video.scaling_mode(&self.handle)
    }

    /// Returns the decoded frame dimensions in pixels.
    pub fn video_pixel_size(&self) -> (i64, i64) {
        self.video.video_pixel_size(&self.handle)
    }

    /// Applies a centred crop preset.
    pub fn set_crop_preset(&mut self, preset: CropPreset) -> Result<(), MpvError> {
        self.video.apply_crop_preset(&self.handle, preset)
    }

    /// Returns the active `video-crop` rectangle, empty when uncropped.
    pub fn video_crop(&self) -> String {
        self.video.video_crop(&self.handle)
    }

    /// Toggles deinterlacing.
    pub fn set_deinterlace(&mut self, enabled: bool) -> Result<(), MpvError> {
        self.video.set_deinterlace(&self.handle, enabled)
    }

    /// Returns whether deinterlacing is enabled.
    pub fn deinterlace(&self) -> bool {
        self.video.deinterlace(&self.handle)
    }

    /// Toggles debanding.
    pub fn set_deband_enabled(&mut self, enabled: bool) -> Result<(), MpvError> {
        self.video.set_deband_enabled(&self.handle, enabled)
    }

    /// Returns whether debanding is enabled.
    pub fn deband_enabled(&self) -> bool {
        self.video.deband_enabled(&self.handle)
    }

    /// Captures a screenshot through the engine to an explicit path.
    pub fn screenshot_to_file(&self, path: &str, mode: ScreenshotMode) -> Result<(), MpvError> {
        self.video.screenshot(&self.handle, path, mode)
    }

    pub fn frame_step(&mut self) -> Result<(), MpvError> {
        self.is_paused = true;
        self.state = PlaybackState::Paused;
        self.video.frame_step(&self.handle)
    }

    pub fn frame_back_step(&mut self) -> Result<(), MpvError> {
        self.is_paused = true;
        self.state = PlaybackState::Paused;
        self.video.frame_back_step(&self.handle)
    }

    pub fn step_by_seconds(&mut self, delta_seconds: f64) -> Result<(), MpvError> {
        if !self.is_paused {
            let _ = self.pause();
        }
        self.video.step_by_seconds(&self.handle, delta_seconds)
    }

    pub fn step_preset_forward(&mut self, preset: SteppingPreset) -> Result<(), MpvError> {
        self.video.step_preset_forward(&self.handle, preset)
    }

    pub fn step_preset_backward(&mut self, preset: SteppingPreset) -> Result<(), MpvError> {
        self.video.step_preset_backward(&self.handle, preset)
    }

    pub fn apply_video_adjustments(
        &self,
        adjustments: &VideoAdjustmentSettings,
    ) -> Result<(), MpvError> {
        self.video
            .apply_video_adjustments(&self.handle, adjustments)
    }

    pub fn reset_video_adjustments(&self) -> Result<(), MpvError> {
        self.video.reset_video_adjustments(&self.handle)
    }

    pub fn apply_anime4k(
        &self,
        preset: Anime4KPreset,
        quality: ShaderQuality,
    ) -> Result<Vec<String>, MpvError> {
        self.video.apply_anime4k(&self.handle, preset, quality)
    }

    pub fn apply_shader_pipeline(
        &self,
        preset: Anime4KPreset,
        quality: ShaderQuality,
        custom_shaders: &[CustomShaderEntry],
    ) -> Result<Vec<String>, MpvError> {
        self.video
            .apply_shader_pipeline(&self.handle, preset, quality, custom_shaders)
    }

    pub fn clear_shaders(&self) -> Result<(), MpvError> {
        self.video.clear_shaders(&self.handle)
    }

    pub fn set_deband_preset(
        &self,
        preset: DebandPreset,
        settings: &mut VideoAdjustmentSettings,
    ) -> Result<(), MpvError> {
        self.video.set_deband_preset(&self.handle, preset, settings)
    }

    pub fn set_target_peak(
        &self,
        target_peak_nits: u32,
        settings: &mut VideoAdjustmentSettings,
    ) -> Result<(), MpvError> {
        self.video
            .set_target_peak(&self.handle, target_peak_nits, settings)
    }

    // -------------------------------------------------------------------------
    // Track, Subtitle & Chapter Delegations
    // -------------------------------------------------------------------------

    pub fn tracks(&self) -> &[TrackInfo] {
        self.track.tracks()
    }

    pub fn chapters(&self) -> &[ChapterInfo] {
        self.track.chapters()
    }

    pub fn current_chapter(&self) -> Option<i64> {
        self.track.current_chapter()
    }

    pub fn chapter_count(&self) -> usize {
        self.track.chapter_count()
    }

    pub fn selected_track(&self, track_type: TrackType) -> Option<&TrackInfo> {
        self.track.selected_track(track_type)
    }

    pub fn tracks_by_type(&self, track_type: TrackType) -> Vec<&TrackInfo> {
        self.track.tracks_by_type(track_type)
    }

    pub fn set_track(&mut self, track_type: TrackType, id: Option<i64>) -> Result<(), MpvError> {
        self.track.set_track(&self.handle, track_type, id)
    }

    pub fn set_subtitle_track(&mut self, id: Option<i64>) -> Result<(), MpvError> {
        self.track.set_subtitle_track(&self.handle, id)
    }

    pub fn set_secondary_subtitle_track(&mut self, id: Option<i64>) -> Result<(), MpvError> {
        self.track.set_secondary_subtitle_track(&self.handle, id)
    }

    pub fn set_secondary_sub_track(&mut self, id: Option<i64>) -> Result<(), MpvError> {
        self.set_secondary_subtitle_track(id)
    }

    pub fn set_audio_track(&mut self, id: Option<i64>) -> Result<(), MpvError> {
        self.track.set_audio_track(&self.handle, id)
    }

    pub fn set_video_track(&mut self, id: Option<i64>) -> Result<(), MpvError> {
        self.track.set_video_track(&self.handle, id)
    }

    pub fn add_subtitle_file(&mut self, path: &str, select: bool) -> Result<(), MpvError> {
        self.track.add_subtitle_file(&self.handle, path, select)
    }

    pub fn sub_delay(&self) -> f64 {
        self.track.sub_delay()
    }

    pub fn set_subtitle_delay(&mut self, delay_seconds: f64) -> Result<(), MpvError> {
        self.track.set_subtitle_delay(&self.handle, delay_seconds)
    }

    pub fn sub_visibility(&self) -> bool {
        self.track.sub_visibility()
    }

    pub fn set_subtitle_visibility(&mut self, visible: bool) -> Result<(), MpvError> {
        self.track.set_subtitle_visibility(&self.handle, visible)
    }

    pub fn secondary_sub_track_id(&self) -> Option<i64> {
        self.track.secondary_sub_track_id()
    }

    pub fn secondary_sub_delay(&self) -> f64 {
        self.handle
            .get_property::<f64>("secondary-sub-delay")
            .unwrap_or(0.0)
    }

    pub fn set_secondary_sub_delay(&self, delay_seconds: f64) -> Result<(), MpvError> {
        self.track
            .set_secondary_sub_delay(&self.handle, delay_seconds)
    }

    pub fn set_secondary_sub_visibility(&self, visible: bool) -> Result<(), MpvError> {
        self.track
            .set_secondary_sub_visibility(&self.handle, visible)
    }

    pub fn configure_dual_subtitle_collision_avoidance(&self) -> Result<(), MpvError> {
        self.track
            .configure_dual_subtitle_collision_avoidance(&self.handle)
    }

    pub fn subtitle_styling(&self) -> &SubtitleStyling {
        self.track.subtitle_styling()
    }

    pub fn set_subtitle_styling(&mut self, styling: SubtitleStyling) -> Result<(), MpvError> {
        self.track.set_subtitle_styling(&self.handle, styling)
    }

    pub fn reset_subtitle_styling(&mut self) -> Result<(), MpvError> {
        self.track.reset_subtitle_styling(&self.handle)
    }

    /// Queries the container demuxer for embedded lyrics (ID3v2 USLT, Vorbis comments, MP4 ©lyr).
    pub fn embedded_lyrics(&self) -> Option<String> {
        self.handle.get_embedded_lyrics()
    }

    /// Queries the currently active media title from the demuxer or fallback metadata.
    pub fn media_title(&self) -> Option<String> {
        self.handle.get_property_string("media-title").ok()
    }

    pub fn next_chapter(&mut self) -> Result<(), MpvError> {
        self.track.next_chapter(&self.handle)
    }

    pub fn prev_chapter(&mut self) -> Result<(), MpvError> {
        self.track.prev_chapter(&self.handle)
    }

    pub fn set_chapter(&mut self, index: i64) -> Result<(), MpvError> {
        self.track.set_chapter(&self.handle, index)
    }

    // -------------------------------------------------------------------------
    // Recovery Notifications
    // -------------------------------------------------------------------------

    /// Takes the next pending notification generated by audio hotplug or decoder watchdog.
    pub fn take_recovery_notification(&mut self) -> Option<String> {
        self.audio
            .take_notification()
            .or_else(|| self.video.take_notification())
    }

    // -------------------------------------------------------------------------
    // Playlist Management
    // -------------------------------------------------------------------------

    pub fn playlist(&self) -> &[PlaylistItem] {
        &self.playlist
    }

    pub fn playlist_pos(&self) -> Option<usize> {
        self.playlist_pos
    }

    pub fn playlist_count(&self) -> usize {
        self.playlist.len()
    }

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

    pub fn playlist_append(&mut self, path: &str) -> Result<(), MpvError> {
        self.handle.playlist_append(path)?;
        self.refresh_playlist();
        Ok(())
    }

    pub fn playlist_append_play(&mut self, path: &str) -> Result<(), MpvError> {
        self.handle.playlist_append_play(path)?;
        self.refresh_playlist();
        Ok(())
    }

    pub fn playlist_play_index(&mut self, index: usize) -> Result<(), MpvError> {
        self.handle.playlist_play_index(index)?;
        self.refresh_playlist();
        Ok(())
    }

    pub fn playlist_next(&mut self) -> Result<(), MpvError> {
        self.handle.playlist_next()?;
        self.refresh_playlist();
        Ok(())
    }

    pub fn playlist_prev(&mut self) -> Result<(), MpvError> {
        self.handle.playlist_prev()?;
        self.refresh_playlist();
        Ok(())
    }

    pub fn playlist_remove(&mut self, index: usize) -> Result<(), MpvError> {
        self.handle.playlist_remove(index)?;
        self.refresh_playlist();
        Ok(())
    }

    pub fn playlist_move(&mut self, from_index: usize, to_index: usize) -> Result<(), MpvError> {
        self.handle.playlist_move(from_index, to_index)?;
        self.refresh_playlist();
        Ok(())
    }

    pub fn playlist_clear(&mut self) -> Result<(), MpvError> {
        self.handle.playlist_clear()?;
        self.refresh_playlist();
        Ok(())
    }

    pub fn playlist_shuffle(&mut self) -> Result<(), MpvError> {
        self.handle.playlist_shuffle()?;
        self.refresh_playlist();
        Ok(())
    }

    pub fn playlist_smart_shuffle(
        &mut self,
        track_metadata: &[crate::shuffle::ShuffleTrackInfo],
        seed: u64,
    ) -> Result<(), MpvError> {
        let n = self.playlist.len();
        if n <= 2 {
            return self.playlist_shuffle();
        }

        let engine = crate::shuffle::SmartShuffleEngine::default();
        let target_order = engine.calculate_order(track_metadata, seed);

        let mut current_positions: Vec<usize> = (0..n).collect();
        for (desired_target_pos, &desired_item_idx) in target_order.iter().enumerate() {
            if let Some(curr_pos) = current_positions
                .iter()
                .position(|&idx| idx == desired_item_idx)
            {
                if curr_pos != desired_target_pos {
                    self.handle.playlist_move(curr_pos, desired_target_pos)?;
                    let item = current_positions.remove(curr_pos);
                    current_positions.insert(desired_target_pos, item);
                }
            }
        }

        self.refresh_playlist();
        Ok(())
    }

    // -------------------------------------------------------------------------
    // A-B Loop & Repeat Mode
    // -------------------------------------------------------------------------

    pub fn ab_loop_state(&self) -> AbLoopState {
        self.ab_loop_state
    }

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
        let _ = self.handle.set_property_string(name, "");
    }

    // -------------------------------------------------------------------------
    // Core Playback Transport Commands
    // -------------------------------------------------------------------------

    pub fn load_file(&mut self, path: &str) -> Result<(), MpvError> {
        self.state = PlaybackState::Loading;
        self.eof_ended = false;
        self.current_file = Some(path.to_string());
        self.last_error = None;
        self.time_pos = 0.0;
        self.duration = 0.0;
        self.video.on_video_dimensions_changed(0, 0);
        self.video.on_video_format_changed(String::new());
        self.audio.on_codec_changed(String::new());
        self.track.on_track_list_changed(Vec::new());
        self.track.on_chapter_list_changed(Vec::new());
        self.track.on_chapter_changed(None);
        self.pending_resume_pos = None;
        self.pending_resume_audio = None;
        self.pending_resume_sub = None;

        self.handle.load_file(path, LoadFileMode::Replace)?;
        let _ = self.handle.play();
        self.is_paused = false;
        Ok(())
    }

    pub fn load_file_async(&mut self, path: &str) -> Result<u64, MpvError> {
        self.state = PlaybackState::Loading;
        self.current_file = Some(path.to_string());
        self.last_error = None;
        self.time_pos = 0.0;
        self.duration = 0.0;
        self.video.on_video_dimensions_changed(0, 0);
        self.video.on_video_format_changed(String::new());
        self.audio.on_codec_changed(String::new());
        self.track.on_track_list_changed(Vec::new());
        self.track.on_chapter_list_changed(Vec::new());
        self.track.on_chapter_changed(None);
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

    pub fn pending_resume_pos(&self) -> Option<f64> {
        self.pending_resume_pos
    }

    pub fn play(&mut self) -> Result<(), MpvError> {
        self.handle.play()?;
        self.is_paused = false;
        if self.state == PlaybackState::Paused || self.state == PlaybackState::Ended {
            self.eof_ended = false;
            self.state = PlaybackState::Playing;
        }
        Ok(())
    }

    pub fn pause(&mut self) -> Result<(), MpvError> {
        self.handle.pause()?;
        self.is_paused = true;
        if self.state == PlaybackState::Playing {
            self.state = PlaybackState::Paused;
        }
        Ok(())
    }

    pub fn toggle_pause(&mut self) -> Result<(), MpvError> {
        if self.is_paused {
            self.play()
        } else {
            self.pause()
        }
    }

    pub fn stop(&mut self) -> Result<(), MpvError> {
        self.handle.stop()?;
        self.eof_ended = false;
        self.state = PlaybackState::Idle;
        self.time_pos = 0.0;
        self.duration = 0.0;
        self.current_file = None;
        self.track.on_track_list_changed(Vec::new());
        self.track.on_chapter_list_changed(Vec::new());
        self.track.on_chapter_changed(None);
        Ok(())
    }

    pub fn seek(&mut self, target_seconds: f64, mode: SeekMode) -> Result<(), MpvError> {
        // An explicit seek is the user reviving a finished item, so the
        // sticky end-of-file latch is cleared here rather than from raw mpv
        // events (mpv also replays `seek` events of its own at end-of-file).
        if self.state == PlaybackState::Ended {
            self.eof_ended = false;
            self.state = if self.is_paused {
                PlaybackState::Paused
            } else {
                PlaybackState::Playing
            };
        }
        self.handle.seek(target_seconds, mode)
    }

    pub fn seek_relative(&mut self, seconds: f64) -> Result<(), MpvError> {
        self.seek(seconds, SeekMode::Relative)
    }

    pub fn seek_async(&mut self, target_seconds: f64, mode: SeekMode) -> Result<u64, MpvError> {
        let id = self.next_async_id;
        self.next_async_id += 1;
        let sec_str = target_seconds.to_string();
        self.handle
            .command_async(id, &["seek", &sec_str, mode.as_str()])?;
        Ok(id)
    }

    // -------------------------------------------------------------------------
    // Event Polling & Property Dispatch
    // -------------------------------------------------------------------------

    pub fn poll_events(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        loop {
            let event = self.handle.wait_event(0.0);
            if matches!(event, Event::None) {
                break;
            }
            self.handle_event(&event);
            events.push(event);
        }

        // Active decoder watchdog check during playback. The drop count is the
        // last value delivered by the observed `frame-drop-count` property; the
        // watchdog must not read the engine here, because this runs on the paint
        // thread and the round trip costs 6-35 ms under 4K decode load.
        let is_playing = self.state == PlaybackState::Playing;
        self.video.check_watchdog_observed(
            &self.handle,
            is_playing,
            self.is_buffering,
            self.time_pos,
        );

        // Audio hotplug maintenance
        self.audio.poll_recovery(&self.handle);

        events
    }

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
                    self.eof_ended = true;
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
                        && !Path::new(attempted).exists()
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
                // The unload that follows EOF also arrives as `Stop`; keep
                // the finished state instead of reporting a bare Idle.
                EndFileReason::Stop | EndFileReason::Quit
                    if self.state != PlaybackState::Loading && !self.eof_ended =>
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
                value,
                ..
            } => {
                self.handle_property_change(*reply_userdata, name, value.as_ref());
            }
            Event::PlaybackRestart | Event::Seek => {
                if let Ok(pos) = self.handle.get_property::<f64>("time-pos") {
                    self.time_pos = pos;
                }
                // Deliberately no state change here: mpv replays
                // `seek`/`playback-restart` while it settles at end-of-file,
                // which used to flip Ended back to Playing (and then hid the
                // end of the file entirely). `seek()`/`play()` revive a
                // finished item from the caller side.
            }
            Event::Shutdown => {
                self.state = PlaybackState::Idle;
            }
            _ => {}
        }
    }

    fn handle_property_change(
        &mut self,
        reply_userdata: u64,
        name: &str,
        value: Option<&crate::event::PropertyChangeValue>,
    ) {
        match reply_userdata {
            PROP_TIME_POS => {
                let pos = match value {
                    Some(crate::event::PropertyChangeValue::Double(v)) => Some(*v),
                    _ => self.handle.get_property::<f64>("time-pos").ok(),
                };
                if let Some(pos) = pos {
                    self.time_pos = pos;
                }
            }
            PROP_DURATION => {
                let dur = match value {
                    Some(crate::event::PropertyChangeValue::Double(v)) => Some(*v),
                    _ => self.handle.get_property::<f64>("duration").ok(),
                };
                if let Some(dur) = dur {
                    self.duration = dur;
                }
            }
            PROP_PAUSE => {
                let paused = match value {
                    Some(crate::event::PropertyChangeValue::Flag(v)) => Some(*v),
                    _ => self.handle.get_property::<bool>("pause").ok(),
                };
                if let Some(paused) = paused {
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
                let caching = match value {
                    Some(crate::event::PropertyChangeValue::Flag(v)) => Some(*v),
                    _ => self.handle.get_property::<bool>("paused-for-cache").ok(),
                };
                if let Some(caching) = caching {
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
                let speed = match value {
                    Some(crate::event::PropertyChangeValue::Double(v)) => Some(*v),
                    _ => self.handle.get_property::<f64>("speed").ok(),
                };
                if let Some(speed) = speed {
                    self.audio.on_speed_changed(speed);
                }
            }
            PROP_VOLUME => {
                let vol = match value {
                    Some(crate::event::PropertyChangeValue::Double(v)) => Some(*v),
                    _ => self.handle.get_property::<f64>("volume").ok(),
                };
                if let Some(vol) = vol {
                    self.audio.on_volume_changed(vol);
                }
            }
            PROP_MUTE => {
                let muted = match value {
                    Some(crate::event::PropertyChangeValue::Flag(v)) => Some(*v),
                    _ => self.handle.get_property::<bool>("mute").ok(),
                };
                if let Some(muted) = muted {
                    self.audio.on_mute_changed(muted);
                }
            }
            PROP_TRACK_LIST => {
                if let Ok(tracks) = self.handle.get_track_list() {
                    self.track.on_track_list_changed(tracks);
                }
            }
            PROP_CHAPTER_LIST => {
                if let Ok(chapters) = self.handle.get_chapter_list() {
                    self.track.on_chapter_list_changed(chapters);
                }
            }
            PROP_CHAPTER => {
                let ch = match value {
                    Some(crate::event::PropertyChangeValue::Int64(v)) => Some(*v),
                    _ => self.handle.get_property::<i64>("chapter").ok(),
                };
                if let Some(ch) = ch {
                    let ch_opt = if ch >= 0 { Some(ch) } else { None };
                    self.track.on_chapter_changed(ch_opt);
                }
            }
            PROP_HWDEC_CURRENT => {
                if let Ok(hw) = self.handle.get_property_string("hwdec-current") {
                    self.video.on_hwdec_changed(&hw);
                }
            }
            PROP_CURRENT_AO => {
                if let Ok(ao) = self.handle.get_property_string("current-ao") {
                    let has_audio = self.track.selected_track(TrackType::Audio).is_some();
                    let is_playing = self.state == PlaybackState::Playing;
                    self.audio
                        .on_current_ao_changed(&self.handle, &ao, has_audio, is_playing);
                }
            }
            PROP_AUDIO_DEVICE_LIST => {
                self.audio.on_device_list_changed(&self.handle);
            }
            PROP_FRAME_DROP_COUNT => {
                // Inline value from the observation; no engine re-read.
                if let Some(crate::event::PropertyChangeValue::Int64(v)) = value {
                    self.video.on_frame_drop_count_changed(*v);
                }
            }
            PROP_SUB_DELAY => {
                let delay = match value {
                    Some(crate::event::PropertyChangeValue::Double(v)) => Some(*v),
                    _ => self.handle.get_property::<f64>("sub-delay").ok(),
                };
                if let Some(delay) = delay {
                    self.track.on_sub_delay_changed(delay);
                }
            }
            PROP_SUB_VISIBILITY => {
                let vis = match value {
                    Some(crate::event::PropertyChangeValue::Flag(v)) => Some(*v),
                    _ => self.handle.get_property::<bool>("sub-visibility").ok(),
                };
                if let Some(vis) = vis {
                    self.track.on_sub_visibility_changed(vis);
                }
            }
            PROP_EOF_REACHED => {
                let eof = match value {
                    Some(crate::event::PropertyChangeValue::Flag(v)) => Some(*v),
                    _ => self.handle.get_property::<bool>("eof-reached").ok(),
                };
                if let Some(eof) = eof {
                    if eof && self.state != PlaybackState::Idle {
                        self.state = PlaybackState::Ended;
                        self.eof_ended = true;
                    }
                }
            }
            PROP_PLAYLIST | PROP_PLAYLIST_POS | PROP_PLAYLIST_COUNT => {
                self.refresh_playlist();
            }
            PROP_SECONDARY_SID => {
                self.track
                    .on_secondary_sid_changed(self.handle.get_secondary_sub_track());
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
                if name == "time-pos" {
                    if let Ok(pos) = self.handle.get_property::<f64>("time-pos") {
                        self.time_pos = pos;
                    }
                } else if name == "playlist" || name == "playlist-pos" || name == "playlist-count" {
                    self.refresh_playlist();
                } else if name == "secondary-sid" {
                    self.track
                        .on_secondary_sid_changed(self.handle.get_secondary_sub_track());
                } else if name == "ab-loop-a" || name == "ab-loop-b" {
                    self.refresh_ab_loop();
                } else if name == "audio-device-list" {
                    self.audio.on_device_list_changed(&self.handle);
                }
            }
        }
    }

    /// Queries all relevant media properties and refreshes internal caches across subsystems.
    pub fn refresh_metadata(&mut self) {
        if let Ok(dur) = self.handle.get_property::<f64>("duration") {
            self.duration = dur;
        }
        if let Ok(pos) = self.handle.get_property::<f64>("time-pos") {
            self.time_pos = pos;
        }
        if let Ok(paused) = self.handle.get_property::<bool>("pause") {
            self.is_paused = paused;
        }
        if let Ok(speed) = self.handle.get_property::<f64>("speed") {
            self.audio.on_speed_changed(speed);
        }
        if let Ok(vol) = self.handle.get_property::<f64>("volume") {
            self.audio.on_volume_changed(vol);
        }
        if let Ok(muted) = self.handle.get_property::<bool>("mute") {
            self.audio.on_mute_changed(muted);
        }
        if let Ok(ac) = self.handle.get_property_string("audio-codec-name") {
            self.audio.on_codec_changed(ac);
        }

        self.video.on_file_loaded(&self.handle);
        self.track.refresh(&self.handle);
        self.refresh_ab_loop();
    }
}
