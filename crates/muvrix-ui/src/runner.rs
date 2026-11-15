//! Native presentation runner (Wayland).
//!
//! Ownership split, chosen in `docs/phase-2.0/02-presentation-architecture-decision.md`:
//!
//! * winit owns the window, the `wl_display` connection and the event loop. It is
//!   the only reader of the socket, which keeps input, IME, clipboard, HiDPI and
//!   fullscreen working without reimplementation.
//! * Muvrix owns the EGL config, context, window surface, swap interval and the
//!   colour description, because those are exactly the parts eframe did not let
//!   us control (it always asks for an 8-bit config and never tags the surface).
//!
//! Everything is single-threaded: one event loop, one GL context, one socket
//! reader, one surface owner. There is no render thread and no second dispatch
//! loop, so `wl_display_dispatch()` appears nowhere in the production path.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use glow::HasContext as _;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::raw_window_handle::{
    HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle,
};
use winit::window::{Window, WindowId};

use crate::app::MuvrixApp;
use crate::fbo::OffscreenFbo;
use crate::mpv_state::MpvStateSampler;
use crate::output::OutputCaps;
use crate::platform::color::{ColorManager, SurfaceDescription};
use crate::platform::composite::{ColorMode, Composite};
use crate::platform::precision::{probe_surface_precision, PrecisionReport};
use crate::platform::presenter::{self, Presenter};
use crate::presentation::{PresentationStateMachine, StateEvent};
use crate::presentation_ack::{AckAction, PresentationAckQueue};
use crate::telemetry::{Counters, SharedMetrics, TelemetryConfig, TelemetryWorker};
use crate::wake::WakeHandle;

/// Reference white used when mapping SDR UI onto a PQ surface (BT.2408).
const UI_WHITE_NITS: f32 = 203.0;

#[derive(Debug, Clone)]
pub struct RunnerConfig {
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub initial_file: Option<String>,
    /// Request a 10-bit surface and attempt PQ output. Disable with
    /// `MUVRIX_NO_HDR=1` to force the SDR path for comparison.
    pub want_hdr: bool,
}

/// Cross-thread hand-off for repaint requests.
///
/// egui's repaint callback can be invoked from any thread (mpv's update callback
/// runs on an engine thread), so it only records a deadline or an immediate
/// flag here, then nudges the winit loop through an `EventLoopProxy`.
///
/// The proxy nudge is not an optimisation. Recording a flag alone is racy: a
/// wake that arrives after `about_to_wait` has read-and-cleared the flag, but
/// before the loop actually blocks, leaves the flag set and the loop asleep
/// forever. That is a real failure — the first 30-minute soak froze after 18
/// minutes with the event loop parked in `epoll_wait` and mpv waiting for a
/// swap report that could never come. `send_event` wakes the loop from any
/// thread, so the hand-off cannot be lost.
pub struct WakeSignal {
    inner: std::sync::Mutex<WakeState>,
    proxy: Option<EventLoopProxy<RunnerEvent>>,
    /// Phase 2.4 instrumentation: what the wake path was actually asked to do.
    deadlines_set: std::sync::atomic::AtomicU64,
    deadlines_replaced: std::sync::atomic::AtomicU64,
    immediate_requests: std::sync::atomic::AtomicU64,
}

#[derive(Default)]
struct WakeState {
    immediate: bool,
    deadline: Option<Instant>,
}

impl Default for WakeSignal {
    fn default() -> Self {
        Self {
            inner: Mutex::new(WakeState::default()),
            proxy: None,
            deadlines_set: Default::default(),
            deadlines_replaced: Default::default(),
            immediate_requests: Default::default(),
        }
    }
}

impl WakeSignal {
    fn new(proxy: EventLoopProxy<RunnerEvent>) -> Self {
        Self {
            inner: Mutex::new(WakeState::default()),
            proxy: Some(proxy),
            deadlines_set: Default::default(),
            deadlines_replaced: Default::default(),
            immediate_requests: Default::default(),
        }
    }

    /// `(deadlines_set, deadlines_replaced, immediate_requests)`.
    ///
    /// Logged at teardown rather than emitted as CSV columns: these are bake-off
    /// diagnostics for whether the 33 ms fallback causes redundant work, not
    /// per-second reliability measurements, and the schema is deliberately not
    /// widened for them.
    fn counters(&self) -> (u64, u64, u64) {
        use std::sync::atomic::Ordering::Relaxed;
        (
            self.deadlines_set.load(Relaxed),
            self.deadlines_replaced.load(Relaxed),
            self.immediate_requests.load(Relaxed),
        )
    }

    #[cfg(test)]
    fn request_immediate(&self) {
        self.request_immediate_from("immediate");
    }

    fn request_immediate_from(&self, source: &'static str) {
        self.immediate_requests
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if let Ok(mut s) = self.inner.lock() {
            s.immediate = true;
        }
        self.nudge(source);
    }

    /// Wakes the event loop. Safe from any thread; ignored if the loop is gone.
    fn nudge(&self, source: &'static str) {
        if let Some(proxy) = &self.proxy {
            // A request is not a delivery: record both sides separately so a run
            // whose sends never reach the loop is distinguishable from one that
            // simply has nothing to do.
            match proxy.send_event(RunnerEvent::Wake) {
                Ok(()) => {
                    crate::trace::record(crate::trace::Ev::ProxySend, "ok", 0, 0);
                    crate::causal_trace::record(
                        crate::causal_trace::Kind::ProxySend,
                        crate::causal_trace::last_callback(),
                        1,
                        0,
                        0,
                        source,
                    );
                }
                Err(_) => {
                    crate::trace::record(crate::trace::Ev::ProxySend, "closed", 0, 0);
                    crate::causal_trace::record(
                        crate::causal_trace::Kind::ProxySend,
                        crate::causal_trace::last_callback(),
                        0,
                        0,
                        0,
                        source,
                    );
                }
            }
        } else {
            crate::trace::record(crate::trace::Ev::ProxySend, "no_proxy", 0, 0);
            crate::causal_trace::record(
                crate::causal_trace::Kind::ProxySend,
                crate::causal_trace::last_callback(),
                -1,
                0,
                0,
                source,
            );
        }
    }

    /// Records the earliest of the pending deadlines and nudges the loop.
    ///
    /// Deliberately not named `set_deadline`: that name belongs to the
    /// platform-neutral [`WakeHandle`] trait, and an inherent method with the
    /// same name is exactly how recursive trait forwarding happens.
    pub fn request_deadline(&self, at: Instant) {
        self.request_deadline_from(at, "deadline");
    }

    fn request_deadline_from(&self, at: Instant, source: &'static str) {
        use std::sync::atomic::Ordering::Relaxed;
        self.deadlines_set.fetch_add(1, Relaxed);
        let mut replaced = false;
        if let Ok(mut s) = self.inner.lock() {
            if s.deadline.is_some() {
                self.deadlines_replaced.fetch_add(1, Relaxed);
                replaced = true;
            }
            s.deadline = Some(match s.deadline {
                Some(existing) if existing < at => existing,
                _ => at,
            });
        }
        let delay_ms = at.saturating_duration_since(Instant::now()).as_millis() as i64;
        crate::trace::record(
            crate::trace::Ev::DeadlineSet,
            if replaced { "replaced" } else { "new" },
            delay_ms,
            self.deadlines_set.load(Relaxed) as i64,
        );
        self.nudge(source);
    }

    /// Returns the pending deadline without consuming it.
    ///
    /// Used by `about_to_wait` to derive `ControlFlow` after arming the
    /// suspension probe, and by tests.
    pub(crate) fn peek_deadline(&self) -> Option<Instant> {
        self.inner.lock().ok().and_then(|s| s.deadline)
    }

    /// Drops any pending deadline.
    ///
    /// Used only when a presentation suspension ends: the probe deadline exists
    /// to notice recovery, so once recovery is observed it must not cause one
    /// more pointless wake. The normal draw-end deadline is re-armed later in
    /// the same paint.
    pub(crate) fn cancel_deadline(&self) {
        if let Ok(mut s) = self.inner.lock() {
            s.deadline = None;
        }
    }

    /// Returns `(immediate, deadline)`, consuming the deadline only when due.
    ///
    /// A deadline still in the future is **retained**: it is the only thing that
    /// will wake the loop for the next frame. The previous implementation
    /// consumed it unconditionally, so any unrelated wake (an egui repaint, a
    /// window event, mpv's update callback) silently disarmed the scheduled
    /// wake. `about_to_wait` then set `ControlFlow::WaitUntil(at)` on an instant
    /// that had already been dropped, and the loop parked forever. That is the
    /// mechanism behind both the Phase 2.2 4K no-frame stall and Test D's
    /// failure to resume after an injected interruption.
    ///
    /// A deadline that has already passed *is* cleared, otherwise every
    /// `about_to_wait` would redraw immediately and turn the loop into a spin.
    fn take_due(&self, now: Instant) -> (bool, Option<Instant>) {
        match self.inner.lock() {
            Ok(mut s) => {
                let immediate = std::mem::take(&mut s.immediate);
                let deadline = s.deadline;
                let mut due = 0i64;
                let mut lateness = 0i64;
                match deadline {
                    Some(at) if at <= now => {
                        s.deadline = None;
                        due = 1;
                        lateness = now.saturating_duration_since(at).as_micros() as i64;
                        crate::trace::record(
                            crate::trace::Ev::DeadlineDue,
                            "due",
                            now.saturating_duration_since(at).as_millis() as i64,
                            0,
                        );
                    }
                    Some(_) => {
                        crate::trace::record(crate::trace::Ev::DeadlineRetained, "retained", 0, 0)
                    }
                    None => {}
                }
                crate::causal_trace::record(
                    crate::causal_trace::Kind::WakeTakeDue,
                    crate::causal_trace::last_callback(),
                    immediate as i64,
                    due,
                    lateness,
                    if due == 1 { "due" } else { "retained" },
                );
                (immediate, deadline)
            }
            Err(_) => (false, None),
        }
    }
}

/// Linux implementation of the platform-neutral wake interface (WP5).
impl WakeHandle for WakeSignal {
    fn set_deadline(&self, deadline: Instant) {
        // Call the inherent method by its full path. `self.set_deadline(..)`
        // would resolve to this trait method and recurse until the stack dies.
        WakeSignal::request_deadline_from(self, deadline, "wake_handle");
    }

    fn wake_now(&self) {
        // Use the immediate flag, not a `now` deadline: a deadline would win
        // the `min()` race against a draw-end/target-time deadline and replace
        // it. The immediate flag wakes the loop while leaving the pacing
        // deadline intact.
        WakeSignal::request_immediate_from(self, "wake_bridge");
    }
}

