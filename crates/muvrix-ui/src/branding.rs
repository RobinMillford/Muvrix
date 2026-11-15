//! Canonical branding, embedded official logo marks, and palette tokens.

use eframe::egui::{self, Color32, Pos2, Rect, Response, Sense, TextureHandle, Vec2};
use std::sync::OnceLock;

/// High-resolution PNG logo embedded into the binary.
pub const LOGO_PNG_BYTES: &[u8] = include_bytes!("../../../assets/branding/muvrix-logo.png");

/// Vector SVG mark embedded into the binary.
pub const LOGO_SVG_BYTES: &[u8] = include_bytes!("../../../assets/branding/muvrix-logo.svg");

/// Canonical color palette extracted from `player_mark.svg`.
#[derive(Debug, Clone, Copy)]
pub struct BrandPalette;

impl BrandPalette {
    /// Circular obsidian disc background (#111A22).
    pub const OBSIDIAN: Color32 = Color32::from_rgb(17, 26, 34);
    /// Stepped elevated panel background (#18232C).
    pub const ELEVATED_SURFACE: Color32 = Color32::from_rgb(24, 35, 44);
    /// Card background surface (#202E39).
    pub const CARD_SURFACE: Color32 = Color32::from_rgb(32, 46, 57);
    /// Muted slate bezel ring stroke (#34434D).
    pub const SLATE_BORDER: Color32 = Color32::from_rgb(52, 67, 77);
    /// Subtle divider line stroke (#22303A).
    pub const SLATE_DIVIDER: Color32 = Color32::from_rgb(34, 48, 58);
    /// Warm cream/ivory monogram M stroke (#F1F0E8).
    pub const IVORY_TEXT: Color32 = Color32::from_rgb(241, 240, 232);
    /// Muted secondary typography (#A0AAB2).
    pub const MUTED_TEXT: Color32 = Color32::from_rgb(160, 170, 178);
    /// Signature electric lime scrubber pin (#C8EE75).
    pub const ELECTRIC_LIME: Color32 = Color32::from_rgb(200, 238, 117);
    /// Electric lime hover/active highlight (#D7F58C).
    pub const ELECTRIC_LIME_HOVER: Color32 = Color32::from_rgb(215, 245, 140);
}

static DECODED_LOGO: OnceLock<(Vec<u8>, u32, u32)> = OnceLock::new();

/// Decodes the embedded 1024x1024 logo PNG into raw RGBA bytes once.
pub fn logo_rgba() -> &'static (Vec<u8>, u32, u32) {
    DECODED_LOGO.get_or_init(|| {
        let img = image::load_from_memory(LOGO_PNG_BYTES)
            .expect("embedded muvrix-logo.png must be a valid PNG")
            .to_rgba8();
        let (width, height) = img.dimensions();
        (img.into_raw(), width, height)
    })
}

/// Creates a `winit::window::Icon` from the embedded brand mark.
pub fn winit_window_icon() -> Option<winit::window::Icon> {
    let (rgba, width, height) = logo_rgba();
    winit::window::Icon::from_rgba(rgba.clone(), *width, *height).ok()
}

/// Creates an `egui::IconData` descriptor for eframe window options.
pub fn egui_icon_data() -> Option<egui::IconData> {
    let (rgba, width, height) = logo_rgba();
    Some(egui::IconData {
        rgba: rgba.clone(),
        width: *width,
        height: *height,
    })
}

/// Paints the official circular Muvrix mark at the requested square dimension.
pub fn render_logo(ui: &mut egui::Ui, texture: &TextureHandle, size: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    ui.painter().image(
        texture.id(),
        rect,
        Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
        Color32::WHITE,
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_logo_decodes_cleanly_with_exact_brand_dimensions() {
        let (rgba, width, height) = logo_rgba();
        assert_eq!(*width, 1024);
        assert_eq!(*height, 1024);
        assert_eq!(rgba.len(), (1024 * 1024 * 4) as usize);

        // Alpha transparency check on corner (0,0)
        assert_eq!(
            rgba[3], 0,
            "corner pixel must be transparent outside circle"
        );
    }

    #[test]
    fn exact_palette_hex_matches_svg_tokens() {
        // Disc: #111A22 -> (17, 26, 34)
        assert_eq!(BrandPalette::OBSIDIAN.r(), 17);
        assert_eq!(BrandPalette::OBSIDIAN.g(), 26);
        assert_eq!(BrandPalette::OBSIDIAN.b(), 34);

        // Slate bezel: #34434D -> (52, 67, 77)
        assert_eq!(BrandPalette::SLATE_BORDER.r(), 52);
        assert_eq!(BrandPalette::SLATE_BORDER.g(), 67);
        assert_eq!(BrandPalette::SLATE_BORDER.b(), 77);

        // Monogram M: #F1F0E8 -> (241, 240, 232)
        assert_eq!(BrandPalette::IVORY_TEXT.r(), 241);
        assert_eq!(BrandPalette::IVORY_TEXT.g(), 240);
        assert_eq!(BrandPalette::IVORY_TEXT.b(), 232);

        // Scrubber Pin: #C8EE75 -> (200, 238, 117)
        assert_eq!(BrandPalette::ELECTRIC_LIME.r(), 200);
        assert_eq!(BrandPalette::ELECTRIC_LIME.g(), 238);
        assert_eq!(BrandPalette::ELECTRIC_LIME.b(), 117);
    }
}
