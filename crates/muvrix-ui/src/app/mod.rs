//! Desktop application window and playback UI implemented with eframe and egui_glow.

pub mod commands;
pub mod formatting;
pub mod init;
pub mod navigation_bar;
pub mod os_integration;
pub mod scanner_channel;
pub mod shortcuts;
pub mod state;

pub use formatting::{
    calculate_progress, format_codec_name, format_speed, format_timestamp, format_timestamp_ms,
    format_track_label, format_volume,
};
pub use state::{
    mini_player_window_state, sleep_deadline_for, LibrarySelection, MuvrixApp, PlaybackStats,
    SleepTimer,
};

use std::time::Duration;

use eframe::egui::{self, Color32, RichText, Stroke};
use muvrix_player::{AccentColor, PlaybackState, ThemeMode};

use crate::branding::BrandPalette;
use crate::views::AppView;

impl MuvrixApp {
    pub fn ensure_logo_texture(&mut self, ctx: &egui::Context) -> &egui::TextureHandle {
        if self.logo_texture.is_none() {
            let (rgba, width, height) = crate::branding::logo_rgba();
            let color_image =
                egui::ColorImage::from_rgba_unmultiplied([*width as usize, *height as usize], rgba);
            let tex = ctx.load_texture(
                "muvrix_brand_logo",
                color_image,
                egui::TextureOptions::LINEAR,
            );
            self.logo_texture = Some(tex);
        }
        self.logo_texture.as_ref().unwrap()
    }

    /// One frame: poll the engine, update state, and build the UI.
    ///
    /// Identical for both shells. `eframe::App::update` and the native runner
    /// both call this; only the surrounding window/swap ownership differs.
    pub fn frame_update(&mut self, ctx: &egui::Context) {
        self.update_count = self.update_count.wrapping_add(1);
        // Test-only: an unattended harness launches the window without user
        // interaction, and GNOME will leave it behind other windows, where the
        // compositor throttles it. Going fullscreen on the first frame is the
        // only way a scripted run can guarantee the surface is visible and
        // focused. Production never sets this variable.
        if self.update_count == 1 && std::env::var("MUVRIX_TEST_FULLSCREEN").is_ok() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
            log::warn!("TEST-ONLY: requesting fullscreen so the harness has a visible surface");
        }
        self.viewport_flags = ctx.input(|i| {
            let vp = i.viewport();
            (vp.focused, vp.minimized, vp.fullscreen)
        });

        // Poll background scanner channel
        let mut scan_finished = false;
        if let Some(ref rx) = self.active_scan_rx {
            while let Ok(progress) = rx.try_recv() {
                if progress.is_finished {
                    scan_finished = true;
                }
                self.scan_progress = Some(progress);
            }
        }
        if scan_finished {
            self.active_scan_cancel = None;
            self.refresh_library();
            self.start_library_enrichment(ctx);
        }
        self.poll_library_enrichment(ctx);

        self.sync_playback_state();
        self.dispatch_mpvex_commands();
        self.sync_desktop_integration();
        self.tick_sleep_timer();
        if matches!(self.sleep_timer, SleepTimer::Minutes(_)) {
            ctx.request_repaint_after(Duration::from_secs(1));
        }
        self.check_decoder_path();
        self.flush_telemetry();
        self.update_frame_timing();
        self.handle_keyboard_inputs(ctx);
        self.update_osd_autohide(ctx);

        // Handle drag and drop media files into the application
        ctx.input(|i| {
            if !i.raw.dropped_files.is_empty() {
                for file in &i.raw.dropped_files {
                    if let Some(ref path) = file.path {
                        let path_str = path.to_string_lossy().to_string();
                        if self.controller.playlist_count() == 0
                            && *self.controller.state() == PlaybackState::Idle
                        {
                            self.load_file(&path_str);
                        } else {
                            let _ = self.controller.playlist_append(&path_str);
                            self.status_message = format!("Appended to playlist: {path_str}");
                        }
                    }
                }
            }
        });

