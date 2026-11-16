use eframe::egui::{self, Color32, RichText};
use muvrix_player::SeekMode;

use crate::app::{format_timestamp, MuvrixApp};
use crate::icons::{self, Icon};

impl MuvrixApp {
    /// Renders the Synchronized LRC Lyrics view with karaoke timeline tracking.
    pub(crate) fn render_lyrics_view(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                icons::icon_label(ui, Icon::Lyrics, 20.0, Color32::from_rgb(56, 189, 248));
                ui.label(
                    RichText::new("Synchronized Lyrics")
                        .strong()
                        .size(17.0)
                        .color(Color32::from_rgb(56, 189, 248)),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button("Load LRC File...")
                        .on_hover_text("Open external LRC file from disk")
                        .clicked()
                    {
                        self.load_lyrics_dialog();
                    }
                    if self.lyrics.is_some() && ui.button("Clear Lyrics").clicked() {
                        self.lyrics = None;
                    }
                    ui.checkbox(&mut self.lyrics_auto_scroll, "Auto-scroll");
                });
            });

            ui.add_space(4.0);
            ui.separator();
            ui.add_space(6.0);

            let current_pos = self.controller.time_pos();

            if let Some(ref lrc) = self.lyrics {
                let title = lrc.title.as_deref().unwrap_or("Unknown Title");
                let artist = lrc.artist.as_deref().unwrap_or("Unknown Artist");
                let album = lrc.album.as_deref().unwrap_or("");

                ui.vertical_centered(|ui| {
                    ui.label(RichText::new(title).strong().size(18.0).color(Color32::WHITE));
                    let sub = if !album.is_empty() {
                        format!("{} — {}", artist, album)
                    } else {
                        artist.to_string()
                    };
                    ui.label(RichText::new(sub).size(13.0).color(Color32::from_gray(180)));
                });

                ui.add_space(10.0);

                let active_idx = lrc.find_active_index((current_pos * 1000.0) as i64);
                let mut seek_to_sec: Option<f64> = None;

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            for (idx, line) in lrc.lines.iter().enumerate() {
                                let is_active = Some(idx) == active_idx;
                                let time_sec = line.timestamp_seconds();
                                let time_label = format_timestamp(time_sec);

                                let (font_size, color, strong) = if is_active {
                                    (19.0, Color32::from_rgb(56, 189, 248), true)
                                } else {
                                    (14.5, Color32::from_rgb(148, 163, 184), false)
                                };

                                ui.horizontal(|ui| {
                                    ui.add_space(ui.available_width() * 0.12);
                                    ui.label(
                                        RichText::new(format!("[{}]", time_label))
                                            .monospace()
                                            .size(11.0)
                                            .color(Color32::from_gray(110)),
                                    );

                                    if is_active && !line.words.is_empty() {
                                        let active_word_idx = line.find_active_word_index((current_pos * 1000.0) as i64);
                                        for (w_idx, word) in line.words.iter().enumerate() {
                                            let (w_color, w_strong) = if Some(w_idx) == active_word_idx {
                                                (Color32::from_rgb(251, 191, 36), true) // Glowing amber/gold active word
                                            } else if active_word_idx.is_some_and(|cur| w_idx < cur) {
                                                (Color32::from_rgb(224, 242, 254), true) // Sung words (light cyan)
                                            } else {
                                                (Color32::from_rgb(148, 163, 184), false) // Upcoming words
                                            };

                                            let mut w_text = RichText::new(&word.text).size(font_size).color(w_color);
                                            if w_strong {
                                                w_text = w_text.strong();
                                            }
                                            if ui.selectable_label(Some(w_idx) == active_word_idx, w_text).clicked() {
                                                seek_to_sec = Some(word.timestamp_seconds());
                                            }
                                        }
                                    } else {
                                        let mut text =
                                            RichText::new(&line.text).size(font_size).color(color);
                                        if strong {
                                            text = text.strong();
                                        }
                                        let resp = ui.selectable_label(is_active, text);
                                        if resp.clicked() {
                                            seek_to_sec = Some(time_sec);
                                        }
                                        if is_active && self.lyrics_auto_scroll {
                                            resp.scroll_to_me(Some(egui::Align::Center));
                                        }
                                    }
                                });
                                ui.add_space(4.0);
                            }
                        });
                    });

                if let Some(target) = seek_to_sec {
                    let _ = self.controller.seek(target, SeekMode::Absolute);
                }
            } else {
                ui.vertical_centered(|ui| {
                    ui.add_space(60.0);
                    icons::icon_label(ui, Icon::Lyrics, 48.0, Color32::from_gray(120));
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new("No Synchronized Lyrics Loaded")
                            .strong()
                            .size(16.0)
                            .color(Color32::from_gray(200)),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(
                            "Open a media file with an adjacent .lrc sidecar file, or click below to select an LRC lyric file manually.",
                        )
                        .size(13.0)
                        .color(Color32::from_gray(150)),
                    );
                    ui.add_space(16.0);
                    if ui
                        .button(RichText::new("📂 Load LRC Lyric File...").strong())
                        .clicked()
                    {
                        self.load_lyrics_dialog();
                    }
                });
            }
        });
    }
}
