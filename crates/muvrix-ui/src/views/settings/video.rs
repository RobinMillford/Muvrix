use eframe::egui::{self, Color32, Pos2, Rect, RichText, Stroke, Vec2};
use muvrix_player::shader::{
    evaluate_tone_mapping_curve, Anime4KPreset, CustomShaderEntry, DebandPreset, GamutMappingMode,
    ShaderManager, ShaderQuality, TargetPrimaries, TargetTrc, ToneMappingAlgorithm,
    VideoAdjustmentSettings,
};

use crate::app::MuvrixApp;

impl MuvrixApp {
    pub(crate) fn render_settings_video(&mut self, ui: &mut egui::Ui) {
        // Section 3: Video Enhancement, GLSL Shaders, Deband & HDR Calibrator
        ui.group(|ui| {
            ui.label(
                RichText::new("3. Video Enhancement, GLSL Shaders & HDR Calibrator")
                    .strong()
                    .size(14.0)
                    .color(Color32::from_rgb(95, 185, 255)),
            );
            ui.label(
                RichText::new(
                    "Configure authentic Anime4K v4.0.1 GLSL shaders, custom shader chains, deband/denoise filters, and libplacebo HDR calibration.",
                )
                .size(12.0)
                .color(Color32::from_gray(160)),
            );

            ui.add_space(6.0);

            // Capability Diagnostics Badges
            ui.horizontal_wrapped(|ui| {
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

                let active_custom = self
                    .video_settings
                    .custom_shaders
                    .iter()
                    .filter(|s| s.enabled)
                    .count();
                ui.label(
                    RichText::new(format!("Custom Shaders: {active_custom} active"))
                        .monospace()
                        .size(11.0)
                        .color(if active_custom > 0 {
                            Color32::from_rgb(250, 204, 21)
                        } else {
                            Color32::from_gray(140)
                        }),
                );
            });

            ui.add_space(8.0);

            // Subsection A: Anime4K Preset & Quality
            ui.label(RichText::new("A. Anime4K v4.0.1 Upscaler Preset:").strong().size(12.5));
            ui.horizontal(|ui| {
                ui.label("Preset Mode: ");
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

            ui.add_space(8.0);
            ui.separator();

            // Subsection B: Custom GLSL / Hook Shader Chain Manager
            ui.label(RichText::new("B. Custom GLSL / .hook Shader Chain:").strong().size(12.5));
            ui.label(
                RichText::new("Add community GLSL or .hook shaders (e.g. FSRCNNX, CAS, KrigBilateral) to the video pipeline.")
                    .size(11.0)
                    .color(Color32::from_gray(150)),
            );

            let mut shader_pipeline_changed = false;

            // Shader add input row
            ui.horizontal(|ui| {
                ui.label("Shader Path:");
                let text_edit = ui.add(
                    egui::TextEdit::singleline(&mut self.custom_shader_path_input)
                        .hint_text("Absolute path to .glsl or .hook file")
                        .desired_width(320.0),
                );

                if ui.button("＋ Add Shader").clicked() || (text_edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                    let path = self.custom_shader_path_input.trim().to_string();
                    if !path.is_empty() {
                        match ShaderManager::validate_shader_path(&path) {
                            Ok(canonical) => {
                                let path_str = canonical.to_string_lossy().to_string();
                                if self.video_settings.custom_shaders.iter().any(|s| s.path == path_str) {
                                    self.custom_shader_error = Some("Shader already present in chain".to_string());
                                } else {
                                    self.video_settings.custom_shaders.push(CustomShaderEntry::new(path_str));
                                    self.custom_shader_path_input.clear();
                                    self.custom_shader_error = None;
                                    shader_pipeline_changed = true;
                                }
                            }
                            Err(e) => {
                                self.custom_shader_error = Some(format!("Invalid shader file: {e:?}"));
                            }
                        }
                    }
                }

                if !self.video_settings.custom_shaders.is_empty() && ui.button("Clear Shaders").clicked() {
                    self.video_settings.custom_shaders.clear();
                    self.custom_shader_error = None;
                    shader_pipeline_changed = true;
                }
            });

            if let Some(err) = &self.custom_shader_error {
                ui.label(RichText::new(format!("⚠ {err}")).size(11.0).color(Color32::from_rgb(248, 113, 113)));
            }

            // Shader chain table/list
            if self.video_settings.custom_shaders.is_empty() {
                ui.label(RichText::new("  (No custom shaders loaded)").size(11.0).color(Color32::from_gray(120)));
            } else {
                let mut to_remove = None;
                let mut to_move_up = None;
                let mut to_move_down = None;
                let len = self.video_settings.custom_shaders.len();

                for (idx, entry) in self.video_settings.custom_shaders.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut entry.enabled, "").changed() {
                            shader_pipeline_changed = true;
                        }
                        ui.label(
                            RichText::new(format!("{}. {}", idx + 1, entry.name))
                                .strong()
                                .size(11.5)
                                .color(if entry.enabled {
                                    Color32::from_rgb(220, 220, 220)
                                } else {
                                    Color32::from_gray(120)
                                }),
                        );
                        ui.label(
                            RichText::new(format!("({})", entry.path))
                                .size(10.0)
                                .color(Color32::from_gray(130)),
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("✕").on_hover_text("Remove shader").clicked() {
                                to_remove = Some(idx);
                            }
                            if idx + 1 < len && ui.button("▼").on_hover_text("Move down").clicked() {
                                to_move_down = Some(idx);
                            }
                            if idx > 0 && ui.button("▲").on_hover_text("Move up").clicked() {
                                to_move_up = Some(idx);
                            }
                        });
                    });
                }

                if let Some(idx) = to_remove {
                    self.video_settings.custom_shaders.remove(idx);
                    shader_pipeline_changed = true;
                }
                if let Some(idx) = to_move_up {
                    self.video_settings.custom_shaders.swap(idx, idx - 1);
                    shader_pipeline_changed = true;
                }
                if let Some(idx) = to_move_down {
                    self.video_settings.custom_shaders.swap(idx, idx + 1);
                    shader_pipeline_changed = true;
                }
            }

