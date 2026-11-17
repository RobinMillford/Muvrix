//! AutoEq parametric equalizer profile parser, validator, mapping engine, and serializer.
//!
//! Supports both standard EqualizerAPO / AutoEq text presets (`ParametricEQ.txt`)
//! and AutoEq / PipeWire JSON presets.

use serde::{Deserialize, Serialize};

/// Supported parametric equalizer filter types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoEqFilterType {
    /// Peaking / Bell filter (PK).
    Peaking,
    /// Low-shelf filter (LSC / LowShelf).
    LowShelf,
    /// High-shelf filter (HSC / HighShelf).
    HighShelf,
}

impl AutoEqFilterType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Peaking => "PK",
            Self::LowShelf => "LSC",
            Self::HighShelf => "HSC",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Peaking => "Peaking",
            Self::LowShelf => "Low Shelf",
            Self::HighShelf => "High Shelf",
        }
    }
}

/// A single parametric filter band within an AutoEq profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutoEqFilter {
    pub filter_type: AutoEqFilterType,
    pub frequency_hz: f64,
    pub gain_db: f64,
    pub q_factor: f64,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

impl AutoEqFilter {
    pub fn new(
        filter_type: AutoEqFilterType,
        frequency_hz: f64,
        gain_db: f64,
        q_factor: f64,
    ) -> Result<Self, AutoEqError> {
        Self::new_with_enabled(filter_type, frequency_hz, gain_db, q_factor, true)
    }

    pub fn new_with_enabled(
        filter_type: AutoEqFilterType,
        frequency_hz: f64,
        gain_db: f64,
        q_factor: f64,
        enabled: bool,
    ) -> Result<Self, AutoEqError> {
        if frequency_hz.is_nan() || frequency_hz.is_infinite() {
            return Err(AutoEqError::InvalidNumber(
                "frequency cannot be NaN or infinite".to_string(),
            ));
        }
        if gain_db.is_nan() || gain_db.is_infinite() {
            return Err(AutoEqError::InvalidNumber(
                "gain cannot be NaN or infinite".to_string(),
            ));
        }
        if q_factor.is_nan() || q_factor.is_infinite() {
            return Err(AutoEqError::InvalidNumber(
                "Q factor cannot be NaN or infinite".to_string(),
            ));
        }

        if frequency_hz <= 0.0 || frequency_hz < 10.0 || frequency_hz > 24000.0 {
            return Err(AutoEqError::InvalidFrequency(frequency_hz));
        }
        if q_factor <= 0.0 || q_factor < 0.05 || q_factor > 50.0 {
            return Err(AutoEqError::InvalidQFactor(q_factor));
        }

        Ok(Self {
            filter_type,
            frequency_hz,
            gain_db: gain_db.clamp(-36.0, 36.0),
            q_factor: q_factor.clamp(0.05, 50.0),
            enabled,
        })
    }

    /// Evaluates continuous transfer function magnitude response in dB at `f_hz`.
    ///
    /// Implements exact 2nd-order analog prototype biquad equations conforming to the
    /// Robert Bristow-Johnson (RBJ) Audio EQ Cookbook specification used by AutoEq and EqualizerAPO:
    /// - Peaking: symmetric bell curve with exact peak gain at `f0` and bandwidth controlled by Q.
    /// - Low-Shelf: exact half-gain in dB at `f0` with slope and transition overshoot governed by Q.
    /// - High-Shelf: exact half-gain in dB at `f0` with slope and transition overshoot governed by Q.
    pub fn gain_at_freq(&self, f_hz: f64) -> f64 {
        if !self.enabled || f_hz <= 0.0 || self.gain_db.abs() < 0.001 {
            return 0.0;
        }

        let q = self.q_factor.clamp(0.01, 50.0);
        let a = 10.0f64.powf(self.gain_db / 40.0);
        let w = f_hz / self.frequency_hz.max(1.0);
        let w2 = w * w;

        match self.filter_type {
            AutoEqFilterType::Peaking => {
                let num = (1.0 - w2).powi(2) + (a / q).powi(2) * w2;
                let den = (1.0 - w2).powi(2) + (1.0 / (a * q)).powi(2) * w2;
                if den <= 0.0 || num <= 0.0 {
                    0.0
                } else {
                    10.0 * (num / den).log10()
                }
            }
            AutoEqFilterType::LowShelf => {
                let num = (a - w2).powi(2) + (a / (q * q)) * w2;
                let den = (1.0 - a * w2).powi(2) + (a / (q * q)) * w2;
                if den <= 0.0 || num <= 0.0 {
                    0.0
                } else {
                    10.0 * ((a * a) * (num / den)).log10()
                }
            }
            AutoEqFilterType::HighShelf => {
                let num = (1.0 - a * w2).powi(2) + (a / (q * q)) * w2;
                let den = (a - w2).powi(2) + (a / (q * q)) * w2;
                if den <= 0.0 || num <= 0.0 {
                    0.0
                } else {
                    10.0 * ((a * a) * (num / den)).log10()
                }
            }
        }
    }
}

