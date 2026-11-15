//! Video shader manager, Anime4K v4.0.1 preset chaining, hardware picture adjustments,
//! geometry framing, and libplacebo HDR-to-SDR tone mapping.

use crate::error::MpvError;
use crate::handle::MpvHandle;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Shader quality levels for Anime4K v4.0.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ShaderQuality {
    Fast, // S - Optimized for integrated graphics
    #[default]
    Balanced, // M - Default balance of fidelity and performance
    High, // L - Maximum CNN layer depth
}

impl ShaderQuality {
    pub fn suffix(&self) -> &'static str {
        match self {
            Self::Fast => "S",
            Self::Balanced => "M",
            Self::High => "L",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Fast => "Fast (S)",
            Self::Balanced => "Balanced (M)",
            Self::High => "High Quality (L)",
        }
    }
}

/// Anime4K v4.0.1 preset modes according to authoritative upstream reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Anime4KPreset {
    #[default]
    Disabled,
    ModeA,  // Fast Line Restoration & Upscale
    ModeB,  // Soft Line Restoration & Upscale
    ModeC,  // Denoise & Upscale
    ModeAA, // Deep Restoration (Mode A+A)
    ModeBB, // Soft Deep Restoration (Mode B+B)
    ModeCA, // Denoise + Restoration (Mode C+A)
}

impl Anime4KPreset {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Disabled => "Disabled",
            Self::ModeA => "Mode A (Sharp Line Restoration & Upscale)",
            Self::ModeB => "Mode B (Soft Line Restoration & Upscale)",
            Self::ModeC => "Mode C (Denoise & Upscale)",
            Self::ModeAA => "Mode A+A (Deep Line Restoration)",
            Self::ModeBB => "Mode B+B (Soft Deep Restoration)",
            Self::ModeCA => "Mode C+A (Denoise + Line Restoration)",
        }
    }

    pub fn short_name(&self) -> &'static str {
        match self {
            Self::Disabled => "Off",
            Self::ModeA => "Mode A",
            Self::ModeB => "Mode B",
            Self::ModeC => "Mode C",
            Self::ModeAA => "Mode A+A",
            Self::ModeBB => "Mode B+B",
            Self::ModeCA => "Mode C+A",
        }
    }
}

/// libplacebo tone-mapping algorithms supported by libmpv.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ToneMappingAlgorithm {
    #[default]
    Auto,
    Clip,
    Mobius,
    Reinhard,
    Hable,
    Gamma,
    Linear,
    Spline,
    Bt2390,
    Bt2446a,
    St2094_40,
    St2094_10,
}

impl ToneMappingAlgorithm {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Clip => "clip",
            Self::Mobius => "mobius",
            Self::Reinhard => "reinhard",
            Self::Hable => "hable",
            Self::Gamma => "gamma",
            Self::Linear => "linear",
            Self::Spline => "spline",
            Self::Bt2390 => "bt.2390",
            Self::Bt2446a => "bt.2446a",
            Self::St2094_40 => "st2094-40",
            Self::St2094_10 => "st2094-10",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Auto => "Auto (Engine Default)",
            Self::Clip => "Clip (Hard Knee)",
            Self::Mobius => "Mobius (Smooth Knee)",
            Self::Reinhard => "Reinhard (Classic Photographic)",
            Self::Hable => "Hable (Uncharted 2 Filmic)",
            Self::Gamma => "Gamma (Simple Power Law)",
            Self::Linear => "Linear (Exposure Scaling)",
            Self::Spline => "Spline (Smooth Hermite)",
            Self::Bt2390 => "ITU-R BT.2390 (EETF Recommended)",
            Self::Bt2446a => "ITU-R BT.2446 Method A",
            Self::St2094_40 => "SMPTE ST 2094-40 (Dynamic)",
            Self::St2094_10 => "SMPTE ST 2094-10 (Dynamic)",
        }
    }
}