/// Arms the next independent suspension-probe deadline.
///
/// Called from `about_to_wait` **after** `take_due`. Arming before `take_due`
/// would merge this future deadline behind the probe deadline that is already
/// due, and consuming the due deadline would then clear both — leaving no next
/// wake. Returns `true` when a probe is active.
///
/// This is deliberately separate from [`Shell::note_frame_withholding`]:
/// that function decides *whether* a probe is due, while this one guarantees
/// the timer exists. It does not depend on `RedrawRequested`, so it keeps the
/// loop alive when the compositor withholds frame callbacks.
fn arm_suspension_probe(wake: &WakeSignal, withheld: bool, now: Instant) -> bool {
    if !withheld {
        return false;
    }
    let deadline = now + SUSPENSION_PROBE_INTERVAL;
    match wake.peek_deadline() {
        Some(existing) if existing <= deadline => {
            // A wake is already scheduled no later than the next probe. Do NOT
            // call `request_deadline` again: it always nudges the proxy, and
            // nudging from every `about_to_wait` is a self-feeding event loop
            // (measured at ~383k wakes/s in the first recovery validation).
            // The retained deadline is what keeps the loop waking.
        }
        _ => wake.request_deadline(deadline),
    }
    true
}

/// How long an outstanding redraw request may go unhonoured before presentation
/// is declared suspended rather than stalled.
///
/// A healthy Wayland frame callback arrives within a refresh interval (~16 ms).
/// 300 ms is far beyond any scheduling jitter and far below the 1 s stall
/// threshold, so the two classifications cannot fight over the same event.
pub const WITHHELD_FRAME_CALLBACK_MS: Duration = Duration::from_millis(300);

/// How often a suspended surface re-requests a paint so it can notice that the
/// compositor resumed compositing it.
pub const SUSPENSION_PROBE_INTERVAL: Duration = Duration::from_millis(500);

/// Bounded redraw-request gate.
///
/// Winit coalesces redraw requests internally, but the application previously
/// issued one on every wake and deadline without any visible gate. This makes
/// the policy explicit: while a request is outstanding, another is suppressed
/// unless the suspension-probe interval has elapsed. That bounds request storms
/// during callback withholding while still allowing the probe to retry. A
/// completed paint clears the gate.
#[derive(Debug, Default)]
struct RedrawGate {
    pending_since: Option<Instant>,
    last_request_at: Option<Instant>,
}

impl RedrawGate {
    /// Returns true when a request should be sent to winit.
    fn request(&mut self, now: Instant, retry_after: Duration) -> bool {
        if self.pending_since.is_some() {
            if let Some(last) = self.last_request_at {
                if now.saturating_duration_since(last) < retry_after {
                    return false;
                }
            }
        } else {
            self.pending_since = Some(now);
        }
        self.last_request_at = Some(now);
        true
    }

    /// A paint completed, so the next request is immediate.
    fn complete(&mut self) {
        self.pending_since = None;
        self.last_request_at = None;
    }

    fn pending_since(&self) -> Option<Instant> {
        self.pending_since
    }
}

/// Test-only controlled rendering interruption (Phase 2.4, Part 4/Test D).
///
/// There is no UI affordance and no production path: it arms only when
/// `MUVRIX_TEST_RENDER_STALL_MS` is set by the test harness. While active the
/// runner skips GL, mpv rendering and the swap, but keeps the process, event
/// loop and telemetry thread alive — which is what makes it different from
/// `kill -STOP`, where measurement would stop too.
#[derive(Debug)]
struct FaultInjection {
    stall: Option<Duration>,
    at: Duration,
    started_at: Option<Instant>,
    finished: bool,
    /// Paints that arrived while the window was stalled (diagnostic).
    draw_passes: u64,
}

impl FaultInjection {
    fn from_env() -> Self {
        let stall = std::env::var("MUVRIX_TEST_RENDER_STALL_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|ms| (100..=120_000).contains(ms))
            .map(Duration::from_millis);
        let at = std::env::var("MUVRIX_TEST_RENDER_STALL_AT_S")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(10.0);
        if let Some(len) = stall {
            log::warn!(
                "TEST-ONLY render stall injection armed: {} ms at t={}s",
                len.as_millis(),
                at
            );
        }
        Self {
            stall,
            at: Duration::from_secs_f64(at.max(0.0)),
            started_at: None,
            finished: false,
            draw_passes: 0,
        }
    }

    /// True while a stall window is open (test-only).
    fn is_stalled(&self) -> bool {
        self.started_at.is_some() && !self.finished
    }

    /// Some(end) while the pipeline must stay paused.
    fn stall_until(&mut self, now: Instant, origin: Instant) -> Option<Instant> {
        let len = self.stall?;
        if self.finished {
            return None;
        }
        match self.started_at {
            None => {
                if now.saturating_duration_since(origin) < self.at {
                    return None;
                }
                self.started_at = Some(now);
                self.draw_passes = 1;
                log::warn!("TEST-ONLY render stall START ({} ms)", len.as_millis());
                Some(now + len)
            }
            Some(start) => {
                let end = start + len;
                if now < end {
                    self.draw_passes = self.draw_passes.saturating_add(1);
                    Some(end)
                } else {
                    self.finished = true;
                    log::warn!(
                        "TEST-ONLY render stall END after {} ms ({} paint passes, \
                         {:.1}/s)",
                        now.saturating_duration_since(start).as_millis(),
                        self.draw_passes,
                        self.draw_passes as f64 / len.as_secs_f64()
                    );
                    None
                }
            }
        }
    }
}

/// Everything that must exist while the window is alive.
struct Shell {
    window: Arc<Window>,
    presenter: Presenter,
    gl: Arc<glow::Context>,
    painter: egui_glow::Painter,
    egui_state: egui_winit::State,
    egui_ctx: egui::Context,
    app: MuvrixApp,
    test_sequence: Vec<(f64, String)>,
    next_sequence: usize,
    ui_fbo: OffscreenFbo,
    composite: Composite,
    gpu: crate::gpu_timing::GpuTiming,
    color: Option<ColorManager>,
    description: Option<SurfaceDescription>,
    color_mode: ColorMode,
    precision: PrecisionReport,
    output: OutputCaps,
    /// Counters and snapshot published to the independent telemetry thread.
    metrics: Arc<SharedMetrics>,
    telemetry: TelemetryWorker,
    /// Paint-independent mpv playback-state sampler (`MUVRIX_MPV_STATE_CSV`).
    /// Runs on its own thread so engine state stays observable when `draw()`
    /// stops.
    mpv_state: Option<MpvStateSampler>,
    /// Local cumulative counters that are not owned by another component.
    counters: Counters,
    /// When the previous successful handoff happened, for inter-swap gaps.
    last_present_at: Option<Instant>,
    /// Count of update callbacks already fed to the state machine.
    fed_update_callbacks: u64,
    /// Run origin; the telemetry thread shares this instant.
    started: Instant,
    /// Test-only rendering interruption (Phase 2.4 fault injection).
    fault: FaultInjection,
    /// When the stall-end self-wake was last issued.
    ///
    /// Re-armed periodically rather than once: a single wake is a *request* for a
    /// redraw, and a surface the compositor is not compositing never receives
    /// one, which left a test stall open for the rest of the run (Test 1,
    /// Phase 2.5). 500 ms between attempts cannot spin.
    stall_wake_armed: Option<Instant>,
    /// Second at which the mpv-derived playback stats were last refreshed
    /// (see `publish_tick`).
    last_published_sec: u64,
    /// Playback stats cached once per second.
    ///
    /// `playback_stats()` performs five `get_property` round trips, which is why
    /// the counter set used to be published only once per second. Caching them
    /// keeps the per-paint publication below cheap: only the snapshot and the
    /// already-atomic counters are stored per frame.
    cached_stats: crate::app::PlaybackStats,
    /// WP3 explicit state, driven only by authoritative events.
    state_machine: PresentationStateMachine,
    /// Last playback state seen, so pause/resume/EOF produce exactly one event.
    last_playback_state: Option<muvrix_player::PlaybackState>,
    wake: Arc<WakeSignal>,
    frame_count: u64,
    /// Optional run-length limit in seconds (`MUVRIX_RUN_SECONDS`), used by the
    /// measurement harness so the normal exit path — including teardown — is
    /// the one under test.
    run_limit: Option<Duration>,
    /// When the outstanding redraw request was issued (cleared by `draw`).
    ///
    /// Phase 2.5 found the decisive symptom here: after a redraw request the
    /// compositor can withhold surface frame callbacks (an obscured Wayland
    /// surface is not composited), so `RedrawRequested` never arrives even though
    /// the event loop is healthy and mpv keeps producing frames. Tracking the
    /// outstanding request turns that into a *known* presentation suspension
    /// instead of a false stall.
    redraw_gate: RedrawGate,
    /// True while the compositor is withholding frame callbacks.
    frame_callbacks_withheld: bool,
    /// Last recovery probe issued while presentation is suspended.
    ///
    /// A suspended surface must keep asking, at a low rate, otherwise it never
    /// learns that the compositor is compositing it again and the picture stays
    /// black for the rest of the run.
    last_suspension_probe: Option<Instant>,
    exit_requested: bool,
    occluded: bool,
    destroyed: bool,
    /// Last physical drawable size logged; re-logged on every change so
    /// fullscreen/scale transitions are recorded, not just the pre-fullscreen
    /// first frame.
    last_drawable: (u32, u32),
    /// One-time monitor refresh report for cadence evidence.
    monitor_reported: bool,
    /// Token-ordered wp_presentation acknowledgement state.
    presentation_ack: PresentationAckQueue,
    /// Last logical frame-drop count recorded with a frame deadline.
    last_frame_drop_total: Option<i64>,
}

impl Shell {
    /// Test-only sequential media loads for A/B transitions in one process.
    fn poll_test_sequence(&mut self) {
        if self.test_sequence.is_empty() {
            return;
        }
        // Keep engine observation alive even when the compositor withholds
        // paints. This is test-only (`MUVRIX_TEST_SEQUENCE`) and never runs in
        // normal playback.
        self.app.poll_engine_only();
        if self.next_sequence >= self.test_sequence.len() {
            return;
        }
        let (due, path) = &self.test_sequence[self.next_sequence];
        if self.started.elapsed().as_secs_f64() >= *due {
            let path = path.clone();
            log::warn!("TEST-ONLY: sequential media load at {due:.1}s: {path}");
            self.app.load_file(&path);
            self.next_sequence += 1;
        }
    }

