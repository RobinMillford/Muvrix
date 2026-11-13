//! What the shell knows about the output path, in the terms the HDR decision
//! needs.
//!
//! This type exists so that `MuvrixApp` never has to guess: every field is
//! something a specific part of the pipeline observed, and the HDR decision is
//! derived from them rather than from a build flag or from "the file is HDR".

use muvrix_player::DisplayCapabilities;

/// Evidence collected about the presentation output.
#[derive(Debug, Clone)]
pub struct OutputCaps {
    /// `true` only when the whole chain below is satisfied: 10-bit surface,
    /// verified precision, PQ + BT.2020 supported by the compositor, and the
    /// description accepted on the real window surface.
    pub native_hdr_available: bool,
    /// Whether a 10-bit EGLConfig was selected.
    pub surface_ten_bit: bool,
    pub surface_bits: u32,
    /// Result of the round-trip precision probe.
    pub precision_verified: bool,
    /// `wp_color_manager_v1` bound and PQ/BT.2020 advertised.
    pub color_manager_pq_bt2020: bool,
    /// Compositor sent `ready` for our PQ description and accepted it on the
    /// window surface.
    pub description_accepted: bool,
    pub description_identity: Option<u64>,
    /// Never promoted by the runner: it cannot observe scanout or the panel.
    pub physical_verified: bool,
    /// Human-readable pipeline label for diagnostics.
    pub pipeline: String,
    /// Why HDR is off, when it is off.
    pub fallback_reason: Option<String>,
    /// Display capability model handed to `plan_presentation`.
    pub display: DisplayCapabilities,
    /// Target peak luminance requested from the engine, and where it came from.
    pub target_peak_nits: Option<u32>,
    pub peak_source: &'static str,
    pub swap_interval: i32,
    /// True when the native runner (not eframe) owns the window and swap chain.
    pub native_shell: bool,
}

impl Default for OutputCaps {
    fn default() -> Self {
        Self {
            native_hdr_available: false,
            surface_ten_bit: false,
            surface_bits: 8,
            precision_verified: false,
            color_manager_pq_bt2020: false,
            description_accepted: false,
            description_identity: None,
            physical_verified: false,
            pipeline: "sdr".to_string(),
            fallback_reason: Some("no presentation output attached".to_string()),
            display: DisplayCapabilities::default(),
            target_peak_nits: None,
            peak_source: "none",
            swap_interval: 0,
            native_shell: false,
        }
    }
}

impl OutputCaps {
    /// True when the native runner presents the frames itself, which is also
    /// when the shell must composite video and UI in one pass.
    pub fn is_native_shell(&self) -> bool {
        self.native_shell
    }

    /// Text block used by the diagnostics view and the log.
    pub fn summary_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "Surface: {}-bit (ten_bit={} precision_verified={})",
            self.surface_bits, self.surface_ten_bit, self.precision_verified
        ));
        out.push(format!(
            "Compositor: color_management_pq_bt2020={} description_accepted={} identity={}",
            self.color_manager_pq_bt2020,
            self.description_accepted,
            self.description_identity
                .map(|i| i.to_string())
                .unwrap_or_else(|| "-".to_string())
        ));
        out.push(format!("Pipeline: {}", self.pipeline));
        out.push(format!(
            "Physical display: {}",
            if self.physical_verified {
                "verified"
            } else {
                "not verified (no scanout/panel observation available)"
            }
        ));
        if let Some(reason) = &self.fallback_reason {
            out.push(format!("Reason: {reason}"));
        }
        out
    }
}
