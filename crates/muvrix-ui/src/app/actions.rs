//! Centralized playback action layer.
//!
//! Every user-facing playback operation — toolbar button, context-menu item,
//! OSD control or keyboard shortcut — is expressed as a [`PlayerAction`] and
//! dispatched through [`MuvrixApp::execute_player_action`]. That keeps the UI
//! and the shortcut registry in sync: there is exactly one implementation of
//! each operation, and the button label, tooltip shortcut hint and key binding
//! all derive from the same identifier.

use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use eframe::egui;
use muvrix_player::{
    AbLoopState, CropPreset, PlaybackState, RepeatMode, ScalingMode, ScreenshotMode, SeekMode,
    TrackType,
};
use serde::{Deserialize, Serialize};

use crate::app::formatting::format_timestamp;
use crate::app::MuvrixApp;
use crate::views::AppView;

/// Grouping used by the shortcut manager and the help overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ShortcutCategory {
    Playback,
    Seeking,
    Volume,
    Audio,
    Subtitles,
    Video,
    Playlist,
    Navigation,
    Window,
    Advanced,
}

impl ShortcutCategory {
    pub const ALL: &'static [ShortcutCategory] = &[
        ShortcutCategory::Playback,
        ShortcutCategory::Seeking,
        ShortcutCategory::Volume,
        ShortcutCategory::Audio,
        ShortcutCategory::Subtitles,
        ShortcutCategory::Video,
        ShortcutCategory::Playlist,
        ShortcutCategory::Navigation,
        ShortcutCategory::Window,
        ShortcutCategory::Advanced,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Playback => "Playback",
            Self::Seeking => "Seeking",
            Self::Volume => "Volume",
            Self::Audio => "Audio",
            Self::Subtitles => "Subtitles",
            Self::Video => "Video",
            Self::Playlist => "Playlist",
            Self::Navigation => "Navigation",
            Self::Window => "Window",
            Self::Advanced => "Advanced",
        }
    }
}

/// Cycle order for the aspect-ratio override.
pub const ASPECT_PRESETS: &[(&str, &str)] = &[
    ("auto", "Auto"),
    ("1:1", "1:1"),
    ("4:3", "4:3"),
    ("5:4", "5:4"),
    ("16:9", "16:9"),
    ("16:10", "16:10"),
    ("21:9", "21:9"),
    ("2.21:1", "2.21:1"),
    ("2.35:1", "2.35:1"),
    ("2.39:1", "2.39:1"),
];

/// Every playback operation MUVRIX implements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PlayerAction {
    // Playback
    PlayPause,
    Play,
    Pause,
    Stop,
    NextMedia,
    PreviousMedia,
    ToggleRepeatMode,
    ToggleAbLoop,
    FrameStepForward,
    FrameStepBack,
    SpeedIncrease,
    SpeedDecrease,
    SpeedReset,
    SpeedHalf,
    SpeedDouble,
    SpeedSetQuarter,
    SpeedSetHalf,
    SpeedSetThreeQuarter,
    SpeedSetNormal,
    SpeedSetOneAndQuarter,
    SpeedSetOneAndHalf,
    SpeedSetOneAndThreeQuarter,
    SpeedSetDouble,
    SpeedSetTwoAndHalf,
    SpeedSetTriple,
    SpeedSetQuadruple,

    // Seeking
    SeekForwardShort,
    SeekBackwardShort,
    SeekForwardVeryShort,
    SeekBackwardVeryShort,
    SeekForwardMedium,
    SeekBackwardMedium,
    SeekForwardLong,
    SeekBackwardLong,
    SeekForwardTenMinutes,
    SeekBackwardTenMinutes,
    SeekToBeginning,
    SeekToEnd,
    StopAndRewind,

    // Volume
    VolumeUp,
    VolumeDown,
    VolumeUpSmall,
    VolumeDownSmall,
    VolumeUpLarge,
    VolumeDownLarge,
    VolumeSetFull,
    VolumeSetHalf,
    ToggleMute,
    Unmute,
    RestoreVolume,

    // Audio
    CycleAudioTrack,
    CycleAudioDevice,
    AudioDelayIncrease,
    AudioDelayDecrease,
    AudioDelayReset,

    // Subtitles
    CycleSubtitleTrack,
    CycleSubtitleTrackReverse,
    ToggleSubtitles,
    CycleSecondarySubtitle,
    SubtitleDelayIncrease,
    SubtitleDelayDecrease,
    SubtitleDelayReset,
    SubtitleFontIncrease,
    SubtitleFontDecrease,
    SubtitleFontReset,
    SubtitleMoveUp,
    SubtitleMoveDown,
    AddSubtitleFile,

    // Video
    CycleAspectRatio,
    AspectRatioAuto,
    AspectRatio16x9,
    AspectRatio4x3,
    CycleCropPreset,
    ResetCrop,
    CycleScalingMode,
    ZoomIn,
    ZoomOut,
    ResetZoom,
    /// Upstream mpv's `Alt+BS`: drop the zoom *and* the pan offset.
    ResetVideoTransform,
    ZoomFit,
    ZoomFill,
    ZoomOriginal,
    PanLeft,
    PanRight,
    PanUp,
    PanDown,
    CenterVideo,
    ToggleDeinterlace,
    ToggleDeband,
    ToggleHardwareDecoding,
    Screenshot,
    ScreenshotWithoutSubtitles,

    // Playlist / chapters
    NextChapter,
    PreviousChapter,
    ToggleShuffle,
    ClearPlaylist,
    AddBookmark,

    // Navigation / window
    ToggleFullscreen,
    ExitFullscreen,
    ToggleCinema,
    ExportDiagnostics,
    ToggleAlwaysOnTop,
    ToggleMiniPlayer,
    TogglePlaylistDrawer,
    ToggleBrowserView,
    ToggleControls,
    ToggleStatistics,
    /// Prints the current position and duration as an on-screen message —
    /// VLC's `T`.
    ShowPlaybackPosition,
    OpenFile,

    // Advanced
    ShowShortcutHelp,
    ReloadMedia,
    ResetVideoAdjustments,
}