    /// Requests a redraw and counts it (WP4 `redraw_requests_*`).
    ///
    /// The count lives in `SharedMetrics` so the per-second CSV and the teardown
    /// summary read one atomic instead of two independent tallies (Part 6).
    fn request_redraw(&mut self) {
        let now = Instant::now();
        let pending_before = self.redraw_gate.pending_since().is_some() as i64;
        let accepted = self.redraw_gate.request(now, SUSPENSION_PROBE_INTERVAL);
        crate::causal_trace::record(
            crate::causal_trace::Kind::RedrawRequest,
            crate::causal_trace::last_callback(),
            accepted as i64,
            pending_before,
            self.redraw_gate.pending_since().is_some() as i64,
            if accepted { "accepted" } else { "coalesced" },
        );
        if !accepted {
            crate::trace::record(crate::trace::Ev::RedrawRequested, "coalesced", 0, 0);
            return;
        }
        self.metrics.note_redraw_request();
        crate::trace::record(crate::trace::Ev::RedrawRequested, "request", 0, 0);
        self.window.request_redraw();
    }

    /// Counts one wake event delivered by the event loop (WP4 `wake_events_*`).
    fn note_wake(&mut self) {
        self.metrics.note_wake_delivered();
    }

    /// Dispatches already-decoded Wayland feedback and applies the ack queue.
    ///
    /// `ColorManager` uses the foreign display's private queue; winit remains
    /// the only socket reader. This never performs a blocking read and never
    /// touches the GL thread's file I/O.
    fn drain_presentation_feedback(&mut self) {
        let mut events = Vec::new();
        if let Some(cm) = self.color.as_mut() {
            cm.dispatch_pending();
            events = cm.drain_presentation_events();
        }
        for event in events {
            match event {
                crate::platform::color::PresentationEvent::Presented { token, .. } => {
                    self.metrics.note_presented_feedback();
                    let actions = self.presentation_ack.presented(token);
                    self.apply_ack_actions(actions);
                }
                crate::platform::color::PresentationEvent::Discarded { token } => {
                    let actions = self.presentation_ack.discarded(token);
                    self.apply_ack_actions(actions);
                }
            }
        }
        let expired = self.presentation_ack.expire(Instant::now());
        self.apply_ack_actions(expired);
    }

    /// Requests feedback for the frame that will be committed by this swap.
    ///
    /// Returns true only when a feedback object was created. A false result is
    /// the compositor-without-wp_presentation fallback: acknowledge after the
    /// successful swap, exactly as before, rather than leaving libmpv waiting.
    fn request_presentation_feedback(&mut self, token: Option<u64>) -> bool {
        let Some(token) = token else {
            return false;
        };
        let Some(cm) = self.color.as_mut() else {
            return false;
        };
        if !cm.has_presentation_feedback() {
            return false;
        }
        match cm.request_presentation_feedback(self.presenter.surface_ptr(), token) {
            Ok(()) => true,
            Err(e) => {
                log::warn!("presentation feedback request failed: {e}");
                false
            }
        }
    }

    /// Records one rendered frame's deadline correlation.
    ///
    /// The record is keyed by the engine frame token. It joins:
    /// `FrameCallbackRequest`/`RedrawRequested` (via the callback seq),
    /// `DrawEnter` (via the draw seq), `RenderEnter`/`RenderExit`,
    /// `SwapExit`, `PresentationPresented`, and `ReportSwap`. The drop delta is
    /// read from the asynchronous watcher; no mpv property read is added to the
    /// GL thread.
    fn record_frame_deadline(
        &mut self,
        token: u64,
        callback_seq: u64,
        swap_us: i64,
        swap_ok: bool,
    ) {
        let (drop_total, _) = crate::causal_trace::drop_snapshot();
        let drop_delta = match (self.last_frame_drop_total, drop_total) {
            (Some(previous), current) if previous >= 0 && current >= previous => current - previous,
            (_, current) if current >= 0 => 0,
            _ => -1,
        };
        self.last_frame_drop_total = Some(drop_total);
        let render_us = self
            .app
            .render_telemetry()
            .map(|telemetry| telemetry.last_render_block_us as i64)
            .unwrap_or(-1);
        crate::causal_trace::record5(
            crate::causal_trace::Kind::FrameDeadline,
            token,
            callback_seq as i64,
            crate::causal_trace::last_draw() as i64,
            render_us,
            swap_us,
            drop_delta,
            if swap_ok { "swap_ok" } else { "swap_fail" },
        );
    }

    fn apply_ack_actions(&mut self, actions: Vec<AckAction>) {
        // Batch the presentation confirmations so one compositor burst costs one
        // engine lock acquisition. Drop actions retire their feedback proxy
        // without ever calling report_swap().
        let tokens: Vec<u64> = actions
            .iter()
            .filter_map(|action| match action {
                AckAction::Report { token } => Some(*token),
                AckAction::Drop { .. } => None,
            })
            .collect();
        if !tokens.is_empty() {
            self.app.report_swap_tokens(&tokens);
        }
        if let Some(cm) = self.color.as_mut() {
            for action in actions {
                // Report actions retire the proxy because the frame has already
                // been accounted for; older FIFO entries retired by a newer
                // presentation must not retain their feedback objects. Drop
                // actions retire without ever reporting.
                let token = match action {
                    AckAction::Report { token } | AckAction::Drop { token } => token,
                };
                cm.discard_feedback(token);
            }
        }
    }

    /// Arms the acknowledgement watchdog below libmpv's 200 ms VO timeout.
    ///
    /// The deadline is merged with the existing wake deadline; it is not a
    /// spin. Once all tokens are retired the queue has no next deadline.
    fn arm_presentation_ack_watchdog(&mut self) {
        let Some(at) = self.presentation_ack.next_deadline() else {
            return;
        };
        let already_armed = self
            .wake
            .peek_deadline()
            .map(|existing| existing <= at)
            .unwrap_or(false);
        if !already_armed {
            self.wake.request_deadline(at);
        }
    }

    /// Declares a known presentation suspension when an outstanding redraw
    /// request is not honoured.
    ///
    /// This is the Phase 2.5 correction for the false-stall class. The evidence
    /// is a `request_redraw` that produced no `RedrawRequested` for longer than
    /// `WITHHELD_FRAME_CALLBACK_MS`, while the event loop continues to run. On
    /// Wayland that is what an obscured surface looks like: the compositor does
    /// not composite it and therefore sends no frame callbacks. It is an
    /// intentional absence of presentation, not a fault and not a stall.
    fn note_frame_withholding(&mut self, now: Instant) {
        let Some(since) = self.redraw_gate.pending_since() else {
            return;
        };
        if self.frame_callbacks_withheld {
            // Probe for recovery. The *timer* that wakes the loop is armed by
            // `about_to_wait` through `arm_suspension_probe`, independently of
            // any future `RedrawRequested`. When that timer fires, the due
            // deadline branch there issues the redraw request, so this method
            // only advances the bookkeeping. Keeping the two separate is what
            // makes the probe work while the compositor withholds callbacks and
            // avoids issuing two redraws for one probe.
            let due = self
                .last_suspension_probe
                .map(|at| now.saturating_duration_since(at) >= SUSPENSION_PROBE_INTERVAL)
                .unwrap_or(true);
            if due {
                self.last_suspension_probe = Some(now);
            }
            return;
        }
        let waiting = now.saturating_duration_since(since);
        if waiting < WITHHELD_FRAME_CALLBACK_MS {
            return;
        }
        self.frame_callbacks_withheld = true;
        // Baseline for the probe cadence. `about_to_wait` arms the first
        // independent deadline in the same iteration, after `take_due`.
        self.last_suspension_probe = Some(now);
        log::warn!(
            "no RedrawRequested for {}ms after a redraw request: presentation suspended \
             (compositor is withholding frame callbacks)",
            waiting.as_millis()
        );
        crate::trace::record(
            crate::trace::Ev::StallStart,
            "frame_callbacks_withheld",
            waiting.as_millis() as i64,
            0,
        );
        crate::trace::dump("frame_callbacks_withheld");
        self.state_machine
            .on_event(StateEvent::PresentationSuspended, now);
        // Publish immediately: `draw` is exactly what is not happening, so
        // waiting for the next paint would leave the telemetry thread with a
        // stale `presenting` snapshot and it would keep classifying a stall.
        self.publish();
    }

    /// Maps the playback state onto WP3 events, emitting only on change.
    fn sync_playback_state(&mut self, now: Instant) {
        use muvrix_player::PlaybackState;
        let current = self.app.playback_state();
        let previous = self.last_playback_state.clone();
        self.last_playback_state = Some(current.clone());
        if previous.as_ref() == Some(&current) {
            return;
        }
        let event = match current {
            PlaybackState::Playing => {
                if matches!(previous, Some(PlaybackState::Paused)) {
                    Some(StateEvent::PlaybackResumed)
                } else {
                    Some(StateEvent::PlaybackStarted)
                }
            }
            PlaybackState::Paused => Some(StateEvent::PlaybackPaused),
            PlaybackState::Ended => Some(StateEvent::PlaybackEof),
            PlaybackState::Idle => None,
            // Loading/Buffering/Error: the engine has not stopped expecting
            // frames, and none of these is evidence of a surface failure.
            PlaybackState::Loading | PlaybackState::Buffering | PlaybackState::Error(_) => None,
        };
        if let Some(event) = event {
            self.state_machine.on_event(event, now);
        }
    }

    /// Cumulative counters for the per-second telemetry row.
    ///
    /// Every field is read from the component that owns the event, so the CSV
    /// cannot report a number that no subsystem actually incremented.
    fn counters(&self) -> Counters {
        let engine = self.app.render_telemetry();
        // Cached once per second by `publish_tick`: reading these properties on
        // every paint starved the render thread (Test A).
        let stats = self.cached_stats;
        Counters {
            update_callbacks_total: self.app.update_callback_count(),
            new_video_frames_total: engine.as_ref().map(|e| e.mpv_renders).unwrap_or(0),
            fbo_render_calls_total: self.counters.fbo_render_calls_total,
            egl_swap_attempts_total: self.counters.egl_swap_attempts_total,
            egl_swap_successes_total: self.counters.egl_swap_successes_total,
            egl_swap_failures_total: self.counters.egl_swap_failures_total,
            mpv_report_swap_calls_total: engine.as_ref().map(|e| e.swap_reports).unwrap_or(0),
            wake_events_total: self.metrics.wake_events(),
            redraw_requests_total: self.metrics.redraw_requests(),
            no_frame_fallback_deadlines_total: engine
                .as_ref()
                .map(|e| e.no_frame_fallback_deadlines)
                .unwrap_or(0),
            dropped_frames_total: stats.dropped.unwrap_or(0).max(0) as u64,
            decoder_dropped_total: stats.decoder_dropped.unwrap_or(-1).max(-1),
        }
    }

