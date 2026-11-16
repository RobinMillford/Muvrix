//! `muvrix-player`: Safe, high-level Rust wrapper for the libmpv media engine.

pub mod capability;
pub mod command;
pub mod controller;
pub mod decode;
pub mod equalizer;
pub mod error;
pub mod event;
pub mod handle;
pub mod hdr;
pub mod metadata;
pub mod playlist;
pub mod property;
pub mod render;
pub mod settings;
pub mod shader;
pub mod shuffle;
pub mod stream;
pub mod subsystems;
pub mod thumbnail;
pub mod track;
pub mod version;

pub use capability::{
    EngineCapabilities, HdrPresentationMode, HwdecMemoryPath, HwdecStatus, MediaCapabilities,
    PlaybackDiagnostics,
};

pub use command::{LoadFileMode, SeekMode};
pub use controller::{
    parse_mpvex_command, AbLoopState, DynamicsTelemetry, MpvexCommand, PlaybackController,
    PlaybackState, RepeatMode, SteppingPreset,
};
pub use decode::{DecodeObservation, DecodePath, DecodePolicy, PlatformFamily};
pub use equalizer::{
    AudioDeviceInfo, AudioOutputDevice, Bs2bPreset, EqBand, EqualizerManager, EqualizerPreset,
    EqualizerSettings, EQ_FREQUENCIES,
};
pub use error::MpvError;
pub use event::{EndFileReason, Event};
pub use handle::MpvHandle;
pub use hdr::{
    classify_source, plan_presentation, CapabilityDetection, ColorPlan, DisplayCapabilities,
    HdrAvailability, HdrSourceType, PresentationMode, PresentationState,
};
pub use metadata::MpvMetadataExtractor;
pub use playlist::PlaylistItem;
pub use property::{GetProperty, PropertyFormat, SetProperty};
pub use render::{MpvRenderContext, NativeDisplay};
pub use settings::{
    AccentColor, AudioChannelsSetting, PersistedMediaSettings, ThemeMode, CURRENT_SETTINGS_VERSION,
};
pub use shader::{
    evaluate_tone_mapping_curve, Anime4KPreset, CustomShaderEntry, DebandPreset, GamutMappingMode,
    ShaderManager, ShaderProfile, ShaderQuality, TargetPrimaries, TargetTrc, ToneMappingAlgorithm,
    VideoAdjustmentSettings,
};
pub use shuffle::{ShuffleTrackInfo, SmartShuffleEngine};
pub use stream::{
    classify_io_error, HttpRangeStream, IoSeekableStream, MemoryStream, MpvStreamBridge,
    ReadAheadStream, RecoverableStream, RecoveryState, SeekableStream, SftpSeekableStream,
    SmbSeekableStream, StreamErrorKind, StreamRegistry, StreamRetryPolicy, StreamTelemetry,
};
pub use subsystems::{
    AudioDeviceChangeEvent, AudioRecoveryStatus, AudioSubsystem, DecoderFallbackReason,
    DecoderWatchdog, TrackSubsystem, VideoSubsystem, WatchdogStatus,
};
pub use thumbnail::MpvThumbnailExtractor;
pub use track::{
    AssOverrideMode, ChapterInfo, SubtitleJustify, SubtitleStyling, TrackInfo, TrackType,
};
pub use version::ApiVersion;