/// An AutoEq parametric equalizer profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutoEqProfile {
    pub name: String,
    pub preamp_db: f64,
    pub filters: Vec<AutoEqFilter>,
}

/// Errors occurring during AutoEq parsing, mapping, or validation.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum AutoEqError {
    #[error("Empty or whitespace-only AutoEq input")]
    EmptyInput,
    #[error("Failed to parse AutoEq text format: {0}")]
    TextParseError(String),
    #[error("Failed to parse AutoEq JSON format: {0}")]
    JsonParseError(String),
    #[error("Unsupported filter type: {0}")]
    UnsupportedFilterType(String),
    #[error("Missing required field: {0}")]
    MissingField(String),
    #[error("Invalid numerical value (NaN or Infinite): {0}")]
    InvalidNumber(String),
    #[error("Invalid frequency: {0} Hz (must be positive and between 10 Hz and 24,000 Hz)")]
    InvalidFrequency(f64),
    #[error("Invalid Q factor: {0} (must be positive and between 0.05 and 50.0)")]
    InvalidQFactor(f64),
    #[error("No valid parametric filters found in AutoEq profile")]
    NoFiltersFound,
}

impl AutoEqProfile {
    /// Creates a new profile with validation.
    pub fn new(
        name: String,
        preamp_db: f64,
        filters: Vec<AutoEqFilter>,
    ) -> Result<Self, AutoEqError> {
        if preamp_db.is_nan() || preamp_db.is_infinite() {
            return Err(AutoEqError::InvalidNumber(
                "preamp cannot be NaN or infinite".to_string(),
            ));
        }
        if filters.is_empty() {
            return Err(AutoEqError::NoFiltersFound);
        }
        for (idx, filter) in filters.iter().enumerate() {
            if filter.frequency_hz.is_nan()
                || filter.frequency_hz.is_infinite()
                || filter.frequency_hz <= 0.0
            {
                return Err(AutoEqError::InvalidNumber(format!(
                    "Filter {idx} frequency must be a finite positive number"
                )));
            }
            if filter.gain_db.is_nan() || filter.gain_db.is_infinite() {
                return Err(AutoEqError::InvalidNumber(format!(
                    "Filter {idx} gain cannot be NaN or infinite"
                )));
            }
            if filter.q_factor.is_nan() || filter.q_factor.is_infinite() || filter.q_factor <= 0.0 {
                return Err(AutoEqError::InvalidNumber(format!(
                    "Filter {idx} Q factor must be a finite positive number"
                )));
            }
        }
        Ok(Self {
            name,
            preamp_db: preamp_db.clamp(-36.0, 36.0),
            filters,
        })
    }