/// Target Transfer Characteristics (EOTF) for color grading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TargetTrc {
    #[default]
    Auto,
    Srgb,
    Bt1886,
    Linear,
    Gamma22,
    Gamma24,
    Pq,
    Hlg,
}

impl TargetTrc {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Srgb => "srgb",
            Self::Bt1886 => "bt.1886",
            Self::Linear => "linear",
            Self::Gamma22 => "gamma2.2",
            Self::Gamma24 => "gamma2.4",
            Self::Pq => "pq",
            Self::Hlg => "hlg",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Auto => "Auto (Display Standard)",
            Self::Srgb => "sRGB (Standard Desktop)",
            Self::Bt1886 => "BT.1886 (Rec.709 Video)",
            Self::Linear => "Linear Light",
            Self::Gamma22 => "Gamma 2.2",
            Self::Gamma24 => "Gamma 2.4 (Dark Room)",
            Self::Pq => "PQ / SMPTE ST 2084",
            Self::Hlg => "HLG (Hybrid Log-Gamma)",
        }
    }
}

/// Target color primaries for gamut mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TargetPrimaries {
    #[default]
    Auto,
    Bt709,
    Bt2020,
    DciP3,
    DisplayP3,
    AdobeRgb,
}

impl TargetPrimaries {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Bt709 => "bt.709",
            Self::Bt2020 => "bt.2020",
            Self::DciP3 => "dci-p3",
            Self::DisplayP3 => "display-p3",
            Self::AdobeRgb => "adobe",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Auto => "Auto (Match Display)",
            Self::Bt709 => "BT.709 (sRGB / HDTV)",
            Self::Bt2020 => "BT.2020 (Wide Gamut HDR)",
            Self::DciP3 => "DCI-P3 (Digital Cinema)",
            Self::DisplayP3 => "Display P3 (Apple Wide Gamut)",
            Self::AdobeRgb => "Adobe RGB",
        }
    }
}

/// Gamut mapping mode in libplacebo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GamutMappingMode {
    #[default]
    Auto,
    Clip,
    Perceptual,
    Relative,
    Saturation,
    Absolute,
    Desaturate,
    Darken,
    Linear,
}

impl GamutMappingMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Clip => "clip",
            Self::Perceptual => "perceptual",
            Self::Relative => "relative",
            Self::Saturation => "saturation",
            Self::Absolute => "absolute",
            Self::Desaturate => "desaturate",
            Self::Darken => "darken",
            Self::Linear => "linear",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Auto => "Auto (Recommended)",
            Self::Clip => "Clip Out-of-Gamut",
            Self::Perceptual => "Perceptual Intent",
            Self::Relative => "Relative Colorimetric",
            Self::Saturation => "Saturation Preserving",
            Self::Absolute => "Absolute Colorimetric",
            Self::Desaturate => "Desaturate Highlights",
            Self::Darken => "Darken Gamut Mapping",
            Self::Linear => "Linear Gamut Compression",
        }
    }
}

/// Picture adjustment settings matching native mpv fragment shader and libplacebo properties.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoAdjustmentSettings {
    // Hardware color adjustments
    pub brightness: i32, // -100..100, default 0
    pub contrast: i32,   // -100..100, default 0
    pub saturation: i32, // -100..100, default 0
    pub gamma: i32,      // -100..100, default 0
    pub hue: i32,        // -100..100, default 0
    pub sharpen: f64,    // -5.0..5.0, default 0.0
    pub deband: bool,    // default false

    // Video geometry and framing
    pub aspect_ratio: String, // "auto" (-1), "16:9", "4:3", "2.35:1", "1:1"
    pub zoom: f64,            // -3.0..3.0, default 0.0
    pub pan_x: f64,           // -1.0..1.0, default 0.0
    pub pan_y: f64,           // -1.0..1.0, default 0.0
    pub rotate: i64,          // 0, 90, 180, 270

    // libplacebo HDR tone mapping and gamut options
    pub tone_mapping: ToneMappingAlgorithm,
    pub target_trc: TargetTrc,
    pub target_prim: TargetPrimaries,
    pub gamut_mapping: GamutMappingMode,
    pub hdr_compute_peak: bool,
    pub tone_mapping_max_boost: f64,

    pub active_shaders: Vec<String>,
}