impl PlayerAction {
    pub const ALL: &'static [PlayerAction] = &[
        PlayerAction::PlayPause,
        PlayerAction::Play,
        PlayerAction::Pause,
        PlayerAction::Stop,
        PlayerAction::NextMedia,
        PlayerAction::PreviousMedia,
        PlayerAction::ToggleRepeatMode,
        PlayerAction::ToggleAbLoop,
        PlayerAction::FrameStepForward,
        PlayerAction::FrameStepBack,
        PlayerAction::SpeedIncrease,
        PlayerAction::SpeedDecrease,
        PlayerAction::SpeedReset,
        PlayerAction::SpeedHalf,
        PlayerAction::SpeedDouble,
        PlayerAction::SpeedSetQuarter,
        PlayerAction::SpeedSetHalf,
        PlayerAction::SpeedSetThreeQuarter,
        PlayerAction::SpeedSetNormal,
        PlayerAction::SpeedSetOneAndQuarter,
        PlayerAction::SpeedSetOneAndHalf,
        PlayerAction::SpeedSetOneAndThreeQuarter,
        PlayerAction::SpeedSetDouble,
        PlayerAction::SpeedSetTwoAndHalf,
        PlayerAction::SpeedSetTriple,
        PlayerAction::SpeedSetQuadruple,
        PlayerAction::SeekForwardShort,
        PlayerAction::SeekBackwardShort,
        PlayerAction::SeekForwardVeryShort,
        PlayerAction::SeekBackwardVeryShort,
        PlayerAction::SeekForwardMedium,
        PlayerAction::SeekBackwardMedium,
        PlayerAction::SeekForwardLong,
        PlayerAction::SeekBackwardLong,
        PlayerAction::SeekForwardTenMinutes,
        PlayerAction::SeekBackwardTenMinutes,
        PlayerAction::SeekToBeginning,
        PlayerAction::SeekToEnd,
        PlayerAction::StopAndRewind,
        PlayerAction::VolumeUp,
        PlayerAction::VolumeDown,
        PlayerAction::VolumeUpSmall,
        PlayerAction::VolumeDownSmall,
        PlayerAction::VolumeUpLarge,
        PlayerAction::VolumeDownLarge,
        PlayerAction::VolumeSetFull,
        PlayerAction::VolumeSetHalf,
        PlayerAction::ToggleMute,
        PlayerAction::Unmute,
        PlayerAction::RestoreVolume,
        PlayerAction::CycleAudioTrack,
        PlayerAction::CycleAudioDevice,
        PlayerAction::AudioDelayIncrease,
        PlayerAction::AudioDelayDecrease,
        PlayerAction::AudioDelayReset,
        PlayerAction::CycleSubtitleTrack,
        PlayerAction::CycleSubtitleTrackReverse,
        PlayerAction::ToggleSubtitles,
        PlayerAction::CycleSecondarySubtitle,
        PlayerAction::SubtitleDelayIncrease,
        PlayerAction::SubtitleDelayDecrease,
        PlayerAction::SubtitleDelayReset,
        PlayerAction::SubtitleFontIncrease,
        PlayerAction::SubtitleFontDecrease,
        PlayerAction::SubtitleFontReset,
        PlayerAction::SubtitleMoveUp,
        PlayerAction::SubtitleMoveDown,
        PlayerAction::AddSubtitleFile,
        PlayerAction::CycleAspectRatio,
        PlayerAction::AspectRatioAuto,
        PlayerAction::AspectRatio16x9,
        PlayerAction::AspectRatio4x3,
        PlayerAction::CycleCropPreset,
        PlayerAction::ResetCrop,
        PlayerAction::CycleScalingMode,
        PlayerAction::ZoomIn,
        PlayerAction::ZoomOut,
        PlayerAction::ResetZoom,
        PlayerAction::ResetVideoTransform,
        PlayerAction::ZoomFit,
        PlayerAction::ZoomFill,
        PlayerAction::ZoomOriginal,
        PlayerAction::PanLeft,
        PlayerAction::PanRight,
        PlayerAction::PanUp,
        PlayerAction::PanDown,
        PlayerAction::CenterVideo,
        PlayerAction::ToggleDeinterlace,
        PlayerAction::ToggleDeband,
        PlayerAction::ToggleHardwareDecoding,
        PlayerAction::Screenshot,
        PlayerAction::ScreenshotWithoutSubtitles,
        PlayerAction::NextChapter,
        PlayerAction::PreviousChapter,
        PlayerAction::ToggleShuffle,
        PlayerAction::ClearPlaylist,
        PlayerAction::AddBookmark,
        PlayerAction::ToggleFullscreen,
        PlayerAction::ExitFullscreen,
        PlayerAction::ToggleCinema,
        PlayerAction::ToggleAlwaysOnTop,
        PlayerAction::ToggleMiniPlayer,
        PlayerAction::TogglePlaylistDrawer,
        PlayerAction::ToggleBrowserView,
        PlayerAction::ToggleControls,
        PlayerAction::ToggleStatistics,
        PlayerAction::ShowPlaybackPosition,
        PlayerAction::ExportDiagnostics,
        PlayerAction::OpenFile,
        PlayerAction::ShowShortcutHelp,
        PlayerAction::ReloadMedia,
        PlayerAction::ResetVideoAdjustments,
    ];

    /// Whether holding the bound key down should keep firing this action.
    ///
    /// Continuous actions (seeking, volume, zoom, panning, delays, speed
    /// nudges, subtitle size/placement) must repeat so that a held key
    /// behaves like a jog wheel. Everything else is discrete: holding Space
    /// must not rapid-toggle play/pause, and holding F must not thrash
    /// fullscreen. Auto-repeat presses are filtered out for those.
    pub fn allows_key_repeat(&self) -> bool {
        use PlayerAction as A;
        matches!(
            self,
            A::SeekForwardShort
                | A::SeekBackwardShort
                | A::SeekForwardVeryShort
                | A::SeekBackwardVeryShort
                | A::SeekForwardMedium
                | A::SeekBackwardMedium
                | A::SeekForwardLong
                | A::SeekBackwardLong
                | A::SeekForwardTenMinutes
                | A::SeekBackwardTenMinutes
                | A::VolumeUp
                | A::VolumeUpSmall
                | A::VolumeUpLarge
                | A::VolumeDown
                | A::VolumeDownSmall
                | A::VolumeDownLarge
                | A::ZoomIn
                | A::ZoomOut
                | A::PanLeft
                | A::PanRight
                | A::PanUp
                | A::PanDown
                | A::AudioDelayIncrease
                | A::AudioDelayDecrease
                | A::SubtitleDelayIncrease
                | A::SubtitleDelayDecrease
                | A::SubtitleFontIncrease
                | A::SubtitleFontDecrease
                | A::SubtitleMoveUp
                | A::SubtitleMoveDown
                | A::SpeedIncrease
                | A::SpeedDecrease
        )
    }

    pub fn category(&self) -> ShortcutCategory {
        use PlayerAction as A;
        match self {
            A::PlayPause
            | A::Play
            | A::Pause
            | A::Stop
            | A::NextMedia
            | A::PreviousMedia
            | A::ToggleRepeatMode
            | A::ToggleAbLoop
            | A::FrameStepForward
            | A::FrameStepBack
            | A::SpeedIncrease
            | A::SpeedDecrease
            | A::SpeedReset
            | A::SpeedHalf
            | A::SpeedDouble
            | A::StopAndRewind
            | A::SpeedSetQuarter
            | A::SpeedSetHalf
            | A::SpeedSetThreeQuarter
            | A::SpeedSetNormal
            | A::SpeedSetOneAndQuarter
            | A::SpeedSetOneAndHalf
            | A::SpeedSetOneAndThreeQuarter
            | A::SpeedSetDouble
            | A::SpeedSetTwoAndHalf
            | A::SpeedSetTriple
            | A::SpeedSetQuadruple => ShortcutCategory::Playback,

            A::SeekForwardShort
            | A::SeekBackwardShort
            | A::SeekForwardVeryShort
            | A::SeekBackwardVeryShort
            | A::SeekForwardMedium
            | A::SeekBackwardMedium
            | A::SeekForwardLong
            | A::SeekBackwardLong
            | A::SeekForwardTenMinutes
            | A::SeekBackwardTenMinutes
            | A::SeekToBeginning
            | A::SeekToEnd => ShortcutCategory::Seeking,

            A::VolumeUp
            | A::VolumeDown
            | A::VolumeUpSmall
            | A::VolumeDownSmall
            | A::VolumeUpLarge
            | A::VolumeDownLarge
            | A::VolumeSetFull
            | A::VolumeSetHalf
            | A::ToggleMute
            | A::Unmute
            | A::RestoreVolume => ShortcutCategory::Volume,

            A::CycleAudioTrack
            | A::CycleAudioDevice
            | A::AudioDelayIncrease
            | A::AudioDelayDecrease
            | A::AudioDelayReset => ShortcutCategory::Audio,

            A::CycleSubtitleTrack
            | A::CycleSubtitleTrackReverse
            | A::ToggleSubtitles
            | A::CycleSecondarySubtitle
            | A::SubtitleDelayIncrease
            | A::SubtitleDelayDecrease
            | A::SubtitleDelayReset
            | A::SubtitleFontIncrease
            | A::SubtitleFontDecrease
            | A::SubtitleFontReset
            | A::SubtitleMoveUp
            | A::SubtitleMoveDown
            | A::AddSubtitleFile => ShortcutCategory::Subtitles,

            A::CycleAspectRatio
            | A::AspectRatioAuto
            | A::AspectRatio16x9
            | A::AspectRatio4x3
            | A::CycleCropPreset
            | A::ResetCrop
            | A::CycleScalingMode
            | A::ZoomIn
            | A::ZoomOut
            | A::ResetZoom
            | A::ResetVideoTransform
            | A::ZoomFit
            | A::ZoomFill
            | A::ZoomOriginal
            | A::PanLeft
            | A::PanRight
            | A::PanUp
            | A::PanDown
            | A::CenterVideo
            | A::ToggleDeinterlace
            | A::ToggleDeband
            | A::ToggleHardwareDecoding
            | A::Screenshot
            | A::ScreenshotWithoutSubtitles => ShortcutCategory::Video,

            A::NextChapter
            | A::PreviousChapter
            | A::ToggleShuffle
            | A::ClearPlaylist
            | A::AddBookmark => ShortcutCategory::Playlist,

            A::TogglePlaylistDrawer | A::ToggleBrowserView | A::OpenFile | A::ShowShortcutHelp => {
                ShortcutCategory::Navigation
            }

            A::ToggleFullscreen
            | A::ExitFullscreen
            | A::ToggleCinema
            | A::ToggleAlwaysOnTop
            | A::ToggleMiniPlayer => ShortcutCategory::Window,

            A::ToggleControls
            | A::ToggleStatistics
            | A::ShowPlaybackPosition
            | A::ExportDiagnostics
            | A::ReloadMedia
            | A::ResetVideoAdjustments => ShortcutCategory::Advanced,
        }
    }

    pub fn display_name(&self) -> &'static str {
        use PlayerAction as A;
        match self {
            A::PlayPause => "Play / Pause",
            A::Play => "Play",
            A::Pause => "Pause",
            A::Stop => "Stop",
            A::NextMedia => "Next Media",
            A::PreviousMedia => "Previous Media",
            A::ToggleRepeatMode => "Cycle Repeat Mode",
            A::ToggleAbLoop => "Cycle A-B Loop",
            A::FrameStepForward => "Next Frame",
            A::FrameStepBack => "Previous Frame",
            A::SpeedIncrease => "Playback Speed Up",
            A::SpeedDecrease => "Playback Speed Down",
            A::SpeedReset => "Reset Playback Speed",
            A::SpeedHalf => "Halve Playback Speed",
            A::SpeedDouble => "Double Playback Speed",
            A::StopAndRewind => "Stop and Rewind",
            A::SpeedSetQuarter => "Speed: 0.25×",
            A::SpeedSetHalf => "Speed: 0.5×",
            A::SpeedSetThreeQuarter => "Speed: 0.75×",
            A::SpeedSetNormal => "Speed: 1.0×",
            A::SpeedSetOneAndQuarter => "Speed: 1.25×",
            A::SpeedSetOneAndHalf => "Speed: 1.5×",
            A::SpeedSetOneAndThreeQuarter => "Speed: 1.75×",
            A::SpeedSetDouble => "Speed: 2.0×",
            A::SpeedSetTwoAndHalf => "Speed: 2.5×",
            A::SpeedSetTriple => "Speed: 3.0×",
            A::SpeedSetQuadruple => "Speed: 4.0×",
            // The magnitudes are user-configurable, so the labels describe the
            // step class rather than hardcoding a duration that the tuning
            // panel can change.
            A::SeekForwardShort => "Seek Forward (Small Step)",
            A::SeekBackwardShort => "Seek Back (Small Step)",
            A::SeekForwardVeryShort => "Seek Forward (Very Short)",
            A::SeekBackwardVeryShort => "Seek Back (Very Short)",
            A::SeekForwardMedium => "Seek Forward (Medium Step)",
            A::SeekBackwardMedium => "Seek Back (Medium Step)",
            A::SeekForwardLong => "Seek Forward (Large Step)",
            A::SeekBackwardLong => "Seek Back (Large Step)",
            A::SeekForwardTenMinutes => "Seek Forward 10 min",
            A::SeekBackwardTenMinutes => "Seek Back 10 min",
            A::SeekToBeginning => "Seek to Beginning",
            A::SeekToEnd => "Seek to End",
            A::VolumeUp => "Volume Up",
            A::VolumeDown => "Volume Down",
            A::VolumeUpSmall => "Volume Up (Fine)",
            A::VolumeDownSmall => "Volume Down (Fine)",
            A::VolumeUpLarge => "Volume Up (Coarse)",
            A::VolumeDownLarge => "Volume Down (Coarse)",
            A::VolumeSetFull => "Volume: 100%",
            A::VolumeSetHalf => "Volume: 50%",
            A::ToggleMute => "Mute / Unmute",
            A::Unmute => "Unmute",
            A::RestoreVolume => "Restore Previous Volume",
            A::CycleAudioTrack => "Cycle Audio Track",
            A::CycleAudioDevice => "Cycle Audio Device",
            A::AudioDelayIncrease => "Audio Delay +",
            A::AudioDelayDecrease => "Audio Delay −",
            A::AudioDelayReset => "Reset Audio Delay",
            A::CycleSubtitleTrack => "Cycle Subtitle Track",
            A::CycleSubtitleTrackReverse => "Cycle Subtitle Track (Reverse)",
            A::ToggleSubtitles => "Toggle Subtitles",
            A::CycleSecondarySubtitle => "Cycle Secondary Subtitle",
            A::SubtitleDelayIncrease => "Subtitle Delay +",
            A::SubtitleDelayDecrease => "Subtitle Delay −",
            A::SubtitleDelayReset => "Reset Subtitle Delay",
            A::SubtitleFontIncrease => "Subtitle Font Larger",
            A::SubtitleFontDecrease => "Subtitle Font Smaller",
            A::SubtitleFontReset => "Reset Subtitle Font Size",
            A::SubtitleMoveUp => "Subtitles Move Up",
            A::SubtitleMoveDown => "Subtitles Move Down",
            A::AddSubtitleFile => "Load Subtitle File",
            A::CycleAspectRatio => "Cycle Aspect Ratio",
            A::AspectRatioAuto => "Aspect Ratio: Auto",
            A::AspectRatio16x9 => "Aspect Ratio: 16:9",
            A::AspectRatio4x3 => "Aspect Ratio: 4:3",
            A::CycleCropPreset => "Cycle Crop Preset",
            A::ResetCrop => "Reset Crop",
            A::CycleScalingMode => "Cycle Fit / Fill / Stretch",
            A::ZoomIn => "Zoom In",
            A::ZoomOut => "Zoom Out",
            A::ResetZoom => "Reset Zoom",
            A::ResetVideoTransform => "Reset Zoom & Pan",
            A::ZoomFit => "Zoom: Fit",
            A::ZoomFill => "Zoom: Fill",
            A::ZoomOriginal => "Zoom: Original Size",
            A::PanLeft => "Pan Left",
            A::PanRight => "Pan Right",
            A::PanUp => "Pan Up",
            A::PanDown => "Pan Down",
            A::CenterVideo => "Center Video",
            A::ToggleDeinterlace => "Toggle Deinterlacing",
            A::ToggleDeband => "Toggle Debanding",
            A::ToggleHardwareDecoding => "Toggle Hardware Decoding",
            A::Screenshot => "Take Screenshot",
            A::ScreenshotWithoutSubtitles => "Screenshot Without Subtitles",
            A::NextChapter => "Next Chapter",
            A::PreviousChapter => "Previous Chapter",
            A::ToggleShuffle => "Shuffle Queue",
            A::ClearPlaylist => "Clear Queue",
            A::AddBookmark => "Add Bookmark",
            A::ToggleFullscreen => "Toggle Fullscreen",
            A::ExitFullscreen => "Exit Fullscreen",
            A::ToggleCinema => "Toggle Cinema Mode",
            A::ExportDiagnostics => "Export Playback Report",
            A::ToggleAlwaysOnTop => "Toggle Always on Top",
            A::ToggleMiniPlayer => "Toggle Mini-Player",
            A::TogglePlaylistDrawer => "Toggle Playlist Drawer",
            A::ToggleBrowserView => "Toggle Media Browser",
            A::ToggleControls => "Toggle On-Screen Controls",
            A::ToggleStatistics => "Toggle Playback Statistics",
            A::ShowPlaybackPosition => "Show Playback Position",
            A::OpenFile => "Open Media File",
            A::ShowShortcutHelp => "Keyboard Shortcut Help",
            A::ReloadMedia => "Reload Current Media",
            A::ResetVideoAdjustments => "Reset Video Adjustments",
        }
    }
}

