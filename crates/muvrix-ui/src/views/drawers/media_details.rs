use eframe::egui::{self, Color32, RichText};
use muvrix_player::SeekMode;

use crate::app::{format_timestamp, MuvrixApp};
use crate::branding::BrandPalette;
use crate::ui_components;
use crate::views::{format_file_size, AppView};

impl MuvrixApp {
    fn aspect_ratio_str(w: u32, h: u32) -> String {
        if w == 0 || h == 0 {
            return "N/A".to_string();
        }
        let ratio = w as f64 / h as f64;
        if (ratio - 16.0 / 9.0).abs() < 0.05 {
            "16:9 (Widescreen)".to_string()
        } else if (ratio - 21.0 / 9.0).abs() < 0.08 || (ratio - 2.39).abs() < 0.08 {
            "21:9 (Cinemascope 2.39:1)".to_string()
        } else if (ratio - 4.0 / 3.0).abs() < 0.05 {
            "4:3 (Standard)".to_string()
        } else {
            format!("{:.2}:1", ratio)
        }
    }

    fn audio_layout_str(channels: u32) -> String {
        match channels {
            1 => "1.0 (Mono)".to_string(),
            2 => "2.0 (Stereo)".to_string(),
            6 => "5.1 Surround".to_string(),
            8 => "7.1 Surround".to_string(),
            n if n > 0 => format!("{n} Channels"),
            _ => "2.0 (Stereo)".to_string(),
        }
    }

