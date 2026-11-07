//! Navigation views, panels, and secondary UI components for Muvrix.
//!
//! Implements the Media Browser, Native Playlist Queue, Playback History,
//! Subtitle Styling Settings, Picture-in-Picture controls, and Mini-Player Deck.

use std::path::PathBuf;

use eframe::egui::{self, Color32, RichText, Stroke, Vec2};
use muvrix_core::browser::{EntryKind, MediaFilter, SortBy};
use muvrix_player::{PlaybackState, SeekMode, SubtitleStyling, TrackInfo, TrackType};

use crate::app::{format_timestamp, format_track_label, MuvrixApp};
use crate::icons::{self, Icon};

/// Top-level application view modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppView {
    #[default]
    Player,
    Browser,
    Playlist,
    Recent,
    Settings,
}

impl AppView {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Player => "Player",
            Self::Browser => "Browser",
            Self::Playlist => "Playlist",
            Self::Recent => "History",
            Self::Settings => "Settings",
        }
    }

    pub fn icon(&self) -> Icon {
        match self {
            Self::Player => Icon::Play,
            Self::Browser => Icon::Folder,
            Self::Playlist => Icon::Playlist,
            Self::Recent => Icon::History,
            Self::Settings => Icon::Settings,
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
            ui.separator();
            ui.add_space(8.0);

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
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

                        ui.checkbox(
                            &mut self.active_styling.ass_override,
                            "Force override embedded ASS subtitle styles",
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

                    ui.add_space(10.0);

                    // Section 3: Compact Mini-Player Mode & Window Display
                    ui.group(|ui| {
                        ui.label(
                            RichText::new("3. Compact Mini-Player Mode & Window Display")
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

                    // Section 4: History & Persistence Controls
                    ui.group(|ui| {
                        ui.label(
                            RichText::new("4. History & Privacy Preferences")
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
}
