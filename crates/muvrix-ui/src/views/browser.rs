use std::path::PathBuf;

use eframe::egui::{self, Color32, RichText, Stroke};
use muvrix_core::browser::{EntryKind, MediaFilter, SortBy};
use muvrix_core::network::{
    JellyfinProvider, NetworkConnection, NetworkProvider, NfsProvider, NfsVersion, PlexProvider,
    SftpProvider, SmbProvider, WebDavProvider,
};

use crate::app::MuvrixApp;
use crate::branding::BrandPalette;
use crate::icons::{self, Icon};
use crate::remote_playback::{connection_username, smb_share};
use crate::ui_components;
use crate::views::{format_file_size, AppView};

impl MuvrixApp {
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

                    // Phase C2: Grid / List view mode toggle
                    ui.group(|ui| {
                        if ui.selectable_label(self.browser_is_grid, "Grid").clicked() {
                            self.browser_is_grid = true;
                        }
                        if ui.selectable_label(!self.browser_is_grid, "List").clicked() {
                            self.browser_is_grid = false;
                        }
                    });
                });
            });

            ui.add_space(4.0);
            ui.separator();

            // Phase C2: Browser Entry Context Menu Dialog
            if self.show_browser_context_menu {
                if let Some(ref entry_path) = self.browser_context_entry {
                    let mut close_menu = false;
                    let mut is_open = true;
                    let is_dir = entry_path.is_dir();
                    let display_name = entry_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("Entry")
                        .to_string();
                    let path_str = entry_path.to_string_lossy().to_string();
                    let path_buf = entry_path.clone();

                    egui::Window::new(format!("Options: {display_name}"))
                        .collapsible(false)
                        .resizable(false)
                        .default_width(260.0)
                        .open(&mut is_open)
                        .show(ctx, |ui| {
                            ui.vertical(|ui| {
                                if is_dir {
                                    if ui.button("📁 Open Folder").clicked() {
                                        self.media_browser.navigate_to(path_buf.clone());
                                        close_menu = true;
                                    }
                                    if ui.button("➕ Add as Media Library Root").clicked() {
                                        let root = muvrix_core::database::LibraryRoot {
                                            id: 0,
                                            display_name: display_name.clone(),
                                            root_path: path_str.clone(),
                                            source_type: "local".to_string(),
                                            is_recursive: true,
                                            is_enabled: true,
                                            exclusion_patterns: Vec::new(),
                                            last_scanned_at: 0,
                                            scan_status: "idle".to_string(),
                                        };
                                        if let Ok(id) = self.db.insert_root(&root) {
                                            self.start_library_scan(id);
                                            self.status_message =
                                                format!("Added library root: {display_name}");
                                        }
                                        close_menu = true;
                                    }
                                } else {
                                    if ui.button("▶ Play Now").clicked() {
                                        self.load_file(&path_str);
                                        self.current_view = AppView::Player;
                                        close_menu = true;
                                    }
                                    if ui.button("➕ Enqueue to Playlist").clicked() {
                                        let _ = self.controller.playlist_append(&path_str);
                                        self.status_message =
                                            format!("Added to playlist: {display_name}");
                                        close_menu = true;
                                    }
                                }
                                if ui.button("📋 Copy Path").clicked() {
                                    ui.ctx().copy_text(path_str.clone());
                                    self.status_message = "Path copied to clipboard".to_string();
                                    close_menu = true;
                                }
                            });
                        });
                    if close_menu || !is_open {
                        self.show_browser_context_menu = false;
                        self.browser_context_entry = None;
                    }
                }
            }

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
                ui.add_space(20.0);
                if ui_components::empty_state_box(
                    ui,
                    Some(Icon::FolderOpen),
                    "No Media Files Found",
                    "No media files or directories found matching current filter.",
                    Some("Open Folder in Dialog..."),
                ) == Some(true)
                {
                    if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                        self.media_browser.navigate_to(folder);
                    }
                }
                return;
            }

            let mut dir_to_open: Option<PathBuf> = None;
            let mut file_to_play: Option<String> = None;
            let mut file_to_enqueue: Option<String> = None;

            if self.browser_is_grid {
                // Phase C2: Grid View for Browser
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let available = ui.available_width();
                        let usable_width = (available - 48.0).max(220.0);
                        let (cols, spacing) = if usable_width >= 960.0 {
                            (4, 12.0_f32)
                        } else if usable_width >= 640.0 {
                            (3, 10.0_f32)
                        } else if usable_width >= 360.0 {
                            (2, 8.0_f32)
                        } else {
                            (1, 6.0_f32)
                        };
                        let card_width =
                            ((usable_width - ((cols - 1) as f32 * spacing)) / cols as f32).floor();

                        for chunk in filtered_entries.chunks(cols) {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = spacing;
                                for entry in chunk {
                                    egui::Frame::new()
                                        .fill(BrandPalette::SLATE_SURFACE)
                                        .stroke(Stroke::new(1.0_f32, BrandPalette::BORDER_SUBTLE))
                                        .corner_radius(8.0)
                                        .inner_margin(egui::Margin::same(10))
                                        .show(ui, |ui| {
                                            let inner_w = (card_width - 24.0).max(120.0);
                                            ui.set_width(inner_w);
                                            ui.set_max_width(inner_w);
                                            ui.vertical(|ui| match entry.kind {
                                                EntryKind::Directory => {
                                                    icons::icon_label(
                                                        ui,
                                                        Icon::Folder,
                                                        28.0,
                                                        Color32::from_rgb(255, 195, 75),
                                                    );
                                                    ui.add_space(4.0);
                                                    ui.add(
                                                        egui::Label::new(
                                                            RichText::new(&entry.name)
                                                                .strong()
                                                                .color(BrandPalette::PRIMARY_TEXT),
                                                        )
                                                        .truncate(),
                                                    );
                                                    ui.label(
                                                        RichText::new("Folder")
                                                            .size(10.5)
                                                            .color(BrandPalette::SECONDARY_TEXT),
                                                    );
                                                    ui.add_space(6.0);
                                                    ui.horizontal(|ui| {
                                                        if ui.button("Open").clicked() {
                                                            dir_to_open = Some(entry.path.clone());
                                                        }
                                                        if ui
                                                            .button("⋮")
                                                            .on_hover_text("Options")
                                                            .clicked()
                                                        {
                                                            self.browser_context_entry =
                                                                Some(entry.path.clone());
                                                            self.show_browser_context_menu = true;
                                                        }
                                                    });
                                                }
                                                EntryKind::VideoFile | EntryKind::AudioFile => {
                                                    let icon = if entry.kind == EntryKind::VideoFile
                                                    {
                                                        Icon::Play
                                                    } else {
                                                        Icon::AudioTrack
                                                    };
                                                    let icon_col =
                                                        if entry.kind == EntryKind::VideoFile {
                                                            BrandPalette::ELECTRIC_LIME
                                                        } else {
                                                            Color32::from_rgb(140, 230, 120)
                                                        };
                                                    icons::icon_label(ui, icon, 24.0, icon_col);
                                                    ui.add_space(4.0);
                                                    ui.add(
                                                        egui::Label::new(
                                                            RichText::new(&entry.name)
                                                                .strong()
                                                                .color(BrandPalette::PRIMARY_TEXT),
                                                        )
                                                        .truncate(),
                                                    );
                                                    ui.label(
                                                        RichText::new(format_file_size(
                                                            entry.size_bytes,
                                                        ))
                                                        .size(10.5)
                                                        .color(BrandPalette::SECONDARY_TEXT),
                                                    );
                                                    ui.add_space(6.0);
                                                    ui.horizontal(|ui| {
                                                        if ui
                                                            .button(
                                                                RichText::new("▶ Play")
                                                                    .strong()
                                                                    .color(
                                                                        BrandPalette::ELECTRIC_LIME,
                                                                    ),
                                                            )
                                                            .clicked()
                                                        {
                                                            file_to_play = Some(
                                                                entry
                                                                    .path
                                                                    .to_string_lossy()
                                                                    .to_string(),
                                                            );
                                                        }
                                                        if ui
                                                            .button("⋮")
                                                            .on_hover_text("Options")
                                                            .clicked()
                                                        {
                                                            self.browser_context_entry =
                                                                Some(entry.path.clone());
                                                            self.show_browser_context_menu = true;
                                                        }
                                                    });
                                                }
                                                EntryKind::OtherFile => {
                                                    icons::icon_label(
                                                        ui,
                                                        Icon::Folder,
                                                        20.0,
                                                        BrandPalette::MUTED_TEXT,
                                                    );
                                                    ui.add_space(4.0);
                                                    ui.add(
                                                        egui::Label::new(
                                                            RichText::new(&entry.name)
                                                                .color(BrandPalette::MUTED_TEXT),
                                                        )
                                                        .truncate(),
                                                    );
                                                    ui.label(
                                                        RichText::new(format_file_size(
                                                            entry.size_bytes,
                                                        ))
                                                        .size(10.5)
                                                        .color(BrandPalette::MUTED_TEXT),
                                                    );
                                                }
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
                                            ui.label(
                                                RichText::new("-").color(Color32::from_gray(120)),
                                            );
                                            ui.horizontal(|ui| {
                                                if ui.button("Open").clicked() {
                                                    dir_to_open = Some(entry.path.clone());
                                                }
                                                if ui.button("⋮").on_hover_text("Options").clicked()
                                                {
                                                    self.browser_context_entry =
                                                        Some(entry.path.clone());
                                                    self.show_browser_context_menu = true;
                                                }
                                            });
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
                                                    file_to_play = Some(
                                                        entry.path.to_string_lossy().to_string(),
                                                    );
                                                }
                                                if icons::icon_text_button(
                                                    ui,
                                                    Icon::Plus,
                                                    "Queue",
                                                    "Add to playlist queue",
                                                )
                                                .clicked()
                                                {
                                                    file_to_enqueue = Some(
                                                        entry.path.to_string_lossy().to_string(),
                                                    );
                                                }
                                                if ui.button("⋮").on_hover_text("Options").clicked()
                                                {
                                                    self.browser_context_entry =
                                                        Some(entry.path.clone());
                                                    self.show_browser_context_menu = true;
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
            }

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
            "nfs" => {
                let provider = NfsProvider::new(
                    &conn.host,
                    conn.port,
                    &conn.remote_path,
                    NfsVersion::V4,
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
                        self.browsing_error = Some(format!("NFS: {e}"));
                    }
                }
            }
            "jellyfin" => {
                let token = conn
                    .credential_ref
                    .as_deref()
                    .and_then(|cr| self.network_vault.get(cr));
                let is_https = conn.port == 8920 || conn.port == 443;
                let provider = JellyfinProvider::new(
                    &conn.host,
                    conn.port,
                    is_https,
                    token,
                    conn.credential_ref.clone(),
                    None,
                    self.network_vault.clone(),
                );
                match provider.list_directory(&path) {
                    Ok(entries) => {
                        self.browsing_entries = entries;
                        self.browsing_error = None;
                    }
                    Err(e) => {
                        self.browsing_error = Some(format!("Jellyfin: {e}"));
                    }
                }
            }
            "plex" => {
                let token = conn
                    .credential_ref
                    .as_deref()
                    .and_then(|cr| self.network_vault.get(cr));
                let is_https = conn.port == 443;
                let provider = PlexProvider::new(
                    &conn.host,
                    conn.port,
                    is_https,
                    token,
                    conn.credential_ref.clone(),
                    self.network_vault.clone(),
                );
                match provider.list_directory(&path) {
                    Ok(entries) => {
                        self.browsing_entries = entries;
                        self.browsing_error = None;
                    }
                    Err(e) => {
                        self.browsing_error = Some(format!("Plex: {e}"));
                    }
                }
            }
            _ => {
                self.browsing_error = Some(format!("Unsupported protocol: {}", conn.protocol));
            }
        }
    }
}