    /// Publishes the current counters and presentation snapshot. Never blocks.
    ///
    /// `counters()` reads mpv properties, so this is *not* cheap enough to call
    /// on every paint: doing that starved the render thread (Test A showed
    /// `dropped` climbing into the thousands and the pipeline freezing after
    /// ~3.5 min). Rows are per second, and ages are computed by the worker at
    /// sample time from per-event atomics, so a once-per-second publish loses
    /// nothing and keeps the second-aligned deltas honest.
    fn publish(&self) {
        self.metrics
            .publish(&self.counters(), self.state_machine.snapshot());
    }

    /// Publishes per paint; refreshes the mpv-derived stats once per second.
    ///
    /// The split matters for correctness, not only for cost. A published
    /// snapshot is the only carrier of `last_present_at`, `last_swap_at` and
    /// `last_callback_at`, and the telemetry thread computes those ages at its
    /// *own* sampling instant. Publishing the snapshot once per second left the
    /// ages up to one second stale, which is longer than the 1000 ms stall
    /// threshold, so a healthy renderer could be classified `stalled` purely
    /// because the sample landed between two publishes (WP1 preflight-01,
    /// t=3.012: `last_present_age_s=1.002` with 60 successful swaps that second).
    ///
    /// Counters are cumulative atomics, so publishing them per paint also makes
    /// each row's `*_this_second` delta cover exactly that row's own window
    /// instead of a variable number of once-per-second publications.
    fn publish_tick(&mut self) {
        let second = self.started.elapsed().as_secs();
        if second != self.last_published_sec {
            self.last_published_sec = second;
            self.cached_stats = self.app.playback_stats();
        }
        self.publish();
    }

    /// Feeds one presentation event per mpv update callback observed since the
    /// previous paint, so `last_callback_age_s` is a real measurement instead of
    /// a permanently unavailable column.
    fn feed_update_callbacks(&mut self, now: Instant) {
        let total = self.app.update_callback_count();
        if total <= self.fed_update_callbacks {
            return;
        }
        // Bound the work per paint: the counter is only a wake indicator, and a
        // batch of thousands carries no extra state-machine information.
        let pending = (total - self.fed_update_callbacks).min(64);
        for _ in 0..pending {
            self.state_machine.on_event(StateEvent::UpdateCallback, now);
        }
        self.fed_update_callbacks = total;
    }

    fn build(
        event_loop: &ActiveEventLoop,
        proxy: &EventLoopProxy<RunnerEvent>,
        cfg: &RunnerConfig,
    ) -> Result<Self, String> {
        crate::causal_trace::init_from_env();
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title(cfg.title.clone())
                        .with_inner_size(winit::dpi::PhysicalSize::new(cfg.width, cfg.height))
                        .with_min_inner_size(winit::dpi::PhysicalSize::new(640, 360)),
                )
                .map_err(|e| format!("create_window failed: {e}"))?,
        );

        let (display_ptr, surface_ptr) = wayland_handles(&window)?;
        let size = window.inner_size();
        let (width, height) = (size.width.max(1), size.height.max(1));

        // 1. Presenter: config + context + surface, interval 0. If this fails we
        //    cannot present at all, so it is a hard error rather than a fallback.
        let presenter =
            unsafe { Presenter::new(display_ptr, surface_ptr, width, height, cfg.want_hdr) }
                .map_err(|e| format!("EGL presentation unavailable: {e}"))?;

        let gl = unsafe {
            glow::Context::from_loader_function(|name| presenter::get_proc_address(name))
        };
        let gl = Arc::new(gl);
        let gl_version = unsafe { gl.get_parameter_string(glow::VERSION) };
        let gl_renderer = unsafe { gl.get_parameter_string(glow::RENDERER) };
        log::info!("runner GL: {gl_version} / {gl_renderer}");

        // 2. Precision probe, before the first swap: it must read the surface's
        //    actual back buffer, so it has to run on an untouched surface.
        let precision = unsafe { probe_surface_precision(&gl, width, height) };
        log::info!("surface {}", precision.summary());

        // 3. Colour description. Only attempted when the surface really is
        //    10-bit and the probe proved the precision: a PQ description on an
        //    8-bit surface is worse than no HDR at all.
        let mut output = OutputCaps {
            native_shell: true,
            surface_ten_bit: presenter.format().ten_bit,
            surface_bits: if presenter.format().ten_bit { 10 } else { 8 },
            precision_verified: precision.distinct,
            swap_interval: presenter.swap_interval(),
            physical_verified: false,
            ..OutputCaps::default()
        };

        let mut description = None;
        let mut color_mode = ColorMode::Sdr;

        let mut refusal: Option<String> = None;
        if !cfg.want_hdr {
            refusal = Some("HDR disabled by MUVRIX_NO_HDR".to_string());
        } else if !presenter.format().ten_bit {
            refusal = Some(format!(
                "surface is {}-bit (R{}G{}B{}A{}): 8-bit cannot carry PQ without visible banding",
                output.surface_bits,
                presenter.format().red,
                presenter.format().green,
                presenter.format().blue,
                presenter.format().alpha
            ));
        } else if !precision.distinct {
            refusal = Some(
                "precision probe could not separate adjacent 10-bit values on this surface"
                    .to_string(),
            );
        }

        // Attach the shared foreign-display manager on every Wayland path, not
        // only the HDR path: wp_presentation is needed for the production
        // acknowledgement even when the output is SDR.
        let mut color_manager = match unsafe { ColorManager::attach(display_ptr) } {
            Ok(cm) => Some(cm),
            Err(e) => {
                if refusal.is_none() {
                    refusal = Some(format!("wp_color_manager_v1 unavailable: {e}"));
                }
                None
            }
        };
        let mut presentation_feedback_available = false;
        if let Some(cm) = color_manager.as_mut() {
            presentation_feedback_available = cm.has_presentation_feedback();
            output.color_manager_pq_bt2020 = cm.supports_pq_bt2020();
            if refusal.is_none() {
                if output.color_manager_pq_bt2020 {
                    let peak = requested_peak_nits();
                    output.target_peak_nits = Some(peak.0);
                    output.peak_source = peak.1;
                    match cm.describe_pq_bt2020(surface_ptr, peak.0, UI_WHITE_NITS as u32) {
                        Ok(desc) => {
                            let identity = desc.identity();
                            output.description_accepted = true;
                            output.description_identity = Some(identity);
                            color_mode = ColorMode::Pq;
                            description = Some(desc);
                            log::info!(
                                "HDR output accepted by compositor: identity={identity} peak={} nits ({})",
                                peak.0,
                                peak.1
                            );
                        }
                        Err(e) => {
                            refusal = Some(format!("PQ description refused: {e}"));
                        }
                    }
                } else {
                    refusal = Some(format!(
                        "compositor does not advertise PQ + BT.2020 (manager v{}, tf={:?}, primaries={:?})",
                        cm.manager_version().unwrap_or(0),
                        cm.capabilities().transfer_functions,
                        cm.capabilities().primaries
                    ));
                }
            }

            // Do not attach an sRGB image description on the SDR fallback.
            // Some compositors expose wp_color_manager_v1 but reject the named
            // sRGB transfer function; a protocol error there would kill the
            // shared Wayland connection. The presentation-feedback object is
            // independent of the colour description and remains bound above.
            // HDR descriptions are still attempted only through the supported
            // PQ/BT.2020 feature gate.
        }

        output.native_hdr_available = color_mode == ColorMode::Pq;
        output.pipeline = if output.native_hdr_available {
            "pq-bt2020 (verified surface, compositor-accepted description)".to_string()
        } else {
            "sdr (engine tone-maps HDR sources)".to_string()
        };
        output.fallback_reason = if output.native_hdr_available {
            None
        } else {
            refusal
                .clone()
                .or_else(|| Some("HDR path not established".to_string()))
        };
        output.display = display_capabilities(&output, color_manager.as_ref());

        if let Some(reason) = &output.fallback_reason {
            log::warn!("HDR refused: {reason}");
        } else {
            log::info!("HDR path established (physical panel verification still outstanding)");
        }