impl MuvrixApp {
    /// Shows a transient on-screen confirmation (used by shortcuts and OSD controls).
    pub fn push_osd(&mut self, text: impl Into<String>) {
        self.osd_notice = Some((text.into(), Instant::now()));
        self.last_mouse_activity = Instant::now();
    }

    /// Executes one centralized player action.
    pub fn execute_player_action(&mut self, action: PlayerAction, ctx: &egui::Context) {
        use PlayerAction as A;

        match action {
            A::PlayPause => {
                // Stopping unloads the item, so Play/Pause had nothing to act
                // on and still reported "Paused". VLC restarts the current
                // entry from Stop; re-open the remembered path instead.
                if *self.controller.state() == PlaybackState::Idle {
                    if self.path_input.trim().is_empty() {
                        self.push_osd("Nothing to play");
                    } else {
                        let path = self.path_input.clone();
                        self.load_file(&path);
                    }
                    return;
                }
                // A finished item is not paused: Play/Pause has to rewind it,
                // otherwise Space at the end of the file did nothing at all.
                if *self.controller.state() == PlaybackState::Ended {
                    let _ = self.controller.seek(0.0, SeekMode::Absolute);
                    let _ = self.controller.play();
                    self.push_osd("▶ Replaying");
                    return;
                }
                self.toggle_pause();
                self.push_osd(if self.controller.is_paused() {
                    "⏸ Paused"
                } else {
                    "▶ Playing"
                });
            }
            A::Play => {
                let _ = self.controller.play();
                self.push_osd("▶ Playing");
            }
            A::Pause => {
                let _ = self.controller.pause();
                self.push_osd("⏸ Paused");
            }
            A::Stop => {
                // Keep the title so Play can restart it and Ctrl+R can reload
                // it; `controller.stop()` clears its own `current_file`.
                if let Some(file) = self.controller.current_file() {
                    self.path_input = file.to_string();
                }
                let _ = self.controller.stop();
                self.push_osd("■ Stopped");
            }
            A::NextMedia => {
                let _ = self.controller.playlist_next();
            }
            A::PreviousMedia => {
                let _ = self.controller.playlist_prev();
            }
            A::ToggleRepeatMode => {
                self.cycle_repeat_mode();
                self.push_osd(format!("Repeat: {}", self.repeat_label()));
            }
            A::ToggleAbLoop => self.cycle_ab_loop(),
            A::FrameStepForward => {
                let _ = self.controller.step_preset_forward(self.stepping_preset);
            }
            A::FrameStepBack => {
                let _ = self.controller.step_preset_backward(self.stepping_preset);
            }
            A::SpeedIncrease => {
                let step = self.shortcut_tuning().speed_step;
                let _ = self.controller.adjust_speed(step);
                self.push_osd(format!("Speed: {:.2}×", self.controller.speed()));
            }
            A::SpeedDecrease => {
                let step = self.shortcut_tuning().speed_step;
                let _ = self.controller.adjust_speed(-step);
                self.push_osd(format!("Speed: {:.2}×", self.controller.speed()));
            }
            A::SpeedReset => {
                let _ = self.controller.reset_speed();
                self.push_osd("Speed: 1.00×");
            }
            A::SpeedHalf => {
                let _ = self
                    .controller
                    .set_speed((self.controller.speed() * 0.5).max(0.1));
                self.push_osd(format!("Speed: {:.2}×", self.controller.speed()));
            }
            A::SpeedDouble => {
                let _ = self
                    .controller
                    .set_speed((self.controller.speed() * 2.0).min(8.0));
                self.push_osd(format!("Speed: {:.2}×", self.controller.speed()));
            }
            A::StopAndRewind => {
                let _ = self.controller.stop();
                let _ = self.controller.seek(0.0, SeekMode::Absolute);
                self.push_osd("■ Stopped");
            }
            A::SpeedSetQuarter
            | A::SpeedSetHalf
            | A::SpeedSetThreeQuarter
            | A::SpeedSetNormal
            | A::SpeedSetOneAndQuarter
            | A::SpeedSetOneAndHalf
            | A::SpeedSetOneAndThreeQuarter
            | A::SpeedSetDouble
            | A::SpeedSetTwoAndHalf
            | A::SpeedSetTriple
            | A::SpeedSetQuadruple => {
                let rate = match action {
                    A::SpeedSetQuarter => 0.25,
                    A::SpeedSetHalf => 0.5,
                    A::SpeedSetThreeQuarter => 0.75,
                    A::SpeedSetOneAndQuarter => 1.25,
                    A::SpeedSetOneAndHalf => 1.5,
                    A::SpeedSetOneAndThreeQuarter => 1.75,
                    A::SpeedSetDouble => 2.0,
                    A::SpeedSetTwoAndHalf => 2.5,
                    A::SpeedSetTriple => 3.0,
                    A::SpeedSetQuadruple => 4.0,
                    _ => 1.0,
                };
                match self.controller.set_speed(rate) {
                    Ok(()) => self.push_osd(format!("Speed: {rate:.2}×")),
                    Err(e) => self.push_osd(format!("Speed unavailable: {e}")),
                }
            }

            A::SeekForwardShort => {
                let step = self.shortcut_tuning().seek_small_secs;
                self.seek_with_osd(step)
            }
            A::SeekBackwardShort => {
                let step = self.shortcut_tuning().seek_small_secs;
                self.seek_with_osd(-step)
            }
            A::SeekForwardVeryShort => self.seek_with_osd(1.0),
            A::SeekBackwardVeryShort => self.seek_with_osd(-1.0),
            A::SeekForwardMedium => {
                let step = self.shortcut_tuning().seek_medium_secs;
                self.seek_with_osd(step)
            }
            A::SeekBackwardMedium => {
                let step = self.shortcut_tuning().seek_medium_secs;
                self.seek_with_osd(-step)
            }
            A::SeekForwardLong => {
                let step = self.shortcut_tuning().seek_large_secs;
                self.seek_with_osd(step)
            }
            A::SeekBackwardLong => {
                let step = self.shortcut_tuning().seek_large_secs;
                self.seek_with_osd(-step)
            }
            A::SeekForwardTenMinutes => self.seek_with_osd(600.0),
            A::SeekBackwardTenMinutes => self.seek_with_osd(-600.0),
            A::SeekToBeginning => {
                let _ = self.controller.seek(0.0, SeekMode::Absolute);
                self.push_osd("⏮ Beginning");
            }
            A::SeekToEnd => {
                let duration = self.controller.duration();
                if duration > 0.0 {
                    let _ = self
                        .controller
                        .seek((duration - 0.1).max(0.0), SeekMode::Absolute);
                    self.push_osd("⏭ End");
                }
            }

            A::VolumeUp => {
                let step = self.shortcut_tuning().volume_step;
                let _ = self.controller.adjust_volume(step);
                self.push_osd(format!("Volume: {:.0}%", self.controller.volume()));
            }
            A::VolumeDown => {
                let step = self.shortcut_tuning().volume_step;
                let _ = self.controller.adjust_volume(-step);
                self.push_osd(format!("Volume: {:.0}%", self.controller.volume()));
            }
            A::VolumeUpSmall => {
                let _ = self.controller.adjust_volume(1.0);
                self.push_osd(format!("Volume: {:.0}%", self.controller.volume()));
            }
            A::VolumeDownSmall => {
                let _ = self.controller.adjust_volume(-1.0);
                self.push_osd(format!("Volume: {:.0}%", self.controller.volume()));
            }
            A::VolumeUpLarge => {
                let _ = self.controller.adjust_volume(10.0);
                self.push_osd(format!("Volume: {:.0}%", self.controller.volume()));
            }
            A::VolumeDownLarge => {
                let _ = self.controller.adjust_volume(-10.0);
                self.push_osd(format!("Volume: {:.0}%", self.controller.volume()));
            }
            A::VolumeSetFull => {
                let _ = self.controller.set_volume(100.0);
                self.push_osd("Volume: 100%");
            }
            A::VolumeSetHalf => {
                let _ = self.controller.set_volume(50.0);
                self.push_osd("Volume: 50%");
            }
            A::ToggleMute => {
                // Remember a usable level so "Restore Previous Volume" has
                // something to return to after a mute.
                if !self.controller.is_muted() {
                    let current = self.controller.volume();
                    if current > 0.0 {
                        self.pre_mute_volume = Some(current);
                    }
                }
                let _ = self.controller.toggle_mute();
                self.push_osd(if self.controller.is_muted() {
                    "🔇 Muted"
                } else {
                    "🔊 Unmuted"
                });
            }
            A::Unmute => {
                let _ = self.controller.set_mute(false);
                self.push_osd("🔊 Unmuted");
            }
            A::RestoreVolume => {
                if let Some(volume) = self.pre_mute_volume {
                    let _ = self.controller.set_mute(false);
                    let _ = self.controller.set_volume(volume);
                    self.push_osd(format!("Volume: {volume:.0}%"));
                } else {
                    self.push_osd("No previous volume");
                }
            }

            A::CycleAudioTrack => self.cycle_audio_track(),
            A::CycleAudioDevice => self.cycle_audio_device(),
            A::AudioDelayIncrease => {
                let step = self.shortcut_tuning().audio_delay_step_ms / 1000.0;
                self.adjust_audio_delay(step)
            }
            A::AudioDelayDecrease => {
                let step = self.shortcut_tuning().audio_delay_step_ms / 1000.0;
                self.adjust_audio_delay(-step)
            }
            A::AudioDelayReset => {
                let _ = self.controller.set_audio_delay(0.0);
                self.push_osd("Audio delay: 0.00s");
            }

            A::CycleSubtitleTrack => self.cycle_subtitle_track(false),
            A::CycleSubtitleTrackReverse => self.cycle_subtitle_track(true),
            A::ToggleSubtitles => {
                let visible = !self.controller.sub_visibility();
                let _ = self.controller.set_subtitle_visibility(visible);
                self.push_osd(if visible {
                    "Subtitles on"
                } else {
                    "Subtitles off"
                });
            }
            A::CycleSecondarySubtitle => self.cycle_secondary_subtitle(),
            A::SubtitleDelayIncrease => {
                let step = self.shortcut_tuning().subtitle_delay_step_ms / 1000.0;
                self.adjust_subtitle_delay(step)
            }
            A::SubtitleDelayDecrease => {
                let step = self.shortcut_tuning().subtitle_delay_step_ms / 1000.0;
                self.adjust_subtitle_delay(-step)
            }
            A::SubtitleDelayReset => {
                let _ = self.controller.set_subtitle_delay(0.0);
                self.push_osd("Subtitle delay: 0.00s");
            }
            A::SubtitleFontIncrease
            | A::SubtitleFontDecrease
            | A::SubtitleFontReset
            | A::SubtitleMoveUp
            | A::SubtitleMoveDown => {
                let mut styling = self.controller.subtitle_styling().clone();
                match action {
                    A::SubtitleFontReset => styling.font_size = 45,
                    A::SubtitleFontIncrease => styling.font_size = (styling.font_size + 5).min(120),
                    A::SubtitleFontDecrease => styling.font_size = (styling.font_size - 5).max(10),
                    // `pos` runs 0 (top) .. 100 (bottom).
                    A::SubtitleMoveUp => styling.pos = (styling.pos - 5).clamp(0, 100),
                    A::SubtitleMoveDown => styling.pos = (styling.pos + 5).clamp(0, 100),
                    _ => {}
                }
                match self.controller.set_subtitle_styling(styling) {
                    Ok(()) => self.push_osd(match action {
                        A::SubtitleMoveUp | A::SubtitleMoveDown => {
                            format!(
                                "Subtitle position: {}",
                                self.controller.subtitle_styling().pos
                            )
                        }
                        _ => format!(
                            "Subtitle size: {}",
                            self.controller.subtitle_styling().font_size
                        ),
                    }),
                    Err(e) => self.push_osd(format!("Subtitle styling unavailable: {e}")),
                }
            }
            A::AddSubtitleFile => self.add_subtitle_dialog(),

            A::CycleAspectRatio => {
                // libmpv reports `video-aspect-override` as a float (`-1.000000`
                // when unset, `1.777778` after `16:9`), never as the label we
                // wrote. Comparing strings therefore always failed to find the
                // current preset and re-applied the same one, so the key looked
                // dead after the first press. Match by ratio instead.
                let current = parse_aspect_ratio(&self.controller.aspect_override());
                let idx = current
                    .and_then(|ratio| {
                        ASPECT_PRESETS.iter().position(|(value, _)| {
                            parse_aspect_ratio(value)
                                .map(|preset| (preset - ratio).abs() < 0.01)
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(0);
                let (value, label) = ASPECT_PRESETS[(idx + 1) % ASPECT_PRESETS.len()];
                self.apply_aspect(value);
                self.push_osd(format!("Aspect: {label}"));
            }
            A::AspectRatioAuto => {
                self.apply_aspect("auto");
                self.push_osd("Aspect: Auto");
            }
            A::AspectRatio16x9 => {
                self.apply_aspect("16:9");
                self.push_osd("Aspect: 16:9");
            }
            A::AspectRatio4x3 => {
                self.apply_aspect("4:3");
                self.push_osd("Aspect: 4:3");
            }
            A::CycleCropPreset => {
                let idx = self.crop_preset_index();
                let next = CropPreset::ALL[(idx + 1) % CropPreset::ALL.len()];
                match self.controller.set_crop_preset(next) {
                    Ok(()) => self.push_osd(format!("Crop: {}", next.label())),
                    Err(e) => self.push_osd(format!("Crop unavailable: {e}")),
                }
            }
            A::ResetCrop => match self.controller.set_crop_preset(CropPreset::Default) {
                Ok(()) => self.push_osd("Crop: Default"),
                Err(e) => self.push_osd(format!("Crop unavailable: {e}")),
            },
            A::ZoomFit | A::ZoomFill | A::ZoomOriginal => {
                let mode = match action {
                    A::ZoomFill => ScalingMode::Fill,
                    A::ZoomOriginal => ScalingMode::Original,
                    _ => ScalingMode::Fit,
                };
                match self.controller.set_scaling_mode(mode) {
                    Ok(()) => self.push_osd(format!("View: {}", mode.label())),
                    Err(e) => self.push_osd(format!("View mode unavailable: {e}")),
                }
            }
            A::CycleScalingMode => {
                let current = self.controller.scaling_mode();
                let idx = ScalingMode::ALL
                    .iter()
                    .position(|m| *m == current)
                    .unwrap_or(0);
                let next = ScalingMode::ALL[(idx + 1) % ScalingMode::ALL.len()];
                match self.controller.set_scaling_mode(next) {
                    Ok(()) => self.push_osd(format!("View: {}", next.label())),
                    Err(e) => self.push_osd(format!("View mode unavailable: {e}")),
                }
            }
            A::ZoomIn => match self.controller.adjust_video_zoom(0.1) {
                Ok(()) => self.push_osd(format!("Zoom: {:+.1}", self.controller.video_zoom())),
                Err(e) => self.push_osd(format!("Zoom unavailable: {e}")),
            },
            A::ZoomOut => match self.controller.adjust_video_zoom(-0.1) {
                Ok(()) => self.push_osd(format!("Zoom: {:+.1}", self.controller.video_zoom())),
                Err(e) => self.push_osd(format!("Zoom unavailable: {e}")),
            },
            A::ResetZoom => {
                let _ = self.controller.set_video_zoom(0.0);
                self.push_osd("Zoom: 0.0");
            }
            // mpv's `Alt+BS` resets video-zoom, video-pan-x and video-pan-y in
            // one keystroke. ResetZoom alone leaves the picture off-centre,
            // which looks like the key did nothing when the offset was the
            // only transformation in play.
            A::ResetVideoTransform => {
                let _ = self.controller.set_video_zoom(0.0);
                let _ = self.controller.reset_video_pan();
                self.push_osd("View reset");
            }
            A::PanLeft => self.pan_video(-0.1, 0.0),
            A::PanRight => self.pan_video(0.1, 0.0),
            A::PanUp => self.pan_video(0.0, 0.1),
            A::PanDown => self.pan_video(0.0, -0.1),
            A::CenterVideo => {
                let _ = self.controller.reset_video_pan();
                self.push_osd("Video centred");
            }
            A::ToggleDeinterlace => {
                let enabled = !self.controller.deinterlace();
                let _ = self.controller.set_deinterlace(enabled);
                self.push_osd(if enabled {
                    "Deinterlacing on"
                } else {
                    "Deinterlacing off"
                });
            }
            A::ToggleDeband => {
                let enabled = !self.controller.deband_enabled();
                let _ = self.controller.set_deband_enabled(enabled);
                self.push_osd(if enabled {
                    "Debanding on"
                } else {
                    "Debanding off"
                });
            }
            A::ToggleHardwareDecoding => {
                let enabled = self.controller.requested_hwdec() != "no";
                let target = if enabled { "no" } else { "auto-safe" };
                match self.controller.set_hwdec_mode(target) {
                    Ok(()) => self.push_osd(if enabled {
                        "Hardware decoding off"
                    } else {
                        "Hardware decoding auto"
                    }),
                    Err(e) => self.push_osd(format!("Decoder switch failed: {e}")),
                }
            }
            A::Screenshot => self.capture_screenshot(ScreenshotMode::Subtitles),
            A::ScreenshotWithoutSubtitles => self.capture_screenshot(ScreenshotMode::Video),

            A::NextChapter => {
                let _ = self.controller.next_chapter();
            }
            A::PreviousChapter => {
                let _ = self.controller.prev_chapter();
            }
            A::ToggleShuffle => self.execute_shuffle(),
            A::ClearPlaylist => {
                let _ = self.controller.playlist_clear();
                self.sync_active_queue_to_db();
                self.push_osd("Queue cleared");
            }
            A::AddBookmark => {
                let label = format!(
                    "Bookmark {}",
                    crate::app::format_timestamp(self.controller.time_pos())
                );
                self.add_bookmark(&label, None);
                self.push_osd("Bookmark added");
            }

            A::ToggleFullscreen => self.toggle_fullscreen(ctx),
            A::ExitFullscreen => self.exit_fullscreen(ctx),
            A::ToggleCinema => self.toggle_cinema(ctx),
            A::ExportDiagnostics => {
                let result = self
                    .diagnostics
                    .as_ref()
                    .ok_or_else(|| "diagnostics unavailable".to_string())
                    .and_then(|probe| probe.export_report());
                match result {
                    Ok(path) => self.push_osd(format!("Report saved: {}", path.display())),
                    Err(e) => self.push_osd(format!("Report failed: {e}")),
                }
            }
            A::ToggleAlwaysOnTop => {
                self.always_on_top = !self.always_on_top;
                ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(if self.always_on_top {
                    egui::WindowLevel::AlwaysOnTop
                } else {
                    egui::WindowLevel::Normal
                }));
                self.push_osd(if self.always_on_top {
                    "Always on top: on"
                } else {
                    "Always on top: off"
                });
            }
            A::ToggleMiniPlayer => self.toggle_pip(ctx),
            A::TogglePlaylistDrawer => {
                self.show_playlist_drawer = !self.show_playlist_drawer;
            }
            A::ToggleBrowserView => {
                self.current_view = if self.current_view == AppView::Browser {
                    AppView::Player
                } else {
                    AppView::Browser
                };
            }
            A::ToggleControls => {
                self.ui_overlay_visible = !self.ui_overlay_visible;
                self.osd_manually_hidden = !self.ui_overlay_visible;
                self.last_mouse_activity = Instant::now();
            }
            A::ToggleStatistics => {
                self.show_playback_stats = !self.show_playback_stats;
                if let Some(probe) = &self.diagnostics {
                    probe.set_enabled(self.show_playback_stats);
                }
            }
            A::ShowPlaybackPosition => {
                let pos = self.controller.time_pos();
                let duration = self.controller.duration();
                let text = if duration > 0.0 {
                    format!("{} / {}", format_timestamp(pos), format_timestamp(duration))
                } else {
                    format_timestamp(pos)
                };
                self.push_osd(text);
            }
            A::OpenFile => self.open_file_dialog(),
            A::ShowShortcutHelp => {
                self.show_shortcuts = !self.show_shortcuts;
            }
            A::ReloadMedia => self.reload_current_media(),
            A::ResetVideoAdjustments => match self.controller.reset_video_adjustments() {
                Ok(()) => self.push_osd("Video adjustments reset"),
                Err(e) => self.push_osd(format!("Reset failed: {e}")),
            },
        }
    }

    fn repeat_label(&self) -> &'static str {
        match self.controller.repeat_mode() {
            RepeatMode::Off => "Off",
            RepeatMode::One => "One",
            RepeatMode::All => "All",
        }
    }

    /// Sanitized shortcut magnitudes currently in effect.
    ///
    /// Read on every dispatch so a change in the settings panel applies to the
    /// next key press without restarting playback.
    pub(crate) fn shortcut_tuning(&self) -> crate::app::shortcuts::ShortcutTuning {
        self.keybindings.tuning.sanitized()
    }

    fn seek_with_osd(&mut self, delta: f64) {
        let target = (self.controller.time_pos() + delta).max(0.0);
        match self.controller.seek(delta, SeekMode::Relative) {
            Ok(()) => {
                let sign = if delta >= 0.0 { "⏩" } else { "⏪" };
                self.push_osd(format!(
                    "{sign} {} / {}",
                    crate::app::format_timestamp(target),
                    crate::app::format_timestamp(self.controller.duration())
                ));
            }
            Err(e) => self.push_osd(format!("Seek failed: {e}")),
        }
    }

    fn pan_video(&mut self, dx: f64, dy: f64) {
        let (x, y) = self.controller.video_pan();
        match self.controller.set_video_pan(x + dx, y + dy) {
            Ok(()) => {
                let (nx, ny) = self.controller.video_pan();
                self.push_osd(format!("Pan: {nx:+.2} / {ny:+.2}"));
            }
            Err(e) => self.push_osd(format!("Pan unavailable: {e}")),
        }
    }

    /// Applies an aspect-ratio override, recording failures as an OSD message.
    pub fn apply_aspect(&mut self, aspect: &str) {
        if let Err(e) = self.controller.set_aspect_override(aspect) {
            self.push_osd(format!("Aspect unavailable: {e}"));
        }
    }

    /// Index of the crop preset that the current engine geometry most closely
    /// matches. Used so the crop button cycles predictably from any state.
    pub fn crop_preset_index(&self) -> usize {
        let crop = self.controller.video_crop();
        if crop.is_empty() {
            return 0;
        }
        let (w, h) = self.controller.video_pixel_size();
        let current = parse_crop_ratio(&crop);
        CropPreset::ALL
            .iter()
            .position(|preset| {
                let rect = muvrix_player::subsystems::video::crop_rect_for_ratio(*preset, w, h);
                rect == crop || (preset.ratio().is_some() && current == preset.ratio())
            })
            .unwrap_or(0)
    }

    fn cycle_audio_track(&mut self) {
        let ids: Vec<i64> = self
            .controller
            .tracks_by_type(TrackType::Audio)
            .iter()
            .map(|t| t.id)
            .collect();
        if ids.is_empty() {
            self.push_osd("No audio tracks");
            return;
        }
        let current = self
            .controller
            .selected_track(TrackType::Audio)
            .map(|t| t.id);
        let mut options: Vec<Option<i64>> = vec![None];
        options.extend(ids.iter().copied().map(Some));
        let idx = options.iter().position(|id| *id == current).unwrap_or(0);
        let target = options[(idx + 1) % options.len()];
        let _ = self.controller.set_audio_track(target);
        self.push_osd(match target {
            Some(id) => format!("Audio track {id}"),
            None => "Audio track: off".to_string(),
        });
    }

    fn cycle_audio_device(&mut self) {
        if self.audio_devices.is_empty() {
            self.push_osd("No audio devices");
            return;
        }
        let idx = self
            .audio_devices
            .iter()
            .position(|d| d.name == self.selected_audio_device)
            .unwrap_or(0);
        let next = &self.audio_devices[(idx + 1) % self.audio_devices.len()];
        let name = next.name.clone();
        let display = next.description.clone();
        match self.controller.set_audio_device_with_fallback(&name) {
            Ok(active) => {
                self.selected_audio_device = active;
                self.push_osd(format!("Audio device: {display}"));
            }
            Err(e) => self.push_osd(format!("Audio device unavailable: {e}")),
        }
    }

    fn adjust_audio_delay(&mut self, delta: f64) {
        let current = self.controller.audio_delay().unwrap_or(0.0);
        let next = current + delta;
        let _ = self.controller.set_audio_delay(next);
        self.push_osd(format!("Audio delay: {next:+.2}s"));
    }

    fn adjust_subtitle_delay(&mut self, delta: f64) {
        let current = self.controller.sub_delay();
        let next = current + delta;
        let _ = self.controller.set_subtitle_delay(next);
        self.push_osd(format!("Subtitle delay: {next:+.2}s"));
    }

    fn cycle_subtitle_track(&mut self, reverse: bool) {
        let ids: Vec<i64> = self
            .controller
            .tracks_by_type(TrackType::Subtitle)
            .iter()
            .map(|t| t.id)
            .collect();
        if ids.is_empty() {
            self.push_osd("No subtitle tracks");
            return;
        }
        let current = self
            .controller
            .selected_track(TrackType::Subtitle)
            .map(|t| t.id);
        let mut options: Vec<Option<i64>> = vec![None];
        options.extend(ids.iter().copied().map(Some));
        let len = options.len();
        let idx = options.iter().position(|id| *id == current).unwrap_or(0);
        let next = if reverse {
            (idx + len - 1) % len
        } else {
            (idx + 1) % len
        };
        let target = options[next];
        let _ = self.controller.set_subtitle_track(target);
        self.push_osd(match target {
            Some(id) => format!("Subtitle track {id}"),
            None => "Subtitles off".to_string(),
        });
    }

    fn cycle_secondary_subtitle(&mut self) {
        let ids: Vec<i64> = self
            .controller
            .tracks_by_type(TrackType::Subtitle)
            .iter()
            .map(|t| t.id)
            .collect();
        if ids.is_empty() {
            self.push_osd("No subtitle tracks");
            return;
        }
        let current = self.controller.secondary_sub_track_id();
        let mut options: Vec<Option<i64>> = vec![None];
        options.extend(ids.iter().copied().map(Some));
        let idx = options.iter().position(|id| *id == current).unwrap_or(0);
        let target = options[(idx + 1) % options.len()];
        let _ = self.controller.set_secondary_sub_track(target);
        self.push_osd(match target {
            Some(id) => format!("Secondary subtitle {id}"),
            None => "Secondary subtitles off".to_string(),
        });
    }

    fn capture_screenshot(&mut self, mode: ScreenshotMode) {
        let dir = std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join("Pictures").join("Muvrix"))
            .unwrap_or_else(std::env::temp_dir);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.push_osd(format!("Screenshot directory unavailable: {e}"));
            return;
        }
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let stem = self
            .controller
            .current_file()
            .and_then(|f| std::path::Path::new(f).file_stem().map(|s| s.to_owned()))
            .and_then(|s| s.to_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "muvrix".to_string());
        let path = dir.join(format!("{stem}-{stamp}.png"));
        let path_str = path.to_string_lossy().to_string();
        match self.controller.screenshot_to_file(&path_str, mode) {
            Ok(()) => self.push_osd(format!("Screenshot saved: {path_str}")),
            Err(e) => self.push_osd(format!("Screenshot failed: {e}")),
        }
    }