    /// Renders the Media Details Drawer.
    pub(crate) fn render_library_details_drawer(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        let mut close_drawer = false;
        let mut play_path: Option<String> = None;
        let mut enqueue_path: Option<String> = None;
        let mut persistent_save: Option<(String, Option<String>, u64)> = None;
        let mut delete_bm_id: Option<i64> = None;
        let mut save_bm: Option<(i64, String, Option<String>)> = None;

        if let Some(ref item_view) = self.selected_library_item {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Media Details")
                        .strong()
                        .size(15.0)
                        .color(Color32::from_rgb(85, 175, 255)),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("✖").on_hover_text("Close details").clicked() {
                        close_drawer = true;
                    }
                });
            });

            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let title = item_view
                        .metadata
                        .as_ref()
                        .and_then(|m| m.title.as_ref())
                        .unwrap_or(&item_view.item.file_name);
                    ui.label(RichText::new(title).strong().size(14.0));

                    ui.add_space(6.0);

                    // Actions
                    ui.horizontal(|ui| {
                        if ui.button(RichText::new("▶ Play Now").strong()).clicked() {
                            play_path = Some(item_view.item.canonical_path.clone());
                        }
                        if ui.button("➕ Enqueue").clicked() {
                            enqueue_path = Some(item_view.item.canonical_path.clone());
                        }
                        if ui.button("💾 Save to Playlist").clicked() {
                            persistent_save = Some((
                                item_view.item.canonical_path.clone(),
                                item_view
                                    .metadata
                                    .as_ref()
                                    .and_then(|meta| meta.title.clone()),
                                item_view
                                    .metadata
                                    .as_ref()
                                    .map(|meta| meta.duration_ms)
                                    .unwrap_or(0),
                            ));
                        }
                    });

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(4.0);

                    // Section 1: Container & File
                    ui_components::card_frame().show(ui, |ui| {
                        ui.label(
                            RichText::new("Container & File")
                                .strong()
                                .size(13.0)
                                .color(BrandPalette::ELECTRIC_LIME),
                        );
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("File Name:").color(BrandPalette::SECONDARY_TEXT),
                            );
                            ui.label(
                                RichText::new(&item_view.item.file_name)
                                    .strong()
                                    .color(BrandPalette::PRIMARY_TEXT),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Container:").color(BrandPalette::SECONDARY_TEXT),
                            );
                            let c = item_view
                                .metadata
                                .as_ref()
                                .and_then(|m| m.container.as_deref())
                                .unwrap_or("Matroska / MP4");
                            ui.label(RichText::new(c).color(BrandPalette::PRIMARY_TEXT));
                        });
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Size:").color(BrandPalette::SECONDARY_TEXT));
                            ui.label(
                                RichText::new(format_file_size(item_view.item.file_size))
                                    .color(BrandPalette::PRIMARY_TEXT),
                            );
                        });
                        if let Some(ref meta) = item_view.metadata {
                            if meta.duration_ms > 0 {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("Duration:")
                                            .color(BrandPalette::SECONDARY_TEXT),
                                    );
                                    ui.label(
                                        RichText::new(format_timestamp(
                                            meta.duration_ms as f64 / 1000.0,
                                        ))
                                        .color(BrandPalette::PRIMARY_TEXT),
                                    );
                                });
                            }
                            if meta.overall_bitrate > 0 {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("Overall Bitrate:")
                                            .color(BrandPalette::SECONDARY_TEXT),
                                    );
                                    ui.label(
                                        RichText::new(format!(
                                            "{} kbps",
                                            meta.overall_bitrate / 1000
                                        ))
                                        .color(BrandPalette::PRIMARY_TEXT),
                                    );
                                });
                            }
                        }
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Location:").color(BrandPalette::SECONDARY_TEXT),
                            );
                            ui.label(
                                RichText::new(&item_view.item.canonical_path)
                                    .size(10.5)
                                    .color(BrandPalette::SECONDARY_TEXT),
                            );
                            if ui
                                .button("📋")
                                .on_hover_text("Copy path to clipboard")
                                .clicked()
                            {
                                ui.ctx().copy_text(item_view.item.canonical_path.clone());
                            }
                        });
                    });

                    ui.add_space(8.0);

                    // Section 2: Video Stream Information
                    ui_components::card_frame().show(ui, |ui| {
                        ui.label(
                            RichText::new("Video Stream")
                                .strong()
                                .size(13.0)
                                .color(BrandPalette::ELECTRIC_LIME),
                        );
                        ui.add_space(4.0);
                        if let Some(ref meta) = item_view.metadata {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Codec:").color(BrandPalette::SECONDARY_TEXT),
                                );
                                let v_codec = meta.video_codec.as_deref().unwrap_or("H.264 / AVC");
                                ui.label(
                                    RichText::new(v_codec.to_uppercase())
                                        .strong()
                                        .color(BrandPalette::PRIMARY_TEXT),
                                );
                                ui.label(
                                    RichText::new("(Stream #0:0)")
                                        .size(11.0)
                                        .color(BrandPalette::MUTED_TEXT),
                                );
                            });
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Resolution:")
                                        .color(BrandPalette::SECONDARY_TEXT),
                                );
                                let w = if meta.width > 0 { meta.width } else { 1920 };
                                let h = if meta.height > 0 { meta.height } else { 1080 };
                                ui.label(
                                    RichText::new(format!("{}×{}", w, h))
                                        .color(BrandPalette::PRIMARY_TEXT),
                                );
                                ui.label(
                                    RichText::new(format!("({})", Self::aspect_ratio_str(w, h)))
                                        .size(11.0)
                                        .color(BrandPalette::MUTED_TEXT),
                                );
                            });
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Frame Rate:")
                                        .color(BrandPalette::SECONDARY_TEXT),
                                );
                                let fps = if meta.fps > 0.0 { meta.fps } else { 24.000 };
                                ui.label(
                                    RichText::new(format!("{:.3} fps", fps))
                                        .color(BrandPalette::PRIMARY_TEXT),
                                );
                            });
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Bit Depth:").color(BrandPalette::SECONDARY_TEXT),
                                );
                                let bd = if meta.bit_depth > 0 {
                                    meta.bit_depth
                                } else {
                                    8
                                };
                                ui.label(
                                    RichText::new(format!("{}-bit", bd))
                                        .color(BrandPalette::PRIMARY_TEXT),
                                );
                            });
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Dynamic Range:")
                                        .color(BrandPalette::SECONDARY_TEXT),
                                );
                                let hdr = meta.hdr_type.as_deref().unwrap_or("SDR (BT.709)");
                                ui_components::badge_pill(
                                    ui,
                                    hdr,
                                    BrandPalette::OBSIDIAN,
                                    if hdr.contains("HDR") {
                                        BrandPalette::ELECTRIC_LIME
                                    } else {
                                        BrandPalette::SECONDARY_TEXT
                                    },
                                );
                            });
                        } else {
                            ui.label(
                                RichText::new("No video stream metadata indexed.")
                                    .italics()
                                    .color(BrandPalette::MUTED_TEXT),
                            );
                        }
                    });

                    ui.add_space(8.0);

                    // Section 3: Audio Stream Information
                    ui_components::card_frame().show(ui, |ui| {
                        ui.label(
                            RichText::new("Audio Stream")
                                .strong()
                                .size(13.0)
                                .color(BrandPalette::ELECTRIC_LIME),
                        );
                        ui.add_space(4.0);
                        if let Some(ref meta) = item_view.metadata {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Codec:").color(BrandPalette::SECONDARY_TEXT),
                                );
                                let a_codec = meta.audio_codec.as_deref().unwrap_or("AAC");
                                ui.label(
                                    RichText::new(a_codec.to_uppercase())
                                        .strong()
                                        .color(BrandPalette::PRIMARY_TEXT),
                                );
                                ui.label(
                                    RichText::new("(Stream #0:1)")
                                        .size(11.0)
                                        .color(BrandPalette::MUTED_TEXT),
                                );
                            });
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Channels / Layout:")
                                        .color(BrandPalette::SECONDARY_TEXT),
                                );
                                let ch = if meta.audio_channels > 0 {
                                    meta.audio_channels
                                } else {
                                    2
                                };
                                ui.label(
                                    RichText::new(Self::audio_layout_str(ch))
                                        .color(BrandPalette::PRIMARY_TEXT),
                                );
                            });
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Sample Rate:")
                                        .color(BrandPalette::SECONDARY_TEXT),
                                );
                                let sr = if meta.sample_rate > 0 {
                                    meta.sample_rate
                                } else {
                                    48000
                                };
                                ui.label(
                                    RichText::new(format!("{sr} Hz"))
                                        .color(BrandPalette::PRIMARY_TEXT),
                                );
                            });
                        } else {
                            ui.label(
                                RichText::new("No audio stream metadata indexed.")
                                    .italics()
                                    .color(BrandPalette::MUTED_TEXT),
                            );
                        }
                    });

                    // Section 4: Tracks & Subtitles
                    let tracks = self.db.get_tracks(item_view.item.id).unwrap_or_default();
                    if !tracks.is_empty() {
                        ui.add_space(8.0);
                        ui_components::card_frame().show(ui, |ui| {
                            ui.label(
                                RichText::new(format!("Embedded Tracks ({})", tracks.len()))
                                    .strong()
                                    .size(13.0)
                                    .color(BrandPalette::ELECTRIC_LIME),
                            );
                            ui.add_space(4.0);
                            for t in &tracks {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!("#{}", t.track_index))
                                            .monospace()
                                            .color(BrandPalette::SECONDARY_TEXT),
                                    );
                                    ui_components::badge_pill(
                                        ui,
                                        t.track_type.as_str(),
                                        BrandPalette::OBSIDIAN,
                                        BrandPalette::PRIMARY_TEXT,
                                    );
                                    if let Some(ref codec) = t.codec {
                                        ui.label(
                                            RichText::new(codec)
                                                .strong()
                                                .color(BrandPalette::PRIMARY_TEXT),
                                        );
                                    }
                                    if let Some(ref lang) = t.language {
                                        ui.label(
                                            RichText::new(format!("[{lang}]"))
                                                .color(BrandPalette::MUTED_TEXT),
                                        );
                                    }
                                    if t.is_default {
                                        ui_components::badge_pill(
                                            ui,
                                            "DEFAULT",
                                            BrandPalette::OBSIDIAN,
                                            BrandPalette::ELECTRIC_LIME,
                                        );
                                    }
                                });
                            }
                        });
                    }

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(4.0);

                    // Tags
                    ui.label(
                        RichText::new("Tags")
                            .strong()
                            .size(12.5)
                            .color(Color32::from_gray(210)),
                    );
                    if item_view.tags.is_empty() {
                        ui.label(
                            RichText::new("No tags attached")
                                .italics()
                                .size(11.0)
                                .color(Color32::from_gray(140)),
                        );
                    } else {
                        ui.horizontal_wrapped(|ui| {
                            for tag in &item_view.tags {
                                ui.label(
                                    RichText::new(format!("#{}", tag))
                                        .color(Color32::from_rgb(100, 180, 255))
                                        .size(11.5),
                                );
                            }
                        });
                    }

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(4.0);

                    // User Bookmarks
                    ui.label(
                        RichText::new("Saved Bookmarks")
                            .strong()
                            .size(12.5)
                            .color(Color32::from_gray(210)),
                    );
                    let bookmarks = self
                        .db
                        .list_bookmarks_for_media(item_view.item.id)
                        .unwrap_or_default();
                    if bookmarks.is_empty() {
                        ui.label(
                            RichText::new("No bookmarks saved for this media item.")
                                .italics()
                                .size(11.0)
                                .color(Color32::from_gray(140)),
                        );
                    } else {
                        for bm in &bookmarks {
                            if self.editing_bookmark_id == Some(bm.id) {
                                ui.group(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(
                                            RichText::new("Edit Bookmark")
                                                .strong()
                                                .size(11.5)
                                                .color(Color32::from_rgb(245, 158, 11)),
                                        );
                                        ui.horizontal(|ui| {
                                            ui.label("Label:");
                                            ui.text_edit_singleline(
                                                &mut self.editing_bookmark_label,
                                            );
                                        });
                                        ui.horizontal(|ui| {
                                            ui.label("Note: ");
                                            ui.text_edit_singleline(
                                                &mut self.editing_bookmark_note,
                                            );
                                        });
                                        ui.horizontal(|ui| {
                                            if ui.button("Save").clicked() {
                                                let note_opt =
                                                    if self.editing_bookmark_note.trim().is_empty()
                                                    {
                                                        None
                                                    } else {
                                                        Some(
                                                            self.editing_bookmark_note
                                                                .trim()
                                                                .to_string(),
                                                        )
                                                    };
                                                save_bm = Some((
                                                    bm.id,
                                                    self.editing_bookmark_label.trim().to_string(),
                                                    note_opt,
                                                ));
                                                self.editing_bookmark_id = None;
                                            }
                                            if ui.button("Cancel").clicked() {
                                                self.editing_bookmark_id = None;
                                            }
                                        });
                                    });
                                });
                            } else {
                                ui.group(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.vertical(|ui| {
                                            ui.label(RichText::new(&bm.label).strong().size(12.0));
                                            let time_str = format_timestamp(bm.timestamp_seconds());
                                            ui.label(
                                                RichText::new(time_str)
                                                    .monospace()
                                                    .size(11.0)
                                                    .color(Color32::from_rgb(245, 158, 11)),
                                            );
                                            if let Some(ref note) = bm.note {
                                                ui.label(
                                                    RichText::new(note)
                                                        .size(10.5)
                                                        .color(Color32::from_gray(160)),
                                                );
                                            }
                                        });
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if ui
                                                    .button("✖")
                                                    .on_hover_text("Delete bookmark")
                                                    .clicked()
                                                {
                                                    delete_bm_id = Some(bm.id);
                                                }
                                                if ui
                                                    .button("✏")
                                                    .on_hover_text("Edit label and note")
                                                    .clicked()
                                                {
                                                    self.editing_bookmark_id = Some(bm.id);
                                                    self.editing_bookmark_label = bm.label.clone();
                                                    self.editing_bookmark_note =
                                                        bm.note.clone().unwrap_or_default();
                                                }
                                                if ui.button("▶ Jump").clicked() {
                                                    play_path =
                                                        Some(item_view.item.canonical_path.clone());
                                                    let _ = self.controller.seek(
                                                        bm.timestamp_seconds(),
                                                        SeekMode::Absolute,
                                                    );
                                                }
                                            },
                                        );
                                    });
                                });
                            }
                            ui.add_space(2.0);
                        }
                    }
                });
        }

        if let Some(id) = delete_bm_id {
            let _ = self.db.delete_bookmark(id);
            self.refresh_active_bookmarks();
        }
        if let Some((id, label, note)) = save_bm {
            let _ = self.db.update_bookmark(id, &label, note.as_deref());
            self.refresh_active_bookmarks();
        }

        if close_drawer {
            self.selected_library_item = None;
        }
        if let Some(path) = play_path {
            self.load_file(&path);
            self.current_view = AppView::Player;
        }
        if let Some(path) = enqueue_path {
            let _ = self.controller.playlist_append(&path);
            self.status_message = "Added item to playlist".to_string();
        }
        if let Some((path, title, duration_ms)) = persistent_save {
            self.add_to_selected_playlist(path, title, duration_ms);
        }
    }
}
