//! Rendering engine coordinating libmpv OpenGL playback and quad presentation.
//!
//! # Frame pacing
//!
//! The embedded render API has three moving parts that must agree:
//!
//! * `MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME` — whether `render` may block until
//!   the frame's target display time.
//! * `MPV_RENDER_PARAM_NEXT_FRAME_INFO` — the absolute time the frame should be
//!   displayed, so the host can wait itself instead of blocking the UI thread.
//! * `mpv_render_context_report_swap()` — tells the engine a frame reached the
//!   screen, which is what its timing code calibrates against.
//!
//! [`PacingMode`] selects which combination is used, and [`SwapReportMode`] which
//! swap-reporting policy is used; both are selectable at runtime so they can be
//! measured against each other.

use glow::HasContext as _;
use muvrix_player::MpvRenderContext;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::fbo::OffscreenFbo;
use crate::quad::QuadRenderer;
use crate::wake::{schedule_no_frame_fallback, WakeHandle};

/// How the render loop paces frames against the engine's target display times.
///
/// `Unpaced` was removed in Phase 2.0. It rendered on every paint callback
/// regardless of whether a new engine frame existed, which duplicated render
/// work (and, in copy-back mode, re-uploaded the same frame) so that the paint
/// rate — not the media rate — decided how often the engine was driven. The
/// runner drives paints from the compositor's frame callbacks, so an unpaced
/// mode there is both wasteful and semantically wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacingMode {
    /// Block inside `mpv_render_context_render` until the target time
    /// (`block_for_target_time = 1`). Diagnostic only: blocks the caller.
    Blocking,
    /// Never block: take the frame's target time from `NEXT_FRAME_INFO`, render
    /// only when a new frame exists, and schedule the next paint near the target.
    /// This is the production mode.
    Scheduled,
}

impl PacingMode {
    /// Reads `MUVRIX_PACING` (`blocking` | `scheduled`).
    pub fn from_env() -> Self {
        Self::from_value(&std::env::var("MUVRIX_PACING").unwrap_or_default())
    }

    fn from_value(value: &str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "blocking" => Self::Blocking,
            _ => Self::Scheduled,
        }
    }

    fn blocks(&self) -> bool {
        matches!(self, Self::Blocking)
    }

    /// Production pacing.
    pub fn default_mode() -> Self {
        Self::Scheduled
    }
}

#[cfg(test)]
mod pacing_tests {
    use super::*;

    #[test]
    fn unpaced_is_not_a_valid_pacing_mode() {
        // Phase 2.0 removed `Unpaced`; an old environment value must fall back
        // to the scheduled production mode, not resurrect unpaced rendering.
        assert_eq!(PacingMode::from_value("unpaced"), PacingMode::Scheduled);
        assert_eq!(PacingMode::from_value("scheduled"), PacingMode::Scheduled);
        assert_eq!(PacingMode::from_value("blocking"), PacingMode::Blocking);
    }

    #[test]
    fn nanosecond_target_time_is_normalized_to_microseconds() {
        // Real values measured from the libmpv 0.42 build MUVRIX links: the
        // target sits ~1000x above the engine clock and advances by one frame
        // period in nanoseconds.
        let now_us = 11_181_879;
        let target_ns = 11_157_579_500;
        let got = normalize_target_time(target_ns, Some(now_us));
        assert_eq!(got, 11_157_579, "ns target must be scaled into us");
        let delta = got - now_us;
        assert!(
            delta.abs() < 1_000_000,
            "delta {delta} us is not a sane lead"
        );
    }

    #[test]
    fn microsecond_target_time_is_left_alone() {
        // A build that follows the documented contract reports microseconds.
        let now_us = Some(11_181_879);
        let target_us = 11_157_579;
        assert_eq!(normalize_target_time(target_us, now_us), target_us);
    }

    #[test]
    fn normalization_is_a_no_op_without_a_clock_base() {
        assert_eq!(normalize_target_time(11_157_579_500, None), 11_157_579_500);
    }
}

/// Construction options that differ between the eframe shell and the native runner.
#[derive(Debug, Clone, Copy)]
pub struct EngineOptions {
    /// Internal format for the engine's working target (`GL_RGBA8` or `GL_RGBA16F`).
    pub internal_format: u32,
    /// When true, `paint` renders into the working target but does not blit it to
    /// the current framebuffer: the shell composites video and UI in one pass
    /// (`platform::Composite`) so both are blended in the correct domain.
    pub deferred_composite: bool,
}