        // 4. egui + app. The app is built last because it needs the final output
        //    capabilities to choose the engine's target and colour options.
        let egui_ctx = egui::Context::default();
        let max_texture_side = unsafe {
            gl.get_parameter_i32(glow::MAX_TEXTURE_SIZE)
                .clamp(2048, 16384) as usize
        };
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &*window,
            Some(window.scale_factor() as f32),
            None,
            Some(max_texture_side),
        );
        let painter = egui_glow::Painter::new(
            gl.clone(),
            "",
            Some(egui_glow::ShaderVersion::get(&gl)),
            false,
        )
        .map_err(|e| format!("egui_glow Painter: {e}"))?;

        let loader = |name: &std::ffi::CStr| -> *const std::ffi::c_void {
            presenter::get_proc_address(&name.to_string_lossy()) as *const std::ffi::c_void
        };
        let loader_ref: &dyn Fn(&std::ffi::CStr) -> *const std::ffi::c_void = &loader;
        let mut app = MuvrixApp::new_shell(
            gl.clone(),
            egui_ctx.clone(),
            Some(loader_ref),
            Some(muvrix_player::NativeDisplay::Wayland(display_ptr)),
            output.clone(),
        )
        .map_err(|e| format!("MuvrixApp init failed: {e}"))?;

        // The native runner owns report_swap timing through wp_presentation.
        // The render engine records a token instead of setting its single
        // pending flag; if feedback is unavailable the runner falls back after a
        // successful swap.
        app.enable_external_presentation_ack();

        crate::causal_trace::install_drop_watcher(app.controller.handle().clone());

        if let Some(path) = &cfg.initial_file {
            app.load_file(path);
        }

        // From here on the app owns live engine resources: any failure must tear
        // it down before returning, because dropping a render context while mpv
        // is still mid-setup aborts the process instead of unwinding.
        let mut ui_fbo = match unsafe { OffscreenFbo::new(&gl, width, height) } {
            Ok(fbo) => fbo,
            Err(e) => {
                app.shutdown(Some(&gl));
                return Err(format!("UI target: {e}"));
            }
        };
        let composite = match unsafe { Composite::new(&gl) } {
            Ok(c) => c,
            Err(e) => {
                unsafe { ui_fbo.destroy(&gl) };
                app.shutdown(Some(&gl));
                return Err(format!("composite pass: {e}"));
            }
        };

        // Route egui's repaint requests (raised by the engine's update callback
        // on mpv's threads) into winit's redraw scheduling.
        let wake = Arc::new(WakeSignal::new(proxy.clone()));
        let wake_for_callback = wake.clone();
        egui_ctx.set_request_repaint_callback(move |info| {
            let delay_us = info.delay.as_micros() as i64;
            crate::causal_trace::record(
                crate::causal_trace::Kind::EguiRepaintRequest,
                crate::causal_trace::last_callback(),
                delay_us,
                0,
                0,
                if info.delay.is_zero() {
                    "egui_immediate"
                } else {
                    "egui_delay"
                },
            );
            if info.delay.is_zero() {
                wake_for_callback.request_immediate_from("egui_immediate");
            } else {
                wake_for_callback
                    .request_deadline_from(Instant::now() + info.delay, "egui_deadline");
            }
        });

        // Wire the wake signal into the render engine so the no-new-frame path
        // can keep the event loop from parking indefinitely when mpv has no
        // frame to present (the 4K HDR stall).
        app.render_engine.lock().unwrap().set_wake(wake.clone());
        // Independent wake path for the mpv update callback. This does not go
        // through egui, so an egui repaint-deduplication state cannot strand the
        // loop (Phase 2.6 WP1 Run-01).
        app.install_wake(wake.clone() as Arc<dyn WakeHandle>);

        let started = Instant::now();
        let state_machine = PresentationStateMachine::new(started);
        let metrics = Arc::new(SharedMetrics::new(state_machine.snapshot()));
        metrics.set_presentation_feedback_available(presentation_feedback_available);
        // The emission thread starts here, not on the first paint: rows must
        // exist even if the very first render stalls.
        // Parsed once so the painter and the worker enforce the same deadline.
        let run_limit = std::env::var("MUVRIX_RUN_SECONDS")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .map(Duration::from_secs_f64);
        let telemetry = TelemetryWorker::spawn(
            metrics.clone(),
            TelemetryConfig::from_env(target_frame_interval(), started)
                .with_notify(wake.clone() as Arc<dyn crate::wake::WakeHandle>)
                .with_run_limit(run_limit)
                .with_stall_threshold(state_machine.stall_threshold()),
        );

        // Paint-independent mpv state observability. Opt-in so ordinary runs do
        // not write an extra file. The sampler is stopped in `destroy`, before
        // the mpv handle is torn down.
        let mpv_state = std::env::var("MUVRIX_MPV_STATE_CSV")
            .ok()
            .filter(|p| !p.trim().is_empty())
            .map(|p| MpvStateSampler::spawn(app.controller.handle().clone(), p, started));

        Ok(Self {
            window,
            presenter,
            gl,
            painter,
            egui_state,
            egui_ctx,
            app,
            test_sequence: std::env::var("MUVRIX_TEST_SEQUENCE")
                .ok()
                .map(|raw| {
                    raw.split('|')
                        .filter_map(|item| {
                            let (sec, path) = item.split_once(':')?;
                            Some((sec.parse().ok()?, path.to_string()))
                        })
                        .collect()
                })
                .unwrap_or_default(),
            next_sequence: 0,
            ui_fbo,
            composite,
            gpu: crate::gpu_timing::GpuTiming::new(),
            color: color_manager,
            description,
            color_mode,
            precision,
            output,
            metrics,
            telemetry,
            mpv_state,
            counters: Counters::default(),
            last_present_at: None,
            started,
            fault: FaultInjection::from_env(),
            stall_wake_armed: None,
            last_published_sec: 0,
            cached_stats: crate::app::PlaybackStats::default(),
            fed_update_callbacks: 0,
            state_machine,
            last_playback_state: None,
            wake,
            frame_count: 0,
            run_limit,
            redraw_gate: RedrawGate::default(),
            frame_callbacks_withheld: false,
            last_suspension_probe: None,
            exit_requested: false,
            occluded: false,
            destroyed: false,
            last_drawable: (0, 0),
            monitor_reported: false,
            presentation_ack: PresentationAckQueue::default(),
            last_frame_drop_total: None,
        })
    }

    fn draw(&mut self) {
        let now = Instant::now();
        // Process feedback that arrived since the previous iteration before the
        // next render decision. This is the only place a token is retired.
        self.drain_presentation_feedback();
        let draw_callback_seq = crate::causal_trace::last_callback();
        let draw_seq = crate::causal_trace::record(
            crate::causal_trace::Kind::DrawEnter,
            draw_callback_seq,
            self.frame_count as i64,
            0,
            0,
            "draw",
        );
        crate::causal_trace::set_last_draw(draw_seq);
        // Liveness beacon: if this stops advancing while mpv expects a swap, the
        // telemetry thread asks for a repaint so a parked loop cannot deadlock.
        self.metrics.note_paint();
        self.redraw_gate.complete();
        crate::causal_trace::record(
            crate::causal_trace::Kind::RedrawGateComplete,
            draw_seq,
            0,
            0,
            0,
            "draw",
        );
        let drawable = self.window.inner_size();
        if (drawable.width, drawable.height) != self.last_drawable {
            log::info!(
                "drawable: {}x{} physical pixels",
                drawable.width,
                drawable.height
            );
            self.last_drawable = (drawable.width, drawable.height);
        }
        if !self.monitor_reported {
            if let Some(monitor) = self.window.current_monitor() {
                log::info!(
                    "monitor: name={:?} refresh_millihertz={:?}",
                    monitor.name(),
                    monitor.refresh_rate_millihertz()
                );
                self.monitor_reported = true;
            }
        }
        if self.frame_callbacks_withheld {
            self.frame_callbacks_withheld = false;
            self.last_suspension_probe = None;
            // The probe exists only to discover this recovery. Drop any pending
            // probe deadline so it cannot cause one more pointless wake; the
            // normal draw-end deadline is re-armed below.
            self.wake.cancel_deadline();
            log::info!("compositor resumed delivering frame callbacks");
            crate::trace::record(
                crate::trace::Ev::StallRecovery,
                "frame_callbacks_resumed",
                0,
                0,
            );
            // A paint is happening again, so a frame is expected again; the next
            // successful swap moves it back to `presenting`.
            self.state_machine.on_event(StateEvent::FrameExpected, now);
            self.publish();
        }

        // The worker enforces the run deadline too. This check makes the painter
        // honour it on its next pass without re-parsing the limit.
        if !self.exit_requested && self.metrics.deadline_expired() {
            log::info!("run deadline already expired (worker); requesting clean exit");
            self.exit_requested = true;
        }

        self.sync_playback_state(now);
        self.feed_update_callbacks(now);

        // --- Test-only rendering interruption (Phase 2.4). While it is active
        //     no GL, no mpv render and no swap happens, but the process, the
        //     event loop and the telemetry thread keep running. That is the
        //     whole point: it isolates a *render* stall from a process stall.
        if let Some(end) = self.fault.stall_until(now, self.started) {
            // Self-wake for the end of the window, re-armed at most every
            // 500 ms. Requesting a redraw on every stalled paint re-enters this
            // function immediately and spins the loop at millions of events per
            // second — measured in the first Test D run (1,053,167 deadlines for
            // a 25 s run) — which starves the engine instead of measuring an idle
            // pipeline. One wake is not enough either: the compositor can drop
            // the redraw it causes, so the stall never ended.
            let rearm = self
                .stall_wake_armed
                .map(|at| now.saturating_duration_since(at) >= Duration::from_millis(500))
                .unwrap_or(true);
            if rearm {
                self.stall_wake_armed = Some(now);
                self.wake.request_deadline(end);
            }
            self.state_machine.tick(now);
            self.publish_tick();
            return;
        }
        self.stall_wake_armed = None;

        // The run limit is checked before any pipeline work: while stalled there
        // is no swap to reach the check further down, and a bounded run must
        // still terminate on time.
        if !self.exit_requested {
            if let Some(limit) = self.run_limit {
                if self.started.elapsed() >= limit {
                    log::info!("run limit reached ({limit:?}); requesting clean exit");
                    self.exit_requested = true;
                }
            }
        }
        if self.exit_requested {
            return;
        }

        if self.occluded {
            // Nothing is visible, nothing is presented; the engine keeps
            // decoding, and the first visible frame after un-occlusion is
            // presented normally. Occlusion is an *expected* absence, not a
            // stall and not a surface failure.
            self.state_machine
                .on_event(StateEvent::PresentationSuspended, Instant::now());
            self.presenter.mark_unavailable(Instant::now());
            return;
        }

        let size = self.window.inner_size();
        let (width, height) = (size.width.max(1), size.height.max(1));
        self.presenter.resize(width, height);

        if let Err(e) = self.presenter.make_current() {
            log::error!("make_current failed: {e}");
            return;
        }

        let raw_input = self.egui_state.take_egui_input(&self.window);
        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            self.app.frame_update(ctx);
        });
        self.egui_state
            .handle_platform_output(&self.window, full_output.platform_output);

        let ppp = full_output.pixels_per_point;
        let viewport_output = full_output.viewport_output.clone();
        let clipped = self.egui_ctx.tessellate(full_output.shapes, ppp);

        // --- UI pass: into our own RGBA8 target, video area left transparent.
        unsafe {
            self.ui_fbo.ensure_size(&self.gl, width, height);
        }
        let ui_fbo = self.ui_fbo.fbo();
        unsafe {
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(ui_fbo));
            self.gl.viewport(0, 0, width as i32, height as i32);
            self.gl.disable(glow::SCISSOR_TEST);
            self.gl.clear_color(0.0, 0.0, 0.0, 0.0);
            self.gl.clear(glow::COLOR_BUFFER_BIT);
        }
        self.painter.paint_and_update_textures(
            [width, height],
            ppp,
            &clipped,
            &full_output.textures_delta,
        );

        // The egui paint callback has now executed the engine's `paint()`. A
        // token exists only when libmpv actually rendered a new frame, which is
        // exactly when a presentation feedback object is meaningful.
        let rendered_token = self.app.take_presentation_token();

        // --- Composite pass: video + UI in one go, into the window surface.
        let (video_tex, rect) = self.app.video_output().unwrap_or((None, [0, 0, 0, 0]));
        unsafe {
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            self.gl.viewport(0, 0, width as i32, height as i32);
            self.gl.clear_color(0.0, 0.0, 0.0, 1.0);
            self.gl.clear(glow::COLOR_BUFFER_BIT);
            let composite_started = Instant::now();
            self.gpu.poll(&self.gl);
            let gpu_query = self.gpu.begin(&self.gl);
            self.composite.run(
                &self.gl,
                video_tex,
                self.ui_fbo.texture(),
                (width, height),
                rect,
                self.color_mode,
                UI_WHITE_NITS,
            );
            self.gpu.end(
                &self.gl,
                gpu_query,
                crate::gpu_timing::GpuStage::Composite,
                crate::causal_trace::last_draw(),
                composite_started.elapsed().as_micros() as i64,
            );
            crate::causal_trace::record(
                crate::causal_trace::Kind::Composite,
                crate::causal_trace::last_draw(),
                composite_started.elapsed().as_micros() as i64,
                width as i64,
                height as i64,
                "video_ui",
            );
            // Test-only, isolated final-window capture. This is a synchronous
            // readback and must never run in normal playback.
            if let Ok(capture_path) = std::env::var("MUVRIX_CAPTURE_WINDOW_PNG") {
                if self.frame_count == 61 {
                    let mut pixels = vec![0u8; (width * height * 4) as usize];
                    self.gl.read_pixels(
                        0,
                        0,
                        width as i32,
                        height as i32,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        glow::PixelPackData::Slice(Some(&mut pixels)),
                    );
                    let stride = (width * 4) as usize;
                    let mut flipped = Vec::with_capacity(pixels.len());
                    for row in pixels.chunks_exact(stride).rev() {
                        flipped.extend_from_slice(row);
                    }
                    if let Err(e) = image::save_buffer(
                        &capture_path,
                        &flipped,
                        width,
                        height,
                        image::ExtendedColorType::Rgba8,
                    ) {
                        log::error!("Failed to save final window capture: {e}");
                    } else {
                        log::info!("Captured final window frame to {capture_path}");
                    }
                }
            }
        }

        // --- WP4 chain: count frames the engine actually rendered into the
        //     working target this paint. The engine renders at most one per
        //     paint; compare cumulative counters rather than assuming one.
        if let Some(engine) = self.app.render_telemetry() {
            let mut pending = engine
                .mpv_renders
                .saturating_sub(self.state_machine.rendered_frames_total());
            while pending > 0 {
                self.state_machine
                    .on_event(StateEvent::FrameRendered, Instant::now());
                pending -= 1;
            }
        }

        // --- Present. Every event carries the instant it actually happened;
        //     sampling later is what makes the ages meaningful.
        //
        // The feedback object must be created for the commit before the EGL
        // swap. It belongs to the newly rendered token, not to every repaint.
        let feedback_requested = self.request_presentation_feedback(rendered_token);
        self.window.pre_present_notify();
        // Winit's pre_present_notify() is the wl_surface.frame request site on
        // Wayland. Record the request attempt; Winit suppresses duplicates while
        // a callback is outstanding.
        crate::causal_trace::record(
            crate::causal_trace::Kind::FrameCallbackRequest,
            crate::causal_trace::last_draw(),
            self.frame_count as i64,
            crate::causal_trace::monotonic_ns(),
            0,
            "pre_present",
        );
        crate::trace::record(crate::trace::Ev::SwapEnter, "egl", 0, 0);
        crate::causal_trace::record(
            crate::causal_trace::Kind::SwapEnter,
            crate::causal_trace::last_draw(),
            0,
            0,
            0,
            "egl",
        );
        let attempt_at = Instant::now();
        self.counters.egl_swap_attempts_total =
            self.counters.egl_swap_attempts_total.saturating_add(1);
        self.counters.fbo_render_calls_total =
            self.counters.fbo_render_calls_total.saturating_add(1);
        self.state_machine
            .on_event(StateEvent::SwapAttempted, attempt_at);
        let ok = self.presenter.swap();
        let swap_us = attempt_at.elapsed().as_micros() as i64;
        crate::trace::record(
            crate::trace::Ev::SwapExit,
            if ok { "ok" } else { "fail" },
            swap_us,
            0,
        );
        crate::causal_trace::record5(
            crate::causal_trace::Kind::SwapExit,
            crate::causal_trace::last_draw(),
            ok as i64,
            swap_us,
            0,
            0,
            crate::causal_trace::monotonic_ns(),
            if ok { "ok" } else { "fail" },
        );
        if let Some(token) = rendered_token {
            self.record_frame_deadline(token, draw_callback_seq, swap_us, ok);
        }
        if ok {
            // The swap is the strongest evidence available without a
            // presentation-feedback protocol: the buffer is now the
            // compositor's. A success also *is* the recovery signal, which the
            // state machine counts once if it was in an unavailable state.
            self.counters.egl_swap_successes_total =
                self.counters.egl_swap_successes_total.saturating_add(1);
            let presented_at = Instant::now();
            if let Some(previous) = self.last_present_at {
                self.metrics
                    .note_present_gap(presented_at.duration_since(previous).as_micros() as u64);
            }
            self.last_present_at = Some(presented_at);
            self.metrics
                .note_present(presented_at, self.presenter.stats().last_swap_us);
            if let Some(token) = rendered_token {
                if feedback_requested {
                    // The compositor will report actual presentation on the
                    // shared foreign queue. Do not acknowledge the EGL submit.
                    let actions = self.presentation_ack.enqueue(token, Instant::now());
                    self.apply_ack_actions(actions);
                } else {
                    // No wp_presentation: preserve the old post-swap fallback
                    // rather than let libmpv wait for an acknowledgement that
                    // cannot arrive.
                    self.app.report_swap_tokens(&[token]);
                }
            }
            self.presenter.mark_presented();
            self.state_machine
                .on_event(StateEvent::SwapSucceeded, presented_at);
        } else {
            // The frame was not flipped, so telling libmpv it was would corrupt
            // its timing. Retire its feedback object and token without reporting.
            self.counters.egl_swap_failures_total =
                self.counters.egl_swap_failures_total.saturating_add(1);
            if let Some(token) = rendered_token {
                if feedback_requested {
                    if let Some(cm) = self.color.as_mut() {
                        cm.discard_feedback(token);
                    }
                }
                let actions = self.presentation_ack.failed(token);
                self.apply_ack_actions(actions);
            }
            let device_lost = matches!(
                self.presenter.state(),
                crate::platform::presenter::PresentationState::DeviceLost
            );
            self.state_machine.on_event(
                if device_lost {
                    StateEvent::DeviceLost
                } else {
                    StateEvent::SwapFailed
                },
                Instant::now(),
            );
        }
        self.frame_count = self.frame_count.wrapping_add(1);

        // --- Publish for the telemetry thread. The CSV itself is written by
        //     that thread, so a later render stall cannot stop measurement.
        self.state_machine.tick(Instant::now());
        self.publish_tick();

        // egui and the engine both report when they next need a frame; the
        // event loop waits until then instead of polling.
        let repaint_after = viewport_output
            .values()
            .map(|v| v.repaint_delay)
            .min()
            .unwrap_or(Duration::from_millis(16));
        if repaint_after < Duration::from_millis(1000) {
            self.wake
                .request_deadline_from(Instant::now() + repaint_after, "draw_end");
        }
        for vp in viewport_output.values() {
            if !vp.commands.is_empty() {
                apply_commands(&self.window, &vp.commands);
            }
        }

        if let Some(limit) = self.run_limit {
            if self.started.elapsed() >= limit {
                log::info!("run limit reached ({limit:?}); requesting clean exit");
                self.exit_requested = true;
            }
        }

        // A/V timing, once per second. `avsync` is the engine's own measure;
        // it is not a substitute for measuring audio and video arrival times,
        // but it is the only sync evidence this runner can collect without
        // audio loopback instrumentation.
        if self.frame_count.is_multiple_of(60) {
            let stats = self.app.playback_stats();
            log::info!(
                "sync: avsync={:?}s display_fps={:?} vf_fps={:?} dropped={:?} decoder_dropped={:?}",
                stats.avsync.map(|v| (v * 1000.0).round() / 1000.0),
                stats.display_fps.map(|v| (v * 100.0).round() / 100.0),
                stats.estimated_vf_fps.map(|v| (v * 100.0).round() / 100.0),
                stats.dropped,
                stats.decoder_dropped
            );
        }
    }

    fn destroy(&mut self) {
        if self.destroyed {
            return;
        }
        self.destroyed = true;
        // Unpresented commits must never be reported. Retire their feedback
        // proxies and tokens; no report_swap() is called for a teardown drop.
        let pending = self.presentation_ack.clear();
        self.apply_ack_actions(pending);
        // Final publish before the summary so the worker's last sample and the
        // reconciliation block read the freshest snapshot. The reconciliation
        // delta itself is the work done after the last per-second row, which is
        // stated by the worker and proportional to the event rate.
        self.publish();
        log::info!("runner teardown: frames={}", self.frame_count);
        let stats = self.presenter.stats().clone();
        log::info!(
            "presentation summary: swaps={} failures={} avg_swap={}us max_swap={}us \
             slow_swaps(>50ms)={} longest_gap={}us unavailable_episodes={} unavailable_ms={} \
             precision={} mode={} peak={:?}nits description_identity={:?}",
            stats.swaps,
            stats.swap_failures,
            stats.avg_swap_us(),
            stats.max_swap_us,
            stats.slow_swaps_over_50ms,
            stats.longest_gap_us,
            stats.unavailable_episodes,
            stats.unavailable_ms_total,
            self.precision.summary(),
            self.color_mode.as_str(),
            self.output.target_peak_nits,
            self.output.description_identity
        );

        let (deadlines_set, deadlines_replaced, immediate_requests) = self.wake.counters();
        log::info!(
            "wake summary: deadlines_set={deadlines_set} replaced={deadlines_replaced} \
             immediate={immediate_requests} wakes_delivered={} redraw_requests={}",
            self.metrics.wake_events(),
            self.metrics.redraw_requests()
        );
        crate::trace::record(
            crate::trace::Ev::Teardown,
            "runner",
            self.metrics.wake_events() as i64,
            self.metrics.redraw_requests() as i64,
        );
        crate::trace::dump("runner_teardown");
        crate::causal_trace::record(
            crate::causal_trace::Kind::Teardown,
            crate::causal_trace::last_draw(),
            self.frame_count as i64,
            self.metrics.wake_events() as i64,
            0,
            "runner",
        );

        // Stop measuring before GL teardown: the worker must not sample a
        // half-destroyed runtime, and it must join cleanly with the process.
        self.telemetry.shutdown();
        log::info!(
            "telemetry worker: rows={} publish_skips={} (render thread never waited)",
            self.telemetry.rows_written(),
            self.metrics.publish_skips()
        );

        // Stop the paint-independent sampler before mpv is torn down. It holds
        // an `Arc<MpvHandle>`; joining here guarantees no property read can
        // race `mpv_terminate_destroy`.
        if let Some(mut sampler) = self.mpv_state.take() {
            sampler.shutdown();
        }

        // Stop the causal drop watcher and flush the trace before mpv/EGL
        // teardown. This is diagnostic-only; scheduling state is untouched.
        crate::causal_trace::shutdown();

        if let Err(e) = self.presenter.make_current() {
            log::warn!("make_current before teardown failed: {e}");
        }
        self.app.shutdown(Some(&self.gl));
        self.painter.destroy();
        unsafe {
            self.composite.destroy(&self.gl);
            self.ui_fbo.destroy(&self.gl);
        }
        // Description first (it references the surface), then the surface.
        drop(self.description.take());
        drop(self.color.take());
        self.presenter.destroy();
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        self.destroy();
    }
}

