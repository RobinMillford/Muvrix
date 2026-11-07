//! Integration tests for metadata extraction providers in muvrix-player.

use muvrix_core::database::TrackType;
use muvrix_core::metadata::{FallbackMetadataExtractor, MediaMetadataExtractor};
use muvrix_core::thumbnails::ThumbnailExtractor;
use muvrix_player::metadata::MpvMetadataExtractor;
use std::path::PathBuf;

fn fixture_path(name: &str) -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests")
        .join("fixtures")
        .join(name)
}

#[test]
fn test_fallback_metadata_extractor() {
    let extractor = FallbackMetadataExtractor;
    let video_path = PathBuf::from("/fake/path/movie.mkv");
    let (meta, tracks) = extractor.extract_metadata(&video_path, 42).unwrap();

    assert_eq!(meta.media_id, 42);
    assert_eq!(meta.title.as_deref(), Some("movie"));
    assert_eq!(meta.container.as_deref(), Some("mkv"));
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].track_type, TrackType::Video);

    let audio_path = PathBuf::from("/fake/music/song.flac");
    let (audio_meta, audio_tracks) = extractor.extract_metadata(&audio_path, 43).unwrap();
    assert_eq!(audio_meta.title.as_deref(), Some("song"));
    assert_eq!(audio_tracks.len(), 1);
    assert_eq!(audio_tracks[0].track_type, TrackType::Audio);
}

#[test]
fn test_mpv_metadata_extractor_on_test_video_fixture() {
    let fixture = fixture_path("test_video.mp4");
    if !fixture.exists() {
        eprintln!("Fixture test_video.mp4 not found, skipping");
        return;
    }

    let extractor = MpvMetadataExtractor::new().expect("Failed to create MpvMetadataExtractor");
    let (meta, tracks) = extractor
        .extract_metadata(&fixture, 1)
        .expect("Failed to extract metadata");

    assert_eq!(meta.media_id, 1);
    assert_eq!(meta.width, 640);
    assert_eq!(meta.height, 360);
    assert_eq!(meta.video_codec.as_deref(), Some("h264"));
    assert_eq!(meta.audio_codec.as_deref(), Some("aac"));
    assert!(meta.duration_ms > 0);

    // Verify tracks: should have at least 1 video track and 1 audio track
    assert!(tracks.iter().any(|t| t.track_type == TrackType::Video));
    assert!(tracks.iter().any(|t| t.track_type == TrackType::Audio));
}

#[test]
fn test_mpv_metadata_extractor_on_corrupted_media() {
    let fixture = fixture_path("corrupted.mp4");
    if !fixture.exists() {
        return;
    }

    let extractor = MpvMetadataExtractor::new().expect("Create extractor");
    let res = extractor.extract_metadata(&fixture, 2);
    // Corrupted file must return an error gracefully without crashing
    assert!(res.is_err());
}

#[test]
fn test_mpv_thumbnail_extractor_on_test_video_fixture() {
    let fixture = fixture_path("test_video.mp4");
    if !fixture.exists() {
        return;
    }

    let temp = tempfile::TempDir::new().unwrap();
    let out = temp.path().join("thumb.png");
    let extractor = muvrix_player::thumbnail::MpvThumbnailExtractor::new();
    extractor
        .extract_thumbnail(&fixture, 3000, &out)
        .expect("Thumbnail extraction should succeed");

    assert!(out.is_file());
    assert!(out.metadata().unwrap().len() > 0);
}
