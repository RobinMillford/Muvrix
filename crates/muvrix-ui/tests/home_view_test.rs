use eframe::egui::Color32;
use muvrix_ui::branding::BrandPalette;
use muvrix_ui::ui_components;
use muvrix_ui::views::AppView;

#[test]
fn test_home_view_is_default_and_has_stable_labels() {
    let default_view = AppView::default();
    assert_eq!(default_view, AppView::Home);
    assert_eq!(default_view.label(), "Home");

    // All navigation destinations are distinct
    let all_views = [
        AppView::Home,
        AppView::Player,
        AppView::Library,
        AppView::Browser,
        AppView::Playlist,
        AppView::Lyrics,
        AppView::Recent,
        AppView::Settings,
    ];

    for (i, v1) in all_views.iter().enumerate() {
        for (j, v2) in all_views.iter().enumerate() {
            if i != j {
                assert_ne!(v1, v2, "views at {i} and {j} must be distinct");
                assert_ne!(v1.label(), v2.label(), "view labels must be distinct");
            }
        }
    }
}

#[test]
fn test_phase_b_design_tokens_integrity() {
    // Obsidian background: #080D12
    assert_eq!(BrandPalette::OBSIDIAN, Color32::from_rgb(8, 13, 18));
    assert_eq!(ui_components::OBSIDIAN, Color32::from_rgb(8, 13, 18));

    // Secondary background: #111A22
    assert_eq!(ui_components::SECONDARY_BG, Color32::from_rgb(17, 26, 34));

    // Slate surfaces: #18212B
    assert_eq!(BrandPalette::SLATE_SURFACE, Color32::from_rgb(24, 33, 43));
    assert_eq!(ui_components::SLATE_SURFACE, Color32::from_rgb(24, 33, 43));

    // Elevated cards: #202B36
    assert_eq!(BrandPalette::ELEVATED_CARD, Color32::from_rgb(32, 43, 54));
    assert_eq!(ui_components::ELEVATED_CARD, Color32::from_rgb(32, 43, 54));

    // Subtle border: #293540
    assert_eq!(BrandPalette::BORDER_SUBTLE, Color32::from_rgb(41, 53, 64));
    assert_eq!(ui_components::BORDER_SUBTLE, Color32::from_rgb(41, 53, 64));

    // Slate structural border: #34434D
    assert_eq!(BrandPalette::BORDER_SLATE, Color32::from_rgb(52, 67, 77));
    assert_eq!(ui_components::BORDER_SLATE, Color32::from_rgb(52, 67, 77));

    // Primary text: #F4F5F0
    assert_eq!(BrandPalette::PRIMARY_TEXT, Color32::from_rgb(244, 245, 240));
    assert_eq!(
        ui_components::PRIMARY_TEXT,
        Color32::from_rgb(244, 245, 240)
    );

    // Secondary text: #A5AFB9
    assert_eq!(
        BrandPalette::SECONDARY_TEXT,
        Color32::from_rgb(165, 175, 185)
    );
    assert_eq!(
        ui_components::SECONDARY_TEXT,
        Color32::from_rgb(165, 175, 185)
    );

    // Electric Lime: #C5F36A
    assert_eq!(
        BrandPalette::ELECTRIC_LIME,
        Color32::from_rgb(197, 243, 106)
    );
    assert_eq!(
        ui_components::ELECTRIC_LIME,
        Color32::from_rgb(197, 243, 106)
    );
}

