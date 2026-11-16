use eframe::egui::{self, Color32, RichText, Stroke, Vec2};
use muvrix_core::database::{
    LibraryItemView, LibraryRoot, LibrarySortField, MediaType, SortDirection,
};

use crate::app::{format_timestamp, MuvrixApp};
use crate::branding::BrandPalette;
use crate::icons::{self, Icon};
use crate::ui_components;
use crate::views::{format_file_size, format_timestamp_relative, AppView};

impl MuvrixApp {
    fn render_library_batch_bar(&mut self, ui: &mut egui::Ui) {
        let ids = self.library_selection.ids();
        let playlists = self.db.list_playlists().unwrap_or_default();
        ui.group(|ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(format!("Batch actions · {} selected", ids.len()))
                        .strong()
                        .color(Color32::from_rgb(125, 211, 252)),
                );
                if ui.button("Select All Page").clicked() {
                    let all = self.library_items.iter().map(|item| item.item.id);
                    self.library_selection.select_all(all);
                }
                if ui.button("Clear Selection").clicked() {
                    self.library_selection.clear();
                }
                ui.separator();
                if playlists.is_empty() {
                    ui.label(RichText::new("No saved playlists").italics().small());
                } else {
                    egui::ComboBox::from_id_salt("batch_playlist_combo")
                        .selected_text(
                            self.selected_persistent_playlist
                                .and_then(|id| playlists.iter().find(|p| p.id == id))
                                .map(|p| p.name.clone())
                                .unwrap_or_else(|| "Choose playlist".to_string()),
                        )
                        .show_ui(ui, |ui| {
                            for playlist in &playlists {
                                if ui
                                    .selectable_label(
                                        self.selected_persistent_playlist == Some(playlist.id),
                                        format!("{} ({})", playlist.name, playlist.item_count),
                                    )
                                    .clicked()
                                {
                                    self.selected_persistent_playlist = Some(playlist.id);
                                }
                            }
                        });
                    if ui.button("Add Selected to Playlist").clicked() && !ids.is_empty() {
                        let target = self
                            .selected_persistent_playlist
                            .or_else(|| playlists.first().map(|playlist| playlist.id));
                        if let Some(target) = target {
                            for id in &ids {
                                if let Ok(Some(item)) = self.db.get_media_item_by_id(*id) {
                                    let metadata = self.db.get_metadata(*id).ok().flatten();
                                    let _ = self.db.add_playlist_entry(
                                        target,
                                        &item.canonical_path,
                                        metadata.as_ref().and_then(|m| m.title.as_deref()),
                                        metadata.as_ref().map(|m| m.duration_ms).unwrap_or(0),
                                    );
                                }
                            }
                            self.playlist_status = Some(format!(
                                "Added {} selected item(s) to saved playlist",
                                ids.len()
                            ));
                        }
                    }
                }
                ui.separator();
                if ui.button("Mark Watched").clicked() && !ids.is_empty() {
                    for id in &ids {
                        let _ = self.db.set_watched(*id, true);
                    }
                    self.library_selection.clear();
                    self.refresh_library();
                }
                if ui.button("Mark Unwatched").clicked() && !ids.is_empty() {
                    for id in &ids {
                        let _ = self.db.set_watched(*id, false);
                    }
                    self.library_selection.clear();
                    self.refresh_library();
                }
                ui.separator();
                if ui.button("✏️ Batch Edit Metadata").clicked() && !ids.is_empty() {
                    self.open_batch_metadata_editor();
                }
                ui.separator();
                if self.library_batch_confirm_delete {
                    ui.label(
                        RichText::new(format!("Remove {} selected item(s)?", ids.len()))
                            .color(Color32::from_rgb(251, 146, 60)),
                    );
                    if ui.button("Confirm Remove").clicked() && !ids.is_empty() {
                        for id in &ids {
                            let _ = self.db.delete_media_item(*id);
                        }
                        self.library_selection.clear();
                        self.library_batch_confirm_delete = false;
                        self.refresh_library();
                    }
                    if ui.button("Cancel").clicked() {
                        self.library_batch_confirm_delete = false;
                    }
                } else if ui.button("Remove Selected").clicked() && !ids.is_empty() {
                    self.library_batch_confirm_delete = true;
                }
            });
        });
        ui.add_space(4.0);
    }

    // -------------------------------------------------------------------------
    // MUVRIX Home Screen (Branding, Search, Continue Watching, Hub & Highlights)
    // -------------------------------------------------------------------------

    pub fn render_library_view(&mut self, ctx: &egui::Context) {
        // 1. Right-side details drawer if an item is selected
        if self.selected_library_item.is_some() {
            egui::SidePanel::right("library_details_drawer")
                .min_width(280.0)
                .max_width(360.0)
                .resizable(true)
                .show(ctx, |ui| {
                    self.render_library_details_drawer(ui);
                });
        }

        // 2. Library Roots Management Dialog
        if self.add_root_dialog_open {
            let mut close_dialog = false;
            let mut is_open = true;
            egui::Window::new("Manage Media Library Roots")
                .collapsible(false)
                .resizable(true)
                .default_width(520.0)
                .open(&mut is_open)
                .show(ctx, |ui| {
                    self.render_roots_dialog(ui, &mut close_dialog);
                });
            if close_dialog || !is_open {
                self.add_root_dialog_open = false;
            }
        }

        // 2b. Phase C1: Library Item Context Menu Dialog
        if self.show_library_context_menu {
            if let Some(ref item_view) = self.library_context_item {
                let mut close_dialog = false;
                let mut is_open = true;
                let title = item_view
                    .metadata
                    .as_ref()
                    .and_then(|m| m.title.as_deref())
                    .unwrap_or(&item_view.item.file_name);
                let modal_title = format!("Item Actions: {title}");
                let item_clone = item_view.clone();
                egui::Window::new(modal_title)
                    .collapsible(false)
                    .resizable(false)
                    .default_width(260.0)
                    .open(&mut is_open)
                    .show(ctx, |ui| {
                        ui.vertical(|ui| {
                            if ui.button("▶ Play Now").clicked() {
                                self.load_file(&item_clone.item.canonical_path);
                                self.current_view = AppView::Player;
                                close_dialog = true;
                            }
                            if ui.button("➕ Enqueue to Playlist").clicked() {
                                let _ = self
                                    .controller
                                    .playlist_append(&item_clone.item.canonical_path);
                                self.status_message =
                                    format!("Added to playlist: {}", item_clone.item.file_name);
                                close_dialog = true;
                            }
                            let fav_label = if item_clone.is_favourite {
                                "☆ Remove from Favourites"
                            } else {
                                "★ Add to Favourites"
                            };
                            if ui.button(fav_label).clicked() {
                                let _ = self
                                    .db
                                    .set_favourite(item_clone.item.id, !item_clone.is_favourite);
                                self.refresh_library();
                                close_dialog = true;
                            }
                            if ui.button("ℹ View Technical Details").clicked() {
                                self.selected_library_item = Some(item_clone.clone());
                                close_dialog = true;
                            }
                            if ui.button("📋 Copy File Path").clicked() {
                                ui.ctx().copy_text(item_clone.item.canonical_path.clone());
                                self.status_message = "File path copied to clipboard".to_string();
                                close_dialog = true;
                            }
                        });
                    });
                if close_dialog || !is_open {
                    self.show_library_context_menu = false;
                    self.library_context_item = None;
                }
            }
        }

        // 2c. Phase E3: Smart Collections Rule Builder Dialog
        if self.smart_collection_dialog_open {
            self.render_smart_collection_dialog(ctx);
        }

        // 2d. Phase E3: Batch Metadata Editor Dialog
        if self.batch_metadata_dialog_open {
            self.render_batch_metadata_dialog(ctx);
        }

        // 2e. Phase E3: Integrity Scanner & Catalog Cleanup Dialog
        if self.integrity_dialog_open {
            self.render_integrity_dialog(ctx);
        }

        // 3. Central Library Workspace Panel
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(4.0);

            // Action deferred intents
            let mut filter_changed = false;
            let mut play_path: Option<String> = None;
            let mut enqueue_path: Option<String> = None;
            let mut select_item: Option<LibraryItemView> = None;
            let mut toggle_fav: Option<(i64, bool)> = None;
            let mut trigger_scan_root: Option<i64> = None;
            let mut selection_toggle: Option<i64> = None;

            // Header Toolbar
            ui.horizontal(|ui| {
                icons::icon_label(ui, Icon::Library, 18.0, Color32::from_rgb(60, 160, 240));
                ui.label(
                    RichText::new("Media Library")
                        .strong()
                        .size(16.0)
                        .color(Color32::from_gray(230)),
                );
                ui.label(
                    RichText::new(format!("({} items)", self.library_total_count))
                        .size(12.0)
                        .color(Color32::from_gray(150)),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .selectable_label(self.library_multi_select, "Select")
                        .on_hover_text("Toggle multi-select batch actions")
                        .clicked()
                    {
                        self.library_multi_select = !self.library_multi_select;
                        self.library_batch_confirm_delete = false;
                        if !self.library_multi_select {
                            self.library_selection.clear();
                        }
                    }

                    // View Mode Toggle (Grid / List)
                    ui.group(|ui| {
                        if ui.selectable_label(self.library_is_grid, "Grid").clicked() {
                            self.library_is_grid = true;
                        }
                        if ui.selectable_label(!self.library_is_grid, "List").clicked() {
                            self.library_is_grid = false;
                        }
                    });

                    // Integrity & Cleanup Button
                    if ui
                        .button(RichText::new("🔍 Integrity & Cleanup").size(12.0))
                        .clicked()
                    {
                        self.open_integrity_dialog();
                    }

                    // Manage Roots Button
                    if ui.button(RichText::new("Manage Roots").size(12.0)).clicked() {
                        self.add_root_dialog_open = true;
                    }

                    // Scan Roots Button
                    let is_scanning = self.active_scan_cancel.is_some();
                    if ui
                        .add_enabled(
                            !is_scanning,
                            egui::Button::new(RichText::new("Scan Library").size(12.0)),
                        )
                        .clicked()
                    {
                        if let Ok(roots) = self.db.get_roots() {
                            if let Some(first_root) = roots.first() {
                                trigger_scan_root = Some(first_root.id);
                            } else {
                                self.add_root_dialog_open = true;
                            }
                        }
                    }
                });
            });

            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);

            // Filter & Search Controls
            ui.horizontal(|ui| {
                icons::icon_label(ui, Icon::Search, 14.0, Color32::from_gray(160));
                let mut query = self.library_filter.search_query.clone().unwrap_or_default();
                let search_box = egui::TextEdit::singleline(&mut query)
                    .hint_text("Search library titles, files, genres...")
                    .desired_width(220.0);
                if ui.add(search_box).changed() {
                    self.library_filter.search_query = if query.trim().is_empty() {
                        None
                    } else {
                        Some(query.trim().to_string())
                    };
                    filter_changed = true;
                }

                if self.library_filter.search_query.is_some()
                    && ui
                        .button("✖")
                        .on_hover_text("Clear search query")
                        .clicked()
                {
                    self.library_filter.search_query = None;
                    filter_changed = true;
                }

                ui.separator();

                // Category Filter Pills
                let is_all = self.library_filter.media_type.is_none()
                    && !self.library_filter.favourites_only
                    && self.library_filter.resolution_tier.is_none();
                if ui.selectable_label(is_all, "All").clicked() {
                    self.library_filter.media_type = None;
                    self.library_filter.favourites_only = false;
                    self.library_filter.resolution_tier = None;
                    filter_changed = true;
                }

                let is_video = self.library_filter.media_type == Some(MediaType::Video)
                    && !self.library_filter.favourites_only;
                if ui.selectable_label(is_video, "Video").clicked() {
                    self.library_filter.media_type = Some(MediaType::Video);
                    self.library_filter.favourites_only = false;
                    self.library_filter.resolution_tier = None;
                    filter_changed = true;
                }

                let is_audio = self.library_filter.media_type == Some(MediaType::Audio)
                    && !self.library_filter.favourites_only;
                if ui.selectable_label(is_audio, "Audio").clicked() {
                    self.library_filter.media_type = Some(MediaType::Audio);
                    self.library_filter.favourites_only = false;
                    self.library_filter.resolution_tier = None;
                    filter_changed = true;
                }

                let is_fav = self.library_filter.favourites_only;
                if ui.selectable_label(is_fav, "⭐ Favourites").clicked() {
                    self.library_filter.favourites_only = !is_fav;
                    filter_changed = true;
                }

                let is_4k = self.library_filter.resolution_tier.as_deref() == Some("4K");
                if ui.selectable_label(is_4k, "4K UHD").clicked() {
                    self.library_filter.resolution_tier = if is_4k {
                        None
                    } else {
                        Some("4K".to_string())
                    };
                    self.library_filter.media_type = Some(MediaType::Video);
                    filter_changed = true;
                }

                let is_1080p = self.library_filter.resolution_tier.as_deref() == Some("1080p");
                if ui.selectable_label(is_1080p, "1080p HD").clicked() {
                    self.library_filter.resolution_tier = if is_1080p {
                        None
                    } else {
                        Some("1080p".to_string())
                    };
                    self.library_filter.media_type = Some(MediaType::Video);
                    filter_changed = true;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Sort Direction Toggle
                    let dir_label = match self.library_sort_dir {
                        SortDirection::Ascending => "▲ Asc",
                        SortDirection::Descending => "▼ Desc",
                    };
                    if ui.button(dir_label).clicked() {
                        self.library_sort_dir = match self.library_sort_dir {
                            SortDirection::Ascending => SortDirection::Descending,
                            SortDirection::Descending => SortDirection::Ascending,
                        };
                        filter_changed = true;
                    }

                    // Sort Field Combo
                    let current_sort_label = match self.library_sort_field {
                        LibrarySortField::Title => "Title",
                        LibrarySortField::FileName => "File Name",
                        LibrarySortField::DateAdded => "Date Added",
                        LibrarySortField::DateModified => "Date Modified",
                        LibrarySortField::Duration => "Duration",
                        LibrarySortField::FileSize => "File Size",
                    };
                    egui::ComboBox::from_id_salt("library_sort_field_combo")
                        .selected_text(current_sort_label)
                        .show_ui(ui, |ui| {
                            let fields = [
                                (LibrarySortField::Title, "Title"),
                                (LibrarySortField::FileName, "File Name"),
                                (LibrarySortField::DateAdded, "Date Added"),
                                (LibrarySortField::DateModified, "Date Modified"),
                                (LibrarySortField::Duration, "Duration"),
                                (LibrarySortField::FileSize, "File Size"),
                            ];
                            for (field, label) in fields {
                                if ui
                                    .selectable_label(self.library_sort_field == field, label)
                                    .clicked()
                                {
                                    self.library_sort_field = field;
                                    filter_changed = true;
                                }
                            }
                        });
                    ui.label("Sort by:");
                });
            });

            ui.add_space(3.0);

            // Smart Collections Navigation Row
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new("✨ Collections:")
                        .strong()
                        .size(12.0)
                        .color(Color32::from_rgb(125, 211, 252)),
                );

                let is_all_sc = self.selected_smart_collection.is_none();
                if ui.selectable_label(is_all_sc, "All Media").clicked() {
                    self.selected_smart_collection = None;
                    filter_changed = true;
                }

                let mut delete_sc_id = None;
                let mut edit_sc = None;

                for sc in &self.smart_collections {
                    let is_active = self.selected_smart_collection == Some(sc.id);
                    let label = format!("{} {}", sc.icon.as_deref().unwrap_or("✨"), sc.name);
                    if ui.selectable_label(is_active, label).clicked() {
                        if is_active {
                            self.selected_smart_collection = None;
                        } else {
                            self.selected_smart_collection = Some(sc.id);
                        }
                        filter_changed = true;
                    }
                    if is_active {
                        if ui.small_button("✏").on_hover_text("Edit Collection Rules").clicked() {
                            edit_sc = Some(sc.clone());
                        }
                        if ui.small_button("🗑").on_hover_text("Delete Collection").clicked() {
                            delete_sc_id = Some(sc.id);
                        }
                    }
                }

                if ui.button("➕ New").on_hover_text("Create a new dynamic smart collection").clicked() {
                    self.open_new_smart_collection_dialog();
                }

                if let Some(sc) = edit_sc {
                    self.open_edit_smart_collection_dialog(&sc);
                }
                if let Some(id) = delete_sc_id {
                    let _ = self.db.delete_smart_collection(id);
                    if self.selected_smart_collection == Some(id) {
                        self.selected_smart_collection = None;
                    }
                    filter_changed = true;
                }
            });

            ui.add_space(2.0);

            if filter_changed {
                self.library_page = 0;
                self.refresh_library();
            }

            if self.library_multi_select {
                self.render_library_batch_bar(ui);
            }

            ui.add_space(4.0);

            // Active metadata/thumbnail enrichment banner
            if let Some(ref progress) = self.enrichment_progress {
                if !progress.finished && progress.error.is_none() {
                    ui.group(|ui| {
                        ui.label(
                            RichText::new(format!(
                                "Preparing library: {}/{} metadata and thumbnails",
                                progress.processed, progress.total
                            ))
                            .size(12.0)
                            .color(Color32::from_rgb(125, 211, 252)),
                        );
                        if let Some(ref file) = progress.current_file {
                            ui.label(
                                RichText::new(file)
                                    .size(11.0)
                                    .color(Color32::from_gray(150)),
                            );
                        }
                    });
                }
            }

            // Active Scan Progress Banner
            let mut cancel_scan = false;
            if let Some(ref progress) = self.scan_progress {
                if !progress.is_finished {
                    let disc = progress.discovered_count;
                    let ind = progress.indexed_count;
                    let skip = progress.skipped_count;
                    let miss = progress.missing_count;
                    let cur = progress.current_path.clone();

                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            icons::icon_label(
                                ui,
                                Icon::Replay,
                                14.0,
                                Color32::from_rgb(85, 175, 255),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "Scanning: Discovered {} | Indexed {} | Skipped {} | Missing {}",
                                    disc, ind, skip, miss
                                ))
                                .size(12.0)
                                .color(Color32::from_gray(210)),
                            );

                            if let Some(ref path) = cur {
                                let current_display = path.split('/').next_back().unwrap_or(path);
                                ui.label(
                                    RichText::new(format!("({})", current_display))
                                        .size(11.0)
                                        .italics()
                                        .color(Color32::from_gray(160)),
                                );
                            }

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui
                                    .button(
                                        RichText::new("Cancel")
                                            .color(Color32::from_rgb(255, 100, 100)),
                                    )
                                    .clicked()
                                {
                                    cancel_scan = true;
                                }
                            });
                        });
                    });
                    ui.add_space(4.0);
                }
            }
            if cancel_scan {
                self.cancel_library_scan();
            }

            // Main Catalog Area
            if self.library_items.is_empty() {
                ui.add_space(20.0);
                if ui_components::empty_state_box(
                    ui,
                    Some(Icon::Library),
                    "No media items in library",
                    "Add media folders to your library roots to index and organize movies, series, and music.",
                    Some("Add Library Root Folder"),
                ) == Some(true)
                {
                    self.add_root_dialog_open = true;
                }
            } else if self.library_is_grid {
                // Responsive Virtualized Grid Card Display
                let available = ui.available_width();
                // Deduct scrollbar (18px) and container padding (30px reserve)
                let usable_width = (available - 48.0).max(220.0);

                let (cols, spacing) = if usable_width >= 1050.0 {
                    (4, 16.0_f32)
                } else if usable_width >= 720.0 {
                    (3, 14.0_f32)
                } else if usable_width >= 460.0 {
                    (2, 12.0_f32)
                } else {
                    (1, 8.0_f32)
                };

                let card_outer_width =
                    ((usable_width - ((cols - 1) as f32 * spacing)) / cols as f32).floor();
                let card_inner_width = (card_outer_width - 16.0).max(160.0);
                let preview_height = (card_inner_width * 0.5625)
                    .round()
                    .clamp(90.0, 160.0);
                let card_row_height = preview_height + 120.0;

                let card_frame = egui::Frame::new()
                    .fill(BrandPalette::ELEVATED_SURFACE)
                    .stroke(Stroke::new(1.0_f32, BrandPalette::SLATE_BORDER))
                    .corner_radius(10.0_f32)
                    .inner_margin(egui::Margin::same(10));

                let total_items = self.library_items.len();
                let total_rows = total_items.div_ceil(cols);

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show_rows(ui, card_row_height, total_rows, |ui, row_range| {
                        for row_idx in row_range {
                            if row_idx >= total_rows {
                                break;
                            }
                            let start = row_idx * cols;
                            let end = (start + cols).min(total_items);
                            let chunk = &self.library_items[start..end];

                            // Pre-load thumbnail textures for visible items in this row only
                            for item_view in chunk {
                                if let Some(ref path_str) = item_view.thumbnail_path {
                                    if !self.thumbnail_textures.contains_key(path_str) {
                                        let p = std::path::Path::new(path_str);
                                        if p.is_file() {
                                            if let Ok(bytes) = std::fs::read(p) {
                                                if let Ok(img) = image::load_from_memory(&bytes) {
                                                    let rgba = img.to_rgba8();
                                                    let size = [img.width() as usize, img.height() as usize];
                                                    let byte_size = size[0] * size[1] * 4;
                                                    let color_image = egui::ColorImage::from_rgba_unmultiplied(
                                                        size,
                                                        rgba.as_raw(),
                                                    );
                                                    let tex = ctx.load_texture(
                                                        path_str,
                                                        color_image,
                                                        egui::TextureOptions::LINEAR,
                                                    );
                                                    self.thumbnail_textures.insert(path_str.clone(), tex, byte_size);
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = spacing;
                                for item_view in chunk {
                                    card_frame.show(ui, |ui| {
                                        ui.set_width(card_inner_width);
                                        ui.set_max_width(card_inner_width);
                                        ui.vertical(|ui| {
                                            ui.set_width(card_inner_width);
                                            ui.set_max_width(card_inner_width);

                                            let preview_height = (card_inner_width * 0.5625)
                                                .round()
                                                .clamp(90.0, 160.0);

                                            // 1. Dedicated Thumbnail Container (16:9 ratio)
                                            let (rect, _response) = ui.allocate_exact_size(
                                                Vec2::new(card_inner_width, preview_height),
                                                egui::Sense::hover(),
                                            );
                                            let painter = ui.painter_at(rect);

                                            let mut rendered_texture = false;
                                            if let Some(ref thumb_path) = item_view.thumbnail_path {
                                                if let Some(texture) =
                                                    self.thumbnail_textures.get(thumb_path)
                                                {
                                                    painter.image(
                                                        texture.id(),
                                                        rect,
                                                        egui::Rect::from_min_max(
                                                            egui::pos2(0.0, 0.0),
                                                            egui::pos2(1.0, 1.0),
                                                        ),
                                                        Color32::WHITE,
                                                    );
                                                    rendered_texture = true;
                                                }
                                            }

                                            if !rendered_texture {
                                                painter.rect_filled(
                                                    rect,
                                                    4.0,
                                                    Color32::from_rgb(24, 26, 32),
                                                );
                                                let icon = if item_view.item.media_type
                                                    == MediaType::Audio
                                                {
                                                    Icon::AudioTrack
                                                } else {
                                                    Icon::Play
                                                };
                                                let icon_rect = egui::Rect::from_center_size(
                                                    rect.center(),
                                                    Vec2::splat(32.0),
                                                );
                                                icons::paint_icon(
                                                    &painter,
                                                    icon_rect,
                                                    icon,
                                                    Color32::from_gray(80),
                                                );
                                            }

                                            painter.rect_stroke(
                                                rect,
                                                4.0_f32,
                                                Stroke::new(1.0_f32, Color32::from_gray(45)),
                                                egui::StrokeKind::Inside,
                                            );

                                            // Badges: Duration & Resolution
                                            if let Some(ref meta) = item_view.metadata {
                                                if meta.duration_ms > 0 {
                                                    let dur_text = format_timestamp(
                                                        meta.duration_ms as f64 / 1000.0,
                                                    );
                                                    let dur_pos =
                                                        rect.right_bottom() - Vec2::new(56.0, 18.0);
                                                    let badge_rect = egui::Rect::from_min_size(
                                                        dur_pos,
                                                        Vec2::new(52.0, 14.0),
                                                    );
                                                    painter.rect_filled(
                                                        badge_rect,
                                                        2.0,
                                                        Color32::from_black_alpha(190),
                                                    );
                                                    painter.text(
                                                        badge_rect.center(),
                                                        egui::Align2::CENTER_CENTER,
                                                        dur_text,
                                                        egui::FontId::monospace(10.0),
                                                        Color32::WHITE,
                                                    );
                                                }

                                                if meta.width >= 3800 {
                                                    let res_pos =
                                                        rect.left_top() + Vec2::new(4.0, 4.0);
                                                    let badge_rect = egui::Rect::from_min_size(
                                                        res_pos,
                                                        Vec2::new(34.0, 14.0),
                                                    );
                                                    painter.rect_filled(
                                                        badge_rect,
                                                        2.0,
                                                        Color32::from_rgb(180, 120, 20),
                                                    );
                                                    painter.text(
                                                        badge_rect.center(),
                                                        egui::Align2::CENTER_CENTER,
                                                        "4K",
                                                        egui::FontId::proportional(9.0),
                                                        Color32::WHITE,
                                                    );
                                                } else if meta.width >= 1900 {
                                                    let res_pos =
                                                        rect.left_top() + Vec2::new(4.0, 4.0);
                                                    let badge_rect = egui::Rect::from_min_size(
                                                        res_pos,
                                                        Vec2::new(36.0, 14.0),
                                                    );
                                                    painter.rect_filled(
                                                        badge_rect,
                                                        2.0,
                                                        Color32::from_rgb(30, 110, 190),
                                                    );
                                                    painter.text(
                                                        badge_rect.center(),
                                                        egui::Align2::CENTER_CENTER,
                                                        "1080p",
                                                        egui::FontId::proportional(9.0),
                                                        Color32::WHITE,
                                                    );
                                                }
                                            }

                                            ui.add_space(6.0);

                                            if self.library_multi_select {
                                                let mut selected =
                                                    self.library_selection.contains(item_view.item.id);
                                                if ui
                                                    .checkbox(&mut selected, "Selected")
                                                    .changed()
                                                {
                                                    selection_toggle = Some(item_view.item.id);
                                                }
                                            }

                                            // 2. Title strictly below thumbnail
                                            let title = item_view
                                                .metadata
                                                .as_ref()
                                                .and_then(|m| m.title.as_ref())
                                                .unwrap_or(&item_view.item.file_name);
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(title)
                                                        .strong()
                                                        .size(13.0)
                                                        .color(BrandPalette::IVORY_TEXT),
                                                )
                                                .truncate(),
                                            );

                                            // 3. Subtitle strictly below title
                                            let sub_info = match &item_view.metadata {
                                                Some(m) => {
                                                    let codec = m
                                                        .video_codec
                                                        .as_deref()
                                                        .or(m.audio_codec.as_deref())
                                                        .unwrap_or("media");
                                                    format!(
                                                        "{} • {}",
                                                        codec.to_uppercase(),
                                                        format_file_size(item_view.item.file_size)
                                                    )
                                                }
                                                None => format_file_size(item_view.item.file_size),
                                            };
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(sub_info)
                                                        .size(11.0)
                                                        .color(BrandPalette::MUTED_TEXT),
                                                )
                                                .truncate(),
                                            );

                                            ui.add_space(6.0);

                                            // 4. Dedicated Action Row strictly below title & metadata
                                            ui.horizontal(|ui| {
                                                ui.set_width(card_inner_width);
                                                let star =
                                                    if item_view.is_favourite { "★" } else { "☆" };
                                                let star_color = if item_view.is_favourite {
                                                    Color32::from_rgb(255, 200, 50)
                                                } else {
                                                    Color32::from_gray(150)
                                                };
                                                if ui
                                                    .button(
                                                        RichText::new(star)
                                                            .size(13.0)
                                                            .color(star_color),
                                                    )
                                                    .on_hover_text("Toggle Favourite")
                                                    .clicked()
                                                {
                                                    toggle_fav = Some((
                                                        item_view.item.id,
                                                        !item_view.is_favourite,
                                                    ));
                                                }

                                                ui.with_layout(
                                                    egui::Layout::right_to_left(
                                                        egui::Align::Center,
                                                    ),
                                                    |ui| {
                                                        if ui
                                                            .button("⋮")
                                                            .on_hover_text("More actions")
                                                            .clicked()
                                                        {
                                                            self.library_context_item = Some(item_view.clone());
                                                            self.show_library_context_menu = true;
                                                        }

                                                        if ui
                                                            .button("ℹ")
                                                            .on_hover_text("View Technical Details")
                                                            .clicked()
                                                        {
                                                            select_item = Some(item_view.clone());
                                                        }

                                                        if ui
                                                            .button("➕")
                                                            .on_hover_text("Enqueue to Playlist")
                                                            .clicked()
                                                        {
                                                            enqueue_path = Some(
                                                                item_view
                                                                    .item
                                                                    .canonical_path
                                                                    .clone(),
                                                            );
                                                        }

                                                        if ui
                                                            .button(
                                                                RichText::new("▶ Play").strong(),
                                                            )
                                                            .clicked()
                                                        {
                                                            play_path = Some(
                                                                item_view
                                                                    .item
                                                                    .canonical_path
                                                                    .clone(),
                                                            );
                                                        }
                                                    },
                                                );
                                            });
                                            ui.add_space(2.0);
                                        });
                                    });
                                }
                            });
                            ui.add_space(spacing);
                        }
                    });
            } else {
                // Table / List View (Virtualized)
                let total_items = self.library_items.len();
                let row_height = 34.0_f32;

                // Sticky Header Row
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    if self.library_multi_select {
                        ui.allocate_ui(Vec2::new(45.0, 20.0), |ui| {
                            ui.label(RichText::new("Select").strong().color(BrandPalette::MUTED_TEXT));
                        });
                    }
                    ui.allocate_ui(Vec2::new(30.0, 20.0), |ui| {
                        ui.label(RichText::new("Fav").strong().color(BrandPalette::MUTED_TEXT));
                    });
                    ui.allocate_ui(Vec2::new(260.0, 20.0), |ui| {
                        ui.label(RichText::new("Title / File Name").strong().color(BrandPalette::MUTED_TEXT));
                    });
                    ui.allocate_ui(Vec2::new(60.0, 20.0), |ui| {
                        ui.label(RichText::new("Type").strong().color(BrandPalette::MUTED_TEXT));
                    });
                    ui.allocate_ui(Vec2::new(70.0, 20.0), |ui| {
                        ui.label(RichText::new("Format").strong().color(BrandPalette::MUTED_TEXT));
                    });
                    ui.allocate_ui(Vec2::new(75.0, 20.0), |ui| {
                        ui.label(RichText::new("Duration").strong().color(BrandPalette::MUTED_TEXT));
                    });
                    ui.allocate_ui(Vec2::new(75.0, 20.0), |ui| {
                        ui.label(RichText::new("Size").strong().color(BrandPalette::MUTED_TEXT));
                    });
                    ui.label(RichText::new("Actions").strong().color(BrandPalette::MUTED_TEXT));
                });
                ui.separator();

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show_rows(ui, row_height, total_items, |ui, row_range| {
                        for idx in row_range {
                            if idx >= total_items {
                                break;
                            }
                            let item_view = &self.library_items[idx];
                            let bg_color = if idx % 2 == 1 {
                                Color32::from_rgba_unmultiplied(255, 255, 255, 4)
                            } else {
                                Color32::TRANSPARENT
                            };

                            egui::Frame::new()
                                .fill(bg_color)
                                .inner_margin(egui::Margin::symmetric(4, 2))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 12.0;

                                        if self.library_multi_select {
                                            ui.allocate_ui(Vec2::new(45.0, 24.0), |ui| {
                                                let mut selected = self.library_selection.contains(item_view.item.id);
                                                if ui.checkbox(&mut selected, "").changed() {
                                                    selection_toggle = Some(item_view.item.id);
                                                }
                                            });
                                        }

                                        ui.allocate_ui(Vec2::new(30.0, 24.0), |ui| {
                                            let star = if item_view.is_favourite { "★" } else { "☆" };
                                            let star_color = if item_view.is_favourite {
                                                Color32::from_rgb(255, 200, 50)
                                            } else {
                                                Color32::from_gray(150)
                                            };
                                            if ui.button(RichText::new(star).color(star_color)).clicked() {
                                                toggle_fav = Some((item_view.item.id, !item_view.is_favourite));
                                            }
                                        });

                                        ui.allocate_ui(Vec2::new(260.0, 24.0), |ui| {
                                            let title = item_view
                                                .metadata
                                                .as_ref()
                                                .and_then(|m| m.title.as_ref())
                                                .unwrap_or(&item_view.item.file_name);
                                            let title_response = ui.selectable_label(false, title);
                                            if title_response.double_clicked() {
                                                play_path = Some(item_view.item.canonical_path.clone());
                                            } else if title_response.clicked() {
                                                select_item = Some(item_view.clone());
                                            }
                                        });

                                        ui.allocate_ui(Vec2::new(60.0, 24.0), |ui| {
                                            let type_label = match item_view.item.media_type {
                                                MediaType::Video => "Video",
                                                MediaType::Audio => "Audio",
                                            };
                                            ui.label(RichText::new(type_label).color(Color32::from_gray(160)));
                                        });

                                        ui.allocate_ui(Vec2::new(70.0, 24.0), |ui| {
                                            let codec = item_view
                                                .metadata
                                                .as_ref()
                                                .and_then(|m| {
                                                    m.video_codec.as_deref().or(m.audio_codec.as_deref())
                                                })
                                                .unwrap_or(&item_view.item.file_extension);
                                            ui.label(RichText::new(codec.to_uppercase()).color(Color32::from_gray(160)));
                                        });

                                        ui.allocate_ui(Vec2::new(75.0, 24.0), |ui| {
                                            let dur_str = item_view
                                                .metadata
                                                .as_ref()
                                                .map(|m| format_timestamp(m.duration_ms as f64 / 1000.0))
                                                .unwrap_or_else(|| "-".to_string());
                                            ui.label(RichText::new(dur_str).monospace().color(Color32::from_gray(160)));
                                        });

                                        ui.allocate_ui(Vec2::new(75.0, 24.0), |ui| {
                                            ui.label(
                                                RichText::new(format_file_size(item_view.item.file_size))
                                                    .color(Color32::from_gray(160)),
                                            );
                                        });

                                        ui.horizontal(|ui| {
                                            if ui.button("▶ Play").clicked() {
                                                play_path = Some(item_view.item.canonical_path.clone());
                                            }
                                            if ui
                                                .button("➕")
                                                .on_hover_text("Enqueue to Playlist")
                                                .clicked()
                                            {
                                                enqueue_path = Some(item_view.item.canonical_path.clone());
                                            }
                                            if ui.button("ℹ").on_hover_text("View Details").clicked() {
                                                select_item = Some(item_view.clone());
                                            }
                                            if ui.button("⋮").on_hover_text("More actions").clicked() {
                                                self.library_context_item = Some(item_view.clone());
                                                self.show_library_context_menu = true;
                                            }
                                        });
                                    });
                                });
                        }
                    });
            }

            if let Some(id) = selection_toggle {
                self.library_selection.toggle(id);
            }

            ui.add_space(4.0);
            ui.separator();

            // Pagination Controls Footer
            ui.horizontal(|ui| {
                let limit = 24u64;
                let start_idx = self.library_page * limit + 1;
                let end_idx = ((self.library_page + 1) * limit).min(self.library_total_count);
                if self.library_total_count > 0 {
                    ui.label(
                        RichText::new(format!(
                            "Showing {}-{} of {} items",
                            start_idx, end_idx, self.library_total_count
                        ))
                        .size(11.5)
                        .color(Color32::from_gray(150)),
                    );
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let total_pages = self.library_total_count.div_ceil(limit);
                    let has_next = (self.library_page + 1) < total_pages;
                    if ui
                        .add_enabled(has_next, egui::Button::new("Next ▶"))
                        .clicked()
                    {
                        self.library_page += 1;
                        self.refresh_library();
                    }

                    if total_pages > 0 {
                        ui.label(
                            RichText::new(format!(
                                "Page {} of {}",
                                self.library_page + 1,
                                total_pages
                            ))
                            .size(11.5)
                            .color(Color32::from_gray(160)),
                        );
                    }

                    let has_prev = self.library_page > 0;
                    if ui
                        .add_enabled(has_prev, egui::Button::new("◀ Prev"))
                        .clicked()
                    {
                        self.library_page -= 1;
                        self.refresh_library();
                    }
                });
            });

            // Process Deferred Actions
            if let Some(path) = play_path {
                self.load_file(&path);
                self.current_view = AppView::Player;
            }
            if let Some(path) = enqueue_path {
                let _ = self.controller.playlist_append(&path);
                self.status_message = "Added item to playlist".to_string();
            }
            if let Some(item) = select_item {
                self.selected_library_item = Some(item);
            }
            if let Some((id, fav)) = toggle_fav {
                let _ = self.db.set_favourite(id, fav);
                self.refresh_library();
            }
            if let Some(root_id) = trigger_scan_root {
                self.start_library_scan(root_id);
            }
        });
    }

    /// Renders the Library Roots management modal dialog.
    fn render_roots_dialog(&mut self, ui: &mut egui::Ui, close_dialog: &mut bool) {
        ui.label(
            RichText::new("Configured Media Roots")
                .strong()
                .size(14.0)
                .color(Color32::from_rgb(85, 175, 255)),
        );
        ui.label(
            RichText::new(
                "Directories indexed by Muvrix for fast browsing, search, and automatic metadata extraction.",
            )
            .size(11.5)
            .color(Color32::from_gray(150)),
        );

        ui.add_space(8.0);

        let roots = self.db.get_roots().unwrap_or_default();
        let mut delete_root_id: Option<i64> = None;
        let mut scan_root_id: Option<i64> = None;

        egui::ScrollArea::vertical()
            .max_height(200.0)
            .show(ui, |ui| {
                if roots.is_empty() {
                    ui.label(
                        RichText::new("No library roots added yet.")
                            .italics()
                            .color(Color32::from_gray(140)),
                    );
                } else {
                    for root in &roots {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(&root.display_name).strong());
                                    ui.label(
                                        RichText::new(&root.root_path)
                                            .size(11.0)
                                            .color(Color32::from_gray(150)),
                                    );
                                    let status_text = format!(
                                        "Status: {} | Scanned: {}",
                                        root.scan_status,
                                        format_timestamp_relative(root.last_scanned_at)
                                    );
                                    ui.label(
                                        RichText::new(status_text)
                                            .size(10.5)
                                            .color(Color32::from_gray(130)),
                                    );
                                });

                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.button("Remove").clicked() {
                                            delete_root_id = Some(root.id);
                                        }
                                        let is_scanning = self.active_scan_cancel.is_some();
                                        if ui
                                            .add_enabled(!is_scanning, egui::Button::new("Scan"))
                                            .clicked()
                                        {
                                            scan_root_id = Some(root.id);
                                        }
                                    },
                                );
                            });
                        });
                        ui.add_space(4.0);
                    }
                }
            });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        // Add Root Section
        ui.label(RichText::new("Add Directory").strong().size(13.0));
        ui.horizontal(|ui| {
            if ui.button("Browse Folder...").clicked() {
                if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                    let folder_str = folder.to_string_lossy().to_string();
                    let name = folder
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "Media".to_string());
                    let root = LibraryRoot {
                        id: 0,
                        display_name: name,
                        root_path: folder_str,
                        source_type: "local".to_string(),
                        is_recursive: true,
                        is_enabled: true,
                        exclusion_patterns: Vec::new(),
                        last_scanned_at: 0,
                        scan_status: "idle".to_string(),
                    };
                    if let Ok(new_id) = self.db.insert_root(&root) {
                        scan_root_id = Some(new_id);
                    }
                }
            }

            ui.label("or path:");
            ui.add(egui::TextEdit::singleline(&mut self.add_root_path_input).desired_width(180.0));
            if ui.button("Add Path").clicked() && !self.add_root_path_input.trim().is_empty() {
                let path = self.add_root_path_input.trim().to_string();
                let name = std::path::Path::new(&path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Media".to_string());
                let root = LibraryRoot {
                    id: 0,
                    display_name: name,
                    root_path: path,
                    source_type: "local".to_string(),
                    is_recursive: true,
                    is_enabled: true,
                    exclusion_patterns: Vec::new(),
                    last_scanned_at: 0,
                    scan_status: "idle".to_string(),
                };
                if let Ok(new_id) = self.db.insert_root(&root) {
                    scan_root_id = Some(new_id);
                    self.add_root_path_input.clear();
                }
            }
        });

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Done").clicked() {
                *close_dialog = true;
            }
        });

        if let Some(id) = delete_root_id {
            let _ = self.db.delete_root(id);
            self.refresh_library();
        }
        if let Some(id) = scan_root_id {
            self.start_library_scan(id);
        }
    }

    // -------------------------------------------------------------------------
    // Media Browser View
    // -------------------------------------------------------------------------
}
