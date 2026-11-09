//! 10-band parametric equalizer, audio DSP filters, headroom clipping management,
//! and dynamic audio output device routing via native libmpv filters.

use crate::error::MpvError;
use crate::handle::MpvHandle;
use serde::{Deserialize, Serialize};

/// Standard 10-band octave center frequencies (in Hz).
pub const EQ_FREQUENCIES: [f64; 10] = [
    31.25, 62.5, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];

/// Presets for the 10-band equalizer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum EqualizerPreset {
    #[default]
    Flat,
    BassBoost,
    BassReduction,
    TrebleBoost,
    TrebleReduction,
    VocalClarity,
    Rock,
    Classical,
    Electronic,
    Cinema,
    Custom,
}

impl EqualizerPreset {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Flat => "Flat",
            Self::BassBoost => "Bass Boost",
            Self::BassReduction => "Bass Reduction",
            Self::TrebleBoost => "Treble Boost",
            Self::TrebleReduction => "Treble Reduction",
            Self::VocalClarity => "Vocal Clarity",
            Self::Rock => "Rock",
            Self::Classical => "Classical",
            Self::Electronic => "Electronic",
            Self::Cinema => "Cinema",
            Self::Custom => "Custom",
        }
    }

    /// Returns the standard dB gains for each frequency band in this preset.
    pub fn default_gains(&self) -> [f64; 10] {
        match self {
            Self::Flat => [0.0; 10],
            Self::BassBoost => [5.0, 4.0, 2.5, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            Self::BassReduction => [-5.0, -4.0, -2.5, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            Self::TrebleBoost => [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.5, 4.5, 5.0],
            Self::TrebleReduction => [0.0, 0.0, 0.0, 0.0, 0.0, -1.0, -2.0, -3.5, -4.5, -5.0],
            Self::VocalClarity => [-1.5, -1.0, 0.0, 1.0, 2.0, 3.5, 3.0, 2.0, 0.5, 0.0],
            Self::Rock => [4.0, 3.0, 1.5, 0.0, -1.0, 0.5, 2.0, 3.0, 3.5, 4.0],
            Self::Classical => [3.0, 2.0, 1.0, 0.5, 0.0, 0.5, 1.5, 2.0, 2.5, 3.0],
            Self::Electronic => [5.0, 4.5, 2.0, 0.5, 0.0, 1.0, 2.0, 3.0, 4.0, 4.5],
            Self::Cinema => [3.5, 2.5, 1.0, 0.0, -1.0, 0.5, 1.5, 2.5, 3.0, 3.5],
            Self::Custom => [0.0; 10],
        }
    }
}

/// A single parametric equalizer band.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EqBand {
    pub enabled: bool,
    pub frequency_hz: f64,
    pub gain_db: f64,  // -12.0..12.0 dB
    pub q_factor: f64, // 0.1..10.0, default 1.414
}

impl EqBand {
    pub fn new(frequency_hz: f64, gain_db: f64, q_factor: f64) -> Self {
        Self {
            enabled: true,
            frequency_hz,
            gain_db: gain_db.clamp(-12.0, 12.0),
            q_factor: q_factor.clamp(0.1, 10.0),
        }
    }
}

/// Custom user equalizer preset with custom gains and preamp setting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomEqPreset {
    pub name: String,
    pub gains: [f64; 10],
    pub preamp_db: f64,
}

/// State configuration for equalizer and audio DSP.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EqualizerSettings {
    pub is_enabled: bool,
    pub preamp_db: f64, // -12.0..12.0 dB
    pub gains: [f64; 10],
    pub bands: [EqBand; 10],
    pub dynaudnorm: bool,
    pub active_preset: EqualizerPreset,

    // Audio DSP and advanced options
    pub pitch_correction: bool,
    pub audio_delay: f64,       // -10.0..10.0 seconds
    pub channel_layout: String, // "auto", "stereo", "5.1", "7.1"
    pub volume_boost: f64,      // 100.0..150.0%

    // User-defined presets
    pub custom_presets: Vec<CustomEqPreset>,
}

impl Default for EqualizerSettings {
    fn default() -> Self {
        let mut bands = [EqBand::new(0.0, 0.0, 1.414); 10];
        for (i, &freq) in EQ_FREQUENCIES.iter().enumerate() {
            bands[i] = EqBand::new(freq, 0.0, 1.414);
        }

        Self {
            is_enabled: false,
            preamp_db: 0.0,
            gains: [0.0; 10],
            bands,
            dynaudnorm: false,
            active_preset: EqualizerPreset::Flat,
            pitch_correction: true,
            audio_delay: 0.0,
            channel_layout: "auto".to_string(),
            volume_boost: 100.0,
            custom_presets: Vec::new(),
        }
    }
}

