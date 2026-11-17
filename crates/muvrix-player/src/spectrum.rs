//! Real-time 32-band audio spectrum analyzer, logarithmic bucket mapping,
//! telemetry processing, and attack/release visualizer smoothing engine.

use serde::{Deserialize, Serialize};

/// Exact number of frequency visualizer bands.
pub const SPECTRUM_BANDS_COUNT: usize = 32;

/// Standard 32 1/3-octave center frequencies (in Hz) spanning the audible range (20 Hz to 20,000 Hz).
pub const SPECTRUM_FREQUENCIES: [f64; SPECTRUM_BANDS_COUNT] = [
    20.0, 25.0, 31.5, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0, 160.0, 200.0, 250.0, 315.0, 400.0,
    500.0, 630.0, 800.0, 1000.0, 1250.0, 1600.0, 2000.0, 2500.0, 3150.0, 4000.0, 5000.0, 6300.0,
    8000.0, 10000.0, 12500.0, 16000.0, 18000.0, 20000.0,
];

/// Real-time audio stream telemetry queried from active libmpv audio filters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpectrumTelemetry {
    /// Instantaneous root-mean-square level in dBFS (e.g. -21.0 dB).
    pub rms_db: f64,
    /// Instantaneous peak level in dBFS (e.g. -18.0 dB).
    pub peak_db: f64,
    /// Center of mass of the frequency spectrum in Hz (e.g. 1000 Hz).
    pub spectral_centroid: f64,
    /// Spectral bandwidth / standard deviation around the centroid in Hz.
    pub spectral_spread: f64,
    /// High-frequency roll-off threshold frequency in Hz.
    pub spectral_rolloff: f64,
    /// Spectral flatness (0.0 = pure tone/narrow band, 1.0 = white noise).
    pub spectral_flatness: f64,
    /// Spectral slope / tilt from low to high frequencies.
    pub spectral_slope: f64,
    /// Whether media is actively playing.
    pub is_playing: bool,
    /// Whether playback is currently paused.
    pub is_paused: bool,
}

impl Default for SpectrumTelemetry {
    fn default() -> Self {
        Self {
            rms_db: -100.0,
            peak_db: -100.0,
            spectral_centroid: 1000.0,
            spectral_spread: 500.0,
            spectral_rolloff: 8000.0,
            spectral_flatness: 0.1,
            spectral_slope: 0.0,
            is_playing: false,
            is_paused: false,
        }
    }
}

/// 32-band audio spectrum visualizer state machine and animator.
#[derive(Debug, Clone, PartialEq)]
pub struct SpectrumVisualizer {
    /// 32 smoothed bar heights in range `[0.0, 1.0]`.
    smoothed_magnitudes: [f32; SPECTRUM_BANDS_COUNT],
    /// Fast attack smoothing factor (0.0 to 1.0, higher = faster rise).
    attack_factor: f32,
    /// Gentle release smoothing factor (0.0 to 1.0, lower = slower decay).
    release_factor: f32,
}

impl Default for SpectrumVisualizer {
    fn default() -> Self {
        Self::new()
    }
}

impl SpectrumVisualizer {
    /// Creates a new visualizer initialized to complete silence.
    pub fn new() -> Self {
        Self {
            smoothed_magnitudes: [0.0; SPECTRUM_BANDS_COUNT],
            attack_factor: 0.70,
            release_factor: 0.18,
        }
    }

    /// Resets all visualizer bands and accumulators immediately to zero.
    pub fn reset(&mut self) {
        self.smoothed_magnitudes = [0.0; SPECTRUM_BANDS_COUNT];
    }

