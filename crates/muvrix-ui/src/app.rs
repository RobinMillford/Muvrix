//! Desktop application window and playback UI implemented with eframe and egui_glow.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText, Sense, Stroke, Vec2};
use glow::HasContext as _;
use muvrix_player::{
    Event, MpvHandle, MpvRenderContext, PlaybackController, PlaybackState, SeekMode, TrackInfo,
    TrackType,
};

use crate::gl_loader;
use crate::icons::{self, Icon};
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
    show_shortcuts: bool,
    show_diagnostics: bool,
    screenshot_requested: bool,

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
            status_message: "Ready. Open a media file to begin playback.".to_string(),
            last_frame_instant: Instant::now(),
            frame_durations: VecDeque::with_capacity(60),
            fps_measured: 0.0,
            frame_time_ms: 0.0,
            spinner_angle: 0.0,
            path_input: String::new(),
            is_fullscreen: false,
            show_shortcuts: false,
            show_diagnostics: false,
            screenshot_requested: false,
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

    /// Opens the native file picker to select a media file.
    pub fn open_file_dialog(&mut self) {
        if let Some(file) = rfd::FileDialog::new()
            .set_title("Open Media File")
            .add_filter(
                "Media Files",
                &[
                    "mp4", "mkv", "webm", "avi", "mov", "flv", "ts", "m4v", "wmv", "mp3", "flac",
                    "wav", "m4a", "aac", "ogg", "opus", "ac3", "dts",
                ],
            )
            .pick_file()
        {
            let path_str = file.display().to_string();
            self.load_file(&path_str);
        }
    }

    /// Opens the native file picker to load an external subtitle file.
    pub fn add_subtitle_dialog(&mut self) {
        if let Some(file) = rfd::FileDialog::new()
            .set_title("Add Subtitle Track")
            .add_filter(
                "Subtitle Files",
                &["srt", "ass", "ssa", "vtt", "sub", "idx"],
            )
            .pick_file()
        {
            let path_str = file.display().to_string();
            let _ = self.controller.add_subtitle_file(&path_str, true);
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

    /// Toggles fullscreen window state.
    pub fn toggle_fullscreen(&mut self, ctx: &egui::Context) {
        self.is_fullscreen = !self.is_fullscreen;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.is_fullscreen));
    }

    /// Exits fullscreen mode if currently active.
    pub fn exit_fullscreen(&mut self, ctx: &egui::Context) {
        if self.is_fullscreen {
            self.is_fullscreen = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        }
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
            self.frame_time_ms = delta.as_secs_f64() * 1000.0;
            self.frame_durations.clear();
            self.fps_measured = 0.0;
        }
    }

    /// Handles keyboard shortcuts for desktop playback control.
    ///
    /// Checks `ctx.wants_keyboard_input()` to ensure shortcuts do not trigger
    /// while the user is actively typing in a text field.
    fn handle_keyboard_inputs(&mut self, ctx: &egui::Context) {
        // Do NOT intercept shortcuts if user is typing into an active text field!
        if ctx.wants_keyboard_input() {
            return;
        }

        // Space: Toggle Play/Pause
        if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.toggle_pause();
        }

        // Left / Right Arrow: Relative Seek (-5s / +5s)
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
            let _ = self.controller.seek_relative(-5.0);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
            let _ = self.controller.seek_relative(5.0);
        }

        // Up / Down Arrow: Volume adjustments (+5% / -5%)
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            let _ = self.controller.adjust_volume(5.0);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            let _ = self.controller.adjust_volume(-5.0);
        }

        // M: Mute / Unmute toggle
        if ctx.input(|i| i.key_pressed(egui::Key::M)) {
            let _ = self.controller.toggle_mute();
        }

        // F or F11: Fullscreen toggle
        if ctx.input(|i| i.key_pressed(egui::Key::F) || i.key_pressed(egui::Key::F11)) {
            self.toggle_fullscreen(ctx);
        }

        // Escape: Exit Fullscreen or dismiss modal
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if self.show_shortcuts {
                self.show_shortcuts = false;
            } else if self.is_fullscreen {
                self.exit_fullscreen(ctx);
            }
        }

        // [ and ]: Playback speed decrease / increase (-0.25x / +0.25x)
        if ctx.input(|i| i.key_pressed(egui::Key::OpenBracket)) {
            let _ = self.controller.adjust_speed(-0.25);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::CloseBracket)) {
            let _ = self.controller.adjust_speed(0.25);
        }

        // Backspace: Reset speed to 1.0x
        if ctx.input(|i| i.key_pressed(egui::Key::Backspace)) {
            let _ = self.controller.reset_speed();
        }

        // Ctrl+O (or Cmd+O): Open File dialog
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::O)) {
            self.open_file_dialog();
        }

        // F1: Toggle shortcuts help dialog
        if ctx.input(|i| i.key_pressed(egui::Key::F1)) {
            self.show_shortcuts = !self.show_shortcuts;
        }
    }
}

