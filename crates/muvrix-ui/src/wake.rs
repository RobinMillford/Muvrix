//! WP5 — platform-neutral wake interface.
//!
//! `RenderEngine` must not name a Linux-only concrete wake type. It needs exactly
//! one capability: "a frame is expected by this instant, please keep the event
//! loop from parking." That capability is this trait; the Linux implementation
//! (backed by the runner's `WakeSignal`) lives in `runner.rs`, where the existing
//! `#[cfg(target_os = "linux")]` boundary already is.
//!
//! There is deliberately no `#[cfg]` in this module and none in the renderer.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

/// Fallback interval the renderer uses when the engine has no frame to present.
///
/// This exists because both wake paths can go quiet at the same time: mpv raises
/// no update callback (no frame ready) and the paced repaint is gated behind a
/// render that did not happen. Without a bounded poll the event loop arms
/// `ControlFlow::Wait` and parks indefinitely — the observed 4K HDR stall.
///
/// It is a named constant rather than a tunable on purpose: Phase 2.3 measures
/// this behaviour, it does not turn it into configuration.
pub const NO_FRAME_FALLBACK_INTERVAL: Duration = Duration::from_millis(33);

/// A platform-neutral handle for telling the runner/event loop that a frame is
/// expected by a given deadline.
///
/// Write-only by design: reading wake state is the runner's business and part of
/// the WP4 telemetry chain, not of this interface.
pub trait WakeHandle: Send + Sync {
    /// Records `deadline` and nudges the event loop.
    fn set_deadline(&self, deadline: Instant);

    /// Requests a wake as soon as possible **without disturbing** any pending
    /// future deadline.
    ///
    /// Implementations backed by an immediate flag should override this; the
    /// default keeps the trait usable by simple/test handles.
    fn wake_now(&self) {
        self.set_deadline(Instant::now());
    }
}

/// Schedules the documented no-frame fallback deadline through `wake`.
///
/// Returns `true` when a handle was notified. Extracted from `RenderEngine` so
/// the behavior (including the absent-handle case and the exact interval) is
/// testable without a GL/mpv context.
pub fn schedule_no_frame_fallback(wake: Option<&Arc<dyn WakeHandle>>, now: Instant) -> bool {
    match wake {
        Some(handle) => {
            handle.set_deadline(now + NO_FRAME_FALLBACK_INTERVAL);
            true
        }
        None => false,
    }
}

/// Independent event-loop wake bridge for the mpv render update callback.
///
/// Why this exists: the mpv callback used to wake the loop only through
/// `egui::Context::request_repaint()`. In egui 0.31.1 that backend callback is
/// deliberately deduplicated — it fires only when the new delay is strictly
/// less than the stored `repaint_delay`, and only an egui pass resets that
/// value. Once a pass stops running with `repaint_delay == 0`, every later
/// immediate request is swallowed and the event loop can park forever even
/// though mpv keeps signalling work (Phase 2.6 WP1 Run-01).
///
/// This bridge gives the callback a wake path that does not depend on egui or
/// on a successful draw. It performs no rendering and calls no libmpv API from
/// the callback thread.
///
/// Thread model:
/// * [`WakeBridge::wake_now`] runs on mpv's thread; it reads an installed
///   handle lock-free after installation and sets one atomic.
/// * [`WakeBridge::clear`] runs on the event loop after it has drained a wake
///   batch, so a burst of callbacks collapses into at most one outstanding
///   wake request between loop iterations.
///
/// The `outstanding` flag is what bounds redundant wakes. It is deliberately
/// cleared by the *event loop*, not by a paint: if it were cleared only by a
/// successful draw, a suspended compositor would suppress every later wake and
/// recreate the deadlock this bridge exists to remove.
#[derive(Default)]
pub struct WakeBridge {
    handle: OnceLock<Arc<dyn WakeHandle>>,
    outstanding: AtomicBool,
}

impl WakeBridge {
    pub fn new() -> Self {
        Self::default()
    }

    /// Installs the event-loop wake handle. Idempotent: the first install wins.
    pub fn install(&self, handle: Arc<dyn WakeHandle>) {
        let _ = self.handle.set(handle);
    }

    pub fn is_installed(&self) -> bool {
        self.handle.get().is_some()
    }

    /// Requests an immediate event-loop wake, at most one at a time.
    ///
    /// Returns `true` only when this call actually requested a wake from the
    /// handle. Repeated calls before [`WakeBridge::clear`] return `false` and do
    /// no work, which keeps the mpv callback from turning into a proxy event
    /// storm.
    pub fn wake_now(&self) -> bool {
        if self.outstanding.swap(true, Ordering::AcqRel) {
            return false;
        }
        match self.handle.get() {
            Some(handle) => {
                handle.wake_now();
                true
            }
            None => {
                // No event loop is wired yet (construction ordering, or the
                // eframe shell). Do not leave the bridge latched: a later
                // install must be able to wake.
                self.outstanding.store(false, Ordering::Release);
                false
            }
        }
    }

