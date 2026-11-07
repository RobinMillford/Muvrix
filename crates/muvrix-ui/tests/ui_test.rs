//! Unit and integration tests for UI helpers and formatting.

use muvrix_ui::app::format_timestamp;

#[test]
fn test_timestamp_formatting() {
    assert_eq!(format_timestamp(0.0), "00:00");
    assert_eq!(format_timestamp(5.2), "00:05");
    assert_eq!(format_timestamp(59.9), "00:59");
    assert_eq!(format_timestamp(60.0), "01:00");
    assert_eq!(format_timestamp(3599.0), "59:59");
    assert_eq!(format_timestamp(3600.0), "01:00:00");
    assert_eq!(format_timestamp(3665.0), "01:01:05");
    assert_eq!(format_timestamp(7325.0), "02:02:05");
    assert_eq!(format_timestamp(-1.0), "00:00");
    assert_eq!(format_timestamp(f64::NAN), "00:00");
}
