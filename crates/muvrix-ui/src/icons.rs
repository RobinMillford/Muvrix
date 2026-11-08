//! Reusable vector icon rendering layer using `egui::Painter`.
//!
//! Provides pixel-perfect, platform-independent vector icons for transport controls,
//! audio, subtitles, window modes, and navigation without external font or asset dependencies.

use eframe::egui::{self, Align2, Color32, Pos2, Rect, Shape, Stroke, StrokeKind, TextStyle, Vec2};

/// Vector icon identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Play,
    Pause,
    Stop,
    SeekBackward,
    SeekForward,
    SkipPrevious,
    SkipNext,
    VolumeHigh,
    VolumeLow,
    VolumeMuted,
    Fullscreen,
    Windowed,
    FolderOpen,
    Folder,
    Subtitles,
    AudioTrack,
    Shortcuts,
    Developer,
    Diagnostics,
    Replay,
    Warning,
    Playlist,
    History,
    Settings,
    Search,
    ArrowBack,
    ArrowForward,
    ArrowUp,
    Close,
    Shuffle,
    Plus,
    Pip,
    Library,
    Sliders,
    Lyrics,
}

/// Paints a vector icon into the designated bounding rectangle.
pub fn paint_icon(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    if rect.width() <= 1.0 || rect.height() <= 1.0 {
        return;
    }

    let x0 = rect.min.x;
    let y0 = rect.min.y;
    let w = rect.width();
    let h = rect.height();

    // Map normalized [0.0, 1.0] coordinates into rect space
    let at = |u: f32, v: f32| -> Pos2 { Pos2::new(x0 + u * w, y0 + v * h) };

    match icon {
        Icon::Play => {
            let points = vec![at(0.25, 0.15), at(0.85, 0.50), at(0.25, 0.85)];
            painter.add(Shape::convex_polygon(points, color, Stroke::NONE));
        }

        Icon::Pause => {
            let bar_w = w * 0.22;
            let bar1 = Rect::from_min_max(
                at(0.20, 0.15),
                Pos2::new(x0 + 0.20 * w + bar_w, y0 + 0.85 * h),
            );
            let bar2 = Rect::from_min_max(
                Pos2::new(x0 + 0.80 * w - bar_w, y0 + 0.15 * h),
                at(0.80, 0.85),
            );
            painter.rect_filled(bar1, 1.5_f32, color);
            painter.rect_filled(bar2, 1.5_f32, color);
        }

        Icon::Stop => {
            let stop_rect = Rect::from_min_max(at(0.20, 0.20), at(0.80, 0.80));
            painter.rect_filled(stop_rect, 1.5_f32, color);
        }

        Icon::SeekBackward => {
            let tri1 = vec![at(0.48, 0.20), at(0.12, 0.50), at(0.48, 0.80)];
            let tri2 = vec![at(0.88, 0.20), at(0.52, 0.50), at(0.88, 0.80)];
            painter.add(Shape::convex_polygon(tri1, color, Stroke::NONE));
            painter.add(Shape::convex_polygon(tri2, color, Stroke::NONE));
        }

        Icon::SeekForward => {
            let tri1 = vec![at(0.12, 0.20), at(0.48, 0.50), at(0.12, 0.80)];
            let tri2 = vec![at(0.52, 0.20), at(0.88, 0.50), at(0.52, 0.80)];
            painter.add(Shape::convex_polygon(tri1, color, Stroke::NONE));
            painter.add(Shape::convex_polygon(tri2, color, Stroke::NONE));
        }

        Icon::SkipPrevious => {
            let bar = Rect::from_min_max(at(0.12, 0.20), at(0.26, 0.80));
            let tri = vec![at(0.86, 0.20), at(0.30, 0.50), at(0.86, 0.80)];
            painter.rect_filled(bar, 1.0_f32, color);
            painter.add(Shape::convex_polygon(tri, color, Stroke::NONE));
        }

        Icon::SkipNext => {
            let tri = vec![at(0.14, 0.20), at(0.70, 0.50), at(0.14, 0.80)];
            let bar = Rect::from_min_max(at(0.74, 0.20), at(0.88, 0.80));
            painter.add(Shape::convex_polygon(tri, color, Stroke::NONE));
            painter.rect_filled(bar, 1.0_f32, color);
        }

        Icon::VolumeHigh | Icon::VolumeLow | Icon::VolumeMuted => {
            // Speaker base rectangle
            let base = Rect::from_min_max(at(0.12, 0.35), at(0.30, 0.65));
            painter.rect_filled(base, 1.0_f32, color);

            // Flaring cone
            let cone = vec![
                at(0.30, 0.35),
                at(0.54, 0.18),
                at(0.54, 0.82),
                at(0.30, 0.65),
            ];
            painter.add(Shape::convex_polygon(cone, color, Stroke::NONE));

            match icon {
                Icon::VolumeHigh => {
                    let stroke = Stroke::new(1.6_f32, color);
                    // Inner sound wave arc
                    painter.line_segment([at(0.66, 0.36), at(0.72, 0.50)], stroke);
                    painter.line_segment([at(0.72, 0.50), at(0.66, 0.64)], stroke);
                    // Outer sound wave arc
                    painter.line_segment([at(0.78, 0.24), at(0.88, 0.50)], stroke);
                    painter.line_segment([at(0.88, 0.50), at(0.78, 0.76)], stroke);
                }
                Icon::VolumeLow => {
                    let stroke = Stroke::new(1.6_f32, color);
                    // Single sound wave arc
                    painter.line_segment([at(0.68, 0.36), at(0.75, 0.50)], stroke);
                    painter.line_segment([at(0.75, 0.50), at(0.68, 0.64)], stroke);
                }
                Icon::VolumeMuted => {
                    let red_stroke = Stroke::new(1.8_f32, Color32::from_rgb(240, 80, 80));
                    painter.line_segment([at(0.65, 0.32), at(0.88, 0.68)], red_stroke);
                    painter.line_segment([at(0.88, 0.32), at(0.65, 0.68)], red_stroke);
                }
                _ => {}
            }
        }

        Icon::Fullscreen => {
            let stroke = Stroke::new(1.6_f32, color);
            // Top-left
            painter.line_segment([at(0.14, 0.40), at(0.14, 0.16)], stroke);
            painter.line_segment([at(0.14, 0.16), at(0.38, 0.16)], stroke);
            // Top-right
            painter.line_segment([at(0.86, 0.40), at(0.86, 0.16)], stroke);
            painter.line_segment([at(0.86, 0.16), at(0.62, 0.16)], stroke);
            // Bottom-left
            painter.line_segment([at(0.14, 0.60), at(0.14, 0.84)], stroke);
            painter.line_segment([at(0.14, 0.84), at(0.38, 0.84)], stroke);
            // Bottom-right
            painter.line_segment([at(0.86, 0.60), at(0.86, 0.84)], stroke);
            painter.line_segment([at(0.86, 0.84), at(0.62, 0.84)], stroke);
        }

        Icon::Windowed => {
            let stroke = Stroke::new(1.5_f32, color);
            // Back window
            let back = Rect::from_min_max(at(0.32, 0.16), at(0.86, 0.70));
            painter.rect_stroke(back, 1.5_f32, stroke, StrokeKind::Inside);
            // Front window
            let front = Rect::from_min_max(at(0.14, 0.32), at(0.68, 0.86));
            painter.rect_filled(front, 1.5_f32, Color32::from_rgb(20, 22, 28));
            painter.rect_stroke(front, 1.5_f32, stroke, StrokeKind::Inside);
        }

        Icon::FolderOpen => {
            // Folder tab
            let tab = Rect::from_min_max(at(0.12, 0.22), at(0.48, 0.38));
            painter.rect_filled(tab, 1.5_f32, color);
            // Folder body
            let body = Rect::from_min_max(at(0.12, 0.34), at(0.88, 0.80));
            painter.rect_filled(body, 2.0_f32, color);
            // Inner flap contrast
            let flap_stroke = Stroke::new(1.4_f32, Color32::from_rgb(20, 22, 28));
            painter.line_segment([at(0.14, 0.54), at(0.86, 0.54)], flap_stroke);
        }

        Icon::Subtitles => {
            let stroke = Stroke::new(1.5_f32, color);
            let frame = Rect::from_min_max(at(0.12, 0.22), at(0.88, 0.78));
            painter.rect_stroke(frame, 2.0_f32, stroke, StrokeKind::Inside);

            let line_stroke = Stroke::new(1.4_f32, color);
            // Top subtitle line
            painter.line_segment([at(0.26, 0.44), at(0.74, 0.44)], line_stroke);
            // Bottom subtitle line
            painter.line_segment([at(0.34, 0.58), at(0.66, 0.58)], line_stroke);
        }

        Icon::AudioTrack => {
            // Waveform bars
            let bar_w = w * 0.10;
            let bars = [
                (0.16_f32, 0.40_f32, 0.70_f32),
                (0.32_f32, 0.24_f32, 0.84_f32),
                (0.48_f32, 0.14_f32, 0.90_f32),
                (0.64_f32, 0.28_f32, 0.80_f32),
                (0.80_f32, 0.44_f32, 0.66_f32),
            ];
            for (u, v1, v2) in bars {
                let rect = Rect::from_min_max(
                    Pos2::new(x0 + u * w - bar_w * 0.5, y0 + v1 * h),
                    Pos2::new(x0 + u * w + bar_w * 0.5, y0 + v2 * h),
                );
                painter.rect_filled(rect, 1.0_f32, color);
            }
        }

        Icon::Shortcuts => {
            let stroke = Stroke::new(1.5_f32, color);
            let frame = Rect::from_min_max(at(0.12, 0.24), at(0.88, 0.76));
            painter.rect_stroke(frame, 2.0_f32, stroke, StrokeKind::Inside);

            // Key dots
            let dots = [0.26_f32, 0.42_f32, 0.58_f32, 0.74_f32];
            for u in dots {
                painter.circle_filled(at(u, 0.42), 1.2_f32, color);
            }
            // Spacebar
            let sb_stroke = Stroke::new(1.3_f32, color);
            painter.line_segment([at(0.32, 0.60), at(0.68, 0.60)], sb_stroke);
        }

        Icon::Developer => {
            // Code brackets `< / >`
            let stroke = Stroke::new(1.5_f32, color);
            // Left bracket `<`
            painter.line_segment([at(0.32, 0.26), at(0.18, 0.50)], stroke);
            painter.line_segment([at(0.18, 0.50), at(0.32, 0.74)], stroke);
            // Slash `/`
            painter.line_segment([at(0.42, 0.76), at(0.58, 0.24)], stroke);
            // Right bracket `>`
            painter.line_segment([at(0.68, 0.26), at(0.82, 0.50)], stroke);
            painter.line_segment([at(0.82, 0.50), at(0.68, 0.74)], stroke);
        }

        Icon::Diagnostics => {
            // Activity ECG pulse line
            let stroke = Stroke::new(1.6_f32, color);
            painter.line_segment([at(0.10, 0.50), at(0.28, 0.50)], stroke);
            painter.line_segment([at(0.28, 0.50), at(0.40, 0.20)], stroke);
            painter.line_segment([at(0.40, 0.20), at(0.54, 0.80)], stroke);
            painter.line_segment([at(0.54, 0.80), at(0.66, 0.40)], stroke);
            painter.line_segment([at(0.66, 0.40), at(0.74, 0.50)], stroke);
            painter.line_segment([at(0.74, 0.50), at(0.90, 0.50)], stroke);
        }

        Icon::Replay => {
            let stroke = Stroke::new(1.6_f32, color);
            // 3/4 circle arc approximation
            painter.line_segment([at(0.75, 0.40), at(0.80, 0.58)], stroke);
            painter.line_segment([at(0.80, 0.58), at(0.68, 0.78)], stroke);
            painter.line_segment([at(0.68, 0.78), at(0.48, 0.82)], stroke);
            painter.line_segment([at(0.48, 0.82), at(0.28, 0.74)], stroke);
            painter.line_segment([at(0.28, 0.74), at(0.20, 0.54)], stroke);
            painter.line_segment([at(0.20, 0.54), at(0.28, 0.34)], stroke);
            painter.line_segment([at(0.28, 0.34), at(0.50, 0.24)], stroke);
            painter.line_segment([at(0.50, 0.24), at(0.72, 0.32)], stroke);

            // Arrow head
            let arrow = vec![at(0.72, 0.16), at(0.78, 0.38), at(0.56, 0.34)];
            painter.add(Shape::convex_polygon(arrow, color, Stroke::NONE));
        }

        Icon::Warning => {
            // Triangle with exclamation mark
            let tri = vec![at(0.50, 0.14), at(0.88, 0.84), at(0.12, 0.84)];
            painter.add(Shape::convex_polygon(tri, color, Stroke::NONE));

            let dark_stroke = Stroke::new(1.6_f32, Color32::from_rgb(20, 22, 28));
            painter.line_segment([at(0.50, 0.38), at(0.50, 0.62)], dark_stroke);
            painter.circle_filled(at(0.50, 0.72), 1.2_f32, Color32::from_rgb(20, 22, 28));
        }

        Icon::Folder => {
            let tab = Rect::from_min_max(at(0.12, 0.22), at(0.46, 0.36));
            painter.rect_filled(tab, 1.5_f32, color);
            let body = Rect::from_min_max(at(0.12, 0.34), at(0.88, 0.80));
            painter.rect_filled(body, 2.0_f32, color);
        }

        Icon::Playlist => {
            let stroke = Stroke::new(1.6_f32, color);
            painter.line_segment([at(0.30, 0.30), at(0.85, 0.30)], stroke);
            painter.line_segment([at(0.30, 0.50), at(0.85, 0.50)], stroke);
            painter.line_segment([at(0.30, 0.70), at(0.85, 0.70)], stroke);
            // Bullets
            painter.circle_filled(at(0.18, 0.30), 1.8_f32, color);
            painter.circle_filled(at(0.18, 0.50), 1.8_f32, color);
            painter.circle_filled(at(0.18, 0.70), 1.8_f32, color);
        }

        Icon::History => {
            let stroke = Stroke::new(1.5_f32, color);
            painter.circle_stroke(at(0.50, 0.50), w * 0.35, stroke);
            // Clock hands
            painter.line_segment([at(0.50, 0.50), at(0.50, 0.26)], stroke);
            painter.line_segment([at(0.50, 0.50), at(0.70, 0.50)], stroke);
        }

        Icon::Settings => {
            let stroke = Stroke::new(1.5_f32, color);
            // Sliders
            painter.line_segment([at(0.15, 0.28), at(0.85, 0.28)], stroke);
            painter.line_segment([at(0.15, 0.50), at(0.85, 0.50)], stroke);
            painter.line_segment([at(0.15, 0.72), at(0.85, 0.72)], stroke);
            // Knobs
            painter.circle_filled(at(0.38, 0.28), 2.2_f32, color);
            painter.circle_filled(at(0.64, 0.50), 2.2_f32, color);
            painter.circle_filled(at(0.30, 0.72), 2.2_f32, color);
        }

        Icon::Search => {
            let stroke = Stroke::new(1.5_f32, color);
            painter.circle_stroke(at(0.42, 0.42), w * 0.24, stroke);
            let handle_stroke = Stroke::new(1.8_f32, color);
            painter.line_segment([at(0.59, 0.59), at(0.84, 0.84)], handle_stroke);
        }

        Icon::ArrowBack => {
            let stroke = Stroke::new(1.6_f32, color);
            painter.line_segment([at(0.80, 0.50), at(0.22, 0.50)], stroke);
            painter.line_segment([at(0.44, 0.28), at(0.22, 0.50)], stroke);
            painter.line_segment([at(0.44, 0.72), at(0.22, 0.50)], stroke);
        }

        Icon::ArrowForward => {
            let stroke = Stroke::new(1.6_f32, color);
            painter.line_segment([at(0.20, 0.50), at(0.78, 0.50)], stroke);
            painter.line_segment([at(0.56, 0.28), at(0.78, 0.50)], stroke);
            painter.line_segment([at(0.56, 0.72), at(0.78, 0.50)], stroke);
        }

        Icon::ArrowUp => {
            let stroke = Stroke::new(1.6_f32, color);
            painter.line_segment([at(0.50, 0.80), at(0.50, 0.22)], stroke);
            painter.line_segment([at(0.28, 0.44), at(0.50, 0.22)], stroke);
            painter.line_segment([at(0.72, 0.44), at(0.50, 0.22)], stroke);
        }

        Icon::Close => {
            let stroke = Stroke::new(1.6_f32, color);
            painter.line_segment([at(0.25, 0.25), at(0.75, 0.75)], stroke);
            painter.line_segment([at(0.75, 0.25), at(0.25, 0.75)], stroke);
        }

        Icon::Shuffle => {
            let stroke = Stroke::new(1.5_f32, color);
            painter.line_segment([at(0.18, 0.32), at(0.42, 0.32)], stroke);
            painter.line_segment([at(0.42, 0.32), at(0.68, 0.68)], stroke);
            painter.line_segment([at(0.68, 0.68), at(0.82, 0.68)], stroke);

            painter.line_segment([at(0.18, 0.68), at(0.42, 0.68)], stroke);
            painter.line_segment([at(0.42, 0.68), at(0.68, 0.32)], stroke);
            painter.line_segment([at(0.68, 0.32), at(0.82, 0.32)], stroke);
            // Arrows
            painter.line_segment([at(0.72, 0.24), at(0.82, 0.32)], stroke);
            painter.line_segment([at(0.72, 0.76), at(0.82, 0.68)], stroke);
        }

        Icon::Plus => {
            let stroke = Stroke::new(1.6_f32, color);
            painter.line_segment([at(0.50, 0.22), at(0.50, 0.78)], stroke);
            painter.line_segment([at(0.22, 0.50), at(0.78, 0.50)], stroke);
        }

        Icon::Pip => {
            let stroke = Stroke::new(1.4_f32, color);
            let outer = Rect::from_min_max(at(0.12, 0.18), at(0.88, 0.82));
            painter.rect_stroke(outer, 1.5_f32, stroke, StrokeKind::Inside);
            let pip_rect = Rect::from_min_max(at(0.50, 0.48), at(0.82, 0.76));
            painter.rect_filled(pip_rect, 1.0_f32, color);
        }

        Icon::Library => {
            let stroke = Stroke::new(1.3_f32, color);
            let b1 = Rect::from_min_max(at(0.15, 0.20), at(0.32, 0.85));
            let b2 = Rect::from_min_max(at(0.38, 0.20), at(0.55, 0.85));
            let b3 = Rect::from_min_max(at(0.61, 0.20), at(0.78, 0.85));
            painter.rect_stroke(b1, 1.0_f32, stroke, StrokeKind::Inside);
            painter.rect_stroke(b2, 1.0_f32, stroke, StrokeKind::Inside);
            painter.rect_stroke(b3, 1.0_f32, stroke, StrokeKind::Inside);
            painter.line_segment([at(0.12, 0.88), at(0.88, 0.88)], stroke);
        }

        Icon::Sliders => {
            let stroke = Stroke::new(1.3_f32, color);
            painter.line_segment([at(0.25, 0.15), at(0.25, 0.85)], stroke);
            painter.circle_filled(at(0.25, 0.40), 2.5_f32, color);

            painter.line_segment([at(0.50, 0.15), at(0.50, 0.85)], stroke);
            painter.circle_filled(at(0.50, 0.65), 2.5_f32, color);

            painter.line_segment([at(0.75, 0.15), at(0.75, 0.85)], stroke);
            painter.circle_filled(at(0.75, 0.30), 2.5_f32, color);
        }

        Icon::Lyrics => {
            let stroke = Stroke::new(1.3_f32, color);
            painter.circle_filled(at(0.35, 0.70), 2.8_f32, color);
            painter.line_segment([at(0.42, 0.70), at(0.42, 0.25)], stroke);
            painter.line_segment([at(0.42, 0.25), at(0.75, 0.20)], stroke);
            painter.line_segment([at(0.75, 0.20), at(0.75, 0.60)], stroke);
            painter.circle_filled(at(0.68, 0.60), 2.8_f32, color);
            painter.line_segment(
                [at(0.20, 0.88), at(0.80, 0.88)],
                Stroke::new(1.0_f32, color),
            );
        }
    }
}