impl EqualizerSettings {
    /// Synchronizes gains array into bands.
    pub fn sync_gains_to_bands(&mut self) {
        for (i, &gain) in self.gains.iter().enumerate() {
            self.bands[i].gain_db = gain;
        }
    }

    /// Synchronizes bands into gains array.
    pub fn sync_bands_to_gains(&mut self) {
        for (i, band) in self.bands.iter().enumerate() {
            self.gains[i] = band.gain_db;
        }
    }

    /// Sets band gain and marks preset as Custom.
    pub fn set_band_gain(&mut self, index: usize, gain_db: f64) {
        if index < 10 {
            let clamped = gain_db.clamp(-12.0, 12.0);
            self.gains[index] = clamped;
            self.bands[index].gain_db = clamped;
            self.active_preset = EqualizerPreset::Custom;
        }
    }

    /// Sets band Q factor and marks preset as Custom.
    pub fn set_band_q(&mut self, index: usize, q_factor: f64) {
        if index < 10 {
            self.bands[index].q_factor = q_factor.clamp(0.1, 10.0);
            self.active_preset = EqualizerPreset::Custom;
        }
    }

    /// Sets band enabled toggle.
    pub fn set_band_enabled(&mut self, index: usize, enabled: bool) {
        if index < 10 {
            self.bands[index].enabled = enabled;
            self.active_preset = EqualizerPreset::Custom;
        }
    }

    /// Applies a preset configuration.
    pub fn apply_preset(&mut self, preset: EqualizerPreset) {
        self.active_preset = preset;
        let default_gains = preset.default_gains();
        self.gains = default_gains;
        for (i, &gain) in default_gains.iter().enumerate() {
            self.bands[i].gain_db = gain;
            self.bands[i].enabled = true;
            self.bands[i].q_factor = 1.414;
        }
        if preset == EqualizerPreset::Flat {
            self.preamp_db = 0.0;
        }
    }

    /// Saves current equalizer gains and preamp setting as a named custom preset.
    pub fn save_custom_preset(&mut self, name: &str) {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return;
        }
        if let Some(existing) = self.custom_presets.iter_mut().find(|p| p.name == trimmed) {
            existing.gains = self.gains;
            existing.preamp_db = self.preamp_db;
        } else {
            self.custom_presets.push(CustomEqPreset {
                name: trimmed.to_string(),
                gains: self.gains,
                preamp_db: self.preamp_db,
            });
        }
    }

    /// Renames an existing custom preset. Returns true if renamed successfully.
    pub fn rename_custom_preset(&mut self, old_name: &str, new_name: &str) -> bool {
        let trimmed_new = new_name.trim();
        if trimmed_new.is_empty() {
            return false;
        }
        if let Some(pos) = self.custom_presets.iter().position(|p| p.name == old_name) {
            self.custom_presets[pos].name = trimmed_new.to_string();
            true
        } else {
            false
        }
    }

    /// Deletes a custom preset by name. Returns true if found and removed.
    pub fn delete_custom_preset(&mut self, name: &str) -> bool {
        if let Some(pos) = self.custom_presets.iter().position(|p| p.name == name) {
            self.custom_presets.remove(pos);
            true
        } else {
            false
        }
    }

    /// Restores gains and preamp from a saved custom preset. Returns true if restored.
    pub fn restore_custom_preset(&mut self, name: &str) -> bool {
        if let Some(preset) = self.custom_presets.iter().find(|p| p.name == name).cloned() {
            self.gains = preset.gains;
            self.preamp_db = preset.preamp_db;
            for (i, &gain) in preset.gains.iter().enumerate() {
                self.bands[i].gain_db = gain;
                self.bands[i].enabled = true;
            }
            self.active_preset = EqualizerPreset::Custom;
            true
        } else {
            false
        }
    }

    /// Evaluates the estimated combined frequency response of the equalizer filter cascade in dB at frequency `f_hz`.
    pub fn combined_gain_at_freq(&self, f_hz: f64) -> f64 {
        let mut total = 0.0;
        for (i, band) in self.bands.iter().enumerate() {
            let gain = if band.gain_db.abs() > 0.001 {
                band.gain_db
            } else if i < self.gains.len() {
                self.gains[i]
            } else {
                0.0
            };
            if !band.enabled || gain.abs() < 0.01 {
                continue;
            }
            let f0 = band.frequency_hz;
            let ratio = f_hz / f0;
            if ratio <= 0.0 {
                continue;
            }
            let omega = ratio - (1.0 / ratio);
            let q = band.q_factor.max(0.1);
            let weight = 1.0 / (1.0 + (q * omega).powi(2));
            total += gain * weight;
        }
        total
    }

    /// Evaluates the peak combined gain (dB) across logarithmically spaced frequencies from 20 Hz to 20,000 Hz,
    /// accounting for cascading and overlapping adjacent peaking biquad filters.
    pub fn peak_combined_boost(&self) -> f64 {
        let mut peak = 0.0f64;
        // Sample 100 logarithmically spaced frequencies from 20 Hz to 20 kHz
        for step in 0..=100 {
            let log_f = 1.30103 + (step as f64 / 100.0) * (4.30103 - 1.30103);
            let freq = 10.0f64.powf(log_f);
            let gain = self.combined_gain_at_freq(freq);
            if gain > peak {
                peak = gain;
            }
        }
        // Also explicitly evaluate exact center frequencies of all enabled bands
        for (i, band) in self.bands.iter().enumerate() {
            let gain = if band.gain_db.abs() > 0.001 {
                band.gain_db
            } else if i < self.gains.len() {
                self.gains[i]
            } else {
                0.0
            };
            if band.enabled && gain > 0.0 {
                let g = self.combined_gain_at_freq(band.frequency_hz);
                if g > peak {
                    peak = g;
                }
            }
        }
        peak
    }

    /// Calculates safe digital headroom attenuation (preamp dB) to prevent clipping distortion
    /// when boosting frequency bands, accounting for cascading filter overlap, input peak headroom,
    /// and a 0.2 dB numerical safety margin.
    ///
    /// NOTE: This is a digital headroom risk reduction mechanism operating strictly within the
    /// software DSP pipeline (preventing integer/fixed-point overflow and harsh distortion before DAC output);
    /// it does not provide hardware limiter or acoustic hearing protection.
    pub fn calculate_safe_preamp(&self) -> f64 {
        let peak_boost = self.peak_combined_boost();
        if peak_boost > 0.01 {
            -((peak_boost + 0.2) * 10.0).round() / 10.0
        } else {
            0.0
        }
    }

    /// Checks if active EQ boosts and preamp exceed digital headroom (0 dBFS), risking clipping distortion.
    pub fn has_clipping_risk(&self) -> bool {
        let peak_boost = self.peak_combined_boost();
        (self.preamp_db + peak_boost) > 0.1
    }
}