fn wayland_handles(
    window: &Window,
) -> Result<(*mut std::ffi::c_void, *mut std::ffi::c_void), String> {
    let display = window
        .display_handle()
        .map_err(|e| format!("no display handle: {e}"))?;
    let surface = window
        .window_handle()
        .map_err(|e| format!("no window handle: {e}"))?;
    match (display.as_raw(), surface.as_raw()) {
        (RawDisplayHandle::Wayland(d), RawWindowHandle::Wayland(s)) => {
            Ok((d.display.as_ptr(), s.surface.as_ptr()))
        }
        _ => Err("runner requires a Wayland session (WL_ or X11 session detected)".to_string()),
    }
}

/// Expected frame interval for this run, used for the exceedance buckets.
///
/// Derived from `MUVRIX_TARGET_FPS` (default 60) rather than guessed from the
/// engine, so the recorded value is a declaration and not circular with the
/// measurements it classifies.
fn target_frame_interval() -> Duration {
    let fps = std::env::var("MUVRIX_TARGET_FPS")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| *v > 0.0 && *v < 1000.0)
        .unwrap_or(60.0);
    Duration::from_secs_f64(1.0 / fps)
}

fn requested_peak_nits() -> (u32, &'static str) {
    if let Ok(value) = std::env::var("MUVRIX_PEAK_NITS") {
        if let Ok(parsed) = value.parse::<u32>() {
            if parsed >= 100 {
                return (parsed, "MUVRIX_PEAK_NITS");
            }
        }
        log::warn!("ignoring MUVRIX_PEAK_NITS={value} (expected >= 100)");
    }
    // 617 nits is what the Phase 1.9 POC measured as a safe peak for this
    // panel's HDR mode. It is a configuration value, not a measurement of this
    // session, which is why it is labelled in every report.
    (617, "phase-1.9 POC default")
}

