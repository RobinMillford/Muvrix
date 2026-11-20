//! Video, Decoder Watchdog, Shader, and Frame-Stepping domain subsystem.

use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use crate::decode::{classify, DecodeObservation, DecodePath};
use crate::error::MpvError;
use crate::handle::MpvHandle;
use crate::shader::{
    Anime4KPreset, CustomShaderEntry, DebandPreset, ShaderManager, ShaderQuality,
    VideoAdjustmentSettings,
};

/// Precision frame-stepping preset modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SteppingPreset {
    /// Advance/retreat exactly one decoded video frame using native libmpv frame-step.
    #[default]
    Frame,
    /// Advance/retreat by 1/24 second (~41.67ms, Film standard).
    Film24,
    /// Advance/retreat by 1/30 second (~33.33ms, NTSC standard).
    Ntsc30,
    /// Advance/retreat by 1/60 second (~16.67ms, High-Rate standard).
    HighRate60,
}

/// How the decoded video is fitted into the presentation surface.
///
/// These are deliberately distinct from aspect override, crop and zoom: the
/// scaling mode only decides what happens to the parts of the surface the
/// video does not cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScalingMode {
    /// Preserve aspect ratio and show the whole frame (letterbox / pillarbox).
    #[default]
    Fit,
    /// Preserve aspect ratio and fill the surface, cropping the excess.
    Fill,
    /// Ignore the source aspect ratio and fill the surface exactly.
    Stretch,
    /// Present at native pixel dimensions without upscaling.
    Original,
}

impl ScalingMode {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Fit => "Fit (Contain)",
            Self::Fill => "Fill (Cover)",
            Self::Stretch => "Stretch",
            Self::Original => "Original Size",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Fit => "Show the whole frame, letterboxing where needed",
            Self::Fill => "Fill the view, cropping the excess edges",
            Self::Stretch => "Stretch to the view, ignoring the source aspect",
            Self::Original => "Display at the source's native pixel size",
        }
    }

    pub const ALL: &'static [ScalingMode] = &[
        ScalingMode::Fit,
        ScalingMode::Fill,
        ScalingMode::Stretch,
        ScalingMode::Original,
    ];
}

/// Common crop presets expressed as a target display ratio.
///
/// The crop rectangle is derived from the decoded frame size and centred, so
/// it always removes the requested amount from the visible area rather than
/// relying on container metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CropPreset {
    #[default]
    Default,
    R1x1,
    R4x3,
    R5x4,
    R16x9,
    R16x10,
    R21x9,
    R221x100,
    R235x100,
    R239x100,
}

impl CropPreset {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Default => "Default (No Crop)",
            Self::R1x1 => "1:1",
            Self::R4x3 => "4:3",
            Self::R5x4 => "5:4",
            Self::R16x9 => "16:9",
            Self::R16x10 => "16:10",
            Self::R21x9 => "21:9",
            Self::R221x100 => "2.21:1",
            Self::R235x100 => "2.35:1",
            Self::R239x100 => "2.39:1",
        }
    }

    /// Returns the target width/height ratio, or `None` for the reset preset.
    pub fn ratio(&self) -> Option<(f64, f64)> {
        match self {
            Self::Default => None,
            Self::R1x1 => Some((1.0, 1.0)),
            Self::R4x3 => Some((4.0, 3.0)),
            Self::R5x4 => Some((5.0, 4.0)),
            Self::R16x9 => Some((16.0, 9.0)),
            Self::R16x10 => Some((16.0, 10.0)),
            Self::R21x9 => Some((21.0, 9.0)),
            Self::R221x100 => Some((2.21, 1.0)),
            Self::R235x100 => Some((2.35, 1.0)),
            Self::R239x100 => Some((2.39, 1.0)),
        }
    }

    pub const ALL: &'static [CropPreset] = &[
        CropPreset::Default,
        CropPreset::R1x1,
        CropPreset::R4x3,
        CropPreset::R5x4,
        CropPreset::R16x9,
        CropPreset::R16x10,
        CropPreset::R21x9,
        CropPreset::R221x100,
        CropPreset::R235x100,
        CropPreset::R239x100,
    ];
}