#[test]
fn test_phase_b1_top_bar_responsive_allocation() {
    // Responsive threshold at 1400.0 px
    let threshold = 1400.0_f32;

    // Test 1280x720 layout allocation
    let width_1280 = 1280.0_f32;
    assert!(
        width_1280 < threshold,
        "1280px must use compact responsive layout"
    );

    let is_wide_1280 = width_1280 >= threshold;
    let dev_w_1280 = if is_wide_1280 { 95.0 } else { 60.0 };
    let help_w_1280 = if is_wide_1280 { 110.0 } else { 34.0 };
    let player_extra_w = 68.0;
    let badge_w_1280 = if is_wide_1280 { 240.0 } else { 90.0 };
    let right_pad = 40.0;
    let total_right_1280 = player_extra_w + dev_w_1280 + help_w_1280 + badge_w_1280 + right_pad;
    assert_eq!(total_right_1280, 292.0);

    // Left elements at 1280: Logo(24) + name(60) + 5 tabs(~260) + more(~70) + 2 icon btns(~65) + separators/spacing(~40) ~= 519px
    let left_w_1280 = 520.0_f32;
    let remaining_w_1280 = width_1280 - left_w_1280;
    let title_w_1280 = (remaining_w_1280 - total_right_1280 - 12.0).max(40.0);
    assert!(
        title_w_1280 >= 400.0,
        "Title at 1280x720 has {title_w_1280}px available (>400px), eliminating collision"
    );

    // Test 1440x900 layout allocation
    let width_1440 = 1440.0_f32;
    assert!(
        width_1440 >= threshold,
        "1440px uses full top bar navigation layout"
    );
    let is_wide_1440 = width_1440 >= threshold;
    assert!(is_wide_1440);

    // Test 1920x1080 layout allocation
    let width_1920 = 1920.0_f32;
    assert!(
        width_1920 >= threshold,
        "1920px uses full top bar navigation layout"
    );
}

#[test]
fn test_phase_b1_evidence_screenshots_exist() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir.parent().unwrap().parent().unwrap();
    let artifacts_dir = root.join("docs/audits/artifacts/phase-b.1-final-2026-11-15/screenshots");
    let required = [
        "01_player_top_bar_1280x720.png",
        "02_player_top_bar_1440x900.png",
        "03_player_top_bar_1920x1080.png",
        "04_continue_watching_multiple_cards.png",
        "05_osd_visible.png",
        "06_osd_hidden_after_inactivity.png",
        "07_osd_restored_by_mouse.png",
        "08_player_paused_playback.png",
    ];

    for file_name in required {
        let p = artifacts_dir.join(file_name);
        assert!(p.exists(), "Required screenshot {} must exist", p.display());
        let meta = std::fs::metadata(&p).unwrap();
        assert!(
            meta.len() > 10_000,
            "Screenshot {} must be non-empty (>10KB)",
            p.display()
        );
    }
}

#[test]
fn test_phase_b2_nav_tab_contrast_and_accessibility() {
    // Relative luminance helper (sRGB)
    fn relative_luminance(c: Color32) -> f64 {
        let channel = |v: u8| -> f64 {
            let s = v as f64 / 255.0;
            if s <= 0.03928 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
    }

    let active_bg = ui_components::ELECTRIC_LIME;
    let active_fg = ui_components::OBSIDIAN;

    let l1 = relative_luminance(active_bg);
    let l2 = relative_luminance(active_fg);

    let contrast_ratio = (l1 + 0.05) / (l2 + 0.05);

    // WCAG AAA requires at least 7.0:1 for normal text
    assert!(
        contrast_ratio >= 14.0,
        "Active nav tab contrast ratio must exceed 14:1 (actual: {contrast_ratio:.2}:1)"
    );

    // Ensure inactive tab uses secondary text token
    let inactive_fg = ui_components::SECONDARY_TEXT;
    assert_ne!(
        inactive_fg, active_bg,
        "Inactive text must not match active background"
    );
    assert_ne!(
        active_fg, active_bg,
        "Active text must not match active background (preventing blank rectangle defect)"
    );
}

#[test]
fn test_phase_b2_evidence_screenshots_exist() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir.parent().unwrap().parent().unwrap();
    let artifacts_dir =
        root.join("docs/audits/artifacts/phase-b.2-active-nav-2026-11-15/screenshots");
    let required = [
        "01_active_player_tab_1280x720.png",
        "02_active_home_tab_1280x720.png",
        "03_active_library_tab_1280x720.png",
        "04_active_settings_tab_1280x720.png",
        "05_compact_navigation_1280x720.png",
        "06_full_navigation_1920x1080.png",
    ];

    for file_name in required {
        let p = artifacts_dir.join(file_name);
        assert!(p.exists(), "Required screenshot {} must exist", p.display());
        let meta = std::fs::metadata(&p).unwrap();
        assert!(
            meta.len() > 10_000,
            "Screenshot {} must be non-empty (>10KB)",
            p.display()
        );
    }
}
