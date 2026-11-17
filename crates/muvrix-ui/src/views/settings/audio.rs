use eframe::egui::{self, Color32, RichText, Sense, StrokeKind};
use muvrix_player::equalizer::{Bs2bPreset, EqualizerPreset, EqualizerSettings, EQ_FREQUENCIES};
use muvrix_player::AudioChannelsSetting;

use crate::app::MuvrixApp;
use crate::branding::BrandPalette;
use crate::icons::{self, Icon};

/// Renders a spline-interpolated, mathematically calibrated frequency response curve
/// for the 10-band peaking biquad equalizer cascade across 20 Hz to 20 kHz.
fn render_eq_frequency_response_curve(ui: &mut egui::Ui, eq: &EqualizerSettings) {
    let height = 120.0;
    let width = ui.available_width().max(320.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
    let painter = ui.painter_at(rect);

    // Obsidian Dark canvas
    painter.rect_filled(rect, 4.0, Color32::from_rgb(8, 13, 18));
    painter.rect_stroke(
        rect,
        4.0,
        egui::Stroke::new(1.0_f32, Color32::from_rgb(24, 33, 43)),
        StrokeKind::Inside,
    );

    let left = rect.left() + 40.0;
    let right = rect.right() - 14.0;
    let top = rect.top() + 10.0;
    let bottom = rect.bottom() - 20.0;
    let plot_w = (right - left).max(10.0);
    let plot_h = (bottom - top).max(10.0);
    let zero_y = top + plot_h * 0.5;

    // dB grid lines: +12, +6, 0, -6, -12
    let db_steps = [
        (12.0_f64, "+12"),
        (6.0_f64, "+6"),
        (0.0_f64, "0dB"),
        (-6.0_f64, "-6"),
        (-12.0_f64, "-12"),
    ];
    for &(db, label) in &db_steps {
        let norm_y = (12.0 - db) / 24.0;
        let y = top + plot_h * norm_y as f32;
        let is_zero = db == 0.0;
        let stroke_color = if is_zero {
            Color32::from_rgba_unmultiplied(197, 243, 106, 110)
        } else {
            Color32::from_rgba_unmultiplied(255, 255, 255, 18)
        };
        painter.line_segment(
            [egui::pos2(left, y), egui::pos2(right, y)],
            egui::Stroke::new(if is_zero { 1.5_f32 } else { 0.75_f32 }, stroke_color),
        );
        painter.text(
            egui::pos2(rect.left() + 34.0, y),
            egui::Align2::RIGHT_CENTER,
            label,
            egui::FontId::monospace(9.0),
            if is_zero {
                Color32::from_rgb(197, 243, 106)
            } else {
                Color32::from_gray(120)
            },
        );
    }

    // Logarithmic frequency grid lines and labels: 20 Hz to 20 kHz
    let freq_labels: [(f64, &'static str); 11] = [
        (20.0, "20"),
        (50.0, "50"),
        (100.0, "100"),
        (250.0, "250"),
        (500.0, "500"),
        (1000.0, "1k"),
        (2000.0, "2k"),
        (4000.0, "4k"),
        (8000.0, "8k"),
        (16000.0, "16k"),
        (20000.0, "20k"),
    ];
    let log_min = 20.0f64.log10();
    let log_max = 20000.0f64.log10();

    for &(f, label) in &freq_labels {
        let t = ((f.log10() - log_min) / (log_max - log_min)).clamp(0.0, 1.0) as f32;
        let x = left + plot_w * t;
        painter.line_segment(
            [egui::pos2(x, top), egui::pos2(x, bottom)],
            egui::Stroke::new(0.5_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 18)),
        );
        painter.text(
            egui::pos2(x, bottom + 10.0),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::monospace(9.0),
            Color32::from_gray(130),
        );
    }

    // Sample frequency curve points (128 points for smooth biquad rendering)
    let curve_points = eq.frequency_response_curve(128);
    let mut screen_points = Vec::with_capacity(curve_points.len());

    for (freq, gain) in &curve_points {
        let t = ((freq.log10() - log_min) / (log_max - log_min)).clamp(0.0, 1.0) as f32;
        let x = left + plot_w * t;
        let norm_y = ((12.0 - gain) / 24.0).clamp(0.0, 1.0) as f32;
        let y = top + plot_h * norm_y;
        screen_points.push(egui::pos2(x, y));
    }

    // Shaded fill between 0 dB baseline and response curve
    if screen_points.len() > 1 {
        let mut fill_mesh = egui::Mesh::default();
        let fill_color = Color32::from_rgba_unmultiplied(197, 243, 106, 26);
        for i in 0..screen_points.len() - 1 {
            let p1 = screen_points[i];
            let p2 = screen_points[i + 1];
            let b1 = egui::pos2(p1.x, zero_y);
            let b2 = egui::pos2(p2.x, zero_y);

            let idx = fill_mesh.vertices.len() as u32;
            fill_mesh.vertices.push(egui::epaint::Vertex {
                pos: p1,
                uv: egui::epaint::WHITE_UV,
                color: fill_color,
            });
            fill_mesh.vertices.push(egui::epaint::Vertex {
                pos: p2,
                uv: egui::epaint::WHITE_UV,
                color: fill_color,
            });
            fill_mesh.vertices.push(egui::epaint::Vertex {
                pos: b2,
                uv: egui::epaint::WHITE_UV,
                color: fill_color,
            });
            fill_mesh.vertices.push(egui::epaint::Vertex {
                pos: b1,
                uv: egui::epaint::WHITE_UV,
                color: fill_color,
            });
            fill_mesh.add_triangle(idx, idx + 1, idx + 2);
            fill_mesh.add_triangle(idx, idx + 2, idx + 3);
        }
        painter.add(egui::Shape::mesh(fill_mesh));

        // Response curve polyline in Electric Lime
        painter.add(egui::Shape::line(
            screen_points,
            egui::Stroke::new(2.0_f32, Color32::from_rgb(197, 243, 106)),
        ));
    }

    // Interactive hover crosshair and readout
    if let Some(hover_pos) = response.hover_pos() {
        if hover_pos.x >= left
            && hover_pos.x <= right
            && hover_pos.y >= top
            && hover_pos.y <= bottom
        {
            let t = ((hover_pos.x - left) / plot_w).clamp(0.0, 1.0) as f64;
            let hovered_freq = 10.0f64.powf(log_min + t * (log_max - log_min));
            let hovered_gain = eq.cumulative_response_at_freq(hovered_freq);
            let norm_y = ((12.0 - hovered_gain) / 24.0).clamp(0.0, 1.0) as f32;
            let curve_y = top + plot_h * norm_y;

            // Crosshair guidelines
            painter.line_segment(
                [
                    egui::pos2(hover_pos.x, top),
                    egui::pos2(hover_pos.x, bottom),
                ],
                egui::Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 70)),
            );
            painter.line_segment(
                [egui::pos2(left, curve_y), egui::pos2(right, curve_y)],
                egui::Stroke::new(0.75_f32, Color32::from_rgba_unmultiplied(197, 243, 106, 90)),
            );
            painter.circle_filled(
                egui::pos2(hover_pos.x, curve_y),
                3.5,
                Color32::from_rgb(197, 243, 106),
            );

            // Frequency and Gain pill badge
            let text = format!("{:.0} Hz | {:+.1} dB", hovered_freq, hovered_gain);
            let pill_pos = egui::pos2(
                (hover_pos.x + 8.0).min(right - 95.0),
                (curve_y - 20.0).max(top + 4.0),
            );
            let pill_rect = egui::Rect::from_min_size(pill_pos, egui::vec2(95.0, 16.0));
            painter.rect_filled(pill_rect, 3.0, Color32::from_rgb(24, 33, 43));
            painter.rect_stroke(
                pill_rect,
                3.0,
                egui::Stroke::new(1.0_f32, Color32::from_rgb(197, 243, 106)),
                StrokeKind::Inside,
            );
            painter.text(
                pill_rect.center(),
                egui::Align2::CENTER_CENTER,
                text,
                egui::FontId::monospace(9.5),
                Color32::from_rgb(244, 245, 240),
            );
        }
    }
}

