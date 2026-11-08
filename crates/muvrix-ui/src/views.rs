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
    redact_credentials_from_url, NetworkConnection, NetworkProvider, WebDavProvider,
};
use muvrix_player::equalizer::{EqualizerManager, EqualizerPreset, EQ_FREQUENCIES};
use muvrix_player::shader::{Anime4KPreset, VideoAdjustmentSettings};
use muvrix_player::{
    AssOverrideMode, PlaybackState, SeekMode, SubtitleStyling, TrackInfo, TrackType,
};

use crate::app::{format_timestamp, format_track_label, MuvrixApp};
use crate::icons::{self, Icon};

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
    Preferences,
}

impl SettingsTab {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Subtitles => "Subtitles & Engine",
            Self::VideoEnhancement => "Video & Shaders",
            Self::EqualizerAudio => "Audio & Equalizer",
            Self::NetworkMedia => "Network Sources",
            Self::Preferences => "Window & History",
        }
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

            ui.add_space(4.0);

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
                                ui.label(RichText::new("Fav").strong());
                                ui.label(RichText::new("Title / File Name").strong());
                                ui.label(RichText::new("Type").strong());
                                ui.label(RichText::new("Format").strong());
                                ui.label(RichText::new("Duration").strong());
                                ui.label(RichText::new("Size").strong());
                                ui.label(RichText::new("Actions").strong());
                                ui.end_row();

                                for item_view in &self.library_items {
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

    pub fn render_playlist_view(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(4.0);

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
                let tabs = [
                    SettingsTab::Subtitles,
                    SettingsTab::VideoEnhancement,
                    SettingsTab::EqualizerAudio,
                    SettingsTab::NetworkMedia,
                    SettingsTab::Preferences,
                ];
                for tab in tabs {
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
                                self.status_message = "Subtitle styling applied".to_string();
                            }

                            if ui.button("Reset to Engine Defaults").clicked() {
                                let _ = self.controller.reset_subtitle_styling();
                                self.active_styling = SubtitleStyling::default();
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
                                "Configure Anime4K high-quality upscaling shaders, hardware color adjustments, debanding, and sharpening.",
                            )
                            .size(12.0)
                            .color(Color32::from_gray(160)),
                        );

                        ui.add_space(8.0);

                        // Anime4K Preset
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
                                    ];
                                    for p in presets {
                                        if ui.selectable_label(self.anime4k_preset == p, p.as_str()).clicked() {
                                            self.anime4k_preset = p;
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

                        if adj_changed {
                            self.apply_video_enhancements();
                        }

                        ui.add_space(6.0);
                        if ui.button("Reset Picture Adjustments to Flat (0)").clicked() {
                            self.video_settings = VideoAdjustmentSettings::default();
                            self.anime4k_preset = Anime4KPreset::Disabled;
                            self.apply_video_enhancements();
                        }
                    });
                }
                SettingsTab::EqualizerAudio => {
                    // Section 4: Audio Equalizer & Output Device Routing
                    ui.group(|ui| {
                        ui.label(
                            RichText::new("4. 10-Band Graphic Equalizer & Audio Routing")
                                .strong()
                                .size(14.0)
                                .color(Color32::from_rgb(95, 185, 255)),
                        );
                        ui.label(
                            RichText::new(
                                "Configure 10-band peaking biquad equalizer filters, dynamic range compression (dynaudnorm), and physical output device selection.",
                            )
                            .size(12.0)
                            .color(Color32::from_gray(160)),
                        );

                        ui.add_space(8.0);

                        let mut eq_changed = false;

                        if ui.checkbox(&mut self.equalizer_settings.is_enabled, "Enable 10-Band Graphic Equalizer").changed() {
                            eq_changed = true;
                        }

                        if ui.checkbox(&mut self.equalizer_settings.dynaudnorm, "Dynamic Audio Normalizer (dynaudnorm)").changed() {
                            eq_changed = true;
                        }

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

                        // Preamp Slider
                        ui.horizontal(|ui| {
                            ui.label("Preamp Gain: ");
                            if ui.add(egui::Slider::new(&mut self.equalizer_settings.preamp_db, -12.0..=12.0).text("dB")).changed() {
                                if self.equalizer_settings.active_preset != EqualizerPreset::Custom {
                                    self.equalizer_settings.active_preset = EqualizerPreset::Custom;
                                }
                                eq_changed = true;
                            }
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
                                            let _ = EqualizerManager::set_audio_device(self.controller.handle(), &dev.name);
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
                            let mut stream_target: Option<String> = None;
                            for conn in &connections {
                                ui.group(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.vertical(|ui| {
                                            let proto_str = conn.protocol.to_lowercase();
                                            let is_webdav = proto_str == "webdav" || proto_str == "http" || proto_str == "https";
                                            let badge_text = if is_webdav {
                                                format!("[{}]", conn.protocol.to_uppercase())
                                            } else {
                                                format!("[{} - Scaffolded]", conn.protocol.to_uppercase())
                                            };
                                            let badge_color = if is_webdav {
                                                Color32::from_rgb(125, 211, 252)
                                            } else {
                                                Color32::from_rgb(251, 191, 36)
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
                                            let proto_str = conn.protocol.to_lowercase();
                                            let is_webdav = proto_str == "webdav" || proto_str == "http" || proto_str == "https";
                                            if is_webdav {
                                                if ui.button("▶ Stream").on_hover_text("Open network stream directly in Muvrix player").clicked() {
                                                    let target = format!("{}://{}:{}{}", conn.protocol, conn.host, conn.port, conn.remote_path);
                                                    stream_target = Some(target);
                                                }
                                            } else {
                                                ui.add_enabled(false, egui::Button::new("▶ Stream (Scaffolded)"))
                                                    .on_disabled_hover_text(format!(
                                                        "{} streaming backend is scaffolded for schema validation. Remote playback requires future direct VFS driver.",
                                                        conn.protocol.to_uppercase()
                                                    ));
                                            }
                                        });
                                    });
                                });
                                ui.add_space(4.0);
                            }
                            if let Some(target) = stream_target {
                                self.load_file(&target);
                                self.current_view = AppView::Player;
                            }
                        }

                        if let Some(id) = delete_conn_id {
                            let _ = self.db.delete_connection(id);
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
                                        let cred_ref = if !self.network_pass_input.is_empty() {
                                            let key = format!("{}_{}_{}", proto, self.network_host_input, self.network_user_input);
                                            self.network_vault.store(&key, &self.network_pass_input);
                                            Some(key)
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
                                            protocol: proto,
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
                                        let provider = WebDavProvider::new(
                                            &self.network_host_input,
                                            self.network_port_input,
                                            &self.network_path_input,
                                            self.network_https_input,
                                            if self.network_user_input.is_empty() { None } else { Some(self.network_user_input.clone()) },
                                            None,
                                            self.network_vault.clone(),
                                        );
                                        match provider.test_connection() {
                                            Ok(true) => {
                                                self.network_status_msg = Some(format!("Success: Endpoint {} verified", provider.base_url()));
                                            }
                                            Ok(false) => {
                                                self.network_status_msg = Some("Connection test returned negative".to_string());
                                            }
                                            Err(e) => {
                                                self.network_status_msg = Some(format!("Error: {}", e));
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
                SettingsTab::Preferences => {
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
                        let vol_slider = egui::Slider::new(&mut vol, 0.0..=100.0)
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
}