/// Paints a standalone icon inside a Ui layout.
pub fn icon_label(ui: &mut egui::Ui, icon: Icon, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        paint_icon(ui.painter(), rect, icon, color);
    }
}

/// A compact icon-only button with standard interaction styles.
pub fn icon_button(ui: &mut egui::Ui, icon: Icon, tooltip: &str) -> egui::Response {
    icon_button_sized(ui, icon, Vec2::new(26.0, 24.0), tooltip)
}

/// An icon button with custom dimensions.
pub fn icon_button_sized(
    ui: &mut egui::Ui,
    icon: Icon,
    size: Vec2,
    tooltip: &str,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&response);
        ui.painter().rect(
            rect,
            visuals.corner_radius,
            visuals.bg_fill,
            visuals.bg_stroke,
            StrokeKind::Inside,
        );

        let icon_dim = (size.x.min(size.y) * 0.60).clamp(12.0, 20.0);
        let icon_rect = Rect::from_center_size(rect.center(), Vec2::splat(icon_dim));
        paint_icon(ui.painter(), icon_rect, icon, visuals.fg_stroke.color);
    }

    if !tooltip.is_empty() {
        response.on_hover_text(tooltip)
    } else {
        response
    }
}

/// A prominent primary icon button with subtle accent styling (e.g. for Play/Pause).
pub fn primary_icon_button(
    ui: &mut egui::Ui,
    icon: Icon,
    size: Vec2,
    tooltip: &str,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let (bg_fill, stroke, icon_color) = if response.is_pointer_button_down_on() {
            (
                Color32::from_rgb(24, 75, 145),
                Stroke::new(1.0_f32, Color32::from_rgb(60, 130, 220)),
                Color32::WHITE,
            )
        } else if response.hovered() {
            (
                Color32::from_rgb(32, 90, 170),
                Stroke::new(1.0_f32, Color32::from_rgb(90, 160, 255)),
                Color32::WHITE,
            )
        } else {
            (
                Color32::from_rgb(25, 65, 125),
                Stroke::new(1.0_f32, Color32::from_rgb(50, 110, 195)),
                Color32::from_gray(240),
            )
        };

        ui.painter()
            .rect(rect, 4.0_f32, bg_fill, stroke, StrokeKind::Inside);

        let icon_dim = (size.x.min(size.y) * 0.58).clamp(14.0, 22.0);
        let icon_rect = Rect::from_center_size(rect.center(), Vec2::splat(icon_dim));
        paint_icon(ui.painter(), icon_rect, icon, icon_color);
    }

    if !tooltip.is_empty() {
        response.on_hover_text(tooltip)
    } else {
        response
    }
}

