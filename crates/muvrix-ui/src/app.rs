//! Desktop application window and playback UI implemented with eframe and egui_glow.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText, Sense, Stroke, Vec2};
use glow::HasContext as _;
use muvrix_player::{
    Event, MpvHandle, MpvRenderContext, PlaybackController, PlaybackState, SeekMode, TrackType,
};

use crate::gl_loader;
use crate::render_engine::RenderEngine;

/// Main Muvrix desktop player application.
pub struct MuvrixApp {
    controller: PlaybackController,
    render_engine: Arc<Mutex<RenderEngine>>,
    gl: Arc<glow::Context>,

    // Hardware and OpenGL diagnostics
    gpu_vendor: String,
    gpu_renderer: String,
    gl_version: String,

    // UI and interaction state
    is_seeking: bool,
    seek_target: f64,
    status_message: String,
    path_input: String,
    is_fullscreen: bool,

    // Frame timing and UI responsiveness measurements
    last_frame_instant: Instant,
    frame_durations: VecDeque<Duration>,
    fps_measured: f64,
    frame_time_ms: f64,
    spinner_angle: f32,
}

impl MuvrixApp {
    /// Creates a new `MuvrixApp` instance inside eframe's creation callback.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Result<Self, Box<dyn std::error::Error>> {
        log::info!("Initializing MuvrixApp OpenGL 3.3+ Desktop Shell");

        // 1. Inspect OpenGL context and GPU diagnostics
        let gl = cc
            .gl
            .clone()
            .ok_or("Glow OpenGL context was not provided by eframe")?;
        let (gpu_vendor, gpu_renderer, gl_version) = unsafe {
            let vendor = gl.get_parameter_string(glow::VENDOR);
            let renderer = gl.get_parameter_string(glow::RENDERER);
            let version = gl.get_parameter_string(glow::VERSION);
            log::info!("GPU Vendor:   {vendor}");
            log::info!("GPU Renderer: {renderer}");
            log::info!("GL Version:   {version}");
            (vendor, renderer, version)
        };

        // 2. Initialize libmpv handle with required options
        let mpv = Arc::new(MpvHandle::new()?);
        mpv.set_option_string("vo", "libmpv")?;
        mpv.set_option_string("hwdec", "auto-safe")?;
        mpv.set_option_string("keepaspect", "yes")?;
        mpv.set_option_string("video-timing-offset", "0")?;
        mpv.initialize()?;

        // 3. Initialize libmpv OpenGL render context
        let get_proc_address = cc
            .get_proc_address
            .ok_or("GL get_proc_address required for libmpv")?;
        let mpv_render_ctx = unsafe {
            MpvRenderContext::new_gl(
                &mpv,
                Some(gl_loader::gl_get_proc_address_trampoline),
                &get_proc_address as *const _ as *mut std::ffi::c_void,
            )?
        };

        // 4. Create RenderEngine with off-screen FBO and quad compositor
        let mut render_engine = unsafe {
            RenderEngine::new(&gl, mpv_render_ctx, 1280, 720)
                .map_err(|e| format!("Failed to create RenderEngine: {e}"))?
        };

        // 5. Connect mpv update callback to request eframe repaints
        let egui_ctx = cc.egui_ctx.clone();
        render_engine
            .mpv_render_ctx_mut()
            .set_update_callback(move || {
                egui_ctx.request_repaint();
            });

        let render_engine = Arc::new(Mutex::new(render_engine));

        // 6. Wrap in PlaybackController
        let controller = PlaybackController::new(mpv)?;

