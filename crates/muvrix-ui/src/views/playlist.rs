use eframe::egui::{self, Color32, RichText, Stroke, Vec2};

use crate::app::MuvrixApp;
use crate::branding::BrandPalette;
use crate::icons::{self, Icon};
use crate::ui_components;
use crate::views::AppView;

impl MuvrixApp {
    fn render_saved_playlists(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.label(
                RichText::new("Saved Playlists")
                    .strong()
                    .size(14.0)
                    .color(Color32::from_rgb(95, 185, 255)),
            );
            let playlists = self.db.list_playlists().unwrap_or_default();
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.playlist_name_input)
                        .hint_text("New playlist name")
                        .desired_width(180.0),
                );
                if ui.button("Create").clicked() && !self.playlist_name_input.trim().is_empty() {
                    match self.db.create_playlist(self.playlist_name_input.trim()) {
                        Ok(id) => {
                            self.selected_persistent_playlist = Some(id);
                            self.playlist_name_input.clear();
                            self.playlist_status = Some("Playlist created".to_string());
                        }
                        Err(e) => self.playlist_status = Some(format!("Create failed: {e}")),
                    }
                }
                ui.separator();
                if ui.button("Import .m3u/.m3u8...").clicked() {
                    if let Some(file) = rfd::FileDialog::new()
                        .add_filter("M3U Playlist", &["m3u", "m3u8"])
                        .pick_file()
                    {
                        self.import_m3u(&file.to_string_lossy());
                    }
                }
                // Phase C4: M3U8 Export
                if let Some(id) = self.selected_persistent_playlist {
                    if ui
                        .button("Export .m3u8...")
                        .on_hover_text("Export active playlist to M3U8 file")
                        .clicked()
                    {
                        self.show_export_playlist_modal = true;
                        self.export_playlist_target_id = Some(id);
                        let default_name = playlists
                            .iter()
                            .find(|p| p.id == id)
                            .map(|p| format!("{}.m3u8", p.name.replace(' ', "_")))
                            .unwrap_or_else(|| format!("playlist_{id}.m3u8"));
                        self.export_playlist_path_input = default_name;
                        self.export_playlist_status = None;
                    }
                }
                ui.add(
                    egui::TextEdit::singleline(&mut self.playlist_import_input)
                        .hint_text("M3U URL")
                        .desired_width(200.0),
                );
                if ui.button("Import URL").clicked()
                    && !self.playlist_import_input.trim().is_empty()
                {
                    let source = self.playlist_import_input.trim().to_string();
                    self.import_m3u(&source);
                }
            });

            if playlists.is_empty() {
                ui.label(
                    RichText::new("No saved playlists yet.")
                        .italics()
                        .size(11.5)
                        .color(Color32::from_gray(140)),
                );
            } else {
                ui.horizontal_wrapped(|ui| {
                    for playlist in &playlists {
                        let selected = self.selected_persistent_playlist == Some(playlist.id);
                        if ui
                            .selectable_label(
                                selected,
                                format!("{} ({})", playlist.name, playlist.item_count),
                            )
                            .clicked()
                        {
                            self.selected_persistent_playlist = Some(playlist.id);
                        }
                    }
                });
            }

            if let Some(id) = self.selected_persistent_playlist {
                ui.horizontal(|ui| {
                    if ui.button("▶ Play All").clicked() {
                        match self.play_saved_playlist(id) {
                            Ok(()) => {
                                self.playlist_status = Some("Playing saved playlist".to_string())
                            }
                            Err(e) => self.playlist_status = Some(e),
                        }
                    }
                    if let Some(item) = self.selected_library_item.clone() {
                        if ui.button("Add Selected Library Item").clicked() {
                            self.add_to_selected_playlist(
                                item.item.canonical_path.clone(),
                                item.metadata.as_ref().and_then(|m| m.title.clone()),
                                item.metadata.as_ref().map(|m| m.duration_ms).unwrap_or(0),
                            );
                        }
                    }
                    // Phase C3: Rename Playlist
                    if ui.button("Rename").clicked() {
                        if let Some(pl) = playlists.iter().find(|p| p.id == id) {
                            self.playlist_rename_id = Some(id);
                            self.playlist_rename_input = pl.name.clone();
                        }
                    }
                    // Phase C3: Clear with confirmation
                    if ui.button("Clear").clicked() {
                        self.playlist_confirm_clear = Some(id);
                    }
                    // Phase C3: Delete with confirmation
                    if ui.button("Delete Playlist").clicked() {
                        self.playlist_confirm_delete = Some(id);
                    }
                });

                let entries = self.db.list_playlist_entries(id).unwrap_or_default();
                let mut remove = None;
                let mut move_up = None;
                let mut move_down = None;
                for entry in &entries {
                    ui.horizontal(|ui| {
                        let label = entry.title.as_deref().unwrap_or(entry.path.as_str());
                        if ui.link(RichText::new(label).size(12.0)).clicked() {
                            self.load_file(&entry.path);
                            self.current_view = AppView::Player;
                        }
                        ui.label(
                            RichText::new(&entry.path)
                                .monospace()
                                .size(10.0)
                                .color(Color32::from_gray(140)),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("✖").clicked() {
                                remove = Some(entry.id);
                            }
                            if ui.button("▼").clicked() {
                                move_down = Some(entry.id);
                            }
                            if ui.button("▲").clicked() {
                                move_up = Some(entry.id);
                            }
                        });
                    });
                }
                if let Some(entry_id) = remove {
                    let _ = self.db.remove_playlist_entry(id, entry_id);
                }
                if let Some(entry_id) = move_up {
                    if let Some(pos) = entries.iter().position(|entry| entry.id == entry_id) {
                        let _ = self
                            .db
                            .reorder_playlist_entry(id, entry_id, pos.saturating_sub(1));
                    }
                }
                if let Some(entry_id) = move_down {
                    if let Some(pos) = entries.iter().position(|entry| entry.id == entry_id) {
                        let _ = self.db.reorder_playlist_entry(id, entry_id, pos + 1);
                    }
                }
            }

            if let Some(status) = &self.playlist_status {
                ui.label(
                    RichText::new(status)
                        .size(11.0)
                        .color(Color32::from_rgb(125, 211, 252)),
                );
            }

            // Phase C3: Playlist Rename Dialog
            if let Some(id) = self.playlist_rename_id {
                let mut close_dialog = false;
                let mut is_open = true;
                egui::Window::new("Rename Playlist")
                    .collapsible(false)
                    .resizable(false)
                    .default_width(300.0)
                    .open(&mut is_open)
                    .show(ui.ctx(), |ui| {
                        ui.label("Enter new name for playlist:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.playlist_rename_input)
                                .desired_width(260.0),
                        );
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("Save").clicked()
                                && !self.playlist_rename_input.trim().is_empty()
                            {
                                let _ = self
                                    .db
                                    .rename_playlist(id, self.playlist_rename_input.trim());
                                self.playlist_status = Some("Playlist renamed".to_string());
                                close_dialog = true;
                            }
                            if ui.button("Cancel").clicked() {
                                close_dialog = true;
                            }
                        });
                    });
                if close_dialog || !is_open {
                    self.playlist_rename_id = None;
                }
            }

            // Phase C3: Playlist Confirm Delete Dialog
            if let Some(id) = self.playlist_confirm_delete {
                let mut close_dialog = false;
                let mut is_open = true;
                egui::Window::new("Confirm Delete Playlist")
                    .collapsible(false)
                    .resizable(false)
                    .default_width(320.0)
                    .open(&mut is_open)
                    .show(ui.ctx(), |ui| {
                        ui.label(
                            RichText::new("Are you sure you want to delete this playlist?")
                                .strong()
                                .color(Color32::from_rgb(255, 100, 100)),
                        );
                        ui.label("This action cannot be undone.");
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if ui
                                .button(
                                    RichText::new("Delete")
                                        .color(Color32::from_rgb(255, 80, 80))
                                        .strong(),
                                )
                                .clicked()
                            {
                                let _ = self.db.delete_playlist(id);
                                self.selected_persistent_playlist = None;
                                self.playlist_status = Some("Playlist deleted".to_string());
                                close_dialog = true;
                            }
                            if ui.button("Cancel").clicked() {
                                close_dialog = true;
                            }
                        });
                    });
                if close_dialog || !is_open {
                    self.playlist_confirm_delete = None;
                }
            }

            // Phase C3: Playlist Confirm Clear Dialog
            if let Some(id) = self.playlist_confirm_clear {
                let mut close_dialog = false;
                let mut is_open = true;
                egui::Window::new("Confirm Clear Playlist")
                    .collapsible(false)
                    .resizable(false)
                    .default_width(320.0)
                    .open(&mut is_open)
                    .show(ui.ctx(), |ui| {
                        ui.label(
                            RichText::new(
                                "Are you sure you want to remove all items from this playlist?",
                            )
                            .strong()
                            .color(Color32::from_rgb(255, 180, 50)),
                        );
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if ui
                                .button(
                                    RichText::new("Clear All Items")
                                        .color(Color32::from_rgb(255, 100, 100))
                                        .strong(),
                                )
                                .clicked()
                            {
                                let _ = self.db.clear_playlist(id);
                                self.playlist_status = Some("Playlist cleared".to_string());
                                close_dialog = true;
                            }
                            if ui.button("Cancel").clicked() {
                                close_dialog = true;
                            }
                        });
                    });
                if close_dialog || !is_open {
                    self.playlist_confirm_clear = None;
                }
            }

            // Phase C4: Export Playlist Dialog (active queue or saved playlist)
            if self.show_export_playlist_modal {
                let mut close_dialog = false;
                let mut is_open = true;
                let title = if self.export_playlist_target_id.is_some() {
                    "Export Saved Playlist"
                } else {
                    "Export Playback Queue"
                };
                egui::Window::new(title)
                    .collapsible(false)
                    .resizable(false)
                    .default_width(380.0)
                    .open(&mut is_open)
                    .show(ui.ctx(), |ui| {
                        ui.label(
                            RichText::new("Export Playlist to File (.m3u / .m3u8):")
                                .strong()
                                .color(BrandPalette::PRIMARY_TEXT),
                        );
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.export_playlist_path_input)
                                    .hint_text("Path to output .m3u or .m3u8 file")
                                    .desired_width(260.0),
                            );
                            if ui.button("Browse...").clicked() {
                                if let Some(file) = rfd::FileDialog::new()
                                    .set_title("Export Playlist Destination")
                                    .add_filter("M3U8 Playlist", &["m3u8", "m3u"])
                                    .save_file()
                                {
                                    self.export_playlist_path_input =
                                        file.to_string_lossy().to_string();
                                }
                            }
                        });
                        ui.add_space(8.0);
                        if let Some(ref status) = self.export_playlist_status {
                            ui.label(
                                RichText::new(status)
                                    .size(12.0)
                                    .color(BrandPalette::ELECTRIC_LIME),
                            );
                            ui.add_space(6.0);
                        }
                        ui.horizontal(|ui| {
                            if ui
                                .button(
                                    RichText::new("Export Now")
                                        .color(BrandPalette::OBSIDIAN)
                                        .strong(),
                                )
                                .clicked()
                            {
                                let dest = self.export_playlist_path_input.trim();
                                if !dest.is_empty() {
                                    let m3u_entries: Vec<muvrix_core::playlist::M3uEntry> =
                                        if let Some(id) = self.export_playlist_target_id {
                                            self.db
                                                .list_playlist_entries(id)
                                                .unwrap_or_default()
                                                .into_iter()
                                                .map(|e| muvrix_core::playlist::M3uEntry {
                                                    path: e.path,
                                                    title: e.title,
                                                    duration_ms: e.duration_ms,
                                                    logo: None,
                                                    group: None,
                                                })
                                                .collect()
                                        } else {
                                            self.controller
                                                .playlist()
                                                .iter()
                                                .map(|e| muvrix_core::playlist::M3uEntry {
                                                    path: e.filename.clone(),
                                                    title: e.title.clone(),
                                                    duration_ms: 0,
                                                    logo: None,
                                                    group: None,
                                                })
                                                .collect()
                                        };
                                    match muvrix_core::playlist::export_m3u(&m3u_entries, dest) {
                                        Ok(()) => {
                                            self.export_playlist_status = Some(format!(
                                                "Successfully exported {} entries to {}",
                                                m3u_entries.len(),
                                                dest
                                            ));
                                            self.playlist_status =
                                                Some(format!("Exported playlist to {dest}"));
                                        }
                                        Err(e) => {
                                            self.export_playlist_status =
                                                Some(format!("Export failed: {e}"));
                                        }
                                    }
                                }
                            }
                            if ui.button("Close").clicked() {
                                close_dialog = true;
                            }
                        });
                    });
                if close_dialog || !is_open {
                    self.show_export_playlist_modal = false;
                }
            }
        });
        ui.add_space(6.0);
    }

    fn import_m3u(&mut self, source: &str) {
        let playlist_id = match self.selected_persistent_playlist {
            Some(id) => id,
            None => {
                let name = std::path::Path::new(source)
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .filter(|stem| !stem.is_empty())
                    .unwrap_or("Imported Playlist")
                    .to_string();
                match self.db.create_playlist(&name) {
                    Ok(id) => {
                        self.selected_persistent_playlist = Some(id);
                        id
                    }
                    Err(e) => {
                        self.playlist_status = Some(format!("Create failed: {e}"));
                        return;
                    }
                }
            }
        };
        match muvrix_core::playlist::load_m3u(source) {
            Ok(entries) => {
                for entry in entries {
                    let _ = self.db.add_playlist_entry(
                        playlist_id,
                        &entry.path,
                        entry.title.as_deref(),
                        entry.duration_ms,
                    );
                }
                self.playlist_status = Some(format!("Imported playlist from {source}"));
            }
            Err(e) => self.playlist_status = Some(format!("Import failed: {e}")),
        }
    }

    pub fn render_playlist_view(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(4.0);
            self.render_saved_playlists(ui);

            let playlist_count = self.controller.playlist_count();
            let current_pos = self.controller.playlist_pos();

            // Header Toolbar
            ui.horizontal(|ui| {
                icons::icon_label(ui, Icon::Playlist, 18.0, Color32::from_rgb(85, 175, 255));
                ui.label(
                    RichText::new(format!("Playback Queue ({playlist_count} items)"))
                        .strong()
                        .size(16.0),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icons::icon_text_button(
                        ui,
                        Icon::FolderOpen,
                        "Add Files...",
                        "Pick media files to append to queue",
                    )
                    .clicked()
                    {
                        if let Some(files) = rfd::FileDialog::new()
                            .set_title("Add Files to Playlist")
                            .pick_files()
                        {
                            for f in files {
                                let path_str = f.to_string_lossy().to_string();
                                let _ = self.controller.playlist_append(&path_str);
                            }
                        }
                    }

                    if icons::icon_text_button(
                        ui,
                        Icon::Shuffle,
                        "Shuffle",
                        "Randomize playback queue order",
                    )
                    .clicked()
                    {
                        let _ = self.controller.playlist_shuffle();
                    }

                    if icons::icon_text_button(
                        ui,
                        Icon::Playlist,
                        "Export Queue...",
                        "Export active playback queue to M3U / M3U8 playlist file",
                    )
                    .clicked()
                    {
                        self.show_export_playlist_modal = true;
                        self.export_playlist_target_id = None;
                        self.export_playlist_path_input = "active_queue.m3u8".to_string();
                        self.export_playlist_status = None;
                    }

                    if icons::icon_text_button(
                        ui,
                        Icon::Close,
                        "Clear Queue",
                        "Remove all queued items",
                    )
                    .clicked()
                    {
                        let _ = self.controller.playlist_clear();
                    }
                });
            });

            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);

            if playlist_count == 0 {
                ui.add_space(20.0);
                if ui_components::empty_state_box(
                    ui,
                    Some(Icon::Playlist),
                    "Your playback queue is empty",
                    "Use the Media Browser or Open File to enqueue videos, or drag files directly into the window.",
                    Some("Browse Media Files..."),
                ) == Some(true)
                {
                    self.current_view = AppView::Browser;
                }
                return;
            }

            let playlist_items = self.controller.playlist().to_vec();
            let mut play_idx: Option<usize> = None;
            let mut remove_idx: Option<usize> = None;
            let mut move_up_idx: Option<usize> = None;
            let mut move_down_idx: Option<usize> = None;

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (idx, item) in playlist_items.iter().enumerate() {
                        let is_active = current_pos == Some(idx) || item.is_playing || item.is_current;

                        let frame_color = if is_active {
                            BrandPalette::CARD_SURFACE
                        } else if idx % 2 == 0 {
                            Color32::from_rgba_premultiplied(24, 35, 44, 180)
                        } else {
                            Color32::from_rgba_premultiplied(17, 26, 34, 180)
                        };

                        let border_stroke = if is_active {
                            Stroke::new(1.0_f32, BrandPalette::ELECTRIC_LIME)
                        } else {
                            Stroke::NONE
                        };

                        egui::Frame::NONE
                            .fill(frame_color)
                            .stroke(border_stroke)
                            .corner_radius(4.0)
                            .inner_margin(egui::Margin::symmetric(10, 6))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    // Index and Play Indicator
                                    if is_active {
                                        icons::icon_label(
                                            ui,
                                            Icon::Play,
                                            14.0,
                                            Color32::from_rgb(80, 190, 255),
                                        );
                                    } else {
                                        ui.label(
                                            RichText::new(format!("{:02}", idx + 1))
                                                .monospace()
                                                .color(Color32::from_gray(130)),
                                        );
                                    }

                                    ui.add_space(6.0);

                                    // Item Title & Path
                                    let filename = std::path::Path::new(&item.filename)
                                        .file_name()
                                        .and_then(|n| n.to_str())
                                        .unwrap_or(&item.filename);
                                    let title = item.title.as_deref().unwrap_or(filename);

                                    let title_color = if is_active {
                                        Color32::WHITE
                                    } else {
                                        Color32::from_gray(220)
                                    };

                                    let title_btn = ui.link(
                                        RichText::new(title)
                                            .strong()
                                            .color(title_color),
                                    );
                                    if title_btn.clicked() {
                                        play_idx = Some(idx);
                                    }

                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if icons::icon_button(
                                                ui,
                                                Icon::Close,
                                                "Remove from queue",
                                            )
                                            .clicked()
                                            {
                                                remove_idx = Some(idx);
                                            }

                                            if idx + 1 < playlist_count
                                                && ui
                                                    .button("▼")
                                                    .on_hover_text("Move down in queue")
                                                    .clicked()
                                            {
                                                move_down_idx = Some(idx);
                                            }

                                            if idx > 0
                                                && ui
                                                    .button("▲")
                                                    .on_hover_text("Move up in queue")
                                                    .clicked()
                                            {
                                                move_up_idx = Some(idx);
                                            }

                                            if !is_active
                                                && icons::icon_text_button(
                                                    ui,
                                                    Icon::Play,
                                                    "Play",
                                                    "Jump to this queued item",
                                                )
                                                .clicked()
                                            {
                                                play_idx = Some(idx);
                                            }
                                        },
                                    );
                                });
                            });
                        ui.add_space(2.0);
                    }
                });

            if let Some(idx) = play_idx {
                let _ = self.controller.playlist_play_index(idx);
                self.current_view = AppView::Player;
            }
            if let Some(idx) = remove_idx {
                let _ = self.controller.playlist_remove(idx);
            }
            if let Some(idx) = move_up_idx {
                let _ = self.controller.playlist_move(idx, idx - 1);
            }
            if let Some(idx) = move_down_idx {
                let _ = self.controller.playlist_move(idx, idx + 2);
            }
        });
    }

    // -------------------------------------------------------------------------
    // Collapsible Right Playlist Drawer (in Player View)
    // -------------------------------------------------------------------------

    pub fn render_playlist_drawer(&mut self, ctx: &egui::Context) {
        if !self.show_playlist_drawer {
            return;
        }

        egui::SidePanel::right("playlist_side_drawer")
            .resizable(false)
            .exact_width(280.0)
            .show(ctx, |ui| {
                ui.add_space(4.0);

                let playlist_count = self.controller.playlist_count();
                let current_pos = self.controller.playlist_pos();

                ui.horizontal(|ui| {
                    icons::icon_label(ui, Icon::Playlist, 16.0, Color32::from_rgb(85, 175, 255));
                    ui.label(RichText::new(format!("Queue ({playlist_count})")).strong());

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icons::icon_button(ui, Icon::Close, "Close queue drawer (P)").clicked() {
                            self.show_playlist_drawer = false;
                        }
                        if icons::icon_button(ui, Icon::Shuffle, "Shuffle queue").clicked() {
                            let _ = self.controller.playlist_shuffle();
                        }
                    });
                });

                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);

                if playlist_count == 0 {
                    ui.add_space(20.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("Queue is empty")
                                .size(13.0)
                                .color(Color32::from_gray(160)),
                        );
                        ui.add_space(8.0);
                        if ui.button("Add Files...").clicked() {
                            self.open_file_dialog();
                        }
                    });
                    return;
                }

                let playlist_items = self.controller.playlist().to_vec();
                let mut play_idx: Option<usize> = None;
                let mut remove_idx: Option<usize> = None;

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (idx, item) in playlist_items.iter().enumerate() {
                            let is_active =
                                current_pos == Some(idx) || item.is_playing || item.is_current;

                            let bg_color = if is_active {
                                Color32::from_rgba_premultiplied(30, 60, 110, 240)
                            } else {
                                Color32::from_rgba_premultiplied(22, 25, 32, 180)
                            };

                            egui::Frame::NONE
                                .fill(bg_color)
                                .corner_radius(3.0)
                                .inner_margin(egui::Margin::symmetric(6, 4))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        if is_active {
                                            icons::icon_label(
                                                ui,
                                                Icon::Play,
                                                12.0,
                                                Color32::from_rgb(80, 190, 255),
                                            );
                                        } else {
                                            ui.label(
                                                RichText::new(format!("{}", idx + 1))
                                                    .monospace()
                                                    .size(11.0)
                                                    .color(Color32::from_gray(140)),
                                            );
                                        }

                                        let filename = std::path::Path::new(&item.filename)
                                            .file_name()
                                            .and_then(|n| n.to_str())
                                            .unwrap_or(&item.filename);

                                        let text_color = if is_active {
                                            Color32::WHITE
                                        } else {
                                            Color32::from_gray(215)
                                        };

                                        let btn = ui.link(
                                            RichText::new(filename).size(11.5).color(text_color),
                                        );
                                        if btn.clicked() {
                                            play_idx = Some(idx);
                                        }

                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if icons::icon_button_sized(
                                                    ui,
                                                    Icon::Close,
                                                    Vec2::splat(18.0),
                                                    "Remove",
                                                )
                                                .clicked()
                                                {
                                                    remove_idx = Some(idx);
                                                }
                                            },
                                        );
                                    });
                                });
                            ui.add_space(2.0);
                        }
                    });

                if let Some(idx) = play_idx {
                    let _ = self.controller.playlist_play_index(idx);
                }
                if let Some(idx) = remove_idx {
                    let _ = self.controller.playlist_remove(idx);
                }
            });
    }

    // -------------------------------------------------------------------------
}
