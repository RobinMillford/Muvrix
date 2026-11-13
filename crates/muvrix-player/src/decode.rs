//! Capability-aware hardware decoding policy and observation.
//!
//! The engine is the only authority on which decoder actually ran. This module
//! never derives a decode path from the *requested* mode; it classifies what
//! `hwdec-current` and the decoded frame pixel format report after the decoder
//! has initialised, and it supplies a platform-appropriate ordered list of
//! decoder modes to try on subsequent loads.

/// Host platform family, used to pick decoder candidate names.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PlatformFamily {
    Linux,
    Windows,
    MacOs,
    Other,
}

impl PlatformFamily {
    /// Platform this binary was compiled for.
    pub const fn current() -> Self {
        if cfg!(target_os = "linux") {
            Self::Linux
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Other
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Windows => "windows",
            Self::MacOs => "macos",
            Self::Other => "other",
        }
    }
}

/// Actual memory path observed for the running decoder.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DecodePath {
    /// Hardware decoding with GPU-surface sharing (no host copy-back).
    DirectHw,
    /// Hardware decoding with download to host memory before upload.
    HwCopyBack,
    /// CPU decoding.
    Software,
    /// No decoder active (idle / audio-only / between files).
    Inactive,
    /// `hwdec-current` and the pixel format disagree, or the decoder is not a
    /// known family. Reported as unknown rather than guessed.
    Unknown,
}

impl DecodePath {
    pub fn is_hardware(self) -> bool {
        matches!(self, Self::DirectHw | Self::HwCopyBack)
    }

    pub fn is_direct(self) -> bool {
        matches!(self, Self::DirectHw)
    }
}

/// Ordered decoder modes to try, most capable first.
///
/// `auto-safe` is the engine's own capability ladder: it picks a hardware mode
/// it can actually initialise and falls back on its own. The explicit modes are
/// only tried after an observed software fallback, so a platform without the
/// required interop never gets a mode forced on it.
#[derive(Debug, Clone)]
pub struct DecodePolicy {
    family: PlatformFamily,
    candidates: Vec<&'static str>,
}

impl DecodePolicy {
    /// Builds the policy for the current platform.
    ///
    /// `has_native_display` is true when the application handed the engine a
    /// native display resource. Without one, direct modes cannot initialise and
    /// must not be requested.
    pub fn for_platform(family: PlatformFamily, has_native_display: bool) -> Self {
        let candidates = match (family, has_native_display) {
            (PlatformFamily::Linux, true) => {
                vec!["auto-safe", "vaapi", "vaapi-copy"]
            }
            // No native display: the engine cannot create an interop context, so
            // direct modes are omitted rather than silently downgrading.
            (PlatformFamily::Linux, false) => vec!["auto-safe", "vaapi-copy"],
            (PlatformFamily::Windows, true) => {
                vec!["auto-safe", "d3d11va", "d3d11va-copy"]
            }
            (PlatformFamily::Windows, false) => vec!["auto-safe", "d3d11va-copy"],
            (PlatformFamily::MacOs, _) => vec!["auto-safe", "videotoolbox", "videotoolbox-copy"],
            (PlatformFamily::Other, _) => vec!["auto-safe"],
        };
        Self { family, candidates }
    }

    /// Policy for the running platform.
    pub fn for_current(has_native_display: bool) -> Self {
        if let Some(forced) = Self::test_override() {
            log::warn!(
                "TEST-ONLY decoder override active: MUVRIX_TEST_HWDEC={forced}. \
                 This bypasses the capability ladder and is not a production path."
            );
            return Self {
                family: PlatformFamily::current(),
                candidates: vec![forced],
            };
        }
        Self::for_platform(PlatformFamily::current(), has_native_display)
    }

    /// Reads the **test-only** decoder override `MUVRIX_TEST_HWDEC`.
    ///
    /// Exists so a controlled experiment can change exactly one variable (the
    /// decoder) without editing production defaults: `vaapi` forces the direct
    /// path, `vaapi-copy` the copy-back path, `no` software decoding, and any
    /// other engine mode name is passed through verbatim. When unset, the policy
    /// ladder above is used unchanged. The value is leaked because the policy
    /// stores `&'static str` candidates and this only ever runs once per process.
    fn test_override() -> Option<&'static str> {
        let raw = std::env::var("MUVRIX_TEST_HWDEC").ok()?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(Box::leak(trimmed.to_string().into_boxed_str()))
    }

    pub fn family(&self) -> PlatformFamily {
        self.family
    }

    /// Mode to request on the first load.
    pub fn initial_mode(&self) -> &'static str {
        self.candidates.first().copied().unwrap_or("auto-safe")
    }

    /// Mode to request after `attempt` observed fallbacks, if any remain.
    pub fn next_mode(&self, attempt: usize) -> Option<&'static str> {
        self.candidates.get(attempt + 1).copied()
    }

    pub fn candidates(&self) -> &[&'static str] {
        &self.candidates
    }
}

/// What the engine reported after decoder initialisation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeObservation {
    pub active: String,
    pub pixel_format: String,
    pub path: DecodePath,
}

