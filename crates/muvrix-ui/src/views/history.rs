use eframe::egui::{self, Color32, RichText, Stroke};
use muvrix_player::SeekMode;

use crate::app::{format_timestamp, MuvrixApp};
use crate::icons::{self, Icon};
use crate::views::{format_timestamp_relative, AppView};

impl MuvrixApp {
    pub fn render_recent_view(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(4.0);

            let history_len = self.history_store.entries.len();

            // Header Toolbar
            ui.horizontal(|ui| {
                icons::icon_label(ui, Icon::History, 18.0, Color32::from_rgb(85, 175, 255));
                ui.label(
                    RichText::new(format!("Playback History ({history_len} items)"))
                        .strong()
                        .size(16.0),
                );

                ui.separator();

                let mut track_enabled = self.history_store.enabled;
                if ui
                    .checkbox(&mut track_enabled, "Record History")
                    .on_hover_text("Enable or pause tracking recently played media")
                    .changed()
                {
                    self.history_store.set_enabled(track_enabled);
                    let _ = self.history_store.save();
                }

                let mut auto_resume = self.history_store.auto_resume_enabled;
                if ui
                    .checkbox(&mut auto_resume, "Auto-Resume Prompt")
                    .on_hover_text("Prompt to resume playback from where you left off")
                    .changed()
                {
                    self.history_store.set_auto_resume(auto_resume);
                    let _ = self.history_store.save();
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icons::icon_text_button(
                        ui,
                        Icon::Close,
                        "Clear History",
                        "Delete all playback history records",
                    )
                    .clicked()
                    {
                        self.history_store.clear();
                        let _ = self.history_store.save();
                    }
                });
            });

            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);

            if history_len == 0 {
                ui.add_space(50.0);
                ui.vertical_centered(|ui| {
                    icons::icon_label(ui, Icon::History, 36.0, Color32::from_gray(120));
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new("No playback history recorded.")
                            .size(15.0)
                            .strong()
                            .color(Color32::from_gray(180)),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(
                            "Media you play will automatically appear here with durable resume timestamps.",
                        )
                        .size(12.5)
                        .color(Color32::from_gray(140)),
                    );
                });
                return;
            }

            let entries = self.history_store.entries.clone();
            let mut file_to_resume: Option<(String, f64)> = None;
            let mut file_to_restart: Option<String> = None;
            let mut file_to_remove: Option<String> = None;

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for entry in entries {
                        egui::Frame::NONE
                            .fill(Color32::from_rgba_premultiplied(22, 25, 34, 200))
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(45, 52, 68)))
                            .corner_radius(4.0)
                            .inner_margin(egui::Margin::symmetric(12, 8))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    // Title and full path
                                    ui.vertical(|ui| {
                                        ui.label(
                                            RichText::new(&entry.display_name)
                                                .strong()
                                                .size(13.5)
                                                .color(Color32::from_gray(235)),
                                        );
                                        ui.label(
                                            RichText::new(&entry.path)
                                                .size(11.0)
                                                .color(Color32::from_gray(130)),
                                        );

                                        // Progress Bar
                                        ui.add_space(2.0);
                                        let progress = if entry.duration_sec > 0.0 {
                                            (entry.last_position_sec / entry.duration_sec) as f32
                                        } else {
                                            0.0
                                        }
                                        .clamp(0.0, 1.0);

                                        ui.horizontal(|ui| {
                                            let pbar = egui::ProgressBar::new(progress)
                                                .desired_width(200.0)
                                                .desired_height(6.0)
                                                .fill(Color32::from_rgb(60, 150, 245));
                                            ui.add(pbar);

                                            let pos_str = format_timestamp(entry.last_position_sec);
                                            let dur_str = format_timestamp(entry.duration_sec);
                                            let percent = (progress * 100.0).round() as u32;

                                            ui.label(
                                                RichText::new(format!(
                                                    "{pos_str} / {dur_str} ({percent}%) · {}",
                                                    format_timestamp_relative(
                                                        entry.last_played_timestamp
                                                    )
                                                ))
                                                .monospace()
                                                .size(11.0)
                                                .color(Color32::from_gray(155)),
                                            );

                                            if entry.completed {
                                                ui.label(
                                                    RichText::new("Finished")
                                                        .size(10.5)
                                                        .color(Color32::from_rgb(100, 210, 120)),
                                                );
                                            }
                                        });
                                    });

                                    // Action buttons
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if icons::icon_button(
                                                ui,
                                                Icon::Close,
                                                "Remove from history",
                                            )
                                            .clicked()
                                            {
                                                file_to_remove = Some(entry.path.clone());
                                            }

                                            if icons::icon_text_button(
                                                ui,
                                                Icon::Replay,
                                                "Start Over",
                                                "Play from 00:00",
                                            )
                                            .clicked()
                                            {
                                                file_to_restart = Some(entry.path.clone());
                                            }

                                            if entry.last_position_sec > 5.0 && !entry.completed {
                                                let resume_label = format!(
                                                    "Resume ({})",
                                                    format_timestamp(entry.last_position_sec)
                                                );
                                                if icons::icon_text_button(
                                                    ui,
                                                    Icon::Play,
                                                    &resume_label,
                                                    "Resume playback at saved position",
                                                )
                                                .clicked()
                                                {
                                                    file_to_resume = Some((
                                                        entry.path.clone(),
                                                        entry.last_position_sec,
                                                    ));
                                                }
                                            } else if icons::icon_text_button(
                                                ui,
                                                Icon::Play,
                                                "Play",
                                                "Play file",
                                            )
                                            .clicked()
                                            {
                                                file_to_restart = Some(entry.path.clone());
                                            }
                                        },
                                    );
                                });
                            });
                        ui.add_space(4.0);
                    }
                });

            if let Some((path, pos)) = file_to_resume {
                self.load_file(&path);
                let _ = self.controller.seek(pos, SeekMode::Absolute);
                let _ = self.controller.play();
                self.pending_resume_prompt = None;
                self.current_view = AppView::Player;
            }
            if let Some(path) = file_to_restart {
                self.load_file(&path);
                let _ = self.controller.seek(0.0, SeekMode::Absolute);
                let _ = self.controller.play();
                self.pending_resume_prompt = None;
                self.current_view = AppView::Player;
            }
            if let Some(path) = file_to_remove {
                self.history_store.remove_entry(&path);
                let _ = self.history_store.save();
            }
        });
    }

    // -------------------------------------------------------------------------
}
