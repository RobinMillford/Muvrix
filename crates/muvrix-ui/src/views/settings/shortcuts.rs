use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke};

use crate::app::actions::{PlayerAction, ShortcutCategory};
use crate::app::shortcuts::ShortcutProfile;
use crate::app::MuvrixApp;
use crate::branding::BrandPalette;
use crate::ui_components;

impl MuvrixApp {
    pub(crate) fn render_settings_shortcuts(&mut self, ui: &mut egui::Ui) {
        // ------------------------------------------------------------------
        // Profile selector + global actions
        // ------------------------------------------------------------------
        egui::Frame::NONE
            .fill(BrandPalette::SLATE_SURFACE)
            .stroke(Stroke::new(1.0_f32, BrandPalette::BORDER_SUBTLE))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(egui::Margin::symmetric(14, 12))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new("Profile")
                            .strong()
                            .size(12.0)
                            .color(BrandPalette::PRIMARY_TEXT),
                    );
                    let mut profile = self.keybindings.profile;
                    egui::ComboBox::from_id_salt("shortcut_profile_selector")
                        .selected_text(profile.label())
                        .width(230.0)
                        .show_ui(ui, |ui| {
                            for candidate in ShortcutProfile::ALL {
                                if ui
                                    .selectable_label(profile == *candidate, candidate.label())
                                    .clicked()
                                {
                                    profile = *candidate;
                                }
                            }
                        });
                    if profile != self.keybindings.profile {
                        self.keybindings.set_profile(profile);
                        let _ = self.keybindings.save_to_disk();
                        self.pending_shortcut_conflict = None;
                    }

