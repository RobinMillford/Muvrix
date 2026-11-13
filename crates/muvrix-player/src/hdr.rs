//! HDR source classification, display capability model and presentation policy.
//!
//! This module deliberately keeps four distinct facts apart, because conflating
//! them is how players end up claiming HDR they do not deliver:
//!
//! 1. what the *source* is (`HdrSourceType`),
//! 2. what the *display* can do (`DisplayCapabilities`),
//! 3. what the *platform path* can deliver (`HdrAvailability`),
//! 4. what is actually happening now (`PresentationState`).
//!
//! No luminance value is ever invented: every photometric field is `Option`.

/// HDR format of the source stream, derived from signalled metadata only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HdrSourceType {
    /// Standard dynamic range (SDR) or no HDR signalling.
    Sdr,
    /// HDR10: PQ (SMPTE ST 2084) transfer, BT.2020 primaries, static metadata.
    Hdr10Pq,
    /// Hybrid Log-Gamma.
    Hlg,
    /// PQ with dynamic metadata that the pipeline may or may not act on.
    Hdr10Plus,
    /// Dolby Vision (profile determines whether it is usable at all).
    DolbyVision { profile: Option<u8> },
    /// PQ transfer but missing/contradictory primaries or metadata.
    Incomplete,
}

impl HdrSourceType {
    pub fn is_hdr(&self) -> bool {
        !matches!(self, Self::Sdr)
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Sdr => "SDR",
            Self::Hdr10Pq => "HDR10 (PQ/BT.2020)",
            Self::Hlg => "HLG",
            Self::Hdr10Plus => "HDR10+",
            Self::DolbyVision { .. } => "Dolby Vision",
            Self::Incomplete => "Unknown HDR",
        }
    }
}

/// Classifies the source from engine-reported colour properties.
///
/// `gamma` is `video-params/gamma` (`pq`, `hlg`, `bt.1886`, …), `primaries` is
/// `video-params/primaries`. Nothing is inferred beyond these signals.
pub fn classify_source(
    gamma: &str,
    primaries: &str,
    dolby_vision_profile: Option<u8>,
) -> HdrSourceType {
    let gamma = gamma.trim().to_ascii_lowercase();
    let primaries = primaries.trim().to_ascii_lowercase();

    if let Some(profile) = dolby_vision_profile {
        return HdrSourceType::DolbyVision {
            profile: Some(profile),
        };
    }

    match gamma.as_str() {
        "pq" | "smpte st 2084" | "smpte2084" => {
            if primaries.is_empty() || primaries == "bt.2020" || primaries == "bt2020" {
                HdrSourceType::Hdr10Pq
            } else {
                HdrSourceType::Incomplete
            }
        }
        "hlg" | "arib-std-b67" => HdrSourceType::Hlg,
        "" => {
            if primaries == "bt.2020" || primaries == "bt2020" {
                HdrSourceType::Incomplete
            } else {
                HdrSourceType::Sdr
            }
        }
        _ => HdrSourceType::Sdr,
    }
}

/// What the operating system reports about the active output.
///
/// Filled from platform APIs; unknown values stay `None` rather than being
/// estimated. `hdr_enabled` is the OS presentation state, which is not the same
/// as the panel being HDR-capable.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DisplayCapabilities {
    pub display_id: Option<String>,
    pub hdr_capable: Option<bool>,
    pub hdr_enabled: Option<bool>,
    pub max_luminance_nits: Option<f64>,
    pub min_luminance_nits: Option<f64>,
    pub reference_white_nits: Option<f64>,
    pub primaries: Option<String>,
    pub transfer_functions: Vec<String>,
    pub presentation_formats: Vec<String>,
    pub detection_status: CapabilityDetection,
}