fn display_capabilities(
    output: &OutputCaps,
    cm: Option<&ColorManager>,
) -> muvrix_player::DisplayCapabilities {
    let mut caps = muvrix_player::DisplayCapabilities {
        display_id: std::env::var("WAYLAND_DISPLAY").ok(),
        hdr_capable: Some(output.surface_ten_bit && output.precision_verified),
        hdr_enabled: Some(output.native_hdr_available),
        reference_white_nits: Some(UI_WHITE_NITS as f64),
        max_luminance_nits: output.target_peak_nits.map(|p| p as f64),
        primaries: if output.native_hdr_available {
            Some("bt2020".to_string())
        } else {
            Some("srgb".to_string())
        },
        ..Default::default()
    };
    if let Some(cm) = cm {
        caps.transfer_functions = cm
            .capabilities()
            .transfer_functions
            .iter()
            .map(|v| v.to_string())
            .collect();
    }
    caps.presentation_formats.push(match output.surface_bits {
        10 => format!(
            "EGL R10G10B10A2 (config-verified, probe={})",
            output.precision_verified
        ),
        _ => "EGL RGBA8888".to_string(),
    });
    caps.detection_status = if cm.is_some() {
        muvrix_player::CapabilityDetection::Reported
    } else {
        muvrix_player::CapabilityDetection::Unavailable {
            reason: "no wp_color_manager_v1 support advertised".to_string(),
        }
    };
    caps
}

/// Applies the viewport commands egui produced (eframe normally does this).
fn apply_commands(window: &Window, commands: &[egui::ViewportCommand]) {
    use egui::ViewportCommand;
    for cmd in commands {
        match cmd {
            ViewportCommand::Fullscreen(true) => {
                window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
            }
            ViewportCommand::Fullscreen(false) => window.set_fullscreen(None),
            ViewportCommand::Title(title) => window.set_title(title),
            ViewportCommand::InnerSize(size) => {
                let _ = window
                    .request_inner_size(winit::dpi::LogicalSize::new(size.x as f64, size.y as f64));
            }
            ViewportCommand::Visible(visible) => window.set_visible(*visible),
            ViewportCommand::Decorations(decorations) => window.set_decorations(*decorations),
            ViewportCommand::WindowLevel(level) => {
                let level = match level {
                    egui::WindowLevel::AlwaysOnTop => winit::window::WindowLevel::AlwaysOnTop,
                    egui::WindowLevel::AlwaysOnBottom => winit::window::WindowLevel::AlwaysOnBottom,
                    egui::WindowLevel::Normal => winit::window::WindowLevel::Normal,
                };
                window.set_window_level(level);
            }
            _ => {}
        }
    }
}

/// Events the runner accepts from other threads.
#[derive(Debug, Clone, Copy)]
enum RunnerEvent {
    /// Something asked for a frame (mpv update callback, or an egui deadline).
    Wake,
}

struct Runner {
    cfg: Option<RunnerConfig>,
    shell: Option<Shell>,
    proxy: EventLoopProxy<RunnerEvent>,
}

