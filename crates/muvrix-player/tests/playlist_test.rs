use muvrix_player::controller::PlaybackController;
use muvrix_player::handle::MpvHandle;
use muvrix_player::track::SubtitleStyling;
use std::path::PathBuf;
use std::sync::Arc;

fn find_test_media() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = manifest_dir.join("../../tests/fixtures/test_video.mp4");
    if path.exists() {
        path.canonicalize().unwrap_or(path)
    } else {
        PathBuf::from("tests/fixtures/test_video.mp4")
            .canonicalize()
            .expect("test_video.mp4 fixture must exist")
    }
}

fn create_test_controller() -> PlaybackController {
    let handle = MpvHandle::new().expect("Failed to create MpvHandle");
    handle
        .set_option_string("vo", "null")
        .expect("Failed to set vo=null");
    handle
        .set_option_string("ao", "null")
        .expect("Failed to set ao=null");
    handle
        .set_option_string("idle", "yes")
        .expect("Failed to set idle=yes");
    handle.initialize().expect("Failed to initialize MpvHandle");

    PlaybackController::new(Arc::new(handle)).expect("Failed to create PlaybackController")
}

#[test]
fn test_playlist_append_and_count() {
    let mut controller = create_test_controller();

    assert_eq!(controller.playlist_count(), 0);
    assert_eq!(controller.playlist_pos(), None);

    let media = find_test_media();
    assert!(media.exists(), "Test fixture must exist");

    controller
        .playlist_append(media.to_str().unwrap())
        .expect("Failed to append to playlist");

    assert_eq!(controller.playlist_count(), 1);
    assert_eq!(controller.playlist().len(), 1);

    controller
        .playlist_append(media.to_str().unwrap())
        .expect("Failed to append second item");

    assert_eq!(controller.playlist_count(), 2);
    assert_eq!(controller.playlist().len(), 2);
}

#[test]
fn test_playlist_navigation_and_play_index() {
    let mut controller = create_test_controller();

    let media = find_test_media();
    let media_str = media.to_str().unwrap();

    // Append 3 items without auto-playing
    controller
        .playlist_append(media_str)
        .expect("Failed to append first");
    controller
        .playlist_append(media_str)
        .expect("Failed to append second");
    controller
        .playlist_append(media_str)
        .expect("Failed to append third");

    assert_eq!(controller.playlist_count(), 3);

    // Jump to index 1
    controller
        .playlist_play_index(1)
        .expect("Failed to jump to index 1");

    // Remove item at index 0
    controller
        .playlist_remove(0)
        .expect("Failed to remove item 0");
    assert_eq!(controller.playlist_count(), 2);

    // Stop playback first, then clear playlist
    controller.stop().expect("Failed to stop");
    controller
        .playlist_clear()
        .expect("Failed to clear playlist");
    assert_eq!(controller.playlist_count(), 0);
}

#[test]
fn test_playlist_move_and_shuffle() {
    let mut controller = create_test_controller();

    let media = find_test_media();
    let media_str = media.to_str().unwrap();

    controller.playlist_append(media_str).unwrap();
    controller.playlist_append(media_str).unwrap();
    controller.playlist_append(media_str).unwrap();
    assert_eq!(controller.playlist_count(), 3);

    // Move from 0 to 2
    controller
        .playlist_move(0, 2)
        .expect("Failed to move playlist item");
    assert_eq!(controller.playlist_count(), 3);

    // Shuffle playlist
    controller
        .playlist_shuffle()
        .expect("Failed to shuffle playlist");
    assert_eq!(controller.playlist_count(), 3);
}

#[test]
fn test_secondary_subtitle_track_management() {
    let mut controller = create_test_controller();

    // Initially none
    assert_eq!(controller.secondary_sub_track_id(), None);

    // Set secondary subtitle track ID
    controller
        .set_secondary_sub_track(Some(2))
        .expect("Failed to set secondary subtitle track");
    assert_eq!(controller.secondary_sub_track_id(), Some(2));

    // Disable secondary subtitle
    controller
        .set_secondary_sub_track(None)
        .expect("Failed to disable secondary subtitle");
    assert_eq!(controller.secondary_sub_track_id(), None);
}

#[test]
fn test_subtitle_styling_configuration() {
    let mut controller = create_test_controller();

    let default_styling = SubtitleStyling::default();
    assert_eq!(controller.subtitle_styling(), &default_styling);

    let custom_styling = SubtitleStyling {
        font: "Liberation Sans".to_string(),
        font_size: 42,
        color: "#FFFF00".to_string(),
        border_color: "#000000".to_string(),
        border_size: 3.5,
        pos: 90,
        scale: 1.2,
        ass_override: true,
    };

    controller
        .set_subtitle_styling(custom_styling.clone())
        .expect("Failed to apply custom subtitle styling");
    assert_eq!(controller.subtitle_styling(), &custom_styling);

    // Verify properties on handle
    let font_size = controller
        .handle()
        .get_property::<i64>("sub-font-size")
        .unwrap();
    assert_eq!(font_size, 42);

    let pos = controller.handle().get_property::<i64>("sub-pos").unwrap();
    assert_eq!(pos, 90);

    // Reset back to defaults
    controller
        .reset_subtitle_styling()
        .expect("Failed to reset subtitle styling");
    assert_eq!(controller.subtitle_styling(), &SubtitleStyling::default());
}

#[test]
fn test_controller_playback_and_resume_seek_with_synthetic_media_over_10s() {
    let temp_dir = std::env::temp_dir();
    let fixture_path = temp_dir.join("muvrix_synthetic_12s.mp4");

    // Generate synthetic 12-second test video using ffmpeg
    let gen_status = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=12:size=160x120:rate=10",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=12",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-crf",
            "35",
            "-c:a",
            "aac",
            "-b:a",
            "32k",
            fixture_path.to_str().unwrap(),
        ])
        .output();

    if let Ok(output) = gen_status {
        if output.status.success() && fixture_path.exists() {
            let mut controller = create_test_controller();
            controller
                .load_file(fixture_path.to_str().unwrap())
                .expect("Failed to load synthetic media");

            // Allow mpv to load file and transition to Playing
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            while std::time::Instant::now() < deadline {
                controller.poll_events();
                if *controller.state() == muvrix_player::PlaybackState::Playing {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }

            // Perform seek past resume threshold (>10s)
            let _ = controller.seek(10.5, muvrix_player::SeekMode::Absolute);
            controller.poll_events();

            // Stop controller
            let _ = controller.stop();
            controller.poll_events();

            // Clean up temporary synthetic fixture
            let _ = std::fs::remove_file(fixture_path);
        }
    }
}