    /// Automatically detects whether input is JSON or text, and parses accordingly.
    pub fn parse(input: &str, name_hint: Option<&str>) -> Result<Self, AutoEqError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(AutoEqError::EmptyInput);
        }

        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            Self::parse_json(trimmed, name_hint)
        } else {
            Self::parse_text(trimmed, name_hint)
        }
    }

    /// Parses an AutoEq text preset (EqualizerAPO / ParametricEQ syntax).
    ///
    /// Accepts lines like:
    /// `Preamp: -6.5 dB`
    /// `Filter 1: ON PK Fc 28 Hz Gain 6.1 dB Q 1.41`
    /// `Filter 2: ON LSC Fc 105 Hz Gain 5.5 dB Q 0.70`
    /// `Filter 3: ON HSC Fc 10000 Hz Gain -2.0 dB Q 0.70`
    pub fn parse_text(input: &str, name_hint: Option<&str>) -> Result<Self, AutoEqError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(AutoEqError::EmptyInput);
        }

        let mut preamp_db = 0.0;
        let mut filters = Vec::new();
        let mut profile_name = name_hint.unwrap_or("AutoEq Profile").to_string();

        for (line_idx, raw_line) in trimmed.lines().enumerate() {
            let line_num = line_idx + 1;
            let line = raw_line.trim();

            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                // If comment contains name or headline, extract if default
                if line.starts_with("# Name:") || line.starts_with("# Profile:") {
                    let parts: Vec<&str> = line.splitn(2, ':').collect();
                    if parts.len() == 2 && !parts[1].trim().is_empty() {
                        profile_name = parts[1].trim().to_string();
                    }
                }
                continue;
            }

            // Check for Preamp line
            let line_lower = line.to_lowercase();
            if line_lower.starts_with("preamp:") || line_lower.starts_with("preamp ") {
                let parts: Vec<&str> = line
                    .split(&[':', ' '][..])
                    .filter(|s| !s.is_empty())
                    .collect();
                let mut found_val = None;
                for part in &parts[1..] {
                    let clean = part.trim_end_matches("db").trim_end_matches("dB");
                    if let Ok(val) = clean.parse::<f64>() {
                        found_val = Some(val);
                        break;
                    }
                }
                if let Some(val) = found_val {
                    if val.is_nan() || val.is_infinite() {
                        return Err(AutoEqError::InvalidNumber(format!(
                            "Preamp at line {line_num} is NaN or infinite"
                        )));
                    }
                    preamp_db = val;
                } else {
                    return Err(AutoEqError::TextParseError(format!(
                        "Could not parse Preamp value at line {line_num}: '{line}'"
                    )));
                }
                continue;
            }

            // Check for Filter line
            if line_lower.starts_with("filter")
                || line_lower.starts_with("on")
                || line_lower.starts_with("off")
                || line_lower.starts_with("pk")
                || line_lower.starts_with("lsc")
                || line_lower.starts_with("hsc")
            {
                let filter = Self::parse_filter_line(line, line_num)?;
                filters.push(filter);
            }
        }

        if filters.is_empty() {
            return Err(AutoEqError::NoFiltersFound);
        }

        Self::new(profile_name, preamp_db, filters)
    }

    fn parse_filter_line(line: &str, line_num: usize) -> Result<AutoEqFilter, AutoEqError> {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.is_empty() {
            return Err(AutoEqError::TextParseError(format!(
                "Empty filter line {line_num}"
            )));
        }

        let mut enabled = true;
        let mut filter_type = None;
        let mut fc = None;
        let mut gain = None;
        let mut q = None;

        let mut i = 0;
        // Skip optional "Filter" and "1:" tokens
        if tokens[i].eq_ignore_ascii_case("filter") {
            i += 1;
            if i < tokens.len() && (tokens[i].ends_with(':') || tokens[i].parse::<usize>().is_ok())
            {
                i += 1;
            }
        }

        while i < tokens.len() {
            let tok = tokens[i].trim_end_matches(':');
            let tok_lower = tok.to_lowercase();

            if tok_lower == "on" {
                enabled = true;
                i += 1;
                continue;
            } else if tok_lower == "off" {
                enabled = false;
                i += 1;
                continue;
            }

            // Check filter type keyword
            if filter_type.is_none() {
                if tok_lower == "pk" || tok_lower == "peaking" || tok_lower == "bell" {
                    filter_type = Some(AutoEqFilterType::Peaking);
                    i += 1;
                    continue;
                } else if tok_lower == "lsc" || tok_lower == "lowshelf" || tok_lower == "low_shelf"
                {
                    filter_type = Some(AutoEqFilterType::LowShelf);
                    i += 1;
                    continue;
                } else if tok_lower == "hsc"
                    || tok_lower == "highshelf"
                    || tok_lower == "high_shelf"
                {
                    filter_type = Some(AutoEqFilterType::HighShelf);
                    i += 1;
                    continue;
                }
            }

            // Parameter key-value pairs: Fc, Gain, Q
            if tok_lower == "fc" || tok_lower == "freq" || tok_lower == "frequency" {
                if i + 1 < tokens.len() {
                    let clean = tokens[i + 1].trim_end_matches("hz").trim_end_matches("Hz");
                    let val = clean.parse::<f64>().map_err(|_| {
                        AutoEqError::TextParseError(format!(
                            "Invalid frequency '{clean}' at line {line_num}"
                        ))
                    })?;
                    fc = Some(val);
                    i += 2;
                    // Skip optional "Hz" if in next token
                    if i < tokens.len() && tokens[i].eq_ignore_ascii_case("hz") {
                        i += 1;
                    }
                    continue;
                }
            } else if tok_lower == "gain" {
                if i + 1 < tokens.len() {
                    let clean = tokens[i + 1].trim_end_matches("db").trim_end_matches("dB");
                    let val = clean.parse::<f64>().map_err(|_| {
                        AutoEqError::TextParseError(format!(
                            "Invalid gain '{clean}' at line {line_num}"
                        ))
                    })?;
                    gain = Some(val);
                    i += 2;
                    // Skip optional "dB" if in next token
                    if i < tokens.len() && tokens[i].eq_ignore_ascii_case("db") {
                        i += 1;
                    }
                    continue;
                }
            } else if (tok_lower == "q" || tok_lower == "q_factor") && i + 1 < tokens.len() {
                let clean = tokens[i + 1];
                let val = clean.parse::<f64>().map_err(|_| {
                    AutoEqError::TextParseError(format!(
                        "Invalid Q factor '{clean}' at line {line_num}"
                    ))
                })?;
                q = Some(val);
                i += 2;
                continue;
            }

            i += 1;
        }

        let ftype = filter_type.ok_or_else(|| {
            AutoEqError::TextParseError(format!("Missing filter type at line {line_num}: '{line}'"))
        })?;
        let frequency = fc.ok_or_else(|| {
            AutoEqError::TextParseError(format!(
                "Missing frequency (Fc) at line {line_num}: '{line}'"
            ))
        })?;
        let gain_val = gain.ok_or_else(|| {
            AutoEqError::TextParseError(format!("Missing gain at line {line_num}: '{line}'"))
        })?;
        let q_val = q.unwrap_or(1.414); // Default standard Q if not explicitly provided

        AutoEqFilter::new_with_enabled(ftype, frequency, gain_val, q_val, enabled)
    }

    /// Parses an AutoEq JSON preset.
    pub fn parse_json(input: &str, name_hint: Option<&str>) -> Result<Self, AutoEqError> {
        let value: serde_json::Value =
            serde_json::from_str(input).map_err(|e| AutoEqError::JsonParseError(e.to_string()))?;

        // Extract profile name
        let name = value
            .get("name")
            .and_then(|v| v.as_str())
            .or(name_hint)
            .unwrap_or("AutoEq Profile")
            .to_string();

        // Extract preamp
        let preamp_db = value
            .get("preamp")
            .or_else(|| value.get("preamp_db"))
            .or_else(|| value.get("pregain"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        // Extract filters array
        let raw_filters = value
            .get("filters")
            .or_else(|| value.get("parametric_filters"))
            .or_else(|| value.get("bands"))
            .and_then(|v| v.as_array())
            .or_else(|| value.as_array())
            .ok_or_else(|| {
                AutoEqError::JsonParseError("No filters array found in JSON".to_string())
            })?;

        if raw_filters.is_empty() {
            return Err(AutoEqError::NoFiltersFound);
        }

        let mut filters = Vec::new();
        for (idx, item) in raw_filters.iter().enumerate() {
            let filter = Self::parse_json_filter_item(item, idx + 1)?;
            filters.push(filter);
        }

        Self::new(name, preamp_db, filters)
    }

    fn parse_json_filter_item(
        item: &serde_json::Value,
        index: usize,
    ) -> Result<AutoEqFilter, AutoEqError> {
        let type_str = item
            .get("type")
            .or_else(|| item.get("filter_type"))
            .and_then(|v| v.as_str())
            .unwrap_or("PK");

        let filter_type = match type_str.to_lowercase().as_str() {
            "pk" | "peaking" | "bell" => AutoEqFilterType::Peaking,
            "lsc" | "lowshelf" | "low_shelf" => AutoEqFilterType::LowShelf,
            "hsc" | "highshelf" | "high_shelf" => AutoEqFilterType::HighShelf,
            other => return Err(AutoEqError::UnsupportedFilterType(other.to_string())),
        };

        let fc = item
            .get("fc")
            .or_else(|| item.get("freq"))
            .or_else(|| item.get("frequency"))
            .or_else(|| item.get("frequency_hz"))
            .and_then(|v| v.as_f64())
            .ok_or_else(|| {
                AutoEqError::MissingField(format!("Missing frequency for filter #{index}"))
            })?;

        let gain = item
            .get("gain")
            .or_else(|| item.get("gain_db"))
            .and_then(|v| v.as_f64())
            .ok_or_else(|| {
                AutoEqError::MissingField(format!("Missing gain for filter #{index}"))
            })?;

        let q = item
            .get("q")
            .or_else(|| item.get("q_factor"))
            .or_else(|| item.get("Q"))
            .and_then(|v| v.as_f64())
            .unwrap_or(1.414);

        let enabled = item
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        AutoEqFilter::new_with_enabled(filter_type, fc, gain, q, enabled)
    }

    /// Exports the profile as a standard EqualizerAPO / AutoEq text format.
    pub fn export_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("# Name: {}\n", self.name));
        out.push_str(&format!("Preamp: {:.1} dB\n", self.preamp_db));

        for (idx, filter) in self.filters.iter().enumerate() {
            let state = if filter.enabled { "ON" } else { "OFF" };
            out.push_str(&format!(
                "Filter {}: {} {} Fc {:.1} Hz Gain {:.1} dB Q {:.2}\n",
                idx + 1,
                state,
                filter.filter_type.as_str(),
                filter.frequency_hz,
                filter.gain_db,
                filter.q_factor
            ));
        }

        out
    }

    /// Exports the profile as portable JSON.
    pub fn export_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Evaluates cumulative response in dB at `f_hz` (including preamp).
    pub fn cumulative_response_at_freq(&self, f_hz: f64) -> f64 {
        let mut total = self.preamp_db;
        for filter in &self.filters {
            total += filter.gain_at_freq(f_hz);
        }
        total
    }

    /// Maps the AutoEq profile's transfer function to the standard 10 octave bands.
    pub fn map_to_10_bands(&self) -> [f64; 10] {
        const STANDARD_FREQS: [f64; 10] = [
            31.25, 62.5, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
        ];

        let mut gains = [0.0; 10];
        for (i, &f) in STANDARD_FREQS.iter().enumerate() {
            // Compute combined gain without preamp for the slider representation
            let mut band_gain = 0.0;
            for filter in &self.filters {
                band_gain += filter.gain_at_freq(f);
            }
            gains[i] = (band_gain.clamp(-12.0, 12.0) * 10.0).round() / 10.0;
        }
        gains
    }
}
