//! Architectural subsystems decomposing PlaybackController into specialized domains.

pub mod audio;
pub mod track;
pub mod video;

pub use audio::{
    spectrum_telemetry_for, AudioDeviceChangeEvent, AudioRecoveryStatus, AudioSubsystem,
    DynamicsTelemetry,
};
pub use track::TrackSubsystem;
pub use video::{
    crop_rect_for_ratio, CropPreset, DecoderFallbackReason, DecoderWatchdog, ScalingMode,
    ScreenshotMode, SteppingPreset, VideoSubsystem, WatchdogStatus,
};