impl Default for EngineOptions {
    fn default() -> Self {
        Self {
            internal_format: glow::RGBA8,
            deferred_composite: false,
        }
    }
}

/// When the engine is told that a frame was presented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapReportMode {
    /// Never report (control; the engine's timing degrades and it may log
    /// "render() not being called or stuck").
    None,
    /// Report immediately after rendering into the intermediate FBO, i.e. before
    /// eframe has presented. This was the previous production behaviour.
    Immediate,
    /// Acknowledge the *previous* frame at the start of the next paint callback,
    /// so the report lands after the frame actually reached the screen.
    Deferred,
}

impl SwapReportMode {
    /// Reads `MUVRIX_SWAP_REPORT` (`none` | `immediate` | `deferred`).
    pub fn from_env() -> Self {
        match std::env::var("MUVRIX_SWAP_REPORT")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "none" => Self::None,
            "immediate" => Self::Immediate,
            "deferred" => Self::Deferred,
            _ => Self::Deferred,
        }
    }
}

/// Histogram buckets for the duration of one `mpv_render_context_render` call
/// on the UI thread. Upper bounds: 1, 2, 4, 8, 16, 33, 66, ∞ ms.
pub const BLOCK_BUCKETS_US: [u64; 7] = [1_000, 2_000, 4_000, 8_000, 16_000, 33_000, 66_000];

/// Counters and timings exposed for telemetry.
#[derive(Debug, Clone, Default)]
pub struct RenderTelemetry {
    pub pain_count: u64,
    pub mpv_renders: u64,
    pub mpv_skips: u64,
    pub swap_reports: u64,
    /// No-frame fallback deadlines scheduled through the wake handle (WP5).
    pub no_frame_fallback_deadlines: u64,
    pub last_target_time_us: Option<i64>,
    pub last_render_block_us: u64,
    pub max_render_block_us: u64,
    pub render_block_us_total: u64,
    pub render_block_hist: [u64; 8],
}

/// Coordinates libmpv off-screen rendering and compositing into the egui glow pipeline.
pub struct RenderEngine {
    mpv_render_ctx: MpvRenderContext,
    fbo: OffscreenFbo,
    /// The GL3.3 blit quad. `None` when the shell composites, because the quad
    /// is never drawn there and its GLSL 3.30 shader is invalid in a GLES
    /// context (Mesa crashes while reporting that link error).
    quad: Option<QuadRenderer>,
    frame_count: u64,
    pacing: PacingMode,
    swap_report: SwapReportMode,
    pending_swap: bool,
    /// Native Wayland runner owns `report_swap()` timing through wp_presentation.
    external_swap_ack: bool,
    /// Token for the most recent frame that actually rendered. The runner
    /// consumes this after `paint()` and maps it to wp_presentation feedback.
    last_rendered_token: Option<u64>,
    egui_ctx: Option<egui::Context>,
    /// `(Instant::now(), mpv clock)` pair used to convert between the monotonic
    /// clock and the engine's microsecond time base without touching the mpv
    /// handle from the render path.
    clock_base: Option<(Instant, i64)>,
    /// Set by the engine's update callback when it has work; consumed by the
    /// next paint. This is what makes the loop callback-driven rather than
    /// timer-driven, without re-rendering the same frame on every vsync.
    frame_signal: Option<Arc<AtomicBool>>,
    telemetry: RenderTelemetry,
    options: EngineOptions,
    /// Handle to the event-loop wake signal, used to keep the loop from parking
    /// indefinitely when mpv has no frame to present.
    ///
    /// Platform-neutral by design (WP5): the engine knows only
    /// [`WakeHandle`], never the Linux `WakeSignal`. Set after construction by
    /// the shell via [`set_wake`]; defaults to `None` until the engine is wired
    /// into a running event loop.
    wake: Option<Arc<dyn WakeHandle>>,
    /// The viewport rectangle the engine last rendered into, in window pixels
    /// with a bottom-left origin. Consumed by the shell's composite pass.
    last_video_rect: [i32; 4],
    gpu: crate::gpu_timing::GpuTiming,
}