impl Default for VideoAdjustmentSettings {
    fn default() -> Self {
        Self {
            brightness: 0,
            contrast: 0,
            saturation: 0,
            gamma: 0,
            hue: 0,
            sharpen: 0.0,
            deband: false,
            aspect_ratio: "auto".to_string(),
            zoom: 0.0,
            pan_x: 0.0,
            pan_y: 0.0,
            rotate: 0,
            tone_mapping: ToneMappingAlgorithm::Auto,
            target_trc: TargetTrc::Auto,
            target_prim: TargetPrimaries::Auto,
            gamut_mapping: GamutMappingMode::Auto,
            hdr_compute_peak: true,
            tone_mapping_max_boost: 1.0,
            active_shaders: Vec::new(),
        }
    }
}

/// Shader profile configuration describing an active enhancement pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShaderProfile {
    pub id: String,
    pub display_name: String,
    pub mode: Anime4KPreset,
    pub quality: ShaderQuality,
    pub is_enabled: bool,
    pub shaders: Vec<String>,
}

/// Manager for video enhancement properties, Anime4K v4.0.1 shader profiles, and libplacebo tone mapping.
pub struct ShaderManager;

impl ShaderManager {
    /// Applies video picture adjustments and geometry framing to an mpv instance.
    pub fn apply_adjustments(
        handle: &MpvHandle,
        settings: &VideoAdjustmentSettings,
    ) -> Result<(), MpvError> {
        let b = settings.brightness.clamp(-100, 100) as i64;
        let c = settings.contrast.clamp(-100, 100) as i64;
        let s = settings.saturation.clamp(-100, 100) as i64;
        let g = settings.gamma.clamp(-100, 100) as i64;
        let h = settings.hue.clamp(-100, 100) as i64;
        let sh = settings.sharpen.clamp(-5.0, 5.0);

        handle.set_property("brightness", b)?;
        handle.set_property("contrast", c)?;
        handle.set_property("saturation", s)?;
        handle.set_property("gamma", g)?;
        handle.set_property("hue", h)?;
        handle.set_property("sharpen", sh)?;
        handle.set_property("deband", settings.deband)?;

        // Video geometry & aspect ratio
        let aspect_val = if settings.aspect_ratio.is_empty() || settings.aspect_ratio == "auto" {
            "-1"
        } else {
            &settings.aspect_ratio
        };
        let _ = handle.set_property_string("video-aspect-override", aspect_val);
        let _ = handle.set_property("video-zoom", settings.zoom.clamp(-3.0, 3.0));
        let _ = handle.set_property("video-pan-x", settings.pan_x.clamp(-1.0, 1.0));
        let _ = handle.set_property("video-pan-y", settings.pan_y.clamp(-1.0, 1.0));
        let _ = handle.set_property("video-rotate", settings.rotate % 360);

        // libplacebo HDR tone-mapping & color gamut properties.
        //
        // `Auto` deliberately leaves the engine target untouched. The shell
        // owns the output target: it sets PQ/BT.2020 after the compositor
        // accepts a 10-bit HDR description. Writing `auto` here (the default
        // `VideoAdjustmentSettings` value) used to overwrite that target after
        // startup. Auto means "do not fight the output target", not "reset it".
        let _ = handle.set_property_string("tone-mapping", settings.tone_mapping.as_str());
        if settings.target_trc != TargetTrc::Auto {
            let _ = handle.set_property_string("target-trc", settings.target_trc.as_str());
        }
        if settings.target_prim != TargetPrimaries::Auto {
            let _ = handle.set_property_string("target-prim", settings.target_prim.as_str());
        }
        let _ = handle.set_property_string("gamut-mapping-mode", settings.gamut_mapping.as_str());
        let _ = handle.set_property_string(
            "hdr-compute-peak",
            if settings.hdr_compute_peak {
                "auto"
            } else {
                "no"
            },
        );
        let _ = handle.set_property(
            "tone-mapping-max-boost",
            settings.tone_mapping_max_boost.clamp(1.0, 10.0),
        );

        Ok(())
    }

