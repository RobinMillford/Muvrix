//! Video, Decoder Watchdog, Shader, and Frame-Stepping domain subsystem.

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
    pub fn check_watchdog(
        &self,
        handle: &MpvHandle,
        is_playing: bool,
        is_buffering: bool,
        current_pos: f64,
    ) -> Option<DecoderFallbackReason> {
        let drops = self.frame_drop_count(handle);
        self.watchdog
            .check_watchdog(handle, is_playing, is_buffering, current_pos, drops)
    }

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