/// Builds a centred libmpv `video-crop` rectangle for `ratio` inside a
/// `width`x`height` decoded frame. Dimensions are rounded to even numbers so
/// chroma-subsampled formats crop cleanly.
///
/// Returns an empty string when the preset is the reset preset or the frame
/// dimensions are unknown.
pub fn crop_rect_for_ratio(preset: CropPreset, width: i64, height: i64) -> String {
    let Some((num, den)) = preset.ratio() else {
        return String::new();
    };
    if width <= 0 || height <= 0 || num <= 0.0 || den <= 0.0 {
        return String::new();
    }

    let source_ratio = width as f64 / height as f64;
    let target_ratio = num / den;

    let (crop_w, crop_h) = if target_ratio > source_ratio {
        // Target is wider than the source: keep the full width, cut height.
        let h = (width as f64 / target_ratio).round() as i64;
        (width, h)
    } else {
        // Target is narrower: keep the full height, cut width.
        let w = (height as f64 * target_ratio).round() as i64;
        (w, height)
    };

    let even = |v: i64| -> i64 { (v / 2).max(1) * 2 };
    let crop_w = even(crop_w.min(width));
    let crop_h = even(crop_h.min(height));
    let x = ((width - crop_w) / 2).max(0);
    let y = ((height - crop_h) / 2).max(0);

    format!("{crop_w}x{crop_h}+{x}+{y}")
}

/// Screenshot capture source for [`VideoSubsystem::screenshot`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScreenshotMode {
    /// Video frame with subtitles and OSD burnt in.
    #[default]
    Subtitles,
    /// Raw video frame with subtitles and OSD omitted.
    Video,
    /// The rendered window contents.
    Window,
}

impl ScreenshotMode {
    fn mpv_arg(&self) -> &'static str {
        match self {
            Self::Subtitles => "subtitles",
            Self::Video => "video",
            Self::Window => "window",
        }
    }
}

impl SteppingPreset {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Frame => "Frame",
            Self::Film24 => "1/24s (Film)",
            Self::Ntsc30 => "1/30s (NTSC)",
            Self::HighRate60 => "1/60s (High-Rate)",
        }
    }

    pub fn delta_seconds(&self) -> Option<f64> {
        match self {
            Self::Frame => None,
            Self::Film24 => Some(1.0 / 24.0),
            Self::Ntsc30 => Some(1.0 / 30.0),
            Self::HighRate60 => Some(1.0 / 60.0),
        }
    }
}

/// Reasons why a hardware decoder fallback was triggered.
#[derive(Debug, Clone, PartialEq)]
pub enum DecoderFallbackReason {
    InitFailed,
    StallDetected {
        stall_duration_secs: f64,
        dropped_frames: i64,
    },
    PersistentErrors {
        error_count: u32,
    },
    ShaderIncompatible {
        shader: String,
    },
    UnsupportedPath {
        mode: String,
    },
    ExplicitUserRequest,
}

impl DecoderFallbackReason {
    pub fn describe(&self) -> String {
        match self {
            Self::InitFailed => "Hardware decoder failed to initialize".to_string(),
            Self::StallDetected {
                stall_duration_secs,
                dropped_frames,
            } => {
                format!(
                    "Decoder presentation stall detected ({:.1}s, {} dropped frames)",
                    stall_duration_secs, dropped_frames
                )
            }
            Self::PersistentErrors { error_count } => {
                format!("Persistent decoder decode errors ({error_count} events)")
            }
            Self::ShaderIncompatible { shader } => {
                format!("Hardware surface incompatible with custom shader '{shader}'")
            }
            Self::UnsupportedPath { mode } => {
                format!("Requested hardware decode mode '{mode}' unsupported by device")
            }
            Self::ExplicitUserRequest => "Explicit user decoder selection".to_string(),
        }
    }
}

/// Operational status of the decoder watchdog.
#[derive(Debug, Clone, PartialEq)]
pub enum WatchdogStatus {
    Stable {
        active_mode: String,
        path: DecodePath,
    },
    StallWarning {
        elapsed_secs: f64,
        drop_count: i64,
    },
    FallenBack {
        active_mode: String,
        path: DecodePath,
        reason: DecoderFallbackReason,
    },
    Exhausted {
        active_mode: String,
        reason: String,
    },
}

impl Default for WatchdogStatus {
    fn default() -> Self {
        Self::Stable {
            active_mode: "auto-safe".to_string(),
            path: DecodePath::Inactive,
        }
    }
}

struct WatchdogInner {
    requested_hwdec: String,
    current_hwdec: String,
    fallback_stage: usize,
    last_pos: f64,
    last_progress_instant: Instant,
    last_drop_count: i64,
    error_count: u32,
    status: WatchdogStatus,
    disabled_modes: Vec<String>,
    pending_notifications: Vec<String>,
}

/// Autonomous watchdog monitoring hardware decoder health, presentation stalls,
/// and executing capability-preserving fallback ladders.
pub struct DecoderWatchdog {
    inner: Mutex<WatchdogInner>,
}

impl Default for DecoderWatchdog {
    fn default() -> Self {
        Self::new()
    }
}

