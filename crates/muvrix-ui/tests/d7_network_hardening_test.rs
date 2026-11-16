use muvrix_player::{
    MpvHandle, PersistedMediaSettings, PlaybackController, RecoveryState, StreamTelemetry,
};
use std::sync::Arc;

fn create_headless_controller() -> PlaybackController {
    let handle = MpvHandle::new().expect("Create mpv handle");
    handle.set_property_string("vo", "null").expect("set vo");
    handle.set_property_string("ao", "null").expect("set ao");
    handle.initialize().expect("Initialize mpv handle");
    PlaybackController::new(Arc::new(handle)).expect("PlaybackController initialization")
}

#[test]
fn test_persisted_media_settings_stream_buffer_defaults() {
    let settings = PersistedMediaSettings::default();

    assert_eq!(settings.stream_buffer_mb, 150);
    assert_eq!(settings.stream_back_buffer_mb, 50);
    assert_eq!(settings.stream_readahead_secs, 10);
    assert_eq!(settings.stream_max_retries, 5);
}

#[test]
fn test_persisted_media_settings_stream_buffer_persistence_roundtrip() {
    let temp_dir = std::env::temp_dir();
    let file_path = temp_dir.join(format!(
        "muvrix_test_settings_{}.json",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    let settings = PersistedMediaSettings {
        stream_buffer_mb: 350,
        stream_back_buffer_mb: 80,
        stream_readahead_secs: 25,
        stream_max_retries: 8,
        ..Default::default()
    };

    settings.save(Some(&file_path)).expect("Save failed");
    let loaded = PersistedMediaSettings::load(Some(&file_path));

    assert_eq!(loaded.stream_buffer_mb, 350);
    assert_eq!(loaded.stream_back_buffer_mb, 80);
    assert_eq!(loaded.stream_readahead_secs, 25);
    assert_eq!(loaded.stream_max_retries, 8);

    let _ = std::fs::remove_file(file_path);
}

#[test]
fn test_controller_stream_buffering_configuration() {
    let mut controller = create_headless_controller();

    let res = controller.configure_stream_buffering(200 * 1024 * 1024, 60 * 1024 * 1024, 15.0);
    assert!(res.is_ok());

    // Verify cache duration and idle queries do not crash
    let duration = controller.demuxer_cache_duration();
    let cache_time = controller.demuxer_cache_time();
    let idle = controller.demuxer_cache_idle();

    assert!(duration >= 0.0);
    assert!(cache_time >= 0.0);
    assert!(idle || !idle);
}

#[test]
fn test_controller_stream_telemetry_reconnect_state() {
    let mut controller = create_headless_controller();

    assert!(!controller.is_stream_reconnecting());
    assert!(controller.stream_telemetry().is_none());

    let telemetry = StreamTelemetry::new();
    controller.set_stream_telemetry(Some(telemetry.clone()));

    assert!(controller.stream_telemetry().is_some());
    assert_eq!(telemetry.current_state(), RecoveryState::Connected);
    assert_eq!(telemetry.reconnect_attempts(), 0);
    assert_eq!(telemetry.bytes_read(), 0);

    // Simulate reconnect state
    *telemetry.state.write().unwrap() = RecoveryState::Reconnecting {
        attempt: 2,
        max_attempts: 5,
    };
    assert!(controller.is_stream_reconnecting());

    // Simulate recovery back to connected
    *telemetry.state.write().unwrap() = RecoveryState::Connected;
    assert!(!controller.is_stream_reconnecting());
}