impl ApplicationHandler<RunnerEvent> for Runner {
    /// A wake from another thread: honour it immediately.
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: RunnerEvent) {
        match event {
            RunnerEvent::Wake => {
                // A Wake is only sent when something wants a repaint (immediate
                // or deadline-based). Honour it unconditionally: the deadline
                // itself is already armed by `about_to_wait` via `WaitUntil` when
                // it is in the future, so a redraw here is safe and does not spin.
                log::trace!("Runner: Wake event received");
                crate::trace::record(crate::trace::Ev::WakeDelivered, "user_event", 0, 0);
                crate::causal_trace::record(
                    crate::causal_trace::Kind::UserEvent,
                    crate::causal_trace::last_callback(),
                    0,
                    0,
                    0,
                    "wake",
                );
                if let Some(shell) = self.shell.as_mut() {
                    shell.note_wake();
                    shell.request_redraw();
                }
            }
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.shell.is_some() {
            return;
        }
        let Some(cfg) = self.cfg.take() else {
            return;
        };
        match Shell::build(event_loop, &self.proxy, &cfg) {
            Ok(mut shell) => {
                shell.request_redraw();
                self.shell = Some(shell);
            }
            Err(e) => {
                log::error!("cannot start native presentation: {e}");
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(shell) = self.shell.as_mut() else {
            return;
        };
        if window_id != shell.window.id() {
            return;
        }

        let response = shell.egui_state.on_window_event(&shell.window, &event);
        // While the injected stall is open, egui still reports `repaint` (mpv's
        // update callback keeps calling `request_repaint`), and honouring it here
        // re-enters `draw` immediately: 152,577 paints/s were measured in Test D.
        // The work is deliberately paused, so the repaint is dropped; exactly one
        // wake at the end of the window resumes the loop.
        if response.repaint && !shell.fault.is_stalled() {
            shell.request_redraw();
        }

        match event {
            WindowEvent::CloseRequested => {
                if let Some(mut shell) = self.shell.take() {
                    shell.destroy();
                }
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if size.width > 0 && size.height > 0 {
                    shell.presenter.resize(size.width, size.height);
                    shell.request_redraw();
                }
            }
            WindowEvent::Occluded(occluded) => {
                log::info!("window occluded={occluded}");
                shell.occluded = occluded;
                if !occluded {
                    shell.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                crate::trace::record(crate::trace::Ev::DrawEnter, "redraw", 0, 0);
                crate::causal_trace::record(
                    crate::causal_trace::Kind::RedrawRequested,
                    crate::causal_trace::last_callback(),
                    crate::causal_trace::monotonic_ns(),
                    0,
                    0,
                    "winit",
                );
                shell.draw();
                crate::trace::record(
                    crate::trace::Ev::DrawExit,
                    "redraw",
                    shell.frame_count as i64,
                    0,
                );
                crate::causal_trace::record(
                    crate::causal_trace::Kind::DrawExit,
                    crate::causal_trace::last_draw(),
                    shell.frame_count as i64,
                    0,
                    0,
                    "redraw",
                );
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                shell.request_redraw();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // No busy loop. A frame is requested when egui asks for one (the engine
        // raises that from mpv's update callback) or when a deadline the engine
        // computed for its next frame passes.
        if let Some(shell) = self.shell.as_mut() {
            shell.poll_test_sequence();
        }
        crate::trace::record(crate::trace::Ev::AboutToWait, "enter", 0, 0);
        crate::causal_trace::record(
            crate::causal_trace::Kind::AboutToWaitEnter,
            crate::causal_trace::last_callback(),
            0,
            0,
            0,
            "loop",
        );

        // The run deadline is enforced by the telemetry thread as well as by the
        // painter, so a run with no paints still terminates here (Part 7).
        if self
            .shell
            .as_ref()
            .map(|s| s.metrics.deadline_expired())
            .unwrap_or(false)
        {
            if let Some(mut shell) = self.shell.take() {
                log::info!("run deadline enforced by the event loop; tearing down");
                shell.destroy();
            }
            event_loop.exit();
            return;
        }

        if self.shell.is_none() {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        }
        if self
            .shell
            .as_ref()
            .map(|s| s.exit_requested)
            .unwrap_or(false)
        {
            if let Some(mut shell) = self.shell.take() {
                shell.destroy();
            }
            event_loop.exit();
            return;
        }
        let Some(shell) = self.shell.as_mut() else {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        };

        // Presentation feedback arrives on the shared foreign queue after the
        // compositor presents a committed buffer. Drain it here as well as at
        // draw entry so the acknowledgement does not wait for a paint.
        shell.drain_presentation_feedback();

        // The update-callback bridge latches at most one wake per loop
        // iteration. Clear it here, not in a paint: a paint is exactly what does
        // not happen while the compositor withholds frame callbacks.
        shell.app.clear_update_wake();

        let now = Instant::now();
        shell.note_frame_withholding(now);
        let (immediate, deadline) = shell.wake.take_due(now);
        if immediate || matches!(deadline, Some(at) if at <= now) {
            shell.request_redraw();
        }

        // Arm the next probe **after** `take_due`: a due probe deadline was
        // just consumed, so this is the only point at which the next future
        // deadline can be stored without being merged behind the due one and
        // then cleared with it. While withholding, this keeps the loop waking
        // even though the compositor sends no frame callbacks.
        arm_suspension_probe(&shell.wake, shell.frame_callbacks_withheld, now);
        shell.arm_presentation_ack_watchdog();

        match shell.wake.peek_deadline() {
            Some(at) => {
                crate::causal_trace::record(
                    crate::causal_trace::Kind::ControlFlow,
                    crate::causal_trace::last_callback(),
                    at.saturating_duration_since(now).as_micros() as i64,
                    0,
                    0,
                    "wait_until",
                );
                event_loop.set_control_flow(ControlFlow::WaitUntil(at))
            }
            None => {
                crate::causal_trace::record(
                    crate::causal_trace::Kind::ControlFlow,
                    crate::causal_trace::last_callback(),
                    0,
                    0,
                    0,
                    "wait",
                );
                event_loop.set_control_flow(ControlFlow::Wait)
            }
        }
        crate::causal_trace::record(
            crate::causal_trace::Kind::AboutToWaitExit,
            crate::causal_trace::last_callback(),
            0,
            0,
            0,
            "loop",
        );
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(mut shell) = self.shell.take() {
            shell.destroy();
        }
    }
}

/// Entry point used by `main`.
pub fn run(cfg: RunnerConfig) -> Result<(), String> {
    let event_loop = EventLoop::<RunnerEvent>::with_user_event()
        .build()
        .map_err(|e| format!("event loop: {e}"))?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut runner = Runner {
        cfg: Some(cfg),
        shell: None,
        proxy: event_loop.create_proxy(),
    };
    event_loop
        .run_app(&mut runner)
        .map_err(|e| format!("event loop run: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wake_signal_trait_forwarding_does_not_recurse() {
        // If `impl WakeHandle` accidentally called `self.set_deadline(..)` it
        // would resolve to the trait method and overflow the stack here.
        let signal = WakeSignal::default();
        let deadline = Instant::now() + Duration::from_millis(33);
        WakeHandle::set_deadline(&signal, deadline);
        assert_eq!(signal.peek_deadline(), Some(deadline));
    }

    #[test]
    fn wake_signal_keeps_the_earliest_deadline() {
        let signal = WakeSignal::default();
        let now = Instant::now();
        let later = now + Duration::from_millis(50);
        let sooner = now + Duration::from_millis(10);
        WakeHandle::set_deadline(&signal, later);
        WakeHandle::set_deadline(&signal, sooner);
        assert_eq!(signal.peek_deadline(), Some(sooner));
    }

    #[test]
    fn wake_signal_is_usable_as_a_trait_object() {
        let signal: Arc<dyn WakeHandle> = Arc::new(WakeSignal::default());
        let deadline = Instant::now() + Duration::from_millis(5);
        signal.set_deadline(deadline);
    }

    /// The independent mpv callback wake must not replace a pacing deadline.
    #[test]
    fn wake_now_uses_the_immediate_flag_and_keeps_the_pending_deadline() {
        let signal = WakeSignal::default();
        let now = Instant::now();
        let pacing = now + Duration::from_millis(16);
        signal.request_deadline(pacing);

        WakeHandle::wake_now(&signal);
        assert_eq!(
            signal.peek_deadline(),
            Some(pacing),
            "an immediate wake must not clobber the future pacing deadline"
        );

        let (immediate, seen) = signal.take_due(now);
        assert!(immediate, "wake_now must surface as an immediate request");
        assert_eq!(seen, Some(pacing));
        assert_eq!(signal.peek_deadline(), Some(pacing));
    }

    /// Regression for the Phase 2.2/2.3 wake bug: an unrelated wake must not
    /// disarm a future deadline, or the loop parks and the pipeline never
    /// resumes (Test D reproduced this exactly).
    #[test]
    fn a_future_deadline_survives_an_unrelated_wake_and_fires_once_due() {
        let signal = WakeSignal::default();
        let now = Instant::now();
        let deadline = now + Duration::from_millis(33);
        signal.request_deadline(deadline);

        // Some other wake arrives first (egui repaint, window event, ...).
        let (immediate, seen) = signal.take_due(now);
        assert!(!immediate);
        assert_eq!(seen, Some(deadline));
        assert_eq!(
            signal.peek_deadline(),
            Some(deadline),
            "a future deadline must stay armed after take_due"
        );

        // When the deadline is due it is handed out and cleared, so the loop
        // cannot spin on a stale instant.
        let (_, due) = signal.take_due(deadline);
        assert_eq!(due, Some(deadline));
        assert_eq!(signal.peek_deadline(), None);
    }

    #[test]
    fn an_immediate_request_does_not_clear_a_pending_deadline() {
        let signal = WakeSignal::default();
        let now = Instant::now();
        let deadline = now + Duration::from_millis(16);
        signal.request_deadline(deadline);
        signal.request_immediate();

        let (immediate, seen) = signal.take_due(now);
        assert!(immediate);
        assert_eq!(seen, Some(deadline));
        assert_eq!(signal.peek_deadline(), Some(deadline));
    }

    #[test]
    fn target_frame_interval_defaults_to_60hz() {
        // The env var is process-global; only assert the invariant that the
        // derived interval is positive and sane for whatever value is set.
        let d = target_frame_interval();
        assert!(d > Duration::ZERO);
        assert!(d <= Duration::from_secs(1));
    }

    // --- Phase 2.6 WP1 recovery: independent suspension probe ---------------

    #[test]
    fn withholding_arms_a_future_independent_deadline() {
        let signal = WakeSignal::default();
        let now = Instant::now();

        assert!(arm_suspension_probe(&signal, true, now));
        let armed = signal
            .peek_deadline()
            .expect("withholding must arm a deadline");
        assert_eq!(armed, now + SUSPENSION_PROBE_INTERVAL);
        assert!(armed > now, "the probe must never be a zero-delay spin");
    }

    #[test]
    fn persistent_withholding_rearms_after_the_probe_fires() {
        let signal = WakeSignal::default();
        let t0 = Instant::now();
        assert!(arm_suspension_probe(&signal, true, t0));

        // Model `about_to_wait`: consume the due probe, then arm the next one.
        let probe = t0 + SUSPENSION_PROBE_INTERVAL;
        let (immediate, due) = signal.take_due(probe);
        assert!(!immediate);
        assert_eq!(due, Some(probe));
        assert_eq!(
            signal.peek_deadline(),
            None,
            "the due deadline was consumed"
        );

        assert!(arm_suspension_probe(&signal, true, probe));
        assert_eq!(
            signal.peek_deadline(),
            Some(probe + SUSPENSION_PROBE_INTERVAL),
            "a persistent suspension must get a next probe"
        );
    }

    #[test]
    fn recovery_stops_probe_scheduling_and_cancels_a_pending_deadline() {
        let signal = WakeSignal::default();
        let now = Instant::now();
        assert!(arm_suspension_probe(&signal, true, now));
        assert!(signal.peek_deadline().is_some());

        // `draw` observes a real paint, so it cancels the probe and stops
        // scheduling. A later normal deadline can still be armed by the paint.
        signal.cancel_deadline();
        assert!(!arm_suspension_probe(&signal, false, now));
        assert_eq!(signal.peek_deadline(), None);
    }

    #[test]
    fn repeated_probe_arms_do_not_accumulate_or_go_zero_delay() {
        let signal = WakeSignal::default();
        let t0 = Instant::now();
        for i in 0..100u64 {
            arm_suspension_probe(&signal, true, t0 + Duration::from_millis(i));
        }
        let only = signal
            .peek_deadline()
            .expect("one probe deadline must remain");
        assert_eq!(
            only,
            t0 + SUSPENSION_PROBE_INTERVAL,
            "re-arms must keep the earliest future deadline, not queue or spin"
        );
        assert!(only > t0);
    }

    /// Regression for recovery-fix-01: arming from every `about_to_wait` must
    /// not nudge the proxy again while a deadline is already pending, or the
    /// loop feeds itself (34.4M deadlines in a 90 s run).
    #[test]
    fn an_already_armed_probe_does_not_request_again() {
        let signal = WakeSignal::default();
        let t0 = Instant::now();
        assert!(arm_suspension_probe(&signal, true, t0));
        let (set_after_first, _, _) = signal.counters();

        for i in 0..1_000u64 {
            arm_suspension_probe(&signal, true, t0 + Duration::from_micros(i));
        }
        let (set_after_many, _, _) = signal.counters();

        assert_eq!(
            set_after_first, set_after_many,
            "an already-armed probe must not request another wake"
        );
        assert_eq!(
            signal.peek_deadline(),
            Some(t0 + SUSPENSION_PROBE_INTERVAL),
            "the original future deadline must be retained"
        );
    }

    #[test]
    fn redraw_gate_allows_one_outstanding_request_and_a_bounded_retry() {
        let mut gate = RedrawGate::default();
        let t0 = Instant::now();
        let retry = SUSPENSION_PROBE_INTERVAL;

        assert!(gate.request(t0, retry));
        assert_eq!(gate.pending_since(), Some(t0));
        assert!(
            !gate.request(t0 + Duration::from_millis(100), retry),
            "a second request while one is outstanding must be coalesced"
        );
        assert_eq!(gate.pending_since(), Some(t0));

        assert!(
            gate.request(t0 + retry, retry),
            "the probe interval must permit a retry"
        );
        assert_eq!(
            gate.pending_since(),
            Some(t0),
            "a retry must not lose the original withholding age"
        );

        gate.complete();
        assert_eq!(gate.pending_since(), None);
        assert!(
            gate.request(t0 + retry + Duration::from_millis(1), retry),
            "after a paint the next request is immediate"
        );
    }

    #[test]
    fn an_immediate_wake_keeps_the_probe_deadline_visible_to_control_flow() {
        // Mirrors `about_to_wait`: an immediate request must not lose a future
        // deadline, and the probe is armed after `take_due`.
        let signal = WakeSignal::default();
        let now = Instant::now();
        let draw_deadline = now + Duration::from_millis(16);
        signal.request_deadline(draw_deadline);
        signal.request_immediate();

        let (immediate, seen) = signal.take_due(now);
        assert!(immediate);
        assert_eq!(seen, Some(draw_deadline));
        arm_suspension_probe(&signal, true, now);
        assert_eq!(
            signal.peek_deadline(),
            Some(draw_deadline),
            "the earlier normal deadline must win over the 500 ms probe"
        );
    }
}
