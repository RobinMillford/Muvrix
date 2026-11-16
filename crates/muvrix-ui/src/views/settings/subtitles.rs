use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use muvrix_player::{AssOverrideMode, SubtitleJustify, SubtitleStyling, TrackInfo, TrackType};

use crate::app::{format_track_label, MuvrixApp};

const STANDARD_FONTS: &[&str] = &[
    "sans-serif",
    "serif",
    "monospace",
    "Liberation Sans",
    "DejaVu Sans",
    "Arial",
    "Roboto",
    "Ubuntu",
    "Cantarell",
];

/// Parses hex color strings into `egui::Color32`.
/// Supports `#RRGGBB` (6 chars) and `#AARRGGBB` / `#RRGGBBAA` (8 chars).
fn parse_hex_color(hex: &str, default: Color32) -> Color32 {
    let s = hex.trim().trim_start_matches('#');
    match s.len() {
        6 => {
            if let (Ok(r), Ok(g), Ok(b)) = (
                u8::from_str_radix(&s[0..2], 16),
                u8::from_str_radix(&s[2..4], 16),
                u8::from_str_radix(&s[4..6], 16),
            ) {
                Color32::from_rgb(r, g, b)
            } else {
                default
            }
        }
        8 => {
            // libmpv standard format: #AARRGGBB
            if let (Ok(a), Ok(r), Ok(g), Ok(b)) = (
                u8::from_str_radix(&s[0..2], 16),
                u8::from_str_radix(&s[2..4], 16),
                u8::from_str_radix(&s[4..6], 16),
                u8::from_str_radix(&s[6..8], 16),
            ) {
                Color32::from_rgba_unmultiplied(r, g, b, a)
            } else {
                default
            }
        }
        _ => default,
    }
}

