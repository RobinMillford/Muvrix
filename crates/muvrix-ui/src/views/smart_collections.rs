//! Smart Collections UI: navigation bar, rule builder modal, and live count preview.

use eframe::egui::{self, Color32, RichText};
use muvrix_core::database::{
    LibraryOperator, NumericOperator, RuleCombinationMode, RuleDefinition, RuleField,
    SmartCollection, SmartRule, TextOperator,
};

use crate::app::MuvrixApp;

impl MuvrixApp {
    /// Opens the Smart Collection builder to create a new collection.
    pub fn open_new_smart_collection_dialog(&mut self) {
        self.editing_smart_collection = None;
        self.smart_collection_name_input = String::new();
        self.smart_collection_desc_input = String::new();
        self.smart_collection_icon_input = "✨".to_string();
        self.smart_collection_rules = vec![SmartRule::Text {
            field: RuleField::Title,
            operator: TextOperator::Contains,
            value: String::new(),
        }];
        self.smart_collection_combination_mode = RuleCombinationMode::And;
        self.smart_collection_error = None;
        self.smart_collection_preview_count = None;
        self.smart_collection_dialog_open = true;
    }

    /// Opens the Smart Collection builder to edit an existing collection.
    pub fn open_edit_smart_collection_dialog(&mut self, sc: &SmartCollection) {
        self.editing_smart_collection = Some(sc.clone());
        self.smart_collection_name_input = sc.name.clone();
        self.smart_collection_desc_input = sc.description.clone();
        self.smart_collection_icon_input = sc.icon.clone().unwrap_or_else(|| "✨".to_string());
        self.smart_collection_rules = sc.rules.rules.clone();
        self.smart_collection_combination_mode = sc.combination_mode;
        self.smart_collection_error = None;
        self.smart_collection_preview_count = None;
        self.smart_collection_dialog_open = true;
    }