/// Normalizes the engine's `target_time` into the microsecond clock used by
/// [`RenderEngine::now_us`].
///
/// `mpv/render.h` documents `target_time` as "the same unit and base as
/// `mpv_get_time_us()`", i.e. microseconds. The libmpv 0.42 build this
/// application links reports **nanoseconds** instead: measured on this machine,
/// consecutive frames advance the value by 16,666,685 on 60 fps content and by
/// 42,000,000 on 23.976 fps content — exactly one frame period in ns — and it
/// sits ~1000x above `mpv_get_time_us()` at the same instant.
///
/// Treating those as microseconds made `delta_us` ~1000x too large, so the
/// computed wait always hit its 50 ms cap. At 60 fps that is three frame
/// periods, so the paced wake fired after the frame it was meant to present was
/// already due; at 24 fps a 50 ms cap is only 1.2 frame periods, which is why
/// the defect surfaced only at 60 fps.
///
/// The unit is derived from the value rather than assumed, so a build that
/// follows the documented contract keeps working: a genuine target is at most a
/// few hundred milliseconds beyond `now`, so a gap above one second can only mean
/// the engine reported nanoseconds.
pub(crate) fn normalize_target_time(target_time: i64, now_us: Option<i64>) -> i64 {
    const MAX_PLAUSIBLE_LEAD_US: i64 = 1_000_000;
    match now_us {
        Some(now) if target_time - now > MAX_PLAUSIBLE_LEAD_US => target_time / 1_000,
        _ => target_time,
    }
}

impl RenderEngine {
    /// Creates a new `RenderEngine` instance.
    ///
    /// # Safety
    ///
    /// Must be called with the OpenGL context current on the calling thread.
    pub unsafe fn new(
        gl: &glow::Context,
        mpv_render_ctx: MpvRenderContext,
        initial_width: u32,
        initial_height: u32,
    ) -> Result<Self, String> {
        Self::new_with_options(
            gl,
            mpv_render_ctx,
            initial_width,
            initial_height,
            EngineOptions::default(),
        )
    }

    /// Creates the engine with explicit target format / compositing behaviour.
    ///
    /// # Safety
    ///
    /// Must be called with the OpenGL context current on the calling thread.
    pub unsafe fn new_with_options(
        gl: &glow::Context,
        mpv_render_ctx: MpvRenderContext,
        initial_width: u32,
        initial_height: u32,
        options: EngineOptions,
    ) -> Result<Self, String> {
        let fbo = OffscreenFbo::new_with_format(
            gl,
            initial_width,
            initial_height,
            options.internal_format as i32,
        )?;
        let quad = if options.deferred_composite {
            None
        } else {
            Some(QuadRenderer::new(gl)?)
        };

        Ok(Self {
            mpv_render_ctx,
            fbo,
            quad,
            frame_count: 0,
            pacing: PacingMode::from_env(),
            swap_report: SwapReportMode::from_env(),
            pending_swap: false,
            external_swap_ack: false,
            last_rendered_token: None,
            egui_ctx: None,
            clock_base: None,
            frame_signal: None,
            telemetry: RenderTelemetry::default(),
            options,
            last_video_rect: [0, 0, 0, 0],
            gpu: crate::gpu_timing::GpuTiming::new(),
            wake: None,
        })
    }

    /// Video texture of the engine's working target (composite pass input).
    pub fn video_texture(&self) -> glow::Texture {
        self.fbo.texture()
    }

    /// Viewport rectangle of the last rendered frame, window pixels, bottom-left origin.
    pub fn last_video_rect(&self) -> [i32; 4] {
        self.last_video_rect
    }

    /// True when the working target is a floating-point format.
    pub fn working_target_is_float(&self) -> bool {
        self.fbo.internal_format() == glow::RGBA16F as i32
            || self.fbo.internal_format() == glow::RGBA32F as i32
    }

    /// Enables external acknowledgement ownership (native Wayland runner).
    ///
    /// In this mode `paint()` records a token but does not set the single
    /// `pending_swap` flag; the runner enqueues the token only after a successful
    /// swap and calls [`Self::report_swap_tokens`] when wp_presentation reports
    /// actual presentation.
    pub fn set_external_swap_ack(&mut self, enabled: bool) {
        self.external_swap_ack = enabled;
        if enabled {
            self.pending_swap = false;
        }
    }

    pub fn external_swap_ack(&self) -> bool {
        self.external_swap_ack
    }