/// Whether capability detection ran and succeeded.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum CapabilityDetection {
    /// Detection was not attempted on this platform/path.
    #[default]
    NotAttempted,
    /// Detection ran and produced values.
    Reported,
    /// Detection ran but the platform exposes nothing useful.
    Unavailable { reason: String },
    /// Detection failed.
    Failed { reason: String },
}

/// Whether true HDR presentation is possible on this platform/path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HdrAvailability {
    SupportedAndActive,
    SupportedButDisabled,
    DisplayNotHdrCapable,
    CompositorUnsupported,
    NativePresentationUnsupported,
    UnsupportedByEngineBuild,
    InitializationFailed { reason: String },
    NotTested,
}

/// Presentation modes the renderer can be asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresentationMode {
    /// SDR output, engine tone-maps HDR sources.
    SdrToneMapped,
    /// SDR output, SDR source passed through.
    SdrPassthrough,
    /// True HDR10 output (PQ transfer, BT.2020 primaries).
    Hdr10Pq,
    /// HLG output.
    Hlg,
    /// Windows scRGB (linear, FP16).
    Scrgb,
    /// macOS Extended Dynamic Range.
    Edr,
}

/// What the application is doing right now, with the reason when it is a fallback.
#[derive(Debug, Clone, PartialEq)]
pub struct PresentationState {
    pub source: HdrSourceType,
    pub mode: PresentationMode,
    pub availability: HdrAvailability,
    /// True only when HDR pixels are actually being delivered to an HDR output.
    pub hdr_presented: bool,
    /// True when the engine is converting HDR to SDR.
    pub tone_mapping: bool,
    pub reason: Option<String>,
}

/// The engine options implied by a presentation decision.
///
/// `None` means "leave the engine default alone" rather than "unset".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ColorPlan {
    pub target_prim: Option<&'static str>,
    pub target_trc: Option<&'static str>,
    pub tone_mapping: Option<&'static str>,
    pub hdr_compute_peak: Option<&'static str>,
}

