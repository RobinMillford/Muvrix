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
pub mod stream;
pub mod thumbnail;
pub mod track;
pub mod version;

pub use capability::{
    EngineCapabilities, HdrPresentationMode, HwdecMemoryPath, HwdecStatus, MediaCapabilities,
    PlaybackDiagnostics,
};

pub use command::{LoadFileMode, SeekMode};
pub use controller::{PlaybackController, PlaybackState};
pub use decode::{DecodeObservation, DecodePath, DecodePolicy, PlatformFamily};
pub use equalizer::{
    AudioDeviceInfo, AudioOutputDevice, EqBand, EqualizerManager, EqualizerPreset,
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
pub use settings::{PersistedMediaSettings, CURRENT_SETTINGS_VERSION};
pub use shader::{
    Anime4KPreset, GamutMappingMode, ShaderManager, ShaderProfile, ShaderQuality, TargetPrimaries,
    TargetTrc, ToneMappingAlgorithm, VideoAdjustmentSettings,
};
pub use stream::{
    HttpRangeStream, IoSeekableStream, MemoryStream, MpvStreamBridge, ReadAheadStream,
    RecoverableStream, RecoveryState, SeekableStream, SftpSeekableStream, SmbSeekableStream,
    StreamErrorKind, StreamRegistry, StreamRetryPolicy,
};
pub use thumbnail::MpvThumbnailExtractor;
pub use track::{AssOverrideMode, ChapterInfo, SubtitleStyling, TrackInfo, TrackType};
pub use version::ApiVersion;