    /// Re-applies the shell-owned output colour target.
    ///
    /// The target belongs to the presentation surface, not to the user's
    /// picture-adjustment settings. Calling this after applying adjustments
    /// keeps a compositor-accepted PQ/BT.2020 target intact. `Auto` in
    /// [`VideoAdjustmentSettings`] intentionally does not write a target.
    pub fn apply_output_target(
        handle: &MpvHandle,
        native_hdr: bool,
        peak_nits: Option<u32>,
    ) -> Result<(), MpvError> {
        if native_hdr {
            handle.set_property_string("target-trc", "pq")?;
            handle.set_property_string("target-prim", "bt.2020")?;
            if let Some(peak) = peak_nits {
                handle.set_property_string("target-peak", &peak.to_string())?;
            }
        } else {
            handle.set_property_string("target-trc", "auto")?;
            handle.set_property_string("target-prim", "auto")?;
        }
        Ok(())
    }

    /// Resets all picture adjustments to flat defaults (0).
    ///
    /// Output target properties are left alone for the same reason as in
    /// [`ShaderManager::apply_adjustments`].
    pub fn reset_adjustments(handle: &MpvHandle) -> Result<(), MpvError> {
        Self::apply_adjustments(handle, &VideoAdjustmentSettings::default())
    }

    /// Appends a custom user shader to the active GLSL shader list with path-traversal validation.
    pub fn append_shader(handle: &MpvHandle, shader_path: &str) -> Result<(), MpvError> {
        Self::validate_shader_path(shader_path)?;
        handle.command(&["change-list", "glsl-shaders", "append", shader_path])
    }

    /// Clears all active user shaders.
    pub fn clear_shaders(handle: &MpvHandle) -> Result<(), MpvError> {
        handle.command(&["change-list", "glsl-shaders", "clr", ""])
    }

