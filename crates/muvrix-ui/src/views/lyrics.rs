use eframe::egui::{self, Color32, RichText};
use muvrix_core::lyrics::LyricSource;
use muvrix_player::SeekMode;

use crate::app::{format_timestamp, MuvrixApp};
use crate::icons::{self, Icon};

impl MuvrixApp {
    /// Renders the Synchronized & Embedded Lyrics view with honest timeline tracking
    /// and clean un-timed typography.
    pub(crate) fn render_lyrics_view(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                icons::icon_label(ui, Icon::Lyrics, 20.0, Color32::from_rgb(56, 189, 248));
                ui.label(
                    RichText::new("Lyrics Studio")
                        .strong()
                        .size(17.0)
                        .color(Color32::from_rgb(56, 189, 248)),
                );

                if let Some(ref lrc) = self.lyrics {
                    ui.add_space(8.0);
                    let (badge_text, badge_color) = match lrc.source {
                        LyricSource::ExternalSynced => {
                            ("External LRC (Synchronized)", Color32::from_rgb(56, 189, 248))
                        }
                        LyricSource::EmbeddedSynced => (
                            "Embedded Lyrics (Synchronized)",
                            Color32::from_rgb(52, 211, 153),
                        ),
                        LyricSource::ExternalUnsynced => (
                            "External Text (Unsynchronized)",
                            Color32::from_rgb(251, 146, 60),
                        ),
                        LyricSource::EmbeddedUnsynced => (
                            "Embedded Lyrics (Unsynchronized)",
                            Color32::from_rgb(168, 85, 247),
                        ),
                    };

                    ui.label(
                        RichText::new(format!("[{badge_text}]"))
                            .size(11.5)
                            .strong()
                            .color(badge_color),
                    );
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button("Load File...")
                        .on_hover_text("Open external .lrc or .txt file from disk")
                        .clicked()
                    {
                        self.load_lyrics_dialog();
                    }
                    if ui
                        .button("Reload")
                        .on_hover_text("Re-evaluate external sidecar and embedded lyrics")
                        .clicked()
                    {
                        self.reload_lyrics_for_current_media();
                    }
                    if self.lyrics.is_some() && ui.button("Clear Lyrics").clicked() {
                        self.lyrics = None;
                    }
                    if self.lyrics.as_ref().is_some_and(|l| l.is_synchronized) {
                        ui.checkbox(&mut self.lyrics_auto_scroll, "Auto-scroll");
                    }
                });
            });

            ui.add_space(4.0);
            ui.separator();
            ui.add_space(6.0);

            let current_pos = self.controller.time_pos();

            if let Some(ref lrc) = self.lyrics {
                let default_title = self.controller.media_title().unwrap_or_else(|| "Unknown Title".to_string());
                let title = lrc.title.as_deref().unwrap_or(&default_title);
                let artist = lrc.artist.as_deref().unwrap_or("");
                let album = lrc.album.as_deref().unwrap_or("");

                ui.vertical_centered(|ui| {
                    ui.label(RichText::new(title).strong().size(18.0).color(Color32::WHITE));
                    let mut sub = String::new();
                    if !artist.is_empty() {
                        sub.push_str(artist);
                    }
                    if !album.is_empty() {
                        if !sub.is_empty() {
                            sub.push_str(" — ");
                        }
                        sub.push_str(album);
                    }
                    if !sub.is_empty() {
                        ui.label(RichText::new(sub).size(13.0).color(Color32::from_gray(180)));
                    }
                });

                ui.add_space(10.0);

                if lrc.is_synchronized {
                    // Synchronized LRC rendering with active line tracking and karaoke word highlighting
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
                                            let active_word_idx = line
                                                .find_active_word_index((current_pos * 1000.0) as i64);
                                            for (w_idx, word) in line.words.iter().enumerate() {
                                                let (w_color, w_strong) =
                                                    if Some(w_idx) == active_word_idx {
                                                        (Color32::from_rgb(251, 191, 36), true)
                                                    } else if active_word_idx.is_some_and(|cur| w_idx < cur) {
                                                        (Color32::from_rgb(224, 242, 254), true)
                                                    } else {
                                                        (Color32::from_rgb(148, 163, 184), false)
                                                    };

                                                let mut w_text = RichText::new(&word.text)
                                                    .size(font_size)
                                                    .color(w_color);
                                                if w_strong {
                                                    w_text = w_text.strong();
                                                }
                                                if ui
                                                    .selectable_label(
                                                        Some(w_idx) == active_word_idx,
                                                        w_text,
                                                    )
                                                    .clicked()
                                                {
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
                    // Honest unsynchronized lyrics rendering (no fake timestamps or karaoke highlights)
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.vertical_centered(|ui| {
                                for line in &lrc.lines {
                                    let trimmed = line.text.trim();
                                    if trimmed.is_empty() {
                                        ui.add_space(10.0);
                                    } else {
                                        ui.label(
                                            RichText::new(&line.text)
                                                .size(15.5)
                                                .color(Color32::from_rgb(226, 232, 240)),
                                        );
                                        ui.add_space(3.0);
                                    }
                                }
                            });
                        });
                }
            } else {
                ui.vertical_centered(|ui| {
                    ui.add_space(60.0);
                    icons::icon_label(ui, Icon::Lyrics, 48.0, Color32::from_gray(120));
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new("No Lyrics Loaded")
                            .strong()
                            .size(16.0)
                            .color(Color32::from_gray(200)),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(
                            "Muvrix automatically extracts lyrics from embedded container tags (ID3v2 USLT, Vorbis comments, MP4 ©lyr)\nas well as adjacent .lrc and .txt sidecar files.",
                        )
                        .size(13.0)
                        .color(Color32::from_gray(150)),
                    );
                    ui.add_space(16.0);
                    ui.horizontal(|ui| {
                        ui.add_space(ui.available_width() * 0.35);
                        if ui
                            .button(RichText::new("📂 Load Lyrics File...").strong())
                            .clicked()
                        {
                            self.load_lyrics_dialog();
                        }
                        if ui.button("🔍 Check Embedded").clicked() {
                            self.reload_lyrics_for_current_media();
                        }
                    });
                });
            }
        });
    }
}