        // Optional automated window screenshot capture for verification
        if let Ok(capture_path) = std::env::var("MUVRIX_CAPTURE_WINDOW_PNG") {
            self.screenshot_frames += 1;
            if self.screenshot_frames < 25 {
                ctx.request_repaint();
            }
            let media_ready = self.controller.current_file().is_none()
                || self.controller.duration() > 0.0
                || self.screenshot_frames >= 30;
            let state_ready = *self.controller.state() == PlaybackState::Playing
                || *self.controller.state() == PlaybackState::Idle
                || *self.controller.state() == PlaybackState::Paused;

            if self.screenshot_frames >= 15
                && media_ready
                && state_ready
                && !self.screenshot_requested
            {
                self.screenshot_requested = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            } else if !self.screenshot_requested {
                ctx.request_repaint();
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
                            if std::env::var("MUVRIX_CAPTURE_AND_EXIT").is_ok() {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                std::process::exit(0);
                            }
                        }
                    }
                }
            });
        }

        // Keep UI animating during active playback
        if *self.controller.state() == PlaybackState::Playing {
            ctx.request_repaint_after(Duration::from_millis(16));
        }

        // Apply the persisted appearance mode and accent color.
        let mut visuals = match self.theme_mode {
            ThemeMode::Obsidian => {
                let mut visuals = egui::Visuals::dark();
                visuals.panel_fill = BrandPalette::OBSIDIAN;
                visuals.window_fill = BrandPalette::OBSIDIAN;
                visuals.extreme_bg_color = Color32::from_rgb(12, 19, 25);
                visuals.faint_bg_color = BrandPalette::ELEVATED_SURFACE;
                visuals.code_bg_color = Color32::from_rgb(14, 21, 28);
                visuals.window_stroke = Stroke::new(1.0_f32, BrandPalette::SLATE_BORDER);
                visuals.widgets.noninteractive.bg_fill = BrandPalette::ELEVATED_SURFACE;
                visuals.widgets.noninteractive.bg_stroke =
                    Stroke::new(1.0_f32, BrandPalette::SLATE_DIVIDER);
                visuals.widgets.noninteractive.fg_stroke =
                    Stroke::new(1.0_f32, BrandPalette::MUTED_TEXT);
                visuals.widgets.inactive.bg_fill = BrandPalette::ELEVATED_SURFACE;
                visuals.widgets.inactive.bg_stroke =
                    Stroke::new(1.0_f32, BrandPalette::SLATE_BORDER);
                visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, BrandPalette::IVORY_TEXT);
                visuals.widgets.hovered.bg_fill = BrandPalette::CARD_SURFACE;
                visuals.widgets.hovered.bg_stroke =
                    Stroke::new(1.0_f32, BrandPalette::ELECTRIC_LIME);
                visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, BrandPalette::IVORY_TEXT);
                visuals
            }
            ThemeMode::Dark => {
                let mut visuals = egui::Visuals::dark();
                visuals.panel_fill = Color32::from_rgb(18, 20, 26);
                visuals
            }
            ThemeMode::Light => egui::Visuals::light(),
            ThemeMode::Amoled => {
                let mut visuals = egui::Visuals::dark();
                let black = Color32::BLACK;
                visuals.panel_fill = black;
                visuals.window_fill = black;
                visuals.extreme_bg_color = black;
                visuals.faint_bg_color = black;
                visuals.code_bg_color = black;
                visuals
            }
        };
        let accent = match self.accent_color {
            AccentColor::Lime => BrandPalette::ELECTRIC_LIME,
            AccentColor::Blue => Color32::from_rgb(60, 150, 245),
            AccentColor::Purple => Color32::from_rgb(168, 85, 247),
            AccentColor::Teal => Color32::from_rgb(20, 184, 166),
            AccentColor::Green => Color32::from_rgb(74, 222, 128),
            AccentColor::Orange => Color32::from_rgb(249, 115, 22),
            AccentColor::Red => Color32::from_rgb(239, 68, 68),
        };
        visuals.selection.bg_fill = accent;
        visuals.selection.stroke = Stroke::new(1.0_f32, accent);
        visuals.hyperlink_color = accent;
        visuals.widgets.active.bg_fill = accent;
        if self.accent_color == AccentColor::Lime {
            visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, BrandPalette::OBSIDIAN);
        }
        ctx.set_visuals(visuals);

        if let Some(path) = self.crash_notice.clone() {
            let mut dismiss = false;
            egui::TopBottomPanel::top("crash_notice_banner").show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new("Muvrix recovered from an earlier crash. Report:")
                            .strong()
                            .color(Color32::from_rgb(251, 146, 60)),
                    );
                    ui.label(
                        RichText::new(path.display().to_string())
                            .monospace()
                            .size(11.0)
                            .color(Color32::from_gray(200)),
                    );
                    if ui.button("Dismiss").clicked() {
                        dismiss = true;
                    }
                });
            });
            if dismiss {
                if let Err(error) = crate::crash::acknowledge(&path) {
                    log::warn!(
                        "failed to acknowledge crash report {}: {error}",
                        path.display()
                    );
                }
                self.crash_notice = None;
            }
        }

        self.render_navigation_bar(ctx);

        if self.current_view != AppView::Player {
            self.render_mini_player_deck(ctx);
            match self.current_view {
                AppView::Home => self.render_home_view(ctx),
                AppView::Library => self.render_library_view(ctx),
                AppView::Browser => self.render_browser_view(ctx),
                AppView::Playlist => self.render_playlist_view(ctx),
                AppView::Lyrics => self.render_lyrics_view(ctx),
                AppView::Recent => self.render_recent_view(ctx),
                AppView::Settings => self.render_settings_view(ctx),
                AppView::Player => unreachable!(),
            }
        } else {
            self.render_player_view(ctx);
        }

        self.render_shortcuts_modal(ctx);
    }

    pub fn shutdown(&mut self, gl: Option<&glow::Context>) {
        log::info!("Initiating safe teardown of MuvrixApp");
        self.cancel_library_scan();
        self.cancel_library_enrichment();
        self.sleep_inhibitor.set_inhibited(false);
        self.sleep_inhibited = false;

        // Persist final playback position in history store
        if let Some(path) = self.controller.current_file() {
            let time_pos = self.controller.time_pos();
            let duration = self.controller.duration();
            if time_pos > 0.0 && duration > 0.0 {
                self.history_store.record_playback(path, time_pos, duration);
                let _ = self.history_store.save();
            }
        }
        self.save_media_settings();
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

impl eframe::App for MuvrixApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frame_update(ctx);
    }

    fn on_exit(&mut self, gl: Option<&glow::Context>) {
        self.shutdown(gl);
    }
}