    /// Returns the target platform's list separator for mpv option strings.
    /// On Windows, mpv expects semicolons (';') to avoid collisions with drive letter colons (e.g. 'C:\').
    /// On Unix platforms (Linux, macOS), mpv expects colons (':').
    pub fn platform_shader_separator() -> &'static str {
        if cfg!(windows) {
            ";"
        } else {
            ":"
        }
    }

    /// Serializes a slice of shader paths for the target OS environment.
    /// Uses ';' for Windows (protecting drive letter colons like 'C:\') and ':' for Unix/macOS.
    pub fn serialize_shader_paths_for_target(paths: &[&str], is_windows: bool) -> String {
        let sep = if is_windows { ";" } else { ":" };
        paths.join(sep)
    }

    /// Validates that every shader path in the given slice is unique.
    /// Rejects chains with duplicated shader files.
    pub fn validate_shader_chain_uniqueness<T: AsRef<str>>(paths: &[T]) -> Result<(), MpvError> {
        let mut seen = std::collections::HashSet::new();
        for p in paths {
            let s = p.as_ref();
            if !seen.insert(s) {
                return Err(MpvError::InvalidParameter);
            }
        }
        Ok(())
    }

    /// Sets shaders on an mpv instance using the native `change-list` list operation.
    /// This bypasses path delimiter ambiguities entirely by passing each path as an isolated argument,
    /// robustly supporting Windows drive letters, spaces in paths, and multi-file chains.
    pub fn set_shaders_via_change_list(
        handle: &MpvHandle,
        shader_paths: &[&str],
    ) -> Result<(), MpvError> {
        Self::validate_shader_chain_uniqueness(shader_paths)?;
        for path in shader_paths {
            Self::validate_shader_path(path)?;
        }

        // 1. Clear existing shaders
        handle.command(&["change-list", "glsl-shaders", "clr", ""])?;

        // 2. Append each shader path individually
        for path in shader_paths {
            handle.command(&["change-list", "glsl-shaders", "append", path])?;
        }

        Ok(())
    }

    /// Sets the active shader set.
    pub fn set_shaders(handle: &MpvHandle, shader_paths: &[&str]) -> Result<(), MpvError> {
        Self::set_shaders_via_change_list(handle, shader_paths)
    }

    /// Validates shader file path to prevent path traversal, non-existent files, and unsupported extensions.
    /// Canonicalizes the path to resolve symlinks and ensure the file is an existing regular file.
    pub fn validate_shader_path(path: &str) -> Result<PathBuf, MpvError> {
        if path.is_empty() || path.contains('\0') || path.contains('\n') || path.contains('\r') {
            return Err(MpvError::InvalidParameter);
        }
        let p = Path::new(path);
        // Extension must be .glsl or .hook
        match p.extension().and_then(|ext| ext.to_str()) {
            Some("glsl") | Some("hook") => {}
            _ => return Err(MpvError::InvalidParameter),
        }

        let canonical = p.canonicalize().map_err(|_| MpvError::InvalidParameter)?;
        if !canonical.is_file() {
            return Err(MpvError::InvalidParameter);
        }
        Ok(canonical)
    }

    /// Validates that a shader path exists, has a valid extension, and resolves strictly inside the allowed directory.
    /// Canonicalizes both the path and the directory to prevent symlink traversal and relative path escapes.
    pub fn validate_shader_confined_to_dir(
        path: &str,
        allowed_dir: &Path,
    ) -> Result<PathBuf, MpvError> {
        let canonical_shader = Self::validate_shader_path(path)?;
        let canonical_dir = allowed_dir
            .canonicalize()
            .map_err(|_| MpvError::InvalidParameter)?;
        if !canonical_shader.starts_with(&canonical_dir) {
            return Err(MpvError::SecurityViolation(format!(
                "Shader path '{}' escapes allowed directory '{}'",
                path,
                allowed_dir.display()
            )));
        }
        Ok(canonical_shader)
    }

    /// Generates the standard Anime4K v4.0.1 shader filenames for a preset mode with specified quality.
    /// Every shader file path within a generated chain is strictly unique.
    pub fn get_anime4k_shader_names_with_quality(
        preset: Anime4KPreset,
        quality: ShaderQuality,
    ) -> Vec<String> {
        let q = quality.suffix();
        match preset {
            Anime4KPreset::Disabled => vec![],
            Anime4KPreset::ModeA => vec![
                "Anime4K_Clamp_Highlights.glsl".to_string(),
                format!("Anime4K_Restore_CNN_{q}.glsl"),
                format!("Anime4K_Upscale_CNN_x2_{q}.glsl"),
                "Anime4K_AutoDownscalePre_x2.glsl".to_string(),
                format!("Anime4K_Upscale_CNN_x2_Pass2_{q}.glsl"),
            ],
            Anime4KPreset::ModeB => vec![
                "Anime4K_Clamp_Highlights.glsl".to_string(),
                format!("Anime4K_Restore_CNN_Soft_{q}.glsl"),
                format!("Anime4K_Upscale_CNN_x2_{q}.glsl"),
                "Anime4K_AutoDownscalePre_x2.glsl".to_string(),
                format!("Anime4K_Upscale_CNN_x2_Pass2_{q}.glsl"),
            ],
            Anime4KPreset::ModeC => vec![
                "Anime4K_Clamp_Highlights.glsl".to_string(),
                format!("Anime4K_Upscale_Denoise_CNN_x2_{q}.glsl"),
                "Anime4K_AutoDownscalePre_x2.glsl".to_string(),
                format!("Anime4K_Upscale_CNN_x2_{q}.glsl"),
            ],
            Anime4KPreset::ModeAA => vec![
                "Anime4K_Clamp_Highlights.glsl".to_string(),
                format!("Anime4K_Restore_CNN_{q}.glsl"),
                format!("Anime4K_Upscale_CNN_x2_{q}.glsl"),
                "Anime4K_AutoDownscalePre_x2.glsl".to_string(),
                format!("Anime4K_Restore_CNN_Pass2_{q}.glsl"),
                format!("Anime4K_Upscale_CNN_x2_Pass2_{q}.glsl"),
            ],
            Anime4KPreset::ModeBB => vec![
                "Anime4K_Clamp_Highlights.glsl".to_string(),
                format!("Anime4K_Restore_CNN_Soft_{q}.glsl"),
                format!("Anime4K_Upscale_CNN_x2_{q}.glsl"),
                "Anime4K_AutoDownscalePre_x2.glsl".to_string(),
                format!("Anime4K_Restore_CNN_Soft_Pass2_{q}.glsl"),
                format!("Anime4K_Upscale_CNN_x2_Pass2_{q}.glsl"),
            ],
            Anime4KPreset::ModeCA => vec![
                "Anime4K_Clamp_Highlights.glsl".to_string(),
                format!("Anime4K_Upscale_Denoise_CNN_x2_{q}.glsl"),
                "Anime4K_AutoDownscalePre_x2.glsl".to_string(),
                format!("Anime4K_Restore_CNN_{q}.glsl"),
                format!("Anime4K_Upscale_CNN_x2_{q}.glsl"),
            ],
        }
    }

    /// Compatibility helper for existing code defaulting to Balanced (M) quality.
    /// Every shader file path within a generated chain is strictly unique.
    pub fn get_anime4k_shader_names(preset: Anime4KPreset) -> Vec<&'static str> {
        match preset {
            Anime4KPreset::Disabled => vec![],
            Anime4KPreset::ModeA => vec![
                "Anime4K_Clamp_Highlights.glsl",
                "Anime4K_Restore_CNN_M.glsl",
                "Anime4K_Upscale_CNN_x2_M.glsl",
                "Anime4K_AutoDownscalePre_x2.glsl",
                "Anime4K_Upscale_CNN_x2_Pass2_M.glsl",
            ],
            Anime4KPreset::ModeB => vec![
                "Anime4K_Clamp_Highlights.glsl",
                "Anime4K_Restore_CNN_Soft_M.glsl",
                "Anime4K_Upscale_CNN_x2_M.glsl",
                "Anime4K_AutoDownscalePre_x2.glsl",
                "Anime4K_Upscale_CNN_x2_Pass2_M.glsl",
            ],
            Anime4KPreset::ModeC => vec![
                "Anime4K_Clamp_Highlights.glsl",
                "Anime4K_Upscale_Denoise_CNN_x2_M.glsl",
                "Anime4K_AutoDownscalePre_x2.glsl",
                "Anime4K_Upscale_CNN_x2_M.glsl",
            ],
            Anime4KPreset::ModeAA => vec![
                "Anime4K_Clamp_Highlights.glsl",
                "Anime4K_Restore_CNN_M.glsl",
                "Anime4K_Upscale_CNN_x2_M.glsl",
                "Anime4K_AutoDownscalePre_x2.glsl",
                "Anime4K_Restore_CNN_Pass2_M.glsl",
                "Anime4K_Upscale_CNN_x2_Pass2_M.glsl",
            ],
            Anime4KPreset::ModeBB => vec![
                "Anime4K_Clamp_Highlights.glsl",
                "Anime4K_Restore_CNN_Soft_M.glsl",
                "Anime4K_Upscale_CNN_x2_M.glsl",
                "Anime4K_AutoDownscalePre_x2.glsl",
                "Anime4K_Restore_CNN_Soft_Pass2_M.glsl",
                "Anime4K_Upscale_CNN_x2_Pass2_M.glsl",
            ],
            Anime4KPreset::ModeCA => vec![
                "Anime4K_Clamp_Highlights.glsl",
                "Anime4K_Upscale_Denoise_CNN_x2_M.glsl",
                "Anime4K_AutoDownscalePre_x2.glsl",
                "Anime4K_Restore_CNN_M.glsl",
                "Anime4K_Upscale_CNN_x2_M.glsl",
            ],
        }
    }

    /// Discovers bundled or system Anime4K shader asset directory.
    pub fn resolve_anime4k_shader_dir() -> Option<PathBuf> {
        let candidates = [
            PathBuf::from("assets/shaders/Anime4K/upstream"),
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/shaders/Anime4K/upstream"),
            PathBuf::from("../assets/shaders/Anime4K/upstream"),
            PathBuf::from("/usr/share/muvrix/shaders/Anime4K/upstream"),
        ];

        for dir in &candidates {
            if dir.exists() && dir.is_dir() {
                // Check if clamp highlights exists as a smoke check
                if dir.join("Anime4K_Clamp_Highlights.glsl").exists() {
                    return dir.canonicalize().ok().or_else(|| Some(dir.clone()));
                }
            }
        }

        None
    }

    /// Applies an Anime4K preset mode and quality level to the mpv instance.
    pub fn apply_anime4k_mode(
        handle: &MpvHandle,
        preset: Anime4KPreset,
        quality: ShaderQuality,
        base_dir: Option<&Path>,
    ) -> Result<Vec<String>, MpvError> {
        if preset == Anime4KPreset::Disabled {
            Self::clear_shaders(handle)?;
            return Ok(vec![]);
        }

        let dir = match base_dir {
            Some(d) => d.to_path_buf(),
            None => Self::resolve_anime4k_shader_dir().ok_or(MpvError::Generic)?,
        };

        let names = Self::get_anime4k_shader_names_with_quality(preset, quality);
        let mut paths = Vec::new();
        for name in &names {
            let full_path = dir.join(name);
            let path_str = full_path.to_str().ok_or(MpvError::InvalidParameter)?;
            let canonical = Self::validate_shader_confined_to_dir(path_str, &dir)?;
            paths.push(canonical.to_string_lossy().to_string());
        }

        Self::validate_shader_chain_uniqueness(&paths)?;
        let str_refs: Vec<&str> = paths.iter().map(|s| s.as_str()).collect();
        Self::set_shaders_via_change_list(handle, &str_refs)?;
        Ok(paths)
    }

    /// Applies an Anime4K preset mode with automatic fallback.
    /// If applying the mode fails (e.g. invalid shader, missing file, or security violation),
    /// the method reverts `glsl-shaders` to the previous shader configuration (or disables shaders if none was valid),
    /// safely safeguarding the playback pipeline.
    pub fn apply_anime4k_mode_with_fallback(
        handle: &MpvHandle,
        preset: Anime4KPreset,
        quality: ShaderQuality,
        base_dir: Option<&Path>,
        previous_shaders: &[&str],
    ) -> Result<Vec<String>, MpvError> {
        match Self::apply_anime4k_mode(handle, preset, quality, base_dir) {
            Ok(paths) => Ok(paths),
            Err(e) => {
                if previous_shaders.is_empty() {
                    let _ = Self::clear_shaders(handle);
                } else {
                    let _ = Self::set_shaders(handle, previous_shaders);
                }
                Err(e)
            }
        }
    }

    /// Applies an Anime4K preset with default Balanced quality.
    pub fn apply_anime4k_preset(
        handle: &MpvHandle,
        preset: Anime4KPreset,
        base_dir: &Path,
    ) -> Result<Vec<String>, MpvError> {
        Self::apply_anime4k_mode(handle, preset, ShaderQuality::Balanced, Some(base_dir))
    }
}
