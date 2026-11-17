//! 10-band parametric equalizer, audio DSP filters, headroom clipping management,
//! and dynamic audio output device routing via native libmpv filters.

use crate::autoeq::AutoEqProfile;
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

/// Presets for the Bauer stereophonic-to-binaural (bs2b) headphone crossfeed filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Bs2bPreset {
    #[default]
    Default,
    ChuMoy,
    JanMeier,
}

impl Bs2bPreset {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::ChuMoy => "Chu Moy",
            Self::JanMeier => "Jan Meier",
        }
    }

    pub fn mpv_profile(&self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::ChuMoy => "cmoy",
            Self::JanMeier => "jmeier",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Default => {
                "Standard Bauer curve (cut 700 Hz, feed 4.5 dB) for natural headphone balance."
            }
            Self::ChuMoy => {
                "Chu Moy circuit (cut 700 Hz, feed 6.0 dB) for moderate crossfeed blending."
            }
            Self::JanMeier => {
                "Jan Meier circuit (cut 650 Hz, feed 9.5 dB) for stronger acoustic cross-talk."
            }
        }
    }
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

/// Selection policy for dynamic range and loudness normalization systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NormalizationMode {
    #[default]
    None,
    EbuR128,
    Dynamic,
}

impl NormalizationMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "Disabled",
            Self::EbuR128 => "EBU R128 (Broadcast)",
            Self::Dynamic => "Dynamic (dynaudnorm)",
        }
    }
}

/// Standardized presets for EBU R128 loudness normalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EbuR128Preset {
    /// EBU R128 European Broadcast Target (EBU R128 / EBU Tech 3343 / ITU-R BS.1770-4):
    /// - Programme Loudness: -23.0 LUFS (EBU R128 §1 recommended programme level, ±0.5 LU tolerance)
    /// - Maximum True Peak: -2.0 dBTP (EBU Tech 3343 §4.3 conservative ceiling for lossy data reduction e.g. MPEG-1 Layer II, AC-3; -1.0 dBTP for linear PCM)
    /// - Loudness Range (LRA): 7.0 LU (MUVRIX dynamic processing window; note LRA is an assessment parameter, not a mandatory 7.0 LU requirement)
    Broadcast,
    /// Music Streaming Target (Aligned with AES TD1008 (2021) track-normalized music recommendation):
    /// - Target Loudness: -16.0 LUFS (Music-oriented streaming target; note AES TD1008 recommends -18 LUFS for speech/podcasts, -16 LUFS for track-normalized music, and -14 LUFS for album loudest track)
    /// - Maximum True Peak: -1.5 dBTP (Prevents inter-sample clipping on downstream lossy transcode)
    /// - Loudness Range (LRA): 11.0 LU (Comfortable dynamic processing window for streaming music)
    #[default]
    Streaming,
    /// Custom user-defined target thresholds
    Custom,
}

impl EbuR128Preset {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Broadcast => "EBU R128 Broadcast (-23.0 LUFS, -2.0 dBTP, 7.0 LU)",
            Self::Streaming => "Streaming Music Target (-16.0 LUFS, -1.5 dBTP, 11.0 LU)",
            Self::Custom => "Custom",
        }
    }

    pub fn short_name(&self) -> &'static str {
        match self {
            Self::Broadcast => "Broadcast (-23 LUFS)",
            Self::Streaming => "Streaming Music (-16 LUFS)",
            Self::Custom => "Custom",
        }
    }

    /// Returns the target (I, TP, LRA) values for this preset.
    pub fn target_values(&self) -> Option<(f64, f64, f64)> {
        match self {
            Self::Broadcast => Some((-23.0, -2.0, 7.0)),
            Self::Streaming => Some((-16.0, -1.5, 11.0)),
            Self::Custom => None,
        }
    }
}

/// Configuration for EBU R128 professional audio loudness normalization (`loudnorm`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EbuR128Settings {
    pub enabled: bool,
    #[serde(default)]
    pub preset: EbuR128Preset,
    /// Integrated loudness target in LUFS (-70.0 to -5.0 dB, default -16.0).
    pub integrated_loudness: f64,
    /// Maximum True Peak ceiling in dBTP (-9.0 to 0.0 dB, default -1.5).
    pub true_peak: f64,
    /// Loudness range target in LU (1.0 to 50.0 LU, default 11.0).
    pub loudness_range: f64,
}

impl Default for EbuR128Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            preset: EbuR128Preset::Streaming,
            integrated_loudness: -16.0,
            true_peak: -1.5,
            loudness_range: 11.0,
        }
    }
}

impl EbuR128Settings {
    /// Applies a preset profile to this configuration.
    pub fn apply_preset(&mut self, preset: EbuR128Preset) {
        self.preset = preset;
        if let Some((i, tp, lra)) = preset.target_values() {
            self.integrated_loudness = i;
            self.true_peak = tp;
            self.loudness_range = lra;
        }
    }