/// A button containing a vector icon followed by text.
pub fn icon_text_button(
    ui: &mut egui::Ui,
    icon: Icon,
    text: &str,
    tooltip: &str,
) -> egui::Response {
    let font_id = TextStyle::Button.resolve(ui.style());
    let galley = ui.fonts(|f| f.layout_no_wrap(text.to_string(), font_id, Color32::PLACEHOLDER));
    let icon_dim = 14.0_f32;
    let spacing = 6.0_f32;
    let padding = ui.spacing().button_padding;
    let total_width = icon_dim + spacing + galley.size().x + padding.x * 2.0;
    let total_height = icon_dim.max(galley.size().y) + padding.y * 2.0;

    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(total_width, total_height), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&response);
        ui.painter().rect(
            rect,
            visuals.corner_radius,
            visuals.bg_fill,
            visuals.bg_stroke,
            StrokeKind::Inside,
        );

        let icon_rect = Rect::from_min_size(
            Pos2::new(rect.min.x + padding.x, rect.center().y - icon_dim * 0.5),
            Vec2::splat(icon_dim),
        );
        paint_icon(ui.painter(), icon_rect, icon, visuals.fg_stroke.color);

        let text_pos = Pos2::new(
            rect.min.x + padding.x + icon_dim + spacing,
            rect.center().y - galley.size().y * 0.5,
        );
        ui.painter().text(
            text_pos,
            Align2::LEFT_TOP,
            text,
            TextStyle::Button.resolve(ui.style()),
            visuals.fg_stroke.color,
        );
    }

    if !tooltip.is_empty() {
        response.on_hover_text(tooltip)
    } else {
        response
    }
}

