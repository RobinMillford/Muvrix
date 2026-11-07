//! Seeds a sanitized demo database for screenshot capture and manual testing.

use muvrix_core::database::{
    ArtworkRecord, ArtworkSource, AvailabilityState, DatabaseManager, LibraryRoot, MediaItem,
    MediaMetadata, MediaType, MetadataStatus,
};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db_path = Path::new("/tmp/muvrix_demo_library.db");
    if db_path.exists() {
        let _ = std::fs::remove_file(db_path);
    }

    let db = DatabaseManager::open(db_path)?;

    // 1. Root
    let root = LibraryRoot {
        id: 0,
        display_name: "Demo Videos".to_string(),
        root_path: "/media/DemoVideos".to_string(),
        source_type: "local".to_string(),
        is_recursive: true,
        is_enabled: true,
        exclusion_patterns: Vec::new(),
        last_scanned_at: 1700000000,
        scan_status: "idle".to_string(),
    };
    let root_id = db.insert_root(&root)?;

    // 2. Items
    let items = vec![
        (
            "Cosmos_Deep_Space_4K.mkv",
            "Cosmos: Deep Space Exploration (4K)",
            MediaType::Video,
            "hevc",
            Some("dts"),
            3840,
            2160,
            5400000,
            18500000,
            1024 * 1024 * 12500,
            true,
            "4K",
        ),
        (
            "Nature_Documentary_1080p.mp4",
            "Nature Documentary (1080p)",
            MediaType::Video,
            "h264",
            Some("aac"),
            1920,
            1080,
            7200000,
            4500000,
            1024 * 1024 * 3800,
            true,
            "Nature",
        ),
        (
            "Classical_Symphony.flac",
            "Symphony No. 9 in D minor",
            MediaType::Audio,
            "flac",
            None,
            0,
            0,
            3900000,
            960000,
            1024 * 1024 * 450,
            false,
            "Classical",
        ),
        (
            "Animation_Short_Film.webm",
            "Spring Morning Animation",
            MediaType::Video,
            "vp9",
            Some("opus"),
            1920,
            1080,
            900000,
            2200000,
            1024 * 1024 * 240,
            false,
            "Animation",
        ),
        (
            "Open_Source_Media_Showcase.mp4",
            "Open Source Media Showcase",
            MediaType::Video,
            "av1",
            Some("aac"),
            1920,
            1080,
            1800000,
            3100000,
            1024 * 1024 * 680,
            false,
            "Showcase",
        ),
    ];

    for (fname, title, mtype, vcodec, acodec, w, h, dur, br, sz, fav, tag) in items {
        let media_item = MediaItem {
            id: 0,
            root_id,
            relative_path: fname.to_string(),
            canonical_path: format!("/media/DemoVideos/{}", fname),
            file_name: fname.to_string(),
            file_extension: fname.split('.').next_back().unwrap_or("").to_string(),
            media_type: mtype,
            file_size: sz,
            modified_time: 1700000000,
            added_time: 1700000000,
            last_scanned_at: 1700000000,
            availability_state: AvailabilityState::Available,
            metadata_status: MetadataStatus::Extracted,
        };

        let media_id = db.upsert_media_item(&media_item)?;

        let meta = MediaMetadata {
            media_id,
            title: Some(title.to_string()),
            original_title: None,
            description: Some(
                "Sanitized demo media fixture for Muvrix library indexing validation.".to_string(),
            ),
            genre: Some(tag.to_string()),
            release_year: Some(2026),
            duration_ms: dur,
            overall_bitrate: br,
            container: Some(fname.split('.').next_back().unwrap_or("").to_string()),
            video_codec: if mtype == MediaType::Video {
                Some(vcodec.to_string())
            } else {
                None
            },
            audio_codec: acodec.map(|s| s.to_string()).or_else(|| {
                if mtype == MediaType::Audio {
                    Some(vcodec.to_string())
                } else {
                    None
                }
            }),
            width: w,
            height: h,
            fps: if mtype == MediaType::Video { 30.0 } else { 0.0 },
            hdr_type: None,
            audio_channels: 2,
            sample_rate: 48000,
            bit_depth: 16,
            artist: if mtype == MediaType::Audio {
                Some("Demo Orchestra".to_string())
            } else {
                None
            },
            album: if mtype == MediaType::Audio {
                Some("Masterpieces".to_string())
            } else {
                None
            },
            album_artist: None,
            track_number: None,
            disc_number: None,
        };

        db.upsert_metadata(&meta)?;
        if fav {
            db.set_favourite(media_id, true)?;
        }
        db.add_tag_to_media(media_id, tag)?;

        // Attach genuine extracted thumbnail fixture for video items if available
        if mtype == MediaType::Video {
            let fixture_candidates = [
                std::env::current_dir()
                    .unwrap_or_default()
                    .join("tests/fixtures/captured_movie_frame.png"),
                std::env::current_dir()
                    .unwrap_or_default()
                    .join("tests/fixtures/captured_test_frame.png"),
                std::path::PathBuf::from(
                    "/home/itsyamin/Codes/muvrix-desktop/tests/fixtures/captured_movie_frame.png",
                ),
                std::path::PathBuf::from(
                    "/home/itsyamin/Codes/muvrix-desktop/tests/fixtures/captured_test_frame.png",
                ),
            ];

            let chosen = if media_id % 2 == 1 {
                fixture_candidates
                    .iter()
                    .find(|p| p.exists() && p.to_string_lossy().contains("captured_test_frame"))
            } else {
                fixture_candidates
                    .iter()
                    .find(|p| p.exists() && p.to_string_lossy().contains("captured_movie_frame"))
            };

            if let Some(path) = chosen {
                let artwork = ArtworkRecord {
                    id: 0,
                    media_id,
                    cache_key: format!("demo_thumb_{}", media_id),
                    file_path: path.to_string_lossy().to_string(),
                    source_type: ArtworkSource::FrameCapture,
                    width: 320,
                    height: 180,
                    generated_at: 1700000000,
                };
                let _ = db.upsert_artwork(&artwork);
            }
        }
    }

    println!("Seeded /tmp/muvrix_demo_library.db successfully.");
    Ok(())
}
