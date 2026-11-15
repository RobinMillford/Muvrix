//! Canonical branding, embedded official logo marks, and palette tokens.

use eframe::egui::{self, Color32, Pos2, Rect, Response, Sense, TextureHandle, Vec2};
use std::sync::OnceLock;

/// High-resolution PNG logo embedded into the binary.
pub const LOGO_PNG_BYTES: &[u8] = include_bytes!("../../../assets/branding/muvrix-logo.png");

/// Vector SVG mark embedded into the binary.
pub const LOGO_SVG_BYTES: &[u8] = include_bytes!("../../../assets/branding/muvrix-logo.svg");

/// Canonical color palette defining the MUVRIX design system.
#[derive(Debug, Clone, Copy)]
pub struct BrandPalette;

impl BrandPalette {
    /// Obsidian primary background (#080D12).
    pub const OBSIDIAN: Color32 = Color32::from_rgb(8, 13, 18);
    /// Secondary background (#111A22).
    pub const SECONDARY_BG: Color32 = Color32::from_rgb(17, 26, 34);
    /// Slate surfaces (#18212B).
    pub const SLATE_SURFACE: Color32 = Color32::from_rgb(24, 33, 43);
    /// Elevated cards and containers (#202B36).
    pub const ELEVATED_CARD: Color32 = Color32::from_rgb(32, 43, 54);
    /// Subtle borders and dividers (#293540).
    pub const BORDER_SUBTLE: Color32 = Color32::from_rgb(41, 53, 64);
    /// Slate structural bezel and card borders (#34434D).
    pub const BORDER_SLATE: Color32 = Color32::from_rgb(52, 67, 77);
    /// Warm ivory primary text (#F4F5F0).
    pub const PRIMARY_TEXT: Color32 = Color32::from_rgb(244, 245, 240);
    /// Secondary text and metadata (#A5AFB9).
    pub const SECONDARY_TEXT: Color32 = Color32::from_rgb(165, 175, 185);
    /// Electric Lime signature accent (#C5F36A).
    pub const ELECTRIC_LIME: Color32 = Color32::from_rgb(197, 243, 106);
    /// Electric lime hover/highlight state (#D7F58C).
    pub const ELECTRIC_LIME_HOVER: Color32 = Color32::from_rgb(215, 245, 140);

    // Compatibility aliases
    pub const ELEVATED_SURFACE: Color32 = Self::SLATE_SURFACE;
    pub const CARD_SURFACE: Color32 = Self::ELEVATED_CARD;
    pub const SLATE_BORDER: Color32 = Self::BORDER_SLATE;
    pub const SLATE_DIVIDER: Color32 = Self::BORDER_SUBTLE;
    pub const IVORY_TEXT: Color32 = Self::PRIMARY_TEXT;
    pub const MUTED_TEXT: Color32 = Self::SECONDARY_TEXT;
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
        // Obsidian background: #080D12 -> (8, 13, 18)
        assert_eq!(BrandPalette::OBSIDIAN.r(), 8);
        assert_eq!(BrandPalette::OBSIDIAN.g(), 13);
        assert_eq!(BrandPalette::OBSIDIAN.b(), 18);

        // Secondary background: #111A22 -> (17, 26, 34)
        assert_eq!(BrandPalette::SECONDARY_BG.r(), 17);
        assert_eq!(BrandPalette::SECONDARY_BG.g(), 26);
        assert_eq!(BrandPalette::SECONDARY_BG.b(), 34);

        // Slate surfaces: #18212B -> (24, 33, 43)
        assert_eq!(BrandPalette::SLATE_SURFACE.r(), 24);
        assert_eq!(BrandPalette::SLATE_SURFACE.g(), 33);
        assert_eq!(BrandPalette::SLATE_SURFACE.b(), 43);

        // Elevated cards: #202B36 -> (32, 43, 54)
        assert_eq!(BrandPalette::ELEVATED_CARD.r(), 32);
        assert_eq!(BrandPalette::ELEVATED_CARD.g(), 43);
        assert_eq!(BrandPalette::ELEVATED_CARD.b(), 54);

        // Subtle border: #293540 -> (41, 53, 64)
        assert_eq!(BrandPalette::BORDER_SUBTLE.r(), 41);
        assert_eq!(BrandPalette::BORDER_SUBTLE.g(), 53);
        assert_eq!(BrandPalette::BORDER_SUBTLE.b(), 64);

        // Slate border: #34434D -> (52, 67, 77)
        assert_eq!(BrandPalette::BORDER_SLATE.r(), 52);
        assert_eq!(BrandPalette::BORDER_SLATE.g(), 67);
        assert_eq!(BrandPalette::BORDER_SLATE.b(), 77);

        // Primary text: #F4F5F0 -> (244, 245, 240)
        assert_eq!(BrandPalette::PRIMARY_TEXT.r(), 244);
        assert_eq!(BrandPalette::PRIMARY_TEXT.g(), 245);
        assert_eq!(BrandPalette::PRIMARY_TEXT.b(), 240);

        // Secondary text: #A5AFB9 -> (165, 175, 185)
        assert_eq!(BrandPalette::SECONDARY_TEXT.r(), 165);
        assert_eq!(BrandPalette::SECONDARY_TEXT.g(), 175);
        assert_eq!(BrandPalette::SECONDARY_TEXT.b(), 185);

        // Electric Lime: #C5F36A -> (197, 243, 106)
        assert_eq!(BrandPalette::ELECTRIC_LIME.r(), 197);
        assert_eq!(BrandPalette::ELECTRIC_LIME.g(), 243);
        assert_eq!(BrandPalette::ELECTRIC_LIME.b(), 106);
    }
}
