use eframe::egui::{self, Color32, RichText, Stroke, Vec2};

use crate::app::{format_timestamp, MuvrixApp};
use crate::branding::{self, BrandPalette};
use crate::icons::{self, Icon};
use crate::ui_components;
use crate::views::{format_file_size, AppView};

impl MuvrixApp {
    pub fn render_home_view(&mut self, ctx: &egui::Context) {
        self.load_home_sections(false);
        let mut play_file: Option<(String, Option<f64>)> = None;

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(8.0);

                    // 1. Hero Brand Card with Search & Quick Open
                    ui_components::hero_frame().show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let logo_tex = self.ensure_logo_texture(ui.ctx());
                            branding::render_logo(ui, logo_tex, 64.0);
                            ui.add_space(16.0);
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new("MUVRIX")
                                        .strong()
                                        .size(24.0)
                                        .color(BrandPalette::PRIMARY_TEXT),
                                );
                                ui.label(
                                    RichText::new(
                                        "Modern Native Media Player · Powered by libmpv 0.42.0",
                                    )
                                    .size(13.0)
                                    .color(BrandPalette::SECONDARY_TEXT),
                                );
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.horizontal(|ui| {
                                    ui_components::badge_pill(
                                        ui,
                                        "Ctrl+O",
                                        BrandPalette::SLATE_SURFACE,
                                        BrandPalette::ELECTRIC_LIME,
                                    );
                                    if ui_components::primary_button(
                                        ui,
                                        "Open Media File",
                                        Some("Open local media file (Ctrl+O)"),
                                    )
                                    .clicked()
                                    {
                                        self.open_file_dialog();
                                    }
                                });
                            });
                        });

                        ui.add_space(14.0);

                        // Unified Search Bar
                        let mut search_changed = false;
                        ui.horizontal(|ui| {
                            icons::icon_label(ui, Icon::Search, 15.0, BrandPalette::SECONDARY_TEXT);
                            ui.add_space(4.0);
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut self.settings_search)
                                    .hint_text("Search media catalog, settings & commands...")
                                    .desired_width((ui.available_width() - 80.0).max(180.0)),
                            );
                            if resp.changed() {
                                search_changed = true;
                                self.global_search_query = self.settings_search.clone();
                                self.global_search_active = !self.settings_search.trim().is_empty();
                            }
                            if !self.settings_search.is_empty() && ui.button("Clear").clicked() {
                                self.settings_search.clear();
                                self.global_search_query.clear();
                                self.global_search_active = false;
                                search_changed = true;
                            }
                        });
                        if search_changed {
                            let q = self.settings_search.clone();
                            self.run_global_search(&q);
                        }
                    });

                    ui.add_space(12.0);

                    // Phase C5: Global Search Results
                    if self.global_search_active && !self.global_search_query.trim().is_empty() {
                        let total_results = self.global_search_videos.len()
                            + self.global_search_audio.len()
                            + self.global_search_playlists.len();
                        ui_components::section_header(
                            ui,
                            &format!("Global Search Results for \"{}\"", self.global_search_query.trim()),
                            Some(total_results),
                            None,
                        );
                        ui.add_space(6.0);

                        ui_components::card_frame().show(ui, |ui| {
                            if total_results == 0 {
                                ui.label(
                                    RichText::new("No matching media or playlists found.")
                                        .color(BrandPalette::SECONDARY_TEXT)
                                        .size(12.5),
                                );
                            } else {
                                if !self.global_search_videos.is_empty() {
                                    ui.label(
                                        RichText::new(format!("Videos ({})", self.global_search_videos.len()))
                                            .strong()
                                            .color(BrandPalette::ELECTRIC_LIME),
                                    );
                                    for item in &self.global_search_videos {
                                        ui.horizontal(|ui| {
                                            icons::icon_label(ui, Icon::Play, 14.0, BrandPalette::ELECTRIC_LIME);
                                            let title = item.metadata.as_ref().and_then(|m| m.title.as_deref()).unwrap_or(&item.item.file_name);
                                            ui.label(RichText::new(title).size(12.5).color(BrandPalette::PRIMARY_TEXT));
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                if ui.button(RichText::new("▶ Play").strong().color(BrandPalette::ELECTRIC_LIME)).clicked() {
                                                    play_file = Some((item.item.canonical_path.clone(), None));
                                                }
                                                ui.label(RichText::new(format_file_size(item.item.file_size)).size(11.0).color(BrandPalette::MUTED_TEXT));
                                            });
                                        });
                                        ui.separator();
                                    }
                                }
                                if !self.global_search_audio.is_empty() {
                                    ui.add_space(4.0);
                                    ui.label(
                                        RichText::new(format!("Audio Tracks ({})", self.global_search_audio.len()))
                                            .strong()
                                            .color(Color32::from_rgb(140, 230, 120)),
                                    );
                                    for item in &self.global_search_audio {
                                        ui.horizontal(|ui| {
                                            icons::icon_label(ui, Icon::AudioTrack, 14.0, Color32::from_rgb(140, 230, 120));
                                            let title = item.metadata.as_ref().and_then(|m| m.title.as_deref()).unwrap_or(&item.item.file_name);
                                            ui.label(RichText::new(title).size(12.5).color(BrandPalette::PRIMARY_TEXT));
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                if ui.button(RichText::new("▶ Play").strong()).clicked() {
                                                    play_file = Some((item.item.canonical_path.clone(), None));
                                                }
                                                ui.label(RichText::new(format_file_size(item.item.file_size)).size(11.0).color(BrandPalette::MUTED_TEXT));
                                            });
                                        });
                                        ui.separator();
                                    }
                                }
                                if !self.global_search_playlists.is_empty() {
                                    ui.add_space(4.0);
                                    ui.label(
                                        RichText::new(format!("Playlists ({})", self.global_search_playlists.len()))
                                            .strong()
                                            .color(Color32::from_rgb(95, 185, 255)),
                                    );
                                    for pl in &self.global_search_playlists {
                                        ui.horizontal(|ui| {
                                            icons::icon_label(ui, Icon::Playlist, 14.0, Color32::from_rgb(95, 185, 255));
                                            ui.label(RichText::new(&pl.name).size(12.5).color(BrandPalette::PRIMARY_TEXT));
                                            ui.label(RichText::new(format!("({} items)", pl.item_count)).size(11.0).color(BrandPalette::MUTED_TEXT));
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                if ui.button("Open Playlist").clicked() {
                                                    self.selected_persistent_playlist = Some(pl.id);
                                                    self.current_view = AppView::Playlist;
                                                }
                                            });
                                        });
                                        ui.separator();
                                    }
                                }
                            }
                        });
                        ui.add_space(14.0);
                    }

                    // 1b. Library Overview Statistics Bar
                    let video_count = self
                        .library_items
                        .iter()
                        .filter(|i| i.item.media_type == muvrix_core::database::MediaType::Video)
                        .count();
                    let audio_count = self
                        .library_items
                        .iter()
                        .filter(|i| i.item.media_type == muvrix_core::database::MediaType::Audio)
                        .count();
                    let total_bytes: u64 =
                        self.library_items.iter().map(|item| item.item.file_size).sum();
                    let playlist_count = self.db.list_playlists().map(|p| p.len()).unwrap_or(0);

                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        let stat_pill = |ui: &mut egui::Ui, icon: Icon, count: &str, label: &str| {
                                egui::Frame::new()
                                    .fill(BrandPalette::ELEVATED_SURFACE)
                                    .stroke(egui::Stroke::new(1.0_f32, BrandPalette::SLATE_DIVIDER))
                                    .corner_radius(6.0_f32)
                                    .inner_margin(egui::Margin::symmetric(10, 5))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            icons::icon_label(
                                                ui,
                                                icon,
                                                13.0,
                                                BrandPalette::ELECTRIC_LIME,
                                            );
                                            ui.label(
                                                RichText::new(count)
                                                    .strong()
                                                    .size(12.0)
                                                    .color(BrandPalette::IVORY_TEXT),
                                            );
                                            ui.label(
                                                RichText::new(label)
                                                    .size(11.0)
                                                    .color(BrandPalette::MUTED_TEXT),
                                            );
                                        });
                                    });
                            };

                        stat_pill(ui, Icon::Play, &video_count.to_string(), "Videos");
                        stat_pill(
                            ui,
                            Icon::AudioTrack,
                            &audio_count.to_string(),
                            "Audio Tracks",
                        );
                        stat_pill(
                            ui,
                            Icon::Folder,
                            &format_file_size(total_bytes),
                            "Indexed",
                        );
                        stat_pill(
                            ui,
                            Icon::Playlist,
                            &playlist_count.to_string(),
                            "Playlists",
                        );
                    });

                    ui.add_space(16.0);

                    // 2. Quick Access Navigation Hub (Entire card clickable with hover feedback)
                    ui_components::section_header(ui, "Quick Access", None, None);
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let available_w = ui.available_width();
                        let card_w = ((available_w - 36.0) / 4.0).max(120.0);

                        let mut quick_nav: Option<AppView> = None;
                        let hub_card = |ui: &mut egui::Ui,
                                        icon: Icon,
                                        title: &str,
                                        subtitle: &str,
                                        target: AppView|
                         -> Option<AppView> {
                            let (rect, resp) =
                                ui.allocate_exact_size(Vec2::new(card_w, 64.0), egui::Sense::click());
                            let is_hovered = resp.hovered();
                            let fill = if is_hovered {
                                BrandPalette::ELEVATED_CARD
                            } else {
                                BrandPalette::SLATE_SURFACE
                            };
                            let stroke = if is_hovered {
                                Stroke::new(1.5_f32, BrandPalette::ELECTRIC_LIME)
                            } else {
                                Stroke::new(1.0_f32, BrandPalette::BORDER_SUBTLE)
                            };
                            ui.painter().rect(rect, 10.0_f32, fill, stroke, egui::StrokeKind::Inside);

                            let mut child_ui = ui.new_child(
                                egui::UiBuilder::default()
                                    .max_rect(rect.shrink(10.0))
                                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                            );
                            let icon_col = if is_hovered {
                                BrandPalette::ELECTRIC_LIME
                            } else {
                                BrandPalette::PRIMARY_TEXT
                            };
                            icons::icon_label(&mut child_ui, icon, 22.0, icon_col);
                            child_ui.add_space(10.0);
                            child_ui.vertical(|ui| {
                                ui.label(
                                    RichText::new(title)
                                        .strong()
                                        .size(13.5)
                                        .color(BrandPalette::PRIMARY_TEXT),
                                );
                                ui.label(
                                    RichText::new(subtitle)
                                        .size(11.0)
                                        .color(BrandPalette::SECONDARY_TEXT),
                                );
                            });

                            if is_hovered {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            if resp.clicked() {
                                Some(target)
                            } else {
                                None
                            }
                        };

                        if let Some(target) = hub_card(
                            ui,
                            Icon::Library,
                            "Library",
                            "Browse catalog",
                            AppView::Library,
                        ) {
                            quick_nav = Some(target);
                        }
                        if let Some(target) = hub_card(
                            ui,
                            Icon::FolderOpen,
                            "Folders",
                            "File explorer",
                            AppView::Browser,
                        ) {
                            quick_nav = Some(target);
                        }
                        if let Some(target) = hub_card(
                            ui,
                            Icon::Playlist,
                            "Playlists",
                            "Queue & M3U",
                            AppView::Playlist,
                        ) {
                            quick_nav = Some(target);
                        }
                        if let Some(target) = hub_card(
                            ui,
                            Icon::Settings,
                            "Settings",
                            "Audio & engine",
                            AppView::Settings,
                        ) {
                            quick_nav = Some(target);
                        }
                        if let Some(nav) = quick_nav {
                            self.current_view = nav;
                        }
                    });

                    ui.add_space(20.0);

                    // 3. Continue Watching (Recently Played)
                    let recent_entries: Vec<muvrix_core::history::MediaHistoryEntry> =
                        self.history_store.entries.clone();

                    let avail_w = ui.available_width();
                    let num_visible = if avail_w >= 1600.0 {
                        5.0
                    } else if avail_w >= 1200.0 {
                        4.0
                    } else if avail_w >= 850.0 {
                        3.0
                    } else {
                        2.0
                    };
                    let card_w = ((avail_w - (num_visible - 1.0) * 12.0) / num_visible).max(240.0);

                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Continue Watching")
                                .strong()
                                .size(16.0)
                                .color(BrandPalette::PRIMARY_TEXT),
                        );
                        if !recent_entries.is_empty() {
                            ui_components::badge_pill(
                                ui,
                                &recent_entries.len().to_string(),
                                BrandPalette::ELEVATED_CARD,
                                BrandPalette::ELECTRIC_LIME,
                            );
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if !recent_entries.is_empty() {
                                if ui
                                    .link(
                                        RichText::new("View Full History →")
                                            .strong()
                                            .size(12.5)
                                            .color(BrandPalette::ELECTRIC_LIME),
                                    )
                                    .clicked()
                                {
                                    self.current_view = AppView::Recent;
                                }
                                ui.add_space(8.0);
                                if ui
                                    .button(
                                        RichText::new("Next ▶")
                                            .size(12.0)
                                            .color(BrandPalette::PRIMARY_TEXT),
                                    )
                                    .on_hover_text("Scroll carousel to next items")
                                    .clicked()
                                {
                                    self.home_carousel_scroll_offset += card_w + 12.0;
                                }
                                if ui
                                    .button(
                                        RichText::new("◀ Prev")
                                            .size(12.0)
                                            .color(BrandPalette::PRIMARY_TEXT),
                                    )
                                    .on_hover_text("Scroll carousel to previous items")
                                    .clicked()
                                {
                                    self.home_carousel_scroll_offset =
                                        (self.home_carousel_scroll_offset - (card_w + 12.0)).max(0.0);
                                }
                            }
                        });
                    });

                    ui.add_space(8.0);

                    if recent_entries.is_empty() {
                        ui_components::empty_state_box(
                            ui,
                            Some(Icon::History),
                            "No recently watched media",
                            "Play a video or audio file to pick up where you left off.",
                            None,
                        );
                    } else {
                        egui::ScrollArea::horizontal()
                            .id_salt("home_continue_watching_scroll")
                            .auto_shrink([false, false])
                            .horizontal_scroll_offset(self.home_carousel_scroll_offset)
                            .drag_to_scroll(true)
                            .scroll_bar_visibility(
                                egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded,
                            )
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 12.0;
                                    for entry in recent_entries.iter().take(12) {
                                        let file_exists =
                                            std::path::Path::new(&entry.path).exists();
                                        ui_components::highlighted_card_frame().show(ui, |ui| {
                                            ui.set_width(card_w);
                                            ui.set_max_width(card_w);
                                            ui.vertical(|ui| {
                                                ui.horizontal(|ui| {
                                                    icons::icon_label(
                                                        ui,
                                                        Icon::Play,
                                                        18.0,
                                                        if file_exists {
                                                            BrandPalette::ELECTRIC_LIME
                                                        } else {
                                                            BrandPalette::MUTED_TEXT
                                                        },
                                                    );
                                                    ui.add_space(4.0);
                                                    let name = &entry.display_name;
                                                    let label_w = (card_w - 48.0).max(100.0);
                                                    ui.add_sized(
                                                        [label_w, 18.0],
                                                        egui::Label::new(
                                                            RichText::new(name)
                                                                .strong()
                                                                .size(13.0)
                                                                .color(BrandPalette::IVORY_TEXT),
                                                        )
                                                        .truncate(),
                                                    );
                                                });

                                                if !file_exists {
                                                    ui.add_space(2.0);
                                                    ui_components::badge_pill(
                                                        ui,
                                                        "Missing File",
                                                        BrandPalette::OBSIDIAN,
                                                        Color32::from_rgb(220, 100, 100),
                                                    );
                                                }

                                                ui.add_space(6.0);
                                                let progress = if entry.duration_sec > 0.0 {
                                                    (entry.last_position_sec / entry.duration_sec)
                                                        .clamp(0.0, 1.0)
                                                        as f32
                                                } else {
                                                    0.0
                                                };
                                                ui.add(
                                                    egui::ProgressBar::new(progress)
                                                        .desired_height(4.0)
                                                        .fill(BrandPalette::ELECTRIC_LIME),
                                                );
                                                ui.add_space(4.0);
                                                ui.horizontal(|ui| {
                                                    ui.label(
                                                        RichText::new(format!(
                                                            "{:.0}% · {} / {}",
                                                            progress * 100.0,
                                                            format_timestamp(
                                                                entry.last_position_sec
                                                            ),
                                                            format_timestamp(entry.duration_sec)
                                                        ))
                                                        .size(11.0)
                                                        .color(BrandPalette::MUTED_TEXT),
                                                    );
                                                });

                                                ui.add_space(6.0);
                                                ui.horizontal(|ui| {
                                                    if file_exists {
                                                        if ui
                                                            .button(
                                                                RichText::new("▶ Resume")
                                                                    .color(
                                                                        BrandPalette::ELECTRIC_LIME,
                                                                    )
                                                                    .strong(),
                                                            )
                                                            .clicked()
                                                        {
                                                            play_file = Some((
                                                                entry.path.clone(),
                                                                Some(entry.last_position_sec),
                                                            ));
                                                        }
                                                    } else {
                                                        ui.label(
                                                            RichText::new("Not available")
                                                                .size(11.0)
                                                                .color(BrandPalette::MUTED_TEXT),
                                                        );
                                                    }
                                                });
                                            });
                                        });
                                    }
                                });
                            });
                    }

                    ui.add_space(20.0);

                    // 4. Media Catalog Highlights
                    if ui_components::section_header(
                        ui,
                        "Indexed Media Catalog",
                        Some(self.library_total_count as usize),
                        Some("Open Full Library →"),
                    ) == Some(true)
                    {
                        self.current_view = AppView::Library;
                    }

                    ui.add_space(8.0);
                    if self.library_items.is_empty() {
                        if ui_components::empty_state_box(
                            ui,
                            Some(Icon::Library),
                            "No media directories indexed yet",
                            "Add media root folders to index movies, videos, and music into Muvrix.",
                            Some("Add Library Root Folder..."),
                        ) == Some(true)
                        {
                            self.add_root_dialog_open = true;
                            self.current_view = AppView::Library;
                        }
                    } else {
                        ui_components::card_frame().show(ui, |ui| {
                            for item_view in self.library_items.iter().take(6) {
                                ui.horizontal(|ui| {
                                    let icon = if item_view.item.media_type
                                        == muvrix_core::database::MediaType::Audio
                                    {
                                        Icon::AudioTrack
                                    } else {
                                        Icon::Play
                                    };
                                    icons::icon_label(
                                        ui,
                                        icon,
                                        16.0,
                                        BrandPalette::ELECTRIC_LIME,
                                    );
                                    let title = item_view
                                        .metadata
                                        .as_ref()
                                        .and_then(|m| m.title.as_deref())
                                        .unwrap_or(&item_view.item.file_name);
                                    ui.label(
                                        RichText::new(title)
                                            .size(13.0)
                                            .strong()
                                            .color(BrandPalette::IVORY_TEXT),
                                    );

                                    // Resolution & Codec Badges
                                    if let Some(ref meta) = item_view.metadata {
                                        if meta.width >= 3840 {
                                            ui_components::badge_pill(
                                                ui,
                                                "4K",
                                                BrandPalette::OBSIDIAN,
                                                BrandPalette::ELECTRIC_LIME,
                                            );
                                        } else if meta.width >= 1920 {
                                            ui_components::badge_pill(
                                                ui,
                                                "1080p",
                                                BrandPalette::OBSIDIAN,
                                                BrandPalette::IVORY_TEXT,
                                            );
                                        } else if meta.width >= 1280 {
                                            ui_components::badge_pill(
                                                ui,
                                                "720p",
                                                BrandPalette::OBSIDIAN,
                                                BrandPalette::SECONDARY_TEXT,
                                            );
                                        }
                                        if let Some(ref vc) = meta.video_codec {
                                            ui_components::badge_pill(
                                                ui,
                                                vc,
                                                BrandPalette::OBSIDIAN,
                                                BrandPalette::SECONDARY_TEXT,
                                            );
                                        }
                                        if meta.duration_ms > 0 {
                                            let dur_sec = meta.duration_ms as f64 / 1000.0;
                                            ui.label(
                                                RichText::new(format_timestamp(dur_sec))
                                                    .size(11.0)
                                                    .color(BrandPalette::MUTED_TEXT),
                                            );
                                        }
                                    }

                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui
                                                .button(
                                                    RichText::new("▶ Play")
                                                        .color(BrandPalette::ELECTRIC_LIME)
                                                        .strong(),
                                                )
                                                .clicked()
                                            {
                                                play_file = Some((
                                                    item_view.item.canonical_path.clone(),
                                                    None,
                                                ));
                                            }
                                            ui.label(
                                                RichText::new(format_file_size(
                                                    item_view.item.file_size,
                                                ))
                                                .size(11.0)
                                                .color(BrandPalette::MUTED_TEXT),
                                            );
                                        },
                                    );
                                });
                                ui.separator();
                            }
                        });
                    }

                    // Phase C5: Quick Access Section: Recently Added
                    if !self.home_recently_added.is_empty() {
                        ui.add_space(14.0);
                        ui_components::section_header(
                            ui,
                            "Recently Added to Library",
                            Some(self.home_recently_added.len()),
                            Some("View Library →"),
                        );
                        ui.add_space(6.0);
                        ui_components::card_frame().show(ui, |ui| {
                            for item in self.home_recently_added.iter().take(4) {
                                ui.horizontal(|ui| {
                                    let icon = if item.item.media_type == muvrix_core::database::MediaType::Audio {
                                        Icon::AudioTrack
                                    } else {
                                        Icon::Play
                                    };
                                    icons::icon_label(ui, icon, 14.0, BrandPalette::ELECTRIC_LIME);
                                    let title = item.metadata.as_ref().and_then(|m| m.title.as_deref()).unwrap_or(&item.item.file_name);
                                    ui.label(RichText::new(title).size(12.5).color(BrandPalette::PRIMARY_TEXT));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.button(RichText::new("▶ Play").color(BrandPalette::ELECTRIC_LIME).strong()).clicked() {
                                            play_file = Some((item.item.canonical_path.clone(), None));
                                        }
                                        ui.label(RichText::new(format_file_size(item.item.file_size)).size(11.0).color(BrandPalette::MUTED_TEXT));
                                    });
                                });
                                ui.separator();
                            }
                        });
                    }

                    // Phase C5: Quick Access Section: Favorites
                    if !self.home_favorites.is_empty() {
                        ui.add_space(14.0);
                        ui_components::section_header(
                            ui,
                            "Favorite Media",
                            Some(self.home_favorites.len()),
                            Some("View Favorites →"),
                        );
                        ui.add_space(6.0);
                        ui_components::card_frame().show(ui, |ui| {
                            for item in self.home_favorites.iter().take(4) {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("★").color(Color32::from_rgb(255, 200, 50)));
                                    let title = item.metadata.as_ref().and_then(|m| m.title.as_deref()).unwrap_or(&item.item.file_name);
                                    ui.label(RichText::new(title).size(12.5).color(BrandPalette::PRIMARY_TEXT));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.button(RichText::new("▶ Play").color(BrandPalette::ELECTRIC_LIME).strong()).clicked() {
                                            play_file = Some((item.item.canonical_path.clone(), None));
                                        }
                                    });
                                });
                                ui.separator();
                            }
                        });
                    }

                    // Phase C5: Quick Access Section: Longest Media
                    if !self.home_longest.is_empty() {
                        ui.add_space(14.0);
                        ui_components::section_header(
                            ui,
                            "Longest Duration Media",
                            Some(self.home_longest.len()),
                            None,
                        );
                        ui.add_space(6.0);
                        ui_components::card_frame().show(ui, |ui| {
                            for item in self.home_longest.iter().take(3) {
                                ui.horizontal(|ui| {
                                    icons::icon_label(ui, Icon::Play, 14.0, BrandPalette::PRIMARY_TEXT);
                                    let title = item.metadata.as_ref().and_then(|m| m.title.as_deref()).unwrap_or(&item.item.file_name);
                                    ui.label(RichText::new(title).size(12.5).color(BrandPalette::PRIMARY_TEXT));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.button(RichText::new("▶ Play").color(BrandPalette::ELECTRIC_LIME).strong()).clicked() {
                                            play_file = Some((item.item.canonical_path.clone(), None));
                                        }
                                        if let Some(ref m) = item.metadata {
                                            if m.duration_ms > 0 {
                                                ui.label(RichText::new(format_timestamp(m.duration_ms as f64 / 1000.0)).size(11.0).color(BrandPalette::MUTED_TEXT));
                                            }
                                        }
                                    });
                                });
                                ui.separator();
                            }
                        });
                    }

                    // Phase C5: Quick Access Section: Largest Files
                    if !self.home_largest.is_empty() {
                        ui.add_space(14.0);
                        ui_components::section_header(
                            ui,
                            "Largest Media Files",
                            Some(self.home_largest.len()),
                            None,
                        );
                        ui.add_space(6.0);
                        ui_components::card_frame().show(ui, |ui| {
                            for item in self.home_largest.iter().take(3) {
                                ui.horizontal(|ui| {
                                    icons::icon_label(ui, Icon::Folder, 14.0, BrandPalette::PRIMARY_TEXT);
                                    let title = item.metadata.as_ref().and_then(|m| m.title.as_deref()).unwrap_or(&item.item.file_name);
                                    ui.label(RichText::new(title).size(12.5).color(BrandPalette::PRIMARY_TEXT));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.button(RichText::new("▶ Play").color(BrandPalette::ELECTRIC_LIME).strong()).clicked() {
                                            play_file = Some((item.item.canonical_path.clone(), None));
                                        }
                                        ui.label(RichText::new(format_file_size(item.item.file_size)).size(11.0).color(BrandPalette::MUTED_TEXT));
                                    });
                                });
                                ui.separator();
                            }
                        });
                    }

                    ui.add_space(24.0);
                });
        });

        if let Some((path, resume_sec)) = play_file {
            if let Some(pos) = resume_sec {
                self.load_file_and_resume(&path, pos);
            } else {
                self.load_file(&path);
            }
        }
    }
}
