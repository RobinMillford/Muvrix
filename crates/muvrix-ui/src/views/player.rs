use std::sync::Arc;

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Shape, Stroke, StrokeKind, Vec2};
use muvrix_player::{
    AbLoopState, AccentColor, CropPreset, PlaybackState, ScalingMode, SeekMode, SteppingPreset,
    ThemeMode, TrackInfo, TrackType,
};

use crate::app::actions::{PlayerAction, ASPECT_PRESETS};
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
        // The deck floats over the video instead of shrinking the viewport, so
        // the picture keeps the full window and nothing re-flows on hover.
        let available = ctx.available_rect();
        let overlay_width = available.width();
        // The deck floats: 8 px from each edge and the bottom, with rounded
        // corners, so it reads as an overlay instead of a taskbar strip.
        let deck_w = (overlay_width - 16.0).max(360.0);
        let deck_inner_w = (deck_w - 24.0).max(320.0);
        egui::Area::new(egui::Id::new("player_controls_overlay"))
            .anchor(egui::Align2::CENTER_BOTTOM, egui::Vec2::new(0.0, -8.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                // Fully faded HUD paints nothing and, importantly, captures no
                // pointer: an invisible deck that still hit-tests is what made
                // hover behaviour feel broken.
                if self.overlay_alpha <= 0.01 {
                    self.deck_rect = None;
                    return;
                }
                ui.set_width(deck_w);
                ui.set_opacity(self.overlay_alpha);
                let deck = egui::Frame::NONE
                    .fill(BrandPalette::ELEVATED_CARD.gamma_multiply(0.94))
                    .stroke(egui::Stroke::new(1.0_f32, BrandPalette::BORDER_SLATE))
                    .corner_radius(10.0_f32)
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.add_space(1.0);
                        // Pin the inner width: an auto-sized Area otherwise
                        // keeps last frame's content width, which made the
                        // timeline half-length and looked broken.
                        ui.set_width(deck_inner_w);

                        let deck_width = deck_inner_w;

                        // -------------------------------------------------------------
                        // Row 1: Dedicated Timeline Row
                        // -------------------------------------------------------------
                        let (time_pos, duration) =
                            if *self.controller.state() == PlaybackState::Idle {
                                (0.0, 0.0)
                            } else {
                                (self.controller.time_pos(), self.controller.duration())
                            };

                        ui.horizontal(|ui| {
                            let cur_str = format_timestamp_ms(time_pos);
                            let dur_str = format_timestamp(duration);
                            let remaining_str = format_timestamp((duration - time_pos).max(0.0));

                            // Current position timestamp. VLC-sized: the clock
                            // is primary information, not a caption, so it gets
                            // a fixed-width slot and a readable 16 px face.
                            let time_w = 96.0;
                            let dur_w = 86.0;
                            ui.add_sized(
                                [time_w, 24.0],
                                egui::Label::new(
                                    RichText::new(&cur_str)
                                        .monospace()
                                        .strong()
                                        .size(16.0)
                                        .color(BrandPalette::PRIMARY_TEXT),
                                )
                                .selectable(false),
                            );

                            // Full-width seeker with hover timestamp tooltip.
                            let max_dur = if duration > 0.0 { duration } else { 1.0 };
                            let mut slider_val = if self.is_seeking {
                                self.seek_target
                            } else {
                                time_pos
                            };

                            // A plain Slider would paint at whatever
                            // `spacing.slider_width` or its parent offered; on
                            // smaller windows that was half the deck. This
                            // widget owns an exact rect, so the rail always
                            // spans the row and the hit area is unambiguous.
                            let slider_width =
                                (deck_width - time_w - dur_w - 20.0).max(120.0);
                            let (track_rect, response) = ui.allocate_exact_size(
                                egui::vec2(slider_width, 30.0),
                                Sense::click_and_drag(),
                            );

                            // Pointer position drives the value while dragging;
                            // the seek/commit logic below stays unchanged.
                            if response.is_pointer_button_down_on() {
                                if let Some(pos) = response.interact_pointer_pos() {
                                    let frac = ((pos.x - track_rect.min.x) / track_rect.width())
                                        .clamp(0.0, 1.0);
                                    slider_val = frac as f64 * max_dur;
                                }
                            }

                            // Rail: recessed track, played fill, buffered range,
                            // and the lime scrub pin are painted here. Chapters,
                            // bookmarks and the A-B badges paint on top below.
                            {
                                let painter = ui.painter();
                                let rail = Rect::from_center_size(
                                    response.rect.center(),
                                    egui::vec2(response.rect.width(), 6.0),
                                );
                                painter.rect_filled(rail, 3.0, Color32::from_gray(64));

                                let accent = ui.visuals().selection.bg_fill;
                                let frac = (slider_val / max_dur).clamp(0.0, 1.0) as f32;
                                if frac > 0.0 {
                                    let played = Rect::from_min_max(
                                        rail.min,
                                        Pos2::new(rail.min.x + frac * rail.width(), rail.max.y),
                                    );
                                    painter.rect_filled(played, 3.0, accent);
                                }

                                let pin_x = response.rect.min.x + frac * response.rect.width();
                                let radius = if response.hovered() || self.is_seeking {
                                    8.0
                                } else {
                                    6.0
                                };
                                painter.circle_filled(
                                    Pos2::new(pin_x, response.rect.center().y),
                                    radius,
                                    accent,
                                );
                                painter.circle_stroke(
                                    Pos2::new(pin_x, response.rect.center().y),
                                    radius,
                                    Stroke::new(2.0_f32, BrandPalette::OBSIDIAN),
                                );
                            }

                            // Context-aware hover information: A-B loop boundaries, chapter title, bookmark pin details, or seek timestamp
                            let mut hovered_chapter: Option<(i64, f64)> = None;
                            let mut hovered_ab_point: Option<f64> = None;
                            let (hover_sec, custom_hover) = if let Some(mouse_pos) =
                                response.hover_pos()
                            {
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
                                            let x_a = response.rect.min.x
                                                + frac_a * response.rect.width();
                                            let x_b = response.rect.min.x
                                                + frac_b * response.rect.width();
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
                                            let x_a = response.rect.min.x
                                                + frac_a * response.rect.width();
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
                                            let frac = (bm.timestamp_seconds() / duration)
                                                .clamp(0.0, 1.0)
                                                as f32;
                                            let x =
                                                response.rect.min.x + frac * response.rect.width();
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
                                            let x =
                                                response.rect.min.x + frac * response.rect.width();
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
                                        let x_a =
                                            response.rect.min.x + frac_a * response.rect.width();
                                        let x_b =
                                            response.rect.min.x + frac_b * response.rect.width();

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
                                        let x_a =
                                            response.rect.min.x + frac_a * response.rect.width();

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
                                    let frac =
                                        (bm.timestamp_seconds() / duration).clamp(0.0, 1.0) as f32;
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
                                let x_start =
                                    response.rect.min.x + frac_cur * response.rect.width();
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
                                    let _ =
                                        self.controller.seek(self.seek_target, SeekMode::Absolute);
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

                            // Duration timestamp — same optical weight as the
                            // current time so the row reads like a player clock,
                            // not a debug readout.
                            ui.add_sized(
                                [dur_w, 24.0],
                                egui::Label::new(
                                    RichText::new(&dur_str)
                                        .monospace()
                                        .size(16.0)
                                        .color(BrandPalette::PRIMARY_TEXT.gamma_multiply(0.82)),
                                )
                                .selectable(false),
                            )
                            .on_hover_text(format!(
                                "Duration: {} (Remaining: -{})",
                                dur_str, remaining_str
                            ));
                        });

                        // -------------------------------------------------------------
                        // Row 2: one horizontal band.
                        //   left third  → queue, mute, volume
                        //   middle third→ Previous · Rewind · Play/Pause · Forward · Next
                        //   right third → audio, subtitles, speed, Video View,
                        //                 screenshot, more
                        // Three equal thirds put the transport cluster exactly on the
                        // video centre line at every width; nothing re-flows on hover.
                        // -------------------------------------------------------------
                        ui.add_space(2.0);
                        let band_w = ui.available_width();
                        let third = (band_w / 3.0).max(1.0);
                        // Responsive tiers: a 3-way band cannot hold every
                        // control at 800 px without collisions, so the least
                        // used buttons retreat into the overflow popover.
                        let compact = band_w < 1000.0;
                        let narrow = band_w < 820.0;

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;

                            // ---- Left third --------------------------------------
                            let left_slot = ui.allocate_ui_with_layout(
                                egui::vec2(third, 44.0),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    if !self.is_cinema_mode
                                        && icons::icon_button_sized(
                                            ui,
                                            Icon::Playlist,
                                            Vec2::new(28.0, 26.0),
                                            "Playlist (Ctrl+L)",
                                        )
                                        .clicked()
                                    {
                                        self.show_playlist_drawer = !self.show_playlist_drawer;
                                    }

                                    let (mute_icon, mute_tip) = if self.controller.is_muted() {
                                        (Icon::VolumeMuted, "Unmute (M)")
                                    } else if self.controller.volume() < 40.0 {
                                        (Icon::VolumeLow, "Mute (M)")
                                    } else {
                                        (Icon::VolumeHigh, "Mute (M)")
                                    };
                                    if icons::icon_button_sized(
                                        ui,
                                        mute_icon,
                                        Vec2::new(26.0, 26.0),
                                        mute_tip,
                                    )
                                    .clicked()
                                    {
                                        self.execute_player_action(PlayerAction::ToggleMute, ctx);
                                    }

                                    let mut vol = self.controller.volume();
                                    let max_volume = self.controller.volume_boost_max().max(100.0);
                                    let vol_w = if deck_width < 1100.0 { 48.0 } else { 84.0 };
                                    let vol_res = ui
                                        .add_sized(
                                            [vol_w, 16.0],
                                            egui::Slider::new(&mut vol, 0.0..=max_volume)
                                                .show_value(false),
                                        )
                                        .on_hover_text("Volume — scroll over the video also works");
                                    if vol_res.changed() {
                                        let _ = self.controller.set_volume(vol);
                                    }
                                    if !narrow {
                                        ui.add_sized(
                                            [34.0, 18.0],
                                            egui::Label::new(
                                                RichText::new(format!("{:.0}%", vol))
                                                    .monospace()
                                                    .size(10.0)
                                                    .color(BrandPalette::MUTED_TEXT),
                                            )
                                            .selectable(false),
                                        );
                                    }
                                },
                            );
                            // Content-sized children used to pull the middle
                            // third left; pad the slot back out to a true third.
                            ui.add_space((third - left_slot.response.rect.width()).max(0.0));

                            // ---- Middle third: transport, exactly centred --------
                            // The cluster must fit inside its third at the
                            // 640 px minimum window; a fixed 232 px cluster
                            // overflowed into the right-hand controls.
                            let transport_compact = third < 250.0;
                            let (nav_sz, seek_sz, play_sz, gap, cluster_w) =
                                if transport_compact {
                                    (26.0, 28.0, 44.0, 6.0, 176.0)
                                } else {
                                    (30.0, 32.0, 52.0, 12.0, 224.0)
                                };
                            let mid_slot = ui.allocate_ui_with_layout(
                                egui::vec2(third, 44.0),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.add_space(((third - cluster_w) * 0.5).max(0.0));

                                    let hint_prev =
                                        self.keybindings.hint(PlayerAction::PreviousMedia);
                                    if icons::icon_button_sized(
                                        ui,
                                        Icon::SkipPrevious,
                                        Vec2::new(nav_sz, nav_sz),
                                        &format!("Previous chapter / track ({hint_prev})"),
                                    )
                                    .clicked()
                                    {
                                        if !self.controller.chapters().is_empty() {
                                            self.execute_player_action(
                                                PlayerAction::PreviousChapter,
                                                ctx,
                                            );
                                        } else {
                                            self.execute_player_action(
                                                PlayerAction::PreviousMedia,
                                                ctx,
                                            );
                                        }
                                    }

                                    ui.add_space(gap);

                                    let hint_back =
                                        self.keybindings.hint(PlayerAction::SeekBackwardMedium);
                                    if icons::icon_button_sized(
                                        ui,
                                        Icon::SeekBackward,
                                        Vec2::new(seek_sz, nav_sz),
                                        &format!("Back {:.0}s ({hint_back})", self.skip_seconds),
                                    )
                                    .clicked()
                                    {
                                        self.seek_relative(-self.skip_seconds);
                                        self.push_osd(format!("⏪ {:.0}s", self.skip_seconds));
                                    }

                                    ui.add_space(gap);

                                    let (play_icon, play_tip) = match self.controller.state() {
                                        PlaybackState::Playing => (Icon::Pause, "Pause (Space)"),
                                        _ => (Icon::Play, "Play (Space)"),
                                    };
                                    if icons::electric_play_button(
                                        ui,
                                        play_icon,
                                        Vec2::new(play_sz, play_sz * 0.73),
                                        play_tip,
                                    )
                                    .clicked()
                                    {
                                        self.execute_player_action(PlayerAction::PlayPause, ctx);
                                    }

                                    ui.add_space(gap);

                                    let hint_fwd =
                                        self.keybindings.hint(PlayerAction::SeekForwardMedium);
                                    if icons::icon_button_sized(
                                        ui,
                                        Icon::SeekForward,
                                        Vec2::new(seek_sz, nav_sz),
                                        &format!("Forward {:.0}s ({hint_fwd})", self.skip_seconds),
                                    )
                                    .clicked()
                                    {
                                        self.seek_relative(self.skip_seconds);
                                        self.push_osd(format!("⏩ {:.0}s", self.skip_seconds));
                                    }

                                    ui.add_space(gap);

                                    let hint_next = self.keybindings.hint(PlayerAction::NextMedia);
                                    if icons::icon_button_sized(
                                        ui,
                                        Icon::SkipNext,
                                        Vec2::new(nav_sz, nav_sz),
                                        &format!("Next chapter / track ({hint_next})"),
                                    )
                                    .clicked()
                                    {
                                        if !self.controller.chapters().is_empty() {
                                            self.execute_player_action(
                                                PlayerAction::NextChapter,
                                                ctx,
                                            );
                                        } else {
                                            self.execute_player_action(
                                                PlayerAction::NextMedia,
                                                ctx,
                                            );
                                        }
                                    }
                                },
                            );
                            ui.add_space((third - mid_slot.response.rect.width()).max(0.0));

                            // ---- Right third -------------------------------------
                            ui.allocate_ui_with_layout(
                                egui::vec2(third, 44.0),
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    // Overflow: advanced controls (frame stepping, jog,
                                    // sync, sleep timer) live in one anchored popover.
                                    let more_text = if self.player_more_overflow_open {
                                        RichText::new("•••")
                                            .size(13.0)
                                            .strong()
                                            .color(BrandPalette::ELECTRIC_LIME)
                                    } else {
                                        RichText::new("•••")
                                            .size(13.0)
                                            .strong()
                                            .color(BrandPalette::PRIMARY_TEXT)
                                    };
                                    if ui
                                        .button(more_text)
                                        .on_hover_text("More playback controls (Alt+M)")
                                        .clicked()
                                    {
                                        self.player_more_overflow_open =
                                            !self.player_more_overflow_open;
                                        self.player_view_menu_open = false;
                                    }

                                    if !compact
                                        && icons::icon_button_sized(
                                            ui,
                                            Icon::Screenshot,
                                            Vec2::new(26.0, 26.0),
                                            &format!(
                                                "Screenshot ({})",
                                                self.keybindings.hint(PlayerAction::Screenshot)
                                            ),
                                        )
                                        .clicked()
                                    {
                                        self.execute_player_action(PlayerAction::Screenshot, ctx);
                                    }

                                    if !compact {
                                        let view_open = self.player_view_menu_open;
                                        let aspect_label = self.current_aspect_label();
                                        if icons::icon_button_sized(
                                            ui,
                                            Icon::AspectRatio,
                                            Vec2::new(28.0, 26.0),
                                            &format!(
                                                "Video view — aspect, crop, fit, zoom ({aspect_label})"
                                            ),
                                        )
                                        .clicked()
                                        {
                                            self.player_view_menu_open =
                                                !self.player_view_menu_open;
                                            self.player_more_overflow_open = false;
                                        }
                                        let _ = view_open;
                                    }

                                    if !narrow {
                                        self.speed_menu(ui);
                                    }
                                    self.subtitle_track_menu(ui);
                                    self.audio_track_menu(ui);
                                },
                            );
                        });

                        // Advanced deck (chapters, A-B, repeat, bookmarks) is reached
                        // through the overflow popover; keeping it out of the deck is
                        // what makes the HUD ~90 px rather than ~290 px tall.
                    });
                self.deck_rect = Some(deck.response.rect);
            });

        self.render_player_top_overlay(ctx);
        self.render_player_view_popover(ctx);
        self.render_playback_stats(ctx);
        self.render_sync_panel(ctx);
        self.render_osd(ctx);

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

                // Mouse wheel over the video adjusts volume; horizontal wheel
                // seeks. Both are also available from the keyboard.
                if response.hovered() {
                    let (dy, dx) =
                        ctx.input(|i| (i.smooth_scroll_delta.y, i.smooth_scroll_delta.x));
                    if dy.abs() > 0.5 {
                        let step = if dy > 0.0 { 2.5 } else { -2.5 };
                        let _ = self.controller.adjust_volume(step);
                        self.push_osd(format!("Volume: {:.0}%", self.controller.volume()));
                    } else if dx.abs() > 0.5 {
                        self.execute_player_action(
                            if dx > 0.0 {
                                PlayerAction::SeekForwardShort
                            } else {
                                PlayerAction::SeekBackwardShort
                            },
                            ctx,
                        );
                    }
                }

                response.context_menu(|ui| self.player_context_menu(ui, ctx));

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
                        Pos2::new(rect.center().x - banner_w * 0.5, rect.min.y + 64.0),
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

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui
                            .button(if self.show_playback_stats {
                                "Hide statistics"
                            } else {
                                "Playback statistics"
                            })
                            .on_hover_text("Real libmpv timing, drop and sync counters (Ctrl+I)")
                            .clicked()
                        {
                            self.show_playback_stats = !self.show_playback_stats;
                            if let Some(probe) = &self.diagnostics {
                                probe.set_enabled(self.show_playback_stats);
                            }
                        }
                        if ui
                            .button("A/V sync…")
                            .on_hover_text(
                                "Audio/video and audio/subtitle delay offsets with 10/50/100 ms steps",
                            )
                            .clicked()
                        {
                            self.sync_panel_open = !self.sync_panel_open;
                        }
                        if ui.button("Export report").clicked() {
                            self.execute_player_action(PlayerAction::ExportDiagnostics, ctx);
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

// ---------------------------------------------------------------------------
// Player overlay chrome: top bar, secondary toolbars and the video-view popover
// ---------------------------------------------------------------------------

impl MuvrixApp {
    /// Subtle gradient top bar over the video: back, title, media info and the
    /// window-level controls on the right.
    pub(crate) fn render_player_top_overlay(&mut self, ctx: &egui::Context) {
        if self.overlay_alpha <= 0.01 {
            self.top_overlay_rect = None;
            return;
        }
        // Start below the app navigation bar (and the diagnostics strip when
        // it is open) so the two bars never stack on top of each other.
        let available = ctx.available_rect();
        let width = available.width();
        let height = 52.0;

        egui::Area::new(egui::Id::new("player_top_overlay"))
            .fixed_pos(available.min)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.set_width(width);
                ui.set_opacity(self.overlay_alpha);
                let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
                self.top_overlay_rect = Some(rect);

                // Gradient: opaque at the top edge, transparent above the video.
                {
                    let painter = ui.painter().clone();
                    let alphas = [150_u8, 112, 74, 38, 0];
                    let steps = alphas.len() as f32;
                    for (i, alpha) in alphas.iter().enumerate() {
                        let y0 = rect.min.y + height * i as f32 / steps;
                        let y1 = rect.min.y + height * (i as f32 + 1.0) / steps;
                        painter.rect_filled(
                            Rect::from_min_max(
                                Pos2::new(rect.min.x, y0),
                                Pos2::new(rect.max.x, y1),
                            ),
                            0.0,
                            Color32::from_black_alpha(*alpha),
                        );
                    }
                }

                let inner = rect.shrink2(egui::vec2(14.0, 6.0));
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(inner)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );

                // Cinema mode is video-only: no navigation affordances.
                if !self.is_cinema_mode {
                    if icons::icon_button_sized(
                        &mut child,
                        Icon::ArrowBack,
                        Vec2::new(28.0, 26.0),
                        "Back to Home",
                    )
                    .clicked()
                    {
                        self.current_view = AppView::Home;
                    }
                    child.add_space(6.0);
                }

                let title = self
                    .controller
                    .current_file()
                    .and_then(|f| std::path::Path::new(f).file_name())
                    .and_then(|f| f.to_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "MUVRIX Player".to_string());
                let (vw, vh) = self.controller.video_pixel_size();
                let info = if vw > 1 && vh > 1 {
                    format!("{vw}×{vh}")
                } else {
                    String::new()
                };

                let title_w = (inner.width() - 300.0).max(140.0);
                child.add_sized(
                    [title_w, 22.0],
                    egui::Label::new(
                        RichText::new(title)
                            .size(13.0)
                            .strong()
                            .color(BrandPalette::PRIMARY_TEXT),
                    )
                    .wrap_mode(egui::TextWrapMode::Truncate),
                );
                if !info.is_empty() && width >= 780.0 {
                    child.label(
                        RichText::new(info)
                            .monospace()
                            .size(11.0)
                            .color(BrandPalette::MUTED_TEXT),
                    );
                }

                child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (fs_icon, fs_tip) = if self.is_fullscreen || self.is_cinema_mode {
                        (Icon::Windowed, "Exit fullscreen (F / Esc / Shift+Enter)")
                    } else {
                        (Icon::Fullscreen, "Fullscreen (F / F11)")
                    };
                    if icons::icon_button_sized(ui, fs_icon, Vec2::new(28.0, 26.0), fs_tip)
                        .clicked()
                    {
                        if self.is_cinema_mode {
                            self.execute_player_action(PlayerAction::ExitFullscreen, ctx);
                        } else {
                            self.execute_player_action(PlayerAction::ToggleFullscreen, ctx);
                        }
                    }

                    if !self.is_cinema_mode {
                        ui.add_space(2.0);

                        if icons::icon_button_sized(
                            ui,
                            Icon::Settings,
                            Vec2::new(26.0, 26.0),
                            "Player settings",
                        )
                        .clicked()
                        {
                            self.current_view = AppView::Settings;
                        }

                        ui.add_space(2.0);

                        let pin_tip = if self.always_on_top {
                            "Always on top: on"
                        } else {
                            "Always on top: off"
                        };
                        if icons::icon_button_sized(ui, Icon::Pin, Vec2::new(26.0, 26.0), pin_tip)
                            .clicked()
                        {
                            self.execute_player_action(PlayerAction::ToggleAlwaysOnTop, ctx);
                        }

                        ui.add_space(2.0);

                        if width >= 780.0 {
                            let pip_tip = if self.is_pip {
                                "Exit mini-player (I)"
                            } else {
                                "Mini-player (I)"
                            };
                            if icons::icon_button_sized(
                                ui,
                                Icon::Pip,
                                Vec2::new(26.0, 26.0),
                                pip_tip,
                            )
                            .clicked()
                            {
                                self.execute_player_action(PlayerAction::ToggleMiniPlayer, ctx);
                            }
                        }
                    }
                });
            });
    }

    /// Compact audio-track selector for the secondary toolbar.
    pub(crate) fn audio_track_menu(&mut self, ui: &mut egui::Ui) {
        let tracks: Vec<TrackInfo> = self
            .controller
            .tracks_by_type(TrackType::Audio)
            .into_iter()
            .cloned()
            .collect();
        let selected = self
            .controller
            .selected_track(TrackType::Audio)
            .map(|t| t.id);
        // Compact badge + popover instead of a permanently visible dropdown:
        // the deck stays icon-sized and the menu carries the detail.
        ui.menu_button(
            RichText::new("AUD")
                .size(9.5)
                .strong()
                .color(BrandPalette::MUTED_TEXT),
            |ui| {
                if ui.selectable_label(selected.is_none(), "Off").clicked() {
                    let _ = self.controller.set_audio_track(None);
                }
                for track in &tracks {
                    if ui
                        .selectable_label(selected == Some(track.id), format_track_label(track))
                        .clicked()
                    {
                        let _ = self.controller.set_audio_track(Some(track.id));
                    }
                }
                if tracks.is_empty() {
                    ui.label(
                        RichText::new("No audio tracks")
                            .size(11.0)
                            .color(BrandPalette::MUTED_TEXT),
                    );
                }
            },
        )
        .response
        .on_hover_text("Audio track (B)");
    }

    /// Compact subtitle selector, including the secondary (bilingual) track.
    pub(crate) fn subtitle_track_menu(&mut self, ui: &mut egui::Ui) {
        let tracks: Vec<TrackInfo> = self
            .controller
            .tracks_by_type(TrackType::Subtitle)
            .into_iter()
            .cloned()
            .collect();
        let primary = self
            .controller
            .selected_track(TrackType::Subtitle)
            .map(|t| t.id);
        let secondary = self.controller.secondary_sub_track_id();
        let active = self.controller.sub_visibility() && (primary.is_some() || secondary.is_some());
        ui.menu_button(
            RichText::new("CC").size(9.5).strong().color(if active {
                BrandPalette::ELECTRIC_LIME
            } else {
                BrandPalette::MUTED_TEXT
            }),
            |ui| {
                ui.label(
                    RichText::new("Primary (bottom)")
                        .strong()
                        .size(10.5)
                        .color(BrandPalette::MUTED_TEXT),
                );
                if ui.selectable_label(primary.is_none(), "Off").clicked() {
                    let _ = self.controller.set_subtitle_track(None);
                }
                for track in &tracks {
                    if ui
                        .selectable_label(primary == Some(track.id), format_track_label(track))
                        .clicked()
                    {
                        let _ = self.controller.set_subtitle_track(Some(track.id));
                    }
                }

                ui.separator();
                ui.label(
                    RichText::new("Secondary (top)")
                        .strong()
                        .size(10.5)
                        .color(BrandPalette::MUTED_TEXT),
                );
                if ui.selectable_label(secondary.is_none(), "Off").clicked() {
                    let _ = self.controller.set_secondary_sub_track(None);
                }
                for track in &tracks {
                    if ui
                        .selectable_label(secondary == Some(track.id), format_track_label(track))
                        .clicked()
                    {
                        let _ = self.controller.set_secondary_sub_track(Some(track.id));
                    }
                }

                ui.separator();
                if icons::icon_text_button(
                    ui,
                    Icon::FolderOpen,
                    "Load subtitle file…",
                    "Add an external subtitle file",
                )
                .clicked()
                {
                    self.add_subtitle_dialog();
                }
            },
        )
        .response
        .on_hover_text("Subtitles (V)");
    }

    /// Compact playback-speed selector with presets.
    pub(crate) fn speed_menu(&mut self, ui: &mut egui::Ui) {
        let speed = self.controller.speed();
        ui.menu_button(
            RichText::new(format!("{speed:.2}×"))
                .size(10.0)
                .strong()
                .color(if (speed - 1.0).abs() > 0.01 {
                    BrandPalette::ELECTRIC_LIME
                } else {
                    BrandPalette::PRIMARY_TEXT
                }),
            |ui| {
                for preset in [0.25_f64, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 3.0, 4.0] {
                    let label = if (preset - 1.0).abs() < f64::EPSILON {
                        "1.00× (Normal)".to_string()
                    } else {
                        format!("{preset:.2}×")
                    };
                    if ui
                        .selectable_label((speed - preset).abs() < 0.01, label)
                        .clicked()
                    {
                        let _ = self.controller.set_speed(preset);
                    }
                }
                ui.separator();
                if ui.button("Slower").clicked() {
                    let _ = self.controller.adjust_speed(-0.25);
                }
                if ui.button("Faster").clicked() {
                    let _ = self.controller.adjust_speed(0.25);
                }
            },
        )
        .response
        .on_hover_text("Playback speed (] / [)");
    }

    /// Popover covering aspect ratio, fit mode, crop, zoom, pan and the
    /// deinterlace/deband toggles. Every control writes the real libmpv
    /// property and re-reads engine state on the next frame.
    pub(crate) fn render_player_view_popover(&mut self, ctx: &egui::Context) {
        if !self.player_view_menu_open {
            return;
        }

        let mut open = true;
        egui::Window::new("Video View")
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .default_width(348.0)
            .anchor(egui::Align2::RIGHT_BOTTOM, [-16.0, -142.0])
            .frame(
                egui::Frame::NONE
                    .fill(BrandPalette::ELEVATED_CARD)
                    .stroke(egui::Stroke::new(1.0_f32, BrandPalette::BORDER_SLATE))
                    .corner_radius(10.0_f32)
                    .inner_margin(egui::Margin::symmetric(14, 12)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Video View")
                            .strong()
                            .size(13.0)
                            .color(BrandPalette::PRIMARY_TEXT),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icons::icon_button_sized(
                            ui,
                            Icon::Close,
                            Vec2::new(22.0, 22.0),
                            "Close (Esc)",
                        )
                        .clicked()
                        {
                            open = false;
                        }
                    });
                });
                ui.add_space(6.0);

                // Aspect ratio -------------------------------------------------
                ui.label(
                    RichText::new("ASPECT RATIO")
                        .size(10.0)
                        .strong()
                        .color(BrandPalette::MUTED_TEXT),
                );
                let current_aspect = crate::app::actions::normalize_aspect_public(
                    &self.controller.aspect_override(),
                );
                ui.horizontal_wrapped(|ui| {
                    for (value, label) in ASPECT_PRESETS {
                        let selected = current_aspect == *value;
                        if ui.add(chip(selected, label)).clicked() {
                            self.apply_aspect(value);
                        }
                    }
                });

                ui.add_space(8.0);

                // Fit mode -----------------------------------------------------
                ui.label(
                    RichText::new("FIT")
                        .size(10.0)
                        .strong()
                        .color(BrandPalette::MUTED_TEXT),
                );
                let current_mode = self.controller.scaling_mode();
                ui.horizontal_wrapped(|ui| {
                    for mode in ScalingMode::ALL {
                        let selected = current_mode == *mode;
                        if ui.add(chip(selected, mode.label())).clicked() {
                            if let Err(e) = self.controller.set_scaling_mode(*mode) {
                                self.push_osd(format!("View mode unavailable: {e}"));
                            }
                        }
                    }
                });

                ui.add_space(8.0);

                // Crop ---------------------------------------------------------
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("CROP")
                            .size(10.0)
                            .strong()
                            .color(BrandPalette::MUTED_TEXT),
                    );
                    let crop = self.controller.video_crop();
                    if !crop.is_empty() {
                        ui.label(
                            RichText::new(&crop)
                                .monospace()
                                .size(10.0)
                                .color(BrandPalette::MUTED_TEXT),
                        );
                    }
                });
                let current_crop = self.crop_preset_index();
                ui.horizontal_wrapped(|ui| {
                    for (idx, preset) in CropPreset::ALL.iter().enumerate() {
                        let selected = idx == current_crop;
                        if ui.add(chip(selected, preset.label())).clicked() {
                            if let Err(e) = self.controller.set_crop_preset(*preset) {
                                self.push_osd(format!("Crop unavailable: {e}"));
                            }
                        }
                    }
                });

                ui.add_space(8.0);

                // Zoom ---------------------------------------------------------
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("ZOOM")
                            .size(10.0)
                            .strong()
                            .color(BrandPalette::MUTED_TEXT),
                    );
                    if icons::icon_button_sized(
                        ui,
                        Icon::ZoomOut,
                        Vec2::new(26.0, 24.0),
                        "Zoom out (Alt+-)",
                    )
                    .clicked()
                    {
                        self.execute_player_action(PlayerAction::ZoomOut, ctx);
                    }
                    ui.label(
                        RichText::new(format!("{:+.1}", self.controller.video_zoom()))
                            .monospace()
                            .size(11.0)
                            .color(BrandPalette::PRIMARY_TEXT),
                    );
                    if icons::icon_button_sized(
                        ui,
                        Icon::ZoomIn,
                        Vec2::new(26.0, 24.0),
                        "Zoom in (Alt++)",
                    )
                    .clicked()
                    {
                        self.execute_player_action(PlayerAction::ZoomIn, ctx);
                    }
                    if ui
                        .small_button("Reset")
                        .on_hover_text("Reset zoom (Alt+Backspace)")
                        .clicked()
                    {
                        self.execute_player_action(PlayerAction::ResetZoom, ctx);
                    }
                });

                // Pan ----------------------------------------------------------
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("PAN")
                            .size(10.0)
                            .strong()
                            .color(BrandPalette::MUTED_TEXT),
                    );
                    for (label, action, tip) in [
                        ("↑", PlayerAction::PanUp, "Pan up (Alt+Up)"),
                        ("↓", PlayerAction::PanDown, "Pan down (Alt+Down)"),
                        ("←", PlayerAction::PanLeft, "Pan left (Alt+Left)"),
                        ("→", PlayerAction::PanRight, "Pan right (Alt+Right)"),
                    ] {
                        if ui.small_button(label).on_hover_text(tip).clicked() {
                            self.execute_player_action(action, ctx);
                        }
                    }
                    let (px, py) = self.controller.video_pan();
                    ui.label(
                        RichText::new(format!("{px:+.2} / {py:+.2}"))
                            .monospace()
                            .size(10.0)
                            .color(BrandPalette::MUTED_TEXT),
                    );
                    if ui.small_button("Center").clicked() {
                        self.execute_player_action(PlayerAction::CenterVideo, ctx);
                    }
                });

                ui.add_space(8.0);
                ui.separator();

                // Filters ------------------------------------------------------
                let mut deinterlace = self.controller.deinterlace();
                if ui
                    .checkbox(&mut deinterlace, RichText::new("Deinterlace").size(11.5))
                    .changed()
                {
                    if let Err(e) = self.controller.set_deinterlace(deinterlace) {
                        self.push_osd(format!("Deinterlace unavailable: {e}"));
                    }
                }
                let mut deband = self.controller.deband_enabled();
                if ui
                    .checkbox(&mut deband, RichText::new("Deband").size(11.5))
                    .changed()
                {
                    if let Err(e) = self.controller.set_deband_enabled(deband) {
                        self.push_osd(format!("Deband unavailable: {e}"));
                    }
                }

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if icons::icon_text_button(
                        ui,
                        Icon::Screenshot,
                        "Screenshot",
                        "Save a PNG of the current frame",
                    )
                    .clicked()
                    {
                        self.execute_player_action(PlayerAction::Screenshot, ctx);
                    }
                    if ui
                        .button("Reset all")
                        .on_hover_text(
                            "Restore auto aspect, default crop, 100% zoom and centred pan",
                        )
                        .clicked()
                    {
                        self.apply_aspect("auto");
                        let _ = self.controller.set_crop_preset(CropPreset::Default);
                        let _ = self.controller.set_video_zoom(0.0);
                        let _ = self.controller.reset_video_pan();
                        let _ = self.controller.set_scaling_mode(ScalingMode::Fit);
                        self.push_osd("Video view reset");
                    }
                });
            });

        if !open {
            self.player_view_menu_open = false;
        }
    }
}