    fn reload_current_media(&mut self) {
        let path = match self.controller.current_file() {
            Some(file) => file.to_string(),
            // After Stop the engine has no file but the title is still known.
            None if !self.path_input.trim().is_empty() => self.path_input.clone(),
            None => {
                self.push_osd("Nothing to reload");
                return;
            }
        };
        let pos = self.controller.time_pos();
        let audio = self
            .controller
            .selected_track(TrackType::Audio)
            .map(|t| t.id);
        let sub = self
            .controller
            .selected_track(TrackType::Subtitle)
            .map(|t| t.id);
        match self
            .controller
            .reload_with_resume(&path, pos, Some(audio), Some(sub))
        {
            Ok(()) => self.push_osd("Reloading media…"),
            Err(e) => self.push_osd(format!("Reload failed: {e}")),
        }
    }
}

/// Public normalisation used by the player chrome to highlight the active
/// aspect preset.
pub fn normalize_aspect_public(value: &str) -> String {
    normalize_aspect(value)
}

/// Parses libmpv's aspect notation (`16:9`, `2.35:1`, `1.777778`, `-1.000000`)
/// into a positive ratio. `None` means "no override", i.e. auto.
fn parse_aspect_ratio(value: &str) -> Option<f64> {
    let value = value.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("no") || value.eq_ignore_ascii_case("auto") {
        return None;
    }
    if let Some((num, den)) = value.split_once(':') {
        let num: f64 = num.trim().parse().ok()?;
        let den: f64 = den.trim().parse().ok()?;
        return (num > 0.0 && den > 0.0).then_some(num / den);
    }
    let ratio: f64 = value.parse().ok()?;
    (ratio > 0.0).then_some(ratio)
}

