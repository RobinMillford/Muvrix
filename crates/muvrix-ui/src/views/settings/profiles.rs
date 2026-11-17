use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke};

use crate::app::MuvrixApp;
use crate::branding::BrandPalette;
use crate::ui_components;

impl MuvrixApp {
    pub(crate) fn render_settings_profiles(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.label(
                RichText::new("Playback Profiles & Settings Isolation")
                    .strong()
                    .size(15.0)
                    .color(BrandPalette::ELECTRIC_LIME),
            );
            ui.label(
                RichText::new(
                    "Switch between distinct audio EQ curves, subtitle typography, shader presets, \
                     and volume levels in <50ms without reloading the media engine or interrupting playback.",
                )
                .size(11.5)
                .color(BrandPalette::SECONDARY_TEXT),
            );

            ui.add_space(6.0);

            // Performance & Latency Telemetry Banner
            if let Some(elapsed) = self.profile_switch_elapsed {
                let ms = elapsed.as_secs_f64() * 1000.0;
                let color = if ms <= 50.0 {
                    BrandPalette::ELECTRIC_LIME
                } else {
                    Color32::from_rgb(255, 80, 80)
                };
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "⚡ Active Profile Switch Latency: {:.2} ms (Budget: < 50.00 ms)",
                            ms
                        ))
                        .strong()
                        .color(color),
                    );
                    if ms <= 50.0 {
                        ui.label(
                            RichText::new("PASS (Seamless In-Engine Switch)")
                                .size(11.0)
                                .color(BrandPalette::MUTED_TEXT),
                        );
                    }
                });
                ui.add_space(4.0);
            }

            if let Some((ref msg, is_err)) = self.profile_status_msg {
                let col = if is_err {
                    Color32::from_rgb(255, 80, 80)
                } else {
                    BrandPalette::ELECTRIC_LIME
                };
                ui.label(RichText::new(msg).color(col));
                ui.add_space(4.0);
            }

            ui.horizontal(|ui| {
                if ui_components::primary_button(ui, "+ Create New Profile", None).clicked() {
                    self.new_profile_dialog_open = true;
                }
                if ui.button("↻ Reload Profiles from DB").clicked() {
                    self.reload_profiles_from_db();
                }
            });
        });

        ui.add_space(8.0);

        // New Profile Inline Creator if opened
        if self.new_profile_dialog_open {
            ui.group(|ui| {
                ui.label(
                    RichText::new("New Playback Profile")
                        .strong()
                        .color(Color32::from_rgb(95, 185, 255)),
                );
                ui.label(
                    "Captures current runtime volume, audio EQ, subtitle style, and shader presets into a reusable profile.",
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("Profile Name:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_profile_name)
                            .hint_text("e.g. Living Room, Studio Monitors..."),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Description:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_profile_desc)
                            .hint_text("Optional details..."),
                    );
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui_components::primary_button(ui, "Save Profile", None).clicked() {
                        let name = self.new_profile_name.clone();
                        let desc = if self.new_profile_desc.trim().is_empty() {
                            None
                        } else {
                            Some(self.new_profile_desc.clone())
                        };
                        let res = self.create_profile_from_state(&name, desc.as_deref());
                        if res.is_ok() {
                            self.new_profile_name.clear();
                            self.new_profile_desc.clear();
                            self.new_profile_dialog_open = false;
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        self.new_profile_dialog_open = false;
                    }
                });
            });
            ui.add_space(8.0);
        }

        // Profile Cards
        let profiles = self.available_profiles.clone();
        for profile in profiles {
            let is_active = self.active_profile.as_ref().map(|a| a.id) == Some(profile.id);

            let border_stroke = if is_active {
                Stroke::new(1.5_f32, BrandPalette::ELECTRIC_LIME)
            } else {
                Stroke::new(1.0_f32, BrandPalette::BORDER_SLATE)
            };

            let bg_fill = if is_active {
                BrandPalette::SLATE_SURFACE
            } else {
                BrandPalette::OBSIDIAN
            };

            egui::Frame::NONE
                .fill(bg_fill)
                .stroke(border_stroke)
                .corner_radius(CornerRadius::same(6))
                .inner_margin(egui::Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&profile.name).strong().size(14.0).color(
                            if is_active {
                                BrandPalette::ELECTRIC_LIME
                            } else {
                                BrandPalette::PRIMARY_TEXT
                            },
                        ));

                        if is_active {
                            ui.label(
                                RichText::new("● ACTIVE PROFILE")
                                    .strong()
                                    .size(11.0)
                                    .color(BrandPalette::ELECTRIC_LIME),
                            );
                        }

                        if let Some(ref desc) = profile.description {
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(desc)
                                    .size(11.5)
                                    .color(BrandPalette::SECONDARY_TEXT),
                            );
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let is_default = profile.name.eq_ignore_ascii_case("default");
                            if !is_default && !is_active && ui.button("🗑 Delete").clicked() {
                                let _ = self.delete_profile(profile.id);
                            }

                            if ui.button("💾 Overwrite with Current State").clicked() {
                                let _ = self.update_profile_from_state(profile.id);
                            }

                            if !is_active
                                && ui_components::primary_button(ui, "⚡ Activate", None).clicked()
                            {
                                let _ = self.switch_profile(profile.id);
                            }
                        });
                    });

                    ui.add_space(6.0);

                    // Isolated Settings Grid
                    egui::Grid::new(format!("prof_grid_{}", profile.id))
                        .spacing([24.0, 4.0])
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new("🔊 Master Volume:")
                                    .color(BrandPalette::SECONDARY_TEXT),
                            );
                            ui.label(RichText::new(format!("{:.0}%", profile.volume)).strong());

                            ui.label(
                                RichText::new("🎛 Equalizer Preset:")
                                    .color(BrandPalette::SECONDARY_TEXT),
                            );
                            ui.label(RichText::new(&profile.equalizer_preset).strong());

                            ui.end_row();

                            ui.label(
                                RichText::new("⏱ Audio Delay:").color(BrandPalette::SECONDARY_TEXT),
                            );
                            ui.label(
                                RichText::new(format!("{} ms", profile.audio_delay_ms)).strong(),
                            );

                            ui.label(
                                RichText::new("📝 Subtitle Style:")
                                    .color(BrandPalette::SECONDARY_TEXT),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "{} pt, Color: {}, Bold: {}",
                                    profile.sub_font_size, profile.sub_color, profile.sub_bold
                                ))
                                .strong(),
                            );

                            ui.end_row();

                            ui.label(
                                RichText::new("🎨 Video Shader:")
                                    .color(BrandPalette::SECONDARY_TEXT),
                            );
                            ui.label(RichText::new(&profile.shader_preset).strong());

                            ui.label(
                                RichText::new("🎚 Loudness Normalization:")
                                    .color(BrandPalette::SECONDARY_TEXT),
                            );
                            let loud_str = profile
                                .loudness_target
                                .as_deref()
                                .map(|t| format!("{t} LUFS"))
                                .unwrap_or_else(|| "Off / Untargeted".to_string());
                            ui.label(RichText::new(loud_str).strong());

                            ui.end_row();
                        });
                });

            ui.add_space(6.0);
        }
    }
}