/// Decides the presentation mode and colour options.
///
/// `native_hdr_available` must be true only when the *whole* path is proven
/// (surface format, surface colour tagging, OS state). A high-precision
/// intermediate render target alone does **not** qualify.
pub fn plan_presentation(
    source: &HdrSourceType,
    display: &DisplayCapabilities,
    native_hdr_available: bool,
) -> (PresentationState, ColorPlan) {
    let display_hdr_enabled = display.hdr_enabled.unwrap_or(false);

    let availability = if native_hdr_available {
        if display_hdr_enabled {
            HdrAvailability::SupportedAndActive
        } else {
            HdrAvailability::SupportedButDisabled
        }
    } else if display.hdr_capable == Some(false) {
        HdrAvailability::DisplayNotHdrCapable
    } else {
        HdrAvailability::NativePresentationUnsupported
    };

    match source {
        HdrSourceType::Sdr => {
            // SDR content is passed through unchanged. Presenting SDR inside an
            // HDR container is a separate, deliberate feature (reference-white
            // scaling) and is not enabled implicitly.
            (
                PresentationState {
                    source: source.clone(),
                    mode: PresentationMode::SdrPassthrough,
                    availability,
                    hdr_presented: false,
                    tone_mapping: false,
                    reason: None,
                },
                ColorPlan::default(),
            )
        }
        hdr if hdr.is_hdr() => {
            if native_hdr_available && display_hdr_enabled {
                (
                    PresentationState {
                        source: source.clone(),
                        mode: PresentationMode::Hdr10Pq,
                        availability,
                        hdr_presented: true,
                        tone_mapping: false,
                        reason: None,
                    },
                    ColorPlan {
                        target_prim: Some("bt.2020"),
                        target_trc: Some("pq"),
                        tone_mapping: Some("clip"),
                        hdr_compute_peak: None,
                    },
                )
            } else {
                (
                    PresentationState {
                        source: source.clone(),
                        mode: PresentationMode::SdrToneMapped,
                        availability: availability.clone(),
                        hdr_presented: false,
                        tone_mapping: true,
                        reason: Some(match &availability {
                            HdrAvailability::DisplayNotHdrCapable => {
                                "Display does not report HDR capability; tone-mapping to SDR"
                                    .to_string()
                            }
                            HdrAvailability::SupportedButDisabled => {
                                "HDR is supported but not enabled in the OS; tone-mapping to SDR"
                                    .to_string()
                            }
                            _ => {
                                "Native HDR presentation is unavailable on this path; tone-mapping to SDR"
                                    .to_string()
                            }
                        }),
                    },
                    ColorPlan {
                        target_prim: Some("auto"),
                        target_trc: Some("auto"),
                        tone_mapping: Some("auto"),
                        hdr_compute_peak: None,
                    },
                )
            }
        }
        _ => unreachable!("non-SDR sources are handled by the is_hdr arm"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hdr_display(enabled: bool) -> DisplayCapabilities {
        DisplayCapabilities {
            hdr_capable: Some(true),
            hdr_enabled: Some(enabled),
            ..Default::default()
        }
    }

    #[test]
    fn pq_bt2020_is_hdr10() {
        assert_eq!(
            classify_source("pq", "bt.2020", None),
            HdrSourceType::Hdr10Pq
        );
    }

    #[test]
    fn hlg_is_separate() {
        assert_eq!(classify_source("hlg", "bt.2020", None), HdrSourceType::Hlg);
    }

    #[test]
    fn sdr_is_sdr() {
        assert_eq!(
            classify_source("bt.1886", "bt.709", None),
            HdrSourceType::Sdr
        );
        assert_eq!(classify_source("", "", None), HdrSourceType::Sdr);
    }

    #[test]
    fn pq_with_wrong_primaries_is_incomplete_not_hdr10() {
        assert_eq!(
            classify_source("pq", "bt.709", None),
            HdrSourceType::Incomplete
        );
    }

    #[test]
    fn hdr_source_without_native_path_is_tone_mapped_and_says_so() {
        let (state, plan) = plan_presentation(&HdrSourceType::Hdr10Pq, &hdr_display(true), false);
        assert_eq!(state.mode, PresentationMode::SdrToneMapped);
        assert!(state.tone_mapping);
        assert!(!state.hdr_presented);
        assert!(state.reason.is_some());
        assert_eq!(plan.target_trc, Some("auto"));
    }

    #[test]
    fn hdr_source_with_native_path_is_not_tone_mapped() {
        let (state, plan) = plan_presentation(&HdrSourceType::Hdr10Pq, &hdr_display(true), true);
        assert_eq!(state.mode, PresentationMode::Hdr10Pq);
        assert!(state.hdr_presented);
        assert!(!state.tone_mapping);
        assert_eq!(plan.target_trc, Some("pq"));
        assert_eq!(plan.target_prim, Some("bt.2020"));
    }

    #[test]
    fn hdr_disabled_in_os_is_not_presented_as_hdr() {
        let (state, _) = plan_presentation(&HdrSourceType::Hdr10Pq, &hdr_display(false), true);
        assert!(!state.hdr_presented);
        assert_eq!(state.availability, HdrAvailability::SupportedButDisabled);
    }

    #[test]
    fn sdr_source_is_never_tone_mapped() {
        let (state, plan) = plan_presentation(&HdrSourceType::Sdr, &hdr_display(true), true);
        assert_eq!(state.mode, PresentationMode::SdrPassthrough);
        assert!(!state.tone_mapping);
        assert_eq!(plan, ColorPlan::default());
    }

    #[test]
    fn non_hdr_capable_display_is_named_as_such() {
        let display = DisplayCapabilities {
            hdr_capable: Some(false),
            ..Default::default()
        };
        let (state, _) = plan_presentation(&HdrSourceType::Hdr10Pq, &display, false);
        assert_eq!(state.availability, HdrAvailability::DisplayNotHdrCapable);
    }
}
