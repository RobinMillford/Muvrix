use std::sync::Arc;

use muvrix_player::{MpvHandle, PlaybackController, SubtitleJustify, SubtitleStyling};

#[test]
fn extended_subtitle_styling_is_applied_to_libmpv() {
    let handle = MpvHandle::new().expect("create handle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().expect("initialize");
    let mut controller = PlaybackController::new(Arc::new(handle)).expect("controller");

    let styling = SubtitleStyling {
        sub_speed: 1.25,
        sub_shadow_offset: 4.5,
        sub_back_color: "#10203040".to_string(),
        sub_justify: SubtitleJustify::Center,
        sub_fonts_dir: "/tmp/muvrix-fonts".to_string(),
        margin_y: 48,
        margin_x: 32,
        shadow_color: "#80123456".to_string(),
        ..SubtitleStyling::default()
    };
    controller
        .set_subtitle_styling(styling.clone())
        .expect("apply styling");
    assert_eq!(controller.subtitle_styling(), &styling);
    assert_eq!(
        controller
            .handle()
            .get_property::<f64>("sub-speed")
            .expect("sub-speed"),
        1.25
    );
    assert_eq!(
        controller
            .handle()
            .get_property::<f64>("sub-shadow-offset")
            .expect("sub-shadow-offset"),
        4.5
    );
    assert_eq!(
        controller
            .handle()
            .get_property_string("sub-justify")
            .expect("sub-justify"),
        "center"
    );
    assert_eq!(
        controller
            .handle()
            .get_property::<i64>("sub-margin-y")
            .expect("sub-margin-y"),
        48
    );
    assert_eq!(
        controller
            .handle()
            .get_property::<i64>("sub-margin-x")
            .expect("sub-margin-x"),
        32
    );
}

#[test]
fn test_embedded_lyrics_query_on_controller() {
    let handle = MpvHandle::new().expect("create handle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().expect("initialize");
    let controller = PlaybackController::new(Arc::new(handle)).expect("controller");

    // With no media loaded, embedded lyrics return None
    assert_eq!(controller.embedded_lyrics(), None);
}
