use eframe::egui::{self, Color32, RichText};
use muvrix_player::shader::{
    Anime4KPreset, GamutMappingMode, ShaderQuality, TargetPrimaries, TargetTrc,
    ToneMappingAlgorithm, VideoAdjustmentSettings,
};

use crate::app::MuvrixApp;

impl MuvrixApp {
    pub(crate) fn render_settings_video(&mut self, ui: &mut egui::Ui) {
        // Section 3: Video Enhancement, GLSL Shaders & Picture Adjustments
        ui.group(|ui| {
                        ui.label(
                            RichText::new("3. Video Enhancement, GLSL Shaders & Picture Adjustments")
                                .strong()
                                .size(14.0)
                                .color(Color32::from_rgb(95, 185, 255)),
                        );
                        ui.label(
                            RichText::new(
                                "Configure authentic Anime4K v4.0.1 GLSL shaders, framing geometry, color adjustments, and libplacebo tone mapping.",
                            )
                            .size(12.0)
                            .color(Color32::from_gray(160)),
                        );

                        ui.add_space(6.0);

                        // Capability Diagnostics Badges
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Engine:").size(11.0).color(Color32::from_gray(140)));
                            ui.label(
                                RichText::new(format!("libmpv {}", self.media_capabilities.libmpv_version))
                                    .monospace()
                                    .size(11.0)
                                    .color(Color32::from_rgb(147, 197, 253)),
                            );
                            let placebo_str = if self.media_capabilities.libplacebo_available {
                                "libplacebo: Active"
                            } else {
                                "libplacebo: Unavailable"
                            };
                            ui.label(
                                RichText::new(placebo_str)
                                    .monospace()
                                    .size(11.0)
                                    .color(if self.media_capabilities.libplacebo_available {
                                        Color32::from_rgb(134, 239, 172)
                                    } else {
                                        Color32::from_rgb(252, 165, 165)
                                    }),
                            );
                            let shader_str = if self.media_capabilities.shader_support {
                                "GLSL Shaders: Enabled"
                            } else {
                                "GLSL Shaders: Disabled"
                            };
                            ui.label(
                                RichText::new(shader_str)
                                    .monospace()
                                    .size(11.0)
                                    .color(Color32::from_rgb(216, 180, 254)),
                            );
                        });

                        ui.add_space(8.0);

                        // Anime4K Preset & Quality
                        ui.horizontal(|ui| {
                            ui.label("Anime4K Upscaler Preset: ");
                            let preset_label = self.anime4k_preset.as_str();
                            egui::ComboBox::from_id_salt("anime4k_preset_combo")
                                .selected_text(preset_label)
                                .show_ui(ui, |ui| {
                                    let presets = [
                                        Anime4KPreset::Disabled,
                                        Anime4KPreset::ModeA,
                                        Anime4KPreset::ModeB,
                                        Anime4KPreset::ModeC,
                                        Anime4KPreset::ModeAA,
                                        Anime4KPreset::ModeBB,
                                        Anime4KPreset::ModeCA,
                                    ];
                                    for p in presets {
                                        if ui.selectable_label(self.anime4k_preset == p, p.as_str()).clicked() {
                                            self.anime4k_preset = p;
                                            self.apply_video_enhancements();
                                        }
                                    }
                                });

                            ui.add_space(10.0);
                            ui.label("Quality Level: ");
                            let quality_label = match self.anime4k_quality {
                                ShaderQuality::Fast => "Fast (S)",
                                ShaderQuality::Balanced => "Balanced (M)",
                                ShaderQuality::High => "High Quality (L)",
                            };
                            egui::ComboBox::from_id_salt("anime4k_quality_combo")
                                .selected_text(quality_label)
                                .show_ui(ui, |ui| {
                                    let qualities = [
                                        (ShaderQuality::Fast, "Fast (S - Low GPU Load)"),
                                        (ShaderQuality::Balanced, "Balanced (M - Recommended)"),
                                        (ShaderQuality::High, "High Quality (L - Best Fidelity)"),
                                    ];
                                    for (q, label) in qualities {
                                        if ui.selectable_label(self.anime4k_quality == q, label).clicked() {
                                            self.anime4k_quality = q;
                                            self.apply_video_enhancements();
                                        }
                                    }
                                });
                        });

                        ui.add_space(6.0);
                        ui.label(RichText::new("Color & Picture Adjustments:").strong().size(12.5));

                        let mut adj_changed = false;

                        // Sliders: Brightness, Contrast, Saturation, Gamma, Hue
                        ui.horizontal(|ui| {
                            ui.label("Brightness: ");
                            if ui.add(egui::Slider::new(&mut self.video_settings.brightness, -100..=100)).changed() {
                                adj_changed = true;
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("Contrast:   ");
                            if ui.add(egui::Slider::new(&mut self.video_settings.contrast, -100..=100)).changed() {
                                adj_changed = true;
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("Saturation: ");
                            if ui.add(egui::Slider::new(&mut self.video_settings.saturation, -100..=100)).changed() {
                                adj_changed = true;
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("Gamma:      ");
                            if ui.add(egui::Slider::new(&mut self.video_settings.gamma, -100..=100)).changed() {
                                adj_changed = true;
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("Hue:        ");
                            if ui.add(egui::Slider::new(&mut self.video_settings.hue, -100..=100)).changed() {
                                adj_changed = true;
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("Sharpen:    ");
                            if ui.add(egui::Slider::new(&mut self.video_settings.sharpen, -5.0..=5.0).step_by(0.1)).changed() {
                                adj_changed = true;
                            }
                        });

                        if ui.checkbox(&mut self.video_settings.deband, "Deband filter (smooth color gradients and reduce banding)").changed() {
                            adj_changed = true;
                        }

                        ui.add_space(6.0);
                        ui.separator();
                        ui.label(RichText::new("Video Framing & Geometry:").strong().size(12.5));

                        ui.horizontal(|ui| {
                            ui.label("Aspect Ratio: ");
                            let cur_aspect = self.video_settings.aspect_ratio.clone();
                            egui::ComboBox::from_id_salt("video_aspect_combo")
                                .selected_text(&cur_aspect)
                                .show_ui(ui, |ui| {
                                    let ratios = [
                                        ("auto", "Auto / Native"),
                                        ("16:9", "16:9 Widescreen"),
                                        ("4:3", "4:3 Standard"),
                                        ("2.35:1", "2.35:1 Cinematic"),
                                        ("1:1", "1:1 Square"),
                                    ];
                                    for (val, desc) in ratios {
                                        let sel = cur_aspect == val;
                                        if ui.selectable_label(sel, desc).clicked() {
                                            self.video_settings.aspect_ratio = val.to_string();
                                            adj_changed = true;
                                        }
                                    }
                                });

                            ui.add_space(8.0);
                            ui.label("Zoom: ");
                            if ui.add(egui::Slider::new(&mut self.video_settings.zoom, -1.0..=2.0).step_by(0.05)).changed() {
                                adj_changed = true;
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("Pan-Scan / Crop:");
                            let mut panscan = self.controller.panscan();
                            if ui
                                .add(egui::Slider::new(&mut panscan, 0.0..=1.0).step_by(0.01))
                                .changed()
                            {
                                self.set_panscan(panscan);
                            }
                            if ui.button("Fit (0.0)").clicked() {
                                let _ = self.controller.set_panscan_fit();
                            }
                            if ui.button("Crop (1.0)").clicked() {
                                let _ = self.controller.set_panscan_crop();
                            }
                            ui.label(format!("{:.0}%", self.controller.panscan() * 100.0));
                        });

                        ui.horizontal(|ui| {
                            ui.label("Pan X:        ");
                            if ui.add(egui::Slider::new(&mut self.video_settings.pan_x, -0.5..=0.5).step_by(0.01)).changed() {
                                adj_changed = true;
                            }
                            ui.add_space(8.0);
                            ui.label("Pan Y: ");
                            if ui.add(egui::Slider::new(&mut self.video_settings.pan_y, -0.5..=0.5).step_by(0.01)).changed() {
                                adj_changed = true;
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("Rotation:     ");
                            for deg in [0, 90, 180, 270] {
                                if ui.selectable_label(self.video_settings.rotate == deg, format!("{deg}°")).clicked() {
                                    self.video_settings.rotate = deg;
                                    adj_changed = true;
                                }
                            }
                        });

                        ui.add_space(6.0);
                        ui.separator();
                        ui.label(RichText::new("libplacebo Color Management & HDR Tone-Mapping:").strong().size(12.5));

                        ui.horizontal(|ui| {
                            ui.label("Tone-Mapping: ");
                            let tm_label = self.video_settings.tone_mapping.as_str();
                            egui::ComboBox::from_id_salt("tone_mapping_combo")
                                .selected_text(tm_label)
                                .show_ui(ui, |ui| {
                                    let algorithms = [
                                        ToneMappingAlgorithm::Auto,
                                        ToneMappingAlgorithm::Clip,
                                        ToneMappingAlgorithm::Mobius,
                                        ToneMappingAlgorithm::Reinhard,
                                        ToneMappingAlgorithm::Hable,
                                        ToneMappingAlgorithm::Bt2390,
                                        ToneMappingAlgorithm::Spline,
                                    ];
                                    for alg in algorithms {
                                        if ui.selectable_label(self.video_settings.tone_mapping == alg, alg.as_str()).clicked() {
                                            self.video_settings.tone_mapping = alg;
                                            adj_changed = true;
                                        }
                                    }
                                });

                            ui.add_space(8.0);
                            ui.label("Gamut Mapping: ");
                            let gm_label = self.video_settings.gamut_mapping.as_str();
                            egui::ComboBox::from_id_salt("gamut_mapping_combo")
                                .selected_text(gm_label)
                                .show_ui(ui, |ui| {
                                    let modes = [
                                        GamutMappingMode::Auto,
                                        GamutMappingMode::Clip,
                                        GamutMappingMode::Perceptual,
                                        GamutMappingMode::Relative,
                                        GamutMappingMode::Saturation,
                                        GamutMappingMode::Desaturate,
                                        GamutMappingMode::Linear,
                                    ];
                                    for m in modes {
                                        if ui.selectable_label(self.video_settings.gamut_mapping == m, m.as_str()).clicked() {
                                            self.video_settings.gamut_mapping = m;
                                            adj_changed = true;
                                        }
                                    }
                                });
                        });

                        ui.horizontal(|ui| {
                            ui.label("Target TRC:   ");
                            let trc_label = match self.video_settings.target_trc {
                                TargetTrc::Auto => "Auto (SDR Default)",
                                TargetTrc::Srgb => "sRGB (Standard)",
                                TargetTrc::Bt1886 => "BT.1886 (HDTV)",
                                TargetTrc::Gamma22 => "Gamma 2.2",
                                TargetTrc::Linear => "Linear Light",
                                TargetTrc::Pq => "PQ (Native HDR - Deferred)",
                                TargetTrc::Hlg => "HLG (Native HDR - Deferred)",
                                _ => self.video_settings.target_trc.as_str(),
                            };
                            egui::ComboBox::from_id_salt("target_trc_combo")
                                .selected_text(trc_label)
                                .show_ui(ui, |ui| {
                                    ui.label(egui::RichText::new("SDR Display Targets (Active FBO)").small().strong());
                                    let sdr_trcs = [
                                        (TargetTrc::Auto, "Auto (SDR Default)"),
                                        (TargetTrc::Srgb, "sRGB (Standard PC)"),
                                        (TargetTrc::Bt1886, "BT.1886 (Rec.709 Video)"),
                                        (TargetTrc::Gamma22, "Gamma 2.2"),
                                        (TargetTrc::Linear, "Linear Light"),
                                    ];
                                    for (trc, lbl) in sdr_trcs {
                                        if ui.selectable_label(self.video_settings.target_trc == trc, lbl).clicked() {
                                            self.video_settings.target_trc = trc;
                                            adj_changed = true;
                                        }
                                    }

                                    ui.separator();
                                    ui.label(egui::RichText::new("Native HDR (Deferred — requires FP16/scRGB)").small().weak());
                                    let hdr_trcs = [
                                        (TargetTrc::Pq, "PQ / ST 2084 (Deferred)"),
                                        (TargetTrc::Hlg, "HLG / BT.2100 (Deferred)"),
                                    ];
                                    for (trc, lbl) in hdr_trcs {
                                        let btn = ui.add_enabled(
                                            false,
                                            egui::Button::new(lbl)
                                                .selected(self.video_settings.target_trc == trc),
                                        );
                                        btn.on_disabled_hover_text("Native HDR presentation is deferred while rendering to GL_RGBA8 SDR FBO. HDR sources are tone-mapped to SDR targets above.");
                                    }
                                });

                            ui.add_space(8.0);
                            ui.label("Target Primaries: ");
                            let prim_label = self.video_settings.target_prim.as_str();
                            egui::ComboBox::from_id_salt("target_primaries_combo")
                                .selected_text(prim_label)
                                .show_ui(ui, |ui| {
                                    let prims = [
                                        TargetPrimaries::Auto,
                                        TargetPrimaries::Bt709,
                                        TargetPrimaries::Bt2020,
                                        TargetPrimaries::DciP3,
                                        TargetPrimaries::DisplayP3,
                                        TargetPrimaries::AdobeRgb,
                                    ];
                                    for prim in prims {
                                        if ui.selectable_label(self.video_settings.target_prim == prim, prim.as_str()).clicked() {
                                            self.video_settings.target_prim = prim;
                                            adj_changed = true;
                                        }
                                    }
                                });
                        });

                        ui.add_space(2.0);
                        ui.label(
                            egui::RichText::new("ℹ GL_RGBA8 SDR Compositor active: HDR10/HLG inputs are tone-mapped via libplacebo into SDR.")
                                .small()
                                .weak(),
                        );

                        ui.horizontal(|ui| {
                            if ui.checkbox(&mut self.video_settings.hdr_compute_peak, "Compute Dynamic Peak Luminance").changed() {
                                adj_changed = true;
                            }
                            ui.add_space(8.0);
                            ui.label("Max Boost: ");
                            if ui.add(egui::Slider::new(&mut self.video_settings.tone_mapping_max_boost, 1.0..=3.0).step_by(0.1)).changed() {
                                adj_changed = true;
                            }
                        });

                        ui.label(
                            RichText::new(
                                "ℹ Tone-mapping processes wide color gamut and HDR10/HLG dynamic ranges into standard 8-bit GL_RGBA8 SDR framebuffers via libplacebo shaders. True 10-bit scRGB/HDR passthrough is deferred to Phase 1.3.",
                            )
                            .size(10.5)
                            .color(Color32::from_gray(140)),
                        );

                        if adj_changed {
                            self.apply_video_enhancements();
                        }

                        ui.add_space(6.0);
                        if ui.button("Reset Picture Adjustments & Shaders to Defaults").clicked() {
                            self.video_settings = VideoAdjustmentSettings::default();
                            self.anime4k_preset = Anime4KPreset::Disabled;
                            self.anime4k_quality = ShaderQuality::Balanced;
                            self.apply_video_enhancements();
                        }
                    });
    }
}
