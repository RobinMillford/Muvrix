use eframe::egui::{self, Color32, RichText};
use muvrix_player::{AssOverrideMode, SubtitleJustify, SubtitleStyling, TrackInfo, TrackType};

use crate::app::{format_track_label, MuvrixApp};

impl MuvrixApp {
    pub(crate) fn render_settings_subtitles(&mut self, ui: &mut egui::Ui) {
        // Section 1: Dual Subtitles (Primary & Secondary)
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
                                    if ui.selectable_label(selected_primary.is_none(), "Off (Disabled)").clicked() {
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
                                    if ui.selectable_label(selected_secondary.is_none(), "Off (Disabled)").clicked() {
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
                                .on_hover_text("Pins primary subtitles at bottom (pos=100) and secondary subtitles at top (pos=10)")
                                .clicked()
                            {
                                let _ = self.controller.configure_dual_subtitle_collision_avoidance();
                                self.status_message = "Enforced dual-subtitle collision avoidance (Primary: Bottom, Secondary: Top)".to_string();
                            }
                        });
                    });

        ui.add_space(10.0);

        // Section 2: Subtitle Appearance Styling
        ui.group(|ui| {
                        ui.label(
                            RichText::new("2. Subtitle Appearance & Typography Styling")
                                .strong()
                                .size(14.0)
                                .color(Color32::from_rgb(95, 185, 255)),
                        );
                        ui.label(
                            RichText::new(
                                "Configure font typography, scale, border outlines, and position. Settings are applied directly to the libmpv rendering core.",
                            )
                            .size(12.0)
                            .color(Color32::from_gray(160)),
                        );

                        ui.add_space(8.0);

                        // Font Family
                        ui.horizontal(|ui| {
                            ui.label("Font Family:    ");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.active_styling.font)
                                    .hint_text("sans-serif, Liberation Sans, Arial...")
                                    .desired_width(180.0),
                            );
                            if ui.button("Sans").clicked() {
                                self.active_styling.font = "sans-serif".to_string();
                            }
                            if ui.button("Serif").clicked() {
                                self.active_styling.font = "serif".to_string();
                            }
                            if ui.button("Monospace").clicked() {
                                self.active_styling.font = "monospace".to_string();
                            }
                        });

                        ui.add_space(4.0);

                        // Font Size Slider
                        ui.horizontal(|ui| {
                            ui.label("Font Size:      ");
                            ui.add(
                                egui::Slider::new(&mut self.active_styling.font_size, 20..=90)
                                    .text("pt"),
                            );
                        });

                        ui.add_space(4.0);

                        // Colors
                        ui.horizontal(|ui| {
                            ui.label("Font Color:     ");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.active_styling.color)
                                    .desired_width(80.0),
                            );
                            if ui.button("White").clicked() {
                                self.active_styling.color = "#FFFFFF".to_string();
                            }
                            if ui.button("Yellow").clicked() {
                                self.active_styling.color = "#FFFF00".to_string();
                            }
                            if ui.button("Cyan").clicked() {
                                self.active_styling.color = "#00FFFF".to_string();
                            }
                            if ui.button("Green").clicked() {
                                self.active_styling.color = "#00FF66".to_string();
                            }
                        });

                        ui.add_space(4.0);

                        // Outline Border
                        ui.horizontal(|ui| {
                            ui.label("Border Color:   ");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.active_styling.border_color)
                                    .desired_width(80.0),
                            );
                            if ui.button("Black").clicked() {
                                self.active_styling.border_color = "#000000".to_string();
                            }
                            if ui.button("Gray").clicked() {
                                self.active_styling.border_color = "#222222".to_string();
                            }
                            if ui.button("None").clicked() {
                                self.active_styling.border_color = "#00000000".to_string();
                            }
                        });

                        ui.add_space(4.0);

                        ui.horizontal(|ui| {
                            ui.label("Border Size:    ");
                            ui.add(
                                egui::Slider::new(&mut self.active_styling.border_size, 0.0..=10.0)
                                    .text("pt"),
                            );
                        });

                        ui.add_space(4.0);

                        // Position & Scale
                        ui.horizontal(|ui| {
                            ui.label("Vertical Pos:   ");
                            ui.add(
                                egui::Slider::new(&mut self.active_styling.pos, 0..=100)
                                    .text("% (0=top, 100=bottom)"),
                            );
                        });

                        ui.add_space(4.0);

                        ui.horizontal(|ui| {
                            ui.label("Scale Factor:   ");
                            ui.add(
                                egui::Slider::new(&mut self.active_styling.scale, 0.5..=2.5)
                                    .text("x"),
                            );
                        });

                        ui.add_space(4.0);

                        ui.horizontal(|ui| {
                            ui.label("SSA/ASS Override:  ");
                            let mode_label = match self.active_styling.ass_override_mode {
                                AssOverrideMode::Preserve => "Preserve Author Styles (Default)",
                                AssOverrideMode::SafeScale => "Safe Font Scaling Only",
                                AssOverrideMode::Force => "Force User Styling Overrides",
                                AssOverrideMode::Strip => "Strip SSA/ASS to Plain Text",
                            };
                            egui::ComboBox::from_id_salt("ass_override_mode_combo")
                                .selected_text(mode_label)
                                .show_ui(ui, |ui| {
                                    if ui.selectable_label(self.active_styling.ass_override_mode == AssOverrideMode::Preserve, "Preserve Author Styles (Default)").clicked() {
                                        self.active_styling.ass_override_mode = AssOverrideMode::Preserve;
                                    }
                                    if ui.selectable_label(self.active_styling.ass_override_mode == AssOverrideMode::SafeScale, "Safe Font Scaling Only").clicked() {
                                        self.active_styling.ass_override_mode = AssOverrideMode::SafeScale;
                                    }
                                    if ui.selectable_label(self.active_styling.ass_override_mode == AssOverrideMode::Force, "Force User Styling Overrides").clicked() {
                                        self.active_styling.ass_override_mode = AssOverrideMode::Force;
                                    }
                                    if ui.selectable_label(self.active_styling.ass_override_mode == AssOverrideMode::Strip, "Strip SSA/ASS to Plain Text").clicked() {
                                        self.active_styling.ass_override_mode = AssOverrideMode::Strip;
                                    }
                                });
                        });

                        ui.add_space(4.0);

                        ui.horizontal(|ui| {
                            ui.label("Secondary Sub Pos: ");
                            ui.add(
                                egui::Slider::new(&mut self.active_styling.secondary_pos, 0..=100)
                                    .text("% (0=top, 100=bottom)"),
                            );
                        });

                        ui.add_space(4.0);
                        ui.separator();
                        ui.label(RichText::new("Advanced subtitle timing & rendering").strong());
                        ui.horizontal(|ui| {
                            ui.label("Subtitle Speed:");
                            ui.add(
                                egui::Slider::new(&mut self.active_styling.sub_speed, 0.5..=2.0)
                                    .text("x"),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label("Shadow Offset:");
                            ui.add(
                                egui::Slider::new(
                                    &mut self.active_styling.sub_shadow_offset,
                                    0.0..=10.0,
                                )
                                .text("pt"),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label("Background Color:");
                            ui.add(
                                egui::TextEdit::singleline(
                                    &mut self.active_styling.sub_back_color,
                                )
                                .desired_width(100.0),
                            );
                            ui.label("Justify:");
                            egui::ComboBox::from_id_salt("sub_justify_combo")
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
                                            .selectable_label(
                                                self.active_styling.sub_justify == justify,
                                                label,
                                            )
                                            .clicked()
                                        {
                                            self.active_styling.sub_justify = justify;
                                        }
                                    }
                                });
                        });
                        ui.horizontal(|ui| {
                            ui.label("Custom Fonts Folder:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.active_styling.sub_fonts_dir)
                                    .desired_width(240.0)
                                    .hint_text("Directory containing .ttf/.otf fonts"),
                            );
                            if ui.button("Choose...").clicked() {
                                if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                                    self.active_styling.sub_fonts_dir =
                                        folder.to_string_lossy().to_string();
                                }
                            }
                            if ui.button("Clear").clicked() {
                                self.active_styling.sub_fonts_dir.clear();
                            }
                        });

                        ui.add_space(2.0);
                        ui.label(
                            RichText::new("Note: Secondary subtitle positioning (pos=10) and ASS override policy are supported natively. Independent font/color customization for secondary subtitles is unsupported by libmpv.")
                                .italics()
                                .size(11.0)
                                .color(Color32::from_gray(140)),
                        );

                        ui.add_space(4.0);

                        ui.checkbox(
                            &mut self.active_styling.ass_override,
                            "Force override embedded ASS subtitle styles (Legacy mode)",
                        );

                        ui.add_space(8.0);

                        ui.horizontal(|ui| {
                            if ui
                                .button(RichText::new("Apply Styling to Engine").strong())
                                .clicked()
                            {
                                let styling = self.active_styling.clone();
                                let _ = self.controller.set_subtitle_styling(styling);
                                self.save_media_settings();
                                self.status_message = "Subtitle styling applied".to_string();
                            }

                            if ui.button("Reset to Engine Defaults").clicked() {
                                let _ = self.controller.reset_subtitle_styling();
                                self.active_styling = SubtitleStyling::default();
                                self.save_media_settings();
                                self.status_message = "Subtitle styling reset to defaults".to_string();
                            }
                        });
                    });
    }
}
