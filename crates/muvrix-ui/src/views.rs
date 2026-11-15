//! Navigation views, panels, and secondary UI components for Muvrix.
//!
//! Implements the Media Browser, Native Playlist Queue, Playback History,
//! Subtitle Styling Settings, Picture-in-Picture controls, and Mini-Player Deck.

use std::path::PathBuf;

use eframe::egui::{self, Color32, RichText, Stroke, Vec2};
use muvrix_core::browser::{EntryKind, MediaFilter, SortBy};
use muvrix_core::database::{
    LibraryItemView, LibraryRoot, LibrarySortField, MediaType, SortDirection,
};
use muvrix_core::network::{
    redact_credentials_from_url, NetworkConnection, NetworkProvider, SftpProvider, SmbProvider,
    WebDavProvider,
};
use muvrix_player::equalizer::{EqualizerManager, EqualizerPreset, EQ_FREQUENCIES};
use muvrix_player::shader::{
    Anime4KPreset, GamutMappingMode, ShaderQuality, TargetPrimaries, TargetTrc,
    ToneMappingAlgorithm, VideoAdjustmentSettings,
};
use muvrix_player::{
    AccentColor, AssOverrideMode, PlaybackState, SeekMode, SubtitleJustify, SubtitleStyling,
    ThemeMode, TrackInfo, TrackType,
};

use crate::app::{format_timestamp, format_track_label, MuvrixApp, SleepTimer};
use crate::config_editor::ConfigEditorFile;
use crate::icons::{self, Icon};
use crate::remote_playback::{build_remote_stream, connection_username, smb_share};

/// Top-level application view modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppView {
    #[default]
    Player,
    Library,
    Browser,
    Playlist,
    Lyrics,
    Recent,
    Settings,
}

impl AppView {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Player => "Player",
            Self::Library => "Library",
            Self::Browser => "Browser",
            Self::Playlist => "Playlist",
            Self::Lyrics => "Lyrics",
            Self::Recent => "History",
            Self::Settings => "Settings",
        }
    }

    pub fn icon(&self) -> Icon {
        match self {
            Self::Player => Icon::Play,
            Self::Library => Icon::Library,
            Self::Browser => Icon::Folder,
            Self::Playlist => Icon::Playlist,
            Self::Lyrics => Icon::Lyrics,
            Self::Recent => Icon::History,
            Self::Settings => Icon::Settings,
        }
    }
}

/// Tab selector for the Settings view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    Subtitles,
    VideoEnhancement,
    EqualizerAudio,
    NetworkMedia,
    Appearance,
    ConfigEditor,
    Preferences,
}

impl SettingsTab {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Subtitles => "Subtitles & Engine",
            Self::VideoEnhancement => "Video & Shaders",
            Self::EqualizerAudio => "Audio & Equalizer",
            Self::NetworkMedia => "Network Sources",
            Self::Appearance => "Appearance",
            Self::ConfigEditor => "Config Editor",
            Self::Preferences => "Window & History",
        }
    }

    pub fn matches_search(&self, query: &str) -> bool {
        let query = query.trim().to_ascii_lowercase();
        if query.is_empty() {
            return true;
        }
        let keywords = match self {
            Self::Subtitles => "subtitle font shadow color justify speed fonts ass",
            Self::VideoEnhancement => "video shader zoom pan panscan crop hdr brightness",
            Self::EqualizerAudio => "audio equalizer volume boost device delay",
            Self::NetworkMedia => "network webdav smb sftp remote stream",
            Self::Appearance => "theme appearance accent light dark amoled color",
            Self::ConfigEditor => "config mpv.conf input.conf editor file apply",
            Self::Preferences => "window history sleep timer privacy",
        };
        self.label().to_ascii_lowercase().contains(&query) || keywords.contains(&query)
    }
}