impl DecoderWatchdog {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(WatchdogInner {
                requested_hwdec: "auto-safe".to_string(),
                current_hwdec: "inactive".to_string(),
                fallback_stage: 0,
                last_pos: 0.0,
                last_progress_instant: Instant::now(),
                last_drop_count: 0,
                error_count: 0,
                status: WatchdogStatus::default(),
                disabled_modes: Vec::new(),
                pending_notifications: Vec::new(),
            }),
        }
    }

    fn get_copy_back_mode(requested: &str) -> &'static str {
        match requested {
            "vaapi" => "vaapi-copy",
            "nvdec" => "nvdec-copy",
            "d3d11va" => "d3d11va-copy",
            "videotoolbox" => "videotoolbox-copy",
            _ => "auto-copy",
        }
    }

    /// Evaluates decoder health.
    pub fn check_watchdog(
        &self,
        handle: &MpvHandle,
        is_playing: bool,
        is_buffering: bool,
        current_pos: f64,
        drops: i64,
    ) -> Option<DecoderFallbackReason> {
        let mut inner = self.inner.lock().unwrap();

        if !is_playing || is_buffering {
            inner.last_progress_instant = Instant::now();
            inner.last_pos = current_pos;
            inner.last_drop_count = drops;
            return None;
        }

        if (current_pos - inner.last_pos).abs() > 0.05 {
            inner.last_pos = current_pos;
            inner.last_progress_instant = Instant::now();
            inner.last_drop_count = drops;
            if matches!(inner.status, WatchdogStatus::StallWarning { .. }) {
                inner.status = WatchdogStatus::Stable {
                    active_mode: inner.current_hwdec.clone(),
                    path: classify(&inner.current_hwdec, ""),
                };
            }
            return None;
        }

        let elapsed = inner.last_progress_instant.elapsed().as_secs_f64();
        let delta_drops = drops - inner.last_drop_count;

        if elapsed >= 1.5 && (delta_drops >= 3 || elapsed >= 2.5) {
            inner.status = WatchdogStatus::StallWarning {
                elapsed_secs: elapsed,
                drop_count: delta_drops,
            };
        }

        if elapsed >= 3.0 && (delta_drops >= 5 || elapsed >= 5.0) {
            let path = classify(&inner.current_hwdec, "");
            if path.is_hardware() {
                let reason = DecoderFallbackReason::StallDetected {
                    stall_duration_secs: elapsed,
                    dropped_frames: delta_drops,
                };
                Self::execute_fallback(&mut inner, handle, reason.clone());
                return Some(reason);
            }
        }

        None
    }

    fn execute_fallback(
        inner: &mut WatchdogInner,
        handle: &MpvHandle,
        reason: DecoderFallbackReason,
    ) -> String {
        let previous_mode = inner.current_hwdec.clone();
        if !inner.disabled_modes.contains(&previous_mode) {
            inner.disabled_modes.push(previous_mode.clone());
        }

        let next_mode = if inner.fallback_stage == 0 {
            inner.fallback_stage = 1;
            let copy_mode = Self::get_copy_back_mode(&inner.requested_hwdec);
            if inner.disabled_modes.contains(&copy_mode.to_string()) {
                inner.fallback_stage = 2;
                "no"
            } else {
                copy_mode
            }
        } else {
            inner.fallback_stage = 2;
            "no"
        };

        log::warn!(
            "Decoder watchdog triggered fallback ({:?}): '{}' -> '{}'",
            reason,
            previous_mode,
            next_mode
        );

        if let Err(e) = handle.set_property_string("hwdec", next_mode) {
            log::error!("Failed to switch hwdec property to '{next_mode}': {e}");
        }

        inner.current_hwdec = next_mode.to_string();
        let path = classify(next_mode, "");

        if next_mode == "no" {
            inner.status = WatchdogStatus::Exhausted {
                active_mode: "no".to_string(),
                reason: reason.describe(),
            };
        } else {
            inner.status = WatchdogStatus::FallenBack {
                active_mode: next_mode.to_string(),
                path,
                reason: reason.clone(),
            };
        }

        let note = format!(
            "Decoder fallback: switched to {} ({})",
            next_mode,
            reason.describe()
        );
        inner.pending_notifications.push(note);
        inner.last_progress_instant = Instant::now();

        next_mode.to_string()
    }

    pub fn trigger_fallback(&self, handle: &MpvHandle, reason: DecoderFallbackReason) -> String {
        let mut inner = self.inner.lock().unwrap();
        Self::execute_fallback(&mut inner, handle, reason)
    }

    pub fn reset_for_new_media(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.fallback_stage = 0;
        inner.last_pos = 0.0;
        inner.last_progress_instant = Instant::now();
        inner.last_drop_count = 0;
        inner.error_count = 0;
        inner.disabled_modes.clear();
        inner.status = WatchdogStatus::Stable {
            active_mode: inner.requested_hwdec.clone(),
            path: classify(&inner.requested_hwdec, ""),
        };
    }

    pub fn set_requested_hwdec(&self, mode: &str) {
        let mut inner = self.inner.lock().unwrap();
        inner.requested_hwdec = mode.to_string();
        inner.fallback_stage = 0;
        inner.disabled_modes.clear();
    }

    pub fn on_hwdec_changed(&self, new_hwdec: &str) {
        let mut inner = self.inner.lock().unwrap();
        inner.current_hwdec = new_hwdec.to_string();
        if !matches!(inner.status, WatchdogStatus::FallenBack { .. })
            && !matches!(inner.status, WatchdogStatus::Exhausted { .. })
        {
            inner.status = WatchdogStatus::Stable {
                active_mode: new_hwdec.to_string(),
                path: classify(new_hwdec, ""),
            };
        }
    }

    pub fn current_hwdec(&self) -> String {
        self.inner.lock().unwrap().current_hwdec.clone()
    }

    pub fn requested_hwdec(&self) -> String {
        self.inner.lock().unwrap().requested_hwdec.clone()
    }

    pub fn status(&self) -> WatchdogStatus {
        self.inner.lock().unwrap().status.clone()
    }

    pub fn take_notification(&self) -> Option<String> {
        let mut inner = self.inner.lock().unwrap();
        if inner.pending_notifications.is_empty() {
            None
        } else {
            Some(inner.pending_notifications.remove(0))
        }
    }
}