        Ok(Self {
            controller,
            render_engine,
            gl,
            gpu_vendor,
            gpu_renderer,
            gl_version,
            is_seeking: false,
            seek_target: 0.0,
            status_message: "Ready. Select a test video to begin playback.".to_string(),
            last_frame_instant: Instant::now(),
            frame_durations: VecDeque::with_capacity(60),
            fps_measured: 0.0,
            frame_time_ms: 0.0,
            spinner_angle: 0.0,
            path_input: String::new(),
            is_fullscreen: false,
        })
    }

    /// Loads a video file or URL into the playback controller, resolving relative paths,
    /// home directories (~), and candidate fixtures.
    pub fn load_file(&mut self, raw_path: &str) {
        let trimmed = raw_path.trim();
        if trimmed.is_empty() {
            return;
        }

        log::info!("Resolving media path: {trimmed}");
        match muvrix_core::path::resolve_media_path(trimmed) {
            Ok(resolved) => {
                let path_str = resolved.to_string_lossy().to_string();
                log::info!("Resolved media path -> {path_str}");
                self.path_input = path_str.clone();
                match self.controller.load_file(&path_str) {
                    Ok(()) => {
                        self.status_message = format!("Loading: {path_str}");
                    }
                    Err(e) => {
                        let err_msg = format!("Failed to initiate playback on '{path_str}': {e}");
                        self.status_message = err_msg.clone();
                        self.controller.set_error(err_msg);
                        log::error!("Controller failed to load {path_str}: {e}");
                    }
                }
            }
            Err(e) => {
                self.path_input = trimmed.to_string();
                let err_msg = format!("File error: {e}");
                self.status_message = err_msg.clone();
                self.controller.set_error(err_msg);
                log::error!("Failed to resolve media path '{trimmed}': {e}");
            }
        }
    }

    /// Resolves and loads the first existing preset fixture from a list of candidate paths.
    pub fn load_preset(&mut self, candidates: &[&str]) {
        match muvrix_core::path::resolve_preset(candidates) {
            Ok(resolved) => {
                let path_str = resolved.to_string_lossy().to_string();
                self.load_file(&path_str);
            }
            Err(e) => {
                let err_msg = format!("Preset unavailable: {e}");
                self.status_message = err_msg.clone();
                self.controller.set_error(err_msg);
                log::warn!("Preset fixture missing: {e}");
            }
        }
    }

    /// Toggles play/pause state.
    pub fn toggle_pause(&mut self) {
        let _ = self.controller.toggle_pause();
    }

    /// Seeks relative to current playback position by `seconds`.
    pub fn seek_relative(&mut self, seconds: f64) {
        let _ = self.controller.seek(seconds, SeekMode::Relative);
    }

    /// Updates playback properties and polls events from mpv.
    fn sync_playback_state(&mut self) {
        let events = self.controller.poll_events();
        for event in events {
            match event {
                Event::FileLoaded => {
                    let (w, h) = self.controller.video_dimensions();
                    let vfmt = self.controller.video_format();
                    let afmt = self.controller.audio_codec();
                    let ao = self.controller.current_ao();
                    let hw = self.controller.hwdec_current();
                    log::info!(
                        "File loaded: {w}x{h} [video: {vfmt}, audio: {afmt}, ao: {ao}, hwdec: {hw}]"
                    );
                    self.status_message =
                        format!("Playback active: {vfmt} ({w}x{h}) | HWDEC: {hw} | AO: {ao}");
                }
                Event::EndFile { reason, error, .. } => {
                    if let Some(err) = error {
                        let path = self.controller.current_file().unwrap_or("<unknown>");
                        self.status_message = format!("Playback error on '{path}': {err}");
                    } else if reason == muvrix_player::EndFileReason::Eof {
                        self.status_message = "End of file reached".to_string();
                    } else {
                        self.status_message = format!("Playback stopped: {reason:?}");
                    }
                }
                Event::PlaybackRestart => {
                    self.status_message = "Playback active".to_string();
                }
                _ => {}
            }
        }
    }

    /// Measures actual frame presentation rate and frame delta times.
    fn update_frame_timing(&mut self) {
        let now = Instant::now();
        let delta = now.duration_since(self.last_frame_instant);
        self.last_frame_instant = now;

        let is_playing = *self.controller.state() == PlaybackState::Playing;

        if is_playing {
            if self.frame_durations.len() >= 60 {
                self.frame_durations.pop_front();
            }
            self.frame_durations.push_back(delta);

            let total: Duration = self.frame_durations.iter().sum();
            let count = self.frame_durations.len() as f64;
            if count > 0.0 && total.as_secs_f64() > 0.0 {
                self.frame_time_ms = (total.as_secs_f64() * 1000.0) / count;
                self.fps_measured = count / total.as_secs_f64();
            }
            self.spinner_angle = (self.spinner_angle + 0.08) % (2.0 * std::f32::consts::PI);
        } else {
            // When not actively playing, eframe runs in reactive mode where repaints occur
            // only on OS input events or explicit redraw triggers.
            self.frame_time_ms = delta.as_secs_f64() * 1000.0;
            self.frame_durations.clear();
            self.fps_measured = 0.0;
        }
    }

    /// Handles keyboard shortcuts for playback control.
    fn handle_keyboard_inputs(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.toggle_pause();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
            self.seek_relative(-5.0);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
            self.seek_relative(5.0);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            let new_vol = (self.controller.volume() + 5.0).min(100.0);
            let _ = self.controller.set_volume(new_vol);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            let new_vol = (self.controller.volume() - 5.0).max(0.0);
            let _ = self.controller.set_volume(new_vol);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F11)) {
            self.is_fullscreen = !self.is_fullscreen;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.is_fullscreen));
        }
    }
}

