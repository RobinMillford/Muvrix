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
use crate::views::AppView;

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
                            .slow_metrics
                            .container_fps
                            .or(self.slow_metrics.estimated_vf_fps)
                            .unwrap_or(0.0);
                        let drops = self.slow_metrics.frame_drops;
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
                let (time_pos, duration) = if *self.controller.state() == PlaybackState::Idle {
                    (0.0, 0.0)
                } else {
                    (self.controller.time_pos(), self.controller.duration())
                };

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
                    let cache_time = self.slow_metrics.demuxer_cache_time;
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

                    // Skip Prev / Next (Chapter if available, otherwise Playlist)
                    if icons::icon_button_sized(
                        ui,
                        Icon::SkipPrevious,
                        Vec2::new(26.0, 26.0),
                        "Previous (Chapter / Track)",
                    )
                    .clicked()
                    {
                        if !self.controller.chapters().is_empty() {
                            let _ = self.controller.prev_chapter();
                        } else {
                            let _ = self.controller.playlist_prev();
                        }
                    }

                    if icons::icon_button_sized(
                        ui,
                        Icon::SkipNext,
                        Vec2::new(26.0, 26.0),
                        "Next (Chapter / Track)",
                    )
                    .clicked()
                    {
                        if !self.controller.chapters().is_empty() {
                            let _ = self.controller.next_chapter();
                        } else {
                            let _ = self.controller.playlist_next();
                        }
                    }

                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    // Volume & Mute Controls
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
                        45.0
                    } else if deck_width < 1400.0 {
                        55.0
                    } else {
                        70.0
                    };
                    let vol_res = ui
                        .add_sized([vol_w, 18.0], vol_slider)
                        .on_hover_text(format!("Volume: {:.0}% (Up/Down Arrow)", vol));
                    if vol_res.changed() {
                        let _ = self.controller.set_volume(vol);
                    }

                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    // Secondary Deck Toggle Button
                    let secondary_visible = self.is_secondary_deck_visible(deck_width);
                    let (deck_text, deck_tip) = if secondary_visible {
                        (
                            RichText::new("▼ Decks")
                                .size(11.5)
                                .color(BrandPalette::ELECTRIC_LIME),
                            "Collapse secondary controls deck (Ctrl+D)",
                        )
                    } else {
                        (
                            RichText::new("▲ Decks")
                                .size(11.5)
                                .color(BrandPalette::MUTED_TEXT),
                            "Expand secondary controls deck (Ctrl+D)",
                        )
                    };
                    if ui.button(deck_text).on_hover_text(deck_tip).clicked() {
                        self.toggle_secondary_deck(deck_width);
                    }

                    // Right-aligned Tier 1 items: Fullscreen & More (...) Overflow trigger
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

                        // More (...) Overflow Menu Trigger
                        let (more_text, more_tip) = if self.player_more_overflow_open {
                            (
                                RichText::new("•••")
                                    .size(13.0)
                                    .strong()
                                    .color(BrandPalette::ELECTRIC_LIME),
                                "Close Advanced Controls (Alt+M / Esc)",
                            )
                        } else {
                            (
                                RichText::new("•••")
                                    .size(13.0)
                                    .strong()
                                    .color(BrandPalette::PRIMARY_TEXT),
                                "Advanced Controls (Alt+M)\nFrame stepping, audio/sub sync delay, sleep timer & diagnostics",
                            )
                        };
                        if ui.button(more_text).on_hover_text(more_tip).clicked() {
                            self.player_more_overflow_open = !self.player_more_overflow_open;
                        }
                    });
                });

                // -------------------------------------------------------------
                // Row 3: Tier 2 — Secondary Playback Deck (Collapsible)
                // -------------------------------------------------------------
                if self.is_secondary_deck_visible(deck_width) {
                    ui.add_space(3.0);
                    ui.separator();
                    ui.add_space(3.0);

                    ui.horizontal(|ui| {
                        // 1. Chapters navigation / dropdown
                        let chapters = self.controller.chapters().to_vec();
                        if !chapters.is_empty() {
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
                                .width(if deck_width < 1400.0 { 75.0 } else { 120.0 })
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

                        // 2. A-B Loop Controls
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

                        // 3. Repeat Mode
                        let repeat_label = match self.controller.repeat_mode() {
                            RepeatMode::Off => "Rep: Off",
                            RepeatMode::One => "Rep: 1",
                            RepeatMode::All => "Rep: All",
                        };
                        if ui
                            .button(repeat_label)
                            .on_hover_text("Cycle repeat mode (Off -> One -> All)")
                            .clicked()
                        {
                            self.cycle_repeat_mode();
                        }

                        // 4. Playback Speed Selector
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

                        // 5. Bookmarks
                        if self.controller.current_file().is_some() {
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

                            if !self.active_bookmarks.is_empty() {
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

                        // Right-aligned audio & subtitle track combos
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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

                            // Subtitle track combo
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
                                (Some(p), Some(s)) => format!("Sub: {p}+{s}"),
                                (Some(p), None) => format!("Sub: {p}"),
                                (None, Some(s)) => format!("Sub: {s} (Top)"),
                                (None, None) => "Sub: Off".to_string(),
                            };

                            egui::ComboBox::from_id_salt("sub_combo_deck")
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

                            // Audio track selector
                            let audio_summary = if let Some(id) = selected_audio_id {
                                if let Some(t) = audio_tracks.iter().find(|t| t.id == id) {
                                    let label = t
                                        .lang
                                        .as_deref()
                                        .unwrap_or(t.title.as_deref().unwrap_or("On"));
                                    format!("Aud: {label}")
                                } else {
                                    "Aud: On".to_string()
                                }
                            } else {
                                "Aud: None".to_string()
                            };

                            egui::ComboBox::from_id_salt("audio_combo_deck")
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
                        });
                    });
                }

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
                        let card_width = 540.0_f32.min(rect.width() - 32.0);
                        let card_height = 430.0_f32.min(rect.height() - 32.0);
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
                        child_ui.add_space(8.0);

                        // Status badge: Ready to Play
                        child_ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("●")
                                    .size(10.0)
                                    .color(BrandPalette::ELECTRIC_LIME),
                            );
                            ui.label(
                                RichText::new("Ready to Play")
                                    .size(12.0)
                                    .color(BrandPalette::ELECTRIC_LIME),
                            );
                        });
                        child_ui.add_space(10.0);

                        // Action buttons row: Open File, Open Stream URL, Browse Library
                        child_ui.horizontal(|ui| {
                            if icons::primary_icon_button_text(
                                ui,
                                Icon::FolderOpen,
                                "Open File",
                                "Open video or audio file from disk (Ctrl+O)",
                            )
                            .clicked()
                            {
                                self.open_file_dialog();
                            }

                            let url_btn_text = if self.player_empty_url_input_open {
                                "Close URL [x]"
                            } else {
                                "Open Stream URL..."
                            };
                            if icons::icon_text_button(
                                ui,
                                Icon::Search,
                                url_btn_text,
                                "Toggle network stream URL input (Ctrl+U)",
                            )
                            .clicked()
                            {
                                self.player_empty_url_input_open =
                                    !self.player_empty_url_input_open;
                            }

                            if icons::icon_text_button(
                                ui,
                                Icon::FolderOpen,
                                "Browse Library",
                                "Switch to media library collections",
                            )
                            .clicked()
                            {
                                self.current_view = AppView::Library;
                            }
                        });

                        // Inline URL input when toggled open
                        if self.player_empty_url_input_open {
                            child_ui.add_space(6.0);
                            child_ui.horizontal(|ui| {
                                let te = ui.add(
                                    egui::TextEdit::singleline(&mut self.path_input)
                                        .hint_text("Enter stream URL (http/https/rtmp/rtsp)...")
                                        .desired_width(280.0),
                                );
                                if (ui.button("Load Stream").clicked()
                                    || (te.lost_focus()
                                        && ui.input(|i| i.key_pressed(egui::Key::Enter))))
                                    && !self.path_input.trim().is_empty()
                                {
                                    let path = self.path_input.clone();
                                    self.load_file(&path);
                                    self.player_empty_url_input_open = false;
                                }
                            });
                        }

                        child_ui.add_space(10.0);

                        // Supported format pills
                        child_ui.horizontal_wrapped(|ui| {
                            for fmt in &["MP4", "MKV", "WebM", "AVI", "MOV", "FLAC", "MP3", "AAC"] {
                                egui::Frame::NONE
                                    .fill(BrandPalette::SLATE_SURFACE)
                                    .stroke(egui::Stroke::new(1.0_f32, BrandPalette::BORDER_SLATE))
                                    .corner_radius(4.0_f32)
                                    .inner_margin(egui::Margin::symmetric(6, 2))
                                    .show(ui, |ui| {
                                        ui.label(
                                            RichText::new(*fmt)
                                                .size(10.0)
                                                .monospace()
                                                .color(Color32::from_gray(180)),
                                        );
                                    });
                            }
                        });

                        child_ui.add_space(8.0);

                        // Keyboard Shortcuts Cheatsheet Card
                        egui::Frame::NONE
                            .fill(Color32::from_rgba_unmultiplied(12, 14, 20, 180))
                            .stroke(egui::Stroke::new(1.0_f32, Color32::from_gray(45)))
                            .corner_radius(6.0_f32)
                            .inner_margin(egui::Margin::symmetric(10, 6))
                            .show(&mut child_ui, |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    let mut render_hint = |key: &str, desc: &str| {
                                        ui.label(
                                            RichText::new(key)
                                                .monospace()
                                                .size(10.0)
                                                .strong()
                                                .color(BrandPalette::ELECTRIC_LIME),
                                        );
                                        ui.label(
                                            RichText::new(desc)
                                                .size(10.0)
                                                .color(BrandPalette::MUTED_TEXT),
                                        );
                                        ui.add_space(6.0);
                                    };
                                    render_hint("Space", "Play/Pause");
                                    render_hint("←/→", "Seek 5s");
                                    render_hint("↑/↓", "Vol ±5%");
                                    render_hint("M", "Mute");
                                    render_hint("F", "Fullscreen");
                                    render_hint("L", "A-B Loop");
                                    render_hint("Ctrl+D", "Decks");
                                    render_hint("Alt+M", "More");
                                });
                            });
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
                    PlaybackState::Playing | PlaybackState::Paused => {
                        let has_video = self.controller.track().tracks().iter().any(|t| {
                            t.track_type == TrackType::Video
                                && !matches!(
                                    t.codec
                                        .as_deref()
                                        .map(|s| s.to_ascii_lowercase())
                                        .as_deref(),
                                    Some("mjpeg" | "png" | "jpeg" | "jpg")
                                )
                        });

                        let force_audio_view = std::env::var("MUVRIX_AUDIO_PLAYBACK_MODE").is_ok()
                            || std::env::var("MUVRIX_SHOW_SPECTRUM").is_ok();

                        if !has_video || force_audio_view {
                            self.render_audio_player_viewport(ui, rect);
                        }
                    }
                }
            });

        // ---------------------------------------------------------------------
        // 4. Tier 3 — Advanced Controls Overflow Popover Window
        // ---------------------------------------------------------------------
        if self.player_more_overflow_open {
            let mut more_open = self.player_more_overflow_open;
            let time_pos = self.controller.time_pos();
            egui::Window::new("Advanced Controls")
                .open(&mut more_open)
                .resizable(false)
                .collapsible(false)
                .anchor(egui::Align2::RIGHT_BOTTOM, [-16.0, -90.0])
                .frame(
                    egui::Frame::NONE
                        .fill(BrandPalette::ELEVATED_CARD)
                        .stroke(egui::Stroke::new(1.0_f32, BrandPalette::BORDER_SLATE))
                        .corner_radius(8.0_f32)
                        .inner_margin(egui::Margin::symmetric(14, 12)),
                )
                .show(ctx, |ui| {
                    ui.set_width(340.0);

                    // Section 1: Precision Frame Stepping & Jog-Wheel
                    ui.horizontal(|ui| {
                        icons::icon_label(ui, Icon::Sliders, 14.0, BrandPalette::ELECTRIC_LIME);
                        ui.label(
                            RichText::new("Frame Stepping & Scrubbing")
                                .strong()
                                .color(BrandPalette::ELECTRIC_LIME),
                        );
                    });
                    ui.add_space(4.0);

                    ui.horizontal(|ui| {
                        let cur_preset = self.stepping_preset;
                        egui::ComboBox::from_id_salt("popover_stepping_preset_combo")
                            .selected_text(cur_preset.label())
                            .width(110.0)
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
                            });

                        let back_tip =
                            format!("Step backward [{}] (,)", self.stepping_preset.label());
                        if ui.button("◀| Step Back").on_hover_text(back_tip).clicked() {
                            let _ = self.controller.step_preset_backward(self.stepping_preset);
                        }

                        let fwd_tip =
                            format!("Step forward [{}] (.)", self.stepping_preset.label());
                        if ui.button("Step Fwd |▶").on_hover_text(fwd_tip).clicked() {
                            let _ = self.controller.step_preset_forward(self.stepping_preset);
                        }
                    });

                    ui.add_space(6.0);

                    // Tactile Jog-Wheel Scrub Widget
                    ui.label(
                        RichText::new("Tactile Jog-Wheel Scrub Dial:")
                            .size(11.0)
                            .color(BrandPalette::MUTED_TEXT),
                    );
                    let jog_size = Vec2::new(ui.available_width(), 26.0);
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
                        painter.rect_filled(jog_rect, 4.0, jog_bg);
                        painter.rect_stroke(
                            jog_rect,
                            4.0,
                            Stroke::new(
                                1.0_f32,
                                if jog_response.dragged() {
                                    BrandPalette::ELECTRIC_LIME
                                } else {
                                    BrandPalette::BORDER_SLATE
                                },
                            ),
                            StrokeKind::Inside,
                        );

                        // Horizontal dial graduation tick marks
                        let center_x = jog_rect.center().x;
                        let offset = self.jog_drag_accumulator % 10.0;
                        for i in -15..=15 {
                            let tick_x = center_x + (i as f32) * 10.0 + offset;
                            if tick_x >= jog_rect.min.x + 4.0 && tick_x <= jog_rect.max.x - 4.0 {
                                let alpha = (1.0
                                    - ((tick_x - center_x).abs() / (jog_size.x * 0.5)))
                                    .clamp(0.1, 0.85);
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
                            Stroke::new(2.0_f32, BrandPalette::ELECTRIC_LIME),
                        );
                    }

                    if jog_response.dragged() {
                        let dx = jog_response.drag_delta().x;
                        self.jog_drag_accumulator += dx;
                        const JOG_THRESHOLD: f32 = 8.0;
                        while self.jog_drag_accumulator >= JOG_THRESHOLD {
                            let _ = self.controller.step_preset_forward(self.stepping_preset);
                            self.jog_drag_accumulator -= JOG_THRESHOLD;
                        }
                        while self.jog_drag_accumulator <= -JOG_THRESHOLD {
                            let _ = self.controller.step_preset_backward(self.stepping_preset);
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
                        let _ = self.controller.step_preset_forward(self.stepping_preset);
                    }

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(6.0);

                    // Section 2: Audio & Subtitle Sync Delay Offsets
                    ui.horizontal(|ui| {
                        icons::icon_label(
                            ui,
                            Icon::Sliders,
                            14.0,
                            Color32::from_rgb(120, 200, 255),
                        );
                        ui.label(
                            RichText::new("A/V & Subtitle Sync Adjustments")
                                .strong()
                                .color(Color32::from_rgb(120, 200, 255)),
                        );
                    });
                    ui.add_space(4.0);

                    // Primary Subtitle Delay
                    let cur_sub_delay = self.controller.sub_delay();
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("Primary Sub: {:+.2}s", cur_sub_delay))
                                .monospace(),
                        );
                        if ui.button("-0.25s").clicked() {
                            let _ = self.controller.set_subtitle_delay(cur_sub_delay - 0.25);
                        }
                        if ui.button("0.0s").clicked() {
                            let _ = self.controller.set_subtitle_delay(0.0);
                        }
                        if ui.button("+0.25s").clicked() {
                            let _ = self.controller.set_subtitle_delay(cur_sub_delay + 0.25);
                        }
                    });

                    // Secondary Subtitle Delay
                    let cur_sec_delay = self.controller.secondary_sub_delay();
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("Secondary Sub: {:+.2}s", cur_sec_delay))
                                .monospace(),
                        );
                        if ui.button("-0.25s").clicked() {
                            let _ = self
                                .controller
                                .set_secondary_sub_delay(cur_sec_delay - 0.25);
                        }
                        if ui.button("0.0s").clicked() {
                            let _ = self.controller.set_secondary_sub_delay(0.0);
                        }
                        if ui.button("+0.25s").clicked() {
                            let _ = self
                                .controller
                                .set_secondary_sub_delay(cur_sec_delay + 0.25);
                        }
                    });

                    // Audio Delay
                    let cur_audio_delay = self.controller.audio_delay().unwrap_or(0.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("Audio Offset: {:+.2}s", cur_audio_delay))
                                .monospace(),
                        );
                        if ui.button("-0.10s").clicked() {
                            let _ = self.controller.set_audio_delay(cur_audio_delay - 0.10);
                        }
                        if ui.button("0.0s").clicked() {
                            let _ = self.controller.set_audio_delay(0.0);
                        }
                        if ui.button("+0.10s").clicked() {
                            let _ = self.controller.set_audio_delay(cur_audio_delay + 0.10);
                        }
                    });

                    let mut vis = self.controller.sub_visibility();
                    if ui.checkbox(&mut vis, "Primary Subtitles Visible").changed() {
                        let _ = self.controller.set_subtitle_visibility(vis);
                    }

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(6.0);

                    // Section 3: Sleep Timer
                    ui.horizontal(|ui| {
                        icons::icon_label(ui, Icon::History, 14.0, Color32::from_rgb(250, 180, 80));
                        ui.label(
                            RichText::new("Sleep Timer")
                                .strong()
                                .color(Color32::from_rgb(250, 180, 80)),
                        );
                    });
                    ui.add_space(4.0);

                    ui.horizontal(|ui| {
                        let sleep_label = format!("Sleep: {}", self.sleep_timer.label());
                        egui::ComboBox::from_id_salt("popover_sleep_timer_combo")
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
                                        .selectable_label(
                                            self.sleep_timer == option,
                                            option.label(),
                                        )
                                        .clicked()
                                    {
                                        self.set_sleep_timer(option);
                                    }
                                }
                            });

                        if let Some(remaining) = self.sleep_remaining() {
                            let seconds = remaining.as_secs();
                            ui.label(
                                RichText::new(format!(
                                    "⏰ {:02}:{:02}",
                                    seconds / 60,
                                    seconds % 60
                                ))
                                .monospace()
                                .size(11.5)
                                .color(Color32::from_rgb(125, 211, 252)),
                            );
                        }
                    });

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(6.0);

                    // Section 4: External Subtitles & Developer Diagnostics
                    ui.horizontal(|ui| {
                        if icons::icon_text_button(
                            ui,
                            Icon::FolderOpen,
                            "Add Subtitle...",
                            "Load external subtitle file",
                        )
                        .clicked()
                        {
                            self.add_subtitle_dialog();
                        }

                        if ui
                            .button(if self.show_diagnostics {
                                "Hide Diagnostics"
                            } else {
                                "Show Diagnostics"
                            })
                            .on_hover_text("Toggle Developer Diagnostics Overlay")
                            .clicked()
                        {
                            self.show_diagnostics = !self.show_diagnostics;
                        }
                    });

                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("Alt+M to toggle · Esc to dismiss")
                            .size(10.0)
                            .color(BrandPalette::MUTED_TEXT),
                    );
                });
            self.player_more_overflow_open = more_open;
        }
    }

    /// Renders the specialized audio player experience featuring the 32-band spectrum visualizer
    /// or the classic high-contrast audio card when the visualizer is disabled.
    pub fn render_audio_player_viewport(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let center_pos = rect.center() - Vec2::new(0.0, 35.0);
        let card_w = 720.0_f32.min(rect.width() - 40.0);
        let card_h = 320.0_f32.min(rect.height() - 150.0);
        if card_w < 200.0 || card_h < 120.0 {
            return;
        }

        let card_rect = Rect::from_center_size(center_pos, Vec2::new(card_w, card_h));

        // High contrast Obsidian card
        ui.painter().rect(
            card_rect,
            10.0_f32,
            BrandPalette::OBSIDIAN,
            Stroke::new(1.5_f32, BrandPalette::SLATE_BORDER),
            StrokeKind::Inside,
        );

        let mut child_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(card_rect.shrink(16.0))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );

        // Header: Title, Artist, Mode Pill, and Toggle Button
        child_ui.horizontal(|ui| {
            icons::icon_label(ui, Icon::AudioTrack, 18.0, BrandPalette::ELECTRIC_LIME);
            ui.add_space(4.0);
            ui.vertical(|ui| {
                let title = self
                    .controller
                    .media_title()
                    .or_else(|| {
                        self.controller.current_file().map(|f| {
                            std::path::Path::new(f)
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or(f)
                                .to_string()
                        })
                    })
                    .unwrap_or_else(|| "Audio Playback".to_string());

                ui.label(
                    RichText::new(title)
                        .strong()
                        .size(15.0)
                        .color(BrandPalette::IVORY_TEXT),
                );

                let audio_codec = self.controller.audio_codec();
                let codec_desc = if audio_codec.is_empty() {
                    "Audio Stream".to_string()
                } else {
                    format!("Codec: {}", audio_codec.to_uppercase())
                };
                ui.label(
                    RichText::new(codec_desc)
                        .size(11.0)
                        .color(BrandPalette::MUTED_TEXT),
                );
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let vis_enabled = self.equalizer_settings.spectrum_visualizer_enabled;
                let toggle_label = if vis_enabled {
                    "Visualizer [ON]"
                } else {
                    "Visualizer [OFF]"
                };
                let toggle_color = if vis_enabled {
                    BrandPalette::ELECTRIC_LIME
                } else {
                    Color32::from_gray(160)
                };

                if ui
                    .button(
                        RichText::new(toggle_label)
                            .strong()
                            .size(11.0)
                            .color(toggle_color),
                    )
                    .on_hover_text("Toggle 32-band real-time audio spectrum visualizer")
                    .clicked()
                {
                    self.equalizer_settings.spectrum_visualizer_enabled = !vis_enabled;
                    self.apply_equalizer_settings();
                }

                ui.add_space(8.0);
                // Badge Pill: 32-Band Spectrum Analyzer
                egui::Frame::NONE
                    .fill(BrandPalette::SLATE_SURFACE)
                    .stroke(Stroke::new(1.0_f32, BrandPalette::BORDER_SLATE))
                    .corner_radius(4.0_f32)
                    .inner_margin(egui::Margin::symmetric(6, 2))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("32-BAND SPECTRUM")
                                .monospace()
                                .size(9.5)
                                .strong()
                                .color(BrandPalette::ELECTRIC_LIME),
                        );
                    });
            });
        });

        child_ui.add_space(8.0);

        if self.equalizer_settings.spectrum_visualizer_enabled {
            // Render 32-band real-time visualizer canvas
            let canvas_w = child_ui.available_width().max(280.0);
            let canvas_h = (child_ui.available_height() - 32.0).clamp(100.0, 180.0);
            let (viz_rect, _) =
                child_ui.allocate_exact_size(egui::vec2(canvas_w, canvas_h), Sense::hover());
            let painter = child_ui.painter_at(viz_rect);

            // Obsidian Dark Canvas
            painter.rect_filled(viz_rect, 4.0, Color32::from_rgb(8, 13, 18));
            painter.rect_stroke(
                viz_rect,
                4.0,
                Stroke::new(1.0_f32, Color32::from_rgb(24, 33, 43)),
                StrokeKind::Inside,
            );

            let left = viz_rect.left() + 12.0;
            let right = viz_rect.right() - 12.0;
            let top = viz_rect.top() + 12.0;
            let bottom = viz_rect.bottom() - 22.0; // Room for frequency axis labels
            let plot_w = (right - left).max(10.0);
            let plot_h = (bottom - top).max(10.0);

            // Draw subtle horizontal grid lines (-12dB, -24dB, -36dB)
            for &(rel_y, stroke_c) in &[
                (0.25_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 12)),
                (0.50_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 15)),
                (0.75_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 12)),
            ] {
                let y = top + plot_h * rel_y;
                painter.line_segment(
                    [egui::pos2(left, y), egui::pos2(right, y)],
                    Stroke::new(0.5_f32, stroke_c),
                );
            }

            // Draw baseline line
            painter.line_segment(
                [egui::pos2(left, bottom), egui::pos2(right, bottom)],
                Stroke::new(1.0_f32, Color32::from_rgb(45, 58, 70)),
            );

            // Render 32 bars
            let num_bars = muvrix_player::SPECTRUM_BANDS_COUNT;
            let gap = 3.0_f32.min((plot_w / 64.0).max(1.5));
            let bar_w = ((plot_w - (num_bars as f32 - 1.0) * gap) / num_bars as f32).max(2.0);

            let smoothed = self.spectrum_visualizer.smoothed_bands();

            for (i, &mag_val) in smoothed.iter().enumerate().take(num_bars) {
                let bar_x = left + i as f32 * (bar_w + gap);
                let mag = mag_val.clamp(0.0, 1.0);
                let bar_h = (plot_h * mag).max(1.0);
                let bar_top = bottom - bar_h;

                // Subtle inactive background bar
                let inactive_rect =
                    Rect::from_min_max(egui::pos2(bar_x, top), egui::pos2(bar_x + bar_w, bottom));
                painter.rect_filled(
                    inactive_rect,
                    1.5,
                    Color32::from_rgba_unmultiplied(200, 238, 117, 14),
                );

                // Active magnitude bar in Electric Lime
                let active_rect = Rect::from_min_max(
                    egui::pos2(bar_x, bar_top),
                    egui::pos2(bar_x + bar_w, bottom),
                );
                painter.rect_filled(active_rect, 1.5, BrandPalette::ELECTRIC_LIME);
            }

            // Frequency axis labels at octave landmarks
            let landmark_indices = [
                (0, "20"),
                (4, "50"),
                (7, "100"),
                (11, "250"),
                (14, "500"),
                (17, "1k"),
                (20, "2k"),
                (23, "4k"),
                (26, "8k"),
                (29, "16k"),
                (31, "20k"),
            ];

            for (idx, label) in landmark_indices {
                let bar_x = left + idx as f32 * (bar_w + gap) + bar_w * 0.5;
                painter.text(
                    egui::pos2(bar_x, bottom + 11.0),
                    egui::Align2::CENTER_CENTER,
                    label,
                    egui::FontId::monospace(8.5),
                    Color32::from_gray(130),
                );
            }

            // Footer telemetry summary row
            child_ui.add_space(4.0);
            child_ui.horizontal(|ui| {
                let norm_mode = self.equalizer_settings.effective_normalization_mode();
                let norm_label = match norm_mode {
                    muvrix_player::NormalizationMode::EbuR128 => {
                        format!(
                            "EBU R128 ({:.1} LUFS | {:.1} dBTP)",
                            self.equalizer_settings.ebu_r128.integrated_loudness,
                            self.equalizer_settings.ebu_r128.true_peak,
                        )
                    }
                    muvrix_player::NormalizationMode::Dynamic => {
                        "Dynamic Normalizer (dynaudnorm)".to_string()
                    }
                    muvrix_player::NormalizationMode::None => "Normalization: Bypassed".to_string(),
                };

                ui.label(
                    RichText::new(format!("Mode: {norm_label}"))
                        .size(11.0)
                        .color(Color32::from_gray(160)),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new("60 FPS FFT Telemetry")
                            .monospace()
                            .size(10.0)
                            .color(BrandPalette::ELECTRIC_LIME),
                    );
                });
            });
        } else {
            // Fallback view when visualizer is explicitly disabled
            child_ui.vertical_centered(|ui| {
                ui.add_space(20.0);
                icons::icon_label(ui, Icon::AudioTrack, 44.0, Color32::from_gray(100));
                ui.add_space(8.0);
                ui.label(
                    RichText::new("32-Band Spectrum Visualizer is Disabled")
                        .strong()
                        .size(14.0)
                        .color(BrandPalette::MUTED_TEXT),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new(
                        "Audio decoding and playback active with zero visualization overhead.",
                    )
                    .size(11.5)
                    .color(Color32::from_gray(130)),
                );
                ui.add_space(12.0);
                if icons::primary_icon_button_text(
                    ui,
                    Icon::Sliders,
                    "Enable 32-Band Visualizer",
                    "Engage real-time spectrum analysis",
                )
                .clicked()
                {
                    self.equalizer_settings.spectrum_visualizer_enabled = true;
                    self.apply_equalizer_settings();
                }
            });
        }
    }
}
