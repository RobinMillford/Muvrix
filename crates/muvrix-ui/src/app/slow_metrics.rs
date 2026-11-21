//! Off-thread engine probes for the paint path.
//!
//! `mpv_get_property` blocks until the engine core services the request. Under
//! 4K HEVC decode pressure the core can defer a reply by tens of milliseconds,
//! and every such read performed by the paint thread is time the compositor
//! does not get a buffer — so Mutter never presents, frame callbacks stall, and
//! the engine drops frames to keep audio sync. The measured cost of the
//! previous per-paint probes was ~21–28 ms per read at the 4K HEVC profile this
//! machine plays (see `docs/verification/`).
//!
//! The probes therefore run on a dedicated worker thread, exactly like
//! [`crate::app::diagnostics::DiagnosticsProbe`]. The paint path only ever
//! reads a snapshot from shared memory, so no engine round trip can delay a
//! frame. Values are at most one sampling interval stale, which is invisible
//! for a drop counter, an FPS estimate or a spectrum bar.
//!
//! Sampling pauses when there is nothing to sample: the worker sleeps at a low
//! rate until playback starts, and the visualizer probe only runs while the
//! visualizer is actually on screen.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use muvrix_player::MpvHandle;
use muvrix_player::SpectrumTelemetry;

/// Sampling cadence while playing.
const ACTIVE_INTERVAL: Duration = Duration::from_millis(100);
/// Cadence while not playing; also the latency to notice playback started.
const IDLE_INTERVAL: Duration = Duration::from_millis(250);
/// Sleep granularity so the worker notices a stop request promptly.
const SLEEP_SLICE: Duration = Duration::from_millis(25);

/// One immutable sample of the low-rate scalar probes plus the spectrum.
#[derive(Debug, Clone)]
pub(crate) struct ProbeSnapshot {
    pub(crate) spectrum: SpectrumTelemetry,
    pub(crate) demuxer_cache_time: f64,
    pub(crate) estimated_vf_fps: Option<f64>,
    pub(crate) frame_drops: i64,
    pub(crate) container_fps: Option<f64>,
    /// Decoded frame size. The HUD title needs it every frame, and
    /// `video-pixel-size` used to be two blocking reads on the paint thread.
    pub(crate) video_size: (i64, i64),
    /// Raw `video-aspect-override` (`"no"` when unset). The deck's label is
    /// derived from this; reading it per frame was a blocking engine call.
    pub(crate) aspect_override: String,
}

impl Default for ProbeSnapshot {
    fn default() -> Self {
        Self {
            spectrum: SpectrumTelemetry::default(),
            demuxer_cache_time: 0.0,
            estimated_vf_fps: None,
            frame_drops: 0,
            container_fps: None,
            video_size: (0, 0),
            aspect_override: "no".to_string(),
        }
    }
}

/// Background sampler for the probes the player view and visualizer need.
pub(crate) struct SlowMetrics {
    shared: Arc<Mutex<ProbeSnapshot>>,
    /// Set while audio is playing; gates the seven-read spectrum probe.
    spectrum_enabled: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    /// Bumped by the paint path when it wants a fresh sample immediately
    /// (e.g. the statistics overlay just opened).
    refresh_request: Arc<AtomicU64>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Default for SlowMetrics {
    fn default() -> Self {
        Self::new(None)
    }
}

impl SlowMetrics {
    /// Starts the sampler. Without a handle it stays idle and serves defaults,
    /// which keeps tests and headless construction working unchanged.
    pub(crate) fn new(handle: Option<Arc<MpvHandle>>) -> Self {
        let shared = Arc::new(Mutex::new(ProbeSnapshot::default()));
        let spectrum_enabled = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let refresh_request = Arc::new(AtomicU64::new(0));

        let worker = handle.and_then(|handle| {
            let shared = Arc::clone(&shared);
            let spectrum_enabled = Arc::clone(&spectrum_enabled);
            let stop = Arc::clone(&stop);
            let refresh_request = Arc::clone(&refresh_request);
            std::thread::Builder::new()
                .name("muvrix-probes".to_string())
                .spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        let playing = !handle.get_property::<bool>("pause").unwrap_or(true)
                            && !handle.get_property::<bool>("idle-active").unwrap_or(true);

                        let mut sample = ProbeSnapshot {
                            demuxer_cache_time: handle
                                .get_property::<f64>("demuxer-cache-time")
                                .unwrap_or(0.0),
                            estimated_vf_fps: handle.get_property::<f64>("estimated-vf-fps").ok(),
                            frame_drops: handle
                                .get_property::<i64>("frame-drop-count")
                                .unwrap_or(0),
                            container_fps: handle.get_property::<f64>("container-fps").ok(),
                            video_size: (
                                handle.get_property::<i64>("width").unwrap_or(0),
                                handle.get_property::<i64>("height").unwrap_or(0),
                            ),
                            aspect_override: handle
                                .get_property_string("video-aspect-override")
                                .unwrap_or_else(|_| "no".to_string()),
                            spectrum: SpectrumTelemetry::default(),
                        };

                        if spectrum_enabled.load(Ordering::Relaxed) {
                            let paused = handle.get_property::<bool>("pause").unwrap_or(false);
                            sample.spectrum =
                                muvrix_player::spectrum_telemetry_for(&handle, playing, paused);
                        }

                        if let Ok(mut lock) = shared.lock() {
                            *lock = sample;
                        }

                        // Consume one refresh request per completed sample.
                        let pending = refresh_request.load(Ordering::Relaxed);
                        if pending > 0 {
                            refresh_request.store(pending - 1, Ordering::Relaxed);
                        }

                        let interval = if playing {
                            ACTIVE_INTERVAL
                        } else {
                            IDLE_INTERVAL
                        };
                        let deadline = Instant::now() + interval;
                        while Instant::now() < deadline && !stop.load(Ordering::Relaxed) {
                            std::thread::sleep(SLEEP_SLICE);
                        }
                    }
                })
                .ok()
        });

        Self {
            shared,
            spectrum_enabled,
            stop,
            refresh_request,
            worker,
        }
    }

    /// Enables or pauses the spectrum probe. Only the visualizer needs it, so it
    /// is off unless the visualizer is on screen and enabled.
    pub(crate) fn set_spectrum_enabled(&self, on: bool) {
        self.spectrum_enabled.store(on, Ordering::Relaxed);
    }

    /// Requests an immediate sample on the next worker pass.
    pub(crate) fn request_refresh(&self) {
        self.refresh_request.fetch_add(1, Ordering::Relaxed);
    }

    /// Latest published sample. Cheap: it never touches the engine.
    pub(crate) fn snapshot(&self) -> ProbeSnapshot {
        self.shared.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

impl Drop for SlowMetrics {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_without_a_handle_serves_defaults() {
        let metrics = SlowMetrics::default();
        let sample = metrics.snapshot();
        assert_eq!(sample.frame_drops, 0);
        assert!(!sample.spectrum.is_playing);
    }

    #[test]
    fn spectrum_gate_starts_closed() {
        let metrics = SlowMetrics::default();
        assert!(!metrics.spectrum_enabled.load(Ordering::Relaxed));
        metrics.set_spectrum_enabled(true);
        assert!(metrics.spectrum_enabled.load(Ordering::Relaxed));
    }

    #[test]
    fn refresh_requests_are_counted_not_dropped() {
        let metrics = SlowMetrics::default();
        metrics.request_refresh();
        metrics.request_refresh();
        assert_eq!(metrics.refresh_request.load(Ordering::Relaxed), 2);
    }
}
