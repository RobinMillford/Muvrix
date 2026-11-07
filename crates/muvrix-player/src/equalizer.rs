//! 10-band graphic equalizer, dynamic normalizer, and audio device enumeration via native mpv filters.

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
            Self::Custom => [0.0; 10],
        }
    }
}

/// State configuration for equalizer and audio DSP.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EqualizerSettings {
    pub is_enabled: bool,
    pub preamp_db: f64, // -12.0..12.0
    pub gains: [f64; 10],
    pub dynaudnorm: bool,
    pub active_preset: EqualizerPreset,
}

impl Default for EqualizerSettings {
    fn default() -> Self {
        Self {
            is_enabled: false,
            preamp_db: 0.0,
            gains: [0.0; 10],
            dynaudnorm: false,
            active_preset: EqualizerPreset::Flat,
        }
    }
}

/// Information describing a detected hardware audio output device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioDeviceInfo {
    pub name: String,
    pub description: String,
}

pub struct EqualizerManager;

impl EqualizerManager {
    /// Builds an mpv `af` audio filter graph string representing the equalizer and DSP settings.
    pub fn build_filter_string(settings: &EqualizerSettings) -> String {
        let mut filters = Vec::new();

        if settings.is_enabled {
            // Preamp gain filter if non-zero
            let preamp = settings.preamp_db.clamp(-12.0, 12.0);
            if preamp.abs() > 0.01 {
                filters.push(format!("volume=volume={:.1}dB", preamp));
            }

            // Chain 10 peaking biquad equalizer filters
            for (i, &freq) in EQ_FREQUENCIES.iter().enumerate() {
                let gain = settings.gains[i].clamp(-12.0, 12.0);
                if gain.abs() > 0.01 {
                    filters.push(format!("equalizer=f={:.2}:t=q:w=1.414:g={:.1}", freq, gain));
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
        handle.set_property_string("af", &filter_str)
    }

    /// Resets all equalizer settings to flat and clears audio filters.
    pub fn reset(handle: &MpvHandle) -> Result<(), MpvError> {
        handle.set_property_string("af", "")
    }

    /// Enumerates detected audio output devices via `audio-device-list`.
    pub fn query_audio_devices(handle: &MpvHandle) -> Result<Vec<AudioDeviceInfo>, MpvError> {
        // Query audio-device-list property string or JSON representation
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
}
