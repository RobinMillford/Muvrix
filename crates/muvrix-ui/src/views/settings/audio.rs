use eframe::egui::{self, Color32, RichText};
use muvrix_player::equalizer::{EqualizerManager, EqualizerPreset, EQ_FREQUENCIES};
use muvrix_player::AudioChannelsSetting;

use crate::app::MuvrixApp;
use crate::branding::BrandPalette;

impl MuvrixApp {
    pub(crate) fn render_settings_audio(&mut self, ui: &mut egui::Ui) {
        // Section 1: Audio Channel Routing & Output Balance
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("1. Audio Channel Routing")
                    .strong()
                    .size(14.0)
                    .color(BrandPalette::ELECTRIC_LIME),
            );
            ui.label(
                RichText::new(
                    "Configure speaker channel downmix policy and stereo channel orientation.",
                )
                .size(12.0)
                .color(BrandPalette::MUTED_TEXT),
            );
            ui.add_space(4.0);

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

            ui.add_space(2.0);
            ui.label(
                RichText::new(format!(
                    "Active policy: {}",
                    self.audio_channels.description()
                ))
                .italics()
                .size(11.0)
                .color(Color32::from_gray(160)),
            );
        });

        ui.add_space(6.0);

        // Section 2: Audio Processing & Normalization
        ui.group(|ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new("2. Audio Processing & Normalization")
                                .strong()
                                .size(14.0)
                                .color(BrandPalette::ELECTRIC_LIME),
                        );
                        ui.add_space(4.0);

                        if ui
                            .checkbox(
                                &mut self.volume_normalization,
                                "Volume Normalization (dynaudnorm)",
                            )
                            .on_hover_text(
                                "Smooth out extreme volume fluctuations between loud action and quiet dialogue.",
                            )
                            .changed()
                        {
                            self.sync_audio_preferences();
                            self.save_media_settings();
                        }

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

        // Section 3: Preferred Languages & Audio Delay
        ui.group(|ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new("3. Language Selection & Track Sync")
                                .strong()
                                .size(14.0)
                                .color(BrandPalette::ELECTRIC_LIME),
                        );
                        ui.label(
                            RichText::new(
                                "Preferred Audio Languages (comma-separated ISO-639 codes, e.g. en, eng, ja, jpn):",
                            )
                            .size(11.5)
                            .color(BrandPalette::MUTED_TEXT),
                        );
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            let mut changed = false;
                            if ui
                                .add(
                                    egui::TextEdit::singleline(
                                        &mut self.preferred_audio_languages,
                                    )
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

                        ui.add_space(8.0);
                        ui.label(
                            RichText::new("Audio Delay Synchronization:")
                                .size(12.0)
                                .strong(),
                        );
                        ui.horizontal(|ui| {
                            let mut delay = self.default_audio_delay_ms;
                            if ui
                                .add(
                                    egui::Slider::new(&mut delay, -5000..=5000)
                                        .text("ms delay offset"),
                                )
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
                    });

        ui.add_space(6.0);

        // Section 4: Audio Equalizer & Output Device Routing
        ui.group(|ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new("4. 10-Band Parametric Equalizer & Audio DSP Routing")
                                .strong()
                                .size(14.0)
                                .color(Color32::from_rgb(95, 185, 255)),
                        );
                        ui.label(
                            RichText::new(
                                "Configure 10-band peaking biquad equalizer filters with Q-factor shaping, dynamic audio normalization, delay synchronization, and physical output routing.",
                            )
                            .size(12.0)
                            .color(Color32::from_gray(160)),
                        );

                        ui.add_space(6.0);

                        ui.group(|ui| {
                            ui.label(RichText::new("Volume Boost Ceiling").strong());
                            ui.horizontal(|ui| {
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
                        });

                        ui.add_space(6.0);

                        // Capability Diagnostics Badges
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("DSP Engine:").size(11.0).color(Color32::from_gray(140)));
                            ui.label(
                                RichText::new("FFmpeg Biquad / libmpv af")
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

                        ui.add_space(8.0);

                        let mut eq_changed = false;

                        ui.horizontal(|ui| {
                            if ui.checkbox(&mut self.equalizer_settings.is_enabled, "Enable 10-Band Equalizer").changed() {
                                eq_changed = true;
                            }
                            if ui.checkbox(&mut self.equalizer_settings.dynaudnorm, "Dynamic Audio Normalizer (dynaudnorm)").changed() {
                                eq_changed = true;
                            }
                            if ui.checkbox(&mut self.equalizer_settings.pitch_correction, "Preserve Pitch (scaletempo2)").changed() {
                                eq_changed = true;
                            }
                        });

                        ui.add_space(4.0);

                        // Presets
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
                                        if ui.selectable_label(self.equalizer_settings.active_preset == p, p.as_str()).clicked() {
                                            self.equalizer_settings.active_preset = p;
                                            self.equalizer_settings.gains = p.default_gains();
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
                                    RichText::new("⚠️ Positive EQ gains may cause digital clipping with current preamp.")
                                        .color(Color32::from_rgb(251, 191, 36))
                                        .size(11.5),
                                );
                                let safe_preamp = self.equalizer_settings.calculate_safe_preamp();
                                if ui.button(format!("Auto-Attenuate Preamp ({:.1} dB)", safe_preamp)).clicked() {
                                    self.equalizer_settings.preamp_db = safe_preamp;
                                    eq_changed = true;
                                }
                            });
                        }

                        // Preamp Slider
                        ui.horizontal(|ui| {
                            ui.label("Preamp Gain: ");
                            if ui.add(egui::Slider::new(&mut self.equalizer_settings.preamp_db, -12.0..=12.0).text("dB")).changed() {
                                if self.equalizer_settings.active_preset != EqualizerPreset::Custom {
                                    self.equalizer_settings.active_preset = EqualizerPreset::Custom;
                                }
                                eq_changed = true;
                            }
                            ui.add_space(10.0);
                            ui.checkbox(&mut self.eq_show_q_factors, "Show Parametric Q Controls");
                        });

                        ui.add_space(6.0);

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
                                    let slider = egui::Slider::new(&mut self.equalizer_settings.gains[i], -12.0..=12.0)
                                        .vertical()
                                        .show_value(false);
                                    if ui.add_sized([24.0, 90.0], slider).changed() {
                                        self.equalizer_settings.active_preset = EqualizerPreset::Custom;
                                        eq_changed = true;
                                    }
                                    ui.label(RichText::new(format!("{:.1}", self.equalizer_settings.gains[i])).size(9.5));
                                });
                            }
                        });

                        // Parametric Q Controls (when toggled)
                        if self.eq_show_q_factors {
                            ui.add_space(4.0);
                            ui.label(RichText::new("Band Q-Factors (Bandwidth: Higher = Sharper peak):").size(11.0).color(Color32::from_gray(160)));
                            ui.horizontal(|ui| {
                                for (i, &freq) in EQ_FREQUENCIES.iter().enumerate() {
                                    ui.vertical(|ui| {
                                        let label = if freq >= 1000.0 {
                                            format!("{:.0}k", freq / 1000.0)
                                        } else {
                                            format!("{:.0}", freq)
                                        };
                                        ui.label(RichText::new(&label).size(9.5));
                                        let mut q = self.equalizer_settings.bands.get(i).map(|b| b.q_factor).unwrap_or(1.414);
                                        if ui.add(egui::DragValue::new(&mut q).range(0.1..=10.0).speed(0.05)).changed() {
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
                        ui.separator();
                        ui.label(RichText::new("Audio DSP & Synchronization:").strong().size(12.5));

                        ui.horizontal(|ui| {
                            ui.label("Audio Delay: ");
                            if ui.add(egui::Slider::new(&mut self.equalizer_settings.audio_delay, -2.0..=2.0).step_by(0.05).text("seconds")).changed() {
                                eq_changed = true;
                            }
                            ui.add_space(10.0);
                            ui.label("Channel Layout: ");
                            let cur_layout = self.equalizer_settings.channel_layout.clone();
                            egui::ComboBox::from_id_salt("eq_channel_layout_combo")
                                .selected_text(&cur_layout)
                                .show_ui(ui, |ui| {
                                    for layout in ["auto", "stereo", "5.1", "7.1"] {
                                        if ui.selectable_label(cur_layout == layout, layout).clicked() {
                                            self.equalizer_settings.channel_layout = layout.to_string();
                                            eq_changed = true;
                                        }
                                    }
                                });
                        });

                        if eq_changed {
                            self.apply_equalizer_settings();
                        }

                        ui.add_space(6.0);
                        if ui.button("Reset Equalizer to Flat (0 dB)").clicked() {
                            self.equalizer_settings.gains = [0.0; 10];
                            self.equalizer_settings.preamp_db = 0.0;
                            self.equalizer_settings.active_preset = EqualizerPreset::Flat;
                            self.apply_equalizer_settings();
                        }

                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(4.0);

                        // Audio Output Device
                        ui.label(RichText::new("Audio Output Device:").strong().size(12.5));
                        ui.horizontal(|ui| {
                            let current_dev_desc = self.audio_devices
                                .iter()
                                .find(|d| d.name == self.selected_audio_device)
                                .map(|d| d.description.as_str())
                                .unwrap_or("Auto-select device");

                            egui::ComboBox::from_id_salt("audio_output_device_combo")
                                .selected_text(current_dev_desc)
                                .show_ui(ui, |ui| {
                                    for dev in &self.audio_devices {
                                        if ui.selectable_label(self.selected_audio_device == dev.name, &dev.description).clicked() {
                                            self.selected_audio_device = dev.name.clone();
                                            let _ = EqualizerManager::set_audio_device_with_fallback(self.controller.handle(), &dev.name);
                                        }
                                    }
                                });

                            if ui.button("Refresh Devices").clicked() {
                                self.refresh_audio_devices();
                            }
                        });
                    });
    }
}