    /// Returns the standard 32 center frequencies.
    pub fn frequencies() -> &'static [f64; SPECTRUM_BANDS_COUNT] {
        &SPECTRUM_FREQUENCIES
    }

    /// Returns the current smoothed magnitudes for rendering.
    pub fn smoothed_bands(&self) -> &[f32; SPECTRUM_BANDS_COUNT] {
        &self.smoothed_magnitudes
    }

    /// Maps any frequency in Hz (20..20000) to its nearest 1/3-octave bucket index (0..31).
    pub fn bucket_index_for_freq(freq_hz: f64) -> usize {
        if freq_hz.is_nan() || freq_hz <= SPECTRUM_FREQUENCIES[0] {
            return 0;
        }
        if freq_hz >= SPECTRUM_FREQUENCIES[SPECTRUM_BANDS_COUNT - 1] {
            return SPECTRUM_BANDS_COUNT - 1;
        }

        let log_target = freq_hz.log10();
        let mut best_idx = 0;
        let mut min_dist = f64::MAX;

        for (i, &band_freq) in SPECTRUM_FREQUENCIES.iter().enumerate() {
            let dist = (band_freq.log10() - log_target).abs();
            if dist < min_dist {
                min_dist = dist;
                best_idx = i;
            }
        }

        best_idx
    }

    /// Evaluates instantaneous 32-band frequency magnitudes from real audio telemetry.
    ///
    /// Returns 32 normalized floats in `[0.0, 1.0]`.
    /// Handles silence, paused, stopped, and invalid inputs strictly without producing random noise.
    pub fn compute_magnitudes(&self, telemetry: &SpectrumTelemetry) -> [f32; SPECTRUM_BANDS_COUNT] {
        // Strict silence / inactive handling
        if !telemetry.is_playing
            || telemetry.rms_db.is_nan()
            || telemetry.rms_db.is_infinite()
            || telemetry.rms_db <= -60.0
        {
            return [0.0; SPECTRUM_BANDS_COUNT];
        }

        // Clamp telemetry parameters to physically valid ranges
        let rms = telemetry.rms_db.clamp(-60.0, 0.0);
        let energy = ((rms + 60.0) / 60.0).clamp(0.0, 1.0);

        let centroid = if telemetry.spectral_centroid.is_nan() || telemetry.spectral_centroid <= 0.0
        {
            1000.0
        } else {
            telemetry.spectral_centroid.clamp(20.0, 20000.0)
        };

        let spread = if telemetry.spectral_spread.is_nan() || telemetry.spectral_spread <= 0.0 {
            400.0
        } else {
            telemetry.spectral_spread.clamp(50.0, 10000.0)
        };

        let rolloff = if telemetry.spectral_rolloff.is_nan() || telemetry.spectral_rolloff <= 0.0 {
            12000.0
        } else {
            telemetry.spectral_rolloff.clamp(100.0, 22000.0)
        };

        let flatness = if telemetry.spectral_flatness.is_nan() {
            0.1
        } else {
            telemetry.spectral_flatness.clamp(0.0, 1.0)
        };

        let slope = if telemetry.spectral_slope.is_nan() {
            0.0
        } else {
            telemetry.spectral_slope.clamp(-1.0, 1.0)
        };

        let log_centroid = centroid.log10();
        // Dynamic bandwidth: when audio is highly tonal (low flatness), sharpen selectivity
        // so that pure tones cleanly peak in their exact frequency band.
        let base_spread = (1.0 + spread / centroid).log10().clamp(0.04, 1.5);
        let tonal_factor = (0.25 + 0.75 * flatness).clamp(0.25, 1.0);
        let log_spread = (base_spread * tonal_factor).max(0.035);

        let mut output = [0.0f32; SPECTRUM_BANDS_COUNT];

        for (i, &band_freq) in SPECTRUM_FREQUENCIES.iter().enumerate() {
            let log_f = band_freq.log10();

            // Gaussian spectral bell centered on measured centroid
            let z = (log_f - log_centroid) / log_spread;
            let bell = (-0.5 * z * z).exp();

            // Broad background excitation weighted by spectral flatness
            let broad = flatness * (0.8 - (i as f64 / SPECTRUM_BANDS_COUNT as f64) * 0.4);

            // Spectral tilt / slope
            let tilt = (1.0 + slope * (i as f64 - 16.0) / 16.0).clamp(0.1, 2.0);

            // High frequency roll-off attenuation
            let rolloff_att = if band_freq > rolloff {
                1.0 / (1.0 + ((band_freq - rolloff) / (0.35 * rolloff)).powi(2))
            } else {
                1.0
            };

            let combined = (((1.0 - 0.85 * flatness) * bell + (0.85 * flatness) * broad)
                * tilt
                * rolloff_att
                * energy)
                .clamp(0.0, 1.0);
            output[i] = combined as f32;
        }

        output
    }

    /// Steps the visualizer forward by one frame and updates smoothed magnitudes.
    pub fn update(&mut self, telemetry: &SpectrumTelemetry) -> [f32; SPECTRUM_BANDS_COUNT] {
        if telemetry.is_paused {
            // Decay smoothly to zero on pause
            for val in &mut self.smoothed_magnitudes {
                *val = (*val * 0.88).max(0.0);
                if *val < 0.005 {
                    *val = 0.0;
                }
            }
            return self.smoothed_magnitudes;
        }

        if !telemetry.is_playing {
            self.reset();
            return self.smoothed_magnitudes;
        }

        let targets = self.compute_magnitudes(telemetry);

        for (i, &target) in targets.iter().enumerate() {
            let current = self.smoothed_magnitudes[i];
            if target > current {
                // Fast attack
                self.smoothed_magnitudes[i] = current + (target - current) * self.attack_factor;
            } else {
                // Gentle release
                self.smoothed_magnitudes[i] = current - (current - target) * self.release_factor;
            }
            self.smoothed_magnitudes[i] = self.smoothed_magnitudes[i].clamp(0.0, 1.0);
        }

        self.smoothed_magnitudes
    }
}
