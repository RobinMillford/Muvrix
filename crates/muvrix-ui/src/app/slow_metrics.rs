//! Adaptive, cached mpv probes for the paint path.
//!
//! `mpv_get_property` blocks until the engine core services the request. Under
//! 4K HEVC decode pressure the core can defer a reply by tens to hundreds of
//! milliseconds, and anything this app reads per paint (spectrum probe, drop
//! counters, diagnostics) stalls the whole presentation loop with it. The
//! measured cost was ~150 ms per presented frame, which serialized decode,
//! render, composite, swap and the wp_presentation acknowledgement into a
//! ~5 FPS pipeline.
//!
//! This module keeps exactly one instance of each expensive probe, refreshed
//! on a bounded, adaptive interval:
//!
//! * a refresh that takes <= [`FAST_LIMIT`] resets the cadence to the base
//!   interval;
//! * a refresh slower than that doubles the interval (up to [`MAX_INTERVAL`]),
//!   so a sick engine costs 2 Hz of probes, not 10 Hz.
//!
//! All probes are safe to skip: the visualizer and diagnostics show slightly
//! stale values rather than block a frame.

use std::time::{Duration, Instant};

use muvrix_player::{PlaybackController, SpectrumTelemetry};

const BASE_INTERVAL: Duration = Duration::from_millis(100);
const MAX_INTERVAL: Duration = Duration::from_secs(2);
/// A refresh that completes within this budget means the core is healthy and
/// the base cadence can be restored immediately.
const FAST_LIMIT: Duration = Duration::from_millis(20);

/// Cached probes read at a low rate. Defaults keep the UI fully functional
/// before the first refresh (spectrum reports "not playing").
#[derive(Debug, Clone)]
pub(crate) struct SlowMetrics {
    pub(crate) spectrum: SpectrumTelemetry,
    spectrum_interval: Duration,
    spectrum_updated: Option<Instant>,

    pub(crate) demuxer_cache_time: f64,
    pub(crate) estimated_vf_fps: Option<f64>,
    pub(crate) frame_drops: i64,
    pub(crate) container_fps: Option<f64>,
    status_interval: Duration,
    status_updated: Option<Instant>,
}

impl Default for SlowMetrics {
    fn default() -> Self {
        Self {
            spectrum: SpectrumTelemetry::default(),
            spectrum_interval: BASE_INTERVAL,
            spectrum_updated: None,
            demuxer_cache_time: 0.0,
            estimated_vf_fps: None,
            frame_drops: 0,
            container_fps: None,
            status_interval: BASE_INTERVAL,
            status_updated: None,
        }
    }
}

impl SlowMetrics {
    /// Refreshes the spectrum probe when due. Only meaningful while playing
    /// with the visualizer enabled; the caller decides.
    pub(crate) fn refresh_spectrum(&mut self, controller: &PlaybackController, now: Instant) {
        if !due(self.spectrum_updated, self.spectrum_interval, now) {
            return;
        }
        let started = Instant::now();
        self.spectrum = controller.spectrum_telemetry();
        self.spectrum_interval = adapt(self.spectrum_interval, started.elapsed());
        self.spectrum_updated = Some(now);
    }

    /// Refreshes the low-rate scalar probes (~2 Hz worth of round trips).
    pub(crate) fn refresh_status(&mut self, controller: &PlaybackController, now: Instant) {
        if !due(self.status_updated, self.status_interval, now) {
            return;
        }
        let started = Instant::now();
        self.demuxer_cache_time = controller.demuxer_cache_time();
        self.estimated_vf_fps = controller.estimated_vf_fps();
        self.frame_drops = controller.frame_drop_count();
        self.container_fps = controller.container_fps();
        self.status_interval = adapt(self.status_interval, started.elapsed());
        self.status_updated = Some(now);
    }
}

fn due(updated: Option<Instant>, interval: Duration, now: Instant) -> bool {
    updated
        .map(|t| now.saturating_duration_since(t) >= interval)
        .unwrap_or(true)
}

fn adapt(current: Duration, took: Duration) -> Duration {
    if took <= FAST_LIMIT {
        BASE_INTERVAL
    } else {
        (current * 2).min(MAX_INTERVAL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_probes_double_the_interval_and_cap() {
        let mut i = BASE_INTERVAL;
        i = adapt(i, Duration::from_millis(50));
        assert_eq!(i, BASE_INTERVAL * 2);
        i = adapt(i, Duration::from_millis(50));
        assert_eq!(i, BASE_INTERVAL * 4);
        i = adapt(i, Duration::from_millis(50));
        i = adapt(i, Duration::from_millis(50));
        i = adapt(i, Duration::from_millis(50));
        i = adapt(i, Duration::from_millis(50));
        i = adapt(i, Duration::from_millis(50));
        assert_eq!(i, MAX_INTERVAL);
        // A fast refresh restores the base cadence immediately.
        i = adapt(i, Duration::from_millis(1));
        assert_eq!(i, BASE_INTERVAL);
    }

    #[test]
    fn never_refreshed_is_due() {
        let now = Instant::now();
        assert!(due(None, BASE_INTERVAL, now));
        assert!(!due(Some(now), BASE_INTERVAL, now));
        assert!(due(Some(now - BASE_INTERVAL), BASE_INTERVAL, now));
    }
}
