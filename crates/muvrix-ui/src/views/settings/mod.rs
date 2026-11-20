pub mod about;
pub mod advanced;
pub mod appearance;
pub mod audio;
pub mod config_editor;
pub mod network;
pub mod profiles;
pub mod shortcuts;
pub mod subtitles;
pub mod video;

use eframe::egui::{self, Color32, RichText};

use crate::app::MuvrixApp;
use crate::branding::BrandPalette;
use crate::icons::{self, Icon};
use crate::views::SettingsTab;

fn settings_tab_icon(tab: SettingsTab) -> Icon {
    match tab {
        SettingsTab::Audio | SettingsTab::EqualizerAudio => Icon::VolumeHigh,
        SettingsTab::Subtitles => Icon::Subtitles,
        SettingsTab::VideoEnhancement => Icon::Sliders,
        SettingsTab::Appearance => Icon::AspectRatio,
        SettingsTab::Profiles => Icon::Library,
        SettingsTab::Shortcuts => Icon::Shortcuts,
        SettingsTab::NetworkMedia => Icon::Folder,
        SettingsTab::ConfigEditor => Icon::Developer,
        SettingsTab::Advanced | SettingsTab::Preferences => Icon::Diagnostics,
        SettingsTab::About => Icon::Information,
    }
}

impl MuvrixApp {
    pub fn render_settings_view(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                let logo_tex = self.ensure_logo_texture(ui.ctx());
                crate::branding::render_logo(ui, logo_tex, 24.0);
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Preferences")
                        .strong()
                        .size(16.0)
                        .color(BrandPalette::PRIMARY_TEXT),
                );
                ui.label(
                    RichText::new("Engine configuration & personalisation")
                        .size(11.5)
                        .color(BrandPalette::MUTED_TEXT),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.settings_search)
                            .hint_text("Search settings…")
                            .desired_width(220.0),
                    );
                    if !self.settings_search.is_empty() && ui.button("✕").clicked() {
                        self.settings_search.clear();
                    }
                });
            });

            ui.add_space(6.0);
            ui.separator();

            let tabs = [
                SettingsTab::Audio,
                SettingsTab::Subtitles,
                SettingsTab::VideoEnhancement,
                SettingsTab::Appearance,
                SettingsTab::Profiles,
                SettingsTab::Shortcuts,
                SettingsTab::NetworkMedia,
                SettingsTab::ConfigEditor,
                SettingsTab::Advanced,
                SettingsTab::About,
            ];
            let visible: Vec<SettingsTab> = tabs
                .into_iter()
                .filter(|tab| tab.matches_search(&self.settings_search))
                .collect();
            if !self.settings_tab.matches_search(&self.settings_search) {
                if let Some(first) = visible.first() {
                    self.settings_tab = *first;
                }
            }

            ui.add_space(4.0);

            ui.horizontal_top(|ui| {
                // ---- Navigation sidebar -------------------------------------
                ui.allocate_ui_with_layout(
                    egui::vec2(200.0, ui.available_height()),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("CATEGORIES")
                                .size(9.5)
                                .strong()
                                .color(BrandPalette::MUTED_TEXT),
                        );
                        ui.add_space(4.0);

                        egui::ScrollArea::vertical()
                            .id_salt("settings_sidebar")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                for tab in &visible {
                                    let selected = self.settings_tab == *tab;
                                    let resp = ui.add_sized(
                                        [ui.available_width(), 32.0],
                                        egui::Button::new(
                                            RichText::new(tab.label()).size(12.5).color(
                                                if selected {
                                                    BrandPalette::OBSIDIAN
                                                } else {
                                                    BrandPalette::PRIMARY_TEXT
                                                },
                                            ),
                                        )
                                        .fill(if selected {
                                            BrandPalette::ELECTRIC_LIME
                                        } else {
                                            Color32::TRANSPARENT
                                        })
                                        .stroke(if selected {
                                            egui::Stroke::NONE
                                        } else {
                                            egui::Stroke::new(0.0_f32, Color32::TRANSPARENT)
                                        })
                                        .corner_radius(6.0_f32),
                                    );
                                    if resp.clicked() {
                                        self.settings_tab = *tab;
                                    }
                                }
                            });
                    },
                );

                ui.separator();

                // ---- Content area -------------------------------------------
                ui.vertical(|ui| {
                    ui.add_space(2.0);
                    let mut scroll_area = egui::ScrollArea::vertical().auto_shrink([false, false]);
                    if let Ok(offset_str) = std::env::var("MUVRIX_SETTINGS_SCROLL_Y") {
                        if let Ok(offset) = offset_str.parse::<f32>() {
                            scroll_area = scroll_area.vertical_scroll_offset(offset);
                        }
                    }
                    scroll_area
                        .id_salt(format!("settings_content_{:?}", self.settings_tab))
                        .show(ui, |ui| {
                            ui.set_max_width(920.0);
                            ui.horizontal(|ui| {
                                icons::icon_label(
                                    ui,
                                    settings_tab_icon(self.settings_tab),
                                    15.0,
                                    BrandPalette::ELECTRIC_LIME,
                                );
                                ui.label(
                                    RichText::new(self.settings_tab.label())
                                        .strong()
                                        .size(15.0)
                                        .color(BrandPalette::PRIMARY_TEXT),
                                );
                            });
                            ui.add_space(6.0);
                            match self.settings_tab {
                                SettingsTab::Subtitles => self.render_settings_subtitles(ui),
                                SettingsTab::VideoEnhancement => self.render_settings_video(ui),
                                SettingsTab::Audio | SettingsTab::EqualizerAudio => {
                                    self.render_settings_audio(ui)
                                }
                                SettingsTab::Profiles => self.render_settings_profiles(ui),
                                SettingsTab::Shortcuts => self.render_settings_shortcuts(ui),
                                SettingsTab::NetworkMedia => self.render_settings_network(ui),
                                SettingsTab::Appearance => self.render_settings_appearance(ui),
                                SettingsTab::ConfigEditor => self.render_settings_config_editor(ui),
                                SettingsTab::Advanced | SettingsTab::Preferences => {
                                    self.render_settings_advanced(ui)
                                }
                                SettingsTab::About => self.render_settings_about(ui),
                            }
                        });
                });
            });
        });
    }
}