/// Small selected/unselected pill used across the video-view popover.
fn chip(selected: bool, label: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(label).size(10.5).color(if selected {
        BrandPalette::OBSIDIAN
    } else {
        BrandPalette::PRIMARY_TEXT
    }))
    .fill(if selected {
        BrandPalette::ELECTRIC_LIME
    } else {
        BrandPalette::SLATE_SURFACE
    })
    .stroke(egui::Stroke::new(
        1.0_f32,
        if selected {
            BrandPalette::ELECTRIC_LIME
        } else {
            BrandPalette::BORDER_SLATE
        },
    ))
    .min_size(egui::vec2(0.0, 22.0))
}

impl MuvrixApp {
    /// Right-click menu on the video surface. Groups mirror the toolbars so
    /// every action is reachable without leaving the picture.
    pub(crate) fn player_context_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.set_min_width(230.0);

        let playing = *self.controller.state() == PlaybackState::Playing;
        if ui.button(if playing { "Pause" } else { "Play" }).clicked() {
            self.execute_player_action(PlayerAction::PlayPause, ctx);
            ui.close_menu();
        }
        if ui.button("Stop").clicked() {
            self.execute_player_action(PlayerAction::Stop, ctx);
            ui.close_menu();
        }
        ui.horizontal(|ui| {
            if ui.button("Back 10s").clicked() {
                self.execute_player_action(PlayerAction::SeekBackwardMedium, ctx);
                ui.close_menu();
            }
            if ui.button("Forward 10s").clicked() {
                self.execute_player_action(PlayerAction::SeekForwardMedium, ctx);
                ui.close_menu();
            }
        });

