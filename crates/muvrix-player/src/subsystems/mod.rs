//! Architectural subsystems decomposing PlaybackController into specialized domains.

pub mod audio;
pub mod track;
pub mod video;

pub use audio::{AudioDeviceChangeEvent, AudioRecoveryStatus, AudioSubsystem, DynamicsTelemetry};
pub use track::TrackSubsystem;
pub use video::{
    DecoderFallbackReason, DecoderWatchdog, SteppingPreset, VideoSubsystem, WatchdogStatus,
};
