use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke};

use crate::app::shortcuts::ShortcutAction;
use crate::app::MuvrixApp;
use crate::branding::BrandPalette;
use crate::ui_components;

impl MuvrixApp {
    pub(crate) fn render_settings_shortcuts(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.label(
                RichText::new("Configurable Keyboard Shortcuts Mapper")
                    .strong()
                    .size(15.0)
                    .color(BrandPalette::ELECTRIC_LIME),
            );
            ui.label(
                RichText::new(
                    "Customize playback, navigation, and display hotkeys. \
                     Includes modifier support (Ctrl, Alt, Shift), conflict detection, and reset to defaults.",
                )
                .size(11.5)
                .color(BrandPalette::SECONDARY_TEXT),
            );

            ui.add_space(6.0);

            // Conflict Warning Banner
            let conflicts = self.keybindings.find_conflicts();
            if !conflicts.is_empty() {
                egui::Frame::NONE
                    .fill(Color32::from_rgb(60, 20, 20))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(255, 80, 80)))
                    .corner_radius(CornerRadius::same(4))
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(format!(
                                "⚠ Conflict Detected ({} conflicting combination(s))",
                                conflicts.len()
                            ))
                            .strong()
                            .color(Color32::from_rgb(255, 80, 80)),
                        );
                        for (act_a, act_b, combo) in &conflicts {
                            ui.label(
                                RichText::new(format!(
                                    "• Key '{}' is assigned to both '{}' and '{}'",
                                    combo.display_string(),
                                    act_a.display_name(),
                                    act_b.display_name()
                                ))
                                .size(11.5)
                                .color(Color32::from_rgb(255, 180, 180)),
                            );
                        }
                    });
                ui.add_space(6.0);
            }

            // Key Recording Status Banner
            if let Some(target) = self.recording_shortcut {
                egui::Frame::NONE
                    .fill(Color32::from_rgb(20, 45, 30))
                    .stroke(Stroke::new(1.0_f32, BrandPalette::ELECTRIC_LIME))
                    .corner_radius(CornerRadius::same(4))
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(format!(
                                "🔴 Recording new shortcut for '{}'... Press any key combination (or Esc to cancel)",
                                target.display_name()
                            ))
                            .strong()
                            .color(BrandPalette::ELECTRIC_LIME),
                        );
                    });
                ui.add_space(6.0);
            }

            ui.horizontal(|ui| {
                if ui.button("↺ Reset All Shortcuts to Defaults").clicked() {
                    self.keybindings.reset_to_defaults();
                    let _ = self.keybindings.save_to_disk();
                    self.recording_shortcut = None;
                }
            });
        });

        ui.add_space(8.0);

        // Shortcuts Table
        egui::Frame::NONE
            .fill(BrandPalette::SLATE_SURFACE)
            .stroke(Stroke::new(1.0_f32, BrandPalette::BORDER_SLATE))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(egui::Margin::symmetric(14, 10))
            .show(ui, |ui| {
                egui::Grid::new("shortcuts_editor_grid")
                    .striped(true)
                    .spacing([24.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("Action")
                                .strong()
                                .color(BrandPalette::MUTED_TEXT),
                        );
                        ui.label(
                            RichText::new("Binding")
                                .strong()
                                .color(BrandPalette::MUTED_TEXT),
                        );
                        ui.label(
                            RichText::new("Alternate")
                                .strong()
                                .color(BrandPalette::MUTED_TEXT),
                        );
                        ui.label(
                            RichText::new("Operations")
                                .strong()
                                .color(BrandPalette::MUTED_TEXT),
                        );
                        ui.end_row();

                        for &action in ShortcutAction::ALL {
                            let is_recording = self.recording_shortcut == Some(action);

                            ui.label(RichText::new(action.display_name()).strong());

                            let primary_text = if is_recording {
                                "[Press Key...]".to_string()
                            } else {
                                self.keybindings
                                    .get(action)
                                    .map(|c| c.display_string())
                                    .unwrap_or_else(|| "-".to_string())
                            };

                            let key_color = if is_recording {
                                Color32::from_rgb(255, 100, 100)
                            } else {
                                BrandPalette::ELECTRIC_LIME
                            };

                            ui.label(
                                RichText::new(primary_text)
                                    .strong()
                                    .monospace()
                                    .color(key_color),
                            );

                            let alt_str = self
                                .keybindings
                                .alternates
                                .get(&action)
                                .map(|a| a.display_string())
                                .unwrap_or_else(|| "-".to_string());
                            ui.label(
                                RichText::new(alt_str)
                                    .monospace()
                                    .color(BrandPalette::SECONDARY_TEXT),
                            );

                            ui.horizontal(|ui| {
                                if is_recording {
                                    if ui.button("Cancel").clicked() {
                                        self.recording_shortcut = None;
                                    }
                                } else {
                                    if ui_components::primary_button(ui, "Rebind", None).clicked() {
                                        self.recording_shortcut = Some(action);
                                    }
                                    if ui.button("Reset").clicked() {
                                        self.keybindings.reset_action(action);
                                        let _ = self.keybindings.save_to_disk();
                                    }
                                }
                            });

                            ui.end_row();
                        }
                    });
            });
    }
}