    /// Renders the Smart Collection Rule Builder modal dialog.
    pub fn render_smart_collection_dialog(&mut self, ctx: &egui::Context) {
        let mut is_open = true;
        let mut close_dialog = false;

        let title = if self.editing_smart_collection.is_some() {
            "Edit Smart Collection"
        } else {
            "New Smart Collection"
        };

        egui::Window::new(title)
            .collapsible(false)
            .resizable(true)
            .default_width(620.0)
            .open(&mut is_open)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;

                // Header & basic details
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Name:").strong());
                    ui.add(
                        egui::TextEdit::singleline(&mut self.smart_collection_name_input)
                            .desired_width(240.0)
                            .hint_text("e.g. 4K Sci-Fi Movies"),
                    );

                    ui.label(RichText::new("Icon:").strong());
                    egui::ComboBox::from_id_salt("smart_collection_icon_picker")
                        .selected_text(&self.smart_collection_icon_input)
                        .width(60.0)
                        .show_ui(ui, |ui| {
                            for icon in &["✨", "🎬", "🎵", "⭐", "🕒", "🚀", "🔥", "💎", "📦"]
                            {
                                if ui
                                    .selectable_label(
                                        self.smart_collection_icon_input == *icon,
                                        *icon,
                                    )
                                    .clicked()
                                {
                                    self.smart_collection_icon_input = icon.to_string();
                                }
                            }
                        });
                });

                ui.horizontal(|ui| {
                    ui.label(RichText::new("Description:").strong());
                    ui.add(
                        egui::TextEdit::singleline(&mut self.smart_collection_desc_input)
                            .desired_width(450.0)
                            .hint_text("Optional notes or description"),
                    );
                });

                ui.separator();

                // Combination Mode Toggle
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Match:").strong());
                    if ui
                        .radio_value(
                            &mut self.smart_collection_combination_mode,
                            RuleCombinationMode::And,
                            "ALL conditions (AND)",
                        )
                        .clicked()
                    {
                        self.smart_collection_preview_count = None;
                    }
                    if ui
                        .radio_value(
                            &mut self.smart_collection_combination_mode,
                            RuleCombinationMode::Or,
                            "ANY condition (OR)",
                        )
                        .clicked()
                    {
                        self.smart_collection_preview_count = None;
                    }
                });

                ui.separator();

                // Rules List
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "Rules & Conditions ({}/20):",
                            self.smart_collection_rules.len()
                        ))
                        .strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.smart_collection_rules.len() < 20 {
                            if ui.button("➕ Add Condition").clicked() {
                                self.smart_collection_rules.push(SmartRule::Text {
                                    field: RuleField::Title,
                                    operator: TextOperator::Contains,
                                    value: String::new(),
                                });
                                self.smart_collection_preview_count = None;
                            }
                        } else {
                            ui.label(
                                RichText::new("Max rules reached")
                                    .color(Color32::from_rgb(251, 146, 60))
                                    .small(),
                            );
                        }
                    });
                });

                let mut remove_idx = None;
                let mut rules_changed = false;

                egui::ScrollArea::vertical()
                    .max_height(260.0)
                    .show(ui, |ui| {
                        for (idx, rule) in self.smart_collection_rules.iter_mut().enumerate() {
                            ui.push_id(idx, |ui| {
                                ui.group(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(format!("{}.", idx + 1));

                                        match rule {
                                            SmartRule::Text {
                                                field,
                                                operator,
                                                value,
                                            } => {
                                                // Field selector
                                                let current_name = field.display_name();
                                                egui::ComboBox::from_id_salt("field_combo")
                                                    .selected_text(current_name)
                                                    .width(130.0)
                                                    .show_ui(ui, |ui| {
                                                        let fields = [
                                                            RuleField::Title,
                                                            RuleField::Artist,
                                                            RuleField::Album,
                                                            RuleField::AlbumArtist,
                                                            RuleField::Genre,
                                                            RuleField::FilePath,
                                                            RuleField::Format,
                                                            RuleField::MediaType,
                                                        ];
                                                        for f in fields {
                                                            if ui
                                                                .selectable_label(
                                                                    *field == f,
                                                                    f.display_name(),
                                                                )
                                                                .clicked()
                                                            {
                                                                *field = f;
                                                                rules_changed = true;
                                                            }
                                                        }
                                                    });

                                                // Operator selector
                                                let op_name = operator.display_name();
                                                egui::ComboBox::from_id_salt("op_combo")
                                                    .selected_text(op_name)
                                                    .width(110.0)
                                                    .show_ui(ui, |ui| {
                                                        let ops = [
                                                            TextOperator::Contains,
                                                            TextOperator::DoesNotContain,
                                                            TextOperator::Equals,
                                                            TextOperator::DoesNotEqual,
                                                            TextOperator::StartsWith,
                                                            TextOperator::EndsWith,
                                                            TextOperator::IsEmpty,
                                                            TextOperator::IsNotEmpty,
                                                        ];
                                                        for op in ops {
                                                            if ui
                                                                .selectable_label(
                                                                    *operator == op,
                                                                    op.display_name(),
                                                                )
                                                                .clicked()
                                                            {
                                                                *operator = op;
                                                                rules_changed = true;
                                                            }
                                                        }
                                                    });

                                                // Value input
                                                if *operator != TextOperator::IsEmpty
                                                    && *operator != TextOperator::IsNotEmpty
                                                    && ui
                                                        .add(
                                                            egui::TextEdit::singleline(value)
                                                                .desired_width(180.0),
                                                        )
                                                        .changed()
                                                {
                                                    rules_changed = true;
                                                }
                                            }
                                            SmartRule::Numeric {
                                                field,
                                                operator,
                                                value,
                                                value_to,
                                            } => {
                                                let current_name = field.display_name();
                                                egui::ComboBox::from_id_salt("field_combo")
                                                    .selected_text(current_name)
                                                    .width(130.0)
                                                    .show_ui(ui, |ui| {
                                                        let fields = [
                                                            RuleField::ReleaseYear,
                                                            RuleField::Duration,
                                                            RuleField::TrackNumber,
                                                            RuleField::DiscNumber,
                                                            RuleField::FileSize,
                                                            RuleField::Bitrate,
                                                        ];
                                                        for f in fields {
                                                            if ui
                                                                .selectable_label(
                                                                    *field == f,
                                                                    f.display_name(),
                                                                )
                                                                .clicked()
                                                            {
                                                                *field = f;
                                                                rules_changed = true;
                                                            }
                                                        }
                                                    });

                                                let op_name = operator.display_name();
                                                egui::ComboBox::from_id_salt("op_combo")
                                                    .selected_text(op_name)
                                                    .width(110.0)
                                                    .show_ui(ui, |ui| {
                                                        let ops = [
                                                            NumericOperator::Equals,
                                                            NumericOperator::GreaterThan,
                                                            NumericOperator::LessThan,
                                                            NumericOperator::AtLeast,
                                                            NumericOperator::AtMost,
                                                            NumericOperator::Between,
                                                        ];
                                                        for op in ops {
                                                            if ui
                                                                .selectable_label(
                                                                    *operator == op,
                                                                    op.display_name(),
                                                                )
                                                                .clicked()
                                                            {
                                                                *operator = op;
                                                                rules_changed = true;
                                                            }
                                                        }
                                                    });

                                                let mut val_str = value.to_string();
                                                if ui
                                                    .add(
                                                        egui::TextEdit::singleline(&mut val_str)
                                                            .desired_width(80.0),
                                                    )
                                                    .changed()
                                                {
                                                    if let Ok(v) = val_str.parse::<f64>() {
                                                        *value = v;
                                                        rules_changed = true;
                                                    }
                                                }

                                                if *operator == NumericOperator::Between {
                                                    ui.label("and");
                                                    let mut to_str =
                                                        value_to.unwrap_or(*value).to_string();
                                                    if ui
                                                        .add(
                                                            egui::TextEdit::singleline(&mut to_str)
                                                                .desired_width(80.0),
                                                        )
                                                        .changed()
                                                    {
                                                        if let Ok(v) = to_str.parse::<f64>() {
                                                            *value_to = Some(v);
                                                            rules_changed = true;
                                                        }
                                                    }
                                                }
                                            }
                                            SmartRule::Date {
                                                field,
                                                operator,
                                                days,
                                                ..
                                            } => {
                                                ui.label(field.display_name());
                                                ui.label(operator.display_name());
                                                let mut days_str = days.unwrap_or(30).to_string();
                                                if ui
                                                    .add(
                                                        egui::TextEdit::singleline(&mut days_str)
                                                            .desired_width(60.0),
                                                    )
                                                    .changed()
                                                {
                                                    if let Ok(d) = days_str.parse::<i64>() {
                                                        *days = Some(d);
                                                        rules_changed = true;
                                                    }
                                                }
                                                ui.label("days");
                                            }
                                            SmartRule::Library { operator, days } => {
                                                ui.label(operator.display_name());
                                                if *operator == LibraryOperator::RecentlyAddedNDays
                                                {
                                                    let mut days_str =
                                                        days.unwrap_or(7).to_string();
                                                    if ui
                                                        .add(
                                                            egui::TextEdit::singleline(
                                                                &mut days_str,
                                                            )
                                                            .desired_width(60.0),
                                                        )
                                                        .changed()
                                                    {
                                                        if let Ok(d) = days_str.parse::<i64>() {
                                                            *days = Some(d);
                                                            rules_changed = true;
                                                        }
                                                    }
                                                    ui.label("days");
                                                }
                                            }
                                        }

                                        // Switch condition category
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if ui
                                                    .button("🗑")
                                                    .on_hover_text("Delete condition")
                                                    .clicked()
                                                {
                                                    remove_idx = Some(idx);
                                                }
                                            },
                                        );
                                    });
                                });
                            });
                        }
                    });

                if let Some(i) = remove_idx {
                    if self.smart_collection_rules.len() > 1 {
                        self.smart_collection_rules.remove(i);
                        rules_changed = true;
                    }
                }

                if rules_changed {
                    self.smart_collection_preview_count = None;
                }

                ui.separator();

                // Live Count Preview
                let preview_sc = SmartCollection {
                    id: 0,
                    name: self.smart_collection_name_input.clone(),
                    description: self.smart_collection_desc_input.clone(),
                    icon: Some(self.smart_collection_icon_input.clone()),
                    rules: RuleDefinition {
                        rules: self.smart_collection_rules.clone(),
                        mode: self.smart_collection_combination_mode,
                    },
                    combination_mode: self.smart_collection_combination_mode,
                    sort_field: Some("title".to_string()),
                    sort_direction: Some("asc".to_string()),
                    display_order: 0,
                    created_at: 0,
                    updated_at: 0,
                };

                let preview_count = match self.smart_collection_preview_count {
                    Some(c) => c,
                    None => {
                        let c = self
                            .db
                            .count_smart_collection_items(&preview_sc)
                            .unwrap_or(0);
                        self.smart_collection_preview_count = Some(c);
                        c
                    }
                };

                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "⚡ Live Preview: {preview_count} matching item(s) in catalog"
                        ))
                        .strong()
                        .color(Color32::from_rgb(125, 211, 252)),
                    );
                });

                if let Some(ref err) = self.smart_collection_error {
                    ui.label(
                        RichText::new(format!("⚠ {err}"))
                            .color(Color32::from_rgb(239, 68, 68))
                            .small(),
                    );
                }

                ui.add_space(8.0);

                // Footer Buttons
                ui.horizontal(|ui| {
                    if ui
                        .button(RichText::new("Save Collection").strong())
                        .clicked()
                    {
                        if self.smart_collection_name_input.trim().is_empty() {
                            self.smart_collection_error =
                                Some("Collection name cannot be empty".to_string());
                        } else {
                            let mut sc = preview_sc.clone();
                            let res = if let Some(ref existing) = self.editing_smart_collection {
                                sc.id = existing.id;
                                sc.created_at = existing.created_at;
                                sc.display_order = existing.display_order;
                                self.db.update_smart_collection(&sc)
                            } else {
                                self.db.create_smart_collection(&sc).map(|_| ())
                            };

                            match res {
                                Ok(_) => {
                                    close_dialog = true;
                                    self.refresh_library();
                                    self.status_message =
                                        format!("Saved smart collection '{}'", sc.name);
                                }
                                Err(e) => {
                                    self.smart_collection_error =
                                        Some(format!("Failed to save: {e}"));
                                }
                            }
                        }
                    }

                    if ui.button("Cancel").clicked() {
                        close_dialog = true;
                    }
                });
            });

        if close_dialog || !is_open {
            self.smart_collection_dialog_open = false;
        }
    }
}
