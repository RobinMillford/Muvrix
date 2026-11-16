use eframe::egui::{self, Color32, RichText};
use muvrix_player::{AccentColor, ThemeMode};

use crate::app::MuvrixApp;

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
    }
}
