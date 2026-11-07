//! Video shader manager, Anime4K preset chaining, and hardware picture adjustments.

use crate::error::MpvError;
use crate::handle::MpvHandle;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Anime4K v4.0.1 preset modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Anime4KPreset {
    #[default]
    Disabled,
    ModeA,  // Fast Line Restoration & Upscale
    ModeB,  // Soft Line Restoration & Upscale
    ModeC,  // Denoise & Upscale
    ModeAA, // Deep Restoration (Pass 1 + Pass 2)
}

impl Anime4KPreset {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Disabled => "Disabled",
            Self::ModeA => "Mode A (Sharp Upscale)",
            Self::ModeB => "Mode B (Soft Upscale)",
            Self::ModeC => "Mode C (Denoise + Upscale)",
            Self::ModeAA => "Mode A+A (Deep Restoration)",
        }
    }
}

/// Picture adjustment settings matching native mpv fragment shader properties.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoAdjustmentSettings {
    pub brightness: i32, // -100..100, default 0
    pub contrast: i32,   // -100..100, default 0
    pub saturation: i32, // -100..100, default 0
    pub gamma: i32,      // -100..100, default 0
    pub hue: i32,        // -100..100, default 0
    pub sharpen: f64,    // -5.0..5.0, default 0.0
    pub deband: bool,    // default false
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
            active_shaders: Vec::new(),
        }
    }
}

/// Manager for video enhancement properties and custom GLSL shaders.
pub struct ShaderManager;

impl ShaderManager {
    /// Applies video picture adjustments to an mpv instance.
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

        Ok(())
    }

    /// Resets all picture adjustments to flat defaults (0).
    pub fn reset_adjustments(handle: &MpvHandle) -> Result<(), MpvError> {
        Self::apply_adjustments(handle, &VideoAdjustmentSettings::default())
    }

    /// Appends a custom user shader to the active GLSL shader list.
    pub fn append_shader(handle: &MpvHandle, shader_path: &str) -> Result<(), MpvError> {
        handle.command(&["change-list", "glsl-shaders", "append", shader_path])
    }

    /// Clears all active user shaders.
    pub fn clear_shaders(handle: &MpvHandle) -> Result<(), MpvError> {
        handle.command(&["change-list", "glsl-shaders", "clr", ""])
    }

    /// Atomically replaces the active shader set with a colon-delimited list.
    pub fn set_shaders(handle: &MpvHandle, shader_paths: &[&str]) -> Result<(), MpvError> {
        let joined = shader_paths.join(":");
        handle.set_property_string("glsl-shaders", &joined)
    }

    /// Generates the standard Anime4K v4.0.1 relative shader filenames for a preset mode.
    pub fn get_anime4k_shader_names(preset: Anime4KPreset) -> Vec<&'static str> {
        match preset {
            Anime4KPreset::Disabled => vec![],
            Anime4KPreset::ModeA => vec![
                "Anime4K_Clamp_Highlights.glsl",
                "Anime4K_Restore_CNN_M.glsl",
                "Anime4K_Upscale_CNN_x2_M.glsl",
                "Anime4K_AutoDownscalePre_x2.glsl",
                "Anime4K_Upscale_CNN_x2_M.glsl",
            ],
            Anime4KPreset::ModeB => vec![
                "Anime4K_Clamp_Highlights.glsl",
                "Anime4K_Restore_CNN_Soft_M.glsl",
                "Anime4K_Upscale_CNN_x2_M.glsl",
                "Anime4K_AutoDownscalePre_x2.glsl",
                "Anime4K_Upscale_CNN_x2_M.glsl",
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
                "Anime4K_Restore_CNN_M.glsl",
                "Anime4K_Upscale_CNN_x2_M.glsl",
            ],
        }
    }

    /// Applies an Anime4K preset if the shader directory exists.
    pub fn apply_anime4k_preset(
        handle: &MpvHandle,
        preset: Anime4KPreset,
        base_dir: &Path,
    ) -> Result<Vec<String>, MpvError> {
        if preset == Anime4KPreset::Disabled {
            Self::clear_shaders(handle)?;
            return Ok(vec![]);
        }

        let names = Self::get_anime4k_shader_names(preset);
        let mut paths = Vec::new();
        for name in names {
            let full_path = base_dir.join(name);
            paths.push(full_path.to_string_lossy().to_string());
        }

        let str_refs: Vec<&str> = paths.iter().map(|s| s.as_str()).collect();
        Self::set_shaders(handle, &str_refs)?;
        Ok(paths)
    }
}