fn normalize_aspect(value: &str) -> String {
    match value.trim() {
        "" | "no" | "auto" => "auto".to_string(),
        other => other.to_string(),
    }
}

/// Human label for a raw `video-aspect-override` value.
///
/// The HUD builds its aspect label from the off-thread probe snapshot instead of
/// performing a blocking `mpv_get_property_string` on the paint thread every
/// frame.
pub fn aspect_label_from_raw(raw: &str) -> String {
    let current = normalize_aspect(raw);
    ASPECT_PRESETS
        .iter()
        .find(|(value, _)| *value == current)
        .map(|(_, label)| (*label).to_string())
        .unwrap_or_else(|| {
            // libmpv reports "no override" as a negative number; showing
            // that raw float in the deck looked like a broken widget.
            if current
                .parse::<f64>()
                .map(|value| value <= 0.0)
                .unwrap_or(false)
            {
                "Auto".to_string()
            } else {
                current
            }
        })
}

fn parse_crop_ratio(rect: &str) -> Option<(f64, f64)> {
    let dims = rect.split('+').next()?;
    let mut parts = dims.split('x');
    let w: f64 = parts.next()?.parse().ok()?;
    let h: f64 = parts.next()?.parse().ok()?;
    if h <= 0.0 {
        return None;
    }
    Some((w / h, 1.0))
}