/// Information describing a detected audio output device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioDeviceInfo {
    pub name: String,
    pub description: String,
}

impl AudioDeviceInfo {
    /// Returns true if this device represents a physical hardware sink (e.g. PCI, USB, HDMI, Headphones)
    /// rather than a virtual dispatcher, auto-selector, or null test sink.
    ///
    /// NOTE: Classification is based on naming heuristics (matching PCI/USB/HDMI/Headphone/Speaker
    /// keywords while excluding known daemon/virtual dispatchers like auto, pipewire, pulse, alsa, loopback).
    /// In complex PipeWire/ALSA configurations or virtual audio routing (e.g., JACK loopbacks, OBS virtual sinks,
    /// null sinks), some virtual sinks may mimic physical names or vice versa. The application exposes
    /// all discovered devices to the user while using `is_physical()` only as a sorting/grouping recommendation in the UI.
    pub fn is_physical(&self) -> bool {
        let name_lower = self.name.to_lowercase();
        let desc_lower = self.description.to_lowercase();
        if self.name == "auto"
            || self.name == "pipewire"
            || self.name == "pulse"
            || self.name == "alsa"
            || self.name == "jack"
            || self.name == "openal"
            || self.name == "sdl"
        {
            return false;
        }
        if name_lower.contains("null") || desc_lower.contains("null") {
            return false;
        }
        name_lower.contains("pci")
            || name_lower.contains("usb")
            || desc_lower.contains("hdmi")
            || desc_lower.contains("headphone")
            || desc_lower.contains("speaker")
            || desc_lower.contains("controller")
            || desc_lower.contains("audio device")
    }
}

pub type AudioOutputDevice = AudioDeviceInfo;

pub struct EqualizerManager;

