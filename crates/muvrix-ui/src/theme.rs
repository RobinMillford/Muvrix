//! Custom UI theme builder, color palette token management, and JSON theme import/export.

use eframe::egui::{self, Color32, Stroke};
use serde::{Deserialize, Serialize};

/// User-customizable UI theme palette.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomTheme {
    pub name: String,
    pub description: String,
    pub author: String,
    pub background: [u8; 3],     // Obsidian / Window background (#080D12)
    pub secondary_bg: [u8; 3],   // Sidebar / Drawer background (#111A22)
    pub surface: [u8; 3],        // Card surface (#18212B)
    pub elevated: [u8; 3],       // Elevated panels (#202B36)
    pub border_subtle: [u8; 3],  // Subtle borders (#293540)
    pub border_strong: [u8; 3],  // Structural bezel borders (#34434D)
    pub text_primary: [u8; 3],   // Ivory text (#F4F5F0)
    pub text_secondary: [u8; 3], // Muted secondary text (#A5AFB9)
    pub accent: [u8; 3],         // Electric Lime / Interactive accent (#C5F36A)
}

impl Default for CustomTheme {
    fn default() -> Self {
        Self::obsidian_lime()
    }
}

impl CustomTheme {
    /// Canonical signature Obsidian & Electric Lime theme.
    pub fn obsidian_lime() -> Self {
        Self {
            name: "Obsidian Lime (Signature)".to_string(),
            description: "Signature MUVRIX high-contrast obsidian slate with electric lime accents"
                .to_string(),
            author: "MUVRIX Design System".to_string(),
            background: [8, 13, 18],
            secondary_bg: [17, 26, 34],
            surface: [24, 33, 43],
            elevated: [32, 43, 54],
            border_subtle: [41, 53, 64],
            border_strong: [52, 67, 77],
            text_primary: [244, 245, 240],
            text_secondary: [165, 175, 185],
            accent: [197, 243, 106],
        }
    }

    /// Cyberpunk Neon theme with vivid cyan and dark violet.
    pub fn cyberpunk_neon() -> Self {
        Self {
            name: "Cyberpunk Neon".to_string(),
            description: "Vivid neon cyan and deep violet synthetic aesthetic".to_string(),
            author: "MUVRIX Community".to_string(),
            background: [10, 10, 18],
            secondary_bg: [18, 18, 30],
            surface: [26, 26, 44],
            elevated: [36, 36, 60],
            border_subtle: [50, 48, 80],
            border_strong: [72, 70, 115],
            text_primary: [240, 245, 255],
            text_secondary: [150, 160, 195],
            accent: [0, 240, 255],
        }
    }

    /// Nordic Frost theme with cool icy slate and arctic blue accents.
    pub fn nordic_frost() -> Self {
        Self {
            name: "Nordic Frost".to_string(),
            description: "Cool arctic slate with ice-blue indicators and crisp white typography"
                .to_string(),
            author: "MUVRIX Community".to_string(),
            background: [16, 21, 30],
            secondary_bg: [23, 30, 42],
            surface: [32, 42, 58],
            elevated: [43, 55, 75],
            border_subtle: [55, 69, 92],
            border_strong: [72, 88, 115],
            text_primary: [245, 248, 255],
            text_secondary: [160, 180, 205],
            accent: [100, 210, 255],
        }
    }

    /// Solarized Midnight theme with warm amber gold accents.
    pub fn solarized_midnight() -> Self {
        Self {
            name: "Solarized Midnight".to_string(),
            description: "Deep oceanic midnight with warm amber gold highlights".to_string(),
            author: "MUVRIX Community".to_string(),
            background: [7, 20, 30],
            secondary_bg: [12, 30, 44],
            surface: [18, 42, 60],
            elevated: [25, 55, 78],
            border_subtle: [38, 70, 96],
            border_strong: [52, 90, 120],
            text_primary: [253, 246, 227],
            text_secondary: [147, 161, 161],
            accent: [255, 184, 48],
        }
    }

    /// Built-in preset themes collection.
    pub fn presets() -> Vec<Self> {
        vec![
            Self::obsidian_lime(),
            Self::cyberpunk_neon(),
            Self::nordic_frost(),
            Self::solarized_midnight(),
        ]
    }

