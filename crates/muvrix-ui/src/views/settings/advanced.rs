use eframe::egui::{self, Color32, RichText};

use crate::app::{MuvrixApp, SleepTimer};
use crate::branding::BrandPalette;
use crate::config_editor::ConfigEditorFile;
use crate::views::SettingsTab;

impl MuvrixApp {
    pub(crate) fn render_settings_advanced(&mut self, ui: &mut egui::Ui) {
        if let Some(ref status) = self.advanced_status {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("ℹ")
                            .color(BrandPalette::ELECTRIC_LIME)
                            .strong(),
                    );
                    ui.label(
                        RichText::new(status)
                            .size(12.5)
                            .color(BrandPalette::IVORY_TEXT),
                    );
                });
            });
            ui.add_space(6.0);
        }

        // Section 1: Settings Backup & Restore (XML)
        ui.group(|ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new("1. Settings Backup & Restore")
                                .strong()
                                .size(14.0)
                                .color(BrandPalette::ELECTRIC_LIME),
                        );
                        ui.label(
                            RichText::new(
                                "Export or restore your complete Muvrix configuration, audio preferences, subtitle styles, and shader profiles using an XML backup file.",
                            )
                            .size(12.0)
                            .color(BrandPalette::MUTED_TEXT),
                        );
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if ui
                                .button(RichText::new("📤 Export Settings to XML...").strong())
                                .clicked()
                            {
                                self.export_settings_to_xml();
                            }
                            if ui
                                .button(RichText::new("📥 Import Settings from XML...").strong())
                                .clicked()
                            {
                                self.import_settings_from_xml();
                            }
                        });
                    });

        ui.add_space(8.0);

        // Section 2: MPV Configuration Storage Location
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("2. MPV Configuration Storage")
                    .strong()
                    .size(14.0)
                    .color(BrandPalette::ELECTRIC_LIME),
            );
            let config_dir = crate::config_editor::config_dir();
            ui.label(
                RichText::new(format!("Location: {}", config_dir.display()))
                    .monospace()
                    .size(11.5)
                    .color(Color32::from_gray(180)),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("Edit mpv.conf").clicked() {
                    self.config_editor_file = ConfigEditorFile::MpvConf;
                    self.settings_tab = SettingsTab::ConfigEditor;
                }
                if ui.button("Edit input.conf").clicked() {
                    self.config_editor_file = ConfigEditorFile::InputConf;
                    self.settings_tab = SettingsTab::ConfigEditor;
                }
            });
        });

        ui.add_space(8.0);

        // Section 3: Cache Management
        ui.group(|ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new("3. Cache Management")
                                .strong()
                                .size(14.0)
                                .color(BrandPalette::ELECTRIC_LIME),
                        );
                        ui.label(
                            RichText::new(
                                "Safely purge cached thumbnails, subtitle font paths, and engine configurations without touching media files.",
                            )
                            .size(12.0)
                            .color(BrandPalette::MUTED_TEXT),
                        );
                        ui.add_space(6.0);
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Clear Thumbnail Cache").clicked() {
                                self.clear_thumbnail_cache();
                            }
                            if ui.button("Clear Subtitle Font Cache").clicked() {
                                self.clear_font_cache();
                            }
                            if ui.button("Flush Config Cache").clicked() {
                                self.clear_config_cache();
                            }
                        });
                    });

        ui.add_space(8.0);

        // Section 4: Diagnostics & Logging
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("4. Diagnostics & Logging")
                    .strong()
                    .size(14.0)
                    .color(BrandPalette::ELECTRIC_LIME),
            );
            ui.add_space(4.0);
            if ui
                .checkbox(
                    &mut self.verbose_logging,
                    "Enable Verbose Diagnostic Logging",
                )
                .on_hover_text("Enables granular trace and telemetry output for troubleshooting.")
                .changed()
            {
                self.save_media_settings();
            }
            ui.add_space(4.0);
            if ui
                .button(RichText::new("📄 Dump Diagnostic System Report...").strong())
                .clicked()
            {
                self.dump_diagnostic_logs();
            }
        });

        ui.add_space(8.0);

        // Section 5: Playback History & Privacy
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("5. Playback History & Privacy")
                    .strong()
                    .size(14.0)
                    .color(BrandPalette::ELECTRIC_LIME),
            );

            ui.add_space(4.0);

            let mut enabled = self.history_store.enabled;
            if ui
                .checkbox(&mut enabled, "Enable Playback History Recording")
                .changed()
            {
                self.history_store.set_enabled(enabled);
                let _ = self.history_store.save();
            }

            let mut auto_resume = self.history_store.auto_resume_enabled;
            if ui
                .checkbox(
                    &mut auto_resume,
                    "Enable Resume Playback Prompts (>10s threshold)",
                )
                .changed()
            {
                self.history_store.set_auto_resume(auto_resume);
                let _ = self.history_store.save();
            }

            ui.add_space(4.0);
            if ui.button("Clear Playback History").clicked() {
                self.history_store.clear();
                let _ = self.history_store.save();
                self.status_message = "Playback history cleared".to_string();
                self.advanced_status = Some("Playback history cleared".to_string());
            }
        });

        ui.add_space(8.0);

        // Section 6: Sleep Timer & Window Display
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("6. Sleep Timer & Window Mode")
                    .strong()
                    .size(14.0)
                    .color(BrandPalette::ELECTRIC_LIME),
            );
            ui.horizontal(|ui| {
                ui.label("Stop after:");
                egui::ComboBox::from_id_salt("settings_sleep_timer_combo")
                    .selected_text(self.sleep_timer.label())
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
                        RichText::new(format!("Remaining {}:{:02}", seconds / 60, seconds % 60))
                            .monospace()
                            .color(BrandPalette::ELECTRIC_LIME),
                    );
                }
            });

            ui.add_space(6.0);
            let pip_btn_text = if self.is_pip {
                "Exit Compact Mini-Player Mode"
            } else {
                "Enter Compact Mini-Player Mode"
            };
            if ui.button(RichText::new(pip_btn_text).strong()).clicked() {
                self.toggle_pip(ui.ctx());
            }
        });
    }
}
