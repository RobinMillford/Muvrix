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

use eframe::egui::{self, RichText};

use crate::app::MuvrixApp;
use crate::ui_components;
use crate::views::SettingsTab;

impl MuvrixApp {
    pub fn render_settings_view(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                let logo_tex = self.ensure_logo_texture(ui.ctx());
                crate::branding::render_logo(ui, logo_tex, 24.0);
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Preferences & Engine Configuration")
                        .strong()
                        .size(16.0),
                );
            });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("🔎 Search settings:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.settings_search)
                        .hint_text("theme, subtitle, audio, playlist, config...")
                        .desired_width(260.0),
                );
                if !self.settings_search.is_empty() && ui.button("Clear").clicked() {
                    self.settings_search.clear();
                }
            });

            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
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
                for tab in visible {
                    let is_selected = self.settings_tab == tab;
                    if ui_components::nav_tab(ui, tab.label(), is_selected).clicked() {
                        self.settings_tab = tab;
                    }
                }
            });

            ui.add_space(4.0);
            ui.separator();
            ui.add_space(8.0);

            let mut scroll_area = egui::ScrollArea::vertical().auto_shrink([false, false]);
            if let Ok(offset_str) = std::env::var("MUVRIX_SETTINGS_SCROLL_Y") {
                if let Ok(offset) = offset_str.parse::<f32>() {
                    scroll_area = scroll_area.vertical_scroll_offset(offset);
                }
            }
            scroll_area.show(ui, |ui| {
                ui.set_max_width(920.0);
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
    }
}
