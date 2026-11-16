use std::sync::Arc;

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Shape, Stroke, StrokeKind, Vec2};
use muvrix_player::{
    AbLoopState, AccentColor, PlaybackState, RepeatMode, SeekMode, SteppingPreset, ThemeMode,
    TrackInfo, TrackType,
};

use crate::app::{
    format_timestamp, format_timestamp_ms, format_track_label, MuvrixApp, SleepTimer,
};
use crate::branding::{self, BrandPalette};
use crate::icons::{self, Icon};

impl MuvrixApp {
    pub fn render_player_view(&mut self, ctx: &egui::Context) {
        self.render_playlist_drawer(ctx);

        // -----------------------------------------------------------------
        // Optional Diagnostics Bar (when enabled via Developer menu)
        // -----------------------------------------------------------------
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

                        // Observed (never requested) decode + presentation state.
                        let decode_line = self
                            .decode_observation
                            .as_ref()
                            .map(|o| o.describe())
                            .unwrap_or_else(|| "Decoder: not yet observed".to_string());
                        ui.label(
                            RichText::new(format!(
                                "{decode_line} | native display: {}",
                                self.native_display_kind.unwrap_or("none")
                            ))
                            .monospace()
                            .color(Color32::from_rgb(120, 200, 255)),
                        );

                        let watchdog_status = self.controller.video().watchdog_status();
                        match watchdog_status {
                            muvrix_player::WatchdogStatus::FallenBack {
                                active_mode,
                                reason,
                                ..
                            } => {
                                ui.label(
                                    RichText::new(format!(
                                        "Watchdog: Fallback to {active_mode} ({})",
                                        reason.describe()
                                    ))
                                    .monospace()
                                    .color(Color32::from_rgb(255, 120, 100)),
                                );
                            }
                            muvrix_player::WatchdogStatus::StallWarning {
                                elapsed_secs,
                                drop_count,
                            } => {
                                ui.label(
                                    RichText::new(format!(
                                        "Watchdog: Stall Warning ({elapsed_secs:.1}s, {drop_count} drops)"
                                    ))
                                    .monospace()
                                    .color(Color32::from_rgb(255, 200, 50)),
                                );
                            }
                            muvrix_player::WatchdogStatus::Exhausted {
                                active_mode,
                                reason,
                            } => {
                                ui.label(
                                    RichText::new(format!(
                                        "Watchdog Exhausted: {active_mode} ({reason})"
                                    ))
                                    .monospace()
                                    .color(Color32::from_rgb(255, 80, 80)),
                                );
                            }
                            _ => {}
                        }
                        if let Some(hdr) = &self.hdr_state {
                            if hdr.source.is_hdr() {
                                ui.label(
                                    RichText::new(format!(
                                        "HDR: {} | mode {:?} | tone-mapped: {} | HDR output: {}",
                                        hdr.source.label(),
                                        hdr.mode,
                                        hdr.tone_mapping,
                                        hdr.hdr_presented
                                    ))
                                    .monospace()
                                    .color(Color32::from_rgb(255, 200, 120)),
                                );
                            }
                        }

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
            .frame(
                egui::Frame::NONE
                    .fill(BrandPalette::ELEVATED_CARD)
                    .stroke(egui::Stroke::new(1.0_f32, BrandPalette::BORDER_SLATE))
                    .inner_margin(egui::Margin::symmetric(14, 8)),
            )
            .show(ctx, |ui| {
                if !self.ui_overlay_visible {
                    return;
                }
                ui.add_space(4.0);

                let deck_width = ui.available_width();

                // -------------------------------------------------------------
                // Row 1: Dedicated Timeline Row
                // -------------------------------------------------------------
                let time_pos = self.controller.time_pos();
                let duration = self.controller.duration();

                ui.horizontal(|ui| {
                    let cur_str = format_timestamp_ms(time_pos);
                    let dur_str = format_timestamp(duration);
                    let remaining_str = format_timestamp((duration - time_pos).max(0.0));

                    // Current position timestamp
                    ui.label(
                        RichText::new(&cur_str)
                            .monospace()
                            .size(12.0)
                            .color(BrandPalette::PRIMARY_TEXT),
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

                    // Context-aware hover information: A-B loop boundaries, chapter title, bookmark pin details, or seek timestamp
                    let mut hovered_chapter: Option<(i64, f64)> = None;
                    let mut hovered_ab_point: Option<f64> = None;
                    let (hover_sec, custom_hover) = if let Some(mouse_pos) = response.hover_pos() {
                        if duration > 0.0 {
                            let frac = ((mouse_pos.x - response.rect.min.x)
                                / response.rect.width())
                            .clamp(0.0, 1.0);
                            let sec = (frac as f64) * duration;
                            let mut hover_detail = None;

                            // 1. Check A-B loop boundary pins & active region
                            match self.controller.ab_loop_state() {
                                AbLoopState::Loop { a, b } => {
                                    let frac_a = (a / duration).clamp(0.0, 1.0) as f32;
                                    let frac_b = (b / duration).clamp(0.0, 1.0) as f32;
                                    let x_a = response.rect.min.x + frac_a * response.rect.width();
                                    let x_b = response.rect.min.x + frac_b * response.rect.width();
                                    if (mouse_pos.x - x_a).abs() <= 7.0 {
                                        hovered_ab_point = Some(a);
                                        hover_detail = Some(format!(
                                            "🔁 A-B Loop Point A: {} (click to seek)",
                                            format_timestamp_ms(a)
                                        ));
                                    } else if (mouse_pos.x - x_b).abs() <= 7.0 {
                                        hovered_ab_point = Some(b);
                                        hover_detail = Some(format!(
                                            "🔁 A-B Loop Point B: {} (click to seek)",
                                            format_timestamp_ms(b)
                                        ));
                                    } else if mouse_pos.x >= x_a && mouse_pos.x <= x_b {
                                        hover_detail = Some(format!(
                                            "🔁 Active A-B Loop: {} ➔ {} (Span: {:.2}s)",
                                            format_timestamp_ms(a),
                                            format_timestamp_ms(b),
                                            (b - a).max(0.0)
                                        ));
                                    }
                                }
                                AbLoopState::PointA(a) => {
                                    let frac_a = (a / duration).clamp(0.0, 1.0) as f32;
                                    let x_a = response.rect.min.x + frac_a * response.rect.width();
                                    if (mouse_pos.x - x_a).abs() <= 7.0 {
                                        hovered_ab_point = Some(a);
                                        hover_detail = Some(format!(
                                            "🔁 A-B Loop Point A: {} (waiting for Point B)",
                                            format_timestamp_ms(a)
                                        ));
                                    }
                                }
                                AbLoopState::None => {}
                            }

                            // 2. Bookmarks
                            if hover_detail.is_none() {
                                for bm in &self.active_bookmarks {
                                    let frac =
                                        (bm.timestamp_seconds() / duration).clamp(0.0, 1.0) as f32;
                                    let x = response.rect.min.x + frac * response.rect.width();
                                    if (mouse_pos.x - x).abs() <= 7.0 {
                                        let note_str = bm
                                            .note
                                            .as_deref()
                                            .map(|n| format!(" — {}", n))
                                            .unwrap_or_default();
                                        hover_detail = Some(format!(
                                            "🔖 {} ({}){}",
                                            bm.label,
                                            format_timestamp_ms(bm.timestamp_seconds()),
                                            note_str
                                        ));
                                        break;
                                    }
                                }
                            }

                            // 3. Chapters with exact snapping
                            if hover_detail.is_none() {
                                for ch in self.controller.chapters() {
                                    let frac = (ch.time / duration).clamp(0.0, 1.0) as f32;
                                    let x = response.rect.min.x + frac * response.rect.width();
                                    if (mouse_pos.x - x).abs() <= 7.0 {
                                        hovered_chapter = Some((ch.index, ch.time));
                                        hover_detail = Some(format!(
                                            "📑 Chapter {}: {} ({})",
                                            ch.index + 1,
                                            ch.title,
                                            format_timestamp_ms(ch.time)
                                        ));
                                        break;
                                    }
                                }
                            }
                            (Some(sec), hover_detail)
                        } else {
                            (None, None)
                        }
                    } else {
                        (None, None)
                    };

                    let tooltip = custom_hover.unwrap_or_else(|| {
                        if let Some(sec) = hover_sec {
                            format!("Seek to {}", format_timestamp_ms(sec))
                        } else {
                            format!("Seek to {}", format_timestamp_ms(slider_val))
                        }
                    });
                    let response = response.on_hover_text(tooltip);

                    // Draw A-B loop highlighted region and boundary flags on slider track
                    if duration > 0.0 {
                        let painter = ui.painter();
                        let lime_col = BrandPalette::ELECTRIC_LIME;
                        match self.controller.ab_loop_state() {
                            AbLoopState::Loop { a, b } => {
                                let a_clamped = a.clamp(0.0, max_dur);
                                let b_clamped = b.clamp(a_clamped, max_dur);
                                let frac_a = (a_clamped / max_dur) as f32;
                                let frac_b = (b_clamped / max_dur) as f32;
                                let x_a = response.rect.min.x + frac_a * response.rect.width();
                                let x_b = response.rect.min.x + frac_b * response.rect.width();

                                // Highlighted filled loop range
                                let loop_rect = Rect::from_min_max(
                                    Pos2::new(x_a, response.rect.min.y + 1.0),
                                    Pos2::new(x_b, response.rect.max.y - 1.0),
                                );
                                painter.rect_filled(
                                    loop_rect,
                                    2.0,
                                    Color32::from_rgba_unmultiplied(197, 243, 106, 45),
                                );
                                painter.line_segment(
                                    [
                                        Pos2::new(x_a, response.rect.min.y + 1.0),
                                        Pos2::new(x_b, response.rect.min.y + 1.0),
                                    ],
                                    Stroke::new(
                                        1.0_f32,
                                        Color32::from_rgba_unmultiplied(197, 243, 106, 160),
                                    ),
                                );
                                painter.line_segment(
                                    [
                                        Pos2::new(x_a, response.rect.max.y - 1.0),
                                        Pos2::new(x_b, response.rect.max.y - 1.0),
                                    ],
                                    Stroke::new(
                                        1.0_f32,
                                        Color32::from_rgba_unmultiplied(197, 243, 106, 160),
                                    ),
                                );

                                // Point A marker and badge
                                painter.line_segment(
                                    [
                                        Pos2::new(x_a, response.rect.min.y - 2.0),
                                        Pos2::new(x_a, response.rect.max.y + 2.0),
                                    ],
                                    Stroke::new(2.0_f32, lime_col),
                                );
                                painter.rect_filled(
                                    Rect::from_center_size(
                                        Pos2::new(x_a, response.rect.min.y - 5.0),
                                        Vec2::new(11.0, 10.0),
                                    ),
                                    2.0,
                                    lime_col,
                                );
                                painter.text(
                                    Pos2::new(x_a, response.rect.min.y - 5.0),
                                    egui::Align2::CENTER_CENTER,
                                    "A",
                                    egui::FontId::proportional(8.5),
                                    BrandPalette::OBSIDIAN,
                                );

                                // Point B marker and badge
                                painter.line_segment(
                                    [
                                        Pos2::new(x_b, response.rect.min.y - 2.0),
                                        Pos2::new(x_b, response.rect.max.y + 2.0),
                                    ],
                                    Stroke::new(2.0_f32, lime_col),
                                );
                                painter.rect_filled(
                                    Rect::from_center_size(
                                        Pos2::new(x_b, response.rect.min.y - 5.0),
                                        Vec2::new(11.0, 10.0),
                                    ),
                                    2.0,
                                    lime_col,
                                );
                                painter.text(
                                    Pos2::new(x_b, response.rect.min.y - 5.0),
                                    egui::Align2::CENTER_CENTER,
                                    "B",
                                    egui::FontId::proportional(8.5),
                                    BrandPalette::OBSIDIAN,
                                );
                            }
                            AbLoopState::PointA(a) => {
                                let a_clamped = a.clamp(0.0, max_dur);
                                let frac_a = (a_clamped / max_dur) as f32;
                                let x_a = response.rect.min.x + frac_a * response.rect.width();

                                // Point A marker and badge
                                painter.line_segment(
                                    [
                                        Pos2::new(x_a, response.rect.min.y - 2.0),
                                        Pos2::new(x_a, response.rect.max.y + 2.0),
                                    ],
                                    Stroke::new(2.0_f32, lime_col),
                                );
                                painter.rect_filled(
                                    Rect::from_center_size(
                                        Pos2::new(x_a, response.rect.min.y - 5.0),
                                        Vec2::new(11.0, 10.0),
                                    ),
                                    2.0,
                                    lime_col,
                                );
                                painter.text(
                                    Pos2::new(x_a, response.rect.min.y - 5.0),
                                    egui::Align2::CENTER_CENTER,
                                    "A",
                                    egui::FontId::proportional(8.5),
                                    BrandPalette::OBSIDIAN,
                                );
                            }
                            AbLoopState::None => {}
                        }
                    }

                    // Draw visual chapter marks on slider track echoing logo notches
                    let chapters = self.controller.chapters();
                    if duration > 0.0 && !chapters.is_empty() {
                        let painter = ui.painter();
                        let notch_col = match self.accent_color {
                            AccentColor::Lime => BrandPalette::ELECTRIC_LIME,
                            AccentColor::Blue => Color32::from_rgb(60, 150, 245),
                            AccentColor::Purple => Color32::from_rgb(168, 85, 247),
                            AccentColor::Teal => Color32::from_rgb(20, 184, 166),
                            AccentColor::Green => Color32::from_rgb(74, 222, 128),
                            AccentColor::Orange => Color32::from_rgb(249, 115, 22),
                            AccentColor::Red => Color32::from_rgb(239, 68, 68),
                        };
                        for ch in chapters {
                            let frac = (ch.time / duration).clamp(0.0, 1.0) as f32;
                            let x = response.rect.min.x + frac * response.rect.width();
                            painter.line_segment(
                                [
                                    Pos2::new(x, response.rect.min.y + 1.0),
                                    Pos2::new(x, response.rect.max.y - 1.0),
                                ],
                                Stroke::new(2.0_f32, notch_col),
                            );
                            painter.circle_filled(
                                Pos2::new(x, response.rect.min.y - 1.5),
                                2.5,
                                notch_col,
                            );
                        }
                    }

                    // Draw visual bookmark pins on slider track
                    if duration > 0.0 && !self.active_bookmarks.is_empty() {
                        let painter = ui.painter();
                        for bm in &self.active_bookmarks {
                            let frac = (bm.timestamp_seconds() / duration).clamp(0.0, 1.0) as f32;
                            let x = response.rect.min.x + frac * response.rect.width();
                            painter.line_segment(
                                [
                                    Pos2::new(x, response.rect.min.y),
                                    Pos2::new(x, response.rect.max.y),
                                ],
                                Stroke::new(2.5_f32, Color32::from_rgb(245, 158, 11)),
                            );
                            let head = vec![
                                Pos2::new(x, response.rect.min.y - 4.0),
                                Pos2::new(x + 4.0, response.rect.min.y + 1.0),
                                Pos2::new(x, response.rect.min.y + 6.0),
                                Pos2::new(x - 4.0, response.rect.min.y + 1.0),
                            ];
                            painter.add(Shape::convex_polygon(
                                head,
                                Color32::from_rgb(245, 158, 11),
                                Stroke::new(1.0_f32, Color32::from_rgb(180, 83, 9)),
                            ));
                        }
                    }

                    // Draw demuxer/stream buffer range on slider track
                    let cache_time = self.controller.demuxer_cache_time();
                    if duration > 0.0 && cache_time > time_pos {
                        let painter = ui.painter();
                        let frac_cur = (time_pos / max_dur).clamp(0.0, 1.0) as f32;
                        let frac_buf = (cache_time / max_dur).clamp(0.0, 1.0) as f32;
                        let x_start = response.rect.min.x + frac_cur * response.rect.width();
                        let x_end = response.rect.min.x + frac_buf * response.rect.width();
                        if x_end > x_start {
                            let buf_rect = Rect::from_min_max(
                                Pos2::new(x_start, response.rect.center().y - 2.5),
                                Pos2::new(x_end, response.rect.center().y + 2.5),
                            );
                            painter.rect_filled(
                                buf_rect,
                                1.5_f32,
                                Color32::from_rgba_premultiplied(90, 160, 240, 150),
                            );
                        }
                    }

                    // Signature electric lime scrubber pin echoing logo right-leg cue
                    if duration > 0.0 {
                        let painter = ui.painter();
                        let frac = (slider_val / max_dur).clamp(0.0, 1.0) as f32;
                        let pin_x = response.rect.min.x + frac * response.rect.width();
                        let pin_y = response.rect.center().y;
                        let pin_col = match self.accent_color {
                            AccentColor::Lime => BrandPalette::ELECTRIC_LIME,
                            AccentColor::Blue => Color32::from_rgb(60, 150, 245),
                            AccentColor::Purple => Color32::from_rgb(168, 85, 247),
                            AccentColor::Teal => Color32::from_rgb(20, 184, 166),
                            AccentColor::Green => Color32::from_rgb(74, 222, 128),
                            AccentColor::Orange => Color32::from_rgb(249, 115, 22),
                            AccentColor::Red => Color32::from_rgb(239, 68, 68),
                        };
                        let radius = if response.hovered() || self.is_seeking {
                            6.5
                        } else {
                            4.5
                        };
                        painter.circle_filled(Pos2::new(pin_x, pin_y), radius, pin_col);
                        painter.circle_stroke(
                            Pos2::new(pin_x, pin_y),
                            radius,
                            Stroke::new(1.5_f32, BrandPalette::OBSIDIAN),
                        );
                    }

                    if response.drag_started() {
                        self.is_seeking = true;
                        self.seek_target = if let Some((_, ch_time)) = hovered_chapter {
                            ch_time
                        } else if let Some(ab_pt) = hovered_ab_point {
                            ab_pt
                        } else {
                            slider_val
                        };
                    }
                    if response.dragged() {
                        self.seek_target = slider_val;
                    }
                    if response.drag_stopped() {
                        if let Some((ch_idx, ch_time)) = hovered_chapter {
                            let _ = self.controller.seek(ch_time, SeekMode::Absolute);
                            let _ = self.controller.set_chapter(ch_idx);
                        } else if let Some(ab_pt) = hovered_ab_point {
                            let _ = self.controller.seek(ab_pt, SeekMode::Absolute);
                        } else {
                            let _ = self.controller.seek(self.seek_target, SeekMode::Absolute);
                        }
                        self.is_seeking = false;
                    }
                    if response.clicked() {
                        if let Some((ch_idx, ch_time)) = hovered_chapter {
                            let _ = self.controller.seek(ch_time, SeekMode::Absolute);
                            let _ = self.controller.set_chapter(ch_idx);
                        } else if let Some(ab_pt) = hovered_ab_point {
                            let _ = self.controller.seek(ab_pt, SeekMode::Absolute);
                        } else {
                            let click_sec = if let Some(sec) = hover_sec {
                                sec
                            } else {
                                slider_val
                            };
                            let _ = self.controller.seek(click_sec, SeekMode::Absolute);
                        }
                    }

                    // Duration timestamp
                    ui.label(
                        RichText::new(&dur_str)
                            .monospace()
                            .size(12.0)
                            .color(BrandPalette::MUTED_TEXT),
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

                    // Primary Play/Pause button in Electric Lime
                    if icons::electric_play_button(ui, play_icon, Vec2::new(38.0, 28.0), play_tip)
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

                    ui.add_space(2.0);

                    // Precision Stepping Preset selector
                    let cur_preset = self.stepping_preset;
                    let preset_tip = format!(
                        "Stepping Preset: {}\n• Frame: 1 decoded video frame (native)\n• 1/24s: Film standard (41.7ms)\n• 1/30s: NTSC standard (33.3ms)\n• 1/60s: High-Rate (16.7ms)",
                        cur_preset.label()
                    );
                    let preset_label = if deck_width < 1400.0 {
                        match cur_preset {
                            SteppingPreset::Frame => "Frame",
                            SteppingPreset::Film24 => "Film",
                            SteppingPreset::Ntsc30 => "NTSC",
                            SteppingPreset::HighRate60 => "60fps",
                        }
                    } else {
                        cur_preset.label()
                    };
                    egui::ComboBox::from_id_salt("stepping_preset_combo")
                        .selected_text(preset_label)
                        .width(if deck_width < 1400.0 { 56.0 } else { 76.0 })
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(
                                    self.stepping_preset == SteppingPreset::Frame,
                                    "Frame (Decoded)",
                                )
                                .clicked()
                            {
                                self.stepping_preset = SteppingPreset::Frame;
                            }
                            if ui
                                .selectable_label(
                                    self.stepping_preset == SteppingPreset::Film24,
                                    "1/24s (Film)",
                                )
                                .clicked()
                            {
                                self.stepping_preset = SteppingPreset::Film24;
                            }
                            if ui
                                .selectable_label(
                                    self.stepping_preset == SteppingPreset::Ntsc30,
                                    "1/30s (NTSC)",
                                )
                                .clicked()
                            {
                                self.stepping_preset = SteppingPreset::Ntsc30;
                            }
                            if ui
                                .selectable_label(
                                    self.stepping_preset == SteppingPreset::HighRate60,
                                    "1/60s (High-Rate)",
                                )
                                .clicked()
                            {
                                self.stepping_preset = SteppingPreset::HighRate60;
                            }
                        })
                        .response
                        .on_hover_text(preset_tip);

                    // Step Backward button
                    let back_tip = format!(
                        "Step backward [{}] (,)",
                        self.stepping_preset.label()
                    );
                    if ui.button("◀|").on_hover_text(back_tip).clicked() {
                        let _ = self
                            .controller
                            .step_preset_backward(self.stepping_preset);
                    }

                    // Tactile Jog-Wheel Scrub Widget
                    let jog_size = Vec2::new(48.0, 22.0);
                    let (jog_rect, jog_response) =
                        ui.allocate_exact_size(jog_size, Sense::click_and_drag());
                    if ui.is_rect_visible(jog_rect) {
                        let painter = ui.painter();
                        let jog_bg = if jog_response.dragged() {
                            BrandPalette::SLATE_SURFACE
                        } else if jog_response.hovered() {
                            BrandPalette::SECONDARY_BG
                        } else {
                            BrandPalette::OBSIDIAN
                        };
                        painter.rect_filled(jog_rect, 3.0, jog_bg);
                        painter.rect_stroke(
                            jog_rect,
                            3.0,
                            Stroke::new(
                                1.0_f32,
                                if jog_response.dragged() {
                                    BrandPalette::ELECTRIC_LIME
                                } else {
                                    Color32::from_gray(60)
                                },
                            ),
                            StrokeKind::Inside,
                        );

                        // Horizontal dial graduation tick marks
                        let center_x = jog_rect.center().x;
                        let offset = self.jog_drag_accumulator % 8.0;
                        for i in -3..=3 {
                            let tick_x = center_x + (i as f32) * 8.0 + offset;
                            if tick_x >= jog_rect.min.x + 3.0 && tick_x <= jog_rect.max.x - 3.0 {
                                let alpha = (1.0
                                    - ((tick_x - center_x).abs() / (jog_size.x * 0.5)))
                                    .clamp(0.1, 0.8);
                                painter.line_segment(
                                    [
                                        Pos2::new(tick_x, jog_rect.min.y + 4.0),
                                        Pos2::new(tick_x, jog_rect.max.y - 4.0),
                                    ],
                                    Stroke::new(
                                        1.0_f32,
                                        Color32::from_rgba_unmultiplied(
                                            165,
                                            175,
                                            185,
                                            (alpha * 255.0) as u8,
                                        ),
                                    ),
                                );
                            }
                        }

                        // Center index needle in Electric Lime
                        painter.line_segment(
                            [
                                Pos2::new(center_x, jog_rect.min.y + 2.0),
                                Pos2::new(center_x, jog_rect.max.y - 2.0),
                            ],
                            Stroke::new(1.5_f32, BrandPalette::ELECTRIC_LIME),
                        );
                    }

                    if jog_response.dragged() {
                        let dx = jog_response.drag_delta().x;
                        self.jog_drag_accumulator += dx;
                        const JOG_THRESHOLD: f32 = 8.0;
                        while self.jog_drag_accumulator >= JOG_THRESHOLD {
                            let _ = self
                                .controller
                                .step_preset_forward(self.stepping_preset);
                            self.jog_drag_accumulator -= JOG_THRESHOLD;
                        }
                        while self.jog_drag_accumulator <= -JOG_THRESHOLD {
                            let _ = self
                                .controller
                                .step_preset_backward(self.stepping_preset);
                            self.jog_drag_accumulator += JOG_THRESHOLD;
                        }
                    } else if !jog_response.hovered() {
                        self.jog_drag_accumulator = 0.0;
                    }

                    let jog_tooltip = format!(
                        "Precision Jog-Wheel: Drag horizontally to scrub [{}]\nTimestamp: {}",
                        self.stepping_preset.label(),
                        format_timestamp_ms(time_pos)
                    );
                    let jog_response = jog_response.on_hover_text(jog_tooltip);
                    if jog_response.clicked() {
                        // Single click toggles 1 frame forward
                        let _ = self
                            .controller
                            .step_preset_forward(self.stepping_preset);
                    }

                    // Step Forward button
                    let fwd_tip =
                        format!("Step forward [{}] (.)", self.stepping_preset.label());
                    if ui.button("|▶").on_hover_text(fwd_tip).clicked() {
                        let _ = self
                            .controller
                            .step_preset_forward(self.stepping_preset);
                    }

                    ui.add_space(2.0);

                    // A-B Loop Controls
                    let ab_state = self.controller.ab_loop_state();
                    let (ab_text, ab_tip) = match ab_state {
                        AbLoopState::None => (
                            RichText::new("A-B")
                                .size(11.5)
                                .color(BrandPalette::PRIMARY_TEXT),
                            "Cycle A-B loop (L) • Click to set Point A at current timestamp\nRight-click for options"
                                .to_string(),
                        ),
                        AbLoopState::PointA(a) => (
                            RichText::new("A-B•")
                                .size(11.5)
                                .color(BrandPalette::ELECTRIC_LIME),
                            format!(
                                "Point A set at {}\nClick to set Point B (L)\nRight-click for options",
                                format_timestamp_ms(a)
                            ),
                        ),
                        AbLoopState::Loop { a, b } => (
                            RichText::new("A-B↺")
                                .size(11.5)
                                .color(BrandPalette::ELECTRIC_LIME),
                            format!(
                                "Active Loop: {} ➔ {}\nClick to clear loop (L)\nRight-click for options",
                                format_timestamp_ms(a),
                                format_timestamp_ms(b)
                            ),
                        ),
                    };

                    let ab_btn = ui.button(ab_text).on_hover_text(ab_tip);
                    if ab_btn.clicked() {
                        self.cycle_ab_loop();
                    }

                    // Context menu on right-click for explicit A-B loop management
                    ab_btn.context_menu(|ui| {
                        ui.label(
                            RichText::new("A-B Loop Options")
                                .strong()
                                .color(BrandPalette::ELECTRIC_LIME),
                        );
                        ui.separator();
                        if ui.button("Set Point A (Current Time)").clicked() {
                            let _ = self.controller.set_ab_loop_a(time_pos);
                            ui.close_menu();
                        }
                        if ui.button("Set Point B (Current Time)").clicked() {
                            let _ = self.controller.set_ab_loop_b(time_pos);
                            ui.close_menu();
                        }
                        if let AbLoopState::Loop { a, b } = ab_state {
                            ui.separator();
                            if ui
                                .button(format!("Jump to Point A ({})", format_timestamp(a)))
                                .clicked()
                            {
                                let _ = self.controller.seek(a, SeekMode::Absolute);
                                ui.close_menu();
                            }
                            if ui
                                .button(format!("Jump to Point B ({})", format_timestamp(b)))
                                .clicked()
                            {
                                let _ = self.controller.seek(b, SeekMode::Absolute);
                                ui.close_menu();
                            }
                        }
                        ui.separator();
                        if ui.button("Clear Loop").clicked() {
                            let _ = self.controller.clear_ab_loop();
                            ui.close_menu();
                        }
                    });

                    let repeat_label = if deck_width < 1400.0 {
                        match self.controller.repeat_mode() {
                            RepeatMode::Off => "Rep: Off",
                            RepeatMode::One => "Rep: 1",
                            RepeatMode::All => "Rep: All",
                        }
                    } else {
                        match self.controller.repeat_mode() {
                            RepeatMode::Off => "Repeat: Off",
                            RepeatMode::One => "Repeat: One",
                            RepeatMode::All => "Repeat: All",
                        }
                    };
                    if ui
                        .button(repeat_label)
                        .on_hover_text("Cycle production repeat mode")
                        .clicked()
                    {
                        self.cycle_repeat_mode();
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

                        if deck_width >= 1120.0 {
                            let cur_ch_idx = self.controller.current_chapter().unwrap_or(0);
                            let selected_title = chapters
                                .iter()
                                .find(|c| c.index == cur_ch_idx)
                                .map(|c| {
                                    if deck_width < 1400.0 {
                                        format!("Ch {}", c.index + 1)
                                    } else {
                                        format!("Ch {}: {}", c.index + 1, c.title)
                                    }
                                })
                                .unwrap_or_else(|| "Chapters".to_string());

                            egui::ComboBox::from_id_salt("chapters_combo")
                                .selected_text(selected_title)
                                .width(if deck_width < 1400.0 { 60.0 } else { 120.0 })
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
                    }

                    // Bookmark actions
                    if self.controller.current_file().is_some() && deck_width >= 1120.0 {
                        ui.add_space(2.0);
                        let bm_btn_text = if deck_width < 1400.0 {
                            "🔖"
                        } else {
                            "🔖 Bookmark"
                        };
                        if ui
                            .button(RichText::new(bm_btn_text).size(11.5))
                            .on_hover_text("Add bookmark at current playback position")
                            .clicked()
                        {
                            let label = format!("Bookmark {}", format_timestamp(time_pos));
                            self.add_bookmark(&label, None);
                        }

                        if !self.active_bookmarks.is_empty() && deck_width >= 1400.0 {
                            ui.add_space(2.0);
                            egui::ComboBox::from_id_salt("bookmarks_combo")
                                .selected_text(format!(
                                    "Bookmarks ({})",
                                    self.active_bookmarks.len()
                                ))
                                .show_ui(ui, |ui| {
                                    for bm in &self.active_bookmarks {
                                        let label = format!(
                                            "{} ({})",
                                            bm.label,
                                            format_timestamp(bm.timestamp_seconds())
                                        );
                                        if ui.button(label).clicked() {
                                            let _ = self
                                                .controller
                                                .seek(bm.timestamp_seconds(), SeekMode::Absolute);
                                        }
                                    }
                                });
                        }
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
                    let max_volume = self.controller.volume_boost_max().max(100.0);
                    let vol_slider = egui::Slider::new(&mut vol, 0.0..=max_volume)
                        .show_value(false)
                        .text(vol_label);
                    let vol_w = if deck_width < 1120.0 {
                        35.0
                    } else if deck_width < 1400.0 {
                        45.0
                    } else {
                        65.0
                    };
                    let vol_res = ui
                        .add_sized([vol_w, 18.0], vol_slider)
                        .on_hover_text(format!("Volume: {:.0}% (Up/Down Arrow)", vol));
                    if vol_res.changed() {
                        let _ = self.controller.set_volume(vol);
                    }

                    ui.add_space(3.0);
                    ui.separator();
                    ui.add_space(3.0);

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

                    if deck_width >= 1400.0
                        && (current_speed - 1.0).abs() > 0.01
                        && ui
                            .button("1.0x")
                            .on_hover_text("Reset to normal speed (Backspace)")
                            .clicked()
                    {
                        let _ = self.controller.reset_speed();
                    }

                    ui.add_space(3.0);
                    ui.separator();
                    ui.add_space(3.0);
                    let sleep_label = if deck_width < 1400.0 {
                        format!("⏰ {}", self.sleep_timer.label())
                    } else {
                        format!("Sleep: {}", self.sleep_timer.label())
                    };
                    egui::ComboBox::from_id_salt("sleep_timer_combo")
                        .selected_text(sleep_label)
                        .show_ui(ui, |ui| {
                            for option in [
                                SleepTimer::Off,
                                SleepTimer::Minutes(15),
                                SleepTimer::Minutes(30),
                                SleepTimer::Minutes(45),
                                SleepTimer::Minutes(60),
                                SleepTimer::EndOfFile,
                            ] {
                                if ui
                                    .selectable_label(self.sleep_timer == option, option.label())
                                    .clicked()
                                {
                                    self.set_sleep_timer(option);
                                }
                            }
                        });
                    if let Some(remaining) = self.sleep_remaining() {
                        let seconds = remaining.as_secs();
                        ui.label(
                            RichText::new(format!("{}:{:02}", seconds / 60, seconds % 60))
                                .monospace()
                                .size(10.5)
                                .color(Color32::from_rgb(125, 211, 252)),
                        );
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
                        let selected_sec_id = self.controller.secondary_sub_track_id();

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

                        // If width is constrained (<1120px), consolidate into a single contextual "Tracks" menu
                        if deck_width < 1120.0 {
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
                                        RichText::new("Primary Subtitles (Bottom)")
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
                                    ui.label(
                                        RichText::new("Secondary Subtitles (Top)")
                                            .strong()
                                            .color(BrandPalette::ELECTRIC_LIME),
                                    );
                                    if ui
                                        .selectable_label(
                                            selected_sec_id.is_none(),
                                            "Off (Disabled)",
                                        )
                                        .clicked()
                                    {
                                        let _ = self.controller.set_secondary_sub_track(None);
                                    }
                                    for track in &sub_tracks {
                                        let label = format_track_label(track);
                                        let is_selected = selected_sec_id == Some(track.id);
                                        if ui.selectable_label(is_selected, label).clicked() {
                                            let _ = self
                                                .controller
                                                .set_secondary_sub_track(Some(track.id));
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
                            // Subtitle track selector (Dual Subtitle support)
                            let pri_label = selected_sub_id
                                .and_then(|id| sub_tracks.iter().find(|t| t.id == id))
                                .map(|t| {
                                    t.lang
                                        .as_deref()
                                        .unwrap_or(t.title.as_deref().unwrap_or("On"))
                                });
                            let sec_label = selected_sec_id
                                .and_then(|id| sub_tracks.iter().find(|t| t.id == id))
                                .map(|t| {
                                    t.lang
                                        .as_deref()
                                        .unwrap_or(t.title.as_deref().unwrap_or("On"))
                                });

                            let sub_summary = match (pri_label, sec_label) {
                                (Some(p), Some(s)) => {
                                    if deck_width < 1400.0 {
                                        format!("Sub: {p}+{s}")
                                    } else {
                                        format!("Subtitles: {p} + {s} (Dual)")
                                    }
                                }
                                (Some(p), None) => {
                                    if deck_width < 1400.0 {
                                        format!("Sub: {p}")
                                    } else {
                                        format!("Subtitles: {p}")
                                    }
                                }
                                (None, Some(s)) => {
                                    if deck_width < 1400.0 {
                                        format!("Sub: {s} (Top)")
                                    } else {
                                        format!("Subtitles: {s} (Top)")
                                    }
                                }
                                (None, None) => {
                                    if deck_width < 1400.0 {
                                        "Sub: Off".to_string()
                                    } else {
                                        "Subtitles: Off".to_string()
                                    }
                                }
                            };

                            egui::ComboBox::from_id_salt("sub_combo")
                                .selected_text(sub_summary)
                                .show_ui(ui, |ui| {
                                    // Primary Subtitles Section
                                    ui.label(
                                        RichText::new("Primary Subtitles (Bottom)")
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
                                    if ui.checkbox(&mut vis, "Primary Visible").changed() {
                                        let _ = self.controller.set_subtitle_visibility(vis);
                                    }

                                    ui.separator();

                                    // Secondary Subtitles Section
                                    ui.label(
                                        RichText::new("Secondary Subtitles (Top - Bilingual)")
                                            .strong()
                                            .color(BrandPalette::ELECTRIC_LIME),
                                    );
                                    if ui
                                        .selectable_label(
                                            selected_sec_id.is_none(),
                                            "Off (Disabled)",
                                        )
                                        .clicked()
                                    {
                                        let _ = self.controller.set_secondary_sub_track(None);
                                    }
                                    for track in &sub_tracks {
                                        let label = format_track_label(track);
                                        let is_selected = selected_sec_id == Some(track.id);
                                        if ui.selectable_label(is_selected, label).clicked() {
                                            let _ = self
                                                .controller
                                                .set_secondary_sub_track(Some(track.id));
                                        }
                                    }

                                    ui.horizontal(|ui| {
                                        ui.label("Sec Delay:");
                                        if ui.button("-0.25s").clicked() {
                                            let _ = self
                                                .controller
                                                .set_secondary_sub_delay(-0.25);
                                        }
                                        if ui.button("0s").clicked() {
                                            let _ = self
                                                .controller
                                                .set_secondary_sub_delay(0.0);
                                        }
                                        if ui.button("+0.25s").clicked() {
                                            let _ = self
                                                .controller
                                                .set_secondary_sub_delay(0.25);
                                        }
                                    });

                                    ui.label(
                                        RichText::new("Positioning: Primary @ Bottom • Secondary @ Top")
                                            .size(10.5)
                                            .color(BrandPalette::MUTED_TEXT),
                                    );

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

                            ui.add_space(3.0);

                            // Audio track selector
                            let audio_summary = if let Some(id) = selected_audio_id {
                                if let Some(t) = audio_tracks.iter().find(|t| t.id == id) {
                                    let label = t
                                        .lang
                                        .as_deref()
                                        .unwrap_or(t.title.as_deref().unwrap_or("On"));
                                    if deck_width < 1400.0 {
                                        format!("Aud: {label}")
                                    } else {
                                        format!("Audio: {label}")
                                    }
                                } else if deck_width < 1400.0 {
                                    "Aud: On".to_string()
                                } else {
                                    "Audio: On".to_string()
                                }
                            } else if deck_width < 1400.0 {
                                "Aud: None".to_string()
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
            .frame(egui::Frame::NONE.fill(Color32::TRANSPARENT))
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

                // Interactive Resume Playback Prompt Banner (Feature Group C)
                if let Some((_, pos)) = self.pending_resume_prompt {
                    let time_str = format_timestamp(pos);

                    let banner_w = 460.0_f32.min(rect.width() - 32.0);
                    let banner_rect = Rect::from_min_size(
                        Pos2::new(rect.center().x - banner_w * 0.5, rect.min.y + 16.0),
                        Vec2::new(banner_w, 42.0),
                    );

                    ui.painter().rect(
                        banner_rect,
                        6.0_f32,
                        Color32::from_rgba_premultiplied(18, 24, 38, 235),
                        Stroke::new(1.0_f32, Color32::from_rgb(60, 140, 240)),
                        StrokeKind::Inside,
                    );

                    let mut banner_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(banner_rect.shrink(6.0))
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    );

                    icons::icon_label(
                        &mut banner_ui,
                        Icon::History,
                        16.0,
                        Color32::from_rgb(85, 180, 255),
                    );
                    banner_ui.add_space(4.0);
                    banner_ui.label(
                        RichText::new(format!("Resume from {time_str}?"))
                            .strong()
                            .size(12.5)
                            .color(Color32::WHITE),
                    );

                    banner_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Dismiss [x]").clicked() {
                            self.pending_resume_prompt = None;
                        }
                        if ui.button("Start 00:00").clicked() {
                            let _ = self.controller.seek(0.0, SeekMode::Absolute);
                            self.pending_resume_prompt = None;
                        }
                        if ui
                            .button(
                                RichText::new("Resume")
                                    .strong()
                                    .color(Color32::from_rgb(90, 190, 255)),
                            )
                            .clicked()
                        {
                            let _ = self.controller.seek(pos, SeekMode::Absolute);
                            let _ = self.controller.play();
                            self.pending_resume_prompt = None;
                        }
                    });
                }

                // Streaming Auto-Reconnect & Buffering Warning Badge (Phase D7)
                if self.controller.is_stream_reconnecting() {
                    let recon_w = 340.0_f32.min(rect.width() - 32.0);
                    let recon_rect = Rect::from_min_size(
                        Pos2::new(rect.center().x - recon_w * 0.5, rect.min.y + 24.0),
                        Vec2::new(recon_w, 36.0),
                    );
                    ui.painter().rect(
                        recon_rect,
                        6.0_f32,
                        Color32::from_rgba_premultiplied(35, 20, 10, 240),
                        Stroke::new(1.5_f32, Color32::from_rgb(250, 160, 20)),
                        StrokeKind::Inside,
                    );
                    let mut recon_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(recon_rect.shrink(6.0))
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    );
                    icons::icon_label(
                        &mut recon_ui,
                        Icon::Warning,
                        16.0,
                        Color32::from_rgb(250, 160, 20),
                    );
                    recon_ui.add_space(6.0);
                    let badge_text = if let Some(telemetry) = self.controller.stream_telemetry() {
                        match telemetry.current_state() {
                            muvrix_player::RecoveryState::Reconnecting {
                                attempt,
                                max_attempts,
                            } => {
                                format!(
                                    "⚡ Reconnecting Stream... (Attempt {attempt}/{max_attempts})"
                                )
                            }
                            muvrix_player::RecoveryState::Failed(msg) => {
                                format!("⚠ Stream Failed: {msg}")
                            }
                            _ => "⚡ Reconnecting Stream...".to_string(),
                        }
                    } else if self.controller.is_paused_for_cache() {
                        "⚡ Buffering Network Stream...".to_string()
                    } else {
                        "⚡ Reconnecting Stream...".to_string()
                    };
                    recon_ui.label(
                        RichText::new(badge_text)
                            .strong()
                            .size(12.0)
                            .color(Color32::from_rgb(255, 215, 80)),
                    );
                }

                // Contextual Overlays for player states
                match self.controller.state() {
                    PlaybackState::Idle => {
                        let center_pos = rect.center();
                        let card_width = 440.0_f32.min(rect.width() - 32.0);
                        let card_height = 320.0_f32.min(rect.height() - 32.0);
                        let card_rect = egui::Rect::from_center_size(
                            center_pos,
                            Vec2::new(card_width, card_height),
                        );

                        // High contrast welcome card container
                        let (card_bg, card_border) = if self.theme_mode == ThemeMode::Obsidian {
                            (BrandPalette::OBSIDIAN, BrandPalette::SLATE_BORDER)
                        } else {
                            (
                                Color32::from_rgba_premultiplied(22, 25, 34, 235),
                                Color32::from_rgb(55, 68, 92),
                            )
                        };
                        ui.painter().rect(
                            card_rect,
                            10.0_f32,
                            card_bg,
                            Stroke::new(1.5_f32, card_border),
                            egui::StrokeKind::Inside,
                        );

                        let mut child_ui = ui.new_child(
                            egui::UiBuilder::new()
                                .max_rect(card_rect.shrink(18.0))
                                .layout(egui::Layout::top_down(egui::Align::Center)),
                        );
                        child_ui.add_space(4.0);
                        let logo_tex = self.ensure_logo_texture(child_ui.ctx());
                        branding::render_logo(&mut child_ui, logo_tex, 96.0);
                        child_ui.add_space(8.0);
                        child_ui.label(
                            RichText::new("MUVRIX MEDIA PLAYER")
                                .strong()
                                .size(19.0)
                                .color(BrandPalette::IVORY_TEXT),
                        );
                        child_ui.add_space(3.0);
                        child_ui.label(
                            RichText::new("Lightweight hardware-accelerated desktop media player")
                                .size(12.5)
                                .color(BrandPalette::MUTED_TEXT),
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
    }
}