        ui.separator();

        ui.menu_button("Audio", |ui| {
            let tracks: Vec<TrackInfo> = self
                .controller
                .tracks_by_type(TrackType::Audio)
                .into_iter()
                .cloned()
                .collect();
            let selected = self
                .controller
                .selected_track(TrackType::Audio)
                .map(|t| t.id);
            if ui.selectable_label(selected.is_none(), "Off").clicked() {
                let _ = self.controller.set_audio_track(None);
                ui.close_menu();
            }
            for track in &tracks {
                if ui
                    .selectable_label(selected == Some(track.id), format_track_label(track))
                    .clicked()
                {
                    let _ = self.controller.set_audio_track(Some(track.id));
                    ui.close_menu();
                }
            }
            ui.separator();
            if ui.button("Cycle audio track").clicked() {
                self.execute_player_action(PlayerAction::CycleAudioTrack, ctx);
                ui.close_menu();
            }
        });

        ui.menu_button("Subtitles", |ui| {
            let tracks: Vec<TrackInfo> = self
                .controller
                .tracks_by_type(TrackType::Subtitle)
                .into_iter()
                .cloned()
                .collect();
            let selected = self
                .controller
                .selected_track(TrackType::Subtitle)
                .map(|t| t.id);
            if ui.selectable_label(selected.is_none(), "Off").clicked() {
                let _ = self.controller.set_subtitle_track(None);
                ui.close_menu();
            }
            for track in &tracks {
                if ui
                    .selectable_label(selected == Some(track.id), format_track_label(track))
                    .clicked()
                {
                    let _ = self.controller.set_subtitle_track(Some(track.id));
                    ui.close_menu();
                }
            }
            ui.separator();
            if ui.button("Load subtitle file…").clicked() {
                self.execute_player_action(PlayerAction::AddSubtitleFile, ctx);
                ui.close_menu();
            }
        });