/// Subsystem managing video rendering, hardware decoders, shader pipelines, and frame-stepping.
pub struct VideoSubsystem {
    video_format: String,
    video_width: i64,
    video_height: i64,
    panscan: f64,
    stepping_preset: SteppingPreset,
    watchdog: DecoderWatchdog,
    current_hwdec: String,
    /// Last `frame-drop-count` delivered by the observed property.
    ///
    /// The watchdog consumes this instead of reading the engine: the observation
    /// carries the value inline, so a paint costs no engine round trip. It is
    /// genuinely `unknown` until the engine has reported a value at least once,
    /// which is kept distinct from a real zero.
    observed_drop_count: AtomicI64,
    observed_drop_count_known: AtomicBool,
}

impl Default for VideoSubsystem {
    fn default() -> Self {
        Self::new()
    }
}

impl VideoSubsystem {
    /// Creates a new `VideoSubsystem`.
    pub fn new() -> Self {
        Self {
            video_format: String::new(),
            video_width: 0,
            video_height: 0,
            panscan: 0.0,
            stepping_preset: SteppingPreset::Frame,
            watchdog: DecoderWatchdog::new(),
            current_hwdec: "inactive".to_string(),
            observed_drop_count: AtomicI64::new(0),
            observed_drop_count_known: AtomicBool::new(false),
        }
    }

    /// Returns the currently active hardware decoder mode.
    pub fn hwdec_current(&self) -> &str {
        &self.current_hwdec
    }

    /// Returns the user's requested hardware decoder mode.
    pub fn requested_hwdec(&self) -> String {
        self.watchdog.requested_hwdec()
    }

    /// Sets the requested hardware decoding mode.
    pub fn set_hwdec_mode(&self, handle: &MpvHandle, mode: &str) -> Result<(), MpvError> {
        handle.set_property_string("hwdec", mode)?;
        self.watchdog.set_requested_hwdec(mode);
        Ok(())
    }

    /// Classifies the currently active decoding observation.
    pub fn decode_observation(&self, handle: &MpvHandle) -> DecodeObservation {
        let active = handle
            .get_property_string("hwdec-current")
            .unwrap_or_else(|_| "inactive".to_string());
        let pixel_format = handle
            .get_property_string("video-format")
            .unwrap_or_default();
        DecodeObservation::new(active, pixel_format)
    }

    /// Returns the currently configured frame-stepping preset.
    pub fn stepping_preset(&self) -> SteppingPreset {
        self.stepping_preset
    }

    /// Sets the frame-stepping preset mode.
    pub fn set_stepping_preset(&mut self, preset: SteppingPreset) {
        self.stepping_preset = preset;
    }

    /// Returns detected video container format.
    pub fn video_format(&self) -> &str {
        &self.video_format
    }

    /// Returns detected video dimensions `(width, height)`.
    pub fn video_dimensions(&self) -> (i64, i64) {
        (self.video_width, self.video_height)
    }

    /// Queries container nominal frame rate.
    pub fn container_fps(&self, handle: &MpvHandle) -> Option<f64> {
        handle.get_property::<f64>("container-fps").ok()
    }

    /// Queries estimated video filter frame rate.
    pub fn estimated_vf_fps(&self, handle: &MpvHandle) -> Option<f64> {
        handle.get_property::<f64>("estimated-vf-fps").ok()
    }

