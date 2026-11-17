use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke};
use muvrix_player::{AccentColor, ThemeMode};

use crate::app::MuvrixApp;
use crate::branding::BrandPalette;
use crate::theme::CustomTheme;
use crate::ui_components;

impl MuvrixApp {
    pub(crate) fn render_settings_appearance(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.label(
                RichText::new("Theme & Accent")
                    .strong()
                    .size(14.0)
                    .color(Color32::from_rgb(95, 185, 255)),
            );
            ui.label(
                RichText::new("Persisted in settings.json and applied immediately.")
                    .size(11.5)
                    .color(Color32::from_gray(150)),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Theme:");
                egui::ComboBox::from_id_salt("theme_mode_combo")
                    .selected_text(match self.theme_mode {
                        ThemeMode::Obsidian => "Obsidian (Signature Brand)",
                        ThemeMode::Dark => "Dark",
                        ThemeMode::Light => "Light",
                        ThemeMode::Amoled => "AMOLED (pure black)",
                    })
                    .show_ui(ui, |ui| {
                        for mode in [
                            ThemeMode::Obsidian,
                            ThemeMode::Dark,
                            ThemeMode::Light,
                            ThemeMode::Amoled,
                        ] {
                            let label = match mode {
                                ThemeMode::Obsidian => "Obsidian (Signature Brand)",
                                ThemeMode::Dark => "Dark",
                                ThemeMode::Light => "Light",
                                ThemeMode::Amoled => "AMOLED (pure black)",
                            };
                            if ui
                                .selectable_label(self.theme_mode == mode, label)
                                .clicked()
                            {
                                self.theme_mode = mode;
                                self.save_media_settings();
                            }
                        }
                    });
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Accent:");
                egui::ComboBox::from_id_salt("accent_color_combo")
                    .selected_text(match self.accent_color {
                        AccentColor::Lime => "Lime (Electric Brand)",
                        other => match other {
                            AccentColor::Blue => "Blue",
                            AccentColor::Purple => "Purple",
                            AccentColor::Teal => "Teal",
                            AccentColor::Green => "Green",
                            AccentColor::Orange => "Orange",
                            AccentColor::Red => "Red",
                            _ => "Custom",
                        },
                    })
                    .show_ui(ui, |ui| {
                        for color in [
                            AccentColor::Lime,
                            AccentColor::Blue,
                            AccentColor::Purple,
                            AccentColor::Teal,
                            AccentColor::Green,
                            AccentColor::Orange,
                            AccentColor::Red,
                        ] {
                            let label = match color {
                                AccentColor::Lime => "Lime (Electric Brand)",
                                AccentColor::Blue => "Blue",
                                AccentColor::Purple => "Purple",
                                AccentColor::Teal => "Teal",
                                AccentColor::Green => "Green",
                                AccentColor::Orange => "Orange",
                                AccentColor::Red => "Red",
                            };
                            if ui
                                .selectable_label(self.accent_color == color, label)
                                .clicked()
                            {
                                self.accent_color = color;
                                self.save_media_settings();
                            }
                        }
                    });
            });
        });

        ui.add_space(8.0);

        // Custom UI Theme Builder & Palette Editor
        ui.group(|ui| {
            ui.label(
                RichText::new("Custom UI Theme Builder & Palette Editor")
                    .strong()
                    .size(15.0)
                    .color(BrandPalette::ELECTRIC_LIME),
            );
            ui.label(
                RichText::new(
                    "Design custom color palettes with live interactive preview, instant egui Visuals application, \
                     and JSON import/export.",
                )
                .size(11.5)
                .color(BrandPalette::SECONDARY_TEXT),
            );

            ui.add_space(6.0);

            // Theme Presets Bar
            ui.horizontal(|ui| {
                ui.label(RichText::new("Theme Presets:").strong());
                if ui.button("Obsidian Lime (Signature)").clicked() {
                    self.custom_theme = CustomTheme::obsidian_lime();
                    self.custom_theme_json = self.custom_theme.to_json().unwrap_or_default();
                    self.custom_theme.apply_to_context(ui.ctx());
                    let _ = self.custom_theme.save_to_disk();
                    self.theme_status_msg = Some(("Loaded Obsidian Lime preset".to_string(), false));
                }
                if ui.button("Cyberpunk Neon").clicked() {
                    self.custom_theme = CustomTheme::cyberpunk_neon();
                    self.custom_theme_json = self.custom_theme.to_json().unwrap_or_default();
                    self.custom_theme.apply_to_context(ui.ctx());
                    let _ = self.custom_theme.save_to_disk();
                    self.theme_status_msg = Some(("Loaded Cyberpunk Neon preset".to_string(), false));
                }
                if ui.button("Nordic Frost").clicked() {
                    self.custom_theme = CustomTheme::nordic_frost();
                    self.custom_theme_json = self.custom_theme.to_json().unwrap_or_default();
                    self.custom_theme.apply_to_context(ui.ctx());
                    let _ = self.custom_theme.save_to_disk();
                    self.theme_status_msg = Some(("Loaded Nordic Frost preset".to_string(), false));
                }
                if ui.button("Solarized Midnight").clicked() {
                    self.custom_theme = CustomTheme::solarized_midnight();
                    self.custom_theme_json = self.custom_theme.to_json().unwrap_or_default();
                    self.custom_theme.apply_to_context(ui.ctx());
                    let _ = self.custom_theme.save_to_disk();
                    self.theme_status_msg =
                        Some(("Loaded Solarized Midnight preset".to_string(), false));
                }
            });

            ui.add_space(8.0);

            // Interactive Color Token Editor & Live Swatch Box
            ui.columns(2, |columns| {
                // Left Column: Color Pickers
                columns[0].vertical(|ui| {
                    ui.label(RichText::new("🎨 Palette Tokens").strong().color(Color32::from_rgb(95, 185, 255)));

                    egui::Grid::new("theme_color_tokens_grid")
                        .spacing([16.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("Base Background:");
                            ui.color_edit_button_srgb(&mut self.custom_theme.background);
                            ui.end_row();

                            ui.label("Secondary Background:");
                            ui.color_edit_button_srgb(&mut self.custom_theme.secondary_bg);
                            ui.end_row();

                            ui.label("Surface Surface:");
                            ui.color_edit_button_srgb(&mut self.custom_theme.surface);
                            ui.end_row();

                            ui.label("Elevated Panel:");
                            ui.color_edit_button_srgb(&mut self.custom_theme.elevated);
                            ui.end_row();

                            ui.label("Subtle Border:");
                            ui.color_edit_button_srgb(&mut self.custom_theme.border_subtle);
                            ui.end_row();

                            ui.label("Structural Border:");
                            ui.color_edit_button_srgb(&mut self.custom_theme.border_strong);
                            ui.end_row();

                            ui.label("Primary Text:");
                            ui.color_edit_button_srgb(&mut self.custom_theme.text_primary);
                            ui.end_row();

                            ui.label("Secondary Text:");
                            ui.color_edit_button_srgb(&mut self.custom_theme.text_secondary);
                            ui.end_row();

                            ui.label("Accent / Interactive:");
                            ui.color_edit_button_srgb(&mut self.custom_theme.accent);
                            ui.end_row();
                        });
                });

                // Right Column: Live Swatch & Component Preview Box
                columns[1].vertical(|ui| {
                    ui.label(RichText::new("👁 Live UI Component Preview").strong().color(Color32::from_rgb(95, 185, 255)));

                    let preview_bg = self.custom_theme.bg_color();
                    let preview_surf = self.custom_theme.surface_color();
                    let preview_border = self.custom_theme.border_subtle_color();
                    let preview_text = self.custom_theme.text_primary_color();
                    let preview_sub = self.custom_theme.text_secondary_color();
                    let preview_accent = self.custom_theme.accent_color();

                    egui::Frame::NONE
                        .fill(preview_bg)
                        .stroke(Stroke::new(1.5_f32, preview_border))
                        .corner_radius(CornerRadius::same(8))
                        .inner_margin(egui::Margin::symmetric(14, 12))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new("MUVRIX Player Card")
                                    .strong()
                                    .size(13.5)
                                    .color(preview_text),
                            );
                            ui.label(
                                RichText::new("Live dynamic simulation of HUD and settings aesthetics.")
                                    .size(11.0)
                                    .color(preview_sub),
                            );

                            ui.add_space(6.0);

                            egui::Frame::NONE
                                .fill(preview_surf)
                                .stroke(Stroke::new(1.0_f32, preview_border))
                                .corner_radius(CornerRadius::same(6))
                                .inner_margin(egui::Margin::symmetric(10, 8))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new("Track 01 • Cybernetic Resonance (FLAC 96kHz)")
                                                .size(11.5)
                                                .color(preview_text),
                                        );
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new("● Engine Synchronized")
                                                .size(10.5)
                                                .color(preview_accent),
                                        );
                                        ui.add_space(8.0);
                                        ui.label(
                                            RichText::new("Stereo 2.0")
                                                .size(10.5)
                                                .color(preview_sub),
                                        );
                                    });
                                });

                            ui.add_space(8.0);

                            ui.horizontal(|ui| {
                                egui::Frame::NONE
                                    .fill(preview_accent)
                                    .corner_radius(CornerRadius::same(4))
                                    .inner_margin(egui::Margin::symmetric(10, 4))
                                    .show(ui, |ui| {
                                        ui.label(
                                            RichText::new("▶ Play Track")
                                                .strong()
                                                .color(preview_bg),
                                        );
                                    });

                                egui::Frame::NONE
                                    .fill(preview_surf)
                                    .stroke(Stroke::new(1.0_f32, preview_border))
                                    .corner_radius(CornerRadius::same(4))
                                    .inner_margin(egui::Margin::symmetric(8, 4))
                                    .show(ui, |ui| {
                                        ui.label(
                                            RichText::new("🎛 Equalizer")
                                                .color(preview_text),
                                        );
                                    });
                            });
                        });
                });
            });

            ui.add_space(10.0);

            // Action Buttons
            ui.horizontal(|ui| {
                if ui_components::primary_button(ui, "⚡ Apply Theme to UI Visuals", None).clicked() {
                    self.custom_theme.apply_to_context(ui.ctx());
                    self.custom_theme_json = self.custom_theme.to_json().unwrap_or_default();
                    let _ = self.custom_theme.save_to_disk();
                    self.theme_status_msg = Some(("Theme applied to runtime UI Visuals and saved".to_string(), false));
                }

                if ui.button("📋 Export JSON to Clipboard").clicked() {
                    let json = self.custom_theme.to_json().unwrap_or_default();
                    self.custom_theme_json = json.clone();
                    ui.ctx().copy_text(json);
                    self.theme_status_msg = Some(("Exported JSON theme copied to clipboard".to_string(), false));
                }
            });

            if let Some((ref msg, is_err)) = self.theme_status_msg {
                let col = if is_err {
                    Color32::from_rgb(255, 80, 80)
                } else {
                    BrandPalette::ELECTRIC_LIME
                };
                ui.add_space(4.0);
                ui.label(RichText::new(msg).color(col));
            }

            ui.add_space(8.0);

            // Collapsible JSON Import / Export Text Area
            let expand_theme_json = std::env::var("MUVRIX_EXPAND_THEME_JSON").is_ok();
            egui::CollapsingHeader::new("JSON Theme Import & Direct Schema Editor")
                .default_open(true)
                .open(if expand_theme_json { Some(true) } else { None })
                .show(ui, |ui| {
                    ui.label("Edit or paste a CustomTheme JSON definition below:");
                    ui.add(
                        egui::TextEdit::multiline(&mut self.custom_theme_json)
                            .desired_rows(8)
                            .desired_width(ui.available_width())
                            .font(egui::TextStyle::Monospace),
                    );
                    ui.add_space(4.0);
                    if ui.button("Import & Apply JSON Theme").clicked() {
                        match CustomTheme::from_json(&self.custom_theme_json) {
                            Ok(theme) => {
                                self.custom_theme = theme;
                                self.custom_theme.apply_to_context(ui.ctx());
                                let _ = self.custom_theme.save_to_disk();
                                self.theme_status_msg =
                                    Some(("Successfully imported, applied, and saved JSON theme".to_string(), false));
                            }
                            Err(e) => {
                                self.theme_status_msg =
                                    Some((format!("JSON Theme parse error: {e}"), true));
                            }
                        }
                    }
                });
        });

        ui.add_space(8.0);
        ui.group(|ui| {
            ui.label(
                RichText::new("Window & Desktop Frame")
                    .strong()
                    .size(14.0)
                    .color(Color32::from_rgb(95, 185, 255)),
            );
            ui.label(
                RichText::new(
                    "Configure native window borders, frameless presentation, and compact mini-player behavior.",
                )
                .size(11.5)
                .color(Color32::from_gray(150)),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Display Mode:");
                let current_mode = if self.is_pip {
                    "Compact Mini-Player (Floating PiP)"
                } else if self.is_fullscreen {
                    "Borderless Fullscreen"
                } else {
                    "Standard Native Window"
                };
                ui.label(
                    RichText::new(current_mode)
                        .strong()
                        .color(BrandPalette::ELECTRIC_LIME),
                );
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui
                    .button(if self.is_pip {
                        "Exit Mini-Player"
                    } else {
                        "Switch to Mini-Player (PiP)"
                    })
                    .clicked()
                {
                    self.toggle_pip(ui.ctx());
                }
                if ui
                    .button(if self.is_fullscreen {
                        "Exit Fullscreen"
                    } else {
                        "Toggle Fullscreen"
                    })
                    .clicked()
                {
                    self.toggle_fullscreen(ui.ctx());
                }
            });
        });
    }
}