impl MuvrixApp {
    pub(crate) fn render_settings_subtitles(&mut self, ui: &mut egui::Ui) {
        let prev_styling = self.active_styling.clone();

        // Section 1: Stream Selection (Dual Subtitles)
        ui.group(|ui| {
            ui.label(
                RichText::new("1. Advanced Subtitle Stream Selection")
                    .strong()
                    .size(14.0)
                    .color(Color32::from_rgb(95, 185, 255)),
            );
            ui.label(
                RichText::new(
                    "Muvrix supports upstream libmpv dual-subtitle rendering (secondary-sid). Select independent primary and secondary subtitle tracks for bilingual media consumption.",
                )
                .size(12.0)
                .color(Color32::from_gray(160)),
            );

            ui.add_space(8.0);

            let sub_tracks: Vec<TrackInfo> = self
                .controller
                .tracks_by_type(TrackType::Subtitle)
                .into_iter()
                .cloned()
                .collect();

            let selected_primary = self
                .controller
                .selected_track(TrackType::Subtitle)
                .map(|t| t.id);
            let selected_secondary = self.controller.secondary_sub_track_id();

            ui.horizontal(|ui| {
                ui.label("Primary Subtitle Track (Bottom):");
                let primary_label = if let Some(id) = selected_primary {
                    sub_tracks
                        .iter()
                        .find(|t| t.id == id)
                        .map(format_track_label)
                        .unwrap_or_else(|| "Track active".to_string())
                } else {
                    "Off (Disabled)".to_string()
                };

                egui::ComboBox::from_id_salt("primary_sub_setting_combo")
                    .selected_text(primary_label)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(selected_primary.is_none(), "Off (Disabled)")
                            .clicked()
                        {
                            let _ = self.controller.set_subtitle_track(None);
                        }
                        for t in &sub_tracks {
                            let label = format_track_label(t);
                            let is_sel = selected_primary == Some(t.id);
                            if ui.selectable_label(is_sel, label).clicked() {
                                let _ = self.controller.set_subtitle_track(Some(t.id));
                            }
                        }
                    });
            });

            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("Secondary Subtitle Track (Top):   ");
                let sec_label = if let Some(id) = selected_secondary {
                    sub_tracks
                        .iter()
                        .find(|t| t.id == id)
                        .map(format_track_label)
                        .unwrap_or_else(|| "Track active".to_string())
                } else {
                    "Off (Disabled)".to_string()
                };

                egui::ComboBox::from_id_salt("secondary_sub_setting_combo")
                    .selected_text(sec_label)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(selected_secondary.is_none(), "Off (Disabled)")
                            .clicked()
                        {
                            let _ = self.controller.set_secondary_sub_track(None);
                        }
                        for t in &sub_tracks {
                            let label = format_track_label(t);
                            let is_sel = selected_secondary == Some(t.id);
                            if ui.selectable_label(is_sel, label).clicked() {
                                let _ = self.controller.set_secondary_sub_track(Some(t.id));
                            }
                        }
                    });
            });

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui
                    .button(RichText::new("⚡ Enforce Collision Avoidance").strong())
                    .on_hover_text(
                        "Pins primary subtitles at bottom (pos=100) and secondary subtitles at top (pos=10)",
                    )
                    .clicked()
                {
                    let _ = self.controller.configure_dual_subtitle_collision_avoidance();
                    self.status_message =
                        "Enforced dual-subtitle collision avoidance (Primary: Bottom, Secondary: Top)"
                            .to_string();
                }
            });
        });

        ui.add_space(10.0);

        // Section 2: Subtitle Studio (Live Preview & Styling)
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("2. Professional Subtitle Studio")
                        .strong()
                        .size(15.0)
                        .color(Color32::from_rgb(95, 185, 255)),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new("● Live MPV Engine Sync Active")
                            .size(11.0)
                            .color(Color32::from_rgb(52, 211, 153)),
                    );
                });
            });

            ui.label(
                RichText::new(
                    "Interactive real-time subtitle typography studio. Adjustments synchronize immediately with the active libmpv engine instance and persist across sessions.",
                )
                .size(12.0)
                .color(Color32::from_gray(160)),
            );

            ui.add_space(8.0);

            // --- LIVE VISUAL PREVIEW CANVAS ---
            render_subtitle_live_preview_box(
                ui,
                &self.active_styling,
                self.controller.secondary_sub_track_id(),
            );

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(6.0);

            // Font Family Selection
            ui.horizontal(|ui| {
                ui.label("Font Family:    ");

                let current_font = self.active_styling.font.trim();
                let display_font = if current_font.is_empty() {
                    "sans-serif (Default)"
                } else {
                    current_font
                };

                egui::ComboBox::from_id_salt("sub_font_family_combo")
                    .selected_text(display_font)
                    .width(160.0)
                    .show_ui(ui, |ui| {
                        for font in STANDARD_FONTS {
                            if ui
                                .selectable_label(self.active_styling.font == *font, *font)
                                .clicked()
                            {
                                self.active_styling.font = (*font).to_string();
                            }
                        }
                    });

                ui.add(
                    egui::TextEdit::singleline(&mut self.active_styling.font)
                        .hint_text("Custom font name...")
                        .desired_width(130.0),
                );
            });

            ui.add_space(4.0);

            // Font Size Slider
            ui.horizontal(|ui| {
                ui.label("Font Size:      ");
                ui.add(
                    egui::Slider::new(&mut self.active_styling.font_size, 20..=90).text("pt"),
                );
            });

            ui.add_space(4.0);

            // Font Color
            ui.horizontal(|ui| {
                ui.label("Font Color:     ");
                ui.add(
                    egui::TextEdit::singleline(&mut self.active_styling.color).desired_width(85.0),
                );
                if ui.button("White").clicked() {
                    self.active_styling.color = "#FFFFFF".to_string();
                }
                if ui.button("Yellow").clicked() {
                    self.active_styling.color = "#FFFF00".to_string();
                }
                if ui.button("Amber").clicked() {
                    self.active_styling.color = "#F59E0B".to_string();
                }
                if ui.button("Cyan").clicked() {
                    self.active_styling.color = "#00FFFF".to_string();
                }
                if ui.button("Green").clicked() {
                    self.active_styling.color = "#22C55E".to_string();
                }
            });

            ui.add_space(4.0);

            // Outline Border Size & Color
            ui.horizontal(|ui| {
                ui.label("Border Size:    ");
                ui.add(
                    egui::Slider::new(&mut self.active_styling.border_size, 0.0..=10.0).text("pt"),
                );
            });

            ui.horizontal(|ui| {
                ui.label("Border Color:   ");
                ui.add(
                    egui::TextEdit::singleline(&mut self.active_styling.border_color)
                        .desired_width(85.0),
                );
                if ui.button("Black").clicked() {
                    self.active_styling.border_color = "#000000".to_string();
                }
                if ui.button("Gray").clicked() {
                    self.active_styling.border_color = "#1E293B".to_string();
                }
                if ui.button("Transparent").clicked() {
                    self.active_styling.border_color = "#00000000".to_string();
                }
            });

            ui.add_space(4.0);

            // Shadow Offset & Color
            ui.horizontal(|ui| {
                ui.label("Shadow Size:    ");
                ui.add(
                    egui::Slider::new(&mut self.active_styling.sub_shadow_offset, 0.0..=10.0)
                        .text("pt"),
                );
            });

            ui.horizontal(|ui| {
                ui.label("Shadow Color:   ");
                ui.add(
                    egui::TextEdit::singleline(&mut self.active_styling.shadow_color)
                        .desired_width(85.0),
                );
                if ui.button("Semi-Black").clicked() {
                    self.active_styling.shadow_color = "#80000000".to_string();
                }
                if ui.button("Solid Black").clicked() {
                    self.active_styling.shadow_color = "#000000".to_string();
                }
                if ui.button("Cyan Glow").clicked() {
                    self.active_styling.shadow_color = "#8000FFFF".to_string();
                }
            });

            ui.add_space(4.0);

            // Subtitle Margins (X and Y)
            ui.horizontal(|ui| {
                ui.label("Vertical Margin:  ");
                ui.add(
                    egui::Slider::new(&mut self.active_styling.margin_y, 0..=120)
                        .text("px (sub-margin-y)"),
                );
            });

            ui.horizontal(|ui| {
                ui.label("Horizontal Margin:");
                ui.add(
                    egui::Slider::new(&mut self.active_styling.margin_x, 0..=120)
                        .text("px (sub-margin-x)"),
                );
            });

            ui.add_space(4.0);

            // Placement Positioning (Primary & Secondary)
            ui.horizontal(|ui| {
                ui.label("Primary Sub Pos:  ");
                ui.add(
                    egui::Slider::new(&mut self.active_styling.pos, 0..=100)
                        .text("% (0=top, 100=bottom)"),
                );
            });

            ui.horizontal(|ui| {
                ui.label("Secondary Sub Pos:");
                ui.add(
                    egui::Slider::new(&mut self.active_styling.secondary_pos, 0..=100)
                        .text("% (0=top, 100=bottom)"),
                );
            });

            ui.horizontal(|ui| {
                ui.label("Scale Multiplier: ");
                ui.add(
                    egui::Slider::new(&mut self.active_styling.scale, 0.5..=2.5).text("x scale"),
                );
            });

            ui.add_space(4.0);

            // Justification & Background Color
            ui.horizontal(|ui| {
                ui.label("Justify:        ");
                egui::ComboBox::from_id_salt("sub_justify_combo_studio")
                    .selected_text(match self.active_styling.sub_justify {
                        SubtitleJustify::Auto => "Auto",
                        SubtitleJustify::Left => "Left",
                        SubtitleJustify::Center => "Center",
                        SubtitleJustify::Right => "Right",
                    })
                    .show_ui(ui, |ui| {
                        for justify in [
                            SubtitleJustify::Auto,
                            SubtitleJustify::Left,
                            SubtitleJustify::Center,
                            SubtitleJustify::Right,
                        ] {
                            let label = match justify {
                                SubtitleJustify::Auto => "Auto",
                                SubtitleJustify::Left => "Left",
                                SubtitleJustify::Center => "Center",
                                SubtitleJustify::Right => "Right",
                            };
                            if ui
                                .selectable_label(self.active_styling.sub_justify == justify, label)
                                .clicked()
                            {
                                self.active_styling.sub_justify = justify;
                            }
                        }
                    });

                ui.add_space(12.0);
                ui.label("Box Background:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.active_styling.sub_back_color)
                        .desired_width(85.0),
                );
                if ui.button("None").clicked() {
                    self.active_styling.sub_back_color = "#00000000".to_string();
                }
                if ui.button("Semi-Dark").clicked() {
                    self.active_styling.sub_back_color = "#80000000".to_string();
                }
            });

            ui.add_space(4.0);

            // SSA/ASS Override Policy
            ui.horizontal(|ui| {
                ui.label("SSA/ASS Override: ");
                let mode_label = match self.active_styling.ass_override_mode {
                    AssOverrideMode::Preserve => "Preserve Author Styles (Default)",
                    AssOverrideMode::SafeScale => "Safe Font Scaling Only",
                    AssOverrideMode::Force => "Force User Styling Overrides",
                    AssOverrideMode::Strip => "Strip SSA/ASS to Plain Text",
                };
                egui::ComboBox::from_id_salt("ass_override_mode_studio_combo")
                    .selected_text(mode_label)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(
                                self.active_styling.ass_override_mode == AssOverrideMode::Preserve,
                                "Preserve Author Styles (Default)",
                            )
                            .clicked()
                        {
                            self.active_styling.ass_override_mode = AssOverrideMode::Preserve;
                        }
                        if ui
                            .selectable_label(
                                self.active_styling.ass_override_mode == AssOverrideMode::SafeScale,
                                "Safe Font Scaling Only",
                            )
                            .clicked()
                        {
                            self.active_styling.ass_override_mode = AssOverrideMode::SafeScale;
                        }
                        if ui
                            .selectable_label(
                                self.active_styling.ass_override_mode == AssOverrideMode::Force,
                                "Force User Styling Overrides",
                            )
                            .clicked()
                        {
                            self.active_styling.ass_override_mode = AssOverrideMode::Force;
                        }
                        if ui
                            .selectable_label(
                                self.active_styling.ass_override_mode == AssOverrideMode::Strip,
                                "Strip SSA/ASS to Plain Text",
                            )
                            .clicked()
                        {
                            self.active_styling.ass_override_mode = AssOverrideMode::Strip;
                        }
                    });
            });

            ui.add_space(4.0);

            // Subtitle Playback Speed
            ui.horizontal(|ui| {
                ui.label("Subtitle Speed: ");
                ui.add(
                    egui::Slider::new(&mut self.active_styling.sub_speed, 0.5..=2.0)
                        .text("x playback rate"),
                );
            });

            ui.add_space(4.0);

            // Custom Fonts Folder
            ui.horizontal(|ui| {
                ui.label("Custom Fonts Folder:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.active_styling.sub_fonts_dir)
                        .desired_width(220.0)
                        .hint_text("Directory containing .ttf/.otf fonts"),
                );
                if ui.button("Choose...").clicked() {
                    if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                        self.active_styling.sub_fonts_dir = folder.to_string_lossy().to_string();
                    }
                }
                if ui.button("Clear").clicked() {
                    self.active_styling.sub_fonts_dir.clear();
                }
            });

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);

            // Control Actions
            ui.horizontal(|ui| {
                if ui
                    .button(RichText::new("⚡ Apply Styling to Active Engine").strong())
                    .clicked()
                {
                    let styling = self.active_styling.clone();
                    let _ = self.controller.set_subtitle_styling(styling);
                    self.save_media_settings();
                    self.status_message = "Subtitle styling applied to engine".to_string();
                }

                if ui.button("🔄 Reset to Engine Defaults").clicked() {
                    let _ = self.controller.reset_subtitle_styling();
                    self.active_styling = SubtitleStyling::default();
                    self.save_media_settings();
                    self.status_message = "Subtitle styling reset to defaults".to_string();
                }
            });
        });

        // Immediate live synchronization across active playback and persistence
        if self.active_styling != prev_styling {
            let _ = self
                .controller
                .set_subtitle_styling(self.active_styling.clone());
            self.save_media_settings();
            self.status_message = "Subtitle styling synced live to playback engine".to_string();
        }
    }
}