    /// Returns count of dropped video frames.
    pub fn frame_drop_count(&self, handle: &MpvHandle) -> i64 {
        handle.get_property::<i64>("frame-drop-count").unwrap_or(0)
    }

    /// Records the `frame-drop-count` delivered by the observed property.
    ///
    /// Called from the event path, never from a paint, so the paint path has no
    /// engine read to perform.
    pub fn on_frame_drop_count_changed(&self, count: i64) {
        self.observed_drop_count.store(count, Ordering::Release);
        self.observed_drop_count_known.store(true, Ordering::Release);
    }

    /// Last observed drop count, or `None` until the engine has reported one.
    pub fn observed_frame_drop_count(&self) -> Option<i64> {
        self.observed_drop_count_known
            .load(Ordering::Acquire)
            .then(|| self.observed_drop_count.load(Ordering::Acquire))
    }

    /// Watchdog tick fed by the observed drop count — performs no engine read.
    pub fn check_watchdog_observed(
        &self,
        handle: &MpvHandle,
        is_playing: bool,
        is_buffering: bool,
        current_pos: f64,
    ) {
        // Before the engine has reported a drop count there is nothing to judge;
        // treating "not yet known" as zero would be a guess, so the watchdog
        // simply waits for the first observation.
        let Some(drops) = self.observed_frame_drop_count() else {
            return;
        };
        self.watchdog
            .check_watchdog(handle, is_playing, is_buffering, current_pos, drops);
    }

    /// Returns count of decoder-level dropped frames.
    pub fn decoder_frame_drop_count(&self, handle: &MpvHandle) -> i64 {
        handle
            .get_property::<i64>("decoder-frame-drop-count")
            .unwrap_or(0)
    }

    /// Returns count of video output delayed frames.
    pub fn vo_delayed_frame_count(&self, handle: &MpvHandle) -> i64 {
        handle
            .get_property::<i64>("vo-delayed-frame-count")
            .unwrap_or(0)
    }

    /// Queries audio/video synchronization offset in seconds.
    pub fn avsync(&self, handle: &MpvHandle) -> Option<f64> {
        handle.get_property::<f64>("avsync").ok()
    }

    /// Queries total accumulated A/V synchronization drift.
    pub fn total_avsync_change(&self, handle: &MpvHandle) -> Option<f64> {
        handle.get_property::<f64>("total-avsync-change").ok()
    }

    /// Queries source video pixel format.
    pub fn video_pixel_format(&self, handle: &MpvHandle) -> String {
        handle
            .get_property_string("video-params/pixelformat")
            .unwrap_or_default()
    }

    /// Queries output video pixel format.
    pub fn video_out_pixel_format(&self, handle: &MpvHandle) -> String {
        handle
            .get_property_string("video-out-params/pixelformat")
            .unwrap_or_default()
    }

    /// Queries source video gamma curve.
    pub fn video_gamma(&self, handle: &MpvHandle) -> String {
        handle
            .get_property_string("video-params/gamma")
            .unwrap_or_default()
    }

    /// Queries source video color primaries.
    pub fn video_primaries(&self, handle: &MpvHandle) -> String {
        handle
            .get_property_string("video-params/primaries")
            .unwrap_or_default()
    }

    /// Queries source video signal peak.
    pub fn video_sig_peak(&self, handle: &MpvHandle) -> Option<f64> {
        handle.get_property::<f64>("video-params/sig-peak").ok()
    }

    /// Returns current panscan crop/fill value.
    pub fn panscan(&self) -> f64 {
        self.panscan
    }

    /// Sets panscan crop value in `[0.0, 1.0]`.
    pub fn set_panscan(&mut self, handle: &MpvHandle, value: f64) -> Result<(), MpvError> {
        let clamped = value.clamp(0.0, 1.0);
        handle.set_property("panscan", clamped)?;
        self.panscan = clamped;
        Ok(())
    }

    /// Resets panscan to 0.0 (Fit).
    pub fn set_panscan_fit(&mut self, handle: &MpvHandle) -> Result<(), MpvError> {
        self.set_panscan(handle, 0.0)
    }

    /// Sets panscan to 1.0 (Fill/Crop).
    pub fn set_panscan_crop(&mut self, handle: &MpvHandle) -> Result<(), MpvError> {
        self.set_panscan(handle, 1.0)
    }

    // -----------------------------------------------------------------
    // Video geometry: aspect override, scaling, crop, zoom and pan.
    // Each of these is an independent libmpv property; the UI reads them
    // back so the displayed state always matches the engine.
    // -----------------------------------------------------------------

    /// Returns the effective aspect override (`"auto"` when none is set).
    pub fn aspect_override(&self, handle: &MpvHandle) -> String {
        match handle.get_property_string("video-aspect-override") {
            Ok(v) if !v.is_empty() && v != "no" => v,
            _ => "auto".to_string(),
        }
    }

