//! Batch Metadata Editor UI: multi-select inspection, mixed-value detection, and selective updating.

use eframe::egui::{self, Color32, RichText};
use muvrix_core::database::{
    BatchMetadataRequest, FieldAction, FieldMixedState, SelectedMetadataSummary,
};

use crate::app::MuvrixApp;

impl MuvrixApp {
    /// Inspects the current library multi-selection and opens the batch metadata editor.
    pub fn open_batch_metadata_editor(&mut self) {
        let ids = self.library_selection.ids();
        if ids.is_empty() {
            return;
        }

        let mut metadata_opts = Vec::new();
        for &id in &ids {
            let meta = self.db.get_metadata(id).ok().flatten();
            metadata_opts.push(meta);
        }

        let summary = SelectedMetadataSummary::analyze(&metadata_opts);

        self.batch_title_input = summary.title.value().cloned().unwrap_or_default();
        self.batch_edit_title_action = 0;

        self.batch_artist_input = summary.artist.value().cloned().unwrap_or_default();
        self.batch_edit_artist_action = 0;

        self.batch_album_input = summary.album.value().cloned().unwrap_or_default();
        self.batch_edit_album_action = 0;

        self.batch_album_artist_input = summary.album_artist.value().cloned().unwrap_or_default();
        self.batch_edit_album_artist_action = 0;

        self.batch_genre_input = summary.genre.value().cloned().unwrap_or_default();
        self.batch_edit_genre_action = 0;

        self.batch_year_input = summary
            .release_year
            .value()
            .map(|y| y.to_string())
            .unwrap_or_default();
        self.batch_edit_year_action = 0;

        self.batch_track_input = summary
            .track_number
            .value()
            .map(|t| t.to_string())
            .unwrap_or_default();
        self.batch_edit_track_action = 0;

        self.batch_disc_input = summary
            .disc_number
            .value()
            .map(|d| d.to_string())
            .unwrap_or_default();
        self.batch_edit_disc_action = 0;

        self.batch_metadata_summary = Some(summary);
        self.batch_edit_status = None;
        self.batch_metadata_dialog_open = true;
    }