impl EqualizerManager {
    /// Builds an mpv `af` audio filter graph string representing the equalizer and DSP settings.
    ///
    /// Constructs a single-pass filter chain with:
    /// 1. Preamp gain (if non-zero)
    /// 2. Active 10-band peaking biquad filters (`equalizer=f=...:t=q:w=...:g=...`)
    /// 3. Dynamic audio normalizer (`dynaudnorm`)
    pub fn build_filter_string(settings: &EqualizerSettings) -> String {
        let mut filters = Vec::new();

        if settings.is_enabled {
            // Preamp gain filter to manage headroom
            let preamp = settings.preamp_db.clamp(-12.0, 12.0);
            if preamp.abs() > 0.01 {
                filters.push(format!("volume=volume={:.1}dB", preamp));
            }

            // Chain 10 peaking biquad equalizer filters
            for (i, band) in settings.bands.iter().enumerate() {
                if !band.enabled {
                    continue;
                }
                // Use settings.gains[i] or band.gain_db
                let gain = settings.gains[i].clamp(-12.0, 12.0);
                if gain.abs() > 0.01 {
                    let freq = band.frequency_hz;
                    let q = band.q_factor.clamp(0.1, 10.0);
                    filters.push(format!(
                        "equalizer=f={:.2}:t=q:w={:.3}:g={:.1}",
                        freq, q, gain
                    ));
                }
            }
        }

        if settings.dynaudnorm {
            filters.push("dynaudnorm=f=150:g=15".to_string());
        }

        filters.join(",")
    }

    /// Dispatches the equalizer filter graph string to an active mpv instance.
    pub fn apply_to_handle(
        handle: &MpvHandle,
        settings: &EqualizerSettings,
    ) -> Result<(), MpvError> {
        let filter_str = Self::build_filter_string(settings);
        handle.set_property_string("af", &filter_str)?;
        Self::apply_dsp_options(handle, settings)?;
        Ok(())
    }

    /// Applies non-filter DSP options (pitch correction, delay, channels).
    pub fn apply_dsp_options(
        handle: &MpvHandle,
        settings: &EqualizerSettings,
    ) -> Result<(), MpvError> {
        // Pitch correction
        let _ = handle.set_property_string(
            "audio-pitch-correction",
            if settings.pitch_correction {
                "yes"
            } else {
                "no"
            },
        );

        // Audio delay (-10.0..10.0 seconds)
        let _ = handle.set_property("audio-delay", settings.audio_delay.clamp(-10.0, 10.0));

        // Channel layout
        let layout = if settings.channel_layout.is_empty() {
            "auto"
        } else {
            &settings.channel_layout
        };
        let _ = handle.set_property_string("audio-channels", layout);

        Ok(())
    }

    /// Resets all equalizer settings to flat and clears audio filters.
    pub fn reset(handle: &MpvHandle) -> Result<(), MpvError> {
        handle.set_property_string("af", "")?;
        let _ = handle.set_property("audio-delay", 0.0);
        let _ = handle.set_property_string("audio-pitch-correction", "yes");
        let _ = handle.set_property_string("audio-channels", "auto");
        Ok(())
    }

    /// Enumerates detected audio output devices via `audio-device-list`.
    pub fn query_audio_devices(handle: &MpvHandle) -> Result<Vec<AudioDeviceInfo>, MpvError> {
        let raw = handle
            .get_property_string("audio-device-list")
            .unwrap_or_default();
        let mut devices = Vec::new();

        // Always include auto default
        devices.push(AudioDeviceInfo {
            name: "auto".to_string(),
            description: "Auto-select device".to_string(),
        });

        // Parse JSON array if returned
        if let Ok(parsed) = serde_json::from_str::<Vec<serde_json::Value>>(&raw) {
            for item in parsed {
                if let (Some(name), Some(desc)) = (
                    item.get("name").and_then(|v| v.as_str()),
                    item.get("description").and_then(|v| v.as_str()),
                ) {
                    if name != "auto" {
                        devices.push(AudioDeviceInfo {
                            name: name.to_string(),
                            description: desc.to_string(),
                        });
                    }
                }
            }
        }

        Ok(devices)
    }

    /// Selects an audio output device on the active engine.
    pub fn set_audio_device(handle: &MpvHandle, device_name: &str) -> Result<(), MpvError> {
        handle.set_property_string("audio-device", device_name)
    }

    /// Selects an audio output device with automatic fallback to "auto" if not available.
    pub fn set_audio_device_with_fallback(
        handle: &MpvHandle,
        preferred_device: &str,
    ) -> Result<String, MpvError> {
        if preferred_device.is_empty() || preferred_device == "auto" {
            Self::set_audio_device(handle, "auto")?;
            return Ok("auto".to_string());
        }

        let devices = Self::query_audio_devices(handle).unwrap_or_default();
        if devices.iter().any(|d| d.name == preferred_device) {
            Self::set_audio_device(handle, preferred_device)?;
            Ok(preferred_device.to_string())
        } else {
            log::warn!(
                "Preferred audio device '{}' not found; falling back to 'auto'",
                preferred_device
            );
            Self::set_audio_device(handle, "auto")?;
            Ok("auto".to_string())
        }
    }
}