    /// Resets targets to the formal EBU R128 European broadcast reference (-23.0 LUFS, -2.0 dBTP, 7.0 LU).
    pub fn reset_to_broadcast_standards(&mut self) {
        self.apply_preset(EbuR128Preset::Broadcast);
    }

    /// Resets targets to the streaming audio/podcast default (-16.0 LUFS, -1.5 dBTP, 11.0 LU).
    pub fn reset_to_streaming_defaults(&mut self) {
        self.apply_preset(EbuR128Preset::Streaming);
    }

    /// Validates all parameters and builds the standard FFmpeg `loudnorm` filter string.
    ///
    /// Formats as: `loudnorm=I={:.1}:TP={:.1}:LRA={:.1}`
    pub fn build_filter_string(&self) -> String {
        let i = if self.integrated_loudness.is_nan() || self.integrated_loudness.is_infinite() {
            -16.0
        } else {
            self.integrated_loudness.clamp(-70.0, -5.0)
        };

        let tp = if self.true_peak.is_nan() || self.true_peak.is_infinite() {
            -1.5
        } else {
            self.true_peak.clamp(-9.0, 0.0)
        };

        let lra = if self.loudness_range.is_nan() || self.loudness_range.is_infinite() {
            11.0
        } else {
            self.loudness_range.clamp(1.0, 50.0)
        };

        format!("loudnorm=I={:.1}:TP={:.1}:LRA={:.1}", i, tp, lra)
    }
}

