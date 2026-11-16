use eframe::egui::{self, Color32, RichText};
use muvrix_player::PlaybackState;

use crate::app::{format_timestamp, MuvrixApp};
use crate::icons::{self, Icon};
use crate::views::AppView;

impl MuvrixApp {
    pub fn render_mini_player_deck(&mut self, ctx: &egui::Context) {
        if self.current_view == AppView::Player {
            return;
        }

        if *self.controller.state() == PlaybackState::Idle {
            return;
        }

        egui::TopBottomPanel::bottom("mini_player_deck")
            .resizable(false)
            .show(ctx, |ui| {
                if !self.ui_overlay_visible {
                    return;
                }
                ui.add_space(4.0);

                let time_pos = self.controller.time_pos();
                let duration = self.controller.duration();

                ui.horizontal(|ui| {
                    // Play / Pause
                    let (play_icon, play_tip) = match self.controller.state() {
                        PlaybackState::Playing => (Icon::Pause, "Pause"),
                        _ => (Icon::Play, "Play"),
                    };
                    if icons::icon_button(ui, play_icon, play_tip).clicked() {
                        self.toggle_pause();
                    }

                    // Stop
                    if icons::icon_button(ui, Icon::Stop, "Stop playback").clicked() {
                        let _ = self.controller.stop();
                    }

                    // Seeks
                    if icons::icon_button(ui, Icon::SeekBackward, "Seek -5s").clicked() {
                        self.seek_relative(-5.0);
                    }
                    if icons::icon_button(ui, Icon::SeekForward, "Seek +5s").clicked() {
                        self.seek_relative(5.0);
                    }

                    ui.separator();

                    // Media Title & Time
                    let file_label = self
                        .controller
                        .current_file()
                        .map(|p| {
                            std::path::Path::new(p)
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or(p)
                                .to_string()
                        })
                        .unwrap_or_else(|| "Media".to_string());

                    ui.label(
                        RichText::new(&file_label)
                            .strong()
                            .color(Color32::from_gray(230)),
                    );

                    let progress_str = format!(
                        "{} / {}",
                        format_timestamp(time_pos),
                        format_timestamp(duration)
                    );
                    ui.label(
                        RichText::new(progress_str)
                            .monospace()
                            .size(11.5)
                            .color(Color32::from_gray(160)),
                    );

                    // Mini Progress Bar
                    let progress = if duration > 0.0 {
                        (time_pos / duration) as f32
                    } else {
                        0.0
                    }
                    .clamp(0.0, 1.0);

                    let pbar = egui::ProgressBar::new(progress)
                        .desired_width(120.0)
                        .desired_height(6.0)
                        .fill(Color32::from_rgb(60, 140, 240));
                    ui.add(pbar);

                    // Return to Video Viewport button
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icons::icon_text_button(
                            ui,
                            Icon::Fullscreen,
                            "Return to Video",
                            "Switch back to Player view",
                        )
                        .clicked()
                        {
                            self.current_view = AppView::Player;
                        }

                        // Mini volume slider
                        let mut vol = self.controller.volume();
                        let vol_label = format!("{:.0}%", vol);
                        let max_volume = self.controller.volume_boost_max().max(100.0);
                        let vol_slider = egui::Slider::new(&mut vol, 0.0..=max_volume)
                            .show_value(false)
                            .text(vol_label);
                        if ui.add_sized([50.0, 16.0], vol_slider).changed() {
                            let _ = self.controller.set_volume(vol);
                        }
                    });
                });

                ui.add_space(4.0);
            });
    }
}