                    ui.add_space(8.0);
                    if ui
                        .button("Export…")
                        .on_hover_text("Write the current registry to a JSON file")
                        .clicked()
                    {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_file_name("muvrix-shortcuts.json")
                            .add_filter("JSON", &["json"])
                            .save_file()
                        {
                            match self.keybindings.export_to(&path) {
                                Ok(()) => {
                                    self.shortcut_status =
                                        Some((format!("Exported to {}", path.display()), true));
                                }
                                Err(e) => {
                                    self.shortcut_status =
                                        Some((format!("Export failed: {e}"), false));
                                }
                            }
                        }
                    }
                    if ui
                        .button("Import…")
                        .on_hover_text("Load a previously exported shortcut file")
                        .clicked()
                    {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("JSON", &["json"])
                            .pick_file()
                        {
                            match self.keybindings.import_from(&path) {
                                Ok(()) => {
                                    let _ = self.keybindings.save_to_disk();
                                    self.shortcut_status =
                                        Some((format!("Imported {}", path.display()), true));
                                }
                                Err(e) => {
                                    self.shortcut_status = Some((e, false));
                                }
                            }
                        }
                    }
                    if ui
                        .button("Reset all…")
                        .on_hover_text("Restore every binding in this profile to its defaults")
                        .clicked()
                    {
                        // Destructive and not undoable, so it asks first.
                        self.confirm_shortcut_reset = true;
                    }
                });

                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(self.keybindings.profile.description())
                            .size(11.5)
                            .color(BrandPalette::SECONDARY_TEXT),
                    );
                    ui.add_space(6.0);
                    ui_components::badge_pill(
                        ui,
                        self.keybindings.profile.source_badge(),
                        BrandPalette::ELECTRIC_LIME.gamma_multiply(0.18),
                        BrandPalette::ELECTRIC_LIME,
                    );
                });
            });

        if let Some((msg, ok)) = &self.shortcut_status {
            ui.add_space(4.0);
            ui.label(RichText::new(msg).size(11.5).color(if *ok {
                BrandPalette::ELECTRIC_LIME
            } else {
                Color32::from_rgb(255, 120, 110)
            }));
        }

        ui.add_space(8.0);

        // ------------------------------------------------------------------
        // Adjustment magnitudes
        // ------------------------------------------------------------------
        // These are the numbers the seek, volume, speed and delay actions
        // actually use, so a change here is visible on the very next key press.
        egui::CollapsingHeader::new("Adjustment steps")
            .default_open(false)
            .show(ui, |ui| {
                let mut tuning = self.keybindings.tuning;
                let before = tuning;
                egui::Grid::new("shortcut_tuning_grid")
                    .num_columns(2)
                    .spacing([16.0, 6.0])
                    .show(ui, |ui| {
                        let row = |ui: &mut egui::Ui,
                                   label: &str,
                                   value: &mut f64,
                                   range: std::ops::RangeInclusive<f64>,
                                   speed: f64,
                                   suffix: &str| {
                            ui.label(
                                RichText::new(label)
                                    .size(11.5)
                                    .color(BrandPalette::SECONDARY_TEXT),
                            );
                            ui.add(
                                egui::DragValue::new(value)
                                    .speed(speed)
                                    .range(range)
                                    .suffix(suffix),
                            );
                            ui.end_row();
                        };
                        row(
                            ui,
                            "Small seek step",
                            &mut tuning.seek_small_secs,
                            0.5..=600.0,
                            0.5,
                            " s",
                        );
                        row(
                            ui,
                            "Medium seek step",
                            &mut tuning.seek_medium_secs,
                            0.5..=1800.0,
                            0.5,
                            " s",
                        );
                        row(
                            ui,
                            "Large seek step",
                            &mut tuning.seek_large_secs,
                            1.0..=3600.0,
                            1.0,
                            " s",
                        );
                        row(
                            ui,
                            "Volume step",
                            &mut tuning.volume_step,
                            0.5..=25.0,
                            0.5,
                            " %",
                        );
                        row(
                            ui,
                            "Playback speed step",
                            &mut tuning.speed_step,
                            0.05..=1.0,
                            0.05,
                            " ×",
                        );
                        row(
                            ui,
                            "Audio delay step",
                            &mut tuning.audio_delay_step_ms,
                            1.0..=1000.0,
                            5.0,
                            " ms",
                        );
                        row(
                            ui,
                            "Subtitle delay step",
                            &mut tuning.subtitle_delay_step_ms,
                            1.0..=1000.0,
                            5.0,
                            " ms",
                        );
                    });
                if tuning != before {
                    self.keybindings.tuning = tuning.sanitized();
                    if let Err(e) = self.keybindings.save_to_disk() {
                        self.shortcut_status = Some((e.to_string(), false));
                    }
                }
                ui.add_space(2.0);
                ui.label(
                    RichText::new(
                        "Used by the seek, volume, speed and subtitle/audio delay shortcuts.",
                    )
                    .size(10.5)
                    .color(BrandPalette::MUTED_TEXT),
                );
            });

        ui.add_space(8.0);

        // ------------------------------------------------------------------
        // Conflicts
        // ------------------------------------------------------------------
        let conflicts = self.keybindings.find_conflicts();
        if !conflicts.is_empty() {
            egui::Frame::NONE
                .fill(Color32::from_rgb(60, 22, 22))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(255, 90, 80)))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!("⚠ {} conflicting binding(s)", conflicts.len()))
                            .strong()
                            .color(Color32::from_rgb(255, 150, 140)),
                    );
                    for conflict in conflicts.iter().take(6) {
                        ui.label(
                            RichText::new(format!(
                                "• {} is used by “{}” and “{}”",
                                conflict.combination.display_string(),
                                conflict.action_a.display_name(),
                                conflict.action_b.display_name()
                            ))
                            .size(11.0)
                            .color(Color32::from_rgb(255, 190, 180)),
                        );
                    }
                });
            ui.add_space(8.0);
        }

        // ------------------------------------------------------------------
        // Search + category filter
        // ------------------------------------------------------------------
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.shortcut_search)
                    .hint_text("Search actions or keys…")
                    .desired_width(240.0),
            );
            if !self.shortcut_search.is_empty() && ui.button("✕").clicked() {
                self.shortcut_search.clear();
            }
            ui.add_space(8.0);
            ui.label(
                RichText::new(format!("{} actions", PlayerAction::ALL.len()))
                    .size(11.0)
                    .color(BrandPalette::MUTED_TEXT),
            );
        });

        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .selectable_label(self.shortcut_category.is_none(), "All")
                .clicked()
            {
                self.shortcut_category = None;
            }
            for category in ShortcutCategory::ALL {
                let selected = self.shortcut_category == Some(*category);
                if ui.selectable_label(selected, category.label()).clicked() {
                    self.shortcut_category = if selected { None } else { Some(*category) };
                }
            }
            if let Some(category) = self.shortcut_category {
                ui.add_space(6.0);
                if ui
                    .small_button(format!("Reset {}", category.label()))
                    .on_hover_text("Restore every action in this category to its default")
                    .clicked()
                {
                    let mut restored = 0usize;
                    for &action in PlayerAction::ALL
                        .iter()
                        .filter(|a| a.category() == category)
                    {
                        self.keybindings.reset_action(action);
                        self.keybindings.set_enabled(action, true);
                        restored += 1;
                    }
                    let _ = self.keybindings.save_to_disk();
                    self.pending_shortcut_conflict = None;
                    self.shortcut_status = Some((
                        format!("{restored} {} shortcuts restored", category.label()),
                        true,
                    ));
                }
            }
        });

        ui.add_space(8.0);

        // ------------------------------------------------------------------
        // Binding table
        // ------------------------------------------------------------------
        let query = self.shortcut_search.to_ascii_lowercase();
        let mut rows: Vec<PlayerAction> = Vec::new();
        for &action in PlayerAction::ALL {
            if let Some(category) = self.shortcut_category {
                if action.category() != category {
                    continue;
                }
            }
            let primary = self
                .keybindings
                .get(action)
                .map(|c| c.display_string())
                .unwrap_or_default();
            let alternate = self
                .keybindings
                .get_alternate(action)
                .map(|c| c.display_string())
                .unwrap_or_default();
            let haystack =
                format!("{} {} {}", action.display_name(), primary, alternate).to_ascii_lowercase();
            if !query.is_empty() && !haystack.contains(&query) {
                continue;
            }
            rows.push(action);
        }

        let row_count = rows.len();
        let mut requested_record: Option<PlayerAction> = None;
        let mut requested_reset: Option<PlayerAction> = None;
        let mut requested_toggle: Option<(PlayerAction, bool)> = None;

        egui::Frame::NONE
            .fill(BrandPalette::SLATE_SURFACE)
            .stroke(Stroke::new(1.0_f32, BrandPalette::BORDER_SUBTLE))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                egui::Grid::new("shortcut_manager_grid")
                    .num_columns(4)
                    .spacing([14.0, 8.0])
                    .striped(true)
                    .show(ui, |ui| {
                        for header in ["Action", "Primary", "Alternative", ""] {
                            ui.label(
                                RichText::new(header)
                                    .size(10.0)
                                    .strong()
                                    .color(BrandPalette::MUTED_TEXT),
                            );
                        }
                        ui.end_row();

                        for action in rows {
                            ui.horizontal(|ui| {
                                let mut enabled = self.keybindings.is_enabled(action);
                                if ui
                                    .checkbox(&mut enabled, "")
                                    .on_hover_text(
                                        "Disable this shortcut without losing its key assignment",
                                    )
                                    .changed()
                                {
                                    requested_toggle = Some((action, enabled));
                                }
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new(action.display_name()).size(12.0).color(
                                            if enabled {
                                                BrandPalette::PRIMARY_TEXT
                                            } else {
                                                BrandPalette::MUTED_TEXT
                                            },
                                        ),
                                    );
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(action.category().label())
                                                .size(9.5)
                                                .color(BrandPalette::MUTED_TEXT),
                                        );
                                        if self.keybindings.is_custom(action) {
                                            ui.label(
                                                RichText::new("• custom")
                                                    .size(9.5)
                                                    .color(BrandPalette::ELECTRIC_LIME),
                                            );
                                        }
                                        if !enabled {
                                            ui.label(
                                                RichText::new("• off")
                                                    .size(9.5)
                                                    .color(BrandPalette::MUTED_TEXT),
                                            );
                                        }
                                    });
                                });
                            });

                            // Primary keycap (click to record)
                            let recording = self.recording_shortcut == Some(action);
                            let primary_label = if recording {
                                "Press a key…".to_string()
                            } else {
                                self.keybindings
                                    .get(action)
                                    .map(|c| c.display_string())
                                    .unwrap_or_else(|| "Unassigned".to_string())
                            };
                            let key_btn = egui::Button::new(
                                RichText::new(primary_label).monospace().size(11.0).color(
                                    if recording {
                                        BrandPalette::OBSIDIAN
                                    } else {
                                        BrandPalette::PRIMARY_TEXT
                                    },
                                ),
                            )
                            .fill(if recording {
                                BrandPalette::ELECTRIC_LIME
                            } else {
                                BrandPalette::ELEVATED_CARD
                            })
                            .stroke(Stroke::new(1.0_f32, BrandPalette::BORDER_SLATE))
                            .min_size(egui::vec2(118.0, 24.0));
                            if ui
                                .add(key_btn)
                                .on_hover_text(if recording {
                                    "Press the new combination, or Esc to cancel"
                                } else {
                                    "Click to record a new shortcut"
                                })
                                .clicked()
                            {
                                requested_record = Some(action);
                            }

                            ui.label(
                                RichText::new(
                                    self.keybindings
                                        .get_alternate(action)
                                        .map(|c| c.display_string())
                                        .unwrap_or_else(|| "—".to_string()),
                                )
                                .monospace()
                                .size(11.0)
                                .color(BrandPalette::SECONDARY_TEXT),
                            );

                            if ui
                                .small_button("Reset")
                                .on_hover_text("Restore this action's default binding")
                                .clicked()
                            {
                                requested_reset = Some(action);
                            }
                            ui.end_row();
                        }
                    });

                if row_count == 0 {
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("No shortcuts match this search.")
                            .size(11.5)
                            .color(BrandPalette::MUTED_TEXT),
                    );
                }
            });

        if let Some(action) = requested_record {
            self.recording_shortcut = if self.recording_shortcut == Some(action) {
                None
            } else {
                Some(action)
            };
            self.pending_shortcut_conflict = None;
        }
        if let Some((action, enabled)) = requested_toggle {
            self.keybindings.set_enabled(action, enabled);
            let _ = self.keybindings.save_to_disk();
            self.shortcut_status = Some((
                format!(
                    "“{}” {}",
                    action.display_name(),
                    if enabled { "enabled" } else { "disabled" }
                ),
                true,
            ));
        }
        if let Some(action) = requested_reset {
            self.keybindings.reset_action(action);
            self.keybindings.set_enabled(action, true);
            let _ = self.keybindings.save_to_disk();
            self.shortcut_status = Some((
                format!("“{}” restored to default", action.display_name()),
                true,
            ));
        }

        ui.add_space(6.0);
        ui.label(
            RichText::new(
                "Click a shortcut to record a new combination. Modifiers (Ctrl, Alt, Shift) are supported. \
                 Typing in text fields is never intercepted.",
            )
            .size(10.5)
            .color(BrandPalette::MUTED_TEXT),
        );

        self.render_shortcut_conflict_dialog(ui.ctx());
        self.render_shortcut_reset_confirm(ui.ctx());
    }

    /// Two-step confirmation for the destructive "reset every shortcut" path.
    fn render_shortcut_reset_confirm(&mut self, ctx: &egui::Context) {
        if !self.confirm_shortcut_reset {
            return;
        }
        let mut confirmed = false;
        let mut cancelled = false;
        egui::Window::new("Reset all shortcuts?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(
                        "Every binding in this profile returns to its default and all \
                         disabled shortcuts are re-enabled. Custom bindings will be lost.",
                    )
                    .size(12.0)
                    .color(BrandPalette::SECONDARY_TEXT),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Reset everything").clicked() {
                        confirmed = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancelled = true;
                    }
                });
            });

        if confirmed {
            self.keybindings.reset_to_defaults();
            let _ = self.keybindings.save_to_disk();
            self.pending_shortcut_conflict = None;
            self.confirm_shortcut_reset = false;
            self.shortcut_status = Some(("All shortcuts reset to defaults".to_string(), true));
        } else if cancelled {
            self.confirm_shortcut_reset = false;
        }
    }

    fn render_shortcut_conflict_dialog(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.pending_shortcut_conflict.clone() else {
            return;
        };

        let mut replace = false;
        let mut swap = false;
        let mut cancel = false;

        egui::Window::new("Shortcut Conflict")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(format!(
                        "{} is already used by:",
                        pending.combination.display_string()
                    ))
                    .strong()
                    .color(BrandPalette::PRIMARY_TEXT),
                );
                ui.add_space(4.0);
                for (action, slot) in &pending.conflicts {
                    ui.label(
                        RichText::new(format!(
                            "• {} ({})",
                            action.display_name(),
                            match slot {
                                crate::app::shortcuts::BindingSlot::Primary => "primary",
                                crate::app::shortcuts::BindingSlot::Alternate => "alternative",
                            }
                        ))
                        .size(12.0)
                        .color(BrandPalette::SECONDARY_TEXT),
                    );
                }
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!(
                        "Assign {} to “{}”?",
                        pending.combination.display_string(),
                        pending.action.display_name()
                    ))
                    .size(12.0)
                    .color(BrandPalette::PRIMARY_TEXT),
                );
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui
                        .button("Replace")
                        .on_hover_text("Remove the conflicting assignment")
                        .clicked()
                    {
                        replace = true;
                    }
                    if ui
                        .button("Swap")
                        .on_hover_text(
                            "Give the conflicting action this action's previous shortcut",
                        )
                        .clicked()
                    {
                        swap = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });

        if replace {
            self.keybindings
                .bind_replace(pending.action, pending.combination);
            let _ = self.keybindings.save_to_disk();
            self.pending_shortcut_conflict = None;
            self.shortcut_status = Some((
                format!(
                    "Assigned {} to “{}”",
                    pending.combination.display_string(),
                    pending.action.display_name()
                ),
                true,
            ));
        } else if swap {
            self.keybindings
                .bind_swap(pending.action, pending.combination);
            let _ = self.keybindings.save_to_disk();
            self.pending_shortcut_conflict = None;
            self.shortcut_status = Some((
                format!(
                    "Swapped {} to “{}”",
                    pending.combination.display_string(),
                    pending.action.display_name()
                ),
                true,
            ));
        } else if cancel {
            self.pending_shortcut_conflict = None;
        }
    }
}