    /// Sets `video-aspect-override`. `"auto"`, an empty string or `"no"`
    /// clears the override and restores the stream's own aspect ratio.
    pub fn set_aspect_override(&self, handle: &MpvHandle, aspect: &str) -> Result<(), MpvError> {
        let value = match aspect.trim() {
            "" | "auto" | "no" => "no",
            other => other,
        };
        handle.set_property_string("video-aspect-override", value)
    }

    /// Returns the current `video-zoom` factor (0.0 = neutral).
    pub fn video_zoom(&self, handle: &MpvHandle) -> f64 {
        handle.get_property::<f64>("video-zoom").unwrap_or(0.0)
    }

    /// Sets `video-zoom`, clamped to a sane range so the frame cannot be
    /// zoomed into invisibility by a stray key repeat.
    pub fn set_video_zoom(&self, handle: &MpvHandle, zoom: f64) -> Result<(), MpvError> {
        handle.set_property("video-zoom", zoom.clamp(-5.0, 5.0))
    }

    /// Adjusts `video-zoom` by `delta`.
    pub fn adjust_video_zoom(&self, handle: &MpvHandle, delta: f64) -> Result<(), MpvError> {
        let next = self.video_zoom(handle) + delta;
        self.set_video_zoom(handle, next)
    }

    /// Returns the current `(video-pan-x, video-pan-y)` offsets.
    pub fn video_pan(&self, handle: &MpvHandle) -> (f64, f64) {
        let x = handle.get_property::<f64>("video-pan-x").unwrap_or(0.0);
        let y = handle.get_property::<f64>("video-pan-y").unwrap_or(0.0);
        (x, y)
    }

    /// Sets the `video-pan-x` / `video-pan-y` offsets.
    pub fn set_video_pan(&self, handle: &MpvHandle, x: f64, y: f64) -> Result<(), MpvError> {
        handle.set_property("video-pan-x", x.clamp(-2.0, 2.0))?;
        handle.set_property("video-pan-y", y.clamp(-2.0, 2.0))
    }

    /// Recentres the video by clearing both pan offsets.
    pub fn reset_video_pan(&self, handle: &MpvHandle) -> Result<(), MpvError> {
        self.set_video_pan(handle, 0.0, 0.0)
    }

    /// Applies a [`ScalingMode`] by configuring the underlying libmpv
    /// properties (`keepaspect`, `video-unscaled`, `panscan`).
    pub fn apply_scaling_mode(
        &mut self,
        handle: &MpvHandle,
        mode: ScalingMode,
    ) -> Result<(), MpvError> {
        match mode {
            ScalingMode::Fit => {
                handle.set_property("video-unscaled", false)?;
                handle.set_property("keepaspect", true)?;
                self.set_panscan(handle, 0.0)?;
            }
            ScalingMode::Fill => {
                handle.set_property("video-unscaled", false)?;
                handle.set_property("keepaspect", true)?;
                self.set_panscan(handle, 1.0)?;
            }
            ScalingMode::Stretch => {
                handle.set_property("video-unscaled", false)?;
                handle.set_property("keepaspect", false)?;
                self.set_panscan(handle, 0.0)?;
            }
            ScalingMode::Original => {
                handle.set_property("keepaspect", true)?;
                self.set_panscan(handle, 0.0)?;
                handle.set_property("video-unscaled", true)?;
            }
        }
        Ok(())
    }

    /// Derives the active [`ScalingMode`] from the live engine properties.
    pub fn scaling_mode(&self, handle: &MpvHandle) -> ScalingMode {
        let unscaled = handle
            .get_property::<bool>("video-unscaled")
            .unwrap_or(false);
        if unscaled {
            return ScalingMode::Original;
        }
        let keepaspect = handle.get_property::<bool>("keepaspect").unwrap_or(true);
        if !keepaspect {
            return ScalingMode::Stretch;
        }
        if self.panscan >= 0.999 {
            ScalingMode::Fill
        } else {
            ScalingMode::Fit
        }
    }

    /// Returns the decoded frame dimensions in pixels.
    pub fn video_pixel_size(&self, handle: &MpvHandle) -> (i64, i64) {
        let w = handle.get_property::<i64>("width").unwrap_or(0);
        let h = handle.get_property::<i64>("height").unwrap_or(0);
        (w, h)
    }

    /// Applies a [`CropPreset`] as a centred `video-crop` rectangle.
    pub fn apply_crop_preset(
        &self,
        handle: &MpvHandle,
        preset: CropPreset,
    ) -> Result<(), MpvError> {
        let (w, h) = self.video_pixel_size(handle);
        let rect = crop_rect_for_ratio(preset, w, h);
        handle.set_property_string("video-crop", &rect)
    }