impl eframe::App for MuvrixApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.sync_playback_state();
        self.update_frame_timing();
        self.handle_keyboard_inputs(ctx);

        // Optional automated window screenshot capture for verification
        if let Ok(capture_path) = std::env::var("MUVRIX_CAPTURE_WINDOW_PNG") {
            if (*self.controller.state() == PlaybackState::Playing
                || *self.controller.state() == PlaybackState::Idle)
                && !self.screenshot_requested
            {
                self.screenshot_requested = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            }

            ctx.input(|i| {
                for event in &i.raw.events {
                    if let egui::Event::Screenshot { image, .. } = event {
                        let pixels: Vec<u8> =
                            image.pixels.iter().flat_map(|p| p.to_array()).collect();
                        if let Err(e) = image::save_buffer(
                            &capture_path,
                            &pixels,
                            image.size[0] as u32,
                            image.size[1] as u32,
                            image::ExtendedColorType::Rgba8,
                        ) {
                            log::error!("Failed to save window screenshot to {capture_path}: {e}");
                        } else {
                            log::info!("Successfully captured window screenshot to {capture_path}");
                        }
                    }
                }
            });
        }

        // Keep UI animating during active playback
        if *self.controller.state() == PlaybackState::Playing {
            ctx.request_repaint_after(Duration::from_millis(16));
        }

        // Apply sleek dark aesthetic theme
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(18, 20, 26);
        ctx.set_visuals(visuals);

        // ---------------------------------------------------------------------
        // 1. Top Identity & Player Header Panel
        // ---------------------------------------------------------------------
        egui::TopBottomPanel::top("player_top_bar")
            .resizable(false)
            .show(ctx, |ui| {
                ui.add_space(3.0);
                let available_w = ui.available_width();

                ui.horizontal(|ui| {
                    // Application branding / identity
                    ui.label(
                        RichText::new("MUVRIX")
                            .strong()
                            .size(13.5)
                            .color(Color32::from_rgb(80, 180, 255)),
                    );

                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    // User file actions with clean vector icons
                    let (open_text, subs_text) = if available_w > 720.0 {
                        ("Open File...", "Add Subs...")
                    } else {
                        ("Open", "Subs")
                    };

                    if icons::icon_text_button(
                        ui,
                        Icon::FolderOpen,
                        open_text,
                        "Open media file from disk (Ctrl+O)",
                    )
                    .clicked()
                    {
                        self.open_file_dialog();
                    }

                    if icons::icon_text_button(
                        ui,
                        Icon::Subtitles,
                        subs_text,
                        "Load external subtitle file (SRT, ASS, VTT)",
                    )
                    .clicked()
                    {
                        self.add_subtitle_dialog();
                    }

                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    // Media file title / label with collision protection
                    let file_label = self
                        .controller
                        .current_file()
                        .map(|path| {
                            std::path::Path::new(path)
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or(path)
                                .to_string()
                        })
                        .unwrap_or_else(|| "No media loaded".to_string());

                    let is_loaded = self.controller.current_file().is_some();
                    let label_color = if is_loaded {
                        Color32::from_gray(230)
                    } else {
                        Color32::from_gray(140)
                    };

                    // Compute reserved space for right-hand actions to prevent collisions at narrow widths
                    let (w, h) = self.controller.video_dimensions();
                    let vfmt = self.controller.video_format().to_string();
                    let afmt = self.controller.audio_codec().to_string();
                    let hw = self.controller.hwdec_current().to_string();
                    let show_badge = (w > 0 || !vfmt.is_empty()) && available_w >= 850.0;

                    let right_reserved = if show_badge {
                        440.0
                    } else if available_w > 720.0 {
                        240.0
                    } else {
                        180.0
                    };
                    let max_title_w = (available_w - right_reserved).max(50.0);

                    ui.allocate_ui_with_layout(
                        Vec2::new(max_title_w, ui.available_height()),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&file_label)
                                        .strong()
                                        .color(label_color),
                                )
                                .truncate(),
                            )
                            .on_hover_text(&file_label);
                        },
                    );

                    // Right-aligned toolbar section
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Developer & Test Menu (moved out of user toolbar)
                        egui::ComboBox::from_id_salt("dev_menu_combo")
                            .selected_text("Developer")
                            .show_ui(ui, |ui| {
                                let diag_label = if self.show_diagnostics {
                                    "Hide Diagnostics Overlay"
                                } else {
                                    "Show Diagnostics Overlay"
                                };
                                if ui.button(diag_label).clicked() {
                                    self.show_diagnostics = !self.show_diagnostics;
                                }

                                ui.separator();
                                ui.label(
                                    RichText::new("Test Presets")
                                        .size(11.0)
                                        .color(Color32::from_rgb(180, 160, 100)),
                                );

                                if ui.button("H.264 (MP4)").clicked() {
                                    self.load_preset(&[
                                        "tests/MuvrixTestMedia/phase-0.4/A-core/h264-aac.mp4",
                                        "tests/MuvrixTestMedia/baseline/h264-aac.mp4",
                                    ]);
                                }
                                if ui.button("HEVC (MP4)").clicked() {
                                    self.load_preset(&[
                                        "tests/MuvrixTestMedia/phase-0.4/A-core/hevc-main-aac.mp4",
                                        "tests/MuvrixTestMedia/codecs/hevc-aac.mp4",
                                    ]);
                                }
                                if ui.button("VP9 (WebM)").clicked() {
                                    self.load_preset(&[
                                        "tests/MuvrixTestMedia/phase-0.4/A-core/vp9-opus.webm",
                                        "tests/MuvrixTestMedia/codecs/vp9-opus.webm",
                                    ]);
                                }
                                if ui.button("AV1 1080p (WebM)").clicked() {
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
                                if ui.button("Video-Only MP4").clicked() {
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
                                if ui.button("Surround 7.1 MKV").clicked() {
                                    self.load_preset(&[
                                        "tests/MuvrixTestMedia/phase-0.4/C-audio/surround-7.1-sample.mkv",
                                        "tests/MuvrixTestMedia/tracks/surround-7.1.mkv",
                                        "tests/MuvrixTestMedia/phase-0.4/C-audio/TrueHD Sample 1 (Final Fantasy VII Advent Children).mkv",
                                    ]);
                                }
                            });

                        ui.separator();

                        // Keyboard shortcuts help button
                        let help_text = if available_w > 800.0 {
                            "Shortcuts (F1)"
                        } else {
                            "Help"
                        };
                        if icons::icon_text_button(
                            ui,
                            Icon::Shortcuts,
                            help_text,
                            "View keyboard shortcuts (F1)",
                        )
                        .clicked()
                        {
                            self.show_shortcuts = !self.show_shortcuts;
                        }

                        // Media metadata badge if active and width allows
                        if show_badge {
                            ui.separator();
                            let v_clean = format_codec_name(&vfmt);
                            let a_clean = format_codec_name(&afmt);
                            let badge = if !a_clean.is_empty() && available_w > 940.0 {
                                format!("{}×{} · {} · {} · HW: {}", w, h, v_clean, a_clean, hw)
                            } else {
                                format!("{}×{} · {} · HW: {}", w, h, v_clean, hw)
                            };
                            ui.label(
                                RichText::new(badge)
                                    .monospace()
                                    .size(11.0)
                                    .color(Color32::from_rgb(115, 205, 255)),
                            );
                        }
                    });
                });
                ui.add_space(3.0);
            });

        // ---------------------------------------------------------------------
        // Optional Diagnostics Bar (when enabled via Developer menu)
        // ---------------------------------------------------------------------
        if self.show_diagnostics {
            egui::TopBottomPanel::top("diagnostics_subpanel")
                .resizable(false)
                .show(ctx, |ui| {
                    ui.add_space(2.0);
                    ui.horizontal_wrapped(|ui| {
                        icons::icon_label(
                            ui,
                            Icon::Diagnostics,
                            14.0,
                            Color32::from_rgb(255, 180, 50),
                        );
                        ui.label(
                            RichText::new("Developer Diagnostics")
                                .strong()
                                .color(Color32::from_rgb(255, 180, 50)),
                        );
                        ui.separator();
                        ui.label(
                            RichText::new(format!(
                                "GL: {} | Renderer: {} | Vendor: {}",
                                self.gl_version, self.gpu_renderer, self.gpu_vendor
                            ))
                            .monospace()
                            .color(Color32::from_gray(160)),
                        );

                        ui.separator();
                        let source_fps = self
                            .controller
                            .container_fps()
                            .or_else(|| self.controller.estimated_vf_fps())
                            .unwrap_or(0.0);
                        let drops = self.controller.frame_drop_count();
                        ui.label(
                            RichText::new(format!(
                                "UI: {:.1} FPS ({:.1} ms) | Video: {:.1} fps ({} drops)",
                                self.fps_measured, self.frame_time_ms, source_fps, drops
                            ))
                            .monospace()
                            .color(Color32::from_rgb(0, 255, 128)),
                        );

                        ui.separator();
                        ui.label("Path/URL:");
                        let text_response = ui.add(
                            egui::TextEdit::singleline(&mut self.path_input)
                                .hint_text("Enter media path or URL...")
                                .desired_width(180.0),
                        );
                        if (ui.button("Load").clicked()
                            || (text_response.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter))))
                            && !self.path_input.trim().is_empty()
                        {
                            let path = self.path_input.clone();
                            self.load_file(&path);
                        }

                        if ui.button("Dismiss [x]").clicked() {
                            self.show_diagnostics = false;
                        }
                    });
                    ui.add_space(2.0);
                });
        }

        // ---------------------------------------------------------------------
        // 2. Playback Controls Bottom Deck
        // ---------------------------------------------------------------------
        egui::TopBottomPanel::bottom("player_controls")
            .resizable(false)
            .show(ctx, |ui| {
                ui.add_space(4.0);

                let deck_width = ui.available_width();

                // -------------------------------------------------------------
                // Row 1: Dedicated Timeline Row
                // -------------------------------------------------------------
                let time_pos = self.controller.time_pos();
                let duration = self.controller.duration();

                ui.horizontal(|ui| {
                    let cur_str = format_timestamp(time_pos);
                    let dur_str = format_timestamp(duration);
                    let remaining_str = format_timestamp((duration - time_pos).max(0.0));

                    // Current position timestamp
                    ui.label(
                        RichText::new(&cur_str)
                            .monospace()
                            .size(12.0)
                            .color(Color32::from_gray(215)),
                    );

                    // Full-width seeker slider with hover timestamp tooltip
                    let max_dur = if duration > 0.0 { duration } else { 1.0 };
                    let mut slider_val = if self.is_seeking {
                        self.seek_target
                    } else {
                        time_pos
                    };

                    let slider_width = (ui.available_width() - 54.0).max(60.0);
                    let slider = egui::Slider::new(&mut slider_val, 0.0..=max_dur)
                        .show_value(false)
                        .trailing_fill(true);

                    let response = ui.add_sized([slider_width, 18.0], slider);
                    let response =
                        response.on_hover_text(format!("Seek to {}", format_timestamp(slider_val)));

                    if response.drag_started() {
                        self.is_seeking = true;
                        self.seek_target = slider_val;
                    }
                    if response.dragged() {
                        self.seek_target = slider_val;
                    }
                    if response.drag_stopped() {
                        let _ = self.controller.seek(self.seek_target, SeekMode::Absolute);
                        self.is_seeking = false;
                    }

                    // Duration timestamp
                    ui.label(
                        RichText::new(&dur_str)
                            .monospace()
                            .size(12.0)
                            .color(Color32::from_gray(185)),
                    )
                    .on_hover_text(format!(
                        "Duration: {} (Remaining: -{})",
                        dur_str, remaining_str
                    ));
                });

                ui.add_space(3.0);

                // -------------------------------------------------------------
                // Row 2: Transport Buttons, Speed, Volume, Tracks, Fullscreen
                // -------------------------------------------------------------
                ui.horizontal(|ui| {
                    // Group 1: Transport Controls (Primary Play/Pause, Stop, Seeks)
                    let (play_icon, play_tip) = match self.controller.state() {
                        PlaybackState::Playing => (Icon::Pause, "Pause (Space)"),
                        _ => (Icon::Play, "Play (Space)"),
                    };

                    // Primary Play/Pause button
                    if icons::primary_icon_button(ui, play_icon, Vec2::new(34.0, 26.0), play_tip)
                        .clicked()
                    {
                        self.toggle_pause();
                    }

                    ui.add_space(2.0);

                    // Stop button
                    if icons::icon_button_sized(
                        ui,
                        Icon::Stop,
                        Vec2::new(26.0, 26.0),
                        "Stop playback",
                    )
                    .clicked()
                    {
                        let _ = self.controller.stop();
                    }

                    ui.add_space(2.0);

                    // Relative seek -5s / +5s
                    if icons::icon_button_sized(
                        ui,
                        Icon::SeekBackward,
                        Vec2::new(28.0, 26.0),
                        "Seek backward 5s (Left Arrow)",
                    )
                    .clicked()
                    {
                        self.seek_relative(-5.0);
                    }

                    if icons::icon_button_sized(
                        ui,
                        Icon::SeekForward,
                        Vec2::new(28.0, 26.0),
                        "Seek forward 5s (Right Arrow)",
                    )
                    .clicked()
                    {
                        self.seek_relative(5.0);
                    }

                    // Chapter navigation if present
                    let chapters = self.controller.chapters().to_vec();
                    if !chapters.is_empty() {
                        ui.add_space(2.0);
                        if icons::icon_button(ui, Icon::SkipPrevious, "Previous Chapter").clicked()
                        {
                            let _ = self.controller.prev_chapter();
                        }
                        if icons::icon_button(ui, Icon::SkipNext, "Next Chapter").clicked() {
                            let _ = self.controller.next_chapter();
                        }

                        let cur_ch_idx = self.controller.current_chapter().unwrap_or(0);
                        let selected_title = chapters
                            .iter()
                            .find(|c| c.index == cur_ch_idx)
                            .map(|c| format!("Ch {}: {}", c.index + 1, c.title))
                            .unwrap_or_else(|| "Chapters".to_string());

                        egui::ComboBox::from_id_salt("chapters_combo")
                            .selected_text(selected_title)
                            .show_ui(ui, |ui| {
                                for ch in &chapters {
                                    let label = format!(
                                        "{}: {} ({})",
                                        ch.index + 1,
                                        ch.title,
                                        format_timestamp(ch.time)
                                    );
                                    let is_selected =
                                        Some(ch.index) == self.controller.current_chapter();
                                    if ui.selectable_label(is_selected, label).clicked() {
                                        let _ = self.controller.set_chapter(ch.index);
                                    }
                                }
                            });
                    }

                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    // Group 2: Volume & Mute Controls
                    let (mute_icon, mute_tip) = if self.controller.is_muted() {
                        (Icon::VolumeMuted, "Unmute Audio (M)")
                    } else if self.controller.volume() < 40.0 {
                        (Icon::VolumeLow, "Mute Audio (M)")
                    } else {
                        (Icon::VolumeHigh, "Mute Audio (M)")
                    };

                    if icons::icon_button_sized(ui, mute_icon, Vec2::new(26.0, 26.0), mute_tip)
                        .clicked()
                    {
                        let _ = self.controller.toggle_mute();
                    }

                    let mut vol = self.controller.volume();
                    let vol_label = format!("{:.0}%", vol);
                    let vol_slider = egui::Slider::new(&mut vol, 0.0..=100.0)
                        .show_value(false)
                        .text(vol_label);
                    let vol_w = if deck_width < 760.0 { 48.0 } else { 65.0 };
                    let vol_res = ui
                        .add_sized([vol_w, 18.0], vol_slider)
                        .on_hover_text(format!("Volume: {:.0}% (Up/Down Arrow)", vol));
                    if vol_res.changed() {
                        let _ = self.controller.set_volume(vol);
                    }

                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    // Group 3: Playback Speed Selector
                    let current_speed = self.controller.speed();
                    let speed_label = if (current_speed - 1.0).abs() < 0.01 {
                        "1.0x".to_string()
                    } else {
                        format!("{:.2}x", current_speed)
                    };

                    egui::ComboBox::from_id_salt("speed_combo")
                        .selected_text(speed_label)
                        .show_ui(ui, |ui| {
                            for &preset in &[0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0] {
                                let label = format!(
                                    "{preset}x{}",
                                    if preset == 1.0 { " (Normal)" } else { "" }
                                );
                                let is_current = (current_speed - preset).abs() < 0.01;
                                if ui.selectable_label(is_current, label).clicked() {
                                    let _ = self.controller.set_speed(preset);
                                }
                            }
                        });

                    if (current_speed - 1.0).abs() > 0.01
                        && ui
                            .button("1.0x")
                            .on_hover_text("Reset to normal speed (Backspace)")
                            .clicked()
                    {
                        let _ = self.controller.reset_speed();
                    }

                    // Group 4: Right-aligned Controls (Fullscreen, Subs, Audio)
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Fullscreen toggle button
                        let (fs_icon, fs_tip) = if self.is_fullscreen {
                            (Icon::Windowed, "Exit Fullscreen (F / Esc)")
                        } else {
                            (Icon::Fullscreen, "Enter Fullscreen (F / F11)")
                        };
                        if icons::icon_button_sized(ui, fs_icon, Vec2::new(28.0, 26.0), fs_tip)
                            .clicked()
                        {
                            self.toggle_fullscreen(ctx);
                        }

                        ui.add_space(4.0);
                        ui.separator();
                        ui.add_space(4.0);

                        let sub_tracks: Vec<TrackInfo> = self
                            .controller
                            .tracks_by_type(TrackType::Subtitle)
                            .into_iter()
                            .cloned()
                            .collect();
                        let selected_sub_id = self
                            .controller
                            .selected_track(TrackType::Subtitle)
                            .map(|t| t.id);

                        let audio_tracks: Vec<TrackInfo> = self
                            .controller
                            .tracks_by_type(TrackType::Audio)
                            .into_iter()
                            .cloned()
                            .collect();
                        let selected_audio_id = self
                            .controller
                            .selected_track(TrackType::Audio)
                            .map(|t| t.id);

                        // If width is constrained (<760px), consolidate into a single contextual "Tracks" menu
                        if deck_width < 760.0 {
                            egui::ComboBox::from_id_salt("tracks_combo_compact")
                                .selected_text("Tracks")
                                .show_ui(ui, |ui| {
                                    ui.label(
                                        RichText::new("Audio Track")
                                            .strong()
                                            .color(Color32::from_rgb(120, 200, 255)),
                                    );
                                    if ui
                                        .selectable_label(
                                            selected_audio_id.is_none(),
                                            "None (Mute Stream)",
                                        )
                                        .clicked()
                                    {
                                        let _ = self.controller.set_audio_track(None);
                                    }
                                    for track in &audio_tracks {
                                        let label = format_track_label(track);
                                        let is_selected = selected_audio_id == Some(track.id);
                                        if ui.selectable_label(is_selected, label).clicked() {
                                            let _ = self.controller.set_audio_track(Some(track.id));
                                        }
                                    }

                                    ui.separator();
                                    ui.label(
                                        RichText::new("Subtitle Track")
                                            .strong()
                                            .color(Color32::from_rgb(120, 200, 255)),
                                    );
                                    if ui
                                        .selectable_label(
                                            selected_sub_id.is_none(),
                                            "Off (Disabled)",
                                        )
                                        .clicked()
                                    {
                                        let _ = self.controller.set_subtitle_track(None);
                                    }
                                    for track in &sub_tracks {
                                        let label = format_track_label(track);
                                        let is_selected = selected_sub_id == Some(track.id);
                                        if ui.selectable_label(is_selected, label).clicked() {
                                            let _ =
                                                self.controller.set_subtitle_track(Some(track.id));
                                        }
                                    }

                                    ui.separator();
                                    if icons::icon_text_button(
                                        ui,
                                        Icon::FolderOpen,
                                        "Add Subtitle File...",
                                        "Load external subtitle file",
                                    )
                                    .clicked()
                                    {
                                        self.add_subtitle_dialog();
                                    }
                                });
                        } else {
                            // Subtitle track selector
                            let sub_summary = if let Some(id) = selected_sub_id {
                                if let Some(t) = sub_tracks.iter().find(|t| t.id == id) {
                                    let label = t
                                        .lang
                                        .as_deref()
                                        .unwrap_or(t.title.as_deref().unwrap_or("On"));
                                    if deck_width < 860.0 {
                                        format!("Sub: {label}")
                                    } else {
                                        format!("Subtitles: {label}")
                                    }
                                } else {
                                    "Sub: On".to_string()
                                }
                            } else if deck_width < 860.0 {
                                "Sub: Off".to_string()
                            } else {
                                "Subtitles: Off".to_string()
                            };

                            egui::ComboBox::from_id_salt("sub_combo")
                                .selected_text(sub_summary)
                                .show_ui(ui, |ui| {
                                    if ui
                                        .selectable_label(
                                            selected_sub_id.is_none(),
                                            "Off (Disabled)",
                                        )
                                        .clicked()
                                    {
                                        let _ = self.controller.set_subtitle_track(None);
                                    }
                                    for track in &sub_tracks {
                                        let label = format_track_label(track);
                                        let is_selected = selected_sub_id == Some(track.id);
                                        if ui.selectable_label(is_selected, label).clicked() {
                                            let _ =
                                                self.controller.set_subtitle_track(Some(track.id));
                                        }
                                    }

                                    ui.separator();
                                    if icons::icon_text_button(
                                        ui,
                                        Icon::FolderOpen,
                                        "Add Subtitle File...",
                                        "Load external subtitle file",
                                    )
                                    .clicked()
                                    {
                                        self.add_subtitle_dialog();
                                    }

                                    let cur_delay = self.controller.sub_delay();
                                    ui.horizontal(|ui| {
                                        ui.label(format!("Delay: {:.2}s", cur_delay));
                                        if ui.button("-0.25s").clicked() {
                                            let _ = self
                                                .controller
                                                .set_subtitle_delay(cur_delay - 0.25);
                                        }
                                        if ui.button("0s").clicked() {
                                            let _ = self.controller.set_subtitle_delay(0.0);
                                        }
                                        if ui.button("+0.25s").clicked() {
                                            let _ = self
                                                .controller
                                                .set_subtitle_delay(cur_delay + 0.25);
                                        }
                                    });

                                    let mut vis = self.controller.sub_visibility();
                                    if ui.checkbox(&mut vis, "Subtitle Visible").changed() {
                                        let _ = self.controller.set_subtitle_visibility(vis);
                                    }
                                });

                            ui.add_space(4.0);

                            // Audio track selector
                            let audio_summary = if let Some(id) = selected_audio_id {
                                if let Some(t) = audio_tracks.iter().find(|t| t.id == id) {
                                    let label = t
                                        .lang
                                        .as_deref()
                                        .unwrap_or(t.title.as_deref().unwrap_or("On"));
                                    format!("Audio: {label}")
                                } else {
                                    "Audio: On".to_string()
                                }
                            } else {
                                "Audio: None".to_string()
                            };

                            egui::ComboBox::from_id_salt("audio_combo")
                                .selected_text(audio_summary)
                                .show_ui(ui, |ui| {
                                    if ui
                                        .selectable_label(
                                            selected_audio_id.is_none(),
                                            "None (Mute Stream)",
                                        )
                                        .clicked()
                                    {
                                        let _ = self.controller.set_audio_track(None);
                                    }
                                    for track in &audio_tracks {
                                        let label = format_track_label(track);
                                        let is_selected = selected_audio_id == Some(track.id);
                                        if ui.selectable_label(is_selected, label).clicked() {
                                            let _ = self.controller.set_audio_track(Some(track.id));
                                        }
                                    }
                                });
                        }
                    });
                });

                ui.add_space(3.0);
            });

        // ---------------------------------------------------------------------
        // 3. Central Video Viewport with Interactive Overlay Cards
        // ---------------------------------------------------------------------
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::BLACK))
            .show(ctx, |ui| {
                let available_size = ui.available_size();
                if available_size.x <= 1.0 || available_size.y <= 1.0 {
                    return;
                }

                // Sense both clicks (play/pause) and double-clicks (fullscreen)
                let (rect, response) = ui.allocate_exact_size(available_size, Sense::click());

                if response.double_clicked() {
                    self.toggle_fullscreen(ctx);
                } else if response.clicked() && *self.controller.state() != PlaybackState::Idle {
                    self.toggle_pause();
                }

                // Render video quad via libmpv OpenGL FBO
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

                // Contextual Overlays for player states
                match self.controller.state() {
                    PlaybackState::Idle => {
                        let center_pos = rect.center();
                        let card_width = 410.0_f32.min(rect.width() - 32.0);
                        let card_height = 205.0_f32.min(rect.height() - 32.0);
                        let card_rect = egui::Rect::from_center_size(
                            center_pos,
                            Vec2::new(card_width, card_height),
                        );

                        // High contrast welcome card container
                        ui.painter().rect(
                            card_rect,
                            8.0_f32,
                            Color32::from_rgba_premultiplied(22, 25, 34, 235),
                            Stroke::new(1.0_f32, Color32::from_rgb(55, 68, 92)),
                            egui::StrokeKind::Inside,
                        );

                        let mut child_ui = ui.new_child(
                            egui::UiBuilder::new()
                                .max_rect(card_rect.shrink(18.0))
                                .layout(egui::Layout::top_down(egui::Align::Center)),
                        );
                        child_ui.add_space(6.0);
                        child_ui.label(
                            RichText::new("MUVRIX MEDIA PLAYER")
                                .strong()
                                .size(19.0)
                                .color(Color32::from_rgb(96, 175, 255)),
                        );
                        child_ui.add_space(3.0);
                        child_ui.label(
                            RichText::new("Lightweight hardware-accelerated desktop media player")
                                .size(12.5)
                                .color(Color32::from_gray(190)),
                        );
                        child_ui.add_space(14.0);

                        // Primary action button: Open Media File
                        if icons::primary_icon_button_text(
                            &mut child_ui,
                            Icon::FolderOpen,
                            "Open Media File",
                            "Open video or audio file from disk (Ctrl+O)",
                        )
                        .clicked()
                        {
                            self.open_file_dialog();
                        }

                        child_ui.add_space(10.0);
                        child_ui.label(
                            RichText::new("MP4 · MKV · WebM · AVI · MOV · FLAC · MP3 · AAC")
                                .size(11.5)
                                .color(Color32::from_gray(165)),
                        );
                        child_ui.add_space(2.0);
                        child_ui.label(
                            RichText::new("Drag and drop media or press Ctrl+O")
                                .size(10.5)
                                .color(Color32::from_gray(135)),
                        );
                    }
                    PlaybackState::Loading | PlaybackState::Buffering => {
                        let center_pos = rect.center();
                        let card_rect =
                            egui::Rect::from_center_size(center_pos, Vec2::new(300.0, 115.0));
                        ui.painter().rect(
                            card_rect,
                            6.0_f32,
                            Color32::from_rgba_premultiplied(18, 20, 28, 225),
                            Stroke::new(1.0_f32, Color32::from_rgb(60, 68, 85)),
                            egui::StrokeKind::Inside,
                        );
                        let mut child_ui = ui.new_child(
                            egui::UiBuilder::new()
                                .max_rect(card_rect.shrink(12.0))
                                .layout(egui::Layout::top_down(egui::Align::Center)),
                        );
                        child_ui.add_space(8.0);

                        let is_loading = *self.controller.state() == PlaybackState::Loading;
                        child_ui.label(
                            RichText::new(if is_loading {
                                "Loading Media..."
                            } else {
                                "Buffering..."
                            })
                            .strong()
                            .size(15.0)
                            .color(Color32::from_rgb(255, 205, 60)),
                        );
                        child_ui.add_space(6.0);
                        let file_hint = self
                            .controller
                            .current_file()
                            .map(|f| {
                                std::path::Path::new(f)
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or(f)
                            })
                            .unwrap_or("Please wait");
                        child_ui.label(
                            RichText::new(file_hint)
                                .size(12.0)
                                .color(Color32::from_gray(200)),
                        );
                    }
                    PlaybackState::Error(err) => {
                        let center_pos = rect.center();
                        let card_rect =
                            egui::Rect::from_center_size(center_pos, Vec2::new(460.0, 200.0));
                        ui.painter().rect(
                            card_rect,
                            8.0_f32,
                            Color32::from_rgba_premultiplied(35, 15, 15, 240),
                            Stroke::new(1.5_f32, Color32::from_rgb(220, 60, 60)),
                            egui::StrokeKind::Inside,
                        );

                        let mut child_ui = ui.new_child(
                            egui::UiBuilder::new()
                                .max_rect(card_rect.shrink(16.0))
                                .layout(egui::Layout::top_down(egui::Align::Center)),
                        );
                        child_ui.add_space(4.0);
                        child_ui.horizontal(|ui| {
                            icons::icon_label(
                                ui,
                                Icon::Warning,
                                18.0,
                                Color32::from_rgb(255, 80, 80),
                            );
                            ui.label(
                                RichText::new("Playback Error")
                                    .strong()
                                    .size(16.0)
                                    .color(Color32::from_rgb(255, 80, 80)),
                            );
                        });
                        child_ui.add_space(8.0);
                        child_ui.label(
                            RichText::new(err)
                                .size(12.0)
                                .color(Color32::from_rgb(240, 190, 190)),
                        );
                        child_ui.add_space(14.0);
                        child_ui.horizontal(|ui| {
                            if ui
                                .button(RichText::new("Open Another File...").strong())
                                .clicked()
                            {
                                self.open_file_dialog();
                            }
                            if ui.button("Dismiss").clicked() {
                                let _ = self.controller.stop();
                            }
                        });
                    }
                    PlaybackState::Ended => {
                        let center_pos = rect.center();
                        let card_rect =
                            egui::Rect::from_center_size(center_pos, Vec2::new(320.0, 135.0));
                        ui.painter().rect(
                            card_rect,
                            8.0_f32,
                            Color32::from_rgba_premultiplied(20, 22, 30, 230),
                            Stroke::new(1.0_f32, Color32::from_rgb(55, 65, 85)),
                            egui::StrokeKind::Inside,
                        );
                        let mut child_ui = ui.new_child(
                            egui::UiBuilder::new()
                                .max_rect(card_rect.shrink(14.0))
                                .layout(egui::Layout::top_down(egui::Align::Center)),
                        );
                        child_ui.add_space(6.0);
                        child_ui.label(
                            RichText::new("Playback Finished")
                                .strong()
                                .size(15.0)
                                .color(Color32::from_gray(230)),
                        );
                        child_ui.add_space(12.0);
                        child_ui.horizontal(|ui| {
                            if icons::icon_text_button(
                                ui,
                                Icon::Replay,
                                "Replay",
                                "Replay from start",
                            )
                            .clicked()
                            {
                                let _ = self.controller.seek(0.0, SeekMode::Absolute);
                                let _ = self.controller.play();
                            }
                            if icons::icon_text_button(
                                ui,
                                Icon::FolderOpen,
                                "Open File...",
                                "Open another media file",
                            )
                            .clicked()
                            {
                                self.open_file_dialog();
                            }
                        });
                    }
                    _ => {}
                }
            });

        // ---------------------------------------------------------------------
        // 4. Modal Shortcuts Dialog
        // ---------------------------------------------------------------------
        if self.show_shortcuts {
            let mut open = true;
            let mut close_requested = false;
            egui::Window::new("Keyboard Shortcuts")
                .open(&mut open)
                .resizable(false)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.set_width(360.0);
                    egui::Grid::new("shortcuts_grid")
                        .striped(true)
                        .spacing([20.0, 6.0])
                        .show(ui, |ui| {
                            ui.label(RichText::new("Space").strong().monospace());
                            ui.label("Play / Pause Toggle");
                            ui.end_row();

                            ui.label(RichText::new("Left / Right").strong().monospace());
                            ui.label("Seek -5s / +5s");
                            ui.end_row();

                            ui.label(RichText::new("Up / Down").strong().monospace());
                            ui.label("Volume +5% / -5%");
                            ui.end_row();

                            ui.label(RichText::new("M").strong().monospace());
                            ui.label("Mute / Unmute Audio");
                            ui.end_row();

                            ui.label(RichText::new("F / F11").strong().monospace());
                            ui.label("Toggle Fullscreen");
                            ui.end_row();

                            ui.label(RichText::new("Esc").strong().monospace());
                            ui.label("Exit Fullscreen / Dismiss");
                            ui.end_row();

                            ui.label(RichText::new("[ / ]").strong().monospace());
                            ui.label("Speed -0.25x / +0.25x");
                            ui.end_row();

                            ui.label(RichText::new("Backspace").strong().monospace());
                            ui.label("Reset Speed to 1.0x");
                            ui.end_row();

                            ui.label(RichText::new("Ctrl+O").strong().monospace());
                            ui.label("Open Media File");
                            ui.end_row();

                            ui.label(RichText::new("Double Click").strong().monospace());
                            ui.label("Toggle Fullscreen");
                            ui.end_row();

                            ui.label(RichText::new("Single Click").strong().monospace());
                            ui.label("Play / Pause (Video)");
                            ui.end_row();
                        });
                    ui.add_space(8.0);
                    if ui.button("Close").clicked() {
                        close_requested = true;
                    }
                });
            self.show_shortcuts = open && !close_requested;
        }
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
    if !seconds.is_finite() || seconds < 0.0 {
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

/// Calculates normalized progress fraction clamped to [0.0, 1.0].
pub fn calculate_progress(time_pos: f64, duration: f64) -> f32 {
    if duration <= 0.0 || time_pos <= 0.0 || time_pos.is_nan() || duration.is_nan() {
        0.0
    } else {
        ((time_pos / duration) as f32).clamp(0.0, 1.0)
    }
}

/// Formats playback speed factor into a concise display string.
pub fn format_speed(speed: f64) -> String {
    if (speed - 1.0).abs() < 0.001 {
        "1.0x (Normal)".to_string()
    } else {
        format!("{:.2}x", speed)
    }
}

/// Formats volume and mute state for user display.
pub fn format_volume(volume: f64, is_muted: bool) -> String {
    if is_muted {
        "Muted (0%)".to_string()
    } else {
        format!("{:.0}%", volume.clamp(0.0, 100.0))
    }
}

/// Formats a raw codec name into standard user-facing presentation.
pub fn format_codec_name(raw: &str) -> String {
    match raw.to_lowercase().as_str() {
        "h264" | "avc" | "avc1" => "H.264".to_string(),
        "hevc" | "h265" | "hev1" => "HEVC".to_string(),
        "vp9" => "VP9".to_string(),
        "av1" | "av01" => "AV1".to_string(),
        "vp8" => "VP8".to_string(),
        "aac" | "mp4a" => "AAC".to_string(),
        "opus" => "Opus".to_string(),
        "flac" => "FLAC".to_string(),
        "mp3" => "MP3".to_string(),
        "ac3" => "AC-3".to_string(),
        "eac3" => "E-AC-3".to_string(),
        "truehd" => "TrueHD".to_string(),
        "dts" => "DTS".to_string(),
        "vorbis" => "Vorbis".to_string(),
        other => other.to_string(),
    }
}

/// Formats a media track for user menus with descriptive labels.
pub fn format_track_label(track: &TrackInfo) -> String {
    let mut parts = Vec::new();
    if let Some(title) = &track.title {
        let t = title.trim();
        if !t.is_empty() {
            parts.push(t.to_string());
        }
    }
    if let Some(lang) = &track.lang {
        let l = lang.trim();
        if !l.is_empty() {
            let lower = l.to_lowercase();
            let lang_clean = match lower.as_str() {
                "eng" | "en" => "English",
                "jpn" | "ja" => "Japanese",
                "spa" | "es" => "Spanish",
                "fra" | "fr" => "French",
                "deu" | "ger" | "de" => "German",
                "ita" | "it" => "Italian",
                "por" | "pt" => "Portuguese",
                "rus" | "ru" => "Russian",
                "zho" | "chi" | "zh" => "Chinese",
                "kor" | "ko" => "Korean",
                _ => l,
            };
            if parts.is_empty() || !parts[0].to_lowercase().contains(&lower) {
                parts.push(lang_clean.to_string());
            }
        }
    }
    if let Some(codec) = &track.codec {
        let c = codec.trim();
        if !c.is_empty() {
            parts.push(format!("[{}]", format_codec_name(c)));
        }
    }

    if parts.is_empty() {
        format!("Track {}", track.id)
    } else {
        format!("Track {}: {}", track.id, parts.join(" · "))
    }
}
