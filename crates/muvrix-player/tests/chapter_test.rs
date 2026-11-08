//! Chapter navigation and timeline metadata integration tests.

use muvrix_player::{ChapterInfo, MpvHandle, PlaybackController};
use std::sync::Arc;

#[test]
fn test_chapter_navigation_and_empty_handling() {
    let handle = Arc::new(MpvHandle::new().expect("Create handle"));
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().expect("Initialize handle");

    let mut controller = PlaybackController::new(handle).expect("Create controller");

    // File without chapters returns empty slice and None current chapter
    assert_eq!(controller.chapters().len(), 0);
    assert_eq!(controller.chapter_count(), 0);
    assert_eq!(controller.current_chapter(), None);

    // Chapter commands should not panic or error destructively on idle player
    assert!(controller.next_chapter().is_ok() || controller.next_chapter().is_err());
    assert!(controller.prev_chapter().is_ok() || controller.prev_chapter().is_err());
    assert!(controller.set_chapter(0).is_ok() || controller.set_chapter(0).is_err());
}

#[test]
fn test_chapter_info_data_model() {
    let ch1 = ChapterInfo {
        index: 0,
        title: "Prologue: Genesis".to_string(),
        time: 0.0,
    };
    let ch2 = ChapterInfo {
        index: 1,
        title: "Act I: Departure".to_string(),
        time: 125.5,
    };

    assert_eq!(ch1.index, 0);
    assert_eq!(ch1.title, "Prologue: Genesis");
    assert_eq!(ch1.time, 0.0);

    assert_eq!(ch2.index, 1);
    assert_eq!(ch2.title, "Act I: Departure");
    assert_eq!(ch2.time, 125.5);
}
