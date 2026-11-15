//! Focused tests for the pinned `mpv_render_frame_info` interpretation.
//!
//! The wrapper-level unit test lives in `render.rs`; this integration test
//! pins the public API interpretation and ABI-visible constants.

use libmpv_sys::mpv_render_param_type::MPV_RENDER_PARAM_NEXT_FRAME_INFO;
use libmpv_sys::{
    mpv_render_frame_info, mpv_render_param, MPV_RENDER_FRAME_INFO_BLOCK_VSYNC,
    MPV_RENDER_FRAME_INFO_PRESENT, MPV_RENDER_FRAME_INFO_REDRAW, MPV_RENDER_FRAME_INFO_REPEAT,
};
use muvrix_player::render::FrameInfo;

#[test]
fn pinned_frame_info_layout_is_c_abi() {
    assert_eq!(std::mem::size_of::<mpv_render_frame_info>(), 16);
    assert_eq!(std::mem::align_of::<mpv_render_frame_info>(), 8);
    assert_eq!(std::mem::offset_of!(mpv_render_frame_info, flags), 0);
    assert_eq!(std::mem::offset_of!(mpv_render_frame_info, target_time), 8);
    assert_eq!(std::mem::size_of::<mpv_render_param>(), 16);
    assert_eq!(MPV_RENDER_PARAM_NEXT_FRAME_INFO as i32, 11);
    assert_eq!(MPV_RENDER_FRAME_INFO_PRESENT, 1);
    assert_eq!(MPV_RENDER_FRAME_INFO_REDRAW, 2);
    assert_eq!(MPV_RENDER_FRAME_INFO_REPEAT, 4);
    assert_eq!(MPV_RENDER_FRAME_INFO_BLOCK_VSYNC, 8);
}

#[test]
fn frame_info_states_are_distinct() {
    let base = FrameInfo {
        valid: true,
        queried: true,
        api_result: 0,
        flags: MPV_RENDER_FRAME_INFO_PRESENT,
        target_time: 123,
    };
    assert_eq!(
        muvrix_player::render::usable_target_time_for_test(&base),
        Some(123)
    );

    let zero_target = FrameInfo {
        target_time: 0,
        ..base
    };
    assert_eq!(
        muvrix_player::render::usable_target_time_for_test(&zero_target),
        None,
        "PRESENT with target=0 is valid, but not a deadline"
    );

    let no_present = FrameInfo {
        valid: false,
        flags: 0,
        ..base
    };
    assert_eq!(
        muvrix_player::render::usable_target_time_for_test(&no_present),
        None
    );

    let api_error = FrameInfo {
        api_result: -1,
        ..base
    };
    assert_eq!(
        muvrix_player::render::usable_target_time_for_test(&api_error),
        None
    );
}