    /// Takes the token for the most recent newly rendered frame, if any.
    ///
    /// A paint that only recomposited an existing frame returns `None`, so the
    /// runner never requests presentation feedback for a non-frame commit.
    pub fn take_rendered_token(&mut self) -> Option<u64> {
        self.last_rendered_token.take()
    }

    /// Acknowledges one or more frames that a compositor presentation feedback
    /// confirmed. Each token maps to exactly one `report_swap()` call.
    pub fn report_swap_tokens(&mut self, tokens: &[u64]) {
        for &token in tokens {
            self.mpv_render_ctx.report_swap();
            self.telemetry.swap_reports += 1;
            crate::trace::record(crate::trace::Ev::ReportSwap, "presentation_feedback", 0, 0);
            crate::causal_trace::record(
                crate::causal_trace::Kind::ReportSwap,
                token,
                1,
                0,
                0,
                "after_presentation",
            );
        }
    }

    /// Acknowledges the frame that was presented by the shell's swap.
    ///
    /// Called *after* `eglSwapBuffers`, which is what makes the acknowledgement
    /// mean "handed to the compositor" rather than "drawn into an FBO".
    pub fn report_presented(&mut self) {
        if self.pending_swap {
            crate::trace::record(crate::trace::Ev::ReportSwap, "deferred", 0, 0);
            self.mpv_render_ctx.report_swap();
            self.telemetry.swap_reports += 1;
            self.pending_swap = false;
            crate::causal_trace::record(
                crate::causal_trace::Kind::ReportSwap,
                crate::causal_trace::last_draw(),
                1,
                0,
                0,
                "after_successful_swap",
            );
        }
    }

    /// Drops a queued swap acknowledgement without reporting it.
    ///
    /// Used when `eglSwapBuffers` failed: the frame was never flipped, so
    /// reporting it on a later successful swap would attribute an old frame to a
    /// newer display instant. The API is explicit that inconsistent use of
    /// `mpv_render_context_report_swap` degrades playback, so a frame that was
    /// rendered but not shown is better dropped than mis-timed.
    pub fn discard_pending_swap(&mut self) {
        self.pending_swap = false;
    }

    /// Provides the egui context so paced rendering can schedule the next wake-up.
    pub fn set_egui_context(&mut self, ctx: egui::Context) {
        self.egui_ctx = Some(ctx);
    }

    /// Provides the event-loop wake signal so the no-new-frame path can keep the
    /// loop from parking indefinitely when mpv has no frame to present.
    ///
    /// Accepts any platform implementation of [`WakeHandle`]; on Linux the
    /// runner passes its `WakeSignal`.
    pub fn set_wake(&mut self, wake: Arc<dyn WakeHandle>) {
        self.wake = Some(wake);
    }

    /// True when a wake handle is configured (headless/test engines have none).
    pub fn has_wake_handle(&self) -> bool {
        self.wake.is_some()
    }

    /// Registers the flag raised by the engine update callback.
    ///
    /// Without it, deciding whether to render from `mpv_render_context_update()`
    /// alone races the decoder: at 60 fps the flag can be clear at the instant a
    /// paint callback runs, the pending frame then ages out and is dropped, and
    /// presentation collapses to half the source rate (measured: 30.0 fps and
    /// ~30 drops/s on 60 fps content).
    pub fn set_frame_signal(&mut self, signal: Arc<AtomicBool>) {
        self.frame_signal = Some(signal);
    }

    /// Anchors the engine's microsecond clock to a local `Instant`.
    ///
    /// `mpv_get_time_us()` is only valid with a live handle, so the host samples
    /// it once at startup and the render path interpolates from there.
    pub fn set_clock_base(&mut self, mpv_time_us: i64) {
        if mpv_time_us > 0 {
            self.clock_base = Some((Instant::now(), mpv_time_us));
        }
    }

    fn now_us(&self) -> Option<i64> {
        let (anchor, base) = self.clock_base?;
        Some(base + anchor.elapsed().as_micros() as i64)
    }

    /// Access the underlying mpv render context.
    pub fn mpv_render_ctx(&self) -> &MpvRenderContext {
        &self.mpv_render_ctx
    }

    /// Access the underlying mpv render context mutably.
    pub fn mpv_render_ctx_mut(&mut self) -> &mut MpvRenderContext {
        &mut self.mpv_render_ctx
    }