        ui.menu_button("Video", |ui| {
            if ui.button("Aspect ratio…").clicked() {
                self.player_view_menu_open = true;
                ui.close_menu();
            }
            for (value, label) in ASPECT_PRESETS.iter().take(4) {
                if ui.button(format!("Aspect {label}")).clicked() {
                    self.apply_aspect(value);
                    ui.close_menu();
                }
            }
            ui.separator();
            if ui.button("Zoom in").clicked() {
                self.execute_player_action(PlayerAction::ZoomIn, ctx);
                ui.close_menu();
            }
            if ui.button("Zoom out").clicked() {
                self.execute_player_action(PlayerAction::ZoomOut, ctx);
                ui.close_menu();
            }
            if ui.button("Reset zoom & pan").clicked() {
                self.execute_player_action(PlayerAction::ResetZoom, ctx);
                self.execute_player_action(PlayerAction::CenterVideo, ctx);
                ui.close_menu();
            }
            ui.separator();
            ui.menu_button("Skip duration", |ui| {
                for secs in [5.0_f64, 10.0, 30.0, 60.0] {
                    if ui
                        .selectable_label(
                            (self.skip_seconds - secs).abs() < 0.5,
                            format!("{secs:.0} s"),
                        )
                        .clicked()
                    {
                        self.skip_seconds = secs;
                        ui.close_menu();
                    }
                }
            });
            ui.separator();
            let mut deinterlace = self.controller.deinterlace();
            if ui.checkbox(&mut deinterlace, "Deinterlace").changed() {
                let _ = self.controller.set_deinterlace(deinterlace);
            }
            if ui.button("Screenshot").clicked() {
                self.execute_player_action(PlayerAction::Screenshot, ctx);
                ui.close_menu();
            }
        });