    /// Runs on the event loop after it has processed a wake batch.
    pub fn clear(&self) {
        self.outstanding.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Records every deadline it is given.
    struct RecordingWake {
        deadlines: Mutex<Vec<Instant>>,
    }

    impl RecordingWake {
        fn new() -> Self {
            Self {
                deadlines: Mutex::new(Vec::new()),
            }
        }

        fn deadlines(&self) -> Vec<Instant> {
            self.deadlines.lock().unwrap().clone()
        }
    }

    impl WakeHandle for RecordingWake {
        fn set_deadline(&self, deadline: Instant) {
            self.deadlines.lock().unwrap().push(deadline);
        }
    }

    #[test]
    fn trait_object_receives_the_fallback_deadline() {
        let now = Instant::now();
        let recording = Arc::new(RecordingWake::new());
        let wake: Arc<dyn WakeHandle> = recording.clone();

        assert!(schedule_no_frame_fallback(Some(&wake), now));

        let deadlines = recording.deadlines();
        assert_eq!(deadlines.len(), 1);
        assert_eq!(deadlines[0], now + NO_FRAME_FALLBACK_INTERVAL);
        assert_eq!(NO_FRAME_FALLBACK_INTERVAL, Duration::from_millis(33));
    }

    #[test]
    fn absent_handle_is_harmless() {
        let now = Instant::now();
        assert!(!schedule_no_frame_fallback(None, now));
    }

    #[test]
    fn exactly_one_notification_per_call() {
        let now = Instant::now();
        let recording = Arc::new(RecordingWake::new());
        let wake: Arc<dyn WakeHandle> = recording.clone();
        for _ in 0..3 {
            schedule_no_frame_fallback(Some(&wake), now);
        }
        assert_eq!(recording.deadlines().len(), 3);
    }

    /// Deadline accuracy is a scheduler property, not a property of this code.
    ///
    /// This test measures the observed lateness of a wake that the OS delivers
    /// against a monotonic target. It is a bound for *this host*, not a
    /// guarantee, and it says nothing about a loaded or throttled system.
    #[test]
    fn observed_wake_lateness_is_bounded_against_the_monotonic_deadline() {
        let now = Instant::now();
        let recorder = Arc::new(RecordingWake::new());
        let wake: Arc<dyn WakeHandle> = recorder.clone();
        assert!(schedule_no_frame_fallback(Some(&wake), now));
        let deadline = recorder.deadlines()[0];
        assert_eq!(deadline, now + NO_FRAME_FALLBACK_INTERVAL);

        // Stand in for the event loop: sleep until the requested deadline and
        // record when the thread actually resumed.
        let wait = deadline.saturating_duration_since(Instant::now());
        std::thread::sleep(wait);
        let observed = Instant::now();
        let lateness = observed.saturating_duration_since(deadline);
        assert!(
            lateness < Duration::from_millis(250),
            "wake arrived {lateness:?} after the requested deadline"
        );
    }

    #[test]
    fn trait_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync + ?Sized>() {}
        assert_send_sync::<dyn WakeHandle>();
    }

    // --- Phase 2.6 WP1 recovery: independent wake bridge --------------------

    /// Reproduces the real egui 0.31.1 repaint-deduplication edge case.
    ///
    /// `ContextImpl::request_repaint_after` calls the backend callback only when
    /// the requested delay is *strictly less* than the stored `repaint_delay`.
    /// A zero-delay request stores zero, so every later zero-delay request is
    /// swallowed until an egui pass resets the value. This is the exact
    /// condition that parked the Run-01 event loop.
    #[test]
    fn egui_0_31_suppresses_repeated_immediate_repaint_requests() {
        use std::sync::atomic::AtomicUsize;
        let ctx = egui::Context::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        ctx.set_request_repaint_callback(move |_info| {
            counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        });

        ctx.request_repaint();
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::Relaxed),
            1,
            "the first immediate request must reach the backend"
        );

        for _ in 0..64 {
            ctx.request_repaint();
        }
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::Relaxed),
            1,
            "egui 0.31.1 must deduplicate repeated zero-delay requests"
        );
    }

    /// The bridge must keep waking even while egui is suppressing its callback.
    #[test]
    fn wake_bridge_is_independent_of_egui_repaint_suppression() {
        use std::sync::atomic::AtomicUsize;
        let ctx = egui::Context::default();
        let egui_calls = Arc::new(AtomicUsize::new(0));
        let counter = egui_calls.clone();
        ctx.set_request_repaint_callback(move |_info| {
            counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        });
        ctx.request_repaint();
        for _ in 0..8 {
            ctx.request_repaint();
        }
        let suppressed = egui_calls.load(std::sync::atomic::Ordering::Relaxed);
        assert_eq!(suppressed, 1, "egui must be in its suppressing state");

        let recording = Arc::new(RecordingWake::new());
        let bridge = WakeBridge::new();
        bridge.install(recording.clone());

        // Model eight event-loop iterations: mpv's callback wakes, the loop
        // drains, then the next callback arrives.
        for _ in 0..8 {
            assert!(bridge.wake_now());
            bridge.clear();
        }
        assert_eq!(
            recording.deadlines().len(),
            8,
            "every loop iteration must receive an independent wake"
        );
        assert_eq!(
            egui_calls.load(std::sync::atomic::Ordering::Relaxed),
            suppressed,
            "the bridge must not depend on egui being un-suppressed"
        );
    }

    #[test]
    fn wake_bridge_coalesces_a_callback_burst_between_loop_iterations() {
        let recording = Arc::new(RecordingWake::new());
        let bridge = WakeBridge::new();
        bridge.install(recording.clone());

        assert!(bridge.wake_now());
        for _ in 0..100 {
            assert!(
                !bridge.wake_now(),
                "a burst before the loop drains is one wake"
            );
        }
        assert_eq!(recording.deadlines().len(), 1);

        bridge.clear();
        assert!(bridge.wake_now());
        assert_eq!(recording.deadlines().len(), 2);
    }

    #[test]
    fn wake_bridge_before_install_does_not_latch() {
        let bridge = WakeBridge::new();
        assert!(!bridge.is_installed());
        assert!(!bridge.wake_now());

        let recording = Arc::new(RecordingWake::new());
        bridge.install(recording.clone());
        assert!(bridge.is_installed());
        assert!(bridge.wake_now());
        assert_eq!(recording.deadlines().len(), 1);
    }

    #[test]
    fn wake_bridge_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<WakeBridge>();
        assert_send_sync::<Arc<WakeBridge>>();
    }
}
