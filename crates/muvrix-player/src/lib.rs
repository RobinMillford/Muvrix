//! `muvrix-player`: Safe, high-level Rust wrapper for the libmpv media engine.

pub mod capability;
pub mod command;
pub mod controller;
pub mod equalizer;
pub mod error;
pub mod event;
pub mod handle;
pub mod metadata;
pub mod playlist;
pub mod property;
pub mod render;
pub mod shader;
pub mod stream;
pub mod thumbnail;
pub mod track;
pub mod version;

pub use capability::{
    EngineCapabilities, HdrPresentationMode, HwdecMemoryPath, HwdecStatus, PlaybackDiagnostics,
};

pub use command::{LoadFileMode, SeekMode};
pub use controller::{PlaybackController, PlaybackState};
pub use equalizer::{
    AudioDeviceInfo, EqualizerManager, EqualizerPreset, EqualizerSettings, EQ_FREQUENCIES,
};
pub use error::MpvError;
pub use event::{EndFileReason, Event};
pub use handle::MpvHandle;
pub use metadata::MpvMetadataExtractor;
pub use playlist::PlaylistItem;
pub use property::{GetProperty, PropertyFormat, SetProperty};
pub use render::MpvRenderContext;
pub use shader::{Anime4KPreset, ShaderManager, VideoAdjustmentSettings};
pub use stream::{
    HttpRangeStream, IoSeekableStream, MemoryStream, MpvStreamBridge, ReadAheadStream,
    SeekableStream, SftpSeekableStream, SmbSeekableStream, StreamRegistry,
};
pub use thumbnail::MpvThumbnailExtractor;
pub use track::{AssOverrideMode, ChapterInfo, SubtitleStyling, TrackInfo, TrackType};
pub use version::ApiVersion;