            if shader_pipeline_changed {
                self.apply_video_enhancements();
            }

            ui.add_space(8.0);
            ui.separator();

            // Subsection C: Picture Adjustments
            ui.label(RichText::new("C. Color & Picture Adjustments:").strong().size(12.5));

            let mut adj_changed = false;

            // Sliders: Brightness, Contrast, Saturation, Gamma, Hue, Sharpen
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

            ui.add_space(8.0);
            ui.separator();

            // Subsection D: Deband & Denoise Filter Tuning
            ui.label(RichText::new("D. Deband & Denoise Video Filters:").strong().size(12.5));
            ui.horizontal(|ui| {
                if ui.checkbox(&mut self.video_settings.deband, "Enable Deband Filter (Gradient Smoothing)").changed() {
                    adj_changed = true;
                }

                if self.video_settings.deband {
                    ui.label("  Preset: ");
                    for preset in [
                        DebandPreset::Default,
                        DebandPreset::Medium,
                        DebandPreset::Aggressive,
                        DebandPreset::Anime,
                    ] {
                        if ui.button(preset.as_str()).on_hover_text(preset.label()).clicked() {
                            preset.apply_to(&mut self.video_settings);
                            adj_changed = true;
                        }
                    }
                }
            });

            if self.video_settings.deband {
                ui.indent("deband_details", |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Iterations: ");
                        if ui.add(egui::Slider::new(&mut self.video_settings.deband_iterations, 1..=8)).changed() {
                            adj_changed = true;
                        }
                        ui.add_space(8.0);
                        ui.label("Threshold: ");
                        if ui.add(egui::Slider::new(&mut self.video_settings.deband_threshold, 16..=512)).changed() {
                            adj_changed = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Range:      ");
                        if ui.add(egui::Slider::new(&mut self.video_settings.deband_range, 4..=64)).changed() {
                            adj_changed = true;
                        }
                        ui.add_space(8.0);
                        ui.label("Grain:     ");
                        if ui.add(egui::Slider::new(&mut self.video_settings.deband_grain, 0..=256)).changed() {
                            adj_changed = true;
                        }
                    });
                });
            }

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.checkbox(&mut self.video_settings.denoise, "Enable Temporal Denoise (hqdn3d)").changed() {
                    adj_changed = true;
                }
                if self.video_settings.denoise {
                    ui.label("Strength: ");
                    if ui.add(egui::Slider::new(&mut self.video_settings.denoise_strength, 0.5..=10.0).step_by(0.1)).changed() {
                        adj_changed = true;
                    }
                }
            });

            ui.add_space(8.0);
            ui.separator();

            // Subsection E: Video Framing & Geometry
            ui.label(RichText::new("E. Video Framing & Geometry:").strong().size(12.5));

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

            ui.add_space(8.0);
            ui.separator();

            // Subsection F: libplacebo Color Management & HDR Tone-Mapping
            ui.label(RichText::new("F. libplacebo HDR Tone-Mapping & Gamut Calibrator:").strong().size(12.5));

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
                            ToneMappingAlgorithm::Bt2446a,
                            ToneMappingAlgorithm::Spline,
                            ToneMappingAlgorithm::Linear,
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

            // Target Peak Nits slider & HDR compute peak
            ui.horizontal(|ui| {
                ui.label("Target Peak Luminance: ");
                let peak_val = self.video_settings.target_peak;
                let peak_label = if peak_val == 0 {
                    "Auto (Display Native)".to_string()
                } else {
                    format!("{peak_val} nits")
                };

                let mut slider_val = if peak_val == 0 { 1000 } else { peak_val };
                if ui.add(egui::Slider::new(&mut slider_val, 100..=4000).suffix(" nits")).changed() {
                    self.video_settings.target_peak = slider_val;
                    adj_changed = true;
                }

                if ui.selectable_label(peak_val == 0, "Auto").clicked() {
                    self.video_settings.target_peak = 0;
                    adj_changed = true;
                }

                ui.label(RichText::new(format!("Active: {peak_label}")).size(11.0).color(Color32::from_rgb(147, 197, 253)));
            });

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

            ui.add_space(8.0);
            ui.separator();

            // Subsection G: Interactive Tone-Mapping Curve Visualizer
            ui.label(RichText::new("G. Interactive Tone-Mapping Response Curve:").strong().size(12.5));
            ui.label(
                RichText::new("Real-time mathematical transfer curve mapping input scene luminance (0..10,000 nits) to display capability.")
                    .size(11.0)
                    .color(Color32::from_gray(150)),
            );

            ui.add_space(4.0);
            self.render_tone_mapping_curve_canvas(ui);

            if adj_changed {
                self.apply_video_enhancements();
            }

            ui.add_space(8.0);
            if ui.button("Reset Picture Adjustments & Shaders to Defaults").clicked() {
                self.video_settings = VideoAdjustmentSettings::default();
                self.anime4k_preset = Anime4KPreset::Disabled;
                self.anime4k_quality = ShaderQuality::Balanced;
                self.custom_shader_path_input.clear();
                self.custom_shader_error = None;
                self.apply_video_enhancements();
            }
        });
    }

    /// Draws the interactive tone-mapping response curve canvas.
    fn render_tone_mapping_curve_canvas(&mut self, ui: &mut egui::Ui) {
        let canvas_w = ui.available_width().max(320.0);
        let canvas_h = 150.0;
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(canvas_w, canvas_h), egui::Sense::hover());
        let painter = ui.painter_at(rect);

        // Background
        painter.rect_filled(rect, 4.0, Color32::from_rgb(18, 20, 24));
        painter.rect_stroke(
            rect,
            4.0,
            Stroke::new(1.0_f32, Color32::from_gray(50)),
            egui::StrokeKind::Inside,
        );

        // Parameters
        let peak = if self.video_settings.target_peak > 0 {
            self.video_settings.target_peak as f64
        } else {
            1000.0
        };
        let max_boost = self.video_settings.tone_mapping_max_boost;
        let alg = self.video_settings.tone_mapping;

        // X-axis: 0 to 10,000 nits (log scale for natural perceptual distribution)
        // Log mapping: log10(nits + 10) from log10(10)=1.0 to log10(10010)=4.0
        let log_min = 1.0_f64; // log10(10)
        let log_max = 4.0_f64; // log10(10000)
        let log_span = log_max - log_min;

        let nits_to_x = |nits: f64| -> f32 {
            let log_val = (nits + 10.0).log10();
            let frac = ((log_val - log_min) / log_span).clamp(0.0, 1.0) as f32;
            rect.left() + 45.0 + frac * (rect.width() - 55.0)
        };

        let x_to_nits = |x: f32| -> f64 {
            let frac = ((x - (rect.left() + 45.0)) / (rect.width() - 55.0)).clamp(0.0, 1.0) as f64;
            let log_val = log_min + frac * log_span;
            10.0_f64.powf(log_val) - 10.0
        };

        let y_bottom = rect.bottom() - 25.0;
        let y_top = rect.top() + 15.0;
        let y_span = y_bottom - y_top;

        let out_to_y = |out: f64| -> f32 {
            let frac = out.clamp(0.0, 1.0) as f32;
            y_bottom - frac * y_span
        };

        // Grid lines (Luminance reference points: 100 nits SDR, 400 nits, 1000 nits, 4000 nits, 10000 nits)
        let nits_refs = [
            (100.0, "100n (SDR)"),
            (400.0, "400n"),
            (1000.0, "1000n"),
            (4000.0, "4000n"),
            (10000.0, "10kn"),
        ];

        for (nits, label) in nits_refs {
            let x = nits_to_x(nits);
            painter.line_segment(
                [Pos2::new(x, y_top), Pos2::new(x, y_bottom)],
                Stroke::new(1.0_f32, Color32::from_gray(35)),
            );
            painter.text(
                Pos2::new(x, y_bottom + 4.0),
                egui::Align2::CENTER_TOP,
                label,
                egui::FontId::proportional(9.0),
                Color32::from_gray(120),
            );
        }

        // Horizontal Y-axis grid (0%, 25%, 50%, 75%, 100% display capability)
        for pct in [0.0, 0.25, 0.50, 0.75, 1.0] {
            let y = out_to_y(pct);
            painter.line_segment(
                [
                    Pos2::new(rect.left() + 45.0, y),
                    Pos2::new(rect.right() - 10.0, y),
                ],
                Stroke::new(1.0_f32, Color32::from_gray(35)),
            );
            painter.text(
                Pos2::new(rect.left() + 40.0, y),
                egui::Align2::RIGHT_CENTER,
                format!("{:.0}%", pct * 100.0),
                egui::FontId::monospace(9.0),
                Color32::from_gray(120),
            );
        }

        // Target peak marker vertical line
        let peak_x = nits_to_x(peak);
        painter.line_segment(
            [Pos2::new(peak_x, y_top), Pos2::new(peak_x, y_bottom)],
            Stroke::new(1.0_f32, Color32::from_rgba_premultiplied(95, 185, 255, 100)),
        );
        painter.text(
            Pos2::new(peak_x, y_top - 10.0),
            egui::Align2::CENTER_BOTTOM,
            format!("Target Peak: {:.0}n", peak),
            egui::FontId::proportional(9.5),
            Color32::from_rgb(95, 185, 255),
        );

        // Tone-mapping curve points
        let num_points = 120;
        let mut curve_points = Vec::with_capacity(num_points);

        for i in 0..num_points {
            let frac = i as f64 / (num_points - 1) as f64;
            let log_val = log_min + frac * log_span;
            let in_nits = (10.0_f64.powf(log_val) - 10.0).max(0.0);
            let out_val = evaluate_tone_mapping_curve(alg, in_nits, peak, max_boost);
            let x = nits_to_x(in_nits);
            let y = out_to_y(out_val);
            curve_points.push(Pos2::new(x, y));
        }

        // Draw glowing tone-mapping curve
        let curve_color = Color32::from_rgb(134, 239, 172); // Lime/Emerald tone
        for w in curve_points.windows(2) {
            painter.line_segment([w[0], w[1]], Stroke::new(2.0_f32, curve_color));
        }

        // Interactive hover cursor
        if let Some(mouse_pos) = response.hover_pos() {
            if mouse_pos.x >= rect.left() + 45.0
                && mouse_pos.x <= rect.right() - 10.0
                && mouse_pos.y >= y_top
                && mouse_pos.y <= y_bottom
            {
                let hover_nits = x_to_nits(mouse_pos.x);
                self.tone_mapping_hover_nits = Some(hover_nits);
                let hover_out = evaluate_tone_mapping_curve(alg, hover_nits, peak, max_boost);
                let cursor_y = out_to_y(hover_out);

                // Draw crosshairs
                painter.line_segment(
                    [
                        Pos2::new(mouse_pos.x, y_top),
                        Pos2::new(mouse_pos.x, y_bottom),
                    ],
                    Stroke::new(1.0_f32, Color32::from_rgba_premultiplied(250, 204, 21, 160)),
                );
                painter.circle_filled(
                    Pos2::new(mouse_pos.x, cursor_y),
                    4.0,
                    Color32::from_rgb(250, 204, 21),
                );

                // Hover readout badge
                let readout = format!(
                    "In: {:.0} nits (HDR) → Out: {:.1}% | Gain: {:.2}x",
                    hover_nits,
                    hover_out * 100.0,
                    if hover_nits > 0.0 {
                        (hover_out * peak) / hover_nits
                    } else {
                        1.0
                    }
                );

                let badge_pos = Pos2::new(
                    (mouse_pos.x + 10.0).min(rect.right() - 220.0),
                    (cursor_y - 20.0).max(rect.top() + 5.0),
                );
                let badge_rect = Rect::from_min_size(badge_pos, Vec2::new(210.0, 18.0));
                painter.rect_filled(
                    badge_rect,
                    3.0,
                    Color32::from_rgba_premultiplied(10, 10, 12, 220),
                );
                painter.rect_stroke(
                    badge_rect,
                    3.0,
                    Stroke::new(1.0_f32, Color32::from_rgb(250, 204, 21)),
                    egui::StrokeKind::Inside,
                );
                painter.text(
                    badge_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    readout,
                    egui::FontId::monospace(9.5),
                    Color32::from_rgb(250, 204, 21),
                );
            }
        } else {
            self.tone_mapping_hover_nits = None;
        }
    }
}
