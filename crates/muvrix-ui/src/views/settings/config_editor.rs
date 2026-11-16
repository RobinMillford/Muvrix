use eframe::egui::{self, Color32, RichText};

use crate::app::MuvrixApp;
use crate::config_editor::ConfigEditorFile;
use crate::ui_components;

impl MuvrixApp {
    pub(crate) fn render_settings_config_editor(&mut self, ui: &mut egui::Ui) {
        self.ensure_config_editor_loaded();
        ui.group(|ui| {
                        ui.label(
                            RichText::new("Sandboxed libmpv Configuration")
                                .strong()
                                .size(14.0)
                                .color(Color32::from_rgb(95, 185, 255)),
                        );
                        ui.label(
                            RichText::new(
                                "Edit mpv.conf or input.conf in Muvrix's config directory. Applying mpv.conf issues load-config-file.",
                            )
                            .size(11.5)
                            .color(Color32::from_gray(150)),
                        );
                        ui.horizontal(|ui| {
                            for file in [ConfigEditorFile::MpvConf, ConfigEditorFile::InputConf] {
                                let is_selected = self.config_editor_file == file;
                                if ui_components::nav_tab(ui, file.label(), is_selected).clicked() {
                                    self.config_editor_file = file;
                                    self.config_editor_loaded = false;
                                    self.config_editor_text.clear();
                                }
                            }
                            if ui.button("Reload").clicked() {
                                self.config_editor_loaded = false;
                                self.ensure_config_editor_loaded();
                            }
                        });
                        ui.add_space(4.0);
                        ui.add(
                            egui::TextEdit::multiline(&mut self.config_editor_text)
                                .code_editor()
                                .desired_rows(18)
                                .desired_width(f32::INFINITY),
                        );
                        ui.horizontal(|ui| {
                            if ui.button(RichText::new("Save & Apply").strong()).clicked() {
                                self.save_config_editor();
                            }
                            if !self.config_editor_status.is_empty() {
                                ui.label(
                                    RichText::new(&self.config_editor_status)
                                        .size(11.0)
                                        .color(Color32::from_gray(170)),
                                );
                            }
                        });
                    });
    }
}