    /// Exports theme definition as pretty-printed JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Imports theme from JSON string with schema validation.
    pub fn from_json(json_str: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json_str)
    }

    // Helper Color32 accessors
    pub fn bg_color(&self) -> Color32 {
        Color32::from_rgb(self.background[0], self.background[1], self.background[2])
    }

    pub fn secondary_bg_color(&self) -> Color32 {
        Color32::from_rgb(
            self.secondary_bg[0],
            self.secondary_bg[1],
            self.secondary_bg[2],
        )
    }

    pub fn surface_color(&self) -> Color32 {
        Color32::from_rgb(self.surface[0], self.surface[1], self.surface[2])
    }

    pub fn elevated_color(&self) -> Color32 {
        Color32::from_rgb(self.elevated[0], self.elevated[1], self.elevated[2])
    }

    pub fn border_subtle_color(&self) -> Color32 {
        Color32::from_rgb(
            self.border_subtle[0],
            self.border_subtle[1],
            self.border_subtle[2],
        )
    }

    pub fn border_strong_color(&self) -> Color32 {
        Color32::from_rgb(
            self.border_strong[0],
            self.border_strong[1],
            self.border_strong[2],
        )
    }

    pub fn text_primary_color(&self) -> Color32 {
        Color32::from_rgb(
            self.text_primary[0],
            self.text_primary[1],
            self.text_primary[2],
        )
    }

    pub fn text_secondary_color(&self) -> Color32 {
        Color32::from_rgb(
            self.text_secondary[0],
            self.text_secondary[1],
            self.text_secondary[2],
        )
    }

    pub fn accent_color(&self) -> Color32 {
        Color32::from_rgb(self.accent[0], self.accent[1], self.accent[2])
    }

    /// Applies custom theme colors directly to egui's runtime `Visuals`.
    pub fn apply_to_visuals(&self, visuals: &mut egui::Visuals) {
        visuals.panel_fill = self.bg_color();
        visuals.window_fill = self.bg_color();
        visuals.faint_bg_color = self.surface_color();
        visuals.window_stroke = Stroke::new(1.0_f32, self.border_strong_color());

        visuals.widgets.noninteractive.bg_fill = self.surface_color();
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, self.border_subtle_color());
        visuals.widgets.noninteractive.fg_stroke =
            Stroke::new(1.0_f32, self.text_secondary_color());

        visuals.widgets.inactive.bg_fill = self.surface_color();
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, self.border_strong_color());
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, self.text_primary_color());

        visuals.widgets.hovered.bg_fill = self.elevated_color();
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, self.accent_color());
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, self.text_primary_color());

        visuals.widgets.active.bg_fill = self.accent_color();
        visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, self.accent_color());
        visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, self.bg_color());

        visuals.selection.bg_fill = self.accent_color();
        visuals.selection.stroke = Stroke::new(1.0_f32, self.accent_color());
    }

    /// Convenience method to apply theme directly to the egui Context style.
    pub fn apply_to_context(&self, ctx: &egui::Context) {
        let mut visuals = ctx.style().visuals.clone();
        self.apply_to_visuals(&mut visuals);
        ctx.set_visuals(visuals);
    }

    /// Configuration file path for persisting user custom theme.
    pub fn config_path() -> std::path::PathBuf {
        crate::config_editor::config_dir().join("theme.json")
    }

    /// Loads custom theme from disk, or returns None if missing/invalid.
    pub fn load_from_disk() -> Option<Self> {
        let path = Self::config_path();
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(theme) = Self::from_json(&content) {
                    return Some(theme);
                }
            }
        }
        None
    }

    /// Saves theme to disk in the Muvrix config directory.
    pub fn save_to_disk(&self) -> std::io::Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = self
            .to_json()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_custom_theme_json_serialization() {
        let theme = CustomTheme {
            name: "Sunset Synth".to_string(),
            description: "Deep orange and magenta gradient".to_string(),
            author: "Aesthetic Engineer".to_string(),
            background: [15, 10, 20],
            secondary_bg: [25, 18, 35],
            surface: [38, 25, 52],
            elevated: [50, 35, 70],
            border_subtle: [65, 45, 90],
            border_strong: [85, 60, 115],
            text_primary: [255, 245, 235],
            text_secondary: [195, 175, 185],
            accent: [255, 110, 60],
        };

        // 1. Serialize to JSON
        let json = theme.to_json().expect("Serialize custom theme to JSON");
        assert!(json.contains("Sunset Synth"));
        assert!(json.contains("255,"));

        // 2. Deserialize from JSON
        let deserialized = CustomTheme::from_json(&json).expect("Deserialize custom theme");
        assert_eq!(theme, deserialized);
        assert_eq!(deserialized.accent_color(), Color32::from_rgb(255, 110, 60));

        // 3. Invalid JSON error handling
        let invalid = CustomTheme::from_json("{ invalid json format");
        assert!(invalid.is_err());

        // 4. Missing required field handling
        let missing_accent = r#"{
            "name": "Broken Theme",
            "description": "Missing accent field",
            "author": "Tester",
            "background": [0, 0, 0],
            "secondary_bg": [10, 10, 10],
            "surface": [20, 20, 20],
            "elevated": [30, 30, 30],
            "border_subtle": [40, 40, 40],
            "border_strong": [50, 50, 50],
            "text_primary": [255, 255, 255],
            "text_secondary": [128, 128, 128]
        }"#;
        assert!(CustomTheme::from_json(missing_accent).is_err());

        // 5. Out-of-range RGB value rejection (> 255)
        let out_of_range = r#"{
            "name": "Invalid RGB Theme",
            "description": "Color value out of range",
            "author": "Tester",
            "background": [300, 0, 0],
            "secondary_bg": [10, 10, 10],
            "surface": [20, 20, 20],
            "elevated": [30, 30, 30],
            "border_subtle": [40, 40, 40],
            "border_strong": [50, 50, 50],
            "text_primary": [255, 255, 255],
            "text_secondary": [128, 128, 128],
            "accent": [0, 255, 0]
        }"#;
        assert!(CustomTheme::from_json(out_of_range).is_err());
    }

    #[test]
    fn test_custom_theme_presets_integrity() {
        let presets = CustomTheme::presets();
        assert_eq!(presets.len(), 4);
        for p in presets {
            assert!(!p.name.is_empty());
            assert!(!p.description.is_empty());
            let json = p.to_json().unwrap();
            let roundtrip = CustomTheme::from_json(&json).unwrap();
            assert_eq!(p, roundtrip);
        }
    }
}
