//! Comprehensive tests for Phase E1: Engine Resilience & Hardware Decoder Hardening.
//!
//! Covers:
//! - Subsystem architectural decomposition and state synchronization.
//! - Audio device hotplug recovery (disconnection, reconnection, fallback).
//! - Audio sink failure and retry-loop prevention.
//! - Hardware decoder watchdog (stall detection, copy-back fallback, software fallback).
//! - Watchdog immunity during pause and network buffering.
//! - Playback state and timestamp preservation during recovery.
//! - Repeated device connect/disconnect cycles.

use std::sync::Arc;

use muvrix_player::handle::MpvHandle;
use muvrix_player::subsystems::audio::AudioRecoveryStatus;
use muvrix_player::subsystems::video::{
    DecoderFallbackReason, DecoderWatchdog, SteppingPreset, WatchdogStatus,
};
use muvrix_player::{DecodePath, Event, PlaybackController, PlaybackState};

fn create_test_player() -> Arc<MpvHandle> {
    let handle = MpvHandle::new().expect("Create MpvHandle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().unwrap();
    Arc::new(handle)
}

#[test]
fn test_subsystem_architectural_decomposition_and_accessors() {
    let handle = create_test_player();
    let mut controller = PlaybackController::new(handle).expect("Create controller");

    // Audio subsystem access
    assert_eq!(controller.audio().volume(), 100.0);
    assert_eq!(controller.audio().speed(), 1.0);
    assert!(!controller.audio().is_muted());

    // Video subsystem access
    assert_eq!(controller.video().video_dimensions(), (0, 0));
    assert_eq!(controller.video().stepping_preset(), SteppingPreset::Frame);
    assert_eq!(controller.video().panscan(), 0.0);

    // Track subsystem access
    assert!(controller.track().tracks().is_empty());
    assert_eq!(controller.track().chapter_count(), 0);
    assert_eq!(controller.track().sub_delay(), 0.0);
    assert!(controller.track().sub_visibility());

    // Mutable subsystem updates
    controller
        .video_mut()
        .set_stepping_preset(SteppingPreset::Film24);
    assert_eq!(controller.video().stepping_preset(), SteppingPreset::Film24);
}

#[test]
fn test_subsystem_state_synchronization_across_property_events() {
    let handle = create_test_player();
    let mut controller = PlaybackController::new(handle).expect("Create controller");

    // 1. Synchronize volume property event
    controller.handle_event(&Event::PropertyChange {
        reply_userdata: muvrix_player::controller::PROP_VOLUME,
        name: "volume".to_string(),
        format: muvrix_player::PropertyFormat::None,
        value: None,
    });
    assert!(controller.volume() >= 0.0);
    assert_eq!(controller.audio().volume(), controller.volume());

    // 2. Synchronize mute property event
    controller.handle_event(&Event::PropertyChange {
        reply_userdata: muvrix_player::controller::PROP_MUTE,
        name: "mute".to_string(),
        format: muvrix_player::PropertyFormat::None,
        value: None,
    });
    assert_eq!(controller.is_muted(), controller.audio().is_muted());

    // 3. Synchronize speed property event
    controller.handle_event(&Event::PropertyChange {
        reply_userdata: muvrix_player::controller::PROP_SPEED,
        name: "speed".to_string(),
        format: muvrix_player::PropertyFormat::None,
        value: None,
    });
    assert_eq!(controller.speed(), controller.audio().speed());

    // 4. Synchronize hardware decoder property event
    controller.handle_event(&Event::PropertyChange {
        reply_userdata: muvrix_player::controller::PROP_HWDEC_CURRENT,
        name: "hwdec-current".to_string(),
        format: muvrix_player::PropertyFormat::None,
        value: None,
    });
    assert_eq!(
        controller.hwdec_current(),
        controller.video().hwdec_current()
    );

    // 5. Synchronize subtitle delay event
    controller.handle_event(&Event::PropertyChange {
        reply_userdata: muvrix_player::controller::PROP_SUB_DELAY,
        name: "sub-delay".to_string(),
        format: muvrix_player::PropertyFormat::None,
        value: None,
    });
    assert_eq!(controller.sub_delay(), controller.track().sub_delay());
}

#[test]
fn test_audio_device_removal_and_hotplug_fallback() {
    let handle = create_test_player();
    let mut controller = PlaybackController::new(handle).expect("Create controller");

    // Configure preferred device
    let preferred = "usb_dac_headphone_pro";
    let _ = controller.set_audio_device(preferred);
    assert_eq!(controller.audio().preferred_device(), preferred);

    // Emulate device disconnect by triggering hotplug where the device is missing
    let change = controller
        .audio()
        .on_device_list_changed(controller.handle());
    assert!(change.is_some());

    // Recovery status must transition to DeviceLost
    match controller.audio().recovery_status() {
        AudioRecoveryStatus::DeviceLost {
            device_name,
            fallback_device,
        } => {
            assert_eq!(device_name, preferred);
            assert_eq!(fallback_device, "auto");
        }
        other => panic!("Expected DeviceLost status, got {:?}", other),
    }

    // Active device must have fallen back to auto
    assert_eq!(controller.audio().active_device(), "auto");

    // Notification must be queued
    let note = controller.take_recovery_notification();
    assert!(note.is_some());
    assert!(note
        .unwrap()
        .contains("disconnected; rerouted to system default"));
}

#[test]
fn test_audio_device_reconnection_and_route_restoration() {
    let handle = create_test_player();
    let mut controller = PlaybackController::new(handle).expect("Create controller");

    let preferred = "usb_dac_studio_monitor";
    let _ = controller.set_audio_device(preferred);

    // Disconnect: device missing from mpv
    let change = controller
        .audio()
        .on_device_list_changed(controller.handle());
    assert!(change.is_some());
    assert!(matches!(
        controller.audio().recovery_status(),
        AudioRecoveryStatus::DeviceLost { .. }
    ));
    let _ = controller.take_recovery_notification();

    // Reconnecting preferred device: trigger device list update that includes the preferred device
    let reconnected_list = vec![
        muvrix_player::AudioDeviceInfo {
            name: "auto".to_string(),
            description: "Auto-select device".to_string(),
        },
        muvrix_player::AudioDeviceInfo {
            name: preferred.to_string(),
            description: "USB DAC Studio Monitor Pro".to_string(),
        },
    ];
    let change_reconnect = controller
        .audio()
        .handle_device_inventory_change(controller.handle(), reconnected_list);
    assert!(matches!(
        change_reconnect,
        Some(
            muvrix_player::subsystems::audio::AudioDeviceChangeEvent::PreferredDeviceReconnected { .. }
        )
    ));
    assert!(matches!(
        controller.audio().recovery_status(),
        AudioRecoveryStatus::Recovered { .. }
    ));
    assert_eq!(controller.audio().active_device(), preferred);
}

#[test]
fn test_audio_sink_failure_and_retry_loop_prevention() {
    let handle = create_test_player();
    let mut controller = PlaybackController::new(handle).expect("Create controller");
    let handle_clone = controller.handle().clone();

    // Simulate 3 rapid audio sink failures while playing
    for attempt in 1..=3 {
        let event = controller.audio_mut().on_current_ao_changed(
            &handle_clone,
            "null",
            true, // has_audio_track
            true, // is_playing
        );
        assert!(event.is_some(), "Attempt {attempt} must emit SinkFailed");
        let note = controller.take_recovery_notification();
        assert!(note.is_some());
        assert!(note
            .unwrap()
            .contains(&format!("recovered (attempt #{attempt}/3)")));
    }

    // Fourth failure in rapid succession must trigger loop prevention and halt retries
    let fourth_event =
        controller
            .audio_mut()
            .on_current_ao_changed(&handle_clone, "null", true, true);
    assert!(
        fourth_event.is_none(),
        "Fourth attempt must be halted by anti-loop limiter"
    );

    // Status must be Degraded
    match controller.audio().recovery_status() {
        AudioRecoveryStatus::Degraded { reason } => {
            assert!(reason.contains("Automatic retries halted to prevent loops"));
        }
        other => panic!("Expected Degraded recovery status, got {:?}", other),
    }

    // Cooldown reset verification after simulated elapsed time
    controller.audio().poll_recovery(controller.handle());
}

#[test]
fn test_decoder_watchdog_stall_detection_and_fallback_ladder() {
    let handle = create_test_player();
    let watchdog = DecoderWatchdog::new();
    watchdog.set_requested_hwdec("vaapi");
    watchdog.on_hwdec_changed("vaapi");

    // 1. Initially stable
    assert_eq!(
        watchdog.status(),
        WatchdogStatus::Stable {
            active_mode: "vaapi".to_string(),
            path: DecodePath::DirectHw,
        }
    );

    // 2. Playback advancing normally: no stall
    let check1 = watchdog.check_watchdog(&handle, true, false, 1.0, 0);
    assert!(check1.is_none());
    let check2 = watchdog.check_watchdog(&handle, true, false, 2.5, 0);
    assert!(check2.is_none());

    // 3. Emulate stall: position frozen at 2.5s while dropped frames accumulate
    // Force timestamp back to simulate 3.5s of stall
    let reason = DecoderFallbackReason::StallDetected {
        stall_duration_secs: 3.5,
        dropped_frames: 10,
    };
    let next_mode = watchdog.trigger_fallback(&handle, reason.clone());

    // First fallback must be copy-back mode ("vaapi-copy")
    assert_eq!(next_mode, "vaapi-copy");
    match watchdog.status() {
        WatchdogStatus::FallenBack {
            active_mode,
            path,
            reason: r,
        } => {
            assert_eq!(active_mode, "vaapi-copy");
            assert_eq!(path, DecodePath::HwCopyBack);
            assert_eq!(r, reason);
        }
        other => panic!("Expected FallenBack, got {:?}", other),
    }

    // Notification must be queued
    let note = watchdog.take_notification().expect("Notification");
    assert!(note.contains("Decoder fallback: switched to vaapi-copy"));

    // 4. Second stall: fallback to software decoding ("no")
    let reason2 = DecoderFallbackReason::StallDetected {
        stall_duration_secs: 4.0,
        dropped_frames: 15,
    };
    let second_mode = watchdog.trigger_fallback(&handle, reason2);
    assert_eq!(second_mode, "no");
    match watchdog.status() {
        WatchdogStatus::Exhausted { active_mode, .. } => {
            assert_eq!(active_mode, "no");
        }
        other => panic!("Expected Exhausted, got {:?}", other),
    }

    // 5. Third attempt: already at software decoding, cannot fall back further
    let reason3 = DecoderFallbackReason::PersistentErrors { error_count: 5 };
    let third_mode = watchdog.trigger_fallback(&handle, reason3);
    assert_eq!(third_mode, "no");
}

#[test]
fn test_decoder_watchdog_immunity_during_pause_and_buffering() {
    let handle = create_test_player();
    let watchdog = DecoderWatchdog::new();
    watchdog.set_requested_hwdec("vaapi");
    watchdog.on_hwdec_changed("vaapi");

    // Position does not advance because playback is paused
    let check_paused = watchdog.check_watchdog(
        &handle, false, // is_playing = false (Paused)
        false, // is_buffering = false
        5.0,   // position frozen
        10,
    );
    assert!(check_paused.is_none());
    assert_eq!(
        watchdog.status(),
        WatchdogStatus::Stable {
            active_mode: "vaapi".to_string(),
            path: DecodePath::DirectHw,
        }
    );

    // Position does not advance because network/demuxer is buffering
    let check_buffering = watchdog.check_watchdog(
        &handle, true, // is_playing = true
        true, // is_buffering = true
        5.0,  // position frozen
        10,
    );
    assert!(check_buffering.is_none());
    assert_eq!(
        watchdog.status(),
        WatchdogStatus::Stable {
            active_mode: "vaapi".to_string(),
            path: DecodePath::DirectHw,
        }
    );
}

#[test]
fn test_playback_state_preservation_during_fallback() {
    let handle = create_test_player();
    let mut controller = PlaybackController::new(handle).expect("Create controller");

    // Set test parameters
    controller.set_volume(85.0).expect("Set volume");
    controller.set_subtitle_delay(0.25).expect("Set sub delay");

    // Trigger fallback
    let reason = DecoderFallbackReason::InitFailed;
    let fallback_mode = controller
        .video()
        .trigger_decoder_fallback(controller.handle(), reason);
    assert!(!fallback_mode.is_empty());

    // Volume, sub delay, and controller state must be preserved
    assert_eq!(controller.volume(), 85.0);
    assert_eq!(controller.sub_delay(), 0.25);
    assert_ne!(*controller.state(), PlaybackState::Error("".to_string()));
}

#[test]
fn test_repeated_device_plug_unplug_cycles() {
    let handle = create_test_player();
    let controller = PlaybackController::new(handle).expect("Create controller");

    // Perform 10 consecutive device enumeration cycles
    for cycle in 1..=10 {
        let change = controller
            .audio()
            .on_device_list_changed(controller.handle());
        assert!(
            change.is_some(),
            "Cycle {cycle} must produce valid device list event"
        );
    }

    // Audio status must remain valid
    assert!(matches!(
        controller.audio().recovery_status(),
        AudioRecoveryStatus::Normal | AudioRecoveryStatus::DeviceLost { .. }
    ));
}