    /// Total video frames composited.
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    pub fn pacing(&self) -> PacingMode {
        self.pacing
    }

    pub fn swap_report(&self) -> SwapReportMode {
        self.swap_report
    }

    pub fn telemetry(&self) -> &RenderTelemetry {
        &self.telemetry
    }

    /// Paints the video frame enforcing the Framebuffer Restoration Contract.
    ///
    /// # Safety
    ///
    /// Must be called with the OpenGL context current on the calling thread.
    pub unsafe fn paint(
        &mut self,
        gl: &glow::Context,
        painter: &egui_glow::Painter,
        viewport_px: [i32; 4],
    ) {
        let [left, bottom, width, height] = viewport_px;
        if width <= 0 || height <= 0 {
            return;
        }

        self.telemetry.pain_count += 1;

        // 0. Deferred swap acknowledgement (eframe shell only): the frame
        //    rendered in the previous paint callback has been presented by the
        //    compositor by now. The native runner reports after its own
        //    `eglSwapBuffers` instead, which is stronger evidence.
        if !self.options.deferred_composite
            && self.swap_report == SwapReportMode::Deferred
            && self.pending_swap
        {
            self.mpv_render_ctx.report_swap();
            self.telemetry.swap_reports += 1;
            self.pending_swap = false;
            crate::causal_trace::record(
                crate::causal_trace::Kind::ReportSwap,
                crate::causal_trace::last_draw(),
                1,
                0,
                0,
                "deferred_eframe",
            );
        }

        // 1. Decide whether this callback has a video frame to draw.
        //
        //    `mpv_render_context_update()` is the engine's own "a frame is
        //    ready" signal. Rendering without it re-renders the same frame at
        //    display rate, which wastes the whole render pipeline and, in
        //    copy-back mode, re-uploads every frame.
        let draw_corr = crate::causal_trace::last_draw();
        self.gpu.poll(gl);
        crate::causal_trace::record(
            crate::causal_trace::Kind::UpdateEnter,
            draw_corr,
            0,
            0,
            0,
            "paint",
        );
        let update_flags = self.mpv_render_ctx.update();
        let engine_has_frame = MpvRenderContext::update_has_frame(update_flags);
        crate::trace::record(
            crate::trace::Ev::RenderUpdate,
            "paint",
            update_flags as i64,
            engine_has_frame as i64,
        );
        crate::causal_trace::record(
            crate::causal_trace::Kind::UpdateExit,
            draw_corr,
            update_flags as i64,
            engine_has_frame as i64,
            0,
            "paint",
        );
        let signalled = self
            .frame_signal
            .as_ref()
            .map(|s| s.swap(false, Ordering::AcqRel))
            .unwrap_or(false);
        let should_render = engine_has_frame || signalled;
        crate::causal_trace::record(
            crate::causal_trace::Kind::FrameSignal,
            draw_corr,
            signalled as i64,
            engine_has_frame as i64,
            should_render as i64,
            "after_update",
        );
        crate::causal_trace::record(
            crate::causal_trace::Kind::RenderDecision,
            draw_corr,
            engine_has_frame as i64,
            signalled as i64,
            should_render as i64,
            "update_or_signal",
        );
        self.last_video_rect = viewport_px;

        // 0b. Capture GL state *before* libmpv renders into the FBO.
        // libmpv's render call may mutate viewport/scissor/etc. internally
        // (it sizes its own rendering target to the FBO), so capturing only
        // around the quad composite would restore the post-render state
        // instead of the true pre-paint state. Holding the guard for the
        // whole of `paint()` makes the paint operation state-neutral.
        let _state_guard = GlStateGuard::capture(gl);

        let prev_fbo = gl.get_parameter_i32(glow::FRAMEBUFFER_BINDING);
        self.fbo.ensure_size(gl, width as u32, height as u32);

        if should_render {
            let started = Instant::now();
            crate::trace::record(
                crate::trace::Ev::RenderEnter,
                "mpv",
                width as i64,
                height as i64,
            );
            crate::causal_trace::record(
                crate::causal_trace::Kind::RenderEnter,
                draw_corr,
                width as i64,
                height as i64,
                0,
                "mpv",
            );
            let gpu_query = self.gpu.begin(gl);
            let frame = self.mpv_render_ctx.render_gl_to_fbo(
                self.fbo.fbo().0.get() as i32,
                width,
                height,
                self.fbo.internal_format(),
                false,
                self.pacing.blocks(),
                true,
            );
            let target = match &frame {
                Ok(info) if info.valid && info.target_time > 0 => {
                    Some(normalize_target_time(info.target_time, self.now_us()))
                }
                _ => None,
            };
            let elapsed = started.elapsed();
            self.gpu.end(
                gl,
                gpu_query,
                crate::gpu_timing::GpuStage::MpvRender,
                draw_corr,
                elapsed.as_micros() as i64,
            );
            let target_state = match &frame {
                Ok(info) if info.valid => 1,
                Ok(_) => 0,
                Err(_) => -1,
            };
            if let Ok(info) = &frame {
                crate::causal_trace::record5(
                    crate::causal_trace::Kind::FrameInfo,
                    draw_corr,
                    info.flags as i64,
                    info.target_time,
                    info.valid as i64,
                    info.block_vsync() as i64,
                    info.api_result as i64,
                    "every_query",
                );
            }
            crate::causal_trace::record(
                crate::causal_trace::Kind::RenderExit,
                draw_corr,
                elapsed.as_micros() as i64,
                target_state,
                0,
                "mpv",
            );
            crate::trace::record(
                crate::trace::Ev::RenderExit,
                "mpv",
                elapsed.as_micros() as i64,
                target_state,
            );
            let block_us = elapsed.as_micros() as u64;
            self.telemetry.last_render_block_us = block_us;
            self.telemetry.max_render_block_us = self.telemetry.max_render_block_us.max(block_us);
            self.telemetry.render_block_us_total += block_us;
            let bucket = BLOCK_BUCKETS_US
                .iter()
                .position(|limit| block_us < *limit)
                .unwrap_or(BLOCK_BUCKETS_US.len());
            self.telemetry.render_block_hist[bucket] += 1;
            self.telemetry.mpv_renders += 1;
            self.frame_count += 1;
            self.last_rendered_token = Some(self.frame_count);

            match self.swap_report {
                SwapReportMode::Immediate if !self.external_swap_ack => {
                    self.mpv_render_ctx.report_swap();
                    self.telemetry.swap_reports += 1;
                    crate::trace::record(crate::trace::Ev::ReportSwap, "immediate", 0, 0);
                    crate::causal_trace::record(
                        crate::causal_trace::Kind::ReportSwap,
                        self.frame_count,
                        1,
                        0,
                        0,
                        "immediate",
                    );
                }
                SwapReportMode::Immediate => {
                    // The native runner owns acknowledgement timing. An
                    // environment diagnostic must not reintroduce a premature
                    // post-swap report while wp_presentation is active.
                }
                SwapReportMode::Deferred if !self.external_swap_ack => self.pending_swap = true,
                SwapReportMode::Deferred => {
                    // The native runner owns the acknowledgement. Do not create a
                    // single pending flag; it cannot represent multiple frames in
                    // flight or present/discard ordering.
                }
                SwapReportMode::None => {}
            }

            // 1b. Non-blocking pacing: if the engine told us when this frame is
            //     due, ask for a repaint then instead of spinning on vsync.
            if let Some(target_us) = target {
                self.telemetry.last_target_time_us = Some(target_us);
                self.schedule_repaint_at(target_us);
            }

            if self.frame_count % 30 == 1 {
                // A glReadPixels here synchronizes with GPU completion. It is
                // trace-only diagnostics, so it must not run when trace logging
                // is off: doing so inserted a periodic synchronous readback into
                // the presentation path and could make the next frame miss its
                // display deadline.
                if log::log_enabled!(log::Level::Trace) {
                    let center_pixel = self.fbo.read_pixel_sample(gl);
                    log::trace!(
                        "Rendered frame #{}, viewport: [{}, {}, {}x{}], center RGBA: {:?}",
                        self.frame_count,
                        left,
                        bottom,
                        width,
                        height,
                        center_pixel
                    );
                }

                // Optional automated frame capture for verification testing
                if let Ok(capture_path) = std::env::var("MUVRIX_CAPTURE_FRAME_PNG") {
                    if self.frame_count == 181
                        || (self.frame_count == 61 && !capture_path.contains("movie"))
                    {
                        let count = self.frame_count;
                        if let Err(e) = self.fbo.save_screenshot_png(gl, &capture_path) {
                            log::error!("Failed to save screenshot: {e}");
                        } else {
                            log::info!("Successfully captured frame #{count} to {capture_path}");
                        }
                    }
                }
            }
        } else {
            // No new frame at this paint. Do NOT call
            // `mpv_render_context_render(SKIP_RENDERING)`: the engine counts that
            // as a completed render and advances the frame's display slot, so an
            // alternating paint/every-other-paint pattern silently drops every
            // second frame at 60 fps (measured: 30.0 fps, ~30 drops/s). Leaving
            // the pending frame untouched lets the next paint pick it up.
            self.telemetry.mpv_skips += 1;

            // Keep the event loop from parking indefinitely when mpv has no frame
            // to present. Without this, both wake paths go quiet at the same time:
            // the mpv update callback is not raised (no frame ready) and
            // `schedule_repaint_at` is not reached (gated behind `should_render`).
            // The loop would then arm `ControlFlow::Wait` (indefinite) and calloop
            // would park forever — the 4K HDR stall. A bounded poll interval lets
            // the next paint re-examine `has_frame()` for a newly-decoded frame
            // without busy-waiting.
            //
            // Use the wake handle directly instead of egui's
            // request_repaint_after: egui's callback is async and the deadline it
            // sets can be overwritten by the draw()-end deadline before the
            // callback fires, leaving the loop with a late deadline and no
            // pending Wake to interrupt it.
            //
            // The interval is the named, documented NO_FRAME_FALLBACK_INTERVAL
            // constant (Phase 2.3 measures it; it is not a new tunable).
            if schedule_no_frame_fallback(self.wake.as_ref(), Instant::now()) {
                self.telemetry.no_frame_fallback_deadlines =
                    self.telemetry.no_frame_fallback_deadlines.saturating_add(1);
            }
        }

        // 2. Framebuffer Restoration Contract: Restore target framebuffer
        let restore_target = painter
            .intermediate_fbo()
            .or_else(|| NonZeroU32::new(prev_fbo as u32).map(glow::NativeFramebuffer));
        gl.bind_framebuffer(glow::FRAMEBUFFER, restore_target);

        // 3. Composite the (fresh or previous) frame.
        //
        //    Skipped when the shell composites: there the engine's output is a
        //    texture the shell reads, and drawing it here would either double-
        //    draw it or (with a premultiplied UI) place the video on top of the
        //    UI. The engine leaves the working target bound and untouched.
        if !self.options.deferred_composite {
            gl.viewport(left, bottom, width, height);
            gl.disable(glow::SCISSOR_TEST);
            gl.disable(glow::DEPTH_TEST);
            gl.disable(glow::CULL_FACE);
            gl.disable(glow::BLEND);

            if let Some(quad) = &self.quad {
                quad.render(gl, self.fbo.texture());
            }
            // _state_guard restores viewport, scissor, blend, depth, cull, active texture,
            // bound texture, program, and VAO on drop.
        }

        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, None);
    }

    /// Requests a repaint shortly before `target_us` (microseconds, engine clock).
    fn schedule_repaint_at(&self, target_us: i64) {
        let Some(ctx) = &self.egui_ctx else {
            return;
        };
        let Some(now_us) = self.now_us() else {
            return;
        };
        let delta_us = target_us - now_us;
        // Present one refresh interval early: egui's timer fires on the next
        // vsync boundary, so waking exactly at the target would be late.
        let lead_us = 4_000;
        let wait_us = (delta_us - lead_us).max(0);
        crate::causal_trace::record(
            crate::causal_trace::Kind::TargetSchedule,
            crate::causal_trace::last_draw(),
            target_us,
            now_us,
            wait_us,
            "egui_repaint_after",
        );
        if wait_us > 0 {
            // Future target: wake shortly before it.
            let capped = (wait_us as u64).min(50_000);
            ctx.request_repaint_after(Duration::from_micros(capped));
        }
    }

    /// Destroys OpenGL resources.
    ///
    /// # Safety
    ///
    /// Must be called with the OpenGL context current on the calling thread,
    /// before the windowing-system connection is released. The context must be
    /// the one that created this engine and its FBO/quad resources.
    pub unsafe fn destroy(&mut self, gl: &glow::Context) {
        // Acknowledge any outstanding frame so the engine's teardown is not left
        // waiting on a swap that will never be reported.
        if self.pending_swap {
            self.mpv_render_ctx.report_swap();
            self.pending_swap = false;
        }
        self.gpu.destroy(gl);
        self.mpv_render_ctx.destroy();
        self.fbo.destroy(gl);
        if let Some(quad) = self.quad.as_mut() {
            quad.destroy(gl);
        }
    }
}