impl eframe::App for MuvrixApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.sync_playback_state();
        self.update_frame_timing();
        self.handle_keyboard_inputs(ctx);

        // Keep UI animating if actively playing
        if *self.controller.state() == PlaybackState::Playing {
            ctx.request_repaint_after(Duration::from_millis(16));
        }

        // Apply dark aesthetic theme
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(18, 18, 24);
        ctx.set_visuals(visuals);

        // 1. Diagnostics Header Panel
        egui::TopBottomPanel::top("diagnostics_header")
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("MUVRIX")
                            .strong()
                            .color(Color32::from_rgb(90, 180, 255)),
                    );
                    ui.separator();
                    ui.label(format!(
                        "GL: {} | Renderer: {} | Vendor: {}",
                        self.gl_version, self.gpu_renderer, self.gpu_vendor
                    ));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Responsiveness spinner
                        let is_playing = *self.controller.state() == PlaybackState::Playing;
                        let spinner_color = if is_playing {
                            Color32::from_rgb(0, 255, 128)
                        } else {
                            Color32::from_gray(120)
                        };

                        let (rect, _) =
                            ui.allocate_exact_size(Vec2::new(16.0, 16.0), Sense::hover());
                        let center = rect.center();
                        let radius = 6.0;
                        let tip = center
                            + Vec2::new(self.spinner_angle.cos(), self.spinner_angle.sin())
                                * radius;
                        ui.painter().circle_stroke(
                            center,
                            radius,
                            Stroke::new(1.5_f32, Color32::from_gray(80)),
                        );
                        ui.painter().line_segment(
                            [center, tip],
                            Stroke::new(2.0_f32, spinner_color),
                        );

                        let (fps_text, text_color) = match self.controller.state() {
                            PlaybackState::Playing => {
                                let source_fps = self
                                    .controller
                                    .container_fps()
                                    .or_else(|| self.controller.estimated_vf_fps())
                                    .unwrap_or(0.0);
                                let drops = self.controller.frame_drop_count();
                                if source_fps > 0.0 {
                                    (
                                        format!(
                                            "UI: {:.1} FPS ({:.1} ms) | Video: {:.1} fps ({} drops)",
                                            self.fps_measured, self.frame_time_ms, source_fps, drops
                                        ),
                                        Color32::from_rgb(0, 255, 128),
                                    )
                                } else {
                                    (
                                        format!(
                                            "UI: {:.1} FPS ({:.1} ms)",
                                            self.fps_measured, self.frame_time_ms
                                        ),
                                        Color32::from_rgb(0, 255, 128),
                                    )
                                }
                            }
                            PlaybackState::Loading => {
                                ("UI: Reactive (Loading...)".to_string(), Color32::from_rgb(255, 200, 50))
                            }
                            PlaybackState::Paused => {
                                ("UI: Reactive (Paused)".to_string(), Color32::from_rgb(255, 200, 50))
                            }
                            PlaybackState::Buffering => {
                                ("UI: Reactive (Buffering...)".to_string(), Color32::from_rgb(255, 160, 0))
                            }
                            PlaybackState::Ended => {
                                ("UI: Reactive (Ended)".to_string(), Color32::from_gray(160))
                            }
                            PlaybackState::Error(_) => {
                                ("UI: Reactive (Error)".to_string(), Color32::from_rgb(255, 80, 80))
                            }
                            PlaybackState::Idle => {
                                ("UI: Reactive (Idle)".to_string(), Color32::from_gray(160))
                            }
                        };

                        ui.label(
                            RichText::new(fps_text)
                                .monospace()
                                .color(text_color),
                        );
                    });
                });
            });

        // 2. Playback & Test Controls Panel
        egui::TopBottomPanel::bottom("playback_controls")
            .resizable(false)
            .show(ctx, |ui| {
                ui.add_space(4.0);

                // Seek bar and timing
                let time_pos = self.controller.time_pos();
                let duration = self.controller.duration();

                ui.horizontal(|ui| {
                    let cur_str = format_timestamp(time_pos);
                    let dur_str = format_timestamp(duration);
                    ui.label(RichText::new(cur_str).monospace());

                    let max_dur = duration.max(0.1);
                    let mut slider_val = if self.is_seeking {
                        self.seek_target
                    } else {
                        time_pos
                    };
                    let response = ui.add(
                        egui::Slider::new(&mut slider_val, 0.0..=max_dur)
                            .show_value(false)
                            .trailing_fill(true),
                    );

                    if response.drag_started() {
                        self.is_seeking = true;
                        self.seek_target = slider_val;
                    }
                    if response.dragged() {
                        self.seek_target = slider_val;
                    }
                    if response.drag_stopped() {
                        let _ = self
                            .controller
                            .seek(self.seek_target, SeekMode::Absolute);
                        self.is_seeking = false;
                    }

                    ui.label(RichText::new(dur_str).monospace());
                });

                ui.add_space(2.0);

                // Primary controls row
                ui.horizontal(|ui| {
                    let play_pause_icon = match self.controller.state() {
                        PlaybackState::Playing => "⏸ Pause",
                        PlaybackState::Buffering => "⏳ Buffering",
                        PlaybackState::Loading => "⏳ Loading",
                        _ => "▶ Play",
                    };
                    if ui.button(play_pause_icon).clicked() {
                        self.toggle_pause();
                    }

                    if ui.button("⏹ Stop").clicked() {
                        let _ = self.controller.stop();
                    }

                    if ui.button("⏮ -10s").clicked() {
                        self.seek_relative(-10.0);
                    }
                    if ui.button("⏭ +10s").clicked() {
                        self.seek_relative(10.0);
                    }

                    ui.separator();

                    // Playback speed presets
                    ui.label("Speed:");
                    let current_speed = self.controller.speed();
                    for &preset in &[0.5, 1.0, 1.25, 1.5, 2.0] {
                        let text = format!("{preset}x");
                        let is_current = (current_speed - preset).abs() < 0.01;
                        if ui.selectable_label(is_current, text).clicked() {
                            let _ = self.controller.set_speed(preset);
                        }
                    }

                    ui.separator();

                    // Volume slider & mute
                    let mute_label = if self.controller.is_muted() { "🔇" } else { "🔊" };
                    if ui.button(mute_label).clicked() {
                        let _ = self.controller.toggle_mute();
                    }

                    let mut vol = self.controller.volume();
                    if ui
                        .add(egui::Slider::new(&mut vol, 0.0..=100.0).show_value(true))
                        .changed()
                    {
                        let _ = self.controller.set_volume(vol);
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let fs_label = if self.is_fullscreen {
                            "🗗 Window"
                        } else {
                            "⛶ Fullscreen"
                        };
                        if ui.button(fs_label).clicked() {
                            self.is_fullscreen = !self.is_fullscreen;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(
                                self.is_fullscreen,
                            ));
                        }
                    });
                });

                ui.separator();

                // Track and Subtitle Selection Row
                ui.horizontal(|ui| {
                    // Audio track selector
                    let audio_tracks: Vec<_> = self
                        .controller
                        .tracks_by_type(TrackType::Audio)
                        .into_iter()
                        .cloned()
                        .collect();
                    let selected_audio_id = self
                        .controller
                        .selected_track(TrackType::Audio)
                        .map(|t| t.id);

                    ui.label("Audio:");
                    let mut audio_name = "None".to_string();
                    if let Some(id) = selected_audio_id {
                        if let Some(t) = audio_tracks.iter().find(|t| t.id == id) {
                            audio_name = format!(
                                "ID {}: {} ({})",
                                t.id,
                                t.title.as_deref().unwrap_or(t.lang.as_deref().unwrap_or("track")),
                                t.codec.as_deref().unwrap_or("pcm")
                            );
                        }
                    }

                    egui::ComboBox::from_id_salt("audio_select_combo")
                        .selected_text(audio_name)
                        .show_ui(ui, |ui| {
                            for track in &audio_tracks {
                                let label = format!(
                                    "Track {}: {} [{}]",
                                    track.id,
                                    track.title.as_deref().unwrap_or(track.lang.as_deref().unwrap_or("default")),
                                    track.codec.as_deref().unwrap_or("audio")
                                );
                                let is_selected = selected_audio_id == Some(track.id);
                                if ui.selectable_label(is_selected, label).clicked() {
                                    let _ = self.controller.set_audio_track(Some(track.id));
                                }
                            }
                        });

                    ui.separator();

                    // Subtitle track selector
                    let sub_tracks: Vec<_> = self
                        .controller
                        .tracks_by_type(TrackType::Subtitle)
                        .into_iter()
                        .cloned()
                        .collect();
                    let selected_sub_id = self
                        .controller
                        .selected_track(TrackType::Subtitle)
                        .map(|t| t.id);

                    ui.label("Subtitles:");
                    let mut sub_name = "Off".to_string();
                    if let Some(id) = selected_sub_id {
                        if let Some(t) = sub_tracks.iter().find(|t| t.id == id) {
                            sub_name = format!(
                                "ID {}: {} ({})",
                                t.id,
                                t.title.as_deref().unwrap_or(t.lang.as_deref().unwrap_or("sub")),
                                t.codec.as_deref().unwrap_or("")
                            );
                        }
                    }

                    egui::ComboBox::from_id_salt("sub_select_combo")
                        .selected_text(sub_name)
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(selected_sub_id.is_none(), "Off (Disabled)").clicked() {
                                let _ = self.controller.set_subtitle_track(None);
                            }
                            for track in &sub_tracks {
                                let label = format!(
                                    "Track {}: {} [{}]",
                                    track.id,
                                    track.title.as_deref().unwrap_or(track.lang.as_deref().unwrap_or("track")),
                                    track.codec.as_deref().unwrap_or("")
                                );
                                let is_selected = selected_sub_id == Some(track.id);
                                if ui.selectable_label(is_selected, label).clicked() {
                                    let _ = self.controller.set_subtitle_track(Some(track.id));
                                }
                            }
                        });

                    // Subtitle Delay Controls
                    let cur_sub_delay = self.controller.sub_delay();
                    ui.label(format!("Sub Delay: {:.2}s", cur_sub_delay));
                    if ui.button("-0.25s").clicked() {
                        let _ = self.controller.set_subtitle_delay(cur_sub_delay - 0.25);
                    }
                    if ui.button("0.0s").clicked() {
                        let _ = self.controller.set_subtitle_delay(0.0);
                    }
                    if ui.button("+0.25s").clicked() {
                        let _ = self.controller.set_subtitle_delay(cur_sub_delay + 0.25);
                    }

                    // Subtitle Visibility Toggle
                    let mut vis = self.controller.sub_visibility();
                    if ui.checkbox(&mut vis, "Visible").changed() {
                        let _ = self.controller.set_subtitle_visibility(vis);
                    }
                });

                ui.separator();

                // Format Test Presets & File Chooser Row
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Test Presets:").strong());
                    if ui.button("H.264").clicked() {
                        self.load_preset(&[
                            "tests/MuvrixTestMedia/phase-0.4/A-core/h264-aac.mp4",
                            "tests/MuvrixTestMedia/baseline/h264-aac.mp4",
                        ]);
                    }
                    if ui.button("HEVC").clicked() {
                        self.load_preset(&[
                            "tests/MuvrixTestMedia/phase-0.4/A-core/hevc-main-aac.mp4",
                            "tests/MuvrixTestMedia/codecs/hevc-aac.mp4",
                        ]);
                    }
                    if ui.button("VP9").clicked() {
                        self.load_preset(&[
                            "tests/MuvrixTestMedia/phase-0.4/A-core/vp9-opus.webm",
                            "tests/MuvrixTestMedia/codecs/vp9-opus.webm",
                        ]);
                    }
                    if ui.button("AV1 1080p").clicked() {
                        self.load_preset(&[
                            "tests/MuvrixTestMedia/phase-0.4/generated/av1-1080p-svtav1-synthetic.webm",
                            "tests/MuvrixTestMedia/codecs/av1-1080p.webm",
                            "tests/MuvrixTestMedia/phase-0.4/A-core/av1-4k-cityhall.webm",
                        ]);
                    }
                    if ui.button("Multi-Audio MOV").clicked() {
                        self.load_preset(&[
                            "tests/MuvrixTestMedia/phase-0.4/D-edge-cases/multiple-audio-streams.mov",
                            "tests/MuvrixTestMedia/tracks/multiple-audio-streams.mov",
                        ]);
                    }
                    if ui.button("Video-Only").clicked() {
                        self.load_preset(&[
                            "tests/MuvrixTestMedia/phase-0.4/D-edge-cases/video-only.mp4",
                            "tests/MuvrixTestMedia/tracks/video-only.mp4",
                        ]);
                    }
                    if ui.button("Audio-Only FLAC").clicked() {
                        self.load_preset(&[
                            "tests/MuvrixTestMedia/phase-0.4/C-audio/flac-24bit-96khz.flac",
                            "tests/MuvrixTestMedia/audio/audio.flac",
                        ]);
                    }
                    if ui.button("Multi-Sub MKV").clicked() {
                        self.load_preset(&[
                            "tests/MuvrixTestMedia/phase-0.4/C-subtitles/embedded-srt-ass.mkv",
                            "tests/MuvrixTestMedia/tracks/synthetic-multi-sub.mkv",
                        ]);
                    }
                    if ui.button("Colony 7.1 MKV").clicked() {
                        self.load_preset(&[
                            "~/Videos/Colony.2026.1080p.BluRay.DUAL.DDP7.1.x264-PRiME.mkv",
                            "tests/MuvrixTestMedia/phase-0.4/C-audio/TrueHD Sample 1 (Final Fantasy VII Advent Children).mkv",
                        ]);
                    }

                    ui.separator();

                    if ui.button("📂 Open File...").clicked() {
                        if let Some(file) = rfd::FileDialog::new()
                            .add_filter(
                                "Media Files",
                                &[
                                    "mp4", "mkv", "avi", "webm", "mov", "flac", "mp3", "ts",
                                    "m4a", "wav", "aac", "ogg", "ac3", "dts",
                                ],
                            )
                            .pick_file()
                        {
                            self.load_file(&file.display().to_string());
                        }
                    }

                    if ui.button("💬 Add Subtitle...").clicked() {
                        if let Some(file) = rfd::FileDialog::new()
                            .add_filter("Subtitle Files", &["srt", "ass", "ssa", "vtt", "sub"])
                            .pick_file()
                        {
                            let path_str = file.display().to_string();
                            let _ = self.controller.add_subtitle_file(&path_str, true);
                        }
                    }

                    ui.label("Path:");
                    let text_response = ui.add(
                        egui::TextEdit::singleline(&mut self.path_input)
                            .hint_text("Enter media path or URL...")
                            .desired_width(220.0),
                    );
                    if (ui.button("Load").clicked()
                        || (text_response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))))
                        && !self.path_input.trim().is_empty()
                    {
                        let path = self.path_input.clone();
                        self.load_file(&path);
                    }
                });

                ui.separator();

                // Status bar details
                let (w, h) = self.controller.video_dimensions();
                let vfmt = self.controller.video_format();
                let afmt = self.controller.audio_codec();
                let hw = self.controller.hwdec_current();
                let ao = self.controller.current_ao();
                let state_str = format!("{:?}", self.controller.state());

                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("[State: {state_str}]"))
                            .strong()
                            .color(match self.controller.state() {
                                PlaybackState::Playing => Color32::from_rgb(0, 255, 128),
                                PlaybackState::Paused => Color32::from_rgb(255, 200, 50),
                                PlaybackState::Buffering => Color32::from_rgb(255, 160, 0),
                                PlaybackState::Error(_) => Color32::from_rgb(255, 60, 60),
                                _ => Color32::from_gray(180),
                            }),
                    );

                    ui.separator();
                    let status_color = match self.controller.state() {
                        PlaybackState::Error(_) => Color32::from_rgb(255, 120, 120),
                        _ => Color32::from_gray(200),
                    };
                    ui.label(RichText::new(&self.status_message).color(status_color));

                    if w > 0 || !vfmt.is_empty() || !afmt.is_empty() {
                        ui.separator();
                        ui.label(format!(
                            "Video: {vfmt} ({w}x{h}) | HWDEC: {hw} | Audio: {afmt} | AO: {ao}"
                        ));
                    }
                });
                ui.add_space(4.0);
            });

        // 3. Central Video Viewport with CallbackFn
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::BLACK))
            .show(ctx, |ui| {
                let available_size = ui.available_size();
                if available_size.x <= 1.0 || available_size.y <= 1.0 {
                    return;
                }

                let (rect, _response) = ui.allocate_exact_size(available_size, Sense::hover());

                let render_engine = self.render_engine.clone();
                let callback = egui_glow::CallbackFn::new(move |info, painter| {
                    let mut engine = render_engine.lock().unwrap();
                    let viewport_px = info.viewport_in_pixels();
                    unsafe {
                        engine.paint(
                            painter.gl(),
                            painter,
                            [
                                viewport_px.left_px,
                                viewport_px.from_bottom_px,
                                viewport_px.width_px,
                                viewport_px.height_px,
                            ],
                        );
                    }
                });

                ui.painter().add(egui::PaintCallback {
                    rect,
                    callback: Arc::new(callback),
                });
            });
    }

    fn on_exit(&mut self, gl: Option<&glow::Context>) {
        log::info!("Initiating safe teardown of MuvrixApp");
        if let Ok(mut engine) = self.render_engine.lock() {
            if let Some(gl_ctx) = gl {
                unsafe {
                    engine.destroy(gl_ctx);
                }
            } else {
                unsafe {
                    engine.destroy(&self.gl);
                }
            }
        }
        log::info!("MuvrixApp teardown complete");
    }
}

/// Formats seconds into standard HH:MM:SS or MM:SS presentation.
pub fn format_timestamp(seconds: f64) -> String {
    if seconds.is_nan() || seconds < 0.0 {
        return "00:00".to_string();
    }
    let total_secs = seconds.floor() as u64;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{hours:02}:{mins:02}:{secs:02}")
    } else {
        format!("{mins:02}:{secs:02}")
    }
}