/// Renders an interactive live visual preview box of subtitle styling inside a simulated
/// 16:9 video frame canvas.
fn render_subtitle_live_preview_box(
    ui: &mut egui::Ui,
    styling: &SubtitleStyling,
    secondary_track: Option<i64>,
) {
    let desired_width = ui.available_width().max(320.0);
    let desired_height = 160.0;
    let (rect, _response) =
        ui.allocate_exact_size(Vec2::new(desired_width, desired_height), Sense::hover());
    let painter = ui.painter_at(rect);

    // 1. Dark letterboxed video frame backdrop
    painter.rect_filled(rect, 6.0_f32, Color32::from_rgb(11, 15, 25));
    painter.rect_stroke(
        rect,
        6.0_f32,
        Stroke::new(1.0_f32, Color32::from_rgb(30, 41, 59)),
        egui::StrokeKind::Inside,
    );

    // Subtle video grid scanlines simulation (gives realistic monitor context)
    for y_offset in (10..desired_height as i32).step_by(24) {
        let line_y = rect.min.y + y_offset as f32;
        painter.line_segment(
            [
                Pos2::new(rect.min.x + 8.0, line_y),
                Pos2::new(rect.max.x - 8.0, line_y),
            ],
            Stroke::new(0.5_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 4)),
        );
    }

    // Header badge
    painter.text(
        Pos2::new(rect.min.x + 12.0, rect.min.y + 12.0),
        egui::Align2::LEFT_TOP,
        "PREVIEW MONITOR • 16:9 CINEMATIC SIMULATION",
        egui::FontId::monospace(9.5),
        Color32::from_gray(120),
    );

    // Layout boundaries respecting margin-x and margin-y
    let pad_x = (styling.margin_x as f32 * 0.45).clamp(8.0, 45.0);
    let pad_y = (styling.margin_y as f32 * 0.45).clamp(8.0, 45.0);
    let top_y = rect.min.y + 24.0 + pad_y;
    let bottom_y = rect.max.y - pad_y - 12.0;

    // 2. Render Secondary Subtitle (towards top if active or configured)
    if secondary_track.is_some() || styling.secondary_pos <= 40 {
        let sec_pos_norm = (styling.secondary_pos as f32 / 100.0).clamp(0.0, 1.0);
        let sec_y = top_y + (bottom_y - top_y) * sec_pos_norm;
        let sec_text = "[Secondary Track: English (SDH) / Translation Stream]";
        painter.text(
            Pos2::new(rect.center().x, sec_y),
            egui::Align2::CENTER_CENTER,
            sec_text,
            egui::FontId::proportional(11.5),
            Color32::from_rgb(203, 213, 225),
        );
    }

    // 3. Render Primary Subtitle
    let prim_pos_norm = (styling.pos as f32 / 100.0).clamp(0.0, 1.0);
    let prim_y = top_y + (bottom_y - top_y) * prim_pos_norm;

    let prim_text = "MUVRIX Subtitle Studio — Live Typography Preview";
    let font_size = ((styling.font_size as f32 * 0.35) * styling.scale as f32).clamp(11.0, 30.0);

    let text_color = parse_hex_color(&styling.color, Color32::WHITE);
    let border_color = parse_hex_color(&styling.border_color, Color32::BLACK);
    let shadow_color = parse_hex_color(
        &styling.shadow_color,
        Color32::from_rgba_unmultiplied(0, 0, 0, 160),
    );
    let back_color = parse_hex_color(&styling.sub_back_color, Color32::TRANSPARENT);

    let (align, prim_x) = match styling.sub_justify {
        SubtitleJustify::Left => (egui::Align2::LEFT_CENTER, rect.min.x + pad_x),
        SubtitleJustify::Right => (egui::Align2::RIGHT_CENTER, rect.max.x - pad_x),
        SubtitleJustify::Center | SubtitleJustify::Auto => {
            (egui::Align2::CENTER_CENTER, rect.center().x)
        }
    };

    // Subtitle Background Box (if configured with non-zero alpha)
    if back_color.a() > 0 {
        let est_w = prim_text.len() as f32 * (font_size * 0.55);
        let box_min_x = match styling.sub_justify {
            SubtitleJustify::Left => prim_x - 6.0,
            SubtitleJustify::Right => prim_x - est_w - 6.0,
            SubtitleJustify::Center | SubtitleJustify::Auto => prim_x - (est_w / 2.0) - 6.0,
        };
        let box_rect = Rect::from_min_size(
            Pos2::new(box_min_x, prim_y - font_size * 0.6),
            Vec2::new(est_w + 12.0, font_size * 1.3),
        );
        painter.rect_filled(box_rect, 4.0, back_color);
    }

    // Shadow multi-pass rendering
    if styling.sub_shadow_offset > 0.0 && shadow_color.a() > 0 {
        let off = (styling.sub_shadow_offset as f32 * 0.45).clamp(0.5, 4.0);
        painter.text(
            Pos2::new(prim_x + off, prim_y + off),
            align,
            prim_text,
            egui::FontId::proportional(font_size),
            shadow_color,
        );
    }

    // Border multi-pass outline simulation
    if styling.border_size > 0.0 && border_color.a() > 0 {
        let b = (styling.border_size as f32 * 0.35).clamp(0.5, 3.5);
        for (dx, dy) in [
            (-b, 0.0),
            (b, 0.0),
            (0.0, -b),
            (0.0, b),
            (-b * 0.7, -b * 0.7),
            (b * 0.7, -b * 0.7),
            (-b * 0.7, b * 0.7),
            (b * 0.7, b * 0.7),
        ] {
            painter.text(
                Pos2::new(prim_x + dx, prim_y + dy),
                align,
                prim_text,
                egui::FontId::proportional(font_size),
                border_color,
            );
        }
    }

    // Core subtitle text fill
    painter.text(
        Pos2::new(prim_x, prim_y),
        align,
        prim_text,
        egui::FontId::proportional(font_size),
        text_color,
    );
}