fn default_spectrum_visualizer_enabled() -> bool {
    false
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

    // Headphone Crossfeed (BS2B)
    #[serde(default)]
    pub crossfeed_enabled: bool,
    #[serde(default)]
    pub crossfeed_preset: Bs2bPreset,

    // EBU R128 Loudness Normalization & Normalization Mode
    #[serde(default)]
    pub ebu_r128: EbuR128Settings,
    #[serde(default)]
    pub normalization_mode: NormalizationMode,

    // AutoEq Parametric Profile
    #[serde(default)]
    pub autoeq_profile: Option<AutoEqProfile>,

    // Real-Time 32-Band Audio Spectrum Visualizer
    #[serde(default = "default_spectrum_visualizer_enabled")]
    pub spectrum_visualizer_enabled: bool,

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
            crossfeed_enabled: false,
            crossfeed_preset: Bs2bPreset::Default,
            ebu_r128: EbuR128Settings::default(),
            normalization_mode: NormalizationMode::None,
            autoeq_profile: None,
            spectrum_visualizer_enabled: false,
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
            self.autoeq_profile = None;
            true
        } else {
            false
        }
    }

    /// Returns the effective normalization mode, resolving legacy `dynaudnorm`
    /// and `ebu_r128.enabled` settings transparently.
    pub fn effective_normalization_mode(&self) -> NormalizationMode {
        if self.normalization_mode != NormalizationMode::None {
            self.normalization_mode
        } else if self.ebu_r128.enabled {
            NormalizationMode::EbuR128
        } else if self.dynaudnorm {
            NormalizationMode::Dynamic
        } else {
            NormalizationMode::None
        }
    }

    /// Sets the normalization mode, synchronizing legacy booleans.
    pub fn set_normalization_mode(&mut self, mode: NormalizationMode) {
        self.normalization_mode = mode;
        match mode {
            NormalizationMode::None => {
                self.dynaudnorm = false;
                self.ebu_r128.enabled = false;
            }
            NormalizationMode::EbuR128 => {
                self.dynaudnorm = false;
                self.ebu_r128.enabled = true;
            }
            NormalizationMode::Dynamic => {
                self.dynaudnorm = true;
                self.ebu_r128.enabled = false;
            }
        }
    }

    /// Applies an imported AutoEq profile, synchronizing preamp, 10-band slider hints,
    /// enabling the equalizer, and setting preset to Custom.
    pub fn apply_autoeq_profile(&mut self, profile: AutoEqProfile) {
        self.preamp_db = profile.preamp_db;
        let mapped_10 = profile.map_to_10_bands();
        self.gains = mapped_10;
        for (i, &gain) in mapped_10.iter().enumerate() {
            self.bands[i].gain_db = gain;
            self.bands[i].enabled = true;
        }
        self.autoeq_profile = Some(profile);
        self.active_preset = EqualizerPreset::Custom;
        self.is_enabled = true;
    }

    /// Clears any active AutoEq profile, reverting to the standard 10-band equalizer.
    pub fn clear_autoeq_profile(&mut self) {
        self.autoeq_profile = None;
        self.preamp_db = 0.0;
    }

    /// Evaluates the estimated combined frequency response of the equalizer filter cascade in dB at frequency `f_hz`.
    pub fn combined_gain_at_freq(&self, f_hz: f64) -> f64 {
        if let Some(ref autoeq) = self.autoeq_profile {
            let mut total = 0.0;
            for filter in &autoeq.filters {
                total += filter.gain_at_freq(f_hz);
            }
            total
        } else {
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
    }

    /// Evaluates the cumulative frequency response in dB at frequency `f_hz` (including preamp gain if EQ is enabled).
    pub fn cumulative_response_at_freq(&self, f_hz: f64) -> f64 {
        if !self.is_enabled {
            return 0.0;
        }
        self.preamp_db + self.combined_gain_at_freq(f_hz)
    }

    /// Generates a smooth frequency response curve across `num_points` logarithmically spaced frequencies from 20 Hz to 20 kHz.
    pub fn frequency_response_curve(&self, num_points: usize) -> Vec<(f64, f64)> {
        let n = num_points.clamp(16, 512);
        let log_min = 20.0f64.log10();
        let log_max = 20000.0f64.log10();
        let mut curve = Vec::with_capacity(n);
        for i in 0..n {
            let t = i as f64 / (n - 1) as f64;
            let log_f = log_min + t * (log_max - log_min);
            let freq = 10.0f64.powf(log_f);
            let gain = self.cumulative_response_at_freq(freq);
            curve.push((freq, gain));
        }
        curve
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
    /// 2. Active equalizer filters (either 10-band peaking biquads or AutoEq parametric cascade: peaking, lowshelf, highshelf)
    /// 3. Headphone crossfeed (Bauer bs2b)
    /// 4. Loudness normalization (EBU R128 `loudnorm` or Dynamic `dynaudnorm`)
    /// 5. Real-time audio analysis telemetry (`astats` & `aspectralstats` for spectrum visualizer)
    ///
    /// When the spectrum visualizer is disabled, visualization-specific analysis filters (`@vis`, `@spec`)
    /// are omitted from the active filter chain, while other user-configured audio DSP remains active.
    pub fn build_filter_string(settings: &EqualizerSettings) -> String {
        let mut filters = Vec::new();

        if settings.is_enabled {
            // Preamp gain filter to manage headroom
            let preamp = settings.preamp_db.clamp(-36.0, 36.0);
            if preamp.abs() > 0.01 {
                filters.push(format!("volume=volume={:.1}dB", preamp));
            }

            if let Some(ref autoeq) = settings.autoeq_profile {
                for filter in &autoeq.filters {
                    if !filter.enabled || filter.gain_db.abs() < 0.01 {
                        continue;
                    }
                    let freq = filter.frequency_hz.clamp(20.0, 20000.0);
                    let q = filter.q_factor.clamp(0.1, 10.0);
                    let gain = filter.gain_db.clamp(-36.0, 36.0);
                    match filter.filter_type {
                        crate::autoeq::AutoEqFilterType::Peaking => {
                            filters.push(format!(
                                "equalizer=f={:.2}:t=q:w={:.3}:g={:.1}",
                                freq, q, gain
                            ));
                        }
                        crate::autoeq::AutoEqFilterType::LowShelf => {
                            filters.push(format!(
                                "lowshelf=f={:.2}:t=q:w={:.3}:g={:.1}",
                                freq, q, gain
                            ));
                        }
                        crate::autoeq::AutoEqFilterType::HighShelf => {
                            filters.push(format!(
                                "highshelf=f={:.2}:t=q:w={:.3}:g={:.1}",
                                freq, q, gain
                            ));
                        }
                    }
                }
            } else {
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
        }

        if settings.crossfeed_enabled {
            filters.push(format!(
                "bs2b=profile={}",
                settings.crossfeed_preset.mpv_profile()
            ));
        }

        match settings.effective_normalization_mode() {
            NormalizationMode::EbuR128 => {
                filters.push(settings.ebu_r128.build_filter_string());
            }
            NormalizationMode::Dynamic => {
                filters.push("dynaudnorm=f=150:g=15".to_string());
            }
            NormalizationMode::None => {}
        }

        if settings.spectrum_visualizer_enabled {
            filters.push("@vis:lavfi=[astats=metadata=1:reset=1]".to_string());
            filters.push("@spec:lavfi=[aspectralstats]".to_string());
        }

        filters.join(",")
    }

    /// Builds a unified mpv `af` audio filter graph string including channel routing (pan),
    /// preamp, equalizer biquads, Bauer crossfeed, and dynamic normalization.
    pub fn build_unified_filter_string(settings: &EqualizerSettings, needs_pan: bool) -> String {
        let mut filters = Vec::new();
        if needs_pan {
            filters.push("pan=[stereo|c0=c1|c1=c0]".to_string());
        }
        let base = Self::build_filter_string(settings);
        if !base.is_empty() {
            filters.push(base);
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

    /// Dispatches the unified equalizer filter graph string to an active mpv instance,
    /// preserving channel pan orientation if requested.
    pub fn apply_unified_to_handle(
        handle: &MpvHandle,
        settings: &EqualizerSettings,
        needs_pan: bool,
    ) -> Result<(), MpvError> {
        let filter_str = Self::build_unified_filter_string(settings, needs_pan);
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
