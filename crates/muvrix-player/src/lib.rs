//! `muvrix-player`: Safe, high-level Rust wrapper for the libmpv media engine.

pub mod capability;
pub mod command;
pub mod controller;
pub mod error;
pub mod event;
pub mod handle;
pub mod property;
pub mod render;
pub mod track;
pub mod version;

pub use capability::{
    EngineCapabilities, HdrPresentationMode, HwdecMemoryPath, HwdecStatus, PlaybackDiagnostics,
};

pub use command::{LoadFileMode, SeekMode};
pub use controller::{PlaybackController, PlaybackState};
pub use error::MpvError;
pub use event::{EndFileReason, Event};
pub use handle::MpvHandle;
pub use property::{GetProperty, PropertyFormat, SetProperty};
pub use render::MpvRenderContext;
pub use track::{ChapterInfo, TrackInfo, TrackType};
pub use version::ApiVersion;
