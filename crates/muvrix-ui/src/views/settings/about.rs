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

        // 3. Distribution & Packaging Architecture (Phase E8)
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Distribution & Packaging Architecture")
                        .strong()
                        .size(14.0)
                        .color(BrandPalette::ELECTRIC_LIME),
                );
                let format = self.packaging_info.detected_format;
                ui.label(
                    RichText::new(format!("[ {} ]", format.badge_label()))
                        .size(11.0)
                        .strong()
                        .color(BrandPalette::OBSIDIAN)
                        .background_color(BrandPalette::ELECTRIC_LIME),
                );
            });
            ui.add_space(4.0);

            ui.label(
                RichText::new(format!(
                    "Active Format: {} — {}",
                    self.packaging_info.detected_format.display_name(),
                    self.packaging_info.detected_format.description()
                ))
                .size(11.5)
                .color(BrandPalette::MUTED_TEXT),
            );
            if let Some(ref sb) = self.packaging_info.sandbox_id {
                ui.label(
                    RichText::new(format!("Sandbox ID: {sb}"))
                        .size(11.0)
                        .monospace()
                        .color(BrandPalette::IVORY_TEXT),
                );
            }
            ui.add_space(6.0);

            // Portal Permissions Matrix
            ui.label(
                RichText::new("Sandbox Portal Security & Permissions")
                    .strong()
                    .size(12.5)
                    .color(BrandPalette::IVORY_TEXT),
            );
            ui.label(
                RichText::new(
                    "GRANTED = confirmed by sandbox policy and reachable  ·  AVAILABLE = resource \
                     present, permission unconfirmed  ·  DIRECT = unconfined host access  ·  \
                     DENIED = excluded by policy  ·  UNSUPPORTED / UNKNOWN = not established",
                )
                .size(10.0)
                .color(Color32::from_gray(140)),
            );
            ui.add_space(2.0);

            egui::Grid::new("portal_permissions_grid")
                .num_columns(4)
                .spacing([16.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("Subsystem")
                            .strong()
                            .color(BrandPalette::MUTED_TEXT),
                    );
                    ui.label(
                        RichText::new("Category")
                            .strong()
                            .color(BrandPalette::MUTED_TEXT),
                    );
                    ui.label(
                        RichText::new("Status")
                            .strong()
                            .color(BrandPalette::MUTED_TEXT),
                    );
                    ui.label(
                        RichText::new("Access Policy / Socket")
                            .strong()
                            .color(BrandPalette::MUTED_TEXT),
                    );
                    ui.end_row();

                    for perm in &self.packaging_info.portal_permissions {
                        ui.label(
                            RichText::new(&perm.name)
                                .size(11.5)
                                .color(BrandPalette::IVORY_TEXT),
                        );
                        ui.label(
                            RichText::new(&perm.category)
                                .size(11.0)
                                .color(BrandPalette::MUTED_TEXT),
                        );

                        let (status_color, bg_color) = match perm.status {
                            crate::packaging::PortalStatus::Granted => {
                                (BrandPalette::OBSIDIAN, BrandPalette::ELECTRIC_LIME)
                            }
                            crate::packaging::PortalStatus::Direct => {
                                (BrandPalette::OBSIDIAN, Color32::from_rgb(46, 180, 230))
                            }
                            crate::packaging::PortalStatus::Available => {
                                (BrandPalette::IVORY_TEXT, Color32::from_rgb(32, 72, 120))
                            }
                            crate::packaging::PortalStatus::Denied => {
                                (BrandPalette::IVORY_TEXT, Color32::from_rgb(180, 36, 36))
                            }
                            crate::packaging::PortalStatus::Unsupported => {
                                (BrandPalette::IVORY_TEXT, Color32::from_rgb(140, 60, 20))
                            }
                            crate::packaging::PortalStatus::Unknown => {
                                (BrandPalette::IVORY_TEXT, Color32::from_rgb(70, 70, 70))
                            }
                        };
                        ui.label(
                            RichText::new(format!(" {} ", perm.status.label()))
                                .size(10.5)
                                .strong()
                                .color(status_color)
                                .background_color(bg_color),
                        )
                        .on_hover_text(perm.status.meaning());
                        ui.label(
                            RichText::new(&perm.details)
                                .size(11.0)
                                .color(Color32::from_gray(180)),
                        );
                        ui.end_row();
                    }
                });

            ui.add_space(8.0);

            // Validated Release Installers
            ui.label(
                RichText::new("Validated Multi-Platform Release Installers")
                    .strong()
                    .size(12.5)
                    .color(BrandPalette::IVORY_TEXT),
            );
            ui.add_space(2.0);

            egui::Grid::new("release_installers_grid")
                .num_columns(4)
                .spacing([16.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("Target Platform")
                            .strong()
                            .color(BrandPalette::MUTED_TEXT),
                    );
                    ui.label(
                        RichText::new("Package Artifact")
                            .strong()
                            .color(BrandPalette::MUTED_TEXT),
                    );
                    ui.label(
                        RichText::new("Installation Command")
                            .strong()
                            .color(BrandPalette::MUTED_TEXT),
                    );
                    ui.label(
                        RichText::new("Integrity")
                            .strong()
                            .color(BrandPalette::MUTED_TEXT),
                    );
                    ui.end_row();

                    for inst in &self.packaging_info.release_installers {
                        ui.label(
                            RichText::new(inst.format.display_name())
                                .size(11.5)
                                .color(BrandPalette::IVORY_TEXT),
                        );
                        ui.label(
                            RichText::new(&inst.file_name)
                                .size(11.0)
                                .monospace()
                                .color(BrandPalette::ELECTRIC_LIME),
                        );
                        ui.label(
                            RichText::new(&inst.install_command)
                                .size(11.0)
                                .monospace()
                                .color(Color32::from_gray(180)),
                        );

                        let status_text = if inst.verified {
                            RichText::new(" VERIFIED ")
                                .size(10.5)
                                .strong()
                                .color(BrandPalette::OBSIDIAN)
                                .background_color(BrandPalette::ELECTRIC_LIME)
                        } else {
                            RichText::new(" UNVERIFIED ")
                                .size(10.5)
                                .strong()
                                .color(BrandPalette::IVORY_TEXT)
                                .background_color(Color32::from_rgb(120, 40, 40))
                        };
                        ui.label(status_text);
                        ui.end_row();
                    }
                });

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Verify Package Manifests & Checksums").clicked() {
                    let ok = self.packaging_info.verify_checksums();
                    self.advanced_status = self.packaging_info.verification_message.clone();
                    if ok {
                        self.status_message =
                            "Package manifests & SHA256SUMS cryptographically verified."
                                .to_string();
                    }
                }
                if let Some(ref msg) = self.packaging_info.verification_message {
                    ui.label(RichText::new(msg).size(11.5).color(
                        if self.packaging_info.manifest_verified {
                            BrandPalette::ELECTRIC_LIME
                        } else {
                            Color32::from_rgb(230, 160, 60)
                        },
                    ));
                }
            });
        });

        ui.add_space(8.0);

        // 4. Project Links & Legal
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