    /// Returns the active `video-crop` rectangle (`""` when uncropped).
    pub fn video_crop(&self, handle: &MpvHandle) -> String {
        match handle.get_property_string("video-crop") {
            Ok(v) if v != "no" => v,
            _ => String::new(),
        }
    }

    /// Toggles libmpv deinterlacing.
    pub fn set_deinterlace(&self, handle: &MpvHandle, enabled: bool) -> Result<(), MpvError> {
        handle.set_property("deinterlace", enabled)
    }

    /// Returns whether deinterlacing is currently enabled.
    pub fn deinterlace(&self, handle: &MpvHandle) -> bool {
        handle.get_property::<bool>("deinterlace").unwrap_or(false)
    }

    /// Toggles libmpv debanding.
    pub fn set_deband_enabled(&self, handle: &MpvHandle, enabled: bool) -> Result<(), MpvError> {
        handle.set_property("deband", enabled)
    }

    /// Returns whether debanding is currently enabled.
    pub fn deband_enabled(&self, handle: &MpvHandle) -> bool {
        handle.get_property::<bool>("deband").unwrap_or(false)
    }

    /// Captures a screenshot to `path` using the engine's own capture path.
    pub fn screenshot(
        &self,
        handle: &MpvHandle,
        path: &str,
        mode: ScreenshotMode,
    ) -> Result<(), MpvError> {
        handle.command(&["screenshot-to-file", path, mode.mpv_arg()])
    }

    /// Steps forward one video frame.
    pub fn frame_step(&mut self, handle: &MpvHandle) -> Result<(), MpvError> {
        handle.command(&["frame-step"])
    }

    /// Steps backward one video frame.
    pub fn frame_back_step(&mut self, handle: &MpvHandle) -> Result<(), MpvError> {
        handle.command(&["frame-back-step"])
    }

    /// Steps playback by a precise floating-point second offset.
    pub fn step_by_seconds(
        &mut self,
        handle: &MpvHandle,
        delta_seconds: f64,
    ) -> Result<(), MpvError> {
        let delta_str = format!("{delta_seconds:+.6}");
        handle.command(&["seek", &delta_str, "relative", "exact"])
    }

    /// Advances playback using precision preset mode.
    pub fn step_preset_forward(
        &mut self,
        handle: &MpvHandle,
        preset: SteppingPreset,
    ) -> Result<(), MpvError> {
        match preset.delta_seconds() {
            None => self.frame_step(handle),
            Some(delta) => self.step_by_seconds(handle, delta),
        }
    }

    /// Retreats playback using precision preset mode.
    pub fn step_preset_backward(
        &mut self,
        handle: &MpvHandle,
        preset: SteppingPreset,
    ) -> Result<(), MpvError> {
        match preset.delta_seconds() {
            None => self.frame_back_step(handle),
            Some(delta) => self.step_by_seconds(handle, -delta),
        }
    }

    /// Applies color adjustments (brightness, contrast, saturation, gamma, hue).
    pub fn apply_video_adjustments(
        &self,
        handle: &MpvHandle,
        adjustments: &VideoAdjustmentSettings,
    ) -> Result<(), MpvError> {
        ShaderManager::apply_adjustments(handle, adjustments)
    }

    /// Resets color adjustments to neutral defaults.
    pub fn reset_video_adjustments(&self, handle: &MpvHandle) -> Result<(), MpvError> {
        ShaderManager::reset_adjustments(handle)
    }

    /// Applies an Anime4K shader preset.
    pub fn apply_anime4k(
        &self,
        handle: &MpvHandle,
        preset: Anime4KPreset,
        quality: ShaderQuality,
    ) -> Result<Vec<String>, MpvError> {
        ShaderManager::apply_anime4k_mode(handle, preset, quality, None)
    }

    /// Applies a multi-stage GLSL custom shader pipeline.
    pub fn apply_shader_pipeline(
        &self,
        handle: &MpvHandle,
        preset: Anime4KPreset,
        quality: ShaderQuality,
        custom_shaders: &[CustomShaderEntry],
    ) -> Result<Vec<String>, MpvError> {
        ShaderManager::apply_shader_pipeline(handle, preset, quality, None, custom_shaders)
    }

    /// Clears all active custom shaders.
    pub fn clear_shaders(&self, handle: &MpvHandle) -> Result<(), MpvError> {
        ShaderManager::clear_shaders(handle)
    }

    /// Applies a debanding quality preset.
    pub fn set_deband_preset(
        &self,
        handle: &MpvHandle,
        preset: DebandPreset,
        settings: &mut VideoAdjustmentSettings,
    ) -> Result<(), MpvError> {
        preset.apply_to(settings);
        self.apply_video_adjustments(handle, settings)
    }

