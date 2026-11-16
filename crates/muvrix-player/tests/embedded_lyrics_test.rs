use muvrix_player::{MpvHandle, MpvMetadataExtractor, PlaybackController};
use std::sync::Arc;
use tempfile::tempdir;

#[test]
fn test_embedded_lyrics_extractor_graceful_missing_and_corrupt() {
    let extractor = MpvMetadataExtractor::new().expect("create metadata extractor");
    let dir = tempdir().unwrap();

    // 1. Nonexistent file
    let missing_path = dir.path().join("nonexistent.mp3");
    let missing_res = extractor.extract_embedded_lyrics(&missing_path);
    assert!(missing_res.is_err());

    // 2. Deliberately corrupted media file
    let corrupt_path = dir.path().join("corrupt.mp3");
    std::fs::write(&corrupt_path, b"NOT_A_VALID_MEDIA_FILE_BINARY_HEADER").unwrap();
    let corrupt_res = extractor.extract_embedded_lyrics(&corrupt_path);
    // Either returns Ok(None) or error, but never panics
    assert!(corrupt_res.is_ok() || corrupt_res.is_err());
}

#[test]
fn test_controller_embedded_lyrics_lifecycle() {
    let handle = MpvHandle::new().expect("create handle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().expect("initialize");

    let controller = PlaybackController::new(Arc::new(handle)).expect("controller");
    assert_eq!(controller.embedded_lyrics(), None);
}