/// RAII guard that captures and isolates OpenGL pipeline state during video quad compositing.
/// Restores viewport, scissor box & test, depth test, cull face, blend state, active texture unit,
/// bound texture, shader program, and vertex array object upon drop or explicit restoration.
///
/// Exposed publicly for regression tests (`gl_state_guard_test.rs`) that verify
/// the framebuffer compositing contract headlessly.
pub struct GlStateGuard<'a> {
    gl: &'a glow::Context,
    prev_viewport: [i32; 4],
    prev_scissor_box: [i32; 4],
    prev_scissor_test: bool,
    prev_depth_test: bool,
    prev_cull_face: bool,
    prev_blend: bool,
    prev_active_texture: u32,
    prev_texture_2d: Option<glow::NativeTexture>,
    prev_program: Option<glow::NativeProgram>,
    prev_vao: Option<glow::NativeVertexArray>,
}

impl<'a> GlStateGuard<'a> {
    /// Captures the GL state tuples restored by this guard when dropped.
    ///
    /// # Safety
    ///
    /// Must be called with the OpenGL context current on the calling thread.
    pub unsafe fn capture(gl: &'a glow::Context) -> Self {
        let mut prev_viewport = [0i32; 4];
        gl.get_parameter_i32_slice(glow::VIEWPORT, &mut prev_viewport);

        let mut prev_scissor_box = [0i32; 4];
        gl.get_parameter_i32_slice(glow::SCISSOR_BOX, &mut prev_scissor_box);

        let prev_scissor_test = gl.is_enabled(glow::SCISSOR_TEST);
        let prev_depth_test = gl.is_enabled(glow::DEPTH_TEST);
        let prev_cull_face = gl.is_enabled(glow::CULL_FACE);
        let prev_blend = gl.is_enabled(glow::BLEND);
        let prev_active_texture = gl.get_parameter_i32(glow::ACTIVE_TEXTURE) as u32;
        let prev_texture_2d = gl.get_parameter_texture(glow::TEXTURE_BINDING_2D);
        let prev_program = gl.get_parameter_program(glow::CURRENT_PROGRAM);
        let prev_vao = gl.get_parameter_vertex_array(glow::VERTEX_ARRAY_BINDING);

        Self {
            gl,
            prev_viewport,
            prev_scissor_box,
            prev_scissor_test,
            prev_depth_test,
            prev_cull_face,
            prev_blend,
            prev_active_texture,
            prev_texture_2d,
            prev_program,
            prev_vao,
        }
    }