        ui.separator();

        if ui.button("Playlist").clicked() {
            self.show_playlist_drawer = !self.show_playlist_drawer;
            ui.close_menu();
        }
        if ui.button("Settings").clicked() {
            self.current_view = AppView::Settings;
            ui.close_menu();
        }
        if ui
            .button(if self.is_fullscreen {
                "Exit fullscreen"
            } else {
                "Fullscreen"
            })
            .clicked()
        {
            self.execute_player_action(PlayerAction::ToggleFullscreen, ctx);
            ui.close_menu();
        }
        if ui.button("More controls…").clicked() {
            self.player_more_overflow_open = true;
            ui.close_menu();
        }
    }

    /// Transient centred OSD for shortcut-driven changes (volume, speed, seek,
    /// delays, aspect, crop, zoom).
    pub(crate) fn render_osd(&mut self, ctx: &egui::Context) {
        let Some((text, shown_at)) = self.osd_notice.clone() else {
            return;
        };
        let elapsed = shown_at.elapsed();
        let lifetime = std::time::Duration::from_millis(2200);
        if elapsed >= lifetime {
            self.osd_notice = None;
            return;
        }
        let fade = lifetime.saturating_sub(elapsed).as_secs_f32() / 0.45;
        let alpha = fade.clamp(0.0, 1.0);

        egui::Area::new(egui::Id::new("player_osd"))
            .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -168.0])
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(Color32::from_black_alpha((200.0 * alpha) as u8))
                    .stroke(egui::Stroke::new(
                        1.0_f32,
                        BrandPalette::BORDER_SLATE.gamma_multiply(alpha),
                    ))
                    .corner_radius(8.0_f32)
                    .inner_margin(egui::Margin::symmetric(16, 8))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(text)
                                .size(13.0)
                                .strong()
                                .color(BrandPalette::PRIMARY_TEXT.gamma_multiply(alpha)),
                        );
                    });
            });

        ctx.request_repaint_after(std::time::Duration::from_millis(80));
    }
    /// Live playback statistics overlay.
    ///
    /// Every row is a real libmpv property sampled on the diagnostics worker
    /// (never on this thread), so opening the overlay cannot stall playback.
    pub(crate) fn render_playback_stats(&mut self, ctx: &egui::Context) {
        if !self.show_playback_stats {
            return;
        }
        if let Some(probe) = &self.diagnostics {
            probe.set_enabled(true);
        }
        let snap = self
            .diagnostics
            .as_ref()
            .map(|p| p.snapshot())
            .unwrap_or_default();

        let mut open = true;
        let mut export_clicked = false;
        let fmt_opt = |v: Option<f64>, unit: &str| match v {
            Some(x) if x.is_finite() => format!("{x:.2}{unit}"),
            _ => "—".to_string(),
        };
        let fmt_int = |v: Option<i64>| v.map(|x| x.to_string()).unwrap_or_else(|| "—".into());

        egui::Window::new("Playback Statistics")
            .open(&mut open)
            .resizable(true)
            .collapsible(false)
            .default_width(430.0)
            .anchor(egui::Align2::RIGHT_TOP, [-12.0, 100.0])
            .frame(
                egui::Frame::NONE
                    .fill(BrandPalette::ELEVATED_CARD)
                    .stroke(egui::Stroke::new(1.0_f32, BrandPalette::BORDER_SLATE))
                    .corner_radius(8.0_f32)
                    .inner_margin(egui::Margin::symmetric(12, 10)),
            )
            .show(ctx, |ui| {
                let verdict = snap.drop_verdict();
                let healthy = verdict.starts_with("Nominal");
                ui.label(
                    RichText::new(verdict)
                        .size(11.5)
                        .strong()
                        .color(if healthy {
                            BrandPalette::ELECTRIC_LIME
                        } else {
                            Color32::from_rgb(250, 180, 80)
                        }),
                );
                ui.add_space(4.0);

                egui::ScrollArea::vertical()
                    .max_height(420.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        let grid = |ui: &mut egui::Ui| {
                            egui::Grid::new("playback_stats_grid")
                                .num_columns(2)
                                .spacing([14.0, 2.0])
                                .show(ui, |ui| {
                                    let header = |ui: &mut egui::Ui, text: &str| {
                                        ui.label(
                                            RichText::new(text)
                                                .size(10.0)
                                                .strong()
                                                .color(BrandPalette::ELECTRIC_LIME),
                                        );
                                        ui.end_row();
                                    };
                                    let row = |ui: &mut egui::Ui, k: &str, v: String| {
                                        ui.label(
                                            RichText::new(k)
                                                .size(10.5)
                                                .color(BrandPalette::MUTED_TEXT),
                                        );
                                        ui.label(
                                            RichText::new(v)
                                                .monospace()
                                                .size(10.5)
                                                .color(BrandPalette::PRIMARY_TEXT),
                                        );
                                        ui.end_row();
                                    };

                                    header(ui, "VIDEO");
                                    row(
                                        ui,
                                        "Codec",
                                        format!("{} {}", snap.video_codec, snap.codec_profile),
                                    );
                                    row(
                                        ui,
                                        "Resolution",
                                        format!("{}×{}", snap.width, snap.height),
                                    );
                                    row(ui, "Bit depth", fmt_int(snap.bit_depth));
                                    row(ui, "Pixel format", snap.pixel_format.clone());
                                    row(ui, "Output format", snap.output_pixel_format.clone());
                                    row(
                                        ui,
                                        "Colour",
                                        format!(
                                            "{} / {}",
                                            snap.colour_gamma, snap.colour_primaries
                                        ),
                                    );
                                    row(ui, "HDR", snap.hdr.clone());
                                    row(ui, "Source FPS", fmt_opt(snap.container_fps, ""));
                                    row(ui, "Estimated FPS", fmt_opt(snap.estimated_fps, ""));
                                    row(ui, "Display FPS", fmt_opt(snap.display_fps, ""));
                                    row(ui, "Video output", snap.current_vo.clone());
                                    row(
                                        ui,
                                        "HW decoder",
                                        format!(
                                            "{} (requested {})",
                                            snap.hwdec_current, snap.hwdec_requested
                                        ),
                                    );
                                    row(
                                        ui,
                                        "Filters",
                                        if snap.video_filters.is_empty() {
                                            "none".into()
                                        } else {
                                            snap.video_filters.clone()
                                        },
                                    );

                                    header(ui, "FRAME PACING");
                                    row(
                                        ui,
                                        "VO drops",
                                        format!("{} ({:.1}/s)", snap.vo_drops, snap.vo_drop_rate),
                                    );
                                    row(
                                        ui,
                                        "Decoder drops",
                                        format!(
                                            "{} ({:.1}/s)",
                                            snap.decoder_drops, snap.decoder_drop_rate
                                        ),
                                    );
                                    row(ui, "Delayed frames", snap.delayed_frames.to_string());
                                    row(ui, "Mistimed frames", fmt_int(snap.mistimed_frames));
                                    row(
                                        ui,
                                        "Display sync",
                                        match snap.display_sync {
                                            Some(true) => "active".into(),
                                            Some(false) => "inactive".into(),
                                            None => "—".into(),
                                        },
                                    );
                                    row(ui, "VSync ratio", fmt_opt(snap.vsync_ratio, ""));
                                    row(ui, "A/V sync", fmt_opt(snap.avsync, " s"));
                                    row(
                                        ui,
                                        "Total sync change",
                                        fmt_opt(snap.total_avsync_change, " s"),
                                    );

                                    header(ui, "AUDIO");
                                    row(ui, "Codec", snap.audio_codec.clone());
                                    row(ui, "Sample rate", fmt_int(snap.sample_rate));
                                    row(ui, "Channels", fmt_int(snap.channels));
                                    row(ui, "Output", snap.current_ao.clone());
                                    row(ui, "Device", snap.audio_device.clone());
                                    row(ui, "Audio delay", fmt_opt(snap.audio_delay, " s"));
                                    row(ui, "Subtitle delay", fmt_opt(snap.sub_delay, " s"));

                                    header(ui, "DELIVERY & SYSTEM");
                                    row(ui, "Demuxer cache", fmt_opt(snap.cache_duration, " s"));
                                    row(ui, "Cache speed", fmt_opt(snap.cache_speed, " B/s"));
                                    row(ui, "Video bitrate", fmt_opt(snap.video_bitrate, " bit/s"));
                                    row(
                                        ui,
                                        "Playback speed",
                                        format!("{:.2}×", self.controller.speed()),
                                    );
                                    row(ui, "CPU", fmt_opt(snap.cpu_percent, " %"));
                                    row(ui, "Memory", fmt_opt(snap.rss_mib, " MiB"));
                                    row(ui, "Samples", snap.sample_count.to_string());
                                });
                        };
                        grid(ui);
                    });

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui
                        .button("Export report")
                        .on_hover_text(
                            "Writes JSON + text diagnostics to the MUVRIX config directory",
                        )
                        .clicked()
                    {
                        export_clicked = true;
                    }
                    ui.label(
                        RichText::new("Ctrl+I toggles · Ctrl+Shift+I exports")
                            .size(9.5)
                            .color(BrandPalette::MUTED_TEXT),
                    );
                });
            });

        if export_clicked {
            self.execute_player_action(PlayerAction::ExportDiagnostics, ctx);
        }
        if !open {
            self.show_playback_stats = false;
        }
        if let Some(probe) = &self.diagnostics {
            probe.set_enabled(self.show_playback_stats);
        }
        // Keep the snapshot fresh while visible without repainting otherwise.
        if self.show_playback_stats {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
    }

    /// Dedicated A/V and subtitle synchronisation panel.
    ///
    /// Defaults are always zero: manual delay is a correction for genuinely
    /// mistimed media, never a way to hide a broken clock.
    pub(crate) fn render_sync_panel(&mut self, ctx: &egui::Context) {
        if !self.sync_panel_open {
            return;
        }
        let mut open = true;
        let step = self.sync_adjust_ms / 1000.0;
        let audio = self.controller.audio_delay().unwrap_or(0.0);
        let sub = self.controller.sub_delay();
        let mut apply_audio: Option<f64> = None;
        let mut apply_sub: Option<f64> = None;

        egui::Window::new("A/V Synchronisation")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::RIGHT_BOTTOM, [-16.0, -128.0])
            .frame(
                egui::Frame::NONE
                    .fill(BrandPalette::ELEVATED_CARD)
                    .stroke(egui::Stroke::new(1.0_f32, BrandPalette::BORDER_SLATE))
                    .corner_radius(8.0_f32)
                    .inner_margin(egui::Margin::symmetric(14, 10)),
            )
            .show(ctx, |ui| {
                ui.set_width(320.0);
                ui.horizontal(|ui| {
                    icons::icon_label(ui, Icon::Sliders, 14.0, BrandPalette::ELECTRIC_LIME);
                    ui.label(
                        RichText::new("Adjustment increment")
                            .size(11.0)
                            .color(BrandPalette::MUTED_TEXT),
                    );
                    for ms in [10.0_f64, 50.0, 100.0] {
                        ui.selectable_value(
                            &mut self.sync_adjust_ms,
                            ms,
                            RichText::new(format!("{ms:.0} ms")).size(10.5),
                        );
                    }
                });

                ui.add_space(6.0);
                ui.label(
                    RichText::new("AUDIO / VIDEO")
                        .size(10.0)
                        .strong()
                        .color(BrandPalette::ELECTRIC_LIME),
                );
                ui.horizontal(|ui| {
                    if ui
                        .button(format!("−{:.0} ms", self.sync_adjust_ms))
                        .clicked()
                    {
                        apply_audio = Some(audio - step);
                    }
                    let mut ms = audio * 1000.0;
                    let resp = ui.add(
                        egui::DragValue::new(&mut ms)
                            .speed(5.0)
                            .range(-10_000.0..=10_000.0)
                            .suffix(" ms"),
                    );
                    if resp.changed() {
                        apply_audio = Some(ms / 1000.0);
                    }
                    if ui
                        .button(format!("+{:.0} ms", self.sync_adjust_ms))
                        .clicked()
                    {
                        apply_audio = Some(audio + step);
                    }
                    if ui.button("Reset").clicked() {
                        apply_audio = Some(0.0);
                    }
                });
                ui.label(
                    RichText::new(format!(
                        "Audio delay: {:+.0} ms  ({})",
                        audio * 1000.0,
                        if audio.abs() < 0.0005 {
                            "in sync"
                        } else if audio > 0.0 {
                            "audio delayed"
                        } else {
                            "audio advanced"
                        }
                    ))
                    .monospace()
                    .size(10.5)
                    .color(BrandPalette::PRIMARY_TEXT),
                );

                ui.add_space(8.0);
                ui.label(
                    RichText::new("AUDIO / SUBTITLE")
                        .size(10.0)
                        .strong()
                        .color(BrandPalette::ELECTRIC_LIME),
                );
                ui.horizontal(|ui| {
                    if ui
                        .button(format!("−{:.0} ms", self.sync_adjust_ms))
                        .clicked()
                    {
                        apply_sub = Some(sub - step);
                    }
                    let mut ms = sub * 1000.0;
                    let resp = ui.add(
                        egui::DragValue::new(&mut ms)
                            .speed(5.0)
                            .range(-10_000.0..=10_000.0)
                            .suffix(" ms"),
                    );
                    if resp.changed() {
                        apply_sub = Some(ms / 1000.0);
                    }
                    if ui
                        .button(format!("+{:.0} ms", self.sync_adjust_ms))
                        .clicked()
                    {
                        apply_sub = Some(sub + step);
                    }
                    if ui.button("Reset").clicked() {
                        apply_sub = Some(0.0);
                    }
                });
                ui.label(
                    RichText::new(format!("Subtitle delay: {:+.0} ms", sub * 1000.0))
                        .monospace()
                        .size(10.5)
                        .color(BrandPalette::PRIMARY_TEXT),
                );

                ui.add_space(6.0);
                ui.label(
                    RichText::new("K / J adjust audio · H / G adjust subtitles")
                        .size(9.5)
                        .color(BrandPalette::MUTED_TEXT),
                );
            });

        if let Some(value) = apply_audio {
            let _ = self.controller.set_audio_delay(value);
            self.push_osd(format!("Audio delay: {:+.0} ms", value * 1000.0));
        }
        if let Some(value) = apply_sub {
            let _ = self.controller.set_subtitle_delay(value);
            self.push_osd(format!("Subtitle delay: {:+.0} ms", value * 1000.0));
        }
        self.sync_panel_open = open;
    }
}