    /// Renders the Batch Metadata Editor modal dialog.
    pub fn render_batch_metadata_dialog(&mut self, ctx: &egui::Context) {
        let mut is_open = true;
        let mut close_dialog = false;
        let ids = self.library_selection.ids();

        egui::Window::new(format!(
            "Batch Edit Metadata · {} item(s) selected",
            ids.len()
        ))
        .collapsible(false)
        .resizable(true)
        .default_width(620.0)
        .open(&mut is_open)
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;

            ui.label(
                RichText::new(
                    "Edit common metadata tags across selected items. Fields with mixed values are protected by default and will only be changed if you explicitly choose 'Set Value' or 'Clear'.",
                )
                .size(12.0)
                .color(Color32::from_gray(180)),
            );

            ui.separator();

            let summary = self.batch_metadata_summary.clone().unwrap_or_default();

            egui::ScrollArea::vertical().max_height(340.0).show(ui, |ui| {
                egui::Grid::new("batch_metadata_fields_grid")
                    .num_columns(4)
                    .spacing([12.0, 10.0])
                    .show(ui, |ui| {
                        // Title
                        render_field_row(
                            ui,
                            "Title",
                            &summary.title,
                            &mut self.batch_edit_title_action,
                            &mut self.batch_title_input,
                            "New Title",
                        );

                        // Artist
                        render_field_row(
                            ui,
                            "Artist",
                            &summary.artist,
                            &mut self.batch_edit_artist_action,
                            &mut self.batch_artist_input,
                            "Artist name",
                        );

                        // Album
                        render_field_row(
                            ui,
                            "Album",
                            &summary.album,
                            &mut self.batch_edit_album_action,
                            &mut self.batch_album_input,
                            "Album title",
                        );

                        // Album Artist
                        render_field_row(
                            ui,
                            "Album Artist",
                            &summary.album_artist,
                            &mut self.batch_edit_album_artist_action,
                            &mut self.batch_album_artist_input,
                            "Album artist name",
                        );

                        // Genre
                        render_field_row(
                            ui,
                            "Genre",
                            &summary.genre,
                            &mut self.batch_edit_genre_action,
                            &mut self.batch_genre_input,
                            "e.g. Rock, Soundtrack",
                        );

                        // Release Year
                        render_field_row(
                            ui,
                            "Release Year",
                            &summary.release_year,
                            &mut self.batch_edit_year_action,
                            &mut self.batch_year_input,
                            "YYYY (e.g. 1999)",
                        );

                        // Track Number
                        render_field_row(
                            ui,
                            "Track Number",
                            &summary.track_number,
                            &mut self.batch_edit_track_action,
                            &mut self.batch_track_input,
                            "Track #",
                        );

                        // Disc Number
                        render_field_row(
                            ui,
                            "Disc Number",
                            &summary.disc_number,
                            &mut self.batch_edit_disc_action,
                            &mut self.batch_disc_input,
                            "Disc #",
                        );
                    });
            });

            ui.separator();

            ui.label(
                RichText::new("ℹ Catalog updates are transactional and synchronized with FTS5 search. Media files on disk are never altered.")
                    .size(11.0)
                    .color(Color32::from_gray(140)),
            );

            if let Some(ref status) = self.batch_edit_status {
                ui.label(RichText::new(status).color(Color32::from_rgb(239, 68, 68)).small());
            }

            ui.add_space(6.0);

            ui.horizontal(|ui| {
                let apply_btn = egui::Button::new(
                    RichText::new(format!("Apply Changes to {} Item(s)", ids.len()))
                        .strong()
                        .color(if ids.is_empty() {
                            Color32::from_gray(120)
                        } else {
                            Color32::from_rgb(125, 211, 252)
                        }),
                );
                if ui.add_enabled(!ids.is_empty(), apply_btn).clicked() {
                    let req = BatchMetadataRequest {
                        title: to_field_action(self.batch_edit_title_action, &self.batch_title_input),
                        artist: to_field_action(self.batch_edit_artist_action, &self.batch_artist_input),
                        album: to_field_action(self.batch_edit_album_action, &self.batch_album_input),
                        album_artist: to_field_action(
                            self.batch_edit_album_artist_action,
                            &self.batch_album_artist_input,
                        ),
                        genre: to_field_action(self.batch_edit_genre_action, &self.batch_genre_input),
                        release_year: match self.batch_edit_year_action {
                            1 => {
                                if let Ok(y) = self.batch_year_input.trim().parse::<i32>() {
                                    FieldAction::Set(y)
                                } else {
                                    FieldAction::Keep
                                }
                            }
                            2 => FieldAction::Clear,
                            _ => FieldAction::Keep,
                        },
                        track_number: match self.batch_edit_track_action {
                            1 => {
                                if let Ok(t) = self.batch_track_input.trim().parse::<i32>() {
                                    FieldAction::Set(t)
                                } else {
                                    FieldAction::Keep
                                }
                            }
                            2 => FieldAction::Clear,
                            _ => FieldAction::Keep,
                        },
                        disc_number: match self.batch_edit_disc_action {
                            1 => {
                                if let Ok(d) = self.batch_disc_input.trim().parse::<i32>() {
                                    FieldAction::Set(d)
                                } else {
                                    FieldAction::Keep
                                }
                            }
                            2 => FieldAction::Clear,
                            _ => FieldAction::Keep,
                        },
                    };

                    match self.db.batch_update_metadata(&ids, &req) {
                        Ok(res) => {
                            self.status_message = format!(
                                "Batch updated metadata for {} item(s)",
                                res.affected_items
                            );
                            self.refresh_library();
                            close_dialog = true;
                        }
                        Err(e) => {
                            self.batch_edit_status = Some(format!("Batch update failed: {e}"));
                        }
                    }
                }

                if ui.button("Cancel").clicked() {
                    close_dialog = true;
                }
            });
        });

        if close_dialog || !is_open {
            self.batch_metadata_dialog_open = false;
        }
    }
}

fn to_field_action(action: u8, text: &str) -> FieldAction<String> {
    match action {
        1 => FieldAction::Set(text.trim().to_string()),
        2 => FieldAction::Clear,
        _ => FieldAction::Keep,
    }
}

fn render_field_row<T: std::fmt::Display>(
    ui: &mut egui::Ui,
    label: &str,
    state: &FieldMixedState<T>,
    action: &mut u8,
    input: &mut String,
    hint: &str,
) {
    ui.label(RichText::new(label).strong());

    match state {
        FieldMixedState::Mixed => {
            ui.label(
                RichText::new("Mixed Values")
                    .small()
                    .color(Color32::from_rgb(251, 146, 60)),
            );
        }
        FieldMixedState::Uniform(Some(_)) => {
            ui.label(
                RichText::new("Uniform")
                    .small()
                    .color(Color32::from_rgb(74, 222, 128)),
            );
        }
        FieldMixedState::Uniform(None) => {
            ui.label(
                RichText::new("Empty")
                    .small()
                    .color(Color32::from_gray(130)),
            );
        }
    }

    ui.horizontal(|ui| {
        ui.radio_value(action, 0, "Keep");
        ui.radio_value(action, 1, "Set");
        ui.radio_value(action, 2, "Clear");
    });

    ui.add_enabled(
        *action == 1,
        egui::TextEdit::singleline(input)
            .hint_text(hint)
            .desired_width(180.0),
    );

    ui.end_row();
}