impl MuvrixApp {
    pub(crate) fn render_settings_audio(&mut self, ui: &mut egui::Ui) {
        // =====================================================================
        // SECTION 1: 10-Band Parametric Equalizer & Interactive Frequency Curve
        // =====================================================================
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("1. 10-Band Parametric Equalizer & Frequency Response Curve")
                    .strong()
                    .size(14.0)
                    .color(Color32::from_rgb(95, 185, 255)),
            );
            ui.label(
                RichText::new(
                    "High-fidelity peaking biquad equalizer with real-time transfer function visualization from 20 Hz to 20 kHz.",
                )
                .size(12.0)
                .color(Color32::from_gray(160)),
            );

            ui.add_space(4.0);

            // Diagnostics & Headroom status
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("DSP Filter Engine:")
                        .size(11.0)
                        .color(Color32::from_gray(140)),
                );
                ui.label(
                    RichText::new("FFmpeg Biquad Cascade / libmpv af")
                        .monospace()
                        .size(11.0)
                        .color(Color32::from_rgb(147, 197, 253)),
                );
                if self.equalizer_settings.has_clipping_risk() {
                    ui.label(
                        RichText::new("⚠️ Digital Clipping Risk")
                            .size(11.0)
                            .color(Color32::from_rgb(251, 191, 36)),
                    );
                } else {
                    ui.label(
                        RichText::new("✓ Headroom Safe")
                            .size(11.0)
                            .color(Color32::from_rgb(134, 239, 172)),
                    );
                }
            });

            ui.add_space(6.0);

            let mut eq_changed = false;

            ui.horizontal(|ui| {
                if ui
                    .checkbox(
                        &mut self.equalizer_settings.is_enabled,
                        "Enable 10-Band Equalizer",
                    )
                    .changed()
                {
                    eq_changed = true;
                }
            });

            ui.add_space(4.0);

            // Preset selector
            ui.horizontal(|ui| {
                ui.label("EQ Preset: ");
                let preset_label = self.equalizer_settings.active_preset.as_str();
                egui::ComboBox::from_id_salt("eq_preset_combo")
                    .selected_text(preset_label)
                    .show_ui(ui, |ui| {
                        let presets = [
                            EqualizerPreset::Flat,
                            EqualizerPreset::BassBoost,
                            EqualizerPreset::BassReduction,
                            EqualizerPreset::TrebleBoost,
                            EqualizerPreset::TrebleReduction,
                            EqualizerPreset::VocalClarity,
                            EqualizerPreset::Rock,
                            EqualizerPreset::Classical,
                            EqualizerPreset::Electronic,
                            EqualizerPreset::Cinema,
                        ];
                        for p in presets {
                            if ui
                                .selectable_label(
                                    self.equalizer_settings.active_preset == p,
                                    p.as_str(),
                                )
                                .clicked()
                            {
                                self.equalizer_settings.active_preset = p;
                                self.equalizer_settings.gains = p.default_gains();
                                self.equalizer_settings.autoeq_profile = None;
                                if p == EqualizerPreset::Flat {
                                    self.equalizer_settings.preamp_db = 0.0;
                                }
                                eq_changed = true;
                            }
                        }
                    });
            });

            ui.add_space(4.0);

            // Headroom Warning and Auto-Attenuate button
            if self.equalizer_settings.has_clipping_risk() {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(
                            "⚠️ Positive EQ gains may cause digital clipping with current preamp.",
                        )
                        .color(Color32::from_rgb(251, 191, 36))
                        .size(11.5),
                    );
                    let safe_preamp = self.equalizer_settings.calculate_safe_preamp();
                    if ui
                        .button(format!("Auto-Attenuate Preamp ({:.1} dB)", safe_preamp))
                        .clicked()
                    {
                        self.equalizer_settings.preamp_db = safe_preamp;
                        eq_changed = true;
                    }
                });
            }

            // Preamp Slider
            ui.horizontal(|ui| {
                ui.label("Preamp Gain: ");
                if ui
                    .add(
                        egui::Slider::new(&mut self.equalizer_settings.preamp_db, -12.0..=12.0)
                            .text("dB"),
                    )
                    .changed()
                {
                    if self.equalizer_settings.active_preset != EqualizerPreset::Custom {
                        self.equalizer_settings.active_preset = EqualizerPreset::Custom;
                    }
                    eq_changed = true;
                }
                ui.add_space(10.0);
                ui.checkbox(&mut self.eq_show_q_factors, "Show Parametric Q Controls");
            });

            ui.add_space(6.0);

            // -----------------------------------------------------------------
            // D3.1: Interactive Frequency Response Curve
            // -----------------------------------------------------------------
            ui.label(
                RichText::new("Frequency Response (20 Hz – 20 kHz cumulative magnitude):")
                    .size(11.0)
                    .color(Color32::from_gray(160)),
            );
            render_eq_frequency_response_curve(ui, &self.equalizer_settings);

            ui.add_space(8.0);

            // 10 Frequency Band Sliders
            ui.horizontal(|ui| {
                for (i, &freq) in EQ_FREQUENCIES.iter().enumerate() {
                    ui.vertical(|ui| {
                        let label = if freq >= 1000.0 {
                            format!("{:.0}k", freq / 1000.0)
                        } else {
                            format!("{:.0}", freq)
                        };
                        ui.label(RichText::new(&label).size(10.5));
                        let slider =
                            egui::Slider::new(&mut self.equalizer_settings.gains[i], -12.0..=12.0)
                                .vertical()
                                .show_value(false);
                        if ui.add_sized([24.0, 85.0], slider).changed() {
                            self.equalizer_settings.active_preset = EqualizerPreset::Custom;
                            self.equalizer_settings.autoeq_profile = None;
                            eq_changed = true;
                        }
                        ui.label(
                            RichText::new(format!("{:.1}", self.equalizer_settings.gains[i]))
                                .size(9.5),
                        );
                    });
                }
            });

            // Parametric Q Controls (when toggled)
            if self.eq_show_q_factors {
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Band Q-Factors (Bandwidth: Higher = Sharper peak):")
                        .size(11.0)
                        .color(Color32::from_gray(160)),
                );
                ui.horizontal(|ui| {
                    for (i, &freq) in EQ_FREQUENCIES.iter().enumerate() {
                        ui.vertical(|ui| {
                            let label = if freq >= 1000.0 {
                                format!("{:.0}k", freq / 1000.0)
                            } else {
                                format!("{:.0}", freq)
                            };
                            ui.label(RichText::new(&label).size(9.5));
                            let mut q = self
                                .equalizer_settings
                                .bands
                                .get(i)
                                .map(|b| b.q_factor)
                                .unwrap_or(1.414);
                            if ui
                                .add(egui::DragValue::new(&mut q).range(0.1..=10.0).speed(0.05))
                                .changed()
                            {
                                if let Some(b) = self.equalizer_settings.bands.get_mut(i) {
                                    b.q_factor = q;
                                    self.equalizer_settings.active_preset = EqualizerPreset::Custom;
                                    eq_changed = true;
                                }
                            }
                        });
                    }
                });
            }

            ui.add_space(6.0);
            if ui.button("Reset Equalizer to Flat (0 dB)").clicked() {
                self.equalizer_settings.gains = [0.0; 10];
                self.equalizer_settings.preamp_db = 0.0;
                self.equalizer_settings.active_preset = EqualizerPreset::Flat;
                self.equalizer_settings.clear_autoeq_profile();
                eq_changed = true;
            }

            if eq_changed {
                self.apply_equalizer_settings();
            }
        });

        ui.add_space(6.0);

        // =====================================================================
        // SECTION 1B: AutoEq Parametric Preset Management & Headphone Correction
        // =====================================================================
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("AutoEq Preset Management & Headphone Correction")
                    .strong()
                    .size(13.5)
                    .color(BrandPalette::ELECTRIC_LIME),
            );
            ui.label(
                RichText::new(
                    "Import calibrated parametric equalizer profiles (JSON or EqualizerAPO text) from AutoEq, crinacle, oratory1990.",
                )
                .size(11.5)
                .color(BrandPalette::MUTED_TEXT),
            );
            ui.add_space(6.0);

            // Active profile info badge
            let mut clear_clicked = false;
            if let Some(ref autoeq) = self.equalizer_settings.autoeq_profile {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Active Profile:").strong().size(12.0));
                    ui.label(
                        RichText::new(&autoeq.name)
                            .strong()
                            .size(12.0)
                            .color(BrandPalette::ELECTRIC_LIME),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(format!("{} Filter Bands", autoeq.filters.len()))
                            .size(11.5)
                            .color(Color32::from_rgb(147, 197, 253)),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(format!("Preamp: {:+.1} dB", autoeq.preamp_db))
                            .monospace()
                            .size(11.5)
                            .color(Color32::from_gray(200)),
                    );

                    if ui.button("Clear / Revert to Standard EQ").clicked() {
                        clear_clicked = true;
                    }
                });
                ui.add_space(4.0);
            }
            if clear_clicked {
                self.clear_autoeq_profile();
            }

            // Action buttons: Import AutoEq, Export Preset
            ui.horizontal(|ui| {
                if icons::icon_text_button(
                    ui,
                    Icon::FolderOpen,
                    "Import AutoEq Profile (.txt / .json)",
                    "Select an AutoEq JSON or EqualizerAPO text profile",
                )
                .clicked()
                {
                    self.import_autoeq_dialog();
                }

                if icons::icon_text_button(
                    ui,
                    Icon::Playlist,
                    "Export Preset",
                    "Export current equalizer settings as AutoEq text or JSON",
                )
                .clicked()
                {
                    self.export_autoeq_dialog();
                }
            });

            // Status feedback banner
            if let Some((ref msg, is_err)) = self.autoeq_status_msg {
                ui.add_space(4.0);
                let color = if is_err {
                    Color32::from_rgb(255, 100, 100)
                } else {
                    BrandPalette::ELECTRIC_LIME
                };
                ui.label(RichText::new(msg).size(11.0).color(color));
            }
        });

        ui.add_space(6.0);

        // =====================================================================
        // SECTION 2: Headphone Crossfeed (Bauer BS2B)
        // =====================================================================
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("2. Headphone Crossfeed (Bauer BS2B)")
                    .strong()
                    .size(14.0)
                    .color(BrandPalette::ELECTRIC_LIME),
            );
            ui.label(
                RichText::new(
                    "Blends stereo channels using binaural delay and frequency attenuation to eliminate listener fatigue on headphones.",
                )
                .size(12.0)
                .color(BrandPalette::MUTED_TEXT),
            );
            ui.add_space(4.0);

            let mut crossfeed_changed = false;
            ui.horizontal(|ui| {
                if ui
                    .checkbox(
                        &mut self.equalizer_settings.crossfeed_enabled,
                        "Enable Bauer Headphone Crossfeed",
                    )
                    .changed()
                {
                    crossfeed_changed = true;
                }

                ui.add_space(14.0);
                ui.label("Crossfeed Preset: ");
                let cur_preset = self.equalizer_settings.crossfeed_preset;
                egui::ComboBox::from_id_salt("bs2b_preset_combo")
                    .selected_text(cur_preset.as_str())
                    .show_ui(ui, |ui| {
                        for p in [Bs2bPreset::Default, Bs2bPreset::ChuMoy, Bs2bPreset::JanMeier] {
                            if ui.selectable_label(cur_preset == p, p.as_str()).clicked() {
                                self.equalizer_settings.crossfeed_preset = p;
                                crossfeed_changed = true;
                            }
                        }
                    });
            });

            ui.add_space(2.0);
            ui.label(
                RichText::new(format!(
                    "Active Profile: {}",
                    self.equalizer_settings.crossfeed_preset.description()
                ))
                .italics()
                .size(11.0)
                .color(Color32::from_gray(160)),
            );

            if crossfeed_changed {
                self.apply_equalizer_settings();
            }
        });

        ui.add_space(6.0);

        // =====================================================================
        // SECTION 3: Loudness Normalization (EBU R128 & Dynamic Normalizer)
        // =====================================================================
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("3. Audio Loudness Normalization (EBU R128 & Dynamic)")
                    .strong()
                    .size(14.0)
                    .color(BrandPalette::ELECTRIC_LIME),
            );
            ui.label(
                RichText::new(
                    "Broadcast-standard EBU R128 loudness normalization (LUFS, True Peak, LRA) and dynamic compression.",
                )
                .size(12.0)
                .color(BrandPalette::MUTED_TEXT),
            );
            ui.add_space(6.0);

            let mut norm_changed = false;
            let current_mode = self.equalizer_settings.effective_normalization_mode();

            // Normalization mode selection pills
            ui.horizontal(|ui| {
                ui.label(RichText::new("Normalization Mode:").strong());
                for (mode, label) in [
                    (muvrix_player::NormalizationMode::None, "Disabled"),
                    (muvrix_player::NormalizationMode::EbuR128, "EBU R128 (Broadcast Standard)"),
                    (muvrix_player::NormalizationMode::Dynamic, "Dynamic (dynaudnorm)"),
                ] {
                    if ui.selectable_label(current_mode == mode, label).clicked() {
                        self.equalizer_settings.set_normalization_mode(mode);
                        norm_changed = true;
                    }
                }
            });

            ui.add_space(6.0);

            match current_mode {
                muvrix_player::NormalizationMode::EbuR128 => {
                    ui.label(
                        RichText::new("EBU R128 Loudness Target Preset:")
                            .strong()
                            .size(11.5)
                            .color(Color32::from_rgb(147, 197, 253)),
                    );

                    // Preset Selector
                    ui.horizontal(|ui| {
                        let cur_preset = self.equalizer_settings.ebu_r128.preset;
                        egui::ComboBox::from_id_salt("ebu_r128_preset_combo")
                            .selected_text(cur_preset.as_str())
                            .show_ui(ui, |ui| {
                                for p in [
                                    muvrix_player::EbuR128Preset::Streaming,
                                    muvrix_player::EbuR128Preset::Broadcast,
                                    muvrix_player::EbuR128Preset::Custom,
                                ] {
                                    if ui.selectable_label(cur_preset == p, p.as_str()).clicked() {
                                        self.equalizer_settings.ebu_r128.apply_preset(p);
                                        norm_changed = true;
                                    }
                                }
                            });
                    });

                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(
                            "Broadcast Target (-23.0 LUFS, -2.0 dBTP, 7.0 LU): -23.0 LUFS programme loudness (EBU R128); -1.0 dBTP linear PCM ceiling, -2.0 dBTP conservative ceiling for lossy codecs (EBU Tech 3343); 7.0 LU dynamic processing window (assessment parameter, not mandatory requirement).\nStreaming Music Target (-16.0 LUFS, -1.5 dBTP, 11.0 LU): Music-oriented streaming target aligned with AES TD1008 (2021) track-normalized music (-16 LUFS). (Note: AES TD1008 recommends -18 LUFS for speech/podcasts and -14 LUFS for album loudest tracks). Single-pass dynamic processing used for interactive playback.",
                        )
                        .size(10.5)
                        .color(Color32::from_gray(150)),
                    );
                    ui.add_space(4.0);

                    // Integrated loudness (LUFS)
                    ui.horizontal(|ui| {
                        ui.label("Integrated Loudness (LUFS):");
                        if ui
                            .add(
                                egui::Slider::new(
                                    &mut self.equalizer_settings.ebu_r128.integrated_loudness,
                                    -70.0..=-5.0,
                                )
                                .text("LUFS")
                                .step_by(0.5),
                            )
                            .changed()
                        {
                            self.equalizer_settings.ebu_r128.preset = muvrix_player::EbuR128Preset::Custom;
                            norm_changed = true;
                        }
                    });

                    // True Peak (dBTP)
                    ui.horizontal(|ui| {
                        ui.label("True Peak Ceiling (dBTP):");
                        if ui
                            .add(
                                egui::Slider::new(
                                    &mut self.equalizer_settings.ebu_r128.true_peak,
                                    -9.0..=0.0,
                                )
                                .text("dBTP")
                                .step_by(0.1),
                            )
                            .changed()
                        {
                            self.equalizer_settings.ebu_r128.preset = muvrix_player::EbuR128Preset::Custom;
                            norm_changed = true;
                        }
                    });

                    // Loudness Range (LU)
                    ui.horizontal(|ui| {
                        ui.label("Loudness Range (LRA):");
                        if ui
                            .add(
                                egui::Slider::new(
                                    &mut self.equalizer_settings.ebu_r128.loudness_range,
                                    1.0..=50.0,
                                )
                                .text("LU")
                                .step_by(0.5),
                            )
                            .changed()
                        {
                            self.equalizer_settings.ebu_r128.preset = muvrix_player::EbuR128Preset::Custom;
                            norm_changed = true;
                        }
                    });

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("● EBU R128 ACTIVE")
                                .strong()
                                .size(11.0)
                                .color(BrandPalette::ELECTRIC_LIME),
                        );
                        ui.label(
                            RichText::new(self.equalizer_settings.ebu_r128.build_filter_string())
                                .monospace()
                                .size(11.0)
                                .color(Color32::from_gray(180)),
                        );

                        if ui.button("Reset to Broadcast (-23 LUFS)").clicked() {
                            self.equalizer_settings.ebu_r128.reset_to_broadcast_standards();
                            norm_changed = true;
                        }
                        if ui.button("Reset to Streaming (-16 LUFS)").clicked() {
                            self.equalizer_settings.ebu_r128.reset_to_streaming_defaults();
                            norm_changed = true;
                        }
                    });
                }
                muvrix_player::NormalizationMode::Dynamic => {
                    let telemetry = self.controller.dynamics_telemetry(true);

                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("● DYNAMICS ACTIVE")
                                .strong()
                                .size(11.0)
                                .color(BrandPalette::ELECTRIC_LIME),
                        );
                        ui.label(
                            RichText::new(&telemetry.status_message)
                                .size(11.5)
                                .color(Color32::from_gray(180)),
                        );
                    });

                    ui.add_space(6.0);

                    // Calibrated Gain Reduction Meter (0 dB down to -24 dB)
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Gain Reduction:").strong().size(11.5));
                        let gr_db = telemetry.gain_reduction_db.unwrap_or(0.0).clamp(-24.0, 0.0);
                        let meter_width = 240.0;
                        let meter_height = 14.0;
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(meter_width, meter_height),
                            Sense::hover(),
                        );
                        let painter = ui.painter_at(rect);

                        // Meter background track
                        painter.rect_filled(rect, 3.0, Color32::from_rgb(16, 24, 32));
                        painter.rect_stroke(
                            rect,
                            3.0,
                            egui::Stroke::new(1.0_f32, Color32::from_rgb(32, 43, 54)),
                            StrokeKind::Inside,
                        );

                        let reduction_fraction = (-gr_db / 24.0).clamp(0.0, 1.0) as f32;
                        if reduction_fraction > 0.005 {
                            let fill_width = meter_width * reduction_fraction;
                            let fill_rect = egui::Rect::from_min_max(
                                egui::pos2(rect.max.x - fill_width, rect.min.y),
                                egui::pos2(rect.max.x, rect.max.y),
                            );
                            let fill_color = if gr_db < -12.0 {
                                Color32::from_rgb(239, 68, 68) // red
                            } else if gr_db < -6.0 {
                                Color32::from_rgb(245, 158, 11) // amber
                            } else {
                                BrandPalette::ELECTRIC_LIME // lime green
                            };
                            painter.rect_filled(fill_rect, 2.0, fill_color);
                        }

                        // Numeric readout
                        if let Some(val) = telemetry.gain_reduction_db {
                            ui.label(
                                RichText::new(format!("{:.1} dB", val))
                                    .strong()
                                    .size(11.5)
                                    .color(BrandPalette::ELECTRIC_LIME),
                            );
                        } else {
                            ui.label(
                                RichText::new("0.0 dB (Filter Active)")
                                    .size(11.0)
                                    .color(BrandPalette::ELECTRIC_LIME),
                            );
                        }
                    });

                    // Meter scale ticks
                    ui.horizontal(|ui| {
                        ui.add_space(95.0);
                        ui.label(RichText::new("-24dB").size(9.0).color(Color32::from_gray(120)));
                        ui.add_space(34.0);
                        ui.label(RichText::new("-18dB").size(9.0).color(Color32::from_gray(120)));
                        ui.add_space(34.0);
                        ui.label(RichText::new("-12dB").size(9.0).color(Color32::from_gray(120)));
                        ui.add_space(34.0);
                        ui.label(RichText::new("-6dB").size(9.0).color(Color32::from_gray(120)));
                        ui.add_space(34.0);
                        ui.label(RichText::new("0dB").size(9.0).color(Color32::from_gray(120)));
                    });
                }
                muvrix_player::NormalizationMode::None => {
                    ui.label(
                        RichText::new("○ Loudness Normalization Inactive (Bypassed)")
                            .size(11.0)
                            .color(Color32::from_gray(140)),
                    );
                }
            }

            ui.add_space(4.0);
            ui.label(
                RichText::new("Architecture Note: EBU R128 normalization utilizes FFmpeg loudnorm to maintain consistent target LUFS across diverse media sources. Direct per-frame gain telemetry is monitored when exposed by the filter graph.")
                    .italics()
                    .size(10.5)
                    .color(Color32::from_gray(140)),
            );

            if norm_changed {
                self.apply_equalizer_settings();
            }
        });

        ui.add_space(6.0);

        // =====================================================================
        // SECTION 4: Seamless Audio & Gapless Transitions
        // =====================================================================
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("4. Seamless Audio & Gapless Transitions")
                    .strong()
                    .size(14.0)
                    .color(BrandPalette::ELECTRIC_LIME),
            );
            ui.label(
                RichText::new(
                    "Eliminates audible dropouts and micro-gaps between sequential queue tracks with demuxer pre-caching.",
                )
                .size(12.0)
                .color(BrandPalette::MUTED_TEXT),
            );
            ui.add_space(4.0);

            let mut gapless = self.controller.gapless_audio();
            if ui
                .checkbox(&mut gapless, "Enable Gapless Audio Playback")
                .on_hover_text(
                    "Keep audio hardware sinks continuously initialized across track transitions.",
                )
                .changed()
            {
                let mode = if gapless { "yes" } else { "no" };
                let _ = self.controller.set_gapless_audio(mode);
            }

            ui.add_space(2.0);
            ui.label(
                RichText::new("Demuxer queue prefetch: Active (10.0s readahead window)")
                    .italics()
                    .size(11.0)
                    .color(Color32::from_gray(160)),
            );
        });

        ui.add_space(6.0);

        // =====================================================================
        // SECTION 5: Audio Channel Routing & Device Management
        // =====================================================================
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("5. Audio Channel Routing & Output Device")
                    .strong()
                    .size(14.0)
                    .color(BrandPalette::ELECTRIC_LIME),
            );
            ui.label(
                RichText::new(
                    "Configure speaker channel downmix policy, stereo orientation, and physical audio output routing.",
                )
                .size(12.0)
                .color(BrandPalette::MUTED_TEXT),
            );
            ui.add_space(4.0);

            // Channels dropdown
            let mut current_channels = self.audio_channels;
            egui::ComboBox::from_id_salt("audio_channels_dropdown")
                .selected_text(current_channels.label())
                .show_ui(ui, |ui| {
                    for option in [
                        AudioChannelsSetting::Auto,
                        AudioChannelsSetting::AutoSafe,
                        AudioChannelsSetting::Mono,
                        AudioChannelsSetting::Stereo,
                        AudioChannelsSetting::ReverseStereo,
                    ] {
                        let selected = current_channels == option;
                        if ui
                            .selectable_label(
                                selected,
                                format!("{}: {}", option.label(), option.description()),
                            )
                            .clicked()
                        {
                            current_channels = option;
                        }
                    }
                });

            if current_channels != self.audio_channels {
                self.audio_channels = current_channels;
                self.sync_audio_preferences();
                self.save_media_settings();
            }

            ui.add_space(4.0);

            // Pitch correction & Background audio
            ui.horizontal(|ui| {
                if ui
                    .checkbox(
                        &mut self.audio_pitch_correction,
                        "Audio Pitch Correction (scaletempo2)",
                    )
                    .on_hover_text(
                        "Maintain natural vocal pitch across speed changes (0.25x – 4.0x).",
                    )
                    .changed()
                {
                    self.sync_audio_preferences();
                    self.save_media_settings();
                }

                ui.add_space(10.0);
                if ui
                    .checkbox(
                        &mut self.background_audio,
                        "Continue Audio Playback in Background",
                    )
                    .on_hover_text(
                        "Keep playing audio while the desktop window is minimized or unfocused.",
                    )
                    .changed()
                {
                    self.save_media_settings();
                }
            });

            ui.add_space(6.0);

            // Preferred Languages & Audio Delay
            ui.label(
                RichText::new(
                    "Preferred Audio Languages (comma-separated ISO-639 codes, e.g. en, eng, ja, jpn):",
                )
                .size(11.5)
                .color(BrandPalette::MUTED_TEXT),
            );
            ui.horizontal(|ui| {
                let mut changed = false;
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut self.preferred_audio_languages)
                            .hint_text("en, eng, ja, jpn")
                            .desired_width(260.0),
                    )
                    .changed()
                {
                    changed = true;
                }
                if ui.button("Apply Languages").clicked() || changed {
                    self.sync_audio_preferences();
                    self.save_media_settings();
                }
            });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Audio Delay Synchronization:")
                        .size(12.0)
                        .strong(),
                );
                let mut delay = self.default_audio_delay_ms;
                if ui
                    .add(egui::Slider::new(&mut delay, -5000..=5000).text("ms delay offset"))
                    .changed()
                {
                    self.default_audio_delay_ms = delay;
                    self.sync_audio_preferences();
                    self.save_media_settings();
                }
                if ui.button("Reset (0 ms)").clicked() {
                    self.default_audio_delay_ms = 0;
                    self.sync_audio_preferences();
                    self.save_media_settings();
                }
            });

            ui.add_space(6.0);

            // Volume Boost Ceiling
            ui.horizontal(|ui| {
                ui.label(RichText::new("Volume Boost Ceiling:").strong());
                let mut boost = self.equalizer_settings.volume_boost.clamp(100.0, 130.0);
                if ui
                    .add(
                        egui::Slider::new(&mut boost, 100.0..=130.0)
                            .text("% (software gain)"),
                    )
                    .changed()
                {
                    self.equalizer_settings.volume_boost = boost;
                    let _ = self.controller.set_volume_boost(boost);
                    let _ = self.controller.set_volume(self.controller.volume());
                    self.save_media_settings();
                }
                ui.label(format!(
                    "Current ceiling: {:.0}%",
                    self.controller.volume_boost_max()
                ));
            });

            ui.add_space(6.0);

            // Audio Output Device
            ui.label(RichText::new("Audio Output Device:").strong().size(12.5));
            ui.horizontal(|ui| {
                let current_dev_desc = self
                    .audio_devices
                    .iter()
                    .find(|d| d.name == self.selected_audio_device)
                    .map(|d| d.description.as_str())
                    .unwrap_or("Auto-select device");

                egui::ComboBox::from_id_salt("audio_output_device_combo")
                    .selected_text(current_dev_desc)
                    .show_ui(ui, |ui| {
                        for dev in &self.audio_devices {
                            if ui
                                .selectable_label(
                                    self.selected_audio_device == dev.name,
                                    &dev.description,
                                )
                                .clicked()
                            {
                                self.selected_audio_device = dev.name.clone();
                                let _ = self
                                    .controller
                                    .set_audio_device_with_fallback(&dev.name);
                            }
                        }
                    });

                if ui.button("Refresh Devices").clicked() {
                    self.refresh_audio_devices();
                }
            });

            // Display hotplug recovery notification status if active
            let recovery_status = self.controller.audio().recovery_status();
            match recovery_status {
                muvrix_player::AudioRecoveryStatus::DeviceLost {
                    device_name,
                    fallback_device,
                } => {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!(
                            "Notice: Preferred device '{device_name}' disconnected; rerouted to '{fallback_device}'"
                        ))
                        .size(11.5)
                        .color(Color32::from_rgb(251, 146, 60)),
                    );
                }
                muvrix_player::AudioRecoveryStatus::Recovered {
                    active_device,
                    was_device,
                } => {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!(
                            "Restored: Reconnected to '{active_device}' (rerouted from '{was_device}')"
                        ))
                        .size(11.5)
                        .color(Color32::from_rgb(74, 222, 128)),
                    );
                }
                muvrix_player::AudioRecoveryStatus::Degraded { reason } => {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!("Warning: {reason}"))
                            .size(11.5)
                            .color(Color32::from_rgb(239, 68, 68)),
                    );
                }
                _ => {}
            }
        });

        ui.add_space(6.0);

        // =====================================================================
        // SECTION 6: Real-Time 32-Band Audio Spectrum Visualizer
        // =====================================================================
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("6. Real-Time 32-Band Audio Spectrum Visualizer")
                    .strong()
                    .size(14.0)
                    .color(BrandPalette::ELECTRIC_LIME),
            );
            ui.label(
                RichText::new(
                    "Real-time audio-derived frequency visualization responding to music playback at 60 FPS.",
                )
                .size(12.0)
                .color(BrandPalette::MUTED_TEXT),
            );
            ui.add_space(4.0);

            if ui
                .checkbox(
                    &mut self.equalizer_settings.spectrum_visualizer_enabled,
                    "Enable 32-Band Spectrum Visualizer in Player View",
                )
                .on_hover_text(
                    "Displays 32 logarithmic frequency bands with Electric Lime attack/decay animation during audio playback.",
                )
                .changed()
            {
                self.apply_equalizer_settings();
            }
        });
    }
}
