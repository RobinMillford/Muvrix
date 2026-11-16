//! Integrity Scanner & Catalog Cleanup UI: missing file discovery, candidate relinking, and safe database pruning.

use eframe::egui::{self, Color32, RichText};
use muvrix_core::database::find_relink_candidates;
use std::path::PathBuf;

use crate::app::MuvrixApp;

impl MuvrixApp {
    /// Opens the Library Integrity & Catalog Cleanup dialog.
    pub fn open_integrity_dialog(&mut self) {
        self.integrity_dialog_open = true;
        self.integrity_status_msg = None;
        self.selected_missing_item = None;
        self.relink_candidates.clear();
        self.cleanup_preview = None;
        self.cleanup_confirm_open = false;

        // Auto-load currently known unavailable items
        if let Ok(missing) = self.db.get_unavailable_media_items() {
            self.missing_media_items = missing;
        }

        // Populate roots as default search directory
        if self.relink_search_dir_input.is_empty() {
            if let Ok(roots) = self.db.get_roots() {
                if let Some(r) = roots.first() {
                    self.relink_search_dir_input = r.root_path.clone();
                }
            }
        }
    }

    /// Renders the Library Integrity & Catalog Cleanup modal dialog.
    pub fn render_integrity_dialog(&mut self, ctx: &egui::Context) {
        let mut is_open = true;
        let mut close_dialog = false;

        egui::Window::new("Library Integrity & Catalog Cleanup")
            .collapsible(false)
            .resizable(true)
            .default_width(700.0)
            .open(&mut is_open)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;

                ui.horizontal(|ui| {
                    if ui.button(RichText::new("🔍 Run Integrity Scan").strong()).clicked() {
                        match self.db.scan_library_integrity(None) {
                            Ok(progress) => {
                                self.integrity_scan_progress = Some(progress);
                                self.missing_media_items =
                                    self.db.get_unavailable_media_items().unwrap_or_default();
                                self.integrity_status_msg = Some(
                                    "Integrity scan completed successfully".to_string(),
                                );
                            }
                            Err(e) => {
                                self.integrity_status_msg =
                                    Some(format!("Integrity scan failed: {e}"));
                            }
                        }
                    }

                    if ui.button("Inspect Catalog for Cleanup").clicked() {
                        match self.db.generate_cleanup_preview() {
                            Ok(preview) => {
                                self.cleanup_preview = Some(preview);
                                self.cleanup_confirm_open = false;
                                if let Ok(missing) = self.db.get_unavailable_media_items() {
                                    self.missing_media_items = missing;
                                }
                                self.integrity_status_msg = Some(
                                    "Catalog inspection completed. Review findings below.".to_string(),
                                );
                            }
                            Err(e) => {
                                self.integrity_status_msg =
                                    Some(format!("Cleanup preview failed: {e}"));
                            }
                        }
                    }
                });

                ui.separator();

                // Scan Metrics Display
                if let Some(ref progress) = self.integrity_scan_progress {
                    ui.group(|ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(format!("Examined: {}", progress.total_examined))
                                    .strong(),
                            );
                            ui.separator();
                            ui.label(
                                RichText::new(format!("Available: {}", progress.available_count))
                                    .color(Color32::from_rgb(74, 222, 128))
                                    .strong(),
                            );
                            ui.separator();
                            ui.label(
                                RichText::new(format!("Missing: {}", progress.missing_count))
                                    .color(Color32::from_rgb(239, 68, 68))
                                    .strong(),
                            );
                            ui.separator();
                            ui.label(
                                RichText::new(format!(
                                    "Network Shares: {}",
                                    progress.network_unavailable_count
                                ))
                                .color(Color32::from_rgb(125, 211, 252))
                                .strong(),
                            );
                        });
                    });
                }

                // Section 1: Broken File Locator & Candidate Relinking
                ui.label(
                    RichText::new("Broken File Locator & Candidate Relinking")
                        .strong()
                        .size(14.0),
                );

                if self.missing_media_items.is_empty() {
                    if let Some(ref progress) = self.integrity_scan_progress {
                        if progress.missing_count == 0 {
                            ui.label(
                                RichText::new("✔ No broken or missing files detected in the catalog.")
                                    .color(Color32::from_rgb(74, 222, 128))
                                    .italics(),
                            );
                        } else {
                            ui.label(
                                RichText::new("⚠ Scan detected missing items. Click 'Run Integrity Scan' to refresh.")
                                    .color(Color32::from_rgb(251, 146, 60))
                                    .italics(),
                            );
                        }
                    } else {
                        ui.label(
                            RichText::new("Run an integrity scan to verify media file availability across storage volumes.")
                                .color(Color32::from_gray(180))
                                .italics(),
                        );
                    }
                } else {
                    ui.label(format!(
                        "Found {} missing item(s). Select an item to discover candidates and relink:",
                        self.missing_media_items.len()
                    ));

                    egui::ScrollArea::vertical()
                        .max_height(140.0)
                        .show(ui, |ui| {
                            for item in &self.missing_media_items {
                                let is_selected = self
                                    .selected_missing_item
                                    .as_ref()
                                    .map(|s| s.item.id == item.item.id)
                                    .unwrap_or(false);

                                let title = item
                                    .metadata
                                    .as_ref()
                                    .and_then(|m| m.title.as_deref())
                                    .unwrap_or(&item.item.file_name);

                                if ui
                                    .selectable_label(
                                        is_selected,
                                        format!("⚠ {title} — {}", item.item.canonical_path),
                                    )
                                    .clicked()
                                {
                                    self.selected_missing_item = Some(item.clone());
                                    self.relink_candidates.clear();
                                }
                            }
                        });

                    // Candidate Relinking Panel
                    let missing_opt = self.selected_missing_item.clone();
                    if let Some(missing) = missing_opt {
                        ui.group(|ui| {
                            ui.label(
                                RichText::new(format!("Relinking: {}", missing.item.file_name))
                                    .strong(),
                            );
                            ui.horizontal(|ui| {
                                ui.label("Search Directory:");
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.relink_search_dir_input)
                                        .desired_width(320.0),
                                );
                                if ui.button("Browse...").clicked() {
                                    if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                                        self.relink_search_dir_input =
                                            folder.to_string_lossy().to_string();
                                    }
                                }
                                if ui.button("Find Candidates").clicked() {
                                    let dir = PathBuf::from(self.relink_search_dir_input.trim());
                                    let title = missing
                                        .metadata
                                        .as_ref()
                                        .and_then(|m| m.title.as_deref());
                                    let artist = missing
                                        .metadata
                                        .as_ref()
                                        .and_then(|m| m.artist.as_deref());
                                    self.relink_candidates = find_relink_candidates(
                                        &[dir],
                                        &missing.item,
                                        title,
                                        artist,
                                    );
                                }
                            });

                            if !self.relink_candidates.is_empty() {
                                ui.label(RichText::new("Discovered Candidates:").strong().small());
                                let mut relink_target = None;

                                for cand in &self.relink_candidates {
                                    ui.horizontal(|ui| {
                                        let conf_pct = (cand.confidence * 100.0).round() as u32;
                                        let conf_color = if conf_pct >= 85 {
                                            Color32::from_rgb(74, 222, 128)
                                        } else {
                                            Color32::from_rgb(251, 146, 60)
                                        };
                                        ui.label(
                                            RichText::new(format!("[{conf_pct}% Match]"))
                                                .color(conf_color)
                                                .strong(),
                                        );
                                        ui.label(cand.path.to_string_lossy());
                                        if ui.button("Relink to this file").clicked() {
                                            relink_target = Some(cand.path.clone());
                                        }
                                    });
                                }

                                if let Some(target_path) = relink_target {
                                    match self.db.relink_media_item(missing.item.id, &target_path) {
                                        Ok(updated) => {
                                            self.integrity_status_msg = Some(format!(
                                                "Successfully relinked to {}",
                                                updated.canonical_path
                                            ));
                                            self.selected_missing_item = None;
                                            self.relink_candidates.clear();
                                            self.refresh_library();
                                        }
                                        Err(e) => {
                                            self.integrity_status_msg =
                                                Some(format!("Relink failed: {e}"));
                                        }
                                    }
                                }
                            } else {
                                ui.label(
                                    RichText::new("No matching candidates discovered in the specified directory.")
                                        .italics()
                                        .small(),
                                );
                            }
                        });
                    }
                }

                ui.separator();

                // Section 2: Catalog Cleanup Preview & Execution
                ui.label(
                    RichText::new("Catalog Cleanup (Safe Stale Record Pruning)")
                        .strong()
                        .size(14.0),
                );

                let preview_opt = self.cleanup_preview.clone();
                if let Some(preview) = preview_opt {
                    ui.group(|ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(format!(
                                "Confirmed Missing: {} record(s)",
                                preview.confirmed_missing_ids.len()
                            ));
                            ui.separator();
                            ui.label(format!(
                                "Duplicates: {} record(s)",
                                preview.duplicate_ids.len()
                            ));
                            ui.separator();
                            ui.label(format!(
                                "Orphaned Metadata: {} record(s)",
                                preview.orphaned_metadata_ids.len()
                            ));
                        });

                        ui.add_space(4.0);
                        if !preview.suspected_duplicates.is_empty() {
                            ui.label(
                                RichText::new(format!(
                                    "⚠ {} suspected duplicate(s) require manual review (unverified or different hashes; skipped from auto-cleanup)",
                                    preview.suspected_duplicates.len()
                                ))
                                .color(Color32::from_rgb(251, 146, 60))
                                .small(),
                            );
                            ui.add_space(2.0);
                        }
                        ui.label(
                            RichText::new("ℹ Safety Guarantee: Catalog cleanup only purges stale SQLite database records. Media files on disk are NEVER touched or removed.")
                                .color(Color32::from_gray(160))
                                .size(11.0),
                        );

                        ui.add_space(4.0);
                        if preview.total_cleanup_candidates() > 0 {
                            if self.cleanup_confirm_open {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!(
                                            "Confirm purging {} stale database records?",
                                            preview.total_cleanup_candidates()
                                        ))
                                        .color(Color32::from_rgb(251, 146, 60))
                                        .strong(),
                                    );
                                    if ui
                                        .button(RichText::new("Yes, Execute Cleanup").strong())
                                        .clicked()
                                    {
                                        match self.db.execute_catalog_cleanup(&preview) {
                                            Ok(count) => {
                                                self.integrity_status_msg = Some(format!(
                                                    "Catalog cleanup removed {count} stale record(s)"
                                                ));
                                                self.cleanup_preview = None;
                                                self.cleanup_confirm_open = false;
                                                self.missing_media_items = self
                                                    .db
                                                    .get_unavailable_media_items()
                                                    .unwrap_or_default();
                                                self.refresh_library();
                                            }
                                            Err(e) => {
                                                self.integrity_status_msg =
                                                    Some(format!("Cleanup execution failed: {e}"));
                                            }
                                        }
                                    }
                                    if ui.button("Cancel").clicked() {
                                        self.cleanup_confirm_open = false;
                                    }
                                });
                            } else if ui
                                .button(
                                    RichText::new(format!(
                                        "Execute Safe Cleanup ({} records)",
                                        preview.total_cleanup_candidates()
                                    ))
                                    .color(Color32::from_rgb(251, 146, 60)),
                                )
                                .clicked()
                            {
                                self.cleanup_confirm_open = true;
                            }
                        } else {
                            ui.label(
                                RichText::new("Catalog is completely clean. No stale records found.")
                                    .color(Color32::from_rgb(74, 222, 128))
                                    .italics(),
                            );
                        }
                    });
                }

                if let Some(ref msg) = self.integrity_status_msg {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(msg)
                            .color(Color32::from_rgb(125, 211, 252))
                            .small(),
                    );
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Close").clicked() {
                        close_dialog = true;
                    }
                });
            });

        if close_dialog || !is_open {
            self.integrity_dialog_open = false;
        }
    }
}
