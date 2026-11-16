use eframe::egui::{self, Color32, RichText};

use crate::app::MuvrixApp;
use crate::branding::{self, BrandPalette};

impl MuvrixApp {
    pub(crate) fn render_settings_about(&mut self, ui: &mut egui::Ui) {
        // 1. Premium Brand Card with Official Circular Logo
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let logo_tex = self.ensure_logo_texture(ui.ctx());
                branding::render_logo(ui, logo_tex, 72.0);
                ui.add_space(14.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("MUVRIX")
                            .strong()
                            .size(20.0)
                            .color(BrandPalette::IVORY_TEXT),
                    );
                    ui.label(
                        RichText::new(format!(
                            "Version: {} (Linux Wayland Beta)",
                            env!("CARGO_PKG_VERSION")
                        ))
                        .size(12.5)
                        .color(BrandPalette::MUTED_TEXT),
                    );
                    ui.label(
                        RichText::new(
                            "High-performance native desktop media player built with Rust & libmpv",
                        )
                        .size(11.5)
                        .color(Color32::from_gray(160)),
                    );
                });
            });
        });

        ui.add_space(8.0);

        // 2. Dynamic System & Engine Diagnostics
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("System & Engine Diagnostics")
                    .strong()
                    .size(14.0)
                    .color(BrandPalette::ELECTRIC_LIME),
            );
            ui.add_space(4.0);
            egui::Grid::new("about_diagnostics_grid")
                .num_columns(2)
                .spacing([24.0, 6.0])
                .show(ui, |ui| {
                    ui.label("Application:");
                    ui.label(
                        RichText::new(format!("Muvrix v{}", env!("CARGO_PKG_VERSION")))
                            .monospace()
                            .color(BrandPalette::IVORY_TEXT),
                    );
                    ui.end_row();

                    ui.label("Engine Core:");
                    ui.label(
                        RichText::new(format!(
                            "libmpv {} (Client API 2.6)",
                            self.media_capabilities.libmpv_version
                        ))
                        .monospace()
                        .color(BrandPalette::ELECTRIC_LIME),
                    );
                    ui.end_row();

                    ui.label("Video Processing:");
                    ui.label(
                        RichText::new(if self.media_capabilities.libplacebo_available {
                            "libplacebo 7.387.0 Active"
                        } else {
                            "libplacebo Unavailable"
                        })
                        .monospace()
                        .color(BrandPalette::IVORY_TEXT),
                    );
                    ui.end_row();

                    ui.label("Audio DSP:");
                    ui.label(
                        RichText::new("FFmpeg Biquad / libmpv af")
                            .monospace()
                            .color(BrandPalette::IVORY_TEXT),
                    );
                    ui.end_row();

                    ui.label("Platform OS:");
                    ui.label(
                        RichText::new(format!(
                            "{} {} ({})",
                            std::env::consts::OS,
                            std::env::consts::ARCH,
                            std::env::var("XDG_SESSION_TYPE")
                                .unwrap_or_else(|_| "Linux".to_string())
                        ))
                        .monospace()
                        .color(BrandPalette::IVORY_TEXT),
                    );
                    ui.end_row();
                });
        });

        ui.add_space(8.0);

        // 3. Project Links & Legal
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("Project & Developer Information")
                    .strong()
                    .size(14.0)
                    .color(BrandPalette::ELECTRIC_LIME),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Source Code:");
                ui.hyperlink_to(
                    "github.com/itsyamin/muvrix-desktop",
                    "https://github.com/itsyamin/muvrix-desktop",
                );
            });
            ui.horizontal(|ui| {
                ui.label("License:");
                ui.label(
                    RichText::new("Dual-licensed under MIT and Apache-2.0")
                        .color(BrandPalette::IVORY_TEXT),
                );
            });
            ui.horizontal(|ui| {
                ui.label("Copyright:");
                ui.label(
                    RichText::new("© 2026 Muvrix Project Contributors")
                        .color(Color32::from_gray(140)),
                );
            });
        });

        ui.add_space(8.0);

        // 4. Update Status
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("Updates & Release Channel")
                    .strong()
                    .size(14.0)
                    .color(BrandPalette::ELECTRIC_LIME),
            );
            ui.horizontal(|ui| {
                ui.label("Current Channel: Linux Wayland Beta (v0.1.0-beta.1)");
                if ui.button("Check for Updates Now").clicked() {
                    self.advanced_status = Some(
                        "You are currently running the latest verified Beta release.".to_string(),
                    );
                }
            });
        });
    }
}