    /// Configures the display target peak in nits.
    pub fn set_target_peak(
        &self,
        handle: &MpvHandle,
        target_peak_nits: u32,
        settings: &mut VideoAdjustmentSettings,
    ) -> Result<(), MpvError> {
        settings.target_peak = target_peak_nits;
        self.apply_video_adjustments(handle, settings)
    }

    // Watchdog and Recovery delegation
    pub fn trigger_decoder_fallback(
        &self,
        handle: &MpvHandle,
        reason: DecoderFallbackReason,
    ) -> String {
        self.watchdog.trigger_fallback(handle, reason)
    }

    pub fn watchdog_status(&self) -> WatchdogStatus {
        self.watchdog.status()
    }

    pub fn take_notification(&self) -> Option<String> {
        self.watchdog.take_notification()
    }

    // Property update hooks
    pub fn on_hwdec_changed(&mut self, new_hwdec: &str) {
        self.current_hwdec = new_hwdec.to_string();
        self.watchdog.on_hwdec_changed(new_hwdec);
    }

    pub fn on_file_loaded(&mut self, handle: &MpvHandle) {
        self.watchdog.reset_for_new_media();
        // A new file has its own counter; the previous file's value must not be
        // mistaken for this one's until the engine reports again.
        self.observed_drop_count_known.store(false, Ordering::Release);
        if let Ok(w) = handle.get_property::<i64>("width") {
            self.video_width = w;
        }
        if let Ok(h) = handle.get_property::<i64>("height") {
            self.video_height = h;
        }
        if let Ok(fmt) = handle.get_property_string("video-format") {
            self.video_format = fmt;
        }
    }

    pub fn on_video_dimensions_changed(&mut self, width: i64, height: i64) {
        self.video_width = width;
        self.video_height = height;
    }

    pub fn on_video_format_changed(&mut self, format: String) {
        self.video_format = format;
    }

    pub fn on_panscan_changed(&mut self, panscan: f64) {
        self.panscan = panscan;
    }
}

#[cfg(test)]
mod geometry_tests {
    use super::*;

    #[test]
    fn observed_drop_count_is_unknown_until_the_engine_reports_one() {
        let v = VideoSubsystem::new();
        // "Not yet observed" must not be laundered into a real zero: the
        // watchdog would then judge a stall from a counter it never read.
        assert_eq!(v.observed_frame_drop_count(), None);
        v.on_frame_drop_count_changed(0);
        assert_eq!(v.observed_frame_drop_count(), Some(0));
        v.on_frame_drop_count_changed(7);
        assert_eq!(v.observed_frame_drop_count(), Some(7));
    }

    #[test]
    fn new_media_clears_the_observed_drop_count() {
        let v = VideoSubsystem::new();
        v.on_frame_drop_count_changed(42);
        assert_eq!(v.observed_frame_drop_count(), Some(42));
        v.observed_drop_count_known.store(false, Ordering::Release);
        assert_eq!(v.observed_frame_drop_count(), None);
    }

    #[test]
    fn crop_default_and_unknown_sizes_produce_no_rectangle() {
        assert_eq!(crop_rect_for_ratio(CropPreset::Default, 1920, 1080), "");
        assert_eq!(crop_rect_for_ratio(CropPreset::R16x9, 0, 1080), "");
        assert_eq!(crop_rect_for_ratio(CropPreset::R16x9, 1920, 0), "");
    }

    #[test]
    fn wider_target_crop_keeps_width_and_centres_height() {
        // 16:9 source cropped to 21:9 keeps the full width and cuts the height.
        let rect = crop_rect_for_ratio(CropPreset::R21x9, 1920, 1080);
        assert_eq!(rect, "1920x822+0+129");
    }

    #[test]
    fn narrower_target_crop_keeps_height_and_centres_width() {
        // 16:9 source cropped to 4:3 keeps the full height and cuts the width.
        let rect = crop_rect_for_ratio(CropPreset::R4x3, 1920, 1080);
        assert_eq!(rect, "1440x1080+240+0");
    }

    #[test]
    fn square_crop_is_centred_and_even() {
        let rect = crop_rect_for_ratio(CropPreset::R1x1, 1919, 1081);
        assert_eq!(rect, "1080x1080+419+0");
    }

    #[test]
    fn every_scale_and_crop_preset_has_a_label() {
        assert_eq!(ScalingMode::ALL.len(), 4);
        assert_eq!(CropPreset::ALL.len(), 10);
        for mode in ScalingMode::ALL {
            assert!(!mode.label().is_empty());
            assert!(!mode.description().is_empty());
        }
        for preset in CropPreset::ALL {
            assert!(!preset.label().is_empty());
        }
    }
}