    /// Explicitly restores the previously captured GL state.
    ///
    /// # Safety
    ///
    /// Must be called with the OpenGL context current on the calling thread.
    pub unsafe fn restore(&self) {
        let gl = self.gl;
        gl.viewport(
            self.prev_viewport[0],
            self.prev_viewport[1],
            self.prev_viewport[2],
            self.prev_viewport[3],
        );
        gl.scissor(
            self.prev_scissor_box[0],
            self.prev_scissor_box[1],
            self.prev_scissor_box[2],
            self.prev_scissor_box[3],
        );

        if self.prev_scissor_test {
            gl.enable(glow::SCISSOR_TEST);
        } else {
            gl.disable(glow::SCISSOR_TEST);
        }

        if self.prev_depth_test {
            gl.enable(glow::DEPTH_TEST);
        } else {
            gl.disable(glow::DEPTH_TEST);
        }

        if self.prev_cull_face {
            gl.enable(glow::CULL_FACE);
        } else {
            gl.disable(glow::CULL_FACE);
        }

        if self.prev_blend {
            gl.enable(glow::BLEND);
        } else {
            gl.disable(glow::BLEND);
        }

        gl.active_texture(self.prev_active_texture);
        gl.bind_texture(glow::TEXTURE_2D, self.prev_texture_2d);
        gl.use_program(self.prev_program);
        gl.bind_vertex_array(self.prev_vao);
    }
}

impl Drop for GlStateGuard<'_> {
    fn drop(&mut self) {
        unsafe {
            self.restore();
        }
    }
}
