use eframe::egui::{self, Color32, RichText, Vec2};

use crate::app::{format_codec_name, MuvrixApp};
use crate::branding::{self, BrandPalette};
use crate::icons::{self, Icon};
use crate::ui_components;
use crate::views::AppView;
use muvrix_player::AccentColor;

impl MuvrixApp {
    pub(crate) fn render_navigation_bar(&mut self, ctx: &egui::Context) {
        // ---------------------------------------------------------------------
        // 1. Top Identity, Navigation & Player Header Panel
        // ---------------------------------------------------------------------
        egui::TopBottomPanel::top("player_top_bar")
            .resizable(false)
            .frame(
                egui::Frame::NONE
                    .fill(BrandPalette::OBSIDIAN)
                    .stroke(egui::Stroke::new(1.0_f32, BrandPalette::BORDER_SLATE))
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            )
            .show(ctx, |ui| {
                // The navigation bar is application chrome, not part of the
                // player HUD. Tying it to HUD visibility used to collapse the
                // panel to its margins when the HUD faded, which resized the
                // video on every hover transition.
                ui.add_space(3.0);
                let available_w = ui.available_width();

                ui.horizontal(|ui| {
                    // Application branding / identity with official circular emblem
                    let logo_tex = self.ensure_logo_texture(ui.ctx());
                    branding::render_logo(ui, logo_tex, 24.0);
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new("MUVRIX")
                            .strong()
                            .size(13.5)
                            .color(if self.accent_color == AccentColor::Lime {
                                BrandPalette::ELECTRIC_LIME
                            } else {
                                Color32::from_rgb(80, 180, 255)
                            }),
                    );

                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    let is_wide = available_w >= 1400.0;
                    // Below this width the header cannot carry tabs, file
                    // actions, title and the right-hand toolbar side by side;
                    // egui would overlap them instead of clipping.
                    let is_narrow = available_w < 900.0;
                    if is_wide {
                        // At 1400px+ show all 8 tabs
                        for view in &[
                            AppView::Home,
                            AppView::Player,
                            AppView::Library,
                            AppView::Browser,
                            AppView::Playlist,
                            AppView::Lyrics,
                            AppView::Recent,
                            AppView::Settings,
                        ] {
                            let is_active = self.current_view == *view;
                            let text = if *view == AppView::Playlist
                                && self.controller.playlist_count() > 0
                            {
                                format!("Playlist ({})", self.controller.playlist_count())
                            } else {
                                view.label().to_string()
                            };

                            if ui_components::nav_tab(ui, &text, is_active).clicked() {
                                self.current_view = *view;
                            }
                        }
                    } else {
                        // At <1400px show primary tabs + More dropdown for
                        // secondary tabs; at <900px only the three core tabs
                        // stay inline and everything else moves into More.
                        let primary: &[AppView] = if is_narrow {
                            &[AppView::Home, AppView::Player, AppView::Library]
                        } else {
                            &[
                                AppView::Home,
                                AppView::Player,
                                AppView::Library,
                                AppView::Playlist,
                                AppView::Settings,
                            ]
                        };
                        for view in primary {
                            let is_active = self.current_view == *view;
                            let text = if *view == AppView::Playlist
                                && self.controller.playlist_count() > 0
                            {
                                format!("Playlist ({})", self.controller.playlist_count())
                            } else {
                                view.label().to_string()
                            };

                            if ui_components::nav_tab(ui, &text, is_active).clicked() {
                                self.current_view = *view;
                            }
                        }

                        // Secondary views dropdown (Files / Lyrics / History)
                        let secondary_views: &[(AppView, &str)] = if is_narrow {
                            &[
                                (AppView::Browser, "Files (Browser)"),
                                (AppView::Playlist, "Playlist"),
                                (AppView::Lyrics, "Lyrics"),
                                (AppView::Recent, "History (Recent)"),
                                (AppView::Settings, "Settings"),
                            ]
                        } else {
                            &[
                                (AppView::Browser, "Files (Browser)"),
                                (AppView::Lyrics, "Lyrics"),
                                (AppView::Recent, "History (Recent)"),
                            ]
                        };
                        let active_secondary = secondary_views.iter().find(|(v, _)| *v == self.current_view);
                        let more_label = if let Some((_, label)) = active_secondary {
                            format!("{} ▾", label)
                        } else {
                            "More ▾".to_string()
                        };
                        let is_sec_active = active_secondary.is_some();
                        let more_rich = if is_sec_active {
                            RichText::new(&more_label)
                                .strong()
                                .color(BrandPalette::ELECTRIC_LIME)
                        } else {
                            RichText::new(&more_label).color(BrandPalette::SECONDARY_TEXT)
                        };

                        egui::ComboBox::from_id_salt("nav_more_views_combo")
                            .selected_text(more_rich)
                            .show_ui(ui, |ui| {
                                for (view, label) in secondary_views {
                                    let is_sel = self.current_view == *view;
                                    if ui_components::nav_tab(ui, label, is_sel).clicked() {
                                        self.current_view = *view;
                                    }
                                }
                            });
                    }

                    ui.add_space(3.0);
                    ui.separator();
                    ui.add_space(3.0);

                    // User file actions with clean vector icons
                    if is_wide {
                        if icons::icon_text_button(
                            ui,
                            Icon::FolderOpen,
                            "Open File...",
                            "Open media file from disk (Ctrl+O)",
                        )
                        .clicked()
                        {
                            self.open_file_dialog();
                        }

                        if icons::icon_text_button(
                            ui,
                            Icon::Subtitles,
                            "Add Subs...",
                            "Load external subtitle file (SRT, ASS, VTT)",
                        )
                        .clicked()
                        {
                            self.add_subtitle_dialog();
                        }
                    } else {
                        if icons::icon_button(
                            ui,
                            Icon::FolderOpen,
                            "Open media file from disk (Ctrl+O)",
                        )
                        .clicked()
                        {
                            self.open_file_dialog();
                        }

                        if icons::icon_button(
                            ui,
                            Icon::Subtitles,
                            "Load external subtitle file (SRT, ASS, VTT)",
                        )
                        .clicked()
                        {
                            self.add_subtitle_dialog();
                        }
                    }

                    ui.add_space(3.0);
                    ui.separator();
                    ui.add_space(3.0);

                    // Media file title / label with collision protection
                    let file_label = self
                        .controller
                        .current_file()
                        .map(|path| {
                            std::path::Path::new(path)
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or(path)
                                .to_string()
                        })
                        .unwrap_or_else(|| "No media loaded".to_string());

                    let is_loaded = self.controller.current_file().is_some();
                    let label_color = if is_loaded {
                        BrandPalette::PRIMARY_TEXT
                    } else {
                        BrandPalette::SECONDARY_TEXT
                    };

                    // Compute dynamic right-side reserved width
                    let (w, h) = self.controller.video_dimensions();
                    let vfmt = self.controller.video_format().to_string();
                    let afmt = self.controller.audio_codec().to_string();
                    let hw = self.controller.hwdec_current().to_string();
                    let has_media_meta = w > 0 || !vfmt.is_empty();

                    let is_player = self.current_view == AppView::Player;
                    let player_extra_w = if is_player { 68.0 } else { 0.0 };
                    let dev_w = if is_wide { 95.0 } else { 60.0 };
                    let help_w = if is_wide { 110.0 } else { 34.0 };
                    let badge_w = if has_media_meta {
                        if is_wide { 240.0 } else { 90.0 }
                    } else {
                        0.0
                    };
                    let right_padding = 40.0;
                    let total_right_w = if is_narrow {
                        0.0
                    } else {
                        player_extra_w + dev_w + help_w + badge_w + right_padding
                    };

                    let remaining_w = ui.available_width();
                    let title_w = (remaining_w - total_right_w).max(40.0);

                    // Allocated middle title with truncation guarantee. Narrow
                    // windows drop it entirely: the player top overlay already
                    // names the file, and a 40 px stub only added noise.
                    if !is_narrow {
                        ui.allocate_ui_with_layout(
                            Vec2::new(title_w, ui.available_height()),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&file_label)
                                            .strong()
                                            .color(label_color),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(&file_label);
                            },
                        );
                    }

                    // Right-aligned toolbar section
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // In Player view, offer Drawer and PiP toggles
                        if self.current_view == AppView::Player {
                            let drawer_tip = if self.show_playlist_drawer {
                                "Hide Queue Drawer (P)"
                            } else {
                                "Show Queue Drawer (P)"
                            };
                            if icons::icon_button(ui, Icon::Playlist, drawer_tip).clicked() {
                                self.show_playlist_drawer = !self.show_playlist_drawer;
                            }

                            let pip_tip = if self.is_pip {
                                "Exit Compact Mini-Player Mode (I)"
                            } else {
                                "Compact Mini-Player Mode (Always-on-Top, best-effort on Wayland) (I)"
                            };
                            if icons::icon_button(ui, Icon::Pip, pip_tip).clicked() {
                                self.toggle_pip(ctx);
                            }

                            ui.separator();
                        }

                        // Profile and developer menus are the first things to
                        // go when the header has no room for them.
                        if !is_narrow {
                        // Phase E7: Profile Selector Combo Box
                        let active_name = self
                            .active_profile
                            .as_ref()
                            .map(|p| p.name.as_str())
                            .unwrap_or("Default");
                        let prof_label = format!("👤 {active_name} ▾");
                        let prof_rich = RichText::new(prof_label)
                            .strong()
                            .color(BrandPalette::ELECTRIC_LIME);

                        let combo_salt = "nav_profile_selector_combo";
                        let button_id = ui.make_persistent_id(egui::Id::new(combo_salt));
                        let popup_id = button_id.with("popup");
                        if std::env::var("MUVRIX_OPEN_PROFILE_SELECTOR").is_ok() {
                            ui.memory_mut(|mem| mem.open_popup(popup_id));
                        }

                        egui::ComboBox::from_id_salt(combo_salt)
                            .selected_text(prof_rich)
                            .show_ui(ui, |ui| {
                                ui.label(
                                    RichText::new("Playback Profiles")
                                        .size(11.0)
                                        .color(Color32::from_rgb(180, 160, 100)),
                                );
                                ui.separator();

                                let profiles = self.available_profiles.clone();
                                for p in profiles {
                                    let is_active = self.active_profile.as_ref().map(|a| a.id) == Some(p.id);
                                    let prefix = if is_active { "✔ " } else { "   " };
                                    let item_label = format!("{prefix}{}", p.name);
                                    let item_rich = if is_active {
                                        RichText::new(item_label).strong().color(BrandPalette::OBSIDIAN)
                                    } else {
                                        RichText::new(item_label).color(BrandPalette::PRIMARY_TEXT)
                                    };

                                    if ui.selectable_label(is_active, item_rich).clicked() {
                                        let _ = self.switch_profile(p.id);
                                    }
                                }

                                ui.separator();
                                if ui.button("+ New Profile...").clicked() {
                                    self.new_profile_dialog_open = true;
                                }
                                if ui.button("⚙ Manage Profiles...").clicked() {
                                    self.current_view = AppView::Settings;
                                    self.settings_tab = crate::views::SettingsTab::Profiles;
                                }
                            });

                        ui.separator();

                        // Developer & Test Menu
                        let dev_text = if is_wide { "Developer" } else { "Dev" };
                        egui::ComboBox::from_id_salt("dev_menu_combo")
                            .selected_text(dev_text)
                            .show_ui(ui, |ui| {
                                let diag_label = if self.show_diagnostics {
                                    "Hide Diagnostics Overlay"
                                } else {
                                    "Show Diagnostics Overlay"
                                };
                                if ui.button(diag_label).clicked() {
                                    self.show_diagnostics = !self.show_diagnostics;
                                }

                                ui.separator();
                                ui.label(
                                    RichText::new("Test Presets")
                                        .size(11.0)
                                        .color(Color32::from_rgb(180, 160, 100)),
                                );

                                if ui.button("H.264 (MP4)").clicked() {
                                    self.load_preset(&[
                                        "tests/MuvrixTestMedia/phase-0.4/A-core/h264-aac.mp4",
                                        "tests/MuvrixTestMedia/baseline/h264-aac.mp4",
                                    ]);
                                }
                                if ui.button("HEVC (MP4)").clicked() {
                                    self.load_preset(&[
                                        "tests/MuvrixTestMedia/phase-0.4/A-core/hevc-main-aac.mp4",
                                        "tests/MuvrixTestMedia/codecs/hevc-aac.mp4",
                                    ]);
                                }
                                if ui.button("VP9 (WebM)").clicked() {
                                    self.load_preset(&[
                                        "tests/MuvrixTestMedia/phase-0.4/A-core/vp9-opus.webm",
                                        "tests/MuvrixTestMedia/codecs/vp9-opus.webm",
                                    ]);
                                }
                                if ui.button("AV1 1080p (WebM)").clicked() {
                                    self.load_preset(&[
                                        "tests/MuvrixTestMedia/phase-0.4/generated/av1-1080p-svtav1-synthetic.webm",
                                        "tests/MuvrixTestMedia/codecs/av1-1080p.webm",
                                        "tests/MuvrixTestMedia/phase-0.4/A-core/av1-4k-cityhall.webm",
                                    ]);
                                }
                                if ui.button("Multi-Audio MOV").clicked() {
                                    self.load_preset(&[
                                        "tests/MuvrixTestMedia/phase-0.4/D-edge-cases/multiple-audio-streams.mov",
                                        "tests/MuvrixTestMedia/tracks/multiple-audio-streams.mov",
                                    ]);
                                }
                                if ui.button("Video-Only MP4").clicked() {
                                    self.load_preset(&[
                                        "tests/MuvrixTestMedia/phase-0.4/D-edge-cases/video-only.mp4",
                                        "tests/MuvrixTestMedia/tracks/video-only.mp4",
                                    ]);
                                }
                                if ui.button("Audio-Only FLAC").clicked() {
                                    self.load_preset(&[
                                        "tests/MuvrixTestMedia/phase-0.4/C-audio/flac-24bit-96khz.flac",
                                        "tests/MuvrixTestMedia/audio/audio.flac",
                                    ]);
                                }
                                if ui.button("Multi-Sub MKV").clicked() {
                                    self.load_preset(&[
                                        "tests/MuvrixTestMedia/phase-0.4/C-subtitles/embedded-srt-ass.mkv",
                                        "tests/MuvrixTestMedia/tracks/synthetic-multi-sub.mkv",
                                    ]);
                                }
                                if ui.button("Surround 7.1 MKV").clicked() {
                                    self.load_preset(&[
                                        "tests/MuvrixTestMedia/phase-0.4/C-audio/surround-7.1-sample.mkv",
                                        "tests/MuvrixTestMedia/tracks/surround-7.1.mkv",
                                        "tests/MuvrixTestMedia/phase-0.4/C-audio/TrueHD Sample 1 (Final Fantasy VII Advent Children).mkv",
                                    ]);
                                }
                            });
                        }

                        ui.separator();

                        // Keyboard shortcuts help button
                        if is_wide {
                            if icons::icon_text_button(
                                ui,
                                Icon::Shortcuts,
                                "Shortcuts (F1)",
                                "View keyboard shortcuts (F1)",
                            )
                            .clicked()
                            {
                                self.show_shortcuts = !self.show_shortcuts;
                            }
                        } else if icons::icon_button(
                            ui,
                            Icon::Shortcuts,
                            "View keyboard shortcuts (F1)",
                        )
                        .clicked()
                        {
                            self.show_shortcuts = !self.show_shortcuts;
                        }

                        // Stream metadata badge
                        if has_media_meta && !is_narrow {
                            ui.separator();
                            let v_clean = format_codec_name(&vfmt);
                            let a_clean = format_codec_name(&afmt);
                            if is_wide {
                                let badge = if !a_clean.is_empty() {
                                    format!("{}×{} · {} · {} · HW: {}", w, h, v_clean, a_clean, hw)
                                } else {
                                    format!("{}×{} · {} · HW: {}", w, h, v_clean, hw)
                                };
                                ui_components::badge_pill(
                                    ui,
                                    &badge,
                                    BrandPalette::SLATE_SURFACE,
                                    BrandPalette::ELECTRIC_LIME,
                                );
                            } else {
                                let res_label = if w >= 3840 {
                                    "4K".to_string()
                                } else if w >= 1920 {
                                    "1080p".to_string()
                                } else if w >= 1280 {
                                    "720p".to_string()
                                } else if h > 0 {
                                    format!("{}p", h)
                                } else {
                                    "Video".to_string()
                                };
                                let compact_meta = if !hw.is_empty() && hw != "no" {
                                    format!("{} · {}", res_label, hw)
                                } else {
                                    res_label
                                };
                                let rich_tip = format!(
                                    "Resolution: {}×{}\nVideo Codec: {}\nAudio Codec: {}\nHardware Acceleration: {}",
                                    w,
                                    h,
                                    if v_clean.is_empty() { "Unknown" } else { &v_clean },
                                    if a_clean.is_empty() { "None" } else { &a_clean },
                                    if hw.is_empty() { "none" } else { &hw }
                                );
                                ui_components::badge_pill(
                                    ui,
                                    &compact_meta,
                                    BrandPalette::SLATE_SURFACE,
                                    BrandPalette::ELECTRIC_LIME,
                                )
                                .on_hover_text(rich_tip);
                            }
                        }
                    });
                });
                ui.add_space(3.0);
            });

        if self.new_profile_dialog_open {
            let mut open = true;
            egui::Window::new("Create Playback Profile")
                .open(&mut open)
                .resizable(false)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.set_width(340.0);
                    ui.label("Enter a name for the new profile:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_profile_name)
                            .hint_text("e.g. Studio Monitors, Late Night..."),
                    );
                    ui.add_space(4.0);
                    ui.label("Description (optional):");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_profile_desc)
                            .hint_text("Details regarding this playback profile..."),
                    );
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui_components::primary_button(ui, "Save Profile", None).clicked()
                            && !self.new_profile_name.trim().is_empty()
                        {
                            let name = self.new_profile_name.clone();
                            let desc = if self.new_profile_desc.trim().is_empty() {
                                None
                            } else {
                                Some(self.new_profile_desc.clone())
                            };
                            let _ = self.create_profile_from_state(&name, desc.as_deref());
                            self.new_profile_name.clear();
                            self.new_profile_desc.clear();
                            self.new_profile_dialog_open = false;
                        }
                        if ui.button("Cancel").clicked() {
                            self.new_profile_dialog_open = false;
                        }
                    });
                });
            if !open {
                self.new_profile_dialog_open = false;
            }
        }

        // ---------------------------------------------------------------------
        // Active View Dispatch (Player, Library, Browser, Playlist, Lyrics, History, Settings)
        // ---------------------------------------------------------------------
    }
}