/// A prominent primary button containing a vector icon followed by text (e.g. for Welcome Card Open File).
pub fn primary_icon_button_text(
    ui: &mut egui::Ui,
    icon: Icon,
    text: &str,
    tooltip: &str,
) -> egui::Response {
    let font_id = egui::FontId::proportional(14.0);
    let galley =
        ui.fonts(|f| f.layout_no_wrap(text.to_string(), font_id.clone(), Color32::PLACEHOLDER));
    let icon_dim = 16.0_f32;
    let spacing = 8.0_f32;
    let padding = Vec2::new(16.0, 8.0);
    let total_width = icon_dim + spacing + galley.size().x + padding.x * 2.0;
    let total_height = icon_dim.max(galley.size().y) + padding.y * 2.0;

    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(total_width, total_height), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let (bg_fill, stroke, fg_color) = if response.is_pointer_button_down_on() {
            (
                Color32::from_rgb(20, 68, 135),
                Stroke::new(1.0_f32, Color32::from_rgb(60, 130, 225)),
                Color32::WHITE,
            )
        } else if response.hovered() {
            (
                Color32::from_rgb(32, 92, 175),
                Stroke::new(1.0_f32, Color32::from_rgb(95, 165, 255)),
                Color32::WHITE,
            )
        } else {
            (
                Color32::from_rgb(26, 75, 148),
                Stroke::new(1.0_f32, Color32::from_rgb(55, 120, 210)),
                Color32::from_gray(245),
            )
        };

        ui.painter()
            .rect(rect, 5.0_f32, bg_fill, stroke, StrokeKind::Inside);

        let icon_rect = Rect::from_min_size(
            Pos2::new(rect.min.x + padding.x, rect.center().y - icon_dim * 0.5),
            Vec2::splat(icon_dim),
        );
        paint_icon(ui.painter(), icon_rect, icon, fg_color);

        let text_pos = Pos2::new(
            rect.min.x + padding.x + icon_dim + spacing,
            rect.center().y - galley.size().y * 0.5,
        );
        ui.painter()
            .text(text_pos, Align2::LEFT_TOP, text, font_id, fg_color);
    }

    if !tooltip.is_empty() {
        response.on_hover_text(tooltip)
    } else {
        response
    }
}