/// Formats a file size in bytes to a human-readable string.
pub fn format_file_size(bytes: u64) -> String {
    if bytes == 0 {
        return "-".to_string();
    }
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Formats a UNIX timestamp into a relative human-readable label.
pub fn format_timestamp_relative(timestamp_secs: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if timestamp_secs == 0 || now < timestamp_secs {
        return "Recently".to_string();
    }
    let diff = now - timestamp_secs;
    if diff < 60 {
        "Just now".to_string()
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else {
        format!("{}d ago", diff / 86400)
    }
}

impl MuvrixApp {
    // -------------------------------------------------------------------------
    // Media Library View (Indexed Catalog, Filtering, Sorting, Cards & Details)
    // -------------------------------------------------------------------------

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
                ui.vertical_centered(|ui| {
                    ui.add_space(60.0);
                    icons::icon_label(ui, Icon::Library, 48.0, Color32::from_gray(80));
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new("No media items in library")
                            .size(16.0)
                            .strong()
                            .color(Color32::from_gray(180)),
                    );
                    ui.label(
                        RichText::new(
                            "Add media folders to your library roots to index and organize movies, series, and music.",
                        )
                        .size(12.0)
                        .color(Color32::from_gray(140)),
                    );
                    ui.add_space(16.0);
                    if ui
                        .button(RichText::new("Add Library Root Folder").strong())
                        .clicked()
                    {
                        self.add_root_dialog_open = true;
                    }
                    ui.add_space(60.0);
                });
            } else if self.library_is_grid {
                // Pre-load thumbnail textures for visible items
                for item_view in &self.library_items {
                    if let Some(ref path_str) = item_view.thumbnail_path {
                        if !self.thumbnail_textures.contains_key(path_str) {
                            let p = std::path::Path::new(path_str);
                            if p.is_file() {
                                if let Ok(bytes) = std::fs::read(p) {
                                    if let Ok(img) = image::load_from_memory(&bytes) {
                                        let rgba = img.to_rgba8();
                                        let size = [img.width() as usize, img.height() as usize];
                                        let color_image = egui::ColorImage::from_rgba_unmultiplied(
                                            size,
                                            rgba.as_raw(),
                                        );
                                        let tex = ctx.load_texture(
                                            path_str,
                                            color_image,
                                            egui::TextureOptions::LINEAR,
                                        );
                                        self.thumbnail_textures.insert(path_str.clone(), tex);
                                    }
                                }
                            }
                        }
                    }
                }

                // Responsive Grid Card Display
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
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

                        let card_frame = egui::Frame::new()
                            .fill(Color32::from_rgb(22, 24, 30))
                            .stroke(Stroke::new(1.0_f32, Color32::from_gray(45)))
                            .corner_radius(6.0_f32)
                            .inner_margin(egui::Margin::same(8));

                        for chunk in self.library_items.chunks(cols) {
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
                                                        .size(12.5)
                                                        .color(Color32::from_gray(230)),
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
                                                        .color(Color32::from_gray(140)),
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
                // Table / List View
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Grid::new("library_table_grid")
                            .striped(true)
                            .spacing([12.0, 6.0])
                            .show(ui, |ui| {
                                // Header row
                                if self.library_multi_select {
                                    ui.label(RichText::new("Select").strong());
                                }
                                ui.label(RichText::new("Fav").strong());
                                ui.label(RichText::new("Title / File Name").strong());
                                ui.label(RichText::new("Type").strong());
                                ui.label(RichText::new("Format").strong());
                                ui.label(RichText::new("Duration").strong());
                                ui.label(RichText::new("Size").strong());
                                ui.label(RichText::new("Actions").strong());
                                ui.end_row();

                                for item_view in &self.library_items {
                                    if self.library_multi_select {
                                        let mut selected =
                                            self.library_selection.contains(item_view.item.id);
                                        if ui.checkbox(&mut selected, "").changed() {
                                            selection_toggle = Some(item_view.item.id);
                                        }
                                    }
                                    let star = if item_view.is_favourite { "★" } else { "☆" };
                                    let star_color = if item_view.is_favourite {
                                        Color32::from_rgb(255, 200, 50)
                                    } else {
                                        Color32::from_gray(150)
                                    };
                                    if ui.button(RichText::new(star).color(star_color)).clicked() {
                                        toggle_fav =
                                            Some((item_view.item.id, !item_view.is_favourite));
                                    }

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

                                    let type_label = match item_view.item.media_type {
                                        MediaType::Video => "Video",
                                        MediaType::Audio => "Audio",
                                    };
                                    ui.label(
                                        RichText::new(type_label).color(Color32::from_gray(160)),
                                    );

                                    let codec = item_view
                                        .metadata
                                        .as_ref()
                                        .and_then(|m| {
                                            m.video_codec
                                                .as_deref()
                                                .or(m.audio_codec.as_deref())
                                        })
                                        .unwrap_or(&item_view.item.file_extension);
                                    ui.label(
                                        RichText::new(codec.to_uppercase())
                                            .color(Color32::from_gray(160)),
                                    );

                                    let dur_str = item_view
                                        .metadata
                                        .as_ref()
                                        .map(|m| format_timestamp(m.duration_ms as f64 / 1000.0))
                                        .unwrap_or_else(|| "-".to_string());
                                    ui.label(
                                        RichText::new(dur_str)
                                            .monospace()
                                            .color(Color32::from_gray(160)),
                                    );

                                    ui.label(
                                        RichText::new(format_file_size(item_view.item.file_size))
                                            .color(Color32::from_gray(160)),
                                    );

                                    ui.horizontal(|ui| {
                                        if ui.button("▶ Play").clicked() {
                                            play_path = Some(item_view.item.canonical_path.clone());
                                        }
                                        if ui
                                            .button("➕")
                                            .on_hover_text("Enqueue to Playlist")
                                            .clicked()
                                        {
                                            enqueue_path =
                                                Some(item_view.item.canonical_path.clone());
                                        }
                                        if ui.button("ℹ").on_hover_text("View Details").clicked() {
                                            select_item = Some(item_view.clone());
                                        }
                                    });

                                    ui.end_row();
                                }
                            });
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

    /// Renders the Media Details Drawer.
    fn render_library_details_drawer(&mut self, ui: &mut egui::Ui) {
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

                    // Technical Specs
                    ui.label(
                        RichText::new("Technical Information")
                            .strong()
                            .size(12.5)
                            .color(Color32::from_gray(210)),
                    );
                    ui.add_space(4.0);

                    ui.horizontal(|ui| {
                        ui.label(RichText::new("File Name:").color(Color32::from_gray(150)));
                        ui.label(&item_view.item.file_name);
                    });

                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Size:").color(Color32::from_gray(150)));
                        ui.label(format_file_size(item_view.item.file_size));
                    });

                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Path:").color(Color32::from_gray(150)));
                        ui.label(
                            RichText::new(&item_view.item.canonical_path)
                                .size(10.5)
                                .color(Color32::from_gray(170)),
                        );
                    });

                    if let Some(ref meta) = item_view.metadata {
                        if meta.duration_ms > 0 {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Duration:").color(Color32::from_gray(150)));
                                ui.label(format_timestamp(meta.duration_ms as f64 / 1000.0));
                            });
                        }

                        if meta.width > 0 && meta.height > 0 {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Resolution:").color(Color32::from_gray(150)),
                                );
                                ui.label(format!("{}x{}", meta.width, meta.height));
                            });
                        }

                        if let Some(ref v_codec) = meta.video_codec {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Video Codec:").color(Color32::from_gray(150)),
                                );
                                ui.label(v_codec.to_uppercase());
                            });
                        }

                        if let Some(ref a_codec) = meta.audio_codec {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Audio Codec:").color(Color32::from_gray(150)),
                                );
                                ui.label(a_codec.to_uppercase());
                            });
                        }

                        if meta.overall_bitrate > 0 {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Bitrate:").color(Color32::from_gray(150)));
                                ui.label(format!("{} kbps", meta.overall_bitrate / 1000));
                            });
                        }

                        if let Some(ref container) = meta.container {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Container:").color(Color32::from_gray(150)),
                                );
                                ui.label(container);
                            });
                        }
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

    pub fn render_browser_view(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(4.0);

            // 1. Navigation & Breadcrumb Header Toolbar
            ui.horizontal(|ui| {
                // Back / Forward / Parent / Refresh actions
                let can_back = !self.media_browser.history_back.is_empty();
                if ui
                    .add_enabled(can_back, egui::Button::new(RichText::new("◀").size(14.0)))
                    .on_hover_text("Back")
                    .clicked()
                {
                    self.media_browser.navigate_back();
                }

                let can_forward = !self.media_browser.history_forward.is_empty();
                if ui
                    .add_enabled(
                        can_forward,
                        egui::Button::new(RichText::new("▶").size(14.0)),
                    )
                    .on_hover_text("Forward")
                    .clicked()
                {
                    self.media_browser.navigate_forward();
                }

                let can_up = self.media_browser.current_dir.parent().is_some();
                if ui
                    .add_enabled(can_up, egui::Button::new(RichText::new("▲").size(14.0)))
                    .on_hover_text("Up to parent folder")
                    .clicked()
                {
                    self.media_browser.navigate_parent();
                }

                if icons::icon_button(ui, Icon::Replay, "Refresh directory listing").clicked() {
                    self.media_browser.refresh();
                }

                ui.separator();

                // Interactive Breadcrumb path segments
                let crumbs = self.media_browser.breadcrumbs();
                ui.horizontal_wrapped(|ui| {
                    for (name, path) in crumbs {
                        let is_current = path == self.media_browser.current_dir;
                        let text = if is_current {
                            RichText::new(&name)
                                .strong()
                                .color(Color32::from_rgb(90, 180, 255))
                        } else {
                            RichText::new(&name).color(Color32::from_gray(180))
                        };

                        if ui.link(text).clicked() {
                            self.media_browser.navigate_to(path);
                            break;
                        }
                        ui.label(RichText::new("/").color(Color32::from_gray(100)));
                    }
                });
            });

            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);

            // 2. Search Filter & Sort Controls
            ui.horizontal(|ui| {
                icons::icon_label(ui, Icon::Search, 14.0, Color32::from_gray(160));
                ui.add(
                    egui::TextEdit::singleline(&mut self.media_browser.search_query)
                        .hint_text("Search media files in current folder...")
                        .desired_width(260.0),
                );

                if !self.media_browser.search_query.is_empty() && ui.button("Clear").clicked() {
                    self.media_browser.search_query.clear();
                }

                ui.separator();

                // Category Filter Combo
                let filter_label = match self.media_browser.filter {
                    MediaFilter::AllMedia => "All Media",
                    MediaFilter::VideoOnly => "Videos Only",
                    MediaFilter::AudioOnly => "Audio Only",
                };
                egui::ComboBox::from_id_salt("browser_filter_combo")
                    .selected_text(filter_label)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(
                                self.media_browser.filter == MediaFilter::AllMedia,
                                "All Media",
                            )
                            .clicked()
                        {
                            self.media_browser.filter = MediaFilter::AllMedia;
                        }
                        if ui
                            .selectable_label(
                                self.media_browser.filter == MediaFilter::VideoOnly,
                                "Videos Only",
                            )
                            .clicked()
                        {
                            self.media_browser.filter = MediaFilter::VideoOnly;
                        }
                        if ui
                            .selectable_label(
                                self.media_browser.filter == MediaFilter::AudioOnly,
                                "Audio Only",
                            )
                            .clicked()
                        {
                            self.media_browser.filter = MediaFilter::AudioOnly;
                        }
                    });

                // Sort Order Combo
                let sort_label = match self.media_browser.sort_by {
                    SortBy::NameAsc => "Name (A-Z)",
                    SortBy::NameDesc => "Name (Z-A)",
                    SortBy::DateAsc => "Date (Oldest)",
                    SortBy::DateDesc => "Date (Newest)",
                    SortBy::SizeAsc => "Size (Smallest)",
                    SortBy::SizeDesc => "Size (Largest)",
                };
                egui::ComboBox::from_id_salt("browser_sort_combo")
                    .selected_text(sort_label)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(
                                self.media_browser.sort_by == SortBy::NameAsc,
                                "Name (A-Z)",
                            )
                            .clicked()
                        {
                            self.media_browser.set_sort_by(SortBy::NameAsc);
                        }
                        if ui
                            .selectable_label(
                                self.media_browser.sort_by == SortBy::NameDesc,
                                "Name (Z-A)",
                            )
                            .clicked()
                        {
                            self.media_browser.set_sort_by(SortBy::NameDesc);
                        }
                        if ui
                            .selectable_label(
                                self.media_browser.sort_by == SortBy::DateDesc,
                                "Date (Newest)",
                            )
                            .clicked()
                        {
                            self.media_browser.set_sort_by(SortBy::DateDesc);
                        }
                        if ui
                            .selectable_label(
                                self.media_browser.sort_by == SortBy::DateAsc,
                                "Date (Oldest)",
                            )
                            .clicked()
                        {
                            self.media_browser.set_sort_by(SortBy::DateAsc);
                        }
                        if ui
                            .selectable_label(
                                self.media_browser.sort_by == SortBy::SizeDesc,
                                "Size (Largest)",
                            )
                            .clicked()
                        {
                            self.media_browser.set_sort_by(SortBy::SizeDesc);
                        }
                        if ui
                            .selectable_label(
                                self.media_browser.sort_by == SortBy::SizeAsc,
                                "Size (Smallest)",
                            )
                            .clicked()
                        {
                            self.media_browser.set_sort_by(SortBy::SizeAsc);
                        }
                    });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icons::icon_text_button(
                        ui,
                        Icon::FolderOpen,
                        "Open Other Folder...",
                        "Pick a directory using native dialog",
                    )
                    .clicked()
                    {
                        if let Some(folder) = rfd::FileDialog::new()
                            .set_title("Choose Media Folder")
                            .pick_folder()
                        {
                            self.media_browser.navigate_to(folder);
                        }
                    }
                });
            });

            ui.add_space(4.0);
            ui.separator();

            // 3. Directory Entries Table
            let browser_error = self.media_browser.error.clone();
            if let Some(err) = browser_error {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    icons::icon_label(ui, Icon::Warning, 28.0, Color32::from_rgb(240, 90, 90));
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("Failed to read directory")
                            .strong()
                            .size(15.0),
                    );
                    ui.label(RichText::new(&err).color(Color32::from_gray(160)));
                    ui.add_space(10.0);
                    if ui.button("Retry").clicked() {
                        self.media_browser.refresh();
                    }
                });
                return;
            }

            let filtered_entries = self.media_browser.filtered_entries();
            if filtered_entries.is_empty() {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new(
                            "No media files or directories found matching current filter.",
                        )
                        .size(13.0)
                        .color(Color32::from_gray(150)),
                    );
                    ui.add_space(8.0);
                    if ui.button("Open Folder in Dialog...").clicked() {
                        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                            self.media_browser.navigate_to(folder);
                        }
                    }
                });
                return;
            }

            let mut dir_to_open: Option<PathBuf> = None;
            let mut file_to_play: Option<String> = None;
            let mut file_to_enqueue: Option<String> = None;

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Grid::new("browser_entries_grid")
                        .striped(true)
                        .min_col_width(60.0)
                        .spacing([12.0, 6.0])
                        .show(ui, |ui| {
                            // Header Row
                            ui.label(
                                RichText::new("Type")
                                    .strong()
                                    .color(Color32::from_gray(160)),
                            );
                            ui.label(
                                RichText::new("Name")
                                    .strong()
                                    .color(Color32::from_gray(160)),
                            );
                            ui.label(
                                RichText::new("Size")
                                    .strong()
                                    .color(Color32::from_gray(160)),
                            );
                            ui.label(
                                RichText::new("Actions")
                                    .strong()
                                    .color(Color32::from_gray(160)),
                            );
                            ui.end_row();

                            for entry in filtered_entries {
                                match entry.kind {
                                    EntryKind::Directory => {
                                        icons::icon_label(
                                            ui,
                                            Icon::Folder,
                                            15.0,
                                            Color32::from_rgb(255, 195, 75),
                                        );
                                        let name_label = ui.link(
                                            RichText::new(&entry.name)
                                                .strong()
                                                .color(Color32::from_rgb(230, 235, 245)),
                                        );
                                        if name_label.clicked() {
                                            dir_to_open = Some(entry.path.clone());
                                        }
                                        ui.label(RichText::new("-").color(Color32::from_gray(120)));
                                        if ui.button("Open").clicked() {
                                            dir_to_open = Some(entry.path.clone());
                                        }
                                        ui.end_row();
                                    }
                                    EntryKind::VideoFile | EntryKind::AudioFile => {
                                        let icon = if entry.kind == EntryKind::VideoFile {
                                            Icon::Play
                                        } else {
                                            Icon::AudioTrack
                                        };
                                        let icon_color = if entry.kind == EntryKind::VideoFile {
                                            Color32::from_rgb(90, 185, 255)
                                        } else {
                                            Color32::from_rgb(140, 230, 120)
                                        };

                                        icons::icon_label(ui, icon, 14.0, icon_color);

                                        let name_btn = ui.link(
                                            RichText::new(&entry.name)
                                                .color(Color32::from_gray(225)),
                                        );
                                        if name_btn.clicked() {
                                            file_to_play =
                                                Some(entry.path.to_string_lossy().to_string());
                                        }

                                        let size_str = format_file_size(entry.size_bytes);
                                        ui.label(
                                            RichText::new(size_str)
                                                .monospace()
                                                .size(11.5)
                                                .color(Color32::from_gray(160)),
                                        );

                                        ui.horizontal(|ui| {
                                            if icons::icon_text_button(
                                                ui,
                                                Icon::Play,
                                                "Play",
                                                "Play this file now",
                                            )
                                            .clicked()
                                            {
                                                file_to_play =
                                                    Some(entry.path.to_string_lossy().to_string());
                                            }
                                            if icons::icon_text_button(
                                                ui,
                                                Icon::Plus,
                                                "Queue",
                                                "Add to playlist queue",
                                            )
                                            .clicked()
                                            {
                                                file_to_enqueue =
                                                    Some(entry.path.to_string_lossy().to_string());
                                            }
                                        });

                                        ui.end_row();
                                    }
                                    EntryKind::OtherFile => {
                                        ui.label("·");
                                        ui.label(
                                            RichText::new(&entry.name)
                                                .color(Color32::from_gray(140)),
                                        );
                                        let size_str = format_file_size(entry.size_bytes);
                                        ui.label(
                                            RichText::new(size_str)
                                                .monospace()
                                                .size(11.5)
                                                .color(Color32::from_gray(120)),
                                        );
                                        ui.label("");
                                        ui.end_row();
                                    }
                                }
                            }
                        });
                });

            if let Some(target_dir) = dir_to_open {
                self.media_browser.navigate_to(target_dir);
            }
            if let Some(media_path) = file_to_play {
                self.load_file(&media_path);
                self.current_view = AppView::Player;
            }
            if let Some(media_path) = file_to_enqueue {
                let _ = self.controller.playlist_append(&media_path);
                self.status_message = format!("Added to playlist: {media_path}");
            }
        });
    }

    // -------------------------------------------------------------------------
    // Playlist & Queue View
    // -------------------------------------------------------------------------

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
                    if ui.button("Clear").clicked() {
                        let _ = self.db.clear_playlist(id);
                    }
                    if ui.button("Delete Playlist").clicked() {
                        let _ = self.db.delete_playlist(id);
                        self.selected_persistent_playlist = None;
                    }
                });

                let entries = self.db.list_playlist_entries(id).unwrap_or_default();
                let mut remove = None;
                let mut move_up = None;
                let mut move_down = None;
                for entry in &entries {
                    ui.horizontal(|ui| {
                        let label = entry.title.as_deref().unwrap_or(entry.path.as_str());
                        ui.label(RichText::new(label).size(12.0));
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
                ui.add_space(50.0);
                ui.vertical_centered(|ui| {
                    icons::icon_label(ui, Icon::Playlist, 36.0, Color32::from_gray(120));
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new("Your playback queue is empty.")
                            .size(15.0)
                            .strong()
                            .color(Color32::from_gray(180)),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(
                            "Use the Media Browser or Open File to enqueue videos, or drag files directly into the window.",
                        )
                        .size(12.5)
                        .color(Color32::from_gray(140)),
                    );
                    ui.add_space(16.0);
                    if ui.button("Browse Media Files...").clicked() {
                        self.current_view = AppView::Browser;
                    }
                });
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
                            Color32::from_rgba_premultiplied(30, 55, 95, 230)
                        } else if idx % 2 == 0 {
                            Color32::from_rgba_premultiplied(24, 27, 36, 180)
                        } else {
                            Color32::from_rgba_premultiplied(19, 21, 28, 180)
                        };

                        let border_stroke = if is_active {
                            Stroke::new(1.0_f32, Color32::from_rgb(70, 145, 240))
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
    // Playback History View
    // -------------------------------------------------------------------------

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
    // Settings & Subtitle Styling View
    // -------------------------------------------------------------------------

    pub fn render_settings_view(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                icons::icon_label(ui, Icon::Settings, 18.0, Color32::from_rgb(85, 175, 255));
                ui.label(
                    RichText::new("Player Preferences & Subtitle Engine Configuration")
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
                    SettingsTab::Subtitles,
                    SettingsTab::VideoEnhancement,
                    SettingsTab::EqualizerAudio,
                    SettingsTab::NetworkMedia,
                    SettingsTab::Appearance,
                    SettingsTab::ConfigEditor,
                    SettingsTab::Preferences,
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
                    if ui.selectable_label(is_selected, tab.label()).clicked() {
                        self.settings_tab = tab;
                    }
                }
            });

            ui.add_space(4.0);
            ui.separator();
            ui.add_space(8.0);

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    match self.settings_tab {
                        SettingsTab::Subtitles => {
                    // Section 1: Dual Subtitles (Primary & Secondary)
                    ui.group(|ui| {
                        ui.label(
                            RichText::new("1. Advanced Subtitle Stream Selection")
                                .strong()
                                .size(14.0)
                                .color(Color32::from_rgb(95, 185, 255)),
                        );
                        ui.label(
                            RichText::new(
                                "Muvrix supports upstream libmpv dual-subtitle rendering (secondary-sid). Select independent primary and secondary subtitle tracks for bilingual media consumption.",
                            )
                            .size(12.0)
                            .color(Color32::from_gray(160)),
                        );

                        ui.add_space(8.0);

                        let sub_tracks: Vec<TrackInfo> = self
                            .controller
                            .tracks_by_type(TrackType::Subtitle)
                            .into_iter()
                            .cloned()
                            .collect();

                        let selected_primary = self
                            .controller
                            .selected_track(TrackType::Subtitle)
                            .map(|t| t.id);
                        let selected_secondary = self.controller.secondary_sub_track_id();

                        ui.horizontal(|ui| {
                            ui.label("Primary Subtitle Track (Bottom):");
                            let primary_label = if let Some(id) = selected_primary {
                                sub_tracks
                                    .iter()
                                    .find(|t| t.id == id)
                                    .map(format_track_label)
                                    .unwrap_or_else(|| "Track active".to_string())
                            } else {
                                "Off (Disabled)".to_string()
                            };

                            egui::ComboBox::from_id_salt("primary_sub_setting_combo")
                                .selected_text(primary_label)
                                .show_ui(ui, |ui| {
                                    if ui.selectable_label(selected_primary.is_none(), "Off (Disabled)").clicked() {
                                        let _ = self.controller.set_subtitle_track(None);
                                    }
                                    for t in &sub_tracks {
                                        let label = format_track_label(t);
                                        let is_sel = selected_primary == Some(t.id);
                                        if ui.selectable_label(is_sel, label).clicked() {
                                            let _ = self.controller.set_subtitle_track(Some(t.id));
                                        }
                                    }
                                });
                        });

                        ui.add_space(4.0);

                        ui.horizontal(|ui| {
                            ui.label("Secondary Subtitle Track (Top):   ");
                            let sec_label = if let Some(id) = selected_secondary {
                                sub_tracks
                                    .iter()
                                    .find(|t| t.id == id)
                                    .map(format_track_label)
                                    .unwrap_or_else(|| "Track active".to_string())
                            } else {
                                "Off (Disabled)".to_string()
                            };

                            egui::ComboBox::from_id_salt("secondary_sub_setting_combo")
                                .selected_text(sec_label)
                                .show_ui(ui, |ui| {
                                    if ui.selectable_label(selected_secondary.is_none(), "Off (Disabled)").clicked() {
                                        let _ = self.controller.set_secondary_sub_track(None);
                                    }
                                    for t in &sub_tracks {
                                        let label = format_track_label(t);
                                        let is_sel = selected_secondary == Some(t.id);
                                        if ui.selectable_label(is_sel, label).clicked() {
                                            let _ = self.controller.set_secondary_sub_track(Some(t.id));
                                        }
                                    }
                                });
                        });

                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if ui
                                .button(RichText::new("⚡ Enforce Collision Avoidance").strong())
                                .on_hover_text("Pins primary subtitles at bottom (pos=100) and secondary subtitles at top (pos=10)")
                                .clicked()
                            {
                                let _ = self.controller.configure_dual_subtitle_collision_avoidance();
                                self.status_message = "Enforced dual-subtitle collision avoidance (Primary: Bottom, Secondary: Top)".to_string();
                            }
                        });
                    });

                    ui.add_space(10.0);

                    // Section 2: Subtitle Appearance Styling
                    ui.group(|ui| {
                        ui.label(
                            RichText::new("2. Subtitle Appearance & Typography Styling")
                                .strong()
                                .size(14.0)
                                .color(Color32::from_rgb(95, 185, 255)),
                        );
                        ui.label(
                            RichText::new(
                                "Configure font typography, scale, border outlines, and position. Settings are applied directly to the libmpv rendering core.",
                            )
                            .size(12.0)
                            .color(Color32::from_gray(160)),
                        );

                        ui.add_space(8.0);

                        // Font Family
                        ui.horizontal(|ui| {
                            ui.label("Font Family:    ");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.active_styling.font)
                                    .hint_text("sans-serif, Liberation Sans, Arial...")
                                    .desired_width(180.0),
                            );
                            if ui.button("Sans").clicked() {
                                self.active_styling.font = "sans-serif".to_string();
                            }
                            if ui.button("Serif").clicked() {
                                self.active_styling.font = "serif".to_string();
                            }
                            if ui.button("Monospace").clicked() {
                                self.active_styling.font = "monospace".to_string();
                            }
                        });

                        ui.add_space(4.0);

                        // Font Size Slider
                        ui.horizontal(|ui| {
                            ui.label("Font Size:      ");
                            ui.add(
                                egui::Slider::new(&mut self.active_styling.font_size, 20..=90)
                                    .text("pt"),
                            );
                        });

                        ui.add_space(4.0);

                        // Colors
                        ui.horizontal(|ui| {
                            ui.label("Font Color:     ");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.active_styling.color)
                                    .desired_width(80.0),
                            );
                            if ui.button("White").clicked() {
                                self.active_styling.color = "#FFFFFF".to_string();
                            }
                            if ui.button("Yellow").clicked() {
                                self.active_styling.color = "#FFFF00".to_string();
                            }
                            if ui.button("Cyan").clicked() {
                                self.active_styling.color = "#00FFFF".to_string();
                            }
                            if ui.button("Green").clicked() {
                                self.active_styling.color = "#00FF66".to_string();
                            }
                        });

                        ui.add_space(4.0);

                        // Outline Border
                        ui.horizontal(|ui| {
                            ui.label("Border Color:   ");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.active_styling.border_color)
                                    .desired_width(80.0),
                            );
                            if ui.button("Black").clicked() {
                                self.active_styling.border_color = "#000000".to_string();
                            }
                            if ui.button("Gray").clicked() {
                                self.active_styling.border_color = "#222222".to_string();
                            }
                            if ui.button("None").clicked() {
                                self.active_styling.border_color = "#00000000".to_string();
                            }
                        });

                        ui.add_space(4.0);

                        ui.horizontal(|ui| {
                            ui.label("Border Size:    ");
                            ui.add(
                                egui::Slider::new(&mut self.active_styling.border_size, 0.0..=10.0)
                                    .text("pt"),
                            );
                        });

                        ui.add_space(4.0);

                        // Position & Scale
                        ui.horizontal(|ui| {
                            ui.label("Vertical Pos:   ");
                            ui.add(
                                egui::Slider::new(&mut self.active_styling.pos, 0..=100)
                                    .text("% (0=top, 100=bottom)"),
                            );
                        });

                        ui.add_space(4.0);

                        ui.horizontal(|ui| {
                            ui.label("Scale Factor:   ");
                            ui.add(
                                egui::Slider::new(&mut self.active_styling.scale, 0.5..=2.5)
                                    .text("x"),
                            );
                        });

                        ui.add_space(4.0);

                        ui.horizontal(|ui| {
                            ui.label("SSA/ASS Override:  ");
                            let mode_label = match self.active_styling.ass_override_mode {
                                AssOverrideMode::Preserve => "Preserve Author Styles (Default)",
                                AssOverrideMode::SafeScale => "Safe Font Scaling Only",
                                AssOverrideMode::Force => "Force User Styling Overrides",
                                AssOverrideMode::Strip => "Strip SSA/ASS to Plain Text",
                            };
                            egui::ComboBox::from_id_salt("ass_override_mode_combo")
                                .selected_text(mode_label)
                                .show_ui(ui, |ui| {
                                    if ui.selectable_label(self.active_styling.ass_override_mode == AssOverrideMode::Preserve, "Preserve Author Styles (Default)").clicked() {
                                        self.active_styling.ass_override_mode = AssOverrideMode::Preserve;
                                    }
                                    if ui.selectable_label(self.active_styling.ass_override_mode == AssOverrideMode::SafeScale, "Safe Font Scaling Only").clicked() {
                                        self.active_styling.ass_override_mode = AssOverrideMode::SafeScale;
                                    }
                                    if ui.selectable_label(self.active_styling.ass_override_mode == AssOverrideMode::Force, "Force User Styling Overrides").clicked() {
                                        self.active_styling.ass_override_mode = AssOverrideMode::Force;
                                    }
                                    if ui.selectable_label(self.active_styling.ass_override_mode == AssOverrideMode::Strip, "Strip SSA/ASS to Plain Text").clicked() {
                                        self.active_styling.ass_override_mode = AssOverrideMode::Strip;
                                    }
                                });
                        });

                        ui.add_space(4.0);

                        ui.horizontal(|ui| {
                            ui.label("Secondary Sub Pos: ");
                            ui.add(
                                egui::Slider::new(&mut self.active_styling.secondary_pos, 0..=100)
                                    .text("% (0=top, 100=bottom)"),
                            );
                        });

                        ui.add_space(4.0);
                        ui.separator();
                        ui.label(RichText::new("Advanced subtitle timing & rendering").strong());
                        ui.horizontal(|ui| {
                            ui.label("Subtitle Speed:");
                            ui.add(
                                egui::Slider::new(&mut self.active_styling.sub_speed, 0.5..=2.0)
                                    .text("x"),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label("Shadow Offset:");
                            ui.add(
                                egui::Slider::new(
                                    &mut self.active_styling.sub_shadow_offset,
                                    0.0..=10.0,
                                )
                                .text("pt"),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label("Background Color:");
                            ui.add(
                                egui::TextEdit::singleline(
                                    &mut self.active_styling.sub_back_color,
                                )
                                .desired_width(100.0),
                            );
                            ui.label("Justify:");
                            egui::ComboBox::from_id_salt("sub_justify_combo")
                                .selected_text(match self.active_styling.sub_justify {
                                    SubtitleJustify::Auto => "Auto",
                                    SubtitleJustify::Left => "Left",
                                    SubtitleJustify::Center => "Center",
                                    SubtitleJustify::Right => "Right",
                                })
                                .show_ui(ui, |ui| {
                                    for justify in [
                                        SubtitleJustify::Auto,
                                        SubtitleJustify::Left,
                                        SubtitleJustify::Center,
                                        SubtitleJustify::Right,
                                    ] {
                                        let label = match justify {
                                            SubtitleJustify::Auto => "Auto",
                                            SubtitleJustify::Left => "Left",
                                            SubtitleJustify::Center => "Center",
                                            SubtitleJustify::Right => "Right",
                                        };
                                        if ui
                                            .selectable_label(
                                                self.active_styling.sub_justify == justify,
                                                label,
                                            )
                                            .clicked()
                                        {
                                            self.active_styling.sub_justify = justify;
                                        }
                                    }
                                });
                        });
                        ui.horizontal(|ui| {
                            ui.label("Custom Fonts Folder:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.active_styling.sub_fonts_dir)
                                    .desired_width(240.0)
                                    .hint_text("Directory containing .ttf/.otf fonts"),
                            );
                            if ui.button("Choose...").clicked() {
                                if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                                    self.active_styling.sub_fonts_dir =
                                        folder.to_string_lossy().to_string();
                                }
                            }
                            if ui.button("Clear").clicked() {
                                self.active_styling.sub_fonts_dir.clear();
                            }
                        });

                        ui.add_space(2.0);
                        ui.label(
                            RichText::new("Note: Secondary subtitle positioning (pos=10) and ASS override policy are supported natively. Independent font/color customization for secondary subtitles is unsupported by libmpv.")
                                .italics()
                                .size(11.0)
                                .color(Color32::from_gray(140)),
                        );

                        ui.add_space(4.0);

                        ui.checkbox(
                            &mut self.active_styling.ass_override,
                            "Force override embedded ASS subtitle styles (Legacy mode)",
                        );

                        ui.add_space(8.0);

                        ui.horizontal(|ui| {
                            if ui
                                .button(RichText::new("Apply Styling to Engine").strong())
                                .clicked()
                            {
                                let styling = self.active_styling.clone();
                                let _ = self.controller.set_subtitle_styling(styling);
                                self.save_media_settings();
                                self.status_message = "Subtitle styling applied".to_string();
                            }

                            if ui.button("Reset to Engine Defaults").clicked() {
                                let _ = self.controller.reset_subtitle_styling();
                                self.active_styling = SubtitleStyling::default();
                                self.save_media_settings();
                                self.status_message = "Subtitle styling reset to defaults".to_string();
                            }
                        });
                    });
                }
                SettingsTab::VideoEnhancement => {
                    // Section 3: Video Enhancement, GLSL Shaders & Picture Adjustments
                    ui.group(|ui| {
                        ui.label(
                            RichText::new("3. Video Enhancement, GLSL Shaders & Picture Adjustments")
                                .strong()
                                .size(14.0)
                                .color(Color32::from_rgb(95, 185, 255)),
                        );
                        ui.label(
                            RichText::new(
                                "Configure authentic Anime4K v4.0.1 GLSL shaders, framing geometry, color adjustments, and libplacebo tone mapping.",
                            )
                            .size(12.0)
                            .color(Color32::from_gray(160)),
                        );

                        ui.add_space(6.0);

                        // Capability Diagnostics Badges
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Engine:").size(11.0).color(Color32::from_gray(140)));
                            ui.label(
                                RichText::new(format!("libmpv {}", self.media_capabilities.libmpv_version))
                                    .monospace()
                                    .size(11.0)
                                    .color(Color32::from_rgb(147, 197, 253)),
                            );
                            let placebo_str = if self.media_capabilities.libplacebo_available {
                                "libplacebo: Active"
                            } else {
                                "libplacebo: Unavailable"
                            };
                            ui.label(
                                RichText::new(placebo_str)
                                    .monospace()
                                    .size(11.0)
                                    .color(if self.media_capabilities.libplacebo_available {
                                        Color32::from_rgb(134, 239, 172)
                                    } else {
                                        Color32::from_rgb(252, 165, 165)
                                    }),
                            );
                            let shader_str = if self.media_capabilities.shader_support {
                                "GLSL Shaders: Enabled"
                            } else {
                                "GLSL Shaders: Disabled"
                            };
                            ui.label(
                                RichText::new(shader_str)
                                    .monospace()
                                    .size(11.0)
                                    .color(Color32::from_rgb(216, 180, 254)),
                            );
                        });

                        ui.add_space(8.0);

                        // Anime4K Preset & Quality
                        ui.horizontal(|ui| {
                            ui.label("Anime4K Upscaler Preset: ");
                            let preset_label = self.anime4k_preset.as_str();
                            egui::ComboBox::from_id_salt("anime4k_preset_combo")
                                .selected_text(preset_label)
                                .show_ui(ui, |ui| {
                                    let presets = [
                                        Anime4KPreset::Disabled,
                                        Anime4KPreset::ModeA,
                                        Anime4KPreset::ModeB,
                                        Anime4KPreset::ModeC,
                                        Anime4KPreset::ModeAA,
                                        Anime4KPreset::ModeBB,
                                        Anime4KPreset::ModeCA,
                                    ];
                                    for p in presets {
                                        if ui.selectable_label(self.anime4k_preset == p, p.as_str()).clicked() {
                                            self.anime4k_preset = p;
                                            self.apply_video_enhancements();
                                        }
                                    }
                                });

                            ui.add_space(10.0);
                            ui.label("Quality Level: ");
                            let quality_label = match self.anime4k_quality {
                                ShaderQuality::Fast => "Fast (S)",
                                ShaderQuality::Balanced => "Balanced (M)",
                                ShaderQuality::High => "High Quality (L)",
                            };
                            egui::ComboBox::from_id_salt("anime4k_quality_combo")
                                .selected_text(quality_label)
                                .show_ui(ui, |ui| {
                                    let qualities = [
                                        (ShaderQuality::Fast, "Fast (S - Low GPU Load)"),
                                        (ShaderQuality::Balanced, "Balanced (M - Recommended)"),
                                        (ShaderQuality::High, "High Quality (L - Best Fidelity)"),
                                    ];
                                    for (q, label) in qualities {
                                        if ui.selectable_label(self.anime4k_quality == q, label).clicked() {
                                            self.anime4k_quality = q;
                                            self.apply_video_enhancements();
                                        }
                                    }
                                });
                        });

                        ui.add_space(6.0);
                        ui.label(RichText::new("Color & Picture Adjustments:").strong().size(12.5));

                        let mut adj_changed = false;

                        // Sliders: Brightness, Contrast, Saturation, Gamma, Hue
                        ui.horizontal(|ui| {
                            ui.label("Brightness: ");
                            if ui.add(egui::Slider::new(&mut self.video_settings.brightness, -100..=100)).changed() {
                                adj_changed = true;
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("Contrast:   ");
                            if ui.add(egui::Slider::new(&mut self.video_settings.contrast, -100..=100)).changed() {
                                adj_changed = true;
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("Saturation: ");
                            if ui.add(egui::Slider::new(&mut self.video_settings.saturation, -100..=100)).changed() {
                                adj_changed = true;
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("Gamma:      ");
                            if ui.add(egui::Slider::new(&mut self.video_settings.gamma, -100..=100)).changed() {
                                adj_changed = true;
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("Hue:        ");
                            if ui.add(egui::Slider::new(&mut self.video_settings.hue, -100..=100)).changed() {
                                adj_changed = true;
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("Sharpen:    ");
                            if ui.add(egui::Slider::new(&mut self.video_settings.sharpen, -5.0..=5.0).step_by(0.1)).changed() {
                                adj_changed = true;
                            }
                        });

                        if ui.checkbox(&mut self.video_settings.deband, "Deband filter (smooth color gradients and reduce banding)").changed() {
                            adj_changed = true;
                        }

                        ui.add_space(6.0);
                        ui.separator();
                        ui.label(RichText::new("Video Framing & Geometry:").strong().size(12.5));

                        ui.horizontal(|ui| {
                            ui.label("Aspect Ratio: ");
                            let cur_aspect = self.video_settings.aspect_ratio.clone();
                            egui::ComboBox::from_id_salt("video_aspect_combo")
                                .selected_text(&cur_aspect)
                                .show_ui(ui, |ui| {
                                    let ratios = [
                                        ("auto", "Auto / Native"),
                                        ("16:9", "16:9 Widescreen"),
                                        ("4:3", "4:3 Standard"),
                                        ("2.35:1", "2.35:1 Cinematic"),
                                        ("1:1", "1:1 Square"),
                                    ];
                                    for (val, desc) in ratios {
                                        let sel = cur_aspect == val;
                                        if ui.selectable_label(sel, desc).clicked() {
                                            self.video_settings.aspect_ratio = val.to_string();
                                            adj_changed = true;
                                        }
                                    }
                                });

                            ui.add_space(8.0);
                            ui.label("Zoom: ");
                            if ui.add(egui::Slider::new(&mut self.video_settings.zoom, -1.0..=2.0).step_by(0.05)).changed() {
                                adj_changed = true;
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("Pan-Scan / Crop:");
                            let mut panscan = self.controller.panscan();
                            if ui
                                .add(egui::Slider::new(&mut panscan, 0.0..=1.0).step_by(0.01))
                                .changed()
                            {
                                self.set_panscan(panscan);
                            }
                            if ui.button("Fit (0.0)").clicked() {
                                let _ = self.controller.set_panscan_fit();
                            }
                            if ui.button("Crop (1.0)").clicked() {
                                let _ = self.controller.set_panscan_crop();
                            }
                            ui.label(format!("{:.0}%", self.controller.panscan() * 100.0));
                        });

                        ui.horizontal(|ui| {
                            ui.label("Pan X:        ");
                            if ui.add(egui::Slider::new(&mut self.video_settings.pan_x, -0.5..=0.5).step_by(0.01)).changed() {
                                adj_changed = true;
                            }
                            ui.add_space(8.0);
                            ui.label("Pan Y: ");
                            if ui.add(egui::Slider::new(&mut self.video_settings.pan_y, -0.5..=0.5).step_by(0.01)).changed() {
                                adj_changed = true;
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("Rotation:     ");
                            for deg in [0, 90, 180, 270] {
                                if ui.selectable_label(self.video_settings.rotate == deg, format!("{deg}°")).clicked() {
                                    self.video_settings.rotate = deg;
                                    adj_changed = true;
                                }
                            }
                        });

                        ui.add_space(6.0);
                        ui.separator();
                        ui.label(RichText::new("libplacebo Color Management & HDR Tone-Mapping:").strong().size(12.5));

                        ui.horizontal(|ui| {
                            ui.label("Tone-Mapping: ");
                            let tm_label = self.video_settings.tone_mapping.as_str();
                            egui::ComboBox::from_id_salt("tone_mapping_combo")
                                .selected_text(tm_label)
                                .show_ui(ui, |ui| {
                                    let algorithms = [
                                        ToneMappingAlgorithm::Auto,
                                        ToneMappingAlgorithm::Clip,
                                        ToneMappingAlgorithm::Mobius,
                                        ToneMappingAlgorithm::Reinhard,
                                        ToneMappingAlgorithm::Hable,
                                        ToneMappingAlgorithm::Bt2390,
                                        ToneMappingAlgorithm::Spline,
                                    ];
                                    for alg in algorithms {
                                        if ui.selectable_label(self.video_settings.tone_mapping == alg, alg.as_str()).clicked() {
                                            self.video_settings.tone_mapping = alg;
                                            adj_changed = true;
                                        }
                                    }
                                });

                            ui.add_space(8.0);
                            ui.label("Gamut Mapping: ");
                            let gm_label = self.video_settings.gamut_mapping.as_str();
                            egui::ComboBox::from_id_salt("gamut_mapping_combo")
                                .selected_text(gm_label)
                                .show_ui(ui, |ui| {
                                    let modes = [
                                        GamutMappingMode::Auto,
                                        GamutMappingMode::Clip,
                                        GamutMappingMode::Perceptual,
                                        GamutMappingMode::Relative,
                                        GamutMappingMode::Saturation,
                                        GamutMappingMode::Desaturate,
                                        GamutMappingMode::Linear,
                                    ];
                                    for m in modes {
                                        if ui.selectable_label(self.video_settings.gamut_mapping == m, m.as_str()).clicked() {
                                            self.video_settings.gamut_mapping = m;
                                            adj_changed = true;
                                        }
                                    }
                                });
                        });

                        ui.horizontal(|ui| {
                            ui.label("Target TRC:   ");
                            let trc_label = match self.video_settings.target_trc {
                                TargetTrc::Auto => "Auto (SDR Default)",
                                TargetTrc::Srgb => "sRGB (Standard)",
                                TargetTrc::Bt1886 => "BT.1886 (HDTV)",
                                TargetTrc::Gamma22 => "Gamma 2.2",
                                TargetTrc::Linear => "Linear Light",
                                TargetTrc::Pq => "PQ (Native HDR - Deferred)",
                                TargetTrc::Hlg => "HLG (Native HDR - Deferred)",
                                _ => self.video_settings.target_trc.as_str(),
                            };
                            egui::ComboBox::from_id_salt("target_trc_combo")
                                .selected_text(trc_label)
                                .show_ui(ui, |ui| {
                                    ui.label(egui::RichText::new("SDR Display Targets (Active FBO)").small().strong());
                                    let sdr_trcs = [
                                        (TargetTrc::Auto, "Auto (SDR Default)"),
                                        (TargetTrc::Srgb, "sRGB (Standard PC)"),
                                        (TargetTrc::Bt1886, "BT.1886 (Rec.709 Video)"),
                                        (TargetTrc::Gamma22, "Gamma 2.2"),
                                        (TargetTrc::Linear, "Linear Light"),
                                    ];
                                    for (trc, lbl) in sdr_trcs {
                                        if ui.selectable_label(self.video_settings.target_trc == trc, lbl).clicked() {
                                            self.video_settings.target_trc = trc;
                                            adj_changed = true;
                                        }
                                    }

                                    ui.separator();
                                    ui.label(egui::RichText::new("Native HDR (Deferred — requires FP16/scRGB)").small().weak());
                                    let hdr_trcs = [
                                        (TargetTrc::Pq, "PQ / ST 2084 (Deferred)"),
                                        (TargetTrc::Hlg, "HLG / BT.2100 (Deferred)"),
                                    ];
                                    for (trc, lbl) in hdr_trcs {
                                        let btn = ui.add_enabled(
                                            false,
                                            egui::Button::new(lbl)
                                                .selected(self.video_settings.target_trc == trc),
                                        );
                                        btn.on_disabled_hover_text("Native HDR presentation is deferred while rendering to GL_RGBA8 SDR FBO. HDR sources are tone-mapped to SDR targets above.");
                                    }
                                });

                            ui.add_space(8.0);
                            ui.label("Target Primaries: ");
                            let prim_label = self.video_settings.target_prim.as_str();
                            egui::ComboBox::from_id_salt("target_primaries_combo")
                                .selected_text(prim_label)
                                .show_ui(ui, |ui| {
                                    let prims = [
                                        TargetPrimaries::Auto,
                                        TargetPrimaries::Bt709,
                                        TargetPrimaries::Bt2020,
                                        TargetPrimaries::DciP3,
                                        TargetPrimaries::DisplayP3,
                                        TargetPrimaries::AdobeRgb,
                                    ];
                                    for prim in prims {
                                        if ui.selectable_label(self.video_settings.target_prim == prim, prim.as_str()).clicked() {
                                            self.video_settings.target_prim = prim;
                                            adj_changed = true;
                                        }
                                    }
                                });
                        });

                        ui.add_space(2.0);
                        ui.label(
                            egui::RichText::new("ℹ GL_RGBA8 SDR Compositor active: HDR10/HLG inputs are tone-mapped via libplacebo into SDR.")
                                .small()
                                .weak(),
                        );

                        ui.horizontal(|ui| {
                            if ui.checkbox(&mut self.video_settings.hdr_compute_peak, "Compute Dynamic Peak Luminance").changed() {
                                adj_changed = true;
                            }
                            ui.add_space(8.0);
                            ui.label("Max Boost: ");
                            if ui.add(egui::Slider::new(&mut self.video_settings.tone_mapping_max_boost, 1.0..=3.0).step_by(0.1)).changed() {
                                adj_changed = true;
                            }
                        });

                        ui.label(
                            RichText::new(
                                "ℹ Tone-mapping processes wide color gamut and HDR10/HLG dynamic ranges into standard 8-bit GL_RGBA8 SDR framebuffers via libplacebo shaders. True 10-bit scRGB/HDR passthrough is deferred to Phase 1.3.",
                            )
                            .size(10.5)
                            .color(Color32::from_gray(140)),
                        );

                        if adj_changed {
                            self.apply_video_enhancements();
                        }

                        ui.add_space(6.0);
                        if ui.button("Reset Picture Adjustments & Shaders to Defaults").clicked() {
                            self.video_settings = VideoAdjustmentSettings::default();
                            self.anime4k_preset = Anime4KPreset::Disabled;
                            self.anime4k_quality = ShaderQuality::Balanced;
                            self.apply_video_enhancements();
                        }
                    });
                }
                SettingsTab::EqualizerAudio => {
                    // Section 4: Audio Equalizer & Output Device Routing
                    ui.group(|ui| {
                        ui.label(
                            RichText::new("4. 10-Band Parametric Equalizer & Audio DSP Routing")
                                .strong()
                                .size(14.0)
                                .color(Color32::from_rgb(95, 185, 255)),
                        );
                        ui.label(
                            RichText::new(
                                "Configure 10-band peaking biquad equalizer filters with Q-factor shaping, dynamic audio normalization, delay synchronization, and physical output routing.",
                            )
                            .size(12.0)
                            .color(Color32::from_gray(160)),
                        );

                        ui.add_space(6.0);

                        ui.group(|ui| {
                            ui.label(RichText::new("Volume Boost Ceiling").strong());
                            ui.horizontal(|ui| {
                                let mut boost = self.equalizer_settings.volume_boost.clamp(100.0, 130.0);
                                if ui
                                    .add(
                                        egui::Slider::new(&mut boost, 100.0..=130.0)
                                            .text("% (software gain)"),
                                    )
                                    .changed()
                                {
                                    self.equalizer_settings.volume_boost = boost;
                                    let _ = self.controller.set_volume_boost(boost);
                                    let _ = self.controller.set_volume(self.controller.volume());
                                    self.save_media_settings();
                                }
                                ui.label(format!(
                                    "Current ceiling: {:.0}%",
                                    self.controller.volume_boost_max()
                                ));
                            });
                        });

                        ui.add_space(6.0);

                        // Capability Diagnostics Badges
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("DSP Engine:").size(11.0).color(Color32::from_gray(140)));
                            ui.label(
                                RichText::new("FFmpeg Biquad / libmpv af")
                                    .monospace()
                                    .size(11.0)
                                    .color(Color32::from_rgb(147, 197, 253)),
                            );
                            if self.equalizer_settings.has_clipping_risk() {
                                ui.label(
                                    RichText::new("⚠️ Digital Clipping Risk")
                                        .size(11.0)
                                        .color(Color32::from_rgb(251, 191, 36)),
                                );
                            } else {
                                ui.label(
                                    RichText::new("✓ Headroom Safe")
                                        .size(11.0)
                                        .color(Color32::from_rgb(134, 239, 172)),
                                );
                            }
                        });

                        ui.add_space(8.0);

                        let mut eq_changed = false;

                        ui.horizontal(|ui| {
                            if ui.checkbox(&mut self.equalizer_settings.is_enabled, "Enable 10-Band Equalizer").changed() {
                                eq_changed = true;
                            }
                            if ui.checkbox(&mut self.equalizer_settings.dynaudnorm, "Dynamic Audio Normalizer (dynaudnorm)").changed() {
                                eq_changed = true;
                            }
                            if ui.checkbox(&mut self.equalizer_settings.pitch_correction, "Preserve Pitch (scaletempo2)").changed() {
                                eq_changed = true;
                            }
                        });

                        ui.add_space(4.0);

                        // Presets
                        ui.horizontal(|ui| {
                            ui.label("EQ Preset: ");
                            let preset_label = self.equalizer_settings.active_preset.as_str();
                            egui::ComboBox::from_id_salt("eq_preset_combo")
                                .selected_text(preset_label)
                                .show_ui(ui, |ui| {
                                    let presets = [
                                        EqualizerPreset::Flat,
                                        EqualizerPreset::BassBoost,
                                        EqualizerPreset::BassReduction,
                                        EqualizerPreset::TrebleBoost,
                                        EqualizerPreset::TrebleReduction,
                                        EqualizerPreset::VocalClarity,
                                        EqualizerPreset::Rock,
                                        EqualizerPreset::Classical,
                                        EqualizerPreset::Electronic,
                                        EqualizerPreset::Cinema,
                                    ];
                                    for p in presets {
                                        if ui.selectable_label(self.equalizer_settings.active_preset == p, p.as_str()).clicked() {
                                            self.equalizer_settings.active_preset = p;
                                            self.equalizer_settings.gains = p.default_gains();
                                            if p == EqualizerPreset::Flat {
                                                self.equalizer_settings.preamp_db = 0.0;
                                            }
                                            eq_changed = true;
                                        }
                                    }
                                });
                        });

                        ui.add_space(4.0);

                        // Headroom Warning and Auto-Attenuate button
                        if self.equalizer_settings.has_clipping_risk() {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("⚠️ Positive EQ gains may cause digital clipping with current preamp.")
                                        .color(Color32::from_rgb(251, 191, 36))
                                        .size(11.5),
                                );
                                let safe_preamp = self.equalizer_settings.calculate_safe_preamp();
                                if ui.button(format!("Auto-Attenuate Preamp ({:.1} dB)", safe_preamp)).clicked() {
                                    self.equalizer_settings.preamp_db = safe_preamp;
                                    eq_changed = true;
                                }
                            });
                        }

                        // Preamp Slider
                        ui.horizontal(|ui| {
                            ui.label("Preamp Gain: ");
                            if ui.add(egui::Slider::new(&mut self.equalizer_settings.preamp_db, -12.0..=12.0).text("dB")).changed() {
                                if self.equalizer_settings.active_preset != EqualizerPreset::Custom {
                                    self.equalizer_settings.active_preset = EqualizerPreset::Custom;
                                }
                                eq_changed = true;
                            }
                            ui.add_space(10.0);
                            ui.checkbox(&mut self.eq_show_q_factors, "Show Parametric Q Controls");
                        });

                        ui.add_space(6.0);

                        // 10 Frequency Band Sliders
                        ui.horizontal(|ui| {
                            for (i, &freq) in EQ_FREQUENCIES.iter().enumerate() {
                                ui.vertical(|ui| {
                                    let label = if freq >= 1000.0 {
                                        format!("{:.0}k", freq / 1000.0)
                                    } else {
                                        format!("{:.0}", freq)
                                    };
                                    ui.label(RichText::new(&label).size(10.5));
                                    let slider = egui::Slider::new(&mut self.equalizer_settings.gains[i], -12.0..=12.0)
                                        .vertical()
                                        .show_value(false);
                                    if ui.add_sized([24.0, 90.0], slider).changed() {
                                        self.equalizer_settings.active_preset = EqualizerPreset::Custom;
                                        eq_changed = true;
                                    }
                                    ui.label(RichText::new(format!("{:.1}", self.equalizer_settings.gains[i])).size(9.5));
                                });
                            }
                        });

                        // Parametric Q Controls (when toggled)
                        if self.eq_show_q_factors {
                            ui.add_space(4.0);
                            ui.label(RichText::new("Band Q-Factors (Bandwidth: Higher = Sharper peak):").size(11.0).color(Color32::from_gray(160)));
                            ui.horizontal(|ui| {
                                for (i, &freq) in EQ_FREQUENCIES.iter().enumerate() {
                                    ui.vertical(|ui| {
                                        let label = if freq >= 1000.0 {
                                            format!("{:.0}k", freq / 1000.0)
                                        } else {
                                            format!("{:.0}", freq)
                                        };
                                        ui.label(RichText::new(&label).size(9.5));
                                        let mut q = self.equalizer_settings.bands.get(i).map(|b| b.q_factor).unwrap_or(1.414);
                                        if ui.add(egui::DragValue::new(&mut q).range(0.1..=10.0).speed(0.05)).changed() {
                                            if let Some(b) = self.equalizer_settings.bands.get_mut(i) {
                                                b.q_factor = q;
                                                self.equalizer_settings.active_preset = EqualizerPreset::Custom;
                                                eq_changed = true;
                                            }
                                        }
                                    });
                                }
                            });
                        }

                        ui.add_space(6.0);
                        ui.separator();
                        ui.label(RichText::new("Audio DSP & Synchronization:").strong().size(12.5));

                        ui.horizontal(|ui| {
                            ui.label("Audio Delay: ");
                            if ui.add(egui::Slider::new(&mut self.equalizer_settings.audio_delay, -2.0..=2.0).step_by(0.05).text("seconds")).changed() {
                                eq_changed = true;
                            }
                            ui.add_space(10.0);
                            ui.label("Channel Layout: ");
                            let cur_layout = self.equalizer_settings.channel_layout.clone();
                            egui::ComboBox::from_id_salt("eq_channel_layout_combo")
                                .selected_text(&cur_layout)
                                .show_ui(ui, |ui| {
                                    for layout in ["auto", "stereo", "5.1", "7.1"] {
                                        if ui.selectable_label(cur_layout == layout, layout).clicked() {
                                            self.equalizer_settings.channel_layout = layout.to_string();
                                            eq_changed = true;
                                        }
                                    }
                                });
                        });

                        if eq_changed {
                            self.apply_equalizer_settings();
                        }

                        ui.add_space(6.0);
                        if ui.button("Reset Equalizer to Flat (0 dB)").clicked() {
                            self.equalizer_settings.gains = [0.0; 10];
                            self.equalizer_settings.preamp_db = 0.0;
                            self.equalizer_settings.active_preset = EqualizerPreset::Flat;
                            self.apply_equalizer_settings();
                        }

                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(4.0);

                        // Audio Output Device
                        ui.label(RichText::new("Audio Output Device:").strong().size(12.5));
                        ui.horizontal(|ui| {
                            let current_dev_desc = self.audio_devices
                                .iter()
                                .find(|d| d.name == self.selected_audio_device)
                                .map(|d| d.description.as_str())
                                .unwrap_or("Auto-select device");

                            egui::ComboBox::from_id_salt("audio_output_device_combo")
                                .selected_text(current_dev_desc)
                                .show_ui(ui, |ui| {
                                    for dev in &self.audio_devices {
                                        if ui.selectable_label(self.selected_audio_device == dev.name, &dev.description).clicked() {
                                            self.selected_audio_device = dev.name.clone();
                                            let _ = EqualizerManager::set_audio_device_with_fallback(self.controller.handle(), &dev.name);
                                        }
                                    }
                                });

                            if ui.button("Refresh Devices").clicked() {
                                self.refresh_audio_devices();
                            }
                        });
                    });
                }
                SettingsTab::NetworkMedia => {
                    // Section 5: Network Media Connections (WebDAV)
                    ui.group(|ui| {
                        ui.label(
                            RichText::new("5. Network Media Connections & Streaming")
                                .strong()
                                .size(14.0)
                                .color(Color32::from_rgb(95, 185, 255)),
                        );
                        ui.label(
                            RichText::new(
                                "Configure WebDAV / HTTP endpoints for direct network streaming. Credentials are held in application memory for this session and are not persisted by Muvrix.",
                            )
                            .size(12.0)
                            .color(Color32::from_gray(160)),
                        );

                        ui.add_space(8.0);

                        let connections = self.db.list_connections().unwrap_or_default();
                        let mut delete_conn_id: Option<i64> = None;

                        if connections.is_empty() {
                            ui.label(RichText::new("No network connections configured.").italics().color(Color32::from_gray(140)));
                        } else {
                            let mut stream_target: Option<(NetworkConnection, String)> = None;
                            let mut browse_toggle: Option<(i64, String)> = None;

                            for conn in &connections {
                                ui.group(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.vertical(|ui| {
                                            let proto_str = conn.protocol.to_lowercase();
                                            let (badge_text, badge_color) = match proto_str.as_str() {
                                                "webdav" | "http" | "https" => ("[WEBDAV]", Color32::from_rgb(125, 211, 252)),
                                                "smb" => ("[SMB2/3]", Color32::from_rgb(74, 222, 128)),
                                                "sftp" => ("[SFTP]", Color32::from_rgb(192, 132, 252)),
                                                _ => ("[REMOTE]", Color32::from_rgb(251, 191, 36)),
                                            };

                                            ui.horizontal(|ui| {
                                                ui.label(
                                                    RichText::new(badge_text)
                                                        .monospace()
                                                        .size(11.0)
                                                        .color(badge_color),
                                                );
                                                ui.label(RichText::new(&conn.name).strong());
                                            });
                                            let raw_url = format!("{}://{}:{}{}", conn.protocol, conn.host, conn.port, conn.remote_path);
                                            let redacted = redact_credentials_from_url(&raw_url);
                                            ui.label(RichText::new(redacted).size(11.0).color(Color32::from_gray(150)));
                                        });

                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            if ui.button("Delete").clicked() {
                                                delete_conn_id = Some(conn.id);
                                            }
                                            if ui.button("▶ Stream").on_hover_text("Open network stream directly in Muvrix player").clicked() {
                                                stream_target = Some((conn.clone(), conn.remote_path.clone()));
                                            }
                                            let is_browsing = self.browsing_conn_id == Some(conn.id);
                                            let b_label = if is_browsing { "Close Browser ✖" } else { "Browse 📂" };
                                            if ui.button(b_label).clicked() {
                                                browse_toggle = Some((conn.id, conn.remote_path.clone()));
                                            }
                                        });
                                    });

                                    // Render Remote Media Browser when active for this connection
                                    if self.browsing_conn_id == Some(conn.id) {
                                        ui.add_space(4.0);
                                        ui.separator();
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new("Path:").monospace().color(Color32::from_gray(140)));
                                            ui.label(RichText::new(&self.browsing_path).monospace().color(Color32::WHITE));

                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                if ui.button("🔄 Refresh").clicked() {
                                                    self.refresh_remote_directory(conn);
                                                }
                                                if self.browsing_path != "/" && ui.button("⬆ Up").clicked() {
                                                    if let Some(idx) = self.browsing_path.trim_end_matches('/').rfind('/') {
                                                        let parent = &self.browsing_path[..idx];
                                                        self.browsing_path = if parent.is_empty() { "/".to_string() } else { parent.to_string() };
                                                    } else {
                                                        self.browsing_path = "/".to_string();
                                                    }
                                                    self.refresh_remote_directory(conn);
                                                }
                                            });
                                        });

                                        if self.browsing_loading {
                                            ui.horizontal(|ui| {
                                                ui.spinner();
                                                ui.label(RichText::new("Loading directory...").italics().color(Color32::from_gray(160)));
                                            });
                                        } else if let Some(ref err) = self.browsing_error {
                                            ui.label(RichText::new(format!("⚠ {}", err)).color(Color32::from_rgb(255, 120, 120)).size(11.5));
                                        } else if self.browsing_entries.is_empty() {
                                            ui.label(RichText::new("No entries found in this directory.").italics().color(Color32::from_gray(140)));
                                        } else {
                                            egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                                                for entry in &self.browsing_entries.clone() {
                                                    ui.horizontal(|ui| {
                                                        if entry.is_dir {
                                                            if ui.button(format!("📁 {}", entry.name)).clicked() {
                                                                self.browsing_path = entry.remote_path.clone();
                                                                self.refresh_remote_directory(conn);
                                                            }
                                                        } else {
                                                            ui.label(format!("🎬 {}", entry.name));
                                                            ui.label(RichText::new(format_file_size(entry.size_bytes)).size(10.5).color(Color32::from_gray(140)));

                                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                                if ui.button("▶ Play").clicked() {
                                                                    stream_target = Some((conn.clone(), entry.remote_path.clone()));
                                                                }
                                                            });
                                                        }
                                                    });
                                                }
                                            });
                                        }
                                    }
                                });
                                ui.add_space(4.0);
                            }

                            if let Some((id, path)) = browse_toggle {
                                if self.browsing_conn_id == Some(id) {
                                    self.browsing_conn_id = None;
                                } else {
                                    self.browsing_conn_id = Some(id);
                                    if let Some(conn) = connections.iter().find(|c| c.id == id) {
                                        self.browsing_path = if conn.protocol.eq_ignore_ascii_case("smb")
                                        {
                                            // The share is carried in conn.remote_path;
                                            // browsing starts at the share root.
                                            "/".to_string()
                                        } else if path.is_empty() {
                                            "/".to_string()
                                        } else {
                                            path
                                        };
                                        self.refresh_remote_directory(conn);
                                    }
                                }
                            }

                            if let Some((conn, target)) = stream_target {
                                match build_remote_stream(&conn, &target, &self.db, &self.network_vault) {
                                    Ok(stream) => {
                                        let uri = self.stream_bridge.register(stream);
                                        self.browsing_error = None;
                                        self.load_file(&uri);
                                        self.current_view = AppView::Player;
                                    }
                                    Err(e) => {
                                        let message = format!("Remote playback failed: {e}");
                                        self.browsing_error = Some(message.clone());
                                        self.status_message = message;
                                    }
                                }
                            }
                        }

                        if let Some(id) = delete_conn_id {
                            if let Some(conn) = connections.iter().find(|c| c.id == id) {
                                if let Some(ref cred_ref) = conn.credential_ref {
                                    let _ = self.network_vault.remove_native(cred_ref);
                                }
                            }
                            let _ = self.db.delete_connection(id);
                        }

                        // Trusted SSH Host Fingerprints (TOFU Database)
                        let trusted_hosts = self.db.list_ssh_host_trust().unwrap_or_default();
                        if !trusted_hosts.is_empty() {
                            ui.add_space(8.0);
                            ui.separator();
                            ui.label(RichText::new("Trusted SSH Host Fingerprints (TOFU Database):").strong().size(12.5));
                            let mut revoke_target: Option<(String, u16)> = None;
                            egui::Grid::new("ssh_trusted_hosts_grid").striped(true).spacing([12.0, 4.0]).show(ui, |ui| {
                                ui.label(RichText::new("Host:Port").strong());
                                ui.label(RichText::new("Algorithm").strong());
                                ui.label(RichText::new("Fingerprint (SHA256)").strong());
                                ui.label(RichText::new("Action").strong());
                                ui.end_row();

                                for trust in &trusted_hosts {
                                    ui.label(format!("{}:{}", trust.host, trust.port));
                                    ui.label(&trust.key_algorithm);
                                    ui.label(RichText::new(&trust.fingerprint_sha256).monospace().size(11.0));
                                    if ui.button("Revoke").clicked() {
                                        revoke_target = Some((trust.host.clone(), trust.port));
                                    }
                                    ui.end_row();
                                }
                            });
                            if let Some((h, p)) = revoke_target {
                                let _ = self.db.revoke_ssh_host_trust(&h, p);
                                self.network_status_msg = Some("SSH host fingerprint trust revoked.".to_string());
                            }
                        }

                        ui.add_space(6.0);

                        let add_label = if self.add_network_open { "Hide Add Connection Form" } else { "➕ Add Network Connection" };
                        if ui.button(add_label).clicked() {
                            self.add_network_open = !self.add_network_open;
                        }

                        if self.add_network_open {
                            ui.add_space(6.0);
                            ui.group(|ui| {
                                ui.label(RichText::new("New Network Remote Endpoint").strong().size(12.5));

                                ui.horizontal(|ui| {
                                    ui.label("Protocol:   ");
                                    let cur_proto = self.network_protocol_input.clone();
                                    let proto_label = match cur_proto.as_str() {
                                        "smb" => "SMB / Samba Share (Windows / NAS)",
                                        "sftp" => "SFTP (SSH File Transfer Protocol)",
                                        _ => "WebDAV / Nextcloud / ownCloud",
                                    };
                                    egui::ComboBox::from_id_salt("net_proto_combo")
                                        .selected_text(proto_label)
                                        .show_ui(ui, |ui| {
                                            if ui.selectable_label(self.network_protocol_input == "webdav", "WebDAV / Nextcloud").clicked() {
                                                self.network_protocol_input = "webdav".to_string();
                                                self.network_port_input = if self.network_https_input { 443 } else { 80 };
                                            }
                                            if ui.selectable_label(self.network_protocol_input == "smb", "SMB / Samba Share").clicked() {
                                                self.network_protocol_input = "smb".to_string();
                                                self.network_port_input = 445;
                                            }
                                            if ui.selectable_label(self.network_protocol_input == "sftp", "SFTP (SSH File Transfer)").clicked() {
                                                self.network_protocol_input = "sftp".to_string();
                                                self.network_port_input = 22;
                                            }
                                        });
                                });

                                ui.horizontal(|ui| {
                                    ui.label("Name:       ");
                                    ui.add(egui::TextEdit::singleline(&mut self.network_name_input).hint_text("My NAS / Server"));
                                });

                                ui.horizontal(|ui| {
                                    ui.label("Host / IP:  ");
                                    ui.add(egui::TextEdit::singleline(&mut self.network_host_input).hint_text("nas.local or 192.168.1.100"));
                                });

                                ui.horizontal(|ui| {
                                    ui.label("Port:       ");
                                    ui.add(egui::DragValue::new(&mut self.network_port_input).range(1..=65535));
                                    if self.network_protocol_input == "webdav"
                                        && ui
                                            .checkbox(&mut self.network_https_input, "Use HTTPS")
                                            .changed()
                                    {
                                        if self.network_https_input && self.network_port_input == 80
                                        {
                                            self.network_port_input = 443;
                                        } else if !self.network_https_input
                                            && self.network_port_input == 443
                                        {
                                            self.network_port_input = 80;
                                        }
                                    }
                                });

                                ui.horizontal(|ui| {
                                    ui.label("Path/Share: ");
                                    let hint = match self.network_protocol_input.as_str() {
                                        "smb" => "ShareName/Movies/video.mp4",
                                        "sftp" => "/home/user/media/video.mkv",
                                        _ => "/remote.php/webdav/Media/",
                                    };
                                    ui.add(egui::TextEdit::singleline(&mut self.network_path_input).hint_text(hint));
                                });

                                if self.network_protocol_input == "sftp" {
                                    ui.horizontal(|ui| {
                                        ui.label("Host Key:   ");
                                        ui.add(egui::TextEdit::singleline(&mut self.network_host_key_input).hint_text("SHA256:... (pinned server fingerprint)"));
                                        if ui.button("Probe Host Key").clicked() && !self.network_host_input.trim().is_empty() {
                                            match SftpProvider::inspect_host_key(&self.network_host_input, self.network_port_input) {
                                                Ok(presented) => {
                                                    self.pending_ssh_host_trust = Some(presented);
                                                    self.ssh_host_trust_error = None;
                                                }
                                                Err(e) => {
                                                    self.ssh_host_trust_error = Some(format!("Failed to probe host key: {e}"));
                                                }
                                            }
                                        }
                                    });

                                    if let Some(presented) = self.pending_ssh_host_trust.clone() {
                                        let mut clear_trust_prompt = false;
                                        let mut approved = false;
                                        ui.group(|ui| {
                                            ui.label(RichText::new("⚠️ SSH Host Key Verification (Trust On First Use)").strong().color(Color32::from_rgb(251, 191, 36)));
                                            ui.label(format!("Endpoint: {}:{}", presented.host, presented.port));
                                            ui.label(format!("Key Type: {}", presented.algorithm));
                                            ui.label(RichText::new(format!("SHA-256 Fingerprint: {}", presented.fingerprint)).monospace().strong());
                                            ui.label(RichText::new("CRITICAL: Verify this fingerprint matches your server's host key using a trusted out-of-band channel (e.g. server console, physical terminal, or verified administrator call). Never approve an unexpected fingerprint. Approving stores persistent trust in the local database.").size(11.0).color(Color32::from_rgb(240, 200, 120)));

                                            ui.horizontal(|ui| {
                                                if ui.button(RichText::new("✓ Approve & Trust Host").strong().color(Color32::from_rgb(74, 222, 128))).clicked() {
                                                    approved = true;
                                                    clear_trust_prompt = true;
                                                }
                                                if ui.button("✗ Reject").clicked() {
                                                    clear_trust_prompt = true;
                                                    self.network_status_msg = Some("SSH Host verification rejected by user.".to_string());
                                                }
                                            });
                                        });
                                        if approved {
                                            let _ = self.db.record_ssh_host_trust(
                                                &presented.host,
                                                presented.port,
                                                &presented.algorithm,
                                                &presented.fingerprint,
                                                None,
                                            );
                                            self.network_host_key_input = presented.fingerprint.clone();
                                            self.network_status_msg = Some(format!("SSH Host {}:{} trusted in database.", presented.host, presented.port));
                                        }
                                        if clear_trust_prompt {
                                            self.pending_ssh_host_trust = None;
                                        }
                                    }

                                    if let Some(ref err) = self.ssh_host_trust_error {
                                        ui.label(RichText::new(format!("⚠️ {}", err)).color(Color32::from_rgb(255, 120, 120)).size(11.5));
                                    }
                                }

                                ui.horizontal(|ui| {
                                    ui.label("Username:   ");
                                    ui.add(egui::TextEdit::singleline(&mut self.network_user_input));
                                });

                                ui.horizontal(|ui| {
                                    ui.label("Password:   ");
                                    ui.add(egui::TextEdit::singleline(&mut self.network_pass_input).password(true));
                                });

                                ui.add_space(4.0);

                                ui.horizontal(|ui| {
                                    if ui.button(RichText::new("Save Connection").strong()).clicked()
                                        && !self.network_host_input.trim().is_empty()
                                    {
                                        let proto = self.network_protocol_input.clone();
                                        // Persist the HTTPS checkbox for WebDAV so a
                                        // non-standard port does not get treated as HTTP.
                                        let connection_protocol = if proto == "webdav"
                                            && self.network_https_input
                                        {
                                            "https".to_string()
                                        } else {
                                            proto.clone()
                                        };
                                        let cred_ref = if !self.network_pass_input.is_empty() {
                                            let key = muvrix_core::network::generate_credential_ref(
                                                &proto,
                                                self.network_host_input.trim(),
                                                self.network_port_input,
                                                self.network_user_input.trim(),
                                            );
                                            match self.network_vault.store_native(&key, &self.network_pass_input) {
                                                Ok(()) => Some(key),
                                                Err(e) => {
                                                    self.network_status_msg = Some(format!("Failed to save credential to secure storage: {e}"));
                                                    return;
                                                }
                                            }
                                        } else {
                                            None
                                        };
                                        let conn = NetworkConnection {
                                            id: 0,
                                            name: if self.network_name_input.trim().is_empty() {
                                                format!("{} ({})", proto.to_uppercase(), self.network_host_input)
                                            } else {
                                                self.network_name_input.trim().to_string()
                                            },
                                            protocol: connection_protocol,
                                            host: self.network_host_input.trim().to_string(),
                                            port: self.network_port_input,
                                            remote_path: if self.network_path_input.trim().is_empty() {
                                                "/".to_string()
                                            } else {
                                                self.network_path_input.trim().to_string()
                                            },
                                            auth_type: if self.network_pass_input.is_empty() { "anonymous".to_string() } else { "basic".to_string() },
                                            credential_ref: cred_ref,
                                            is_anonymous: self.network_pass_input.is_empty(),
                                            auto_connect: false,
                                            last_connected_at: 0,
                                        };
                                        let _ = self.db.insert_connection(&conn);
                                        self.network_status_msg = Some("Connection saved successfully".to_string());
                                        self.add_network_open = false;
                                    }

                                    if ui.button("Test Endpoint").clicked() {
                                        match self.network_protocol_input.as_str() {
                                            "sftp" => {
                                                let fp = if self.network_host_key_input.trim().is_empty() {
                                                    None
                                                } else {
                                                    Some(self.network_host_key_input.trim().to_string())
                                                };
                                                let provider = SftpProvider::new(
                                                    &self.network_host_input,
                                                    self.network_port_input,
                                                    &self.network_path_input,
                                                    &self.network_user_input,
                                                    if self.network_pass_input.is_empty() { None } else {
                                                        let key = muvrix_core::network::generate_credential_ref(
                                                            "sftp",
                                                            self.network_host_input.trim(),
                                                            self.network_port_input,
                                                            self.network_user_input.trim(),
                                                        );
                                                        self.network_vault.store_ephemeral(&key, &self.network_pass_input);
                                                        Some(key)
                                                    },
                                                    None,
                                                    fp,
                                                    self.network_vault.clone(),
                                                ).with_database(self.db.clone());
                                                match provider.test_connection() {
                                                    Ok(true) => {
                                                        self.network_status_msg = Some(format!("Success: SFTP endpoint {}:{} verified with host-key security", self.network_host_input, self.network_port_input));
                                                    }
                                                    Ok(false) => {
                                                        self.network_status_msg = Some("Connection test returned negative".to_string());
                                                    }
                                                    Err(e) => {
                                                        self.network_status_msg = Some(format!("SFTP Error: {}", e));
                                                    }
                                                }
                                            }
                                            "smb" => {
                                                let provider = SmbProvider::new(
                                                    &self.network_host_input,
                                                    self.network_port_input,
                                                    if self.network_path_input.is_empty() { "Share" } else { &self.network_path_input },
                                                    "/",
                                                    if self.network_user_input.is_empty() { None } else { Some(self.network_user_input.clone()) },
                                                    None,
                                                    if self.network_pass_input.is_empty() { None } else {
                                                        let key = muvrix_core::network::generate_credential_ref(
                                                            "smb",
                                                            self.network_host_input.trim(),
                                                            self.network_port_input,
                                                            self.network_user_input.trim(),
                                                        );
                                                        self.network_vault.store_ephemeral(&key, &self.network_pass_input);
                                                        Some(key)
                                                    },
                                                    self.network_pass_input.is_empty(),
                                                    self.network_vault.clone(),
                                                );
                                                match provider.test_connection() {
                                                    Ok(true) => {
                                                        self.network_status_msg = Some(format!("Success: SMB share on {} verified", self.network_host_input));
                                                    }
                                                    Ok(false) => {
                                                        self.network_status_msg = Some("Connection test returned negative".to_string());
                                                    }
                                                    Err(e) => {
                                                        self.network_status_msg = Some(format!("SMB Error: {}", e));
                                                    }
                                                }
                                            }
                                            _ => {
                                                let provider = WebDavProvider::new(
                                                    &self.network_host_input,
                                                    self.network_port_input,
                                                    &self.network_path_input,
                                                    self.network_https_input,
                                                    if self.network_user_input.is_empty() { None } else { Some(self.network_user_input.clone()) },
                                                    if self.network_pass_input.is_empty() { None } else {
                                                        let key = muvrix_core::network::generate_credential_ref(
                                                            "webdav",
                                                            self.network_host_input.trim(),
                                                            self.network_port_input,
                                                            self.network_user_input.trim(),
                                                        );
                                                        self.network_vault.store_ephemeral(&key, &self.network_pass_input);
                                                        Some(key)
                                                    },
                                                    self.network_vault.clone(),
                                                );
                                                match provider.test_connection() {
                                                    Ok(true) => {
                                                        self.network_status_msg = Some(format!("Success: WebDAV endpoint {} verified", provider.base_url()));
                                                    }
                                                    Ok(false) => {
                                                        self.network_status_msg = Some("Connection test returned negative".to_string());
                                                    }
                                                    Err(e) => {
                                                        self.network_status_msg = Some(format!("WebDAV Error: {}", e));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                });
                            });
                        }

                        if let Some(ref msg) = self.network_status_msg {
                            ui.add_space(4.0);
                            ui.label(RichText::new(msg).color(Color32::from_rgb(100, 200, 100)).size(11.5));
                        }
                    });
                }
                SettingsTab::Appearance => {
                    ui.group(|ui| {
                        ui.label(
                            RichText::new("Theme & Accent")
                                .strong()
                                .size(14.0)
                                .color(Color32::from_rgb(95, 185, 255)),
                        );
                        ui.label(
                            RichText::new("Persisted in settings.json and applied immediately.")
                                .size(11.5)
                                .color(Color32::from_gray(150)),
                        );
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label("Theme:");
                            egui::ComboBox::from_id_salt("theme_mode_combo")
                                .selected_text(match self.theme_mode {
                                    ThemeMode::Dark => "Dark",
                                    ThemeMode::Light => "Light",
                                    ThemeMode::Amoled => "AMOLED (pure black)",
                                })
                                .show_ui(ui, |ui| {
                                    for mode in [ThemeMode::Dark, ThemeMode::Light, ThemeMode::Amoled] {
                                        let label = match mode {
                                            ThemeMode::Dark => "Dark",
                                            ThemeMode::Light => "Light",
                                            ThemeMode::Amoled => "AMOLED (pure black)",
                                        };
                                        if ui.selectable_label(self.theme_mode == mode, label).clicked() {
                                            self.theme_mode = mode;
                                            self.save_media_settings();
                                        }
                                    }
                                });
                        });
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label("Accent:");
                            egui::ComboBox::from_id_salt("accent_color_combo")
                                .selected_text(format!("{:?}", self.accent_color))
                                .show_ui(ui, |ui| {
                                    for color in [
                                        AccentColor::Blue,
                                        AccentColor::Purple,
                                        AccentColor::Teal,
                                        AccentColor::Green,
                                        AccentColor::Orange,
                                        AccentColor::Red,
                                    ] {
                                        if ui
                                            .selectable_label(
                                                self.accent_color == color,
                                                format!("{color:?}"),
                                            )
                                            .clicked()
                                        {
                                            self.accent_color = color;
                                            self.save_media_settings();
                                        }
                                    }
                                });
                        });
                    });
                }
                SettingsTab::ConfigEditor => {
                    self.ensure_config_editor_loaded();
                    ui.group(|ui| {
                        ui.label(
                            RichText::new("Sandboxed libmpv Configuration")
                                .strong()
                                .size(14.0)
                                .color(Color32::from_rgb(95, 185, 255)),
                        );
                        ui.label(
                            RichText::new(
                                "Edit mpv.conf or input.conf in Muvrix's config directory. Applying mpv.conf issues load-config-file.",
                            )
                            .size(11.5)
                            .color(Color32::from_gray(150)),
                        );
                        ui.horizontal(|ui| {
                            for file in [ConfigEditorFile::MpvConf, ConfigEditorFile::InputConf] {
                                if ui
                                    .selectable_label(
                                        self.config_editor_file == file,
                                        file.label(),
                                    )
                                    .clicked()
                                {
                                    self.config_editor_file = file;
                                    self.config_editor_loaded = false;
                                    self.config_editor_text.clear();
                                }
                            }
                            if ui.button("Reload").clicked() {
                                self.config_editor_loaded = false;
                                self.ensure_config_editor_loaded();
                            }
                        });
                        ui.add_space(4.0);
                        ui.add(
                            egui::TextEdit::multiline(&mut self.config_editor_text)
                                .code_editor()
                                .desired_rows(18)
                                .desired_width(f32::INFINITY),
                        );
                        ui.horizontal(|ui| {
                            if ui.button(RichText::new("Save & Apply").strong()).clicked() {
                                self.save_config_editor();
                            }
                            if !self.config_editor_status.is_empty() {
                                ui.label(
                                    RichText::new(&self.config_editor_status)
                                        .size(11.0)
                                        .color(Color32::from_gray(170)),
                                );
                            }
                        });
                    });
                }
                SettingsTab::Preferences => {
                    ui.group(|ui| {
                        ui.label(
                            RichText::new("Sleep Timer")
                                .strong()
                                .size(14.0)
                                .color(Color32::from_rgb(95, 185, 255)),
                        );
                        ui.horizontal(|ui| {
                            ui.label("Stop after:");
                            egui::ComboBox::from_id_salt("settings_sleep_timer_combo")
                                .selected_text(self.sleep_timer.label())
                                .show_ui(ui, |ui| {
                                    for option in [
                                        SleepTimer::Off,
                                        SleepTimer::Minutes(15),
                                        SleepTimer::Minutes(30),
                                        SleepTimer::Minutes(45),
                                        SleepTimer::Minutes(60),
                                        SleepTimer::EndOfFile,
                                    ] {
                                        if ui
                                            .selectable_label(
                                                self.sleep_timer == option,
                                                option.label(),
                                            )
                                            .clicked()
                                        {
                                            self.set_sleep_timer(option);
                                        }
                                    }
                                });
                            if let Some(remaining) = self.sleep_remaining() {
                                let seconds = remaining.as_secs();
                                ui.label(
                                    RichText::new(format!(
                                        "Remaining {}:{:02}",
                                        seconds / 60,
                                        seconds % 60
                                    ))
                                    .monospace()
                                    .color(Color32::from_rgb(125, 211, 252)),
                                );
                            }
                        });
                    });

                    // Section 6: Compact Mini-Player Mode & Window Display
                    ui.group(|ui| {
                        ui.label(
                            RichText::new("6. Compact Mini-Player Mode & Window Display")
                                .strong()
                                .size(14.0)
                                .color(Color32::from_rgb(95, 185, 255)),
                        );
                        ui.label(
                            RichText::new(
                                "Muvrix provides a Compact Mini-Player Mode by resizing the primary window to 480x270 and requesting AlwaysOnTop. Detached Picture-in-Picture (multi-viewport window) is deferred due to OpenGL context sharing hazards across threads/surfaces and Wayland compositor constraints.",
                            )
                            .size(12.0)
                            .color(Color32::from_gray(160)),
                        );

                        ui.add_space(6.0);

                        let pip_btn_text = if self.is_pip {
                            "Exit Compact Mini-Player Mode"
                        } else {
                            "Enter Compact Mini-Player Mode"
                        };
                        if ui.button(RichText::new(pip_btn_text).strong()).clicked() {
                            self.toggle_pip(ctx);
                        }

                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(
                                "Architectural Note: In eframe 0.31 with egui_glow, detached multi-viewports spawn secondary window surfaces with independent OpenGL contexts. libmpv's render context is bound to the primary surface; shared GL context binding across Wayland surfaces is driver-dependent and subject to compositor protocol restrictions. Compact Mini-Player Mode operates within the primary window to maintain 100% stability. Always-on-top on Wayland is best-effort and depends on compositor support.",
                            )
                            .size(11.0)
                            .color(Color32::from_gray(140)),
                        );
                    });

                    ui.add_space(10.0);

                    // Section 7: History & Persistence Controls
                    ui.group(|ui| {
                        ui.label(
                            RichText::new("7. History & Privacy Preferences")
                                .strong()
                                .size(14.0)
                                .color(Color32::from_rgb(95, 185, 255)),
                        );

                        ui.add_space(4.0);

                        let mut enabled = self.history_store.enabled;
                        if ui.checkbox(&mut enabled, "Enable Playback History Recording").changed() {
                            self.history_store.set_enabled(enabled);
                            let _ = self.history_store.save();
                        }

                        let mut auto_resume = self.history_store.auto_resume_enabled;
                        if ui.checkbox(&mut auto_resume, "Enable Resume Playback Prompts (>10s threshold)").changed() {
                            self.history_store.set_auto_resume(auto_resume);
                            let _ = self.history_store.save();
                        }

                        ui.add_space(4.0);
                        if ui.button("Clear History Store").clicked() {
                            self.history_store.clear();
                            let _ = self.history_store.save();
                            self.status_message = "History cleared".to_string();
                        }
                    });
                }
            }
        });
    });
    }

    // -------------------------------------------------------------------------
    // Mini-Player Transport Deck (When browsing other views during playback)
    // -------------------------------------------------------------------------

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

    /// Queries the remote host directory and populates `browsing_entries` for the remote browser view.
    pub(crate) fn refresh_remote_directory(&mut self, conn: &NetworkConnection) {
        let path = if self.browsing_path.is_empty() {
            "/".to_string()
        } else {
            self.browsing_path.clone()
        };

        match conn.protocol.to_lowercase().as_str() {
            "webdav" | "http" | "https" => {
                let is_https = conn.protocol.to_lowercase() == "https"
                    || conn.port == 443
                    || conn.port == 8443;
                let provider = WebDavProvider::new(
                    &conn.host,
                    conn.port,
                    &path,
                    is_https,
                    connection_username(conn),
                    conn.credential_ref.clone(),
                    self.network_vault.clone(),
                );
                match provider.list_directory(&path) {
                    Ok(entries) => {
                        self.browsing_entries = entries;
                        self.browsing_error = None;
                    }
                    Err(e) => {
                        self.browsing_error = Some(format!("WebDAV: {e}"));
                    }
                }
            }
            "sftp" => {
                let provider = SftpProvider::new(
                    &conn.host,
                    conn.port,
                    &path,
                    &connection_username(conn).unwrap_or_default(),
                    conn.credential_ref.clone(),
                    None,
                    if self.network_host_key_input.is_empty() {
                        None
                    } else {
                        Some(self.network_host_key_input.clone())
                    },
                    self.network_vault.clone(),
                );
                match provider.list_directory(&path) {
                    Ok(entries) => {
                        self.browsing_entries = entries;
                        self.browsing_error = None;
                    }
                    Err(e) => {
                        self.browsing_error = Some(format!("SFTP: {e}"));
                    }
                }
            }
            "smb" => {
                let Some(share) = smb_share(&conn.remote_path) else {
                    self.browsing_error =
                        Some("SMB connection has no share in its remote path".to_string());
                    return;
                };
                let provider = SmbProvider::new(
                    &conn.host,
                    conn.port,
                    &share,
                    &path,
                    connection_username(conn),
                    None,
                    conn.credential_ref.clone(),
                    conn.is_anonymous,
                    self.network_vault.clone(),
                );
                match provider.list_directory(&path) {
                    Ok(entries) => {
                        self.browsing_entries = entries;
                        self.browsing_error = None;
                    }
                    Err(e) => {
                        self.browsing_error = Some(format!("SMB: {e}"));
                    }
                }
            }
            _ => {
                self.browsing_error = Some(format!("Unsupported protocol: {}", conn.protocol));
            }
        }
    }
}