/// Maps an A-B loop state to a user-facing label, shared by the toolbar and
/// the OSD.
pub fn ab_loop_label(state: AbLoopState) -> String {
    match state {
        AbLoopState::None => "A-B".to_string(),
        AbLoopState::PointA(_) => "A…".to_string(),
        AbLoopState::Loop { .. } => "A-B↺".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_has_a_category_and_label() {
        for action in PlayerAction::ALL {
            assert!(!action.display_name().is_empty());
            assert!(ShortcutCategory::ALL.contains(&action.category()));
        }
    }

    #[test]
    fn actions_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for action in PlayerAction::ALL {
            assert!(seen.insert(*action), "duplicate action: {action:?}");
        }
    }

    #[test]
    fn aspect_normalization_handles_engine_sentinels() {
        assert_eq!(normalize_aspect("no"), "auto");
        assert_eq!(normalize_aspect(""), "auto");
        assert_eq!(normalize_aspect("16:9"), "16:9");
        assert_eq!(ASPECT_PRESETS[0].0, "auto");
    }

    #[test]
    fn crop_ratio_parser_reads_rectangles() {
        assert_eq!(
            parse_crop_ratio("1920x822+0+129"),
            Some((1920.0 / 822.0, 1.0))
        );
        assert_eq!(parse_crop_ratio(""), None);
    }
}