impl DecodeObservation {
    pub fn new(active: impl Into<String>, pixel_format: impl Into<String>) -> Self {
        let active = active.into();
        let pixel_format = pixel_format.into();
        let path = classify(&active, &pixel_format);
        Self {
            active,
            pixel_format,
            path,
        }
    }

    /// User-facing description of the *observed* path.
    pub fn describe(&self) -> String {
        match self.path {
            DecodePath::DirectHw => {
                format!("Hardware decoding — Direct ({})", self.active)
            }
            DecodePath::HwCopyBack => format!("Hardware decoding — Copy-back ({})", self.active),
            DecodePath::Software => "Software decoding".to_string(),
            DecodePath::Inactive => "Hardware acceleration inactive".to_string(),
            DecodePath::Unknown => format!(
                "Decoder state unknown (hwdec '{}', format '{}')",
                self.active, self.pixel_format
            ),
        }
    }
}

/// Pixel formats that indicate the application is receiving a hardware surface
/// handle rather than decoded pixels.
fn is_hw_pixel_format(pixel_format: &str) -> bool {
    matches!(
        pixel_format,
        "vaapi" | "d3d11" | "cuda" | "videotoolbox" | "drm_prime" | "vulkan" | "mediacodec"
    )
}

/// Decoder names that deliver GPU surfaces without a host round trip.
fn is_direct_decoder(active: &str) -> bool {
    matches!(
        active,
        "vaapi" | "d3d11va" | "videotoolbox" | "nvdec" | "cuda" | "vdpau" | "vulkan" | "mediacodec"
    )
}

/// Classifies the observed decode path.
///
/// `hwdec-current` is authoritative for the decoder; the pixel format is used as
/// a cross-check because a hardware decoder name with a software pixel format
/// means the surface was copied back or the report is stale.
pub fn classify(active: &str, pixel_format: &str) -> DecodePath {
    let active = active.trim();
    let pixel_format = pixel_format.trim();

    if active.is_empty() || active == "no" || active == "inactive" {
        return if pixel_format.is_empty() || is_hw_pixel_format(pixel_format) {
            // Nothing decoded yet.
            if pixel_format.is_empty() {
                DecodePath::Inactive
            } else {
                DecodePath::Unknown
            }
        } else {
            DecodePath::Software
        };
    }

    if active.ends_with("-copy") {
        return DecodePath::HwCopyBack;
    }

    if is_direct_decoder(active) {
        // A direct decoder is only confirmed when the frame is still a hardware
        // surface; a plain planar format means data already came back to RAM.
        if pixel_format.is_empty() || is_hw_pixel_format(pixel_format) {
            return DecodePath::DirectHw;
        }
        return DecodePath::Unknown;
    }

    DecodePath::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_vaapi_is_direct() {
        assert_eq!(classify("vaapi", "vaapi"), DecodePath::DirectHw);
    }

    #[test]
    fn copy_back_suffix_is_copy_back() {
        assert_eq!(classify("vaapi-copy", "nv12"), DecodePath::HwCopyBack);
        assert_eq!(classify("d3d11va-copy", "p010"), DecodePath::HwCopyBack);
        assert_eq!(
            classify("videotoolbox-copy", "nv12"),
            DecodePath::HwCopyBack
        );
    }

    #[test]
    fn software_paths() {
        assert_eq!(classify("", ""), DecodePath::Inactive);
        assert_eq!(classify("no", "yuv420p10"), DecodePath::Software);
        assert_eq!(classify("", "yuv420p"), DecodePath::Software);
    }

    #[test]
    fn direct_decoder_with_software_format_is_not_claimed_as_direct() {
        assert_eq!(classify("vaapi", "nv12"), DecodePath::Unknown);
    }

    #[test]
    fn unknown_decoder_is_unknown() {
        assert_eq!(classify("something-new", "vaapi"), DecodePath::Unknown);
    }

    #[test]
    fn linux_policy_uses_direct_only_with_display() {
        let with = DecodePolicy::for_platform(PlatformFamily::Linux, true);
        assert_eq!(with.initial_mode(), "auto-safe");
        assert!(with.candidates().contains(&"vaapi"));
        let without = DecodePolicy::for_platform(PlatformFamily::Linux, false);
        assert!(!without.candidates().contains(&"vaapi"));
        assert!(without.candidates().contains(&"vaapi-copy"));
    }

    #[test]
    fn windows_policy_never_requests_vaapi() {
        let p = DecodePolicy::for_platform(PlatformFamily::Windows, true);
        assert!(p.candidates().iter().all(|c| !c.contains("vaapi")));
        assert!(p.candidates().contains(&"d3d11va"));
    }

    #[test]
    fn macos_policy_uses_videotoolbox() {
        let p = DecodePolicy::for_platform(PlatformFamily::MacOs, false);
        assert!(p.candidates().contains(&"videotoolbox"));
        assert!(p.candidates().iter().all(|c| !c.contains("vaapi")));
    }

    #[test]
    fn observation_describes_observed_not_requested() {
        let obs = DecodeObservation::new("", "yuv420p10");
        assert!(obs.describe().contains("Software"));
        assert_eq!(obs.path, DecodePath::Software);
    }
}
