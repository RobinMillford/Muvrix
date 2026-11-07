//! Thumbnail caching, atomic disk persistence, and extraction providers.

pub mod cache;
pub mod extractor;

pub use cache::{ThumbnailCache, ThumbnailError, ThumbnailResult, MINIMAL_PNG_BYTES};
pub use extractor::{
    calculate_thumbnail_seek_seconds, FallbackThumbnailExtractor, ThumbnailExtractor,
};
